use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lcdui::Jlet;

/// How the platform reaches a jlet's lifecycle callbacks.
///
/// The callbacks are protected on `Jlet`, so the platform cannot call them from
/// outside the class; the reference puts these four package-visible statics
/// beside it to do it, and every application module names this class for that
/// reason. Each forwards to the running jlet, so a title's own override is what
/// runs.
// class org.kwis.msp.lcdui.JletWrapper
pub struct JletWrapper;

impl JletWrapper {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/JletWrapper",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("startApp", "([Ljava/lang/String;)V", Self::start_app, MethodAccessFlags::STATIC),
                JavaMethodProto::new("pauseApp", "()V", Self::pause_app, MethodAccessFlags::STATIC),
                JavaMethodProto::new("resumeApp", "()V", Self::resume_app, MethodAccessFlags::STATIC),
                JavaMethodProto::new("destroyApp", "(Z)V", Self::destroy_app, MethodAccessFlags::STATIC),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.JletWrapper::<init>({this:?})");

        Ok(())
    }

    async fn start_app(jvm: &Jvm, _: &mut WieJvmContext, args: ClassInstanceRef<Array<String>>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.JletWrapper::startApp({args:?})");

        let Some(jlet) = Self::active(jvm).await? else {
            return Ok(());
        };
        jvm.invoke_virtual(&jlet, "startApp", "([Ljava/lang/String;)V", (args,)).await
    }

    async fn pause_app(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.JletWrapper::pauseApp()");

        let Some(jlet) = Self::active(jvm).await? else {
            return Ok(());
        };
        jvm.invoke_virtual(&jlet, "pauseApp", "()V", ()).await
    }

    async fn resume_app(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.JletWrapper::resumeApp()");

        let Some(jlet) = Self::active(jvm).await? else {
            return Ok(());
        };
        jvm.invoke_virtual(&jlet, "resumeApp", "()V", ()).await
    }

    async fn destroy_app(jvm: &Jvm, _: &mut WieJvmContext, unconditional: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.JletWrapper::destroyApp({unconditional})");

        let Some(jlet) = Self::active(jvm).await? else {
            return Ok(());
        };
        jvm.invoke_virtual(&jlet, "destroyApp", "(Z)V", (unconditional,)).await
    }

    /// The running jlet, or `None` before one exists - a lifecycle call that
    /// early has nothing to deliver to.
    async fn active(jvm: &Jvm) -> JvmResult<Option<ClassInstanceRef<Jlet>>> {
        let jlet: ClassInstanceRef<Jlet> = jvm
            .get_static_field("org/kwis/msp/lcdui/Jlet", "currentJlet", "Lorg/kwis/msp/lcdui/Jlet;")
            .await?;

        Ok((!jlet.is_null()).then_some(jlet))
    }
}
