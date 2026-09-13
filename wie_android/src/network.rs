use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use polling::{Event, Events, PollMode, Poller};
use wie_backend::{Network, NetworkError, NetworkEvent, NetworkPoll};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

enum TcpState {
    Disconnected,
    Connecting,
    Connected(TcpStream),
    Failed(NetworkError),
}

enum Socket {
    Tcp(TcpState),
    Udp(UdpSocket),
}

struct Inner {
    next_handle: i32,
    sockets: HashMap<i32, Socket>,
}

#[derive(Clone)]
pub struct AndroidNetwork {
    inner: Arc<Mutex<Inner>>,
    poller: Arc<Poller>,
    pending_events: Arc<Mutex<VecDeque<NetworkEvent>>>,
}

impl AndroidNetwork {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next_handle: 1,
                sockets: HashMap::new(),
            })),
            poller: Arc::new(Poller::new().expect("failed to create Android network poller")),
            pending_events: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn allocate(inner: &mut Inner, socket: Socket) -> i32 {
        let start = inner.next_handle.max(1);
        let mut handle = start;

        loop {
            if !inner.sockets.contains_key(&handle) {
                inner.sockets.insert(handle, socket);
                inner.next_handle = handle.checked_add(1).unwrap_or(1).max(1);
                return handle;
            }

            handle = handle.checked_add(1).unwrap_or(1).max(1);
            if handle == start {
                return -1;
            }
        }
    }

    fn ipv4(address: u32) -> Ipv4Addr {
        // The handset runtime copies the 32-bit WIPI address directly into
        // sockaddr_in.sin_addr. On little-endian ARM those in-memory bytes are
        // therefore the network-order IPv4 octets.
        Ipv4Addr::from(address.to_le_bytes())
    }

    /// Resolves `host` (a dotted-decimal IPv4 or a hostname) to the first IPv4
    /// address in the WIPI encoding, or `0xFFFF_FFFF` when it cannot be resolved
    /// - the same sentinel the reference's `MC_utilInetAddrInt` returns.
    fn resolve_ipv4(host: &str) -> u32 {
        match (host, 0u16).to_socket_addrs() {
            Ok(addrs) => addrs
                .filter_map(|addr| match addr {
                    SocketAddr::V4(v4) => Some(u32::from_le_bytes(v4.ip().octets())),
                    SocketAddr::V6(_) => None,
                })
                .next()
                .unwrap_or(0xFFFF_FFFF),
            Err(_) => 0xFFFF_FFFF,
        }
    }

    fn register_socket(&self, handle: i32, socket: &Socket) -> Result<(), NetworkError> {
        let interest = Event::all(handle as usize).with_interrupt();

        let result = match socket {
            Socket::Tcp(TcpState::Connected(stream)) => unsafe { self.poller.add_with_mode(stream, interest, PollMode::Edge) },
            Socket::Udp(socket) => unsafe { self.poller.add_with_mode(socket, interest, PollMode::Edge) },
            Socket::Tcp(TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_)) => return Ok(()),
        };

        result.map_err(|_| NetworkError::Other)
    }

    fn unregister_socket(&self, socket: &Socket) {
        match socket {
            Socket::Tcp(TcpState::Connected(stream)) => {
                let _ = self.poller.delete(stream);
            }
            Socket::Udp(socket) => {
                let _ = self.poller.delete(socket);
            }
            Socket::Tcp(TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_)) => {}
        }
    }

    fn map_io(error: &std::io::Error) -> NetworkError {
        use std::io::ErrorKind;

        match error.kind() {
            ErrorKind::WouldBlock => NetworkError::WouldBlock,
            ErrorKind::TimedOut => NetworkError::TimedOut,
            ErrorKind::ConnectionRefused => NetworkError::ConnectionRefused,
            ErrorKind::NotConnected => NetworkError::NotConnected,
            ErrorKind::AddrNotAvailable | ErrorKind::NetworkUnreachable => NetworkError::HostUnreachable,
            ErrorKind::Unsupported => NetworkError::Unsupported,
            _ => NetworkError::Other,
        }
    }
}

