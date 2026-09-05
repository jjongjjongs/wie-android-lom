use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::{
    lcdui::{Display, Graphics},
    lwc::TextComponent,
};

// class org.kwis.msp.lwc.TextComponent$ModeViewer
pub struct TextComponentModeViewer;

impl TextComponentModeViewer {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/TextComponent$ModeViewer",
            parent_class: Some("org/kwis/msp/lcdui/Card"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lwc/TextComponent;Lorg/kwis/msp/lcdui/Display;)V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new("notifyChangeMode", "()V", Self::notify_change_mode, Default::default()),
                JavaMethodProto::new("paint", "(Lorg/kwis/msp/lcdui/Graphics;)V", Self::paint, Default::default()),
                JavaMethodProto::new("paintMode", "(Lorg/kwis/msp/lcdui/Graphics;I)V", Self::paint_mode, Default::default()),
            ],
            fields: vec![
                // Native ModeViewer +0x34 stores the enclosing TextComponent.
                JavaFieldProto::new("__wieOwner", "Lorg/kwis/msp/lwc/TextComponent;", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        owner: ClassInstanceRef<TextComponent>,
        display: ClassInstanceRef<Display>,
    ) -> JvmResult<()> {
        // Native ModeViewer:
        // super(display, 0, 0, 13, 7);
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lcdui/Card",
                "<init>",
                "(Lorg/kwis/msp/lcdui/Display;IIII)V",
                (display, 0, 0, 13, 7),
            )
            .await?;

        jvm.put_field(&mut this, "__wieOwner", "Lorg/kwis/msp/lwc/TextComponent;", owner).await?;

        Ok(())
    }

    async fn notify_change_mode(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        let owner: ClassInstanceRef<TextComponent> = jvm.get_field(&this, "__wieOwner", "Lorg/kwis/msp/lwc/TextComponent;").await?;

        if owner.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let mode_viewer: ClassInstanceRef<TextComponentModeViewer> = jvm
            .get_field(&owner, "__wieModeViewer", "Lorg/kwis/msp/lwc/TextComponent$ModeViewer;")
            .await?;

        if mode_viewer.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let width: i32 = jvm.get_field(&mode_viewer, "w", "I").await?;
        let height: i32 = jvm.get_field(&mode_viewer, "h", "I").await?;

        let _: () = jvm.invoke_virtual(&mode_viewer, "repaint", "(IIII)V", (0, 0, width, height)).await?;

        Ok(())
    }

    async fn paint(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, graphics: ClassInstanceRef<Graphics>) -> JvmResult<()> {
        let owner: ClassInstanceRef<TextComponent> = jvm.get_field(&this, "__wieOwner", "Lorg/kwis/msp/lwc/TextComponent;").await?;

        if owner.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "").await);
        }

        let mode: i32 = jvm.get_field(&owner, "iMode", "I").await?;

        let paint_mode = match mode {
            0 => Some(1),
            1 => Some(0),
            2 => Some(2),
            3 => Some(3),
            99 => Some(4),
            _ => None,
        };

        if let Some(mode) = paint_mode {
            Self::paint_mode(jvm, context, this, graphics, mode).await?;
        }

        Ok(())
    }

    async fn paint_mode(_: &Jvm, _: &mut WieJvmContext, _: ClassInstanceRef<Self>, _: ClassInstanceRef<Graphics>, _: i32) -> JvmResult<()> {
        // Native ModeViewer.paintMode rendering is restored separately.
        Ok(())
    }
}
