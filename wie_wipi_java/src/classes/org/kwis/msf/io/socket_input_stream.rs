use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

/// The read side of an `org.kwis.msf.io.Socket`, over the descriptor the
/// platform opened.
///
/// A read that would block waits rather than reporting end of stream: a title
/// reading a reply through `DataInputStream` expects the bytes, and a zero
/// return would look to it like the peer hung up.
// class org.kwis.msf.io.SocketInputStream
pub struct SocketInputStream;

impl SocketInputStream {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msf/io/SocketInputStream",
            parent_class: Some("java/io/InputStream"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "(I)V", Self::init, Default::default()),
                JavaMethodProto::new("read", "()I", Self::read_byte, Default::default()),
                JavaMethodProto::new("read", "([BII)I", Self::read_array, Default::default()),
                JavaMethodProto::new("available", "()I", Self::available, Default::default()),
                JavaMethodProto::new("close", "()V", Self::close, Default::default()),
            ],
            fields: vec![JavaFieldProto::new("fd", "I", Default::default())],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, fd: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketInputStream::<init>({this:?}, {fd})");

        let _: () = jvm.invoke_special(&this, "java/io/InputStream", "<init>", "()V", ()).await?;
        jvm.put_field(&mut this, "fd", "I", fd).await?;

        Ok(())
    }

    async fn read_byte(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.SocketInputStream::read({this:?})");

        let mut byte = [0u8; 1];
        match Self::recv(jvm, context, &this, &mut byte).await? {
            0 => Ok(-1),
            _ => Ok(byte[0] as i32),
        }
    }

    async fn read_array(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        mut buf: ClassInstanceRef<Array<i8>>,
        offset: i32,
        length: i32,
    ) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.SocketInputStream::read({this:?}, {buf:?}, {offset}, {length})");

        if length <= 0 {
            return Ok(0);
        }

        let mut bytes = alloc::vec![0u8; length as usize];
        let read = Self::recv(jvm, context, &this, &mut bytes).await?;
        if read == 0 {
            return Ok(-1);
        }

        let signed: alloc::vec::Vec<i8> = bytes[..read].iter().map(|&byte| byte as i8).collect();
        jvm.store_array(&mut buf, offset as _, signed).await?;

        Ok(read as i32)
    }

    /// Reads into `buf`, waiting out a would-block. Zero means the peer closed.
    async fn recv(jvm: &Jvm, context: &mut WieJvmContext, this: &ClassInstanceRef<Self>, buf: &mut [u8]) -> JvmResult<usize> {
        use wie_backend::NetworkError;

        let fd: i32 = jvm.get_field(this, "fd", "I").await?;
        if fd < 0 {
            return Err(jvm.exception("java/io/IOException", "Stream closed").await);
        }

        loop {
            let result = {
                let system = context.system();
                let Some(network) = system.platform().network() else {
                    return Err(jvm.exception("java/io/IOException", "No network").await);
                };
                network.read(fd, buf)
            };

            match result {
                Ok(read) => return Ok(read),
                Err(NetworkError::WouldBlock) => context.system().sleep(1).await,
                Err(error) => return Err(jvm.exception("java/io/IOException", &alloc::format!("{error:?}")).await),
            }
        }
    }

    async fn available(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.SocketInputStream::available({this:?})");

        Ok(0)
    }

    async fn close(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.SocketInputStream::close({this:?})");

        // The descriptor belongs to the socket, which closes it; dropping the
        // stream's copy only stops this stream from using it.
        jvm.put_field(&mut this, "fd", "I", -1).await
    }
}
