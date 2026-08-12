use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::TextComponent;

// class org.kwis.msp.lwc.ConstraintChecker
pub struct ConstraintChecker;

impl ConstraintChecker {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/ConstraintChecker",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lwc/TextComponent;)V",
                    Self::init,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "__wieTextComponent",
                    "Lorg/kwis/msp/lwc/TextComponent;",
                    Default::default(),
                ),
                JavaFieldProto::new("__wieConstraint", "I", Default::default()),
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

        jvm.put_field(&mut this, "__wieConstraint", "I", -1)
            .await?;

        Ok(())
    }
}
