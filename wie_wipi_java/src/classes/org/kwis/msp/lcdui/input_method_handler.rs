use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

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
                JavaMethodProto::new("<init>", "(I)V", Self::init, Default::default()),
                JavaMethodProto::new("setCurrentMode", "(I)Z", Self::set_current_mode, Default::default()),
                JavaMethodProto::new(
                    "changeCurrentModeToNext",
                    "()V",
                    Self::change_current_mode_to_next,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getCurrentMode",
                    "()I",
                    Self::get_current_mode,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getCurrentModeCode",
                    "()I",
                    Self::get_current_mode_code,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "hideSymbolCard",
                    "()V",
                    Self::hide_symbol_card,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "notifyKeyInput",
                    "(II)Z",
                    Self::notify_key_input,
                    Default::default(),
                ),
            ],
            fields: vec![JavaFieldProto::new("currentMode", "I", Default::default())],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, constraint: i32) -> JvmResult<()> {
        tracing::debug!("stub org.kwis.msp.lcdui.InputMethodHandler::<init>({this:?}, {constraint})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        // Native constraint 0 initializes the first supported input mode.
        jvm.put_field(&mut this, "currentMode", "I", 0).await?;

        Ok(())
    }

    async fn set_current_mode(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        mode: i32,
    ) -> JvmResult<bool> {
        jvm.put_field(&mut this, "currentMode", "I", mode).await?;

        Ok(true)
    }

    async fn change_current_mode_to_next(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let mode: i32 = jvm.get_field(&this, "currentMode", "I").await?;

        // WipiPlayer Plus constraint 0 cycles the four normal modes and
        // the symbol mode (99), wrapping to the first mode.
        let next = match mode {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 99,
            _ => 0,
        };

        jvm.put_field(&mut this, "currentMode", "I", next).await?;

        Ok(())
    }

    async fn notify_key_input(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        _key: i32,
        event_type: i32,
    ) -> JvmResult<bool> {
        let mode: i32 = jvm.get_field(&this, "currentMode", "I").await?;

        // LoM enters mode 3 before sending ordinary character input.
        // Native InputMethodHandler accepts the type-1 key event and delivers
        // the resulting text through its InputMethodListener callback.
        Ok(mode == 3 && event_type == 1)
    }

    async fn get_current_mode(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(&this, "currentMode", "I").await
    }

    async fn hide_symbol_card(
        _: &Jvm,
        _: &mut WieJvmContext,
        _: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        // WIE does not yet model the native DIME symbol-card object.
        Ok(())
    }

    async fn get_current_mode_code(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(&this, "currentMode", "I").await
    }
}
