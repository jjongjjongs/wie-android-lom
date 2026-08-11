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
                JavaMethodProto::new("insert", "(Ljava/lang/String;III)V", Self::insert, Default::default()),
                JavaMethodProto::new("delete", "(II)V", Self::delete, Default::default()),
                JavaMethodProto::new("replace", "(Ljava/lang/String;II)V", Self::replace, Default::default()),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
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

    async fn insert(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        string: ClassInstanceRef<String>,
        offset: i32,
        length: i32,
        position: i32,
    ) -> JvmResult<()> {
        let text: ClassInstanceRef<String> =
            jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let text_length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

        let inserted: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &string,
                "substring",
                "(II)Ljava/lang/String;",
                (offset, offset + length),
            )
            .await?;

        let prefix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (0, position),
            )
            .await?;

        let suffix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (position, text_length),
            )
            .await?;

        let combined: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &prefix,
                "concat",
                "(Ljava/lang/String;)Ljava/lang/String;",
                (inserted,),
            )
            .await?;

        let combined: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &combined,
                "concat",
                "(Ljava/lang/String;)Ljava/lang/String;",
                (suffix,),
            )
            .await?;

        jvm.put_field(&mut this, "text", "Ljava/lang/String;", combined).await?;

        Ok(())
    }

    async fn delete(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        position: i32,
        length: i32,
    ) -> JvmResult<()> {
        let text: ClassInstanceRef<String> =
            jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let text_length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

        let prefix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (0, position),
            )
            .await?;

        let suffix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (position + length, text_length),
            )
            .await?;

        let combined: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &prefix,
                "concat",
                "(Ljava/lang/String;)Ljava/lang/String;",
                (suffix,),
            )
            .await?;

        jvm.put_field(&mut this, "text", "Ljava/lang/String;", combined).await?;

        Ok(())
    }

    async fn replace(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextComponent>,
        string: ClassInstanceRef<String>,
        length: i32,
        position: i32,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_virtual(
                &this,
                "delete",
                "(II)V",
                (position, length),
            )
            .await?;

        let string_length: i32 =
            jvm.invoke_virtual(&string, "length", "()I", ()).await?;

        jvm.invoke_virtual(
            &this,
            "insert",
            "(Ljava/lang/String;III)V",
            (string, 0, string_length, position),
        )
        .await
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        let game_action: i32 = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getGameAction",
                "(I)I",
                (key,),
            )
            .await?;

        // Native game action 90 is the input-mode key. Only the type 1
        // event advances the mode; the paired type 0 event is consumed.
        if game_action == 90 {
            if event_type == 1 {
                let im_handler =
                    jvm.get_field(&this, "imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;").await?;

                let _: () = jvm
                    .invoke_virtual(&im_handler, "changeCurrentModeToNext", "()V", ())
                    .await?;

                let mode: i32 = jvm
                    .invoke_virtual(&im_handler, "getCurrentModeCode", "()I", ())
                    .await?;

                jvm.put_field(&mut this, "iMode", "I", mode).await?;
            }

            return Ok(true);
        }

        jvm.invoke_special(
            &this,
            "org/kwis/msp/lwc/Component",
            "keyNotify",
            "(II)Z",
            (event_type, key),
        )
        .await
    }

    async fn get_string(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextComponent>) -> JvmResult<ClassInstanceRef<String>> {
        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        Ok(text)
    }
}
