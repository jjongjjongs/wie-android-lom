//! Servers that live inside the emulator.
//!
//! These titles were written against services that have been switched off for
//! years: a game's own server, the carrier's authentication gateway, a ranking
//! board. Dialing out reaches nothing, and a title told its connection failed
//! either shows a notice and carries on or - often enough - stops on a screen
//! it never leaves.
//!
//! A local endpoint answers such a connection in process instead. It is not a
//! proxy and opens no socket: the title's writes are handed to it and its
//! answers are handed back, so what a title sees is a peer that behaves the way
//! the original did, to whatever depth the endpoint implements.
//!
//! `org.kwis.msf.io.Socket`'s in-process billing gateway is the same idea
//! written out by hand for one protocol, and predates this.
//!
//! # Descriptors
//!
//! A local connection is named by a descriptor from a reserved negative range,
//! so it can travel through the same `int fd` a title already carries and can
//! never be mistaken for one the platform handed out (those are zero or above),
//! for the `-1` a closed socket carries, or for the billing gateway's `-2`.

mod ack;
mod capture;

pub use self::{
    ack::{AckEndpoint, Framing},
    capture::{CaptureAddress, CaptureEndpoint},
};

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

/// What a read from a local connection found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalRead {
    /// This many bytes were written into the caller's buffer.
    Data(usize),
    /// The endpoint has nothing to say yet. The title's read waits, exactly as
    /// it would on a connection whose peer has not answered.
    Pending,
    /// The endpoint hung up. The title's read reports end of stream.
    Closed,
}

/// One connection to a [`LocalEndpoint`].
///
/// The title's side is a stream in each direction, so an endpoint that answers
/// a request takes it through `write` and leaves the answer for `read`. Nothing
/// is threaded: both run on the caller's task, so an endpoint must not block.
///
/// `Sync` because the registry holding it is shared with the system it hangs
/// off; the lock around it is what actually serialises the two calls.
pub trait LocalConnection: Send + Sync {
    /// Takes bytes the title wrote to its server.
    fn write(&mut self, bytes: &[u8]);

    /// Fills `out` with what the endpoint has to say.
    fn read(&mut self, out: &mut [u8]) -> LocalRead;

    /// Whether a read right now would hand back bytes rather than wait.
    ///
    /// A title that registers a read callback stops polling and waits to be
    /// told, so an endpoint holding an answer has to say so or that title never
    /// reads it. The default is `false`, which costs such a title only its
    /// callback - a polling title reads either way - so an endpoint that
    /// buffers an answer should override it.
    fn readable(&self) -> bool {
        false
    }

    /// The title hung up.
    fn close(&mut self) {}
}

/// A server the emulator answers for.
pub trait LocalEndpoint: Send + Sync {
    /// Names this endpoint in the trace.
    fn name(&self) -> &str;

    /// Whether this endpoint answers for the address a title just opened.
    ///
    /// `scheme` is the URL scheme the title used, lowercased - `socket` for the
    /// connections a WIPI-C title opens, which carry no scheme of their own.
    fn accepts(&self, scheme: &str, host: &str, port: u16) -> bool;

    /// Opens one connection. Called once per `connect`, so an endpoint that
    /// keeps per-connection state puts it in the returned value.
    fn open(&self, scheme: &str, host: &str, port: u16) -> Box<dyn LocalConnection>;
}

/// The lowest descriptor a local connection can be given. They run downwards
/// from here, leaving every smaller-magnitude negative value - `-1` for closed,
/// `-2` for the billing gateway - to their existing meanings.
const LOCAL_DESCRIPTOR_BASE: i32 = -0x1000;

/// Whether `descriptor` names a local connection rather than a platform socket.
pub fn is_local_descriptor(descriptor: i32) -> bool {
    descriptor <= LOCAL_DESCRIPTOR_BASE
}

/// The endpoints this run answers for, and the connections open to them.
#[derive(Default)]
pub struct LocalNetwork {
    endpoints: Vec<Box<dyn LocalEndpoint>>,
    connections: BTreeMap<i32, Connection>,
    next_descriptor: i32,
}

struct Connection {
    endpoint: String,
    inner: Box<dyn LocalConnection>,
}

