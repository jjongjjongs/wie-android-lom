use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msf.io.Network
pub struct Network;

impl Network {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msf/io/Network",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("connect", "()I", Self::connect, MethodAccessFlags::NATIVE | MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "disconnect",
                    "()V",
                    Self::disconnect,
                    MethodAccessFlags::NATIVE | MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Network::<init>({this:?})");

        Ok(())
    }

    async fn connect(_: &Jvm, _: &mut WieJvmContext) -> JvmResult<i32> {
        Ok(1)
    }

    async fn disconnect(_: &Jvm, _: &mut WieJvmContext) -> JvmResult<()> {
        Ok(())
    }
}
