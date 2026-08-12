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
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        data: ClassInstanceRef<Array<JavaChar>>,
        count: i32,
        change_type: i32,
    ) -> JvmResult<()> {
        let owner: ClassInstanceRef<TextComponent> = jvm
            .get_field(
                &this,
                "__wieTextComponent",
                "Lorg/kwis/msp/lwc/TextComponent;",
            )
            .await?;

        if owner.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let cursor: i32 = jvm
            .get_field(&owner, "m_cPos", "I")
            .await?;

        match change_type {
            -1 => {
                let mut owner = owner;
                jvm.put_field(&mut owner, "iMode", "I", 1).await?;

                let max_length: i32 =
                    jvm.get_field(&owner, "maxLength", "I").await?;

                if max_length > 0 {
                    let text: ClassInstanceRef<java_runtime::classes::java::lang::String> =
                        jvm.get_field(
                            &owner,
                            "text",
                            "Ljava/lang/String;",
                        )
                        .await?;

                    let text_length: i32 =
                        jvm.invoke_virtual(&text, "length", "()I", ()).await?;

                    if max_length < text_length + count {
                        jvm.put_field(
                            &mut this,
                            "__wieChanged",
                            "Z",
                            true,
                        )
                        .await?;

                        if count == 2 {
                            let _: () = jvm
                                .invoke_virtual(
                                    &owner,
                                    "insert",
                                    "([CIII)V",
                                    (data, 0, 1, cursor),
                                )
                                .await?;
                        }

                        let _: bool = jvm
                            .invoke_virtual(
                                &owner,
                                "keyNotify",
                                "(II)Z",
                                (1, -99),
                            )
                            .await?;

                        return Ok(());
                    }
                }

                jvm.invoke_virtual(
                    &owner,
                    "insert",
                    "([CIII)V",
                    (data, 0, count, cursor),
                )
                .await
            }

            0 => {
                let mut owner = owner;
                jvm.put_field(&mut owner, "iMode", "I", 0).await?;

                jvm.invoke_virtual(
                    &owner,
                    "replace",
                    "([CII)V",
                    (data, count, cursor - count),
                )
                .await
            }

            1 => {
                if cursor == 0 {
                    return Ok(());
                }

                let mut owner = owner;
                jvm.put_field(&mut owner, "iMode", "I", 1).await?;

                let length = if count < 0 { cursor } else { count };

                jvm.invoke_virtual(
                    &owner,
                    "delete",
                    "(II)V",
                    (cursor - length, length),
                )
                .await
            }

            _ => Ok(()),
        }
    }
}
