use alloc::{boxed::Box, vec, vec::Vec};
use core::ops::{Deref, DerefMut};

use bytemuck::pod_collect_to_vec;

use wipi_types::wipic::{WIPICFramebuffer, WIPICIndirectPtr, WIPICWord};

use wie_backend::canvas::{ArgbPixel, Canvas, Color, Image, ImageBufferCanvas, PixelType, Rgb8Pixel, Rgb565Pixel, VecImageBuffer};
use wie_util::{Result, WieError};

use crate::context::WIPICContext;

// same 256MB as wie_core_arm's HEAP_SIZE; not referenced directly to avoid the dependency
const MAX_FRAMEBUFFER_BYTES: u32 = 0x1000_0000;

fn buffer_size(width: u32, height: u32, bytes_per_pixel: u32) -> Result<(u32, u32)> {
    let bpl = width.checked_mul(bytes_per_pixel).ok_or(WieError::AllocationFailure)?;
    let size = bpl.checked_mul(height).ok_or(WieError::AllocationFailure)?;
    if size > MAX_FRAMEBUFFER_BYTES {
        return Err(WieError::AllocationFailure);
    }

    Ok((size, bpl))
}

pub struct FrameBuffer(pub WIPICFramebuffer);

impl FrameBuffer {
    pub fn empty() -> Self {
        Self(WIPICFramebuffer {
            width: 0,
            height: 0,
            bpl: 0,
            bpp: 0,
            buf: WIPICIndirectPtr(0),
        })
    }

    pub fn new(context: &mut dyn WIPICContext, width: WIPICWord, height: WIPICWord, bpp: WIPICWord) -> Result<Self> {
        let bytes_per_pixel = bpp / 8;

        let (size, bpl) = buffer_size(width, height, bytes_per_pixel)?;
        let buf = context.alloc(size)?;

        Ok(Self(WIPICFramebuffer {
            width,
            height,
            bpl,
            bpp: bytes_per_pixel * 8,
            buf,
        }))
    }

    pub fn from_image(context: &mut dyn WIPICContext, image: &dyn Image) -> Result<Self> {
        let (size, bpl) = buffer_size(image.width(), image.height(), image.bytes_per_pixel())?;
        let buf = context.alloc(size)?;

        context.write_bytes(context.data_ptr(buf)?, &image.raw())?;

        Ok(Self(WIPICFramebuffer {
            width: image.width(),
            height: image.height(),
            bpl,
            bpp: image.bytes_per_pixel() * 8,
            buf,
        }))
    }

    pub fn data(&self, context: &dyn WIPICContext) -> Result<Vec<u8>> {
        let (size, _) = buffer_size(self.0.width, self.0.height, self.0.bpp / 8)?;
        let mut buf = vec![0; size as _];
        context.read_bytes(context.data_ptr(self.0.buf)?, &mut buf)?;

        Ok(buf)
    }

    pub fn image(&self, context: &mut dyn WIPICContext) -> Result<Box<dyn Image>> {
        let data = self.data(context)?;

        Ok(match self.0.bpp {
            16 => Box::new(VecImageBuffer::<Rgb565Pixel>::from_raw(
                self.0.width as _,
                self.0.height as _,
                pod_collect_to_vec(&data),
            )),
            32 => Box::new(VecImageBuffer::<ArgbPixel>::from_raw(
                self.0.width as _,
                self.0.height as _,
                pod_collect_to_vec(&data),
            )),
            _ => unimplemented!("Unsupported pixel format: {}", self.0.bpp),
        })
    }

    pub fn canvas<'a>(&'a self, context: &'a mut dyn WIPICContext) -> Result<FramebufferCanvas<'a>> {
        let data = self.data(context)?;

        let canvas: Box<dyn Canvas> = match self.0.bpp {
            16 => Box::new(ImageBufferCanvas::new(VecImageBuffer::<Rgb565Pixel>::from_raw(
                self.0.width as _,
                self.0.height as _,
                pod_collect_to_vec(&data),
            ))),
            32 => Box::new(ImageBufferCanvas::new(VecImageBuffer::<ArgbPixel>::from_raw(
                self.0.width as _,
                self.0.height as _,
                pod_collect_to_vec(&data),
            ))),
            _ => unimplemented!("Unsupported pixel format: {}", self.0.bpp),
        };

        Ok(FramebufferCanvas {
            framebuffer: self,
            context,
            canvas,
            flushed: false,
            snapshot: data,
        })
    }

