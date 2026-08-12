use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lcdui::InputMethodHandler;

// class org.kwis.msp.lcdui.CandidateWindow
pub struct CandidateWindow;

impl CandidateWindow {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/CandidateWindow",
            parent_class: Some("org/kwis/msp/lcdui/Card"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/InputMethodHandler;)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setDiableChars",
                    "([C)V",
                    Self::set_diable_chars,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setPosition",
                    "(IIII)V",
                    Self::set_position,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "showNotify",
                    "()V",
                    Self::show_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setCandidateList",
                    "(I)V",
                    Self::set_candidate_list,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "__wieInputMethodHandler",
                    "Lorg/kwis/msp/lcdui/InputMethodHandler;",
                    Default::default(),
                ),
                JavaFieldProto::new("__wieDisableChars", "[C", Default::default()),
                JavaFieldProto::new("__wieCandidateIndex", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        handler: ClassInstanceRef<InputMethodHandler>,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lcdui/Card",
                "<init>",
                "()V",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "__wieInputMethodHandler",
            "Lorg/kwis/msp/lcdui/InputMethodHandler;",
            handler,
        )
        .await?;

        jvm.put_field(&mut this, "__wieCandidateIndex", "I", 0)
            .await?;

        Ok(())
    }

    async fn set_diable_chars(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        chars: ClassInstanceRef<Array<JavaChar>>,
    ) -> JvmResult<()> {
        jvm.put_field(&mut this, "__wieDisableChars", "[C", chars)
            .await
    }

    async fn set_position(
        _: &Jvm,
        _: &mut WieJvmContext,
        _: ClassInstanceRef<Self>,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
    ) -> JvmResult<()> {
        // Native CandidateWindow.setPosition only emits platform logging.
        Ok(())
    }

    async fn show_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        jvm.invoke_virtual(&this, "setCandidateList", "(I)V", (0,))
            .await
    }

    async fn set_candidate_list(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        index: i32,
    ) -> JvmResult<()> {
        // The native class selects one of eleven char[][] symbol groups.
        // Candidate data population is restored separately from this lifecycle.
        jvm.put_field(&mut this, "__wieCandidateIndex", "I", index)
            .await
    }
}
