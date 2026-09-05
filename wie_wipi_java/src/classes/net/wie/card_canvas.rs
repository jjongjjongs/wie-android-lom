use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::{
    javax::microedition::lcdui::{Canvas as MidpCanvas, Display as MidpDisplay, Graphics as MidpGraphics},
    net::wie::MIDPKeyCode,
};

use crate::classes::org::kwis::msp::lcdui::{Card, Display, Graphics};

#[repr(i32)]
#[allow(clippy::upper_case_acronyms, non_camel_case_types)]
#[derive(Copy, Clone)]
pub enum WIPIKeyCode {
    UP = -1,
    DOWN = -2,
    LEFT = -3,
    RIGHT = -4,
    FIRE = -5, // Ok
    LEFT_SOFT_KEY = -6,
    RIGHT_SOFT_KEY = -7,
    CLEAR = -16,
    CALL = -10,
    HANGUP = -11,
    VOLUME_UP = -13,
    VOLUME_DOWN = -14,

    NUM0 = 48,
    NUM1 = 49,
    NUM2 = 50,
    NUM3 = 51,
    NUM4 = 52,
    NUM5 = 53,
    NUM6 = 54,
    NUM7 = 55,
    NUM8 = 56,
    NUM9 = 57,
    HASH = 35, // #
    STAR = 42, // *
}

impl WIPIKeyCode {
    pub fn from_raw(value: i32) -> Option<Self> {
        Some(match value {
            x if x == Self::UP as i32 => Self::UP,
            x if x == Self::DOWN as i32 => Self::DOWN,
            x if x == Self::LEFT as i32 => Self::LEFT,
            x if x == Self::RIGHT as i32 => Self::RIGHT,
            x if x == Self::FIRE as i32 => Self::FIRE,
            x if x == Self::LEFT_SOFT_KEY as i32 => Self::LEFT_SOFT_KEY,
            x if x == Self::RIGHT_SOFT_KEY as i32 => Self::RIGHT_SOFT_KEY,
            x if x == Self::CLEAR as i32 => Self::CLEAR,
            x if x == Self::CALL as i32 => Self::CALL,
            x if x == Self::HANGUP as i32 => Self::HANGUP,
            x if x == Self::VOLUME_UP as i32 => Self::VOLUME_UP,
            x if x == Self::VOLUME_DOWN as i32 => Self::VOLUME_DOWN,
            x if x == Self::NUM0 as i32 => Self::NUM0,
            x if x == Self::NUM1 as i32 => Self::NUM1,
            x if x == Self::NUM2 as i32 => Self::NUM2,
            x if x == Self::NUM3 as i32 => Self::NUM3,
            x if x == Self::NUM4 as i32 => Self::NUM4,
            x if x == Self::NUM5 as i32 => Self::NUM5,
            x if x == Self::NUM6 as i32 => Self::NUM6,
            x if x == Self::NUM7 as i32 => Self::NUM7,
            x if x == Self::NUM8 as i32 => Self::NUM8,
            x if x == Self::NUM9 as i32 => Self::NUM9,
            x if x == Self::HASH as i32 => Self::HASH,
            x if x == Self::STAR as i32 => Self::STAR,
            _ => return None,
        })
    }

    pub fn from_midp_raw(keycode: i32) -> i32 {
        match MIDPKeyCode::from_raw(keycode) {
            Some(MIDPKeyCode::UP) => Self::UP as i32,
            Some(MIDPKeyCode::DOWN) => Self::DOWN as i32,
            Some(MIDPKeyCode::LEFT) => Self::LEFT as i32,
            Some(MIDPKeyCode::RIGHT) => Self::RIGHT as i32,
            Some(MIDPKeyCode::FIRE) => Self::FIRE as i32,
            Some(MIDPKeyCode::LEFT_SOFT_KEY) => Self::LEFT_SOFT_KEY as i32,
            Some(MIDPKeyCode::RIGHT_SOFT_KEY) => Self::RIGHT_SOFT_KEY as i32,
            Some(MIDPKeyCode::CLEAR) => Self::CLEAR as i32,
            Some(MIDPKeyCode::CALL) => Self::CALL as i32,
            Some(MIDPKeyCode::HANGUP) => Self::HANGUP as i32,
            Some(MIDPKeyCode::VOLUME_UP) => Self::VOLUME_UP as i32,
            Some(MIDPKeyCode::VOLUME_DOWN) => Self::VOLUME_DOWN as i32,
            Some(MIDPKeyCode::KEY_NUM0) => Self::NUM0 as i32,
            Some(MIDPKeyCode::KEY_NUM1) => Self::NUM1 as i32,
            Some(MIDPKeyCode::KEY_NUM2) => Self::NUM2 as i32,
            Some(MIDPKeyCode::KEY_NUM3) => Self::NUM3 as i32,
            Some(MIDPKeyCode::KEY_NUM4) => Self::NUM4 as i32,
            Some(MIDPKeyCode::KEY_NUM5) => Self::NUM5 as i32,
            Some(MIDPKeyCode::KEY_NUM6) => Self::NUM6 as i32,
            Some(MIDPKeyCode::KEY_NUM7) => Self::NUM7 as i32,
            Some(MIDPKeyCode::KEY_NUM8) => Self::NUM8 as i32,
            Some(MIDPKeyCode::KEY_NUM9) => Self::NUM9 as i32,
            Some(MIDPKeyCode::KEY_POUND) => Self::HASH as i32,
            Some(MIDPKeyCode::KEY_STAR) => Self::STAR as i32,
            None => keycode,
        }
    }
}