    pub fn write(&self, context: &mut dyn WIPICContext, data: &[u8]) -> Result<()> {
        context.write_bytes(context.data_ptr(self.0.buf)?, data)
    }

    /// Writes back only the bytes that differ between the snapshot the canvas
    /// started from and what it drew, leaving every other pixel exactly as guest
    /// memory holds it now.
    ///
    /// A whole-buffer write of the drawn image would re-stamp the snapshot over
    /// pixels the title wrote straight into the same buffer - its own decoded
    /// artwork, or a blit another thread made after we took the snapshot - which
    /// is why backgrounds came out partly black behind our text and shapes.
    /// Restaging only the pixels a primitive actually changed keeps those
    /// direct writes intact, and matches how the reference draws each primitive
    /// straight into the framebuffer rather than through a full-frame copy.
    /// Fills a solid rectangle by writing only the rows it covers.
    ///
    /// The canvas path a primitive normally takes stages the whole surface:
    /// it reads every byte of it out of guest memory, collects that into a
    /// pixel buffer, draws, and diffs the result back row by row. That is the
    /// right shape for a primitive whose coverage is hard to predict ahead of
    /// the draw, and much the wrong one for a solid rectangle - 엑시온2 draws
    /// its scene, and its minimap, as tens of thousands of 2x2 fills a frame,
    /// and each one was paying two full 240x320 copies out and a whole-surface
    /// comparison back for four pixels of work.
    ///
    /// The rectangle is clamped exactly the way the canvas clamps it, to the
    /// reported width and height, and `MC_grpFillRect` passes the rectangle
    /// itself as the clip so nothing else narrows it. The colour is stored
    /// rather than composed, which is what the canvas does for a fully opaque
    /// one - the caller checks that before coming here.
    ///
    /// `false` when the surface's depth is not one a colour can be packed for,
    /// or its geometry does not fit the addressing; the caller should take the
    /// canvas path instead. Nothing has been written when it returns `false`.
    pub fn fill_rect_direct(&self, context: &mut dyn WIPICContext, x: i32, y: i32, w: u32, h: u32, color: Color) -> Result<bool> {
        let pixel: Vec<u8> = match self.0.bpp {
            16 => Rgb565Pixel::from_color(color).to_le_bytes().to_vec(),
            32 => ArgbPixel::from_color(color).to_le_bytes().to_vec(),
            _ => return Ok(false),
        };

        let bpp = (self.0.bpp / 8).max(1) as i64;
        let bpl = self.0.bpl as i64;
        if bpl <= 0 || pixel.len() as i64 != bpp {
            return Ok(false);
        }

        let left = (x as i64).max(0);
        let right = (x as i64 + w as i64).min(self.0.width as i64);
        let top = (y as i64).max(0);
        let bottom = (y as i64 + h as i64).min(self.0.height as i64);
        if left >= right || top >= bottom {
            return Ok(true);
        }

        // The furthest byte the loop would touch, checked before any of it is
        // written so a surface this cannot address is refused whole rather
        // than half filled.
        let row_bytes = (right - left) * bpp;
        let last = (bottom - 1) * bpl + left * bpp + row_bytes;
        if u32::try_from(last).is_err() {
            return Ok(false);
        }

        let row = pixel.repeat((right - left) as usize);
        let base = context.data_ptr(self.0.buf)?;

        for py in top..bottom {
            let offset = (py * bpl + left * bpp) as u32;
            context.write_bytes(base + offset, &row)?;
        }

        Ok(true)
    }

    pub fn write_diff(&self, context: &mut dyn WIPICContext, snapshot: &[u8], drawn: &[u8]) -> Result<()> {
        let bpl = self.0.bpl as usize;
        let bpp = (self.0.bpp / 8).max(1) as usize;
        if bpl == 0 || snapshot.len() != drawn.len() {
            // Layout we cannot reason about row-wise; fall back to a full write.
            return self.write(context, drawn);
        }

        let base = context.data_ptr(self.0.buf)?;
        for (row, (snap_row, drawn_row)) in snapshot.chunks_exact(bpl).zip(drawn.chunks_exact(bpl)).enumerate() {
            // The changed span within the row - nothing outside it is touched, so
            // a direct write elsewhere in the row survives.
            let Some(first) = (0..bpl).find(|&i| snap_row[i] != drawn_row[i]) else {
                continue;
            };
            let last = (first..bpl).rev().find(|&i| snap_row[i] != drawn_row[i]).unwrap();
            // Snap the span out to whole-pixel boundaries. A 16bpp pixel splits
            // green across its two bytes, so writing a half pixel (when only one
            // of the two bytes changed) would corrupt the colour - the green
            // fringing along drawn edges. Rounding down to the pixel start and up
            // past the pixel end always writes complete pixels.
            let start = first - (first % bpp);
            let end = (bpl).min(last + bpp - (last % bpp));
            let byte_off = row * bpl + start;
            if let Ok(dst) = u32::try_from(byte_off) {
                context.write_bytes(base + dst, &drawn_row[start..end])?;
            }
        }

        Ok(())
    }

