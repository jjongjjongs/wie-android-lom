use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{
    ClassInstanceRef, JavaChar, JavaError, Jvm,
    Result as JvmResult,
};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::lwc::TextFieldComponent;

// class org.kwis.msp.lwc.TextFieldComponent$Action
pub struct TextFieldComponentAction;

impl TextFieldComponentAction {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextFieldComponent$Action",
            parent_class: Some("java/lang/Object"),
            interfaces: vec!["org/kwis/msp/lwc/ActionListener"],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lwc/TextFieldComponent;)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "action",
                    "(Lorg/kwis/msp/lwc/Component;Ljava/lang/Object;)V",
                    Self::action,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "__wieOuter",
                    "Lorg/kwis/msp/lwc/TextFieldComponent;",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponentAction>,
        outer: ClassInstanceRef<TextFieldComponent>,
    ) -> JvmResult<()> {
        // Native ctor @ 0x246528: synthetic outer field +0x00.
        jvm.put_field(
            &mut this,
            "__wieOuter",
            "Lorg/kwis/msp/lwc/TextFieldComponent;",
            outer,
        )
        .await?;

        Ok(())
    }

    async fn action(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFieldComponentAction>,
        source: ClassInstanceRef<()>,
        data: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native action_v0 @ 0x246550.

        let outer: ClassInstanceRef<TextFieldComponent> = jvm
            .get_field(
                &this,
                "__wieOuter",
                "Lorg/kwis/msp/lwc/TextFieldComponent;",
            )
            .await?;

        // Native Object -> String checkcast.
        // A null reference is accepted by checkcast.
        if !data.is_null()
            && !jvm.is_instance(&**data, "java/lang/String")
        {
            let exception = jvm
                .instantiate_class("java/lang/ClassCastException")
                .await?;
            return Err(JavaError::JavaException(exception));
        }

        if source.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        // Native also checkcasts the Component source to the nested
        // TextFieldComponent$TextPopup before reading m_cPos.
        if !jvm.is_instance(
            &**source,
            "org/kwis/msp/lwc/TextFieldComponent$TextPopup",
        ) {
            let exception = jvm
                .instantiate_class("java/lang/ClassCastException")
                .await?;
            return Err(JavaError::JavaException(exception));
        }

        let caret: i32 =
            jvm.get_field(&source, "m_cPos", "I").await?;

        let text: ClassInstanceRef<String> =
            ClassInstanceRef::new(data.instance.clone());

        if outer.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        // Native vslot +0xf0:
        // outer.setString((String)data, source.m_cPos)
        let _: () = jvm
            .invoke_virtual(
                &outer,
                "setString",
                "(Ljava/lang/String;I)V",
                (text, caret),
            )
            .await?;

        let popup_caret: i32 =
            jvm.get_field(&source, "m_cPos", "I").await?;

        let mut outer = outer;

        // access$002(outer, popupCaret - 1)
        let mut visible_end = popup_caret.wrapping_sub(1);
        jvm.put_field(
            &mut outer,
            "__wieTextFieldVisibleEnd",
            "I",
            visible_end,
        )
        .await?;

        // Native +0x40 is TextComponent.charCount.  The Rust port
        // derives it from the current String rather than storing a
        // separate synthetic field.
        let current_text: ClassInstanceRef<String> = jvm
            .get_field(
                &outer,
                "text",
                "Ljava/lang/String;",
            )
            .await?;

        let char_count: i32 = if current_text.is_null() {
            0
        } else {
            jvm.invoke_virtual(
                &current_text,
                "length",
                "()I",
                (),
            )
            .await?
        };

        // Clamp visibleEnd to charCount - 1.
        if visible_end >= char_count {
            visible_end = char_count.wrapping_sub(1);
            jvm.put_field(
                &mut outer,
                "__wieTextFieldVisibleEnd",
                "I",
                visible_end,
            )
            .await?;
        }

        // Starting from visibleEnd, accumulate character widths toward
        // the left.  The first character that would make the range
        // wider than viewportWidth is excluded and visibleStart becomes
        // index + 1.
        if visible_end >= 0 {
            if current_text.is_null() {
                return Err(
                    jvm.exception("java/lang/NullPointerException", "")
                        .await,
                );
            }

            let font: ClassInstanceRef<()> = jvm
                .get_field(
                    &outer,
                    "__wieFont",
                    "Lorg/kwis/msp/lcdui/Font;",
                )
                .await?;

            if font.is_null() {
                return Err(
                    jvm.exception("java/lang/NullPointerException", "")
                        .await,
                );
            }

            let viewport_width: i32 = jvm
                .get_field(
                    &outer,
                    "__wieTextFieldViewportWidth",
                    "I",
                )
                .await?;

            let mut index = visible_end;
            let mut accumulated_width = 0i32;

            while index >= 0 {
                let ch: JavaChar = jvm
                    .invoke_virtual(
                        &current_text,
                        "charAt",
                        "(I)C",
                        (index,),
                    )
                    .await?;

                let char_width: i32 = jvm
                    .invoke_virtual(
                        &font,
                        "charWidth",
                        "(C)I",
                        (ch,),
                    )
                    .await?;

                accumulated_width =
                    accumulated_width.wrapping_add(char_width);

                if accumulated_width > viewport_width {
                    jvm.put_field(
                        &mut outer,
                        "__wieTextFieldVisibleStart",
                        "I",
                        index.wrapping_add(1),
                    )
                    .await?;
                    break;
                }

                index = index.wrapping_sub(1);
            }
        }

        // Hide and release the wide-editor Shell.
        let shell: ClassInstanceRef<()> = jvm
            .get_field(
                &outer,
                "__wieTextFieldPopup",
                "Lorg/kwis/msp/lwc/ShellComponent;",
            )
            .await?;

        if shell.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_virtual(
                &shell,
                "hide",
                "()V",
                (),
            )
            .await?;

        let null_shell: ClassInstanceRef<()> =
            ClassInstanceRef::new(None);

        jvm.put_field(
            &mut outer,
            "__wieTextFieldPopup",
            "Lorg/kwis/msp/lwc/ShellComponent;",
            null_shell,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(
                &outer,
                "repaint",
                "()V",
                (),
            )
            .await?;

        Ok(())
    }
}
