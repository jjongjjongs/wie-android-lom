use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::MethodAccessFlags;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.Component
pub struct Component;

impl Component {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/Component",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, MethodAccessFlags::PROTECTED),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("focusNotify", "(Z)V", Self::focus_notify, Default::default()),
                JavaMethodProto::new("showNotify", "(Z)V", Self::show_notify, Default::default()),
                JavaMethodProto::new("configure", "(IIIII)V", Self::configure, Default::default()),
                JavaMethodProto::new("setFocus", "()V", Self::set_focus, Default::default()),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, Default::default()),
            ],
            // The real Component is a state-bearing class: position, size,
            // colours, parent, event listener, preferred size and a status mask.
            // The names and descriptors match the platform's own field table so
            // a subclass that reaches these by name finds the same storage.
            fields: vec![
                JavaFieldProto::new("x", "I", Default::default()),
                JavaFieldProto::new("y", "I", Default::default()),
                JavaFieldProto::new("w", "I", Default::default()),
                JavaFieldProto::new("h", "I", Default::default()),
                JavaFieldProto::new("parent", "Lorg/kwis/msp/lwc/ContainerComponent;", Default::default()),
                JavaFieldProto::new("bg", "I", Default::default()),
                JavaFieldProto::new("fg", "I", Default::default()),
                JavaFieldProto::new("display", "Lorg/kwis/msp/lcdui/Display;", Default::default()),
                JavaFieldProto::new("evtListener", "Lorg/kwis/msp/lwc/EventListener;", Default::default()),
                JavaFieldProto::new("evtListenerObj", "Ljava/lang/Object;", Default::default()),
                JavaFieldProto::new("prefW", "I", Default::default()),
                JavaFieldProto::new("prefH", "I", Default::default()),
                JavaFieldProto::new("mask", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        Ok(())
    }

    async fn key_notify(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, r#type: i32, chr: i32) -> JvmResult<bool> {
        // A base component consumes the event; subclasses that route input
        // override this. Left as-is until the container tree is wired, so the
        // change stays confined to state rather than input dispatch.
        tracing::debug!("org.kwis.msp.lwc.Component::keyNotify({this:?}, {type:?}, {chr:?})");

        Ok(true)
    }

    async fn focus_notify(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, focus: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::focusNotify({this:?}, {focus:?})");

        Ok(())
    }

    async fn show_notify(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, show: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::showNotify({this:?}, {show:?})");

        Ok(())
    }

    async fn configure(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        layout: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::configure({this:?}, {x}, {y}, {w}, {h}, {layout})");

        jvm.put_field(&mut this, "x", "I", x).await?;
        jvm.put_field(&mut this, "y", "I", y).await?;
        jvm.put_field(&mut this, "w", "I", w).await?;
        jvm.put_field(&mut this, "h", "I", h).await?;

        Ok(())
    }

    async fn set_focus(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::setFocus({this:?})");

        Ok(())
    }

    async fn get_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        tracing::debug!("org.kwis.msp.lwc.Component::getHeight({this:?}) -> {height}");

        Ok(height)
    }
}
