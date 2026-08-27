use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use spin::Mutex;
use wipi_types::wipic::WIPICWord;

use wie_util::{Result, WieError, read_null_terminated_string_bytes};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};


#[derive(Clone, Copy, Default)]
struct SocketCallbacks {
    socket_type: i32,
    connect_callback: WIPICWord,
    connect_context: WIPICWord,
    connect_pending: bool,
    read_callback: WIPICWord,
    read_context: WIPICWord,
    write_callback: WIPICWord,
    write_context: WIPICWord,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ProcessNetworkState {
    #[default]
    Closed,
    Connecting,
    Available,
}

/// A native `MC_netHttpOpen` connection object.
///
/// The LGT firmware keeps this as a 208-byte record threaded onto a global
/// module-memory list; WIE keeps the same observable fields keyed by an opaque
/// handle. `socket` is the stream socket `MC_netHttpOpen` creates eagerly (via
/// `MC_netSocket(2, 1)`), `connected` mirrors the native `[obj+0xc]` "committed"
/// flag that flips once `MC_netHttpConnect` runs, and `request_headers` is the
/// same accumulation `MC_netHttpSetRequestProperty` builds - `"key: value"` for
/// the first property and `"\r\nkey: value"` appended for each one after.
#[derive(Clone, Default)]
struct HttpObject {
    socket: i32,
    // host/port/path are parsed at open time and consumed by the request line
    // MC_netHttpConnect emits in the network unit; they have no reader yet.
    #[allow(dead_code)]
    host: String,
    /// Host byte order; the native stores the network-order `sin_port` but the
    /// value only feeds the request line, so WIE keeps it in host order.
    #[allow(dead_code)]
    port: u16,
    #[allow(dead_code)]
    path: String,
    /// Canonical request method: `"GET"`, `"POST"` or `"HEAD"`. `MC_netHttpOpen`
    /// defaults it to `"GET"`.
    method: String,
    post_body: Vec<u8>,
    request_headers: String,
    proxy_addr: u32,
    proxy_port: u16,
    connected: bool,
    /// Response fields, populated by `MC_netHttpConnect` in the network unit and
    /// only ever read once `connected` is set, so their defaults are never
    /// observed by the response getters before then.
    response_code: i32,
    response_message: String,
    content_type: String,
    content_encoding: String,
    content_length: i32,
}

#[derive(Default)]
pub struct NetworkState {
    process_state: ProcessNetworkState,
    process_callback: WIPICWord,
    process_context: WIPICWord,
    process_generation: u64,
    sockets: BTreeMap<i32, SocketCallbacks>,
    dispatcher_started: bool,
    dispatcher_generation: u64,
    /// In-flight `MC_netGetHostAddr` resolutions, keyed by query id, holding the
    /// game's `(callback, context)` until the `HostResolved` event delivers.
    dns_queries: BTreeMap<u32, (WIPICWord, WIPICWord)>,
    next_query_id: u32,
    /// Live `MC_netHttpOpen` connection objects, keyed by the opaque handle the
    /// API hands back to the game.
    http_objects: BTreeMap<i32, HttpObject>,
    next_http_handle: i32,
}

pub type SharedNetworkState = Arc<Mutex<NetworkState>>;

impl NetworkState {
    fn begin_connect(
        &mut self,
        callback: WIPICWord,
        context: WIPICWord,
    ) -> core::result::Result<u64, i32> {
        match self.process_state {
            ProcessNetworkState::Available => return Err(-10),
            ProcessNetworkState::Connecting => return Err(-7),
            ProcessNetworkState::Closed => {}
        }

        self.process_generation = self.process_generation.wrapping_add(1);
        self.process_state = ProcessNetworkState::Connecting;
        self.process_callback = callback;
        self.process_context = context;
        Ok(self.process_generation)
    }

    fn finish_connect(
        &mut self,
        generation: u64,
    ) -> Option<(WIPICWord, WIPICWord)> {
        if self.process_state != ProcessNetworkState::Connecting
            || self.process_generation != generation
        {
            return None;
        }

        // Native WPNet_NetEventProcess checks the process callback before
        // changing network-object state 2 (connecting) to state 1
        // (available). A null callback therefore leaves the process in
        // connecting state, so a later MC_netConnect returns -7.
        if self.process_callback == 0 {
            return None;
        }

        self.process_state = ProcessNetworkState::Available;
        Some((self.process_callback, self.process_context))
    }

    fn is_available(&self) -> bool {
        self.process_state == ProcessNetworkState::Available
    }

    fn has_process_network(&self) -> bool {
        self.process_state != ProcessNetworkState::Closed
    }

    fn close_process(&mut self) -> Vec<i32> {
        self.process_generation = self.process_generation.wrapping_add(1);
        self.process_state = ProcessNetworkState::Closed;
        self.process_callback = 0;
        self.process_context = 0;

        self.dispatcher_generation = self.dispatcher_generation.wrapping_add(1);
        self.dispatcher_started = false;

        // Native WPNet_Exit walks and frees every network object, which drops
        // the HTTP connection list along with the sockets. An HTTP object's
        // socket already lives in `sockets`, so returning the socket keys is
        // enough to close them; the objects themselves are dropped here.
        self.http_objects.clear();

        let sockets = self.sockets.keys().copied().collect();
        self.sockets.clear();
        sockets
    }

    fn http_alloc(&mut self, object: HttpObject) -> i32 {
        // The native handle is the object's heap pointer; WIE hands out an
        // opaque positive id from a distinct range so it can never be mistaken
        // for a small socket fd. It stays below `i32::MAX` so the WIPI-C
        // convention of "negative result is an error" holds.
        if self.next_http_handle == 0 {
            self.next_http_handle = HTTP_HANDLE_BASE;
        }
        let handle = self.next_http_handle;
        self.next_http_handle = self.next_http_handle.wrapping_add(1);
        self.http_objects.insert(handle, object);
        handle
    }

    fn http_get(&self, handle: i32) -> Option<&HttpObject> {
        self.http_objects.get(&handle)
    }

    fn http_get_mut(&mut self, handle: i32) -> Option<&mut HttpObject> {
        self.http_objects.get_mut(&handle)
    }

    fn http_remove(&mut self, handle: i32) -> Option<HttpObject> {
        self.http_objects.remove(&handle)
    }

    fn register_socket(&mut self, socket: i32, socket_type: i32) {
        let entry = self.sockets.entry(socket).or_default();
        entry.socket_type = socket_type;
    }

    fn socket_type(&self, socket: i32) -> Option<i32> {
        self.sockets.get(&socket).map(|entry| entry.socket_type)
    }

    fn remove_socket(&mut self, socket: i32) {
        self.sockets.remove(&socket);
    }

    fn set_connect_callback(
        &mut self,
        socket: i32,
        callback: WIPICWord,
        context: WIPICWord,
    ) {
        if let Some(entry) = self.sockets.get_mut(&socket) {
            entry.connect_callback = callback;
            entry.connect_context = context;
        }
    }

    fn set_read_callback(&mut self, socket: i32, callback: WIPICWord, context: WIPICWord) {
        if let Some(entry) = self.sockets.get_mut(&socket) {
            entry.read_callback = callback;
            entry.read_context = context;
        }
    }

    fn set_write_callback(&mut self, socket: i32, callback: WIPICWord, context: WIPICWord) {
        if let Some(entry) = self.sockets.get_mut(&socket) {
            entry.write_callback = callback;
            entry.write_context = context;
        }
    }

