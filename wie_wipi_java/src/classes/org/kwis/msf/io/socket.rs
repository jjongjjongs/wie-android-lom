use alloc::{vec, vec::Vec};

use bytemuck::cast_vec;
use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::io::{InputStream, OutputStream};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msf::io::Message;

/// Descriptor standing for the carrier's billing gateway, answered in process.
///
/// Negative, so it can never collide with one the network backend hands out,
/// and distinct from the `-1` a closed socket carries.
const BILLING_DESCRIPTOR: i32 = -2;

/// The gateway's answer to the ez-i SDK's first request.
///
/// The SDK - the one 판타지나이트 and 배틀몬스터 both carry - opens
/// `BillSocket://` and writes a 28 byte request: a `u16` payload length, a
/// `u16` request type, the sixteen byte subscriber number from `PHONENUMBER`,
/// a `u32` service code and a `u32` checksum, all little endian. It then reads
/// twenty bytes back and takes two `u16`s and two `u32`s off the front; the
/// first `u32` is the result, and the SDK's own table of them decides what to
/// do next:
///
/// | result | meaning                                    |
/// |--------|--------------------------------------------|
/// | 1, 2   | game cash granted (500 / 300 원)            |
/// | 3, 12  | SMS opt-in offered (200 원)                 |
/// | 11, 13, 999 | nothing to grant - authenticated, go on |
/// | other  | the SDK gives up and never leaves its screen |
///
/// The gateway has not answered for years and there is nothing here to charge
/// a subscriber for, so answer the one the SDK reads as "you are authenticated
/// and there is nothing to collect": 999. Everything past the two `u32`s is
/// left zero, the checksum included - the SDK reads the result and value and
/// never looks at the rest.
const BILLING_RESPONSE: [u8; 20] = [
    12, 0, // u16 payload length
    1, 0, // u16 request type, echoed
    0xe7, 0x03, 0x00, 0x00, // u32 result: 999
    0, 0, 0, 0, // u32 value
    0, 0, 0, 0, // rest of the payload
    0, 0, 0, 0, // checksum
];

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
                JavaMethodProto::new("send", "(Lorg/kwis/msf/io/Message;)V", Self::send, Default::default()),
                JavaMethodProto::new("recv", "(Lorg/kwis/msf/io/Message;)V", Self::recv, Default::default()),
                JavaMethodProto::new("close", "()V", Self::close, Default::default()),
                JavaMethodProto::new("accept", "()Lorg/kwis/msf/io/Socket;", Self::accept, Default::default()),
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

    /// Binds a new instance to the in-process billing gateway instead of a
    /// connection.
    pub async fn local_billing(jvm: &Jvm) -> JvmResult<ClassInstanceRef<Self>> {
        Self::from_descriptor(jvm, BILLING_DESCRIPTOR).await
    }

    async fn get_input_stream(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<InputStream>> {
        tracing::debug!("org.kwis.msf.io.Socket::getInputStream({this:?})");

        let fd: i32 = jvm.get_field(&this, "fd", "I").await?;
        if fd == BILLING_DESCRIPTOR {
            let mut response = jvm.instantiate_array("B", BILLING_RESPONSE.len()).await?;
            jvm.store_array(&mut response, 0, cast_vec::<u8, i8>(BILLING_RESPONSE.to_vec())).await?;

            let stream = jvm.new_class("java/io/ByteArrayInputStream", "([B)V", (response,)).await?;

            return Ok(stream.into());
        }

        let stream = jvm.new_class("org/kwis/msf/io/SocketInputStream", "(I)V", (fd,)).await?;

        Ok(stream.into())
    }

    async fn get_output_stream(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<OutputStream>> {
        tracing::debug!("org.kwis.msf.io.Socket::getOutputStream({this:?})");

        let fd: i32 = jvm.get_field(&this, "fd", "I").await?;
        if fd == BILLING_DESCRIPTOR {
            // The request is read only to answer it, and the answer does not
            // depend on it, so it goes nowhere.
            let stream = jvm.new_class("java/io/ByteArrayOutputStream", "()V", ()).await?;

            return Ok(stream.into());
        }

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

    /// The message (datagram) half of the interface. `URL.find` opens stream
    /// connections - which is what every archive here asks for - so a title that
    /// reaches these is using a socket kind the platform does not model, and is
    /// told so rather than left to read an empty message back.
    async fn send(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, message: ClassInstanceRef<Message>) -> JvmResult<()> {
        tracing::warn!("org.kwis.msf.io.Socket::send({this:?}, {message:?}) on a stream socket");

        Err(jvm.exception("java/io/IOException", "not a message socket").await)
    }

    async fn recv(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, message: ClassInstanceRef<Message>) -> JvmResult<()> {
        tracing::warn!("org.kwis.msf.io.Socket::recv({this:?}, {message:?}) on a stream socket");

        Err(jvm.exception("java/io/IOException", "not a message socket").await)
    }

    /// Accepting an inbound connection needs a listening socket, which nothing
    /// here opens; a title that asks gets nothing rather than a wrong answer.
    async fn accept(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Self>> {
        tracing::warn!("org.kwis.msf.io.Socket::accept({this:?}) on a socket that is not listening");

        Err(jvm.exception("java/io/IOException", "not a listening socket").await)
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
