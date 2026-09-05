use alloc::{collections::BTreeMap, sync::Arc};

use spin::Mutex;
use wie_util::{Result, read_null_terminated_string_bytes};
use wipi_types::wipic::WIPICWord;

use crate::WIPICContext;

const MAX_SERIAL_PORTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SerialConfig {
    baudrate: i32,
    parity: i32,
    reserved2: i32,
    size: i32,
    flow: i32,
    reserved5: i32,
    reserved6: i32,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baudrate: 115_200,
            parity: 0,
            reserved2: 0,
            size: 8,
            flow: 0,
            reserved5: 0,
            reserved6: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SerialEntry {
    #[allow(dead_code)]
    port_id: i32,
    #[allow(dead_code)]
    config: SerialConfig,
    write_callback: WIPICWord,
    write_context: WIPICWord,
}

pub struct SerialState {
    next_handle: i32,
    entries: BTreeMap<i32, SerialEntry>,
}

impl Default for SerialState {
    fn default() -> Self {
        Self {
            // Native dsio `the_fd_seq` is initialized to 1 and monotonically
            // advances when a newly allocated port record receives an fd.
            next_handle: 1,
            entries: BTreeMap::new(),
        }
    }
}

pub type SharedSerialState = Arc<Mutex<SerialState>>;

pub fn new_state() -> SharedSerialState {
    Arc::new(Mutex::new(SerialState::default()))
}

impl SerialState {
    fn open(&mut self, port_id: i32, config: SerialConfig) -> i32 {
        // Native dsio has MAXSERIALNUM port records. The LGT reference reports
        // MAXSERIALNUM="4"; exhaustion in dsio_open is -2024, which
        // MC_srlOpen does not specially map and therefore returns generic -1.
        if self.entries.len() >= MAX_SERIAL_PORTS {
            return -1;
        }

        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        if self.next_handle <= 0 {
            self.next_handle = 1;
        }

        self.entries.insert(
            handle,
            SerialEntry {
                port_id,
                config,
                write_callback: 0,
                write_context: 0,
            },
        );
        handle
    }

    fn set_write_callback(&mut self, handle: i32, callback: WIPICWord, callback_context: WIPICWord) -> i32 {
        if handle < 0 {
            return -2;
        }

        let Some(entry) = self.entries.get_mut(&handle) else {
            return -1;
        };

        // Native serial object:
        //   +0x08 write callback
        //   +0x10 write callback context
        entry.write_callback = callback;
        entry.write_context = callback_context;
        0
    }

    #[cfg(test)]
    fn write_callback(&self, handle: i32) -> Option<(WIPICWord, WIPICWord)> {
        self.entries.get(&handle).map(|entry| (entry.write_callback, entry.write_context))
    }

    fn contains(&self, handle: i32) -> bool {
        self.entries.contains_key(&handle)
    }

    fn close(&mut self, handle: i32) -> i32 {
        if handle < 0 {
            return -2;
        }

        // Native dsio_close returns -2009 when the WIPI-facing fd does not
        // resolve to an active DSIO port. MC_srlClose forwards that value
        // unchanged.
        if self.entries.remove(&handle).is_none() {
            return -2009;
        }

        // In this LGT build MH_serialClose always returns 0, so a valid DSIO
        // port is freed and MC_srlClose subsequently releases its 28-byte
        // serial object.
        0
    }
}

fn atoi(bytes: &[u8]) -> i32 {
    let mut index = 0usize;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    let negative = bytes.get(index) == Some(&b'-');
    if negative || bytes.get(index) == Some(&b'+') {
        index += 1;
    }

    let mut value = 0i32;
    while let Some(&byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value.wrapping_mul(10).wrapping_add((byte - b'0') as i32);
        index += 1;
    }

    if negative { value.wrapping_neg() } else { value }
}

fn parse_config(bytes: &[u8]) -> SerialConfig {
    let mut config = SerialConfig::default();
    let mut rest = bytes;

    loop {
        // Native MC_srlOpen searches for both '=' and ',' before parsing the
        // current option. If either is absent it immediately opens with the
        // config accumulated so far, so a final option without a trailing
        // comma is deliberately left unparsed.
        let Some(eq) = rest.iter().position(|&byte| byte == b'=') else {
            break;
        };
        let Some(comma) = rest.iter().position(|&byte| byte == b',') else {
            break;
        };
        if eq >= comma {
            break;
        }

        let key = &rest[..eq];
        let value = &rest[eq + 1..comma];

        match key {
            b"baudrate" => config.baudrate = atoi(value),
            b"parity" => {
                config.parity = if value == b"no" {
                    0
                } else if value == b"odd" {
                    1
                } else {
                    2
                };
            }
            b"flow" => {
                config.flow = if value == b"no" {
                    0
                } else if value == b"hardware" {
                    1
                } else {
                    2
                };
            }
            b"size" => config.size = atoi(value),
            _ => {}
        }

        rest = &rest[comma + 1..];
    }

    config
}

/// LGT MC_srlOpen.
///
/// Native contract established from liblgt_system.so:
/// - `port_id` is forwarded unchanged to the serial device implementation.
/// - `config_string` is a NUL-terminated comma-separated option string.
/// - defaults are 115200 / parity=no / size=8 / flow=no.
/// - parity maps no/odd/other to 0/1/2.
/// - flow maps no/hardware/other to 0/1/2.
/// - the handset HAL open routine itself returns success unconditionally.
/// - logical handles start at 1 and at most four serial records can coexist.
pub async fn open(context: &mut dyn WIPICContext, port_id: i32, config_string: WIPICWord) -> Result<i32> {
    let bytes = read_null_terminated_string_bytes(context, config_string)?;
    let config = parse_config(&bytes);

    tracing::debug!(
        "MC_srlOpen({port_id}, {config_string:#x}) baudrate={} parity={} size={} flow={}",
        config.baudrate,
        config.parity,
        config.size,
        config.flow,
    );

    Ok(context.serial_state().lock().open(port_id, config))
}

/// LGT MC_srlWrite.
///
/// The native function checks a null buffer before rejecting the handle.
/// For a non-null buffer, both negative and unknown handles return -2.
/// The LGT handset HAL `MH_serialWrite` unconditionally returns 0, so a
/// valid logical handle returns 0 without inspecting the buffer contents or
/// validating the signed length.
///
/// The native wrapper also has a dormant asynchronous path: dsio -2011 sets
/// the serial object's pending-write bit and returns -19. That path is not
/// reachable with this LGT HAL because its write primitive always succeeds.
pub async fn write(context: &mut dyn WIPICContext, handle: i32, buffer: WIPICWord, length: i32) -> Result<i32> {
    tracing::debug!("MC_srlWrite({handle}, {buffer:#x}, {length})");

    if buffer == 0 {
        return Ok(-9);
    }

    if handle < 0 || !context.serial_state().lock().contains(handle) {
        return Ok(-2);
    }

    Ok(0)
}

/// LGT MC_srlSetWriteCB.
///
/// Native behavior:
/// - handle < 0 returns -2 without looking up the serial object.
/// - a non-negative handle not present in the four-entry object table returns -1.
/// - a valid handle stores the callback at object +0x08 and its context at
///   object +0x10, then returns 0.
/// Callback and context values themselves are not validated, so zero clears
/// either stored value just like the native implementation.
pub async fn set_write_callback(context: &mut dyn WIPICContext, handle: i32, callback: WIPICWord, callback_context: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_srlSetWriteCB({handle}, {callback:#x}, {callback_context:#x})");

    Ok(context.serial_state().lock().set_write_callback(handle, callback, callback_context))
}

/// LGT MC_srlClose.
///
/// Native contract:
/// - a negative handle is rejected immediately with -2.
/// - dsio_close is called with a 50000 tick timeout for every non-negative
///   handle.
/// - an unknown/already-closed handle yields dsio -2009, which MC_srlClose
///   returns unchanged.
/// - MH_serialClose is an unconditional-success stub in this LGT runtime, so
///   a valid handle is freed from DSIO and its WIPI serial object is released.
/// - the global DSIO fd sequence is not rewound by close, so later opens keep
///   allocating monotonically increasing handles rather than reusing this one.
pub async fn close(context: &mut dyn WIPICContext, handle: i32) -> Result<i32> {
    tracing::debug!("MC_srlClose({handle})");
    Ok(context.serial_state().lock().close(handle))
}

#[cfg(test)]
mod tests {
    use crate::{WIPICContext, context::test::TestContext};

    use super::{SerialConfig, close, open, parse_config, set_write_callback, write};

    #[test]
    fn lgt_serial_parser_matches_native_defaults_and_options() {
        assert_eq!(parse_config(b""), SerialConfig::default());

        assert_eq!(
            parse_config(b"baudrate=57600,parity=odd,size=7,flow=hardware,"),
            SerialConfig {
                baudrate: 57_600,
                parity: 1,
                reserved2: 0,
                size: 7,
                flow: 1,
                reserved5: 0,
                reserved6: 0,
            }
        );

        assert_eq!(
            parse_config(b"parity=even,flow=software,"),
            SerialConfig {
                parity: 2,
                flow: 2,
                ..SerialConfig::default()
            }
        );

        // Native compares only "no"/"odd" for parity and "no"/"hardware"
        // for flow, so every other parsed value selects enum value 2.
        assert_eq!(
            parse_config(b"parity=anything,flow=anything,"),
            SerialConfig {
                parity: 2,
                flow: 2,
                ..SerialConfig::default()
            }
        );

        // The native loop requires a comma after the current option before
        // processing it. The final comma-less field is therefore ignored.
        assert_eq!(
            parse_config(b"baudrate=57600,parity=odd,size=7,flow=hardware"),
            SerialConfig {
                baudrate: 57_600,
                parity: 1,
                reserved2: 0,
                size: 7,
                flow: 0,
                reserved5: 0,
                reserved6: 0,
            }
        );
    }

    #[futures_test::test]
    async fn lgt_serial_open_allocates_native_style_handles_and_capacity() {
        let mut context = TestContext::new();
        let config = b"baudrate=115200,parity=no,size=8,flow=hardware\0";
        let ptr = context.alloc_raw(config.len() as u32).unwrap();
        wie_util::ByteWrite::write_bytes(&mut context, ptr, config).unwrap();

        assert_eq!(open(&mut context, 0, ptr).await.unwrap(), 1);
        assert_eq!(open(&mut context, 0, ptr).await.unwrap(), 2);
        assert_eq!(open(&mut context, 0, ptr).await.unwrap(), 3);
        assert_eq!(open(&mut context, 0, ptr).await.unwrap(), 4);

        // dsio_open returns -2024 when its four records are occupied.
        // MC_srlOpen has no dedicated mapping for -2024, so it returns -1.
        assert_eq!(open(&mut context, 0, ptr).await.unwrap(), -1);
    }

    #[futures_test::test]
    async fn lgt_serial_write_matches_native_hal_stub_and_error_order() {
        let mut context = TestContext::new();

        // MC_srlWrite checks NULL buffer before handle validity.
        assert_eq!(write(&mut context, -1, 0, 123).await.unwrap(), -9);
        assert_eq!(write(&mut context, 77, 0, 123).await.unwrap(), -9);

        // With a non-null buffer, negative and unknown handles map to -2.
        assert_eq!(write(&mut context, -1, 0x1234, 123).await.unwrap(), -2);
        assert_eq!(write(&mut context, 77, 0x1234, 123).await.unwrap(), -2);

        let empty_config = context.alloc_raw(1).unwrap();
        wie_util::ByteWrite::write_bytes(&mut context, empty_config, &[0]).unwrap();
        let handle = open(&mut context, 0, empty_config).await.unwrap();
        assert_eq!(handle, 1);

        // MH_serialWrite ignores buffer contents and length and returns 0.
        // Therefore a valid logical handle does not dereference the buffer
        // and even unusual signed lengths remain successful in this LGT build.
        assert_eq!(write(&mut context, handle, 0x1234, 0).await.unwrap(), 0);
        assert_eq!(write(&mut context, handle, 0x1234, -1).await.unwrap(), 0);
        assert_eq!(write(&mut context, handle, 0x1234, i32::MAX).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_serial_set_write_callback_matches_native_object_updates() {
        let mut context = TestContext::new();

        assert_eq!(set_write_callback(&mut context, -1, 0x1111, 0x2222).await.unwrap(), -2);
        assert_eq!(set_write_callback(&mut context, 77, 0x1111, 0x2222).await.unwrap(), -1);

        let empty_config = context.alloc_raw(1).unwrap();
        wie_util::ByteWrite::write_bytes(&mut context, empty_config, &[0]).unwrap();
        let handle = open(&mut context, 0, empty_config).await.unwrap();
        assert_eq!(handle, 1);

        assert_eq!(set_write_callback(&mut context, handle, 0x1234, 0x5678).await.unwrap(), 0);
        assert_eq!(context.serial_state().lock().write_callback(handle), Some((0x1234, 0x5678)));

        // Native performs no callback/context validation; zero values are
        // stored directly and therefore act as ordinary clears.
        assert_eq!(set_write_callback(&mut context, handle, 0, 0).await.unwrap(), 0);
        assert_eq!(context.serial_state().lock().write_callback(handle), Some((0, 0)));
    }

    #[futures_test::test]
    async fn lgt_serial_close_releases_native_objects_and_preserves_fd_sequence() {
        let mut context = TestContext::new();

        // MC_srlClose itself rejects only negative handles.
        assert_eq!(close(&mut context, -1).await.unwrap(), -2);

        // Non-negative handles proceed into dsio_close; a missing fd is -2009
        // and is returned by MC_srlClose without remapping.
        assert_eq!(close(&mut context, 0).await.unwrap(), -2009);
        assert_eq!(close(&mut context, 77).await.unwrap(), -2009);

        let empty_config = context.alloc_raw(1).unwrap();
        wie_util::ByteWrite::write_bytes(&mut context, empty_config, &[0]).unwrap();

        let first = open(&mut context, 0, empty_config).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(set_write_callback(&mut context, first, 0x1234, 0x5678).await.unwrap(), 0);

        // HAL close succeeds, DSIO frees the port and MC_srlClose releases
        // the corresponding serial object.
        assert_eq!(close(&mut context, first).await.unwrap(), 0);
        assert_eq!(write(&mut context, first, 0x1234, 1).await.unwrap(), -2);
        assert_eq!(set_write_callback(&mut context, first, 0x1111, 0x2222).await.unwrap(), -1);

        // Closing it again reaches dsio_close and reports the missing fd.
        assert_eq!(close(&mut context, first).await.unwrap(), -2009);

        // free_port restores capacity but does not rewind the global fd
        // sequence, so the next logical handle is 2 rather than 1.
        let second = open(&mut context, 0, empty_config).await.unwrap();
        assert_eq!(second, 2);
    }
}