    pub fn pixel_to_color(&self, pixel: WIPICWord) -> Color {
        match self.0.bpp {
            16 => Rgb565Pixel::to_color(pixel as u16),
            _ => Rgb8Pixel::to_color(pixel),
        }
    }
}

pub struct FramebufferCanvas<'a> {
    framebuffer: &'a FrameBuffer,
    context: &'a mut dyn WIPICContext,
    canvas: Box<dyn Canvas>,
    flushed: bool,
    /// The framebuffer bytes as they were when this canvas was taken, so
    /// `flush` can write back only what the primitive actually changed.
    snapshot: Vec<u8>,
}

impl FramebufferCanvas<'_> {
    pub fn flush(mut self) -> Result<()> {
        self.flushed = true;

        let drawn = self.canvas.image().raw();
        self.framebuffer.write_diff(self.context, &self.snapshot, &drawn)
    }
}

// best-effort fallback for canvases dropped without an explicit flush
impl Drop for FramebufferCanvas<'_> {
    fn drop(&mut self) {
        if self.flushed {
            return;
        }

        tracing::warn!("framebuffer canvas dropped without explicit flush; write-back errors will be lost");

        let drawn = self.canvas.image().raw();
        if let Err(err) = self.framebuffer.write_diff(self.context, &self.snapshot, &drawn) {
            tracing::error!("Failed to flush framebuffer canvas: {err}");
        }
    }
}

impl Deref for FramebufferCanvas<'_> {
    type Target = Box<dyn Canvas>;

    fn deref(&self) -> &Self::Target {
        &self.canvas
    }
}

impl DerefMut for FramebufferCanvas<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.canvas
    }
}

#[cfg(test)]
mod test {
    use wie_util::{ByteRead, ByteWrite, WieError};

    use wie_backend::canvas::{Clip, Color};

    use crate::WIPICContext;
    use crate::context::test::TestContext;

    use super::FrameBuffer;

    /// write_diff restages only the pixels a primitive changed, so a byte the
    /// title wrote straight into the framebuffer after the canvas snapshot (its
    /// own decoded artwork) survives our write-back instead of being re-stamped
    /// with the stale snapshot.
    #[test]
    fn write_diff_preserves_pixels_the_primitive_did_not_touch() {
        let mut context = TestContext::new();
        // 4x2 @ 16bpp -> bpl 8, 16 bytes.
        let fb = FrameBuffer::new(&mut context, 4, 2, 16).unwrap();
        let base = context.data_ptr(fb.0.buf).unwrap();

        // The snapshot the canvas started from.
        let snapshot = [0x11u8; 16];
        context.write_bytes(base, &snapshot).unwrap();

        // Our primitive changed exactly one pixel (bytes 4..6 of row 0).
        let mut drawn = snapshot;
        drawn[4] = 0xAA;
        drawn[5] = 0xBB;

        // Meanwhile the title blitted its own pixel straight into row 1,
        // *after* the snapshot was taken.
        context.write_bytes(base + 12, &[0xCC, 0xDD]).unwrap();

        fb.write_diff(&mut context, &snapshot, &drawn).unwrap();

        let mut out = [0u8; 16];
        context.read_bytes(base, &mut out).unwrap();
        // Our drawn pixel landed.
        assert_eq!(&out[4..6], &[0xAA, 0xBB]);
        // The title's direct write survived (not clobbered by the snapshot).
        assert_eq!(&out[12..14], &[0xCC, 0xDD]);
        // Everything else is still the snapshot.
        assert_eq!(out[0], 0x11);
        assert_eq!(out[6], 0x11);
        assert_eq!(out[14], 0x11);
    }

