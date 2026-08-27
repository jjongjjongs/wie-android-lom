use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
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

    fn register_socket(&self, handle: i32, socket: &Socket) -> Result<(), NetworkError> {
        let interest = Event::all(handle as usize).with_interrupt();

        let result = match socket {
            Socket::Tcp(TcpState::Connected(stream)) => unsafe {
                self.poller
                    .add_with_mode(stream, interest, PollMode::Edge)
            },
            Socket::Udp(socket) => unsafe {
                self.poller
                    .add_with_mode(socket, interest, PollMode::Edge)
            },
            Socket::Tcp(
                TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_),
            ) => return Ok(()),
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
            Socket::Tcp(
                TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_),
            ) => {}
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
                let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
                    .map_err(|error| Self::map_io(&error))?;
                socket
                    .set_nonblocking(true)
                    .map_err(|error| Self::map_io(&error))?;
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
                    return NetworkPoll::Ready(
                        udp.connect(remote).map_err(|error| Self::map_io(&error)),
                    );
                }
            }
        }

        let inner = self.inner.clone();
        let poller = self.poller.clone();
        let pending_events = self.pending_events.clone();

        thread::spawn(move || {
            let result = TcpStream::connect_timeout(&remote, CONNECT_TIMEOUT)
                .and_then(|stream| {
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

                    if unsafe {
                        poller.add_with_mode(&stream, interest, PollMode::Edge)
                    }
                    .is_ok()
                    {
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
            Socket::Tcp(TcpState::Connected(stream)) => {
                stream.read(buf).map_err(|error| Self::map_io(&error))
            }
            Socket::Udp(socket) => socket.recv(buf).map_err(|error| Self::map_io(&error)),
            Socket::Tcp(TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_)) => {
                Err(NetworkError::NotConnected)
            }
        }
    }

    fn write(&self, socket: i32, buf: &[u8]) -> Result<usize, NetworkError> {
        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());
        let Some(entry) = inner.sockets.get_mut(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        match entry {
            Socket::Tcp(TcpState::Connected(stream)) => {
                stream.write(buf).map_err(|error| Self::map_io(&error))
            }
            Socket::Udp(socket) => socket.send(buf).map_err(|error| Self::map_io(&error)),
            Socket::Tcp(TcpState::Disconnected | TcpState::Connecting | TcpState::Failed(_)) => {
                Err(NetworkError::NotConnected)
            }
        }
    }

    fn close(&self, socket: i32) -> Result<(), NetworkError> {
        let mut inner = self.inner.lock().unwrap_or_else(|x| x.into_inner());

        let Some(entry) = inner.sockets.get(&socket) else {
            return Err(NetworkError::InvalidSocket);
        };

        self.unregister_socket(entry);
        inner.sockets.remove(&socket);

        self.pending_events
            .lock()
            .unwrap_or_else(|x| x.into_inner())
            .retain(|event| match event {
                NetworkEvent::Connected(fd)
                | NetworkEvent::ConnectFailed(fd)
                | NetworkEvent::Readable(fd)
                | NetworkEvent::Writable(fd) => *fd != socket,
            });

        Ok(())
    }

    fn poll_event(&self) -> Option<NetworkEvent> {
        {
            let mut pending = self
                .pending_events
                .lock()
                .unwrap_or_else(|x| x.into_inner());

            if let Some(event) = pending.pop_front() {
                return Some(event);
            }
        }

        let mut events = Events::new();

        if self
            .poller
            .wait(&mut events, Some(Duration::ZERO))
            .ok()?
            == 0
        {
            return None;
        }

        let mut pending = self
            .pending_events
            .lock()
            .unwrap_or_else(|x| x.into_inner());

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
