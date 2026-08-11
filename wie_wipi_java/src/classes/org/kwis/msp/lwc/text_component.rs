use alloc::{string::ToString, vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.TextComponent
pub struct TextComponent;

impl TextComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("setMaxLength", "(I)V", Self::set_max_length, Default::default()),
                JavaMethodProto::new("getString", "()Ljava/lang/String;", Self::get_string, Default::default()),
                JavaMethodProto::new("insert", "(Ljava/lang/String;III)V", Self::insert, Default::default()),
                JavaMethodProto::new("delete", "(II)V", Self::delete, Default::default()),
                JavaMethodProto::new("replace", "(Ljava/lang/String;II)V", Self::replace, Default::default()),
                JavaMethodProto::new("keyNotify", "(II)Z", Self::key_notify, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("m_cPos", "I", Default::default()),
                JavaFieldProto::new("imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;", Default::default()),
                JavaFieldProto::new("iMode", "I", Default::default()),
                JavaFieldProto::new("text", "Ljava/lang/String;", Default::default()),
                JavaFieldProto::new("maxLength", "I", Default::default()),
                // Native TextComponent has its own display/modeViewer state.
                // Synthetic names avoid colliding with Component.display while
                // preserving the native per-instance lifecycle.
                JavaFieldProto::new(
                    "__wieTextDisplay",
                    "Lorg/kwis/msp/lcdui/Display;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieModeViewer",
                    "Lorg/kwis/msp/lwc/TextComponent$ModeViewer;",
                    Default::default(),
                ),
                // WipiPlayer Plus keeps these values in TextComponent's
                // native per-instance auxiliary area (+0x74..+0x84).
                // They are WIE-internal storage, not platform Java fields.
                JavaFieldProto::new("__wieViewportHeight", "I", Default::default()),
                JavaFieldProto::new("__wieViewportY", "I", Default::default()),
                JavaFieldProto::new("__wieViewportWidth", "I", Default::default()),
                JavaFieldProto::new("__wieViewportX", "I", Default::default()),
                JavaFieldProto::new("__wieViewportParentHeight", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<TextComponent>) -> JvmResult<()> {
        tracing::debug!("stub org.kwis.msp.lwc.TextComponent::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "org/kwis/msp/lwc/Component", "<init>", "()V", ()).await?;

        // Native no-display constructor obtains Display.getDefaultDisplay()
        // and delegates with CONSTRAINT_ANY (0) on the current WIE path.
        let display: ClassInstanceRef<crate::classes::org::kwis::msp::lcdui::Display> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "__wieTextDisplay",
            "Lorg/kwis/msp/lcdui/Display;",
            display.clone(),
        )
        .await?;

        let im_handler = jvm
            .new_class(
                "org/kwis/msp/lcdui/InputMethodHandler",
                "(I)V",
                (0,),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "imHandler",
            "Lorg/kwis/msp/lcdui/InputMethodHandler;",
            im_handler,
        )
        .await?;

        let mode_viewer = jvm
            .new_class(
                "org/kwis/msp/lwc/TextComponent$ModeViewer",
                "(Lorg/kwis/msp/lwc/TextComponent;Lorg/kwis/msp/lcdui/Display;)V",
                (this.clone(), display),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "__wieModeViewer",
            "Lorg/kwis/msp/lwc/TextComponent$ModeViewer;",
            mode_viewer,
        )
        .await?;

        let text = JavaLangString::from_rust_string(jvm, "").await?;
        jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
        jvm.put_field(&mut this, "maxLength", "I", -1).await?;

        Ok(())
    }

    pub(crate) async fn calc_view_port_area(
        jvm: &Jvm,
        mut this: ClassInstanceRef<TextComponent>,
    ) -> JvmResult<()> {
        // Native TextComponent.calcViewPortArea:
        // intersect this component's screen rectangle with every ancestor.
        let mut top: i32 = jvm
            .invoke_virtual(&this, "getYOnScreen", "()I", ())
            .await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;
        let mut bottom = top + height;

        let mut left: i32 = jvm
            .invoke_virtual(&this, "getXOnScreen", "()I", ())
            .await?;
        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let mut right = left + width;

        let mut current: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        while !current.is_null() {
            let current_top: i32 = jvm
                .invoke_virtual(&current, "getYOnScreen", "()I", ())
                .await?;
            let current_height: i32 = jvm.get_field(&current, "h", "I").await?;
            let current_bottom = current_top + current_height;

            if current_top > top {
                top = current_top;
            }
            if current_bottom < bottom {
                bottom = current_bottom;
            }

            let current_left: i32 = jvm
                .invoke_virtual(&current, "getXOnScreen", "()I", ())
                .await?;
            let current_width: i32 = jvm.get_field(&current, "w", "I").await?;
            let current_right = current_left + current_width;

            if current_left > left {
                left = current_left;
            }
            if current_right < right {
                right = current_right;
            }

            current = jvm
                .get_field(
                    &current,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;
        }

        // Native stores the subtraction result without clamping.
        jvm.put_field(&mut this, "__wieViewportHeight", "I", bottom - top)
            .await?;
        jvm.put_field(&mut this, "__wieViewportY", "I", top)
            .await?;
        jvm.put_field(&mut this, "__wieViewportWidth", "I", right - left)
            .await?;
        jvm.put_field(&mut this, "__wieViewportX", "I", left)
            .await?;

        Ok(())
    }

    pub(crate) async fn pcalc_view_port_area(
        jvm: &Jvm,
        mut this: ClassInstanceRef<TextComponent>,
    ) -> JvmResult<()> {
        // Native pcalcViewPortArea starts at the parent, not at this.
        let mut current: ClassInstanceRef<()> = jvm
            .get_field(
                &this,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        // Native dereferences the parent immediately.
        if current.is_null() {
            return Err(
                jvm.exception("java/lang/NullPointerException", "")
                    .await,
            );
        }

        let mut top: i32 = jvm
            .invoke_virtual(&current, "getYOnScreen", "()I", ())
            .await?;
        let height: i32 = jvm.get_field(&current, "h", "I").await?;
        let mut bottom = top + height;

        current = jvm
            .get_field(
                &current,
                "parent",
                "Lorg/kwis/msp/lwc/ContainerComponent;",
            )
            .await?;

        while !current.is_null() {
            let current_top: i32 = jvm
                .invoke_virtual(&current, "getYOnScreen", "()I", ())
                .await?;
            let current_height: i32 = jvm.get_field(&current, "h", "I").await?;
            let current_bottom = current_top + current_height;

            if current_top > top {
                top = current_top;
            }
            if current_bottom < bottom {
                bottom = current_bottom;
            }

            current = jvm
                .get_field(
                    &current,
                    "parent",
                    "Lorg/kwis/msp/lwc/ContainerComponent;",
                )
                .await?;
        }

        // Native +0x84: vertical intersection span of the ancestor chain.
        jvm.put_field(
            &mut this,
            "__wieViewportParentHeight",
            "I",
            bottom - top,
        )
        .await?;

        Ok(())
    }

    async fn set_max_length(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        max_length: i32,
    ) -> JvmResult<()> {
        jvm.put_field(&mut this, "maxLength", "I", max_length).await?;

        if max_length > 0 {
            let text: ClassInstanceRef<String> =
                jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
            let length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

            if length > max_length {
                let text: ClassInstanceRef<String> = jvm
                    .invoke_virtual(
                        &text,
                        "substring",
                        "(II)Ljava/lang/String;",
                        (0, max_length),
                    )
                    .await?;

                jvm.put_field(&mut this, "text", "Ljava/lang/String;", text).await?;
                jvm.put_field(&mut this, "m_cPos", "I", 0).await?;
            }
        }

        Ok(())
    }

    async fn insert(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        string: ClassInstanceRef<String>,
        offset: i32,
        length: i32,
        position: i32,
    ) -> JvmResult<()> {
        let text: ClassInstanceRef<String> =
            jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let text_length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

        let inserted: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &string,
                "substring",
                "(II)Ljava/lang/String;",
                (offset, offset + length),
            )
            .await?;

        let prefix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (0, position),
            )
            .await?;

        let suffix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (position, text_length),
            )
            .await?;

        let combined: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &prefix,
                "concat",
                "(Ljava/lang/String;)Ljava/lang/String;",
                (inserted,),
            )
            .await?;

        let combined: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &combined,
                "concat",
                "(Ljava/lang/String;)Ljava/lang/String;",
                (suffix,),
            )
            .await?;

        jvm.put_field(&mut this, "text", "Ljava/lang/String;", combined).await?;

        Ok(())
    }

    async fn delete(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        position: i32,
        length: i32,
    ) -> JvmResult<()> {
        let text: ClassInstanceRef<String> =
            jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
        let text_length: i32 = jvm.invoke_virtual(&text, "length", "()I", ()).await?;

        let prefix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (0, position),
            )
            .await?;

        let suffix: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &text,
                "substring",
                "(II)Ljava/lang/String;",
                (position + length, text_length),
            )
            .await?;

        let combined: ClassInstanceRef<String> = jvm
            .invoke_virtual(
                &prefix,
                "concat",
                "(Ljava/lang/String;)Ljava/lang/String;",
                (suffix,),
            )
            .await?;

        jvm.put_field(&mut this, "text", "Ljava/lang/String;", combined).await?;

        Ok(())
    }

    async fn replace(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<TextComponent>,
        string: ClassInstanceRef<String>,
        length: i32,
        position: i32,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_virtual(
                &this,
                "delete",
                "(II)V",
                (position, length),
            )
            .await?;

        let string_length: i32 =
            jvm.invoke_virtual(&string, "length", "()I", ()).await?;

        jvm.invoke_virtual(
            &this,
            "insert",
            "(Ljava/lang/String;III)V",
            (string, 0, string_length, position),
        )
        .await
    }

    async fn key_notify(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<TextComponent>,
        event_type: i32,
        key: i32,
    ) -> JvmResult<bool> {
        let game_action: i32 = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getGameAction",
                "(I)I",
                (key,),
            )
            .await?;

        // Native game action 90 is the input-mode key. Only the type 1
        // event advances the mode; the paired type 0 event is consumed.
        if game_action == 90 {
            if event_type == 1 {
                let im_handler =
                    jvm.get_field(&this, "imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;").await?;

                let _: () = jvm
                    .invoke_virtual(&im_handler, "changeCurrentModeToNext", "()V", ())
                    .await?;

                let mode: i32 = jvm
                    .invoke_virtual(&im_handler, "getCurrentModeCode", "()I", ())
                    .await?;

                jvm.put_field(&mut this, "iMode", "I", mode).await?;
            }

            return Ok(true);
        }

        if matches!(event_type, 1 | 2 | 3) {
            let im_handler =
                jvm.get_field(&this, "imHandler", "Lorg/kwis/msp/lcdui/InputMethodHandler;").await?;

            let handled: bool = jvm
                .invoke_virtual(
                    &im_handler,
                    "notifyKeyInput",
                    "(II)Z",
                    (key, event_type),
                )
                .await?;

            if handled {
                // Native mode-3 input reaches TextComponent through
                // InputMethodListener.notifyTextChanged and inserts at m_cPos.
                let text: ClassInstanceRef<String> =
                    jvm.get_field(&this, "text", "Ljava/lang/String;").await?;
                let text_length: i32 =
                    jvm.invoke_virtual(&text, "length", "()I", ()).await?;

                let max_length: i32 = jvm.get_field(&this, "maxLength", "I").await?;
                if max_length < 0 || text_length < max_length {
                    if let Some(chr) = core::char::from_u32(key as u32) {
                        let input = JavaLangString::from_rust_string(jvm, &chr.to_string()).await?;
                        let position: i32 = jvm.get_field(&this, "m_cPos", "I").await?;

                        let _: () = jvm
                            .invoke_virtual(
                                &this,
                                "insert",
                                "(Ljava/lang/String;III)V",
                                (input, 0, 1, position),
                            )
                            .await?;

                        jvm.put_field(&mut this, "m_cPos", "I", position + 1).await?;
                    }
                }

                return Ok(true);
            }
        }

        jvm.invoke_special(
            &this,
            "org/kwis/msp/lwc/Component",
            "keyNotify",
            "(II)Z",
            (event_type, key),
        )
        .await
    }

    async fn get_string(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<TextComponent>) -> JvmResult<ClassInstanceRef<String>> {
        let text: ClassInstanceRef<String> = jvm.get_field(&this, "text", "Ljava/lang/String;").await?;

        Ok(text)
    }
}