    fn connect_is_pending(&self, socket: i32) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|entry| entry.connect_pending)
    }

    fn set_connect_pending(&mut self, socket: i32, pending: bool) {
        if let Some(entry) = self.sockets.get_mut(&socket) {
            entry.connect_pending = pending;
        }
    }

    fn clear_connect_callback(&mut self, socket: i32) {
        if let Some(entry) = self.sockets.get_mut(&socket) {
            entry.connect_callback = 0;
            entry.connect_context = 0;
        }
    }

    fn take_callback_for_event(
        &mut self,
        event: wie_backend::NetworkEvent,
    ) -> Option<(WIPICWord, [WIPICWord; 3])> {
        let (socket, callback, result, callback_context) = match event {
            wie_backend::NetworkEvent::Connected(socket) => {
                let entry = self.sockets.get_mut(&socket)?;
                let callback = entry.connect_callback;
                let callback_context = entry.connect_context;
                entry.connect_pending = false;
                entry.connect_callback = 0;
                entry.connect_context = 0;
                (socket, callback, 0, callback_context)
            }
            wie_backend::NetworkEvent::ConnectFailed(socket) => {
                let entry = self.sockets.get_mut(&socket)?;
                let callback = entry.connect_callback;
                let callback_context = entry.connect_context;
                entry.connect_pending = false;
                entry.connect_callback = 0;
                entry.connect_context = 0;
                (socket, callback, u32::MAX, callback_context)
            }
            wie_backend::NetworkEvent::Readable(socket) => {
                let entry = self.sockets.get(&socket)?;
                (socket, entry.read_callback, 0, entry.read_context)
            }
            wie_backend::NetworkEvent::Writable(socket) => {
                let entry = self.sockets.get(&socket)?;
                (socket, entry.write_callback, 0, entry.write_context)
            }
            wie_backend::NetworkEvent::HostResolved { query_id, address } => {
                let (callback, context) = self.dns_queries.remove(&query_id)?;
                // The DNS callback takes (address, context); reuse the first
                // array slot for the address (a bit-preserving i32 round trip)
                // and the second for the context. The third slot is unused.
                (address as i32, callback, context, 0)
            }
        };

        (callback != 0).then_some((
            callback,
            [socket as WIPICWord, result, callback_context],
        ))
    }

    fn begin_dns_query(&mut self, callback: WIPICWord, context: WIPICWord) -> u32 {
        let query_id = self.next_query_id;
        self.next_query_id = self.next_query_id.wrapping_add(1);
        self.dns_queries.insert(query_id, (callback, context));
        query_id
    }

    fn take_dns_query(&mut self, query_id: u32) -> Option<(WIPICWord, WIPICWord)> {
        self.dns_queries.remove(&query_id)
    }

    fn has_callbacks(&self) -> bool {
        !self.dns_queries.is_empty()
            || self.sockets.values().any(|entry| {
                entry.connect_callback != 0
                    || entry.read_callback != 0
                    || entry.write_callback != 0
            })
    }

    fn start_dispatcher(&mut self) -> Option<u64> {
        if self.dispatcher_started {
            return None;
        }

        self.dispatcher_generation = self.dispatcher_generation.wrapping_add(1);
        self.dispatcher_started = true;
        Some(self.dispatcher_generation)
    }

    fn dispatcher_is_current(&self, generation: u64) -> bool {
        self.dispatcher_started && self.dispatcher_generation == generation
    }

    fn stop_dispatcher(&mut self, generation: u64) {
        if self.dispatcher_generation == generation {
            self.dispatcher_started = false;
        }
    }
}

pub fn new_state() -> SharedNetworkState {
    Arc::new(Mutex::new(NetworkState::default()))
}

