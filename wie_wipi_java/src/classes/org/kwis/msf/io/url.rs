use alloc::{format, string::String as RustString, vec};

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msf::io::Socket;

/// How long a connection is given to settle before it is called a failure.
///
/// The platform's own connect is asynchronous and reports failure through an
/// event; polling it here cannot tell "still trying" from "failed and about to
/// be retried", so the attempt is bounded instead. Long enough for a server
/// that answers, short enough that a title waiting on one that does not gets
/// its answer while the player is still looking at the screen.
const CONNECT_TIMEOUT_MS: u64 = 5000;

/// Message a failed connection is reported with, matching what the reference
/// player shows a title that asks it to open one it cannot.
const CONNECT_FAILED: &str = "연결에 실패하였습니다.";

/// `org.kwis.msf.io.URL` - the WIPI connection factory.
///
/// `find` is what a title calls to open a URL: it takes `<scheme>://<host>:<port>`
/// and hands back a connected [`Socket`]. The schemes these titles use are all
/// stream connections over TCP - `socket://` for a game's own server and
/// `BillSocket://` for the carrier's billing and authentication gateway, which
/// differ on the handset in the billing header the platform adds and not in how
/// they connect.
///
/// A title that cannot reach its server is expected to be told so: the ez-i
/// authentication these titles run at start-up puts up its "connection failed"
/// notice and carries on into the game. Without this class it was never told
/// anything, and sat on its "authenticating" screen forever.
// class org.kwis.msf.io.URL
pub struct URL;

impl URL {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msf/io/URL",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "find",
                    "(Ljava/lang/String;)Lorg/kwis/msf/io/Socket;",
                    Self::find,
                    MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.URL::<init>({this:?})");

        Ok(())
    }

    async fn find(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        url: ClassInstanceRef<java_runtime::classes::java::lang::String>,
    ) -> JvmResult<ClassInstanceRef<Socket>> {
        let url = JavaLangString::to_rust_string(jvm, &url).await?;
        tracing::info!("org.kwis.msf.io.URL::find({url:?})");

        let Some((host, port)) = parse_authority(&url) else {
            return Err(jvm.exception("org/kwis/msf/io/SchemeNotFoundException", &url).await);
        };

        match Self::connect(context, &host, port).await {
            Ok(fd) => {
                tracing::info!("org.kwis.msf.io.URL::find({url:?}) connected as {fd}");
                Socket::from_descriptor(jvm, fd).await
            }
            Err(reason) => {
                tracing::info!("org.kwis.msf.io.URL::find({url:?}) failed: {reason}");
                Err(jvm.exception("java/io/IOException", CONNECT_FAILED).await)
            }
        }
    }

    /// Opens a stream socket to `host:port`, or reports why it could not.
    async fn connect(context: &mut WieJvmContext, host: &str, port: u16) -> Result<i32, RustString> {
        use wie_backend::NetworkPoll;

        // WIPI's own numbering: family 2 is AF_INET, type 1 is a stream.
        const AF_INET: i32 = 2;
        const SOCK_STREAM: i32 = 1;

        let address = {
            let system = context.system();
            let network = system.platform().network().ok_or("no network backend")?;
            network.resolve_host_blocking(host)
        };
        if address == 0xFFFF_FFFF {
            return Err(format!("cannot resolve {host}"));
        }

        let socket = {
            let system = context.system();
            let network = system.platform().network().ok_or("no network backend")?;
            network.socket(AF_INET, SOCK_STREAM).map_err(|error| format!("{error:?}"))?
        };

        let deadline = context.system().platform().now().raw() + CONNECT_TIMEOUT_MS;
        loop {
            let poll = {
                let system = context.system();
                let network = system.platform().network().ok_or("no network backend")?;
                network.connect(socket, address, port)
            };

            match poll {
                NetworkPoll::Ready(Ok(())) => return Ok(socket),
                NetworkPoll::Ready(Err(error)) => {
                    Self::close(context, socket);
                    return Err(format!("{error:?}"));
                }
                NetworkPoll::Pending => {
                    if context.system().platform().now().raw() >= deadline {
                        Self::close(context, socket);
                        return Err(format!("no answer from {host}:{port}"));
                    }
                    context.system().sleep(10).await;
                }
            }
        }
    }

    fn close(context: &mut WieJvmContext, socket: i32) {
        if let Some(network) = context.system().platform().network() {
            let _ = network.close(socket);
        }
    }
}

/// Splits `<scheme>://<host>:<port>` into its host and port.
///
/// `None` for a URL with no authority or no port - the schemes these titles
/// open always name both, and a scheme this cannot read is one the platform
/// would report as unknown rather than guess at.
fn parse_authority(url: &str) -> Option<(RustString, u16)> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = authority.rsplit_once(':')?;

    if host.is_empty() {
        return None;
    }

    Some((host.into(), port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::parse_authority;

    #[test]
    fn reads_the_host_and_port_a_title_opens() {
        // The two schemes these titles use, and a path after the authority.
        assert_eq!(parse_authority("BillSocket://218.50.3.88:2508"), Some(("218.50.3.88".into(), 2508)));
        assert_eq!(parse_authority("socket://218.38.12.48:5100"), Some(("218.38.12.48".into(), 5100)));
        assert_eq!(parse_authority("socket://host.example:80/path"), Some(("host.example".into(), 80)));

        // Nothing to connect to.
        assert_eq!(parse_authority("socket://218.38.12.48"), None);
        assert_eq!(parse_authority("socket://:2508"), None);
        assert_eq!(parse_authority("218.38.12.48:5100"), None);
        assert_eq!(parse_authority("socket://host:notaport"), None);
    }
}
