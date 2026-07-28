//! Bridge for the Jlet of an ahead-of-time compiled LGT application.
//!
//! The application's main class has no bytecode - it was compiled into
//! `binary.mod` - so this stands in for it on the JVM side and forwards the
//! Jlet lifecycle to the compiled code. Entry points are resolved from the
//! class table the application registered through import `0x07`, so nothing
//! here is specific to one title.
//!
//! The instance the compiled code expects is not yet understood, so `<init>`
//! hands it a bare allocation. That is enough for the constructor to be
//! entered and not much further; see `docs/lgt.md`.

use alloc::{
    collections::BTreeMap,
    string::{String as RustString, ToString},
    sync::Arc,
    vec,
};
use core::sync::atomic::{AtomicU32, Ordering};

use java_class_proto::{JavaClassProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_core_arm::{Allocator, ArmCore};

use crate::runtime::java::app_classes::AppClass;

/// Size of the stand-in instance handed to the compiled constructor. The real
/// object layout is still unknown.
const NATIVE_INSTANCE_SIZE: u32 = 0x10;

#[derive(Clone)]
pub struct LmContext {
    pub core: ArmCore,
    pub native_this: Arc<AtomicU32>,
    /// Method name to compiled entry point, read out of the application.
    pub entries: Arc<BTreeMap<RustString, u32>>,
}

impl LmContext {
    pub fn new(core: ArmCore, class: &AppClass) -> Self {
        let entries = class
            .methods()
            .filter_map(|member| Some((member.name().to_string(), class.method_entry(member.name())?)))
            .collect();

        Self {
            core,
            native_this: Arc::new(AtomicU32::new(0)),
            entries: Arc::new(entries),
        }
    }

    fn entry(&self, name: &str) -> Option<u32> {
        self.entries.get(name).copied()
    }
}

pub struct Lm;

impl Lm {
    /// `name` and `parent` are leaked because `JavaClassProto` holds them for
    /// the life of the program. Both come from the application's own class
    /// table and are registered once per run.
    pub fn as_proto(name: &'static str, parent: &'static str) -> JavaClassProto<LmContext> {
        JavaClassProto {
            name,
            parent_class: Some(parent),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("startApp", "([Ljava/lang/String;)V", Self::start_app, Default::default()),
                JavaMethodProto::new("pauseApp", "()V", Self::pause_app, Default::default()),
                JavaMethodProto::new("resumeApp", "()V", Self::resume_app, Default::default()),
                JavaMethodProto::new("destroyApp", "(Z)V", Self::destroy_app, Default::default()),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    /// Runs a compiled lifecycle method with the stand-in instance.
    async fn call_native(jvm: &Jvm, context: &mut LmContext, name: &str) -> JvmResult<()> {
        let Some(entry) = context.entry(name) else {
            tracing::warn!("Application class has no compiled {name}");
            return Ok(());
        };

        let native_this = context.native_this.load(Ordering::SeqCst);
        if native_this == 0 {
            tracing::error!("{name} called before the native instance was created");
            return Ok(());
        }

        tracing::debug!("Calling compiled {name} at {entry:#x} with instance {native_this:#x}");

        match context.core.run_function::<()>(entry, &[native_this]).await {
            Ok(()) => Ok(()),
            Err(error) => Err(jvm.exception("net/wie/WieError", &error.to_string()).await),
        }
    }

    async fn init(_: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        let Some(entry) = context.entry("<init>") else {
            tracing::warn!("Application class has no compiled <init>");
            return Ok(());
        };

        let native_this = match Allocator::alloc(&mut context.core, NATIVE_INSTANCE_SIZE) {
            Ok(address) => address,
            Err(error) => {
                tracing::error!("Failed to allocate the native instance: {error:?}");
                return Ok(());
            }
        };

        tracing::debug!("Calling compiled <init> at {entry:#x} with instance {native_this:#x}");

        match context.core.run_function::<()>(entry, &[native_this]).await {
            Ok(()) => {
                context.native_this.store(native_this, Ordering::SeqCst);
                tracing::debug!("Native instance initialized at {native_this:#x}");
            }
            // The object layout the compiled constructor expects is not known
            // yet, so this is the wall the application currently hits.
            Err(error) => tracing::error!("Compiled <init> failed: {error:?}"),
        }

        Ok(())
    }

    async fn start_app(jvm: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>, _: ClassInstanceRef<Array<String>>) -> JvmResult<()> {
        Self::call_native(jvm, context, "startApp").await
    }

    async fn pause_app(jvm: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        Self::call_native(jvm, context, "pauseApp").await
    }

    async fn resume_app(jvm: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>) -> JvmResult<()> {
        Self::call_native(jvm, context, "resumeApp").await
    }

    async fn destroy_app(jvm: &Jvm, context: &mut LmContext, _: ClassInstanceRef<Self>, _: bool) -> JvmResult<()> {
        Self::call_native(jvm, context, "destroyApp").await
    }
}
