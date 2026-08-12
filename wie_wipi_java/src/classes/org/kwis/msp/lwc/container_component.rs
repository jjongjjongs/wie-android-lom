use alloc::{boxed::Box, vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::MethodAccessFlags;
use jvm::{ClassInstanceRef, JavaError, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::Component;

// class org.kwis.msp.lwc.ContainerComponent
pub struct ContainerComponent;

impl ContainerComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/ContainerComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, MethodAccessFlags::PROTECTED),
                JavaMethodProto::new(
                    "addComponent",
                    "(ILorg/kwis/msp/lwc/Component;)V",
                    Self::add_component_index,
                    MethodAccessFlags::SYNCHRONIZED,
                ),
                JavaMethodProto::new(
                    "addComponent",
                    "(Lorg/kwis/msp/lwc/Component;)I",
                    Self::add_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "controlInset",
                    "(Z)V",
                    Self::control_inset,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "useFrame",
                    "(Z)V",
                    Self::use_frame,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "paintFrame",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    Self::paint_frame,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "paint",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    Self::paint,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "repaint",
                    "()V",
                    Self::repaint,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "repaint",
                    "(IIII)V",
                    Self::repaint_with_area,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "validate",
                    "()V",
                    Self::validate,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getComponent",
                    "(I)Lorg/kwis/msp/lwc/Component;",
                    Self::get_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getIndexOf",
                    "(Lorg/kwis/msp/lwc/Component;)I",
                    Self::get_index_of,
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
                    "getFocusComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    Self::get_focus_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "findBranch",
                    "(Lorg/kwis/msp/lwc/Component;Lorg/kwis/msp/lwc/Component;)Lorg/kwis/msp/lwc/Component;",
                    Self::find_branch,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setFocus",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    Self::set_focus_component,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "keyNotify",
                    "(II)Z",
                    Self::key_notify,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "processEvent",
                    "(IIII)Z",
                    Self::process_event,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "removeComponent",
                    "(I)V",
                    Self::remove_component_index,
                    MethodAccessFlags::SYNCHRONIZED,
                ),
                JavaMethodProto::new(
                    "removeComponent",
                    "(Lorg/kwis/msp/lwc/Component;)V",
                    Self::remove_component,
                    MethodAccessFlags::SYNCHRONIZED,
                ),
                JavaMethodProto::new(
                    "removeAllComponents",
                    "()V",
                    Self::remove_all_components,
                    MethodAccessFlags::SYNCHRONIZED,
                ),
                JavaMethodProto::new(
                    "setComponent",
                    "(ILorg/kwis/msp/lwc/Component;)V",
                    Self::set_component,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "children",
                    "[Lorg/kwis/msp/lwc/Component;",
                    Default::default(),
                ),
                JavaFieldProto::new("childCount", "I", Default::default()),
                JavaFieldProto::new(
                    "focusComponent",
                    "Lorg/kwis/msp/lwc/Component;",
                    Default::default(),
                ),
                JavaFieldProto::new("offsetX", "I", Default::default()),
                JavaFieldProto::new("offsetY", "I", Default::default()),
                JavaFieldProto::new("insetTop", "S", Default::default()),
                JavaFieldProto::new("insetBottom", "S", Default::default()),
                JavaFieldProto::new("insetLeft", "S", Default::default()),
                JavaFieldProto::new("insetRight", "S", Default::default()),
                JavaFieldProto::new("useFrame", "Z", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("stub org.kwis.msp.lwc.ContainerComponent::<init>({this:?})");

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "<init>",
                "()V",
                (),
            )
            .await?;

        let children = jvm
            .instantiate_array("Lorg/kwis/msp/lwc/Component;", 4)
            .await?;

        let mut this = this;
        jvm.put_field(
            &mut this,
            "children",
            "[Lorg/kwis/msp/lwc/Component;",
            children,
        )
        .await?;
        jvm.put_field(&mut this, "childCount", "I", 0).await?;
        jvm.put_field(
            &mut this,
            "focusComponent",
            "Lorg/kwis/msp/lwc/Component;",
            ClassInstanceRef::<Component>::new(None),
        )
        .await?;

        jvm.put_field(&mut this, "offsetX", "I", 0).await?;
        jvm.put_field(&mut this, "offsetY", "I", 0).await?;
        jvm.put_field(&mut this, "insetTop", "S", 0i16).await?;
        jvm.put_field(&mut this, "insetBottom", "S", 0i16).await?;
        jvm.put_field(&mut this, "insetLeft", "S", 0i16).await?;
        jvm.put_field(&mut this, "insetRight", "S", 0i16).await?;
        jvm.put_field(&mut this, "useFrame", "Z", false).await?;

        Ok(())
    }


    async fn control_inset(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        enabled: bool,
    ) -> JvmResult<()> {
        // Native ContainerComponent.controlInset(Z):
        // false -> all four insets = 0
        // true  -> all four insets = 2
        let inset: i16 = if enabled { 2 } else { 0 };

        jvm.put_field(&mut this, "insetTop", "S", inset).await?;
        jvm.put_field(&mut this, "insetBottom", "S", inset).await?;
        jvm.put_field(&mut this, "insetLeft", "S", inset).await?;
        jvm.put_field(&mut this, "insetRight", "S", inset).await?;

        Ok(())
    }

    async fn use_frame(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        enabled: bool,
    ) -> JvmResult<()> {
        // Native ContainerComponent.useFrame(Z):
        //
        // if (useFrame == enabled)
        //     return;
        //
        // useFrame = enabled;
        // controlInset(enabled);
        // invalidate();
        // repaint();

        let current: bool =
            jvm.get_field(&this, "useFrame", "Z").await?;

        if current == enabled {
            return Ok(());
        }

        jvm.put_field(&mut this, "useFrame", "Z", enabled)
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "controlInset",
                "(Z)V",
                (enabled,),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn paint_frame(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        graphics: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native ContainerComponent.paintFrame(Graphics):
        //
        // g.setColor(Decorator color at static +0x54);
        // g.drawRoundRect(0, 0, width - 1, height - 1, 3, 3);
        //
        // Decorator.<clinit> initializes +0x54 with RGB(0,0,0),
        // therefore the exact color value is 0x000000.

        if graphics.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setColor",
                "(I)V",
                (0i32,),
            )
            .await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawRoundRect",
                "(IIIIII)V",
                (0, 0, width - 1, height - 1, 3, 3),
            )
            .await?;

        Ok(())
    }

    async fn paint(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        graphics: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // WipiPlayer Plus ContainerComponent.paint(Graphics)
        //
        // Native flow:
        //  1. Save incoming clip.
        //  2. paintContent().
        //  3. paintFrame() when useFrame is set.
        //  4. Translate by offsetX/offsetY.
        //  5. Restore translated Graphics state after reset().
        //  6. Restrict clip to the container's inset content area.
        //  7. Paint intersecting children.
        //       ContainerComponent -> paint()
        //       other Component    -> paintContent()
        //  8. Restore Graphics state after every child.
        //  9. Undo offsetX/offsetY translation.

        if graphics.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        // Native +0x70/+0x74/+0x6c/+0x68.
        let clip_x: i32 = jvm
            .invoke_virtual(&graphics, "getClipX", "()I", ())
            .await?;
        let clip_y: i32 = jvm
            .invoke_virtual(&graphics, "getClipY", "()I", ())
            .await?;
        let clip_width: i32 = jvm
            .invoke_virtual(&graphics, "getClipWidth", "()I", ())
            .await?;
        let clip_height: i32 = jvm
            .invoke_virtual(&graphics, "getClipHeight", "()I", ())
            .await?;

        // this.paintContent(g)
        let _: () = jvm
            .invoke_virtual(
                &this,
                "paintContent",
                "(Lorg/kwis/msp/lcdui/Graphics;)V",
                (graphics.clone(),),
            )
            .await?;

        // if (useFrame) paintFrame(g)
        let use_frame: bool =
            jvm.get_field(&this, "useFrame", "Z").await?;

        if use_frame {
            let _: () = jvm
                .invoke_virtual(
                    &this,
                    "paintFrame",
                    "(Lorg/kwis/msp/lcdui/Graphics;)V",
                    (graphics.clone(),),
                )
                .await?;
        }

        let offset_x: i32 =
            jvm.get_field(&this, "offsetX", "I").await?;
        let offset_y: i32 =
            jvm.get_field(&this, "offsetY", "I").await?;

        // Enter the container's scrolled coordinate system.
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "translate",
                "(II)V",
                (offset_x, offset_y),
            )
            .await?;

        // Incoming clip expressed in the translated coordinate system.
        let local_clip_x = clip_x - offset_x;
        let local_clip_y = clip_y - offset_y;

        // Native saves these after applying offsetX/offsetY.
        let translated_x: i32 = jvm
            .invoke_virtual(&graphics, "getTranslateX", "()I", ())
            .await?;
        let translated_y: i32 = jvm
            .invoke_virtual(&graphics, "getTranslateY", "()I", ())
            .await?;

        // Native reset -> translate(saved translation) -> setClip.
        let _: () = jvm
            .invoke_virtual(&graphics, "reset", "()V", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "translate",
                "(II)V",
                (translated_x, translated_y),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "setClip",
                "(IIII)V",
                (
                    local_clip_x,
                    local_clip_y,
                    clip_width,
                    clip_height,
                ),
            )
            .await?;

        let inset_top =
            jvm.get_field::<i16>(&this, "insetTop", "S").await? as i32;
        let inset_bottom =
            jvm.get_field::<i16>(&this, "insetBottom", "S").await? as i32;
        let inset_left =
            jvm.get_field::<i16>(&this, "insetLeft", "S").await? as i32;
        let inset_right =
            jvm.get_field::<i16>(&this, "insetRight", "S").await? as i32;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        // Exact native inset rectangle:
        //
        // x = insetLeft - offsetX
        // y = insetTop  - offsetY
        // w = width  - insetLeft - insetRight
        // h = height - insetTop  - insetBottom
        let inner_x = inset_left - offset_x;
        let inner_y = inset_top - offset_y;
        let inner_width = width - inset_left - inset_right;
        let inner_height = height - inset_top - inset_bottom;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "clipRect",
                "(IIII)V",
                (
                    inner_x,
                    inner_y,
                    inner_width,
                    inner_height,
                ),
            )
            .await?;

        let child_count: i32 =
            jvm.get_field(&this, "childCount", "I").await?;

        let clip_right = local_clip_x + clip_width;
        let clip_bottom = local_clip_y + clip_height;

        for index in 0..child_count {
            let child: ClassInstanceRef<Component> = jvm
                .invoke_virtual(
                    &this,
                    "getComponent",
                    "(I)Lorg/kwis/msp/lwc/Component;",
                    (index,),
                )
                .await?;

            // Native dereferences the child immediately.
            if child.is_null() {
                return Err(
                    jvm.exception("java/lang/NullPointerException", "")
                        .await,
                );
            }

            let child_x: i32 =
                jvm.get_field(&child, "x", "I").await?;
            let child_y: i32 =
                jvm.get_field(&child, "y", "I").await?;
            let child_width: i32 =
                jvm.get_field(&child, "w", "I").await?;
            let child_height: i32 =
                jvm.get_field(&child, "h", "I").await?;

            // Exact native intersection rejection tests.
            if child_x >= clip_right
                || local_clip_x >= child_x + child_width
                || clip_bottom <= child_y
                || local_clip_y >= child_y + child_height
            {
                continue;
            }

            // Restrict painting to this child's bounds.
            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "clipRect",
                    "(IIII)V",
                    (
                        child_x,
                        child_y,
                        child_width,
                        child_height,
                    ),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "translate",
                    "(II)V",
                    (child_x, child_y),
                )
                .await?;

            if jvm.is_instance(
                &**child,
                "org/kwis/msp/lwc/ContainerComponent",
            ) {
                // Native instanceof + cast + virtual Container.paint().
                let _: () = jvm
                    .invoke_virtual(
                        &child,
                        "paint",
                        "(Lorg/kwis/msp/lcdui/Graphics;)V",
                        (graphics.clone(),),
                    )
                    .await?;
            } else {
                // Non-container child: Component.paintContent().
                let _: () = jvm
                    .invoke_virtual(
                        &child,
                        "paintContent",
                        "(Lorg/kwis/msp/lcdui/Graphics;)V",
                        (graphics.clone(),),
                    )
                    .await?;
            }

            // Native restores the parent Graphics state after each child.
            let _: () = jvm
                .invoke_virtual(&graphics, "reset", "()V", ())
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "translate",
                    "(II)V",
                    (translated_x, translated_y),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "setClip",
                    "(IIII)V",
                    (
                        local_clip_x,
                        local_clip_y,
                        clip_width,
                        clip_height,
                    ),
                )
                .await?;

            let _: () = jvm
                .invoke_virtual(
                    &graphics,
                    "clipRect",
                    "(IIII)V",
                    (
                        inner_x,
                        inner_y,
                        inner_width,
                        inner_height,
                    ),
                )
                .await?;
        }

        // Undo the initial Container offset translation.
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "translate",
                "(II)V",
                (-offset_x, -offset_y),
            )
            .await?;

        Ok(())
    }

    async fn repaint(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        // Native ContainerComponent.repaint():
        // repaint(-offsetX, -offsetY, width, height)
        let offset_x: i32 =
            jvm.get_field(&this, "offsetX", "I").await?;
        let offset_y: i32 =
            jvm.get_field(&this, "offsetY", "I").await?;
        let width: i32 =
            jvm.get_field(&this, "w", "I").await?;
        let height: i32 =
            jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "repaint",
                "(IIII)V",
                (-offset_x, -offset_y, width, height),
            )
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
        // Native ContainerComponent.repaint(IIII)
        let shown: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        if !shown {
            return Ok(());
        }

        let parent: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        if parent.is_null() {
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
                &parent,
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

    async fn validate(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        // WipiPlayer Plus ContainerComponent.validate()
        //
        // 1. Validate this container itself if invalid.
        // 2. Validate every invalid child.
        // 3. If this container owns focus:
        //    - if it can handle input and has no focused child,
        //      choose next, then previous traversal target.
        //    - if it cannot handle input, drop its focus and search
        //      upward for a replacement traversal target.

        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;

        if mask & 0x1 == 0 {
            let _: () = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/Component",
                    "validate",
                    "()V",
                    (),
                )
                .await?;
        }

        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if child_count > 0 {
            let children = jvm
                .get_field(
                    &this,
                    "children",
                    "[Lorg/kwis/msp/lwc/Component;",
                )
                .await?;

            let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
                jvm.load_array(&children, 0, child_count as usize).await?;

            for child in values {
                if child.is_null() {
                    return Err(
                        jvm.exception(
                            "java/lang/NullPointerException",
                            "null child in ContainerComponent",
                        )
                        .await,
                    );
                }

                let child_valid: bool =
                    jvm.invoke_virtual(&child, "isValid", "()Z", ()).await?;

                if !child_valid {
                    let _: () =
                        jvm.invoke_virtual(&child, "validate", "()V", ()).await?;
                }
            }
        }

        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;

        // No focus on this container: nothing more to repair.
        if mask & 0x2 == 0 {
            return Ok(());
        }

        if mask & 0x4 != 0 {
            // A focused, input-capable container should have a focused child
            // when a traversal candidate exists.
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

            let mut target: ClassInstanceRef<Component> = jvm
                .invoke_virtual(
                    &this,
                    "getNextTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    (),
                )
                .await?;

            if target.is_null() {
                target = jvm
                    .invoke_virtual(
                        &this,
                        "getPrevTraversalComponent",
                        "()Lorg/kwis/msp/lwc/Component;",
                        (),
                    )
                    .await?;
            }

            if !target.is_null() {
                let _: () =
                    jvm.invoke_virtual(&target, "setFocus", "()V", ()).await?;
            }

            return Ok(());
        }

        // This container currently has focus but cannot handle input.
        // Native code sends FOCUS=false first.
        let _: bool = jvm
            .invoke_virtual(
                &this,
                "processEvent",
                "(IIII)Z",
                (1, 0, 0, 0),
            )
            .await?;

        let mut current: ClassInstanceRef<ContainerComponent> = jvm
            .get_field(
                &this,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        if current.is_null() {
            return Ok(());
        }

        // Native explicitly clears the immediate parent's focusComponent.
        let mut immediate_parent = current.clone();
        jvm.put_field(
            &mut immediate_parent,
            "focusComponent",
            "Lorg/kwis/msp/lwc/Component;",
            ClassInstanceRef::<Component>::new(None),
        )
        .await?;

        loop {
            let mut target: ClassInstanceRef<Component> = jvm
                .invoke_virtual(
                    &current,
                    "getNextTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    (),
                )
                .await?;

            if target.is_null() {
                target = jvm
                    .invoke_virtual(
                        &current,
                        "getPrevTraversalComponent",
                        "()Lorg/kwis/msp/lwc/Component;",
                        (),
                    )
                    .await?;
            }

            if !target.is_null() {
                let _: () =
                    jvm.invoke_virtual(&target, "setFocus", "()V", ()).await?;
                return Ok(());
            }

            current = jvm
                .get_field(
                    &current,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;

            if current.is_null() {
                return Ok(());
            }
        }
    }

    async fn get_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        index: i32,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if index < 0 || index >= child_count {
            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        let children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
            jvm.load_array(&children, index as usize, 1).await?;

        Ok(values[0].clone())
    }

    async fn get_index_of(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<i32> {
        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if child_count <= 0 {
            return Ok(-1);
        }

        let children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let mut index = child_count - 1;

        while index >= 0 {
            let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
                jvm.load_array(&children, index as usize, 1).await?;

            let child = &values[0];

            let same = if component.is_null() {
                child.is_null()
            } else {
                !child.is_null()
                    && child.identity() == component.identity()
            };

            if same {
                return Ok(index);
            }

            index -= 1;
        }

        Ok(-1)
    }

    async fn get_prev_traversal_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let focus_index: i32 = jvm
            .invoke_virtual(
                &this,
                "getIndexOf",
                "(Lorg/kwis/msp/lwc/Component;)I",
                (focus,),
            )
            .await?;

        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        let start = if focus_index >= 0 {
            focus_index
        } else {
            child_count
        };

        if start == 0 {
            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        let mut index = start - 1;

        while index >= 0 {
            let child: ClassInstanceRef<Component> = jvm
                .invoke_virtual(
                    &this,
                    "getComponent",
                    "(I)Lorg/kwis/msp/lwc/Component;",
                    (index,),
                )
                .await?;

            if child.is_null() {
                return Ok(child);
            }

            let mask: i32 = jvm.get_field(&child, "mask", "I").await?;

            if mask & 0x4 != 0 {
                if jvm.is_instance(
                    &**child,
                    "org/kwis/msp/lwc/ContainerComponent",
                ) {
                    let nested: ClassInstanceRef<Component> = jvm
                        .invoke_virtual(
                            &child,
                            "getPrevTraversalComponent",
                            "()Lorg/kwis/msp/lwc/Component;",
                            (),
                        )
                        .await?;

                    if !nested.is_null() {
                        return Ok(nested);
                    }
                }

                return Ok(child);
            }

            index -= 1;
        }

        Ok(ClassInstanceRef::<Component>::new(None))
    }

    async fn get_next_traversal_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let focus_index: i32 = jvm
            .invoke_virtual(
                &this,
                "getIndexOf",
                "(Lorg/kwis/msp/lwc/Component;)I",
                (focus,),
            )
            .await?;

        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if child_count < 0 || focus_index == child_count - 1 {
            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        let mut index = focus_index + 1;

        loop {
            let child: ClassInstanceRef<Component> = jvm
                .invoke_virtual(
                    &this,
                    "getComponent",
                    "(I)Lorg/kwis/msp/lwc/Component;",
                    (index,),
                )
                .await?;

            let current_child_count: i32 =
                jvm.get_field(&this, "childCount", "I").await?;

            if index >= current_child_count {
                return Ok(ClassInstanceRef::<Component>::new(None));
            }

            if child.is_null() {
                return Ok(child);
            }

            let mask: i32 = jvm.get_field(&child, "mask", "I").await?;

            if mask & 0x4 != 0 {
                if jvm.is_instance(
                    &**child,
                    "org/kwis/msp/lwc/ContainerComponent",
                ) {
                    let nested: ClassInstanceRef<Component> = jvm
                        .invoke_virtual(
                            &child,
                            "getNextTraversalComponent",
                            "()Lorg/kwis/msp/lwc/Component;",
                            (),
                        )
                        .await?;

                    if !nested.is_null() {
                        return Ok(nested);
                    }
                }

                return Ok(child);
            }

            index += 1;
        }
    }

    async fn get_focus_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        let card: ClassInstanceRef<()> = jvm
            .invoke_virtual(
                &this,
                "getCard",
                "()Lorg/kwis/msp/lcdui/Card;",
                (),
            )
            .await?;

        if card.is_null() {
            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        let raw: Box<dyn jvm::ClassInstance> = this.clone().into();
        let mut current: ClassInstanceRef<Component> = raw.into();

        loop {
            let parent: ClassInstanceRef<Component> = jvm
                .get_field(
                    &current,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;

            if parent.is_null() {
                break;
            }

            current = parent;
        }

        loop {
            let focus: ClassInstanceRef<Component> = jvm
                .get_field(
                    &current,
                    "focusComponent",
                    "Lorg/kwis/msp/lwc/Component;",
                )
                .await?;

            if focus.is_null() {
                return Ok(current);
            }

            if !jvm.is_instance(
                &**focus,
                "org/kwis/msp/lwc/ContainerComponent",
            ) {
                let current_focus: ClassInstanceRef<Component> = jvm
                    .get_field(
                        &current,
                        "focusComponent",
                        "Lorg/kwis/msp/lwc/Component;",
                    )
                    .await?;

                if current_focus.is_null() {
                    return Ok(current);
                }

                return Ok(current_focus);
            }

            // Native re-reads focusComponent after the instanceof check.
            let next: ClassInstanceRef<Component> = jvm
                .get_field(
                    &current,
                    "focusComponent",
                    "Lorg/kwis/msp/lwc/Component;",
                )
                .await?;

            if next.is_null() {
                let exception = jvm
                    .instantiate_class("java/lang/NullPointerException")
                    .await?;

                return Err(JavaError::JavaException(exception));
            }

            if !jvm.is_instance(
                &**next,
                "org/kwis/msp/lwc/ContainerComponent",
            ) {
                let exception = jvm
                    .instantiate_class("java/lang/ClassCastException")
                    .await?;

                return Err(JavaError::JavaException(exception));
            }

            current = next;
        }
    }

    async fn find_branch(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        _: ClassInstanceRef<Self>,
        first: ClassInstanceRef<Component>,
        second: ClassInstanceRef<Component>,
    ) -> JvmResult<ClassInstanceRef<Component>> {
        // Native compares the two references first. null == null then
        // dereferences the common value, producing a raw NPE.
        if first.is_null() && second.is_null() {
            let exception = jvm
                .instantiate_class("java/lang/NullPointerException")
                .await?;

            return Err(JavaError::JavaException(exception));
        }

        if !first.is_null()
            && !second.is_null()
            && first.identity() == second.identity()
        {
            let parent: ClassInstanceRef<Component> = jvm
                .get_field(
                    &first,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;

            return Ok(parent);
        }

        let mut outer = first;

        while !outer.is_null() {
            let mut inner = second.clone();

            while !inner.is_null() {
                if outer.identity() == inner.identity() {
                    return Ok(outer);
                }

                inner = jvm
                    .get_field(
                        &inner,
                        "parent",
                        "Lorg/kwis/msp/lwc/ContainerComponent;",
                    )
                    .await?;
            }

            outer = jvm
                .get_field(
                    &outer,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;
        }

        Ok(ClassInstanceRef::<Component>::new(None))
    }

    async fn set_focus_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        let mut this = this;

        let shown: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        // Native treats null as this ContainerComponent.
        let this_raw: Box<dyn jvm::ClassInstance> = this.clone().into();
        let this_component: ClassInstanceRef<Component> = this_raw.into();

        let requested = if component.is_null() {
            this_component.clone()
        } else {
            component
        };

        // Native returns immediately when the requested component has no card.
        let card: ClassInstanceRef<()> = jvm
            .invoke_virtual(
                &requested,
                "getCard",
                "()Lorg/kwis/msp/lcdui/Card;",
                (),
            )
            .await?;

        if card.is_null() {
            return Ok(());
        }

        let mut old_focus = ClassInstanceRef::<Component>::new(None);
        let mut branch = ClassInstanceRef::<ContainerComponent>::new(None);

        if shown {
            old_focus = jvm
                .invoke_virtual(
                    &this,
                    "getFocusComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    (),
                )
                .await?;

            if old_focus.identity() != requested.identity() {
                let found: ClassInstanceRef<Component> = jvm
                    .invoke_virtual(
                        &this,
                        "findBranch",
                        "(Lorg/kwis/msp/lwc/Component;Lorg/kwis/msp/lwc/Component;)Lorg/kwis/msp/lwc/Component;",
                        (old_focus.clone(), requested.clone()),
                    )
                    .await?;

                if !found.is_null() {
                    let found_raw: Box<dyn jvm::ClassInstance> = found.into();

                    if !jvm.is_instance(
                        &*found_raw,
                        "org/kwis/msp/lwc/ContainerComponent",
                    ) {
                        let exception = jvm
                            .instantiate_class("java/lang/ClassCastException")
                            .await?;

                        return Err(JavaError::JavaException(exception));
                    }

                    branch = found_raw.into();

                    let old_branch_child: ClassInstanceRef<Component> = jvm
                        .get_field(
                            &branch,
                            "focusComponent",
                            "Lorg/kwis/msp/lwc/Component;",
                        )
                        .await?;

                    if !old_branch_child.is_null()
                        && old_branch_child.identity() != requested.identity()
                    {
                        let _: bool = jvm
                            .invoke_virtual(
                                &old_branch_child,
                                "processEvent",
                                "(IIII)Z",
                                (1, 0, 0, 0),
                            )
                            .await?;
                    }
                }
            }
        }

        let local_focus = if requested.identity() == this_component.identity() {
            ClassInstanceRef::<Component>::new(None)
        } else {
            requested.clone()
        };

        jvm.put_field(
            &mut this,
            "focusComponent",
            "Lorg/kwis/msp/lwc/Component;",
            local_focus,
        )
        .await?;

        // Propagate this container as the focused child through each parent.
        let mut current = this_component.clone();

        loop {
            let parent: ClassInstanceRef<ContainerComponent> = jvm
                .get_field(
                    &current,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;

            if parent.is_null() {
                break;
            }

            let mut parent_for_write = parent.clone();

            jvm.put_field(
                &mut parent_for_write,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
                current.clone(),
            )
            .await?;

            let parent_raw: Box<dyn jvm::ClassInstance> = parent.into();
            current = parent_raw.into();
        }

        if shown
            && old_focus.identity() != requested.identity()
            && !branch.is_null()
        {
            let new_branch_child: ClassInstanceRef<Component> = jvm
                .get_field(
                    &branch,
                    "focusComponent",
                    "Lorg/kwis/msp/lwc/Component;",
                )
                .await?;

            if !new_branch_child.is_null() {
                let _: bool = jvm
                    .invoke_virtual(
                        &new_branch_child,
                        "processEvent",
                        "(IIII)Z",
                        (1, 0, 1, 0),
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        // Native resolves the game action before checking event_type.
        let game_action: i32 = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getGameAction",
                "(I)I",
                (key,),
            )
            .await?;

        if event_type != 2 {
            return Ok(false);
        }

        let target: ClassInstanceRef<Component> = match game_action {
            // UP / LEFT
            1 | 2 => {
                jvm.invoke_virtual(
                    &this,
                    "getPrevTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    (),
                )
                .await?
            }

            // RIGHT / DOWN
            5 | 6 => {
                jvm.invoke_virtual(
                    &this,
                    "getNextTraversalComponent",
                    "()Lorg/kwis/msp/lwc/Component;",
                    (),
                )
                .await?
            }

            _ => return Ok(false),
        };

        if target.is_null() {
            return Ok(false);
        }

        let _: () = jvm
            .invoke_virtual(
                &target,
                "setFocus",
                "()V",
                (),
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
        match event {
            // FOCUS
            1 => {
                let focus: ClassInstanceRef<Component> = jvm
                    .get_field(
                        &this,
                        "focusComponent",
                        "Lorg/kwis/msp/lwc/Component;",
                    )
                    .await?;

                if !focus.is_null() {
                    let _: bool = jvm
                        .invoke_virtual(
                            &focus,
                            "processEvent",
                            "(IIII)Z",
                            (1, p1, p2, p3),
                        )
                        .await?;
                }

                let _: bool = jvm
                    .invoke_special(
                        &this,
                        "org/kwis/msp/lwc/Component",
                        "processEvent",
                        "(IIII)Z",
                        (1, p1, p2, p3),
                    )
                    .await?;

                Ok(true)
            }

            // SHOW
            2 => {
                let mut index = 0i32;

                let initial_child_count: i32 =
                    jvm.get_field(&this, "childCount", "I").await?;

                if initial_child_count > 0 {
                    loop {
                        let children = jvm
                            .get_field(
                                &this,
                                "children",
                                "[Lorg/kwis/msp/lwc/Component;",
                            )
                            .await?;

                        let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
                            jvm.load_array(&children, index as usize, 1).await?;

                        let child = values[0].clone();

                        if child.is_null() {
                            let exception = jvm
                                .instantiate_class("java/lang/NullPointerException")
                                .await?;

                            return Err(JavaError::JavaException(exception));
                        }

                        let _: bool = jvm
                            .invoke_virtual(
                                &child,
                                "processEvent",
                                "(IIII)Z",
                                (2, p1, p2, p3),
                            )
                            .await?;

                        index += 1;

                        let child_count: i32 =
                            jvm.get_field(&this, "childCount", "I").await?;

                        if index >= child_count {
                            break;
                        }
                    }
                }

                let _: bool = jvm
                    .invoke_special(
                        &this,
                        "org/kwis/msp/lwc/Component",
                        "processEvent",
                        "(IIII)Z",
                        (2, p1, p2, p3),
                    )
                    .await?;

                Ok(true)
            }

            // KEY
            3 => {
                let focus: ClassInstanceRef<Component> = jvm
                    .get_field(
                        &this,
                        "focusComponent",
                        "Lorg/kwis/msp/lwc/Component;",
                    )
                    .await?;

                if !focus.is_null() {
                    let handled: bool = jvm
                        .invoke_virtual(
                            &focus,
                            "processEvent",
                            "(IIII)Z",
                            (3, p1, p2, p3),
                        )
                        .await?;

                    if handled {
                        return Ok(true);
                    }
                }

                jvm.invoke_special(
                    &this,
                    "org/kwis/msp/lwc/Component",
                    "processEvent",
                    "(IIII)Z",
                    (3, p1, p2, p3),
                )
                .await
            }

            // HIDE / container-wide event 4
            4 => {
                let mut index = 0i32;

                let initial_child_count: i32 =
                    jvm.get_field(&this, "childCount", "I").await?;

                if initial_child_count > 0 {
                    loop {
                        let children = jvm
                            .get_field(
                                &this,
                                "children",
                                "[Lorg/kwis/msp/lwc/Component;",
                            )
                            .await?;

                        let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
                            jvm.load_array(&children, index as usize, 1).await?;

                        let child = values[0].clone();

                        if child.is_null() {
                            let exception = jvm
                                .instantiate_class("java/lang/NullPointerException")
                                .await?;

                            return Err(JavaError::JavaException(exception));
                        }

                        let _: bool = jvm
                            .invoke_virtual(
                                &child,
                                "processEvent",
                                "(IIII)Z",
                                (4, p1, p2, p3),
                            )
                            .await?;

                        index += 1;

                        let child_count: i32 =
                            jvm.get_field(&this, "childCount", "I").await?;

                        if index >= child_count {
                            break;
                        }
                    }
                }

                let _: bool = jvm
                    .invoke_special(
                        &this,
                        "org/kwis/msp/lwc/Component",
                        "processEvent",
                        "(IIII)Z",
                        (4, p1, p2, p3),
                    )
                    .await?;

                Ok(false)
            }

            _ => Ok(false),
        }
    }

    async fn add_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<i32> {
        let index: i32 = jvm.get_field(&this, "childCount", "I").await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "addComponent",
                "(ILorg/kwis/msp/lwc/Component;)V",
                (index, component),
            )
            .await?;

        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        Ok(child_count - 1)
    }

    async fn add_component_index(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        index: i32,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        let mut this = this;
        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        // WipiPlayer Plus: valid insertion range is 0 <= index <= childCount.
        if index < 0 || index > child_count {
            return Err(
                jvm.exception(
                    "java/lang/IndexOutOfBoundsException",
                    "Invalid index",
                )
                .await,
            );
        }

        // WipiPlayer Plus resolver pair +0x210/+0x218.
        if component.is_null() {
            let exception = jvm
                .new_class("java/lang/NullPointerException", "()V", ())
                .await?;

            return Err(JavaError::JavaException(exception));
        }

        // A Component already attached to a Container cannot be inserted again.
        // WipiPlayer Plus resolver pair +0x1d8/+0x1e0.
        let parent: ClassInstanceRef<()> = jvm
            .get_field(
                &component,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        if !parent.is_null() {
            let exception = jvm
                .new_class("java/lang/IllegalArgumentException", "()V", ())
                .await?;

            return Err(JavaError::JavaException(exception));
        }

        let children_before_inflate = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let capacity = jvm.array_length(&children_before_inflate).await?;
        let requested = child_count as usize + 1;

        // Native checkAndInflateArray(this, childCount + 1):
        // grow when requested >= capacity, to exactly capacity * 2.
        if requested >= capacity {
            let old_values: alloc::vec::Vec<ClassInstanceRef<Component>> =
                if capacity == 0 {
                    alloc::vec::Vec::new()
                } else {
                    jvm.load_array(&children_before_inflate, 0, capacity).await?
                };

            let new_capacity = capacity * 2;
            let mut expanded = jvm
                .instantiate_array(
                    "Lorg/kwis/msp/lwc/Component;",
                    new_capacity,
                )
                .await?;

            if !old_values.is_empty() {
                jvm.store_array(&mut expanded, 0, old_values).await?;
            }

            jvm.put_field(
                &mut this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
                expanded,
            )
            .await?;
        }

        // Native re-reads childCount and children after checkAndInflateArray().
        let current_child_count: i32 =
            jvm.get_field(&this, "childCount", "I").await?;

        let mut children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        // Shift [index, childCount) one position to the right.
        let mut pos = current_child_count as usize;
        let insertion_index = index as usize;

        while pos > insertion_index {
            let value: alloc::vec::Vec<ClassInstanceRef<Component>> =
                jvm.load_array(&children, pos - 1, 1).await?;

            jvm.store_array(
                &mut children,
                pos,
                [value[0].clone()],
            )
            .await?;

            pos -= 1;
        }

        // Native implementation increments childCount before the aastore.
        jvm.put_field(
            &mut this,
            "childCount",
            "I",
            current_child_count + 1,
        )
        .await?;

        jvm.store_array(
            &mut children,
            insertion_index,
            [component.clone()],
        )
        .await?;

        let mut component_for_parent = component.clone();
        jvm.put_field(
            &mut component_for_parent,
            "parent",
            "Lorg/kwis/msp/lwc/ContainerComponent;",
            this.clone(),
        )
        .await?;

        // Recompute ContainerComponent.canHandleInput (mask bit 0x4).
        let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        mask &= !0x4;

        let new_child_count = current_child_count + 1;
        let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
            jvm.load_array(
                &children,
                0,
                new_child_count as usize,
            )
            .await?;

        for child in &values {
            let child_mask: i32 =
                jvm.get_field(child, "mask", "I").await?;

            if child_mask & 0x4 != 0 {
                mask |= 0x4;
                break;
            }
        }

        jvm.put_field(&mut this, "mask", "I", mask).await?;

        // Native code captures the old deepest focus before SHOW(true).
        let old_focus: ClassInstanceRef<Component> = jvm
            .invoke_virtual(
                &this,
                "getFocusComponent",
                "()Lorg/kwis/msp/lwc/Component;",
                (),
            )
            .await?;

        let shown: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        if shown {
            let _: bool = jvm
                .invoke_virtual(
                    &component,
                    "processEvent",
                    "(IIII)Z",
                    (2, 0, 1, 0),
                )
                .await?;
        }

        // WipiPlayer Plus checks isShown() again before automatic focus.
        let shown_for_focus: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        let mut can_assign_initial_focus = false;

        if shown_for_focus {
            if old_focus.is_null() {
                can_assign_initial_focus = true;
            } else if old_focus.identity() == this.identity() {
                let local_focus: ClassInstanceRef<Component> = jvm
                    .get_field(
                        &this,
                        "focusComponent",
                        "Lorg/kwis/msp/lwc/Component;",
                    )
                    .await?;

                if local_focus.is_null() {
                    can_assign_initial_focus = true;
                }
            }
        }

        let component_mask: i32 =
            jvm.get_field(&component, "mask", "I").await?;

        if can_assign_initial_focus && component_mask & 0x4 != 0 {
            let mut focus_target = component.clone();

            if jvm.is_instance(
                &**component,
                "org/kwis/msp/lwc/ContainerComponent",
            ) {
                let nested: ClassInstanceRef<Component> = jvm
                    .invoke_virtual(
                        &component,
                        "getNextTraversalComponent",
                        "()Lorg/kwis/msp/lwc/Component;",
                        (),
                    )
                    .await?;

                if !nested.is_null() {
                    focus_target = nested;
                }
            }

            let _: () = jvm
                .invoke_virtual(
                    &focus_target,
                    "setFocus",
                    "()V",
                    (),
                )
                .await?;
        }

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn set_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        index: i32,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        let child_count: i32 =
            jvm.get_field(&this, "childCount", "I").await?;

        if index < 0 || index >= child_count {
            return Err(
                jvm.exception(
                    "java/lang/IndexOutOfBoundsException",
                    "Invalid index",
                )
                .await,
            );
        }

        if component.is_null() {
            let exception = jvm
                .new_class("java/lang/NullPointerException", "()V", ())
                .await?;

            return Err(JavaError::JavaException(exception));
        }

        let parent: ClassInstanceRef<()> = jvm
            .get_field(
                &component,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        if !parent.is_null() {
            let exception = jvm
                .new_class("java/lang/IllegalArgumentException", "()V", ())
                .await?;

            return Err(JavaError::JavaException(exception));
        }

        let children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let old_values: alloc::vec::Vec<ClassInstanceRef<Component>> =
            jvm.load_array(&children, index as usize, 1).await?;

        let old = old_values[0].clone();

        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let transfer_focus =
            if focus.is_null() && old.is_null() {
                let exception = jvm
                    .instantiate_class("java/lang/NullPointerException")
                    .await?;

                return Err(JavaError::JavaException(exception));
            } else if !focus.is_null()
                && !old.is_null()
                && focus.identity() == old.identity()
            {
                let _: bool = jvm
                    .invoke_virtual(
                        &old,
                        "processEvent",
                        "(IIII)Z",
                        (1, 0, 0, 0),
                    )
                    .await?;

                true
            } else {
                false
            };

        let shown: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        if shown {
            let shown_children = jvm
                .get_field(
                    &this,
                    "children",
                    "[Lorg/kwis/msp/lwc/Component;",
                )
                .await?;

            let shown_old_values: alloc::vec::Vec<ClassInstanceRef<Component>> =
                jvm.load_array(&shown_children, index as usize, 1).await?;

            let shown_old = shown_old_values[0].clone();

            if shown_old.is_null() {
                let exception = jvm
                    .instantiate_class("java/lang/NullPointerException")
                    .await?;

                return Err(JavaError::JavaException(exception));
            }

            let _: bool = jvm
                .invoke_virtual(
                    &shown_old,
                    "processEvent",
                    "(IIII)Z",
                    (2, 0, 0, 0),
                )
                .await?;

            let _: bool = jvm
                .invoke_virtual(
                    &component,
                    "processEvent",
                    "(IIII)Z",
                    (2, 0, 1, 0),
                )
                .await?;
        }

        let mut current_children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        jvm.store_array(
            &mut current_children,
            index as usize,
            [component.clone()],
        )
        .await?;

        let mut component_for_parent = component.clone();

        jvm.put_field(
            &mut component_for_parent,
            "parent",
            "Lorg/kwis/msp/lwc/ContainerComponent;",
            this.clone(),
        )
        .await?;

        // Preserve WipiPlayer Plus behavior:
        // the replaced component's parent field is not cleared here.

        if transfer_focus {
            let _: () = jvm
                .invoke_virtual(
                    &component,
                    "setFocus",
                    "()V",
                    (),
                )
                .await?;
        }

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn remove_all_components(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let mut this = this;

        let shown: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !focus.is_null() {
            let _: bool = jvm
                .invoke_virtual(
                    &focus,
                    "processEvent",
                    "(IIII)Z",
                    (1, 0, 0, 0),
                )
                .await?;
        }

        let child_count: i32 =
            jvm.get_field(&this, "childCount", "I").await?;

        let children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let values: alloc::vec::Vec<ClassInstanceRef<Component>> =
            if child_count > 0 {
                jvm.load_array(
                    &children,
                    0,
                    child_count as usize,
                )
                .await?
            } else {
                alloc::vec::Vec::new()
            };

        for child in values {
            let mut child_for_parent = child.clone();

            jvm.put_field(
                &mut child_for_parent,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
                ClassInstanceRef::<Self>::new(None),
            )
            .await?;

            if shown {
                let _: bool = jvm
                    .invoke_virtual(
                        &child,
                        "processEvent",
                        "(IIII)Z",
                        (2, 0, 0, 0),
                    )
                    .await?;
            }
        }

        jvm.put_field(&mut this, "childCount", "I", 0)
            .await?;

        let new_children = jvm
            .instantiate_array(
                "Lorg/kwis/msp/lwc/Component;",
                4,
            )
            .await?;

        jvm.put_field(
            &mut this,
            "children",
            "[Lorg/kwis/msp/lwc/Component;",
            new_children,
        )
        .await?;

        // Native ContainerComponent.removeAllComponents():
        // offsetX = 0; offsetY = 0;
        jvm.put_field(&mut this, "offsetX", "I", 0).await?;
        jvm.put_field(&mut this, "offsetY", "I", 0).await?;

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn remove_component_index(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        index: i32,
    ) -> JvmResult<()> {
        let mut this = this;
        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if index < 0 || index >= child_count {
            return Err(
                jvm.exception(
                    "java/lang/IndexOutOfBoundsException",
                    "Invalid index",
                )
                .await,
            );
        }

        let mut children = jvm
            .get_field(
                &this,
                "children",
                "[Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        let removed_values: alloc::vec::Vec<ClassInstanceRef<Component>> =
            jvm.load_array(&children, index as usize, 1).await?;
        let removed = removed_values[0].clone();

        // WipiPlayer Plus sends FOCUS(false) before detaching the child.
        let focus: ClassInstanceRef<Component> = jvm
            .get_field(
                &this,
                "focusComponent",
                "Lorg/kwis/msp/lwc/Component;",
            )
            .await?;

        if !focus.is_null()
            && !removed.is_null()
            && focus.identity() == removed.identity()
        {
            let _: bool = jvm
                .invoke_virtual(
                    &removed,
                    "processEvent",
                    "(IIII)Z",
                    (1, 0, 0, 0),
                )
                .await?;
        }

        // Detach before shifting the array.
        let mut removed_for_parent = removed.clone();
        jvm.put_field(
            &mut removed_for_parent,
            "parent",
            "Lorg/kwis/msp/lwc/ContainerComponent;",
            ClassInstanceRef::<Self>::new(None),
        )
        .await?;

        // Shift [index + 1, childCount) one position to the left.
        let mut pos = index as usize;
        let old_count = child_count as usize;

        while pos + 1 < old_count {
            let next: alloc::vec::Vec<ClassInstanceRef<Component>> =
                jvm.load_array(&children, pos + 1, 1).await?;

            jvm.store_array(
                &mut children,
                pos,
                [next[0].clone()],
            )
            .await?;

            pos += 1;
        }

        let new_count = child_count - 1;
        jvm.put_field(&mut this, "childCount", "I", new_count)
            .await?;

        // Native code clears the stale tail only when children remain.
        // If count becomes zero, children[0] is left stale but ignored.
        if new_count > 0 {
            jvm.store_array(
                &mut children,
                new_count as usize,
                [ClassInstanceRef::<Component>::new(None)],
            )
            .await?;
        }

        // Recompute canHandleInput (mask bit 0x4).
        //
        // Preserve the reference implementation's backward scan literally:
        // its generated loop does not inspect index 0.
        let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        mask &= !0x4;

        let mut scan_index = new_count - 1;
        while scan_index > 0 {
            let child: alloc::vec::Vec<ClassInstanceRef<Component>> =
                jvm.load_array(&children, scan_index as usize, 1).await?;

            if !child[0].is_null() {
                let child_mask: i32 =
                    jvm.get_field(&child[0], "mask", "I").await?;

                if child_mask & 0x4 != 0 {
                    mask |= 0x4;
                    break;
                }
            }

            scan_index -= 1;
        }

        jvm.put_field(&mut this, "mask", "I", mask).await?;

        // SHOW(false) occurs after parent=null and array/count update.
        let shown: bool = jvm
            .invoke_virtual(&this, "isShown", "()Z", ())
            .await?;

        if shown {
            let _: bool = jvm
                .invoke_virtual(
                    &removed,
                    "processEvent",
                    "(IIII)Z",
                    (2, 0, 0, 0),
                )
                .await?;
        }

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;

        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn remove_component(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        component: ClassInstanceRef<Component>,
    ) -> JvmResult<()> {
        let index: i32 = jvm
            .invoke_virtual(
                &this,
                "getIndexOf",
                "(Lorg/kwis/msp/lwc/Component;)I",
                (component,),
            )
            .await?;

        if index < 0 {
            let exception = jvm
                .new_class("java/lang/IllegalArgumentException", "()V", ())
                .await?;

            return Err(JavaError::JavaException(exception));
        }

        let _: () = jvm
            .invoke_virtual(
                &this,
                "removeComponent",
                "(I)V",
                (index,),
            )
            .await?;

        Ok(())
    }
}
