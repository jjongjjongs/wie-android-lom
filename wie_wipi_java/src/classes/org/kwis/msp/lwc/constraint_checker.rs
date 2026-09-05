use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult};

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
                JavaMethodProto::new("<init>", "(Lorg/kwis/msp/lwc/TextComponent;)V", Self::init, Default::default()),
                JavaMethodProto::new("setConstraint", "(I)V", Self::set_constraint, Default::default()),
                JavaMethodProto::new("checkData", "([CII)Z", Self::check_data, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("__wieTextComponent", "Lorg/kwis/msp/lwc/TextComponent;", Default::default()),
                JavaFieldProto::new("__wieConstraint", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, owner: ClassInstanceRef<TextComponent>) -> JvmResult<()> {
        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        jvm.put_field(&mut this, "__wieTextComponent", "Lorg/kwis/msp/lwc/TextComponent;", owner)
            .await?;

        jvm.put_field(&mut this, "__wieConstraint", "I", -1).await?;

        Ok(())
    }

    async fn set_constraint(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, constraint: i32) -> JvmResult<()> {
        jvm.put_field(&mut this, "__wieConstraint", "I", constraint).await
    }

    async fn check_data(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        data: ClassInstanceRef<Array<JavaChar>>,
        offset: i32,
        length: i32,
    ) -> JvmResult<bool> {
        let constraint: i32 = jvm.get_field(&this, "__wieConstraint", "I").await?;

        if constraint != 1 && constraint != 2 && constraint != 5 {
            return Ok(true);
        }

        if length <= 0 {
            return Ok(true);
        }

        if data.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        if offset < 0 {
            return Err(jvm.exception("java/lang/ArrayIndexOutOfBoundsException", "").await);
        }

        let chars = jvm.load_array::<JavaChar>(&data, offset as usize, length as usize).await?;

        for ch in chars {
            let ch = ch as u16;

            let valid = match constraint {
                1 => (ch >= b'0' as u16 && ch <= b'9' as u16) || ch == b' ' as u16 || ch == b'-' as u16,
                2 => ch >= b'0' as u16 && ch <= b'9' as u16,
                5 => (ch >= b'0' as u16 && ch <= b'9' as u16) || ch == b' ' as u16 || ch == b'-' as u16 || ch == b'+' as u16 || ch == b'#' as u16,
                _ => true,
            };

            if !valid {
                return Ok(false);
            }
        }

        Ok(true)
    }
}
