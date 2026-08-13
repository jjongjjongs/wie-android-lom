use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.ScrollbarComponent
pub struct ScrollbarComponent;

impl ScrollbarComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/ScrollbarComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<clinit>",
                    "()V",
                    Self::cl_init,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "<init>",
                    "()V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(I)V",
                    Self::init_with_direction,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(IIIIII)V",
                    Self::init_with_values,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getPreferredWidth",
                    "()I",
                    Self::get_preferred_width,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getPreferredHeight",
                    "(I)I",
                    Self::get_preferred_height_with_width,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getPreferredHeight",
                    "()I",
                    Self::get_preferred_height,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "keyNotify",
                    "(II)Z",
                    Self::key_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "focusNotify",
                    "(Z)V",
                    Self::focus_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "paintContent",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    Self::paint_content,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getForegroundColor",
                    "()I",
                    Self::get_foreground_color,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setForegroundColor",
                    "(I)V",
                    Self::set_foreground_color,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getDirection",
                    "()I",
                    Self::get_direction,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setDirection",
                    "(I)V",
                    Self::set_direction,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getCurrentValue",
                    "()I",
                    Self::get_current_value,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setCurrentValue",
                    "(I)V",
                    Self::set_current_value,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getMinimum",
                    "()I",
                    Self::get_minimum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setMinimum",
                    "(I)V",
                    Self::set_minimum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getMaximum",
                    "()I",
                    Self::get_maximum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setMaximum",
                    "(I)V",
                    Self::set_maximum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getViewAmount",
                    "()I",
                    Self::get_view_amount,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setViewAmount",
                    "(I)V",
                    Self::set_view_amount,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getChangeAmount",
                    "()I",
                    Self::get_change_amount,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setChangeAmount",
                    "(I)V",
                    Self::set_change_amount,
                    Default::default(),
                ),
            ],
            fields: vec![
                // Native Java-visible fields.
                JavaFieldProto::new(
                    "HORIZONTAL",
                    "I",
                    FieldAccessFlags::STATIC,
                ),
                JavaFieldProto::new(
                    "VERTICAL",
                    "I",
                    FieldAccessFlags::STATIC,
                ),

                // WIE-private storage for native per-instance slots.
                JavaFieldProto::new(
                    "__wieScrollbarDirection",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarCurrentValue",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarViewAmount",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarMaximum",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarMinimum",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarChangeAmount",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarInitialized",
                    "Z",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarFocused",
                    "Z",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarForegroundColor",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarPaintCross",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarPaintThumb",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarPaintInset",
                    "I",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn cl_init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
    ) -> JvmResult<()> {
        jvm.put_static_field(
            "org/kwis/msp/lwc/ScrollbarComponent",
            "HORIZONTAL",
            "I",
            1i32,
        )
        .await?;

        jvm.put_static_field(
            "org/kwis/msp/lwc/ScrollbarComponent",
            "VERTICAL",
            "I",
            2i32,
        )
        .await?;

        Ok(())
    }

    async fn init(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        Self::init_with_values(
            jvm,
            context,
            this,
            2,
            0,
            1,
            0,
            10,
            1,
        )
        .await
    }

    async fn init_with_direction(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        direction: i32,
    ) -> JvmResult<()> {
        Self::init_with_values(
            jvm,
            context,
            this,
            direction,
            0,
            1,
            0,
            10,
            1,
        )
        .await
    }

    async fn init_with_values(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        direction: i32,
        current_value: i32,
        view_amount: i32,
        minimum: i32,
        maximum: i32,
        change_amount: i32,
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

        if direction != 1 && direction != 2 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "illegal ScrollbarComponent direction",
                )
                .await,
            );
        }

        if maximum <= minimum {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid maximum <= minimum value",
                )
                .await,
            );
        }

        if view_amount < 1 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid viewAmount < 1 value",
                )
                .await,
            );
        }

        if change_amount > view_amount {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid chAmount > viewAmount value",
                )
                .await,
            );
        }

        if current_value < minimum {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid currentValue< minimum value",
                )
                .await,
            );
        }

        if current_value > maximum - view_amount {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid currentValue > maximum - viewAmount value",
                )
                .await,
            );
        }

        if change_amount < 1 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid chAmount < 1",
                )
                .await,
            );
        }

        let mut this = this;

        // Native constructor sets Component mask bit 2.
        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        jvm.put_field(&mut this, "mask", "I", mask | 4).await?;

        jvm.put_field(
            &mut this,
            "__wieScrollbarDirection",
            "I",
            direction,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarCurrentValue",
            "I",
            current_value,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarViewAmount",
            "I",
            view_amount,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMaximum",
            "I",
            maximum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMinimum",
            "I",
            minimum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarChangeAmount",
            "I",
            change_amount,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarInitialized",
            "Z",
            true,
        )
        .await?;

        // Native constructor explicitly clears private +0x38/+0x3c.
        jvm.put_field(
            &mut this,
            "__wieScrollbarFocused",
            "Z",
            false,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarForegroundColor",
            "I",
            0i32,
        )
        .await?;

        Ok(())
    }

    async fn get_preferred_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        let direction: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarDirection",
                "I",
            )
            .await?;

        if direction == 2 {
            // Native vertical scrollbar forces its width to 5.
            jvm.put_field(&mut this, "w", "I", 5i32).await?;
            return Ok(5);
        }

        if direction == 1 {
            let parent: ClassInstanceRef<()> = jvm
                .get_field(
                    &this,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;

            if parent.is_null() {
                return Err(
                    jvm.exception(
                        "java/lang/NullPointerException",
                        "",
                    )
                    .await,
                );
            }

            let width: i32 = jvm.get_field(&parent, "w", "I").await?;
            jvm.put_field(&mut this, "w", "I", width).await?;
            return Ok(width);
        }

        // The native direction setter prevents this state.
        jvm.get_field(&this, "w", "I").await
    }

    async fn get_preferred_height_with_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        _width: i32,
    ) -> JvmResult<i32> {
        // Native overload ignores its width argument and virtually invokes
        // getPreferredHeight().
        jvm.invoke_virtual(
            &this,
            "getPreferredHeight",
            "()I",
            (),
        )
        .await
    }

    async fn get_preferred_height(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        let direction: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarDirection",
                "I",
            )
            .await?;

        if direction == 1 {
            // Native horizontal scrollbar forces its height to 5.
            jvm.put_field(&mut this, "h", "I", 5i32).await?;
            return Ok(5);
        }

        if direction == 2 {
            let parent: ClassInstanceRef<()> = jvm
                .get_field(
                    &this,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;

            if parent.is_null() {
                return Err(
                    jvm.exception(
                        "java/lang/NullPointerException",
                        "",
                    )
                    .await,
                );
            }

            let height: i32 = jvm.get_field(&parent, "h", "I").await?;
            jvm.put_field(&mut this, "h", "I", height).await?;
            return Ok(height);
        }

        jvm.get_field(&this, "h", "I").await
    }

    async fn get_foreground_color(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarForegroundColor",
            "I",
        )
        .await
    }

    async fn set_foreground_color(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        color: i32,
    ) -> JvmResult<()> {
        // Native is a direct +0x3c store: no invalidate/repaint.
        jvm.put_field(
            &mut this,
            "__wieScrollbarForegroundColor",
            "I",
            color,
        )
        .await
    }

    async fn focus_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        focus: bool,
    ) -> JvmResult<()> {
        if focus {
            let _: () = jvm
                .invoke_virtual(
                    &this,
                    "setBackground",
                    "(I)V",
                    (0x003e9effi32,),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &this,
                    "setForegroundColor",
                    "(I)V",
                    (151i32,),
                )
                .await?;
        } else {
            let _: () = jvm
                .invoke_virtual(
                    &this,
                    "setBackground",
                    "(I)V",
                    (0x00d9ecffi32,),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &this,
                    "setForegroundColor",
                    "(I)V",
                    (0x003737ffi32,),
                )
                .await?;
        }

        jvm.put_field(
            &mut this,
            "__wieScrollbarFocused",
            "Z",
            focus,
        )
        .await?;

        // Native performs an explicit repaint after updating +0x38.
        let _: () = jvm
            .invoke_virtual(
                &this,
                "repaint",
                "()V",
                (),
            )
            .await?;

        Ok(())
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        r#type: i32,
        chr: i32,
    ) -> JvmResult<bool> {
        let focused: bool = jvm
            .get_field(
                &this,
                "__wieScrollbarFocused",
                "Z",
            )
            .await?;

        if !focused {
            return Ok(false);
        }

        let action: i32 = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getGameAction",
                "(I)I",
                (chr,),
            )
            .await?;

        // Native processes only key-event type 2.
        if r#type != 2 {
            return Ok(false);
        }

        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let handled = match action {
            // Native treats UP/LEFT identically.
            1 | 2 => {
                let value = current_value - change_amount;

                if value < 0 {
                    jvm.put_field(
                        &mut this,
                        "__wieScrollbarCurrentValue",
                        "I",
                        0i32,
                    )
                    .await?;

                    false
                } else {
                    jvm.put_field(
                        &mut this,
                        "__wieScrollbarCurrentValue",
                        "I",
                        value,
                    )
                    .await?;

                    true
                }
            }

            // Native treats DOWN/RIGHT identically.
            5 | 6 => {
                let value = current_value + change_amount;

                // Native stores this intermediate value before its second
                // changeAmount boundary check.
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarCurrentValue",
                    "I",
                    value,
                )
                .await?;

                if value + change_amount > maximum {
                    jvm.put_field(
                        &mut this,
                        "__wieScrollbarCurrentValue",
                        "I",
                        maximum - change_amount,
                    )
                    .await?;

                    false
                } else {
                    true
                }
            }

            _ => return Ok(false),
        };

        jvm.put_field(
            &mut this,
            "__wieScrollbarInitialized",
            "Z",
            true,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "repaint",
                "()V",
                (),
            )
            .await?;

        Ok(handled)
    }

    async fn paint_content(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        graphics: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native ScrollbarComponent.paintContent(Graphics):
        //
        //   Component.paintContent(g);
        //   calcPositionValue();
        //   paintBar(g);
        //
        // Component.paintContent is explicitly the superclass implementation.
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "paintContent",
                "(Lorg/kwis/msp/lcdui/Graphics;)V",
                (graphics.clone(),),
            )
            .await?;

        Self::calc_position_value(jvm, this.clone()).await?;
        Self::paint_bar(jvm, this, graphics).await
    }

    async fn calc_position_value(
        jvm: &Jvm,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let direction: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarDirection",
                "I",
            )
            .await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let inset: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarPaintInset",
                "I",
            )
            .await?;

        // Native +0x40:
        //   vertical   -> width - 2
        //   horizontal -> height - 2
        //
        // Native +0x44 initially receives the major-axis size:
        //   vertical   -> height
        //   horizontal -> width
        let cross = if direction == 2 {
            width.wrapping_sub(2)
        } else {
            height.wrapping_sub(2)
        };

        let major = if direction == 2 {
            height
        } else {
            width
        };

        if maximum == 0 {
            return Err(
                jvm.exception(
                    "java/lang/ArithmeticException",
                    "",
                )
                .await,
            );
        }

        let available = major.wrapping_sub(inset.wrapping_mul(2));
        let numerator = available.wrapping_mul(view_amount);

        // ARM/Java integer division special case used by the native helper.
        let thumb = if numerator == i32::MIN && maximum == -1 {
            i32::MIN
        } else {
            numerator / maximum
        };

        jvm.put_field(
            &mut this,
            "__wieScrollbarPaintCross",
            "I",
            cross,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "__wieScrollbarPaintThumb",
            "I",
            thumb,
        )
        .await?;

        Ok(())
    }

    async fn paint_bar(
        jvm: &Jvm,
        this: ClassInstanceRef<Self>,
        graphics: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        if graphics.is_null() {
            return Err(
                jvm.exception(
                    "java/lang/NullPointerException",
                    "",
                )
                .await,
            );
        }

        let direction: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarDirection",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let inset: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarPaintInset",
                "I",
            )
            .await?;

        let cross: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarPaintCross",
                "I",
            )
            .await?;

        let thumb: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarPaintThumb",
                "I",
            )
            .await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let major = if direction == 2 {
            height
        } else {
            width
        };

        let range = maximum.wrapping_sub(minimum);

        if range == 0 {
            return Err(
                jvm.exception(
                    "java/lang/ArithmeticException",
                    "",
                )
                .await,
            );
        }

        let available = major.wrapping_sub(inset.wrapping_mul(2));
        let numerator = available.wrapping_mul(current_value);

        let position = if numerator == i32::MIN && range == -1 {
            i32::MIN
        } else {
            numerator / range
        };

        // Decorator static +0x38:
        // RGB(210,230,255) = 0x00d2e6ff.
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setColor",
                "(I)V",
                (0x00d2_e6ffi32,),
            )
            .await?;

        // Native beveled outer border.
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (2, 0, width.wrapping_sub(3), 0),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (
                    2,
                    height.wrapping_sub(1),
                    width.wrapping_sub(3),
                    height.wrapping_sub(1),
                ),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (0, 2, 0, height.wrapping_sub(3)),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (
                    width.wrapping_sub(1),
                    2,
                    width.wrapping_sub(1),
                    height.wrapping_sub(3),
                ),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (1, 1, 1, 1),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (
                    width.wrapping_sub(2),
                    1,
                    width.wrapping_sub(2),
                    height.wrapping_sub(2),
                ),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (
                    width.wrapping_sub(2),
                    height.wrapping_sub(2),
                    width.wrapping_sub(2),
                    height.wrapping_sub(2),
                ),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawLine",
                "(IIII)V",
                (
                    1,
                    height.wrapping_sub(2),
                    1,
                    height.wrapping_sub(2),
                ),
            )
            .await?;

        // Decorator static +0x40:
        // RGB(200,200,255) = 0x00c8c8ff.
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setColor",
                "(I)V",
                (0x00c8_c8ffi32,),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "fillRect",
                "(IIII)V",
                (
                    2,
                    2,
                    width.wrapping_sub(4),
                    height.wrapping_sub(4),
                ),
            )
            .await?;

        // Decorator static +0x44:
        // RGB(100,100,210) = 0x006464d2.
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setColor",
                "(I)V",
                (0x0064_64d2i32,),
            )
            .await?;

        if direction == 2 {
            // vertical:
            // fillRect(1, position + 1, width - 2, thumb)
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "fillRect",
                    "(IIII)V",
                    (
                        1,
                        position.wrapping_add(1),
                        cross,
                        thumb,
                    ),
                )
                .await?;
        } else {
            // horizontal:
            // fillRect(position + 1, 1, thumb, height - 2)
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "fillRect",
                    "(IIII)V",
                    (
                        position.wrapping_add(1),
                        1,
                        thumb,
                        cross,
                    ),
                )
                .await?;
        }

        Ok(())
    }

    async fn get_direction(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarDirection",
            "I",
        )
        .await
    }

    async fn set_direction(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        direction: i32,
    ) -> JvmResult<()> {
        let old: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarDirection",
                "I",
            )
            .await?;

        if old == direction {
            return Ok(());
        }

        if direction != 1 && direction != 2 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "illegal ScrollbarComponent direction",
                )
                .await,
            );
        }

        jvm.put_field(
            &mut this,
            "__wieScrollbarDirection",
            "I",
            direction,
        )
        .await?;

        Ok(())
    }

    async fn get_current_value(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarCurrentValue",
            "I",
        )
        .await
    }

    async fn set_current_value(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        value: i32,
    ) -> JvmResult<()> {
        let view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        // Native setCurrentValue clamps negative input to zero before
        // delegating to synchronized setValues().
        let value = value.max(0);

        Self::set_values(
            jvm,
            this,
            value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_minimum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarMinimum",
            "I",
        )
        .await
    }

    async fn set_minimum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        minimum: i32,
    ) -> JvmResult<()> {
        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let old_minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let mut view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let mut change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let range = maximum - minimum;

        // Native adjusts existing slots only while raising minimum.
        if minimum > old_minimum {
            if range < view_amount {
                view_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarViewAmount",
                    "I",
                    view_amount,
                )
                .await?;
            }

            if range < change_amount {
                change_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarChangeAmount",
                    "I",
                    change_amount,
                )
                .await?;
            }
        }

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_maximum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarMaximum",
            "I",
        )
        .await
    }

    async fn set_maximum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        maximum: i32,
    ) -> JvmResult<()> {
        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let old_maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let mut view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let mut change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        let mut current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let range = maximum - minimum;

        // Native pre-adjusts the existing state only when maximum shrinks.
        if maximum < old_maximum {
            if range < view_amount {
                view_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarViewAmount",
                    "I",
                    view_amount,
                )
                .await?;
            }

            if range < change_amount {
                change_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarChangeAmount",
                    "I",
                    change_amount,
                )
                .await?;
            }

            let max_current = maximum - view_amount;

            if max_current < current_value {
                current_value = max_current;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarCurrentValue",
                    "I",
                    current_value,
                )
                .await?;
            }
        }

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_view_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarViewAmount",
            "I",
        )
        .await
    }

    async fn set_view_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        view_amount: i32,
    ) -> JvmResult<()> {
        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_change_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarChangeAmount",
            "I",
        )
        .await
    }

    async fn set_change_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        change_amount: i32,
    ) -> JvmResult<()> {
        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn set_values(
        jvm: &Jvm,
        mut this: ClassInstanceRef<Self>,
        mut current_value: i32,
        mut view_amount: i32,
        minimum: i32,
        mut maximum: i32,
        mut change_amount: i32,
    ) -> JvmResult<()> {
        // Native ScrollbarComponent.setValues():
        // maximum <= minimum is normalized, not rejected.
        if maximum <= minimum {
            maximum = minimum + 1;
        }

        let range = maximum - minimum;

        if view_amount >= range {
            view_amount = range;
        }

        if current_value < 0 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid currentValue < 0",
                )
                .await,
            );
        }

        if view_amount <= 0 {
            view_amount = 1;
        }

        // Defensive native branch. With the preceding normalization this
        // normally cannot fire, but preserve it exactly.
        if range < view_amount {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid chAmount > viewAmount value",
                )
                .await,
            );
        }

        if minimum > current_value {
            current_value = minimum;
        }

        let max_current = maximum - view_amount;

        if max_current < current_value {
            current_value = max_current;
        }

        if change_amount > view_amount {
            change_amount = view_amount;
        }

        // Native uses IllegalArgumentException() without a message here.
        if current_value < 0 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "",
                )
                .await,
            );
        }

        jvm.put_field(
            &mut this,
            "__wieScrollbarInitialized",
            "Z",
            true,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarCurrentValue",
            "I",
            current_value,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarViewAmount",
            "I",
            view_amount,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMinimum",
            "I",
            minimum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMaximum",
            "I",
            maximum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarChangeAmount",
            "I",
            change_amount,
        )
        .await?;

        Ok(())
    }
}
