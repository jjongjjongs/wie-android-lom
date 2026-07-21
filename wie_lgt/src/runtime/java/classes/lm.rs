use alloc::{string::ToString, vec};
use java_class_proto::{JavaClassProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};
use wie_core_arm::{Allocator, ArmCore};
use wie_util::ByteRead;

#[derive(Clone)]
pub struct LmContext {
    pub core: ArmCore,
    pub native_this: Option<u32>,
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

    async fn init(_: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("Lm::<init> -> native 0x10c8");

        let native_this = match Allocator::alloc(&mut context.core, 0x10) {
            Ok(address) => address,
            Err(error) => {
                tracing::error!("Lm native allocation failed: {error:?}");
                return Ok(());
            }
        };

        let mut table_bytes = [0u8; 0x120];

        match context.core.read_bytes(0x01500960, &mut table_bytes) {
            Ok(read) => {
                tracing::warn!("Lm runtime table @0x01500960, read={read:#x}: {:02x?}", &table_bytes[..read]);
            }
            Err(error) => {
                tracing::error!("Lm runtime table read failed: {error:?}");
            }
        }

        match context.core.run_function::<()>(0x10c8, &[native_this]).await {
            Ok(_) => {
                context.native_this = Some(native_this);
                tracing::warn!("Lm native object initialized at {native_this:#x}");
            }
            Err(error) => {
                tracing::error!("Lm native constructor failed: {error:?}");
            }
        }

        Ok(())
    }

    async fn start_app(jvm: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>, _: ClassInstanceRef<Array<String>>) -> JvmResult<()> {
        tracing::warn!("Lm::startApp -> native 0x1118");

        let Some(native_this) = context.native_this else {
            tracing::error!("Lm::startApp called without native object");
            return Ok(());
        };

        match context.core.run_function::<()>(0x1118, &[native_this]).await {
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
