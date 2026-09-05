use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::io::{InputStream, OutputStream};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

/// A connected WIPI socket, as `org.kwis.msf.io.URL.find` hands one back.
///
/// The reference declares this an interface and returns one of its `com.velox`
/// implementations; a title only ever sees it through this name, so one class
/// standing for both is the same thing from the title's side.
///
/// The socket the platform opened is kept as its descriptor. The streams read
/// and write through that descriptor, so a title that wraps them in
/// `DataInputStream`/`DataOutputStream` - which is what these titles do - talks
/// to the connection.
// class org.kwis.msf.io.Socket
pub struct Socket;

impl Socket {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msf/io/Socket",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("getInputStream", "()Ljava/io/InputStream;", Self::get_input_stream, Default::default()),
                JavaMethodProto::new("getOutputStream", "()Ljava/io/OutputStream;", Self::get_output_stream, Default::default()),
                JavaMethodProto::new("isStream", "()Z", Self::is_stream, Default::default()),
                JavaMethodProto::new("getMessageCount", "()I", Self::get_message_count, Default::default()),
                JavaMethodProto::new("getMessageMaxLength", "()I", Self::get_message_max_length, Default::default()),
                JavaMethodProto::new("close", "()V", Self::close, Default::default()),
                JavaMethodProto::new("getSocketDiscripter", "()I", Self::get_socket_discripter, Default::default()),
            ],
            fields: vec![JavaFieldProto::new("fd", "I", Default::default())],
            access_flags: Default::default(),
        }
    }

    async fn init(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Socket::<init>({this:?})");

        Ok(())
    }

    /// Binds a freshly connected descriptor to a new instance.
    pub async fn from_descriptor(jvm: &Jvm, fd: i32) -> JvmResult<ClassInstanceRef<Self>> {
        let mut this = jvm.new_class("org/kwis/msf/io/Socket", "()V", ()).await?;
        jvm.put_field(&mut this, "fd", "I", fd).await?;

        Ok(this.into())
    }

    async fn get_input_stream(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<InputStream>> {
        tracing::debug!("org.kwis.msf.io.Socket::getInputStream({this:?})");

        let fd: i32 = jvm.get_field(&this, "fd", "I").await?;
        let stream = jvm.new_class("org/kwis/msf/io/SocketInputStream", "(I)V", (fd,)).await?;

        Ok(stream.into())
    }

    async fn get_output_stream(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<OutputStream>> {
        tracing::debug!("org.kwis.msf.io.Socket::getOutputStream({this:?})");

        let fd: i32 = jvm.get_field(&this, "fd", "I").await?;
        let stream = jvm.new_class("org/kwis/msf/io/SocketOutputStream", "(I)V", (fd,)).await?;

        Ok(stream.into())
    }

    /// A stream connection, as opposed to the message (datagram) kind - which is
    /// what `URL.find` opens, and all these titles ask for.
    async fn is_stream(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msf.io.Socket::isStream({this:?})");

        Ok(true)
    }

    async fn get_message_count(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Socket::getMessageCount({this:?})");

        Ok(0)
    }

    async fn get_message_max_length(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Socket::getMessageMaxLength({this:?})");

        Ok(0)
    }

    async fn close(jvm: &Jvm, context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Socket::close({this:?})");

        let fd: i32 = jvm.get_field(&this, "fd", "I").await?;
        if fd < 0 {
            return Ok(());
        }

        if let Some(network) = context.system().platform().network() {
            let _ = network.close(fd);
        }
        jvm.put_field(&mut this, "fd", "I", -1).await?;

        Ok(())
    }

    async fn get_socket_discripter(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Socket::getSocketDiscripter({this:?})");

        jvm.get_field(&this, "fd", "I").await
    }
}
