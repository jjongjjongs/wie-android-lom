use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::TextComponent;

// class org.kwis.msp.lwc.InputListener
pub struct InputListener;

impl InputListener {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/InputListener",
            parent_class: Some("java/lang/Object"),
            interfaces: vec!["org/kwis/msp/lcdui/InputMethodListener"],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lwc/TextComponent;)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "notifyTextChanged",
                    "([CII)V",
                    Self::notify_text_changed,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "__wieTextComponent",
                    "Lorg/kwis/msp/lwc/TextComponent;",
                    Default::default(),
                ),
                JavaFieldProto::new("__wieChanged", "Z", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        owner: ClassInstanceRef<TextComponent>,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "java/lang/Object", "<init>", "()V", ())
            .await?;

        jvm.put_field(
            &mut this,
            "__wieTextComponent",
            "Lorg/kwis/msp/lwc/TextComponent;",
            owner,
        )
        .await?;

        jvm.put_field(&mut this, "__wieChanged", "Z", false)
            .await?;

        Ok(())
    }

    async fn notify_text_changed(
        _: &Jvm,
        _: &mut WieJvmContext,
        _: ClassInstanceRef<Self>,
        _: ClassInstanceRef<Array<JavaChar>>,
        _: i32,
        _: i32,
    ) -> JvmResult<()> {
        // Native edit dispatch is restored separately from listener wiring.
        Ok(())
    }
}
