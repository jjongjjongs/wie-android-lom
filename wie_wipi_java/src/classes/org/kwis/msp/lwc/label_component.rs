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

}
