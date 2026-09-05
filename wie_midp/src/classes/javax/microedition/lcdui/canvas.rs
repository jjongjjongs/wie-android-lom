use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::ClassAccessFlags;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::{
    javax::microedition::lcdui::{Display, Graphics},
    net::wie::{KeyboardEventType, MIDPKeyCode},
};

// abstract class javax.microedition.lcdui.Canvas
pub struct Canvas;

impl Canvas {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "javax/microedition/lcdui/Canvas",
            parent_class: Some("javax/microedition/lcdui/Displayable"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("repaint", "()V", Self::repaint, Default::default()),
                JavaMethodProto::new("repaint", "(IIII)V", Self::repaint_with_area, Default::default()),
                JavaMethodProto::new("serviceRepaints", "()V", Self::service_repaints, Default::default()),
                JavaMethodProto::new_abstract("paint", "(Ljavax/microedition/lcdui/Graphics;)V", Default::default()),
                JavaMethodProto::new("getGameAction", "(I)I", Self::get_game_action, Default::default()),
                JavaMethodProto::new("keyPressed", "(I)V", Self::key_pressed, Default::default()),
                JavaMethodProto::new("keyRepeated", "(I)V", Self::key_repeated, Default::default()),
                JavaMethodProto::new("keyReleased", "(I)V", Self::key_released, Default::default()),
                JavaMethodProto::new("setFullScreenMode", "(Z)V", Self::set_full_screen_mode, Default::default()),
                JavaMethodProto::new("isDoubleBuffered", "()Z", Self::is_double_buffered, Default::default()),
                // wie private methods
                JavaMethodProto::new("handleKeyEvent", "(II)V", Self::handle_key_event, Default::default()),
                JavaMethodProto::new(
                    "handlePaintEvent",
                    "(Ljavax/microedition/lcdui/Graphics;)V",
                    Self::handle_paint_event,
                    Default::default(),
                ),
            ],
            fields: vec![
                // The region a title asked to have repainted, in canvas
                // coordinates, unioned across the `repaint` calls made since
                // the last paint. A width at or below zero means "the whole
                // canvas": either nothing has been asked for, or a caller asked
                // for everything. See `take_dirty_region`.
                JavaFieldProto::new("__wieDirtyX", "I", Default::default()),
                JavaFieldProto::new("__wieDirtyY", "I", Default::default()),
                JavaFieldProto::new("__wieDirtyWidth", "I", Default::default()),
                JavaFieldProto::new("__wieDirtyHeight", "I", Default::default()),
            ],
            access_flags: ClassAccessFlags::ABSTRACT,
        }
    }

    async fn init(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::<init>({this:?})");

        let _: () = jvm
            .invoke_special(&this, "javax/microedition/lcdui/Displayable", "<init>", "()V", ())
            .await?;

        Ok(())
    }

    async fn repaint(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::repaint({this:?})");

        Self::mark_dirty(jvm, &this, 0, 0, -1, -1).await?;

        let display = jvm.get_field(&this, "currentDisplay", "Ljavax/microedition/lcdui/Display;").await?;
        let _: () = jvm.invoke_virtual(&display, "repaint", "(IIII)V", (0, 0, -1, -1)).await?;

        Ok(())
    }

    /// Adds one requested region to what the next paint has to cover.
    ///
    /// A paint pass is asynchronous - `repaint` only wakes the event queue - so
    /// several requests can pile up before it runs, and it has to cover all of
    /// them. A request that is not a positive rectangle means "everything", and
    /// once everything is pending nothing narrows it again.
    async fn mark_dirty(jvm: &Jvm, this: &ClassInstanceRef<Self>, x: i32, y: i32, width: i32, height: i32) -> JvmResult<()> {
        let mut this = this.clone();

        let pending_width: i32 = jvm.get_field(&this, "__wieDirtyWidth", "I").await?;
        let pending_height: i32 = jvm.get_field(&this, "__wieDirtyHeight", "I").await?;
        let everything_pending = pending_width <= 0 || pending_height <= 0;

        if width <= 0 || height <= 0 {
            jvm.put_field(&mut this, "__wieDirtyWidth", "I", -1).await?;
            jvm.put_field(&mut this, "__wieDirtyHeight", "I", -1).await?;

            return Ok(());
        }

        if everything_pending {
            // Nothing was pending: this request is the whole of it. (A pending
            // "everything" stays that way, and is handled by the branch below.)
            let nothing_pending = pending_width == 0 && pending_height == 0;
            if nothing_pending {
                jvm.put_field(&mut this, "__wieDirtyX", "I", x).await?;
                jvm.put_field(&mut this, "__wieDirtyY", "I", y).await?;
                jvm.put_field(&mut this, "__wieDirtyWidth", "I", width).await?;
                jvm.put_field(&mut this, "__wieDirtyHeight", "I", height).await?;
            }

            return Ok(());
        }

        let pending_x: i32 = jvm.get_field(&this, "__wieDirtyX", "I").await?;
        let pending_y: i32 = jvm.get_field(&this, "__wieDirtyY", "I").await?;

        let left = pending_x.min(x);
        let top = pending_y.min(y);
        let right = pending_x.saturating_add(pending_width).max(x.saturating_add(width));
        let bottom = pending_y.saturating_add(pending_height).max(y.saturating_add(height));

        jvm.put_field(&mut this, "__wieDirtyX", "I", left).await?;
        jvm.put_field(&mut this, "__wieDirtyY", "I", top).await?;
        jvm.put_field(&mut this, "__wieDirtyWidth", "I", right - left).await?;
        jvm.put_field(&mut this, "__wieDirtyHeight", "I", bottom - top).await?;

        Ok(())
    }

    async fn repaint_with_area(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::repaint({this:?}, {x}, {y}, {width}, {height})");

        Self::mark_dirty(jvm, &this, x, y, width, height).await?;

        let display = jvm.get_field(&this, "currentDisplay", "Ljavax/microedition/lcdui/Display;").await?;
        let _: () = jvm.invoke_virtual(&display, "repaint", "(IIII)V", (x, y, width, height)).await?;

        Ok(())
    }

    async fn service_repaints(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::warn!("stub javax.microedition.lcdui.Canvas::serviceRepaints({this:?})");

        jvm.invoke_virtual(&this, "repaint", "(IIII)V", (0, 0, 0, 0)).await
    }

    async fn get_game_action(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, key: i32) -> JvmResult<i32> {
        tracing::debug!("javax.microedition.lcdui.Canvas::getGameAction({this:?}, {key})");

        let action = match MIDPKeyCode::from_raw(key) {
            Some(MIDPKeyCode::UP) => 1,    // UP
            Some(MIDPKeyCode::DOWN) => 6,  // DOWN
            Some(MIDPKeyCode::LEFT) => 2,  // LEFT
            Some(MIDPKeyCode::RIGHT) => 5, // RIGHT
            Some(MIDPKeyCode::FIRE) => 8,  // FIRE,
            _ => 0,
        };

        Ok(action)
    }

    async fn key_pressed(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, key: i32) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::keyPressed({this:?}, {key})");

        Ok(())
    }

    async fn key_repeated(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, key: i32) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::keyRepeated({this:?}, {key})");

        Ok(())
    }

    async fn key_released(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, key: i32) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::keyReleased({this:?}, {key})");

        Ok(())
    }

    async fn set_full_screen_mode(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, mode: bool) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::setFullScreenMode({this:?}, {mode})");

        let previous_mode: bool = jvm.get_field(&this, "isInFullScreenMode", "Z").await?;
        if previous_mode == mode {
            return Ok(());
        }

        let display: ClassInstanceRef<Display> = jvm.get_field(&this, "currentDisplay", "Ljavax/microedition/lcdui/Display;").await?;
        if !display.is_null() {
            let _: () = jvm.invoke_virtual(&display, "setFullscreen", "(Z)V", (mode,)).await?;
        }

        jvm.put_field(&mut this, "isInFullScreenMode", "Z", mode).await?;

        Ok(())
    }

    async fn is_double_buffered(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::warn!("stub javax.microedition.lcdui.Canvas::isDoubleBuffered({this:?})");

        Ok(true)
    }

    async fn handle_key_event(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, event_type: i32, code: i32) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::handleKeyEvent({this:?}, {event_type}, {code})");

        let event_type = if let Some(event_type) = KeyboardEventType::from_raw(event_type) {
            event_type
        } else {
            return Err(jvm.exception("java/lang/IllegalArgumentException", "Invalid keyboard event type").await);
        };

        let _: () = match event_type {
            KeyboardEventType::KeyPressed => jvm.invoke_virtual(&this, "keyPressed", "(I)V", (code,)).await,
            KeyboardEventType::KeyReleased => jvm.invoke_virtual(&this, "keyReleased", "(I)V", (code,)).await,
            KeyboardEventType::KeyRepeated => jvm.invoke_virtual(&this, "keyRepeated", "(I)V", (code,)).await,
            _ => unimplemented!(),
        }?;

        Ok(())
    }

    async fn handle_paint_event(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        graphics: ClassInstanceRef<Graphics>,
    ) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Canvas::handlePaintEvent({this:?}, {graphics:?})");

        let _: () = jvm
            .invoke_virtual(&this, "paint", "(Ljavax/microedition/lcdui/Graphics;)V", (graphics,))
            .await?;

        Ok(())
    }
}
