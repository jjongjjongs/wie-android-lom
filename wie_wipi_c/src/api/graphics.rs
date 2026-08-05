mod framebuffer;
mod grp_context;
mod image;

use core::mem::size_of;

use alloc::{string::String, vec, vec::Vec};

use wie_backend::{
    Event,
    canvas::{Canvas, Clip, Color, Image, PixelType, Rgb8Pixel, Rgb565Pixel, TextAlignment, string_width},
};
use wie_util::{Result, read_generic, read_null_terminated_string_bytes, write_generic};

use wipi_types::wipic::{WIPICDisplayInfo, WIPICFramebuffer, WIPICGraphicsContext, WIPICImage, WIPICIndirectPtr, WIPICWord};

use crate::context::WIPICContext;

use self::{framebuffer::FrameBuffer, grp_context::WIPICGraphicsContextIdx, image::create_wipi_image};

const FRAMEBUFFER_DEPTH: u32 = 16; // XXX hardcode to 16bpp as some game requires 16bpp framebuffer
const SCREEN_FRAMEBUFFER_PTR: u32 = 0x7fff1000;
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

    let (width, height) = {
        let platform = context.system().platform();
        let screen = platform.screen();
        (screen.width(), screen.height())
    };

    let framebuffer = FrameBuffer::new(context, width, height, FRAMEBUFFER_DEPTH)?;

    let memory = context.alloc(size_of::<WIPICFramebuffer>() as WIPICWord)?;
    write_generic(context, context.data_ptr(memory)?, framebuffer.0)?;
    write_generic(context, SCREEN_FRAMEBUFFER_PTR, memory.0)?;

    Ok(memory)
}

