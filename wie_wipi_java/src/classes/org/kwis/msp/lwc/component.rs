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
                JavaMethodProto::new("getWidth", "()I", Self::get_width, Default::default()),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, Default::default()),
                JavaMethodProto::new("getXOnScreen", "()I", Self::get_x_on_screen, Default::default()),
                JavaMethodProto::new("getYOnScreen", "()I", Self::get_y_on_screen, Default::default()),
                JavaMethodProto::new("getPreferredWidth", "()I", Self::get_preferred_width, Default::default()),
                JavaMethodProto::new("getPreferredHeight", "(I)I", Self::get_preferred_height_with_width, Default::default()),
                JavaMethodProto::new("getPreferredHeight", "()I", Self::get_preferred_height, Default::default()),
                JavaMethodProto::new("calcPreferredSize", "(I)V", Self::calc_preferred_size, Default::default()),
                JavaMethodProto::new("canHandleInput", "()Z", Self::can_handle_input, Default::default()),
                JavaMethodProto::new("hasFocus", "()Z", Self::has_focus, Default::default()),
                JavaMethodProto::new("setBackground", "(I)V", Self::set_background, Default::default()),
                JavaMethodProto::new("setForeground", "(I)V", Self::set_foreground, Default::default()),
                JavaMethodProto::new("getBackground", "()I", Self::get_background, Default::default()),
                JavaMethodProto::new("getForeground", "()I", Self::get_foreground, Default::default()),
                JavaMethodProto::new(
                    "paintContent",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    Self::paint_content,
                    Default::default(),
                ),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("focusNotify", "(Z)V", Self::focus_notify, Default::default()),
                JavaMethodProto::new("showNotify", "(Z)V", Self::show_notify, Default::default()),
                JavaMethodProto::new("pointerNotify", "(III)Z", Self::pointer_notify, Default::default()),
                JavaMethodProto::new("processEvent", "(IIII)Z", Self::process_event, Default::default()),
                JavaMethodProto::new("configure", "(IIIII)V", Self::configure, Default::default()),
                JavaMethodProto::new("layout", "()V", Self::layout, Default::default()),
                JavaMethodProto::new("validate", "()V", Self::validate, Default::default()),
                JavaMethodProto::new("setFocus", "()V", Self::set_focus, Default::default()),
                JavaMethodProto::new("getCard", "()Lorg/kwis/msp/lcdui/Card;", Self::get_card, Default::default()),
                JavaMethodProto::new("isValid", "()Z", Self::is_valid, Default::default()),
                JavaMethodProto::new("invalidate", "()V", Self::invalidate, Default::default()),
                JavaMethodProto::new("isShown", "()Z", Self::is_shown, Default::default()),
                JavaMethodProto::new("repaint", "()V", Self::repaint, Default::default()),
                JavaMethodProto::new("repaint", "(IIII)V", Self::repaint_with_area, Default::default()),
                JavaMethodProto::new("serviceRepaints", "()V", Self::service_repaints, Default::default()),
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

        let mut this = this;
        jvm.put_field(&mut this, "w", "I", 1).await?;
        jvm.put_field(&mut this, "h", "I", 1).await?;
        jvm.put_field(&mut this, "bg", "I", -1).await?;
        jvm.put_field(&mut this, "fg", "I", -1).await?;
        jvm.put_field(&mut this, "prefW", "I", -1).await?;
        jvm.put_field(&mut this, "prefH", "I", -1).await?;
        jvm.put_field(&mut this, "mask", "I", 0).await?;

        Ok(())
    }

    async fn key_notify(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, r#type: i32, chr: i32) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lwc.Component::keyNotify({this:?}, {type:?}, {chr:?})");

        // WipiPlayer Plus base Component does not consume key events.
        Ok(false)
    }

    async fn focus_notify(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, focus: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::focusNotify({this:?}, {focus:?})");

        let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        if focus {
            mask |= 0x2;
        } else {
            mask &= !0x2;
        }
        jvm.put_field(&mut this, "mask", "I", mask).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn show_notify(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, show: bool) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::showNotify({this:?}, {show:?})");

        Ok(())
    }

    async fn layout(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        // Native Component.layout() is the default no-op.
        tracing::debug!("org.kwis.msp.lwc.Component::layout({this:?})");
        Ok(())
    }

    async fn validate(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        // WipiPlayer Plus Component.validate():
        //   if (!(mask & VALID)) {
        //       layout();
        //       mask |= VALID;
        //   }
        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;

        if mask & 0x1 == 0 {
            let _: () = jvm.invoke_virtual(&this, "layout", "()V", ()).await?;

            let mut this = this;
            let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
            jvm.put_field(&mut this, "mask", "I", mask | 0x1).await?;
        }

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
        flags: i32,
    ) -> JvmResult<()> {
        // Native WipiPlayer Plus Component.configure(IIIII)V:
        // bit 0 updates position, bit 1 updates size.
        //
        // Position change:
        //   repaint old area -> store x/y -> repaint new area
        //
        // Size change:
        //   repaint old area -> store w/h -> invalidate -> repaint new area
        //
        // Native storage truncates each coordinate/dimension to signed 16-bit.
        if flags & 0x1 != 0 {
            let old_x: i32 = jvm.get_field(&this, "x", "I").await?;
            let old_y: i32 = jvm.get_field(&this, "y", "I").await?;

            if old_x != x || old_y != y {
                let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

                jvm.put_field(&mut this, "x", "I", x as i16 as i32).await?;
                jvm.put_field(&mut this, "y", "I", y as i16 as i32).await?;

                let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;
            }
        }

        if flags & 0x2 != 0 {
            let old_w: i32 = jvm.get_field(&this, "w", "I").await?;
            let old_h: i32 = jvm.get_field(&this, "h", "I").await?;

            if old_w != w || old_h != h {
                let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

                jvm.put_field(&mut this, "w", "I", w as i16 as i32).await?;
                jvm.put_field(&mut this, "h", "I", h as i16 as i32).await?;

                let _: () = jvm.invoke_virtual(&this, "invalidate", "()V", ()).await?;
                let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;
            }
        }

        Ok(())
    }

    async fn set_focus(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::setFocus({this:?})");

        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if !parent.is_null() {
            let _: () = jvm
                .invoke_virtual(&parent, "setFocus", "(Lorg/kwis/msp/lwc/Component;)V", (this,))
                .await?;
        }

        Ok(())
    }

    async fn get_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<()>> {
        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if parent.is_null() {
            return Ok(ClassInstanceRef::new(None));
        }

        jvm.invoke_virtual(&parent, "getCard", "()Lorg/kwis/msp/lcdui/Card;", ()).await
    }

    async fn is_valid(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        Ok(mask & 0x1 != 0)
    }

    async fn invalidate(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        jvm.put_field(&mut this, "prefW", "I", -1).await?;
        jvm.put_field(&mut this, "prefH", "I", -1).await?;

        let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        mask &= !0x1;
        jvm.put_field(&mut this, "mask", "I", mask).await?;

        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if parent.is_null() {
            return Ok(());
        }

        let parent_valid: bool = jvm.invoke_virtual(&parent, "isValid", "()Z", ()).await?;

        if parent_valid {
            let _: () = jvm.invoke_virtual(&parent, "invalidate", "()V", ()).await?;
        }

        Ok(())
    }

    async fn is_shown(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        let mut current: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if current.is_null() {
            return Ok(false);
        }

        loop {
            let parent: ClassInstanceRef<()> = jvm.get_field(&current, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

            if parent.is_null() {
                break;
            }

            current = parent;
        }

        jvm.invoke_virtual(&current, "isShown", "()Z", ()).await
    }

    async fn get_preferred_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        let pref_w: i32 = jvm.get_field(&this, "prefW", "I").await?;

        if pref_w < 0 {
            let _: () = jvm.invoke_virtual(&this, "calcPreferredSize", "(I)V", (-1,)).await?;
        }

        jvm.get_field(&this, "prefW", "I").await
    }

    async fn get_preferred_height_with_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, width: i32) -> JvmResult<i32> {
        let pref_h: i32 = jvm.get_field(&this, "prefH", "I").await?;
        let pref_w: i32 = jvm.get_field(&this, "prefW", "I").await?;

        if pref_h < 0 || pref_w != width {
            let _: () = jvm.invoke_virtual(&this, "calcPreferredSize", "(I)V", (width,)).await?;
        }

        jvm.get_field(&this, "prefH", "I").await
    }

    async fn get_preferred_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        let pref_h: i32 = jvm.get_field(&this, "prefH", "I").await?;

        if pref_h < 0 {
            let _: () = jvm.invoke_virtual(&this, "calcPreferredSize", "(I)V", (-1,)).await?;
        }

        jvm.get_field(&this, "prefH", "I").await
    }

    async fn calc_preferred_size(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, width: i32) -> JvmResult<()> {
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        jvm.put_field(&mut this, "prefW", "I", width).await?;
        jvm.put_field(&mut this, "prefH", "I", height).await?;

        Ok(())
    }

    async fn get_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        jvm.get_field(&this, "w", "I").await
    }

    async fn get_x_on_screen(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        let mut x: i32 = jvm.invoke_virtual(&this, "getX", "()I", ()).await?;

        let mut parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        while !parent.is_null() {
            let parent_x: i32 = jvm.invoke_virtual(&parent, "getX", "()I", ()).await?;
            x += parent_x;

            if jvm.is_instance(&**parent, "org/kwis/msp/lwc/ContainerComponent") {
                let offset_x: i32 = jvm.get_field(&parent, "offsetX", "I").await?;
                x += offset_x;
            }

            parent = jvm.get_field(&parent, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;
        }

        Ok(x)
    }

    async fn get_y_on_screen(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        let mut y: i32 = jvm.invoke_virtual(&this, "getY", "()I", ()).await?;

        let mut parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        while !parent.is_null() {
            let parent_y: i32 = jvm.invoke_virtual(&parent, "getY", "()I", ()).await?;
            y += parent_y;

            if jvm.is_instance(&**parent, "org/kwis/msp/lwc/ContainerComponent") {
                let offset_y: i32 = jvm.get_field(&parent, "offsetY", "I").await?;
                y += offset_y;
            }

            parent = jvm.get_field(&parent, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;
        }

        Ok(y)
    }

    async fn get_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        tracing::debug!("org.kwis.msp.lwc.Component::getHeight({this:?}) -> {height}");

        Ok(height)
    }

    async fn can_handle_input(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        Ok(mask & 0x4 != 0)
    }

    async fn has_focus(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        Ok(mask & 0x2 != 0)
    }

    async fn set_background(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, value: i32) -> JvmResult<()> {
        jvm.put_field(&mut this, "bg", "I", value).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn set_foreground(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, value: i32) -> JvmResult<()> {
        jvm.put_field(&mut this, "fg", "I", value).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn get_background(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        jvm.get_field(&this, "bg", "I").await
    }

    async fn get_foreground(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        jvm.get_field(&this, "fg", "I").await
    }

    async fn pointer_notify(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, r#type: i32, x: i32, y: i32) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lwc.Component::pointerNotify({this:?}, {type:?}, {x:?}, {y:?})");

        Ok(false)
    }

    async fn process_event(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        event: i32,
        param1: i32,
        param2: i32,
        param3: i32,
    ) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lwc.Component::processEvent({this:?}, {event}, {param1}, {param2}, {param3})");

        let listener: ClassInstanceRef<()> = jvm.get_field(&this, "evtListener", "Lorg/kwis/msp/lwc/EventListener;").await?;

        if !listener.is_null() {
            let listener_obj: ClassInstanceRef<()> = jvm.get_field(&this, "evtListenerObj", "Ljava/lang/Object;").await?;

            let handled: bool = jvm
                .invoke_virtual(
                    &listener,
                    "eventNotify",
                    "(IIIILjava/lang/Object;)Z",
                    (event, param1, param2, param3, listener_obj),
                )
                .await?;

            if handled {
                return Ok(true);
            }
        }

        match event {
            1 => {
                let focus = param2 != 0;

                let _: () = jvm.invoke_virtual(&this, "focusNotify", "(Z)V", (focus,)).await?;

                if !focus {
                    let mut parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

                    if !parent.is_null() {
                        let no_focus = ClassInstanceRef::<Component>::new(None);
                        jvm.put_field(&mut parent, "focusComponent", "Lorg/kwis/msp/lwc/Component;", no_focus)
                            .await?;
                    }
                }

                Ok(true)
            }
            2 => {
                let _: () = jvm.invoke_virtual(&this, "showNotify", "(Z)V", (param2 != 0,)).await?;

                Ok(true)
            }
            3 => jvm.invoke_virtual(&this, "keyNotify", "(II)Z", (param1, param2)).await,
            4 => jvm.invoke_virtual(&this, "pointerNotify", "(III)Z", (param1, param2, param3)).await,
            _ => Ok(true),
        }
    }

    async fn paint_content(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, graphics: ClassInstanceRef<()>) -> JvmResult<()> {
        // WipiPlayer Plus Component.paintContent(Graphics)
        //
        // if (!isValid())
        //     validate();
        //
        // if (background < 0)
        //     return;
        //
        // g.setColor(background);
        // g.fillRect(0, 0, width, height);

        let valid: bool = jvm.invoke_virtual(&this, "isValid", "()Z", ()).await?;

        if !valid {
            let _: () = jvm.invoke_virtual(&this, "validate", "()V", ()).await?;
        }

        let background: i32 = jvm.get_field(&this, "bg", "I").await?;

        if background < 0 {
            return Ok(());
        }

        if graphics.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (background,)).await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm.invoke_virtual(&graphics, "fillRect", "(IIII)V", (0, 0, width, height)).await?;

        Ok(())
    }

    async fn repaint(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lwc.Component::repaint({this:?})");

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        // Equivalent to the native full-component repaint path.
        let _: () = jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0, 0, width, height)).await?;

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
        // Native Component.repaint(IIII)
        let parent: ClassInstanceRef<Component> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if parent.is_null() {
            return Ok(());
        }

        let shown: bool = jvm.invoke_virtual(&this, "isShown", "()Z", ()).await?;

        if !shown {
            return Ok(());
        }

        let this_x: i32 = jvm.get_field(&this, "x", "I").await?;
        let this_y: i32 = jvm.get_field(&this, "y", "I").await?;

        let parent_offset_x: i32 = jvm.get_field(&parent, "offsetX", "I").await?;
        let parent_offset_y: i32 = jvm.get_field(&parent, "offsetY", "I").await?;

        let mut x = parent_offset_x + this_x + repaint_x;
        let mut y = parent_offset_y + this_y + repaint_y;

        // Native walks to the top-level component itself rather than
        // recursively dispatching through every Container repaint(),
        // avoiding double application of container offsets.
        let mut current = parent;

        loop {
            let next: ClassInstanceRef<Component> = jvm.get_field(&current, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

            if next.is_null() {
                break;
            }

            let current_x: i32 = jvm.get_field(&current, "x", "I").await?;
            let current_y: i32 = jvm.get_field(&current, "y", "I").await?;

            let next_offset_x: i32 = jvm.get_field(&next, "offsetX", "I").await?;
            let next_offset_y: i32 = jvm.get_field(&next, "offsetY", "I").await?;

            x += next_offset_x + current_x;
            y += next_offset_y + current_y;

            current = next;
        }

        let _: () = jvm
            .invoke_virtual(&current, "repaint", "(IIII)V", (x, y, repaint_width, repaint_height))
            .await?;

        Ok(())
    }

    async fn service_repaints(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        // Native Component.serviceRepaints()
        let parent: ClassInstanceRef<Component> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        if parent.is_null() {
            return Ok(());
        }

        let shown: bool = jvm.invoke_virtual(&this, "isShown", "()Z", ()).await?;

        if !shown {
            return Ok(());
        }

        let mut current = parent;

        loop {
            let next: ClassInstanceRef<Component> = jvm.get_field(&current, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

            if next.is_null() {
                break;
            }

            current = next;
        }

        let _: () = jvm.invoke_virtual(&current, "serviceRepaints", "()V", ()).await?;

        Ok(())
    }
}
