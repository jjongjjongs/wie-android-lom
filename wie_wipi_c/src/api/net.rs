use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use spin::Mutex;
use wipi_types::wipic::WIPICWord;

use wie_util::{Result, WieError, read_null_terminated_string_bytes};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};

#[derive(Clone, Copy)]
struct SocketCallbacks {
    socket_type: i32,
    /// Native socket object +0x18.
    ///
    /// 0 = ordinary socket
    /// 1 = MC_netBillSocket
    /// 2 = MC_netTestBillSocket
    billing_mode: u8,
    /// Locally synthesized application response for the proven LGT
    /// purchase operation 0x68. A fixed buffer preserves SocketCallbacks'
    /// Copy semantics and is sufficient for the seven-byte 0x69 success
    /// frame.
    billing_local_response: [u8; LGT_LOCAL_PURCHASE_RESPONSE_SIZE],
    billing_local_response_len: u8,
    billing_local_response_offset: u8,
    /// Native socket object +0x4c..+0x83: 56-byte inbound billing header.
    billing_read_header: [u8; LGT_BILL_READ_HEADER_SIZE],
    /// Native socket object +0x1c.
    ///
    /// Set to one when a complete 56-byte billing response header has been
    /// assembled. A later direct positive recv clears it.
    billing_read_direct: bool,
    connect_callback: WIPICWord,
    connect_context: WIPICWord,
    connect_pending: bool,
    read_callback: WIPICWord,
    read_context: WIPICWord,
    write_callback: WIPICWord,
    write_context: WIPICWord,
}

