use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

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
                    "setSymbolPosition",
                    "(IIII)V",
                    Self::set_symbol_position,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "notifyKeyInput",
                    "(II)Z",
                    Self::notify_key_input,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new("currentMode", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolCardActive", "Z", Default::default()),
                JavaFieldProto::new(
                    "__wieSymbolCard",
                    "Lorg/kwis/msp/lcdui/CandidateWindow;",
                    Default::default(),
                ),
                JavaFieldProto::new("__wieSymbolX", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolY", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolWidth", "I", Default::default()),
                JavaFieldProto::new("__wieSymbolHeight", "I", Default::default()),
            ],
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
        let previous_mode: i32 = jvm.get_field(&this, "currentMode", "I").await?;

        jvm.put_field(&mut this, "currentMode", "I", mode).await?;

        // Native setCurrentMode(I) delegates to setCurrentMode(I, false).
        // Entering symbol mode creates/shows a CandidateWindow, while leaving
        // symbol mode destroys the symbol-card object.
        if mode == 99 {
            Self::show_symbol_card(jvm, &mut this).await?;
        } else if previous_mode == 99 {
            Self::remove_symbol_card(jvm, &mut this).await?;
        }

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

        let previous_mode = mode;

        jvm.put_field(&mut this, "currentMode", "I", next).await?;

        // Keep the synthetic symbol-card lifecycle consistent with native
        // setCurrentMode when mode cycling enters or leaves symbol mode.
        if next == 99 {
            Self::show_symbol_card(jvm, &mut this).await?;
        } else if previous_mode == 99 {
            Self::remove_symbol_card(jvm, &mut this).await?;
        }

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
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
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
        let card: ClassInstanceRef<CandidateWindow> = jvm
            .get_field(
                &this,
                "__wieSymbolCard",
                "Lorg/kwis/msp/lcdui/CandidateWindow;",
            )
            .await?;

        if card.is_null() {
            return Ok(());
        }

        // Native validates geometry before dispatching CandidateWindow.setPosition.
        if x <= 0 || y <= 0 || width <= 0 || height <= 0 {
            return Err(
                jvm.exception("java/lang/IllegalArgumentException", "pos neg value")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_virtual(
                &card,
                "setPosition",
                "(IIII)V",
                (x, y, width, height),
            )
            .await?;

        jvm.put_field(&mut this, "__wieSymbolX", "I", x).await?;
        jvm.put_field(&mut this, "__wieSymbolY", "I", y).await?;
        jvm.put_field(&mut this, "__wieSymbolWidth", "I", width)
            .await?;
        jvm.put_field(&mut this, "__wieSymbolHeight", "I", height)
            .await?;

        Ok(())
    }

    async fn show_symbol_card(
        jvm: &Jvm,
        this: &mut ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let mut card: ClassInstanceRef<CandidateWindow> = jvm
            .get_field(
                this,
                "__wieSymbolCard",
                "Lorg/kwis/msp/lcdui/CandidateWindow;",
            )
            .await?;

        if card.is_null() {
            card = jvm
                .new_class(
                    "org/kwis/msp/lcdui/CandidateWindow",
                    "(Lorg/kwis/msp/lcdui/InputMethodHandler;)V",
                    (this.clone(),),
                )
                .await?
                .into();

            jvm.put_field(
                this,
                "__wieSymbolCard",
                "Lorg/kwis/msp/lcdui/CandidateWindow;",
                card.clone(),
            )
            .await?;
        }

        let _: () = jvm
            .invoke_virtual(
                &card,
                "setDiableChars",
                "([C)V",
                (None,),
            )
            .await?;

        let display: ClassInstanceRef<Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &display,
                "pushCard",
                "(Lorg/kwis/msp/lcdui/Card;)V",
                (card.clone(),),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &card,
                "showNotify",
                "()V",
                (),
            )
            .await?;

        jvm.put_field(this, "__wieSymbolCardActive", "Z", true)
            .await?;

        Ok(())
    }

    async fn remove_symbol_card(
        jvm: &Jvm,
        this: &mut ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let card: ClassInstanceRef<CandidateWindow> = jvm
            .get_field(
                this,
                "__wieSymbolCard",
                "Lorg/kwis/msp/lcdui/CandidateWindow;",
            )
            .await?;

        if !card.is_null() {
            let display: ClassInstanceRef<Display> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Display",
                    "getDefaultDisplay",
                    "()Lorg/kwis/msp/lcdui/Display;",
                    (),
                )
                .await?;

            let _: bool = jvm
                .invoke_virtual(
                    &display,
                    "removeCard",
                    "(Lorg/kwis/msp/lcdui/Card;)Z",
                    (card,),
                )
                .await?;
        }

        jvm.put_field(
            this,
            "__wieSymbolCard",
            "Lorg/kwis/msp/lcdui/CandidateWindow;",
            None,
        )
        .await?;

        jvm.put_field(this, "__wieSymbolCardActive", "Z", false)
            .await?;
        jvm.put_field(this, "__wieSymbolX", "I", 0).await?;
        jvm.put_field(this, "__wieSymbolY", "I", 0).await?;
        jvm.put_field(this, "__wieSymbolWidth", "I", 0).await?;
        jvm.put_field(this, "__wieSymbolHeight", "I", 0).await?;

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
