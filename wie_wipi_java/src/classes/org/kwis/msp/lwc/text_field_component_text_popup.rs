use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.TextPopup
pub struct TextFieldComponentTextPopup;

impl TextFieldComponentTextPopup {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextFieldComponent$TextPopup",
            parent_class: Some("org/kwis/msp/lwc/TextBoxComponent"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lwc/TextFieldComponent;Ljava/lang/String;III)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setWide",
                    "(ZLorg/kwis/msp/lwc/ActionListener;)V",
                    Self::set_wide,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "keyNotify",
                    "(II)Z",
                    Self::key_notify,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new(
                    "__wieTextFieldPopupActionListener",
                    "Lorg/kwis/msp/lwc/ActionListener;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieTextFieldPopupOuter",
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
        mut this: ClassInstanceRef<TextFieldComponentTextPopup>,
        outer: ClassInstanceRef<()>,
        data: ClassInstanceRef<String>,
        constraint: i32,
        mode: i32,
        caret_position: i32,
    ) -> JvmResult<()> {
        // Native ctor @ 0x246954.
        //
        // TextBoxComponent.<init>(String, constraint)
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextBoxComponent",
                "<init>",
                "(Ljava/lang/String;I)V",
                (data, constraint),
            )
            .await?;

        // +0xb0 = outer TextFieldComponent
        jvm.put_field(
            &mut this,
            "__wieTextFieldPopupOuter",
            "Lorg/kwis/msp/lwc/TextFieldComponent;",
            outer,
        )
        .await?;

        // inherited m_cPos(+0x3c) = caret
        jvm.put_field(
            &mut this,
            "m_cPos",
            "I",
            caret_position,
        )
        .await?;

        // formatter(+0x90).setCurrent(caret)
        let formatter: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "__wieTextFormatter",
                "Lorg/kwis/msp/lwc/TextFormatProcessor;",
            )
            .await?;

        if formatter.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let _: () = jvm
            .invoke_virtual(
                &formatter,
                "setCurrent",
                "(I)V",
                (caret_position,),
            )
            .await?;

        // imHandler(+0x38).setCurrentMode(mode)
        let im_handler: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "imHandler",
                "Lorg/kwis/msp/lcdui/InputMethodHandler;",
            )
            .await?;

        if im_handler.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let _: bool = jvm
            .invoke_virtual(
                &im_handler,
                "setCurrentMode",
                "(I)Z",
                (mode,),
            )
            .await?;

        // iMode(+0x4c) = mode
        jvm.put_field(
            &mut this,
            "iMode",
            "I",
            mode,
        )
        .await?;

        // this.changeModeCard(mode)
        let _: () = jvm
            .invoke_virtual(
                &this,
                "changeModeCard",
                "(I)V",
                (mode,),
            )
            .await?;

        // mode 99 => setSymbolPosition()
        if mode == 99 {
            let _: () = jvm
                .invoke_virtual(
                    &this,
                    "setSymbolPosition",
                    "()V",
                    (),
                )
                .await?;
        }

        Ok(())
    }

    async fn set_wide(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextFieldComponentTextPopup>,
        wide: bool,
        listener: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native:
        //   TextBox/TextPopup data +0x98 = wide
        //   TextPopup data         +0xac = ActionListener
        jvm.put_field(
            &mut this,
            "__wieTextBoxFlag98",
            "I",
            if wide { 1 } else { 0 },
        )
        .await?;

        jvm.put_field(
            &mut this,
            "__wieTextFieldPopupActionListener",
            "Lorg/kwis/msp/lwc/ActionListener;",
            listener,
        )
        .await?;

        Ok(())
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextFieldComponentTextPopup>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        // Native always resolves the game action before event-type dispatch.
        let action: i32 = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getGameAction",
                "(I)I",
                (key,),
            )
            .await?;

        if matches!(event_type, 1 | 2 | 3) {
            if action == 90 {
                if event_type != 1 {
                    return Ok(true);
                }

                let im_handler: ClassInstanceRef<()> = jvm
                    .get_field(
                        &this,
                        "imHandler",
                        "Lorg/kwis/msp/lcdui/InputMethodHandler;",
                    )
                    .await?;

                if im_handler.is_null() {
                    return Err(
                        jvm.exception("java/lang/NullPointerException", "")
                            .await,
                    );
                }

                let _: () = jvm
                    .invoke_virtual(
                        &im_handler,
                        "changeCurrentModeToNext",
                        "()V",
                        (),
                    )
                    .await?;

                let mode: i32 = jvm
                    .invoke_virtual(
                        &im_handler,
                        "getCurrentMode",
                        "()I",
                        (),
                    )
                    .await?;

                let _: () = jvm
                    .invoke_virtual(
                        &this,
                        "modeSetting",
                        "(I)V",
                        (mode,),
                    )
                    .await?;

                let mode: i32 =
                    jvm.get_field(&this, "iMode", "I").await?;

                let _: () = jvm
                    .invoke_virtual(
                        &this,
                        "changeModeCard",
                        "(I)V",
                        (mode,),
                    )
                    .await?;

                return Ok(true);
            }

            if action == 8 {
                if event_type != 1 {
                    return Ok(true);
                }

                let im_handler: ClassInstanceRef<()> = jvm
                    .get_field(
                        &this,
                        "imHandler",
                        "Lorg/kwis/msp/lcdui/InputMethodHandler;",
                    )
                    .await?;

                if im_handler.is_null() {
                    return Err(
                        jvm.exception("java/lang/NullPointerException", "")
                            .await,
                    );
                }

                let mode: i32 = jvm
                    .invoke_virtual(
                        &im_handler,
                        "getCurrentMode",
                        "()I",
                        (),
                    )
                    .await?;

                if mode == 99 {
                    return Ok(true);
                }

                let _: bool = jvm
                    .invoke_virtual(
                        &im_handler,
                        "notifyKeyInput",
                        "(II)Z",
                        (-99, event_type),
                    )
                    .await?;

                let text: ClassInstanceRef<String> = jvm
                    .invoke_virtual(
                        &this,
                        "getString",
                        "()Ljava/lang/String;",
                        (),
                    )
                    .await?;

                let listener: ClassInstanceRef<()> = jvm
                    .get_field(
                        &this,
                        "__wieTextFieldPopupActionListener",
                        "Lorg/kwis/msp/lwc/ActionListener;",
                    )
                    .await?;

                if listener.is_null() {
                    return Err(
                        jvm.exception("java/lang/NullPointerException", "")
                            .await,
                    );
                }

                let _: () = jvm
                    .invoke_virtual(
                        &listener,
                        "action",
                        "(Lorg/kwis/msp/lwc/Component;Ljava/lang/Object;)V",
                        (this.clone(), text),
                    )
                    .await?;

                return Ok(true);
            }

            return jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/TextBoxComponent",
                    "keyNotify",
                    "(II)Z",
                    (event_type, key),
                )
                .await;
        }

        if event_type == 4 {
            let _: bool = jvm
                .invoke_special(
                    &this,
                    "org/kwis/msp/lwc/TextBoxComponent",
                    "keyNotify",
                    "(II)Z",
                    (event_type, key),
                )
                .await?;

            return Ok(true);
        }

        Ok(false)
    }
}