impl Default for SocketCallbacks {
    fn default() -> Self {
        Self {
            socket_type: 0,
            billing_mode: 0,
            billing_local_response: [0u8; LGT_LOCAL_PURCHASE_RESPONSE_SIZE],
            billing_local_response_len: 0,
            billing_local_response_offset: 0,
            billing_read_header: [0u8; LGT_BILL_READ_HEADER_SIZE],
            billing_read_direct: false,
            connect_callback: 0,
            connect_context: 0,
            connect_pending: false,
            read_callback: 0,
            read_context: 0,
            write_callback: 0,
            write_context: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct BillingReadState {
    /// Native unnamed global @ 0x004de3f0.
    remaining_payload: usize,
    /// Native s_HeaderOffset @ 0x004de3f4.
    header_offset: usize,
    /// Native s_RemainHeaderLen @ 0x00478804.
    remaining_header: usize,
}

impl Default for BillingReadState {
    fn default() -> Self {
        Self {
            remaining_payload: 0,
            header_offset: 0,
            // Native .data initial value is 56.
            remaining_header: LGT_BILL_READ_HEADER_SIZE,
        }
    }
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
    /// Mirrors the native `[obj+0xc]` committed flag: set the moment
    /// `MC_netHttpConnect` starts the exchange, which freezes the request
    /// configuration.
    connected: bool,
    /// Mirrors the native `[obj+0xcc]` received flag: set once the response has
    /// been received and parsed. The response getters gate on this - matching
    /// `WPNet_ParsePacket`, which returns -1 until then.
    response_ready: bool,
    /// Response fields, populated when the exchange completes.
    response_code: i32,
    response_message: String,
    content_type: String,
    content_encoding: String,
    content_length: i32,
    /// All response headers in receipt order, for `MC_netHttpGetHeaderField`.
    response_headers: Vec<(String, String)>,
}

#[derive(Default)]
pub struct NetworkState {
    process_state: ProcessNetworkState,
    process_callback: WIPICWord,
    process_context: WIPICWord,
    process_generation: u64,
    sockets: BTreeMap<i32, SocketCallbacks>,
    /// Native global s_BillHeader @ 0x004de384.
    ///
    /// WPBill_SetHeader and WPBill_Write share this single 108-byte object
    /// across every billing socket. None represents its initial zeroed BSS
    /// state before SetHeader/Write has materialized it.
    billing_header: Option<[u8; LGT_BILL_HEADER_SIZE]>,
    /// WPBill_Read uses three globals shared by all billing sockets.
    billing_read: BillingReadState,
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
    fn begin_connect(&mut self, callback: WIPICWord, context: WIPICWord) -> core::result::Result<u64, i32> {
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

    fn finish_connect(&mut self, generation: u64) -> Option<(WIPICWord, WIPICWord)> {
        if self.process_state != ProcessNetworkState::Connecting || self.process_generation != generation {
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

    fn register_socket(&mut self, socket: i32, socket_type: i32, billing_mode: u8) {
        let entry = self.sockets.entry(socket).or_default();
        entry.socket_type = socket_type;
        entry.billing_mode = billing_mode;
    }

    fn socket_type(&self, socket: i32) -> Option<i32> {
        self.sockets.get(&socket).map(|entry| entry.socket_type)
    }

    fn billing_mode(&self, socket: i32) -> Option<u8> {
        self.sockets.get(&socket).map(|entry| entry.billing_mode)
    }

    fn queue_local_billing_response(&mut self, socket: i32, response: [u8; LGT_LOCAL_PURCHASE_RESPONSE_SIZE]) {
        let entry = self.sockets.get_mut(&socket).expect("socket metadata disappeared");

        entry.billing_local_response = response;
        entry.billing_local_response_len = LGT_LOCAL_PURCHASE_RESPONSE_SIZE as u8;
        entry.billing_local_response_offset = 0;
    }

    fn take_local_billing_response(&mut self, socket: i32, output: &mut [u8]) -> Option<usize> {
        let entry = self.sockets.get_mut(&socket).expect("socket metadata disappeared");

        let length = entry.billing_local_response_len as usize;
        let offset = entry.billing_local_response_offset as usize;

        if length == 0 || offset >= length {
            entry.billing_local_response_len = 0;
            entry.billing_local_response_offset = 0;
            return None;
        }

        let read = output.len().min(length - offset);

        output[..read].copy_from_slice(&entry.billing_local_response[offset..offset + read]);

        let next = offset + read;

        if next == length {
            entry.billing_local_response_len = 0;
            entry.billing_local_response_offset = 0;
        } else {
            entry.billing_local_response_offset = next as u8;
        }

        Some(read)
    }

    fn install_billing_header(&mut self, header: [u8; LGT_BILL_HEADER_SIZE]) {
        // Native WPBill_SetHeader writes the single global s_BillHeader.
        self.billing_header = Some(header);

        // It also resets WPBill_Read's three global counters:
        //
        //   *(0x004de3f0) = 0
        //   s_HeaderOffset = 0
        //   s_RemainHeaderLen = 56
        self.billing_read = BillingReadState::default();
    }

    fn update_billing_header(&mut self, header: [u8; LGT_BILL_HEADER_SIZE]) {
        // Native WPBill_Write mutates the same global s_BillHeader but does
        // not reset any WPBill_Read state.
        self.billing_header = Some(header);
    }

    fn billing_header(&self) -> Option<[u8; LGT_BILL_HEADER_SIZE]> {
        self.billing_header
    }

    fn remove_socket(&mut self, socket: i32) {
        self.sockets.remove(&socket);
    }

    fn set_connect_callback(&mut self, socket: i32, callback: WIPICWord, context: WIPICWord) {
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
        self.sockets.get(&socket).is_some_and(|entry| entry.connect_pending)
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

    fn take_callback_for_event(&mut self, event: wie_backend::NetworkEvent) -> Option<(WIPICWord, [WIPICWord; 3])> {
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

        (callback != 0).then_some((callback, [socket as WIPICWord, result, callback_context]))
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
            || self
                .sockets
                .values()
                .any(|entry| entry.connect_callback != 0 || entry.read_callback != 0 || entry.write_callback != 0)
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

pub async fn legacy_connect_stub(context: &mut dyn WIPICContext, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    tracing::warn!("stub MC_netConnect({cb:#x}, {param:#x})");

    struct ConnectCallback {
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ConnectCallback {
        #[tracing::instrument(name = "timer", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            context.system().sleep(1).await;
            context.call_function(self.cb, &[u32::MAX, self.param]).await?;

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

pub async fn legacy_socket_close_stub(_context: &mut dyn WIPICContext, fd: i32) -> Result<i32> {
    tracing::warn!("stub MC_netSocketClose({fd})");
    Ok(-1)
}

pub async fn connect(context: &mut dyn WIPICContext, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
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
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            let callback = context.network_state().lock().finish_connect(self.generation);

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

/// Restored legacy `MC_netBillSocket` constructor.
///
/// The reference LGT library's public entry is carrier-disabled and returns
/// `M_E_NOTCONN`, but the preserved native implementation immediately after
/// that gate shows the original constructor semantics. WIE restores those
/// semantics for legacy titles that depend on billing sockets.
///
/// Native socket object `+0x18` is set to 1 for `MC_netBillSocket`.
/// Billing-aware connect/read/write behavior is implemented separately.
pub async fn bill_socket(context: &mut dyn WIPICContext, family: i32, socket_type: i32) -> Result<i32> {
    // The preserved native body calls WPNet_IsAvailable before dsocket_open.
    // Unlike ordinary MC_netSocket, a merely-existing process network object
    // is therefore insufficient here.
    if !context.network_state().lock().is_available() {
        return Ok(M_E_NOTCONN);
    }

    let Some(network) = context.system().platform().network() else {
        return Ok(M_E_NOTCONN);
    };

    Ok(match network.socket(family, socket_type) {
        Ok(socket) => {
            context.network_state().lock().register_socket(socket, socket_type, 1);
            socket
        }
        Err(error) => map_network_error(error),
    })
}

const M_E_ERROR: i32 = -1;
const M_E_BADFD: i32 = -2;
const M_E_INVALID: i32 = -9;
const M_E_NOTCONN: i32 = -14;
const M_E_NOTSUP: i32 = -16;
const M_E_WOULDBLOCK: i32 = -19;
const M_E_TIMEOUT: i32 = -20;

/// Native MC_netTestBillSocket (billing mode 2) uses this fixed development
/// gateway. WIE's production MC_netBillSocket mode 1 is satisfied locally and
/// never falls back to this development endpoint.
const LGT_TEST_BILL_GATEWAY: &str = "wipigwdev.ez-i.co.kr:30000";

const LGT_LOCAL_PURCHASE_REQUEST_TYPE: u16 = 0x0068;
const LGT_LOCAL_PURCHASE_RESPONSE_TYPE: u16 = 0x0069;
const LGT_LOCAL_PURCHASE_RESPONSE_SIZE: usize = 7;

/// Build the native-compatible LGT purchase-success application frame.
///
/// Red Gem's native protocol establishes:
///
///   frame +0x00 : 0xffff
///   frame +0x02 : total application-frame length (u16, network order)
///   frame +0x04 : message type (u16, network order)
///   frame +0x06 : application payload
///
/// Request 0x68 is the purchase transaction and response 0x69 carries
/// its carrier result.  Status zero is purchase success.  Returning the
/// ordinary 0x69/status-zero frame lets the guest's own parser write its
/// result enum zero; WIE never modifies guest billing state directly.
fn lgt_local_purchase_success_response(request: &[u8]) -> Option<[u8; LGT_LOCAL_PURCHASE_RESPONSE_SIZE]> {
    if request.len() < 6 {
        return None;
    }

    if request[0] != 0xff || request[1] != 0xff {
        return None;
    }

    let declared_length = u16::from_be_bytes([request[2], request[3]]) as usize;
    let message_type = u16::from_be_bytes([request[4], request[5]]);

<<<<<<< HEAD
    // Native LGT clients may expose only the current socket-write slice even
    // though the application header declares the complete frame length.
    // Red Gem builds a 19-byte 0x68 frame but calls MC_netSocketWrite with
    // its first 10 bytes. Accept a header-complete prefix, while rejecting
    // malformed slices that exceed the declared frame.
    if declared_length < 6 || request.len() > declared_length || message_type != LGT_LOCAL_PURCHASE_REQUEST_TYPE {
=======
    if declared_length != request.len()
        || message_type != LGT_LOCAL_PURCHASE_REQUEST_TYPE
    {
>>>>>>> parent of 54b34ba (Accept native LGT billing write prefixes)
        return None;
    }

    let response_length = LGT_LOCAL_PURCHASE_RESPONSE_SIZE as u16;

    Some([
        0xff,
        0xff,
        (response_length >> 8) as u8,
        response_length as u8,
        (LGT_LOCAL_PURCHASE_RESPONSE_TYPE >> 8) as u8,
        LGT_LOCAL_PURCHASE_RESPONSE_TYPE as u8,
        0x00,
    ])
}

const LGT_BILL_HEADER_SIZE: usize = 108;
const LGT_BILL_READ_HEADER_SIZE: usize = 56;
const LGT_BILL_READ_PAYLOAD_LENGTH_OFFSET: usize = 0x30;
const LGT_BILL_READ_TAG_OFFSET: usize = 0x34;

const LGT_BILL_INFO_PHONE_MODEL: &str = "PHONEMODEL";
const LGT_BILL_INFO_MDN: &str = "MDN";
const LGT_BILL_INFO_CURRENT_CH: &str = "CURRENTCH";
const LGT_BILL_INFO_SID: &str = "SID";
const LGT_BILL_INFO_NID: &str = "NID";
const LGT_BILL_INFO_BASE_ID: &str = "BASEID";
const LGT_BILL_INFO_BEST_PN: &str = "BESTPN";

/// First opaque handle `MC_netHttpOpen` hands out. The value only has to be a
/// stable positive id distinct from a socket fd; the game treats it as opaque.
const HTTP_HANDLE_BASE: i32 = 0x4000_0000;

/// Consecutive would-block yields the HTTP exchange tolerates on send or receive
/// before abandoning a stalled socket, so a silent peer can never wedge the task.
const HTTP_IDLE_POLL_LIMIT: u32 = 60_000;

fn copy_lgt_bill_c_string(header: &mut [u8; LGT_BILL_HEADER_SIZE], offset: usize, value: &[u8]) {
    if offset >= header.len() {
        return;
    }

    // WPBill_SetHeader uses strcpy. We reproduce the bytes that remain
    // inside s_BillHeader; later native strcpy calls may overwrite an
    // earlier long string, notably the DLET launch parameter at +0x04.
    let available = header.len() - offset;
    let copy_len = value.len().min(available.saturating_sub(1));

    header[offset..offset + copy_len].copy_from_slice(&value[..copy_len]);

    if offset + copy_len < header.len() {
        header[offset + copy_len] = 0;
    }
}

fn normalize_lgt_bill_mdn(mdn: &[u8]) -> Vec<u8> {
    match mdn.len() {
        10 => {
            let mut result = Vec::with_capacity(12);
            result.extend_from_slice(&mdn[..3]);
            result.extend_from_slice(b"00");
            result.extend_from_slice(&mdn[3..]);
            result
        }
        11 => {
            let mut result = Vec::with_capacity(12);
            result.extend_from_slice(&mdn[..3]);
            result.push(b'0');
            result.extend_from_slice(&mdn[3..]);
            result
        }
        _ => mdn.to_vec(),
    }
}

fn build_lgt_bill_header(
    platform: &dyn wie_backend::Platform,
    aid: &str,
    current_time: u64,
    address: WIPICWord,
    port: u16,
) -> [u8; LGT_BILL_HEADER_SIZE] {
    // Native: memset(s_BillHeader, 0, 108).
    let mut header = [0u8; LGT_BILL_HEADER_SIZE];

    // Android WipiPlayer builds mWipiParam as:
    // /android/<AID>.jar:binary.mod
    // That String is passed through startWipiN -> MH_pltStart ->
    // wipi_exec_directly -> dlet_register -> dlet_get_name.
    let dlet_name = alloc::format!("/android/{aid}.jar:binary.mod");

    // Preserve native strcpy order exactly.
    copy_lgt_bill_c_string(&mut header, 0x04, dlet_name.as_bytes());
    copy_lgt_bill_c_string(&mut header, 0x0e, b"1.1.1");
    copy_lgt_bill_c_string(&mut header, 0x18, b"1.54");

    if let Some(value) = platform.system_information(LGT_BILL_INFO_PHONE_MODEL) {
        copy_lgt_bill_c_string(&mut header, 0x22, value.as_bytes());
    }

    if let Some(value) = platform.system_information(LGT_BILL_INFO_MDN) {
        let normalized = normalize_lgt_bill_mdn(value.as_bytes());
        copy_lgt_bill_c_string(&mut header, 0x2c, &normalized);
    }

    if let Some(value) = platform.system_information(LGT_BILL_INFO_CURRENT_CH) {
        copy_lgt_bill_c_string(&mut header, 0x3c, value.as_bytes());
    }

    if let Some(value) = platform.system_information(LGT_BILL_INFO_SID) {
        copy_lgt_bill_c_string(&mut header, 0x3e, value.as_bytes());
    }

    if let Some(value) = platform.system_information(LGT_BILL_INFO_NID) {
        copy_lgt_bill_c_string(&mut header, 0x43, value.as_bytes());
    }

    // Native BASEID first, BESTPN second into the same +0x48 destination.
    if let Some(value) = platform.system_information(LGT_BILL_INFO_BASE_ID) {
        copy_lgt_bill_c_string(&mut header, 0x48, value.as_bytes());
    }

    if let Some(value) = platform.system_information(LGT_BILL_INFO_BEST_PN) {
        copy_lgt_bill_c_string(&mut header, 0x48, value.as_bytes());
    }

    // Original game destination, not rewritten billing gateway.
    header[0x52..0x54].copy_from_slice(&port.to_le_bytes());
    header[0x54..0x58].copy_from_slice(&address.to_le_bytes());

    // MC_knlCurrentTime -> platform.now().raw() epoch milliseconds.
    // Native MC_utilHtonl is a byte swap on ARM LE before STR.
    let network_time = (current_time as u32).swap_bytes();
    header[0x58..0x5c].copy_from_slice(&network_time.to_le_bytes());

    header
}

fn lgt_bill_read_payload_length(header: &[u8; LGT_BILL_READ_HEADER_SIZE]) -> u32 {
    u32::from_be_bytes(
        header[LGT_BILL_READ_PAYLOAD_LENGTH_OFFSET..LGT_BILL_READ_PAYLOAD_LENGTH_OFFSET + 4]
            .try_into()
            .expect("billing payload length slice"),
    )
}

fn lgt_bill_read_tag(header: &[u8; LGT_BILL_READ_HEADER_SIZE]) -> [u8; 4] {
    header[LGT_BILL_READ_TAG_OFFSET..LGT_BILL_READ_TAG_OFFSET + 4]
        .try_into()
        .expect("billing tag slice")
}

fn lgt_bill_read_reject_tag(tag: [u8; 4]) -> bool {
    // Native performs this exact sequential strncmp chain:
    //
    // KCBD -> KCSP -> KCNB -> KCEP -> KCEN ->
    // UATO -> UATT -> UAFO -> UAFT -> UAFR
    //
    // Every comparison is against the same four bytes and every prior
    // comparison must be equal to reach the next one. With normal strncmp
    // semantics the ten distinct literals therefore cannot all match.
    //
    // Keep the actual predicate visible rather than inventing a carrier
    // meaning for an unreachable branch.
    [
        *b"KCBD", *b"KCSP", *b"KCNB", *b"KCEP", *b"KCEN", *b"UATO", *b"UATT", *b"UAFO", *b"UAFT", *b"UAFR",
    ]
    .into_iter()
    .all(|candidate| tag == candidate)
}

fn build_lgt_bill_write_frame(mut header: [u8; LGT_BILL_HEADER_SIZE], payload: &[u8]) -> ([u8; LGT_BILL_HEADER_SIZE], Vec<u8>) {
    // Native WPBill_Write:
    //   total = payload_len + 108
    //   s_BillHeader[0..4] = MC_utilHtonl(total)
    //
    // The converted 32-bit value is then stored by little-endian ARM STR,
    // leaving big-endian length bytes in the wire header.
    let total = (payload.len() as u32).wrapping_add(LGT_BILL_HEADER_SIZE as u32);
    let network_total = total.swap_bytes();

    header[0..4].copy_from_slice(&network_total.to_le_bytes());

    let mut frame = Vec::with_capacity(LGT_BILL_HEADER_SIZE + payload.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(payload);

    (header, frame)
}

fn lgt_bill_write_public_result(written: usize) -> i32 {
    // Native WPBill_Write returns zero when the lower send reaches no
    // payload byte at all, otherwise it removes the 108-byte header count.
    if written <= LGT_BILL_HEADER_SIZE {
        0
    } else {
        (written - LGT_BILL_HEADER_SIZE) as i32
    }
}

fn resolve_lgt_bill_gateway(network: &dyn wie_backend::Network, gateway: &str) -> Option<(WIPICWord, u16)> {
    // Native WPBill_SetGW accepts either host:port or http://host:port.
    // parse_http_url already implements those authority semantics.
    let (host, port, _) = parse_http_url(gateway);

    if host.is_empty() {
        return None;
    }

    let address = network.resolve_host_blocking(&host);
    if address == 0xFFFF_FFFF {
        return None;
    }

    // Native converts the decimal gateway port through MC_utilHtons before
    // passing it to dsocket_connect.
    Some((address, port.swap_bytes()))
}

fn map_network_error(error: wie_backend::NetworkError) -> i32 {
    use wie_backend::NetworkError;

    match error {
        NetworkError::InvalidSocket => M_E_BADFD,
        NetworkError::NotConnected => M_E_NOTCONN,
        NetworkError::Unsupported => M_E_NOTSUP,
        NetworkError::WouldBlock => M_E_WOULDBLOCK,
        NetworkError::TimedOut => M_E_TIMEOUT,
        NetworkError::ConnectionRefused | NetworkError::HostUnreachable | NetworkError::Other => M_E_ERROR,
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
            context.network_state().lock().register_socket(socket, socket_type, 0);
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

    let billing_mode = state.lock().billing_mode(socket).expect("socket metadata disappeared");

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

    // WIE locally satisfies the legacy production LGT billing connection.
    //
    // The original handset redirected MC_netBillSocket through carrier
    // gateway configuration that no longer exists. For mode 1 we keep
    // the guest-visible native success contract without opening a real
    // backend connection:
    //
    //   * preserve the callback/context already stored above;
    //   * record the original game destination in the billing header;
    //   * mark the connect pending;
    //   * asynchronously deliver Connected(socket), whose callback result is 0.
    //
    // PATCH85B then handles only the proven purchase request 0x68 locally.
    // Mode 2 remains the native fixed development-gateway path.
    if billing_mode == 1 {
        let aid = alloc::string::String::from(context.system().aid());
        let current_time = context.system().platform().now().raw();

        let billing_header = build_lgt_bill_header(context.system().platform(), &aid, current_time, address, port as u16);

        {
            let mut state = state.lock();
            state.install_billing_header(billing_header);
            state.set_connect_pending(socket, true);
        }

        struct DeferredLocalBillConnect {
            socket: i32,
        }

        #[async_trait::async_trait]
        impl MethodBody<WieError> for DeferredLocalBillConnect {
            async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
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

        if let Err(error) = context.spawn(Box::new(DeferredLocalBillConnect { socket })) {
            let mut state = state.lock();
            state.set_connect_pending(socket, false);
            state.clear_connect_callback(socket);
            return Err(error);
        }

        return Ok(0);
    }

    // Ordinary sockets use the original destination. Test billing sockets
    // retain the native fixed LGT development gateway.
    let billing_gateway: Option<String> = match billing_mode {
        0 => None,
        2 => Some(LGT_TEST_BILL_GATEWAY.into()),
        _ => return Ok(M_E_ERROR),
    };

    let (connect_address, connect_port) = if let Some(gateway) = billing_gateway {
        let Some(network) = context.system().platform().network() else {
            return Ok(M_E_NOTCONN);
        };

        let Some(destination) = resolve_lgt_bill_gateway(network, &gateway) else {
            return Ok(M_E_ERROR);
        };

        destination
    } else {
        (address, port as u16)
    };

    let result = {
        let network = context.system().platform().network().expect("network backend disappeared");
        network.connect(socket, connect_address, connect_port)
    };

    match result {
        wie_backend::NetworkPoll::Ready(Ok(())) => {
            if billing_mode != 0 {
                // Native calls WPBill_SetHeader only after lower connect
                // succeeds or reports pending. Preserve the ORIGINAL game
                // destination here; outbound header construction follows in
                // a later billing-write patch.
                let aid = alloc::string::String::from(context.system().aid());
                let current_time = context.system().platform().now().raw();

                let billing_header = build_lgt_bill_header(context.system().platform(), &aid, current_time, address, port as u16);

                state.lock().install_billing_header(billing_header);
            }

            // Native synchronous success posts event 205 and sets socket
            // +0x20 to 2 until that event is processed.
            state.lock().set_connect_pending(socket, true);

            struct DeferredConnect {
                socket: i32,
            }

            #[async_trait::async_trait]
            impl MethodBody<WieError> for DeferredConnect {
                async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
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
            if billing_mode != 0 {
                // Native also initializes WPBill_SetHeader state when
                // dsocket_connect reports its pending result (-19).
                let aid = alloc::string::String::from(context.system().aid());
                let current_time = context.system().platform().now().raw();

                let billing_header = build_lgt_bill_header(context.system().platform(), &aid, current_time, address, port as u16);

                state.lock().install_billing_header(billing_header);
            }

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
/// Ordinary stream sockets return the lower send result directly when it is
/// nonnegative, so a partial write returns its own byte count. Negative lower
/// errors map as: -2077 -> -2, -2022 -> -9, -2011/-4005 -> -19
/// (would-block/pending), -2107 -> -14, anything else -> -1.
/// `map_network_error` reproduces the same public codes from the backend error
/// variants.
///
/// ABI: r0 = socket, r1 = buffer, r2 = length. The native gates in this
/// exact order:
///
/// 1. buffer == 0 || length < 0  -> -9
/// 2. `WPNet_IsAvailable()` < 0   -> -14
/// 3. `find_socket_obj()` == null -> -2
/// 4. socket type != stream       -> -16
///
/// Ordinary sockets call the lower `dsocket_send` directly.
///
/// Billing modes 1/2 instead call `WPBill_Write`. That routine mutates the
/// saved 108-byte billing header first, writing Htonl(payload_len + 108) at
/// offset zero, allocates/copies `header || payload`, and performs one lower
/// send of the combined frame. A negative lower result is mapped normally.
/// A nonnegative result <= 108 becomes public 0; a result > 108 becomes
/// `written - 108`.
///
/// Updating the saved header before the backend write is intentional:
/// native `WPBill_Write` updates global `s_BillHeader[0]` before allocation
/// and `dsocket_send`, so even a later send failure leaves the new length.
pub async fn socket_write(context: &mut dyn WIPICContext, socket: i32, buffer: WIPICWord, length: i32) -> Result<i32> {
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

    let billing_mode = state.lock().billing_mode(socket).expect("socket metadata disappeared");

    let mut data = alloc::vec![0u8; length as usize];
    context.read_bytes(buffer, &mut data)?;

    if billing_mode != 0 {
        if billing_mode == 1 {
            if let Some(response) = lgt_local_purchase_success_response(&data) {
                state.lock().queue_local_billing_response(socket, response);

                // Match a successful application-level socket write. The
                // request is consumed locally, so no carrier/backend write
                // occurs for this purchase transaction.
                return Ok(length);
            }
        }

        // Before WPBill_SetHeader has ever run, native s_BillHeader is BSS
        // zero. Use the same zero state rather than rejecting the write.
        let header = state.lock().billing_header().unwrap_or([0u8; LGT_BILL_HEADER_SIZE]);

        let (header, frame) = build_lgt_bill_write_frame(header, &data);

        // Native has already mutated s_BillHeader[0] at this point, even if
        // the lower allocation/send subsequently fails.
        state.lock().update_billing_header(header);

        let Some(network) = context.system().platform().network() else {
            return Ok(M_E_NOTCONN);
        };

        return Ok(match network.write(socket, &frame) {
            Ok(written) => lgt_bill_write_public_result(written),
            Err(error) => map_network_error(error),
        });
    }

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
/// (stream) -> -16. Ordinary stream sockets return the lower recv count
/// directly when it is nonnegative, so a partial read returns its own byte
/// count and only that many bytes reach the guest buffer. Negative lower errors
/// map identically to write: -2077 -> -2, -2022 -> -9, -2011/-4005 -> -19
/// (would-block/pending), -2107 -> -14, anything else -> -1.
///
/// The public validation/error mapping remains identical to ordinary stream
/// sockets. Billing modes 1/2 dispatch through native `WPBill_Read`.
///
/// `WPBill_Read` has two kinds of state:
///
/// - three globals shared by every billing socket:
///   remaining payload, header offset, remaining header length;
/// - socket-local state:
///   a 56-byte response header at object +0x4c and direct-read flag +0x1c.
///
/// A fresh billing response accumulates exactly 56 header bytes using at most
/// three lower recv attempts in one public call. Header +0x30 contains the
/// network-order payload length and +0x34 contains the four-byte response tag.
/// Payload bytes are then delivered without the 56-byte header.
///
/// A partial payload is continued on later calls through the shared remaining
/// payload counter. If socket +0x1c is already set while no payload remains,
/// native bypasses header parsing and performs one direct recv.
pub async fn socket_read(context: &mut dyn WIPICContext, socket: i32, buffer: WIPICWord, length: i32) -> Result<i32> {
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

    let billing_mode = state.lock().billing_mode(socket).expect("socket metadata disappeared");

    let mut data = alloc::vec![0u8; length as usize];

    if billing_mode == 1 {
        let local_read = state.lock().take_local_billing_response(socket, &mut data);

        if let Some(read) = local_read {
            context.write_bytes(buffer, &data[..read])?;
            return Ok(read as i32);
        }
    }

    if billing_mode == 0 {
        let result = {
            let Some(network) = context.system().platform().network() else {
                return Ok(M_E_NOTCONN);
            };

            network.read(socket, &mut data)
        };

        return match result {
            Ok(read) => {
                context.write_bytes(buffer, &data[..read])?;
                Ok(read as i32)
            }
            Err(error) => Ok(map_network_error(error)),
        };
    }

    // Native first consumes payload left over from a previously parsed
    // 56-byte billing header. No header parsing occurs while this counter is
    // positive.
    let remaining_payload = state.lock().billing_read.remaining_payload;

    if remaining_payload > 0 {
        let read_len = data.len().min(remaining_payload);

        let result = {
            let Some(network) = context.system().platform().network() else {
                return Ok(M_E_NOTCONN);
            };

            network.read(socket, &mut data[..read_len])
        };

        return match result {
            Ok(read) => {
                if read > 0 {
                    state.lock().billing_read.remaining_payload = remaining_payload.saturating_sub(read);
                    context.write_bytes(buffer, &data[..read])?;
                }

                Ok(read as i32)
            }
            Err(error) => Ok(map_network_error(error)),
        };
    }

    // Native socket object +0x1c direct-read path. A positive recv clears the
    // flag. Zero or a negative lower result leaves it set.
    let direct_read = state
        .lock()
        .sockets
        .get(&socket)
        .expect("socket metadata disappeared")
        .billing_read_direct;

    if direct_read {
        let result = {
            let Some(network) = context.system().platform().network() else {
                return Ok(M_E_NOTCONN);
            };

            network.read(socket, &mut data)
        };

        return match result {
            Ok(read) => {
                if read > 0 {
                    if let Some(entry) = state.lock().sockets.get_mut(&socket) {
                        entry.billing_read_direct = false;
                    }

                    context.write_bytes(buffer, &data[..read])?;
                }

                Ok(read as i32)
            }
            Err(error) => Ok(map_network_error(error)),
        };
    }

    // r6 starts at two. Together with the initial attempt this permits up to
    // three dsocket_recv calls while assembling the 56-byte response header.
    for attempt in 0..3 {
        let (header_offset, remaining_header) = {
            let state = state.lock();
            (state.billing_read.header_offset, state.billing_read.remaining_header)
        };

        let mut chunk = alloc::vec![0u8; remaining_header];

        let result = {
            let Some(network) = context.system().platform().network() else {
                return Ok(M_E_NOTCONN);
            };

            network.read(socket, &mut chunk)
        };

        let read = match result {
            Ok(read) => read,
            Err(error) => return Ok(map_network_error(error)),
        };

        let accumulated = header_offset.saturating_add(read);

        if accumulated <= LGT_BILL_READ_HEADER_SIZE - 1 {
            let mut state = state.lock();

            if let Some(entry) = state.sockets.get_mut(&socket) {
                entry.billing_read_header[header_offset..header_offset + read].copy_from_slice(&chunk[..read]);
            }

            state.billing_read.header_offset = accumulated;
            state.billing_read.remaining_header = LGT_BILL_READ_HEADER_SIZE - accumulated;

            if attempt == 2 {
                return Ok(0);
            }

            continue;
        }

        // dsocket_recv is called with exactly remaining_header, therefore a
        // conforming backend reaches this branch only at accumulated == 56.
        if accumulated != LGT_BILL_READ_HEADER_SIZE {
            if attempt == 2 {
                return Ok(0);
            }

            continue;
        }

        let header = {
            let mut state = state.lock();

            let entry = state.sockets.get_mut(&socket).expect("socket metadata disappeared");

            entry.billing_read_header[header_offset..header_offset + read].copy_from_slice(&chunk[..read]);

            entry.billing_read_direct = true;

            let header = entry.billing_read_header;

            // Native resets these globals immediately after a complete
            // 56-byte header has been copied to socket object +0x4c.
            state.billing_read = BillingReadState::default();

            header
        };

        let tag = lgt_bill_read_tag(&header);

        if lgt_bill_read_reject_tag(tag) {
            // Native unreachable ten-tag equality chain returns -9.
            return Ok(M_E_INVALID);
        }

        let payload_length = lgt_bill_read_payload_length(&header);

        // Native uses signed ARM BLE after MC_utilNtohl. Zero and values with
        // the high bit set therefore return zero here and leave +0x1c set.
        if (payload_length as i32) <= 0 {
            return Ok(0);
        }

        let requested = data.len().min(payload_length as usize);

        let result = {
            let Some(network) = context.system().platform().network() else {
                return Ok(M_E_NOTCONN);
            };

            network.read(socket, &mut data[..requested])
        };

        // Once the payload recv has been attempted, native clears socket
        // object +0x1c regardless of success, zero, or lower error.
        if let Some(entry) = state.lock().sockets.get_mut(&socket) {
            entry.billing_read_direct = false;
        }

        return match result {
            Ok(read) => {
                if read > 0 {
                    state.lock().billing_read.remaining_payload = (payload_length as usize).saturating_sub(read);

                    context.write_bytes(buffer, &data[..read])?;
                }

                Ok(read as i32)
            }
            Err(error) => Ok(map_network_error(error)),
        };
    }

    Ok(0)
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
pub async fn socket_accept(context: &mut dyn WIPICContext, socket: i32, _out_address: WIPICWord, _out_port: WIPICWord) -> Result<i32> {
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
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
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
pub async fn set_read_callback(context: &mut dyn WIPICContext, socket: i32, callback: WIPICWord, callback_context: WIPICWord) -> Result<i32> {
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
pub async fn set_write_callback(context: &mut dyn WIPICContext, socket: i32, callback: WIPICWord, callback_context: WIPICWord) -> Result<i32> {
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
        state.register_socket(socket, 1, 0);
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
pub async fn http_get_request_method(context: &mut dyn WIPICContext, handle: i32, out_ptr: WIPICWord, out_len: i32) -> Result<i32> {
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
pub async fn http_set_request_property(context: &mut dyn WIPICContext, handle: i32, key_ptr: WIPICWord, value_ptr: WIPICWord) -> Result<i32> {
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
        if bytes.len() >= key.len() + 2 && bytes[..key.len()].eq_ignore_ascii_case(key) && bytes[key.len()] == b':' && bytes[key.len() + 1] == b' ' {
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
pub async fn http_set_proxy(context: &mut dyn WIPICContext, handle: i32, address: WIPICWord, port: WIPICWord) -> Result<i32> {
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
pub async fn http_get_proxy(context: &mut dyn WIPICContext, handle: i32, out_address: WIPICWord, out_port: WIPICWord) -> Result<i32> {
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
    let (connected, response_ready) = {
        let state = context.network_state();
        let state = state.lock();
        match state.http_get(handle) {
            None => return Ok(M_E_BADFD),
            Some(object) => (object.connected, object.response_ready),
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

    // Before the response is received the parser has nothing to match against,
    // exactly as the native parser walks an empty packet list and finds nothing.
    if !response_ready {
        return Ok(M_E_ERROR);
    }

    let name = read_cstring(context, name_ptr)?;

    let value = {
        let state = context.network_state();
        let state = state.lock();
        let object = state.http_get(handle).expect("validated above");
        object
            .response_headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(&name))
            .map(|(_, value)| value.clone())
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

/// Serialises the configured request into the exact byte stream the native
/// `MC_netHttpConnect` emits.
///
/// Request line: `"{METHOD} {target} HTTP/1.0\r\n"`, where `target` is the path
/// for a direct connection or the absolute `http://host[:port]path` form when a
/// proxy is set. The accumulated request headers follow (each already
/// `"key: value"`), terminated by `\r\n`. A `POST` with a body then appends the
/// native's `"Accept-Ranges: bytes\r\n"`, a `"Content-Length: {n}\r\n"` and the
/// blank line before the body; every other request ends with the blank line.
fn build_http_request(object: &HttpObject) -> Vec<u8> {
    let mut request = Vec::new();
    request.extend_from_slice(object.method.as_bytes());
    request.push(b' ');

    if object.proxy_addr != 0 {
        request.extend_from_slice(b"http://");
        request.extend_from_slice(object.host.as_bytes());
        if object.port != 0 {
            request.extend_from_slice(alloc::format!(":{}", object.port).as_bytes());
        }
    }
    request.extend_from_slice(object.path.as_bytes());
    request.push(b' ');
    request.extend_from_slice(b"HTTP/1.0\r\n");

    if !object.request_headers.is_empty() {
        request.extend_from_slice(object.request_headers.as_bytes());
        request.extend_from_slice(b"\r\n");
    }

    if object.method == "POST" && !object.post_body.is_empty() {
        request.extend_from_slice(b"Accept-Ranges: bytes\r\n");
        request.extend_from_slice(alloc::format!("Content-Length: {}\r\n", object.post_body.len()).as_bytes());
        request.extend_from_slice(b"\r\n");
        request.extend_from_slice(&object.post_body);
    } else {
        request.extend_from_slice(b"\r\n");
    }

    request
}

/// The subset of a parsed HTTP response the WIPI-C getters expose.
struct ParsedResponse {
    code: i32,
    message: String,
    headers: Vec<(String, String)>,
    content_type: String,
    content_encoding: String,
    content_length: i32,
    // The MC_net HTTP API exposes only the headers and status; the response body
    // is parsed for completeness (and covered by tests) but has no C-API reader.
    #[allow(dead_code)]
    body: Vec<u8>,
}

/// Parses a full HTTP response into the fields the native `HttpParser` extracts:
/// the status code and reason phrase, every header, and the `Content-Type`,
/// `Content-Encoding` and `Content-Length` shortcuts. Returns `None` when the
/// status line is missing or malformed.
fn parse_http_response(data: &[u8]) -> Option<ParsedResponse> {
    let split = find_subsequence(data, b"\r\n\r\n");
    let (header_bytes, body) = match split {
        Some(index) => (&data[..index], data[index + 4..].to_vec()),
        None => (data, Vec::new()),
    };

    let header_str = String::from_utf8_lossy(header_bytes);
    let mut lines = header_str.split("\r\n");

    let status = lines.next()?;
    let mut status_parts = status.splitn(3, ' ');
    let _version = status_parts.next()?;
    let code = status_parts.next()?.parse::<i32>().ok()?;
    let message = status_parts.next().unwrap_or("").to_string();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let lookup = |target: &str| -> Option<&str> {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(target))
            .map(|(_, value)| value.as_str())
    };

    let content_type = lookup("Content-Type").unwrap_or("").to_string();
    let content_encoding = lookup("Content-Encoding").unwrap_or("").to_string();
    let content_length = lookup("Content-Length")
        .map(|value| {
            value
                .bytes()
                .take_while(u8::is_ascii_digit)
                .fold(0i64, |acc, b| acc.saturating_mul(10).saturating_add((b - b'0') as i64))
                .min(i32::MAX as i64) as i32
        })
        .unwrap_or(0);

    Some(ParsedResponse {
        code,
        message,
        headers,
        content_type,
        content_encoding,
        content_length,
        body,
    })
}

/// Finds the first index of `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

/// `MC_netHttpConnect` (0x268) @ native 0x1b25ec.
///
/// Serialises the configured request, resolves the host and drives the socket
/// exchange to completion, then invokes the game's completion callback. ABI:
/// r0 = object, r1 = callback, r2 = callback context. Returns 0 once the
/// exchange is under way. The native gates, in order: no WIPI network permission
/// (not modelled here); `WPNet_IsAvailable() < 0` -> -14; a null/unknown object
/// -> -2; a socket already connecting -> -7; a null callback -> -1.
///
/// The native flow resolves the host (`MC_netGetHostAddr`), connects the backing
/// socket, sends the request, receives the response into a packet list and calls
/// the game callback as `callback(object, socket, result, context)` with result 0
/// on success and -1 on failure (`WPNet_GetHostAddrCB` @0x1b3bf8). WIE reproduces
/// that observable flow on the async executor: it sets the committed flag,
/// returns 0, and runs the resolve/connect/send/receive/parse sequence in a
/// spawned task that finally invokes the same callback.
pub async fn http_connect(context: &mut dyn WIPICContext, handle: i32, callback: WIPICWord, callback_context: WIPICWord) -> Result<i32> {
    if !context.network_state().lock().has_process_network() {
        return Ok(M_E_NOTCONN);
    }

    let request = {
        let state = context.network_state();
        let mut state = state.lock();
        let Some(object) = state.http_get_mut(handle) else {
            return Ok(M_E_BADFD);
        };

        // A second connect on an already-committed object mirrors the native
        // socket-connect-pending result.
        if object.connected {
            return Ok(-7);
        }

        if callback == 0 {
            return Ok(M_E_ERROR);
        }

        if object.method.is_empty() {
            return Ok(M_E_ERROR);
        }

        let request = build_http_request(object);
        object.connected = true;
        request
    };

    struct HttpExchange {
        handle: i32,
        request: Vec<u8>,
        callback: WIPICWord,
        callback_context: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for HttpExchange {
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            let outcome = run_http_exchange(context, self.handle, &self.request).await?;

            let (socket, result) = match outcome {
                Some(socket) => (socket, 0),
                None => (0, u32::MAX),
            };

            if self.callback != 0 {
                context
                    .call_function(
                        self.callback,
                        &[self.handle as WIPICWord, socket as WIPICWord, result, self.callback_context],
                    )
                    .await?;
            }

            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(HttpExchange {
        handle,
        request,
        callback,
        callback_context,
    }))?;

    Ok(0)
}

/// Drives the resolve/connect/send/receive/parse sequence for one HTTP request.
///
/// Returns the backing socket fd on success (with the object's response fields
/// populated and `response_ready` set), or `None` on any failure. Every network
/// operation is polled non-blocking with a 1ms yield between attempts, matching
/// the emulator's cooperative executor; only host resolution blocks, on the
/// backend's DNS.
async fn run_http_exchange(context: &mut dyn WIPICContext, handle: i32, request: &[u8]) -> Result<Option<i32>> {
    let Some((socket, host, port)) = ({
        let state = context.network_state();
        let state = state.lock();
        state.http_get(handle).map(|object| (object.socket, object.host.clone(), object.port))
    }) else {
        return Ok(None);
    };

    // Resolve the host.
    let address = {
        let system = context.system();
        let Some(network) = system.platform().network() else {
            return Ok(None);
        };
        network.resolve_host_blocking(&host)
    };
    if address == 0xFFFF_FFFF {
        return Ok(None);
    }

    // Connect the backing socket, polling the async connect until it settles.
    loop {
        let poll = {
            let system = context.system();
            let Some(network) = system.platform().network() else {
                return Ok(None);
            };
            network.connect(socket, address, port)
        };
        match poll {
            wie_backend::NetworkPoll::Ready(Ok(())) => break,
            wie_backend::NetworkPoll::Ready(Err(_)) => return Ok(None),
            wie_backend::NetworkPoll::Pending => context.system().sleep(1).await,
        }
    }

    // Send the whole request. A stalled socket is abandoned after the idle cap
    // so the task can never spin forever.
    let mut sent = 0;
    let mut idle = 0;
    while sent < request.len() {
        let result = {
            let system = context.system();
            let Some(network) = system.platform().network() else {
                return Ok(None);
            };
            network.write(socket, &request[sent..])
        };
        match result {
            Ok(0) => return Ok(None),
            Ok(written) => {
                sent += written;
                idle = 0;
            }
            Err(wie_backend::NetworkError::WouldBlock) => {
                idle += 1;
                if idle > HTTP_IDLE_POLL_LIMIT {
                    return Ok(None);
                }
                context.system().sleep(1).await;
            }
            Err(_) => return Ok(None),
        }
    }

    // Receive the response until the server closes the connection (HTTP/1.0) or
    // the idle cap is hit.
    let mut response = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut idle = 0;
    loop {
        let result = {
            let system = context.system();
            let Some(network) = system.platform().network() else {
                return Ok(None);
            };
            network.read(socket, &mut buffer)
        };
        match result {
            Ok(0) => break,
            Ok(read) => {
                response.extend_from_slice(&buffer[..read]);
                idle = 0;
            }
            Err(wie_backend::NetworkError::WouldBlock) => {
                idle += 1;
                if idle > HTTP_IDLE_POLL_LIMIT {
                    break;
                }
                context.system().sleep(1).await;
            }
            Err(_) => break,
        }
    }

    let Some(parsed) = parse_http_response(&response) else {
        return Ok(None);
    };

    let state = context.network_state();
    let mut state = state.lock();
    let Some(object) = state.http_get_mut(handle) else {
        return Ok(None);
    };
    object.response_code = parsed.code;
    object.response_message = parsed.message;
    object.content_type = parsed.content_type;
    object.content_encoding = parsed.content_encoding;
    object.content_length = parsed.content_length;
    object.response_headers = parsed.headers;
    object.response_ready = true;

    Ok(Some(socket))
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
pub async fn http_get_response_message(context: &mut dyn WIPICContext, handle: i32, out_ptr: WIPICWord, out_len: i32) -> Result<i32> {
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
pub async fn http_get_type(context: &mut dyn WIPICContext, handle: i32, out_ptr: WIPICWord, out_len: i32) -> Result<i32> {
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
pub async fn http_get_encoding(context: &mut dyn WIPICContext, handle: i32, out_ptr: WIPICWord, out_len: i32) -> Result<i32> {
    let encoding = match http_response_field(context, handle) {
        Ok(object) => object.content_encoding.clone(),
        Err(code) => return Ok(code),
    };
    http_copy_response_string(context, &encoding, out_ptr, out_len, M_E_ERROR).await
}

/// Shared response-getter gate: mirrors `WPNet_ParsePacket`, rejecting a
/// null/unknown object with -2 and an object whose response has not yet been
/// received and parsed with -1. Returns an owned copy for the caller to read.
fn http_response_field(context: &mut dyn WIPICContext, handle: i32) -> core::result::Result<HttpObject, i32> {
    let state = context.network_state();
    let state = state.lock();
    match state.http_get(handle) {
        None => Err(M_E_BADFD),
        Some(object) if !object.response_ready => Err(M_E_ERROR),
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
<<<<<<< HEAD
    use alloc::{boxed::Box, vec};

    use test_utils::TestPlatform;
    use wie_backend::{
        AudioSink, DatabaseRepository, DefaultTaskRunner, Filesystem, Instant, Network, NetworkError, NetworkEvent, NetworkPoll, Platform, Screen,
        System,
    };
    use wie_util::ByteWrite;

    use crate::context::test::TestContext;

    use super::*;

    struct LocalBillingTestNetwork;

    impl Network for LocalBillingTestNetwork {
        fn socket(&self, family: i32, socket_type: i32) -> core::result::Result<i32, NetworkError> {
            if family == 2 && socket_type == 1 {
                Ok(31)
            } else {
                Err(NetworkError::Unsupported)
            }
        }

        fn connect(&self, _socket: i32, _address: u32, _port: u16) -> NetworkPoll<()> {
            NetworkPoll::Ready(Err(NetworkError::Unsupported))
        }

        fn bind(&self, _socket: i32, _address: u32, _port: u16) -> core::result::Result<(), NetworkError> {
            Err(NetworkError::Unsupported)
        }

        fn read(&self, _socket: i32, _buf: &mut [u8]) -> core::result::Result<usize, NetworkError> {
            Err(NetworkError::Unsupported)
        }

        fn write(&self, _socket: i32, _buf: &[u8]) -> core::result::Result<usize, NetworkError> {
            Err(NetworkError::Unsupported)
        }

        fn send_to(&self, _socket: i32, _buf: &[u8], _address: u32, _port: u16) -> core::result::Result<usize, NetworkError> {
            Err(NetworkError::Unsupported)
        }

        fn recv_from(&self, _socket: i32, _buf: &mut [u8]) -> core::result::Result<(usize, u32, u16), NetworkError> {
            Err(NetworkError::Unsupported)
        }

        fn close(&self, _socket: i32) -> core::result::Result<(), NetworkError> {
            Ok(())
        }

        fn resolve_host(&self, _host: &str, _query_id: u32) {}

        fn poll_event(&self) -> Option<NetworkEvent> {
            None
        }
    }

    struct LocalBillingTestPlatform {
        base: TestPlatform,
        network: LocalBillingTestNetwork,
    }

    impl LocalBillingTestPlatform {
        fn new() -> Self {
            Self {
                base: TestPlatform::new(),
                network: LocalBillingTestNetwork,
            }
        }
    }

    impl Platform for LocalBillingTestPlatform {
        fn screen(&self) -> &dyn Screen {
            self.base.screen()
        }

        fn now(&self) -> Instant {
            self.base.now()
        }

        fn database_repository(&self) -> &dyn DatabaseRepository {
            self.base.database_repository()
        }

        fn filesystem(&self) -> &dyn Filesystem {
            self.base.filesystem()
        }

        fn audio_sink(&self) -> Box<dyn AudioSink> {
            self.base.audio_sink()
        }

        fn network(&self) -> Option<&dyn Network> {
            Some(&self.network)
        }

        fn system_information(&self, key: &str) -> Option<alloc::string::String> {
            self.base.system_information(key)
        }

        fn open_url(&self, url: &str) -> bool {
            self.base.open_url(url)
        }

        fn write_stdout(&self, buf: &[u8]) {
            self.base.write_stdout(buf);
        }

        fn write_stderr(&self, buf: &[u8]) {
            self.base.write_stderr(buf);
        }

        fn exit(&self) {
            self.base.exit();
        }

        fn vibrate(&self, duration_ms: u64, intensity: u8) {
            self.base.vibrate(duration_ms, intensity);
        }

        fn set_backlight_mode(&self, mode: u8) {
            self.base.set_backlight_mode(mode);
        }
    }

=======
    use alloc::vec;

    use super::*;

>>>>>>> parent of 54b34ba (Accept native LGT billing write prefixes)
    #[test]
    fn process_connect_lifecycle_matches_reference_states() {
        let mut state = NetworkState::default();

        let generation = state.begin_connect(0x1111, 0x2222).unwrap();
        assert!(matches!(state.process_state, ProcessNetworkState::Connecting));

        // Reference MC_netConnect: state 2 -> -7.
        assert_eq!(state.begin_connect(0x3333, 0x4444), Err(-7));

        assert_eq!(state.finish_connect(generation), Some((0x1111, 0x2222)));
        assert!(matches!(state.process_state, ProcessNetworkState::Available));

        // Reference MC_netConnect: state 1 -> -10.
        assert_eq!(state.begin_connect(0x3333, 0x4444), Err(-10));
    }

    #[test]
    fn process_connect_with_null_callback_remains_connecting() {
        let mut state = NetworkState::default();

        let generation = state.begin_connect(0, 0x2222).unwrap();

        assert_eq!(state.finish_connect(generation), None);
        assert!(matches!(state.process_state, ProcessNetworkState::Connecting));

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
        assert!(matches!(state.process_state, ProcessNetworkState::Connecting));

        assert_eq!(state.finish_connect(new_generation), Some((0x3333, 0x4444)));
        assert!(matches!(state.process_state, ProcessNetworkState::Available));
    }

    #[test]
    fn socket_connect_callback_is_one_shot() {
        let mut state = NetworkState::default();
        state.register_socket(7, 1, 0);
        state.set_connect_callback(7, 0x1234, 0x5678);
        state.set_connect_pending(7, true);

        assert!(state.has_callbacks());
        assert!(state.connect_is_pending(7));

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Connected(7)),
            Some((0x1234, [7, 0, 0x5678]))
        );

        assert!(!state.connect_is_pending(7));
        assert_eq!(state.take_callback_for_event(wie_backend::NetworkEvent::Connected(7)), None);
        assert!(!state.has_callbacks());
    }

    #[test]
    fn socket_connect_pending_recall_replaces_native_callback() {
        let mut state = NetworkState::default();
        state.register_socket(8, 1, 0);

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
        state.register_socket(9, 1, 0);
        state.set_connect_callback(9, 0x4321, 0x8765);
        state.set_connect_pending(9, true);

        assert!(state.connect_is_pending(9));

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::ConnectFailed(9)),
            Some((0x4321, [9, u32::MAX, 0x8765]))
        );

        assert!(!state.connect_is_pending(9));
        assert_eq!(state.take_callback_for_event(wie_backend::NetworkEvent::ConnectFailed(9)), None);
    }

    #[test]
    fn read_and_write_callbacks_remain_persistent() {
        let mut state = NetworkState::default();
        state.register_socket(11, 1, 0);
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
        fn require_close_result<'a>(context: &'a mut dyn WIPICContext) {
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

        state.register_socket(3, 1, 0);
        state.register_socket(4, 2, 0);
        state.set_read_callback(3, 0x1000, 0x2000);

        let dispatcher_generation = state.start_dispatcher().unwrap();
        assert!(state.dispatcher_is_current(dispatcher_generation));

        let mut sockets = state.close_process();
        sockets.sort_unstable();

        assert_eq!(sockets, vec![3, 4]);
        assert!(matches!(state.process_state, ProcessNetworkState::Closed));
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
        assert!(matches!(state.process_state, ProcessNetworkState::Connecting));

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

        state.register_socket(21, 1, 0);
        state.register_socket(22, 2, 0);

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
        assert_eq!(parse_http_url("http://host.co.kr:8080/a"), ("host.co.kr".into(), 8080, "/a".into()));
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
        assert_eq!(find_request_property(headers, "CONTENT-LENGTH").as_deref(), Some("42"));
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
    fn build_http_request_matches_native_framing() {
        // GET with no headers: request line then the terminating blank line.
        let get = HttpObject {
            method: "GET".into(),
            path: "/index.html".into(),
            ..Default::default()
        };
        assert_eq!(build_http_request(&get), b"GET /index.html HTTP/1.0\r\n\r\n".to_vec());

        // GET with accumulated headers.
        let mut with_headers = get.clone();
        with_headers.request_headers = "Accept: text/html\r\nUser-Agent: WIE".into();
        assert_eq!(
            build_http_request(&with_headers),
            b"GET /index.html HTTP/1.0\r\nAccept: text/html\r\nUser-Agent: WIE\r\n\r\n".to_vec()
        );

        // POST adds the native's Accept-Ranges + Content-Length before the body.
        let post = HttpObject {
            method: "POST".into(),
            path: "/submit".into(),
            post_body: b"ab=cd".to_vec(),
            ..Default::default()
        };
        assert_eq!(
            build_http_request(&post),
            b"POST /submit HTTP/1.0\r\nAccept-Ranges: bytes\r\nContent-Length: 5\r\n\r\nab=cd".to_vec()
        );

        // A proxy turns the target into an absolute URI.
        let proxied = HttpObject {
            method: "GET".into(),
            host: "example.com".into(),
            port: 8080,
            path: "/p".into(),
            proxy_addr: 0x0100_007f,
            ..Default::default()
        };
        assert_eq!(build_http_request(&proxied), b"GET http://example.com:8080/p HTTP/1.0\r\n\r\n".to_vec());
    }

    #[test]
    fn parse_http_response_extracts_status_headers_and_body() {
        let raw = b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5\r\nContent-Encoding: gzip\r\nX-Custom: hi\r\n\r\nhello";
        let parsed = parse_http_response(raw).unwrap();

        assert_eq!(parsed.code, 200);
        assert_eq!(parsed.message, "OK");
        assert_eq!(parsed.content_type, "text/html");
        assert_eq!(parsed.content_length, 5);
        assert_eq!(parsed.content_encoding, "gzip");
        assert_eq!(parsed.body, b"hello".to_vec());
        assert_eq!(
            parsed
                .headers
                .iter()
                .find(|(name, _)| name == "X-Custom")
                .map(|(_, value)| value.as_str()),
            Some("hi")
        );

        // A multi-word reason phrase is preserved; missing shortcut headers
        // default rather than failing the parse.
        let not_found = parse_http_response(b"HTTP/1.1 404 Not Found\r\n\r\n").unwrap();
        assert_eq!(not_found.code, 404);
        assert_eq!(not_found.message, "Not Found");
        assert_eq!(not_found.content_length, 0);
        assert_eq!(not_found.content_type, "");

        // A malformed status line is rejected.
        assert!(parse_http_response(b"garbage\r\n\r\n").is_none());
    }

    #[test]
    fn close_process_drops_http_objects_and_returns_their_sockets() {
        let mut state = NetworkState::default();

        let generation = state.begin_connect(0x1111, 0x2222).unwrap();
        assert!(state.finish_connect(generation).is_some());

        // MC_netHttpOpen registers the object's stream socket in `sockets`.
        state.register_socket(9, 1, 0);
        let handle = state.http_alloc(HttpObject {
            socket: 9,
            ..Default::default()
        });
        assert!(state.http_get(handle).is_some());

        let sockets = state.close_process();
        assert_eq!(sockets, vec![9]);
        assert!(state.http_get(handle).is_none());
    }
    #[test]
    fn socket_billing_mode_metadata_tracks_native_modes() {
        let mut state = NetworkState::default();

        state.register_socket(10, 1, 0);
        state.register_socket(11, 1, 1);
        state.register_socket(12, 1, 2);

        assert_eq!(state.socket_type(10), Some(1));
        assert_eq!(state.billing_mode(10), Some(0));
        assert_eq!(state.billing_mode(11), Some(1));
        assert_eq!(state.billing_mode(12), Some(2));

        state.remove_socket(11);
        assert_eq!(state.billing_mode(11), None);
    }

    #[test]
    fn restored_bill_socket_metadata_is_stream_mode_one() {
        let mut state = NetworkState::default();

        // Successful restored MC_netBillSocket registration mirrors native:
        // socket type remains the caller-provided stream type and +0x18 = 1.
        state.register_socket(31, 1, 1);

        assert_eq!(state.socket_type(31), Some(1));
        assert_eq!(state.billing_mode(31), Some(1));

        state.remove_socket(31);
        assert_eq!(state.socket_type(31), None);
        assert_eq!(state.billing_mode(31), None);
    }

    #[test]
    fn billing_header_tracks_native_setheader_inputs() {
        let mut state = NetworkState::default();

        assert_eq!(state.billing_header(), None);

        state.register_socket(41, 1, 1);

        let mut header = [0u8; LGT_BILL_HEADER_SIZE];
        header[0x52..0x54].copy_from_slice(&0x5566u16.to_le_bytes());
        header[0x54..0x58].copy_from_slice(&0x1122_3344u32.to_le_bytes());

        state.install_billing_header(header);

        assert_eq!(state.billing_header(), Some(header));

        // Native s_BillHeader is module-global, so removing a socket object
        // does not destroy the header.
        state.remove_socket(41);
        assert_eq!(state.billing_header(), Some(header));
    }

    #[test]
    fn test_bill_gateway_matches_native_mode_two_constant() {
        let (host, port, path) = parse_http_url(LGT_TEST_BILL_GATEWAY);

        assert_eq!(host, "wipigwdev.ez-i.co.kr");
        assert_eq!(port, 30000);
        assert_eq!(path, "/");

        // WPBill_SetGW calls MC_utilHtons before dsocket_connect.
        assert_eq!(port.swap_bytes(), 0x3075);
    }

    #[test]
    fn bill_header_matches_native_setheader_final_bytes() {
        let platform = test_utils::TestPlatform::new()
            .with_system_information(LGT_BILL_INFO_PHONE_MODEL, "MODEL-X")
            .with_system_information(LGT_BILL_INFO_MDN, "0101234567")
            .with_system_information(LGT_BILL_INFO_CURRENT_CH, "7")
            .with_system_information(LGT_BILL_INFO_SID, "1234")
            .with_system_information(LGT_BILL_INFO_NID, "5678")
            .with_system_information(LGT_BILL_INFO_BASE_ID, "BASE")
            .with_system_information(LGT_BILL_INFO_BEST_PN, "BEST");

        let header = build_lgt_bill_header(&platform, "000298AD", 0x11223344, 0x01020304, 0x3075);

        assert_eq!(header.len(), 108);
        assert_eq!(&header[0x00..0x04], &[0, 0, 0, 0]);

        // Initial:
        //   /android/000298AD.jar:binary.mod
        //
        // Native then strcpy()s fixed strings at +0x0e and +0x18.
        assert_eq!(&header[0x04..0x0e], b"/android/0");
        assert_eq!(&header[0x0e..0x14], b"1.1.1\0");
        assert_eq!(&header[0x14..0x18], b"D.ja");
        assert_eq!(&header[0x18..0x1d], b"1.54\0");
        assert_eq!(&header[0x1d..0x22], b"ary.m");

        assert_eq!(&header[0x22..0x2a], b"MODEL-X\0");

        // 10-digit MDN:
        // first 3 + "00" + remaining 7 = 12 bytes.
        assert_eq!(&header[0x2c..0x39], b"010001234567\0");

        assert_eq!(&header[0x3c..0x3e], b"7\0");
        assert_eq!(&header[0x3e..0x43], b"1234\0");
        assert_eq!(&header[0x43..0x48], b"5678\0");

        // BESTPN overwrites BASEID.
        assert_eq!(&header[0x48..0x4d], b"BEST\0");

        // Original WIPI destination words.
        assert_eq!(&header[0x52..0x54], &[0x75, 0x30]);
        assert_eq!(&header[0x54..0x58], &[0x04, 0x03, 0x02, 0x01]);

        // Htonl(0x11223344) followed by little-endian ARM STR.
        assert_eq!(&header[0x58..0x5c], &[0x11, 0x22, 0x33, 0x44]);

        assert_eq!(&header[0x5c..0x6c], &[0; 16]);
    }

    #[test]
    fn bill_header_mdn_normalization_matches_native_lengths() {
        assert_eq!(normalize_lgt_bill_mdn(b"0101234567"), b"010001234567");

        assert_eq!(normalize_lgt_bill_mdn(b"01012345678"), b"010012345678");

        assert_eq!(normalize_lgt_bill_mdn(b"1234"), b"1234");
    }

    #[test]
    fn billing_header_is_shared_across_native_billing_sockets() {
        let mut state = NetworkState::default();

        state.register_socket(81, 1, 1);
        state.register_socket(82, 1, 2);

        let first = [0x11u8; LGT_BILL_HEADER_SIZE];
        let second = [0x22u8; LGT_BILL_HEADER_SIZE];

        state.install_billing_header(first);
        assert_eq!(state.billing_header(), Some(first));

        // A Write through any billing socket mutates the same native global.
        state.update_billing_header(second);
        assert_eq!(state.billing_header(), Some(second));

        state.remove_socket(81);
        state.remove_socket(82);

        // Socket lifetime does not own s_BillHeader.
        assert_eq!(state.billing_header(), Some(second));
    }

    #[test]
    fn billing_write_header_update_preserves_native_read_state() {
        let mut state = NetworkState::default();
        state.register_socket(72, 1, 1);

        state.billing_read.remaining_payload = 123;
        state.billing_read.header_offset = 17;
        state.billing_read.remaining_header = 39;

        state.update_billing_header([0x5au8; LGT_BILL_HEADER_SIZE]);

        assert_eq!(state.billing_read.remaining_payload, 123);
        assert_eq!(state.billing_read.header_offset, 17);
        assert_eq!(state.billing_read.remaining_header, 39);

        assert_eq!(state.billing_header(), Some([0x5au8; LGT_BILL_HEADER_SIZE]));
    }

    #[test]
    fn billing_read_state_matches_native_initial_and_setheader_reset() {
        let initial = BillingReadState::default();

        assert_eq!(initial.remaining_payload, 0);
        assert_eq!(initial.header_offset, 0);
        assert_eq!(initial.remaining_header, LGT_BILL_READ_HEADER_SIZE);

        let mut state = NetworkState::default();
        state.register_socket(71, 1, 1);

        state.billing_read.remaining_payload = 123;
        state.billing_read.header_offset = 17;
        state.billing_read.remaining_header = 39;

        state.install_billing_header([0u8; LGT_BILL_HEADER_SIZE]);

        assert_eq!(state.billing_read.remaining_payload, 0);
        assert_eq!(state.billing_read.header_offset, 0);
        assert_eq!(state.billing_read.remaining_header, LGT_BILL_READ_HEADER_SIZE);
    }

    #[test]
    fn bill_read_header_layout_decodes_native_payload_length_and_tag() {
        let mut header = [0u8; LGT_BILL_READ_HEADER_SIZE];

        header[0x30..0x34].copy_from_slice(&0x1234_5678u32.to_be_bytes());
        header[0x34..0x38].copy_from_slice(b"KCBD");

        assert_eq!(lgt_bill_read_payload_length(&header), 0x1234_5678);
        assert_eq!(lgt_bill_read_tag(&header), *b"KCBD");
    }

    #[test]
    fn bill_read_native_tag_chain_has_no_reachable_literal() {
        for tag in [
            *b"KCBD", *b"KCSP", *b"KCNB", *b"KCEP", *b"KCEN", *b"UATO", *b"UATT", *b"UAFO", *b"UAFT", *b"UAFR",
        ] {
            assert!(!lgt_bill_read_reject_tag(tag));
        }
    }

    #[test]
    fn bill_write_frame_matches_native_header_and_payload_layout() {
        let mut header = [0xa5u8; LGT_BILL_HEADER_SIZE];

        // SetHeader owns bytes +4 onward. Offset zero is replaced by Write.
        header[0..4].fill(0);

        let payload = b"ABC";
        let (updated, frame) = build_lgt_bill_write_frame(header, payload);

        // 108 + 3 = 111 = 0x0000006f, in network byte order on wire.
        assert_eq!(&updated[0..4], &[0x00, 0x00, 0x00, 0x6f]);

        assert_eq!(frame.len(), 111);
        assert_eq!(&frame[0..4], &[0x00, 0x00, 0x00, 0x6f]);
        assert_eq!(&frame[4..LGT_BILL_HEADER_SIZE], &[0xa5; LGT_BILL_HEADER_SIZE - 4]);
        assert_eq!(&frame[LGT_BILL_HEADER_SIZE..], b"ABC");
    }

    #[test]
    fn bill_write_public_result_matches_native_header_accounting() {
        assert_eq!(lgt_bill_write_public_result(0), 0);
        assert_eq!(lgt_bill_write_public_result(1), 0);
        assert_eq!(lgt_bill_write_public_result(LGT_BILL_HEADER_SIZE - 1), 0);
        assert_eq!(lgt_bill_write_public_result(LGT_BILL_HEADER_SIZE), 0);
        assert_eq!(lgt_bill_write_public_result(LGT_BILL_HEADER_SIZE + 1), 1);
        assert_eq!(lgt_bill_write_public_result(LGT_BILL_HEADER_SIZE + 37), 37);
    }

    #[test]
    fn bill_write_frame_replaces_previous_length_each_call() {
        let mut header = [0u8; LGT_BILL_HEADER_SIZE];
        header[0..4].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);

        let (first, _) = build_lgt_bill_write_frame(header, b"12345");
        assert_eq!(&first[0..4], &[0x00, 0x00, 0x00, 0x71]);

        let (second, _) = build_lgt_bill_write_frame(first, b"Z");
        assert_eq!(&second[0..4], &[0x00, 0x00, 0x00, 0x6d]);
    }

    #[test]
    fn local_lgt_bill_connect_reuses_success_callback_contract() {
        let mut state = NetworkState::default();
        state.register_socket(21, 1, 1);

        state.set_connect_callback(21, 0x1234, 0x5678);
        assert!(!state.connect_is_pending(21));

        state.set_connect_pending(21, true);

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Connected(21)),
            Some((0x1234, [21, 0, 0x5678]))
        );

        assert!(!state.connect_is_pending(21));
        assert_eq!(state.take_callback_for_event(wie_backend::NetworkEvent::Connected(21)), None);
    }

    #[test]
    fn local_lgt_bill_connect_pending_recall_keeps_native_minus_seven_state() {
        let mut state = NetworkState::default();
        state.register_socket(22, 1, 1);

        state.set_connect_callback(22, 0x1111, 0x2222);
        state.set_connect_pending(22, true);

        assert!(state.connect_is_pending(22));

        // socket_connect stores a replacement callback before testing
        // connect_pending, exactly like the native path.
        state.set_connect_callback(22, 0x3333, 0x4444);

        assert_eq!(
            state.take_callback_for_event(wie_backend::NetworkEvent::Connected(22)),
            Some((0x3333, [22, 0, 0x4444]))
        );
    }

<<<<<<< HEAD
    #[futures_test::test]
    async fn local_lgt_purchase_bill_socket_accepts_native_ten_byte_write_prefix() {
        const REQUEST: u32 = 0x1000;

        let system = System::new(Box::new(LocalBillingTestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);

        let request_prefix = [0xff, 0xff, 0x00, 0x13, 0x00, 0x68, 0x31, 0x32, 0x33, 0x34];

        context.write_bytes(REQUEST, &request_prefix).unwrap();

        let state = context.network_state();
        state.lock().process_state = ProcessNetworkState::Available;

        let socket = bill_socket(&mut context, 2, 1).await.unwrap();
        assert_eq!(socket, 31);
        assert_eq!(state.lock().socket_type(socket), Some(1));
        assert_eq!(state.lock().billing_mode(socket), Some(1));

        assert_eq!(
            socket_write(&mut context, socket, REQUEST, request_prefix.len() as i32,).await.unwrap(),
            request_prefix.len() as i32
        );

        let mut response = [0u8; LGT_LOCAL_PURCHASE_RESPONSE_SIZE];
        assert_eq!(
            state.lock().take_local_billing_response(socket, &mut response),
            Some(LGT_LOCAL_PURCHASE_RESPONSE_SIZE)
        );
        assert_eq!(response, [0xff, 0xff, 0x00, 0x07, 0x00, 0x69, 0x00,]);
    }

    #[futures_test::test]
    async fn local_lgt_purchase_bill_socket_to_write_real_path_returns_full_length_and_queues_success() {
        const REQUEST: u32 = 0x1000;

        let system = System::new(Box::new(LocalBillingTestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);

        let request = [
            0xff, 0xff, 0x00, 0x13, 0x00, 0x68, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x30, 0x31, 0x02, 0x03,
        ];

        context.write_bytes(REQUEST, &request).unwrap();

        let state = context.network_state();
        state.lock().process_state = ProcessNetworkState::Available;

        let socket = bill_socket(&mut context, 2, 1).await.unwrap();
        assert_eq!(socket, 31);
        assert_eq!(state.lock().socket_type(socket), Some(1));
        assert_eq!(state.lock().billing_mode(socket), Some(1));

        assert_eq!(
            socket_write(&mut context, socket, REQUEST, request.len() as i32,).await.unwrap(),
            request.len() as i32
        );

        let mut response = [0u8; LGT_LOCAL_PURCHASE_RESPONSE_SIZE];
        assert_eq!(
            state.lock().take_local_billing_response(socket, &mut response),
            Some(LGT_LOCAL_PURCHASE_RESPONSE_SIZE)
        );
        assert_eq!(response, [0xff, 0xff, 0x00, 0x07, 0x00, 0x69, 0x00,]);
    }

    #[test]
    fn local_lgt_purchase_accepts_header_complete_native_write_prefix() {
        // Red Gem declares a 19-byte 0x68 application frame, while its
        // native write wrapper passes only the first 10 bytes.
        let request_prefix = [0xff, 0xff, 0x00, 0x13, 0x00, 0x68, 0x31, 0x32, 0x33, 0x34];

        assert_eq!(
            lgt_local_purchase_success_response(&request_prefix),
            Some([0xff, 0xff, 0x00, 0x07, 0x00, 0x69, 0x00,])
        );
    }

=======
>>>>>>> parent of 54b34ba (Accept native LGT billing write prefixes)
    #[test]
    fn local_lgt_purchase_68_builds_69_status_zero_response() {
        let request = [
            0xff, 0xff, 0x00, 0x13, 0x00, 0x68, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x30, 0x31, 0x02, 0x03,
        ];

        assert_eq!(
            lgt_local_purchase_success_response(&request),
            Some([0xff, 0xff, 0x00, 0x07, 0x00, 0x69, 0x00,])
        );
    }

    #[test]
    fn local_lgt_purchase_does_not_intercept_other_operations() {
        let catalog = [0xff, 0xff, 0x00, 0x07, 0x00, 0x66, 0x00];

        let gift_or_other = [0xff, 0xff, 0x00, 0x07, 0x00, 0x6a, 0x00];

        assert_eq!(lgt_local_purchase_success_response(&catalog), None);
        assert_eq!(lgt_local_purchase_success_response(&gift_or_other), None);
    }

    #[test]
<<<<<<< HEAD
    fn local_lgt_purchase_rejects_write_longer_than_declared_frame() {
        let malformed = [0xff, 0xff, 0x00, 0x07, 0x00, 0x68, 0x00, 0x00];

        assert_eq!(lgt_local_purchase_success_response(&malformed), None);
    }

    #[test]
=======
>>>>>>> parent of 54b34ba (Accept native LGT billing write prefixes)
    fn local_lgt_purchase_rejects_malformed_length() {
        let malformed = [0xff, 0xff, 0x00, 0x02, 0x00, 0x68, 0x00];

        assert_eq!(lgt_local_purchase_success_response(&malformed), None);
    }

    #[test]
    fn local_lgt_purchase_response_queue_supports_partial_reads() {
        let mut state = NetworkState::default();
        state.register_socket(7, 1, 1);

        state.queue_local_billing_response(7, [0xff, 0xff, 0x00, 0x07, 0x00, 0x69, 0x00]);

        let mut first = [0u8; 3];
        assert_eq!(state.take_local_billing_response(7, &mut first), Some(3));
        assert_eq!(first, [0xff, 0xff, 0x00]);

        let mut second = [0u8; 8];
        assert_eq!(state.take_local_billing_response(7, &mut second), Some(4));
        assert_eq!(&second[..4], &[0x07, 0x00, 0x69, 0x00]);

        assert_eq!(state.take_local_billing_response(7, &mut second), None);
    }
}
