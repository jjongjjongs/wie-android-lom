use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::lang::String;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use crate::classes::org::kwis::msp::lcdui::{Font, Image};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.LabelComponent
pub struct LabelComponent;

impl LabelComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/LabelComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "<init>",
                    "(Ljava/lang/String;)V",
                    Self::init_label,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Ljava/lang/String;Lorg/kwis/msp/lcdui/Image;)V",
                    Self::init_label_image,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    Self::get_font,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setFont",
                    "(Lorg/kwis/msp/lcdui/Font;)V",
                    Self::set_font,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getImage",
                    "()Lorg/kwis/msp/lcdui/Image;",
                    Self::get_image,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setImage",
                    "(Lorg/kwis/msp/lcdui/Image;)V",
                    Self::set_image,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getLabel",
                    "()Ljava/lang/String;",
                    Self::get_label,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setLabel",
                    "(Ljava/lang/String;)V",
                    Self::set_label,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setLayout",
                    "(I)V",
                    Self::set_layout,
                    Default::default(),
                ),
            ],
            fields: vec![
                JavaFieldProto::new("layout", "I", Default::default()),
                JavaFieldProto::new(
                    "font",
                    "Lorg/kwis/msp/lcdui/Font;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "label",
                    "Ljava/lang/String;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "image",
                    "Lorg/kwis/msp/lcdui/Image;",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "<init>",
                "()V",
                (),
            )
            .await?;

        jvm.put_field(&mut this, "layout", "I", 9).await?;

        let font: ClassInstanceRef<Font> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Font",
                "getDefaultFont",
                "()Lorg/kwis/msp/lcdui/Font;",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "font",
            "Lorg/kwis/msp/lcdui/Font;",
            font,
        )
        .await?;

        Ok(())
    }

    async fn init_label(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        label: ClassInstanceRef<String>,
    ) -> JvmResult<()> {
        Self::init(jvm, context, this.clone()).await?;

        jvm.put_field(
            &mut this,
            "label",
            "Ljava/lang/String;",
            label,
        )
        .await?;

        Ok(())
    }

    async fn init_label_image(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        label: ClassInstanceRef<String>,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        Self::init(jvm, context, this.clone()).await?;

        jvm.put_field(
            &mut this,
            "label",
            "Ljava/lang/String;",
            label,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "image",
            "Lorg/kwis/msp/lcdui/Image;",
            image,
        )
        .await?;

        Ok(())
    }

    async fn get_font(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Font>> {
        jvm.get_field(
            &this,
            "font",
            "Lorg/kwis/msp/lcdui/Font;",
        )
        .await
    }

    async fn set_font(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        mut font: ClassInstanceRef<Font>,
    ) -> JvmResult<()> {
        if font.is_null() {
            font = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Font",
                    "getDefaultFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    (),
                )
                .await?;
        }

        let current: ClassInstanceRef<Font> = jvm
            .get_field(
                &this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
            )
            .await?;

        if current.identity() != font.identity() {
            jvm.put_field(
                &mut this,
                "font",
                "Lorg/kwis/msp/lcdui/Font;",
                font,
            )
            .await?;

            let _: () = jvm
                .invoke_virtual(&this, "invalidate", "()V", ())
                .await?;
        }

        Ok(())
    }

    async fn get_image(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        jvm.get_field(
            &this,
            "image",
            "Lorg/kwis/msp/lcdui/Image;",
        )
        .await
    }

    async fn set_image(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "image",
            "Lorg/kwis/msp/lcdui/Image;",
            image,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;
        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }

    async fn get_label(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<ClassInstanceRef<String>> {
        jvm.get_field(
            &this,
            "label",
            "Ljava/lang/String;",
        )
        .await
    }

    async fn set_label(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        label: ClassInstanceRef<String>,
    ) -> JvmResult<()> {
        jvm.put_field(
            &mut this,
            "label",
            "Ljava/lang/String;",
            label,
        )
        .await?;

        let _: () = jvm
            .invoke_virtual(&this, "invalidate", "()V", ())
            .await?;
        let _: () = jvm
            .invoke_virtual(&this, "repaint", "()V", ())
            .await?;

        Ok(())
    }
    async fn set_layout(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        layout: i32,
    ) -> JvmResult<()> {
        // WipiPlayer Plus LabelComponent.setLayout(int):
        // reject conflicting horizontal/vertical layout bit combinations.
        if (layout & 3) == 3
            || layout > 36
            || (layout & 5) == 5
            || (layout & 6) == 6
            || (layout & 48) == 48
            || (layout & 40) == 40
            || (layout & 24) == 24
        {
            return Err(
                jvm.exception("java/lang/IllegalArgumentException", "")
                    .await,
            );
        }

        jvm.put_field(&mut this, "layout", "I", layout).await?;

        let width: i32 = jvm.get_field(&this, "w", "I").await?;
        let height: i32 = jvm.get_field(&this, "h", "I").await?;

        let _: () = jvm
            .invoke_virtual(
                &this,
                "repaint",
                "(IIII)V",
                (0, 0, width, height),
            )
            .await?;

        Ok(())
    }

}
