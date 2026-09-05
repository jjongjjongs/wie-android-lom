use alloc::{boxed::Box, vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::TextComponent;

// class org.kwis.msp.lwc.TextBoxComponent
pub struct TextBoxComponent;

impl TextBoxComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextBoxComponent",
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
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("focusNotify", "(Z)V", Self::focus_notify, Default::default()),
                JavaMethodProto::new("setMaxLength", "(I)V", Self::set_max_length, Default::default()),
                JavaMethodProto::new("setFont", "(Lorg/kwis/msp/lcdui/Font;)V", Self::set_font, Default::default()),
                JavaMethodProto::new("setString", "(Ljava/lang/String;)V", Self::set_string, Default::default()),
                JavaMethodProto::new("setString", "(Ljava/lang/String;I)V", Self::set_string_with_position, Default::default()),
                JavaMethodProto::new("controlPopup", "()V", Self::control_popup, Default::default()),
                JavaMethodProto::new("delete", "(II)V", Self::delete, Default::default()),
                JavaMethodProto::new("insert", "(Ljava/lang/String;III)V", Self::insert, Default::default()),
                JavaMethodProto::new("getPreferredWidth", "()I", Self::get_preferred_width, Default::default()),
                JavaMethodProto::new("getPreferredHeight", "()I", Self::get_preferred_height, Default::default()),
                JavaMethodProto::new("getPreferredHeight", "(I)I", Self::get_preferred_height_with_width, Default::default()),
                JavaMethodProto::new("configure", "(IIIII)V", Self::configure, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("__wieTextBoxValue", "I", Default::default()),
                JavaFieldProto::new("__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;", Default::default()),
                JavaFieldProto::new("__wieTextBoxFlag94", "I", Default::default()),
                JavaFieldProto::new("__wieTextBoxFlag98", "I", Default::default()),
                JavaFieldProto::new("__wieTextBoxFlag9c", "I", Default::default()),
                JavaFieldProto::new("__wieTextBoxPopup", "Lorg/kwis/msp/lwc/ShellComponent;", Default::default()),
                JavaFieldProto::new("__wieTextBoxAction", "Ljava/lang/Object;", Default::default()),
                JavaFieldProto::new("__wieTextBoxFlagA8", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn insert(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextBoxComponent>,
        string: ClassInstanceRef<String>,
        offset: i32,
        length: i32,
        position: i32,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "insert",
                "(Ljava/lang/String;III)V",
                (string, offset, length, position),
            )
            .await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let chars: ClassInstanceRef<()> = jvm.invoke_virtual(&text, "toCharArray", "()[C", ()).await?;

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: i32 = jvm.invoke_virtual(&formatter, "setData", "([CI)I", (chars, position)).await?;

        let cursor: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let _: () = jvm.invoke_virtual(&formatter, "setCurrent", "(I)V", (cursor,)).await?;

        let current_line: i32 = jvm.invoke_virtual(&formatter, "getCurLine", "()I", ()).await?;

        Self::check_scroll(jvm, this.clone(), current_line).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn delete(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>, position: i32, length: i32) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "delete", "(II)V", (position, length))
            .await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let chars: ClassInstanceRef<()> = jvm.invoke_virtual(&text, "toCharArray", "()[C", ()).await?;

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: i32 = jvm.invoke_virtual(&formatter, "setData", "([CI)I", (chars, position)).await?;

        let cursor: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let _: () = jvm.invoke_virtual(&formatter, "setCurrent", "(I)V", (cursor,)).await?;

        let current_line: i32 = jvm.invoke_virtual(&formatter, "getCurLine", "()I", ()).await?;

        Self::check_scroll(jvm, this.clone(), current_line).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn set_string(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>, data: ClassInstanceRef<String>) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "setString", "(Ljava/lang/String;)V", (data,))
            .await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let chars: ClassInstanceRef<()> = jvm.invoke_virtual(&text, "toCharArray", "()[C", ()).await?;

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if !formatter.is_null() {
            let _: i32 = jvm.invoke_virtual(&formatter, "setData", "([C)I", (chars,)).await?;

            let _: () = jvm.invoke_virtual(&formatter, "setCurrent", "(I)V", (0i32,)).await?;

            let current_line: i32 = jvm.invoke_virtual(&formatter, "getCurLine", "()I", ()).await?;

            Self::check_scroll(jvm, this.clone(), current_line).await?;
        }

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn set_string_with_position(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextBoxComponent>,
        data: ClassInstanceRef<String>,
        position: i32,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "setString", "(Ljava/lang/String;)V", (data,))
            .await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        let chars: ClassInstanceRef<()> = jvm.invoke_virtual(&text, "toCharArray", "()[C", ()).await?;

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: i32 = jvm.invoke_virtual(&formatter, "setData", "([C)I", (chars,)).await?;

        jvm.put_field(&mut this, "m_cPos", "I", position).await?;

        let _: () = jvm.invoke_virtual(&formatter, "setCurrent", "(I)V", (position,)).await?;

        let current_line: i32 = jvm.invoke_virtual(&formatter, "getCurLine", "()I", ()).await?;

        Self::check_scroll(jvm, this.clone(), current_line).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn check_position(jvm: &Jvm, this: ClassInstanceRef<TextBoxComponent>, line: i32, direction: i32) -> JvmResult<bool> {
        let font: ClassInstanceRef<()> = jvm.get_field(&this, "__wieFont", "Lorg/kwis/msp/lcdui/Font;").await?;

        if font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let font_height: i32 = jvm.invoke_virtual(&font, "getHeight", "()I", ()).await?;

        let line_height = font_height.wrapping_add(2);
        let caret_bottom = line.wrapping_add(1).wrapping_mul(line_height);

        let screen_y: i32 = jvm.invoke_virtual(&this, "getYOnScreen", "()I", ()).await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0, 0, width, height)).await?;

        let absolute_bottom = caret_bottom.wrapping_add(screen_y);

        if direction == 11 {
            let viewport_y: i32 = jvm.get_field(&this, "__wieViewportY", "I").await?;

            Ok(absolute_bottom >= viewport_y)
        } else if direction == -11 {
            let viewport_height: i32 = jvm.get_field(&this, "__wieViewportHeight", "I").await?;
            let viewport_y: i32 = jvm.get_field(&this, "__wieViewportY", "I").await?;

            Ok(absolute_bottom <= viewport_y.wrapping_add(viewport_height))
        } else {
            Ok(true)
        }
    }

    async fn check_scroll(jvm: &Jvm, this: ClassInstanceRef<TextBoxComponent>, current_line: i32) -> JvmResult<()> {
        let font: ClassInstanceRef<()> = jvm.get_field(&this, "__wieFont", "Lorg/kwis/msp/lcdui/Font;").await?;

        if font.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let font_height: i32 = jvm.invoke_virtual(&font, "getHeight", "()I", ()).await?;

        let line_height = font_height.wrapping_add(2);

        let this_y: i32 = jvm.invoke_virtual(&this, "getYOnScreen", "()I", ()).await?;

        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if parent.is_null() {
            return Ok(());
        }

        // Native MLA at 0x233b4c:
        // (current_line + 1) * (fontHeight + 2) + thisY.
        let caret_bottom = this_y.wrapping_add(current_line.wrapping_add(1).wrapping_mul(line_height));

        let parent_y: i32 = jvm.invoke_virtual(&parent, "getYOnScreen", "()I", ()).await?;

        let viewport_height: i32 = jvm.get_field(&this, "__wieViewportParentHeight", "I").await?;

        let target_y = if caret_bottom <= parent_y {
            caret_bottom.wrapping_sub(parent_y).wrapping_sub(line_height)
        } else if caret_bottom > parent_y.wrapping_add(viewport_height) {
            caret_bottom.wrapping_sub(viewport_height).wrapping_sub(parent_y)
        } else {
            0
        };

        let _: bool = jvm.invoke_virtual(&parent, "scrollTo", "(II)Z", (0i32, target_y)).await?;

        Ok(())
    }

    async fn get_preferred_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>) -> JvmResult<i32> {
        let width: i32 = jvm.get_field(&this, "w", "I").await?;

        jvm.invoke_virtual(&this, "getPreferredHeight", "(I)I", (width,)).await
    }

    async fn get_preferred_height_with_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextBoxComponent>,
        width: i32,
    ) -> JvmResult<i32> {
        let display: ClassInstanceRef<()> = jvm
            .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
            .await?;

        if display.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let display_height: i32 = jvm.invoke_virtual(&display, "getHeight", "()I", ()).await?;

        let effective_width = if width < 0 { jvm.get_field(&this, "w", "I").await? } else { width };

        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        let parent_height: i32 = if parent.is_null() { 0 } else { jvm.get_field(&parent, "h", "I").await? };

        let available_height = if parent.is_null() { 10 } else { parent_height.wrapping_sub(4) };

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let refresh_cached_height: i32 = jvm.get_field(&this, "__wieTextBoxFlag98", "I").await?;

        // Native +0x98 controls refresh of the +0x8c cached height.
        // This is independent from +0x94, which tracks the narrowed
        // formatter-width mode.
        if refresh_cached_height != 0 {
            jvm.put_field(&mut this, "__wieTextBoxValue", "I", parent_height).await?;
        }

        let mut narrow_mode: i32 = jvm.get_field(&this, "__wieTextBoxFlag94", "I").await?;

        let base_width = effective_width.wrapping_sub(6);

        let mut calculated: i32 = jvm
            .invoke_virtual(&formatter, "setWHSize", "(II)I", (base_width, available_height))
            .await?;
        calculated = calculated.wrapping_add(2);

        let component_height: i32 = jvm.get_field(&this, "h", "I").await?;

        if component_height > display_height {
            if display_height >= calculated {
                narrow_mode = 0;
                jvm.put_field(&mut this, "__wieTextBoxFlag94", "I", narrow_mode).await?;

                calculated = jvm
                    .invoke_virtual(&formatter, "setWHSize", "(II)I", (base_width, available_height))
                    .await?;
                calculated = calculated.wrapping_add(2);
            }
        } else if display_height < calculated && narrow_mode == 0 {
            calculated = jvm
                .invoke_virtual(&formatter, "setWHSize", "(II)I", (base_width.wrapping_sub(5), available_height))
                .await?;
            calculated = calculated.wrapping_add(2);

            narrow_mode = 1;
            jvm.put_field(&mut this, "__wieTextBoxFlag94", "I", narrow_mode).await?;
        }

        let cached: i32 = jvm.get_field(&this, "__wieTextBoxValue", "I").await?;

        Ok(if calculated >= cached { calculated } else { cached })
    }

    async fn get_preferred_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>) -> JvmResult<i32> {
        let mut parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        let mut preferred_width = 0i32;

        while !parent.is_null() {
            let width: i32 = jvm.get_field(&parent, "w", "I").await?;

            if preferred_width < width {
                preferred_width = width;
            }

            parent = jvm.get_field(&parent, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;
        }

        Ok(preferred_width)
    }

    async fn configure(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextBoxComponent>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: i32,
    ) -> JvmResult<()> {
        // Native TextBoxComponent.configure invokes Component.configure
        // directly, then refreshes TextComponent's viewport intersection.
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/Component", "configure", "(IIIII)V", (x, y, w, h, flags))
            .await?;

        let raw: Box<dyn jvm::ClassInstance> = this.into();
        let this: ClassInstanceRef<TextComponent> = raw.into();

        TextComponent::calc_view_port_area(jvm, this).await
    }

    async fn set_font(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>, font: ClassInstanceRef<()>) -> JvmResult<()> {
        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm
            .invoke_virtual(&formatter, "setFont", "(Lorg/kwis/msp/lcdui/Font;)V", (font.clone(),))
            .await?;

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "setFont",
                "(Lorg/kwis/msp/lcdui/Font;)V",
                (font,),
            )
            .await?;

        Ok(())
    }

    async fn set_max_length(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>, max_length: i32) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "setMaxLength", "(I)V", (max_length,))
            .await?;

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let position: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let _: () = jvm.invoke_virtual(&formatter, "setCurrent", "(I)V", (position,)).await?;

        Ok(())
    }

    async fn control_popup(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextBoxComponent>) -> JvmResult<()> {
        let shell = jvm.new_class("org/kwis/msp/lwc/ShellComponent", "()V", ()).await?;

        jvm.put_field(&mut this, "__wieTextBoxPopup", "Lorg/kwis/msp/lwc/ShellComponent;", shell.clone())
            .await?;

        let form = jvm.new_class("org/kwis/msp/lwc/FormComponent", "()V", ()).await?;

        let text: ClassInstanceRef<String> = jvm.invoke_virtual(&this, "getString", "()Ljava/lang/String;", ()).await?;

        let constraint: i32 = jvm.invoke_virtual(&this, "getConstraint", "()I", ()).await?;

        let caret: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let mode: i32 = jvm.get_field(&this, "iMode", "I").await?;

        let popup = jvm
            .new_class("org/kwis/msp/lwc/TextPopup", "(Ljava/lang/String;III)V", (text, constraint, mode, caret))
            .await?;

        let max_length: i32 = jvm.invoke_virtual(&this, "getMaxLength", "()I", ()).await?;

        let _: () = jvm.invoke_virtual(&popup, "setMaxLength", "(I)V", (max_length,)).await?;

        let _: i32 = jvm
            .invoke_virtual(&form, "addComponent", "(Lorg/kwis/msp/lwc/Component;)I", (popup.clone(),))
            .await?;

        let action: ClassInstanceRef<()> = jvm.get_field(&this, "__wieTextBoxAction", "Ljava/lang/Object;").await?;

        let _: () = jvm
            .invoke_virtual(&popup, "setWide", "(ZLorg/kwis/msp/lwc/ActionListener;)V", (true, action))
            .await?;

        let _: () = jvm.invoke_virtual(&shell, "useFrame", "(Z)V", (true,)).await?;

        let title: ClassInstanceRef<String> = JavaLangString::from_rust_string(jvm, "Wide Editor").await?.into();

        let _: () = jvm.invoke_virtual(&shell, "setTitle", "(Ljava/lang/String;)V", (title,)).await?;

        let _: i32 = jvm
            .invoke_virtual(&shell, "addComponent", "(Lorg/kwis/msp/lwc/Component;)I", (form,))
            .await?;

        let _: () = jvm.invoke_virtual(&shell, "show", "()V", ()).await?;

        Ok(())
    }

    async fn focus_notify(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextBoxComponent>, focus: bool) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "focusNotify", "(Z)V", (focus,))
            .await?;

        if !focus {
            return Ok(());
        }

        let mut this = this;
        let position: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

        let position = if position <= 0 {
            0
        } else if position >= length {
            length
        } else {
            position
        };

        jvm.put_field(&mut this, "m_cPos", "I", position).await?;

        Ok(())
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextBoxComponent>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        if event_type == 4 {
            let _: bool = jvm
                .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (event_type, key))
                .await?;
            return Ok(true);
        }

        if event_type != 1 && event_type != 2 && event_type != 3 {
            return Ok(false);
        }

        let handled: bool = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (event_type, key))
            .await?;

        if handled {
            return Ok(true);
        }

        let action: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getGameAction", "(I)I", (key,)).await?;

        if action == 8 {
            let popup_flag: i32 = jvm.get_field(&this, "__wieTextBoxFlag9c", "I").await?;

            if popup_flag != 0 {
                return Ok(false);
            }

            if event_type != 1 {
                return Ok(true);
            }

            let im_handler: ClassInstanceRef<()> = jvm.get_field(&this, "imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;").await?;

            if im_handler.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            let _: () = jvm.invoke_virtual(&im_handler, "changeCurrentModeToNext", "()V", ()).await?;

            jvm.put_field(&mut this, "__wieTextBoxFlag9c", "I", 1).await?;

            let mode: i32 = jvm.invoke_virtual(&im_handler, "getCurrentMode", "()I", ()).await?;

            if mode == 99 {
                return Ok(false);
            }

            let _: bool = jvm
                .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (1, -99))
                .await?;

            let _: () = jvm.invoke_virtual(&this, "controlPopup", "()V", ()).await?;

            return Ok(true);
        }

        if action != 1 && action != 2 && action != 5 && action != 6 {
            return Ok(false);
        }

        let flag_a8: i32 = jvm.get_field(&this, "__wieTextBoxFlagA8", "I").await?;

        // Native key-release path does not move the cursor. It reports
        // whether the preceding movement remained inside the viewport.
        if event_type == 2 {
            return Ok(flag_a8 == 0);
        }

        // Native sends a synthetic press/-99 pair through TextComponent
        // before applying TextBox's cursor movement.
        let _: bool = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "keyNotify", "(II)Z", (1, -99))
            .await?;

        let formatter: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieTextFormatter", "Lorg/kwis/msp/lwc/TextFormatProcessor;")
            .await?;

        if formatter.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let old_position: i32 = jvm.get_field(&this, "m_cPos", "I").await?;
        let char_count: i32 = {
            let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
            jvm.invoke_virtual(&text, "length", "()I", ()).await?
        };

        let new_position = match action {
            1 => jvm.invoke_virtual(&formatter, "getUpDownPosition", "(II)I", (old_position, -1)).await?,
            2 => old_position.wrapping_sub(1),
            5 => old_position.wrapping_add(1),
            6 => jvm.invoke_virtual(&formatter, "getUpDownPosition", "(II)I", (old_position, 1)).await?,
            _ => unreachable!(),
        };

        jvm.put_field(&mut this, "m_cPos", "I", new_position).await?;

        if new_position < 0 {
            jvm.put_field(&mut this, "__wieTextBoxFlagA8", "I", 1).await?;
            jvm.put_field(&mut this, "m_cPos", "I", old_position).await?;
            return Ok(false);
        }

        if new_position > char_count {
            jvm.put_field(&mut this, "__wieTextBoxFlagA8", "I", 1).await?;
            jvm.put_field(&mut this, "m_cPos", "I", char_count).await?;
            return Ok(false);
        }

        jvm.put_field(&mut this, "__wieTextBoxFlagA8", "I", 0).await?;

        let position: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let _: () = jvm.invoke_virtual(&formatter, "setCurrent", "(I)V", (position,)).await?;

        let line: i32 = jvm.invoke_virtual(&formatter, "getCurLine", "()I", ()).await?;

        let direction = match action {
            1 | 2 => 11,
            5 | 6 => -11,
            _ => unreachable!(),
        };

        let result = Self::check_position(jvm, this.clone(), line, direction).await?;

        jvm.put_field(&mut this, "__wieTextBoxFlagA8", "I", if result { 0 } else { 1 }).await?;

        Ok(result)
    }

    async fn init_with_display(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextBoxComponent>,
        display: ClassInstanceRef<()>,
        data: ClassInstanceRef<String>,
        constraint: i32,
    ) -> JvmResult<()> {
        // Native TextBoxComponent(Display,String,int) @ 0x236094:
        // invoke TextComponent(Display,constraint) directly.
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "<init>",
                "(Lorg/kwis/msp/lcdui/Display;I)V",
                (display, constraint),
            )
            .await?;

        // Native +0x8c = -1, +0x9c/+0x94/+0x98 = 0.
        jvm.put_field(&mut this, "__wieTextBoxValue", "I", -1).await?;

        jvm.put_field(&mut this, "__wieTextBoxFlag9c", "I", 0).await?;

        jvm.put_field(&mut this, "__wieTextBoxFlag94", "I", 0).await?;

        jvm.put_field(&mut this, "__wieTextBoxFlag98", "I", 0).await?;

        // Native new TextBoxComponent$Action(this).
        let action = jvm
            .new_class(
                "org/kwis/msp/lwc/TextBoxComponent$Action",
                "(Lorg/kwis/msp/lwc/TextBoxComponent;)V",
                (this.clone(),),
            )
            .await?;

        // Native +0xa8 = 0, +0xa4 = Action.
        jvm.put_field(&mut this, "__wieTextBoxFlagA8", "I", 0).await?;

        jvm.put_field(&mut this, "__wieTextBoxAction", "Ljava/lang/Object;", action).await?;

        // Native new TextFormatProcessor().
        let formatter = jvm.new_class("org/kwis/msp/lwc/TextFormatProcessor", "()V", ()).await?;

        jvm.put_field(
            &mut this,
            "__wieTextFormatter",
            "Lorg/kwis/msp/lwc/TextFormatProcessor;",
            formatter.clone(),
        )
        .await?;

        // Native formatter.setConstraints(constraint).
        let _: () = jvm.invoke_virtual(&formatter, "setConstraints", "(I)V", (constraint,)).await?;

        // Native this.setString(data) via TextComponent/TextBox vtable +0xb8.
        let _: () = jvm.invoke_virtual(&this, "setString", "(Ljava/lang/String;)V", (data,)).await?;

        Ok(())
    }

    async fn init(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<TextBoxComponent>,
        data: ClassInstanceRef<String>,
        constraint: i32,
    ) -> JvmResult<()> {
        // Default-display constructor delegates to the native Display-taking
        // initialization path.
        let display: ClassInstanceRef<()> = jvm
            .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
            .await?;

        Self::init_with_display(jvm, context, this, display, data, constraint).await
    }
}