impl LocalNetwork {
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            connections: BTreeMap::new(),
            next_descriptor: LOCAL_DESCRIPTOR_BASE,
        }
    }

    /// Adds an endpoint. Endpoints are asked in the order they were added, so
    /// one registered earlier answers an address a later one would also take.
    pub fn register(&mut self, endpoint: Box<dyn LocalEndpoint>) {
        tracing::debug!("Local network endpoint registered: {}", endpoint.name());
        self.endpoints.push(endpoint);
    }

    /// Whether any endpoint is registered, so a caller can skip the lookup
    /// entirely on the ordinary run where none is.
    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty()
    }

    /// Opens a connection to whichever endpoint answers for this address, or
    /// `None` when none does and the caller should dial out as usual.
    pub fn connect(&mut self, scheme: &str, host: &str, port: u16) -> Option<i32> {
        let scheme = ascii_lowercase(scheme);
        let index = self.endpoints.iter().position(|endpoint| endpoint.accepts(&scheme, host, port))?;

        let inner = self.endpoints[index].open(&scheme, host, port);
        let endpoint = String::from(self.endpoints[index].name());

        let descriptor = self.next_descriptor;
        self.next_descriptor = self.next_descriptor.checked_sub(1)?;
        self.connections.insert(descriptor, Connection { endpoint, inner });

        tracing::info!(
            "{scheme}://{host}:{port} answered in process by {} as {descriptor}",
            self.endpoints[index].name()
        );

        Some(descriptor)
    }

    /// Takes a connection the caller has already decided on, rather than one an
    /// endpoint matched. A protocol answered by name rather than by address -
    /// the carrier's billing gateway is opened by its URL scheme - belongs here
    /// and still travels as an ordinary local descriptor.
    pub fn open(&mut self, endpoint: &str, connection: Box<dyn LocalConnection>) -> Option<i32> {
        let descriptor = self.next_descriptor;
        self.next_descriptor = self.next_descriptor.checked_sub(1)?;

        self.connections.insert(
            descriptor,
            Connection {
                endpoint: String::from(endpoint),
                inner: connection,
            },
        );

        tracing::info!("{endpoint} answered in process as {descriptor}");

        Some(descriptor)
    }

    /// Hands `bytes` to the endpoint `descriptor` is connected to. `None` when
    /// the descriptor names no open local connection.
    pub fn write(&mut self, descriptor: i32, bytes: &[u8]) -> Option<usize> {
        let connection = self.connections.get_mut(&descriptor)?;

        tracing::trace!("Local network write to {} ({descriptor}): {} bytes", connection.endpoint, bytes.len());
        connection.inner.write(bytes);

        Some(bytes.len())
    }

    /// Reads from the endpoint `descriptor` is connected to. `None` when the
    /// descriptor names no open local connection.
    pub fn read(&mut self, descriptor: i32, out: &mut [u8]) -> Option<LocalRead> {
        let connection = self.connections.get_mut(&descriptor)?;

        Some(connection.inner.read(out))
    }

    /// Whether the connection on `descriptor` has bytes waiting. `false` for a
    /// descriptor this does not know, which is also what a title gets for one
    /// that has been closed.
    pub fn readable(&self, descriptor: i32) -> bool {
        self.connections.get(&descriptor).is_some_and(|connection| connection.inner.readable())
    }

    /// Closes a local connection. `false` when the descriptor named none.
    pub fn close(&mut self, descriptor: i32) -> bool {
        match self.connections.remove(&descriptor) {
            Some(mut connection) => {
                tracing::debug!("Local network connection {descriptor} to {} closed", connection.endpoint);
                connection.inner.close();
                true
            }
            None => false,
        }
    }
}