/// WIPI `Card.keyNotify(int type, int key)` event types (org.kwis.msp.lcdui).
/// A title compares the type against these: 시드's compiled keyNotify dispatches
/// `type == 1` as a press and `type == 2` as a release (`cmp r5, #1` / `#2` at
/// its entry), so a press must be 1. The LGT `CletWrapperCard` re-bases these
/// onto its own clet event ids, so keep the two in step when changing them.
const KEY_PRESSED: i32 = 1;
const KEY_RELEASED: i32 = 2;
const KEY_REPEATED: i32 = 3;

// class net.wie.CardCanvas
pub struct CardCanvas;

impl CardCanvas {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "net/wie/CardCanvas",
            parent_class: Some("javax/microedition/lcdui/Canvas"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("paint", "(Ljavax/microedition/lcdui/Graphics;)V", Self::paint, Default::default()),
                JavaMethodProto::new("keyPressed", "(I)V", Self::key_pressed, Default::default()),
                JavaMethodProto::new("keyRepeated", "(I)V", Self::key_repeated, Default::default()),
                JavaMethodProto::new("keyReleased", "(I)V", Self::key_released, Default::default()),
                JavaMethodProto::new("pushCard", "(Lorg/kwis/msp/lcdui/Card;)V", Self::push_card, Default::default()),
                JavaMethodProto::new("setDockedCard", "(Lorg/kwis/msp/lcdui/Card;)V", Self::set_docked_card, Default::default()),
                JavaMethodProto::new("popCard", "()Lorg/kwis/msp/lcdui/Card;", Self::pop_card, Default::default()),
                JavaMethodProto::new("removeCard", "(Lorg/kwis/msp/lcdui/Card;)Z", Self::remove_card, Default::default()),
                JavaMethodProto::new("countCard", "()I", Self::count_card, Default::default()),
                JavaMethodProto::new("removeAllCards", "()V", Self::remove_all_cards, Default::default()),
                // wie private
                JavaMethodProto::new("handleNotifyEvent", "(III)V", Self::handle_notify_event, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("cards", "Ljava/util/Vector;", Default::default()),
                // A single background card set via org.kwis Display.setDockedCard,
                // painted behind the pushed-card stack. Cardless-Jlet titles
                // (Fantasy Knight, Battle Monster) show their screen this way
                // rather than through pushCard.
                JavaFieldProto::new("dockedCard", "Lorg/kwis/msp/lcdui/Card;", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "javax/microedition/lcdui/Canvas", "<init>", "()V", ()).await?;

        let _: () = jvm.invoke_virtual(&this, "setFullScreenMode", "(Z)V", (true,)).await?;

        let cards = jvm.new_class("java/util/Vector", "()V", ()).await?;
        jvm.put_field(&mut this, "cards", "Ljava/util/Vector;", cards).await?;

        Ok(())
    }

    async fn paint(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, g: ClassInstanceRef<MidpGraphics>) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::paint({this:?}, {g:?})");

        // MIDP may reuse a Graphics instance whose translate/clip/color state
        // was changed by a previous paint. CardCanvas establishes a fresh
        // canvas drawing state before wrapping it as WIPI Graphics.
        let _: () = jvm.invoke_virtual(&g, "reset", "()V", ()).await?;

        let graphics: ClassInstanceRef<Graphics> = jvm
            .new_class("org/kwis/msp/lcdui/Graphics", "(Ljavax/microedition/lcdui/Graphics;)V", (g,))
            .await?
            .into();

        // What the title asked to have repainted since the last pass. A card
        // paints its whole scene however small the region is, so the region has
        // to become the clip: the ez-i SDK titles type a dialogue out by asking
        // for one 10x10 glyph cell at a time and letting the rest of the box
        // stand, and repainting the lot unclipped wiped every letter but the
        // newest.
        let region = Self::take_dirty_region(jvm, &this).await?;

        // The docked card is the background layer; the pushed-card stack draws
        // on top of it.
        let docked: ClassInstanceRef<Card> = jvm.get_field(&this, "dockedCard", "Lorg/kwis/msp/lcdui/Card;").await?;
        if !docked.is_null() {
            Self::paint_one(jvm, &graphics, &docked, 0, region).await?;
        }

        let client_top = Self::client_top(jvm, &this, &docked).await?;

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;

        for i in 0..length {
            let card: ClassInstanceRef<Card> = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (i,)).await?;
            Self::paint_one(jvm, &graphics, &card, client_top, region).await?;
        }

