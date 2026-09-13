//! An endpoint that listens and says nothing.
//!
//! Writing a server for a title means knowing what the title says to one, and
//! nothing here records that today: a connection to a switched-off service
//! fails at `connect`, so the title never gets as far as sending its first
//! request.
//!
//! This endpoint takes the connection so the title does send it, and writes
//! every byte to the trace. It answers nothing, so a title waiting for a reply
//! waits - which is why a host registers it deliberately, for reading a
//! protocol, and not on an ordinary run.

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};

use super::{LocalConnection, LocalEndpoint, LocalRead};

/// The address a [`CaptureEndpoint`] answers for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureAddress {
    /// Every stream connection the title opens.
    Any,
    /// One host and port, as the title spells them.
    HostPort(String, u16),
}

impl CaptureAddress {
    /// Whether this address covers `host:port`.
    pub fn matches_address(&self, host: &str, port: u16) -> bool {
        match self {
            Self::Any => true,
            Self::HostPort(wanted_host, wanted_port) => host == wanted_host && port == *wanted_port,
        }
    }
}

/// Records what a title sends its server.
pub struct CaptureEndpoint {
    address: CaptureAddress,
    name: String,
}

impl CaptureEndpoint {
    pub fn new(address: CaptureAddress) -> Self {
        let name = match &address {
            CaptureAddress::Any => "capture(any)".to_string(),
            CaptureAddress::HostPort(host, port) => format!("capture({host}:{port})"),
        };

        Self { address, name }
    }

    /// Reads `WIE_LOCAL_NET_CAPTURE` the way a host would: unset or empty for
    /// no capture, `1`/`any` for every connection, `host:port` for one.
    ///
    /// Parsing lives here rather than in each host so they all spell the
    /// setting the same way.
    pub fn from_setting(setting: Option<&str>) -> Option<Self> {
        let setting = setting?.trim();

        if setting.is_empty() || setting == "0" {
            return None;
        }

        if setting == "1" || setting.eq_ignore_ascii_case("any") {
            return Some(Self::new(CaptureAddress::Any));
        }

        let (host, port) = setting.rsplit_once(':')?;
        let port = port.parse().ok()?;
        if host.is_empty() {
            return None;
        }

        Some(Self::new(CaptureAddress::HostPort(host.into(), port)))
    }
}

impl LocalEndpoint for CaptureEndpoint {
    fn name(&self) -> &str {
        &self.name
    }

    fn accepts(&self, scheme: &str, host: &str, port: u16) -> bool {
        // Stream connections only. The billing gateway has an answer of its
        // own and is not something to sit silently on.
        scheme == "socket" && self.address.matches_address(host, port)
    }

    fn open(&self, _: &str, host: &str, port: u16) -> Box<dyn LocalConnection> {
        Box::new(CaptureConnection {
            peer: format!("{host}:{port}"),
            written: 0,
        })
    }
}

struct CaptureConnection {
    peer: String,
    written: usize,
}

impl LocalConnection for CaptureConnection {
    fn write(&mut self, bytes: &[u8]) {
        tracing::info!("capture {} +{:#06x} {} bytes\n{}", self.peer, self.written, bytes.len(), hex_dump(bytes));

        self.written += bytes.len();
    }

    fn read(&mut self, _: &mut [u8]) -> LocalRead {
        LocalRead::Pending
    }

    fn close(&mut self) {
        tracing::info!("capture {} closed after {} bytes", self.peer, self.written);
    }
}

/// Sixteen bytes to a line, hex then printable ASCII - the shape every other
/// dump in this project reads in.
fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();

    for (index, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
        let text: String = chunk
            .iter()
            .map(|&byte| if (0x20..0x7f).contains(&byte) { char::from(byte) } else { '.' })
            .collect();

        out.push_str(&format!("  {:04x}  {:<47}  {text}\n", index * 16, hex.join(" ")));
    }

    out
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{CaptureAddress, CaptureEndpoint, hex_dump};
    use crate::local_network::{LocalEndpoint, LocalRead};

    #[test]
    fn a_setting_says_which_connections_to_record() {
        assert_eq!(CaptureEndpoint::from_setting(None).map(|x| x.name().to_string()), None);
        assert_eq!(CaptureEndpoint::from_setting(Some("")).map(|x| x.name().to_string()), None);
        assert_eq!(CaptureEndpoint::from_setting(Some("0")).map(|x| x.name().to_string()), None);

        assert_eq!(
            CaptureEndpoint::from_setting(Some("1")).map(|x| x.name().to_string()),
            Some("capture(any)".to_string())
        );
        assert_eq!(
            CaptureEndpoint::from_setting(Some("ANY")).map(|x| x.name().to_string()),
            Some("capture(any)".to_string())
        );
        assert_eq!(
            CaptureEndpoint::from_setting(Some("210.222.18.25:31000")).map(|x| x.name().to_string()),
            Some("capture(210.222.18.25:31000)".to_string())
        );

        // Nothing that names no port is an address to sit on.
        assert!(CaptureEndpoint::from_setting(Some("210.222.18.25")).is_none());
        assert!(CaptureEndpoint::from_setting(Some(":31000")).is_none());
        assert!(CaptureEndpoint::from_setting(Some("host:notaport")).is_none());
    }

    #[test]
    fn only_the_named_address_is_recorded() {
        let endpoint = CaptureEndpoint::new(CaptureAddress::HostPort("210.222.18.25".into(), 31000));

        assert!(endpoint.accepts("socket", "210.222.18.25", 31000));
        assert!(!endpoint.accepts("socket", "210.222.18.25", 31001));
        assert!(!endpoint.accepts("socket", "10.0.0.1", 31000));
    }

    #[test]
    fn the_billing_gateway_keeps_its_own_answer() {
        let endpoint = CaptureEndpoint::new(CaptureAddress::Any);

        assert!(endpoint.accepts("socket", "anywhere", 1));
        assert!(!endpoint.accepts("billsocket", "anywhere", 1));
    }

    #[test]
    fn a_recorded_connection_never_answers() {
        let endpoint = CaptureEndpoint::new(CaptureAddress::Any);
        let mut connection = endpoint.open("socket", "host", 1);

        connection.write(b"login");

        // The title waits, as it would on a peer that has not replied.
        assert_eq!(connection.read(&mut [0u8; 8]), LocalRead::Pending);
    }

    #[test]
    fn a_dump_shows_the_bytes_and_what_they_spell() {
        let dump = hex_dump(b"OZ\x00\x01");

        assert!(dump.contains("4f 5a 00 01"), "{dump}");
        assert!(dump.contains("OZ.."), "{dump}");
    }

    #[test]
    fn a_dump_runs_sixteen_bytes_to_the_line() {
        let dump = hex_dump(&[0u8; 20]);

        assert_eq!(dump.lines().count(), 2);
        assert!(dump.contains("0000  "), "{dump}");
        assert!(dump.contains("0010  "), "{dump}");
    }
}
