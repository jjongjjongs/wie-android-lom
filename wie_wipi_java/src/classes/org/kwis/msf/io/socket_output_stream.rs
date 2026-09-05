use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

/// The write side of an `org.kwis.msf.io.Socket`, over the descriptor the
/// platform opened.
// class org.kwis.msf.io.SocketOutputStream
pub struct SocketOutputStream;

impl SocketOutputStream {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msf/io/SocketOutputStream",
            parent_class: Some("java/io/OutputStream"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "(I)V", Self::init, Default::default()),
                JavaMethodProto::new("write", "(I)V", Self::write_byte, Default::default()),
                JavaMethodProto::new("write", "([BII)V", Self::write_array, Default::default()),
                JavaMethodProto::new("flush", "()V", Self::flush, Default::default()),
                JavaMethodProto::new("close", "()V", Self::close, Default::default()),
            ],
            fields: vec![JavaFieldProto::new("fd", "I", Default::default())],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, fd: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketOutputStream::<init>({this:?}, {fd})");

        let _: () = jvm.invoke_special(&this, "java/io/OutputStream", "<init>", "()V", ()).await?;
        jvm.put_field(&mut this, "fd", "I", fd).await?;

        Ok(())
    }

    async fn write_byte(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, byte: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketOutputStream::write({this:?}, {byte})");

        Self::send(jvm, context, &this, &[byte as u8]).await
    }

    async fn write_array(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        buf: ClassInstanceRef<Array<i8>>,
        offset: i32,
        length: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketOutputStream::write({this:?}, {buf:?}, {offset}, {length})");

        if length <= 0 {
            return Ok(());
        }

        let signed: alloc::vec::Vec<i8> = jvm.load_array(&buf, offset as _, length as _).await?;
        let bytes: alloc::vec::Vec<u8> = signed.into_iter().map(|byte| byte as u8).collect();

        Self::send(jvm, context, &this, &bytes).await
    }

    /// Writes all of `bytes`, waiting out a would-block.
    async fn send(jvm: &Jvm, context: &mut WieJvmContext, this: &ClassInstanceRef<Self>, bytes: &[u8]) -> JvmResult<()> {
        use wie_backend::NetworkError;

        let fd: i32 = jvm.get_field(this, "fd", "I").await?;
        if fd < 0 {
            return Err(jvm.exception("java/io/IOException", "Stream closed").await);
        }

        let mut written = 0;
        while written < bytes.len() {
            let result = {
                let system = context.system();
                let Some(network) = system.platform().network() else {
                    return Err(jvm.exception("java/io/IOException", "No network").await);
                };
                network.write(fd, &bytes[written..])
            };

            match result {
                Ok(0) => return Err(jvm.exception("java/io/IOException", "Connection closed").await),
                Ok(count) => written += count,
                Err(NetworkError::WouldBlock) => context.system().sleep(1).await,
                Err(error) => return Err(jvm.exception("java/io/IOException", &alloc::format!("{error:?}")).await),
            }
        }

        Ok(())
    }

    async fn flush(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketOutputStream::flush({this:?})");

        Ok(())
    }

    async fn close(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketOutputStream::close({this:?})");

        jvm.put_field(&mut this, "fd", "I", -1).await
    }
}
