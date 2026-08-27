use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use spin::Mutex;
use wipi_types::wipic::WIPICWord;

use wie_util::{Result, WieError};

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

#[derive(Default)]
pub struct NetworkState {
    process_state: ProcessNetworkState,
    process_callback: WIPICWord,
    process_context: WIPICWord,
    process_generation: u64,
    sockets: BTreeMap<i32, SocketCallbacks>,
    dispatcher_started: bool,
    dispatcher_generation: u64,
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

        let sockets = self.sockets.keys().copied().collect();
        self.sockets.clear();
        sockets
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
        };

        (callback != 0).then_some((
            callback,
            [socket as WIPICWord, result, callback_context],
        ))
    }

    fn has_callbacks(&self) -> bool {
        self.sockets.values().any(|entry| {
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

pub async fn set_read_callback(
    context: &mut dyn WIPICContext,
    socket: i32,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<i32> {

    context
        .network_state()
        .lock()
        .set_read_callback(socket, callback, callback_context);

    if callback != 0 {
        ensure_event_dispatcher(context)?;
    }

    Ok(0)
}

pub async fn set_write_callback(
    context: &mut dyn WIPICContext,
    socket: i32,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<i32> {

    context
        .network_state()
        .lock()
        .set_write_callback(socket, callback, callback_context);

    if callback != 0 {
        ensure_event_dispatcher(context)?;
    }

    Ok(0)
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
}
