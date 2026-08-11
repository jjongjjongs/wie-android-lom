use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

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
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("focusNotify", "(Z)V", Self::focus_notify, Default::default()),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn focus_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextBoxComponent>,
        focus: bool,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextComponent",
                "focusNotify",
                "(Z)V",
                (focus,),
            )
            .await?;

        if !focus {
            return Ok(());
        }

        let mut this = this;
        let position: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

        let text: ClassInstanceRef<String> =
            jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let length: i32 =
            jvm.invoke_virtual(&text, "length", "()I", ()).await?;

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
        this: ClassInstanceRef<TextBoxComponent>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        match event_type {
            0 => Ok(false),
            1 | 2 | 3 => {
                jvm.invoke_special(
                    &this,
                    "org/kwis/msp/lwc/TextComponent",
                    "keyNotify",
                    "(II)Z",
                    (event_type, key),
                )
                .await
            }
            4 => {
                let _: bool = jvm
                    .invoke_special(
                        &this,
                        "org/kwis/msp/lwc/TextComponent",
                        "keyNotify",
                        "(II)Z",
                        (event_type, key),
                    )
                    .await?;

                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextBoxComponent>,
        data: ClassInstanceRef<String>,
        constraint: i32,
    ) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lwc.TextBoxComponent::<init>({this:?}, {data:?}, {constraint:?})");

        let _: () = jvm.invoke_special(&this, "org/kwis/msp/lwc/TextComponent", "<init>", "()V", ()).await?;

        if !data.is_null() {
            let mut this = this;
            jvm.put_field(&mut this, "text", "Ljava/lang/String;", data).await?;
        }

        Ok(())
    }
}
