use alloc::vec;

use java_class_proto::JavaMethodProto;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class com.xce.io.ByteToCharEUC_KR
//
// The named converter, following `sun.io`'s naming the way SK-VM does. It is
// the one titles instantiate; `convert` is inherited from the base class,
// which decodes EUC-KR because that is the only encoding the platform has.
//
// The Rust name mirrors the Java one, underscore and all.
#[allow(non_camel_case_types)]
pub struct ByteToCharEUC_KR;

impl ByteToCharEUC_KR {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "com/xce/io/ByteToCharEUC_KR",
            parent_class: Some("com/xce/io/ByteToCharConverter"),
            interfaces: vec![],
            methods: vec![JavaMethodProto::new("<init>", "()V", Self::init, Default::default())],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("com.xce.io.ByteToCharEUC_KR::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "com/xce/io/ByteToCharConverter", "<init>", "()V", ()).await?;

        Ok(())
    }
}