pub async fn init_context(context: &mut dyn WIPICContext, p_grp_ctx: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpInitContext({p_grp_ctx:#x})");

    let grp_ctx = WIPICGraphicsContext::default();
    write_generic(context, p_grp_ctx, grp_ctx)?;
    Ok(())
}

pub async fn set_context(context: &mut dyn WIPICContext, p_grp_ctx: WIPICWord, op: WIPICGraphicsContextIdx, pv: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpSetContext({p_grp_ctx:#x}, {op:?}, {pv:#x})");

    let mut grp_ctx: WIPICGraphicsContext = read_generic(context, p_grp_ctx)?;
    match op {
        WIPICGraphicsContextIdx::ClipIdx => {
            grp_ctx.clip = read_generic(context, pv)?;
        }
        WIPICGraphicsContextIdx::FgPixelIdx => {
            grp_ctx.fgpxl = pv as _;
        }
        WIPICGraphicsContextIdx::BgPixelIdx => {
            grp_ctx.bgpxl = pv as _;
        }
        WIPICGraphicsContextIdx::TransPixelIdx => {
            grp_ctx.transpxl = pv as _;
        }
        WIPICGraphicsContextIdx::AlphaIdx => {
            grp_ctx.alpha = pv as _;
            // grp_ctx.pixel_op_func_ptr = todo!();
            // grp_ctx.param1 = todo!();
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
        WIPICGraphicsContextIdx::OffsetIdx => {
            grp_ctx.offset = read_generic(context, pv)?;
        }
        _ => {
            tracing::warn!("MC_grpSetContext({p_grp_ctx:#x}, {op:?}, {pv:#x}): ignoring invalid op");
        }
    }
    write_generic(context, p_grp_ctx, grp_ctx)?;

    Ok(())
}

pub async fn put_pixel(context: &mut dyn WIPICContext, dst_fb: WIPICIndirectPtr, x: i32, y: i32, p_gctx: WIPICWord) -> Result<()> {
    tracing::debug!("MC_grpPutPixel({:#x}, {x}, {y}, {p_gctx:?})", dst_fb.0);

    let framebuffer = FrameBuffer(read_generic(context, context.data_ptr(dst_fb)?)?);
    let gctx: WIPICGraphicsContext = read_generic(context, p_gctx)?;

    let mut canvas = framebuffer.canvas(context)?;
    let color = framebuffer.pixel_to_color(gctx.fgpxl);
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
    let mut canvas = framebuffer.canvas(context)?;

    let clip = Clip {
        x: x as _,
        y: y as _,
        width: w as _,
        height: h as _,
    };

    let color = framebuffer.pixel_to_color(gctx.fgpxl);
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
    arc_angle: i32,
    p_gctx: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpDrawArc({:#x}, {x}, {y}, {w}, {h}, {start_angle}, {arc_angle}, {p_gctx:#x})", dst.0);

    if w <= 0 || h <= 0 {
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

    let color = framebuffer.pixel_to_color(gctx.fgpxl);
    canvas.draw_arc(x as _, y as _, w as _, h as _, start_angle, arc_angle, color, clip);
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
    arc_angle: i32,
    p_gctx: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_grpFillArc({:#x}, {x}, {y}, {w}, {h}, {start_angle}, {arc_angle}, {p_gctx:#x})", dst.0);

    if w <= 0 || h <= 0 {
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

    let color = framebuffer.pixel_to_color(gctx.fgpxl);
    canvas.fill_arc(x as _, y as _, w as _, h as _, start_angle, arc_angle, color, clip);
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
    let color = framebuffer.pixel_to_color(gctx.fgpxl);
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
    let color = framebuffer.pixel_to_color(gctx.fgpxl);
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

    let image = create_wipi_image(context, image_data, offset, len)?;

    let memory = context.alloc(size_of::<WIPICImage>() as WIPICWord)?;
    write_generic(context, ptr_image, memory)?;
    write_generic(context, context.data_ptr(memory)?, image)?;

    Ok(1) // MC_GRP_IMAGE_DONE
}

pub async fn destroy_image(context: &mut dyn WIPICContext, image: WIPICIndirectPtr) -> Result<()> {
    tracing::debug!("MC_grpDestroyImage({:#x})", image.0);

    context.free(image)?;

    Ok(())
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

    let platform = context.system().platform();
    let screen = platform.screen();

    screen.paint(&*src_canvas);

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

    let platform = context.system().platform();
    let screen = platform.screen();

    let info = WIPICDisplayInfo {
        bpp: FRAMEBUFFER_DEPTH,
        depth: 16,
        width: screen.width(),
        height: screen.height(),
        bpl: 2 * screen.width(),
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

pub async fn create_offscreen_framebuffer(context: &mut dyn WIPICContext, w: i32, h: i32) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_grpCreateOffScreenFrameBuffer({w}, {h})");

    let framebuffer = FrameBuffer::new(context, w as _, h as _, FRAMEBUFFER_DEPTH)?;

    let memory = context.alloc(size_of::<WIPICFramebuffer>() as WIPICWord)?;
    write_generic(context, context.data_ptr(memory)?, framebuffer.0)?;

    Ok(memory)
}

pub async fn destroy_offscreen_framebuffer(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<()> {
    tracing::debug!("MC_grpDestroyOffScreenFrameBuffer({:#x})", framebuffer.0);

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

pub async fn get_font(_: &mut dyn WIPICContext, face: i32, size: i32, style: i32) -> Result<i32> {
    tracing::warn!("stub MC_grpGetFont({face}, {size}, {style})");

    Ok(0)
}

pub async fn get_font_height(_: &mut dyn WIPICContext, font: i32) -> Result<i32> {
    tracing::warn!("stub MC_grpGetFontHeight({font})");

    Ok(12)
}

pub async fn get_font_ascent(_: &mut dyn WIPICContext, font: i32) -> Result<i32> {
    tracing::warn!("stub MC_grpGetFontAscent({font})");

    Ok(10)
}

pub async fn get_font_descent(_: &mut dyn WIPICContext, font: i32) -> Result<i32> {
    tracing::warn!("stub MC_grpGetFontDescent({font})");

    Ok(2)
}

pub async fn get_string_width(context: &mut dyn WIPICContext, font: i32, ptr_string: WIPICWord, length: i32) -> Result<i32> {
    tracing::debug!("MC_grpGetStringWidth({font}, {ptr_string:#x}, {length})");

    let string = read_wipi_string(context, ptr_string, length)?;

    Ok(string_width(&string, 10.0) as i32)
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

    let mut canvas = framebuffer.canvas(context)?;
    let color = framebuffer.pixel_to_color(gctx.fgpxl);
    canvas.draw_text(&string, x, y, TextAlignment::Left, color, clip);
    canvas.flush()?;

    Ok(())
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

    let color = framebuffer.pixel_to_color(gctx.fgpxl);
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

    let color = framebuffer.pixel_to_color(gctx.fgpxl);
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
pub async fn get_framebuffer_pointer(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<WIPICWord> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_POINTER({:#x})", framebuffer.0);

    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(framebuffer)?)?;

    Ok(framebuffer.buf.0)
}

pub async fn get_framebuffer_width(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_WIDTH({:#x})", framebuffer.0);

    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(framebuffer)?)?;

    Ok(framebuffer.width as _)
}

pub async fn get_framebuffer_height(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_HEIGHT({:#x})", framebuffer.0);

    let framebuffer: WIPICFramebuffer = read_generic(context, context.data_ptr(framebuffer)?)?;

    Ok(framebuffer.height as _)
}

pub async fn get_framebuffer_bpl(context: &mut dyn WIPICContext, framebuffer: WIPICIndirectPtr) -> Result<i32> {
    tracing::debug!("MC_GRP_GET_FRAME_BUFFER_BPL({:#x})", framebuffer.0);

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
    use super::destination_stride;

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
