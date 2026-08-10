use alloc::vec::Vec;
use alloc::vec;

use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::javax::microedition::lcdui::{Font as MidpFont, Graphics as MidpGraphics, Image as MidpImage};

use crate::classes::org::kwis::msp::lcdui::{Display, Font, Image};

// class org.kwis.msp.lcdui.Graphics
pub struct Graphics;

#[allow(clippy::too_many_arguments)]
impl Graphics {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/Graphics",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "(Lorg/kwis/msp/lcdui/Display;)V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Ljavax/microedition/lcdui/Graphics;)V",
                    Self::init_with_midp_graphics,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Image;)V",
                    Self::init_with_image,
                    Default::default(),
                ),
                JavaMethodProto::new("getFont", "()Lorg/kwis/msp/lcdui/Font;", Self::get_font, Default::default()),
                JavaMethodProto::new("copyArea", "(IIIIII)V", Self::copy_area, Default::default()),
                JavaMethodProto::new("setColor", "(I)V", Self::set_color, Default::default()),
                JavaMethodProto::new("setColor", "(III)V", Self::set_color_by_rgb, Default::default()),
                JavaMethodProto::new("setFont", "(Lorg/kwis/msp/lcdui/Font;)V", Self::set_font, Default::default()),
                JavaMethodProto::new("setAlpha", "(I)V", Self::set_alpha, Default::default()),
                JavaMethodProto::new("fillRect", "(IIII)V", Self::fill_rect, Default::default()),
                JavaMethodProto::new("fillRoundRect", "(IIIIII)V", Self::fill_round_rect, Default::default()),
                JavaMethodProto::new("fillArc", "(IIIIII)V", Self::fill_arc, Default::default()),
                JavaMethodProto::new("fillPolygon", "([I[I)V", Self::fill_polygon, Default::default()),
                JavaMethodProto::new("drawLine", "(IIII)V", Self::draw_line, Default::default()),
                JavaMethodProto::new("drawRect", "(IIII)V", Self::draw_rect, Default::default()),
                JavaMethodProto::new("drawRoundRect", "(IIIIII)V", Self::draw_round_rect, Default::default()),
                JavaMethodProto::new("drawArc", "(IIIIII)V", Self::draw_arc, Default::default()),
                JavaMethodProto::new("drawPolygon", "([I[I)V", Self::draw_polygon, Default::default()),
                JavaMethodProto::new("drawChar", "(CIII)V", Self::draw_char, Default::default()),
                JavaMethodProto::new("drawChars", "([CIIIII)V", Self::draw_chars, Default::default()),
                JavaMethodProto::new("drawString", "(Ljava/lang/String;III)V", Self::draw_string, Default::default()),
                JavaMethodProto::new("drawSubstring", "(Ljava/lang/String;IIIII)V", Self::draw_substring, Default::default()),
                JavaMethodProto::new("drawImage", "(Lorg/kwis/msp/lcdui/Image;III)V", Self::draw_image, Default::default()),
                JavaMethodProto::new("setClip", "(IIII)V", Self::set_clip, Default::default()),
                JavaMethodProto::new("clipRect", "(IIII)V", Self::clip_rect, Default::default()),
                JavaMethodProto::new("getColor", "()I", Self::get_color, Default::default()),
                JavaMethodProto::new("getBlueComponent", "()I", Self::get_blue_component, Default::default()),
                JavaMethodProto::new("getGrayScale", "()I", Self::get_gray_scale, Default::default()),
                JavaMethodProto::new("getGreenComponent", "()I", Self::get_green_component, Default::default()),
                JavaMethodProto::new("getRedComponent", "()I", Self::get_red_component, Default::default()),
                JavaMethodProto::new("getStrokeStyle", "()I", Self::get_stroke_style, Default::default()),
                JavaMethodProto::new("setStrokeStyle", "(I)V", Self::set_stroke_style, Default::default()),
                JavaMethodProto::new("getClipX", "()I", Self::get_clip_x, Default::default()),
                JavaMethodProto::new("getClipY", "()I", Self::get_clip_y, Default::default()),
                JavaMethodProto::new("getClipWidth", "()I", Self::get_clip_width, Default::default()),
                JavaMethodProto::new("getClipHeight", "()I", Self::get_clip_height, Default::default()),
                JavaMethodProto::new("getTranslateX", "()I", Self::get_translate_x, Default::default()),
                JavaMethodProto::new("getTranslateY", "()I", Self::get_translate_y, Default::default()),
                JavaMethodProto::new("translate", "(II)V", Self::translate, Default::default()),
                JavaMethodProto::new("setPixel", "(II)V", Self::set_pixel, Default::default()),
                JavaMethodProto::new("setRGBPixels", "(IIII[III)V", Self::set_rgb_pixels, Default::default()),
                JavaMethodProto::new("setGrayScale", "(I)V", Self::set_gray_scale, Default::default()),
                JavaMethodProto::new("setXORMode", "(Z)V", Self::set_xor_mode, Default::default()),
                JavaMethodProto::new("getPixel", "(II)I", Self::get_pixel, Default::default()),
                JavaMethodProto::new("getPixels", "(IIII[BII)V", Self::get_pixels, Default::default()),
                JavaMethodProto::new("setPixels", "(IIII[BII)V", Self::set_pixels, Default::default()),
                JavaMethodProto::new("reset", "()V", Self::reset, Default::default()),
                JavaMethodProto::new("getAlpha", "()I", Self::get_alpha, Default::default()),
                JavaMethodProto::new("isXORMode", "()Z", Self::is_xor_mode, Default::default()),
                JavaMethodProto::new("encodeImage", "(IIII)[B", Self::encode_image, Default::default()),
                JavaMethodProto::new("getRGBPixels", "(IIII[III)V", Self::get_rgb_pixels, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("midpGraphics", "Ljavax/microedition/lcdui/Graphics;", Default::default()),
                JavaFieldProto::new("baseTranslateX", "I", Default::default()),
                JavaFieldProto::new("baseTranslateY", "I", Default::default()),
                JavaFieldProto::new("alpha", "I", Default::default()),
                JavaFieldProto::new("strokeStyle", "I", Default::default()),
                JavaFieldProto::new("xorMode", "Z", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, display: ClassInstanceRef<Display>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::<init>({this:?})");

        let midp_display = Display::midp_display(jvm, &display).await?;
        let midp_graphics = jvm
            .new_class(
                "javax/microedition/lcdui/Graphics",
                "(Ljavax/microedition/lcdui/Display;)V",
                (midp_display,),
            )
            .await?;

        let base_translate_x: i32 = jvm
            .invoke_virtual(&midp_graphics, "getTranslateX", "()I", ())
            .await?;
        let base_translate_y: i32 = jvm
            .invoke_virtual(&midp_graphics, "getTranslateY", "()I", ())
            .await?;

        jvm.put_field(&mut this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;", midp_graphics)
            .await?;
        jvm.put_field(&mut this, "baseTranslateX", "I", base_translate_x).await?;
        jvm.put_field(&mut this, "baseTranslateY", "I", base_translate_y).await?;
        jvm.put_field(&mut this, "alpha", "I", 255).await?;
        jvm.put_field(&mut this, "strokeStyle", "I", 0).await?;
        jvm.put_field(&mut this, "xorMode", "Z", false).await?;

        Ok(())
    }

    async fn init_with_midp_graphics(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        midp_graphics: ClassInstanceRef<MidpGraphics>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::<init>({this:?})");

        let base_translate_x: i32 = jvm
            .invoke_virtual(&midp_graphics, "getTranslateX", "()I", ())
            .await?;
        let base_translate_y: i32 = jvm
            .invoke_virtual(&midp_graphics, "getTranslateY", "()I", ())
            .await?;

        jvm.put_field(&mut this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;", midp_graphics)
            .await?;
        jvm.put_field(&mut this, "baseTranslateX", "I", base_translate_x).await?;
        jvm.put_field(&mut this, "baseTranslateY", "I", base_translate_y).await?;
        jvm.put_field(&mut this, "alpha", "I", 255).await?;
        jvm.put_field(&mut this, "strokeStyle", "I", 0).await?;
        jvm.put_field(&mut this, "xorMode", "Z", false).await?;

        Ok(())
    }

    async fn init_with_image(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::<init>({this:?}, {image:?})");

        if image.is_null() {
            return Err(jvm
                .exception("java/lang/NullPointerException", "image is null")
                .await);
        }

        let midp_image = Image::midp_image(jvm, &image).await?;
        let midp_graphics: ClassInstanceRef<MidpGraphics> = jvm
            .invoke_virtual(
                &midp_image,
                "getGraphics",
                "()Ljavax/microedition/lcdui/Graphics;",
                (),
            )
            .await?;

        let base_translate_x: i32 = jvm
            .invoke_virtual(&midp_graphics, "getTranslateX", "()I", ())
            .await?;
        let base_translate_y: i32 = jvm
            .invoke_virtual(&midp_graphics, "getTranslateY", "()I", ())
            .await?;

        jvm.put_field(
            &mut this,
            "midpGraphics",
            "Ljavax/microedition/lcdui/Graphics;",
            midp_graphics,
        )
        .await?;
        jvm.put_field(&mut this, "baseTranslateX", "I", base_translate_x)
            .await?;
        jvm.put_field(&mut this, "baseTranslateY", "I", base_translate_y)
            .await?;
        jvm.put_field(&mut this, "alpha", "I", 255).await?;
        jvm.put_field(&mut this, "strokeStyle", "I", 0).await?;
        jvm.put_field(&mut this, "xorMode", "Z", false).await?;

        let _: () = jvm.invoke_virtual(&this, "reset", "()V", ()).await?;

        Ok(())
    }

    async fn get_font(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Font>> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getFont({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let midp_font: ClassInstanceRef<MidpFont> = jvm
            .invoke_virtual(&midp_graphics, "getFont", "()Ljavax/microedition/lcdui/Font;", ())
            .await?;

        Ok(jvm
            .new_class("org/kwis/msp/lcdui/Font", "(Ljavax/microedition/lcdui/Font;)V", (midp_font,))
            .await?
            .into())
    }

    async fn copy_area(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        dx: i32,
        dy: i32,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::copyArea({this:?}, {dx}, {dy}, {sx}, {sy}, {w}, {h})");

        if w <= 0 || h <= 0 {
            return Ok(());
        }

        let mut midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let image = MidpGraphics::image(jvm, &mut midp_graphics).await?;
        let mut canvas = MidpImage::canvas(jvm, &image).await?;

        let translate_x: i32 = jvm.invoke_virtual(&midp_graphics, "getTranslateX", "()I", ()).await?;
        let translate_y: i32 = jvm.invoke_virtual(&midp_graphics, "getTranslateY", "()I", ()).await?;
        let clip = MidpGraphics::clip(jvm, &midp_graphics).await?;

        canvas.copy_area(
            translate_x + dx,
            translate_y + dy,
            translate_x + sx,
            translate_y + sy,
            w as _,
            h as _,
            clip,
        );
        Ok(())
    }

    async fn set_color(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, color: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setColor({this:?}, {color})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "setColor", "(I)V", (color,)).await
    }

    async fn set_color_by_rgb(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, r: i32, g: i32, b: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setColor({this:?}, {r}, {g}, {b})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "setColor", "(III)V", (r, g, b)).await
    }

    async fn set_font(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, font: ClassInstanceRef<Font>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setFont({this:?}, {font:?})");

        // Native Graphics.setFont(null) substitutes Font.getDefaultFont()
        // before applying the font to the native graphics state.
        let font = if font.is_null() {
            jvm.invoke_static(
                "org/kwis/msp/lcdui/Font",
                "getDefaultFont",
                "()Lorg/kwis/msp/lcdui/Font;",
                (),
            )
            .await?
        } else {
            font
        };

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let midp_font = Font::midp_font(jvm, &font).await?;

        jvm.invoke_virtual(&midp_graphics, "setFont", "(Ljavax/microedition/lcdui/Font;)V", (midp_font,))
            .await
    }

    async fn set_alpha(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, alpha: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setAlpha({this:?}, {alpha})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let _: () = jvm.invoke_virtual(&midp_graphics, "setXORMode", "(Z)V", (false,)).await?;
        jvm.put_field(&mut this, "alpha", "I", if alpha == 0 { 0 } else { 255 }).await?;
        jvm.put_field(&mut this, "xorMode", "Z", false).await?;

        Ok(())
    }

    async fn fill_rect(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::fillRect({this:?}, {x}, {y}, {width}, {height})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "fillRect", "(IIII)V", (x, y, width, height)).await
    }

    async fn fill_round_rect(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        arc_width: i32,
        arc_height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::fillRoundRect({this:?}, {x}, {y}, {width}, {height}, {arc_width}, {arc_height})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "fillRoundRect", "(IIIIII)V", (x, y, width, height, arc_width, arc_height))
            .await
    }

    async fn fill_arc(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        start_angle: i32,
        arc_angle: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::fillArc({this:?}, {x}, {y}, {width}, {height}, {start_angle}, {arc_angle})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "fillArc", "(IIIIII)V", (x, y, width, height, start_angle, arc_angle))
            .await
    }

    async fn fill_polygon(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x_points: ClassInstanceRef<Array<i32>>,
        y_points: ClassInstanceRef<Array<i32>>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::fillPolygon({this:?}, {x_points:?}, {y_points:?})");

        if x_points.is_null() {
            return Err(jvm
                .exception("java/lang/NullPointerException", "x is null.")
                .await);
        }
        if y_points.is_null() {
            return Err(jvm
                .exception("java/lang/NullPointerException", "y is null.")
                .await);
        }

        let x_len = jvm.array_length(&x_points).await?;
        let y_len = jvm.array_length(&y_points).await?;
        if x_len != y_len {
            return Err(jvm
                .exception(
                    "java/lang/IllegalArgumentException",
                    "x.length != y.length",
                )
                .await);
        }

        if x_len == 0 {
            return Ok(());
        }

        let xs: Vec<i32> = jvm.load_array(&x_points, 0, x_len).await?;
        let ys: Vec<i32> = jvm.load_array(&y_points, 0, y_len).await?;

        let midp_graphics =
            jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;")
                .await?;

        // Native dgraphics_fill_polygon() first draws the closed outline.
        for i in 1..x_len {
            let _: () = jvm
                .invoke_virtual(
                    &midp_graphics,
                    "drawLine",
                    "(IIII)V",
                    (xs[i - 1], ys[i - 1], xs[i], ys[i]),
                )
                .await?;
        }

        let last = x_len - 1;
        let _: () = jvm
            .invoke_virtual(
                &midp_graphics,
                "drawLine",
                "(IIII)V",
                (xs[last], ys[last], xs[0], ys[0]),
            )
            .await?;

        if x_len < 3 {
            return Ok(());
        }

        let min_y = *ys.iter().min().unwrap();
        let max_y = *ys.iter().max().unwrap();

        // Native uses a fixed-point active-edge scanline fill.
        // Use 12.12 fixed point here as well; 4096 is the scale visible
        // in dgraphics_fill_polygon.
        const FP_SHIFT: i64 = 12;
        const FP_ONE: i64 = 1 << FP_SHIFT;

        for scan_y in min_y..=max_y {
            let mut intersections: Vec<i64> = Vec::new();

            for i in 0..x_len {
                let j = if i + 1 == x_len { 0 } else { i + 1 };

                let mut x1 = xs[i] as i64;
                let mut y1 = ys[i] as i64;
                let mut x2 = xs[j] as i64;
                let mut y2 = ys[j] as i64;

                if y1 == y2 {
                    continue;
                }

                if y1 > y2 {
                    core::mem::swap(&mut x1, &mut x2);
                    core::mem::swap(&mut y1, &mut y2);
                }

                let y = scan_y as i64;

                // Half-open edge interval prevents a shared vertex from
                // contributing twice to the same scanline.
                if y < y1 || y >= y2 {
                    continue;
                }

                let dy = y2 - y1;
                let dx = x2 - x1;
                let x_fp = x1 * FP_ONE + (y - y1) * dx * FP_ONE / dy;
                intersections.push(x_fp);
            }

            intersections.sort_unstable();

            for pair in intersections.chunks_exact(2) {
                // Match integer raster semantics: left edge rounds upward,
                // right edge rounds downward.
                let left = ((pair[0] + FP_ONE - 1) >> FP_SHIFT) as i32;
                let right = (pair[1] >> FP_SHIFT) as i32;

                if left <= right {
                    let _: () = jvm
                        .invoke_virtual(
                            &midp_graphics,
                            "drawLine",
                            "(IIII)V",
                            (left, scan_y, right, scan_y),
                        )
                        .await?;
                }
            }
        }

        Ok(())
    }

    async fn draw_line(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, x1: i32, y1: i32, x2: i32, y2: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawLine({this:?}, {x1}, {y1}, {x2}, {y2})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawLine", "(IIII)V", (x1, y1, x2, y2)).await
    }

    async fn draw_rect(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawRect({this:?}, {x}, {y}, {width}, {height})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawRect", "(IIII)V", (x, y, width, height)).await
    }

    async fn draw_round_rect(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        arc_width: i32,
        arc_height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawRoundRect({this:?}, {x}, {y}, {width}, {height}, {arc_width}, {arc_height})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawRoundRect", "(IIIIII)V", (x, y, width, height, arc_width, arc_height))
            .await
    }

    async fn draw_arc(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        start_angle: i32,
        arc_angle: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawArc({this:?}, {x}, {y}, {width}, {height}, {start_angle}, {arc_angle})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawArc", "(IIIIII)V", (x, y, width, height, start_angle, arc_angle))
            .await
    }

    async fn draw_polygon(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x_points: ClassInstanceRef<Array<i32>>,
        y_points: ClassInstanceRef<Array<i32>>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawPolygon({this:?}, {x_points:?}, {y_points:?})");

        if x_points.is_null() {
            return Err(jvm
                .exception("java/lang/NullPointerException", "x is null.")
                .await);
        }
        if y_points.is_null() {
            return Err(jvm
                .exception("java/lang/NullPointerException", "y is null.")
                .await);
        }

        let x_len = jvm.array_length(&x_points).await?;
        let y_len = jvm.array_length(&y_points).await?;
        if x_len != y_len {
            return Err(jvm
                .exception(
                    "java/lang/IllegalArgumentException",
                    "x.length != y.length",
                )
                .await);
        }

        // The native implementation ultimately performs:
        //   point[i] -> point[i + 1]
        // and then closes the polygon with last -> first.
        //
        // Avoid dereferencing an empty Java array here. The original native
        // routine has no explicit zero-length guard before accessing point 0.
        if x_len == 0 {
            return Ok(());
        }

        let xs: Vec<i32> = jvm.load_array(&x_points, 0, x_len).await?;
        let ys: Vec<i32> = jvm.load_array(&y_points, 0, y_len).await?;

        let midp_graphics =
            jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;")
                .await?;

        for i in 1..x_len {
            let _: () = jvm
                .invoke_virtual(
                    &midp_graphics,
                    "drawLine",
                    "(IIII)V",
                    (xs[i - 1], ys[i - 1], xs[i], ys[i]),
                )
                .await?;
        }

        let last = x_len - 1;
        let _: () = jvm
            .invoke_virtual(
                &midp_graphics,
                "drawLine",
                "(IIII)V",
                (xs[last], ys[last], xs[0], ys[0]),
            )
            .await?;

        Ok(())
    }

    async fn draw_char(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        ch: JavaChar,
        x: i32,
        y: i32,
        anchor: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawChar({this:?}, {ch:?}, {x}, {y}, {anchor})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawChar", "(CIII)V", (ch, x, y, anchor)).await
    }

    async fn draw_chars(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        chars: ClassInstanceRef<Array<JavaChar>>,
        offset: i32,
        length: i32,
        x: i32,
        y: i32,
        anchor: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawChars({this:?}, {chars:?}, {offset}, {length}, {x}, {y}, {anchor})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawChars", "([CIIIII)V", (chars, offset, length, x, y, anchor))
            .await
    }

    async fn draw_string(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        string: ClassInstanceRef<String>,
        x: i32,
        y: i32,
        anchor: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawString({this:?}, {string:?}, {x}, {y}, {anchor})");

        // Some legacy WIPI applications use null for an absent optional label.
        // Treat it as an empty draw request rather than forwarding null to MIDP.
        if string.is_null() {
            return Ok(());
        }

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;

        jvm.invoke_virtual(&midp_graphics, "drawString", "(Ljava/lang/String;III)V", (string, x, y, anchor))
            .await
    }

    async fn draw_substring(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        string: ClassInstanceRef<String>,
        offset: i32,
        len: i32,
        x: i32,
        y: i32,
        anchor: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawSubstring({this:?}, {string:?}, {offset}, {len}, {x}, {y}, {anchor})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;

        jvm.invoke_virtual(
            &midp_graphics,
            "drawSubstring",
            "(Ljava/lang/String;IIIII)V",
            (string, offset, len, x, y, anchor),
        )
        .await
    }

    async fn draw_image(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        image: ClassInstanceRef<Image>,
        x: i32,
        y: i32,
        anchor: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::drawImage({this:?}, {image:?}, {x}, {y}, {anchor})");

        if image.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "image is null").await);
        }

        let midp_graphics =
            jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let midp_image = Image::midp_image(jvm, &image).await?;
        let transparent_color: i32 =
            jvm.get_field(&image, "transparentColor", "I").await?;

        // -1 is the native "no transparent color" sentinel.
        // Keep the ordinary path unchanged when color-keying is disabled.
        if transparent_color == -1 {
            return jvm
                .invoke_virtual(
                    &midp_graphics,
                    "drawImage",
                    "(Ljavax/microedition/lcdui/Image;III)V",
                    (midp_image, x, y, anchor),
                )
                .await;
        }

        let backend_image = MidpImage::image(jvm, &midp_image).await?;
        let width = backend_image.width() as i32;
        let height = backend_image.height() as i32;

        // MIDP/WIPI image anchors use the same bit values:
        // HCENTER=1, VCENTER=2, LEFT=4, RIGHT=8, TOP=16, BOTTOM=32.
        let draw_x = if anchor & 1 != 0 {
            x - width / 2
        } else if anchor & 8 != 0 {
            x - width
        } else {
            x
        };

        let draw_y = if anchor & 2 != 0 {
            y - height / 2
        } else if anchor & 32 != 0 {
            y - height
        } else {
            y
        };

        let key = transparent_color as u32;
        let key565 =
            ((key >> 8) & 0xf800) |
            ((key >> 5) & 0x07e0) |
            ((key >> 3) & 0x001f);

        let pixel_count = match (width as usize).checked_mul(height as usize) {
            Some(value) => value,
            None => return Ok(()),
        };
        let mut rgb = Vec::with_capacity(pixel_count);

        for source_y in 0..height {
            for source_x in 0..width {
                let pixel = backend_image.get_pixel(source_x, source_y);

                let source565 =
                    (((pixel.r as u32) << 8) & 0xf800) |
                    (((pixel.g as u32) << 3) & 0x07e0) |
                    (((pixel.b as u32) >> 3) & 0x001f);

                let alpha = if source565 == key565 {
                    0
                } else {
                    pixel.a as u32
                };

                rgb.push(
                    ((alpha << 24)
                        | ((pixel.r as u32) << 16)
                        | ((pixel.g as u32) << 8)
                        | pixel.b as u32) as i32,
                );
            }
        }

        let mut rgb_array = jvm.instantiate_array("I", rgb.len()).await?;
        jvm.store_array(&mut rgb_array, 0, rgb).await?;

        // drawRGB applies the existing MIDP translation, clipping and XOR path.
        let _: () = jvm
            .invoke_virtual(
                &midp_graphics,
                "drawRGB",
                "([IIIIIIIZ)V",
                (
                    rgb_array,
                    0,
                    width,
                    draw_x,
                    draw_y,
                    width,
                    height,
                    true,
                ),
            )
            .await?;

        Ok(())
    }

    async fn set_clip(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setClip({this:?}, {x}, {y}, {width}, {height})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "setClip", "(IIII)V", (x, y, width, height)).await
    }

    async fn clip_rect(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::clipRect({this:?}, {x}, {y}, {width}, {height})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "clipRect", "(IIII)V", (x, y, width, height)).await
    }

    async fn get_color(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getColor({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getColor", "()I", ()).await
    }

    async fn get_blue_component(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getBlueComponent({this:?})");

        let color: i32 = jvm.invoke_virtual(&this, "getColor", "()I", ()).await?;
        Ok(color & 0xff)
    }

    async fn get_gray_scale(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getGrayScale({this:?})");

        let color: i32 = jvm.invoke_virtual(&this, "getColor", "()I", ()).await?;
        Ok((((color >> 16) & 0xff) + ((color >> 8) & 0xff) + (color & 0xff)) / 3)
    }

    async fn get_green_component(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getGreenComponent({this:?})");

        let color: i32 = jvm.invoke_virtual(&this, "getColor", "()I", ()).await?;
        Ok((color >> 8) & 0xff)
    }

    async fn get_red_component(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getRedComponent({this:?})");

        let color: i32 = jvm.invoke_virtual(&this, "getColor", "()I", ()).await?;
        Ok((color >> 16) & 0xff)
    }

    async fn get_stroke_style(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getStrokeStyle({this:?})");

        jvm.get_field(&this, "strokeStyle", "I").await
    }

    async fn set_stroke_style(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, style: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setStrokeStyle({this:?}, {style})");

        if style != 0 && style != 1 {
            return Err(jvm.exception("java/lang/IllegalArgumentException", "invalid stroke style").await);
        }

        jvm.put_field(&mut this, "strokeStyle", "I", style).await
    }

    async fn get_clip_x(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getClipX({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getClipX", "()I", ()).await
    }

    async fn get_clip_y(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getClipY({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getClipY", "()I", ()).await
    }

    async fn get_clip_width(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getClipWidth({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getClipWidth", "()I", ()).await
    }

    async fn get_clip_height(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getClipHeight({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getClipHeight", "()I", ()).await
    }

    async fn get_translate_x(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getTranslateX({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getTranslateX", "()I", ()).await
    }

    async fn get_translate_y(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getTranslateY({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "getTranslateY", "()I", ()).await
    }

    async fn translate(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, x: i32, y: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::translate({this:?}, {x}, {y})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "translate", "(II)V", (x, y)).await
    }

    async fn set_pixel(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, x: i32, y: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setPixel({this:?}, {x}, {y})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "drawLine", "(IIII)V", (x, y, x, y)).await
    }

    async fn set_rgb_pixels(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        rgb_pixels: ClassInstanceRef<Array<i32>>,
        offset: i32,
        bpl: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setRGBPixels({this:?}, {x}, {y}, {width}, {height}, {rgb_pixels:?}, {offset}, {bpl})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;

        jvm.invoke_virtual(
            &midp_graphics,
            "drawRGB",
            "([IIIIIIIZ)V",
            (rgb_pixels, offset, bpl, x, y, width, height, true),
        )
        .await
    }

    async fn set_gray_scale(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, value: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::SetGrayScale({this:?}, {value})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        jvm.invoke_virtual(&midp_graphics, "setGrayScale", "(I)V", (value,)).await
    }

    async fn set_xor_mode(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, xor_mode: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setXORMode({this:?}, {xor_mode})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let _: () = jvm.invoke_virtual(&midp_graphics, "setXORMode", "(Z)V", (xor_mode,)).await?;
        jvm.put_field(&mut this, "xorMode", "Z", xor_mode).await
    }

    async fn get_pixel(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, x: i32, y: i32) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getPixel({this:?}, {x}, {y})");

        let mut midp_graphics: ClassInstanceRef<MidpGraphics> =
            jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;")
                .await?;

        let width: i32 = jvm.get_field(&midp_graphics, "width", "I").await?;
        let height: i32 = jvm.get_field(&midp_graphics, "height", "I").await?;

        // Native dgraphics_get_pixel() returns M_E_OUTOFBOUND (-2022)
        // rather than throwing when the logical coordinate is outside
        // the Graphics region.
        if x < 0 || y < 0 || x >= width || y >= height {
            return Ok(-2022);
        }

        // Native dgraphics_get_pixel() addresses pixels from the Graphics
        // base origin (+0x3c/+0x40), not the current translated origin
        // (+0x34/+0x38).
        let base_translate_x: i32 = jvm.get_field(&this, "baseTranslateX", "I").await?;
        let base_translate_y: i32 = jvm.get_field(&this, "baseTranslateY", "I").await?;

        let image = MidpGraphics::image(jvm, &mut midp_graphics).await?;
        let backend_image = MidpImage::image(jvm, &image).await?;

        let absolute_x = x + base_translate_x;
        let absolute_y = y + base_translate_y;

        // Avoid an out-of-bounds backend access. Native Graphics objects
        // normally keep their translated origin inside the backing image.
        if absolute_x < 0
            || absolute_y < 0
            || absolute_x as u32 >= backend_image.width()
            || absolute_y as u32 >= backend_image.height()
        {
            return Ok(-2022);
        }

        let pixel = backend_image.get_pixel(absolute_x, absolute_y);

        // The reference implementation reads RGB565 and expands it without
        // bit replication:
        //   R5 << 19, G6 << 10, B5 << 3
        // This is equivalent to masking an 8-bit backend color to
        // RRRR_R000 / GGGG_GG00 / BBBB_B000.
        let r = (pixel.r & 0xf8) as i32;
        let g = (pixel.g & 0xfc) as i32;
        let b = (pixel.b & 0xf8) as i32;

        Ok((r << 16) | (g << 8) | b)
    }

    async fn get_pixels(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        mut pixels: ClassInstanceRef<Array<i8>>,
        offset: i32,
        bytes_per_line: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getPixels({this:?}, {x}, {y}, {width}, {height}, {pixels:?}, {offset}, {bytes_per_line})");

        if pixels.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "pixels is null.")
                    .await,
            );
        }

        let array_length = jvm.array_length(&pixels).await? as i32;
        let required_length = height.wrapping_mul(bytes_per_line);

        if array_length < required_length {
            return Err(
                jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                    .await,
            );
        }

        if width <= 0 || height <= 0 {
            return Ok(());
        }

        let mut midp_graphics: ClassInstanceRef<MidpGraphics> =
            jvm.get_field(
                &this,
                "midpGraphics",
                "Ljavax/microedition/lcdui/Graphics;",
            )
            .await?;

        let graphics_width: i32 = jvm.get_field(&midp_graphics, "width", "I").await?;
        let graphics_height: i32 = jvm.get_field(&midp_graphics, "height", "I").await?;

        let left = x.max(0);
        let top = y.max(0);
        let right = x.saturating_add(width).min(graphics_width);
        let bottom = y.saturating_add(height).min(graphics_height);

        // Native get_raw_data() returns M_E_OUTOFBOUND when there is no
        // intersection, but the Java bridge ignores that return value.
        if left >= right || top >= bottom {
            return Ok(());
        }

        let base_translate_x: i32 =
            jvm.get_field(&this, "baseTranslateX", "I").await?;
        let base_translate_y: i32 =
            jvm.get_field(&this, "baseTranslateY", "I").await?;

        let image = MidpGraphics::image(jvm, &mut midp_graphics).await?;
        let backend_image = MidpImage::image(jvm, &image).await?;

        let row_stride = match width.checked_mul(2) {
            Some(value) if value >= 0 => value as usize,
            _ => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        let copied_width = (right - left) as usize;
        let copied_height = (bottom - top) as usize;
        let copied_row_bytes = copied_width * 2;

        let destination_offset = match offset.checked_mul(4) {
            Some(value) if value >= 0 => value as usize,
            _ => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        // Native writes each clipped row at the beginning of the requested
        // destination row and advances by requested_width * 2 bytes.
        let touched_bytes = match copied_height
            .checked_sub(1)
            .and_then(|rows| rows.checked_mul(row_stride))
            .and_then(|prefix| prefix.checked_add(copied_row_bytes))
        {
            Some(value) => value,
            None => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        if destination_offset
            .checked_add(touched_bytes)
            .is_none_or(|end| end > array_length as usize)
        {
            return Err(
                jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                    .await,
            );
        }

        let mut data: Vec<i8> =
            jvm.load_array(&pixels, destination_offset, touched_bytes).await?;

        for row in 0..copied_height {
            let source_y = base_translate_y + top + row as i32;
            let destination_row = row * row_stride;

            for column in 0..copied_width {
                let source_x = base_translate_x + left + column as i32;

                if source_x < 0
                    || source_y < 0
                    || source_x as u32 >= backend_image.width()
                    || source_y as u32 >= backend_image.height()
                {
                    continue;
                }

                let pixel = backend_image.get_pixel(source_x, source_y);

                let r5 = (pixel.r as u16) >> 3;
                let g6 = (pixel.g as u16) >> 2;
                let b5 = (pixel.b as u16) >> 3;
                let raw = (r5 << 11) | (g6 << 5) | b5;

                let destination = destination_row + column * 2;
                data[destination] = (raw & 0xff) as i8;
                data[destination + 1] = (raw >> 8) as i8;
            }
        }

        jvm.store_array(&mut pixels, destination_offset, data).await?;

        Ok(())
    }

    async fn set_pixels(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        pixels: ClassInstanceRef<Array<i8>>,
        offset: i32,
        bytes_per_line: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::setPixels({this:?}, {x}, {y}, {width}, {height}, {pixels:?}, {offset}, {bytes_per_line})");

        if pixels.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "pixels is null.")
                    .await,
            );
        }

        let array_length = jvm.array_length(&pixels).await? as i32;
        let required_length = height.wrapping_mul(bytes_per_line);

        if array_length < required_length {
            return Err(
                jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                    .await,
            );
        }

        if width <= 0 || height <= 0 {
            return Ok(());
        }

        let pixel_count = match width.checked_mul(height) {
            Some(value) if value >= 0 => value as usize,
            _ => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        let source_bytes = match pixel_count.checked_mul(2) {
            Some(value) => value,
            None => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        // The 32-bit WIPI bridge treats the Java byte[] offset as a
        // four-byte-scaled native offset.
        let source_offset = match offset.checked_mul(4) {
            Some(value) if value >= 0 => value as usize,
            _ => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        // The reference implementation does not perform this second bounds
        // check and may access outside the Java array. Preserve normal native
        // semantics while keeping the Rust implementation memory-safe.
        if source_offset
            .checked_add(source_bytes)
            .is_none_or(|end| end > array_length as usize)
        {
            return Err(
                jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                    .await,
            );
        }

        let raw: Vec<i8> = jvm
            .load_array(&pixels, source_offset, source_bytes)
            .await?;

        let mut rgb = Vec::with_capacity(pixel_count);

        for pair in raw.chunks_exact(2) {
            let value = (pair[0] as u8 as u16) | ((pair[1] as u8 as u16) << 8);

            let r5 = ((value >> 11) & 0x1f) as u32;
            let g6 = ((value >> 5) & 0x3f) as u32;
            let b5 = (value & 0x1f) as u32;

            // Expand RGB565 to ARGB8888. The backend will quantize again when
            // the destination image itself uses a 16-bit representation.
            let r = (r5 << 3) | (r5 >> 2);
            let g = (g6 << 2) | (g6 >> 4);
            let b = (b5 << 3) | (b5 >> 2);

            rgb.push((0xff00_0000u32 | (r << 16) | (g << 8) | b) as i32);
        }

        let mut rgb_array = jvm.instantiate_array("I", pixel_count).await?;
        jvm.store_array(&mut rgb_array, 0, rgb).await?;

        // MIDP drawRGB already applies the current translation, clipping and
        // XOR state, matching dgraphics_draw_raw_data().
        let _: () = jvm
            .invoke_virtual(
                &this,
                "setRGBPixels",
                "(IIII[III)V",
                (x, y, width, height, rgb_array, 0, width),
            )
            .await?;

        Ok(())
    }

    async fn reset(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::reset({this:?})");

        let midp_graphics = jvm.get_field(&this, "midpGraphics", "Ljavax/microedition/lcdui/Graphics;").await?;
        let _: () = jvm.invoke_virtual(&midp_graphics, "reset", "()V", ()).await?;
        jvm.put_field(&mut this, "alpha", "I", 255).await?;
        jvm.put_field(&mut this, "strokeStyle", "I", 0).await?;
        jvm.put_field(&mut this, "xorMode", "Z", false).await?;

        // Native Graphics.reset() performs reset0(), then restores
        // Font.getDefaultFont() through this.setFont(defaultFont).
        let default_font: ClassInstanceRef<Font> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Font",
                "getDefaultFont",
                "()Lorg/kwis/msp/lcdui/Font;",
                (),
            )
            .await?;
        let _: () = jvm
            .invoke_virtual(
                &this,
                "setFont",
                "(Lorg/kwis/msp/lcdui/Font;)V",
                (default_font,),
            )
            .await?;

        Ok(())
    }

    async fn get_alpha(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getAlpha({this:?})");

        jvm.get_field(&this, "alpha", "I").await
    }

    async fn is_xor_mode(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::isXORMode({this:?})");

        jvm.get_field(&this, "xorMode", "Z").await
    }

    async fn encode_image(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<ClassInstanceRef<Array<u8>>> {
        tracing::debug!(
            "org.kwis.msp.lcdui.Graphics::encodeImage({this:?}, {x}, {y}, {width}, {height})"
        );

        // The native Java bridge allocates a 24-bpp BMP-sized byte array
        // from the originally requested dimensions before native clipping.
        if width <= 0 || height <= 0 {
            return Ok(jvm.instantiate_array("B", 0).await?.into());
        }

        let requested_width = width as usize;
        let requested_height = height as usize;

        let requested_row_stride = match requested_width
            .checked_mul(3)
            .and_then(|value| value.checked_add(3))
            .map(|value| value & !3)
        {
            Some(value) => value,
            None => return Ok(jvm.instantiate_array("B", 0).await?.into()),
        };

        let requested_image_size =
            match requested_row_stride.checked_mul(requested_height) {
                Some(value) => value,
                None => return Ok(jvm.instantiate_array("B", 0).await?.into()),
            };

        let allocated_size = match requested_image_size.checked_add(54) {
            Some(value) => value,
            None => return Ok(jvm.instantiate_array("B", 0).await?.into()),
        };

        let mut result = vec![0u8; allocated_size];

        let mut midp_graphics: ClassInstanceRef<MidpGraphics> =
            jvm.get_field(
                &this,
                "midpGraphics",
                "Ljavax/microedition/lcdui/Graphics;",
            )
            .await?;

        let graphics_width: i32 =
            jvm.get_field(&midp_graphics, "width", "I").await?;
        let graphics_height: i32 =
            jvm.get_field(&midp_graphics, "height", "I").await?;

        // Native encode_image(flag=0) intersects against the Graphics'
        // logical region, not the current translated origin or clip.
        let request_right = x.wrapping_add(width);
        let request_bottom = y.wrapping_add(height);

        let left = x.max(0);
        let top = y.max(0);
        let right = request_right.min(graphics_width);
        let bottom = request_bottom.min(graphics_height);

        // The bridge ignores the native error return. Since VNI_NewByteArray
        // has already produced a zero-filled array, a non-intersecting request
        // simply returns that untouched allocation.
        if right <= left || bottom <= top {
            let mut data_array = jvm.instantiate_array("B", result.len()).await?;
            jvm.array_raw_buffer_mut(&mut data_array)
                .await?
                .write(0, &result)?;
            return Ok(data_array.into());
        }

        let encoded_width = (right - left) as usize;
        let encoded_height = (bottom - top) as usize;

        let row_stride = (encoded_width * 3 + 3) & !3;
        let image_size = row_stride * encoded_height;
        let file_size = image_size + 54;

        // BITMAPFILEHEADER
        result[0] = b'B';
        result[1] = b'M';
        result[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
        // bfReserved1 / bfReserved2 remain zero.
        result[10..14].copy_from_slice(&(54u32).to_le_bytes());

        // BITMAPINFOHEADER
        result[14..18].copy_from_slice(&(40u32).to_le_bytes());
        result[18..22].copy_from_slice(&(encoded_width as i32).to_le_bytes());
        result[22..26].copy_from_slice(&(encoded_height as i32).to_le_bytes());
        result[26..28].copy_from_slice(&(1u16).to_le_bytes());
        result[28..30].copy_from_slice(&(24u16).to_le_bytes());
        // biCompression = BI_RGB
        result[30..34].copy_from_slice(&(0u32).to_le_bytes());
        result[34..38].copy_from_slice(&(image_size as u32).to_le_bytes());
        // Native bmp_encode leaves X/Y pixels-per-meter and palette fields 0.

        let image = MidpGraphics::image(jvm, &mut midp_graphics).await?;
        let backend_image = MidpImage::image(jvm, &image).await?;

        // Native BMP encoder is bottom-up: the first output scanline is the
        // bottom row of the selected source rectangle.
        for output_row in 0..encoded_height {
            let source_y = bottom - 1 - output_row as i32;
            let destination_row = 54 + output_row * row_stride;

            for column in 0..encoded_width {
                let source_x = left + column as i32;

                if source_x < 0
                    || source_y < 0
                    || source_x as u32 >= backend_image.width()
                    || source_y as u32 >= backend_image.height()
                {
                    continue;
                }

                let pixel = backend_image.get_pixel(source_x, source_y);

                // Native backing storage is RGB565. Expand exactly as the
                // reference encoder does, without low-bit replication.
                let red = pixel.r & 0xf8;
                let green = pixel.g & 0xfc;
                let blue = pixel.b & 0xf8;

                let destination = destination_row + column * 3;
                result[destination] = blue;
                result[destination + 1] = green;
                result[destination + 2] = red;
            }
        }

        let mut data_array = jvm.instantiate_array("B", result.len()).await?;
        jvm.array_raw_buffer_mut(&mut data_array)
            .await?
            .write(0, &result)?;

        Ok(data_array.into())
    }

    async fn get_rgb_pixels(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        mut pixels: ClassInstanceRef<Array<i32>>,
        offset: i32,
        bpl: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Graphics::getRGBPixels({this:?}, {x}, {y}, {width}, {height}, {pixels:?}, {offset}, {bpl})");

        if pixels.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "pixels is null.")
                    .await,
            );
        }

        let array_length = jvm.array_length(&pixels).await? as i32;
        let required_length = height.wrapping_mul(bpl);

        if array_length < required_length {
            return Err(
                jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                    .await,
            );
        }

        // Native get_rgb_data() returns M_E_OUTOFBOUND for negative x/y.
        // The Java bridge ignores that return value.
        if width <= 0 || height <= 0 || x < 0 || y < 0 {
            return Ok(());
        }

        let mut midp_graphics: ClassInstanceRef<MidpGraphics> =
            jvm.get_field(
                &this,
                "midpGraphics",
                "Ljavax/microedition/lcdui/Graphics;",
            )
            .await?;

        let graphics_width: i32 =
            jvm.get_field(&midp_graphics, "width", "I").await?;
        let graphics_height: i32 =
            jvm.get_field(&midp_graphics, "height", "I").await?;

        if x >= graphics_width || y >= graphics_height {
            return Ok(());
        }

        let right = x.saturating_add(width).min(graphics_width);
        let bottom = y.saturating_add(height).min(graphics_height);

        if right <= x || bottom <= y {
            return Ok(());
        }

        let copied_width = (right - x) as usize;
        let copied_height = (bottom - y) as usize;
        let row_stride = width as usize;

        // For int[] the native bridge's offset<<2 is the normal four-byte
        // element addressing, so offset is an ordinary Java int[] index.
        let destination_offset = match usize::try_from(offset) {
            Ok(value) => value,
            Err(_) => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        let touched_elements = match copied_height
            .checked_sub(1)
            .and_then(|rows| rows.checked_mul(row_stride))
            .and_then(|prefix| prefix.checked_add(copied_width))
        {
            Some(value) => value,
            None => {
                return Err(
                    jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                        .await,
                );
            }
        };

        // The reference bridge does not perform this second range check.
        // Keep normal native behavior while preventing an unsafe JVM-array
        // access for malformed arguments.
        if destination_offset
            .checked_add(touched_elements)
            .is_none_or(|end| end > array_length as usize)
        {
            return Err(
                jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "")
                    .await,
            );
        }

        let mut data: Vec<i32> =
            jvm.load_array(&pixels, destination_offset, touched_elements)
                .await?;

        let base_translate_x: i32 =
            jvm.get_field(&this, "baseTranslateX", "I").await?;
        let base_translate_y: i32 =
            jvm.get_field(&this, "baseTranslateY", "I").await?;

        let image = MidpGraphics::image(jvm, &mut midp_graphics).await?;
        let backend_image = MidpImage::image(jvm, &image).await?;

        for row in 0..copied_height {
            let source_y = base_translate_y + y + row as i32;
            let destination_row = row * row_stride;

            for column in 0..copied_width {
                let source_x = base_translate_x + x + column as i32;

                if source_x < 0
                    || source_y < 0
                    || source_x as u32 >= backend_image.width()
                    || source_y as u32 >= backend_image.height()
                {
                    continue;
                }

                let pixel = backend_image.get_pixel(source_x, source_y);

                // Native reads RGB565 and expands without bit replication:
                // R5 << 19, G6 << 10, B5 << 3. Alpha remains zero.
                let r = (pixel.r & 0xf8) as i32;
                let g = (pixel.g & 0xfc) as i32;
                let b = (pixel.b & 0xf8) as i32;

                data[destination_row + column] =
                    (r << 16) | (g << 8) | b;
            }
        }

        jvm.store_array(&mut pixels, destination_offset, data).await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use alloc::boxed::Box;

    use jvm::{Array, ClassInstanceRef, JavaError};
    use test_utils::run_jvm_test;
    use wie_midp::classes::javax::microedition::lcdui::Image as MidpImage;
    use wie_util::Result;

    use crate::{classes::org::kwis::msp::lcdui::{Font, Image}, get_protos};

    use super::Graphics;

    #[test]
    fn test_copy_area() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static("org/kwis/msp/lcdui/Image", "createImage", "(II)Lorg/kwis/msp/lcdui/Image;", (4, 1))
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm.invoke_virtual(&image, "getGraphics", "()Lorg/kwis/msp/lcdui/Graphics;", ()).await?;

            let colors = [0xff0000, 0x00ff00, 0x0000ff, 0x000000];
            for (x, color) in colors.into_iter().enumerate() {
                let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (color,)).await?;
                let _: () = jvm.invoke_virtual(&graphics, "fillRect", "(IIII)V", (x as i32, 0, 1, 1)).await?;
            }

            let _: () = jvm.invoke_virtual(&graphics, "copyArea", "(IIIIII)V", (1, 0, 0, 0, 3, 1)).await?;

            let midp_image = Image::midp_image(&jvm, &image).await?;
            let backend_image = MidpImage::image(&jvm, &midp_image).await?;

            let pixel0 = backend_image.get_pixel(0, 0);
            let pixel1 = backend_image.get_pixel(1, 0);
            let pixel2 = backend_image.get_pixel(2, 0);
            let pixel3 = backend_image.get_pixel(3, 0);

            assert_eq!((pixel0.r, pixel0.g, pixel0.b), (0xff, 0x00, 0x00));
            assert_eq!((pixel1.r, pixel1.g, pixel1.b), (0xff, 0x00, 0x00));
            assert_eq!((pixel2.r, pixel2.g, pixel2.b), (0x00, 0xff, 0x00));
            assert_eq!((pixel3.r, pixel3.g, pixel3.b), (0x00, 0x00, 0xff));

            Ok(())
        })
    }

    #[test]
    fn test_alpha_stroke_xor_and_reset_state() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static("org/kwis/msp/lcdui/Image", "createImage", "(II)Lorg/kwis/msp/lcdui/Image;", (2, 2))
                .await?;
            let graphics: ClassInstanceRef<Graphics> = jvm.invoke_virtual(&image, "getGraphics", "()Lorg/kwis/msp/lcdui/Graphics;", ()).await?;

            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getAlpha", "()I", ()).await?, 255);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getStrokeStyle", "()I", ()).await?, 0);
            assert!(!jvm.invoke_virtual::<_, bool>(&graphics, "isXORMode", "()Z", ()).await?);

            let _: () = jvm.invoke_virtual(&graphics, "setStrokeStyle", "(I)V", (1,)).await?;
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getStrokeStyle", "()I", ()).await?, 1);
            assert!(jvm.invoke_virtual::<_, ()>(&graphics, "setStrokeStyle", "(I)V", (2,)).await.is_err());
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getStrokeStyle", "()I", ()).await?, 1);

            let _: () = jvm.invoke_virtual(&graphics, "setXORMode", "(Z)V", (true,)).await?;
            assert!(jvm.invoke_virtual::<_, bool>(&graphics, "isXORMode", "()Z", ()).await?);
            let _: () = jvm.invoke_virtual(&graphics, "setAlpha", "(I)V", (0,)).await?;
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getAlpha", "()I", ()).await?, 0);
            assert!(!jvm.invoke_virtual::<_, bool>(&graphics, "isXORMode", "()Z", ()).await?);
            let _: () = jvm.invoke_virtual(&graphics, "setAlpha", "(I)V", (42,)).await?;
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getAlpha", "()I", ()).await?, 255);

            let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0x123456,)).await?;
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getRedComponent", "()I", ()).await?, 0x12);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getGreenComponent", "()I", ()).await?, 0x34);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getBlueComponent", "()I", ()).await?, 0x56);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getGrayScale", "()I", ()).await?, 0x34);

            let custom_font: ClassInstanceRef<Font> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Font",
                    "getFont",
                    "(III)Lorg/kwis/msp/lcdui/Font;",
                    (0, 1, 16),
                )
                .await?;
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "setFont",
                    "(Lorg/kwis/msp/lcdui/Font;)V",
                    (custom_font,),
                )
                .await?;

            let _: () = jvm.invoke_virtual(&graphics, "setStrokeStyle", "(I)V", (1,)).await?;
            let _: () = jvm.invoke_virtual(&graphics, "reset", "()V", ()).await?;
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getColor", "()I", ()).await?, 0);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getAlpha", "()I", ()).await?, 255);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&graphics, "getStrokeStyle", "()I", ()).await?, 0);
            assert!(!jvm.invoke_virtual::<_, bool>(&graphics, "isXORMode", "()Z", ()).await?);

            let reset_font: ClassInstanceRef<Font> = jvm
                .invoke_virtual(
                    &graphics,
                    "getFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    (),
                )
                .await?;
            let default_font: ClassInstanceRef<Font> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Font",
                    "getDefaultFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    (),
                )
                .await?;

            let reset_face: i32 =
                jvm.invoke_virtual(&reset_font, "getFace", "()I", ()).await?;
            let reset_style: i32 =
                jvm.invoke_virtual(&reset_font, "getStyle", "()I", ()).await?;
            let reset_size: i32 =
                jvm.invoke_virtual(&reset_font, "getSize", "()I", ()).await?;

            let default_face: i32 =
                jvm.invoke_virtual(&default_font, "getFace", "()I", ()).await?;
            let default_style: i32 =
                jvm.invoke_virtual(&default_font, "getStyle", "()I", ()).await?;
            let default_size: i32 =
                jvm.invoke_virtual(&default_font, "getSize", "()I", ()).await?;

            assert_eq!(reset_face, default_face);
            assert_eq!(reset_style, default_style);
            assert_eq!(reset_size, default_size);

            // Native setFont(null) substitutes Font.getDefaultFont().
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "setFont",
                    "(Lorg/kwis/msp/lcdui/Font;)V",
                    (ClassInstanceRef::<Font>::new(None),),
                )
                .await?;

            let null_font: ClassInstanceRef<Font> = jvm
                .invoke_virtual(
                    &graphics,
                    "getFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    (),
                )
                .await?;

            let null_face: i32 =
                jvm.invoke_virtual(&null_font, "getFace", "()I", ()).await?;
            let null_style: i32 =
                jvm.invoke_virtual(&null_font, "getStyle", "()I", ()).await?;
            let null_size: i32 =
                jvm.invoke_virtual(&null_font, "getSize", "()I", ()).await?;

            assert_eq!(null_face, default_face);
            assert_eq!(null_style, default_style);
            assert_eq!(null_size, default_size);

            Ok(())
        })
    }

    #[test]
    fn test_get_pixel_rgb565_base_origin_and_bounds() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (4, 4),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            // Draw a non-RGB565-exact color at absolute backing coordinate (2, 1).
            let _: () = jvm
                .invoke_virtual(&graphics, "setColor", "(I)V", (0x12_34_56,))
                .await?;
            let _: () = jvm
                .invoke_virtual(&graphics, "setPixel", "(II)V", (2, 1))
                .await?;

            // Native getPixel reads RGB565 and expands without bit replication:
            // 0x12 -> 0x10, 0x34 -> 0x34, 0x56 -> 0x50.
            let pixel: i32 = jvm
                .invoke_virtual(&graphics, "getPixel", "(II)I", (2, 1))
                .await?;
            assert_eq!(pixel, 0x10_34_50);

            // Native getPixel uses the Graphics base origin, so a later
            // translate() does not move the raw pixel lookup.
            let _: () = jvm
                .invoke_virtual(&graphics, "translate", "(II)V", (1, 0))
                .await?;
            let translated: i32 = jvm
                .invoke_virtual(&graphics, "getPixel", "(II)I", (2, 1))
                .await?;
            assert_eq!(translated, 0x10_34_50);

            // Bounds are checked against the logical Graphics region before
            // translation, and native returns M_E_OUTOFBOUND (-2022).
            let left: i32 = jvm
                .invoke_virtual(&graphics, "getPixel", "(II)I", (-1, 0))
                .await?;
            assert_eq!(left, -2022);

            let right: i32 = jvm
                .invoke_virtual(&graphics, "getPixel", "(II)I", (4, 0))
                .await?;
            assert_eq!(right, -2022);

            let bottom: i32 = jvm
                .invoke_virtual(&graphics, "getPixel", "(II)I", (0, 4))
                .await?;
            assert_eq!(bottom, -2022);

            Ok(())
        })
    }

    #[test]
    fn test_get_pixel_preserves_wrapped_midp_base_origin() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let midp_image: ClassInstanceRef<MidpImage> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "(II)Ljavax/microedition/lcdui/Image;",
                    (4, 4),
                )
                .await?;

            let midp_graphics: ClassInstanceRef<
                wie_midp::classes::javax::microedition::lcdui::Graphics,
            > = jvm
                .invoke_virtual(
                    &midp_image,
                    "getGraphics",
                    "()Ljavax/microedition/lcdui/Graphics;",
                    (),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(&midp_graphics, "translate", "(II)V", (1, 1))
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .new_class(
                    "org/kwis/msp/lcdui/Graphics",
                    "(Ljavax/microedition/lcdui/Graphics;)V",
                    (midp_graphics,),
                )
                .await?
                .into();

            // The wrapped MIDP Graphics already has base translation (1, 1),
            // so setPixel(1, 1) writes backing coordinate (2, 2).
            let _: () = jvm
                .invoke_virtual(&graphics, "setColor", "(I)V", (0x20_40_60,))
                .await?;
            let _: () = jvm
                .invoke_virtual(&graphics, "setPixel", "(II)V", (1, 1))
                .await?;

            // Add a current translation after wrapping. Native getPixel still
            // uses the base origin captured at construction.
            let _: () = jvm
                .invoke_virtual(&graphics, "translate", "(II)V", (1, 0))
                .await?;

            let pixel: i32 = jvm
                .invoke_virtual(&graphics, "getPixel", "(II)I", (1, 1))
                .await?;
            assert_eq!(pixel, 0x20_40_60);

            Ok(())
        })
    }

    #[test]
    fn test_get_pixels_rgb565_offset_base_origin_and_clipping() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (4, 2),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            // RGB565-exact source pixels:
            // x=0 -> red   0xf800 -> [00, f8]
            // x=1 -> green 0x07e0 -> [e0, 07]
            // x=2 -> blue  0x001f -> [1f, 00]
            for (x, color) in [(0, 0xff0000), (1, 0x00ff00), (2, 0x0000ff)] {
                let _: () = jvm
                    .invoke_virtual(&graphics, "setColor", "(I)V", (color,))
                    .await?;
                let _: () = jvm
                    .invoke_virtual(&graphics, "setPixel", "(II)V", (x, 0))
                    .await?;
            }

            // Later translation must not affect raw getPixels source coordinates.
            let _: () = jvm
                .invoke_virtual(&graphics, "translate", "(II)V", (1, 0))
                .await?;

            let mut pixels = jvm.instantiate_array("B", 24).await?;
            jvm.store_array(&mut pixels, 0, [0x55i8; 24]).await?;

            // Native bridge starts at byte offset offset*4 = 4.
            // Request x=-1,width=4 intersects logical x=0..2.
            // Native compacts the clipped source to the start of the destination row,
            // so six RGB565 bytes are written at indices 4..9.
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "getPixels",
                    "(IIII[BII)V",
                    (-1, 0, 4, 1, pixels.clone(), 1, 8),
                )
                .await?;

            let out: alloc::vec::Vec<i8> = jvm.load_array(&pixels, 0, 24).await?;

            assert_eq!(&out[0..4], &[0x55i8; 4]);

            assert_eq!(
                &out[4..10],
                &[
                    0x00i8,
                    0xf8u8 as i8,
                    0xe0u8 as i8,
                    0x07i8,
                    0x1fi8,
                    0x00i8,
                ],
            );

            // The fourth requested pixel was clipped away, so native leaves its
            // destination bytes untouched instead of clearing them.
            assert_eq!(&out[10..12], &[0x55i8; 2]);

            assert_eq!(&out[12..], &[0x55i8; 12]);

            Ok(())
        })
    }

    #[test]
    fn test_set_pixels_rgb565_offset_translation_and_clipping() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (5, 2),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            // Current translation must affect setPixels destination.
            let _: () = jvm
                .invoke_virtual(&graphics, "translate", "(II)V", (1, 0))
                .await?;

            // Absolute clip becomes x=2..3 after the current translation.
            let _: () = jvm
                .invoke_virtual(&graphics, "setClip", "(IIII)V", (1, 0, 2, 1))
                .await?;

            let mut pixels = jvm.instantiate_array("B", 24).await?;
            jvm.store_array(&mut pixels, 0, [0x55i8; 24]).await?;

            // Native byte[] offset is scaled by four, so RGB565 begins at byte 4.
            // Source row:
            // red   = f800 -> 00 f8
            // green = 07e0 -> e0 07
            // blue  = 001f -> 1f 00
            jvm.store_array(
                &mut pixels,
                4,
                [
                    0x00i8,
                    0xf8u8 as i8,
                    0xe0u8 as i8,
                    0x07i8,
                    0x1fi8,
                    0x00i8,
                ],
            )
            .await?;

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "setPixels",
                    "(IIII[BII)V",
                    (0, 0, 3, 1, pixels, 1, 6),
                )
                .await?;

            let mut midp_graphics: ClassInstanceRef<
                wie_midp::classes::javax::microedition::lcdui::Graphics,
            > = jvm
                .get_field(
                    &graphics,
                    "midpGraphics",
                    "Ljavax/microedition/lcdui/Graphics;",
                )
                .await?;

            let midp_image =
                wie_midp::classes::javax::microedition::lcdui::Graphics::image(
                    &jvm,
                    &mut midp_graphics,
                )
                .await?;

            let backend_image =
                wie_midp::classes::javax::microedition::lcdui::Image::image(
                    &jvm,
                    &midp_image,
                )
                .await?;

            // setPixels destination before clipping is absolute x=1..3.
            // Clip keeps only x=2..3, corresponding to source green/blue.
            let p1 = backend_image.get_pixel(1, 0);
            let p2 = backend_image.get_pixel(2, 0);
            let p3 = backend_image.get_pixel(3, 0);

            assert_eq!((p1.r, p1.g, p1.b), (0x00, 0x00, 0x00));
            assert_eq!((p2.r, p2.g, p2.b), (0x00, 0xff, 0x00));
            assert_eq!((p3.r, p3.g, p3.b), (0x00, 0x00, 0xff));

            Ok(())
        })
    }

    #[test]
    fn test_encode_image_bmp_bottom_up_bgr_and_clipping() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (3, 2),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            // Top row:
            // x0 red, x1 green, x2 blue
            // Bottom row:
            // x0 white, x1 black, x2 0x123456
            for (x, y, color) in [
                (0, 0, 0xff0000),
                (1, 0, 0x00ff00),
                (2, 0, 0x0000ff),
                (0, 1, 0xffffff),
                (1, 1, 0x000000),
                (2, 1, 0x123456),
            ] {
                let _: () = jvm
                    .invoke_virtual(&graphics, "setColor", "(I)V", (color,))
                    .await?;
                let _: () = jvm
                    .invoke_virtual(&graphics, "setPixel", "(II)V", (x, y))
                    .await?;
            }

            // Native encodeImage(flag=0) ignores later translation.
            let _: () = jvm
                .invoke_virtual(&graphics, "translate", "(II)V", (1, 1))
                .await?;

            // Request width 3 starting at x=1. Graphics width is 3,
            // so encoded width becomes 2 while allocation still uses width 3.
            let encoded: ClassInstanceRef<Array<i8>> = jvm
                .invoke_virtual(
                    &graphics,
                    "encodeImage",
                    "(IIII)[B",
                    (1, 0, 3, 2),
                )
                .await?;

            let bytes: alloc::vec::Vec<i8> =
                jvm.load_array(&encoded, 0, jvm.array_length(&encoded).await?).await?;
            let bytes: alloc::vec::Vec<u8> =
                bytes.into_iter().map(|value| value as u8).collect();

            // Original request: width=3 -> row stride 12, height=2.
            // Java bridge therefore allocates 54 + 12*2 = 78 bytes.
            assert_eq!(bytes.len(), 78);

            assert_eq!(&bytes[0..2], b"BM");
            assert_eq!(u32::from_le_bytes(bytes[2..6].try_into().unwrap()), 70);
            assert_eq!(u32::from_le_bytes(bytes[10..14].try_into().unwrap()), 54);
            assert_eq!(u32::from_le_bytes(bytes[14..18].try_into().unwrap()), 40);

            // Clipped native encode width/height.
            assert_eq!(i32::from_le_bytes(bytes[18..22].try_into().unwrap()), 2);
            assert_eq!(i32::from_le_bytes(bytes[22..26].try_into().unwrap()), 2);

            assert_eq!(u16::from_le_bytes(bytes[26..28].try_into().unwrap()), 1);
            assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 24);
            assert_eq!(u32::from_le_bytes(bytes[30..34].try_into().unwrap()), 0);

            // Encoded row stride for clipped width 2 is 8 bytes.
            assert_eq!(u32::from_le_bytes(bytes[34..38].try_into().unwrap()), 16);

            // Native leaves pixels-per-meter and palette fields zero.
            assert_eq!(&bytes[38..54], &[0u8; 16]);

            // BMP is bottom-up.
            // Bottom source row x=1..2:
            // black -> 00 00 00
            // 0x123456 -> RGB565 truncation = 10 34 50, stored BGR = 50 34 10.
            assert_eq!(
                &bytes[54..62],
                &[0x00, 0x00, 0x00, 0x50, 0x34, 0x10, 0x00, 0x00]
            );

            // Top source row x=1..2:
            // green -> 00 FC 00
            // blue  -> F8 00 00
            assert_eq!(
                &bytes[62..70],
                &[0x00, 0xfc, 0x00, 0xf8, 0x00, 0x00, 0x00, 0x00]
            );

            // Allocation was based on requested width=3, but BMP itself is
            // only 70 bytes after clipping. Remaining bytes stay zero.
            assert_eq!(&bytes[70..78], &[0u8; 8]);

            Ok(())
        })
    }

    #[test]
    fn test_get_rgb_pixels_offset_base_origin_crop_and_negative_noop() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (3, 1),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            // These colors exercise native RGB565 truncation/expansion.
            for (x, color) in [
                (0, 0x12_34_56),
                (1, 0xff_00_00),
                (2, 0x00_ff_00),
            ] {
                let _: () = jvm
                    .invoke_virtual(&graphics, "setColor", "(I)V", (color,))
                    .await?;
                let _: () = jvm
                    .invoke_virtual(&graphics, "setPixel", "(II)V", (x, 0))
                    .await?;
            }

            // Later translation must not affect raw getRGBPixels source coordinates.
            let _: () = jvm
                .invoke_virtual(&graphics, "translate", "(II)V", (1, 0))
                .await?;

            let mut pixels = jvm.instantiate_array("I", 8).await?;
            jvm.store_array(&mut pixels, 0, [0x5555_5555i32; 8]).await?;

            // Request width 3 starting at x=1. Only source x=1..2 exists,
            // so the third destination element must remain untouched.
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "getRGBPixels",
                    "(IIII[III)V",
                    (1, 0, 3, 1, pixels.clone(), 2, 3),
                )
                .await?;

            let out: alloc::vec::Vec<i32> = jvm.load_array(&pixels, 0, 8).await?;

            assert_eq!(&out[0..2], &[0x5555_5555i32; 2]);
            assert_eq!(out[2], 0x00f8_0000);
            assert_eq!(out[3], 0x0000_fc00);
            assert_eq!(out[4], 0x5555_5555);
            assert_eq!(&out[5..], &[0x5555_5555i32; 3]);

            // Negative x is rejected by native get_rgb_data(). The Java
            // bridge ignores the native error code, leaving the array intact.
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "getRGBPixels",
                    "(IIII[III)V",
                    (-1, 0, 1, 1, pixels.clone(), 0, 1),
                )
                .await?;

            let after_negative: alloc::vec::Vec<i32> =
                jvm.load_array(&pixels, 0, 8).await?;
            assert_eq!(after_negative, out);

            Ok(())
        })
    }

    #[test]
    fn test_fill_polygon_concave_with_horizontal_edges() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (8, 8),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(&graphics, "setColor", "(I)V", (0x00ff00,))
                .await?;

            // Concave L shape:
            //
            // (1,1) ---- (6,1)
            //   |          |
            //   |        (6,3)
            //   |        /
            //   |   (3,3)
            //   |     |
            // (1,6)--(3,6)
            //
            // Includes horizontal top/inner/bottom edges.
            let mut xs: ClassInstanceRef<Array<i32>> =
                jvm.instantiate_array("I", 6).await?.into();
            let mut ys: ClassInstanceRef<Array<i32>> =
                jvm.instantiate_array("I", 6).await?.into();

            jvm.store_array(&mut xs, 0, [1i32, 6, 6, 3, 3, 1]).await?;
            jvm.store_array(&mut ys, 0, [1i32, 1, 3, 3, 6, 6]).await?;

            let _: () = jvm
                .invoke_virtual(&graphics, "fillPolygon", "([I[I)V", (xs, ys))
                .await?;

            let midp_image = Image::midp_image(&jvm, &image).await?;
            let backend_image = MidpImage::image(&jvm, &midp_image).await?;

            let filled_top = backend_image.get_pixel(5, 2);
            assert_eq!(
                (filled_top.r, filled_top.g, filled_top.b),
                (0x00, 0xff, 0x00),
                "upper arm of concave polygon should be filled"
            );

            let filled_left = backend_image.get_pixel(2, 5);
            assert_eq!(
                (filled_left.r, filled_left.g, filled_left.b),
                (0x00, 0xff, 0x00),
                "left arm of concave polygon should be filled"
            );

            let notch = backend_image.get_pixel(5, 5);
            assert_ne!(
                (notch.r, notch.g, notch.b),
                (0x00, 0xff, 0x00),
                "concave notch must remain outside the fill"
            );

            Ok(())
        })
    }

    #[test]
    fn test_fill_polygon_triangle_and_validation() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    (7, 7),
                )
                .await?;

            let graphics: ClassInstanceRef<Graphics> = jvm
                .invoke_virtual(
                    &image,
                    "getGraphics",
                    "()Lorg/kwis/msp/lcdui/Graphics;",
                    (),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(&graphics, "setColor", "(I)V", (0xff0000,))
                .await?;

            let mut xs: ClassInstanceRef<Array<i32>> = jvm.instantiate_array("I", 3).await?.into();
            let mut ys: ClassInstanceRef<Array<i32>> = jvm.instantiate_array("I", 3).await?.into();
            jvm.store_array(&mut xs, 0, [1i32, 5, 3]).await?;
            jvm.store_array(&mut ys, 0, [1i32, 1, 5]).await?;

            let _: () = jvm
                .invoke_virtual(&graphics, "fillPolygon", "([I[I)V", (xs.clone(), ys.clone()))
                .await?;

            let midp_image = Image::midp_image(&jvm, &image).await?;
            let backend_image = MidpImage::image(&jvm, &midp_image).await?;

            let interior = backend_image.get_pixel(3, 3);
            assert_eq!(
                (interior.r, interior.g, interior.b),
                (0xff, 0x00, 0x00),
                "triangle interior should be filled"
            );

            let outside = backend_image.get_pixel(0, 0);
            assert_ne!(
                (outside.r, outside.g, outside.b),
                (0xff, 0x00, 0x00),
                "pixel outside polygon must remain unfilled"
            );

            // Native Graphics_fillPolygon0 throws IllegalArgumentException
            // when x.length != y.length.
            let short_y: ClassInstanceRef<Array<i32>> = jvm.instantiate_array("I", 2).await?.into();
            let mismatch: jvm::Result<()> = jvm
                .invoke_virtual(
                    &graphics,
                    "fillPolygon",
                    "([I[I)V",
                    (xs.clone(), short_y),
                )
                .await;

            let Err(JavaError::JavaException(exception)) = mismatch else {
                panic!("fillPolygon accepted mismatched coordinate arrays");
            };
            assert!(jvm.is_instance(&*exception, "java/lang/IllegalArgumentException"));

            // Native messages:
            // x == null -> NullPointerException("x is null.")
            // y == null -> NullPointerException("y is null.")
            let null_x = ClassInstanceRef::<Array<i32>>::new(None);
            let null_result: jvm::Result<()> = jvm
                .invoke_virtual(
                    &graphics,
                    "fillPolygon",
                    "([I[I)V",
                    (null_x, ys.clone()),
                )
                .await;

            let Err(JavaError::JavaException(exception)) = null_result else {
                panic!("fillPolygon accepted null x array");
            };
            assert!(jvm.is_instance(&*exception, "java/lang/NullPointerException"));

            let null_y = ClassInstanceRef::<Array<i32>>::new(None);
            let null_result: jvm::Result<()> = jvm
                .invoke_virtual(
                    &graphics,
                    "fillPolygon",
                    "([I[I)V",
                    (xs, null_y),
                )
                .await;

            let Err(JavaError::JavaException(exception)) = null_result else {
                panic!("fillPolygon accepted null y array");
            };
            assert!(jvm.is_instance(&*exception, "java/lang/NullPointerException"));

            Ok(())
        })
    }
}
