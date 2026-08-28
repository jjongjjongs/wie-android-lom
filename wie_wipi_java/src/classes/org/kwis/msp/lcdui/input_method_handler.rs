use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::FieldAccessFlags;
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult, runtime::JavaLangString};

use crate::classes::org::kwis::msp::lcdui::{CandidateWindow, Display};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lcdui.InputMethodHandler
pub struct InputMethodHandler;

impl InputMethodHandler {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/InputMethodHandler",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<clinit>", "()V", Self::cl_init, Default::default()),
                JavaMethodProto::new("<init>", "(I)V", Self::init, Default::default()),
                JavaMethodProto::new("setCurrentMode", "(I)Z", Self::set_current_mode, Default::default()),
                JavaMethodProto::new("changeCurrentModeToNext", "()V", Self::change_current_mode_to_next, Default::default()),
                JavaMethodProto::new("getCurrentMode", "()I", Self::get_current_mode, Default::default()),
                JavaMethodProto::new(
                    "getCurrentModeCode",
                    "()Ljava/lang/String;",
                    Self::get_current_mode_code,
                    Default::default(),
                ),
                JavaMethodProto::new("hideSymbolCard", "()V", Self::hide_symbol_card, Default::default()),
                JavaMethodProto::new("setSymbolPosition", "(IIII)V", Self::set_symbol_position, Default::default()),
                JavaMethodProto::new("notifyKeyInput", "(II)Z", Self::notify_key_input, Default::default()),
                JavaMethodProto::new(
                    "setInputMethodListener",
                    "(Lorg/kwis/msp/lcdui/InputMethodListener;)V",
                    Self::set_input_method_listener,
                    Default::default(),
                ),
                JavaMethodProto::new("notifyDataSelected", "([CII)V", Self::notify_data_selected, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("__wieSymbolModeCode", "Ljava/lang/String;", FieldAccessFlags::STATIC),
                JavaFieldProto::new("__wieKeyUp", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("__wieKeyDown", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("__wieKeyLeft", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("__wieKeyRight", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("__wieKeyClear", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("currentMode", "I", Default::default()),
                JavaFieldProto::new("__wieConstraint", "I", Default::default()),
                JavaFieldProto::new("__wieAllowedModes", "[I", Default::default()),
                JavaFieldProto::new("__wieSupportedModes", "[Ljava/lang/String;", Default::default()),
                JavaFieldProto::new("__wieSymbolCardActive", "Z", Default::default()),
                JavaFieldProto::new("__wieSymbolCard", "Lorg/kwis/msp/lcdui/CandidateWindow;", Default::default()),
                JavaFieldProto::new("__wieSymbolX", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolY", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolWidth", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolHeight", "I", Default::default()),
                JavaFieldProto::new("__wieInputMethodListener", "Lorg/kwis/msp/lcdui/InputMethodListener;", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn cl_init(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<()> {
        let symbol = JavaLangString::from_rust_string(jvm, "SYMBOL").await?;

        jvm.put_static_field(
            "org/kwis/msp/lcdui/InputMethodHandler",
            "__wieSymbolModeCode",
            "Ljava/lang/String;",
            symbol,
        )
        .await?;

        for (field, game_key) in [
            ("__wieKeyUp", 1),
            ("__wieKeyDown", 6),
            ("__wieKeyLeft", 2),
            ("__wieKeyRight", 5),
            ("__wieKeyClear", 99),
        ] {
            let key_code: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (game_key,)).await?;

            jvm.put_static_field("org/kwis/msp/lcdui/InputMethodHandler", field, "I", key_code)
                .await?;
        }

        Ok(())
    }

    async fn init(jvm: &Jvm, context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, constraint: i32) -> JvmResult<()> {
        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        let mut supported_modes = jvm.instantiate_array("Ljava/lang/String;", 4).await?;

        let en_short = JavaLangString::from_rust_string(jvm, "EN/S").await?;
        let en_long = JavaLangString::from_rust_string(jvm, "EN/L").await?;
        let numeric = JavaLangString::from_rust_string(jvm, "N123").await?;
        let korean = JavaLangString::from_rust_string(jvm, "KO").await?;

        jvm.store_array(&mut supported_modes, 0, vec![en_short, en_long, numeric, korean]).await?;

        jvm.put_field(&mut this, "__wieSupportedModes", "[Ljava/lang/String;", supported_modes)
            .await?;

        if !(0..=5).contains(&constraint) {
            return Err(jvm.exception("java/lang/IllegalArgumentException", "").await);
        }

        jvm.put_field(&mut this, "__wieConstraint", "I", constraint).await?;

        // Native InputMethodHandler builds an int[] of modes allowed by the
        // constructor constraint.  Constraints 3/4 intentionally allocate
        // int[4] and leave the last element at its default zero value.
        let allowed_values: &[i32] = match constraint {
            0 => &[0, 1, 2, 3, 99],
            1 | 2 | 5 => &[2],
            3 | 4 => &[0, 1, 2, 0],
            _ => unreachable!(),
        };

        let mut allowed_modes: ClassInstanceRef<Array<i32>> = jvm.instantiate_array("I", allowed_values.len()).await?.into();
        jvm.store_array(&mut allowed_modes, 0, allowed_values.iter().copied()).await?;

        let initial_mode = allowed_values[0];

        jvm.put_field(&mut this, "__wieAllowedModes", "[I", allowed_modes).await?;

        // Native constructor initializes through setCurrentMode(int).
        Self::set_current_mode(jvm, context, this.clone(), initial_mode).await?;

        Ok(())
    }

    async fn set_current_mode(jvm: &Jvm, context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, mode: i32) -> JvmResult<bool> {
        // Native accepts the supported-mode indices 0..3 and special symbol
        // mode 99.  Any other value throws IllegalArgumentException.
        if !(0..4).contains(&mode) && mode != 99 {
            return Err(jvm.exception("java/lang/IllegalArgumentException", "Invalid mode").await);
        }

        let previous_mode: i32 = jvm.get_field(&this, "currentMode", "I").await?;

        jvm.put_field(&mut this, "currentMode", "I", mode).await?;

        if mode == 99 {
            Self::show_symbol_card(jvm, &mut this).await?;
        } else {
            context.system().set_current_input_mode(mode as u32);

            if previous_mode == 99 {
                Self::remove_symbol_card(jvm, &mut this).await?;
            }
        }

        Ok(true)
    }

    async fn change_current_mode_to_next(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        let mode: i32 = jvm.get_field(&this, "currentMode", "I").await?;

        let allowed_modes: ClassInstanceRef<Array<i32>> = jvm.get_field(&this, "__wieAllowedModes", "[I").await?;
        let allowed_len = jvm.array_length(&allowed_modes).await?;
        let allowed_modes: alloc::vec::Vec<i32> = jvm.load_array(&allowed_modes, 0, allowed_len).await?;

        let Some(index) = allowed_modes.iter().position(|candidate| *candidate == mode) else {
            return Ok(());
        };

        let next = allowed_modes[(index + 1) % allowed_modes.len()];

        let _: bool = Self::set_current_mode(jvm, context, this, next).await?;

        Ok(())
    }

    async fn notify_key_input(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key: i32, event_type: i32) -> JvmResult<bool> {
        let listener: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieInputMethodListener", "Lorg/kwis/msp/lcdui/InputMethodListener;")
            .await?;

        // Native returns false immediately when no listener is installed.
        if listener.is_null() {
            return Ok(false);
        }

        // Native notifyKeyInput only processes press/release event types 1/3.
        let internal_event = match event_type {
            1 => 2,
            3 => 4,
            _ => return Ok(false),
        };

        let key_up: i32 = jvm.get_static_field("org/kwis/msp/lcdui/InputMethodHandler", "__wieKeyUp", "I").await?;
        let key_down: i32 = jvm.get_static_field("org/kwis/msp/lcdui/InputMethodHandler", "__wieKeyDown", "I").await?;
        let key_left: i32 = jvm.get_static_field("org/kwis/msp/lcdui/InputMethodHandler", "__wieKeyLeft", "I").await?;
        let key_right: i32 = jvm
            .get_static_field("org/kwis/msp/lcdui/InputMethodHandler", "__wieKeyRight", "I")
            .await?;
        let key_clear: i32 = jvm
            .get_static_field("org/kwis/msp/lcdui/InputMethodHandler", "__wieKeyClear", "I")
            .await?;

        // CLEAR is handled directly by native InputMethodHandler.
        if key == key_clear {
            let data: ClassInstanceRef<Array<JavaChar>> = ClassInstanceRef::new(None);
            let count = if event_type == 3 { -1 } else { 1 };

            let _: () = jvm.invoke_virtual(&listener, "notifyTextChanged", "([CII)V", (data, count, 1)).await?;

            return Ok(true);
        }

        // Directional keys are converted to the native flush sentinel.
        let normalized_key = if matches!(key, k if k == key_up || k == key_down || k == key_left || k == key_right) {
            -99
        } else {
            key
        };

        let result = context.system().handle_input_method(normalized_key as i8, internal_event);

        if result.output0_len != 0 {
            let mut bytes: ClassInstanceRef<Array<i8>> = jvm.instantiate_array("B", result.output0_len).await?.into();
            jvm.store_array(&mut bytes, 0, result.output0[..result.output0_len].iter().map(|byte| *byte as i8))
                .await?;

            let text: ClassInstanceRef<String> = jvm
                .new_class("java/lang/String", "([BII)V", (bytes, 0, result.output0_len as i32))
                .await?
                .into();
            let chars: ClassInstanceRef<Array<JavaChar>> = jvm.invoke_virtual(&text, "toCharArray", "()[C", ()).await?;
            let char_count = jvm.array_length(&chars).await? as i32;

            let _: () = jvm
                .invoke_virtual(&listener, "notifyTextChanged", "([CII)V", (chars, char_count, -1))
                .await?;
        }

        if result.output1_len != 0 {
            let mut bytes: ClassInstanceRef<Array<i8>> = jvm.instantiate_array("B", result.output1_len).await?.into();
            jvm.store_array(&mut bytes, 0, result.output1[..result.output1_len].iter().map(|byte| *byte as i8))
                .await?;

            let text: ClassInstanceRef<String> = jvm
                .new_class("java/lang/String", "([BII)V", (bytes, 0, result.output1_len as i32))
                .await?
                .into();
            let chars: ClassInstanceRef<Array<JavaChar>> = jvm.invoke_virtual(&text, "toCharArray", "()[C", ()).await?;
            let char_count = jvm.array_length(&chars).await? as i32;

            let _: () = jvm
                .invoke_virtual(&listener, "notifyTextChanged", "([CII)V", (chars, char_count, -1))
                .await?;
        }

        Ok(result.handled)
    }

    async fn set_input_method_listener(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        listener: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "__wieInputMethodListener",
            "Lorg/kwis/msp/lcdui/InputMethodListener;",
            listener,
        )
        .await
    }

    async fn notify_data_selected(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        data: ClassInstanceRef<Array<JavaChar>>,
        count: i32,
        change_type: i32,
    ) -> JvmResult<()> {
        let listener: ClassInstanceRef<()> = jvm
            .get_field(&this, "__wieInputMethodListener", "Lorg/kwis/msp/lcdui/InputMethodListener;")
            .await?;

        if listener.is_null() {
            return Ok(());
        }

        let callback_type = match change_type {
            0 => -1,
            1 | -1 => change_type,
            _ => return Ok(()),
        };

        jvm.invoke_virtual(&listener, "notifyTextChanged", "([CII)V", (data, count, callback_type))
            .await
    }

    async fn get_current_mode(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        jvm.get_field(&this, "currentMode", "I").await
    }

    async fn hide_symbol_card(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        Self::remove_symbol_card(jvm, &mut this).await
    }

    async fn set_symbol_position(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        let card: ClassInstanceRef<CandidateWindow> = jvm.get_field(&this, "__wieSymbolCard", "Lorg/kwis/msp/lcdui/CandidateWindow;").await?;

        if card.is_null() {
            return Ok(());
        }

        // Native validates geometry before dispatching CandidateWindow.setPosition.
        if x <= 0 || y <= 0 || width <= 0 || height <= 0 {
            return Err(jvm.exception("java/lang/IllegalArgumentException", "pos neg value").await);
        }

        let _: () = jvm.invoke_virtual(&card, "setPosition", "(IIII)V", (x, y, width, height)).await?;

        jvm.put_field(&mut this, "__wieSymbolX", "I", x).await?;
        jvm.put_field(&mut this, "__wieSymbolY", "I", y).await?;
        jvm.put_field(&mut this, "__wieSymbolWidth", "I", width).await?;
        jvm.put_field(&mut this, "__wieSymbolHeight", "I", height).await?;

        Ok(())
    }

    async fn show_symbol_card(jvm: &Jvm, this: &mut ClassInstanceRef<Self>) -> JvmResult<()> {
        let mut card: ClassInstanceRef<CandidateWindow> = jvm.get_field(this, "__wieSymbolCard", "Lorg/kwis/msp/lcdui/CandidateWindow;").await?;

        if card.is_null() {
            card = jvm
                .new_class(
                    "org/kwis/msp/lcdui/CandidateWindow",
                    "(Lorg/kwis/msp/lcdui/InputMethodHandler;)V",
                    (this.clone(),),
                )
                .await?
                .into();

            jvm.put_field(this, "__wieSymbolCard", "Lorg/kwis/msp/lcdui/CandidateWindow;", card.clone())
                .await?;
        }

        let _: () = jvm.invoke_virtual(&card, "setDiableChars", "([C)V", (None,)).await?;

        let display: ClassInstanceRef<Display> = jvm
            .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(&display, "pushCard", "(Lorg/kwis/msp/lcdui/Card;)V", (card.clone(),))
            .await?;

        let _: () = jvm.invoke_virtual(&card, "showNotify", "()V", ()).await?;

        jvm.put_field(this, "__wieSymbolCardActive", "Z", true).await?;

        Ok(())
    }

    async fn remove_symbol_card(jvm: &Jvm, this: &mut ClassInstanceRef<Self>) -> JvmResult<()> {
        let card: ClassInstanceRef<CandidateWindow> = jvm.get_field(this, "__wieSymbolCard", "Lorg/kwis/msp/lcdui/CandidateWindow;").await?;

        if !card.is_null() {
            let display: ClassInstanceRef<Display> = jvm
                .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
                .await?;

            let _: bool = jvm
                .invoke_virtual(&display, "removeCard", "(Lorg/kwis/msp/lcdui/Card;)Z", (card,))
                .await?;
        }

        jvm.put_field(this, "__wieSymbolCard", "Lorg/kwis/msp/lcdui/CandidateWindow;", None)
            .await?;

        jvm.put_field(this, "__wieSymbolCardActive", "Z", false).await?;
        jvm.put_field(this, "__wieSymbolX", "I", 0).await?;
        jvm.put_field(this, "__wieSymbolY", "I", 0).await?;
        jvm.put_field(this, "__wieSymbolWidth", "I", 0).await?;
        jvm.put_field(this, "__wieSymbolHeight", "I", 0).await?;

        Ok(())
    }

    async fn get_current_mode_code(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<String>> {
        let mode: i32 = jvm.get_field(&this, "currentMode", "I").await?;

        if mode == 99 {
            return jvm
                .get_static_field("org/kwis/msp/lcdui/InputMethodHandler", "__wieSymbolModeCode", "Ljava/lang/String;")
                .await;
        }

        if mode < 0 {
            return Err(jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "").await);
        }

        let supported_modes: ClassInstanceRef<Array<ClassInstanceRef<String>>> =
            jvm.get_field(&this, "__wieSupportedModes", "[Ljava/lang/String;").await?;

        let mode_code: alloc::vec::Vec<ClassInstanceRef<String>> = jvm.load_array(&supported_modes, mode as usize, 1).await?;

        Ok(mode_code[0].clone())
    }
}