pub async fn legacy_connect_stub(
    context: &mut dyn WIPICContext,
    cb: WIPICWord,
    param: WIPICWord,
) -> Result<i32> {
    tracing::warn!("stub MC_netConnect({cb:#x}, {param:#x})");

    struct ConnectCallback {
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ConnectCallback {
        #[tracing::instrument(name = "timer", skip_all)]
        async fn call(
            &self,
            context: &mut dyn WIPICContext,
            _: Box<[WIPICWord]>,
        ) -> Result<WIPICResult> {
            context.system().sleep(1).await;
            context
                .call_function(self.cb, &[u32::MAX, self.param])
                .await?;

            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(ConnectCallback { cb, param }))?;
    Ok(0)
}

pub async fn legacy_close_stub(_context: &mut dyn WIPICContext) -> Result<()> {
    tracing::warn!("stub MC_netClose()");
    Ok(())
}

pub async fn legacy_socket_close_stub(
    _context: &mut dyn WIPICContext,
    fd: i32,
) -> Result<i32> {
    tracing::warn!("stub MC_netSocketClose({fd})");
    Ok(-1)
}

pub async fn connect(
    context: &mut dyn WIPICContext,
    cb: WIPICWord,
    param: WIPICWord,
) -> Result<i32> {
    let state = context.network_state();
    let generation = match state.lock().begin_connect(cb, param) {
        Ok(generation) => generation,
        Err(error) => return Ok(error),
    };

    struct ConnectCallback {
        generation: u64,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ConnectCallback {
        async fn call(
            &self,
            context: &mut dyn WIPICContext,
            _: Box<[WIPICWord]>,
        ) -> Result<WIPICResult> {
            let callback = context
                .network_state()
                .lock()
                .finish_connect(self.generation);

            if let Some((callback, param)) = callback {
                if callback != 0 {
                    context.call_function(callback, &[0, param]).await?;
                }
            }

            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(ConnectCallback { generation }))?;
    Ok(0)
}

pub async fn close(context: &mut dyn WIPICContext) -> Result<i32> {
    let sockets = context.network_state().lock().close_process();

    if let Some(network) = context.system().platform().network() {
        for socket in sockets {
            let _ = network.close(socket);
        }
    }

    Ok(0)
}

/// `MC_netSocketClose` (0x25e) @ native 0x1b3090.
///
/// Unlike read/write there is no availability, buffer or type gate. ABI:
/// r0 = socket. The native looks up the process network object and returns 0
/// when there is none (nothing to close); looks up the socket and returns -2
/// when it is unknown; then clears the socket's connect-completion callback and
/// calls `dsocket_close`. On success (0) it unlinks and frees the socket object
/// and returns 0; on -2077 it returns -2 without freeing; on -4005 it returns
/// -19; on anything else -1 - and in every error case the socket object stays.
///
/// WIE mirrors this: a Closed process network -> 0; an unknown socket -> -2;
/// on a successful backend close it drops the socket (which also drops its
/// callbacks, matching the native's connect-callback clear plus free) and
/// returns 0; on a backend error it maps the code through `map_network_error`
/// (-2 for a bad socket, -19 for would-block, -1 otherwise) and keeps the
/// socket, as the native does.
pub async fn socket_close(context: &mut dyn WIPICContext, fd: i32) -> Result<i32> {
    let state = context.network_state();

    if state.lock().process_state == ProcessNetworkState::Closed {
        return Ok(0);
    }

    if state.lock().socket_type(fd).is_none() {
        return Ok(M_E_BADFD);
    }

    let Some(network) = context.system().platform().network() else {
        return Ok(M_E_NOTCONN);
    };

    Ok(match network.close(fd) {
        Ok(()) => {
            state.lock().remove_socket(fd);
            0
        }
        Err(error) => map_network_error(error),
    })
}


pub async fn bill_socket(
    _context: &mut dyn WIPICContext,
    _family: i32,
    _socket_type: i32,
) -> Result<i32> {
    Ok(M_E_NOTCONN)
}

const M_E_ERROR: i32 = -1;
const M_E_BADFD: i32 = -2;
const M_E_INVALID: i32 = -9;
const M_E_NOTCONN: i32 = -14;
const M_E_NOTSUP: i32 = -16;
const M_E_WOULDBLOCK: i32 = -19;
const M_E_TIMEOUT: i32 = -20;

/// First opaque handle `MC_netHttpOpen` hands out. The value only has to be a
/// stable positive id distinct from a socket fd; the game treats it as opaque.
const HTTP_HANDLE_BASE: i32 = 0x4000_0000;

fn map_network_error(error: wie_backend::NetworkError) -> i32 {
    use wie_backend::NetworkError;

    match error {
        NetworkError::InvalidSocket => M_E_BADFD,
        NetworkError::NotConnected => M_E_NOTCONN,
        NetworkError::Unsupported => M_E_NOTSUP,
        NetworkError::WouldBlock => M_E_WOULDBLOCK,
        NetworkError::TimedOut => M_E_TIMEOUT,
        NetworkError::ConnectionRefused
        | NetworkError::HostUnreachable
        | NetworkError::Other => M_E_ERROR,
    }
}

pub async fn socket(context: &mut dyn WIPICContext, family: i32, socket_type: i32) -> Result<i32> {

    if family != 2 || !matches!(socket_type, 1 | 2) {
        return Ok(M_E_NOTSUP);
    }

    // Native MC_netSocket only requires the current process to have a
    // network object; it does not require that object's state to be
    // available. In particular, state 2 (connecting) is accepted.
    if !context.network_state().lock().has_process_network() {
        return Ok(M_E_NOTCONN);
    }

    let Some(network) = context.system().platform().network() else {
        return Ok(M_E_NOTCONN);
    };

    Ok(match network.socket(family, socket_type) {
        Ok(socket) => {
            context
                .network_state()
                .lock()
                .register_socket(socket, socket_type);
            socket
        }
        Err(error) => map_network_error(error),
    })
}

pub async fn socket_connect(
    context: &mut dyn WIPICContext,
    socket: i32,
    address: WIPICWord,
    port: WIPICWord,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<i32> {
    if address == 0 || callback == 0 {
        return Ok(M_E_INVALID);
    }

    let state = context.network_state();
    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(socket_type) = state.lock().socket_type(socket) else {
        return Ok(M_E_BADFD);
    };

    if socket_type != 1 {
        return Ok(M_E_NOTSUP);
    }

    if context.system().platform().network().is_none() {
        return Ok(M_E_NOTCONN);
    }

    // Native stores the new callback/context before attempting the connect.
    // If socket +0x20 is already 2 (connect pending), the repeated call
    // therefore replaces the callback but returns -7 without starting
    // another connect operation.
    {
        let mut state = state.lock();
        state.set_connect_callback(socket, callback, callback_context);

        if state.connect_is_pending(socket) {
            return Ok(-7);
        }
    }

    let result = {
        let network = context
            .system()
            .platform()
            .network()
            .expect("network backend disappeared");
        network.connect(socket, address, port as u16)
    };

    match result {
        wie_backend::NetworkPoll::Ready(Ok(())) => {
            // Native synchronous success posts event 205 and sets socket
            // +0x20 to 2 until that event is processed.
            state.lock().set_connect_pending(socket, true);

            struct DeferredConnect {
                socket: i32,
            }

            #[async_trait::async_trait]
            impl MethodBody<WieError> for DeferredConnect {
                async fn call(
                    &self,
                    context: &mut dyn WIPICContext,
                    _: Box<[WIPICWord]>,
                ) -> Result<WIPICResult> {
                    let callback = context
                        .network_state()
                        .lock()
                        .take_callback_for_event(wie_backend::NetworkEvent::Connected(self.socket));

                    if let Some((callback, args)) = callback {
                        context.call_function(callback, &args).await?;
                    }

                    Ok(WIPICResult { results: Vec::new() })
                }
            }

            if let Err(error) = context.spawn(Box::new(DeferredConnect { socket })) {
                let mut state = state.lock();
                state.set_connect_pending(socket, false);
                state.clear_connect_callback(socket);
                return Err(error);
            }

            Ok(0)
        }
        wie_backend::NetworkPoll::Ready(Err(error)) => {
            // Native has already stored socket +0x24/+0x38 at this point
            // and does not clear them when dsocket_connect returns an
            // immediate error. A later connect call overwrites them.
            Ok(map_network_error(error))
        }
        wie_backend::NetworkPoll::Pending => {
            // Native maps the first dsocket_connect == -19 to public 0 and
            // records socket +0x20 = 2. A repeated call while that state is
            // pending returns -7.
            state.lock().set_connect_pending(socket, true);

            if let Err(error) = ensure_event_dispatcher(context) {
                let mut state = state.lock();
                state.set_connect_pending(socket, false);
                state.clear_connect_callback(socket);
                return Err(error);
            }

            Ok(0)
        }
    }
}

/// `MC_netSocketWrite` (0x25c) @ native 0x1b35ec.
///
/// ABI: r0 = socket, r1 = buffer, r2 = length. The native gates in this exact
/// order, and WIE mirrors each one:
///
/// 1. buffer == 0 || length < 0  -> -9  (length == 0 is allowed through)
/// 2. `WPNet_IsAvailable()` < 0   -> -14
/// 3. `find_socket_obj()` == null -> -2
/// 4. socket type (`[sock+0x14]`) != 1 (stream) -> -16
///
/// A billing socket (`[sock+0x18]` in {1,2}) is written through `WPBill_Write`
/// instead of `dsocket_send`; WIE does not model billing sockets, so every
/// socket takes the normal send path. The native returns the lower send result
/// directly when it is >= 0 - so a partial write returns its own byte count -
/// and maps the negative internal errors as: -2077 -> -2, -2022 -> -9,
/// -2011/-4005 -> -19 (would-block/pending), -2107 -> -14, anything else -> -1.
/// `map_network_error` reproduces the same public codes from the backend error
/// variants. The call mutates no socket field: the write callback / `Writable`
/// event path is separate (`MC_netSetWriteCB`).
pub async fn socket_write(
    context: &mut dyn WIPICContext,
    socket: i32,
    buffer: WIPICWord,
    length: i32,
) -> Result<i32> {

    if buffer == 0 || length < 0 {
        return Ok(M_E_INVALID);
    }

    let state = context.network_state();
    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(socket_type) = state.lock().socket_type(socket) else {
        return Ok(M_E_BADFD);
    };

    if socket_type != 1 {
        return Ok(M_E_NOTSUP);
    }

    let mut data = alloc::vec![0u8; length as usize];
    context.read_bytes(buffer, &mut data)?;

    let Some(network) = context.system().platform().network() else {
        return Ok(M_E_NOTCONN);
    };

    Ok(match network.write(socket, &data) {
        Ok(written) => written as i32,
        Err(error) => map_network_error(error),
    })
}

/// `MC_netSocketRead` (0x25d) @ native 0x1b3264.
///
/// Structurally identical to `MC_netSocketWrite`, calling `dsocket_recv` in
/// place of `dsocket_send`. ABI: r0 = socket, r1 = buffer, r2 = length, gated in
/// the same order: buffer == 0 || length < 0 -> -9; `WPNet_IsAvailable()` < 0 ->
/// -14; `find_socket_obj()` == null -> -2; socket type (`[sock+0x14]`) != 1
/// (stream) -> -16. A billing socket (`[sock+0x18]` != 0) is read through
/// `WPBill_Read`, which WIE does not model. The lower recv count is returned
/// directly when it is >= 0 - so a partial read returns its own byte count and
/// only that many bytes reach the guest buffer - and the negative internal
/// errors map identically to write: -2077 -> -2, -2022 -> -9, -2011/-4005 -> -19
/// (would-block/pending), -2107 -> -14, anything else -> -1.
pub async fn socket_read(
    context: &mut dyn WIPICContext,
    socket: i32,
    buffer: WIPICWord,
    length: i32,
) -> Result<i32> {

    if buffer == 0 || length < 0 {
        return Ok(M_E_INVALID);
    }

    let state = context.network_state();
    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(socket_type) = state.lock().socket_type(socket) else {
        return Ok(M_E_BADFD);
    };

    if socket_type != 1 {
        return Ok(M_E_NOTSUP);
    }

    let mut data = alloc::vec![0u8; length as usize];

    let result = {
        let Some(network) = context.system().platform().network() else {
            return Ok(M_E_NOTCONN);
        };

        network.read(socket, &mut data)
    };

    match result {
        Ok(read) => {
            context.write_bytes(buffer, &data[..read])?;
            Ok(read as i32)
        }
        Err(error) => Ok(map_network_error(error)),
    }
}

/// `MC_netSocketBind` (0x25f) @ native 0x1b2ef0.
///
/// Binds a socket to a local address. ABI: r0 = socket, r1 = address,
/// r2 = port; the native stores the port as a 16-bit `sockaddr_in.sin_port`, so
/// only its low sixteen bits are used. Unlike read/write there is no buffer
/// gate, and it accepts both stream and datagram sockets. The native gates, in
/// order: `WPNet_IsAvailable()` < 0 -> -14; `find_socket_obj()` == null -> -2;
/// socket family (`[sock+0x10]`) != 2 -> -16; socket type (`[sock+0x14]`) not in
/// {1,2} -> -16. It then calls `dsocket_bind`, mapping 0 -> 0, -2077 -> -2,
/// -2022 -> -9, -2107 -> -14, anything else -> -1.
///
/// WIE only ever creates AF_INET sockets of type 1 or 2 (see `socket`), so the
/// family and type gates hold for any socket that exists; the backend honours
/// the bind for datagram sockets and accepts it as a no-op for stream sockets,
/// which std binds at connect time.
pub async fn socket_bind(context: &mut dyn WIPICContext, socket: i32, address: WIPICWord, port: WIPICWord) -> Result<i32> {
    let state = context.network_state();
    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(socket_type) = state.lock().socket_type(socket) else {
        return Ok(M_E_BADFD);
    };

    if socket_type != 1 && socket_type != 2 {
        return Ok(M_E_NOTSUP);
    }

    let Some(network) = context.system().platform().network() else {
        return Ok(M_E_NOTCONN);
    };

    Ok(match network.bind(socket, address, port as u16) {
        Ok(()) => 0,
        Err(error) => map_network_error(error),
    })
}

/// The maximum datagram payload `MC_netGetMaxPacketLength` reports. The native
/// reads this from the registered subnet driver, which is not part of the
/// static image; WIE uses the conventional WIPI UDP payload limit - a 1500-byte
/// Ethernet MTU less the 20-byte IPv4 and 8-byte UDP headers - pending
/// confirmation against a device.
const MAX_PACKET_LENGTH: i32 = 1472;

/// `MC_netGetMaxPacketLength` (0x260) @ native 0x1b2eac.
///
/// Takes no arguments and returns the network's maximum datagram payload. The
/// native queries the subnet HAL (`dnetwork_control_subnet(subnet, 1001, ...)`);
/// WIE has no such HAL, so it returns the fixed [`MAX_PACKET_LENGTH`].
pub async fn get_max_packet_length(_context: &mut dyn WIPICContext) -> Result<i32> {
    Ok(MAX_PACKET_LENGTH)
}

/// `MC_netSocketSendTo` (0x261) @ native 0x1b2d78.
///
/// Sends a datagram to an explicit address. ABI: r0 = socket, r1 = buffer,
/// r2 = length, r3 = address, 5th arg = port (stored as a 16-bit sin_port).
/// Unlike write the length must be strictly positive, and only datagram sockets
/// are accepted. The native gates in order: buffer == 0 || length <= 0 -> -9;
/// `WPNet_IsAvailable()` < 0 -> -14; `find_socket_obj()` == null -> -2; family
/// (`[sock+0x10]`) != 2 -> -16; type (`[sock+0x14]`) != 2 (datagram) -> -16.
/// `dsocket_sendto` then maps 0.. -> the sent count, -2077 -> -2, -2022 -> -9,
/// -2011/-4005 -> -19, -2107 -> -14, -4006 -> -16, anything else -> -1.
pub async fn socket_send_to(
    context: &mut dyn WIPICContext,
    socket: i32,
    buffer: WIPICWord,
    length: i32,
    address: WIPICWord,
    port: WIPICWord,
) -> Result<i32> {
    if buffer == 0 || length <= 0 {
        return Ok(M_E_INVALID);
    }

    let state = context.network_state();
    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(socket_type) = state.lock().socket_type(socket) else {
        return Ok(M_E_BADFD);
    };

    if socket_type != 2 {
        return Ok(M_E_NOTSUP);
    }

    let mut data = alloc::vec![0u8; length as usize];
    context.read_bytes(buffer, &mut data)?;

    let Some(network) = context.system().platform().network() else {
        return Ok(M_E_NOTCONN);
    };

    Ok(match network.send_to(socket, &data, address, port as u16) {
        Ok(sent) => sent as i32,
        Err(error) => map_network_error(error),
    })
}

/// `MC_netSocketRcvFrom` (0x262) @ native 0x1b2c30.
///
/// Receives a datagram and reports the sender. ABI: r0 = socket, r1 = buffer,
/// r2 = length, r3 = out-address pointer, 5th arg = out-port pointer. The native
/// gates in order: buffer == 0 || length <= 0 -> -9; out-address == 0 ||
/// out-port == 0 -> -9; `WPNet_IsAvailable()` < 0 -> -14; `find_socket_obj()` ==
/// null -> -2; family (`[sock+0x10]`) != 2 -> -16; type (`[sock+0x14]`) != 2 ->
/// -16. On success it writes the sender's port (a 16-bit `sin_port`) through the
/// out-port pointer and the sender's address (32 bits) through the out-address
/// pointer, and returns the byte count. `dsocket_recvfrom` maps -2077 -> -2,
/// -2022 -> -9, -2011 -> -19, -4006 -> -16, -2107 -> -14, anything else -> -1.
pub async fn socket_recv_from(
    context: &mut dyn WIPICContext,
    socket: i32,
    buffer: WIPICWord,
    length: i32,
    out_address: WIPICWord,
    out_port: WIPICWord,
) -> Result<i32> {
    if buffer == 0 || length <= 0 {
        return Ok(M_E_INVALID);
    }

    if out_address == 0 || out_port == 0 {
        return Ok(M_E_INVALID);
    }

    let state = context.network_state();
    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(socket_type) = state.lock().socket_type(socket) else {
        return Ok(M_E_BADFD);
    };

    if socket_type != 2 {
        return Ok(M_E_NOTSUP);
    }

    let mut data = alloc::vec![0u8; length as usize];

    let result = {
        let Some(network) = context.system().platform().network() else {
            return Ok(M_E_NOTCONN);
        };

        network.recv_from(socket, &mut data)
    };

    match result {
        Ok((read, address, port)) => {
            context.write_bytes(buffer, &data[..read])?;
            // The native writes the sender's port as a 16-bit sin_port and its
            // address as a 32-bit word, both little-endian on ARM.
            context.write_bytes(out_port, &port.to_le_bytes())?;
            context.write_bytes(out_address, &address.to_le_bytes())?;
            Ok(read as i32)
        }
        Err(error) => Ok(map_network_error(error)),
    }
}

/// `MC_netSocketAccept` (0x264) @ native 0x1b21d8.
///
/// Accepts a pending connection on a listening stream socket. ABI: r0 = socket,
/// r1 = out-address pointer, r2 = out-port pointer (the peer's address and port
/// are written there on success). The native gates in order: no process network
/// object -> -14; `find_socket_obj()` == null -> -2; the network object is not
/// available -> -14. It then `dsocket_listen`s the socket and `dsocket_accept`s:
/// a ready connection allocates a new socket object and returns its handle with
/// the peer written back; otherwise the lower result maps -2077 -> -2,
/// -2022 -> -9, -2011/-4005 -> -19, -2107 -> -14, -4006 -> -16, else -1.
///
/// WIE has no TCP-server backend - no target game acts as a server - so a
/// listening socket never has a pending connection. Accept therefore validates
/// faithfully and reports -19 (would-block), the same result a real idle
/// listener gives when nothing has connected yet. Returning a freshly accepted
/// connection is deferred until a game needs server-side sockets.
pub async fn socket_accept(
    context: &mut dyn WIPICContext,
    socket: i32,
    _out_address: WIPICWord,
    _out_port: WIPICWord,
) -> Result<i32> {
    let state = context.network_state();

    if !state.lock().has_process_network() {
        return Ok(M_E_NOTCONN);
    }

    if state.lock().socket_type(socket).is_none() {
        return Ok(M_E_BADFD);
    }

    if !state.lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    Ok(M_E_WOULDBLOCK)
}

/// `MC_netGetHostAddr` (0x263) @ native 0x1b236c.
///
/// Resolves a host name to an IPv4 address and delivers it through a callback.
/// ABI: r0 = DNS server address, r1 = host name, r2 = callback, r3 = callback
/// context. The native rejects a null host name, DNS server or callback with
/// -9. A dotted-decimal host resolves synchronously via `MC_utilInetAddrInt`;
/// any other name is resolved on a worker thread. Either way the callback is
/// invoked as `callback(address, context)`, where `address` is the resolved
/// IPv4 in the WIPI encoding or `0xFFFF_FFFF` when it cannot be resolved (the
/// same sentinel `MC_utilInetAddrInt` returns for a malformed address). The call
/// itself returns 0 once the lookup is under way.
///
/// WIE has no subnet HAL, so it ignores the specific DNS server (validating only
/// that it is non-null) and resolves through the host OS resolver on a worker
/// thread, delivering the result as a `HostResolved` event that the network
/// dispatcher turns into the callback.
pub async fn get_host_addr(
    context: &mut dyn WIPICContext,
    dns_server: WIPICWord,
    host_name: WIPICWord,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<i32> {
    if host_name == 0 || dns_server == 0 || callback == 0 {
        return Ok(M_E_INVALID);
    }

    let host = String::from_utf8_lossy(&read_null_terminated_string_bytes(context, host_name)?).into_owned();

    let query_id = context.network_state().lock().begin_dns_query(callback, callback_context);

    {
        let Some(network) = context.system().platform().network() else {
            context.network_state().lock().take_dns_query(query_id);
            return Ok(M_E_ERROR);
        };
        network.resolve_host(&host, query_id);
    }

    ensure_event_dispatcher(context)?;
    Ok(0)
}

fn ensure_event_dispatcher(context: &mut dyn WIPICContext) -> Result<()> {
    let state = context.network_state();

    let Some(generation) = state.lock().start_dispatcher() else {
        return Ok(());
    };

    struct NetworkEventDispatcher {
        generation: u64,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for NetworkEventDispatcher {
        async fn call(
            &self,
            context: &mut dyn WIPICContext,
            _: Box<[WIPICWord]>,
        ) -> Result<WIPICResult> {
            loop {
                let state = context.network_state();

                if !state.lock().dispatcher_is_current(self.generation) {
                    return Ok(WIPICResult { results: Vec::new() });
                }

                if !state.lock().has_callbacks() {
                    state.lock().stop_dispatcher(self.generation);
                    return Ok(WIPICResult { results: Vec::new() });
                }

                let event = {
                    let system = context.system();
                    let Some(network) = system.platform().network() else {
                        state.lock().stop_dispatcher(self.generation);
                        return Ok(WIPICResult { results: Vec::new() });
                    };

                    network.poll_event()
                };

                let Some(event) = event else {
                    context.system().sleep(1).await;
                    continue;
                };

                let callback = state.lock().take_callback_for_event(event);

                if let Some((callback, args)) = callback {
                    if let Err(error) = context.call_function(callback, &args).await {
                        state.lock().stop_dispatcher(self.generation);
                        return Err(error);
                    }
                }
            }
        }
    }

    if let Err(error) = context.spawn(Box::new(NetworkEventDispatcher { generation })) {
        state.lock().stop_dispatcher(generation);
        return Err(error);
    }

    Ok(())
}

/// `MC_netSetReadCB` (0x265) @ native 0x1b1538.
///
/// Registers the socket's readable callback. ABI: r0 = socket, r1 = callback,
/// r2 = context. The native stores the callback (at socket +0x2c) and context
/// (at socket +0x40) only when the socket exists and its network is available;
/// a missing socket or unavailable network is silently skipped. It always
/// returns 0 - there are no error codes.
pub async fn set_read_callback(
    context: &mut dyn WIPICContext,
    socket: i32,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<i32> {
    let stored = {
        let state = context.network_state();
        let mut state = state.lock();
        if state.is_available() {
            state.set_read_callback(socket, callback, callback_context);
            true
        } else {
            false
        }
    };

    if stored && callback != 0 {
        ensure_event_dispatcher(context)?;
    }

    Ok(0)
}

/// `MC_netSetWriteCB` (0x266) @ native 0x1b14e8.
///
/// Registers the socket's writable callback. ABI: r0 = socket, r1 = callback,
/// r2 = context. Like `MC_netSetReadCB`, the native stores the callback (at
/// socket +0x30) and context (at socket +0x44) only when the socket exists and
/// its network is available, and always returns 0.
pub async fn set_write_callback(
    context: &mut dyn WIPICContext,
    socket: i32,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<i32> {
    let stored = {
        let state = context.network_state();
        let mut state = state.lock();
        if state.is_available() {
            state.set_write_callback(socket, callback, callback_context);
            true
        } else {
            false
        }
    };

    if stored && callback != 0 {
        ensure_event_dispatcher(context)?;
    }

    Ok(0)
}

/// Splits an HTTP URL into `(host, port, path)` the way `MC_netHttpOpen` does.
///
/// The native parser skips an optional `scheme://` prefix, takes the authority
/// up to the first `/`, reads an optional `:port` (leading decimal digits; a
/// zero or absent port keeps the default 80), and uses the remainder as the
/// path, defaulting to `"/"`. WIE reproduces the observable result; the exact
/// slash-normalisation only affects the request line `MC_netHttpConnect` emits
/// and is revisited there.
fn parse_http_url(url: &str) -> (String, u16, String) {
    let after_scheme = match url.find("://") {
        Some(pos) => &url[pos + 3..],
        None => url,
    };

    let (authority, path) = match after_scheme.find('/') {
        Some(index) => (&after_scheme[..index], &after_scheme[index..]),
        None => (after_scheme, "/"),
    };
    let path = if path.is_empty() { "/" } else { path };

    let (host, port) = match authority.rfind(':') {
        Some(index) => {
            let digits: u32 = authority[index + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .fold(0u32, |acc, b| acc.saturating_mul(10).saturating_add((b - b'0') as u32));
            let port = if digits == 0 { 80 } else { digits.min(u16::MAX as u32) as u16 };
            (&authority[..index], port)
        }
        None => (authority, 80),
    };

    (host.into(), port, path.into())
}

/// Reads a NUL-terminated guest string into an owned `String`, lossily.
fn read_cstring(context: &mut dyn WIPICContext, ptr: WIPICWord) -> Result<String> {
    Ok(String::from_utf8_lossy(&read_null_terminated_string_bytes(context, ptr)?).into_owned())
}

/// Writes `bytes` followed by a NUL terminator to the guest buffer at `ptr`,
/// mirroring the native `dlib_strcpy` the getters use.
fn write_cstring(context: &mut dyn WIPICContext, ptr: WIPICWord, bytes: &[u8]) -> Result<()> {
    context.write_bytes(ptr, bytes)?;
    context.write_bytes(ptr + bytes.len() as WIPICWord, &[0])?;
    Ok(())
}

/// `MC_netHttpOpen` (0x267) @ native 0x1b3e20.
///
/// Creates a connection object for `url`. ABI: r0 = URL string; returns the
/// object handle (a positive value) or an error. The native gates
/// `WPNet_IsAvailable() < 0 -> -14` (no process network object; the connecting
/// state is accepted), duplicates and requires a non-empty URL (`-1` otherwise),
/// parses it into host/port/path, then eagerly creates the stream socket via
/// `MC_netSocket(2, 1)` - any failure there is reported as `-14` - and seeds the
/// default request method `"GET"`, an empty header set and no proxy.
///
/// WIE keeps the same observable object keyed by an opaque handle and creates
/// the same backing stream socket so `MC_netHttpConnect` and `MC_netHttpClose`
/// operate on a real fd. Allocation cannot fail in Rust, so the native `-13`
/// out-of-memory paths do not arise.
pub async fn http_open(context: &mut dyn WIPICContext, url_ptr: WIPICWord) -> Result<i32> {
    if !context.network_state().lock().has_process_network() {
        return Ok(M_E_NOTCONN);
    }

    if url_ptr == 0 {
        return Ok(M_E_ERROR);
    }

    let url = read_cstring(context, url_ptr)?;
    if url.is_empty() {
        return Ok(M_E_ERROR);
    }

    let socket = {
        let Some(network) = context.system().platform().network() else {
            return Ok(M_E_NOTCONN);
        };
        match network.socket(2, 1) {
            Ok(socket) => socket,
            Err(_) => return Ok(M_E_NOTCONN),
        }
    };

    let (host, port, path) = parse_http_url(&url);

    let handle = {
        let state = context.network_state();
        let mut state = state.lock();
        state.register_socket(socket, 1);
        state.http_alloc(HttpObject {
            socket,
            host,
            port,
            path,
            method: "GET".into(),
            ..Default::default()
        })
    };

    Ok(handle)
}

/// `MC_netHttpClose` (0x275) @ native 0x1b3154.
///
/// Destroys a connection object. ABI: r0 = object handle; returns 0, or -2 for a
/// null/unknown object. The native frees the object's host, path, POST body and
/// header buffers, unlinks any received-packet list, clears the committed flag,
/// closes the backing socket via `MC_netSocketClose` and frees the object.
///
/// WIE drops the object (releasing its owned buffers), removes and closes the
/// backing socket, and returns 0.
pub async fn http_close(context: &mut dyn WIPICContext, handle: i32) -> Result<i32> {
    let Some(object) = context.network_state().lock().http_remove(handle) else {
        return Ok(M_E_BADFD);
    };

    context.network_state().lock().remove_socket(object.socket);
    if let Some(network) = context.system().platform().network() {
        let _ = network.close(object.socket);
    }

    Ok(0)
}

/// `MC_netHttpSetRequestMethod` (0x269) @ native 0x1b1afc.
///
/// Sets the request method. ABI: r0 = object, r1 = method string, r2 = POST body
/// pointer, r3 = POST body length. The native gates a null/unknown object -> -2
/// and an already-connected object -> -1, then requires a non-null method -> -9.
/// It accepts `"GET"`, `"POST"` and `"HEAD"` case-insensitively, storing the
/// canonical upper-case form; `"POST"` additionally requires a non-null body of
/// positive length (-9 otherwise) which it copies into an owned buffer. Any other
/// method string is -9.
pub async fn http_set_request_method(
    context: &mut dyn WIPICContext,
    handle: i32,
    method_ptr: WIPICWord,
    body_ptr: WIPICWord,
    body_len: i32,
) -> Result<i32> {
    match http_validate_config(context, handle) {
        Ok(()) => {}
        Err(code) => return Ok(code),
    }

    if method_ptr == 0 {
        return Ok(M_E_INVALID);
    }

    let method = read_cstring(context, method_ptr)?;

    let body = if method.eq_ignore_ascii_case("post") {
        if body_ptr == 0 || body_len <= 0 {
            return Ok(M_E_INVALID);
        }
        let mut data = alloc::vec![0u8; body_len as usize];
        context.read_bytes(body_ptr, &mut data)?;
        Some(data)
    } else if method.eq_ignore_ascii_case("get") || method.eq_ignore_ascii_case("head") {
        None
    } else {
        return Ok(M_E_INVALID);
    };

    let canonical = if method.eq_ignore_ascii_case("get") {
        "GET"
    } else if method.eq_ignore_ascii_case("post") {
        "POST"
    } else {
        "HEAD"
    };

    let state = context.network_state();
    let mut state = state.lock();
    let object = state.http_get_mut(handle).expect("validated above");
    object.method = canonical.into();
    if let Some(body) = body {
        object.post_body = body;
    }

    Ok(0)
}

/// `MC_netHttpGetRequestMethod` (0x26a) @ native 0x1b1a74.
///
/// Copies the current request method into the caller's buffer. ABI: r0 = object,
/// r1 = out buffer, r2 = buffer length. Gates a null/unknown object -> -2, an
/// already-connected object -> -1, a null buffer -> -9, and a method that would
/// not fit (`len >= buffer length`) -> -9. On success it copies the method plus
/// a NUL terminator and returns the method length.
pub async fn http_get_request_method(
    context: &mut dyn WIPICContext,
    handle: i32,
    out_ptr: WIPICWord,
    out_len: i32,
) -> Result<i32> {
    match http_validate_config(context, handle) {
        Ok(()) => {}
        Err(code) => return Ok(code),
    }

    if out_ptr == 0 {
        return Ok(M_E_INVALID);
    }

    let method = {
        let state = context.network_state();
        let state = state.lock();
        state.http_get(handle).expect("validated above").method.clone()
    };

    if method.len() as i32 >= out_len {
        return Ok(M_E_INVALID);
    }

    write_cstring(context, out_ptr, method.as_bytes())?;
    Ok(method.len() as i32)
}

/// `MC_netHttpSetRequestProperty` (0x26b) @ native 0x1b1954.
///
/// Appends an HTTP request header. ABI: r0 = object, r1 = key, r2 = value. Gates
/// a null/unknown object -> -2, an already-connected object -> -1, and a null key
/// or value -> -9. The native accumulates the headers as one growing string:
/// the first property is stored as `"key: value"` and every later property is
/// appended as `"\r\nkey: value"`.
pub async fn http_set_request_property(
    context: &mut dyn WIPICContext,
    handle: i32,
    key_ptr: WIPICWord,
    value_ptr: WIPICWord,
) -> Result<i32> {
    match http_validate_config(context, handle) {
        Ok(()) => {}
        Err(code) => return Ok(code),
    }

    if key_ptr == 0 || value_ptr == 0 {
        return Ok(M_E_INVALID);
    }

    let key = read_cstring(context, key_ptr)?;
    let value = read_cstring(context, value_ptr)?;

    let state = context.network_state();
    let mut state = state.lock();
    let object = state.http_get_mut(handle).expect("validated above");
    append_request_property(&mut object.request_headers, &key, &value);

    Ok(0)
}

/// Appends `"key: value"` to the accumulated request-header string, matching the
/// native `MC_netHttpSetRequestProperty`: the first property is stored bare and
/// every later one is prefixed with `"\r\n"`.
fn append_request_property(headers: &mut String, key: &str, value: &str) {
    if !headers.is_empty() {
        headers.push_str("\r\n");
    }
    headers.push_str(key);
    headers.push_str(": ");
    headers.push_str(value);
}

/// `MC_netHttpGetRequestProperty` (0x26c) @ native 0x1b161c.
///
/// Looks a previously set request header up by key. ABI: r0 = object, r1 = key,
/// r2 = out buffer, r3 = buffer length. Gates a null/unknown object -> -2, an
/// already-connected object -> -1, a null key or buffer -> -9, and a non-positive
/// length -> -9. The native scans the accumulated header string line by line,
/// matching the key case-insensitively followed by `": "`, and copies the value
/// (up to the terminating CR) into the buffer, returning its length; a value that
/// would overflow the buffer is -9 and a missing key is -1.
pub async fn http_get_request_property(
    context: &mut dyn WIPICContext,
    handle: i32,
    key_ptr: WIPICWord,
    out_ptr: WIPICWord,
    out_len: i32,
) -> Result<i32> {
    match http_validate_config(context, handle) {
        Ok(()) => {}
        Err(code) => return Ok(code),
    }

    if key_ptr == 0 || out_ptr == 0 {
        return Ok(M_E_INVALID);
    }
    if out_len <= 0 {
        return Ok(M_E_INVALID);
    }

    let key = read_cstring(context, key_ptr)?;

    let value = {
        let state = context.network_state();
        let state = state.lock();
        let headers = &state.http_get(handle).expect("validated above").request_headers;
        find_request_property(headers, &key)
    };

    let Some(value) = value else {
        return Ok(M_E_ERROR);
    };

    if value.len() as i32 >= out_len {
        return Ok(M_E_INVALID);
    }

    write_cstring(context, out_ptr, value.as_bytes())?;
    Ok(value.len() as i32)
}

/// Finds the value of the header whose key matches `key` case-insensitively in
/// the accumulated `"key: value\r\nkey2: value2"` request-header string.
fn find_request_property(headers: &str, key: &str) -> Option<String> {
    let key = key.as_bytes();
    for line in headers.split("\r\n") {
        let bytes = line.as_bytes();
        if bytes.len() >= key.len() + 2
            && bytes[..key.len()].eq_ignore_ascii_case(key)
            && bytes[key.len()] == b':'
            && bytes[key.len() + 1] == b' '
        {
            return Some(line[key.len() + 2..].into());
        }
    }
    None
}

/// `MC_netHttpSetProxy` (0x26d) @ native 0x1b0fd4.
///
/// Records the proxy the connection should route through. ABI: r0 = object,
/// r1 = proxy address (WIPI IPv4 encoding), r2 = proxy port. The native validates
/// the address first - `0` and `0xFFFF_FFFF` are rejected with -9 - then gates a
/// null/unknown object -> -2 and an already-connected object -> -1 before storing
/// the address and 16-bit port. Returns 0 on success.
pub async fn http_set_proxy(
    context: &mut dyn WIPICContext,
    handle: i32,
    address: WIPICWord,
    port: WIPICWord,
) -> Result<i32> {
    if address == 0 || address == 0xFFFF_FFFF {
        return Ok(M_E_INVALID);
    }

    match http_validate_config(context, handle) {
        Ok(()) => {}
        Err(code) => return Ok(code),
    }

    let state = context.network_state();
    let mut state = state.lock();
    let object = state.http_get_mut(handle).expect("validated above");
    object.proxy_addr = address;
    object.proxy_port = port as u16;

    Ok(0)
}

/// `MC_netHttpGetProxy` (0x26e) @ native 0x1b102c.
///
/// Reads back the configured proxy. ABI: r0 = object, r1 = out address pointer,
/// r2 = out port pointer. Gates a null/unknown object -> -2, an already-connected
/// object -> -1, and a null out-address or out-port pointer -> -9. On success it
/// writes the stored address (32-bit) and port (16-bit) and returns 0; an object
/// with no proxy set reports address 0 and port 0.
pub async fn http_get_proxy(
    context: &mut dyn WIPICContext,
    handle: i32,
    out_address: WIPICWord,
    out_port: WIPICWord,
) -> Result<i32> {
    match http_validate_config(context, handle) {
        Ok(()) => {}
        Err(code) => return Ok(code),
    }

    if out_address == 0 || out_port == 0 {
        return Ok(M_E_INVALID);
    }

    let (address, port) = {
        let state = context.network_state();
        let state = state.lock();
        let object = state.http_get(handle).expect("validated above");
        (object.proxy_addr, object.proxy_port)
    };

    // The native writes the port as a 16-bit sin_port and the address as a
    // 32-bit word, both little-endian on ARM.
    context.write_bytes(out_port, &port.to_le_bytes())?;
    context.write_bytes(out_address, &address.to_le_bytes())?;

    Ok(0)
}

/// `MC_netHttpGetHeaderField` (0x272) @ native 0x1b18d8.
///
/// Copies a response header field by name. ABI: r0 = object, r1 = field name,
/// r2 = out buffer, r3 = buffer length. Unlike the request-side getters this one
/// requires the object to be connected: a null/unknown object -> -2, an object
/// that has not connected -> -1, a null name -> -1, and a null buffer or a length
/// below 1 -> -9. When connected it defers to the response parser
/// (`HttpParser_GetHeaderField`).
///
/// The response parser is part of the network path implemented in a later unit;
/// until `MC_netHttpConnect` sets the committed flag, this faithfully reports the
/// not-connected result (-1).
pub async fn http_get_header_field(
    context: &mut dyn WIPICContext,
    handle: i32,
    name_ptr: WIPICWord,
    out_ptr: WIPICWord,
    out_len: i32,
) -> Result<i32> {
    let connected = {
        let state = context.network_state();
        let state = state.lock();
        match state.http_get(handle) {
            None => return Ok(M_E_BADFD),
            Some(object) => object.connected,
        }
    };

    if !connected {
        return Ok(M_E_ERROR);
    }

    if name_ptr == 0 {
        return Ok(M_E_ERROR);
    }
    if out_ptr == 0 || out_len < 1 {
        return Ok(M_E_INVALID);
    }

    // Connected-object response parsing arrives with the network path.
    Ok(M_E_ERROR)
}

/// `MC_netHttpConnect` (0x268) @ native 0x1b25ec.
///
/// Serialises the configured request, opens the backing socket to the host (or
/// proxy) and drives the exchange to completion, flipping the object's committed
/// flag so the response getters may parse. This is the network path implemented
/// in a later unit.
///
/// Until that lands, the connect faithfully reports that the request could not be
/// carried out rather than masquerading as the native success (0): a null/unknown
/// object is -2 and any live object reports -14 (no reachable network path yet).
pub async fn http_connect(context: &mut dyn WIPICContext, handle: i32) -> Result<i32> {
    let state = context.network_state();
    let state = state.lock();
    if state.http_get(handle).is_none() {
        return Ok(M_E_BADFD);
    }

    tracing::warn!("stub MC_netHttpConnect({handle:#x}); network path not yet implemented");
    Ok(M_E_NOTCONN)
}

/// `MC_netHttpGetResponseCode` (0x26f) @ native 0x1b18c0.
///
/// Returns the parsed HTTP status code. ABI: r0 = object. The native lazily
/// parses the response (`WPNet_ParsePacket`) and returns the stored code; a
/// null/unknown object is -2 and an object that has not connected/parsed is -1.
pub async fn http_get_response_code(context: &mut dyn WIPICContext, handle: i32) -> Result<i32> {
    match http_response_field(context, handle) {
        Ok(object) => Ok(object.response_code),
        Err(code) => Ok(code),
    }
}

/// `MC_netHttpGetLength` (0x272->0x273) @ native 0x1b1848.
///
/// Returns the response `Content-Length`. ABI: r0 = object. Gating matches
/// `MC_netHttpGetResponseCode`: -2 for a null/unknown object, -1 before the
/// response is parsed.
pub async fn http_get_length(context: &mut dyn WIPICContext, handle: i32) -> Result<i32> {
    match http_response_field(context, handle) {
        Ok(object) => Ok(object.content_length),
        Err(code) => Ok(code),
    }
}

/// `MC_netHttpGetResponseMessage` (0x270->0x271) @ native 0x1b1860.
///
/// Copies the response status message. ABI: r0 = object, r1 = out buffer,
/// r2 = buffer length. -2 for a null/unknown object, -1 before parsing, and -9
/// when the buffer is null or too small; otherwise copies the message plus a NUL
/// and returns its length.
pub async fn http_get_response_message(
    context: &mut dyn WIPICContext,
    handle: i32,
    out_ptr: WIPICWord,
    out_len: i32,
) -> Result<i32> {
    let message = match http_response_field(context, handle) {
        Ok(object) => object.response_message.clone(),
        Err(code) => return Ok(code),
    };
    http_copy_response_string(context, &message, out_ptr, out_len, M_E_INVALID).await
}

/// `MC_netHttpGetType` (0x273->0x274) @ native 0x1b17e8.
///
/// Copies the response `Content-Type`. ABI matches `MC_netHttpGetResponseMessage`
/// except a null or too-small buffer is -1, not -9.
pub async fn http_get_type(
    context: &mut dyn WIPICContext,
    handle: i32,
    out_ptr: WIPICWord,
    out_len: i32,
) -> Result<i32> {
    let content_type = match http_response_field(context, handle) {
        Ok(object) => object.content_type.clone(),
        Err(code) => return Ok(code),
    };
    http_copy_response_string(context, &content_type, out_ptr, out_len, M_E_ERROR).await
}

/// `MC_netHttpGetEncoding` (0x274->0x275) @ native 0x1b1788.
///
/// Copies the response `Content-Encoding`. Identical gating to
/// `MC_netHttpGetType`.
pub async fn http_get_encoding(
    context: &mut dyn WIPICContext,
    handle: i32,
    out_ptr: WIPICWord,
    out_len: i32,
) -> Result<i32> {
    let encoding = match http_response_field(context, handle) {
        Ok(object) => object.content_encoding.clone(),
        Err(code) => return Ok(code),
    };
    http_copy_response_string(context, &encoding, out_ptr, out_len, M_E_ERROR).await
}

/// Shared response-getter gate: mirrors `WPNet_ParsePacket`, rejecting a
/// null/unknown object with -2 and an unparsed (not-yet-connected) object with
/// -1. Returns a clone-free borrow of the object for the caller to read.
fn http_response_field(
    context: &mut dyn WIPICContext,
    handle: i32,
) -> core::result::Result<HttpObject, i32> {
    let state = context.network_state();
    let state = state.lock();
    match state.http_get(handle) {
        None => Err(M_E_BADFD),
        Some(object) if !object.connected => Err(M_E_ERROR),
        Some(object) => Ok(object.clone()),
    }
}

/// Copies a parsed response string into the guest buffer with a NUL terminator,
/// returning its length. A null buffer or one too small to hold the string plus
/// terminator yields `too_small_code` (-9 for the response message, -1 for the
/// type/encoding getters, matching the native).
async fn http_copy_response_string(
    context: &mut dyn WIPICContext,
    value: &str,
    out_ptr: WIPICWord,
    out_len: i32,
    too_small_code: i32,
) -> Result<i32> {
    if out_ptr == 0 || out_len <= value.len() as i32 {
        return Ok(too_small_code);
    }
    write_cstring(context, out_ptr, value.as_bytes())?;
    Ok(value.len() as i32)
}

/// Shared validation for the request-configuration getters and setters: rejects
/// a null/unknown object with -2 and an already-connected object with -1.
fn http_validate_config(context: &mut dyn WIPICContext, handle: i32) -> core::result::Result<(), i32> {
    let state = context.network_state();
    let state = state.lock();
    match state.http_get(handle) {
        None => Err(M_E_BADFD),
        Some(object) if object.connected => Err(M_E_ERROR),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod network_state_tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn process_connect_lifecycle_matches_reference_states() {
        let mut state = NetworkState::default();

        let generation = state.begin_connect(0x1111, 0x2222).unwrap();
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Connecting
        ));

        // Reference MC_netConnect: state 2 -> -7.
        assert_eq!(state.begin_connect(0x3333, 0x4444), Err(-7));

        assert_eq!(
            state.finish_connect(generation),
            Some((0x1111, 0x2222))
        );
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Available
        ));

        // Reference MC_netConnect: state 1 -> -10.
        assert_eq!(state.begin_connect(0x3333, 0x4444), Err(-10));
    }

    #[test]
    fn process_connect_with_null_callback_remains_connecting() {
        let mut state = NetworkState::default();

        let generation = state.begin_connect(0, 0x2222).unwrap();

        assert_eq!(state.finish_connect(generation), None);
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Connecting
        ));

        // Native event subtype 2 ignores a null callback before changing
        // state 2 to state 1, so a subsequent MC_netConnect still sees
        // connecting and returns -7.
        assert_eq!(state.begin_connect(0x3333, 0x4444), Err(-7));
    }

    #[test]
    fn stale_process_connect_generation_cannot_complete_new_session() {
        let mut state = NetworkState::default();

        let old_generation = state.begin_connect(0x1111, 0x2222).unwrap();
        let sockets = state.close_process();
        assert!(sockets.is_empty());

        let new_generation = state.begin_connect(0x3333, 0x4444).unwrap();
        assert_ne!(old_generation, new_generation);

        assert_eq!(state.finish_connect(old_generation), None);
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Connecting
        ));

        assert_eq!(
            state.finish_connect(new_generation),
            Some((0x3333, 0x4444))
        );
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Available
        ));
    }

    #[test]
    fn socket_connect_callback_is_one_shot() {
        let mut state = NetworkState::default();
        state.register_socket(7, 1);
        state.set_connect_callback(7, 0x1234, 0x5678);
        state.set_connect_pending(7, true);

        assert!(state.has_callbacks());
        assert!(state.connect_is_pending(7));

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Connected(7)),
            Some((0x1234, [7, 0, 0x5678]))
        );

        assert!(!state.connect_is_pending(7));
        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Connected(7)),
            None
        );
        assert!(!state.has_callbacks());
    }

    #[test]
    fn socket_connect_pending_recall_replaces_native_callback() {
        let mut state = NetworkState::default();
        state.register_socket(8, 1);

        state.set_connect_callback(8, 0x1111, 0x2222);
        state.set_connect_pending(8, true);

        assert!(state.connect_is_pending(8));

        // Native MC_netSocketConnect stores +0x24/+0x38 before calling
        // dsocket_connect. A repeated call while +0x20 == 2 therefore
        // replaces the callback even though its public result is -7.
        state.set_connect_callback(8, 0x3333, 0x4444);

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Connected(8)),
            Some((0x3333, [8, 0, 0x4444]))
        );
        assert!(!state.connect_is_pending(8));
    }

    #[test]
    fn socket_connect_failure_callback_is_one_shot_error() {
        let mut state = NetworkState::default();
        state.register_socket(9, 1);
        state.set_connect_callback(9, 0x4321, 0x8765);
        state.set_connect_pending(9, true);

        assert!(state.connect_is_pending(9));

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::ConnectFailed(9)),
            Some((0x4321, [9, u32::MAX, 0x8765]))
        );

        assert!(!state.connect_is_pending(9));
        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::ConnectFailed(9)),
            None
        );
    }

    #[test]
    fn read_and_write_callbacks_remain_persistent() {
        let mut state = NetworkState::default();
        state.register_socket(11, 1);
        state.set_read_callback(11, 0x1000, 0x2000);
        state.set_write_callback(11, 0x3000, 0x4000);

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Readable(11)),
            Some((0x1000, [11, 0, 0x2000]))
        );
        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Readable(11)),
            Some((0x1000, [11, 0, 0x2000]))
        );

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Writable(11)),
            Some((0x3000, [11, 0, 0x4000]))
        );
        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Writable(11)),
            Some((0x3000, [11, 0, 0x4000]))
        );
    }

    #[test]
    fn dispatcher_generation_rejects_stale_dispatcher() {
        let mut state = NetworkState::default();

        let old_generation = state.start_dispatcher().unwrap();
        assert!(state.dispatcher_is_current(old_generation));
        assert_eq!(state.start_dispatcher(), None);

        state.stop_dispatcher(old_generation);
        assert!(!state.dispatcher_is_current(old_generation));

        let new_generation = state.start_dispatcher().unwrap();
        assert_ne!(old_generation, new_generation);
        assert!(!state.dispatcher_is_current(old_generation));
        assert!(state.dispatcher_is_current(new_generation));

        // A stale dispatcher must not stop the current one.
        state.stop_dispatcher(old_generation);
        assert!(state.dispatcher_is_current(new_generation));
    }

    #[test]
    fn process_close_native_result_type_is_i32() {
        // MC_netClose is an int-returning API. Binding close(context) itself
        // to Future<Output = Result<i32>> prevents it from regressing to
        // Result<()>, whose generic WIPI-C conversion emits no r0 result word.
        fn require_close_result<'a>(
            context: &'a mut dyn WIPICContext,
        ) {
            fn require_i32_future<'a, F>(_: F)
            where
                F: core::future::Future<Output = Result<i32>> + 'a,
            {
            }

            require_i32_future(close(context));
        }

        let _ = require_close_result;
    }

    #[test]
    fn close_process_invalidates_dispatcher_and_removes_all_sockets() {
        let mut state = NetworkState::default();

        let process_generation = state.begin_connect(0x1111, 0x2222).unwrap();
        assert!(state.finish_connect(process_generation).is_some());

        state.register_socket(3, 1);
        state.register_socket(4, 2);
        state.set_read_callback(3, 0x1000, 0x2000);

        let dispatcher_generation = state.start_dispatcher().unwrap();
        assert!(state.dispatcher_is_current(dispatcher_generation));

        let mut sockets = state.close_process();
        sockets.sort_unstable();

        assert_eq!(sockets, vec![3, 4]);
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Closed
        ));
        assert!(!state.is_available());
        assert!(state.sockets.is_empty());
        assert!(!state.has_callbacks());
        assert!(!state.dispatcher_is_current(dispatcher_generation));
    }

    #[test]
    fn process_network_existence_matches_native_socket_gate() {
        let mut state = NetworkState::default();

        assert!(!state.has_process_network());

        let generation = state.begin_connect(0, 0x2222).unwrap();
        assert!(matches!(
            state.process_state,
            ProcessNetworkState::Connecting
        ));

        // Native MC_netSocket checks only whether find_network_obj_ex
        // returns an object. It does not require state 1 (available).
        assert!(state.has_process_network());
        assert!(!state.is_available());

        // A null callback keeps the native object in state 2.
        assert_eq!(state.finish_connect(generation), None);
        assert!(state.has_process_network());

        state.close_process();
        assert!(!state.has_process_network());
    }

    #[test]
    fn socket_metadata_tracks_type_and_removal() {
        let mut state = NetworkState::default();

        state.register_socket(21, 1);
        state.register_socket(22, 2);

        assert_eq!(state.socket_type(21), Some(1));
        assert_eq!(state.socket_type(22), Some(2));
        assert_eq!(state.socket_type(23), None);

        state.remove_socket(21);
        assert_eq!(state.socket_type(21), None);
        assert_eq!(state.socket_type(22), Some(2));
    }

    #[test]
    fn parse_http_url_splits_authority_port_and_path() {
        assert_eq!(
            parse_http_url("http://www.example.com/path/to?q=1"),
            ("www.example.com".into(), 80, "/path/to?q=1".into())
        );
        // Explicit port.
        assert_eq!(
            parse_http_url("http://host.co.kr:8080/a"),
            ("host.co.kr".into(), 8080, "/a".into())
        );
        // No path defaults to "/".
        assert_eq!(parse_http_url("http://host"), ("host".into(), 80, "/".into()));
        // Scheme is optional.
        assert_eq!(parse_http_url("host/p"), ("host".into(), 80, "/p".into()));
        // A zero or empty port keeps the default 80, matching the native.
        assert_eq!(parse_http_url("http://host:0/"), ("host".into(), 80, "/".into()));
        // Only the leading decimal run is taken as the port.
        assert_eq!(parse_http_url("http://host:80x/"), ("host".into(), 80, "/".into()));
    }

    #[test]
    fn request_property_accumulation_matches_native_format() {
        let mut headers = String::new();
        // First property is bare; later ones gain a CRLF prefix.
        append_request_property(&mut headers, "Accept", "text/html");
        assert_eq!(headers, "Accept: text/html");
        append_request_property(&mut headers, "User-Agent", "WIE");
        assert_eq!(headers, "Accept: text/html\r\nUser-Agent: WIE");
    }

    #[test]
    fn find_request_property_is_case_insensitive_and_bounded() {
        let headers = "Accept: text/html\r\nContent-Length: 42";
        assert_eq!(find_request_property(headers, "accept").as_deref(), Some("text/html"));
        assert_eq!(
            find_request_property(headers, "CONTENT-LENGTH").as_deref(),
            Some("42")
        );
        // A key that is only a prefix of a header name must not match.
        assert_eq!(find_request_property(headers, "Content"), None);
        assert_eq!(find_request_property(headers, "Missing"), None);
    }

    #[test]
    fn http_objects_get_distinct_positive_handles_and_can_be_removed() {
        let mut state = NetworkState::default();

        let a = state.http_alloc(HttpObject {
            socket: 5,
            method: "GET".into(),
            ..Default::default()
        });
        let b = state.http_alloc(HttpObject {
            socket: 6,
            method: "POST".into(),
            ..Default::default()
        });

        assert!(a > 0 && b > 0 && a != b);
        assert_eq!(state.http_get(a).unwrap().socket, 5);
        assert_eq!(state.http_get(b).unwrap().method, "POST");

        state.http_get_mut(a).unwrap().connected = true;
        assert!(state.http_get(a).unwrap().connected);

        assert_eq!(state.http_remove(a).unwrap().socket, 5);
        assert!(state.http_get(a).is_none());
        assert!(state.http_get(b).is_some());
    }

    #[test]
    fn close_process_drops_http_objects_and_returns_their_sockets() {
        let mut state = NetworkState::default();

        let generation = state.begin_connect(0x1111, 0x2222).unwrap();
        assert!(state.finish_connect(generation).is_some());

        // MC_netHttpOpen registers the object's stream socket in `sockets`.
        state.register_socket(9, 1);
        let handle = state.http_alloc(HttpObject {
            socket: 9,
            ..Default::default()
        });
        assert!(state.http_get(handle).is_some());

        let sockets = state.close_process();
        assert_eq!(sockets, vec![9]);
        assert!(state.http_get(handle).is_none());
    }
}
