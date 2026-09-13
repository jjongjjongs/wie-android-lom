mod bitmap_font;
mod framebuffer;
mod grp_context;
mod image;

use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

use alloc::{string::String, vec, vec::Vec};

use wie_backend::{
    Event,
    canvas::{Canvas, Clip, Color, Image, PixelType, Rgb8Pixel, Rgb565Pixel, TextAlignment, string_width_px},
};
use wie_util::{Result, WieError, read_generic, read_null_terminated_string_bytes, write_generic};

use wipi_types::wipic::{WIPICDisplayInfo, WIPICFramebuffer, WIPICImage, WIPICIndirectPtr, WIPICWord};

use crate::context::WIPICContext;

use self::{
    bitmap_font::BitmapFace,
    framebuffer::FrameBuffer,
    grp_context::{WIPICGraphicsContext, WIPICGraphicsContextIdx},
    image::create_wipi_image,
};

pub use self::bitmap_font::{clear as clear_bios_font, install_from_bios as install_bios_font};

pub const FRAMEBUFFER_DEPTH: u32 = 16; // XXX hardcode to 16bpp as some game requires 16bpp framebuffer
const SCREEN_FRAMEBUFFER_PTR: u32 = 0x7fff1000;
/// Guest word holding the height of the handset's status strip (the WIPI
/// "annunciator"), which sits above the drawing area a title is given. Zero
/// unless the platform stores one, and nothing below changes while it is zero.
pub const ANNUNCIATOR_ROWS_PTR: u32 = 0x7fff2000;

/// Read a WIPI-C string. `length == -1` means NUL-terminated; `length > 0`
/// reads exactly that many bytes; `length == 0` and other negatives yield
/// an empty string.
///
/// The bytes are EUC-KR, which is what a Korean handset's toolchain put in the
/// binary. Reading them as UTF-8 turns every Hangul syllable into U+FFFD, and
/// the font has no glyph for that, so a title's text silently drew nothing at
/// all - which is what dialogue boxes with no words in them were.
fn read_wipi_string(context: &mut dyn WIPICContext, ptr: WIPICWord, length: i32) -> Result<String> {
    let bytes = if length > 0 {
        let mut buf = vec![0u8; length as usize];
        context.read_bytes(ptr, &mut buf)?;
        buf
    } else if length == -1 {
        read_null_terminated_string_bytes(context, ptr)?
    } else {
        Vec::new()
    };

    Ok(encoding_rs::EUC_KR.decode(&bytes).0.into_owned())
}

