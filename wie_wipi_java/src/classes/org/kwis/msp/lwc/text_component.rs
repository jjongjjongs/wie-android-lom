use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.TextComponent
pub struct TextComponent;

impl TextComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("setMaxLength", "(I)V", Self::set_max_length, Default::default()),
                JavaMethodProto::new("getString", "()Ljava/lang/String;", Self::get_string, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("m_cPos", "I", Default::default()),
                JavaFieldProto::new("imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;", Default::default()),
                JavaFieldProto::new("iMode", "I", Default::default()),
                JavaFieldProto::new("text", "Ljava/lang/String;", Default::default()),
                JavaFieldProto::new("maxLength", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextComponent>) -> JvmResult<()> {
        tracing::debug!("stub org.kwis.msp.lwc.TextComponent::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "org/kwis/msp/lwc/Component", "<init>", "()V", ()).await?;

        // TODO constant. 0: CONSTRAINT_ANY
        let im_handler = jvm.new_class("org/kwis/msp/lcdui/InputMethodHandler", "(I)V", (0,)).await?;

        jvm.put_field(&mut this, "imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;", im_handler)
            .await?;

        let text = JavaLangString::from_rust_string(jvm, "").await?;
        jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
        jvm.put_field(&mut this, "maxLength", "I", -1).await?;

        Ok(())
    }

    async fn set_max_length(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        max_length: i32,
    ) -> JvmResult<()> {
        jvm.put_field(&mut this, "maxLength", "I", max_length).await?;

        if max_length > 0 {
            let text: ClassInstanceRef<String> =
                jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
            let length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

            if length > max_length {
                let text: ClassInstanceRef<String> = jvm
                    .invoke_virtual(
                        &text,
                        "substring",
                        "(II)Ljava/lang/String;",
                        (0, max_length),
                    )
                    .await?;

                jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
                jvm.put_field(&mut this, "m_cPos", "I", 0).await?;
            }
        }

        Ok(())
    }

    async fn get_string(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextComponent>) -> JvmResult<ClassInstanceRef<String>> {
        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        Ok(text)
    }
}