    /// When only one byte of a 16bpp pixel changes, write_diff still restages the
    /// whole pixel (both bytes), so green - which straddles the two bytes - is
    /// never left half-written.
    #[test]
    fn fill_rect_direct_matches_the_canvas_it_replaces() {
        // Every rectangle worth disagreeing about: inside, hanging off each
        // edge, straddling a corner, and entirely outside.
        for (x, y, w, h) in [
            (1i32, 1i32, 2u32, 2u32),
            (-2, 1, 4, 2),
            (6, 0, 4, 3),
            (1, -3, 2, 5),
            (-9, -9, 3, 3),
            (9, 9, 2, 2),
        ] {
            let mut direct = TestContext::new();
            let fb = FrameBuffer::new(&mut direct, 8, 6, 16).unwrap();
            let base = direct.data_ptr(fb.0.buf).unwrap();
            direct.write_bytes(base, &[0x5au8; 8 * 6 * 2]).unwrap();

            let color = Color {
                a: 0xff,
                r: 0x12,
                g: 0x34,
                b: 0x56,
            };
            assert!(fb.fill_rect_direct(&mut direct, x, y, w, h, color).unwrap());

            // The same fill through the canvas, which is what it stands in for.
            let mut staged = TestContext::new();
            let other = FrameBuffer::new(&mut staged, 8, 6, 16).unwrap();
            staged.write_bytes(base, &[0x5au8; 8 * 6 * 2]).unwrap();
            let mut canvas = other.canvas(&mut staged).unwrap();
            let clip = Clip { x, y, width: w, height: h };
            canvas.fill_rect(x, y, w, h, color, clip);
            canvas.flush().unwrap();

            let mut got = [0u8; 8 * 6 * 2];
            let mut want = [0u8; 8 * 6 * 2];
            direct.read_bytes(base, &mut got).unwrap();
            staged.read_bytes(base, &mut want).unwrap();
            assert_eq!(got, want, "fill ({x}, {y}, {w}, {h})");
        }
    }

    #[test]
    fn fill_rect_direct_refuses_a_depth_it_cannot_pack() {
        let mut context = TestContext::new();
        let fb = FrameBuffer::new(&mut context, 4, 2, 8).unwrap();

        let color = Color { a: 0xff, r: 0, g: 0, b: 0 };
        assert!(!fb.fill_rect_direct(&mut context, 0, 0, 4, 2, color).unwrap());
    }

    #[test]
    fn write_diff_restages_whole_pixels() {
        let mut context = TestContext::new();
        let fb = FrameBuffer::new(&mut context, 4, 1, 16).unwrap();
        let base = context.data_ptr(fb.0.buf).unwrap();

        let snapshot = [0x11u8; 8];
        context.write_bytes(base, &snapshot).unwrap();

        // Our primitive changed only the low byte of pixel 1 (bytes 2..4).
        let mut drawn = snapshot;
        drawn[2] = 0x77;

        fb.write_diff(&mut context, &snapshot, &drawn).unwrap();

        let mut out = [0u8; 8];
        context.read_bytes(base, &mut out).unwrap();
        // Both bytes of pixel 1 were written (the high byte re-stamped from what
        // we drew), so the pixel is a complete, uncorrupted value.
        assert_eq!(&out[2..4], &[0x77, 0x11]);
        // Neighbouring pixels untouched.
        assert_eq!(&out[0..2], &[0x11, 0x11]);
        assert_eq!(&out[4..6], &[0x11, 0x11]);
    }

    #[test]
    fn test_new_overflow_returns_error() {
        let mut context = TestContext::new();

        assert!(matches!(
            FrameBuffer::new(&mut context, 0x10000, 0x10000, 32),
            Err(WieError::AllocationFailure)
        ));
    }

    #[test]
    fn test_new_over_heap_limit_returns_error() {
        let mut context = TestContext::new();

        assert!(matches!(
            FrameBuffer::new(&mut context, 0x4000, 0x4000, 32),
            Err(WieError::AllocationFailure)
        ));
    }

    #[test]
    fn test_new_zero_height_bpl_overflow_returns_error() {
        let mut context = TestContext::new();

        assert!(matches!(
            FrameBuffer::new(&mut context, 0xffff_ffff, 0, 32),
            Err(WieError::AllocationFailure)
        ));
    }

    #[test]
    fn test_new_normal_size_ok() {
        let mut context = TestContext::new();

        let framebuffer = FrameBuffer::new(&mut context, 100, 100, 16).unwrap();
        assert_eq!(framebuffer.0.width, 100);
        assert_eq!(framebuffer.0.height, 100);
        assert_eq!(framebuffer.0.bpl, 200);
        assert_eq!(framebuffer.0.bpp, 16);
        assert_eq!(framebuffer.data(&context).unwrap().len(), 20000);
    }
}
