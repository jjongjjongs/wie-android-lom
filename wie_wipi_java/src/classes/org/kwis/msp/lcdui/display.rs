use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use java_runtime::classes::java::lang::{Object, Runnable, String};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use wie_midp::classes::javax::microedition::lcdui::Display as MidpDisplay;

use crate::classes::{
    net::wie::WIPIKeyCode,
    org::kwis::msp::lcdui::{Card, Image, Jlet, JletEventListener},
};

// class org.kwis.msp.lcdui.Display
pub struct Display;

impl Display {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/Display",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Jlet;Lorg/kwis/msp/lcdui/DisplayProxy;)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getDisplay",
                    "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Display;",
                    Self::get_display,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getDefaultDisplay",
                    "()Lorg/kwis/msp/lcdui/Display;",
                    Self::get_default_display,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("getActivatedIndex", "()I", Self::get_activated_index, MethodAccessFlags::STATIC),
                JavaMethodProto::new("activateCurrentDisplay", "()I", Self::activate_current_display, Default::default()),
                JavaMethodProto::new("isDoubleBuffered", "()Z", Self::is_double_buffered, Default::default()),
                JavaMethodProto::new("getDockedCard", "()Lorg/kwis/msp/lcdui/Card;", Self::get_docked_card, Default::default()),
                JavaMethodProto::new(
                    "setDockedCard",
                    "(Lorg/kwis/msp/lcdui/Card;I)V",
                    Self::set_docked_card,
                    Default::default(),
                ),
                JavaMethodProto::new("pushCard", "(Lorg/kwis/msp/lcdui/Card;)V", Self::push_card, Default::default()),
                JavaMethodProto::new("popCard", "()Lorg/kwis/msp/lcdui/Card;", Self::pop_card, Default::default()),
                JavaMethodProto::new("removeCard", "(Lorg/kwis/msp/lcdui/Card;)Z", Self::remove_card, Default::default()),
                JavaMethodProto::new("countCard", "()I", Self::count_card, Default::default()),
                JavaMethodProto::new("removeAllCards", "()V", Self::remove_all_cards, Default::default()),
                JavaMethodProto::new(
                    "addJletEventListener",
                    "(Lorg/kwis/msp/lcdui/JletEventListener;)V",
                    Self::add_jlet_event_listener,
                    Default::default(),
                ),
                JavaMethodProto::new("getWidth", "()I", Self::get_width, Default::default()),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, Default::default()),
                JavaMethodProto::new("callSerially", "(Ljava/lang/Runnable;)V", Self::call_serially, Default::default()),
                JavaMethodProto::new(
                    "callSerially",
                    "(Ljava/lang/Runnable;I)V",
                    Self::call_serially_with_timeout,
                    Default::default(),
                ),
                JavaMethodProto::new("isColor", "()Z", Self::is_color, Default::default()),
                JavaMethodProto::new("numColors", "()I", Self::num_colors, Default::default()),
                JavaMethodProto::new("hasPointerEvents", "()Z", Self::has_pointer_events, Default::default()),
                JavaMethodProto::new("hasPointerMotionEvents", "()Z", Self::has_pointer_motion_events, Default::default()),
                JavaMethodProto::new("hasRepeatEvents", "()Z", Self::has_repeat_events, Default::default()),
                JavaMethodProto::new("getKeyName", "(I)Ljava/lang/String;", Self::get_key_name, MethodAccessFlags::STATIC),
                JavaMethodProto::new("getBitsPerPixel", "()I", Self::get_bits_per_pixel, Default::default()),
                JavaMethodProto::new("flush", "()V", Self::flush, Default::default()),
                JavaMethodProto::new(
                    "removeJletEventListener",
                    "(Lorg/kwis/msp/lcdui/JletEventListener;)V",
                    Self::remove_jlet_event_listener,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "grabKey",
                    "(ILorg/kwis/msp/lcdui/JletEventListener;)V",
                    Self::grab_key,
                    Default::default(),
                ),
                JavaMethodProto::new("ungrabKey", "(I)V", Self::ungrab_key, Default::default()),
                JavaMethodProto::new(
                    "grabKey0",
                    "(II)V",
                    Self::grab_key0,
                    MethodAccessFlags::NATIVE | MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "ungrabKey0",
                    "(I)V",
                    Self::ungrab_key0,
                    MethodAccessFlags::NATIVE | MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("getRotation", "()I", Self::get_rotation, Default::default()),
                JavaMethodProto::new(
                    "setAnnunBackground",
                    "(Lorg/kwis/msp/lcdui/Image;)V",
                    Self::set_annun_background,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setAnnunBackgroundDimmingAlpha",
                    "(I)V",
                    Self::set_annun_background_dimming_alpha,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getGameAction",
                    "(I)I",
                    Self::get_game_action,
                    MethodAccessFlags::NATIVE | MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getKeyCode",
                    "(I)I",
                    Self::get_key_code,
                    MethodAccessFlags::NATIVE | MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![
                JavaFieldProto::new("annunBackground", "Lorg/kwis/msp/lcdui/Image;", Default::default()),
                JavaFieldProto::new("annunBackgroundAlpha", "I", Default::default()),
                JavaFieldProto::new("midpDisplay", "Ljavax/microedition/lcdui/Display;", Default::default()),
                JavaFieldProto::new("cardCanvas", "Lnet/wie/CardCanvas;", Default::default()),
                JavaFieldProto::new("dockedCard", "Lorg/kwis/msp/lcdui/Card;", Default::default()),
                // WIE-private storage for native Display instance +0x18.
                JavaFieldProto::new("__wieDisplayIndex", "I", Default::default()),
                // WIE-private storage for native Display class-static +0x50.
                JavaFieldProto::new("__wieActivatedIndex", "I", FieldAccessFlags::STATIC),
                // Native Display class-static +0x28 / +0x2c:
                // JletEventListener[] plus the number of active entries.
                JavaFieldProto::new(
                    "__wieJletEventListeners",
                    "[Lorg/kwis/msp/lcdui/JletEventListener;",
                    FieldAccessFlags::STATIC,
                ),
                JavaFieldProto::new("__wieJletEventListenerCount", "I", FieldAccessFlags::STATIC),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        jlet: ClassInstanceRef<Jlet>,
        display_proxy: ClassInstanceRef<Object>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::<init>({this:?}, {jlet:?}, {display_proxy:?})");

        let midlet = Jlet::midlet(jvm, &jlet).await?;

        let midp_display: ClassInstanceRef<MidpDisplay> = jvm
            .invoke_static(
                "javax/microedition/lcdui/Display",
                "getDisplay",
                "(Ljavax/microedition/midlet/MIDlet;)Ljavax/microedition/lcdui/Display;",
                (midlet,),
            )
            .await?;

        jvm.put_field(&mut this, "midpDisplay", "Ljavax/microedition/lcdui/Display;", midp_display.clone())
            .await?;

        let card_canvas = jvm.new_class("net/wie/CardCanvas", "()V", ()).await?;
        jvm.put_field(&mut this, "cardCanvas", "Lnet/wie/CardCanvas;", card_canvas.clone())
            .await?;

        let _: () = jvm
            .invoke_virtual(&midp_display, "setCurrent", "(Ljavax/microedition/lcdui/Displayable;)V", (card_canvas,))
            .await?;

        // The current WIE construction path creates the default Display.
        // Native Display.getDisplay(null) maps to display index 0.
        jvm.put_field(&mut this, "__wieDisplayIndex", "I", 0i32).await?;

        Ok(())
    }

    async fn get_display(jvm: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<ClassInstanceRef<Self>> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getDisplay({name:?})");

        let jlet: ClassInstanceRef<Jlet> = jvm
            .invoke_static("org/kwis/msp/lcdui/Jlet", "getActiveJlet", "()Lorg/kwis/msp/lcdui/Jlet;", ())
            .await?;

        // Native first asks Jlet.getDisplay(name) for an existing cached
        // default/dual/rotated Display.
        let cached: ClassInstanceRef<Display> = jvm
            .invoke_virtual(&jlet, "getDisplay", "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Display;", (name.clone(),))
            .await?;

        if !cached.is_null() {
            return Ok(cached);
        }

        let index = if name.is_null() {
            0
        } else {
            let name_str = JavaLangString::to_rust_string(jvm, &name).await?;

            match name_str.as_ref() {
                "dual" => 1,
                "rotated" => 3,
                _ => return Ok(None.into()),
            }
        };

        // WIE uses the same MIDP backend for all native Display indices.
        // Construct the wrapper normally, then retain the native index in
        // WIE-private storage corresponding to native instance +0x18.
        let mut display: ClassInstanceRef<Display> = jvm
            .new_class(
                "org/kwis/msp/lcdui/Display",
                "(Lorg/kwis/msp/lcdui/Jlet;Lorg/kwis/msp/lcdui/DisplayProxy;)V",
                (jlet.clone(), None),
            )
            .await?
            .into();

        jvm.put_field(&mut display, "__wieDisplayIndex", "I", index).await?;

        match index {
            1 => {
                jvm.put_field(&mut jlet.clone(), "dualDis", "Lorg/kwis/msp/lcdui/Display;", display.clone())
                    .await?;
            }
            3 => {
                let _: () = jvm
                    .invoke_virtual(&jlet, "setRotatedDisplay", "(Lorg/kwis/msp/lcdui/Display;)V", (display.clone(),))
                    .await?;
            }
            _ => {}
        }

        Ok(display)
    }

    async fn get_default_display(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<ClassInstanceRef<Display>> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getDefaultDisplay");

        let result = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDisplay",
                "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Display;",
                [None.into()],
            )
            .await?;

        Ok(result)
    }

    async fn get_activated_index(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getActivatedIndex");

        jvm.get_static_field("org/kwis/msp/lcdui/Display", "__wieActivatedIndex", "I").await
    }

    async fn activate_current_display(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::activateCurrentDisplay({this:?})");

        let index: i32 = jvm.get_field(&this, "__wieDisplayIndex", "I").await?;

        // Native activateCurrentDisplay() always copies this Display's +0x18
        // index into the class-static +0x50 slot after activateCurrentDisplay0().
        jvm.put_static_field("org/kwis/msp/lcdui/Display", "__wieActivatedIndex", "I", index)
            .await?;

        // WIE has one MIDP display backend, so there is no lower-level
        // platform display-switch status to propagate here.
        Ok(0)
    }

    async fn get_docked_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Card>> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getDockedCard({this:?})");

        jvm.get_field(&this, "dockedCard", "Lorg/kwis/msp/lcdui/Card;").await
    }

    async fn set_docked_card(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        card: ClassInstanceRef<Card>,
        where_: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::setDockedCard({this:?}, {card:?}, {where_})");

        jvm.put_field(&mut this, "dockedCard", "Lorg/kwis/msp/lcdui/Card;", card.clone()).await?;

        // The docked card is the persistent background surface; hand it to the
        // CardCanvas so it is painted behind any pushed cards. Cardless-Jlet
        // titles show their whole screen this way.
        let card_canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;
        jvm.invoke_virtual(&card_canvas, "setDockedCard", "(Lorg/kwis/msp/lcdui/Card;)V", (card,))
            .await
    }

    async fn is_double_buffered(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lcdui.Display::isDoubleBuffered({this:?})");

        let canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;

        jvm.invoke_virtual(&canvas, "isDoubleBuffered", "()Z", ()).await
    }

    async fn push_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, c: ClassInstanceRef<Card>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::pushCard({this:?}, {c:?})");

        let card_canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;
        let _: () = jvm.invoke_virtual(&card_canvas, "pushCard", "(Lorg/kwis/msp/lcdui/Card;)V", (c,)).await?;

        Ok(())
    }

    async fn pop_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Card>> {
        tracing::debug!("org.kwis.msp.lcdui.Display::popCard({this:?})");

        let card_canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;
        jvm.invoke_virtual(&card_canvas, "popCard", "()Lorg/kwis/msp/lcdui/Card;", ()).await
    }

    async fn remove_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, card: ClassInstanceRef<Card>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lcdui.Display::removeCard({this:?}, {card:?})");

        let card_canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;
        jvm.invoke_virtual(&card_canvas, "removeCard", "(Lorg/kwis/msp/lcdui/Card;)Z", (card,))
            .await
    }

    async fn count_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::countCard({this:?})");

        let card_canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;
        jvm.invoke_virtual(&card_canvas, "countCard", "()I", ()).await
    }

    async fn remove_all_cards(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::removeAllCards({this:?})");

        let card_canvas = jvm.get_field(&this, "cardCanvas", "Lnet/wie/CardCanvas;").await?;
        let _: () = jvm.invoke_virtual(&card_canvas, "removeAllCards", "()V", ()).await?;

        Ok(())
    }

    async fn add_jlet_event_listener(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Display>,
        qel: ClassInstanceRef<JletEventListener>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::addJletEventListener({this:?}, {qel:?})");

        let mut listeners: ClassInstanceRef<()> = jvm
            .get_static_field(
                "org/kwis/msp/lcdui/Display",
                "__wieJletEventListeners",
                "[Lorg/kwis/msp/lcdui/JletEventListener;",
            )
            .await?;

        if listeners.is_null() {
            listeners = jvm.instantiate_array("Lorg/kwis/msp/lcdui/JletEventListener;", 4).await?.into();

            jvm.put_static_field(
                "org/kwis/msp/lcdui/Display",
                "__wieJletEventListeners",
                "[Lorg/kwis/msp/lcdui/JletEventListener;",
                listeners.clone(),
            )
            .await?;
        }

        let count: i32 = jvm
            .get_static_field("org/kwis/msp/lcdui/Display", "__wieJletEventListenerCount", "I")
            .await?;

        // Preserve the native generated duplicate-scan literally:
        // it walks count - 1 down through index 1 and never examines
        // slot 0.  The comparison itself is raw-reference equality,
        // so two null references also compare equal on scanned slots.
        let mut index = count - 1;
        while index > 0 {
            let values: alloc::vec::Vec<ClassInstanceRef<JletEventListener>> = jvm.load_array(&listeners, index as usize, 1).await?;
            let current = &values[0];

            let same = if qel.is_null() {
                current.is_null()
            } else {
                !current.is_null() && current.identity() == qel.identity()
            };

            if same {
                return Ok(());
            }

            index -= 1;
        }

        let capacity = jvm.array_length(&listeners).await?;
        if count as usize >= capacity {
            let old_values: alloc::vec::Vec<ClassInstanceRef<JletEventListener>> = if capacity == 0 {
                alloc::vec::Vec::new()
            } else {
                jvm.load_array(&listeners, 0, capacity).await?
            };

            let mut expanded = jvm.instantiate_array("Lorg/kwis/msp/lcdui/JletEventListener;", capacity * 2).await?;

            if !old_values.is_empty() {
                jvm.store_array(&mut expanded, 0, old_values).await?;
            }

            listeners = expanded.into();

            jvm.put_static_field(
                "org/kwis/msp/lcdui/Display",
                "__wieJletEventListeners",
                "[Lorg/kwis/msp/lcdui/JletEventListener;",
                listeners.clone(),
            )
            .await?;
        }

        jvm.store_array(&mut listeners, count as usize, [qel.clone()]).await?;

        jvm.put_static_field("org/kwis/msp/lcdui/Display", "__wieJletEventListenerCount", "I", count + 1)
            .await?;

        Ok(())
    }

    async fn get_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getWidth({this:?})");

        let midp_display: ClassInstanceRef<MidpDisplay> = jvm.get_field(&this, "midpDisplay", "Ljavax/microedition/lcdui/Display;").await?;
        let width: i32 = jvm.invoke_virtual(&midp_display, "getWidth", "()I", ()).await?;

        Ok(width)
    }

    async fn get_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getHeight({this:?})");

        let midp_display: ClassInstanceRef<MidpDisplay> = jvm.get_field(&this, "midpDisplay", "Ljavax/microedition/lcdui/Display;").await?;
        let height: i32 = jvm.invoke_virtual(&midp_display, "getHeight", "()I", ()).await?;

        Ok(height)
    }

    async fn call_serially(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, r: ClassInstanceRef<Runnable>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::callSerially({this:?}, {r:?})");

        let midp_display: ClassInstanceRef<MidpDisplay> = jvm.get_field(&this, "midpDisplay", "Ljavax/microedition/lcdui/Display;").await?;
        let _: () = jvm.invoke_virtual(&midp_display, "callSerially", "(Ljava/lang/Runnable;)V", (r,)).await?;

        Ok(())
    }

    async fn call_serially_with_timeout(
        _: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        runnable: ClassInstanceRef<Runnable>,
        timeout: i32,
    ) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::callSerially({this:?}, {runnable:?}, {timeout})");

        Ok(())
    }

    async fn is_color(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::isColor({this:?})");

        Ok(false)
    }

    async fn num_colors(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::numColors({this:?})");

        Ok(0)
    }

    async fn has_pointer_events(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::hasPointerEvents({this:?})");

        Ok(false)
    }

    async fn has_pointer_motion_events(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::hasPointerMotionEvents({this:?})");

        Ok(false)
    }

    async fn has_repeat_events(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::hasRepeatEvents({this:?})");

        Ok(false)
    }

    async fn get_key_name(_: &Jvm, _: &mut WieJvmContext, key: i32) -> JvmResult<ClassInstanceRef<String>> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::getKeyName({key})");

        Ok(None.into())
    }

    async fn get_bits_per_pixel(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::getBitsPerPixel({this:?})");

        Ok(0)
    }

    async fn flush(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::flush({this:?})");

        Ok(())
    }

    async fn remove_jlet_event_listener(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        listener: ClassInstanceRef<JletEventListener>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::removeJletEventListener({this:?}, {listener:?})");

        let mut listeners: ClassInstanceRef<()> = jvm
            .get_static_field(
                "org/kwis/msp/lcdui/Display",
                "__wieJletEventListeners",
                "[Lorg/kwis/msp/lcdui/JletEventListener;",
            )
            .await?;

        if listeners.is_null() {
            return Ok(());
        }

        let mut count: i32 = jvm
            .get_static_field("org/kwis/msp/lcdui/Display", "__wieJletEventListenerCount", "I")
            .await?;

        // Preserve the native generated loop literally:
        // when count == 1, count - 1 == 0 and the method exits
        // without examining/removing slot 0.
        let mut index = count - 1;
        if index <= 0 {
            return Ok(());
        }

        while index >= 0 {
            let values: alloc::vec::Vec<ClassInstanceRef<JletEventListener>> = jvm.load_array(&listeners, index as usize, 1).await?;
            let current = &values[0];

            let same = if listener.is_null() {
                current.is_null()
            } else {
                !current.is_null() && current.identity() == listener.identity()
            };

            if same {
                let mut pos = index as usize;
                let old_count = count as usize;

                while pos + 1 < old_count {
                    let next: alloc::vec::Vec<ClassInstanceRef<JletEventListener>> = jvm.load_array(&listeners, pos + 1, 1).await?;

                    jvm.store_array(&mut listeners, pos, [next[0].clone()]).await?;

                    pos += 1;
                }

                count -= 1;

                jvm.put_static_field("org/kwis/msp/lcdui/Display", "__wieJletEventListenerCount", "I", count)
                    .await?;

                jvm.store_array(&mut listeners, count as usize, [ClassInstanceRef::<JletEventListener>::new(None)])
                    .await?;

                index = count - 1;
            } else {
                index -= 1;
            }
        }

        Ok(())
    }

    async fn grab_key(
        _: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        key: i32,
        listener: ClassInstanceRef<JletEventListener>,
    ) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::grabKey({this:?}, {key}, {listener:?})");

        Ok(())
    }

    /// What `grabKey`/`ungrabKey` reach on the handset. The grab itself is not
    /// modelled - every key already reaches the title - so these record the
    /// request and leave delivery as it is.
    async fn grab_key0(_: &Jvm, _: &mut WieJvmContext, key: i32, mode: i32) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::grabKey0({key}, {mode})");

        Ok(())
    }

    async fn ungrab_key0(_: &Jvm, _: &mut WieJvmContext, key: i32) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::ungrabKey0({key})");

        Ok(())
    }

    /// The display is never turned, so a title asking how far it is turned is
    /// told none.
    async fn get_rotation(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getRotation({this:?})");

        Ok(0)
    }

    /// The picture behind the handset's status strip, and how far it is dimmed.
    /// The strip here is the title's own drawing area rather than a surface the
    /// platform paints, so both are remembered and neither changes what is
    /// drawn.
    async fn set_annun_background(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::setAnnunBackground({this:?}, {image:?})");

        jvm.put_field(&mut this, "annunBackground", "Lorg/kwis/msp/lcdui/Image;", image).await
    }

    async fn set_annun_background_dimming_alpha(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, alpha: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Display::setAnnunBackgroundDimmingAlpha({this:?}, {alpha})");

        jvm.put_field(&mut this, "annunBackgroundAlpha", "I", alpha).await
    }

    async fn ungrab_key(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, key: i32) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Display::ungrabKey({this:?}, {key})");

        Ok(())
    }

    async fn get_game_action(_jvm: &Jvm, _: &mut WieJvmContext, key: i32) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getGameAction({key})");

        let action = match WIPIKeyCode::from_raw(key) {
            Some(WIPIKeyCode::UP) => 1,
            Some(WIPIKeyCode::DOWN) => 6,
            Some(WIPIKeyCode::LEFT) => 2,
            Some(WIPIKeyCode::RIGHT) => 5,
            Some(WIPIKeyCode::FIRE) => 8,
            Some(WIPIKeyCode::LEFT_SOFT_KEY) => 90,
            Some(WIPIKeyCode::RIGHT_SOFT_KEY) => 91,
            Some(WIPIKeyCode::CLEAR) => 99,
            _ => key,
        };

        Ok(action)
    }

    async fn get_key_code(_jvm: &Jvm, _: &mut WieJvmContext, game_key: i32) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Display::getKeyCode({game_key})");

        let key_code = match game_key {
            1 => WIPIKeyCode::UP as i32,
            2 => WIPIKeyCode::LEFT as i32,
            5 => WIPIKeyCode::RIGHT as i32,
            6 => WIPIKeyCode::DOWN as i32,
            8 => WIPIKeyCode::FIRE as i32,
            90 => WIPIKeyCode::LEFT_SOFT_KEY as i32,
            91 => WIPIKeyCode::RIGHT_SOFT_KEY as i32,
            92 => -8,
            96 => WIPIKeyCode::VOLUME_UP as i32,
            97 => WIPIKeyCode::VOLUME_DOWN as i32,
            98 => -15,
            99 => WIPIKeyCode::CLEAR as i32,
            _ => 0,
        };

        Ok(key_code)
    }

    pub async fn notify_jlet_event_listeners(jvm: &Jvm, r#type: i32, param1: i32, param2: i32) -> JvmResult<()> {
        // Native Display.eventNotify_v0 handles event type 42 through
        // a separate Display-control path and does not dispatch it to
        // JletEventListener.notifyEvent(III)V.
        if r#type == 42 {
            return Ok(());
        }

        let listeners: ClassInstanceRef<()> = jvm
            .get_static_field(
                "org/kwis/msp/lcdui/Display",
                "__wieJletEventListeners",
                "[Lorg/kwis/msp/lcdui/JletEventListener;",
            )
            .await?;

        let count: i32 = jvm
            .get_static_field("org/kwis/msp/lcdui/Display", "__wieJletEventListenerCount", "I")
            .await?;

        if listeners.is_null() || count == 0 {
            return Ok(());
        }

        // Native Display.eventNotify_v0 starts at count - 1 and
        // dispatches listeners in reverse registration order.
        let mut index = count - 1;
        while index >= 0 {
            let values: alloc::vec::Vec<ClassInstanceRef<JletEventListener>> = jvm.load_array(&listeners, index as usize, 1).await?;
            let listener = values[0].clone();

            if listener.is_null() {
                return Err(jvm.exception("java/lang/NullPointerException", "").await);
            }

            let _: () = jvm.invoke_virtual(&listener, "notifyEvent", "(III)V", (r#type, param1, param2)).await?;

            index -= 1;
        }

        Ok(())
    }

    pub async fn midp_display(jvm: &Jvm, this: &ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<MidpDisplay>> {
        jvm.get_field(this, "midpDisplay", "Ljavax/microedition/lcdui/Display;").await
    }
}

#[cfg(test)]
mod test {
    use alloc::boxed::Box;

    use test_utils::run_jvm_test;
    use wie_util::Result;

    use crate::get_protos;

    #[test]
    fn test_get_key_code() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let up: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (1,)).await?;
            let down: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (6,)).await?;
            let left: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (2,)).await?;
            let right: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (5,)).await?;
            let fire: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (8,)).await?;
            let soft1: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (90,)).await?;
            let soft2: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (91,)).await?;
            let soft3: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (92,)).await?;
            let side_up: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (96,)).await?;
            let side_down: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (97,)).await?;
            let side_select: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (98,)).await?;
            let clear: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (99,)).await?;
            let game_a: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (9,)).await?;
            let invalid: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getKeyCode", "(I)I", (1234,)).await?;

            assert_eq!(up, -1);
            assert_eq!(down, -2);
            assert_eq!(left, -3);
            assert_eq!(right, -4);
            assert_eq!(fire, -5);
            assert_eq!(soft1, -6);
            assert_eq!(soft2, -7);
            assert_eq!(soft3, -8);
            assert_eq!(side_up, -13);
            assert_eq!(side_down, -14);
            assert_eq!(side_select, -15);
            assert_eq!(clear, -16);
            assert_eq!(game_a, 0);
            assert_eq!(invalid, 0);

            Ok(())
        })
    }
}
