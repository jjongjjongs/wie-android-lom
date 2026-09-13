use alloc::{
    string::{String as RustString, ToString},
    vec,
};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_backend::InputMethodOutput;
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::{
    javax::microedition::lcdui::{Canvas, Graphics},
    net::wie::MIDPKeyCode,
};

/// Tells the input method to finish whatever syllable it is holding and hand
/// it over, adding nothing new. The platform's own handler uses the same
/// sentinel when a directional key ends the composition.
const IME_FLUSH: i8 = -99;

/// Tells the input method to take one step back inside the syllable it is
/// composing, which is what CLEAR means while a syllable is still open.
const IME_BACKSPACE: i8 = -16;

/// A key press, as the input method numbers its events.
const IME_PRESS: u32 = 2;

/// Inset of the text from the field's border, in pixels.
const TEXT_INSET: i32 = 2;

// class com.xce.lcdui.XTextField
//
// SK-VM's on-screen text field. A title that asks the player for a name, a
// message or a password builds one, forwards its Canvas key events to it, and
// paints it from its own `paint`; the field itself composes the keypresses
// into text.
//
// Composition is the part that has to be right for Korean: a syllable is built
// up over several keys and is still editable while it is being built. The
// input method reports it as two pieces - what is finished (`output0`) and
// what is still open (`output1`) - and the field keeps the open piece at the
// end of its text, replacing it on each key. That is the same contract
// `MC_uicHandleInput` works to, so the two agree on what a half-typed syllable
// looks like.
pub struct XTextField;

