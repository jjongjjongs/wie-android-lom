use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, JavaError, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::TextBoxComponent;

// class org.kwis.msp.lwc.TextBoxComponent$Action
pub struct TextBoxComponentAction;

impl TextBoxComponentAction {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextBoxComponent$Action",
            parent_class: Some("java/lang/Object"),
            interfaces: vec!["org/kwis/msp/lwc/ActionListener"],
            methods: vec![
                JavaMethodProto::new("<init>", "(Lorg/kwis/msp/lwc/TextBoxComponent;)V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "action",
                    "(Lorg/kwis/msp/lwc/Component;Ljava/lang/Object;)V",
                    Self::action,
                    Default::default(),
                ),
            ],
            fields: vec![JavaFieldProto::new(
                "__wieOuter",
                "Lorg/kwis/msp/lwc/TextBoxComponent;",
                Default::default(),
            )],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextBoxComponentAction>,
        outer: ClassInstanceRef<TextBoxComponent>,
    ) -> JvmResult<()> {
        jvm.put_field(&mut this, "__wieOuter", "Lorg/kwis/msp/lwc/TextBoxComponent;", outer)
            .await?;

        Ok(())
    }

    async fn action(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextBoxComponentAction>,
        source: ClassInstanceRef<()>,
        data: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        let outer: ClassInstanceRef<TextBoxComponent> = jvm.get_field(&this, "__wieOuter", "Lorg/kwis/msp/lwc/TextBoxComponent;").await?;

        if source.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let caret: i32 = jvm.get_field(&source, "m_cPos", "I").await?;

        // Native performs Object -> String checkcast here.
        // Java checkcast accepts null, but a non-null non-String must throw
        // ClassCastException before TextBox.setString(String,int).
        if !data.is_null() && !jvm.is_instance(&**data, "java/lang/String") {
            let exception = jvm.instantiate_class("java/lang/ClassCastException").await?;
            return Err(JavaError::JavaException(exception));
        }

        let text: ClassInstanceRef<String> = ClassInstanceRef::new(data.instance.clone());

        if outer.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm.invoke_virtual(&outer, "setString", "(Ljava/lang/String;I)V", (text, caret)).await?;

        let shell: ClassInstanceRef<()> = jvm.get_field(&outer, "__wieTextBoxPopup", "Lorg/kwis/msp/lwc/ShellComponent;").await?;

        if shell.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm.invoke_virtual(&shell, "hide", "()V", ()).await?;

        let mut outer = outer;

        let null_shell: ClassInstanceRef<()> = ClassInstanceRef::new(None);

        jvm.put_field(&mut outer, "__wieTextBoxPopup", "Lorg/kwis/msp/lwc/ShellComponent;", null_shell)
            .await?;

        jvm.put_field(&mut outer, "__wieTextBoxFlag9c", "I", 0i32).await?;

        let _: () = jvm.invoke_virtual(&outer, "repaint", "()V", ()).await?;

        Ok(())
    }
}