impl Default for AndroidNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl Network for AndroidNetwork {
    fn socket(&self, family: i32, socket_type: i32) -> Result<i32, NetworkError> {
        // Reference MC_netSocket accepts AF_INET(2), SOCK_STREAM(1) and
        // SOCK_DGRAM(2).
        if family != 2 {
            return Err(NetworkError::Unsupported);
        }

        let socket = match socket_type {
            1 => Socket::Tcp(TcpState::Disconnected),
            2 => {
                let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|error| Self::map_io(&error))?;
                socket.set_nonblocking(true).map_err(|error| Self::map_io(&error))?;
                Socket::Udp(socket)
            }
            _ => return Err(NetworkError::Unsupported),
        };

        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let handle = Self::allocate(&mut inner, socket);

        if handle < 0 {
            return Err(NetworkError::Other);
        }

        let Some(socket) = inner.sockets.get(&handle) else {
            return Err(NetworkError::Other);
        };

        if matches!(socket, Socket::Udp(_)) {
            self.register_socket(handle, socket)?;
        }

        Ok(handle)
    }

    fn bind(&self, socket: i32, address: u32, port: u16) -> Result<(), NetworkError> {
        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let Some(entry) = inner.sockets.get_mut(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        match entry {
            Socket::Udp(_) => {
                // A std UdpSocket cannot be rebound, so bind a fresh one to the
                // requested local address and swap it in, keeping the old socket
                // if the new bind fails.
                let local = SocketAddr::new(IpAddr::V4(Self::ipv4(address)), port);
                let bound = UdpSocket::bind(local).map_err(|error| Self::map_io(&error))?;
                bound.set_nonblocking(true).map_err(|error| Self::map_io(&error))?;
                self.unregister_socket(entry);
                *entry = Socket::Udp(bound);
                self.register_socket(socket, entry)
            }
            Socket::Tcp(_) => {
                // A std TcpStream picks its local address at connect time and
                // offers no pre-connect bind. The reference's bind on a stream
                // socket only records that local address, so accept it as a
                // no-op success rather than failing a game that binds first.
                Ok(())
            }
        }
    }

    fn send_to(&self, socket: i32, buf: &[u8], address: u32, port: u16) -> Result<usize, NetworkError> {
        let inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let Some(entry) = inner.sockets.get(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        match entry {
            Socket::Udp(udp) => {
                let remote = SocketAddr::new(IpAddr::V4(Self::ipv4(address)), port);
                udp.send_to(buf, remote).map_err(|error| Self::map_io(&error))
            }
            // The reference restricts sendto to datagram sockets (type == 2);
            // the wipi-c layer already gates on that, so a stream socket here is
            // out of contract.
            Socket::Tcp(_) => Err(NetworkError::Unsupported),
        }
    }

    fn resolve_host(&self, host: &str, query_id: u32) {
        // The reference resolves a hostname on a dedicated thread and delivers
        // the result through its event processor; do the same so the emulator
        // never blocks on DNS. A dotted-decimal address resolves immediately.
        let host = host.to_string();
        let pending = self.pending_events.clone();
        thread::spawn(move || {
            let address = Self::resolve_ipv4(&host);
            pending
                .lock()
                .unwrap_or_else(|x| x.into_inner())
                .push_back(NetworkEvent::HostResolved { query_id, address });
        });
    }

    fn resolve_host_blocking(&self, host: &str) -> u32 {
        Self::resolve_ipv4(host)
    }

    fn recv_from(&self, socket: i32, buf: &mut [u8]) -> Result<(usize, u32, u16), NetworkError> {
        let inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let Some(entry) = inner.sockets.get(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        match entry {
            Socket::Udp(udp) => {
                let (read, from) = udp.recv_from(buf).map_err(|error| Self::map_io(&error))?;
                // Re-encode the sender's IPv4 address into the WIPI 32-bit form
                // (the inverse of `ipv4`), so it matches what connect/send_to take.
                let address = match from {
                    SocketAddr::V4(v4) => u32::from_le_bytes(v4.ip().octets()),
                    SocketAddr::V6(_) => 0,
                };
                Ok((read, address, from.port()))
            }
            Socket::Tcp(_) => Err(NetworkError::Unsupported),
        }
    }

    fn connect(&self, socket: i32, address: u32, port: u16) -> NetworkPoll<()> {
        let remote = SocketAddr::new(IpAddr::V4(Self::ipv4(address)), port);

        {
            let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
            let Some(entry) = inner.sockets.get_mut(&socket) else {
                return NetworkPoll::Ready(Err(NetworkError::InvalidSocket));
            };

            match entry {
                Socket::Tcp(TcpState::Disconnected | TcpState::Failed(_)) => {
                    // Native clears its asynchronous connect-pending state
                    // when the failure event is delivered. The next connect
                    // call therefore starts a fresh lower connect attempt
                    // instead of replaying the previous failure.
                    *entry = Socket::Tcp(TcpState::Connecting);
                }
                Socket::Tcp(TcpState::Connecting) => return NetworkPoll::Pending,
                Socket::Tcp(TcpState::Connected(_)) => return NetworkPoll::Ready(Ok(())),
                Socket::Udp(udp) => {
                    return NetworkPoll::Ready(udp.connect(remote).map_err(|error| Self::map_io(&error)));
                }
            }
        }

        let inner = self.inner.clone();
        let poller = self.poller.clone();
        let pending_events = self.pending_events.clone();

        thread::spawn(move || {
            let result = TcpStream::connect_timeout(&remote, CONNECT_TIMEOUT).and_then(|stream| {
                stream.set_nonblocking(true)?;
                Ok(stream)
            });

            let mut inner = inner.lock().unwrap_or_else(|x| x.into_inner());
            let Some(entry) = inner.sockets.get_mut(&socket) else {
                return;
            };

            if !matches!(entry, Socket::Tcp(TcpState::Connecting)) {
                return;
            }

            *entry = match result {
                Ok(stream) => {
                    let interest = Event::all(socket as usize).with_interrupt();

                    if unsafe { poller.add_with_mode(&stream, interest, PollMode::Edge) }.is_ok() {
                        pending_events
                            .lock()
                            .unwrap_or_else(|x| x.into_inner())
                            .push_back(NetworkEvent::Connected(socket));
                        Socket::Tcp(TcpState::Connected(stream))
                    } else {
                        pending_events
                            .lock()
                            .unwrap_or_else(|x| x.into_inner())
                            .push_back(NetworkEvent::ConnectFailed(socket));
                        Socket::Tcp(TcpState::Failed(NetworkError::Other))
                    }
                }
                Err(error) => {
                    pending_events
                        .lock()
                        .unwrap_or_else(|x| x.into_inner())
                        .push_back(NetworkEvent::ConnectFailed(socket));
                    Socket::Tcp(TcpState::Failed(Self::map_io(&error)))
                }
            };
        });

        NetworkPoll::Pending
    }

    fn read(&self, socket: i32, buf: &mut [u8]) -> Result<usize, NetworkError> {
        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let Some(entry) = inner.sockets.get_mut(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        match entry {
            Socket::Tcp(TcpState::Connected(stream)) => stream.read(buf).map_err(|error| Self::map_io(&error)),
            Socket::Udp(socket) => socket.recv(buf).map_err(|error| Self::map_io(&error)),
            Socket::Tcp(TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_)) => Err(NetworkError::NotConnected),
        }
    }

    fn write(&self, socket: i32, buf: &[u8]) -> Result<usize, NetworkError> {
        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let Some(entry) = inner.sockets.get_mut(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        match entry {
            Socket::Tcp(TcpState::Connected(stream)) => stream.write(buf).map_err(|error| Self::map_io(&error)),
            Socket::Udp(socket) => socket.send(buf).map_err(|error| Self::map_io(&error)),
            Socket::Tcp(TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_)) => Err(NetworkError::NotConnected),
        }
    }

    fn close(&self, socket: i32) -> Result<(), NetworkError> {
        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());

        let Some(entry) = inner.sockets.get(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        self.unregister_socket(entry);
        inner.sockets.remove(&socket);

        self.pending_events.lock().unwrap_or_else(|x| x.into_inner()).retain(|event| match event {
            NetworkEvent::Connected(fd) | NetworkEvent::ConnectFailed(fd) | NetworkEvent::Readable(fd) | NetworkEvent::Writable(fd) => *fd != socket,
            // A host resolution is not tied to a socket, so closing one
            // never drops it.
            NetworkEvent::HostResolved { .. } => true,
        });

        Ok(())
    }

    fn poll_event(&self) -> Option<NetworkEvent> {
        {
            let mut pending = self.pending_events.lock().unwrap_or_else(|x| x.into_inner());

            if let Some(event) = pending.pop_front() {
                return Some(event);
            }
        }

        let mut events = Events::new();

        if self.poller.wait(&mut events, Some(Duration::ZERO)).ok()? == 0 {
            return None;
        }

        let mut pending = self.pending_events.lock().unwrap_or_else(|x| x.into_inner());

        for event in events.iter() {
            let Ok(socket) = i32::try_from(event.key) else {
                continue;
            };

            if event.readable {
                pending.push_back(NetworkEvent::Readable(socket));
            }

            if event.writable {
                pending.push_back(NetworkEvent::Writable(socket));
            }
        }

        pending.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wie_backend::Network;

    // MC_netSocketBind (0x25f): the datagram path rebinds a fresh UdpSocket to
    // the requested local address, the stream path is an accepted no-op, and an
    // unknown socket is rejected as the native's find_socket_obj null -> -2.
    #[test]
    fn bind_udp_rebinds_tcp_noop_unknown_rejected() {
        let net = AndroidNetwork::new();
        // 127.0.0.1 in the WIPI address encoding connect() also uses.
        let loopback = u32::from_le_bytes([127, 0, 0, 1]);

        let udp = net.socket(2, 2).expect("udp socket");
        assert!(net.bind(udp, loopback, 0).is_ok());

        let tcp = net.socket(2, 1).expect("tcp socket");
        assert!(net.bind(tcp, loopback, 0).is_ok());

        assert!(matches!(net.bind(0x7fff_ffff, loopback, 0), Err(NetworkError::InvalidSocket)));
    }

    // MC_netSocketSendTo (0x261): a datagram socket delivers to the addressed
    // receiver and returns the sent length; an unknown socket is rejected.
    #[test]
    fn send_to_delivers_datagram() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("receiver");
        receiver.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let port = receiver.local_addr().unwrap().port();
        let loopback = u32::from_le_bytes([127, 0, 0, 1]);

        let net = AndroidNetwork::new();
        let udp = net.socket(2, 2).expect("udp socket");
        let payload = b"wipi-datagram";
        assert_eq!(net.send_to(udp, payload, loopback, port).unwrap(), payload.len());

        let mut buf = [0u8; 64];
        let (n, _) = receiver.recv_from(&mut buf).expect("recv");
        assert_eq!(&buf[..n], payload);

        assert!(matches!(
            net.send_to(0x7fff_ffff, payload, loopback, port),
            Err(NetworkError::InvalidSocket)
        ));
    }

    // MC_netSocketRcvFrom (0x262): a datagram socket receives a packet and
    // reports the sender's address and port in the WIPI encoding.
    #[test]
    fn recv_from_round_trip_reports_sender() {
        let loopback = u32::from_le_bytes([127, 0, 0, 1]);
        let peer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("peer");
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let peer_port = peer.local_addr().unwrap().port();

        let net = AndroidNetwork::new();
        let udp = net.socket(2, 2).expect("udp socket");

        // The backend socket sends first so the peer learns its source address;
        // the peer then replies and the backend receives, naming the peer.
        net.send_to(udp, b"ping", loopback, peer_port).unwrap();
        let mut probe = [0u8; 64];
        let (_, backend_src) = peer.recv_from(&mut probe).expect("peer recv");
        peer.send_to(b"pong-reply", backend_src).expect("peer send");

        let mut buf = [0u8; 64];
        let mut got = None;
        for _ in 0..200 {
            match net.recv_from(udp, &mut buf) {
                Ok(x) => {
                    got = Some(x);
                    break;
                }
                Err(NetworkError::WouldBlock) => thread::sleep(Duration::from_millis(5)),
                Err(other) => panic!("recv_from: {other:?}"),
            }
        }
        let (read, address, port) = got.expect("recv_from delivered");
        assert_eq!(&buf[..read], b"pong-reply");
        assert_eq!(address, loopback);
        assert_eq!(port, peer_port);

        assert!(matches!(net.recv_from(0x7fff_ffff, &mut buf), Err(NetworkError::InvalidSocket)));
    }

    fn wait_for_resolved(net: &AndroidNetwork, query_id: u32) -> u32 {
        for _ in 0..400 {
            if let Some(NetworkEvent::HostResolved { query_id: id, address }) = net.poll_event() {
                if id == query_id {
                    return address;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("host resolution for query {query_id} never delivered");
    }

    // MC_netGetHostAddr (0x263): a dotted-decimal host resolves to the WIPI
    // address encoding, and an unresolvable host yields the 0xFFFFFFFF sentinel.
    #[test]
    fn resolve_host_numeric_and_failure() {
        let net = AndroidNetwork::new();

        net.resolve_host("127.0.0.1", 7);
        assert_eq!(wait_for_resolved(&net, 7), u32::from_le_bytes([127, 0, 0, 1]));

        net.resolve_host("no-such-host.invalid.", 8);
        assert_eq!(wait_for_resolved(&net, 8), 0xFFFF_FFFF);
    }
}