/// `str::to_ascii_lowercase` without pulling the whole `String` conversion into
/// callers that pass an already-lowercase scheme.
fn ascii_lowercase(text: &str) -> String {
    text.chars().map(|character| character.to_ascii_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, string::String, vec::Vec};

    use super::{LocalConnection, LocalEndpoint, LocalNetwork, LocalRead, is_local_descriptor};

    /// Answers `socket://echo:1234` by sending back whatever it was given.
    struct Echo;

    #[derive(Default)]
    struct EchoConnection {
        pending: Vec<u8>,
    }

    impl LocalEndpoint for Echo {
        fn name(&self) -> &str {
            "echo"
        }

        fn accepts(&self, scheme: &str, host: &str, port: u16) -> bool {
            scheme == "socket" && host == "echo" && port == 1234
        }

        fn open(&self, _: &str, _: &str, _: u16) -> Box<dyn LocalConnection> {
            Box::new(EchoConnection::default())
        }
    }

    impl LocalConnection for EchoConnection {
        fn write(&mut self, bytes: &[u8]) {
            self.pending.extend_from_slice(bytes);
        }

        fn read(&mut self, out: &mut [u8]) -> LocalRead {
            if self.pending.is_empty() {
                return LocalRead::Pending;
            }

            let taken = out.len().min(self.pending.len());
            out[..taken].copy_from_slice(&self.pending[..taken]);
            self.pending.drain(..taken);

            LocalRead::Data(taken)
        }

        fn readable(&self) -> bool {
            !self.pending.is_empty()
        }
    }

    /// An endpoint that answers but never says it has, the way one written
    /// before `readable` existed behaves.
    struct Mute;

    impl LocalConnection for Mute {
        fn write(&mut self, _: &[u8]) {}

        fn read(&mut self, _: &mut [u8]) -> LocalRead {
            LocalRead::Pending
        }
    }

    #[test]
    fn a_connection_says_when_it_has_an_answer_waiting() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let descriptor = network.connect("socket", "echo", 1234).unwrap();

        // Nothing asked, nothing to read.
        assert!(!network.readable(descriptor));

        network.write(descriptor, b"hello");
        assert!(network.readable(descriptor));

        // Still readable while part of the answer is left, and not once it is
        // all taken.
        let mut out = [0u8; 2];
        assert_eq!(network.read(descriptor, &mut out), Some(LocalRead::Data(2)));
        assert!(network.readable(descriptor));

        let mut rest = [0u8; 8];
        assert_eq!(network.read(descriptor, &mut rest), Some(LocalRead::Data(3)));
        assert!(!network.readable(descriptor));
    }

    #[test]
    fn a_descriptor_that_names_no_connection_is_not_readable() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let descriptor = network.connect("socket", "echo", 1234).unwrap();
        network.write(descriptor, b"hello");

        assert!(!network.readable(descriptor - 1));

        // And a closed one has nothing to say either.
        assert!(network.close(descriptor));
        assert!(!network.readable(descriptor));
    }

    #[test]
    fn an_endpoint_that_does_not_answer_the_question_is_taken_as_silent() {
        let mut network = LocalNetwork::new();
        let descriptor = network.open("mute", Box::new(Mute)).unwrap();

        assert!(!network.readable(descriptor));
    }

    #[test]
    fn an_empty_network_answers_nothing() {
        let mut network = LocalNetwork::new();

        assert!(network.is_empty());
        assert_eq!(network.connect("socket", "echo", 1234), None);
    }

    #[test]
    fn only_the_address_an_endpoint_accepts_is_answered() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        assert!(!network.is_empty());
        assert!(network.connect("socket", "echo", 1234).is_some());

        // A different host, port or scheme is left to the real network.
        assert_eq!(network.connect("socket", "echo", 1235), None);
        assert_eq!(network.connect("socket", "elsewhere", 1234), None);
        assert_eq!(network.connect("billsocket", "echo", 1234), None);
    }

    #[test]
    fn the_scheme_is_matched_without_case() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        assert!(network.connect("SOCKET", "echo", 1234).is_some());
    }

    #[test]
    fn a_local_descriptor_is_never_one_a_socket_carries() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let descriptor = network.connect("socket", "echo", 1234).unwrap();

        assert!(is_local_descriptor(descriptor));

        // The values already spoken for elsewhere: a platform socket, a closed
        // socket, and `Socket`'s in-process billing gateway.
        assert!(!is_local_descriptor(0));
        assert!(!is_local_descriptor(7));
        assert!(!is_local_descriptor(-1));
        assert!(!is_local_descriptor(-2));
    }

    #[test]
    fn each_connection_gets_its_own_descriptor_and_state() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let first = network.connect("socket", "echo", 1234).unwrap();
        let second = network.connect("socket", "echo", 1234).unwrap();
        assert_ne!(first, second);

        network.write(first, b"only mine");

        let mut buffer = [0u8; 16];
        assert_eq!(network.read(second, &mut buffer), Some(LocalRead::Pending));
        assert_eq!(network.read(first, &mut buffer), Some(LocalRead::Data(9)));
        assert_eq!(&buffer[..9], b"only mine");
    }

    #[test]
    fn a_read_longer_than_the_answer_takes_what_there_is() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let descriptor = network.connect("socket", "echo", 1234).unwrap();
        network.write(descriptor, b"abc");

        let mut buffer = [0u8; 2];
        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Data(2)));
        assert_eq!(&buffer, b"ab");
        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Data(1)));
        assert_eq!(buffer[0], b'c');
        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Pending));
    }

    #[test]
    fn a_connection_can_be_opened_without_an_endpoint_matching_it() {
        let mut network = LocalNetwork::new();

        // No endpoint is registered, so nothing would match by address.
        let descriptor = network.open("billing", Box::new(EchoConnection::default())).unwrap();
        assert!(is_local_descriptor(descriptor));

        network.write(descriptor, b"request");

        let mut buffer = [0u8; 16];
        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Data(7)));
        assert!(network.close(descriptor));
    }

    #[test]
    fn a_directly_opened_connection_shares_the_descriptor_range() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let matched = network.connect("socket", "echo", 1234).unwrap();
        let direct = network.open("billing", Box::new(EchoConnection::default())).unwrap();

        assert_ne!(matched, direct);
    }

    #[test]
    fn a_closed_connection_is_no_longer_addressable() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        let descriptor = network.connect("socket", "echo", 1234).unwrap();
        assert!(network.close(descriptor));

        assert!(!network.close(descriptor));
        assert_eq!(network.write(descriptor, b"gone"), None);
        assert_eq!(network.read(descriptor, &mut [0u8; 4]), None);
    }

    #[test]
    fn a_descriptor_no_endpoint_handed_out_is_not_answered() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Echo));

        // A platform socket's descriptor never reaches an endpoint.
        assert_eq!(network.write(3, b"not mine"), None);
        assert_eq!(network.read(3, &mut [0u8; 4]), None);
    }

    /// Answers, then hangs up.
    struct Once;

    impl LocalEndpoint for Once {
        fn name(&self) -> &str {
            "once"
        }

        fn accepts(&self, _: &str, host: &str, _: u16) -> bool {
            host == "once"
        }

        fn open(&self, _: &str, _: &str, _: u16) -> Box<dyn LocalConnection> {
            Box::new(OnceConnection { answered: false })
        }
    }

    struct OnceConnection {
        answered: bool,
    }

    impl LocalConnection for OnceConnection {
        fn write(&mut self, _: &[u8]) {}

        fn read(&mut self, out: &mut [u8]) -> LocalRead {
            if self.answered {
                return LocalRead::Closed;
            }

            self.answered = true;
            out[0] = b'!';

            LocalRead::Data(1)
        }
    }

    #[test]
    fn an_endpoint_can_report_end_of_stream() {
        let mut network = LocalNetwork::new();
        network.register(Box::new(Once));

        let descriptor = network.connect("socket", "once", 1).unwrap();
        let mut buffer = [0u8; 4];

        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Data(1)));
        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Closed));
    }

    #[test]
    fn the_first_endpoint_that_accepts_answers() {
        struct Everything(&'static str);

        impl LocalEndpoint for Everything {
            fn name(&self) -> &str {
                self.0
            }

            fn accepts(&self, _: &str, _: &str, _: u16) -> bool {
                true
            }

            fn open(&self, _: &str, _: &str, _: u16) -> Box<dyn LocalConnection> {
                Box::new(EchoConnection::default())
            }
        }

        let mut network = LocalNetwork::new();
        network.register(Box::new(Everything("first")));
        network.register(Box::new(Everything("second")));

        // Registration order decides, so an endpoint added for one address is
        // not shadowed by a catch-all added later.
        let descriptor = network.connect("socket", "anywhere", 1).unwrap();
        network.write(descriptor, b"x");

        let mut buffer = [0u8; 1];
        assert_eq!(network.read(descriptor, &mut buffer), Some(LocalRead::Data(1)));

        let _ = String::new();
    }
}