        Ok(())
    }

    /// Takes the region `Canvas.repaint` collected, and leaves nothing pending.
    ///
    /// `None` means "paint everything": either the title asked for the whole
    /// canvas, or this pass was not asked for at all - a redraw the platform
    /// itself wanted - and a full paint is what that has always meant.
    ///
    /// The rectangle is in canvas coordinates, which is what `Card.repaint`
    /// hands the canvas.
    async fn take_dirty_region(jvm: &Jvm, this: &ClassInstanceRef<Self>) -> JvmResult<Option<(i32, i32, i32, i32)>> {
        let mut this = this.clone();

        let x: i32 = jvm.get_field(&this, "__wieDirtyX", "I").await?;
        let y: i32 = jvm.get_field(&this, "__wieDirtyY", "I").await?;
        let width: i32 = jvm.get_field(&this, "__wieDirtyWidth", "I").await?;
        let height: i32 = jvm.get_field(&this, "__wieDirtyHeight", "I").await?;

        jvm.put_field(&mut this, "__wieDirtyWidth", "I", 0).await?;
        jvm.put_field(&mut this, "__wieDirtyHeight", "I", 0).await?;

        if width <= 0 || height <= 0 {
            return Ok(None);
        }

        Ok(Some((x, y, width, height)))
    }

    /// The row a pushed card's own origin lands on.
    ///
    /// A docked status strip is part of the panel, not of the area a title is
    /// given: the reference puts it above the drawing area (the WIPI-C side
    /// splits the two the same way, see `wie_wipi_c::api::graphics`), so a card
    /// pushed while one is docked starts below it. The ez-i SDK titles rely on
    /// exactly that - 판타지나이트 and 배틀몬스터 dock a 240x24 strip and then
    /// repaint `(0, 0, 240, 296)` on a 320-row panel, so with the strip's rows
    /// left to them their last 24 rows kept whatever an earlier full-screen
    /// draw had put there.
    ///
    /// A docked card that fills the panel is a background rather than a strip -
    /// it leaves no room to push anything below it - so it moves nothing.
    async fn client_top(jvm: &Jvm, this: &ClassInstanceRef<Self>, docked: &ClassInstanceRef<Card>) -> JvmResult<i32> {
        if docked.is_null() {
            return Ok(0);
        }

        let height: i32 = jvm.invoke_virtual(docked, "getHeight", "()I", ()).await?;
        let canvas_height: i32 = jvm.invoke_virtual(this, "getHeight", "()I", ()).await?;

        if height <= 0 || height >= canvas_height {
            return Ok(0);
        }

        let y: i32 = jvm.invoke_virtual(docked, "getY", "()I", ()).await?;

        Ok(y + height)
    }

    /// Paints one card at its `(getX, getY)` offset, `offset_y` rows down the
    /// panel, resetting the shared WIPI Graphics around it so a card cannot leak
    /// translate/clip state to the next.
    async fn paint_one(
        jvm: &Jvm,
        graphics: &ClassInstanceRef<Graphics>,
        card: &ClassInstanceRef<Card>,
        offset_y: i32,
        region: Option<(i32, i32, i32, i32)>,
    ) -> JvmResult<()> {
        let x: i32 = jvm.invoke_virtual(card, "getX", "()I", ()).await?;
        let card_y: i32 = jvm.invoke_virtual(card, "getY", "()I", ()).await?;
        let y = card_y + offset_y;

        let _: () = jvm.invoke_virtual(graphics, "reset", "()V", ()).await?;
        let _: () = jvm.invoke_virtual(graphics, "translate", "(II)V", (x, y)).await?;

        // The region is in canvas coordinates and `setClip` is in the card's,
        // which the translate above just established; the strip a docked card
        // takes cancels out, since `Card.repaint` reports a card's own origin
        // without it.
        if let Some((region_x, region_y, width, height)) = region {
            let _: () = jvm
                .invoke_virtual(graphics, "setClip", "(IIII)V", (region_x - x, region_y - card_y, width, height))
                .await?;
        }

        let paint_result: JvmResult<()> = jvm
            .invoke_virtual(card, "paint", "(Lorg/kwis/msp/lcdui/Graphics;)V", (graphics.clone(),))
            .await;
        let reset_result: JvmResult<()> = jvm.invoke_virtual(graphics, "reset", "()V", ()).await;
        paint_result?;
        reset_result?;

        Ok(())
    }

    async fn key_pressed(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::keyPressed({this:?}, {key_code})");

        let key_code = WIPIKeyCode::from_midp_raw(key_code);

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;

        for i in (0..length).rev() {
            let card = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (i,)).await?;
            let propagate: bool = jvm.invoke_virtual(&card, "keyNotify", "(II)Z", (KEY_PRESSED, key_code)).await?;

            if !propagate {
                break;
            }
        }

        Ok(())
    }

    async fn key_repeated(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::keyRepeated({this:?}, {key_code})");

        let key_code = WIPIKeyCode::from_midp_raw(key_code);

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;

        for i in (0..length).rev() {
            let card = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (i,)).await?;
            let propagate: bool = jvm.invoke_virtual(&card, "keyNotify", "(II)Z", (KEY_REPEATED, key_code)).await?;

            if !propagate {
                break;
            }
        }

        Ok(())
    }

    async fn key_released(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::keyReleased({this:?}, {key_code})");

        let key_code = WIPIKeyCode::from_midp_raw(key_code);

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;

        for i in (0..length).rev() {
            let card = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (i,)).await?;
            let propagate: bool = jvm.invoke_virtual(&card, "keyNotify", "(II)Z", (KEY_RELEASED, key_code)).await?;

            if !propagate {
                break;
            }
        }

        Ok(())
    }

    async fn push_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, c: ClassInstanceRef<Card>) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::pushCard({this:?}, {c:?})");

        if c.is_null() {
            return Ok(());
        }

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let canvas: ClassInstanceRef<MidpCanvas> = jvm.get_field(&c, "canvas", "Ljavax/microedition/lcdui/Canvas;").await?;
        let index: i32 = jvm.invoke_virtual(&cards, "indexOf", "(Ljava/lang/Object;)I", (c.clone(),)).await?;
        if !canvas.is_null() || index >= 0 {
            return Ok(());
        }

        let _: () = jvm.invoke_virtual(&cards, "addElement", "(Ljava/lang/Object;)V", (c.clone(),)).await?;

        let _: () = jvm
            .invoke_virtual(&c, "setCanvas", "(Ljavax/microedition/lcdui/Canvas;)V", (this.clone(),))
            .await?;
        let _: () = jvm.invoke_virtual(&c, "showNotify", "(Z)V", (true,)).await?;

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        // HACK: disable java level paint on clet app. A clet draws straight to
        // the LCD framebuffer through the WIPI-C graphics API and flushes it
        // itself, so the MIDP layer must not also flush its own (empty) screen
        // image over the top - that repaints the clet's frame to black.
        //
        // `is_instance` is matched against the internal (slashed) class name and
        // follows the superclass chain; comparing `Class.getName()` here would
        // silently miss, since that returns the dotted form `net.wie.CletWrapperCard`.
        if jvm.is_instance(&**c, "net/wie/CletWrapperCard") {
            let wipi_display: ClassInstanceRef<Display> = jvm
                .invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
                .await?;
            let midp_display: ClassInstanceRef<MidpDisplay> =
                jvm.get_field(&wipi_display, "midpDisplay", "Ljavax/microedition/lcdui/Display;").await?;
            let _: () = jvm.invoke_virtual(&midp_display, "disablePaint", "()V", ()).await?;
        }

        Ok(())
    }

    async fn set_docked_card(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, c: ClassInstanceRef<Card>) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::setDockedCard({this:?}, {c:?})");

        let previous: ClassInstanceRef<Card> = jvm.get_field(&this, "dockedCard", "Lorg/kwis/msp/lcdui/Card;").await?;
        if !previous.is_null() {
            let same: bool = jvm.invoke_virtual(&previous, "equals", "(Ljava/lang/Object;)Z", (c.clone(),)).await?;
            if same {
                return Ok(());
            }
            let _: () = jvm.invoke_virtual(&previous, "showNotify", "(Z)V", (false,)).await?;
            let _: () = jvm
                .invoke_virtual(&previous, "setCanvas", "(Ljavax/microedition/lcdui/Canvas;)V", (None,))
                .await?;
        }

        jvm.put_field(&mut this, "dockedCard", "Lorg/kwis/msp/lcdui/Card;", c.clone()).await?;

        if !c.is_null() {
            let _: () = jvm
                .invoke_virtual(&c, "setCanvas", "(Ljavax/microedition/lcdui/Canvas;)V", (this.clone(),))
                .await?;
            let _: () = jvm.invoke_virtual(&c, "showNotify", "(Z)V", (true,)).await?;
        }

        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(())
    }

    async fn pop_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Card>> {
        tracing::debug!("net.wie.CardCanvas::popCard({this:?})");

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length: i32 = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;
        if length == 0 {
            return Ok(None.into());
        }

        let card: ClassInstanceRef<Card> = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (length - 1,)).await?;
        let _: () = jvm.invoke_virtual(&card, "showNotify", "(Z)V", (false,)).await?;
        let _: ClassInstanceRef<Card> = jvm.invoke_virtual(&cards, "remove", "(I)Ljava/lang/Object;", (length - 1,)).await?;
        let _: () = jvm
            .invoke_virtual(&card, "setCanvas", "(Ljavax/microedition/lcdui/Canvas;)V", (None,))
            .await?;
        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(card)
    }

    async fn remove_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, card: ClassInstanceRef<Card>) -> JvmResult<bool> {
        tracing::debug!("net.wie.CardCanvas::removeCard({this:?}, {card:?})");

        if card.is_null() {
            return Ok(false);
        }

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let index: i32 = jvm.invoke_virtual(&cards, "indexOf", "(Ljava/lang/Object;)I", (card.clone(),)).await?;
        if index < 0 {
            return Ok(false);
        }

        let _: () = jvm.invoke_virtual(&card, "showNotify", "(Z)V", (false,)).await?;
        let _: bool = jvm
            .invoke_virtual(&cards, "removeElement", "(Ljava/lang/Object;)Z", (card.clone(),))
            .await?;
        let _: () = jvm
            .invoke_virtual(&card, "setCanvas", "(Ljavax/microedition/lcdui/Canvas;)V", (None,))
            .await?;
        let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;

        Ok(true)
    }

    async fn count_card(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("net.wie.CardCanvas::countCard({this:?})");

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        jvm.invoke_virtual(&cards, "size", "()I", ()).await
    }

    async fn remove_all_cards(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::removeAllCards");

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;

        for i in 0..length {
            let card = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (i,)).await?;
            let _: () = jvm.invoke_virtual(&card, "showNotify", "(Z)V", (false,)).await?;
            let _: () = jvm
                .invoke_virtual(&card, "setCanvas", "(Ljavax/microedition/lcdui/Canvas;)V", (None,))
                .await?;
        }

        let _: () = jvm.invoke_virtual(&cards, "removeAllElements", "()V", ()).await?;
        if length != 0 {
            let _: () = jvm.invoke_virtual(&this, "repaint", "()V", ()).await?;
        }

        Ok(())
    }

    async fn handle_notify_event(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        r#type: i32,
        param1: i32,
        param2: i32,
    ) -> JvmResult<()> {
        tracing::debug!("net.wie.CardCanvas::handleNotifyEvent({this:?}, {type}, {param1}, {param2})");

        // Native org.kwis.msp.lcdui.Display.eventNotify_v0 dispatches
        // registered JletEventListeners independently of Card presence.
        Display::notify_jlet_event_listeners(jvm, r#type, param1, param2).await?;

        let cards = jvm.get_field(&this, "cards", "Ljava/util/Vector;").await?;
        let length: i32 = jvm.invoke_virtual(&cards, "size", "()I", ()).await?;
        if length == 0 {
            return Ok(());
        }
        let top_card = jvm.invoke_virtual(&cards, "elementAt", "(I)Ljava/lang/Object;", (length - 1,)).await?;

        let _: () = jvm.invoke_virtual(&top_card, "notifyEvent", "(III)V", (r#type, param1, param2)).await?;

        Ok(())
    }
}
