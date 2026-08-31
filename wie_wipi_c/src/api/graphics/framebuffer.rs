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
        })
    }

    pub fn write(&self, context: &mut dyn WIPICContext, data: &[u8]) -> Result<()> {
        context.write_bytes(context.data_ptr(self.0.buf)?, data)
    }

    /// Writes back only the `[x, x+w) x [y, y+h)` rectangle of `data` (a full
    /// framebuffer image, row-major at this buffer's stride), clamped to bounds.
    ///
    /// Text and other small primitives use this instead of writing the whole
    /// buffer: a title may have another thread (its firmware) blitting an image
    /// straight into the same buffer, and a full write-back of a snapshot taken
    /// before that blit would erase it. Touching only the drawn rectangle leaves
    /// those pixels intact.
    pub fn write_rect(&self, context: &mut dyn WIPICContext, data: &[u8], x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let bytes_per_pixel = (self.0.bpp / 8) as usize;
        let bpl = self.0.bpl as usize;
        if bytes_per_pixel == 0 || bpl == 0 {
            return Ok(());
        }

        let fb_w = self.0.width as i32;
        let fb_h = self.0.height as i32;
        let x0 = x.clamp(0, fb_w) as usize;
        let y0 = y.clamp(0, fb_h) as usize;
        let x1 = x.saturating_add(w).clamp(0, fb_w) as usize;
        let y1 = y.saturating_add(h).clamp(0, fb_h) as usize;
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }

        let base = context.data_ptr(self.0.buf)?;
        let row_start = x0 * bytes_per_pixel;
        let row_end = x1 * bytes_per_pixel;
        for row in y0..y1 {
            let off = row * bpl;
            if let Some(src) = data.get(off + row_start..off + row_end)
                && let Ok(dst) = u32::try_from(off + row_start)
            {
                context.write_bytes(base + dst, src)?;
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
}

impl FramebufferCanvas<'_> {
    pub fn flush(mut self) -> Result<()> {
        self.flushed = true;

        self.framebuffer.write(self.context, &self.canvas.image().raw())
    }

    /// Writes back only the given rectangle, leaving the rest of the buffer as
    /// the guest last wrote it. Use for primitives that touch a bounded region
    /// (text, a filled rect) so a concurrent image blit into the same buffer is
    /// not clobbered by a whole-buffer write of a stale snapshot.
    pub fn flush_rect(mut self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        self.flushed = true;

        let raw = self.canvas.image().raw();
        self.framebuffer.write_rect(self.context, &raw, x, y, w, h)
    }
}

// best-effort fallback for canvases dropped without an explicit flush
impl Drop for FramebufferCanvas<'_> {
    fn drop(&mut self) {
        if self.flushed {
            return;
        }

        tracing::warn!("framebuffer canvas dropped without explicit flush; write-back errors will be lost");

        if let Err(err) = self.framebuffer.write(self.context, &self.canvas.image().raw()) {
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
    use wie_util::WieError;

    use crate::context::test::TestContext;

    use super::FrameBuffer;

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
