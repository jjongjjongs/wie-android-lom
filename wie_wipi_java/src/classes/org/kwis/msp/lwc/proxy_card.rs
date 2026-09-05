use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::{
    lcdui::{Display, Graphics},
    lwc::ContainerComponent,
};

// class org.kwis.msp.lwc.ProxyCard
pub struct ProxyCard;

impl ProxyCard {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/ProxyCard",
            parent_class: Some("org/kwis/msp/lcdui/Card"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Lorg/kwis/msp/lwc/ContainerComponent;Z)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Lorg/kwis/msp/lwc/ContainerComponent;IIIIZ)V",
                    Self::init_with_bounds,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getComponent",
                    "()Lorg/kwis/msp/lwc/ContainerComponent;",
                    Self::get_component,
                    Default::default(),
                ),
                JavaMethodProto::new("showNotify", "(Z)V", Self::show_notify, Default::default()),
                JavaMethodProto::new("pointerNotify", "(III)Z", Self::pointer_notify, Default::default()),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("paint", "(Lorg/kwis/msp/lcdui/Graphics;)V", Self::paint, Default::default()),
            ],
            fields: vec![JavaFieldProto::new(
                "component",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
                Default::default(),
            )],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display_ref: ClassInstanceRef<Display>,
        component: ClassInstanceRef<ContainerComponent>,
        transparent: bool,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.ProxyCard::<init>({this:?}, {display_ref:?}, {component:?}, {transparent})");

        if display_ref.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "display is null").await);
        }

        let width: i32 = jvm.invoke_virtual(&display_ref, "getWidth", "()I", ()).await?;
        let height: i32 = jvm.invoke_virtual(&display_ref, "getHeight", "()I", ()).await?;

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lcdui/Card",
                "<init>",
                "(Lorg/kwis/msp/lcdui/Display;IIIIZ)V",
                (display_ref, 0, 0, width, height, transparent),
            )
            .await?;

        let mut this = this;
        jvm.put_field(&mut this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;", component)
            .await?;

        Ok(())
    }

    async fn init_with_bounds(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display_ref: ClassInstanceRef<Display>,
        component: ClassInstanceRef<ContainerComponent>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        transparent: bool,
    ) -> JvmResult<()> {
        if display_ref.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lcdui/Card",
                "<init>",
                "(Lorg/kwis/msp/lcdui/Display;IIIIZ)V",
                (display_ref, x, y, width, height, transparent),
            )
            .await?;

        let mut this = this;
        jvm.put_field(&mut this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;", component)
            .await?;

        Ok(())
    }

    async fn get_component(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<ContainerComponent>> {
        jvm.get_field(&this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;").await
    }

    async fn show_notify(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, shown: bool) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lcdui/Card", "showNotify", "(Z)V", (shown,))
            .await?;

        let component: ClassInstanceRef<ContainerComponent> = jvm.get_field(&this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if component.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "component is null").await);
        }

        let shown_i = if shown { 1 } else { 0 };

        let _: bool = jvm.invoke_virtual(&component, "processEvent", "(IIII)Z", (2, 0, shown_i, 0)).await?;

        let _: bool = jvm.invoke_virtual(&component, "processEvent", "(IIII)Z", (1, 0, shown_i, 0)).await?;

        Ok(())
    }

    async fn pointer_notify(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, event_type: i32, x: i32, y: i32) -> JvmResult<bool> {
        let component: ClassInstanceRef<ContainerComponent> = jvm.get_field(&this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if component.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "component is null").await);
        }

        jvm.invoke_virtual(&component, "processEvent", "(IIII)Z", (4, event_type, x, y)).await
    }

    async fn key_notify(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, event_type: i32, key: i32) -> JvmResult<bool> {
        let component: ClassInstanceRef<ContainerComponent> = jvm.get_field(&this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if component.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "component is null").await);
        }

        jvm.invoke_virtual(&component, "processEvent", "(IIII)Z", (3, event_type, key, 0)).await
    }

    async fn paint(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, graphics: ClassInstanceRef<Graphics>) -> JvmResult<()> {
        let component: ClassInstanceRef<ContainerComponent> = jvm.get_field(&this, "component", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if component.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "component is null").await);
        }

        jvm.invoke_virtual(&component, "paint", "(Lorg/kwis/msp/lcdui/Graphics;)V", (graphics,))
            .await
    }
}
