use alloc::{collections::BTreeMap, sync::Arc};

use spin::Mutex;
use wipi_types::wipic::WIPICWord;
use wie_util::{Result, read_null_terminated_string_bytes};

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

        self.entries.insert(handle, SerialEntry { port_id, config });
        handle
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
        value = value
            .wrapping_mul(10)
            .wrapping_add((byte - b'0') as i32);
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
pub async fn open(
    context: &mut dyn WIPICContext,
    port_id: i32,
    config_string: WIPICWord,
) -> Result<i32> {
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

#[cfg(test)]
mod tests {
    use crate::{WIPICContext, context::test::TestContext};

    use super::{SerialConfig, open, parse_config};

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
}
