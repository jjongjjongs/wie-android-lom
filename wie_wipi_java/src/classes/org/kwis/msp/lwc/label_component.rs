use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

use crate::classes::org::kwis::msp::lcdui::{Font, Image};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.LabelComponent
pub struct LabelComponent;

impl LabelComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/LabelComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Ljava/lang/String;)V",
                    Self::init_label,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Ljava/lang/String;Lorg/kwis/msp/lcdui/Image;)V",
                    Self::init_label_image,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    Self::get_font,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setFont",
                    "(Lorg/kwis/msp/lcdui/Font;)V",
                    Self::set_font,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getImage",
                    "()Lorg/kwis/msp/lcdui/Image;",
                    Self::get_image,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setImage",
                    "(Lorg/kwis/msp/lcdui/Image;)V",
                    Self::set_image,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getLabel",
                    "()Ljava/lang/String;",
                    Self::get_label,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setLabel",
                    "(Ljava/lang/String;)V",
                    Self::set_label,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getOffset",
                    "()I",
                    Self::get_offset,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setLayout",
                    "(I)V",
                    Self::set_layout,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "calcPreferredSize",
                    "(I)V",
                    Self::calc_preferred_size,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "paintContent",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    Self::paint_content,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new("layout", "I", Default::default()),
                JavaFieldProto::new(
                    "font",
                    "Lorg/kwis/msp/lcdui/Font;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "label",
                    "Ljava/lang/String;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "image",
                    "Lorg/kwis/msp/lcdui/Image;",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "<init>",
                "()V",
                (),
            )
            .await?;

        jvm.put_field(&mut this, "layout", "I", 9).await?;

        let font: ClassInstanceRef<Font> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Font",
                "getDefaultFont",
                "()Lorg/kwis/msp/lcdui/Font;",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "font",
            "Lorg/kwis/msp/lcdui/Font;",
            font,
        )
        .await?;

        Ok(())
    }

    async fn init_label(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        label: ClassInstanceRef<String>,
    ) -> JvmResult<()> {
        Self::init(jvm, context, this.clone()).await?;

        jvm.put_field(
            &mut this,
            "label",
            "Ljava/lang/String;",
            label,
        )
        .await?;

        Ok(())
    }

    async fn init_label_image(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        label: ClassInstanceRef<String>,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        Self::init(jvm, context, this.clone()).await?;

        jvm.put_field(
            &mut this,
            "label",
            "Ljava/lang/String;",
            label,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "image",
            "Lorg/kwis/msp/lcdui/Image;",
            image,
        )
        .await?;

        Ok(())
    }

    async fn get_font(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Font>> {
        jvm.get_field(
            &this,
            "font",
            "Lorg/kwis/msp/lcdui/Font;",
        )
        .await
    }

    async fn set_font(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        mut font: ClassInstanceRef<Font>,
    ) -> JvmResult<()> {
        if font.is_null() {
            font = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Font",
                    "getDefaultFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    (),
                )
                .await?;
        }

        let current: ClassInstanceRef<Font> = jvm
            .get_field(
                &this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        if current.identity() != font.identity() {
            jvm.put_field(
                &mut this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
                font,
            )
            .await?;

            let _: () = jvm
                .invoke_virtual(&this, "invalidate", "()V", ())
                .await?;
        }

        Ok(())
    }

    async fn get_image(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        jvm.get_field(
            &this,
            "image",
            "Lorg/kwis/msp/lcdui/Image;",
        )
        .await
    }

    async fn set_image(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "image",
            "Lorg/kwis/msp/lcdui/Image;",
            image,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;
        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn get_label(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<String>> {
        jvm.get_field(
            &this,
            "label",
            "Ljava/lang/String;",
        )
        .await
    }

    async fn set_label(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        label: ClassInstanceRef<String>,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "label",
            "Ljava/lang/String;",
            label,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;
        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }
    async fn set_layout(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        layout: i32,
    ) -> JvmResult<()> {
        // WipiPlayer Plus LabelComponent.setLayout(int):
        // reject conflicting horizontal/vertical layout bit combinations.
        if (layout & 3) == 3
            || layout > 36
            || (layout & 5) == 5
            || (layout & 6) == 6
            || (layout & 48) == 48
            || (layout & 40) == 40
            || (layout & 24) == 24
        {
            return Err(
                jvm.exception("java/lang/IllegalArgumentException", "")
                    .await,
            );
        }

        jvm.put_field(&mut this, "layout", "I", layout).await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "repaint",
                "(IIII)V",
                (0, 0, width, height),
            )
            .await?;

        Ok(())
    }

    async fn get_offset(
        _: &Jvm,
        _: &mut WieJvmContext,
        _: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        // WipiPlayer Plus LabelComponent.getOffset() always returns zero.
        Ok(0)
    }

    async fn get_formatted_width(
        jvm: &Jvm,
        this: &ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        let offset: i32 = jvm
            .invoke_virtual(
                this,
                "getOffset",
                "()I",
                (),
            )
            .await?;
        let mut width = offset;

        let image: ClassInstanceRef<Image> = jvm
            .get_field(
                this,
                "image",
                "Lorg/kwis/msp/lcdui/Image;",
            )
            .await?;

        if !image.is_null() {
            let image_width: i32 = jvm
                .invoke_virtual(
                    &image,
                    "getWidth",
                    "()I",
                    (),
                )
                .await?;
            width += image_width;
        }

        let label: ClassInstanceRef<String> = jvm
            .get_field(
                this,
                "label",
                "Ljava/lang/String;",
            )
            .await?;

        if label.is_null() {
            return Ok(width);
        }

        if !image.is_null() {
            let length: i32 = jvm
                .invoke_virtual(
                    &label,
                    "length",
                    "()I",
                    (),
                )
                .await?;

            if length != 0 {
                width += 4;
            }
        }

        let font: ClassInstanceRef<Font> = jvm
            .get_field(
                this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        if font.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let text_width: i32 = jvm
            .invoke_virtual(
                &font,
                "stringWidth",
                "(Ljava/lang/String;)I",
                (label,),
            )
            .await?;

        Ok(width + text_width)
    }

    async fn get_formatted_height(
        jvm: &Jvm,
        this: &ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        let label: ClassInstanceRef<String> = jvm
            .get_field(
                this,
                "label",
                "Ljava/lang/String;",
            )
            .await?;

        let image: ClassInstanceRef<Image> = jvm
            .get_field(
                this,
                "image",
                "Lorg/kwis/msp/lcdui/Image;",
            )
            .await?;

        if label.is_null() && image.is_null() {
            return Ok(0);
        }

        let text_height = if !label.is_null() {
            let font: ClassInstanceRef<Font> = jvm
                .get_field(
                    this,
                    "font",
                    "Lorg/kwis/msp/lcdui/Font;",
                )
                .await?;

            if font.is_null() {
                return Err(
                    jvm.exception("java/lang/NullPointerException", "")
                        .await,
                );
            }

            let height: i32 = jvm
                .invoke_virtual(
                    &font,
                    "getHeight",
                    "()I",
                    (),
                )
                .await?;

            height + 1
        } else {
            0
        };

        let image_height = if !image.is_null() {
            jvm.invoke_virtual(
                &image,
                "getHeight",
                "()I",
                (),
            )
            .await?
        } else {
            0
        };

        Ok(core::cmp::max(text_height, image_height) + 4)
    }

    async fn get_formatted_height_for_width(
        jvm: &Jvm,
        this: &ClassInstanceRef<Self>,
        width: i32,
    ) -> JvmResult<i32> {
        let font: ClassInstanceRef<Font> = jvm
            .get_field(
                this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        if font.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let font_height: i32 = jvm
            .invoke_virtual(
                &font,
                "getHeight",
                "()I",
                (),
            )
            .await?;

        let line_height = font_height + 1;

        let offset: i32 = jvm
            .invoke_virtual(
                this,
                "getOffset",
                "()I",
                (),
            )
            .await?;

        let image: ClassInstanceRef<Image> = jvm
            .get_field(
                this,
                "image",
                "Lorg/kwis/msp/lcdui/Image;",
            )
            .await?;

        // Native locals correspond to:
        //   height              -> accumulated formatted height before final +2
        //   minimum_height      -> imageHeight+4, or fontHeight+5 without image
        //   remaining_image_h   -> image height still occupying horizontal space
        //   image_width         -> width reserved beside the image
        //   horizontal_offset   -> left edge available to text
        let mut height = 2;
        let minimum_height;
        let mut remaining_image_h = 0;
        let mut image_width = 0;
        let mut horizontal_offset = offset;

        if !image.is_null() {
            let actual_image_width: i32 = jvm
                .invoke_virtual(
                    &image,
                    "getWidth",
                    "()I",
                    (),
                )
                .await?;

            let actual_image_height: i32 = jvm
                .invoke_virtual(
                    &image,
                    "getHeight",
                    "()I",
                    (),
                )
                .await?;

            minimum_height = actual_image_height + 4;

            if width - offset > actual_image_width {
                image_width = actual_image_width;
                remaining_image_h = actual_image_height;
                horizontal_offset = offset + 4;
            } else {
                // Image itself does not fit beside text. Native starts text below it.
                height = actual_image_height + 2;
            }
        } else {
            minimum_height = font_height + 5;
        }

        let label: ClassInstanceRef<String> = jvm
            .get_field(
                this,
                "label",
                "Ljava/lang/String;",
            )
            .await?;

        if !label.is_null() {
            let length: i32 = jvm
                .invoke_virtual(
                    &label,
                    "length",
                    "()I",
                    (),
                )
                .await?;

            if image_width > 0 {
                horizontal_offset += image_width;
            }

            if length > 0 {
                let mut current_width = 0;
                let mut position = 1;
                let mut last_break_position = 0;

                loop {
                    let index = position - 1;

                    let ch: JavaChar = jvm
                        .invoke_virtual(
                            &label,
                            "charAt",
                            "(I)C",
                            (index,),
                        )
                        .await?;

                    let char_width: i32 = jvm
                        .invoke_virtual(
                            &font,
                            "charWidth",
                            "(C)I",
                            (ch,),
                        )
                        .await?;

                    let next_width = current_width + char_width;
                    let available_width = width - horizontal_offset;
                    let newline = u16::from(ch) == 10;

                    if newline || next_width > available_width {
                        let previous_remaining = remaining_image_h;
                        remaining_image_h =
                            core::cmp::max(remaining_image_h - line_height, 0);

                        height += line_height;

                        // Native frees the image's horizontal reservation only
                        // when the subtraction goes negative, not when it is
                        // exactly zero.
                        if previous_remaining < line_height && image_width != 0 {
                            horizontal_offset -= image_width;
                            image_width = 0;
                        }

                        if newline {
                            current_width = 0;
                            last_break_position = position;
                        } else {
                            current_width = char_width;
                            last_break_position = index;
                        }
                    } else {
                        current_width = next_width;
                    }

                    if position == length {
                        break;
                    }

                    position += 1;
                }

                if position > last_break_position {
                    height += line_height;
                }
            }
        }

        Ok(core::cmp::max(height + 2, minimum_height))
    }

    async fn calc_preferred_size(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        width: i32,
    ) -> JvmResult<()> {
        if width >= 0 {
            jvm.put_field(
                &mut this,
                "prefW",
                "I",
                width,
            )
            .await?;

            let height =
                Self::get_formatted_height_for_width(jvm, &this, width)
                    .await?;

            jvm.put_field(
                &mut this,
                "prefH",
                "I",
                height,
            )
            .await?;
        } else {
            let preferred_width =
                Self::get_formatted_width(jvm, &this).await?;

            jvm.put_field(
                &mut this,
                "prefW",
                "I",
                preferred_width,
            )
            .await?;

            let preferred_height =
                Self::get_formatted_height(jvm, &this).await?;

            jvm.put_field(
                &mut this,
                "prefH",
                "I",
                preferred_height,
            )
            .await?;
        }

        Ok(())
    }

    async fn paint_content(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        graphics: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        if graphics.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let font: ClassInstanceRef<Font> = jvm
            .get_field(
                &this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setFont",
                "(Lorg/kwis/msp/lcdui/Font;)V",
                (font.clone(),),
            )
            .await?;

        // Native calls Component.paintContent(Graphics) directly.
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "paintContent",
                "(Lorg/kwis/msp/lcdui/Graphics;)V",
                (graphics.clone(),),
            )
            .await?;

        if font.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let font_height: i32 = jvm
            .invoke_virtual(
                &font,
                "getHeight",
                "()I",
                (),
            )
            .await?;

        let line_height = font_height + 1;

        let mask: i32 =
            jvm.get_field(&this, "mask", "I").await?;
        let width: i32 =
            jvm.get_field(&this, "w", "I").await?;
        let height: i32 =
            jvm.get_field(&this, "h", "I").await?;

        // Native selected/focused background:
        // Decorator static +0x58 = RGB(160,160,200).
        if mask & 6 == 6 {
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "setColor",
                    "(I)V",
                    (0x00a0a0c8i32,),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "fillRect",
                    "(IIII)V",
                    (0, 0, width, height),
                )
                .await?;
        } else {
            let background: i32 =
                jvm.get_field(&this, "bg", "I").await?;

            if background >= 0 {
                let _: () = jvm
                    .invoke_virtual(
                        &graphics,
                        "setColor",
                        "(I)V",
                        (background,),
                    )
                    .await?;

                let _: () = jvm
                    .invoke_virtual(
                        &graphics,
                        "fillRect",
                        "(IIII)V",
                        (0, 0, width, height),
                    )
                    .await?;
            }
        }

        // Native uses the explicit foreground whenever fg >= 0.
        // Only the default-color path depends on the selected/focused state:
        //
        //   selected: Decorator +0x54 = RGB(0,0,0)
        //   normal:   Decorator +0x48 = RGB(0,0,64)
        let configured_foreground: i32 =
            jvm.get_field(&this, "fg", "I").await?;

        let foreground = if configured_foreground >= 0 {
            configured_foreground
        } else if mask & 6 == 6 {
            0x00000000i32
        } else {
            0x00000040i32
        };

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setColor",
                "(I)V",
                (foreground,),
            )
            .await?;

        let offset: i32 = jvm
            .invoke_virtual(
                &this,
                "getOffset",
                "()I",
                (),
            )
            .await?;

        let formatted_height =
            Self::get_formatted_height_for_width(
                jvm,
                &this,
                width,
            )
            .await?;

        let layout: i32 =
            jvm.get_field(&this, "layout", "I").await?;

        let label: ClassInstanceRef<String> = jvm
            .get_field(
                &this,
                "label",
                "Ljava/lang/String;",
            )
            .await?;

        let label_width = if !label.is_null() {
            jvm.invoke_virtual(
                &font,
                "stringWidth",
                "(Ljava/lang/String;)I",
                (label.clone(),),
            )
            .await?
        } else {
            0
        };

        let image: ClassInstanceRef<Image> = jvm
            .get_field(
                &this,
                "image",
                "Lorg/kwis/msp/lcdui/Image;",
            )
            .await?;

        let mut x = offset;
        let mut y = 2;

        // Width occupied by an image beside the current text lines.
        let mut reserved_image_width = 0;

        // Remaining vertical image area that prevents text from using the
        // image's horizontal region.
        let mut remaining_image_height = 0;

        if !image.is_null() {
            let image_width: i32 = jvm
                .invoke_virtual(
                    &image,
                    "getWidth",
                    "()I",
                    (),
                )
                .await?;

            let image_height: i32 = jvm
                .invoke_virtual(
                    &image,
                    "getHeight",
                    "()I",
                    (),
                )
                .await?;

            let spacing =
                if label_width != 0 { 4 } else { 0 };
            let combined_width =
                image_width + label_width + spacing;

            // Horizontal image placement.
            if layout & 1 != 0 {
                x = offset;
            } else if layout & 2 != 0 {
                if combined_width <= width {
                    x = width - combined_width;
                } else {
                    x = offset;
                }
            } else if layout & 4 != 0 {
                if combined_width <= width {
                    x = (width - combined_width) / 2;
                } else {
                    x = offset;
                }
            } else {
                x = offset;
            }

            // Vertical image placement.
            if layout & 8 == 0 && formatted_height < height {
                if layout & 16 != 0 {
                    y = height - formatted_height;
                } else if layout & 32 != 0 {
                    y = (height - formatted_height) / 2;
                }
            }

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "drawImage",
                    "(Lorg/kwis/msp/lcdui/Image;III)V",
                    (image.clone(), x, y, 4),
                )
                .await?;

            // If there is horizontal room to the right of the image,
            // text initially flows beside it. Otherwise text starts below it.
            if width - x > image_width {
                x += 4;
                reserved_image_width = image_width;
                remaining_image_height = image_height;
            } else {
                y += image_height;
            }
        }

        if label.is_null() {
            return Ok(());
        }

        let length: i32 = jvm
            .invoke_virtual(
                &label,
                "length",
                "()I",
                (),
            )
            .await?;

        if reserved_image_width > 0 {
            x += reserved_image_width;
        } else {
            // Initial text alignment when no image occupies the text line.
            if layout & 1 == 0 && width - x > label_width {
                if layout & 4 != 0 {
                    x = (reserved_image_width + width - label_width) / 2;
                } else if layout & 2 != 0 {
                    x = width - label_width;
                }
            }

            // Native 0x225594..0x2255bc uses the label width (r4) in
            // this no-image/no-reservation vertical-alignment branch.
            // It also tests 0x02 for the centering case.
            if layout & 8 == 0 && formatted_height < height {
                if layout & 16 != 0 {
                    y = height - label_width;
                } else if layout & 2 != 0 {
                    y = (height - label_width) / 2;
                }
            }
        }

        if length <= 0 {
            return Ok(());
        }

        // Native string wrapping state:
        //
        // position:
        //   1-based current character position.
        //
        // start:
        //   zero-based substring start for the current line.
        //
        // last_fit:
        //   1-based position of the last character accepted on the line.
        //
        // current_width:
        //   accumulated width of the current line.
        let mut position = 1;
        let mut start = 0;
        let mut last_fit = 0;
        let mut current_width = 0;

        loop {
            let index = position - 1;

            let ch: JavaChar = jvm
                .invoke_virtual(
                    &label,
                    "charAt",
                    "(I)C",
                    (index,),
                )
                .await?;

            let char_width: i32 = jvm
                .invoke_virtual(
                    &font,
                    "charWidth",
                    "(C)I",
                    (ch,),
                )
                .await?;

            let next_width = current_width + char_width;
            let available_width = width - x;
            let newline = u16::from(ch) == 10;

            if next_width <= available_width && !newline {
                current_width = next_width;
                last_fit = position;

                if position == length {
                    // Draw the final accumulated line.
                    if position > start {
                        let line_len = position - start;
                        let mut line_x = x;

                        if layout & 1 == 0 {
                            let line_width: i32 = jvm
                                .invoke_virtual(
                                    &font,
                                    "substringWidth",
                                    "(Ljava/lang/String;II)I",
                                    (
                                        label.clone(),
                                        start,
                                        line_len,
                                    ),
                                )
                                .await?;

                            if width - x > line_width {
                                if layout & 4 != 0 {
                                    line_x =
                                        (
                                            reserved_image_width
                                                + width
                                                - line_width
                                        ) / 2;
                                } else if layout & 2 != 0 {
                                    line_x = width - line_width;
                                }
                            }
                        }

                        let _: () = jvm
                            .invoke_virtual(
                                &graphics,
                                "drawSubstring",
                                "(Ljava/lang/String;IIIII)V",
                                (
                                    label.clone(),
                                    start,
                                    line_len,
                                    line_x,
                                    y,
                                    4,
                                ),
                            )
                            .await?;
                    }

                    return Ok(());
                }

                position += 1;
                continue;
            }

            // A wrap or newline terminates the previously accepted part
            // of the current line.
            if last_fit > start {
                let line_len = last_fit - start;
                let mut line_x = x;

                if layout & 1 == 0 {
                    let line_width: i32 = jvm
                        .invoke_virtual(
                            &font,
                            "substringWidth",
                            "(Ljava/lang/String;II)I",
                            (
                                label.clone(),
                                start,
                                line_len,
                            ),
                        )
                        .await?;

                    if width - x > line_width {
                        if layout & 4 != 0 {
                            line_x =
                                (
                                    reserved_image_width
                                        + width
                                        - line_width
                                ) / 2;
                        } else if layout & 2 != 0 {
                            line_x = width - line_width;
                        }
                    }
                }

                let _: () = jvm
                    .invoke_virtual(
                        &graphics,
                        "drawSubstring",
                        "(Ljava/lang/String;IIIII)V",
                        (
                            label.clone(),
                            start,
                            line_len,
                            line_x,
                            y,
                            4,
                        ),
                    )
                    .await?;
            }

            let next_y = y + line_height;
            let next_remaining =
                remaining_image_height - line_height;

            if next_remaining > 0 {
                remaining_image_height = next_remaining;
            } else if reserved_image_width > 0 {
                x -= reserved_image_width;
                reserved_image_width = 0;
                remaining_image_height = 0;
            } else {
                x = jvm
                    .invoke_virtual(
                        &this,
                        "getOffset",
                        "()I",
                        (),
                    )
                    .await?;
            }

            y = next_y;

            if newline {
                current_width = 0;
                start = position;
            } else {
                current_width = char_width;
                start = index;
            }

            // Native updates this to the current 1-based position before
            // testing whether the current character was the final one.
            last_fit = position;

            if position == length {
                if position > start {
                    let line_len = position - start;
                    let mut line_x = x;

                    if layout & 1 == 0 {
                        let line_width: i32 = jvm
                            .invoke_virtual(
                                &font,
                                "substringWidth",
                                "(Ljava/lang/String;II)I",
                                (
                                    label.clone(),
                                    start,
                                    line_len,
                                ),
                            )
                            .await?;

                        if width - x > line_width {
                            if layout & 4 != 0 {
                                line_x =
                                    (
                                        reserved_image_width
                                            + width
                                            - line_width
                                    ) / 2;
                            } else if layout & 2 != 0 {
                                line_x = width - line_width;
                            }
                        }
                    }

                    let _: () = jvm
                        .invoke_virtual(
                            &graphics,
                            "drawSubstring",
                            "(Ljava/lang/String;IIIII)V",
                            (
                                label.clone(),
                                start,
                                line_len,
                                line_x,
                                y,
                                4,
                            ),
                        )
                        .await?;
                }

                return Ok(());
            }

            position += 1;
        }
    }

}
