use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::javax::microedition::midlet::MIDlet;

use crate::classes::org::kwis::msp::lcdui::{Display, EventQueue};

/// The jlet lifecycle states, as `Jlet.ACTIVE` / `PAUSED` / `DESTROYED`.
const ACTIVE: i32 = 1;
const PAUSED: i32 = 2;
const DESTROYED: i32 = 3;

// class org.kwis.msp.lcdui.Jlet
pub struct Jlet;

impl Jlet {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/Jlet",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "getActiveJlet",
                    "()Lorg/kwis/msp/lcdui/Jlet;",
                    Self::get_active_jlet,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getEventQueue",
                    "()Lorg/kwis/msp/lcdui/EventQueue;",
                    Self::get_event_queue,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getDisplay",
                    "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Display;",
                    Self::get_display,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setRotatedDisplay",
                    "(Lorg/kwis/msp/lcdui/Display;)V",
                    Self::set_rotated_display,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getAppProperty",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    Self::get_app_property,
                    Default::default(),
                ),
                JavaMethodProto::new("notifyDestroyed", "()V", Self::notify_destroyed, Default::default()),
                JavaMethodProto::new(
                    "setActiveJlet",
                    "(Lorg/kwis/msp/lcdui/Jlet;)V",
                    Self::set_active_jlet,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getCurrentJlet",
                    "()Lorg/kwis/msp/lcdui/Jlet;",
                    Self::get_active_jlet,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getJletFromPID",
                    "(I)Lorg/kwis/msp/lcdui/Jlet;",
                    Self::get_jlet_from_pid,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("getCurrentProgramID", "()I", Self::get_current_program_id, Default::default()),
                JavaMethodProto::new("getState", "()I", Self::get_state, Default::default()),
                JavaMethodProto::new("startApp", "([Ljava/lang/String;)V", Self::start_app, Default::default()),
                JavaMethodProto::new("pauseApp", "()V", Self::pause_app, Default::default()),
                JavaMethodProto::new("resumeApp", "()V", Self::resume_app, Default::default()),
                JavaMethodProto::new("destroyApp", "(Z)V", Self::destroy_app, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("wipiMidlet", "Lnet/wie/WIPIMIDlet;", Default::default()),
                JavaFieldProto::new("dis", "Lorg/kwis/msp/lcdui/Display;", Default::default()),
                JavaFieldProto::new("dualDis", "Lorg/kwis/msp/lcdui/Display;", Default::default()),
                JavaFieldProto::new("rotatedDis", "Lorg/kwis/msp/lcdui/Display;", Default::default()),
                JavaFieldProto::new("eq", "Lorg/kwis/msp/lcdui/EventQueue;", Default::default()),
                JavaFieldProto::new("currentJlet", "Lorg/kwis/msp/lcdui/Jlet;", FieldAccessFlags::STATIC),
                JavaFieldProto::new("state", "I", Default::default()),
                JavaFieldProto::new("pid", "I", Default::default()),
                JavaFieldProto::new("ACTIVE", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("PAUSED", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("DESTROYED", "I", FieldAccessFlags::STATIC),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        let midlet: ClassInstanceRef<MIDlet> = jvm
            .get_static_field("javax/microedition/midlet/MIDlet", "currentMIDlet", "Ljavax/microedition/midlet/MIDlet;")
            .await?;
        jvm.put_field(&mut this, "wipiMidlet", "Lnet/wie/WIPIMIDlet;", midlet.clone()).await?;
        let _: () = jvm
            .invoke_virtual(&midlet, "setCurrentJlet", "(Lorg/kwis/msp/lcdui/Jlet;)V", (this.clone(),))
            .await?;

        let display = jvm
            .new_class(
                "org/kwis/msp/lcdui/Display",
                "(Lorg/kwis/msp/lcdui/Jlet;Lorg/kwis/msp/lcdui/DisplayProxy;)V",
                (this.clone(), None),
            )
            .await?;

        jvm.put_field(&mut this, "dis", "Lorg/kwis/msp/lcdui/Display;", display).await?;

        let event_queue = jvm
            .new_class("org/kwis/msp/lcdui/EventQueue", "(Lorg/kwis/msp/lcdui/Jlet;)V", (this.clone(),))
            .await?;

        jvm.put_field(&mut this, "eq", "Lorg/kwis/msp/lcdui/EventQueue;", event_queue).await?;

        jvm.put_static_field("org/kwis/msp/lcdui/Jlet", "currentJlet", "Lorg/kwis/msp/lcdui/Jlet;", this.clone())
            .await?;

        // The three states a jlet reports, and the one it starts in. A title
        // compares what `getState` answers against these fields rather than
        // against numbers, so what matters is that the two agree; the values
        // follow the MIDlet lifecycle the WIPI one is modelled on.
        jvm.put_static_field("org/kwis/msp/lcdui/Jlet", "ACTIVE", "I", ACTIVE).await?;
        jvm.put_static_field("org/kwis/msp/lcdui/Jlet", "PAUSED", "I", PAUSED).await?;
        jvm.put_static_field("org/kwis/msp/lcdui/Jlet", "DESTROYED", "I", DESTROYED).await?;
        jvm.put_field(&mut this, "state", "I", ACTIVE).await?;

        Ok(())
    }

    async fn set_active_jlet(jvm: &Jvm, _: &mut WieJvmContext, jlet: ClassInstanceRef<Jlet>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::setActiveJlet({jlet:?})");

        jvm.put_static_field("org/kwis/msp/lcdui/Jlet", "currentJlet", "Lorg/kwis/msp/lcdui/Jlet;", jlet)
            .await
    }

    /// One application runs at a time here, so the jlet of any process id is the
    /// one that is running.
    async fn get_jlet_from_pid(jvm: &Jvm, context: &mut WieJvmContext, pid: i32) -> JvmResult<ClassInstanceRef<Jlet>> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getJletFromPID({pid})");

        Self::get_active_jlet(jvm, context).await
    }

    /// The application id the title was installed under, which is written as
    /// hexadecimal and identifies the program to the platform.
    async fn get_current_program_id(_: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getCurrentProgramID({this:?})");

        Ok(i32::from_str_radix(context.system().aid(), 16).unwrap_or(0))
    }

    async fn get_state(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getState({this:?})");

        jvm.get_field(&this, "state", "I").await
    }

    /// The lifecycle callbacks. A title overrides the ones it cares about, and
    /// the override is what a call reaches; these stand behind them so a title
    /// that leaves one out - or a platform call made before its class is up -
    /// still finds a method, and so the state a title can ask for stays true.
    async fn start_app(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        args: ClassInstanceRef<jvm::Array<String>>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::startApp({this:?}, {args:?})");

        jvm.put_field(&mut this, "state", "I", ACTIVE).await
    }

    async fn pause_app(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::pauseApp({this:?})");

        jvm.put_field(&mut this, "state", "I", PAUSED).await
    }

    async fn resume_app(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::resumeApp({this:?})");

        jvm.put_field(&mut this, "state", "I", ACTIVE).await
    }

    async fn destroy_app(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, unconditional: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::destroyApp({this:?}, {unconditional})");

        jvm.put_field(&mut this, "state", "I", DESTROYED).await
    }

    async fn get_active_jlet(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<ClassInstanceRef<Jlet>> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getActiveJlet");

        let jlet = jvm
            .get_static_field("org/kwis/msp/lcdui/Jlet", "currentJlet", "Lorg/kwis/msp/lcdui/Jlet;")
            .await?;

        Ok(jlet)
    }

    async fn get_event_queue(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<EventQueue>> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getEventQueue({this:?})");

        let eq = jvm.get_field(&this, "eq", "Lorg/kwis/msp/lcdui/EventQueue;").await?;

        Ok(eq)
    }

    async fn get_display(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        name: ClassInstanceRef<String>,
    ) -> JvmResult<ClassInstanceRef<Display>> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getDisplay({this:?}, {name:?})");

        if name.is_null() {
            return jvm.get_field(&this, "dis", "Lorg/kwis/msp/lcdui/Display;").await;
        }

        let name = JavaLangString::to_rust_string(jvm, &name).await?;

        match name.as_ref() {
            "dual" => jvm.get_field(&this, "dualDis", "Lorg/kwis/msp/lcdui/Display;").await,
            "rotated" => jvm.get_field(&this, "rotatedDis", "Lorg/kwis/msp/lcdui/Display;").await,
            _ => Ok(None.into()),
        }
    }

    async fn set_rotated_display(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        rotated_display: ClassInstanceRef<Display>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::setRotatedDisplay({this:?}, {rotated_display:?})");

        jvm.put_field(&mut this, "rotatedDis", "Lorg/kwis/msp/lcdui/Display;", rotated_display)
            .await
    }

    async fn get_app_property(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        key: ClassInstanceRef<String>,
    ) -> JvmResult<ClassInstanceRef<String>> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::getAppProperty({this:?}, {key:?})");

        let midlet = jvm.get_field(&this, "wipiMidlet", "Lnet/wie/WIPIMIDlet;").await?;
        let value = jvm
            .invoke_virtual(&midlet, "getAppProperty", "(Ljava/lang/String;)Ljava/lang/String;", (key,))
            .await?;

        Ok(value)
    }

    async fn notify_destroyed(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Jlet::notifyDestroyed({this:?})");

        let midlet: ClassInstanceRef<MIDlet> = jvm.get_field(&this, "wipiMidlet", "Lnet/wie/WIPIMIDlet;").await?;
        let _: () = jvm.invoke_virtual(&midlet, "notifyDestroyed", "()V", ()).await?;

        let _: () = jvm.invoke_virtual(&this, "destroyApp", "(Z)V", (false,)).await?;

        Ok(())
    }

    pub async fn midlet(jvm: &Jvm, this: &ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<MIDlet>> {
        jvm.get_field(this, "wipiMidlet", "Lnet/wie/WIPIMIDlet;").await
    }

    pub async fn display(jvm: &Jvm, this: &ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Display>> {
        jvm.get_field(this, "dis", "Lorg/kwis/msp/lcdui/Display;").await
    }
}
