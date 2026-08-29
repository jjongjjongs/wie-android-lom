use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::{lcdui::Display, lwc::AnnunciatorComponent};

// class org.kwis.msp.lwc.AnnunciatorComponent$AnnunciatorEventListener
pub struct AnnunciatorComponentEventListener;

impl AnnunciatorComponentEventListener {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/AnnunciatorComponent$AnnunciatorEventListener",
            parent_class: Some("java/lang/Object"),
            interfaces: vec!["org/kwis/msp/lcdui/JletEventListener"],
            methods: vec![
                JavaMethodProto::new("<init>", "(Lorg/kwis/msp/lwc/AnnunciatorComponent;)V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lwc/AnnunciatorComponent;Lorg/kwis/msp/lwc/AnnunciatorComponent$1;)V",
                    Self::init_synthetic,
                    Default::default(),
                ),
                JavaMethodProto::new("notifyEvent", "(III)V", Self::notify_event, Default::default()),
            ],
            fields: vec![JavaFieldProto::new(
                "__wieOuter",
                "Lorg/kwis/msp/lwc/AnnunciatorComponent;",
                Default::default(),
            )],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        outer: ClassInstanceRef<AnnunciatorComponent>,
    ) -> JvmResult<()> {
        // Native private ctor @ 0x20f504 stores the synthetic outer reference.
        jvm.put_field(&mut this, "__wieOuter", "Lorg/kwis/msp/lwc/AnnunciatorComponent;", outer)
            .await?;

        Ok(())
    }

    async fn init_synthetic(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        outer: ClassInstanceRef<AnnunciatorComponent>,
        _access_marker: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native synthetic ctor @ 0x20f4d4 delegates to the private ctor.
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/AnnunciatorComponent$AnnunciatorEventListener",
                "<init>",
                "(Lorg/kwis/msp/lwc/AnnunciatorComponent;)V",
                (outer,),
            )
            .await?;

        Ok(())
    }

    async fn notify_event(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        event_type: i32,
        _param1: i32,
        _param2: i32,
    ) -> JvmResult<()> {
        // Native notifyEvent_v0 @ 0x20f52c ignores all events except 103.
        if event_type != 103 {
            return Ok(());
        }

        let activated_index: i32 = jvm.invoke_static("org/kwis/msp/lcdui/Display", "getActivatedIndex", "()I", ()).await?;

        let selected_display: ClassInstanceRef<Display> = if activated_index == 3 {
            let rotated = JavaLangString::from_rust_string(jvm, "rotated").await?;

            jvm.invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDisplay",
                "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Display;",
                (rotated,),
            )
            .await?
        } else {
            jvm.invoke_static("org/kwis/msp/lcdui/Display", "getDefaultDisplay", "()Lorg/kwis/msp/lcdui/Display;", ())
                .await?
        };

        let outer: ClassInstanceRef<AnnunciatorComponent> = jvm.get_field(&this, "__wieOuter", "Lorg/kwis/msp/lwc/AnnunciatorComponent;").await?;

        if outer.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let outer_display: ClassInstanceRef<Display> = jvm.get_field(&outer, "display", "Lorg/kwis/msp/lcdui/Display;").await?;

        // Native compares the two object pointers directly.
        if selected_display.is_null() || outer_display.is_null() || selected_display.identity() != outer_display.identity() {
            return Ok(());
        }

        let _: () = jvm.invoke_virtual(&outer, "repaint", "()V", ()).await?;

        let _: () = jvm.invoke_virtual(&outer, "serviceRepaints", "()V", ()).await?;

        Ok(())
    }
}
