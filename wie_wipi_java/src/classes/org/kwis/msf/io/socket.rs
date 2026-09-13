use alloc::{boxed::Box, vec, vec::Vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::io::{InputStream, OutputStream};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_backend::{LocalConnection, LocalRead};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msf::io::Message;

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

/// The billing gateway, as a local-network connection.
///
/// The answer is armed when the connection opens and again whenever the SDK
/// sends a request, so it is there whichever order the SDK reads and writes in
/// - the reference SDK writes its 28 byte request and then reads, but a stream
/// opened and read without one still finds the answer waiting, which is what
/// the stand-in stream this replaces always did.
struct BillingGateway {
    /// What is left of the answer to hand back.
    pending: Vec<u8>,
}

impl BillingGateway {
    fn armed() -> Self {
        Self {
            pending: BILLING_RESPONSE.to_vec(),
        }
    }
}

impl LocalConnection for BillingGateway {
    fn write(&mut self, bytes: &[u8]) {
        // Titles reach this through `BillSocket://` the same way a WIPI-C one
        // reaches `MC_netBillSocket`, and they speak the same protocols - Legend
        // of Master writes a 55-byte record here. So try those answers first and
        // keep the ez-i SDK's own for what is left, which is what this endpoint
        // was written for.
        let response = wie_backend::billing::response(bytes);

        tracing::debug!(
            "billing gateway: {} -> {}",
            wie_backend::billing::bill_frame_trace(bytes),
            match &response {
                Some(response) => wie_backend::billing::bill_frame_trace(response),
                None => alloc::format!("the ez-i SDK's own answer, {} bytes", BILLING_RESPONSE.len()),
            }
        );

        if response.is_none() {
            // Nobody knows this request, so what the title does with the ez-i
            // stand-in it gets instead is the only account of the protocol
            // there is. Record it.
            wie_backend::probe::over_an_unanswered_request(&wie_backend::billing::bill_frame_trace(bytes));
        }

        self.pending = response.unwrap_or_else(|| BILLING_RESPONSE.to_vec());
    }

    fn read(&mut self, out: &mut [u8]) -> LocalRead {
        if self.pending.is_empty() {
            return LocalRead::Pending;
        }

        let taken = out.len().min(self.pending.len());
        out[..taken].copy_from_slice(&self.pending[..taken]);
        self.pending.drain(..taken);

        // The title now has all of an answer that was queued whole, so what it
        // does next is the parse. Anything waiting to be traced over that parse
        // starts here.
        if self.pending.is_empty() {
            wie_backend::probe::drained();
        }

        LocalRead::Data(taken)
    }

    fn readable(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod billing_gateway_tests {
    use alloc::{vec, vec::Vec};

    use spin::Mutex;

    use wie_backend::{LocalConnection, LocalRead};

    use super::BillingGateway;

    /// These drive one gateway each, but the answers they check are the
    /// title's whole protocol; holding this keeps a failure readable as one
    /// test's rather than two interleaved.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// The eighteen bytes 서든어택 포켓 writes when a cash purchase is
    /// confirmed.
    const SUDDEN_ATTACK_PURCHASE: [u8; 18] = [
        0x29, 0x10, 0x00, 0x00, 0x00, 0x0b, 0x30, 0x31, 0x30, 0x34, 0x36, 0x31, 0x31, 0x39, 0x32, 0x36, 0x39, 0x00,
    ];

    /// The medal report that follows it.
    const SUDDEN_ATTACK_MEDALS: [u8; 18] = [
        0x3c, 0x10, 0x00, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00, 0x28,
    ];

    /// Reads a gateway out the way one of these titles does: a length first,
    /// then the body it describes.
    fn read_framed(gateway: &mut BillingGateway) -> Vec<u8> {
        let mut length = [0u8; 4];
        for byte in length.iter_mut() {
            let mut one = [0u8; 1];
            assert!(matches!(gateway.read(&mut one), LocalRead::Data(1)));
            *byte = one[0];
        }

        let mut body = vec![0u8; u32::from_be_bytes(length) as usize];
        assert!(matches!(gateway.read(&mut body), LocalRead::Data(_)));

        body
    }

    #[test]
    fn both_of_a_purchases_exchanges_are_answered_in_full() {
        let _guard = ONE_AT_A_TIME.lock();

        // The shop stops on whichever of the two goes unanswered, so both have
        // to come back whole.
        for frame in [SUDDEN_ATTACK_PURCHASE, SUDDEN_ATTACK_MEDALS] {
            let mut gateway = BillingGateway::armed();
            gateway.write(&frame);

            let body = read_framed(&mut gateway);

            // Everything queued was handed over, so the title is not left
            // waiting on a length that never arrives.
            assert!(!gateway.readable());
            assert!(!body.is_empty());
        }
    }

    #[test]
    fn a_purchase_is_answered_the_way_it_reads_as_granted() {
        let _guard = ONE_AT_A_TIME.lock();

        let mut gateway = BillingGateway::armed();
        gateway.write(&SUDDEN_ATTACK_PURCHASE);

        // Zero is what its parser has to leave behind for the shop to draw
        // 구매 성공 rather than 구매 실패하였습니다.
        assert!(read_framed(&mut gateway).iter().all(|&byte| byte == 0));
    }
}

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
    ///
    /// It is a local-network connection like any other, so the ordinary socket
    /// streams carry it and the gateway's answer is queued rather than handed
    /// out by a stream that stands in for one.
    pub async fn local_billing(jvm: &Jvm, context: &mut WieJvmContext) -> JvmResult<ClassInstanceRef<Self>> {
        let descriptor = {
            let system = context.system();
            let mut local_network = system.local_network();

            local_network.open("billing gateway", Box::new(BillingGateway::armed()))
        };

        let Some(descriptor) = descriptor else {
            return Err(jvm.exception("java/io/IOException", "the local network is full").await);
        };

        Self::from_descriptor(jvm, descriptor).await
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

        if wie_backend::is_local_descriptor(fd) {
            context.system().local_network().close(fd);
            jvm.put_field(&mut this, "fd", "I", -1).await?;

            return Ok(());
        }

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
