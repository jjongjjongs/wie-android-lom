use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::MethodAccessFlags;
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.TextFieldComponent
pub struct TextFieldComponent;

impl TextFieldComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextFieldComponent",
            parent_class: Some("org/kwis/msp/lwc/TextComponent"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "(Ljava/lang/String;I)V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Ljava/lang/String;I)V",
                    Self::init_with_display,
                    Default::default(),
                ),
                JavaMethodProto::new("setString", "(Ljava/lang/String;)V", Self::set_string, Default::default()),
                JavaMethodProto::new("delete", "(II)V", Self::delete, Default::default()),
                JavaMethodProto::new("insert", "(Ljava/lang/String;III)V", Self::insert, Default::default()),
                JavaMethodProto::new("configure", "(IIIII)V", Self::configure, Default::default()),
                JavaMethodProto::new("controlCursor", "(III)V", Self::control_cursor, Default::default()),
                JavaMethodProto::new("notifyChange", "()V", Self::notify_change, Default::default()),
                JavaMethodProto::new("focusNotify", "(Z)V", Self::focus_notify, Default::default()),
                JavaMethodProto::new("controlPopup", "()V", Self::control_popup, Default::default()),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("getPreferredHeight", "()I", Self::get_preferred_height, Default::default()),
                JavaMethodProto::new("getPreferredHeight", "(I)I", Self::get_preferred_height_with_width, Default::default()),
                JavaMethodProto::new("getPreferredWidth", "()I", Self::get_preferred_width, Default::default()),
                JavaMethodProto::new(
                    "paintContent",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    Self::paint_content,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "access$102",
                    "(Lorg/kwis/msp/lwc/TextFieldComponent;I)I",
                    Self::access_102,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "access$000",
                    "(Lorg/kwis/msp/lwc/TextFieldComponent;)I",
                    Self::access_000,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "access$002",
                    "(Lorg/kwis/msp/lwc/TextFieldComponent;I)I",
                    Self::access_002,
                    MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![
                // Native TextFieldComponent instance storage.
                JavaFieldProto::new("__wieTextFieldVisibleStart", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldVisibleEnd", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldPreferredHeight", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldCursorAdjusting", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldAction", "Ljava/lang/Object;", Default::default()),
                JavaFieldProto::new("__wieTextFieldViewportWidth", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldPreferredWidth", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldPopup", "Lorg/kwis/msp/lwc/ShellComponent;", Default::default()),
                JavaFieldProto::new("__wieTextFieldFlagAC", "I", Default::default()),
                JavaFieldProto::new("__wieTextFieldBoundaryHit", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<TextFieldComponent>,
        data: ClassInstanceRef<String>,
        constraint: i32,
    ) -> JvmResult<()> {
        // Native s0 @ 0x2441ac:
        // obtain the default Display and delegate to s1.
        let display: ClassInstanceRef<()> = jvm
            .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
            .await?;

        Self::init_with_display(jvm, context, this, display, data, constraint).await
    }

    async fn init_with_display(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponent>,
        display: ClassInstanceRef<()>,
        data: ClassInstanceRef<String>,
        constraint: i32,
    ) -> JvmResult<()> {
        // Native s1 @ 0x244e50.
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "<init>",
                "(Lorg/kwis/msp/lcdui/Display;I)V",
                (display, constraint),
            )
            .await?;

        // +0x98/+0x8c/+0x90/+0x94 = 0.
        jvm.put_field(&mut this, "__wieTextFieldCursorAdjusting", "I", 0).await?;
        jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0).await?;
        jvm.put_field(&mut this, "__wieTextFieldVisibleEnd", "I", 0).await?;
        jvm.put_field(&mut this, "__wieTextFieldPreferredHeight", "I", 0).await?;

        // Native new TextFieldComponent$Action(this).
        let action = jvm
            .new_class(
                "org/kwis/msp/lwc/TextFieldComponent$Action",
                "(Lorg/kwis/msp/lwc/TextFieldComponent;)V",
                (this.clone(),),
            )
            .await?;

        // +0xb0 = 0
        // +0x9c = Action
        // +0xa4 = -1
        // +0xa0 = -1
        // +0xac = 0
        jvm.put_field(&mut this, "__wieTextFieldBoundaryHit", "I", 0).await?;
        jvm.put_field(&mut this, "__wieTextFieldAction", "Ljava/lang/Object;", action).await?;
        jvm.put_field(&mut this, "__wieTextFieldPreferredWidth", "I", -1).await?;
        jvm.put_field(&mut this, "__wieTextFieldViewportWidth", "I", -1).await?;
        jvm.put_field(&mut this, "__wieTextFieldFlagAC", "I", 0).await?;

        // Native virtual +0xb8 => this.setString(data).
        let _: () = jvm.invoke_virtual(&this, "setString", "(Ljava/lang/String;)V", (data,)).await?;

        // font(+0x68).getHeight() + 4 -> +0x94.
        let font: ClassInstanceRef<()> = jvm.get_field(&this, "__wieFont", "Lorg/kwis/msp/lcdui/Font;").await?;

        if font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let font_height: i32 = jvm.invoke_virtual(&font, "getHeight", "()I", ()).await?;

        jvm.put_field(&mut this, "__wieTextFieldPreferredHeight", "I", font_height.wrapping_add(4))
            .await?;

        Ok(())
    }
    async fn set_string(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponent>,
        data: ClassInstanceRef<String>,
    ) -> JvmResult<()> {
        // Native setString_v0 @ 0x244170:
        // visibleStart(+0x8c) = 0, then TextComponent.setString(data).
        jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;

        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "setString", "(Ljava/lang/String;)V", (data,))
            .await?;

        Ok(())
    }

    async fn delete(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextFieldComponent>, offset: i32, length: i32) -> JvmResult<()> {
        // Native delete_v0 @ 0x244c48:
        // TextComponent.delete(offset, length);
        // repaint(0, 0, width, height);

        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "delete", "(II)V", (offset, length))
            .await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;

        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0i32, 0i32, width, height)).await?;

        Ok(())
    }

    async fn configure(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponent>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: i32,
    ) -> JvmResult<()> {
        // Native configure_v0 @ 0x2440b0:
        // Component.configure(x, y, w, h, flags);
        // if ((flags & 2) != 0) {
        //     preferredWidth(+0xa4) = w;
        // }

        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/Component", "configure", "(IIIII)V", (x, y, w, h, flags))
            .await?;

        if flags & 0x2 != 0 {
            jvm.put_field(&mut this, "__wieTextFieldPreferredWidth", "I", w).await?;
        }

        Ok(())
    }

    async fn control_cursor(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponent>,
        position: i32,
        length: i32,
        mode: i32,
    ) -> JvmResult<()> {
        // Native controlCursor_v0 @ 0x24410c:
        // TextComponent.controlCursor(position, length, mode);
        // cursorAdjusting(+0x98) = 1;
        // notifyChangePosition(this);
        // cursorAdjusting(+0x98) = 0;

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "controlCursor",
                "(III)V",
                (position, length, mode),
            )
            .await?;

        jvm.put_field(&mut this, "__wieTextFieldCursorAdjusting", "I", 1i32).await?;

        Self::notify_change_position(jvm, this.clone()).await?;

        jvm.put_field(&mut this, "__wieTextFieldCursorAdjusting", "I", 0i32).await?;

        Ok(())
    }

    async fn notify_change_position(jvm: &Jvm, mut this: ClassInstanceRef<TextFieldComponent>) -> JvmResult<()> {
        // Native notifyChangePosition_s0 @ 0x2450c8.
        // Config.getLogMask() branches are diagnostic-only and omitted.

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let char_count: i32 = if text.is_null() {
            0
        } else {
            jvm.invoke_virtual(&text, "length", "()I", ()).await?
        };

        let font: ClassInstanceRef<()> = jvm.get_field(&this, "__wieFont", "Lorg/kwis/msp/lcdui/Font;").await?;

        // Native skips the width call when charCount == 0.
        let full_width: i32 = if char_count == 0 {
            0
        } else {
            if text.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            if font.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            // Native @ 0x245134:
            // Font.substringWidth(text, 0, charCount)
            jvm.invoke_virtual(&font, "substringWidth", "(Ljava/lang/String;II)I", (text.clone(), 0i32, char_count))
                .await?
        };

        let viewport_width: i32 = jvm.get_field(&this, "__wieTextFieldViewportWidth", "I").await?;

        let width_limit = viewport_width.wrapping_sub(2);

        // 0x24514c..0x245164:
        // if (viewportWidth - 2 > fullTextWidth) {
        //     visibleStart = 0;
        //     return;
        // }
        if width_limit > full_width {
            jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;
            return Ok(());
        }

        let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let old_start: i32 = jvm.get_field(&this, "__wieTextFieldVisibleStart", "I").await?;

        let old_end: i32 = jvm.get_field(&this, "__wieTextFieldVisibleEnd", "I").await?;

        // -----------------------------------------------------------------
        // Caret moved left of the currently visible window.
        // 0x2452bc...
        // -----------------------------------------------------------------
        if caret < old_start {
            let mut new_start = caret.wrapping_sub(1);
            if new_start < 0 {
                new_start = 0;
            }

            let new_end = caret.wrapping_add(old_end).wrapping_sub(1).wrapping_sub(old_start);

            jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", new_start).await?;

            jvm.put_field(&mut this, "__wieTextFieldVisibleEnd", "I", new_end).await?;

            let adjusting: i32 = jvm.get_field(&this, "__wieTextFieldCursorAdjusting", "I").await?;

            if adjusting == 0 {
                return Ok(());
            }

            let mut accumulated = 0i32;

            if new_end >= new_start && char_count > new_start {
                if text.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                if font.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                let mut i = new_start;

                while i <= new_end && i < char_count {
                    let ch: JavaChar = jvm.invoke_virtual(&text, "charAt", "(I)C", (i,)).await?;

                    let cw: i32 = jvm.invoke_virtual(&font, "charWidth", "(C)I", (ch,)).await?;

                    accumulated = accumulated.wrapping_add(cw);
                    i = i.wrapping_add(1);
                }
            }

            // Native returns immediately if the current visible text
            // already exceeds viewportWidth.
            if viewport_width < accumulated {
                return Ok(());
            }

            // Native 0x2453e0..0x2454b0:
            // while visibleStart > 0, prepend characters until width
            // reaches componentWidth - 6. The character that reaches or
            // exceeds the limit is backed out again.
            let component_width: i32 = jvm.get_field(&this, "w", "I").await?;
            let component_limit = component_width.wrapping_sub(6);

            let mut start: i32 = jvm.get_field(&this, "__wieTextFieldVisibleStart", "I").await?;

            while start > 0 {
                start = start.wrapping_sub(1);

                jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", start).await?;

                if text.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                if font.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                let ch: JavaChar = jvm.invoke_virtual(&text, "charAt", "(I)C", (start,)).await?;

                let cw: i32 = jvm.invoke_virtual(&font, "charWidth", "(C)I", (ch,)).await?;

                accumulated = accumulated.wrapping_add(cw);

                if accumulated >= component_limit {
                    start = start.wrapping_add(1);

                    jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", start).await?;

                    return Ok(());
                }
            }

            jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;

            return Ok(());
        }

        // If the caret is still strictly inside the current right edge,
        // native makes no adjustment.
        if caret < old_end {
            return Ok(());
        }

        // -----------------------------------------------------------------
        // Caret reached/passed the right edge.
        // First test whether [visibleStart, charCount) already fits.
        // -----------------------------------------------------------------
        let remaining_count = char_count.wrapping_sub(old_start);

        let remaining_width = if remaining_count == 0 {
            0i32
        } else {
            if text.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            if font.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            // Native @ 0x245504:
            // Font.substringWidth(text, visibleStart,
            //                     charCount - visibleStart)
            jvm.invoke_virtual(
                &font,
                "substringWidth",
                "(Ljava/lang/String;II)I",
                (text.clone(), old_start, remaining_count),
            )
            .await?
        };

        if width_limit > remaining_width {
            return Ok(());
        }

        // visibleEnd = caret - 1, clamped to charCount - 1.
        let mut visible_end = caret.wrapping_sub(1);

        if visible_end >= char_count {
            visible_end = char_count.wrapping_sub(1);
        }

        jvm.put_field(&mut this, "__wieTextFieldVisibleEnd", "I", visible_end).await?;

        if visible_end < 0 {
            return Ok(());
        }

        if text.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        if font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        // Native walks backwards from visibleEnd.
        // It continues only while accumulatedWidth < viewportWidth - 2.
        // Once the limit is reached, visibleStart becomes index + 1.
        let mut index = visible_end;
        let mut accumulated = 0i32;

        loop {
            let ch: JavaChar = jvm.invoke_virtual(&text, "charAt", "(I)C", (index,)).await?;

            let cw: i32 = jvm.invoke_virtual(&font, "charWidth", "(C)I", (ch,)).await?;

            accumulated = accumulated.wrapping_add(cw);

            let next_index = index.wrapping_sub(1);

            if accumulated >= width_limit {
                jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", index.wrapping_add(1)).await?;

                return Ok(());
            }

            if next_index < 0 {
                jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;

                return Ok(());
            }

            index = next_index;
        }
    }

    async fn notify_change(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextFieldComponent>) -> JvmResult<()> {
        // Native notifyChange_v0 @ 0x24586c.

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let char_count: i32 = if text.is_null() {
            0
        } else {
            jvm.invoke_virtual(&text, "length", "()I", ()).await?
        };

        let font: ClassInstanceRef<()> = jvm.get_field(&this, "__wieFont", "Lorg/kwis/msp/lcdui/Font;").await?;

        let full_width: i32 = if char_count == 0 {
            0
        } else {
            if text.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            if font.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            jvm.invoke_virtual(&font, "substringWidth", "(Ljava/lang/String;II)I", (text.clone(), 0i32, char_count))
                .await?
        };

        let viewport_width: i32 = jvm.get_field(&this, "__wieTextFieldViewportWidth", "I").await?;

        let width_limit = viewport_width.wrapping_sub(2);

        // 0x2458dc..0x2458f0
        if width_limit > full_width {
            jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;
            return Ok(());
        }

        // Native forces the caret to charCount whenever they differ.
        let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        if caret != char_count {
            jvm.put_field(&mut this, "m_cPos", "I", char_count).await?;
        }

        let visible_end: i32 = jvm.get_field(&this, "__wieTextFieldVisibleEnd", "I").await?;

        // 0x245908..0x24590c
        if char_count < visible_end {
            return Ok(());
        }

        let visible_start: i32 = jvm.get_field(&this, "__wieTextFieldVisibleStart", "I").await?;

        let remaining_count = char_count.wrapping_sub(visible_start);

        let remaining_width: i32 = if remaining_count == 0 {
            0
        } else {
            if text.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            if font.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            // Native @ 0x245938:
            // Font.substringWidth(text, visibleStart,
            //                     charCount - visibleStart)
            jvm.invoke_virtual(
                &font,
                "substringWidth",
                "(Ljava/lang/String;II)I",
                (text.clone(), visible_start, remaining_count),
            )
            .await?
        };

        // Native returns if the remaining substring still fits.
        if remaining_width < width_limit {
            return Ok(());
        }

        let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let mut new_end = caret.wrapping_sub(1);

        if new_end >= char_count {
            new_end = char_count.wrapping_sub(1);
        }

        jvm.put_field(&mut this, "__wieTextFieldVisibleEnd", "I", new_end).await?;

        if new_end < 0 {
            // Native reaches visibleStart = 0 through the backward-loop
            // terminal path when there are no characters.
            jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;
            return Ok(());
        }

        if text.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        if font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        // 0x245980..0x245a38:
        // Walk backwards from visibleEnd, accumulating char widths.
        let mut index = new_end;
        let mut accumulated = 0i32;

        loop {
            let ch: JavaChar = jvm.invoke_virtual(&text, "charAt", "(I)C", (index,)).await?;

            let cw: i32 = jvm.invoke_virtual(&font, "charWidth", "(C)I", (ch,)).await?;

            accumulated = accumulated.wrapping_add(cw);

            let next_index = index.wrapping_sub(1);

            if accumulated >= width_limit {
                jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", index.wrapping_add(1)).await?;

                return Ok(());
            }

            if next_index < 0 {
                jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", 0i32).await?;

                return Ok(());
            }

            index = next_index;
        }
    }

    async fn insert(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFieldComponent>,
        string: ClassInstanceRef<String>,
        offset: i32,
        length: i32,
        position: i32,
    ) -> JvmResult<()> {
        // Native insert_v0 @ 0x2460d0:
        // TextComponent.insert(string, offset, length, position);
        // repaint(0, 0, width, height);

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "insert",
                "(Ljava/lang/String;III)V",
                (string, offset, length, position),
            )
            .await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;

        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0i32, 0i32, width, height)).await?;

        Ok(())
    }

    async fn focus_notify(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextFieldComponent>, focus: bool) -> JvmResult<()> {
        // Native focusNotify_v0 @ 0x2442b0.

        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "focusNotify", "(Z)V", (focus,))
            .await?;

        if !focus {
            return Ok(());
        }

        // Clamp m_cPos to [0, charCount].
        let mut caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let char_count: i32 = if text.is_null() {
            0
        } else {
            jvm.invoke_virtual(&text, "length", "()I", ()).await?
        };

        if caret <= 0 {
            caret = 0;

            jvm.put_field(&mut this, "m_cPos", "I", caret).await?;
        } else if caret >= char_count {
            caret = char_count;

            jvm.put_field(&mut this, "m_cPos", "I", caret).await?;
        }

        let mode_viewer: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieModeViewer", "Lorg/kwis/msp/lwc/TextComponent$ModeViewer;")
            .await?;

        // Native virtual slot +0xa4:
        // Component.getXOnScreen()I
        let x: i32 = jvm.invoke_virtual(&this, "getXOnScreen", "()I", ()).await?;

        // Native virtual slot +0xfc:
        // TextComponent.countModeYPos()I
        let y: i32 = jvm.invoke_virtual(&this, "countModeYPos", "()I", ()).await?;

        if mode_viewer.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        // Native ModeViewer vslot +0x2c:
        // inherited Card.move(x, y)
        let _: () = jvm.invoke_virtual(&mode_viewer, "move", "(II)V", (x, y)).await?;

        Ok(())
    }

    async fn control_popup(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextFieldComponent>) -> JvmResult<()> {
        // Native controlPopup_v0 @ 0x244358.

        // new ShellComponent()
        let shell = jvm.new_class("org/kwis/msp/lwc/ShellComponent", "()V", ()).await?;

        // +0xa8 = shell
        jvm.put_field(&mut this, "__wieTextFieldPopup", "Lorg/kwis/msp/lwc/ShellComponent;", shell.clone())
            .await?;

        // new FormComponent()
        let form = jvm.new_class("org/kwis/msp/lwc/FormComponent", "()V", ()).await?;

        // this.getString() -- native vslot +0xc4
        let text: ClassInstanceRef<String> = jvm.invoke_virtual(&this, "getString", "()Ljava/lang/String;", ()).await?;

        // this.getConstraint() -- native vslot +0xd0
        let constraint: i32 = jvm.invoke_virtual(&this, "getConstraint", "()I", ()).await?;

        let mode: i32 = jvm.get_field(&this, "iMode", "I").await?;

        let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        // new TextFieldComponent$TextPopup(
        //     this, getString(), getConstraint(), iMode, m_cPos)
        let popup = jvm
            .new_class(
                "org/kwis/msp/lwc/TextFieldComponent$TextPopup",
                "(Lorg/kwis/msp/lwc/TextFieldComponent;Ljava/lang/String;III)V",
                (this.clone(), text, constraint, mode, caret),
            )
            .await?;

        // this.getMaxLength() -- native vslot +0xc0
        let max_length: i32 = jvm.invoke_virtual(&this, "getMaxLength", "()I", ()).await?;

        // popup.setMaxLength(maxLength)
        let _: () = jvm.invoke_virtual(&popup, "setMaxLength", "(I)V", (max_length,)).await?;

        // form.addComponent(popup)
        let _: i32 = jvm
            .invoke_virtual(&form, "addComponent", "(Lorg/kwis/msp/lwc/Component;)I", (popup.clone(),))
            .await?;

        // popup.setWide(true, action)
        let action: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextFieldAction", "Ljava/lang/Object;").await?;

        let _: () = jvm
            .invoke_virtual(&popup, "setWide", "(ZLorg/kwis/msp/lwc/ActionListener;)V", (true, action))
            .await?;

        // shell.useFrame(true)
        let current_shell: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextFieldPopup", "Lorg/kwis/msp/lwc/ShellComponent;").await?;

        if current_shell.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm.invoke_virtual(&current_shell, "useFrame", "(Z)V", (true,)).await?;

        // shell.setTitle("Wide Editor")
        let title: ClassInstanceRef<String> = jvm.intern_string("Wide Editor").await?.into();

        let current_shell: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextFieldPopup", "Lorg/kwis/msp/lwc/ShellComponent;").await?;

        if current_shell.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm.invoke_virtual(&current_shell, "setTitle", "(Ljava/lang/String;)V", (title,)).await?;

        // shell.addComponent(form)
        let current_shell: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextFieldPopup", "Lorg/kwis/msp/lwc/ShellComponent;").await?;

        if current_shell.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: i32 = jvm
            .invoke_virtual(&current_shell, "addComponent", "(Lorg/kwis/msp/lwc/Component;)I", (form,))
            .await?;

        // shell.show()
        let current_shell: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextFieldPopup", "Lorg/kwis/msp/lwc/ShellComponent;").await?;

        if current_shell.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm.invoke_virtual(&current_shell, "show", "()V", ()).await?;

        Ok(())
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponent>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        // Native keyNotify_v0 @ 0x244510.
        //
        // Display.getGameAction(key) is always called before
        // dispatching on event_type.
        let action: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getGameAction", "(I)I", (key,)).await?;

        // Native types 1/2/3 share the main path.
        if matches!(event_type, 1 | 2 | 3) {
            let handled: bool = jvm
                .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (event_type, key))
                .await?;

            if handled {
                return Ok(true);
            }

            // ----------------------------------------------------------
            // RIGHT -- Display action 5
            // ----------------------------------------------------------
            if action == 5 {
                // Native key-repeat boundary rule:
                // return max(1 - boundaryHit, 0).
                if event_type == 2 {
                    let boundary_hit: i32 = jvm.get_field(&this, "__wieTextFieldBoundaryHit", "I").await?;

                    return Ok(boundary_hit == 0);
                }

                // Native always sends type=1,key=-99 here for type 1/3.
                let _: bool = jvm
                    .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (1i32, -99i32))
                    .await?;

                jvm.put_field(&mut this, "__wieTextState70", "I", key).await?;

                let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;
                let char_count: i32 = {
                    let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

                    if text.is_null() {
                        0
                    } else {
                        jvm.invoke_virtual(&text, "length", "()I", ()).await?
                    }
                };

                let new_caret = caret.wrapping_add(1);

                jvm.put_field(&mut this, "m_cPos", "I", new_caret).await?;

                if new_caret > char_count {
                    jvm.put_field(&mut this, "__wieTextFieldBoundaryHit", "I", 1i32).await?;

                    jvm.put_field(&mut this, "m_cPos", "I", char_count).await?;

                    return Ok(false);
                }

                jvm.put_field(&mut this, "__wieTextFieldBoundaryHit", "I", 0i32).await?;

                Self::notify_change_position(jvm, this.clone()).await?;

                let width: i32 = jvm.get_field(&this, "w", "I").await?;
                let height: i32 = jvm.get_field(&this, "h", "I").await?;

                let _: () = jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0i32, 0i32, width, height)).await?;

                return Ok(true);
            }

            // ----------------------------------------------------------
            // LEFT -- Display action 2
            // ----------------------------------------------------------
            if action == 2 {
                if event_type == 2 {
                    let boundary_hit: i32 = jvm.get_field(&this, "__wieTextFieldBoundaryHit", "I").await?;

                    return Ok(boundary_hit == 0);
                }

                let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

                // Native sends the -99 notification only if caret > 0.
                if caret > 0 {
                    let _: bool = jvm
                        .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (1i32, -99i32))
                        .await?;
                }

                // Reload caret after the parent call, as native does.
                let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

                let new_caret = caret.wrapping_sub(1);

                jvm.put_field(&mut this, "m_cPos", "I", new_caret).await?;

                jvm.put_field(&mut this, "__wieTextState70", "I", key).await?;

                if new_caret < 0 {
                    jvm.put_field(&mut this, "__wieTextFieldBoundaryHit", "I", 1i32).await?;

                    jvm.put_field(&mut this, "m_cPos", "I", 0i32).await?;

                    return Ok(false);
                }

                jvm.put_field(&mut this, "__wieTextFieldBoundaryHit", "I", 0i32).await?;

                Self::notify_change_position(jvm, this.clone()).await?;

                let width: i32 = jvm.get_field(&this, "w", "I").await?;
                let height: i32 = jvm.get_field(&this, "h", "I").await?;

                let _: () = jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0i32, 0i32, width, height)).await?;

                return Ok(true);
            }

            // ----------------------------------------------------------
            // SELECT -- Display action 8
            // ----------------------------------------------------------
            if action == 8 {
                // For key-repeat/release native consumes the event.
                if event_type != 1 {
                    return Ok(true);
                }

                let im_handler: ClassInstanceRef<()> = jvm.get_field(&this, "imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;").await?;

                if im_handler.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                let mode: i32 = jvm.invoke_virtual(&im_handler, "getCurrentMode", "()I", ()).await?;

                // Native mode 99 path returns the existing false result.
                if mode == 99 {
                    return Ok(false);
                }

                let _: bool = jvm
                    .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (event_type, -99i32))
                    .await?;

                // Native invokes this virtually through slot +0x108.
                let _: () = jvm.invoke_virtual(&this, "controlPopup", "()V", ()).await?;

                return Ok(true);
            }

            return Ok(false);
        }

        // Native event type 4 always delegates, discards the result,
        // and returns true.
        if event_type == 4 {
            let _: bool = jvm
                .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (event_type, key))
                .await?;

            return Ok(true);
        }

        Ok(false)
    }

    async fn get_preferred_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextFieldComponent>) -> JvmResult<i32> {
        // Native getPreferredHeight_v0 @ 0x244a28:
        // normal path returns field +0x94 directly.
        let height: i32 = jvm.get_field(&this, "__wieTextFieldPreferredHeight", "I").await?;

        Ok(height)
    }

    async fn get_preferred_height_with_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFieldComponent>,
        _width: i32,
    ) -> JvmResult<i32> {
        // Native getPreferredHeight_v1 @ 0x2448b8:
        // normal path ignores the supplied width and virtually invokes
        // this.getPreferredHeight().
        let height: i32 = jvm.invoke_virtual(&this, "getPreferredHeight", "()I", ()).await?;

        Ok(height)
    }

    async fn get_preferred_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextFieldComponent>) -> JvmResult<i32> {
        // Native getPreferredWidth_v0 @ 0x244b10.

        let preferred_width: i32 = jvm.get_field(&this, "__wieTextFieldPreferredWidth", "I").await?;

        if preferred_width >= 0 {
            return Ok(preferred_width);
        }

        // Native Component +0x10 = parent.
        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if !parent.is_null() {
            // Native parent virtual slot +0x2c:
            // inherited Component.getWidth().
            let width: i32 = jvm.invoke_virtual(&parent, "getWidth", "()I", ()).await?;

            return Ok(width);
        }

        // Native TextComponent +0x50 is the Display supplied to
        // TextComponent(Display, int).
        let display: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextDisplay", "Lorg/kwis/msp/lcdui/Display;").await?;

        if display.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        // Native Display virtual slot +0x64 = getWidth().
        let width: i32 = jvm.invoke_virtual(&display, "getWidth", "()I", ()).await?;

        Ok(width)
    }

    async fn paint_content(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponent>,
        graphics: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native TextFieldComponent.paintContent_v0
        // @ 0x245a94..0x2460d0.
        //
        // Config/logging-only branches are intentionally omitted.

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        // Native +0xa0 = component width - 6.
        let viewport_width = width.wrapping_sub(6);
        jvm.put_field(&mut this, "__wieTextFieldViewportWidth", "I", viewport_width).await?;

        let visible_start: i32 = jvm.get_field(&this, "__wieTextFieldVisibleStart", "I").await?;

        if graphics.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        // Decorator +0x4c = RGB(255,255,255).
        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0x00ff_ffffi32,)).await?;

        // fillRect(0, 0, width, height)
        let _: () = jvm.invoke_virtual(&graphics, "fillRect", "(IIII)V", (0i32, 0i32, width, height)).await?;

        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;

        // Native border color:
        //   focused/selected(mask & 2): Decorator +0x50 = RGB(210,0,0)
        //   otherwise:                Decorator +0x44 = RGB(100,100,210)
        let border_color = if mask & 2 != 0 { 0x00d2_0000i32 } else { 0x0064_64d2i32 };

        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (border_color,)).await?;

        // Native uses Graphics.getFont() for the outer field width.
        let graphics_font: ClassInstanceRef<()> = jvm.invoke_virtual(&graphics, "getFont", "()Lorg/kwis/msp/lcdui/Font;", ()).await?;

        if graphics_font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        if text.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let text_width: i32 = jvm
            .invoke_virtual(&graphics_font, "stringWidth", "(Ljava/lang/String;)I", (text.clone(),))
            .await?;

        // Native: min(stringWidth(text) + 6, width - 1)
        let field_width = text_width.wrapping_add(6).min(width.wrapping_sub(1));

        // fillRoundRect(0, 0, fieldWidth, height - 1, 3, 3)
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "fillRoundRect",
                "(IIIIII)V",
                (0i32, 0i32, field_width, height.wrapping_sub(1), 3i32, 3i32),
            )
            .await?;

        // Native reads Component +0x18 (Rust "bg").
        // If negative, Decorator +0x54 = RGB(0,0,0).
        let configured_bg: i32 = jvm.get_field(&this, "bg", "I").await?;

        let normal_fg = if configured_bg >= 0 { configured_bg } else { 0x0000_0000i32 };

        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (normal_fg,)).await?;

        let char_count: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

        let font: ClassInstanceRef<()> = jvm.get_field(&this, "__wieFont", "Lorg/kwis/msp/lcdui/Font;").await?;

        if font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let constraint: i32 = jvm.get_field(&this, "__wieConstraint", "I").await?;

        let inverse: i32 = jvm.get_field(&this, "__wieTextFieldFlagAC", "I").await?;

        // Native text origin starts at x=3; drawChar uses x+1.
        let mut x = 3i32;
        let mut accumulated = 0i32;
        let mut index = visible_start;

        while index < char_count {
            let ch: JavaChar = jvm.invoke_virtual(&text, "charAt", "(I)C", (index,)).await?;

            let char_width: i32 = jvm.invoke_virtual(&font, "charWidth", "(C)I", (ch,)).await?;

            accumulated = accumulated.wrapping_add(char_width);

            // Native stops before drawing the character that would make
            // the accumulated width exceed +0xa0.
            if accumulated > viewport_width {
                break;
            }

            jvm.put_field(&mut this, "__wieTextFieldVisibleEnd", "I", index).await?;

            let draw_ch: JavaChar = if constraint == 2 { 42u16 } else { ch };

            let draw_x = x.wrapping_add(1);

            if inverse != 0 {
                // Decorator +0x54 = black.
                let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0x0000_0000i32,)).await?;

                let draw_width: i32 = jvm.invoke_virtual(&font, "charWidth", "(C)I", (draw_ch,)).await?;

                let font_height: i32 = jvm.invoke_virtual(&font, "getHeight", "()I", ()).await?;

                let _: () = jvm
                    .invoke_virtual(&graphics, "fillRect", "(IIII)V", (draw_x, 2i32, draw_width, font_height))
                    .await?;

                // Native fallback in inverse mode:
                // Decorator +0x4c = white.
                let inverse_fg = if configured_bg >= 0 { configured_bg } else { 0x00ff_ffffi32 };

                let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (inverse_fg,)).await?;
            }

            let _: () = jvm
                .invoke_virtual(&graphics, "drawChar", "(CIII)V", (draw_ch, draw_x, 2i32, 4i32))
                .await?;

            // If inverse rendering changed the color, native restores the
            // normal text color before continuing.
            if inverse != 0 {
                let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (normal_fg,)).await?;
            }

            // Focused caret inside the visible character range.
            if mask & 2 != 0 {
                let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

                if index == caret {
                    let font_height: i32 = jvm.invoke_virtual(&font, "getHeight", "()I", ()).await?;

                    let _: () = jvm
                        .invoke_virtual(&graphics, "drawLine", "(IIII)V", (x, 2i32, x, font_height.wrapping_add(2)))
                        .await?;
                }
            }

            x = x.wrapping_add(char_width);
            index = index.wrapping_add(1);
        }

        // Native final-caret path.
        if mask & 2 != 0 {
            let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

            let visible_end: i32 = jvm.get_field(&this, "__wieTextFieldVisibleEnd", "I").await?;

            if caret >= char_count || caret == visible_end.wrapping_add(1) {
                let font_height: i32 = jvm.invoke_virtual(&font, "getHeight", "()I", ()).await?;

                let _: () = jvm
                    .invoke_virtual(&graphics, "drawLine", "(IIII)V", (x, 2i32, x, font_height.wrapping_add(2)))
                    .await?;
            }
        }

        Ok(())
    }

    async fn access_102(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextFieldComponent>, value: i32) -> JvmResult<i32> {
        // Native access$102: +0x8c = value; return value.
        jvm.put_field(&mut this, "__wieTextFieldVisibleStart", "I", value).await?;

        Ok(value)
    }

    async fn access_000(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextFieldComponent>) -> JvmResult<i32> {
        // Native access$000: return +0x90.
        jvm.get_field(&this, "__wieTextFieldVisibleEnd", "I").await
    }

    async fn access_002(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextFieldComponent>, value: i32) -> JvmResult<i32> {
        // Native access$002: +0x90 = value; return value.
        jvm.put_field(&mut this, "__wieTextFieldVisibleEnd", "I", value).await?;

        Ok(value)
    }
}
