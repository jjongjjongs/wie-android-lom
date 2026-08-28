use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::Component;

pub struct FormComponent;

impl FormComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/FormComponent",
            parent_class: Some("org/kwis/msp/lwc/ContainerComponent"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("<init>", "(Lorg/kwis/msp/lcdui/Display;)V", Self::init_with_display, Default::default()),
                JavaMethodProto::new("<init>", "(Z)V", Self::init_with_vertical, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Z)V",
                    Self::init_with_display_vertical,
                    Default::default(),
                ),
                JavaMethodProto::new("setPacked", "(Z)V", Self::set_packed, Default::default()),
                JavaMethodProto::new("getPacked", "()Z", Self::get_packed, Default::default()),
                JavaMethodProto::new("setGab", "(I)V", Self::set_gab, Default::default()),
                JavaMethodProto::new("getGab", "()I", Self::get_gab, Default::default()),
                JavaMethodProto::new("setFocus", "(Lorg/kwis/msp/lwc/Component;)V", Self::set_focus, Default::default()),
                JavaMethodProto::new("focusNotify", "(Z)V", Self::focus_notify, Default::default()),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
                JavaMethodProto::new("calcPreferredSize", "(I)V", Self::calc_preferred_size, Default::default()),
                JavaMethodProto::new("paint", "(Lorg/kwis/msp/lcdui/Graphics;)V", Self::paint, Default::default()),
                JavaMethodProto::new("scrollTo", "(II)Z", Self::scroll_to, Default::default()),
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
                JavaMethodProto::new("layout", "()V", Self::layout, Default::default()),
                JavaMethodProto::new("layoutChildHorizontal", "()V", Self::layout_child_horizontal, Default::default()),
                JavaMethodProto::new("layoutChildVertical", "()V", Self::layout_child_vertical, Default::default()),
            ],
            fields: vec![
                // Native platform-visible FormComponent declares only cmpScroll.
                JavaFieldProto::new("cmpScroll", "Lorg/kwis/msp/lwc/ScrollbarComponent;", Default::default()),
                // WIE-private equivalents of native hidden state.
                JavaFieldProto::new("__wieFormPacked", "Z", Default::default()),
                JavaFieldProto::new("__wieFormVertical", "Z", Default::default()),
                JavaFieldProto::new("__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", Default::default()),
                JavaFieldProto::new("__wieFormGab", "I", Default::default()),
                JavaFieldProto::new("__wieFormViewportWidth", "I", Default::default()),
                JavaFieldProto::new("__wieFormViewportX", "I", Default::default()),
                JavaFieldProto::new("__wieFormViewportHeight", "I", Default::default()),
                JavaFieldProto::new("__wieFormViewportY", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        // Native FormComponent() -> FormComponent(true).
        Self::init_with_vertical(jvm, context, this, true).await
    }

    async fn init_with_display(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, display: ClassInstanceRef<()>) -> JvmResult<()> {
        // Native FormComponent(Display) -> FormComponent(Display, true).
        Self::init_with_display_vertical(jvm, context, this, display, true).await
    }

    async fn init_with_vertical(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, vertical: bool) -> JvmResult<()> {
        // Native FormComponent(boolean):
        //   FormComponent(Display.getDefaultDisplay(), vertical)
        let display: ClassInstanceRef<()> = jvm
            .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
            .await?;

        Self::init_with_display_vertical(jvm, context, this, display, vertical).await
    }

    async fn init_with_display_vertical(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<()>,
        vertical: bool,
    ) -> JvmResult<()> {
        // Native main constructor:
        //
        //   ContainerComponent.<init>()
        //   +0x80 (gab)    = 0
        //   +0x64 (packed) = 0
        //   cmpScroll = new ScrollbarComponent()
        //   if (display == null) throw NPE
        //   width = display.getWidth()
        //   +0x84 (vertical layout mode) = argument
        //
        // Config/log-only paths are intentionally omitted.

        let _: () = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/ContainerComponent", "<init>", "()V", ())
            .await?;

        jvm.put_field(&mut this, "__wieFormGab", "I", 0i32).await?;

        jvm.put_field(&mut this, "__wieFormPacked", "Z", false).await?;

        let scrollbar = jvm.instantiate_class("org/kwis/msp/lwc/ScrollbarComponent").await?;

        let _: () = jvm
            .invoke_special(&scrollbar, "org/kwis/msp/lwc/ScrollbarComponent", "<init>", "()V", ())
            .await?;

        jvm.put_field(&mut this, "cmpScroll", "Lorg/kwis/msp/lwc/ScrollbarComponent;", scrollbar)
            .await?;

        if display.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        // Native vtable +0x64 on Display returns the width used to
        // initialize the inherited Component width slot.
        let width: i32 = jvm.invoke_virtual(&display, "getWidth", "()I", ()).await?;

        jvm.put_field(&mut this, "w", "I", width).await?;

        jvm.put_field(&mut this, "__wieFormVertical", "Z", vertical).await?;

        Ok(())
    }

    async fn get_gab(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        jvm.get_field(&this, "__wieFormGab", "I").await
    }

    async fn set_gab(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, gab: i32) -> JvmResult<()> {
        // Native clamps negative values to zero.
        let gab = gab.max(0);

        let old: i32 = jvm.get_field(&this, "__wieFormGab", "I").await?;

        if gab == old {
            return Ok(());
        }

        jvm.put_field(&mut this, "__wieFormGab", "I", gab).await?;

        // Native vtable +0x70 = Component.invalidate().
        jvm.invoke_virtual(&this, "invalidate", "()V", ()).await
    }

    async fn get_packed(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        jvm.get_field(&this, "__wieFormPacked", "Z").await
    }

    async fn set_packed(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, packed: bool) -> JvmResult<()> {
        // Native is an unconditional direct store.
        // No equality test, invalidate, layout, or repaint.
        jvm.put_field(&mut this, "__wieFormPacked", "Z", packed).await
    }

    async fn focus_notify(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, focus: bool) -> JvmResult<()> {
        if !focus {
            let null_component = ClassInstanceRef::<()>::new(None);

            jvm.put_field(&mut this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", null_component)
                .await?;
        }

        Ok(())
    }

    async fn set_focus(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, component: ClassInstanceRef<()>) -> JvmResult<()> {
        // Native first invokes ContainerComponent.setFocus directly.
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "setFocus",
                "(Lorg/kwis/msp/lwc/Component;)V",
                (component.clone(),),
            )
            .await?;

        Self::calc_view_port_area(jvm, this.clone()).await?;

        // Native FormComponent +0x68.
        jvm.put_field(&mut this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", component.clone())
            .await?;

        if component.is_null() {
            return Ok(());
        }

        let vertical: bool = jvm.get_field(&this, "__wieFormVertical", "Z").await?;

        if !vertical {
            let x: i32 = jvm.invoke_virtual(&component, "getXOnScreen", "()I", ()).await?;

            let width: i32 = jvm.get_field(&component, "w", "I").await?;

            let viewport_x: i32 = jvm.get_field(&this, "__wieFormViewportX", "I").await?;

            let viewport_width: i32 = jvm.get_field(&this, "__wieFormViewportWidth", "I").await?;

            if x.wrapping_add(width) < viewport_x || x >= viewport_x.wrapping_add(viewport_width) {
                let _: bool = jvm.invoke_virtual(&this, "scrollTo", "(II)Z", (x.wrapping_sub(viewport_x), 0i32)).await?;
            }

            return Ok(());
        }

        let y: i32 = jvm.invoke_virtual(&component, "getYOnScreen", "()I", ()).await?;

        let height: i32 = jvm.get_field(&component, "h", "I").await?;

        let viewport_y: i32 = jvm.get_field(&this, "__wieFormViewportY", "I").await?;

        let viewport_height: i32 = jvm.get_field(&this, "__wieFormViewportHeight", "I").await?;

        if y.wrapping_add(height) < viewport_y || y >= viewport_y.wrapping_add(viewport_height) {
            let _: bool = jvm.invoke_virtual(&this, "scrollTo", "(II)Z", (0i32, y.wrapping_sub(viewport_y))).await?;
        }

        Ok(())
    }

    async fn calc_view_port_area(jvm: &Jvm, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        let mut top: i32 = jvm.invoke_virtual(&this, "getYOnScreen", "()I", ()).await?;

        let inset_bottom = jvm.get_field::<i16>(&this, "insetBottom", "S").await? as i32;
        let inset_top = jvm.get_field::<i16>(&this, "insetTop", "S").await? as i32;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let mut left: i32 = jvm.invoke_virtual(&this, "getXOnScreen", "()I", ()).await?;

        let inset_right = jvm.get_field::<i16>(&this, "insetRight", "S").await? as i32;
        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let inset_left = jvm.get_field::<i16>(&this, "insetLeft", "S").await? as i32;

        let mut bottom = top.wrapping_add(height).wrapping_sub(inset_bottom);
        top = top.wrapping_add(inset_top);

        let mut right = left.wrapping_add(width).wrapping_sub(inset_right);
        left = left.wrapping_add(inset_left);

        let mut parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

        while !parent.is_null() {
            let parent_top: i32 = jvm.invoke_virtual(&parent, "getYOnScreen", "()I", ()).await?;

            if top < parent_top {
                top = parent_top;
            }

            let parent_height: i32 = jvm.get_field(&parent, "h", "I").await?;
            let parent_bottom = parent_top.wrapping_add(parent_height);

            if bottom >= parent_bottom {
                bottom = parent_bottom;
            }

            let parent_left: i32 = jvm.invoke_virtual(&parent, "getXOnScreen", "()I", ()).await?;

            if left < parent_left {
                left = parent_left;
            }

            let parent_width: i32 = jvm.get_field(&parent, "w", "I").await?;
            let parent_right = parent_left.wrapping_add(parent_width);

            if parent_right < right {
                right = parent_right;
            }

            parent = jvm.get_field(&parent, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;
        }

        jvm.put_field(&mut this, "__wieFormViewportX", "I", left).await?;

        jvm.put_field(&mut this, "__wieFormViewportHeight", "I", bottom.wrapping_sub(top)).await?;

        jvm.put_field(&mut this, "__wieFormViewportY", "I", top).await?;

        jvm.put_field(&mut this, "__wieFormViewportWidth", "I", right.wrapping_sub(left)).await?;

        Ok(())
    }

    async fn layout_child_horizontal(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        let width: i32 = jvm.get_field(&this, "w", "I").await?;

        let mut preferred_width: i32 = jvm.get_field(&this, "prefW", "I").await?;

        if preferred_width < 0 {
            let _: () = jvm.invoke_virtual(&this, "calcPreferredSize", "(I)V", (width,)).await?;

            preferred_width = jvm.get_field(&this, "prefW", "I").await?;
        }

        let inset_top = jvm.get_field::<i16>(&this, "insetTop", "S").await? as i32;
        let inset_left = jvm.get_field::<i16>(&this, "insetLeft", "S").await? as i32;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        let gab: i32 = jvm.get_field(&this, "__wieFormGab", "I").await?;

        let old_overflow: bool = jvm.get_field(&this, "__wieContainerLayoutOverflow", "Z").await?;

        let overflow = preferred_width > width;

        jvm.put_field(&mut this, "__wieContainerLayoutOverflow", "Z", overflow).await?;

        if old_overflow && !overflow {
            jvm.put_field(&mut this, "offsetX", "I", 0i32).await?;
        }

        let available_height = height.wrapping_sub(inset_left).wrapping_sub(inset_top);

        let packed: bool = jvm.get_field(&this, "__wieFormPacked", "Z").await?;

        let mut x = inset_left.wrapping_add(gab / 2);

        if !overflow && width != preferred_width {
            x = width.wrapping_sub(preferred_width.wrapping_sub(gab)) / 2;
        }

        let y = inset_top;

        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if child_count > 0 {
            let children = jvm.get_field(&this, "children", "[Lorg/kwis/msp/lwc/Component;").await?;

            let mut index = 0i32;

            while index < child_count {
                let values: alloc::vec::Vec<ClassInstanceRef<()>> = jvm.load_array(&children, index as usize, 1).await?;

                let child = values[0].clone();

                if child.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                let preferred_child_height: i32 = jvm.invoke_virtual(&child, "getPreferredHeight", "()I", ()).await?;

                let preferred_child_width: i32 = jvm.invoke_virtual(&child, "getPreferredWidth", "()I", ()).await?;

                let child_height = if packed {
                    available_height
                } else {
                    core::cmp::min(preferred_child_height, available_height)
                };

                let _: () = jvm
                    .invoke_virtual(&child, "configure", "(IIIII)V", (x, y, preferred_child_width, child_height, 3i32))
                    .await?;

                let actual_width: i32 = jvm.invoke_virtual(&child, "getWidth", "()I", ()).await?;

                x = x.wrapping_add(gab).wrapping_add(actual_width);

                index += 1;
            }
        }

        if overflow {
            let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;
            mask |= 0x4;
            jvm.put_field(&mut this, "mask", "I", mask).await?;
            return Ok(());
        }

        let children = jvm.get_field(&this, "children", "[Lorg/kwis/msp/lwc/Component;").await?;

        let mut index = child_count - 1;
        let mut focusable = false;

        while index >= 0 {
            let values: alloc::vec::Vec<ClassInstanceRef<()>> = jvm.load_array(&children, index as usize, 1).await?;

            let child = values[0].clone();

            if child.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            let mask: i32 = jvm.get_field(&child, "mask", "I").await?;

            if mask & 0x4 != 0 {
                focusable = true;
                break;
            }

            index -= 1;
        }

        let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;

        if focusable {
            mask |= 0x4;
        } else {
            mask &= !0x4;
        }

        jvm.put_field(&mut this, "mask", "I", mask).await?;

        Ok(())
    }

    async fn layout_child_vertical(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        let width: i32 = jvm.get_field(&this, "w", "I").await?;

        let mut preferred_height: i32 = jvm.get_field(&this, "prefH", "I").await?;

        if preferred_height < 0 {
            let _: () = jvm.invoke_virtual(&this, "calcPreferredSize", "(I)V", (width,)).await?;

            preferred_height = jvm.get_field(&this, "prefH", "I").await?;
        }

        let inset_top = jvm.get_field::<i16>(&this, "insetTop", "S").await? as i32;
        let inset_bottom = jvm.get_field::<i16>(&this, "insetBottom", "S").await? as i32;
        let inset_left = jvm.get_field::<i16>(&this, "insetLeft", "S").await? as i32;
        let inset_right = jvm.get_field::<i16>(&this, "insetRight", "S").await? as i32;

        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        let gab: i32 = jvm.get_field(&this, "__wieFormGab", "I").await?;

        let old_overflow: bool = jvm.get_field(&this, "__wieContainerLayoutOverflow", "Z").await?;

        let overflow = preferred_height > height;

        jvm.put_field(&mut this, "__wieContainerLayoutOverflow", "Z", overflow).await?;

        if old_overflow && !overflow {
            jvm.put_field(&mut this, "offsetY", "I", 0i32).await?;
        }

        let available_width = width
            .wrapping_sub(inset_left)
            .wrapping_sub(inset_right)
            .wrapping_sub(if overflow { 5 } else { 0 });

        let packed: bool = jvm.get_field(&this, "__wieFormPacked", "Z").await?;

        let x = inset_left;
        let mut y = inset_top.wrapping_add(gab / 2);

        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;

        if child_count > 0 {
            let children = jvm.get_field(&this, "children", "[Lorg/kwis/msp/lwc/Component;").await?;

            let mut index = 0i32;

            while index < child_count {
                let values: alloc::vec::Vec<ClassInstanceRef<()>> = jvm.load_array(&children, index as usize, 1).await?;

                let child = values[0].clone();

                if child.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                let child_height: i32 = jvm.invoke_virtual(&child, "getPreferredHeight", "(I)I", (available_width,)).await?;

                let preferred_child_width: i32 = jvm.invoke_virtual(&child, "getPreferredWidth", "()I", ()).await?;

                let child_width = if available_width < 0 {
                    preferred_child_width
                } else if packed {
                    available_width
                } else {
                    core::cmp::min(available_width, preferred_child_width)
                };

                let _: () = jvm
                    .invoke_virtual(&child, "configure", "(IIIII)V", (x, y, child_width, child_height, 3i32))
                    .await?;

                let actual_height: i32 = jvm.invoke_virtual(&child, "getHeight", "()I", ()).await?;

                y = y.wrapping_add(gab).wrapping_add(actual_height);

                index += 1;
            }
        }

        jvm.put_field(&mut this, "prefH", "I", y.wrapping_add(inset_bottom)).await?;

        let scrollbar: ClassInstanceRef<()> = jvm.get_field(&this, "cmpScroll", "Lorg/kwis/msp/lwc/ScrollbarComponent;").await?;

        if scrollbar.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        if overflow {
            let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;

            mask |= 0x4;

            jvm.put_field(&mut this, "mask", "I", mask).await?;

            let scrollbar_height = height.wrapping_sub(2).wrapping_sub(inset_left).wrapping_sub(inset_top);

            let _: () = jvm
                .invoke_virtual(
                    &scrollbar,
                    "configure",
                    "(IIIII)V",
                    (
                        width.wrapping_sub(inset_right).wrapping_sub(5),
                        inset_top.wrapping_add(1),
                        5i32,
                        scrollbar_height,
                        3i32,
                    ),
                )
                .await?;

            let _: i32 = jvm.invoke_virtual(&scrollbar, "getMinimum", "()I", ()).await?;

            let _: i32 = jvm.invoke_virtual(&scrollbar, "getMaximum", "()I", ()).await?;

            let view_amount: i32 = jvm.invoke_virtual(&scrollbar, "getViewAmount", "()I", ()).await?;

            let form_preferred_height: i32 = jvm.invoke_virtual(&this, "getPreferredHeight", "()I", ()).await?;

            let mut form_height: i32 = jvm.get_field(&this, "h", "I").await?;

            if form_height < 1 {
                form_height = 1;
            }

            let current_value: i32 = jvm.invoke_virtual(&scrollbar, "getCurrentValue", "()I", ()).await?;

            let candidate_value = form_preferred_height.wrapping_sub(view_amount);

            if candidate_value < current_value {
                let _: () = jvm.invoke_virtual(&scrollbar, "setCurrentValue", "(I)V", (candidate_value,)).await?;
            }

            let _: () = jvm.invoke_virtual(&scrollbar, "setMaximum", "(I)V", (form_preferred_height,)).await?;

            let _: () = jvm.invoke_virtual(&scrollbar, "setViewAmount", "(I)V", (form_height,)).await?;
        } else {
            let _: () = jvm
                .invoke_virtual(&scrollbar, "configure", "(IIIII)V", (-1i32, -1i32, 1i32, 1i32, 3i32))
                .await?;

            let children = jvm.get_field(&this, "children", "[Lorg/kwis/msp/lwc/Component;").await?;

            let mut index = child_count - 1;
            let mut focusable = false;

            while index >= 0 {
                let values: alloc::vec::Vec<ClassInstanceRef<()>> = jvm.load_array(&children, index as usize, 1).await?;

                let child = values[0].clone();

                if child.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }

                let child_mask: i32 = jvm.get_field(&child, "mask", "I").await?;

                if child_mask & 0x4 != 0 {
                    focusable = true;
                    break;
                }

                index -= 1;
            }

            let mut mask: i32 = jvm.get_field(&this, "mask", "I").await?;

            if focusable {
                mask |= 0x4;
            } else {
                mask &= !0x4;
            }

            jvm.put_field(&mut this, "mask", "I", mask).await?;
        }

        let vertical: bool = jvm.get_field(&this, "__wieFormVertical", "Z").await?;

        if vertical {
            let viewport_height: i32 = jvm.get_field(&this, "__wieFormViewportHeight", "I").await?;

            let offset_y: i32 = jvm.get_field(&this, "offsetY", "I").await?;

            let preferred_height: i32 = jvm.get_field(&this, "prefH", "I").await?;

            let visible_end = viewport_height.wrapping_sub(offset_y);

            if visible_end > preferred_height {
                let _: bool = jvm
                    .invoke_virtual(&this, "scrollTo", "(II)Z", (0i32, visible_end.wrapping_sub(preferred_height)))
                    .await?;
            }
        } else {
            let viewport_width: i32 = jvm.get_field(&this, "__wieFormViewportWidth", "I").await?;

            let offset_x: i32 = jvm.get_field(&this, "offsetX", "I").await?;

            let preferred_width: i32 = jvm.get_field(&this, "prefW", "I").await?;

            let visible_end = viewport_width.wrapping_sub(offset_x);

            if visible_end > preferred_width {
                let _: bool = jvm
                    .invoke_virtual(&this, "scrollTo", "(II)Z", (visible_end.wrapping_sub(preferred_width), 0i32))
                    .await?;
            }
        }

        Ok(())
    }

    async fn get_prev_component(jvm: &Jvm, this: &ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Component>> {
        let child_count: i32 = jvm.get_field(this, "childCount", "I").await?;

        if child_count <= 0 {
            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        // Native 0x21e8e4..0x21e8ec:
        // refresh the visible Form viewport before traversal.
        Self::calc_view_port_area(jvm, this.clone()).await?;

        // Native keeps a Form-private traversal cursor at +0x68.
        // When ContainerComponent.focusComponent (+0x40) is non-null,
        // synchronize +0x68 from it before getIndexOf().
        let focus: ClassInstanceRef<Component> = jvm.get_field(this, "focusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

        if !focus.is_null() {
            let mut form = this.clone();

            jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", focus.clone())
                .await?;
        }

        let current: ClassInstanceRef<Component> = jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

        let current_index: i32 = jvm
            .invoke_virtual(this, "getIndexOf", "(Lorg/kwis/msp/lwc/Component;)I", (current.clone(),))
            .await?;

        let vertical: bool = jvm.get_field(this, "__wieFormVertical", "Z").await?;

        let viewport_start: i32 = if vertical {
            jvm.get_field(this, "__wieFormViewportY", "I").await?
        } else {
            jvm.get_field(this, "__wieFormViewportX", "I").await?
        };

        let viewport_size: i32 = if vertical {
            jvm.get_field(this, "__wieFormViewportHeight", "I").await?
        } else {
            jvm.get_field(this, "__wieFormViewportWidth", "I").await?
        };

        let viewport_end = viewport_start.wrapping_add(viewport_size);

        // Native 0x21e960..0x21eaa0:
        // only a focusable current cursor lying before the viewport
        // enters this correction path.  After any scroll, native reloads
        // both +0x68 and +0x40 before deciding what to return.
        if current_index >= 0 && !current.is_null() {
            let current_position: i32 = if vertical {
                jvm.invoke_virtual(&current, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&current, "getXOnScreen", "()I", ()).await?
            };

            let current_mask: i32 = jvm.get_field(&current, "mask", "I").await?;

            if current_position < viewport_start && current_mask & 0x4 != 0 {
                // Native calls getX/YOnScreen again before computing
                // the actual scroll distance.
                let corrected_position: i32 = if vertical {
                    jvm.invoke_virtual(&current, "getYOnScreen", "()I", ()).await?
                } else {
                    jvm.invoke_virtual(&current, "getXOnScreen", "()I", ()).await?
                };

                if corrected_position < viewport_start {
                    let actual = corrected_position.wrapping_sub(viewport_start);

                    let edge = 12i32.wrapping_sub(viewport_size);
                    let base = if edge > 11 { viewport_size.wrapping_neg() } else { edge };

                    let amount = if base < actual { actual } else { base };

                    let _: bool = if vertical {
                        jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                    } else {
                        jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                    };
                }

                let adjusted_current: ClassInstanceRef<Component> =
                    jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

                if adjusted_current.is_null() {
                    return Ok(adjusted_current);
                }

                let adjusted_mask: i32 = jvm.get_field(&adjusted_current, "mask", "I").await?;

                if adjusted_mask & 0x4 == 0 {
                    let adjusted_focus: ClassInstanceRef<Component> = jvm.get_field(this, "focusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

                    if !adjusted_focus.is_null() {
                        let focus_position: i32 = if vertical {
                            jvm.invoke_virtual(&adjusted_focus, "getYOnScreen", "()I", ()).await?
                        } else {
                            jvm.invoke_virtual(&adjusted_focus, "getXOnScreen", "()I", ()).await?
                        };

                        let focus_extent: i32 = if vertical {
                            jvm.get_field(&adjusted_focus, "h", "I").await?
                        } else {
                            jvm.get_field(&adjusted_focus, "w", "I").await?
                        };

                        let focus_end = focus_position.wrapping_add(focus_extent);

                        if viewport_start > focus_end || focus_position >= viewport_end {
                            let null_component = ClassInstanceRef::<Component>::new(None);

                            let _: () = jvm
                                .invoke_special(
                                    this,
                                    "org/kwis/msp/lwc/ContainerComponent",
                                    "setFocus",
                                    "(Lorg/kwis/msp/lwc/Component;)V",
                                    (null_component,),
                                )
                                .await?;
                        }
                    }

                    return Ok(ClassInstanceRef::<Component>::new(None));
                }

                return Ok(adjusted_current);
            }
        }

        // Native 0x21ebac..0x21ed18:
        // when getIndexOf() returns -1, first align only the final child
        // against the viewport end.  Native then performs a separate
        // backwards cursor scan rather than scrolling every child.
        if current_index < 0 {
            let last_index = child_count - 1;
            let last: ClassInstanceRef<Component> = jvm
                .invoke_virtual(this, "getComponent", "(I)Lorg/kwis/msp/lwc/Component;", (last_index,))
                .await?;

            if last.is_null() {
                return Ok(last);
            }

            let last_position: i32 = if vertical {
                jvm.invoke_virtual(&last, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&last, "getXOnScreen", "()I", ()).await?
            };

            let last_extent: i32 = if vertical {
                jvm.get_field(&last, "h", "I").await?
            } else {
                jvm.get_field(&last, "w", "I").await?
            };

            let last_end = last_position.wrapping_add(last_extent);

            if last_end > viewport_end {
                let amount = viewport_end.wrapping_sub(last_end);

                let _: bool = if vertical {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                } else {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                };
            }

            let mut scan_index = last_index;

            while scan_index >= 0 {
                let candidate: ClassInstanceRef<Component> = jvm
                    .invoke_virtual(this, "getComponent", "(I)Lorg/kwis/msp/lwc/Component;", (scan_index,))
                    .await?;

                if candidate.is_null() {
                    return Ok(candidate);
                }

                let position: i32 = if vertical {
                    jvm.invoke_virtual(&candidate, "getYOnScreen", "()I", ()).await?
                } else {
                    jvm.invoke_virtual(&candidate, "getXOnScreen", "()I", ()).await?
                };

                let mask: i32 = jvm.get_field(&candidate, "mask", "I").await?;

                // Native branches to +0x68 update when either the child
                // is focusable or the viewport start has passed it.
                if mask & 0x4 != 0 || viewport_start > position {
                    let mut form = this.clone();

                    jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", candidate.clone())
                        .await?;

                    if mask & 0x4 != 0 {
                        return Ok(candidate);
                    }

                    return Ok(ClassInstanceRef::<Component>::new(None));
                }

                scan_index -= 1;
            }

            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        // Native 0x21eb44..0x21ed34:
        // index 0 can re-enter the same current-cursor correction path
        // through 0x21ed1c -> 0x21e988.
        if current_index == 0 {
            if current.is_null() {
                return Ok(current);
            }

            let current_position: i32 = if vertical {
                jvm.invoke_virtual(&current, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&current, "getXOnScreen", "()I", ()).await?
            };

            let boundary = viewport_start.wrapping_sub(viewport_size).wrapping_add(12);

            let offset: i32 = if vertical {
                jvm.get_field(this, "offsetY", "I").await?
            } else {
                jvm.get_field(this, "offsetX", "I").await?
            };

            if current_position > boundary && offset >= 0 {
                return Ok(ClassInstanceRef::<Component>::new(None));
            }

            // Native 0x21ed1c..0x21ed34 reloads +0x68 and then
            // re-enters 0x21e988.
            let adjusted_current: ClassInstanceRef<Component> =
                jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

            if adjusted_current.is_null() {
                return Ok(adjusted_current);
            }

            let mut form = this.clone();
            jvm.put_field(
                &mut form,
                "__wieFormFocusComponent",
                "Lorg/kwis/msp/lwc/Component;",
                adjusted_current.clone(),
            )
            .await?;

            let corrected_position: i32 = if vertical {
                jvm.invoke_virtual(&adjusted_current, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&adjusted_current, "getXOnScreen", "()I", ()).await?
            };

            if corrected_position < viewport_start {
                let actual = corrected_position.wrapping_sub(viewport_start);

                let edge = 12i32.wrapping_sub(viewport_size);
                let base = if edge > 11 { viewport_size.wrapping_neg() } else { edge };

                let amount = if base < actual { actual } else { base };

                let _: bool = if vertical {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                } else {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                };
            }

            let final_current: ClassInstanceRef<Component> = jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

            if final_current.is_null() {
                return Ok(final_current);
            }

            let final_mask: i32 = jvm.get_field(&final_current, "mask", "I").await?;

            if final_mask & 0x4 == 0 {
                let final_focus: ClassInstanceRef<Component> = jvm.get_field(this, "focusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

                if !final_focus.is_null() {
                    let focus_position: i32 = if vertical {
                        jvm.invoke_virtual(&final_focus, "getYOnScreen", "()I", ()).await?
                    } else {
                        jvm.invoke_virtual(&final_focus, "getXOnScreen", "()I", ()).await?
                    };

                    let focus_extent: i32 = if vertical {
                        jvm.get_field(&final_focus, "h", "I").await?
                    } else {
                        jvm.get_field(&final_focus, "w", "I").await?
                    };

                    let focus_end = focus_position.wrapping_add(focus_extent);

                    if viewport_start > focus_end || focus_position >= viewport_end {
                        let null_component = ClassInstanceRef::<Component>::new(None);

                        let _: () = jvm
                            .invoke_special(
                                this,
                                "org/kwis/msp/lwc/ContainerComponent",
                                "setFocus",
                                "(Lorg/kwis/msp/lwc/Component;)V",
                                (null_component,),
                            )
                            .await?;
                    }
                }

                return Ok(ClassInstanceRef::<Component>::new(None));
            }

            return Ok(final_current);
        }

        // Native 0x21ed6c..0x21eea8:
        // scan backwards until either a focusable child is found or the
        // viewport start has passed the candidate.  Only that selected
        // candidate becomes the Form-private traversal cursor.
        let mut index = current_index - 1;

        while index >= 0 {
            let candidate: ClassInstanceRef<Component> = jvm
                .invoke_virtual(this, "getComponent", "(I)Lorg/kwis/msp/lwc/Component;", (index,))
                .await?;

            if candidate.is_null() {
                return Ok(candidate);
            }

            let position: i32 = if vertical {
                jvm.invoke_virtual(&candidate, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&candidate, "getXOnScreen", "()I", ()).await?
            };

            let mask: i32 = jvm.get_field(&candidate, "mask", "I").await?;

            if mask & 0x4 == 0 && viewport_start <= position {
                index -= 1;
                continue;
            }

            let mut form = this.clone();

            jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", candidate.clone())
                .await?;

            // Native 0x21eea4 -> 0x21ed28 -> 0x21e988:
            // after selecting a previous candidate, reload +0x68 and
            // run the same current-cursor correction/focus path.
            let adjusted_current: ClassInstanceRef<Component> =
                jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

            if adjusted_current.is_null() {
                return Ok(adjusted_current);
            }

            let corrected_position: i32 = if vertical {
                jvm.invoke_virtual(&adjusted_current, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&adjusted_current, "getXOnScreen", "()I", ()).await?
            };

            if corrected_position < viewport_start {
                let actual = corrected_position.wrapping_sub(viewport_start);

                let edge = 12i32.wrapping_sub(viewport_size);
                let base = if edge > 11 { viewport_size.wrapping_neg() } else { edge };

                let amount = if base < actual { actual } else { base };

                let _: bool = if vertical {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                } else {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                };
            }

            let final_current: ClassInstanceRef<Component> = jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

            if final_current.is_null() {
                return Ok(final_current);
            }

            let final_mask: i32 = jvm.get_field(&final_current, "mask", "I").await?;

            if final_mask & 0x4 == 0 {
                let final_focus: ClassInstanceRef<Component> = jvm.get_field(this, "focusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

                if !final_focus.is_null() {
                    let focus_position: i32 = if vertical {
                        jvm.invoke_virtual(&final_focus, "getYOnScreen", "()I", ()).await?
                    } else {
                        jvm.invoke_virtual(&final_focus, "getXOnScreen", "()I", ()).await?
                    };

                    let focus_extent: i32 = if vertical {
                        jvm.get_field(&final_focus, "h", "I").await?
                    } else {
                        jvm.get_field(&final_focus, "w", "I").await?
                    };

                    let focus_end = focus_position.wrapping_add(focus_extent);

                    if viewport_start > focus_end || focus_position >= viewport_end {
                        let null_component = ClassInstanceRef::<Component>::new(None);

                        let _: () = jvm
                            .invoke_special(
                                this,
                                "org/kwis/msp/lwc/ContainerComponent",
                                "setFocus",
                                "(Lorg/kwis/msp/lwc/Component;)V",
                                (null_component,),
                            )
                            .await?;
                    }
                }

                return Ok(ClassInstanceRef::<Component>::new(None));
            }

            return Ok(final_current);
        }

        Ok(ClassInstanceRef::<Component>::new(None))
    }

    async fn get_next_component(jvm: &Jvm, this: &ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Component>> {
        let child_count: i32 = jvm.get_field(this, "childCount", "I").await?;

        if child_count <= 0 {
            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        Self::calc_view_port_area(jvm, this.clone()).await?;

        let focus: ClassInstanceRef<Component> = jvm.get_field(this, "focusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

        if !focus.is_null() {
            let mut form = this.clone();

            jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", focus.clone())
                .await?;
        }

        let current: ClassInstanceRef<Component> = jvm.get_field(this, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;").await?;

        let current_index: i32 = jvm
            .invoke_virtual(this, "getIndexOf", "(Lorg/kwis/msp/lwc/Component;)I", (current.clone(),))
            .await?;

        let vertical: bool = jvm.get_field(this, "__wieFormVertical", "Z").await?;

        let viewport_start: i32 = if vertical {
            jvm.get_field(this, "__wieFormViewportY", "I").await?
        } else {
            jvm.get_field(this, "__wieFormViewportX", "I").await?
        };

        let viewport_size: i32 = if vertical {
            jvm.get_field(this, "__wieFormViewportHeight", "I").await?
        } else {
            jvm.get_field(this, "__wieFormViewportWidth", "I").await?
        };

        let viewport_end = viewport_start.wrapping_add(viewport_size);

        // Native 0x21f0c8..0x21f3a0:
        // before advancing to the next child, decide whether the current
        // cursor itself must remain selected.  A cursor extending beyond
        // the second viewport minus 12 pixels is retained.  The final
        // child is also retained only when it still extends beyond the
        // visible viewport.
        if current_index >= 0 {
            if current.is_null() {
                return Ok(current);
            }

            let current_position: i32 = if vertical {
                jvm.invoke_virtual(&current, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&current, "getXOnScreen", "()I", ()).await?
            };

            let current_extent: i32 = if vertical {
                jvm.get_field(&current, "h", "I").await?
            } else {
                jvm.get_field(&current, "w", "I").await?
            };

            let current_end = current_position.wrapping_add(current_extent);

            let current_mask: i32 = jvm.get_field(&current, "mask", "I").await?;

            let retain_boundary = viewport_start.wrapping_add(viewport_size.wrapping_mul(2)).wrapping_sub(12);

            let last_index = child_count - 1;

            // Native 0x21f004..0x21f028:
            // a focusable current cursor extending beyond the visible
            // viewport is retained immediately, before the later
            // second-viewport boundary test.
            let retain_current = (current_mask & 0x4 != 0 && current_end > viewport_end)
                || current_end > retain_boundary
                || (current_index >= last_index && current_end > viewport_end);

            if current_index >= last_index && !retain_current {
                return Ok(ClassInstanceRef::<Component>::new(None));
            }

            if retain_current {
                let mut form = this.clone();

                jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", current.clone())
                    .await?;

                if current_end > viewport_end {
                    let overflow = current_end.wrapping_sub(viewport_end);

                    let reduced = viewport_size.wrapping_sub(12);
                    let cap = if reduced > 12 { reduced } else { viewport_size };
                    let amount = if overflow > cap { cap } else { overflow };

                    let _: bool = if vertical {
                        jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                    } else {
                        jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                    };
                }

                // Native 0x21f430..0x21f4b4:
                // after +0x68 selects the traversal cursor, a non-focusable
                // cursor may clear only ContainerComponent.focusComponent
                // when that focus lies outside the visible viewport.
                if current_mask & 0x4 == 0 && !focus.is_null() {
                    let focus_position: i32 = if vertical {
                        jvm.invoke_virtual(&focus, "getYOnScreen", "()I", ()).await?
                    } else {
                        jvm.invoke_virtual(&focus, "getXOnScreen", "()I", ()).await?
                    };

                    let focus_extent: i32 = if vertical {
                        jvm.get_field(&focus, "h", "I").await?
                    } else {
                        jvm.get_field(&focus, "w", "I").await?
                    };

                    let focus_end = focus_position.wrapping_add(focus_extent);

                    if viewport_start > focus_end || focus_position >= viewport_end {
                        let null_component = ClassInstanceRef::<Component>::new(None);

                        let _: () = jvm
                            .invoke_special(
                                this,
                                "org/kwis/msp/lwc/ContainerComponent",
                                "setFocus",
                                "(Lorg/kwis/msp/lwc/Component;)V",
                                (null_component,),
                            )
                            .await?;
                    }
                }

                if current_mask & 0x4 != 0 {
                    return Ok(current);
                }

                return Ok(ClassInstanceRef::<Component>::new(None));
            }
        }

        // Native 0x21f274..0x21f388:
        // with no current child, scan forward from index 0.  Native
        // selects the first focusable child, or a child extending beyond
        // the second viewport boundary.
        if current_index < 0 {
            let second_viewport_end = viewport_start.wrapping_add(viewport_size.wrapping_mul(2));

            let mut scan_index = 0;

            while scan_index < child_count {
                let candidate: ClassInstanceRef<Component> = jvm
                    .invoke_virtual(this, "getComponent", "(I)Lorg/kwis/msp/lwc/Component;", (scan_index,))
                    .await?;

                if candidate.is_null() {
                    return Ok(candidate);
                }

                let position: i32 = if vertical {
                    jvm.invoke_virtual(&candidate, "getYOnScreen", "()I", ()).await?
                } else {
                    jvm.invoke_virtual(&candidate, "getXOnScreen", "()I", ()).await?
                };

                let extent: i32 = if vertical {
                    jvm.get_field(&candidate, "h", "I").await?
                } else {
                    jvm.get_field(&candidate, "w", "I").await?
                };

                let candidate_end = position.wrapping_add(extent);

                let mask: i32 = jvm.get_field(&candidate, "mask", "I").await?;

                if mask & 0x4 == 0 && candidate_end <= second_viewport_end {
                    scan_index += 1;
                    continue;
                }

                if candidate_end > viewport_end {
                    let overflow = candidate_end.wrapping_sub(viewport_end);

                    let reduced = viewport_size.wrapping_sub(12);
                    let cap = if reduced > 12 { reduced } else { viewport_size };
                    let amount = if overflow > cap { cap } else { overflow };

                    let _: bool = if vertical {
                        jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                    } else {
                        jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                    };
                }

                let mut form = this.clone();

                jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", candidate.clone())
                    .await?;

                if mask & 0x4 != 0 {
                    return Ok(candidate);
                }

                return Ok(ClassInstanceRef::<Component>::new(None));
            }

            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        // Native current_index >= 0 path starts immediately after
        // the current child.
        let mut index = current_index + 1;

        while index < child_count {
            let candidate: ClassInstanceRef<Component> = jvm
                .invoke_virtual(this, "getComponent", "(I)Lorg/kwis/msp/lwc/Component;", (index,))
                .await?;

            if candidate.is_null() {
                return Ok(candidate);
            }

            let position: i32 = if vertical {
                jvm.invoke_virtual(&candidate, "getYOnScreen", "()I", ()).await?
            } else {
                jvm.invoke_virtual(&candidate, "getXOnScreen", "()I", ()).await?
            };

            let extent: i32 = if vertical {
                jvm.get_field(&candidate, "h", "I").await?
            } else {
                jvm.get_field(&candidate, "w", "I").await?
            };

            let candidate_end = position.wrapping_add(extent);

            let mask: i32 = jvm.get_field(&candidate, "mask", "I").await?;

            let second_viewport_end = viewport_start.wrapping_add(viewport_size.wrapping_mul(2));

            // Native 0x21f118..0x21f1b0:
            // keep scanning only while the child is non-focusable and
            // still ends inside the second viewport boundary.
            if mask & 0x4 == 0 && candidate_end <= second_viewport_end {
                index += 1;
                continue;
            }

            // Native selects this candidate as the Form-private cursor
            // before final focusability handling.
            let mut form = this.clone();

            jvm.put_field(&mut form, "__wieFormFocusComponent", "Lorg/kwis/msp/lwc/Component;", candidate.clone())
                .await?;

            if candidate_end > viewport_end {
                let overflow = candidate_end.wrapping_sub(viewport_end);

                let reduced = viewport_size.wrapping_sub(12);
                let cap = if reduced > 12 { reduced } else { viewport_size };
                let amount = if overflow > cap { cap } else { overflow };

                let _: bool = if vertical {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (0i32, amount)).await?
                } else {
                    jvm.invoke_virtual(this, "scrollTo", "(II)Z", (amount, 0i32)).await?
                };
            }

            // Native 0x21f430..0x21f4b4:
            // after +0x68 selects the traversal cursor, a non-focusable
            // cursor may clear only ContainerComponent.focusComponent
            // when that focus lies outside the visible viewport.
            if mask & 0x4 == 0 && !focus.is_null() {
                let focus_position: i32 = if vertical {
                    jvm.invoke_virtual(&focus, "getYOnScreen", "()I", ()).await?
                } else {
                    jvm.invoke_virtual(&focus, "getXOnScreen", "()I", ()).await?
                };

                let focus_extent: i32 = if vertical {
                    jvm.get_field(&focus, "h", "I").await?
                } else {
                    jvm.get_field(&focus, "w", "I").await?
                };

                let focus_end = focus_position.wrapping_add(focus_extent);

                if viewport_start > focus_end || focus_position >= viewport_end {
                    let null_component = ClassInstanceRef::<Component>::new(None);

                    let _: () = jvm
                        .invoke_special(
                            this,
                            "org/kwis/msp/lwc/ContainerComponent",
                            "setFocus",
                            "(Lorg/kwis/msp/lwc/Component;)V",
                            (null_component,),
                        )
                        .await?;
                }
            }

            if mask & 0x4 != 0 {
                return Ok(candidate);
            }

            return Ok(ClassInstanceRef::<Component>::new(None));
        }

        Ok(ClassInstanceRef::<Component>::new(None))
    }

    async fn get_prev_traversal_component(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Component>> {
        let mut candidate = Self::get_prev_component(jvm, &this).await?;

        if candidate.is_null() {
            return Ok(candidate);
        }

        while jvm.is_instance(&**candidate, "org/kwis/msp/lwc/ContainerComponent") {
            let last = candidate.clone();

            candidate = jvm
                .invoke_virtual(&candidate, "getPrevTraversalComponent", "()Lorg/kwis/msp/lwc/Component;", ())
                .await?;

            if candidate.is_null() {
                return Ok(last);
            }
        }

        Ok(candidate)
    }

    async fn get_next_traversal_component(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Component>> {
        let mut candidate = Self::get_next_component(jvm, &this).await?;

        if candidate.is_null() {
            return Ok(candidate);
        }

        while jvm.is_instance(&**candidate, "org/kwis/msp/lwc/ContainerComponent") {
            let last = candidate.clone();

            candidate = jvm
                .invoke_virtual(&candidate, "getNextTraversalComponent", "()Lorg/kwis/msp/lwc/Component;", ())
                .await?;

            if candidate.is_null() {
                return Ok(last);
            }
        }

        Ok(candidate)
    }

    async fn scroll_to(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, x: i32, y: i32) -> JvmResult<bool> {
        let vertical: bool = jvm.get_field(&this, "__wieFormVertical", "Z").await?;

        if !vertical {
            let width: i32 = jvm.get_field(&this, "w", "I").await?;
            let inset_left = jvm.get_field::<i16>(&this, "insetLeft", "S").await? as i32;
            let inset_right = jvm.get_field::<i16>(&this, "insetRight", "S").await? as i32;
            let available = width.wrapping_sub(inset_left).wrapping_sub(inset_right);
            let preferred: i32 = jvm.invoke_virtual(&this, "getPreferredWidth", "()I", ()).await?;

            if available >= preferred {
                return jvm
                    .invoke_special(&this, "org/kwis/msp/lwc/ContainerComponent", "scrollTo", "(II)Z", (x, y))
                    .await;
            }

            let desired = x.wrapping_neg();
            let minimum = available.wrapping_sub(preferred);
            let clamped = if desired > 0 {
                0
            } else if desired < minimum {
                minimum
            } else {
                desired
            };
            let remainder = desired.wrapping_sub(clamped);

            jvm.put_field(&mut this, "offsetX", "I", clamped).await?;

            let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;

            let result = if remainder != 0 && !parent.is_null() {
                jvm.invoke_virtual(&parent, "scrollTo", "(II)Z", (remainder, 0i32)).await?
            } else {
                true
            };

            let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;
            return Ok(result);
        }

        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        let inset_top = jvm.get_field::<i16>(&this, "insetTop", "S").await? as i32;
        let inset_bottom = jvm.get_field::<i16>(&this, "insetBottom", "S").await? as i32;
        let available = height.wrapping_sub(inset_top).wrapping_sub(inset_bottom);
        let preferred: i32 = jvm.invoke_virtual(&this, "getPreferredHeight", "()I", ()).await?;

        if available >= preferred {
            return jvm
                .invoke_special(&this, "org/kwis/msp/lwc/ContainerComponent", "scrollTo", "(II)Z", (x, y))
                .await;
        }

        let old_offset_y: i32 = jvm.get_field(&this, "offsetY", "I").await?;
        jvm.put_field(&mut this, "offsetX", "I", 0i32).await?;

        let desired = old_offset_y.wrapping_sub(y);
        let minimum = available.wrapping_sub(preferred);
        let clamped = if desired > 0 {
            0
        } else if desired < minimum {
            minimum
        } else {
            desired
        };
        let scrollbar_value = clamped.wrapping_neg();
        let remainder = desired.wrapping_sub(clamped);

        jvm.put_field(&mut this, "offsetY", "I", clamped).await?;

        let scrollbar: ClassInstanceRef<()> = jvm.get_field(&this, "cmpScroll", "Lorg/kwis/msp/lwc/ScrollbarComponent;").await?;
        if scrollbar.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }
        let _: () = jvm.invoke_virtual(&scrollbar, "setCurrentValue", "(I)V", (scrollbar_value,)).await?;

        let parent: ClassInstanceRef<()> = jvm.get_field(&this, "parent", "Lorg/kwis/msp/lwc/ContainerComponent;").await?;
        let result = if remainder != 0 && !parent.is_null() {
            jvm.invoke_virtual(&parent, "scrollTo", "(II)Z", (0i32, remainder)).await?
        } else {
            true
        };

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;
        Ok(result)
    }

    async fn paint(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, graphics: ClassInstanceRef<()>) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "paint",
                "(Lorg/kwis/msp/lcdui/Graphics;)V",
                (graphics.clone(),),
            )
            .await?;

        let preferred_height: i32 = jvm.invoke_virtual(&this, "getPreferredHeight", "()I", ()).await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        if preferred_height <= height {
            return Ok(());
        }

        let scrollbar: ClassInstanceRef<()> = jvm.get_field(&this, "cmpScroll", "Lorg/kwis/msp/lwc/ScrollbarComponent;").await?;
        if scrollbar.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let x: i32 = jvm.get_field(&scrollbar, "x", "I").await?;
        let y: i32 = jvm.get_field(&scrollbar, "y", "I").await?;
        let _: () = jvm.invoke_virtual(&graphics, "translate", "(II)V", (x, y)).await?;
        let _: () = jvm
            .invoke_virtual(&scrollbar, "paint", "(Lorg/kwis/msp/lcdui/Graphics;)V", (graphics.clone(),))
            .await?;

        // Native 0x2206fc..0x220728 reloads cmpScroll and its
        // post-paint position before restoring the Graphics origin.
        let scrollbar: ClassInstanceRef<()> = jvm.get_field(&this, "cmpScroll", "Lorg/kwis/msp/lwc/ScrollbarComponent;").await?;
        if scrollbar.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let x: i32 = jvm.get_field(&scrollbar, "x", "I").await?;
        let y: i32 = jvm.get_field(&scrollbar, "y", "I").await?;

        let _: () = jvm
            .invoke_virtual(&graphics, "translate", "(II)V", (x.wrapping_neg(), y.wrapping_neg()))
            .await?;
        Ok(())
    }

    async fn calc_preferred_size(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, width: i32) -> JvmResult<()> {
        let gab: i32 = jvm.get_field(&this, "__wieFormGab", "I").await?;
        let vertical: bool = jvm.get_field(&this, "__wieFormVertical", "Z").await?;
        let inset_top = jvm.get_field::<i16>(&this, "insetTop", "S").await? as i32;
        let inset_bottom = jvm.get_field::<i16>(&this, "insetBottom", "S").await? as i32;
        let inset_left = jvm.get_field::<i16>(&this, "insetLeft", "S").await? as i32;
        let inset_right = jvm.get_field::<i16>(&this, "insetRight", "S").await? as i32;
        let child_count: i32 = jvm.get_field(&this, "childCount", "I").await?;
        let children = jvm.get_field(&this, "children", "[Lorg/kwis/msp/lwc/Component;").await?;

        if !vertical {
            let mut sum_width = 0i32;
            let mut max_height = 0i32;
            let mut index = 0i32;
            while index < child_count {
                let values: alloc::vec::Vec<ClassInstanceRef<()>> = jvm.load_array(&children, index as usize, 1).await?;
                let child = values[0].clone();
                if child.is_null() {
                    return Err(jvm.exception("java/lang/NullPointerException", "").await);
                }
                let child_width: i32 = jvm.invoke_virtual(&child, "getPreferredWidth", "()I", ()).await?;
                let child_height: i32 = jvm.invoke_virtual(&child, "getPreferredHeight", "()I", ()).await?;
                sum_width = sum_width.wrapping_add(child_width).wrapping_add(gab);
                if child_height > max_height {
                    max_height = child_height;
                }
                index += 1;
            }
            jvm.put_field(&mut this, "prefW", "I", inset_left.wrapping_add(inset_right).wrapping_add(sum_width))
                .await?;
            jvm.put_field(&mut this, "prefH", "I", inset_top.wrapping_add(inset_bottom).wrapping_add(max_height))
                .await?;
            return Ok(());
        }

        let available_width = {
            let value = width.wrapping_sub(inset_left).wrapping_sub(inset_right);
            if value < 0 { -1 } else { value }
        };
        let mut total_height = inset_top;
        let mut max_width = 0i32;
        let mut index = 0i32;
        while index < child_count {
            let values: alloc::vec::Vec<ClassInstanceRef<()>> = jvm.load_array(&children, index as usize, 1).await?;
            let child = values[0].clone();
            if child.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }
            let child_height: i32 = jvm.invoke_virtual(&child, "getPreferredHeight", "(I)I", (available_width,)).await?;
            let child_width: i32 = jvm.invoke_virtual(&child, "getPreferredWidth", "()I", ()).await?;
            total_height = total_height.wrapping_add(child_height).wrapping_add(gab);
            if child_width > max_width {
                max_width = child_width;
            }
            index += 1;
        }
        if child_count > 0 {
            total_height = total_height.wrapping_sub(gab / 2);
        }
        jvm.put_field(&mut this, "prefW", "I", inset_left.wrapping_add(inset_right).wrapping_add(max_width))
            .await?;
        jvm.put_field(&mut this, "prefH", "I", total_height.wrapping_add(inset_bottom)).await?;
        Ok(())
    }

    async fn key_notify(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, event_type: i32, key: i32) -> JvmResult<bool> {
        let game_action: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getGameAction", "(I)I", (key,)).await?;

        let fallback = match game_action {
            1 | 2 => {
                let offset_x: i32 = jvm.get_field(&this, "offsetX", "I").await?;
                offset_x < 0
            }
            5 | 6 => {
                let vertical: bool = jvm.get_field(&this, "__wieFormVertical", "Z").await?;
                if vertical {
                    let height: i32 = jvm.get_field(&this, "h", "I").await?;
                    let offset_y: i32 = jvm.get_field(&this, "offsetY", "I").await?;
                    let preferred: i32 = jvm.invoke_virtual(&this, "getPreferredHeight", "()I", ()).await?;
                    height.wrapping_sub(offset_y) < preferred
                } else {
                    let width: i32 = jvm.get_field(&this, "w", "I").await?;
                    let offset_x: i32 = jvm.get_field(&this, "offsetX", "I").await?;
                    let preferred: i32 = jvm.invoke_virtual(&this, "getPreferredWidth", "()I", ()).await?;
                    width.wrapping_sub(offset_x) < preferred
                }
            }
            _ => false,
        };

        let handled: bool = jvm
            .invoke_special(&this, "org/kwis/msp/lwc/ContainerComponent", "keyNotify", "(II)Z", (event_type, key))
            .await?;
        Ok(fallback || handled)
    }

    async fn layout(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        let vertical: bool = jvm.get_field(&this, "__wieFormVertical", "Z").await?;

        if vertical {
            // Native vtable +0x118.
            jvm.invoke_virtual(&this, "layoutChildVertical", "()V", ()).await
        } else {
            // Native vtable +0x114.
            jvm.invoke_virtual(&this, "layoutChildHorizontal", "()V", ()).await
        }
    }
}
