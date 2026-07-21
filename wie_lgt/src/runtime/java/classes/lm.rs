use alloc::{string::ToString, vec};
use java_class_proto::{JavaClassProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};
use wie_core_arm::ArmCore;

#[derive(Clone)]
pub struct LmContext {
    pub core: ArmCore,
}
pub struct Lm;

impl Lm {
    pub fn as_proto() -> JavaClassProto<LmContext> {
        JavaClassProto {
            name: "Lm",
            parent_class: Some("org/kwis/msp/lcdui/Jlet"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("startApp", "([Ljava/lang/String;)V", Self::start_app, Default::default()),
                JavaMethodProto::new("pauseApp", "()V", Self::pause_app, Default::default()),
                JavaMethodProto::new("resumeApp", "()V", Self::resume_app, Default::default()),
                JavaMethodProto::new("destroyApp", "(Z)V", Self::destroy_app, Default::default()),
                JavaMethodProto::new("a", "()V", Self::a, Default::default()),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut LmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("Lm::<init> stub");

        let _: () = jvm.invoke_special(&this, "org/kwis/msp/lcdui/Jlet", "<init>", "()V", ()).await?;

        Ok(())
    }

    async fn start_app(jvm: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>, _: ClassInstanceRef<Array<String>>) -> JvmResult<()> {
        tracing::warn!("Lm::startApp -> native 0x1118");

        match context.core.run_function::<()>(0x1118, &[]).await {
            Ok(_) => Ok(()),
            Err(error) => Err(jvm.exception("net/wie/WieError", &error.to_string()).await),
        }
    }

    async fn pause_app(_: &Jvm, _: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("Lm::pauseApp stub");
        Ok(())
    }

    async fn resume_app(_: &Jvm, _: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("Lm::resumeApp stub");
        Ok(())
    }

    async fn destroy_app(_: &Jvm, _: &mut LmContext, _: ClassInstanceRef<Self>, _: bool) -> JvmResult<()> {
        tracing::warn!("Lm::destroyApp stub");
        Ok(())
    }

    async fn a(_: &Jvm, _: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("Lm::a stub");
        Ok(())
    }
}
