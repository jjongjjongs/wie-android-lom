use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.TextPopup
pub struct TextPopup;

impl TextPopup {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextPopup",
            parent_class: Some("org/kwis/msp/lwc/TextBoxComponent"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Ljava/lang/String;III)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Ljava/lang/String;III)V",
                    Self::init_with_display,
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
                // Native TextPopup adds one field at +0xac for the listener.
                JavaFieldProto::new(
                    "__wieTextPopupActionListener",
                    "Lorg/kwis/msp/lwc/ActionListener;",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<TextPopup>,
        data: ClassInstanceRef<String>,
        constraint: i32,
        mode: i32,
        caret_position: i32,
    ) -> JvmResult<()> {
        let display: ClassInstanceRef<()> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_with_display(
            jvm,
            context,
            this,
            display,
            data,
            constraint,
            mode,
            caret_position,
        )
        .await
    }

    async fn init_with_display(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextPopup>,
        display: ClassInstanceRef<()>,
        data: ClassInstanceRef<String>,
        constraint: i32,
        mode: i32,
        caret_position: i32,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/TextBoxComponent",
                "<init>",
                "(Lorg/kwis/msp/lcdui/Display;Ljava/lang/String;I)V",
                (display, data, constraint),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "m_cPos",
            "I",
            caret_position,
        )
        .await?;

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

        jvm.put_field(
            &mut this,
            "iMode",
            "I",
            mode,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "changeModeCard",
                "()V",
                (),
            )
            .await?;

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
        mut this: ClassInstanceRef<TextPopup>,
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
            "__wieTextPopupActionListener",
            "Lorg/kwis/msp/lwc/ActionListener;",
            listener,
        )
        .await?;

        Ok(())
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextPopup>,
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

                let _: () = jvm
                    .invoke_virtual(
                        &this,
                        "changeModeCard",
                        "()V",
                        (),
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
                        "__wieTextPopupActionListener",
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
