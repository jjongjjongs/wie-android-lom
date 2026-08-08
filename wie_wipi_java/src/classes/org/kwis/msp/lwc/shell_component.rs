use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::{
    lcdui::Display,
    lwc::{Component, ProxyCard},
};

// class org.kwis.msp.lwc.ShellComponent
pub struct ShellComponent;

impl ShellComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/ShellComponent",
            parent_class: Some("org/kwis/msp/lwc/ContainerComponent"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;)V",
                    Self::init_with_display,
                    Default::default(),
                ),
                JavaMethodProto::new("<init>", "(Z)V", Self::init_with_flag, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Z)V",
                    Self::init_with_display_flag,
                    Default::default(),
                ),
                JavaMethodProto::new("<init>", "(ZZ)V", Self::init_with_flags, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;ZZ)V",
                    Self::init_with_display_flags,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(IIII)V",
                    Self::init_with_size,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;IIII)V",
                    Self::init_with_display_size,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(IIIIZ)V",
                    Self::init_with_size_flag,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;IIIIZ)V",
                    Self::init_with_display_size_flag,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "addComponent",
                    "(ILorg/kwis/msp/lwc/Component;)V",
                    Self::add_component_at,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "addComponent",
                    "(Lorg/kwis/msp/lwc/Component;)I",
                    Self::add_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setWorkComponent",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    Self::set_work_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getWorkComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    Self::get_work_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getCard",
                    "()Lorg/kwis/msp/lcdui/Card;",
                    Self::get_card,
                    Default::default(),
                ),
                JavaMethodProto::new("isShown", "()Z", Self::is_shown, Default::default()),
                JavaMethodProto::new("show", "()V", Self::show, Default::default()),
                JavaMethodProto::new("hide", "()V", Self::hide, Default::default()),
                JavaMethodProto::new(
                    "setTitle",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    Self::set_title_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getTitle",
                    "()Lorg/kwis/msp/lwc/Component;",
                    Self::get_title,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setCommand",
                    "(Lorg/kwis/msp/lwc/Component;Z)V",
                    Self::set_command,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getCommand",
                    "()Lorg/kwis/msp/lwc/Component;",
                    Self::get_command,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setGrabKeyListener",
                    "(Lorg/kwis/msp/lwc/GrabKeyListener;Ljava/lang/Object;)V",
                    Self::set_grab_key_listener,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "grabKey",
                    "(I)V",
                    Self::grab_key,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "ungrabKey",
                    "(I)V",
                    Self::ungrab_key,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getPrevTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    Self::get_prev_traversal_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getNextTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    Self::get_next_traversal_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "keyNotify",
                    "(II)Z",
                    Self::key_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "pointerNotify",
                    "(III)Z",
                    Self::pointer_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "showNotify",
                    "(Z)V",
                    Self::show_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "configure",
                    "(IIIII)V",
                    Self::configure,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "processEvent",
                    "(IIII)Z",
                    Self::process_event,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "serviceRepaints",
                    "()V",
                    Self::service_repaints,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "repaint",
                    "(IIII)V",
                    Self::repaint_with_area,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "title",
                    "Lorg/kwis/msp/lwc/Component;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "proxyCard",
                    "Lorg/kwis/msp/lwc/ProxyCard;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "shellState",
                    "Z",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "grabbedKeys",
                    "Ljava/util/Vector;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "work",
                    "Lorg/kwis/msp/lwc/Component;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "command",
                    "Lorg/kwis/msp/lwc/Component;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "commandEnabled",
                    "Z",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "grabKeyListener",
                    "Lorg/kwis/msp/lwc/GrabKeyListener;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "grabKeyContext",
                    "Ljava/lang/Object;",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let display: ClassInstanceRef<Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_core(jvm, this, display, false, false).await
    }

    async fn init_with_display(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
    ) -> JvmResult<()> {
        Self::init_core(jvm, this, display, false, false).await
    }

    async fn init_with_flag(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        state: bool,
    ) -> JvmResult<()> {
        let display: ClassInstanceRef<Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_core(jvm, this, display, state, false).await
    }

    async fn init_with_display_flag(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
        state: bool,
    ) -> JvmResult<()> {
        Self::init_core(jvm, this, display, state, false).await
    }

    async fn init_with_flags(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        state: bool,
        transparent: bool,
    ) -> JvmResult<()> {
        let display: ClassInstanceRef<Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_core(jvm, this, display, state, transparent).await
    }

    async fn init_with_display_flags(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
        state: bool,
        transparent: bool,
    ) -> JvmResult<()> {
        Self::init_core(jvm, this, display, state, transparent).await
    }

    async fn service_repaints(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        // Native ShellComponent.serviceRepaints():
        // proxyCard.serviceRepaints()
        let proxy: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "proxyCard",
                "Lorg/kwis/msp/lwc/ProxyCard;",
            )
            .await?;

        if proxy.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_virtual(&proxy, "serviceRepaints", "()V", ())
            .await?;

        Ok(())
    }

    async fn repaint_with_area(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        repaint_x: i32,
        repaint_y: i32,
        repaint_width: i32,
        repaint_height: i32,
    ) -> JvmResult<()> {
        // Native ShellComponent.repaint(IIII)
        let proxy: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "proxyCard",
                "Lorg/kwis/msp/lwc/ProxyCard;",
            )
            .await?;

        if proxy.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let shown: bool = jvm
            .invoke_virtual(&proxy, "isShown", "()Z", ())
            .await?;

        if !shown {
            return Ok(());
        }

        let x: i32 = jvm.get_field(&this, "x", "I").await?;
        let y: i32 = jvm.get_field(&this, "y", "I").await?;
        let offset_x: i32 =
            jvm.get_field(&this, "offsetX", "I").await?;
        let offset_y: i32 =
            jvm.get_field(&this, "offsetY", "I").await?;

        let _: () = jvm
            .invoke_virtual(
                &proxy,
                "repaint",
                "(IIII)V",
                (
                    offset_x + x + repaint_x,
                    offset_y + y + repaint_y,
                    repaint_width,
                    repaint_height,
                ),
            )
            .await?;

        Ok(())
    }

    async fn init_core(
        jvm: &Jvm,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
        state: bool,
        transparent: bool,
    ) -> JvmResult<()> {
        if display.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "display is null")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "<init>",
                "()V",
                (),
            )
            .await?;

        let mut this = this;

        // Native ShellComponent +0x20: inherited Component.display.
        jvm.put_field(
            &mut this,
            "display",
            "Lorg/kwis/msp/lcdui/Display;",
            display.clone(),
        )
        .await?;

        // Native +0x7c.
        jvm.put_field(&mut this, "shellState", "Z", state).await?;

        let proxy: ClassInstanceRef<ProxyCard> = jvm
            .new_class(
                "org/kwis/msp/lwc/ProxyCard",
                "(Lorg/kwis/msp/lcdui/Display;Lorg/kwis/msp/lwc/ContainerComponent;Z)V",
                (display, this.clone(), transparent),
            )
            .await?
            .into();

        // Native ShellComponent +0x60.
        jvm.put_field(
            &mut this,
            "proxyCard",
            "Lorg/kwis/msp/lwc/ProxyCard;",
            proxy,
        )
        .await?;

        Ok(())
    }

    async fn init_with_size(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        // Native s4:
        // this(defaultDisplay, x, y, width, height)
        let display: ClassInstanceRef<Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_bounds_core(
            jvm,
            this,
            display,
            x,
            y,
            width,
            height,
            false,
        )
        .await
    }

    async fn init_with_display_size(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        // Native s5:
        // this(display, x, y, width, height, false)
        Self::init_bounds_core(
            jvm,
            this,
            display,
            x,
            y,
            width,
            height,
            false,
        )
        .await
    }

    async fn init_with_size_flag(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flag: bool,
    ) -> JvmResult<()> {
        // Native s6:
        // this(defaultDisplay, x, y, width, height, flag)
        let display: ClassInstanceRef<Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_bounds_core(
            jvm,
            this,
            display,
            x,
            y,
            width,
            height,
            flag,
        )
        .await
    }

    async fn init_with_display_size_flag(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flag: bool,
    ) -> JvmResult<()> {
        // Native s7.
        Self::init_bounds_core(
            jvm,
            this,
            display,
            x,
            y,
            width,
            height,
            flag,
        )
        .await
    }

    async fn init_bounds_core(
        jvm: &Jvm,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<Display>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flag: bool,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "<init>",
                "()V",
                (),
            )
            .await?;

        if display.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        // Native s7 rejects non-positive dimensions.
        if width <= 0 || height <= 0 {
            return Err(
                jvm.exception("java/lang/IllegalArgumentException", "")
                    .await,
            );
        }

        let mut this = this;

        // inherited Component.display (+0x20)
        jvm.put_field(
            &mut this,
            "display",
            "Lorg/kwis/msp/lcdui/Display;",
            display.clone(),
        )
        .await?;

        // Native +0x74/+0x78 are zeroed by the constructor.
        jvm.put_field(
            &mut this,
            "grabKeyListener",
            "Lorg/kwis/msp/lwc/GrabKeyListener;",
            ClassInstanceRef::<()>::new(None),
        )
        .await?;

        jvm.put_field(
            &mut this,
            "grabKeyContext",
            "Ljava/lang/Object;",
            ClassInstanceRef::<()>::new(None),
        )
        .await?;

        // Native +0x7c is always zero for the bounds constructor.
        jvm.put_field(&mut this, "shellState", "Z", false).await?;

        let proxy: ClassInstanceRef<ProxyCard> = jvm
            .new_class(
                "org/kwis/msp/lwc/ProxyCard",
                "(Lorg/kwis/msp/lcdui/Display;Lorg/kwis/msp/lwc/ContainerComponent;IIIIZ)V",
                (
                    display,
                    this.clone(),
                    x,
                    y,
                    width,
                    height,
                    flag,
                ),
            )
            .await?
            .into();

        jvm.put_field(
            &mut this,
            "proxyCard",
            "Lorg/kwis/msp/lwc/ProxyCard;",
            proxy,
        )
        .await?;

        // s7 stores the supplied size into Component w/h,
        // but resets Component x/y to zero.
        jvm.put_field(&mut this, "w", "I", width).await?;
        jvm.put_field(&mut this, "h", "I", height).await?;
        jvm.put_field(&mut this, "x", "I", 0).await?;
        jvm.put_field(&mut this, "y", "I", 0).await?;

        // Native:
        // flag == true  -> setBackground(0xffffffff)
        // flag == false -> setBackground(0x00ffffff)
        let background: i32 = if flag {
            -1
        } else {
            0x00ff_ffff
        };

        let _: () = jvm
            .invoke_virtual(
                &this,
                "setBackground",
                "(I)V",
                (background,),
            )
            .await?;

        // Both flag paths converge here.
        let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        mask |= i32::MIN;
        jvm.put_field(&mut this, "mask", "I", mask).await?;

        Ok(())
    }

    async fn set_title_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        // WipiPlayer Plus ShellComponent.setTitle(Component):
        //   if old title != null:
        //       ContainerComponent.removeComponent(old)
        //   title = new
        //   if new != null:
        //       ContainerComponent.addComponent(0, new)
        let old: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "title",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !old.is_null() {
            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/ContainerComponent",
                    "removeComponent",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    (old,),
                )
                .await?;
        }

        let mut this = this;

        jvm.put_field(
            &mut this,
            "title",
            "Lorg/kwis/msp/lwc/Component;",
            component.clone(),
        )
        .await?;

        if !component.is_null() {
            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/ContainerComponent",
                    "addComponent",
                    "(ILorg/kwis/msp/lwc/Component;)V",
                    (0, component),
                )
                .await?;
        }

        Ok(())
    }

    async fn get_title(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        jvm.get_field(
            &this,
            "title",
            "Lorg/kwis/msp/lwc/Component;",
        )
        .await
    }

    async fn set_command(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
        enabled: bool,
    ) -> JvmResult<()> {
        // WipiPlayer Plus:
        //   old command -> ContainerComponent.removeComponent(old)
        //   store enabled + new command
        //   if new command != null:
        //       addComponent(childCount, new command)
        let old: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "command",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !old.is_null() {
            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/ContainerComponent",
                    "removeComponent",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    (old,),
                )
                .await?;
        }

        let mut this = this;

        jvm.put_field(
            &mut this,
            "commandEnabled",
            "Z",
            enabled,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "command",
            "Lorg/kwis/msp/lwc/Component;",
            component.clone(),
        )
        .await?;

        if !component.is_null() {
            let child_count: i32 = jvm
                .get_field(
                    &this,
                    "childCount",
                    "I",
                )
                .await?;

            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/ContainerComponent",
                    "addComponent",
                    "(ILorg/kwis/msp/lwc/Component;)V",
                    (child_count, component),
                )
                .await?;
        }

        Ok(())
    }

    async fn get_command(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        jvm.get_field(
            &this,
            "command",
            "Lorg/kwis/msp/lwc/Component;",
        )
        .await
    }

    async fn set_grab_key_listener(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        listener: ClassInstanceRef<()>,
        context: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        let mut this = this;

        jvm.put_field(
            &mut this,
            "grabKeyListener",
            "Lorg/kwis/msp/lwc/GrabKeyListener;",
            listener,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "grabKeyContext",
            "Ljava/lang/Object;",
            context,
        )
        .await?;

        Ok(())
    }

    async fn grab_key(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        key: i32,
    ) -> JvmResult<()> {
        // Native ShellComponent lazily creates java.util.Vector at +0x70.
        let mut keys: ClassInstanceRef<()> = jvm
            .get_field(&this, "grabbedKeys", "Ljava/util/Vector;")
            .await?;

        if keys.is_null() {
            keys = jvm.new_class("java/util/Vector", "()V", ()).await?.into();

            let mut this = this;
            jvm.put_field(
                &mut this,
                "grabbedKeys",
                "Ljava/util/Vector;",
                keys.clone(),
            )
            .await?;
        }

        // Native code boxes every registration as a fresh Integer.
        // Duplicate registrations are therefore preserved.
        let boxed: ClassInstanceRef<()> = jvm
            .new_class("java/lang/Integer", "(I)V", (key,))
            .await?
            .into();

        let _: () = jvm
            .invoke_virtual(
                &keys,
                "addElement",
                "(Ljava/lang/Object;)V",
                (boxed,),
            )
            .await?;

        Ok(())
    }

    async fn chk_key_grab(
        jvm: &Jvm,
        this: &ClassInstanceRef<Self>,
        key: i32,
    ) -> JvmResult<bool> {
        let keys: ClassInstanceRef<()> = jvm
            .get_field(this, "grabbedKeys", "Ljava/util/Vector;")
            .await?;

        if keys.is_null() {
            return Ok(false);
        }

        let boxed: ClassInstanceRef<()> = jvm
            .new_class("java/lang/Integer", "(I)V", (key,))
            .await?
            .into();

        jvm.invoke_virtual(
            &keys,
            "contains",
            "(Ljava/lang/Object;)Z",
            (boxed,),
        )
        .await
    }

    async fn ungrab_key(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        key: i32,
    ) -> JvmResult<()> {
        let keys: ClassInstanceRef<()> = jvm
            .get_field(&this, "grabbedKeys", "Ljava/util/Vector;")
            .await?;

        if keys.is_null() {
            return Ok(());
        }

        // WipiPlayer Plus does not directly call
        // removeElement(new Integer(key)). It enumerates the Vector,
        // finds the first Integer whose intValue() matches, then removes
        // that actual stored object.
        let elements: ClassInstanceRef<()> = jvm
            .invoke_virtual(
                &keys,
                "elements",
                "()Ljava/util/Enumeration;",
                (),
            )
            .await?;

        loop {
            let has_more: bool = jvm
                .invoke_virtual(
                    &elements,
                    "hasMoreElements",
                    "()Z",
                    (),
                )
                .await?;

            if !has_more {
                return Ok(());
            }

            let element: ClassInstanceRef<()> = jvm
                .invoke_virtual(
                    &elements,
                    "nextElement",
                    "()Ljava/lang/Object;",
                    (),
                )
                .await?;

            // Native performs an Integer cast here before intValue().
            // invoke_virtual naturally fails if the object is not compatible.
            let value: i32 = jvm
                .invoke_virtual(
                    &element,
                    "intValue",
                    "()I",
                    (),
                )
                .await?;

            if value == key {
                let _: bool = jvm
                    .invoke_virtual(
                        &keys,
                        "removeElement",
                        "(Ljava/lang/Object;)Z",
                        (element,),
                    )
                    .await?;

                return Ok(());
            }
        }
    }

    async fn get_prev_traversal_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        let result: ClassInstanceRef<Component> = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "getPrevTraversalComponent",
                "()Lorg/kwis/msp/lwc/Component;",
                (),
            )
            .await?;

        if result.is_null() {
            return Ok(result);
        }

        let command: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "command",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !command.is_null()
            && result.identity() == command.identity()
        {
            let enabled: bool = jvm
                .get_field(
                    &this,
                    "commandEnabled",
                    "Z",
                )
                .await?;

            if enabled {
                return Ok(ClassInstanceRef::<Component>::new(None));
            }
        }

        Ok(result)
    }

    async fn get_next_traversal_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        let result: ClassInstanceRef<Component> = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "getNextTraversalComponent",
                "()Lorg/kwis/msp/lwc/Component;",
                (),
            )
            .await?;

        if result.is_null() {
            return Ok(result);
        }

        let command: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "command",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !command.is_null()
            && result.identity() == command.identity()
        {
            let enabled: bool = jvm
                .get_field(
                    &this,
                    "commandEnabled",
                    "Z",
                )
                .await?;

            if enabled {
                return Ok(ClassInstanceRef::<Component>::new(None));
            }
        }

        Ok(result)
    }

    async fn configure(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        flags: i32,
    ) -> JvmResult<()> {
        // WipiPlayer Plus ShellComponent.configure(IIIII)V.
        if flags & 0x3 != 0 {
            let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
            jvm.put_field(
                &mut this,
                "mask",
                "I",
                mask | i32::MIN,
            )
            .await?;
        }

        if flags & 0x1 != 0 {
            let proxy: ClassInstanceRef<ProxyCard> = jvm
                .get_field(
                    &this,
                    "proxyCard",
                    "Lorg/kwis/msp/lwc/ProxyCard;",
                )
                .await?;

            if proxy.is_null() {
                return Err(
                    jvm.exception(
                        "java/lang/NullPointerException",
                        "proxyCard is null",
                    )
                    .await,
                );
            }

            let _: () = jvm
                .invoke_virtual(
                    &proxy,
                    "move",
                    "(II)V",
                    (x, y),
                )
                .await?;
        }

        if flags & 0x2 != 0 {
            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/Component",
                    "configure",
                    "(IIIII)V",
                    (0, 0, w, h, 2),
                )
                .await?;

            let proxy: ClassInstanceRef<ProxyCard> = jvm
                .get_field(
                    &this,
                    "proxyCard",
                    "Lorg/kwis/msp/lwc/ProxyCard;",
                )
                .await?;

            if proxy.is_null() {
                return Err(
                    jvm.exception(
                        "java/lang/NullPointerException",
                        "proxyCard is null",
                    )
                    .await,
                );
            }

            let _: () = jvm
                .invoke_virtual(
                    &proxy,
                    "resize",
                    "(II)V",
                    (w, h),
                )
                .await?;

            let shell_state: bool =
                jvm.get_field(&this, "shellState", "Z").await?;

            if !shell_state {
                let _: () =
                    jvm.invoke_virtual(&this, "invalidate", "()V", ()).await?;
            }
        }

        Ok(())
    }

    async fn show_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        shown: bool,
    ) -> JvmResult<()> {
        if !shown {
            return Ok(());
        }

        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !focus.is_null() {
            return Ok(());
        }

        let target: ClassInstanceRef<Component> = jvm
            .invoke_virtual(
                &this,
                "getNextTraversalComponent",
                "()Lorg/kwis/msp/lwc/Component;",
                (),
            )
            .await?;

        if !target.is_null() {
            let _: () = jvm
                .invoke_virtual(
                    &target,
                    "setFocus",
                    "()V",
                    (),
                )
                .await?;
        }

        Ok(())
    }

    async fn pointer_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        event_type: i32,
        x: i32,
        y: i32,
    ) -> JvmResult<bool> {
        let _: bool = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "pointerNotify",
                "(III)Z",
                (event_type, x, y),
            )
            .await?;

        Ok(true)
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        // WipiPlayer Plus ShellComponent calls ContainerComponent.keyNotify()
        // but discards its boolean result; Shell itself consumes the key.
        let _: bool = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "keyNotify",
                "(II)Z",
                (event_type, key),
            )
            .await?;

        Ok(true)
    }

    async fn process_event(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        event: i32,
        p1: i32,
        p2: i32,
        p3: i32,
    ) -> JvmResult<bool> {
        // Native ShellComponent only adds special dispatch for KEY (3).
        // All other events go directly to ContainerComponent.
        if event != 3 {
            return jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/ContainerComponent",
                    "processEvent",
                    "(IIII)Z",
                    (event, p1, p2, p3),
                )
                .await;
        }

        // 1) GrabKeyListener gets only keys explicitly registered by grabKey().
        let listener: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "grabKeyListener",
                "Lorg/kwis/msp/lwc/GrabKeyListener;",
            )
            .await?;

        if !listener.is_null() && Self::chk_key_grab(jvm, &this, p2).await? {
            let context: ClassInstanceRef<()> = jvm
                .get_field(
                    &this,
                    "grabKeyContext",
                    "Ljava/lang/Object;",
                )
                .await?;

            let handled: bool = jvm
                .invoke_virtual(
                    &listener,
                    "grabKeyNotify",
                    "(IILjava/lang/Object;)Z",
                    (p1, p2, context),
                )
                .await?;

            if handled {
                return Ok(true);
            }
        }

        // 2) When enabled, the command Component receives the KEY event.
        //
        // Native +0x6c is a general Component reference and is dispatched
        // through Component.processEvent(), not through a CommandBar-specific
        // keyNotify ABI.
        let command_enabled: bool = jvm
            .get_field(&this, "commandEnabled", "Z")
            .await?;

        if command_enabled {
            let command: ClassInstanceRef<Component> = jvm
                .get_field(
                    &this,
                    "command",
                    "Lorg/kwis/msp/lwc/Component;",
                )
                .await?;

            // Native assumes command is non-null when the enable flag is set.
            let handled: bool = jvm
                .invoke_virtual(
                    &command,
                    "processEvent",
                    "(IIII)Z",
                    (3, p1, p2, p3),
                )
                .await?;

            if handled {
                return Ok(true);
            }
        }

        // 3) Final fallback: normal Container/focus dispatch.
        jvm.invoke_special(
            &this,
            "org/kwis/msp/lwc/ContainerComponent",
            "processEvent",
            "(IIII)Z",
            (event, p1, p2, p3),
        )
        .await
    }

    async fn add_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<i32> {
        // Native ShellComponent.addComponent(Component):
        //   this.addComponent(0, component);
        //   return title != null ? 1 : 0;
        let _: () = jvm
            .invoke_virtual(
                &this,
                "addComponent",
                "(ILorg/kwis/msp/lwc/Component;)V",
                (0, component),
            )
            .await?;

        let title: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "title",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        Ok(if title.is_null() { 0 } else { 1 })
    }

    async fn add_component_at(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        index: i32,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        // Shell allows only one ordinary "work" component at a time.
        let work: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "work",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !work.is_null() {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "work component already exists",
                )
                .await,
            );
        }

        // Native Shell override accepts only logical indices 0 and 1.
        if !(0..=1).contains(&index) {
            return Err(
                jvm.exception(
                    "java/lang/IndexOutOfBoundsException",
                    "component index out of range",
                )
                .await,
            );
        }

        let title: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "title",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let command: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "command",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        // A component that is neither the title nor the command becomes work.
        if component.identity() != title.identity()
            && component.identity() != command.identity()
        {
            let mut this_mut = this.clone();

            jvm.put_field(
                &mut this_mut,
                "work",
                "Lorg/kwis/msp/lwc/Component;",
                component.clone(),
            )
            .await?;
        }

        // Native ignores the caller's logical 0/1 when selecting the actual
        // Container index. Presence of title reserves slot zero.
        let actual_index = if title.is_null() { 0 } else { 1 };

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "addComponent",
                "(ILorg/kwis/msp/lwc/Component;)V",
                (actual_index, component),
            )
            .await?;

        Ok(())
    }

    async fn set_work_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        // Native:
        //   if old work != null:
        //       Container.removeComponent(old)
        //       work = null
        //   Shell.addComponent(component)
        let old: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "work",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !old.is_null() {
            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/ContainerComponent",
                    "removeComponent",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    (old,),
                )
                .await?;

            let mut this_mut = this.clone();

            jvm.put_field(
                &mut this_mut,
                "work",
                "Lorg/kwis/msp/lwc/Component;",
                ClassInstanceRef::<Component>::new(None),
            )
            .await?;
        }

        // This intentionally uses virtual dispatch: native setWorkComponent
        // calls ShellComponent.addComponent(Component), not Container directly.
        let _: i32 = jvm
            .invoke_virtual(
                &this,
                "addComponent",
                "(Lorg/kwis/msp/lwc/Component;)I",
                (component,),
            )
            .await?;

        Ok(())
    }

    async fn get_work_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        jvm.get_field(
            &this,
            "work",
            "Lorg/kwis/msp/lwc/Component;",
        )
        .await
    }

    async fn get_card(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<()>> {
        jvm.get_field(
            &this,
            "proxyCard",
            "Lorg/kwis/msp/lwc/ProxyCard;",
        )
        .await
    }

    async fn is_shown(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<bool> {
        let proxy: ClassInstanceRef<ProxyCard> = jvm
            .get_field(
                &this,
                "proxyCard",
                "Lorg/kwis/msp/lwc/ProxyCard;",
            )
            .await?;

        if proxy.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "proxyCard is null")
                    .await,
            );
        }

        jvm.invoke_virtual(&proxy, "isShown", "()Z", ()).await
    }

    async fn show(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let shown: bool = jvm.invoke_virtual(&this, "isShown", "()Z", ()).await?;
        if shown {
            return Ok(());
        }

        let display: ClassInstanceRef<Display> = jvm
            .get_field(
                &this,
                "display",
                "Lorg/kwis/msp/lcdui/Display;",
            )
            .await?;

        if display.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "display is null")
                    .await,
            );
        }

        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;

        // Native Shell.show() performs the initial resize/configure
        // when the high layout-state bit is clear.
        if mask & (i32::MIN) == 0 {
            let display_height: i32 =
                jvm.invoke_virtual(&display, "getHeight", "()I", ()).await?;
            let current_height: i32 =
                jvm.invoke_virtual(&this, "getHeight", "()I", ()).await?;

            if display_height != current_height {
                let width: i32 =
                    jvm.invoke_virtual(&this, "getWidth", "()I", ()).await?;

                let _: () = jvm
                    .invoke_virtual(
                        &this,
                        "configure",
                        "(IIIII)V",
                        (0, 0, width, display_height, 2),
                    )
                    .await?;
            }

            let mut this_for_mask = this.clone();
            let mask: i32 = jvm.get_field(&this_for_mask, "mask", "I").await?;
            jvm.put_field(
                &mut this_for_mask,
                "mask",
                "I",
                mask & !i32::MIN,
            )
            .await?;
        }

        let _: () = jvm.invoke_virtual(&this, "validate", "()V", ()).await?;

        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if focus.is_null() {
            let target: ClassInstanceRef<Component> = jvm
                .invoke_virtual(
                    &this,
                    "getNextTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    (),
                )
                .await?;

            if !target.is_null() {
                let _: () =
                    jvm.invoke_virtual(&target, "setFocus", "()V", ()).await?;
            }
        }

        let proxy: ClassInstanceRef<ProxyCard> = jvm
            .get_field(
                &this,
                "proxyCard",
                "Lorg/kwis/msp/lwc/ProxyCard;",
            )
            .await?;

        if proxy.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "proxyCard is null")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_virtual(
                &display,
                "pushCard",
                "(Lorg/kwis/msp/lcdui/Card;)V",
                (proxy,),
            )
            .await?;

        Ok(())
    }

    async fn hide(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let display: ClassInstanceRef<Display> = jvm
            .get_field(
                &this,
                "display",
                "Lorg/kwis/msp/lcdui/Display;",
            )
            .await?;

        if display.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "display is null")
                    .await,
            );
        }

        let proxy: ClassInstanceRef<ProxyCard> = jvm
            .get_field(
                &this,
                "proxyCard",
                "Lorg/kwis/msp/lwc/ProxyCard;",
            )
            .await?;

        let _: bool = jvm
            .invoke_virtual(
                &display,
                "removeCard",
                "(Lorg/kwis/msp/lcdui/Card;)Z",
                (proxy,),
            )
            .await?;

        Ok(())
    }
}