impl XTextField {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "com/xce/lcdui/XTextField",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Ljava/lang/String;IILjavax/microedition/lcdui/Canvas;)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new("setFocus", "(Z)V", Self::set_focus, Default::default()),
                JavaMethodProto::new("setBounds", "(IIII)V", Self::set_bounds, Default::default()),
                JavaMethodProto::new("keyPressed", "(I)V", Self::key_pressed, Default::default()),
                JavaMethodProto::new("keyRepeated", "(I)V", Self::key_repeated, Default::default()),
                JavaMethodProto::new("keyReleased", "(I)V", Self::key_released, Default::default()),
                JavaMethodProto::new("paint", "(Ljavax/microedition/lcdui/Graphics;)V", Self::paint, Default::default()),
                JavaMethodProto::new("getText", "()Ljava/lang/String;", Self::get_text, Default::default()),
                JavaMethodProto::new("setText", "(Ljava/lang/String;)V", Self::set_text, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("text", "Ljava/lang/String;", Default::default()),
                // How many chars at the end of `text` are the syllable still
                // being composed, and so are replaced rather than added to by
                // the next key.
                JavaFieldProto::new("__wieXTextFieldComposition", "I", Default::default()),
                JavaFieldProto::new("__wieXTextFieldMaxSize", "I", Default::default()),
                JavaFieldProto::new("__wieXTextFieldConstraints", "I", Default::default()),
                JavaFieldProto::new("__wieXTextFieldFocused", "Z", Default::default()),
                JavaFieldProto::new("__wieXTextFieldX", "I", Default::default()),
                JavaFieldProto::new("__wieXTextFieldY", "I", Default::default()),
                JavaFieldProto::new("__wieXTextFieldWidth", "I", Default::default()),
                JavaFieldProto::new("__wieXTextFieldHeight", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        text: ClassInstanceRef<String>,
        max_size: i32,
        constraints: i32,
        canvas: ClassInstanceRef<Canvas>,
    ) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::<init>({this:?}, {text:?}, {max_size}, {constraints}, {canvas:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        let text = if text.is_null() {
            JavaLangString::from_rust_string(jvm, "").await?.into()
        } else {
            text
        };

        jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
        jvm.put_field(&mut this, "__wieXTextFieldComposition", "I", 0).await?;
        jvm.put_field(&mut this, "__wieXTextFieldMaxSize", "I", max_size).await?;
        jvm.put_field(&mut this, "__wieXTextFieldConstraints", "I", constraints).await?;
        // A field a title never calls `setFocus` on is still the field the
        // title is forwarding its keys to, so it starts able to take them. A
        // title juggling two fields is the one that calls `setFocus`, and
        // turning the other one off is what that call is for.
        jvm.put_field(&mut this, "__wieXTextFieldFocused", "Z", true).await?;

        Ok(())
    }

    async fn set_focus(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, focus: bool) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::setFocus({this:?}, {focus})");

        jvm.put_field(&mut this, "__wieXTextFieldFocused", "Z", focus).await?;

        Ok(())
    }

    async fn set_bounds(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::setBounds({this:?}, {x}, {y}, {width}, {height})");

        jvm.put_field(&mut this, "__wieXTextFieldX", "I", x).await?;
        jvm.put_field(&mut this, "__wieXTextFieldY", "I", y).await?;
        jvm.put_field(&mut this, "__wieXTextFieldWidth", "I", width).await?;
        jvm.put_field(&mut this, "__wieXTextFieldHeight", "I", height).await?;

        Ok(())
    }

    async fn key_pressed(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::keyPressed({this:?}, {key_code})");

        Self::handle_key(jvm, context, this, key_code).await
    }

    /// A held key repeats the same character, which is what typing a run of
    /// the same letter on a keypad does.
    async fn key_repeated(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::keyRepeated({this:?}, {key_code})");

        Self::handle_key(jvm, context, this, key_code).await
    }

    /// The input method works off presses, so a release changes nothing.
    async fn key_released(_jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::keyReleased({this:?}, {key_code})");

        Ok(())
    }

    async fn handle_key(jvm: &Jvm, context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, key_code: i32) -> JvmResult<()> {
        let focused: bool = jvm.get_field(&this, "__wieXTextFieldFocused", "Z").await?;
        if !focused {
            return Ok(());
        }

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let text = JavaLangString::to_rust_string(jvm, &text).await?;
        let composition: i32 = jvm.get_field(&this, "__wieXTextFieldComposition", "I").await?;
        let max_size: i32 = jvm.get_field(&this, "__wieXTextFieldMaxSize", "I").await?;

        let committed = Self::drop_last_chars(&text, composition);

        let updated = match MIDPKeyCode::from_raw(key_code) {
            // CLEAR steps back inside an open syllable if there is one, and
            // deletes a finished char if there is not.
            Some(MIDPKeyCode::CLEAR) => {
                if composition > 0 {
                    let output = context.system().handle_input_method(IME_BACKSPACE, IME_PRESS);

                    Self::append(&committed, &output, max_size)
                } else {
                    Some((Self::drop_last_chars(&committed, 1), 0))
                }
            }
            // Moving off the field ends the syllable it was holding.
            Some(MIDPKeyCode::UP | MIDPKeyCode::DOWN | MIDPKeyCode::LEFT | MIDPKeyCode::RIGHT) => {
                let output = context.system().handle_input_method(IME_FLUSH, IME_PRESS);

                Self::append(&committed, &output, max_size)
            }
            Some(
                MIDPKeyCode::KEY_NUM0
                | MIDPKeyCode::KEY_NUM1
                | MIDPKeyCode::KEY_NUM2
                | MIDPKeyCode::KEY_NUM3
                | MIDPKeyCode::KEY_NUM4
                | MIDPKeyCode::KEY_NUM5
                | MIDPKeyCode::KEY_NUM6
                | MIDPKeyCode::KEY_NUM7
                | MIDPKeyCode::KEY_NUM8
                | MIDPKeyCode::KEY_NUM9
                | MIDPKeyCode::KEY_POUND
                | MIDPKeyCode::KEY_STAR,
            ) => {
                let output = context.system().handle_input_method(key_code as i8, IME_PRESS);

                Self::append(&committed, &output, max_size)
            }
            // Soft keys, CALL, volume - the title's own, not the field's.
            _ => return Ok(()),
        };

        // A refused key has still moved the input method on, so end the
        // syllable it is now holding and drop it. Leaving it there would see
        // the refused character committed by the next key.
        //
        // What stays is the text as it was, not `committed`: the syllable that
        // was open before this key is still the player's, it has simply
        // stopped being editable now that the input method has moved past it.
        let (text, composition) = match updated {
            Some(updated) => updated,
            None => {
                let _ = context.system().handle_input_method(IME_FLUSH, IME_PRESS);

                (text, 0)
            }
        };

        let text = JavaLangString::from_rust_string(jvm, &text).await?;
        jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
        jvm.put_field(&mut this, "__wieXTextFieldComposition", "I", composition).await?;

        Ok(())
    }

    /// Adds what the input method produced to `committed`, answering the new
    /// text and how much of its tail is still being composed.
    ///
    /// `output0` is finished and `output1` is the syllable still open, both as
    /// the EUC-KR bytes the whole platform speaks. `None` means the result
    /// would run past `max_size`: the whole key is refused rather than half of
    /// it, because taking part of a syllable would leave the text and the
    /// input method disagreeing about what is still open.
    fn append(committed: &str, output: &InputMethodOutput, max_size: i32) -> Option<(RustString, i32)> {
        let finished = encoding_rs::EUC_KR.decode(&output.output0[..output.output0_len]).0;
        let composing = encoding_rs::EUC_KR.decode(&output.output1[..output.output1_len]).0;

        let mut text = committed.to_string();
        text.push_str(&finished);
        text.push_str(&composing);

        if max_size > 0 && text.chars().count() as i32 > max_size {
            return None;
        }

        Some((text, composing.chars().count() as i32))
    }

    fn drop_last_chars(text: &str, count: i32) -> RustString {
        if count <= 0 {
            return text.to_string();
        }

        let keep = text.chars().count().saturating_sub(count as usize);

        text.chars().take(keep).collect()
    }

    /// Draws the field: a border, the text inside it, and a caret at the end
    /// of the text while the field has focus.
    ///
    /// The colour the title was drawing in is put back afterwards, so a title
    /// that paints its own screen around the field is not left drawing in the
    /// field's colour.
    async fn paint(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>, graphics: ClassInstanceRef<Graphics>) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::paint({this:?}, {graphics:?})");

        if graphics.is_null() {
            return Ok(());
        }

        let x: i32 = jvm.get_field(&this, "__wieXTextFieldX", "I").await?;
        let y: i32 = jvm.get_field(&this, "__wieXTextFieldY", "I").await?;
        let width: i32 = jvm.get_field(&this, "__wieXTextFieldWidth", "I").await?;
        let height: i32 = jvm.get_field(&this, "__wieXTextFieldHeight", "I").await?;

        // A field the title never gave bounds to has nothing to draw into.
        if width <= 0 || height <= 0 {
            return Ok(());
        }

        let restore: i32 = jvm.invoke_virtual(&graphics, "getColor", "()I", ()).await?;

        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0xffffffu32 as i32,)).await?;
        let _: () = jvm.invoke_virtual(&graphics, "fillRect", "(IIII)V", (x, y, width, height)).await?;

        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0i32,)).await?;
        let _: () = jvm
            .invoke_virtual(&graphics, "drawRect", "(IIII)V", (x, y, width - 1, height - 1))
            .await?;

        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawString",
                "(Ljava/lang/String;III)V",
                (text.clone(), x + TEXT_INSET, y + TEXT_INSET, 0i32),
            )
            .await?;

        let focused: bool = jvm.get_field(&this, "__wieXTextFieldFocused", "Z").await?;
        if focused {
            let font = jvm.invoke_virtual(&graphics, "getFont", "()Ljavax/microedition/lcdui/Font;", ()).await?;
            let text_width: i32 = jvm
                .invoke_virtual(&font, "stringWidth", "(Ljava/lang/String;)I", (text,))
                .await
                .unwrap_or(0);

            let caret = x + TEXT_INSET + text_width;
            if caret < x + width - 1 {
                let _: () = jvm
                    .invoke_virtual(&graphics, "drawLine", "(IIII)V", (caret, y + TEXT_INSET, caret, y + height - TEXT_INSET))
                    .await?;
            }
        }

        let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (restore,)).await?;

        Ok(())
    }

    async fn get_text(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<String>> {
        tracing::debug!("com.xce.lcdui.XTextField::getText({this:?})");

        let text = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        Ok(text)
    }

    /// Replaces the text outright, which also ends any syllable in progress -
    /// what was being composed is not part of the text the title just set.
    async fn set_text(jvm: &Jvm, _context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, text: ClassInstanceRef<String>) -> JvmResult<()> {
        tracing::debug!("com.xce.lcdui.XTextField::setText({this:?}, {text:?})");

        let text = if text.is_null() {
            JavaLangString::from_rust_string(jvm, "").await?.into()
        } else {
            text
        };

        jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
        jvm.put_field(&mut this, "__wieXTextFieldComposition", "I", 0).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, string::String as RustString};

    use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

    use test_utils::run_jvm_test;
    use wie_util::Result;

    use crate::get_protos;

    fn protos() -> Box<[Box<[wie_jvm_support::WieJavaClassProto]>]> {
        Box::new([wie_midp::get_protos().into(), get_protos().into()])
    }

    /// The SK-VM key codes, as `MIDPKeyCode` numbers them.
    const KEY_2: i32 = 50;
    const KEY_3: i32 = 51;
    const KEY_4: i32 = 52;
    const CLEAR: i32 = 8;
    const RIGHT: i32 = 145;
    const LEFT_SOFT_KEY: i32 = 6;

    async fn field(jvm: &Jvm, text: &str, max_size: i32) -> JvmResult<ClassInstanceRef<()>> {
        let text = JavaLangString::from_rust_string(jvm, text).await?;

        Ok(jvm
            .new_class(
                "com/xce/lcdui/XTextField",
                "(Ljava/lang/String;IILjavax/microedition/lcdui/Canvas;)V",
                (text, max_size, 0i32, ClassInstanceRef::<()>::new(None)),
            )
            .await?
            .into())
    }

    async fn press(jvm: &Jvm, field: &ClassInstanceRef<()>, key: i32) -> JvmResult<()> {
        jvm.invoke_virtual(field, "keyPressed", "(I)V", (key,)).await
    }

    async fn text(jvm: &Jvm, field: &ClassInstanceRef<()>) -> JvmResult<RustString> {
        let text = jvm.invoke_virtual(field, "getText", "()Ljava/lang/String;", ()).await?;

        JavaLangString::to_rust_string(jvm, &text).await
    }

    /// The default input mode is English multi-tap, where 2 starts on `a`.
    #[test]
    fn a_keypad_press_puts_its_letter_in_the_field() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, KEY_2).await?;

            assert_eq!(text(&jvm, &field).await?, "a");

            Ok(())
        })
    }

    /// Tapping the same key again replaces the letter being composed rather
    /// than adding to it - that is what multi-tap is.
    #[test]
    fn tapping_the_same_key_cycles_the_letter_in_place() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, KEY_2).await?;
            press(&jvm, &field, KEY_2).await?;

            assert_eq!(text(&jvm, &field).await?, "b");

            Ok(())
        })
    }

    /// A different key finishes the one being composed and starts a new one.
    #[test]
    fn a_different_key_commits_what_came_before_it() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, KEY_2).await?;
            press(&jvm, &field, KEY_2).await?;
            press(&jvm, &field, KEY_3).await?;

            assert_eq!(text(&jvm, &field).await?, "bd");

            Ok(())
        })
    }

    /// Moving off the field ends the letter it was holding, so the next key
    /// does not replace it.
    #[test]
    fn a_directional_key_ends_the_letter_being_composed() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, KEY_2).await?;
            press(&jvm, &field, RIGHT).await?;
            press(&jvm, &field, KEY_2).await?;

            assert_eq!(text(&jvm, &field).await?, "aa");

            Ok(())
        })
    }

    /// CLEAR takes back the letter still being composed.
    #[test]
    fn clear_takes_back_the_letter_being_composed() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, KEY_2).await?;
            press(&jvm, &field, KEY_3).await?;
            press(&jvm, &field, CLEAR).await?;

            assert_eq!(text(&jvm, &field).await?, "a");

            Ok(())
        })
    }

    /// With nothing being composed, CLEAR deletes a finished char - including
    /// one the title put there itself.
    #[test]
    fn clear_deletes_a_finished_char() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "ab", 0).await?;

            press(&jvm, &field, CLEAR).await?;

            assert_eq!(text(&jvm, &field).await?, "a");

            Ok(())
        })
    }

    /// CLEAR on an empty field is not an error.
    #[test]
    fn clear_on_an_empty_field_leaves_it_empty() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, CLEAR).await?;

            assert_eq!(text(&jvm, &field).await?, "");

            Ok(())
        })
    }

    /// A full field refuses the key outright, and - the part that is easy to
    /// get wrong - the refused letter must not turn up on the next one.
    #[test]
    fn a_full_field_refuses_the_key_and_does_not_keep_it() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 2).await?;

            press(&jvm, &field, KEY_2).await?;
            press(&jvm, &field, KEY_3).await?;
            assert_eq!(text(&jvm, &field).await?, "ad");

            press(&jvm, &field, KEY_4).await?;
            assert_eq!(text(&jvm, &field).await?, "ad");

            press(&jvm, &field, KEY_4).await?;
            assert_eq!(text(&jvm, &field).await?, "ad");

            Ok(())
        })
    }

    /// A field the title has taken focus from is not the one the keys are for.
    #[test]
    fn an_unfocused_field_takes_no_keys() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            let _: () = jvm.invoke_virtual(&field, "setFocus", "(Z)V", (false,)).await?;
            press(&jvm, &field, KEY_2).await?;
            assert_eq!(text(&jvm, &field).await?, "");

            let _: () = jvm.invoke_virtual(&field, "setFocus", "(Z)V", (true,)).await?;
            press(&jvm, &field, KEY_2).await?;
            assert_eq!(text(&jvm, &field).await?, "a");

            Ok(())
        })
    }

    /// Soft keys belong to the title's own menu, not to the field.
    #[test]
    fn a_soft_key_is_left_to_the_title() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, LEFT_SOFT_KEY).await?;

            assert_eq!(text(&jvm, &field).await?, "");

            Ok(())
        })
    }

    /// Setting the text outright ends any letter in progress, so the next key
    /// adds to what was set instead of replacing part of it.
    #[test]
    fn setting_the_text_ends_the_letter_in_progress() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field = field(&jvm, "", 0).await?;

            press(&jvm, &field, KEY_2).await?;

            let replacement = JavaLangString::from_rust_string(&jvm, "xy").await?;
            let _: () = jvm.invoke_virtual(&field, "setText", "(Ljava/lang/String;)V", (replacement,)).await?;

            assert_eq!(text(&jvm, &field).await?, "xy");

            Ok(())
        })
    }

    /// A title that hands the field no text starts it empty rather than with a
    /// null it will later draw.
    #[test]
    fn a_null_initial_text_becomes_an_empty_one() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let field: ClassInstanceRef<()> = jvm
                .new_class(
                    "com/xce/lcdui/XTextField",
                    "(Ljava/lang/String;IILjavax/microedition/lcdui/Canvas;)V",
                    (
                        ClassInstanceRef::<java_runtime::classes::java::lang::String>::new(None),
                        16i32,
                        0i32,
                        ClassInstanceRef::<()>::new(None),
                    ),
                )
                .await?
                .into();

            assert_eq!(text(&jvm, &field).await?, "");

            Ok(())
        })
    }

    /// Painting a field the title never gave bounds to draws nothing rather
    /// than a zero-sized box, and painting one with bounds comes back without
    /// disturbing the colour the title was drawing in.
    #[test]
    fn painting_leaves_the_titles_colour_alone() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let image: ClassInstanceRef<()> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "(II)Ljavax/microedition/lcdui/Image;",
                    (64i32, 24i32),
                )
                .await?;
            let graphics: ClassInstanceRef<()> = jvm
                .invoke_virtual(&image, "getGraphics", "()Ljavax/microedition/lcdui/Graphics;", ())
                .await?;
            let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0x123456i32,)).await?;

            let field = field(&jvm, "ab", 0).await?;

            let _: () = jvm
                .invoke_virtual(&field, "paint", "(Ljavax/microedition/lcdui/Graphics;)V", (graphics.clone(),))
                .await?;
            let color: i32 = jvm.invoke_virtual(&graphics, "getColor", "()I", ()).await?;
            assert_eq!(color, 0x123456);

            let _: () = jvm.invoke_virtual(&field, "setBounds", "(IIII)V", (0i32, 0i32, 64i32, 20i32)).await?;
            let _: () = jvm
                .invoke_virtual(&field, "paint", "(Ljavax/microedition/lcdui/Graphics;)V", (graphics.clone(),))
                .await?;
            let color: i32 = jvm.invoke_virtual(&graphics, "getColor", "()I", ()).await?;
            assert_eq!(color, 0x123456);

            Ok(())
        })
    }
}
