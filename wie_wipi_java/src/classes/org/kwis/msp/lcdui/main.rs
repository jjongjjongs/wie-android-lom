use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lcdui.Main
pub struct Main;

impl Main {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/Main",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![JavaMethodProto::new(
                "main",
                "([Ljava/lang/String;)V",
                Self::main,
                MethodAccessFlags::STATIC,
            )],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn main(jvm: &Jvm, _: &mut WieJvmContext, args: ClassInstanceRef<Array<String>>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Main::main({args:?})");

        let wipi_midlet = jvm.new_class("net/wie/WIPIMIDlet", "()V", ()).await?;

        // argv hands the main class in binary (dotted) form - `atdata.JimaeMD` -
        // but classes are registered in JVM internal (slash) form. Normalise so
        // a packaged main class resolves; a bare class name is unchanged.
        let main_class_name = JavaLangString::to_rust_string(jvm, &jvm.load_array(&args, 0, 1).await?[0])
            .await?
            .replace('.', "/");
        let main_class = jvm.new_class(&main_class_name, "()V", ()).await?;

        let _: () = jvm
            .invoke_virtual(&wipi_midlet, "setCurrentJlet", "(Lorg/kwis/msp/lcdui/Jlet;)V", (main_class,))
            .await?;

        jvm.invoke_static("net/wie/Launcher", "startMIDlet", "(Ljavax/microedition/midlet/MIDlet;)V", (wipi_midlet,))
            .await
    }
}