pub async fn get_screen_framebuffer(context: &mut dyn WIPICContext, a0: WIPICWord) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_grpGetScreenFrameBuffer({a0:#x})");

    let framebuffer_ptr: u32 = read_generic(context, SCREEN_FRAMEBUFFER_PTR)?;
    if framebuffer_ptr != 0 {
        return Ok(WIPICIndirectPtr(framebuffer_ptr));
    }

    // A title asking for the screen it has not been given yet is a title
    // starting, and the surfaces [`trace_offscreen_surfaces`] knows about
    // belong to the one before it. They live in a static, so nothing else
    // forgets them; left behind, they name allocations this title now owns for
    // something else.
    OFFSCREEN_SURFACES.lock().clear();

    let (width, height) = {
        let platform = context.system().platform();
        let screen = platform.screen();
        (screen.width(), screen.height())
    };

    // Guard the screen surface too: a title blits its scene straight into it
    // with the default (unclamped) clip and overruns the bottom edge, and the
    // list-allocator header of the very next block sits 4 bytes past this
    // buffer's end - so an unpadded screen buffer lets the overdraw corrupt the
    // heap. `new_screen_surface` pads it the same way `new_guarded_surface`
    // does, and also splits the panel from the drawing area.
    let framebuffer = new_screen_surface(context, width, height)?;

    let memory = context.alloc(size_of::<WIPICFramebuffer>() as WIPICWord)?;
    write_generic(context, context.data_ptr(memory)?, framebuffer.0)?;
    write_generic(context, SCREEN_FRAMEBUFFER_PTR, memory.0)?;

    Ok(memory)
}

pub async fn init_context(context: &mut dyn WIPICContext, p_grp_ctx: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpInitContext({p_grp_ctx:#x})");

    // Reference MC_grpInitContext (@0x1abc0c) does not zero the whole context;
    // it plants non-zero defaults that drawing then relies on when the game
    // never calls SetContext for a given field. Porting them keeps our output
    // consistent with the firmware:
    //   clip   = whole plane (0,0)-(0x7fff,0x7fff)
    //   bgpxl  = 0x00ffffff (opaque white)
    //   alpha  = 0xff       (fully opaque)
    //   param1 = 0xff
    //   font   = MC_grpGetFont(0,0,0)  (the 12px default face)
    // Everything else (fg, transparent, pixelop, style, offset) stays zero.
    let grp_ctx = WIPICGraphicsContext {
        clip: [0, 0, 0x7fff, 0x7fff],
        bgpxl: 0x00ff_ffff,
        alpha: 0xff,
        param1: 0xff,
        font: font_size_px(0) as WIPICWord,
        ..Default::default()
    };
    write_generic(context, p_grp_ctx, grp_ctx)?;
    Ok(())
}

pub async fn set_context(context: &mut dyn WIPICContext, p_grp_ctx: WIPICWord, op: WIPICGraphicsContextIdx, pv: WIPICWord) -> Result<()> {
    tracing::trace!("MC_grpSetContext({p_grp_ctx:#x}, {op:?}, {pv:#x})");

    let mut grp_ctx: WIPICGraphicsContext = read_generic(context, p_grp_ctx)?;
    match op {
        WIPICGraphicsContextIdx::ClipIdx => {
            // The clip rectangle is passed as four 32-bit words (x1, y1, x2, y2),
            // not four 16-bit ones - reading it as `[u16; 4]` took only the low
            // halves of x1/y1 as the whole rect and dropped x2/y2 entirely, so a
            // title that saved the clip with GetContext and restored it here got a
            // degenerate rectangle back and every later blit clipped to nothing
            // (MapleStory 도적편's sprites vanished). The reference stores the
            // bottom-right corner decremented; GetContext re-adds the 1.
            let x1: u32 = read_generic(context, pv)?;
            let y1: u32 = read_generic(context, pv + 4)?;
            let x2: u32 = read_generic(context, pv + 8)?;
            let y2: u32 = read_generic(context, pv + 12)?;
            grp_ctx.clip = [x1, y1, x2.wrapping_sub(1), y2.wrapping_sub(1)];
        }
        WIPICGraphicsContextIdx::FgPixelIdx => {
            grp_ctx.fgpxl = pv as _;
        }
        WIPICGraphicsContextIdx::BgPixelIdx => {
            grp_ctx.bgpxl = pv as _;
        }
        // The reference stores nothing for op 3 and reports nothing back.
        WIPICGraphicsContextIdx::TransPixelIdx => {}
        // The reference ignores an alpha outside 0..=0xff rather than storing it.
        WIPICGraphicsContextIdx::AlphaIdx => {
            if pv <= 0xff {
                grp_ctx.alpha = pv;
            }
        }
        WIPICGraphicsContextIdx::PixelopIdx => {
            grp_ctx.pixel_op_func_ptr = pv;
        }
        WIPICGraphicsContextIdx::PixelParam1Idx => {
            grp_ctx.param1 = pv;
        }
        WIPICGraphicsContextIdx::FontIdx => {
            grp_ctx.font = pv;
        }
        WIPICGraphicsContextIdx::StyleIdx => {
            grp_ctx.style = pv;
        }
        // XOR mode is a flag of its own, and the reference drives the pixel-op
        // slot from it: turning it on zeroes the alpha and installs its built-in
        // XOR operation, turning it off clears both. We have no pixel-op path to
        // install, so the slot is cleared either way - a title reading it back
        // sees no operation rather than a pointer it could not call here.
        WIPICGraphicsContextIdx::XorModeIdx => {
            grp_ctx.pixel_op_func_ptr = 0;
            if pv == 0 {
                grp_ctx.xor_mode = 0;
            } else {
                grp_ctx.alpha = 0;
                grp_ctx.xor_mode = 1;
            }
        }
        WIPICGraphicsContextIdx::OffsetIdx => {
            // Same 32-bit-word pair as the clip corners, and the counterpart to
            // GetContext's `OffsetIdx`, which writes two 32-bit words back.
            let x: u32 = read_generic(context, pv)?;
            let y: u32 = read_generic(context, pv + 4)?;
            grp_ctx.offset = [x, y];
        }
        _ => {
            tracing::warn!("MC_grpSetContext({p_grp_ctx:#x}, {op:?}, {pv:#x}): ignoring invalid op");
        }
    }
    write_generic(context, p_grp_ctx, grp_ctx)?;

    Ok(())
}

/// `MC_grpGetContext(p_grp_ctx, op, out_ptr)` - the read counterpart to
/// `MC_grpSetContext`. A clet's blitter reads the live drawing state back
/// (foreground colour, alpha, font, clip, offset...) so it can save and restore
/// it around each primitive. Left a stub returning nothing, it zeroed the game's
/// saved context, and the restore then corrupted every following draw - Demon
/// Hunter smeared each screen over the last one and spun redrawing.
///
/// Behaviour mirrors `liblgt_system.so`'s `MC_grpGetContext`: the value is
/// written *through* `out_ptr` (unlike `SetContext`, which passes scalars by
/// value), the call is a no-op when either pointer is null, `TransPixelIdx`
/// reads nothing back, and the clip is reported as `(x1, y1, x2 + 1, y2 + 1)`
/// (the reference stores the bottom-right corner decremented and re-adds it).
pub async fn get_context(context: &mut dyn WIPICContext, p_grp_ctx: WIPICWord, op: WIPICGraphicsContextIdx, out_ptr: WIPICWord) -> Result<()> {
    tracing::trace!("MC_grpGetContext({p_grp_ctx:#x}, {op:?}, {out_ptr:#x})");

    if p_grp_ctx == 0 || out_ptr == 0 {
        return Ok(());
    }

    let grp_ctx: WIPICGraphicsContext = read_generic(context, p_grp_ctx)?;
    match op {
        WIPICGraphicsContextIdx::ClipIdx => {
            let clip = grp_ctx.clip;
            write_generic(context, out_ptr, clip[0])?;
            write_generic(context, out_ptr + 4, clip[1])?;
            write_generic(context, out_ptr + 8, clip[2].wrapping_add(1))?;
            write_generic(context, out_ptr + 12, clip[3].wrapping_add(1))?;
        }
        WIPICGraphicsContextIdx::FgPixelIdx => write_generic(context, out_ptr, grp_ctx.fgpxl)?,
        WIPICGraphicsContextIdx::BgPixelIdx => write_generic(context, out_ptr, grp_ctx.bgpxl)?,
        // The reference reads nothing back for op 3.
        WIPICGraphicsContextIdx::TransPixelIdx => {}
        WIPICGraphicsContextIdx::AlphaIdx => write_generic(context, out_ptr, grp_ctx.alpha)?,
        WIPICGraphicsContextIdx::PixelopIdx => write_generic(context, out_ptr, grp_ctx.pixel_op_func_ptr)?,
        WIPICGraphicsContextIdx::PixelParam1Idx => write_generic(context, out_ptr, grp_ctx.param1)?,
        WIPICGraphicsContextIdx::FontIdx => write_generic(context, out_ptr, grp_ctx.font)?,
        WIPICGraphicsContextIdx::StyleIdx => write_generic(context, out_ptr, grp_ctx.style)?,
        WIPICGraphicsContextIdx::XorModeIdx => write_generic(context, out_ptr, grp_ctx.xor_mode)?,
        WIPICGraphicsContextIdx::OffsetIdx => {
            let offset = grp_ctx.offset;
            write_generic(context, out_ptr, offset[0])?;
            write_generic(context, out_ptr + 4, offset[1])?;
        }
        _ => {
            tracing::warn!("MC_grpGetContext({p_grp_ctx:#x}, {op:?}, {out_ptr:#x}): unsupported op");
        }
    }

    Ok(())
}

/// The colour a primitive paints with: the context's foreground pixel, carrying
/// the context's alpha so a title that asked for a translucent shape gets one.
///
/// 테일즈위버 이스핀편 (`00026308`) draws every one of its panels this way. A
/// two-call helper of its own at `0x160c` sets the foreground pixel and an alpha
/// together - `MC_grpSetContext(gc, 1, pixel)` then `MC_grpSetContext(gc, 4,
/// alpha)`, with alphas from `0x50` to `0xc8` - and fills a rectangle with it;
/// thirty of its thirty-four calls to that helper are followed by
/// `MC_grpFillRect`. Painted opaque, its menu panels came out solid white and
/// its in-game panels solid black, and the white text it then drew on them
/// disappeared into the fill.
///
/// XOR mode is the exception: the reference zeroes the alpha when it turns XOR
/// on, so reading it there would paint nothing at all.
fn context_color(framebuffer: &FrameBuffer, gctx: &WIPICGraphicsContext) -> Color {
    let mut color = framebuffer.pixel_to_color(gctx.fgpxl);

    if gctx.xor_mode == 0 && gctx.alpha <= 0xff {
        color.a = gctx.alpha as u8;
    }

    color
}

pub async fn put_pixel(context: &mut dyn WIPICContext, dst_fb: WIPICIndirectPtr, x: i32, y: i32, p_gctx: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpPutPixel({:#x}, {x}, {y}, {p_gctx:?})", dst_fb.0);

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst_fb)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;

    let mut canvas = framebuffer.canvas(context)?;
    let color = context_color(&framebuffer, &gctx);
    canvas.put_pixel(x as _, y as _, color);
    canvas.flush()?;

    Ok(())
}

pub async fn fill_rect(context: &mut dyn WIPICContext, dst_fb: WIPICIndirectPtr, x: i32, y: i32, w: i32, h: i32, p_gctx: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpFillRect({:#x}, {x}, {y}, {w}, {h}, {p_gctx:#x})", dst_fb.0);

    if w <= 0 || h <= 0 {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst_fb)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;
    let color = context_color(&framebuffer, &gctx);

    // A solid, fully opaque rectangle is just bytes, and the clip this passes
    // is the rectangle itself, so nothing about the result needs the surface
    // staged: write the rows it covers straight into the framebuffer. A title
    // that draws by the pixel - 엑시온2 fills its scene and its minimap 2x2 at
    // a time, tens of thousands of times a frame - was paying two full-surface
    // copies out and a whole-surface diff back for each of them. A colour that
    // is not opaque is composed with what is under it, which is the canvas
    // path's business.
    if color.a == 0xff && framebuffer.fill_rect_direct(context, x, y, w as _, h as _, color)? {
        return Ok(());
    }

    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: x as _,
        y: y as _,
        width: w as _,
        height: h as _,
    };

    canvas.fill_rect(x as _, y as _, w as _, h as _, color, clip);
    canvas.flush()?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn draw_arc(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    start_angle: i32,
    end_angle: i32,
    p_gctx: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpDrawArc({:#x}, {x}, {y}, {w}, {h}, {start_angle}, {end_angle}, {p_gctx:#x})", dst.0);

    if dst.0 == 0 || p_gctx == 0 || w <= 0 || h <= 0 {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: x as _,
        y: y as _,
        width: w as _,
        height: h as _,
    };

    let color = context_color(&framebuffer, &gctx);
    canvas.draw_arc(
        x as _,
        y as _,
        w as _,
        h as _,
        start_angle,
        end_angle.wrapping_sub(start_angle),
        color,
        clip,
    );
    canvas.flush()?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn fill_arc(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    start_angle: i32,
    end_angle: i32,
    p_gctx: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpFillArc({:#x}, {x}, {y}, {w}, {h}, {start_angle}, {end_angle}, {p_gctx:#x})", dst.0);

    if dst.0 == 0 || p_gctx == 0 || w <= 0 || h <= 0 {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: x as _,
        y: y as _,
        width: w as _,
        height: h as _,
    };

    let color = context_color(&framebuffer, &gctx);
    canvas.fill_arc(
        x as _,
        y as _,
        w as _,
        h as _,
        start_angle,
        end_angle.wrapping_sub(start_angle),
        color,
        clip,
    );
    canvas.flush()?;

    Ok(())
}

/// Reads `n` (x, y) vertices from two parallel `M_Int32` arrays, the way the
/// WIPI polygon calls pass them.
fn read_polygon_points(context: &mut dyn WIPICContext, x_points: WIPICWord, y_points: WIPICWord, n: usize) -> Result<Vec<(i32, i32)>> {
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        let offset = (i * size_of::<i32>()) as WIPICWord;
        let x: i32 = read_generic(context, x_points + offset)?;
        let y: i32 = read_generic(context, y_points + offset)?;
        points.push((x, y));
    }
    Ok(points)
}

/// The bounding box of a set of points as `(min_x, min_y, max_x, max_y)`. Used
/// as the draw clip so a stray vertex cannot paint outside the shape's extent.
/// `Clip` is neither `Copy` nor `Clone`, so callers rebuild one per draw from
/// these bounds.
fn polygon_bounds(points: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    let min_x = points.iter().map(|p| p.0).min().unwrap_or(0);
    let min_y = points.iter().map(|p| p.1).min().unwrap_or(0);
    let max_x = points.iter().map(|p| p.0).max().unwrap_or(0);
    let max_y = points.iter().map(|p| p.1).max().unwrap_or(0);
    (min_x, min_y, max_x, max_y)
}

fn bounds_clip(bounds: (i32, i32, i32, i32)) -> Clip {
    let (min_x, min_y, max_x, max_y) = bounds;
    Clip {
        x: min_x,
        y: min_y,
        width: (max_x - min_x + 1).max(0) as u32,
        height: (max_y - min_y + 1).max(0) as u32,
    }
}

pub async fn draw_polygon(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x_points: WIPICWord,
    y_points: WIPICWord,
    n_points: i32,
    p_gctx: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpDrawPolygon({:#x}, {x_points:#x}, {y_points:#x}, {n_points}, {p_gctx:#x})", dst.0);

    if n_points < 2 || x_points == 0 || y_points == 0 {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;
    let points = read_polygon_points(context, x_points, y_points, n_points as usize)?;

    let bounds = polygon_bounds(&points);
    let color = context_color(&framebuffer, &gctx);
    let mut canvas = framebuffer.canvas(context)?;

    // Close the outline back to the first vertex, which is what a polygon is.
    for i in 0..points.len() {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % points.len()];
        canvas.draw_line(x1, y1, x2, y2, color, bounds_clip(bounds));
    }
    canvas.flush()?;

    Ok(())
}

pub async fn fill_polygon(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x_points: WIPICWord,
    y_points: WIPICWord,
    n_points: i32,
    p_gctx: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpFillPolygon({:#x}, {x_points:#x}, {y_points:#x}, {n_points}, {p_gctx:#x})", dst.0);

    if n_points < 3 || x_points == 0 || y_points == 0 {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;
    let points = read_polygon_points(context, x_points, y_points, n_points as usize)?;

    let bounds = polygon_bounds(&points);
    let color = context_color(&framebuffer, &gctx);
    let (min_y, max_y) = (bounds.1, bounds.3);
    let mut canvas = framebuffer.canvas(context)?;

    // Even-odd scanline fill: for each row, gather where the edges cross it,
    // sort, and paint the interior between successive crossing pairs.
    let mut crossings: Vec<i32> = Vec::with_capacity(points.len());
    for y in min_y..=max_y {
        crossings.clear();
        for i in 0..points.len() {
            let (x1, y1) = points[i];
            let (x2, y2) = points[(i + 1) % points.len()];
            // A half-open edge test counts each vertex once, so a scanline
            // passing exactly through a vertex is not filled twice.
            let (lo, hi, xa, xb) = if y1 <= y2 { (y1, y2, x1, x2) } else { (y2, y1, x2, x1) };
            if y >= lo && y < hi {
                let x = xa + (xb - xa) * (y - lo) / (hi - lo);
                crossings.push(x);
            }
        }
        crossings.sort_unstable();
        for pair in crossings.chunks_exact(2) {
            canvas.draw_line(pair[0], y, pair[1], y, color, bounds_clip(bounds));
        }
    }
    canvas.flush()?;

    Ok(())
}

pub async fn create_image(
    context: &mut dyn WIPICContext,
    ptr_image: WIPICWord,
    image_data: WIPICIndirectPtr,
    offset: u32,
    len: u32,
) -> Result<WIPICWord> {
    tracing::debug!("MC_grpCreateImage({ptr_image:#x}, {:#x}, {offset}, {len})", image_data.0);

    // The source pointer can be one the title read out of never-initialised
    // memory - a NULL/valid handle on the reference's zeroed heap, a stray
    // address here - so a read straight from it faults the whole VM instead of
    // failing the one call. The reference returns "not done" for a source it
    // cannot read; do the same and let the title carry on rather than die.
    let image = match create_wipi_image(context, image_data, offset, len) {
        Ok(image) => image,
        Err(WieError::InvalidMemoryAccess(address)) => {
            tracing::warn!(
                "MC_grpCreateImage: unreadable source {:#x} (+{offset}, faulted at {address:#x}); reporting not-done",
                image_data.0
            );
            return Ok(0); // not MC_GRP_IMAGE_DONE
        }
        Err(other) => return Err(other),
    };

    let memory = context.alloc(size_of::<WIPICImage>() as WIPICWord)?;
    write_generic(context, ptr_image, memory)?;
    write_generic(context, context.data_ptr(memory)?, image)?;

    Ok(1) // MC_GRP_IMAGE_DONE
}

pub async fn destroy_image(context: &mut dyn WIPICContext, image: WIPICIndirectPtr) -> Result<()> {
    tracing::debug!("MC_grpDestroyImage({:#x})", image.0);

    if image.0 == 0 {
        return Ok(());
    }

    // Free the pixel planes `create_wipi_image` allocated: the colour plane
    // (`img.buf`) always, and the mask plane (`mask.buf`) only when the source
    // carried alpha. Freeing just the WIPICImage struct - as this did before -
    // leaks both planes, and a title that creates and destroys a scratch image
    // every frame (MapleStory 도적편 does this ~100x/frame) then exhausts the
    // heap. The `buf` field is the caller's own encoded bytes and is not ours.
    let wipi_image: WIPICImage = read_generic(context, context.data_ptr(image)?)?;
    if wipi_image.img.buf.0 != 0 {
        context.free(wipi_image.img.buf)?;
    }
    if wipi_image.mask.buf.0 != 0 {
        context.free(wipi_image.mask.buf)?;
    }

    context.free(image)?;

    Ok(())
}

/// Copy the `(x, y, w, h)` region of `fb` into a freshly allocated framebuffer
/// of the same depth, clamping reads to the source bounds. Used to materialise a
/// sub-image as an independent pixel plane.
fn crop_framebuffer(context: &mut dyn WIPICContext, fb: &FrameBuffer, x: i32, y: i32, w: i32, h: i32) -> Result<FrameBuffer> {
    let bpp = (fb.0.bpp / 8).max(1) as i64;
    let src = fb.data(context)?;
    let src_bpl = fb.0.bpl as i64;
    let (src_w, src_h) = (fb.0.width as i64, fb.0.height as i64);
    let new_bpl = w as i64 * bpp;
    let mut out = vec![0u8; (new_bpl * h as i64) as usize];

    for row in 0..h as i64 {
        let sy = y as i64 + row;
        if sy < 0 || sy >= src_h {
            continue;
        }
        for col in 0..w as i64 {
            let sx = x as i64 + col;
            if sx < 0 || sx >= src_w {
                continue;
            }
            let src_off = (sy * src_bpl + sx * bpp) as usize;
            let dst_off = (row * new_bpl + col * bpp) as usize;
            out[dst_off..dst_off + bpp as usize].copy_from_slice(&src[src_off..src_off + bpp as usize]);
        }
    }

    let dst = FrameBuffer::new(context, w as u32, h as u32, fb.0.bpp)?;
    context.write_bytes(context.data_ptr(dst.0.buf)?, &out)?;
    Ok(dst)
}

/// `MC_grpCreateSubImage(parent, x, y, w, h)` - a view of a rectangular region
/// of an existing image, as its own image handle. The reference copies the
/// region out into an independent plane, which is what a title relies on when it
/// slices a sprite sheet into individual frames; left unmapped it returned the
/// diagnostic stub 0, so every sub-image was null and its draws were skipped.
pub async fn create_sub_image(context: &mut dyn WIPICContext, parent: WIPICIndirectPtr, x: i32, y: i32, w: i32, h: i32) -> Result<WIPICWord> {
    tracing::debug!("MC_grpCreateSubImage({:#x}, {x}, {y}, {w}, {h})", parent.0);

    if parent.0 == 0 || w <= 0 || h <= 0 {
        return Ok(0);
    }

    let parent_image: WIPICImage = read_generic(context, context.data_ptr(parent)?)?;

    let sub_img = crop_framebuffer(context, &FrameBuffer(parent_image.img), x, y, w, h)?;
    let sub_mask = if parent_image.mask.buf.0 != 0 {
        crop_framebuffer(context, &FrameBuffer(parent_image.mask), x, y, w, h)?
    } else {
        FrameBuffer::empty()
    };

    let image = WIPICImage {
        img: sub_img.0,
        mask: sub_mask.0,
        loop_count: 0,
        delay: 0,
        animated: 0,
        buf: WIPICIndirectPtr(0),
        offset: 0,
        current: 0,
        len: 0,
    };

    let memory = context.alloc(size_of::<WIPICImage>() as WIPICWord)?;
    write_generic(context, context.data_ptr(memory)?, image)?;

    Ok(memory.0)
}

/// `MC_grpDecodeNextImage(image)` - advance an animated image to its next frame.
/// `create_wipi_image` already decodes the first (and, for the still images WIE
/// currently produces, only) frame, so the image is ready to draw; report frame
/// 0 as available rather than the diagnostic stub's 0-that-meant-nothing. Real
/// multi-frame stepping is not modelled yet.
pub async fn decode_next_image(context: &mut dyn WIPICContext, image: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_grpDecodeNextImage({:#x})", image.0);

    if image.0 == 0 {
        return Ok(-1);
    }

    let _wipi_image: WIPICImage = read_generic(context, context.data_ptr(image)?)?;
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
pub async fn draw_image(
    context: &mut dyn WIPICContext,
    framebuffer: WIPICIndirectPtr,
    dx: i32,
    dy: i32,
    w: i32,
    h: i32,
    image: WIPICIndirectPtr,
    sx: i32,
    sy: i32,
    graphics_context: WIPICWord,
) -> Result<()> {
    tracing::debug!(
        "MC_grpDrawImage({:#x}, {dx}, {dy}, {w}, {h}, {:#x}, {sx}, {sy}, {graphics_context:#x})",
        framebuffer.0,
        image.0
    );

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(framebuffer)?)?);
    let image: WIPICImage = read_generic(context, context.data_ptr(image)?)?;

    // An image that carries alpha keeps the full colour in the mask plane, and
    // its per-pixel alpha composites straight. One without a mask is a 16bpp
    // colour plane whose transparency is the magenta key instead, so it is
    // keyed rather than blended.
    let keyed = image.mask.buf.0 == 0;
    let source = if keyed { image.img } else { image.mask };
    let src_image = FrameBuffer(source).image(context)?;
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: dx as _,
        y: dy as _,
        width: w as _,
        height: h as _,
    };

    if keyed {
        blit_magenta_keyed(&mut **canvas, dx, dy, w, h, &*src_image, sx, sy);
    } else {
        canvas.draw(dx as _, dy as _, w as _, h as _, &*src_image, sx as _, sy as _, clip);
    }
    canvas.flush()?;

    Ok(())
}

pub async fn flush_lcd(
    context: &mut dyn WIPICContext,
    i: WIPICWord,
    framebuffer: WIPICIndirectPtr,
    x: WIPICWord,
    y: WIPICWord,
    w: WIPICWord,
    h: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpFlushLcd({i:#x}, {:#x}, {x:#x}, {y:#x}, {w:#x}, {h:#x})", framebuffer.0);

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(framebuffer)?)?);

    let src_canvas = framebuffer.image(context)?;

    // DIAGNOSTIC: summarise the frame we are about to present so a device log
    // shows whether an image actually reached the draw buffer. A text-only
    // screen carries a handful of colours; a decoded image carries dozens or
    // hundreds. Distinguishes "the image never got drawn" from "it was drawn
    // but is not reaching the display". Logged at info so it survives a normal
    // capture without turning on the per-primitive flood.
    {
        let (colours, non_black) = surface_content(&*src_canvas);

        tracing::info!(
            "FRAME flush fb={:#x} {}x{} region=({x},{y},{w},{h}) colours={colours} non_black={non_black}",
            framebuffer.0.buf.0,
            src_canvas.width(),
            src_canvas.height(),
        );

        // And the frame itself, on the rounds the off-screen surfaces are drawn
        // on. This is the one picture a reader can hold a screenshot against,
        // which is what says whether a surface reached the screen.
        if FLUSHES.load(Ordering::Relaxed) % OFFSCREEN_TRACE_EVERY == 0 {
            for line in surface_thumbnail(&*src_canvas) {
                tracing::info!("FRAME |{line}|");
            }
        }
    }

    let platform = context.system().platform();
    let screen = platform.screen();

    screen.paint(&*src_canvas);

    // What is on the surfaces the title drew into but never handed back. After
    // the paint, so the frame is on its way before this reads anything, and the
    // canvas it borrowed is done with.
    drop(src_canvas);
    trace_offscreen_surfaces(context);

    Ok(())
}

pub async fn get_pixel_from_rgb(_context: &mut dyn WIPICContext, r: i32, g: i32, b: i32) -> Result<WIPICWord> {
    tracing::debug!("MC_grpGetPixelFromRGB({r:#x}, {g:#x}, {b:#x})");
    if (r > 0xff) || (g > 0xff) | (b > 0xff) {
        tracing::debug!("MC_grpGetPixelFromRGB({r:#x}, {g:#x}, {b:#x}): value clipped to 8 bits");
    }

    let color = Rgb565Pixel::from_color(Color {
        a: 0xff,
        r: r as u8,
        g: g as u8,
        b: b as u8,
    });

    Ok(color as WIPICWord)
}

pub async fn get_rgb_from_pixel(context: &mut dyn WIPICContext, pixel: i32, r: WIPICWord, g: WIPICWord, b: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_grpGetRGBFromPixel({pixel}, {r:#x}, {g:#x}, {b:#x})");

    let color = Rgb565Pixel::to_color(pixel as u16);

    write_generic(context, r, color.r as i32)?;
    write_generic(context, g, color.g as i32)?;
    write_generic(context, b, color.b as i32)?;

    Ok(pixel)
}

pub async fn get_display_info(context: &mut dyn WIPICContext, reserved: WIPICWord, out_ptr: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_grpGetDisplayInfo({reserved:#x}, {out_ptr:#x})");

    assert_eq!(reserved, 0);

    let (width, height) = {
        let platform = context.system().platform();
        let screen = platform.screen();
        (screen.width(), screen.height())
    };

    // The reference's MC_grpGetDisplayInfo (@0x1abcf8) reports width@+8 and
    // height@+0xc, and for a 240/320/400-wide panel subtracts the status strip
    // from the height when the annunciator properties are set - so what a title
    // reads here is the drawing area, not the panel. `annunciator_rows` is that
    // strip, and zero unless the platform reserves one, which leaves the full
    // height reported as before. The colour fields are the driver's direct
    // RGB565 format (matched by Rgb565Pixel), reported here as fixed masks.
    let strip = annunciator_rows(context, width).min(height);
    let info = WIPICDisplayInfo {
        bpp: FRAMEBUFFER_DEPTH,
        depth: 16,
        width,
        height: height - strip,
        bpl: 2 * width,
        color_type: 1, // 1==MC_GRP_DIRECT_COLOR_TYPE
        red_mask: 0xf800,
        green_mask: 0x7e0,
        blue_mask: 0x1f,
    };

    write_generic(context, out_ptr, info)?;
    Ok(1)
}

#[allow(clippy::too_many_arguments)]
pub async fn copy_area(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    dx: i32,
    dy: i32,
    w: i32,
    h: i32,
    x: i32,
    y: i32,
    pgc: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpCopyArea({:#x}, {dx}, {dy}, {w}, {h}, {x}, {y}, {pgc:#x})", dst.0);

    if w < 0 || h < 0 {
        tracing::warn!("Skipping negative dimension");

        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);

    let image = framebuffer.image(context)?;
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: dx as _,
        y: dy as _,
        width: w as _,
        height: h as _,
    };

    canvas.draw(dx as _, dy as _, w as _, h as _, &*image, x as _, y as _, clip);
    canvas.flush()?;

    Ok(())
}

/// Extra rows allocated beneath a draw surface, never reported in its
/// dimensions. A title draws into a surface (the screen framebuffer or an
/// off-screen buffer) with the clip context's default `0x7fff` bound - i.e.
/// effectively unclipped - and its own software blitter does not clamp to the
/// surface height, so a sprite placed near the bottom (MapleStory 도적편
/// rotates its title character down to y≈300 in a 320-row buffer) writes tens
/// of rows past the end. On the reference that overdraw lands in slack the
/// memory map leaves after the surface; here the next allocation sits there, so
/// the overdraw smashes it - and 4 bytes past the screen buffer is the *list
/// allocator header* of the title's work arena, so the stray pixels clear its
/// in-use bit, the heap then re-hands that live arena out, and the resource
/// load into the re-issued block wipes the menu the title just built there.
/// Padding the surface with owned rows keeps that overdraw benign, as it is on
/// the device. Measured worst case for 도적편 is ~24 rows; 256 is generous
/// headroom and still trivial against the heap.
const SURFACE_GUARD_ROWS: u32 = 256;

/// Build a draw surface with `SURFACE_GUARD_ROWS` of owned slack under its
/// reported height. The width/height/stride the surface reports are the real
/// ones, so a title's addressing is identical; the extra rows only exist to
/// absorb its unclamped overdraw instead of the next allocation.
fn new_guarded_surface(context: &mut dyn WIPICContext, width: u32, height: u32) -> Result<FrameBuffer> {
    let mut framebuffer = FrameBuffer::new(context, width, height.saturating_add(SURFACE_GUARD_ROWS), FRAMEBUFFER_DEPTH)?;
    framebuffer.0.height = height as _;
    Ok(framebuffer)
}

/// Height of the status strip above the drawing area, as the platform stored it
/// in `ANNUNCIATOR_ROWS_PTR`, and zero when it stored nothing. The reference
/// only reserves the strip on the panel widths its own table lists, so a title
/// on any other panel sees the whole display exactly as it does today.
fn annunciator_rows(context: &dyn WIPICContext, width: u32) -> u32 {
    if !matches!(width, 240 | 320 | 400) {
        return 0;
    }

    read_generic(context, ANNUNCIATOR_ROWS_PTR).unwrap_or(0)
}

/// Build the screen surface with the status strip above the drawing area.
///
/// The strip is part of the panel, not of the title's drawing area, and the
/// reference splits the two: `MC_grpGetFrameBufferHeight` and
/// `MC_grpGetDisplayInfo` report the drawing area (the panel less the strip),
/// every `MC_grp*` primitive lands inside it, and only
/// `MC_grpGetFrameBufferPointer` steps back to the panel's own first row -
/// `wipic_get_frame_pointer` resolves the screen surface through its *parent*,
/// so the address a title blits to starts at the strip.
///
/// A title that blits straight to that pointer therefore skips the strip
/// itself. MapleStory 도적편 does: it keeps the strip height in a field and adds
/// it to every row index it derives from the pointer, which is why its splash
/// logos, HUD icons and shortcut numbers sat a strip's worth too low here while
/// everything drawn through the `MC_grp*` calls stayed where it belonged.
fn new_screen_surface(context: &mut dyn WIPICContext, width: u32, height: u32) -> Result<FrameBuffer> {
    let strip = annunciator_rows(context, width).min(height);

    // The surface spans the whole panel; the framebuffer reports - and every
    // path but the pointer getter uses - the drawing area below the strip.
    let mut framebuffer = FrameBuffer::new(context, width, height.saturating_add(SURFACE_GUARD_ROWS), FRAMEBUFFER_DEPTH)?;
    framebuffer.0.height = height - strip;
    // Only a platform whose indirect pointers are plain addresses reserves a
    // strip, so this is address arithmetic on the buffer just allocated.
    framebuffer.0.buf = WIPICIndirectPtr(framebuffer.0.buf.0 + strip * framebuffer.0.bpl);

    Ok(framebuffer)
}

/// The off-screen surfaces a title has asked for and not destroyed, so a flush
/// can say what is on them. See [`trace_offscreen_surfaces`].
static OFFSCREEN_SURFACES: spin::Mutex<Vec<(WIPICWord, i32, i32)>> = spin::Mutex::new(Vec::new());

/// Flushes between one round of off-screen fingerprints and the next.
///
/// A fingerprint reads every pixel of every surface, which is more work than a
/// frame; a screen worth looking at holds still for many frames, so sampling
/// says the same thing for a fraction of the cost.
const OFFSCREEN_TRACE_EVERY: u32 = 30;

/// Flushes so far, for [`OFFSCREEN_TRACE_EVERY`].
static FLUSHES: AtomicU32 = AtomicU32::new(0);

/// How much is on a surface: how many of its pixels are not black, and how many
/// distinct colours they are.
///
/// Enough to tell a surface that was drawn on from one that was not, which is
/// the question a missing sprite asks. Counting colours stops at 512 - past
/// that the answer is "a picture" either way.
fn surface_content(canvas: &dyn Image) -> (usize, u32) {
    use alloc::collections::BTreeSet;

    let mut colours: BTreeSet<u32> = BTreeSet::new();
    let mut non_black: u32 = 0;

    for colour in canvas.colors() {
        let packed = ((colour.r as u32) << 16) | ((colour.g as u32) << 8) | colour.b as u32;
        if packed != 0 {
            non_black += 1;
        }
        if colours.len() <= 512 {
            colours.insert(packed);
        }
    }

    (colours.len(), non_black)
}

/// Characters a thumbnail cell can be, darkest first.
///
/// A ramp rather than a threshold: an icon and the panel it sits on differ in
/// shade more often than they differ in being lit at all.
const THUMBNAIL_RAMP: [u8; 10] = *b" .:-=+*#%@";

/// Widest a thumbnail gets, in characters.
const THUMBNAIL_COLUMNS: u32 = 48;

/// Tallest a thumbnail gets, in rows.
const THUMBNAIL_ROWS: u32 = 48;

/// Draws a surface small enough to read in a log.
///
/// A count of lit pixels says a surface was drawn on; it does not say what was
/// drawn, and "an icon" and "the panel behind where an icon should be" both
/// come back as a few thousand lit pixels. This is the difference, at the only
/// resolution a log can carry: each cell is the mean brightness of the block it
/// stands for, mapped through [`THUMBNAIL_RAMP`].
fn surface_thumbnail(canvas: &dyn Image) -> Vec<String> {
    let (width, height) = (canvas.width(), canvas.height());
    if width == 0 || height == 0 {
        return Vec::new();
    }

    // A character cell is about twice as tall as it is wide, so a thumbnail
    // that kept one row per column would squash a portrait surface flat - which
    // is what a 240x320 screen is. Take the rows from the surface's own shape.
    let columns = THUMBNAIL_COLUMNS.min(width);
    let rows = (columns * height / width / 2).clamp(1, THUMBNAIL_ROWS.min(height));

    // One pass over the pixels, accumulating into the cell each falls in, so a
    // thumbnail costs the read it already does rather than a read per cell.
    let mut sums = vec![0u64; (columns * rows) as usize];
    let mut counts = vec![0u32; (columns * rows) as usize];

    for (index, colour) in canvas.colors().into_iter().enumerate() {
        let index = index as u32;
        let (x, y) = (index % width, index / width);
        let cell = (y * rows / height) * columns + (x * columns / width);

        let Some(sum) = sums.get_mut(cell as usize) else {
            continue;
        };
        // Rounded to the eye rather than to the spec: green reads brightest.
        *sum += (colour.r as u64 * 2 + colour.g as u64 * 5 + colour.b as u64) / 8;
        counts[cell as usize] += 1;
    }

    (0..rows)
        .map(|row| {
            (0..columns)
                .map(|column| {
                    let cell = (row * columns + column) as usize;
                    let mean = if counts[cell] == 0 { 0 } else { sums[cell] / counts[cell] as u64 };
                    let step = (mean * (THUMBNAIL_RAMP.len() as u64 - 1) / 255).min(THUMBNAIL_RAMP.len() as u64 - 1);

                    THUMBNAIL_RAMP[step as usize] as char
                })
                .collect()
        })
        .collect()
}

/// Whether what is at a registered surface's address is still that surface.
///
/// The registry is a static and outlives a game, so a pointer left in it by the
/// title before names an allocation this one now owns for something else. Read
/// back as a framebuffer, that reaches a depth nothing supports and takes the
/// emulator down with it - which is how this diagnostic came to crash a game
/// that was run after a game that had used one.
///
/// A surface that no longer says the size it was registered at, in a depth
/// there is a pixel format for, is not that surface any more.
fn still_the_surface(raw: &WIPICFramebuffer, width: i32, height: i32) -> bool {
    raw.width as i32 == width && raw.height as i32 == height && matches!(raw.bpp, 16 | 32)
}

/// Reports what is on each off-screen surface, every so often.
///
/// Some titles never hand their art back through this API: 오셔너스 takes the
/// pointer out of a surface and writes pixels into it itself, calling no blit,
/// image or string call at all, and composes the screen the same way. A log of
/// the calls it makes therefore says nothing about what it drew, and a sprite
/// that fails to appear looks identical to one that was never asked for.
///
/// These lines are the missing half. A surface that stays black was never drawn
/// on, which puts the fault in the title's own decision to draw; one that holds
/// a picture the screen does not show puts it in how the title got it there.
fn trace_offscreen_surfaces(context: &mut dyn WIPICContext) {
    let flushes = FLUSHES.fetch_add(1, Ordering::Relaxed);
    if flushes % OFFSCREEN_TRACE_EVERY != 0 {
        return;
    }

    let surfaces = OFFSCREEN_SURFACES.lock().clone();
    let mut stale = Vec::new();

    for (memory, w, h) in surfaces {
        // A diagnostic never gets in the way of the frame it is describing, so
        // a surface that cannot be read is passed over rather than reported.
        let Ok(data_ptr) = context.data_ptr(WIPICIndirectPtr(memory)) else {
            continue;
        };
        let Ok(raw): Result<WIPICFramebuffer> = read_generic(context, data_ptr) else {
            continue;
        };

        if !still_the_surface(&raw, w, h) {
            stale.push(memory);
            continue;
        }

        let Ok(canvas) = FrameBuffer(raw).image(context) else {
            continue;
        };

        let (colours, non_black) = surface_content(&*canvas);

        tracing::info!("OFFSCREEN {memory:#x} {w}x{h} colours={colours} non_black={non_black}");

        // And what it is, not just how much of it there is. A surface nothing
        // drew on has already said so in the line above; drawing its emptiness
        // as sixteen rows of spaces would only crowd out the ones that matter.
        if non_black == 0 {
            continue;
        }

        for line in surface_thumbnail(&*canvas) {
            tracing::info!("OFFSCREEN {memory:#x} |{line}|");
        }
    }

    if !stale.is_empty() {
        OFFSCREEN_SURFACES.lock().retain(|(memory, _, _)| !stale.contains(memory));
    }
}

pub async fn create_offscreen_framebuffer(context: &mut dyn WIPICContext, w: i32, h: i32) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_grpCreateOffScreenFrameBuffer({w}, {h})");

    let framebuffer = new_guarded_surface(context, w as _, h as _)?;

    let memory = context.alloc(size_of::<WIPICFramebuffer>() as WIPICWord)?;
    write_generic(context, context.data_ptr(memory)?, framebuffer.0)?;

    OFFSCREEN_SURFACES.lock().push((memory.0, w, h));

    Ok(memory)
}

pub async fn destroy_offscreen_framebuffer(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<()> {
    tracing::debug!("MC_grpDestroyOffScreenFrameBuffer({:#x})", framebuffer.0);

    OFFSCREEN_SURFACES.lock().retain(|(memory, _, _)| *memory != framebuffer.0);

    context.free(framebuffer)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn copy_frame_buffer(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    dx: i32,
    dy: i32,
    w: i32,
    h: i32,
    src: WIPICIndirectPtr,
    sx: i32,
    sy: i32,
    pgc: WIPICWord,
) -> Result<()> {
    tracing::debug!(
        "MC_grpCopyFrameBuffer({:#x}, {dx}, {dy}, {w}, {h}, {:#x}, {sx}, {sy}, {pgc:#x})",
        dst.0,
        src.0
    );

    let src_framebuffer = FrameBuffer(read_generic(context, context.data_ptr(src)?)?);
    let dst_framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);

    let src_image = src_framebuffer.image(context)?;
    let mut dst_canvas = dst_framebuffer.canvas(context)?;

    blit_magenta_keyed(&mut **dst_canvas, dx, dy, w, h, &*src_image, sx, sy);
    dst_canvas.flush()?;

    Ok(())
}

/// Whether a colour is the magenta (RGB565 `0xF81F`) that feature-phone titles
/// reserve as a transparent colour key. The top five red and blue bits and no
/// green survive the round trip through a 16bpp buffer as exactly `255, 0, 255`.
fn is_transparent_key(color: Color) -> bool {
    color.r >= 0xf8 && color.g <= 0x07 && color.b >= 0xf8
}

/// Copies `src` onto `canvas`, skipping magenta source pixels. A title draws a
/// layer over a magenta fill and blits it expecting the magenta keyed out; the
/// graphics context carries no transparent pixel for these blits, so the
/// convention is honoured here rather than read from it.
fn blit_magenta_keyed(canvas: &mut dyn Canvas, dx: i32, dy: i32, w: i32, h: i32, src: &dyn Image, sx: i32, sy: i32) {
    let src_w = src.width() as i64;
    let src_h = src.height() as i64;
    let dst_w = canvas.image().width() as i64;
    let dst_h = canvas.image().height() as i64;

    for row in 0..h as i64 {
        let sy_px = sy as i64 + row;
        let dy_px = dy as i64 + row;
        if sy_px < 0 || sy_px >= src_h || dy_px < 0 || dy_px >= dst_h {
            continue;
        }
        for col in 0..w as i64 {
            let sx_px = sx as i64 + col;
            let dx_px = dx as i64 + col;
            if sx_px < 0 || sx_px >= src_w || dx_px < 0 || dx_px >= dst_w {
                continue;
            }

            let color = src.get_pixel(sx_px as i32, sy_px as i32);
            if is_transparent_key(color) {
                continue;
            }
            canvas.put_pixel(dx_px as i32, dy_px as i32, color);
        }
    }
}

/// Draw a string from the handset's own bitmap face.
///
/// `y` is the top of the glyph box, the same origin the outline path draws
/// from, and each glyph is stamped a pixel at a time so the result is the 1-bit
/// shape the face stores rather than an anti-aliased rendering of it.
fn draw_bitmap_string(canvas: &mut dyn Canvas, face: &BitmapFace, string: &str, x: i32, y: i32, color: Color, clip: Clip) {
    let mut pen = x;
    for c in string.chars() {
        let Some(glyph) = face.glyph(c) else {
            continue;
        };

        for row in 0..face.height {
            let py = y + row as i32;
            if py < clip.y || py >= clip.y + clip.height as i32 {
                continue;
            }

            for col in 0..glyph.width() {
                if !glyph.pixel(col, row) {
                    continue;
                }

                let px = pen + col as i32;
                if px < clip.x || px >= clip.x + clip.width as i32 {
                    continue;
                }

                canvas.put_pixel(px, py, color);
            }
        }

        pen += glyph.advance as i32;
    }
}

/// The single font size the WIPI-C text path uses, in device pixels. All of
/// `MC_grpGetFontHeight`, `MC_grpGetStringWidth` and `MC_grpDrawString` must
/// agree on it: a title measures text with the first two and lays it out
/// against the third, so any mismatch makes glyphs overlap. It matches the
/// 10px ascent + 2px descent the metric getters below report.
const FONT_PX_HEIGHT: f32 = 12.0;

/// Pixel height the vendor's `MC_grpGetFont` assigns to each size selector.
///
/// The reference (`MC_grpGetFont` in `liblgt_system.so`) maps the size flag to
/// one of seven glyph heights; a title picks a size for a heading or a HUD and
/// then lays it out with `MC_grpGetStringWidth`/`MC_grpGetFontHeight`, so all
/// three have to agree. We had a single 12px face, which shrank every heading
/// to body size and threw off any layout measured against the real height.
fn font_size_px(size: i32) -> i32 {
    match size {
        0x8 => 10,
        0x10 => 14,
        0x1000 => 16,
        0x2000 => 18,
        0x4000 => 19,
        0x8000 => 22,
        _ => 12,
    }
}

/// The pixel height carried by a font handle. `MC_grpGetFont` returns the
/// height itself as the handle, so decoding is the identity for a real handle
/// and the default face for 0 (an unset `SetContext` font).
fn font_handle_height(font: i32) -> f32 {
    if (8..=64).contains(&font) { font as f32 } else { FONT_PX_HEIGHT }
}

/// Ascent for a face of the given height, keeping the reference's 10:2 split for
/// the 12px face (its metric getters report 10px ascent, 2px descent).
fn font_ascent_px(height: f32) -> f32 {
    (height * 5.0 / 6.0).round()
}

pub async fn get_font(_: &mut dyn WIPICContext, face: i32, size: i32, style: i32) -> Result<i32> {
    // The reference picks one of the seven faces its font module carries from
    // the size flag, and falls back to the default face for a flag that names
    // none. With the BIOS faces installed we do the same, and every metric
    // below then reports the face the title was actually handed.
    let height = match bitmap_font::face_for_size(size as u32) {
        Some(face) => face.height as i32,
        None => font_size_px(size),
    };
    tracing::debug!("MC_grpGetFont({face}, {size}, {style}) -> {height}px");

    // The handle is the pixel height; SetContext stores it, and the draw/measure
    // paths read it back (see font_handle_height).
    Ok(height)
}

pub async fn get_font_height(_: &mut dyn WIPICContext, font: i32) -> Result<i32> {
    tracing::trace!("MC_grpGetFontHeight({font})");

    if let Some(face) = bitmap_font::face_for_height(font as u32) {
        return Ok(face.height as i32);
    }

    Ok(font_handle_height(font) as i32)
}

pub async fn get_font_ascent(_: &mut dyn WIPICContext, font: i32) -> Result<i32> {
    tracing::trace!("MC_grpGetFontAscent({font})");

    if let Some(face) = bitmap_font::face_for_height(font as u32) {
        return Ok(face.ascent as i32);
    }

    Ok(font_ascent_px(font_handle_height(font)) as i32)
}

pub async fn get_font_descent(_: &mut dyn WIPICContext, font: i32) -> Result<i32> {
    tracing::trace!("MC_grpGetFontDescent({font})");

    if let Some(face) = bitmap_font::face_for_height(font as u32) {
        return Ok(face.descent as i32);
    }

    let height = font_handle_height(font);
    Ok((height - font_ascent_px(height)) as i32)
}

pub async fn get_string_width(context: &mut dyn WIPICContext, font: i32, ptr_string: WIPICWord, length: i32) -> Result<i32> {
    tracing::trace!("MC_grpGetStringWidth({font}, {ptr_string:#x}, {length})");

    let string = read_wipi_string(context, ptr_string, length)?;
    if let Some(face) = bitmap_font::face_for_height(font as u32) {
        return Ok(face.string_width(&string) as i32);
    }

    Ok(string_width_px(&string, font_handle_height(font)) as i32)
}

/// Read a UTF-16LE WIPI string. `length == -1` means NUL-terminated (a `0`
/// code unit); `length >= 0` reads exactly that many 16-bit code units. This is
/// the wide-character counterpart to `read_wipi_string`; the reference's
/// `MC_grpGetUnicodeStringWidth` takes UCS-2 rather than the EUC-KR of the
/// byte-string calls.
fn read_wipi_unicode_string(context: &mut dyn WIPICContext, ptr: WIPICWord, length: i32) -> Result<String> {
    let units: Vec<u16> = if length >= 0 {
        let mut buf = vec![0u8; (length as usize) * 2];
        context.read_bytes(ptr, &mut buf)?;
        buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    } else {
        let mut out = Vec::new();
        let mut addr = ptr;
        loop {
            let unit: u16 = read_generic(context, addr)?;
            if unit == 0 {
                break;
            }
            out.push(unit);
            addr += 2;
        }
        out
    };

    Ok(String::from_utf16_lossy(&units))
}

/// `MC_grpGetUnicodeStringWidth(font, ustr, len)` - the UCS-2 counterpart to
/// `MC_grpGetStringWidth`. A title that lays out Unicode text measures it with
/// this; left unmapped it returned the diagnostic-stub 0, collapsing every such
/// string to zero width so the glyphs stacked on one another.
pub async fn get_unicode_string_width(context: &mut dyn WIPICContext, font: i32, ptr_string: WIPICWord, length: i32) -> Result<i32> {
    tracing::trace!("MC_grpGetUnicodeStringWidth({font}, {ptr_string:#x}, {length})");

    let string = read_wipi_unicode_string(context, ptr_string, length)?;
    if let Some(face) = bitmap_font::face_for_height(font as u32) {
        return Ok(face.string_width(&string) as i32);
    }

    Ok(string_width_px(&string, font_handle_height(font)) as i32)
}

pub async fn draw_string(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x: i32,
    y: i32,
    ptr_string: WIPICWord,
    length: i32,
    pgc: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpDrawString({:#x}, {x}, {y}, {ptr_string:#x}, {length}, {pgc:#x})", dst.0);

    let string = read_wipi_string(context, ptr_string, length)?;

    draw_text(context, dst, x, y, &string, pgc).await
}

/// `MC_grpDrawUnicodeString(dst, x, y, ustr, len, pgc)` - the UCS-2 counterpart
/// to `MC_grpDrawString`.
///
/// The same text in the same face; only how the title spells it differs.
/// `MC_grpGetUnicodeStringWidth` already measures these, so a title that lays
/// out Unicode text and then draws it now gets both halves.
pub async fn draw_unicode_string(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x: i32,
    y: i32,
    ptr_string: WIPICWord,
    length: i32,
    pgc: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpDrawUnicodeString({:#x}, {x}, {y}, {ptr_string:#x}, {length}, {pgc:#x})", dst.0);

    let string = read_wipi_unicode_string(context, ptr_string, length)?;

    draw_text(context, dst, x, y, &string, pgc).await
}

/// Draws text into a framebuffer in the face the graphics context selected.
///
/// Shared by the byte-string and the UCS-2 call, which differ only in how the
/// characters were spelled in guest memory.
async fn draw_text(context: &mut dyn WIPICContext, dst: WIPICIndirectPtr, x: i32, y: i32, string: &str, pgc: WIPICWord) -> Result<()> {
    if string.is_empty() {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, pgc)?;

    let clip = Clip {
        x: 0,
        y: 0,
        width: framebuffer.0.width,
        height: framebuffer.0.height,
    };

    let color = context_color(&framebuffer, &gctx);

    // The handset's own face when the BIOS supplied one, drawn a pixel at a
    // time exactly as it is stored. The face is the one the title selected with
    // `SetContext(font)`, so a heading it asked `MC_grpGetFont` for a larger
    // size for is drawn - and measured - in that size.
    if let Some(face) = bitmap_font::face_for_height(gctx.font) {
        let mut canvas = framebuffer.canvas(context)?;
        draw_bitmap_string(&mut **canvas, &face, string, x, y, color, clip);
        canvas.flush()?;

        return Ok(());
    }

    // The size the title selected with SetContext(font); 0 keeps the default face.
    let font_height = font_handle_height(gctx.font as i32);
    let baseline = font_ascent_px(font_height);

    let mut canvas = framebuffer.canvas(context)?;
    canvas.draw_text(string, x, y, font_height, baseline, TextAlignment::Left, color, clip);

    // `flush` writes back only the glyph pixels themselves (see write_diff), so
    // a background the title blitted straight into this buffer shows through the
    // gaps between and around the letters instead of being re-stamped black.
    canvas.flush()?;

    Ok(())
}

/// The largest encoding the reference will hand back, and the same limit here.
const MAX_ENCODED_IMAGE_BYTES: usize = 0x0200_0000;

/// `MC_grpEncodeImage(src, x, y, w, h, out_len)` - a rectangle of a framebuffer
/// as an image file.
///
/// Answers the address of a freshly allocated guest buffer holding the encoded
/// bytes and writes their length through `out_len`, or 0 when it cannot, in
/// which case the length it already cleared stays 0. The buffer is the title's
/// to free.
///
/// The contract is the reference emulator's, read out of
/// `ktf.ktfWIPICGraphicsEncodeImage`: six arguments; `*out_len` is cleared
/// before anything else and only written again on success; `x` and `y` must not
/// be negative, `w` and `h` must be positive, and `x + w` / `y + h` must stay
/// inside the framebuffer; the encoding is `image/bmp`; and an empty result, or
/// one past 32 MiB, is refused.
///
/// The BMP itself is 24-bit bottom-up BGR with rows padded to four bytes,
/// written by the same rules as `org.kwis.msp.lcdui.Graphics.encodeImage`, which
/// was derived from the same native encoder - so a title that saves a screenshot
/// through either door gets the same file.
pub async fn encode_image(
    context: &mut dyn WIPICContext,
    src: WIPICIndirectPtr,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    out_len: WIPICWord,
) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_grpEncodeImage({:#x}, {x}, {y}, {width}, {height}, {out_len:#x})", src.0);

    // Cleared first, so a caller that only reads the length sees 0 on every
    // failure below without this having to remember to write it again.
    if out_len != 0 {
        write_generic(context, out_len, 0u32)?;
    }

    if src.0 == 0 {
        return Ok(WIPICIndirectPtr(0));
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(src)?)?);

    if x < 0 || y < 0 || width <= 0 || height <= 0 {
        return Ok(WIPICIndirectPtr(0));
    }

    let right = (x as i64) + width as i64;
    let bottom = (y as i64) + height as i64;
    if right > framebuffer.0.width as i64 || bottom > framebuffer.0.height as i64 {
        return Ok(WIPICIndirectPtr(0));
    }

    // `image` has no answer for anything but 16- and 32-bit pixels, and a title
    // asking about a framebuffer it built some other way should be told no
    // rather than bringing the run down.
    if framebuffer.0.bpp != 16 && framebuffer.0.bpp != 32 {
        tracing::warn!("MC_grpEncodeImage: nothing to encode from a {}-bit framebuffer", framebuffer.0.bpp);

        return Ok(WIPICIndirectPtr(0));
    }

    let encoded = encode_bmp(&*framebuffer.image(context)?, x, y, width as usize, height as usize);
    if encoded.is_empty() || encoded.len() > MAX_ENCODED_IMAGE_BYTES {
        return Ok(WIPICIndirectPtr(0));
    }

    let memory = context.alloc(encoded.len() as WIPICWord)?;
    let address = context.data_ptr(memory)?;
    context.write_bytes(address, &encoded)?;

    if out_len != 0 {
        write_generic(context, out_len, encoded.len() as u32)?;
    }

    tracing::debug!("MC_grpEncodeImage -> {address:#x}, {} bytes", encoded.len());

    Ok(memory)
}

/// A rectangle of an image as a 24-bit BMP file.
///
/// Bottom-up with four-byte row padding, and the 16-bit source channels widened
/// by masking rather than by replicating their low bits - which is what the
/// native encoder does, and what the WIPI-Java `encodeImage` here already did.
fn encode_bmp(image: &dyn Image, x: i32, y: i32, width: usize, height: usize) -> Vec<u8> {
    let row_stride = (width * 3 + 3) & !3;
    let image_size = row_stride * height;
    let file_size = image_size + 54;

    let mut out = vec![0u8; file_size];

    // BITMAPFILEHEADER
    out[0] = b'B';
    out[1] = b'M';
    out[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());

    // BITMAPINFOHEADER
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    out[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    out[30..34].copy_from_slice(&0u32.to_le_bytes());
    out[34..38].copy_from_slice(&(image_size as u32).to_le_bytes());

    for output_row in 0..height {
        let source_y = y + (height - 1 - output_row) as i32;
        let destination_row = 54 + output_row * row_stride;

        for column in 0..width {
            let source_x = x + column as i32;
            if source_x < 0 || source_y < 0 || source_x as u32 >= image.width() || source_y as u32 >= image.height() {
                continue;
            }

            let pixel = image.get_pixel(source_x, source_y);

            let destination = destination_row + column * 3;
            out[destination] = pixel.b & 0xf8;
            out[destination + 1] = pixel.g & 0xfc;
            out[destination + 2] = pixel.r & 0xf8;
        }
    }

    out
}

pub async fn repaint(context: &mut dyn WIPICContext, lcd: i32, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    tracing::debug!("MC_grpRepaint({lcd}, {x}, {y}, {width}, {height})");

    let platform = context.system().platform();
    let screen = platform.screen();
    screen.request_redraw().unwrap();

    Ok(())
}

/// Row length in bytes and the stride to advance by, or `None` when the call
/// asks for nothing that can be delivered.
///
/// `ipl` is a destination stride, but a handset asked less of it than the name
/// suggests: LGT's own runtime checks only that it is positive and then
/// discards it, writing rows packed at `w * 4`. Titles are written against
/// that. Zenonia reads single pixels with `w = 1, ipl = 1`, and rejecting
/// those left it reading uninitialised stack as pixels - which is what its
/// collision checks were deciding on.
///
/// A stride wide enough to be one is still honoured, since nothing says the
/// other handsets discarded it too; anything smaller falls back to packed.
fn destination_stride(w: i32, h: i32, ipl: i32) -> Option<(i32, i32)> {
    if w <= 0 || h <= 0 {
        return None;
    }
    if ipl <= 0 {
        tracing::warn!("MC_grpGetRGBPixels: invalid ipl {ipl}");
        return None;
    }

    let row_bytes = i32::try_from((w as i64).checked_mul(4)?).ok()?;

    Some((row_bytes, ipl.max(row_bytes)))
}

#[allow(clippy::too_many_arguments)]
pub async fn get_rgb_pixels(
    context: &mut dyn WIPICContext,
    src: WIPICIndirectPtr,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pd: WIPICWord,
    ipl: i32,
) -> Result<()> {
    tracing::debug!("MC_grpGetRGBPixels({:#x}, {x}, {y}, {w}, {h}, {pd:#x}, {ipl})", src.0);

    let Some((row_bytes, ipl)) = destination_stride(w, h, ipl) else {
        return Ok(());
    };

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(src)?)?);
    let image = framebuffer.image(context)?;

    let mut row = vec![0u8; row_bytes as usize];
    for dy in 0..h {
        for dx in 0..w {
            let sx = x + dx;
            let sy = y + dy;
            let color = if sx < 0 || sy < 0 || sx >= image.width() as i32 || sy >= image.height() as i32 {
                Color { a: 0, r: 0, g: 0, b: 0 }
            } else {
                image.get_pixel(sx, sy)
            };
            // WIPI spec: pixels are 0x00RRGGBB (top byte zero).
            let rgb = Rgb8Pixel::from_color(color);
            let off = (dx as usize) * 4;
            row[off..off + 4].copy_from_slice(&rgb.to_le_bytes());
        }
        let row_offset = match (dy as u32).checked_mul(ipl as u32) {
            Some(n) => n,
            None => {
                tracing::warn!("MC_grpGetRGBPixels: row offset overflow (dy={dy}, ipl={ipl})");
                return Ok(());
            }
        };
        let dst_addr = match pd.checked_add(row_offset) {
            Some(n) => n,
            None => {
                tracing::warn!("MC_grpGetRGBPixels: destination address overflow (pd={pd:#x}, row_offset={row_offset})");
                return Ok(());
            }
        };
        context.write_bytes(dst_addr, &row)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn set_rgb_pixels(
    context: &mut dyn WIPICContext,
    dst: WIPICIndirectPtr,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    psrc: WIPICWord,
    ibpl: i32,
    _pgc: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpSetRGBPixels({:#x}, {x}, {y}, {w}, {h}, {psrc:#x}, {ibpl})", dst.0);

    if w <= 0 || h <= 0 {
        return Ok(());
    }
    let row_bytes = match (w as usize).checked_mul(4) {
        Some(n) => n,
        None => {
            tracing::warn!("MC_grpSetRGBPixels: row size overflow (w={w})");
            return Ok(());
        }
    };
    if ibpl < row_bytes as i32 {
        tracing::warn!("MC_grpSetRGBPixels: invalid ibpl {ibpl} (need >= {row_bytes})");
        return Ok(());
    }
    let total_bytes = match row_bytes.checked_mul(h as usize) {
        Some(n) => n,
        None => {
            tracing::warn!("MC_grpSetRGBPixels: total size overflow (w={w}, h={h})");
            return Ok(());
        }
    };

    let mut buf = vec![0u8; total_bytes];
    for dy in 0..h {
        let off = (dy as usize) * row_bytes;
        let row_offset = match (dy as u32).checked_mul(ibpl as u32) {
            Some(n) => n,
            None => {
                tracing::warn!("MC_grpSetRGBPixels: row offset overflow (dy={dy}, ibpl={ibpl})");
                return Ok(());
            }
        };
        let src_addr = match psrc.checked_add(row_offset) {
            Some(n) => n,
            None => {
                tracing::warn!("MC_grpSetRGBPixels: source address overflow (psrc={psrc:#x}, row_offset={row_offset})");
                return Ok(());
            }
        };
        context.read_bytes(src_addr, &mut buf[off..off + row_bytes])?;
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let mut canvas = framebuffer.canvas(context)?;
    for dy in 0..h {
        for dx in 0..w {
            let off = ((dy as usize) * (w as usize) + dx as usize) * 4;
            // WIPI spec: pixels are 0x00RRGGBB.
            let rgb = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
            let color = Rgb8Pixel::to_color(rgb);
            canvas.put_pixel(x + dx, y + dy, color);
        }
    }
    canvas.flush()?;

    Ok(())
}

pub async fn get_image_framebuffer(_context: &mut dyn WIPICContext, image: WIPICIndirectPtr) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_grpGetImageFrameBuffer({:#x})", image.0);

    // WIPICImage starts with `img: WIPICFramebuffer` at offset 0,
    // so the image handle doubles as a framebuffer handle.
    Ok(image)
}

pub async fn get_image_property(context: &mut dyn WIPICContext, image: WIPICIndirectPtr, property: i32) -> Result<i32> {
    tracing::debug!("MC_grpGetImageProperty({:#x}, {property})", image.0);

    let image: WIPICImage = read_generic(context, context.data_ptr(image)?)?;

    Ok(match property {
        4 => image.img.width as _,
        5 => image.img.height as _,
        _ => {
            tracing::warn!("unknown property {property}");
            0
        }
    })
}

pub async fn draw_rect(context: &mut dyn WIPICContext, dst: WIPICIndirectPtr, x: i32, y: i32, w: i32, h: i32, pgc: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpDrawRect({:#x}, {x}, {y}, {w}, {h}, {pgc:#x})", dst.0);

    if w <= 0 || h <= 0 {
        return Ok(());
    }

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, pgc)?;
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: x as _,
        y: y as _,
        width: w as _,
        height: h as _,
    };

    let color = context_color(&framebuffer, &gctx);
    canvas.draw_rect(x as _, y as _, w as _, h as _, color, clip);
    canvas.flush()?;

    Ok(())
}

pub async fn draw_line(context: &mut dyn WIPICContext, dst: WIPICIndirectPtr, x1: i32, y1: i32, x2: i32, y2: i32, pgc: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpDrawLine({:#x}, {x1}, {y1}, {x2}, {y2}, {pgc:#x})", dst.0);

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, pgc)?;
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: 0,
        y: 0,
        width: framebuffer.0.width as _,
        height: framebuffer.0.height as _,
    };

    let color = context_color(&framebuffer, &gctx);
    canvas.draw_line(x1 as _, y1 as _, x2 as _, y2 as _, color, clip);
    canvas.flush()?;

    Ok(())
}

pub async fn post_event(context: &mut dyn WIPICContext, id: i32, r#type: i32, param1: i32, param2: i32) -> Result<i32> {
    tracing::debug!("MC_grpPostEvent({id}, {type}, {param1}, {param2})");

    context.system().event_queue().push(Event::Notify { r#type, param1, param2 });

    Ok(0)
}

// it's not documented api, but lgt apps gets pointer via api call
/// What the reference's framebuffer getters answer for a handle of zero.
///
/// `wipic_get_frame_pointer` (@0x1ad7b0), `_width` (@0x1aaea4), `_height`
/// (@0x1aacd8) and `_bpl` (@0x1ac044) all open by testing the handle and
/// returning before they touch it - `cmp r0, #0` / `mvneq r0, #0` in two of
/// them, `subs`/`subeq r0, r0, #1` in the others. Every one of them hands back
/// minus one.
///
/// A title reaches them with zero. These getters sit in a direct-blit inner
/// loop, called with whatever register happens to be live rather than with a
/// framebuffer, which is the same reason `get_framebuffer_bpp` ignores its
/// argument outright. 열혈택시 does it while loading its images and, with the
/// handle dereferenced instead, the read of address zero failed the whole VM
/// rather than the one call - `net.wie.WieError: Invalid memory access;
/// address: 0` before its first frame.
pub const NO_FRAMEBUFFER: i32 = -1;

pub async fn get_framebuffer_pointer(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<WIPICWord> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_POINTER({:#x})", framebuffer.0);

    if framebuffer.0 == 0 {
        return Ok(NO_FRAMEBUFFER as WIPICWord);
    }

    let handle = framebuffer;
    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(handle)?)?;

    Ok(framebuffer.buf.0 - screen_pointer_lead(context, handle.0, framebuffer.bpl))
}

/// Bytes between the pointer the screen framebuffer hands a title and the
/// drawing area it reports - the status strip - and zero for every other
/// framebuffer. See `new_screen_surface`.
///
/// Generic over the reader so a platform's synchronous fast path for
/// `MC_grpGetFrameBufferPointer` reports the same address this one does.
pub fn screen_pointer_lead<R>(reader: &R, handle: WIPICWord, bpl: WIPICWord) -> WIPICWord
where
    R: ?Sized + wie_util::ByteRead,
{
    let screen: u32 = read_generic(reader, SCREEN_FRAMEBUFFER_PTR).unwrap_or(0);
    if screen == 0 || screen != handle {
        return 0;
    }

    read_generic::<u32, _>(reader, ANNUNCIATOR_ROWS_PTR).unwrap_or(0) * bpl
}

pub async fn get_framebuffer_width(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_WIDTH({:#x})", framebuffer.0);

    if framebuffer.0 == 0 {
        return Ok(NO_FRAMEBUFFER);
    }

    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(framebuffer)?)?;

    Ok(framebuffer.width as _)
}

pub async fn get_framebuffer_height(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_HEIGHT({:#x})", framebuffer.0);

    if framebuffer.0 == 0 {
        return Ok(NO_FRAMEBUFFER);
    }

    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(framebuffer)?)?;

    Ok(framebuffer.height as _)
}

pub async fn get_framebuffer_bpl(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_BPL({:#x})", framebuffer.0);

    if framebuffer.0 == 0 {
        return Ok(NO_FRAMEBUFFER);
    }

    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(framebuffer)?)?;

    Ok(framebuffer.bpl as _)
}

pub async fn get_framebuffer_bpp(_context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_BPP({:#x})", framebuffer.0);

    // The vendor `wipic_get_frame_bpp` ignores its argument and returns the
    // display's depth from a global. Titles rely on that: their direct-blit
    // inner loop calls this with whatever register happens to be live - the
    // frame pointer, the width - not a framebuffer handle, then uses the result
    // as the pixel stride. Dereferencing that argument as a handle here read a
    // garbage struct and returned a garbage depth, so a title's glyph and
    // sprite writes landed at the wrong offset and never appeared while its
    // `MC_grpFillRect` panels, which never touch this call, drew fine. Every
    // framebuffer this runtime hands out is `FRAMEBUFFER_DEPTH`, so report it
    // directly and ignore the argument, as the vendor does.
    Ok(FRAMEBUFFER_DEPTH as _)
}

#[cfg(test)]
mod tests {
    use wie_util::{read_generic, write_generic};

    use wie_backend::canvas::{ArgbPixel, Image, VecImageBuffer};

    use super::WIPICGraphicsContextIdx as Idx;
    use super::{destination_stride, get_context, init_context, set_context, surface_content, surface_thumbnail};
    use crate::context::{WIPICContext, test::TestContext};

    /// A framebuffer of `pixels` (ARGB, row-major) as the guest holds one: the
    /// indirect pointer a WIPI-C call is handed.
    async fn framebuffer_of(context: &mut TestContext, width: u32, height: u32, pixels: &[u32]) -> super::WIPICIndirectPtr {
        let image = VecImageBuffer::<ArgbPixel>::from_raw(width, height, pixels.to_vec());
        let framebuffer = super::FrameBuffer::from_image(context, &image).unwrap();

        let handle = context.alloc(core::mem::size_of::<wipi_types::wipic::WIPICFramebuffer>() as u32).unwrap();
        let address = context.data_ptr(handle).unwrap();
        write_generic(context, address, framebuffer.0).unwrap();

        handle
    }

    fn test_context() -> TestContext {
        use alloc::boxed::Box;
        use test_utils::TestPlatform;
        use wie_backend::{DefaultTaskRunner, System};

        TestContext::with_system(System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner))
    }

    /// The encoding is a 24-bit bottom-up BMP of the rectangle asked for, and
    /// the length comes back through the pointer beside it.
    #[futures_test::test]
    async fn an_encoded_image_is_a_bottom_up_bmp() {
        let mut context = test_context();

        // Two rows: white on top, red underneath.
        let framebuffer = framebuffer_of(&mut context, 2, 2, &[0xffff_ffff, 0xffff_ffff, 0xfff8_0000, 0xfff8_0000]).await;

        let encoded_buffer = super::encode_image(&mut context, framebuffer, 0, 0, 2, 2, 0x100).await.unwrap();
        assert_ne!(encoded_buffer.0, 0);

        let length: u32 = read_generic(&context, 0x100).unwrap();
        // 54-byte header, rows of 2 pixels padded from 6 to 8 bytes.
        assert_eq!(length, 54 + 8 * 2);

        let mut encoded = alloc::vec![0u8; length as usize];
        let data = context.data_ptr(encoded_buffer).unwrap();
        wie_util::ByteRead::read_bytes(&context, data, &mut encoded).unwrap();

        assert_eq!(&encoded[..2], b"BM");
        assert_eq!(u32::from_le_bytes(encoded[10..14].try_into().unwrap()), 54);
        assert_eq!(i32::from_le_bytes(encoded[18..22].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(encoded[22..26].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(encoded[28..30].try_into().unwrap()), 24);

        // Bottom-up: the first stored row is the red one, written BGR.
        assert_eq!(&encoded[54..60], &[0x00, 0x00, 0xf8, 0x00, 0x00, 0xf8]);
        // And the last is the white one.
        assert_eq!(&encoded[62..68], &[0xf8, 0xfc, 0xf8, 0xf8, 0xfc, 0xf8]);
    }

    /// Every way the reference refuses answers 0 and leaves the length at 0,
    /// so a caller that only reads the length is not told a size it cannot
    /// trust.
    #[futures_test::test]
    async fn a_rectangle_that_does_not_fit_is_refused() {
        let mut context = test_context();
        let framebuffer = framebuffer_of(&mut context, 2, 2, &[0xffff_ffff; 4]).await;

        for (x, y, width, height) in [(-1, 0, 2, 2), (0, -1, 2, 2), (0, 0, 0, 2), (0, 0, 2, 0), (1, 0, 2, 2), (0, 1, 2, 2)] {
            write_generic(&mut context, 0x100, 0xffff_ffffu32).unwrap();

            assert_eq!(
                super::encode_image(&mut context, framebuffer, x, y, width, height, 0x100)
                    .await
                    .unwrap()
                    .0,
                0
            );

            let length: u32 = read_generic(&context, 0x100).unwrap();
            assert_eq!(length, 0, "{x},{y} {width}x{height}");
        }
    }

    /// The same text spelled either way paints the same pixels, so a title that
    /// lays out Unicode gets what the byte-string call would have given it.
    #[futures_test::test]
    async fn unicode_text_paints_what_the_byte_string_call_paints() {
        let mut context = test_context();

        let pgc = 0x200;
        init_context(&mut context, pgc).await.unwrap();

        let byte_string = 0x300;
        wie_util::ByteWrite::write_bytes(&mut context, byte_string, b"Hi\0").unwrap();
        let unicode_string = 0x400;
        wie_util::ByteWrite::write_bytes(&mut context, unicode_string, &[b'H', 0, b'i', 0, 0, 0]).unwrap();

        let drawn_as_bytes = framebuffer_of(&mut context, 32, 16, &[0xff00_0000; 32 * 16]).await;
        super::draw_string(&mut context, drawn_as_bytes, 0, 0, byte_string, -1, pgc)
            .await
            .unwrap();

        let drawn_as_unicode = framebuffer_of(&mut context, 32, 16, &[0xff00_0000; 32 * 16]).await;
        super::draw_unicode_string(&mut context, drawn_as_unicode, 0, 0, unicode_string, -1, pgc)
            .await
            .unwrap();

        let from_bytes = super::FrameBuffer(read_generic(&context, context.data_ptr(drawn_as_bytes).unwrap()).unwrap())
            .data(&context)
            .unwrap();
        let from_unicode = super::FrameBuffer(read_generic(&context, context.data_ptr(drawn_as_unicode).unwrap()).unwrap())
            .data(&context)
            .unwrap();

        assert_eq!(from_bytes, from_unicode);
        // And something was actually drawn, so the comparison is not of two
        // empty buffers.
        assert!(from_bytes.iter().any(|&byte| byte != 0));
    }

    /// A surface with `drawn` pixels of one colour on it and the rest black.
    fn surface_with(width: u32, height: u32, drawn: u32, colour: u32) -> impl Image {
        let mut raw: alloc::vec::Vec<u32> = alloc::vec![0xff00_0000; (width * height) as usize];
        for pixel in raw.iter_mut().take(drawn as usize) {
            *pixel = 0xff00_0000 | colour;
        }

        VecImageBuffer::<ArgbPixel>::from_raw(width, height, raw)
    }

    /// A framebuffer as the guest stores one.
    fn described(width: u32, height: u32, bpp: u32) -> wipi_types::wipic::WIPICFramebuffer {
        wipi_types::wipic::WIPICFramebuffer {
            width,
            height,
            bpl: width * (bpp / 8),
            bpp,
            buf: super::WIPICIndirectPtr(0x1000),
        }
    }

    /// The reference's getters test the handle before they touch it and hand
    /// back minus one, so a title that calls them with zero - 열혈택시 does,
    /// out of its image loader - gets an answer instead of a dead VM.
    #[futures_test::test]
    async fn a_null_framebuffer_is_answered_the_way_the_reference_answers_it() {
        use alloc::boxed::Box;
        use test_utils::TestPlatform;
        use wie_backend::{DefaultTaskRunner, System};

        use crate::context::test::TestContext;

        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);
        let null = super::WIPICIndirectPtr(0);

        assert_eq!(super::get_framebuffer_width(&mut context, null).await.unwrap(), super::NO_FRAMEBUFFER);
        assert_eq!(super::get_framebuffer_height(&mut context, null).await.unwrap(), super::NO_FRAMEBUFFER);
        assert_eq!(super::get_framebuffer_bpl(&mut context, null).await.unwrap(), super::NO_FRAMEBUFFER);
        assert_eq!(
            super::get_framebuffer_pointer(&mut context, null).await.unwrap(),
            super::NO_FRAMEBUFFER as u32
        );
    }

    #[test]
    fn a_surface_that_stopped_being_one_is_not_read_as_one() {
        // What a live 60x60 surface looks like.
        assert!(super::still_the_surface(&described(60, 60, 16), 60, 60));
        assert!(super::still_the_surface(&described(60, 60, 32), 60, 60));

        // And what the allocation looks like once another title has it: zeroed,
        // or holding something whose depth has no pixel format, or the wrong
        // size for what was registered. Reading any of these as a framebuffer
        // is what crashed a game run after one that had used a surface.
        assert!(!super::still_the_surface(&described(0, 0, 0), 60, 60));
        assert!(!super::still_the_surface(&described(60, 60, 8), 60, 60));
        assert!(!super::still_the_surface(&described(61, 60, 16), 60, 60));
    }

    #[test]
    fn a_surface_nothing_drew_on_reads_as_empty() {
        let (colours, non_black) = surface_content(&surface_with(60, 60, 0, 0));

        // One colour - black - and nothing lit. This is what a sprite that was
        // never drawn looks like, and it is the whole point of the line.
        assert_eq!(non_black, 0);
        assert_eq!(colours, 1);
    }

    #[test]
    fn a_thumbnail_of_an_empty_surface_is_blank() {
        let lines = surface_thumbnail(&surface_with(60, 60, 0, 0));

        assert!(!lines.is_empty());
        assert!(lines.iter().all(|line| line.chars().all(|c| c == ' ')));
    }

    #[test]
    fn a_thumbnail_shows_where_the_lit_pixels_are() {
        // The top third of a surface lit white, the rest black: the top rows
        // read bright and the bottom rows blank. This is the whole job - an
        // icon that was drawn looks different from a panel that was not.
        let lines = surface_thumbnail(&surface_with(60, 60, 60 * 20, 0xffffff));

        assert_eq!(lines[0].chars().next(), Some('@'));
        assert!(lines.last().unwrap().chars().all(|c| c == ' '));
    }

    #[test]
    fn a_thumbnail_keeps_the_shape_of_what_it_draws() {
        // A 240x320 screen is taller than it is wide, and a thumbnail that gave
        // it as many rows as a landscape one would squash it flat - which is
        // what makes a panel unreadable in a log.
        let portrait = surface_thumbnail(&surface_with(240, 320, 0, 0)).len();
        let landscape = surface_thumbnail(&surface_with(320, 240, 0, 0)).len();

        assert!(portrait > landscape, "{portrait} rows for a portrait, {landscape} for a landscape");
    }

    #[test]
    fn a_thumbnail_never_gets_wider_than_it_can_be_read_at() {
        for (width, height) in [(60, 60), (240, 320), (8, 4)] {
            for line in surface_thumbnail(&surface_with(width, height, 0, 0)) {
                assert!(line.chars().count() <= super::THUMBNAIL_COLUMNS as usize);
            }
        }
    }

    #[test]
    fn a_surface_something_drew_on_says_how_much() {
        let (colours, non_black) = surface_content(&surface_with(60, 60, 900, 0x3366ff));

        assert_eq!(non_black, 900);
        assert_eq!(colours, 2);
    }

    /// A clet saves the drawing state with `MC_grpGetContext` and later restores
    /// it with `MC_grpSetContext`, so a get has to report back exactly what a set
    /// stored. When this read nothing (the old stub), the saved state was zero and
    /// the restore wrecked every later draw.
    #[futures_test::test]
    async fn get_context_reads_back_what_set_context_stored() {
        const CTX: u32 = 0x1000;
        const OUT: u32 = 0x2000;

        let mut context = TestContext::new();
        init_context(&mut context, CTX).await.unwrap();

        // Scalars are passed to set by value and returned by get through a pointer.
        for (op, value) in [
            (Idx::FgPixelIdx, 0x1234u32),
            (Idx::BgPixelIdx, 0x5678),
            (Idx::AlphaIdx, 0x80),
            (Idx::FontIdx, 0x42),
            (Idx::StyleIdx, 0x3),
        ] {
            set_context(&mut context, CTX, op, value).await.unwrap();
            get_context(&mut context, CTX, op, OUT).await.unwrap();
            assert_eq!(read_generic::<u32, _>(&context, OUT).unwrap(), value, "op {op:?}");
        }

        // The clip is passed through memory both ways as four 32-bit words
        // (x1, y1, x2, y2); set decrements the bottom-right corner on the way in
        // and get reports it one past what is stored, matching liblgt_system.so,
        // so a get-then-set round-trip is the identity.
        write_generic(&mut context, OUT, 10u32).unwrap();
        write_generic(&mut context, OUT + 4, 20u32).unwrap();
        write_generic(&mut context, OUT + 8, 100u32).unwrap();
        write_generic(&mut context, OUT + 12, 200u32).unwrap();
        set_context(&mut context, CTX, Idx::ClipIdx, OUT).await.unwrap();
        get_context(&mut context, CTX, Idx::ClipIdx, OUT).await.unwrap();
        assert_eq!(read_generic::<u32, _>(&context, OUT).unwrap(), 10);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 4).unwrap(), 20);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 8).unwrap(), 100);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 12).unwrap(), 200);

        // The offset is the same two-32-bit-word pair through memory, restored
        // verbatim.
        write_generic(&mut context, OUT, 7u32).unwrap();
        write_generic(&mut context, OUT + 4, 9u32).unwrap();
        set_context(&mut context, CTX, Idx::OffsetIdx, OUT).await.unwrap();
        get_context(&mut context, CTX, Idx::OffsetIdx, OUT).await.unwrap();
        assert_eq!(read_generic::<u32, _>(&context, OUT).unwrap(), 7);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 4).unwrap(), 9);
    }

    /// A null context or destination is a no-op, exactly as the vendor guards it,
    /// not a memory fault.
    #[futures_test::test]
    async fn get_context_tolerates_null_pointers() {
        let mut context = TestContext::new();
        get_context(&mut context, 0, Idx::FgPixelIdx, 0x2000).await.unwrap();
        get_context(&mut context, 0x1000, Idx::FgPixelIdx, 0).await.unwrap();
    }

    /// MC_grpInitContext plants non-zero defaults (full clip, white background,
    /// opaque alpha, param1, the 12px font) rather than zeroing the block, and a
    /// game that never sets those fields draws against them. Reading each one
    /// straight back through GetContext proves the port matches the firmware.
    #[futures_test::test]
    async fn init_context_plants_the_reference_defaults() {
        const CTX: u32 = 0x1000;
        const OUT: u32 = 0x2000;

        let mut context = TestContext::new();
        init_context(&mut context, CTX).await.unwrap();

        for (op, value) in [
            (Idx::FgPixelIdx, 0x0),
            (Idx::BgPixelIdx, 0x00ff_ffff),
            (Idx::AlphaIdx, 0xff),
            (Idx::FontIdx, 12),
            (Idx::StyleIdx, 0x0),
        ] {
            get_context(&mut context, CTX, op, OUT).await.unwrap();
            assert_eq!(read_generic::<u32, _>(&context, OUT).unwrap(), value, "op {op:?}");
        }

        // The clip starts at the whole plane; GetContext reports the corner one
        // past what is stored (0x7fff -> 0x8000).
        get_context(&mut context, CTX, Idx::ClipIdx, OUT).await.unwrap();
        assert_eq!(read_generic::<u32, _>(&context, OUT).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 4).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 8).unwrap(), 0x8000);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 12).unwrap(), 0x8000);
    }

    /// The colour path is the driver's direct RGB565: 5 bits red, 6 green, 5
    /// blue, packed r>>3 << 11 | g>>2 << 5 | b>>3, and it round-trips through the
    /// getter within that precision. This is what MC_grpGetDisplayInfo advertises
    /// (masks 0xf800/0x07e0/0x001f) and what a title's own blitter converts
    /// against, so it has to be exact.
    #[futures_test::test]
    async fn pixel_from_rgb_is_direct_rgb565() {
        let mut context = TestContext::new();
        assert_eq!(super::get_pixel_from_rgb(&mut context, 0xff, 0, 0).await.unwrap(), 0xf800);
        assert_eq!(super::get_pixel_from_rgb(&mut context, 0, 0xff, 0).await.unwrap(), 0x07e0);
        assert_eq!(super::get_pixel_from_rgb(&mut context, 0, 0, 0xff).await.unwrap(), 0x001f);
        assert_eq!(super::get_pixel_from_rgb(&mut context, 0xff, 0xff, 0xff).await.unwrap(), 0xffff);
        assert_eq!(super::get_pixel_from_rgb(&mut context, 0, 0, 0).await.unwrap(), 0x0000);

        // A pure-red pixel decodes back to full red (5-bit max scaled to 8-bit).
        const OUT: u32 = 0x3000;
        super::get_rgb_from_pixel(&mut context, 0xf800, OUT, OUT + 4, OUT + 8).await.unwrap();
        assert_eq!(read_generic::<u32, _>(&context, OUT).unwrap(), 0xff);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 4).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, OUT + 8).unwrap(), 0);
    }

    /// The size selector maps to the seven glyph heights `MC_grpGetFont` assigns,
    /// and the handle those return round-trips through the height getter.
    #[futures_test::test]
    async fn get_font_reports_the_reference_heights() {
        let mut context = TestContext::new();
        for (size, height) in [
            (0x8, 10),
            (0x10, 14),
            (0x1000, 16),
            (0x2000, 18),
            (0x4000, 19),
            (0x8000, 22),
            (0, 12),
            (0x1234, 12),
        ] {
            let handle = super::get_font(&mut context, 0, size, 0).await.unwrap();
            assert_eq!(handle, height, "size {size:#x}");
            assert_eq!(super::get_font_height(&mut context, handle).await.unwrap(), height, "height of {size:#x}");
        }
        // An unset SetContext font (0) falls back to the default face.
        assert_eq!(super::get_font_height(&mut context, 0).await.unwrap(), 12);
    }

    /// A single pixel read into four bytes of stack, which is how a title asks
    /// whether it has walked into something. LGT's runtime takes `ipl = 1`.
    #[test]
    fn a_one_pixel_probe_is_delivered() {
        assert_eq!(destination_stride(1, 1, 1), Some((4, 4)));
    }

    #[test]
    fn a_real_stride_is_honoured() {
        // Reading 100 pixels into a 240 wide buffer.
        assert_eq!(destination_stride(100, 50, 960), Some((400, 960)));
    }

    /// Too small to be a stride, so the rows pack - which is what the handset
    /// did with any value at all.
    #[test]
    fn a_short_stride_packs_instead_of_dropping_the_call() {
        assert_eq!(destination_stride(8, 4, 3), Some((32, 32)));
    }

    #[test]
    fn nothing_to_read_is_dropped() {
        assert_eq!(destination_stride(0, 4, 16), None);
        assert_eq!(destination_stride(4, 0, 16), None);
        assert_eq!(destination_stride(4, 4, 0), None);
        assert_eq!(destination_stride(4, 4, -1), None);
    }

    #[test]
    fn an_unreasonable_width_is_dropped_rather_than_overflowing() {
        assert_eq!(destination_stride(i32::MAX, 1, i32::MAX), None);
    }
}
