use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{ClassAccessFlags, FieldAccessFlags, MethodAccessFlags};
use jvm::{Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// interface org.kwis.msp.lcdui.ImageObserver
pub struct ImageObserver;

impl ImageObserver {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/ImageObserver",
            parent_class: None,
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<clinit>", "()V", Self::cl_init, MethodAccessFlags::STATIC),
                JavaMethodProto::new_abstract("notify", "(Lorg/kwis/msp/lcdui/Image;I)V", MethodAccessFlags::ABSTRACT),
            ],
            fields: vec![
                JavaFieldProto::new("FRAME_END", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("IMAGE_END", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("NOT_EXIST", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("DECODE_ERROR", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("OUT_OF_MEMORY", "I", FieldAccessFlags::STATIC),
            ],
            access_flags: ClassAccessFlags::INTERFACE,
        }
    }

    async fn cl_init(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<()> {
        jvm.put_static_field("org/kwis/msp/lcdui/ImageObserver", "FRAME_END", "I", 0i32).await?;
        jvm.put_static_field("org/kwis/msp/lcdui/ImageObserver", "IMAGE_END", "I", 1i32).await?;
        jvm.put_static_field("org/kwis/msp/lcdui/ImageObserver", "NOT_EXIST", "I", -1i32).await?;
        jvm.put_static_field("org/kwis/msp/lcdui/ImageObserver", "DECODE_ERROR", "I", -2i32)
            .await?;
        jvm.put_static_field("org/kwis/msp/lcdui/ImageObserver", "OUT_OF_MEMORY", "I", -3i32)
            .await?;

        Ok(())
    }
}
