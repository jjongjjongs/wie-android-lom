//! An endpoint that approves whatever a title asks.
//!
//! These protocols are length-prefixed binary, not text: there is no "OK" to
//! send back, only a frame shaped the way the title's own parser reads. But the
//! shape is the part that generalises - a length, a message type, then a
//! payload - so a reply built from the request itself is a well-formed answer
//! without knowing what the request meant.
//!
//! That is what this does. Every complete frame a title sends is answered with
//! a frame carrying the same message type and a fixed status payload, which is
//! how a server says "granted" in this family of protocols. It is enough to get
//! a title past a handshake, a login or a heartbeat that otherwise never
//! answers; it is not a server, and a title that needs real content back - a
//! world, an inventory, another player - will read the status and find nothing
//! behind it.
//!
//! 오즈-천공의 기사단 measured as `[u32 length][u32 type][payload]`, big endian,
//! the length counting the whole frame, which is [`Framing::default`].

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use super::{LocalConnection, LocalEndpoint, LocalRead, capture::CaptureAddress};

/// How a title's frames are laid out, so a reply can be built to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framing {
    /// Width of the leading length field, in bytes. Two or four.
    pub length_width: usize,
    /// Whether the length - and the message type - are big endian.
    pub big_endian: bool,
    /// Whether the length counts the length field itself.
    pub length_includes_prefix: bool,
    /// Width of the field after the length that names the message.
    pub type_width: usize,
    /// What message the reply is.
    pub message: MessageType,
    /// What every reply carries after its message type.
    pub status: Status,
}

/// The message type a reply carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    /// The request's own, so the title can pair the answer with what it asked.
    Echo,
    /// A counter, one per reply.
    ///
    /// A title polling for an event rather than an answer is waiting for a
    /// message it did not ask for, and which one is not knowable from the
    /// outside. One run of this offers it a different type each time. 오즈-천공의
    /// 기사단 polls one request over and over while its screen says CONNECTING,
    /// and answering that request in kind never satisfies it.
    Sweep,
    /// The same type every time, right-aligned in the type field.
    Fixed(Vec<u8>),
}

/// The payload a reply carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// The same bytes every time. Zero is how this family of protocols spells
    /// "granted".
    Fixed(Vec<u8>),
    /// The request's own payload, handed straight back - which is what a title
    /// polling for a value it supplied may be waiting to see.
    Echo,
    /// A four-byte counter, one per reply.
    ///
    /// A title that keeps repeating a request is waiting for an answer it has
    /// not been given, and there is no way to know which without trying. One
    /// run of this covers as many values as the title asks, and the log says
    /// which one it was on when it stopped.
    Sweep,
}

impl Default for Framing {
    fn default() -> Self {
        Self {
            length_width: 4,
            big_endian: true,
            length_includes_prefix: true,
            type_width: 4,
            message: MessageType::Echo,
            status: Status::Fixed(vec![0, 0, 0, 0]),
        }
    }
}

impl Framing {
    /// The smallest frame this framing can describe: a length and a type.
    fn header_size(&self) -> usize {
        self.length_width + self.type_width
    }

    fn read_length(&self, bytes: &[u8]) -> usize {
        let field = &bytes[..self.length_width];
        let value = field.iter().fold(0usize, |value, &byte| (value << 8) | byte as usize);

        let value = if self.big_endian {
            value
        } else {
            field.iter().rev().fold(0usize, |value, &byte| (value << 8) | byte as usize)
        };

        if self.length_includes_prefix { value } else { value + self.length_width }
    }

    fn write_length(&self, frame: &mut [u8], total: usize) {
        let value = if self.length_includes_prefix { total } else { total - self.length_width };

        for index in 0..self.length_width {
            let shift = 8 * if self.big_endian { self.length_width - 1 - index } else { index };
            frame[index] = (value >> shift) as u8;
        }
    }

    /// Reads a `len=` word from a setting, as `u16le`, `u16be`, `u32le` or
    /// `u32be`.
    fn apply_length_word(&mut self, word: &str) -> Option<()> {
        let (width, order) = word.split_at(word.len().checked_sub(2)?);

        self.length_width = match width {
            "u16" => 2,
            "u32" => 4,
            _ => return None,
        };
        self.big_endian = match order {
            "be" => true,
            "le" => false,
            _ => return None,
        };

        Some(())
    }
}

/// Answers every request a title sends with a frame of the same type.
pub struct AckEndpoint {
    address: CaptureAddress,
    framing: Framing,
    name: String,
}

impl AckEndpoint {
    pub fn new(address: CaptureAddress, framing: Framing) -> Self {
        let name = match &address {
            CaptureAddress::Any => "ack(any)".to_string(),
            CaptureAddress::HostPort(host, port) => format!("ack({host}:{port})"),
        };

        Self { address, framing, name }
    }

    /// Reads `WIE_LOCAL_NET_ACK`: an address - `1`/`any`, or `host:port` - then
    /// any of `len=u32be`, `type=4` and `status=<hex>`, comma separated.
    ///
    /// Parsing lives here rather than in each host so they all spell the
    /// setting the same way.
    pub fn from_setting(setting: Option<&str>) -> Option<Self> {
        let setting = setting?.trim();
        if setting.is_empty() || setting == "0" {
            return None;
        }

        let mut words = setting.split(',').map(str::trim);
        let address = parse_address(words.next()?)?;
        let mut framing = Framing::default();

        for word in words {
            if word.is_empty() {
                continue;
            }

            let (key, value) = word.split_once('=')?;
            match key {
                "len" => framing.apply_length_word(value)?,
                "type" => framing.type_width = value.parse().ok()?,
                "id" => {
                    framing.message = match value {
                        "echo" => MessageType::Echo,
                        "sweep" => MessageType::Sweep,
                        hex => MessageType::Fixed(parse_hex(hex)?),
                    }
                }
                "status" => {
                    framing.status = match value {
                        "echo" => Status::Echo,
                        "sweep" => Status::Sweep,
                        hex => Status::Fixed(parse_hex(hex)?),
                    }
                }
                "prefix" => {
                    framing.length_includes_prefix = match value {
                        "in" => true,
                        "out" => false,
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }

        if framing.length_width != 2 && framing.length_width != 4 {
            return None;
        }

        Some(Self::new(address, framing))
    }
}

fn parse_address(word: &str) -> Option<CaptureAddress> {
    if word == "1" || word.eq_ignore_ascii_case("any") {
        return Some(CaptureAddress::Any);
    }

    let (host, port) = word.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }

    Some(CaptureAddress::HostPort(host.into(), port.parse().ok()?))
}

/// Reads an even-length run of hex digits, with optional separators.
fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<char> = text.chars().filter(|character| !matches!(character, ' ' | '-' | '_' | ':')).collect();
    if digits.len() % 2 != 0 {
        return None;
    }

    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(&pair.iter().collect::<String>(), 16).ok())
        .collect()
}

impl LocalEndpoint for AckEndpoint {
    fn name(&self) -> &str {
        &self.name
    }

    fn accepts(&self, scheme: &str, host: &str, port: u16) -> bool {
        // Stream connections only. The billing gateway has an answer of its own.
        scheme == "socket" && self.address.matches_address(host, port)
    }

    fn open(&self, _: &str, host: &str, port: u16) -> Box<dyn LocalConnection> {
        Box::new(AckConnection {
            peer: format!("{host}:{port}"),
            framing: self.framing.clone(),
            request: Vec::new(),
            reply: Vec::new(),
            answered: 0,
        })
    }
}

struct AckConnection {
    peer: String,
    framing: Framing,
    /// What the title has sent that is not yet a complete frame.
    request: Vec<u8>,
    /// What is left to hand back.
    reply: Vec<u8>,
    /// How many replies have been sent, which is what [`Status::Sweep`] counts.
    answered: u32,
}

impl AckConnection {
    /// Takes one complete frame off the front of `request`, if there is one.
    fn take_frame(&mut self) -> Option<Vec<u8>> {
        if self.request.len() < self.framing.header_size() {
            return None;
        }

        let total = self.framing.read_length(&self.request);

        // A length that cannot describe this frame is not a frame this framing
        // reads. Saying so once is better than answering nonsense forever.
        if total < self.framing.header_size() {
            tracing::warn!(
                "ack {}: a frame of {total} bytes is shorter than the {} its framing needs; dropping {} buffered bytes",
                self.peer,
                self.framing.header_size(),
                self.request.len()
            );
            self.request.clear();
            return None;
        }

        if self.request.len() < total {
            return None;
        }

        Some(self.request.drain(..total).collect())
    }
}

impl LocalConnection for AckConnection {
    fn write(&mut self, bytes: &[u8]) {
        self.request.extend_from_slice(bytes);

        while let Some(frame) = self.take_frame() {
            let status = match &self.framing.status {
                Status::Fixed(status) => status.clone(),
                Status::Echo => frame[self.framing.header_size()..].to_vec(),
                Status::Sweep => self.answered.to_be_bytes().to_vec(),
            };

            let requested_type = &frame[self.framing.length_width..self.framing.header_size()];

            let total = self.framing.header_size() + status.len();
            let mut reply = vec![0u8; total];
            self.framing.write_length(&mut reply, total);
            reply[self.framing.header_size()..].copy_from_slice(&status);

            let message_type = &mut reply[self.framing.length_width..self.framing.header_size()];
            match &self.framing.message {
                MessageType::Echo => message_type.copy_from_slice(requested_type),
                MessageType::Sweep => {
                    let counter = self.answered.to_be_bytes();
                    let taken = message_type.len().min(counter.len());
                    let start = message_type.len() - taken;
                    message_type[start..].copy_from_slice(&counter[counter.len() - taken..]);
                }
                MessageType::Fixed(fixed) => {
                    let taken = message_type.len().min(fixed.len());
                    let start = message_type.len() - taken;
                    message_type[start..].copy_from_slice(&fixed[fixed.len() - taken..]);
                }
            }

            tracing::info!("ack {}: {} -> {}", self.peer, hex(&frame), hex(&reply));

            self.reply.extend_from_slice(&reply);
            self.answered = self.answered.wrapping_add(1);
        }
    }

    fn read(&mut self, out: &mut [u8]) -> LocalRead {
        if self.reply.is_empty() {
            return LocalRead::Pending;
        }

        let taken = out.len().min(self.reply.len());
        out[..taken].copy_from_slice(&self.reply[..taken]);
        self.reply.drain(..taken);

        // Whether a title reads its answer at all is the first thing to know
        // when it keeps asking: a reply nothing collects says the status bytes
        // are not what is holding it up.
        tracing::debug!("ack {}: {} taken, {} left", self.peer, taken, self.reply.len());

        LocalRead::Data(taken)
    }

    fn readable(&self) -> bool {
        !self.reply.is_empty()
    }

    fn close(&mut self) {
        if !self.request.is_empty() {
            tracing::debug!("ack {} closed with {} bytes of an incomplete frame", self.peer, self.request.len());
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec, vec::Vec};

    use super::{AckEndpoint, Framing, MessageType, Status, parse_hex};
    use crate::local_network::{LocalEndpoint, LocalRead, capture::CaptureAddress};

    fn drain(connection: &mut alloc::boxed::Box<dyn crate::local_network::LocalConnection>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buffer = [0u8; 64];

        while let LocalRead::Data(read) = connection.read(&mut buffer) {
            out.extend_from_slice(&buffer[..read]);
        }

        out
    }

    #[test]
    fn a_request_is_answered_with_its_own_message_type() {
        let endpoint = AckEndpoint::new(CaptureAddress::Any, Framing::default());
        let mut connection = endpoint.open("socket", "host", 1);

        // 오즈's handshake, as captured: len 13, type 1, then its payload.
        connection.write(&[0, 0, 0, 13, 0, 0, 0, 1, 0, 0, 3, 0xe8, 0x28]);

        // Length 12, the same type, then the status.
        assert_eq!(drain(&mut connection), vec![0, 0, 0, 12, 0, 0, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn nothing_is_answered_until_a_whole_frame_arrives() {
        let endpoint = AckEndpoint::new(CaptureAddress::Any, Framing::default());
        let mut connection = endpoint.open("socket", "host", 1);

        connection.write(&[0, 0, 0, 13, 0, 0]);
        assert_eq!(connection.read(&mut [0u8; 16]), LocalRead::Pending);

        connection.write(&[0, 1, 0, 0, 3, 0xe8]);
        assert_eq!(connection.read(&mut [0u8; 16]), LocalRead::Pending);

        // The frame is complete only on its last byte.
        connection.write(&[0x28]);
        assert_eq!(drain(&mut connection), vec![0, 0, 0, 12, 0, 0, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn frames_arriving_together_are_each_answered() {
        let endpoint = AckEndpoint::new(CaptureAddress::Any, Framing::default());
        let mut connection = endpoint.open("socket", "host", 1);

        let mut both = vec![0, 0, 0, 12, 0, 0, 0, 1, 0, 0, 0, 0];
        both.extend_from_slice(&[0, 0, 0, 12, 0, 0, 0, 2, 0, 0, 0, 0]);
        connection.write(&both);

        let answers = drain(&mut connection);
        assert_eq!(&answers[4..8], &[0, 0, 0, 1]);
        assert_eq!(&answers[16..20], &[0, 0, 0, 2]);
        assert_eq!(answers.len(), 24);
    }

    #[test]
    fn a_length_too_small_to_be_a_frame_is_dropped_rather_than_answered() {
        let endpoint = AckEndpoint::new(CaptureAddress::Any, Framing::default());
        let mut connection = endpoint.open("socket", "host", 1);

        connection.write(&[0, 0, 0, 2, 0, 0, 0, 1]);

        assert_eq!(connection.read(&mut [0u8; 16]), LocalRead::Pending);
    }

    #[test]
    fn a_little_endian_framing_reads_and_writes_its_own_order() {
        let framing = Framing {
            length_width: 2,
            big_endian: false,
            length_includes_prefix: true,
            type_width: 2,
            message: MessageType::Echo,
            status: Status::Fixed(vec![0xff]),
        };
        let endpoint = AckEndpoint::new(CaptureAddress::Any, framing);
        let mut connection = endpoint.open("socket", "host", 1);

        // Length 6 little endian, type 0x0102, then its two payload bytes.
        connection.write(&[6, 0, 0x02, 0x01, 0x77, 0x88]);

        assert_eq!(drain(&mut connection), vec![5, 0, 0x02, 0x01, 0xff]);
    }

    #[test]
    fn a_length_that_excludes_its_own_field_is_read_and_written_that_way() {
        let framing = Framing {
            length_includes_prefix: false,
            ..Framing::default()
        };
        let endpoint = AckEndpoint::new(CaptureAddress::Any, framing);
        let mut connection = endpoint.open("socket", "host", 1);

        // Nine bytes after the length field: type plus five of payload.
        connection.write(&[0, 0, 0, 9, 0, 0, 0, 1, 1, 2, 3, 4, 5]);

        // Eight after the length field: type plus the four-byte status.
        assert_eq!(drain(&mut connection), vec![0, 0, 0, 8, 0, 0, 0, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn an_echoing_reply_hands_the_request_payload_straight_back() {
        let framing = Framing {
            status: Status::Echo,
            ..Framing::default()
        };
        let endpoint = AckEndpoint::new(CaptureAddress::Any, framing);
        let mut connection = endpoint.open("socket", "host", 1);

        // 오즈's repeated request, whose payload is a u32 ten.
        connection.write(&[0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 10]);

        assert_eq!(drain(&mut connection), vec![0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 10]);
    }

    #[test]
    fn a_sweeping_reply_counts_up_one_per_answer() {
        let framing = Framing {
            status: Status::Sweep,
            ..Framing::default()
        };
        let endpoint = AckEndpoint::new(CaptureAddress::Any, framing);
        let mut connection = endpoint.open("socket", "host", 1);

        for expected in 0..3u32 {
            connection.write(&[0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 10]);

            let reply = drain(&mut connection);
            assert_eq!(&reply[8..], &expected.to_be_bytes());
        }
    }

    #[test]
    fn a_sweeping_reply_offers_a_different_message_type_each_time() {
        let framing = Framing {
            message: MessageType::Sweep,
            ..Framing::default()
        };
        let endpoint = AckEndpoint::new(CaptureAddress::Any, framing);
        let mut connection = endpoint.open("socket", "host", 1);

        // A title polling one request is offered a different message each time,
        // which is the only way to find the one it is waiting for.
        for expected in 0..3u32 {
            connection.write(&[0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 10]);

            let reply = drain(&mut connection);
            assert_eq!(&reply[4..8], &expected.to_be_bytes());
        }
    }

    #[test]
    fn a_fixed_message_type_sits_at_the_end_of_its_field() {
        let framing = Framing {
            message: MessageType::Fixed(vec![0x2a]),
            ..Framing::default()
        };
        let endpoint = AckEndpoint::new(CaptureAddress::Any, framing);
        let mut connection = endpoint.open("socket", "host", 1);

        connection.write(&[0, 0, 0, 12, 0, 0, 0, 7, 0, 0, 0, 10]);

        // The one byte given lands in the low byte of the four-byte field,
        // rather than at its front where it would read as a huge type.
        assert_eq!(&drain(&mut connection)[4..8], &[0, 0, 0, 0x2a]);
    }

    #[test]
    fn a_setting_says_the_address_and_the_framing() {
        assert!(AckEndpoint::from_setting(None).is_none());
        assert!(AckEndpoint::from_setting(Some("0")).is_none());

        let endpoint = AckEndpoint::from_setting(Some("1")).unwrap();
        assert_eq!(endpoint.name(), "ack(any)");
        assert_eq!(endpoint.framing, Framing::default());

        let endpoint = AckEndpoint::from_setting(Some("210.222.18.25:31000")).unwrap();
        assert_eq!(endpoint.name(), "ack(210.222.18.25:31000)");

        let endpoint = AckEndpoint::from_setting(Some("any,len=u16le,type=2,status=00ff,prefix=out")).unwrap();
        assert_eq!(
            endpoint.framing,
            Framing {
                length_width: 2,
                big_endian: false,
                length_includes_prefix: false,
                type_width: 2,
                message: MessageType::Echo,
                status: Status::Fixed(vec![0x00, 0xff]),
            }
        );
    }

    #[test]
    fn a_setting_this_cannot_read_is_refused_rather_than_guessed_at() {
        assert!(AckEndpoint::from_setting(Some("any,len=u24be")).is_none());
        assert!(AckEndpoint::from_setting(Some("any,len=u32me")).is_none());
        assert!(AckEndpoint::from_setting(Some("any,status=abc")).is_none());
        assert_eq!(AckEndpoint::from_setting(Some("any,status=echo")).unwrap().framing.status, Status::Echo);
        assert_eq!(AckEndpoint::from_setting(Some("any,status=sweep")).unwrap().framing.status, Status::Sweep);
        assert_eq!(
            AckEndpoint::from_setting(Some("any,id=sweep")).unwrap().framing.message,
            MessageType::Sweep
        );
        assert_eq!(
            AckEndpoint::from_setting(Some("any,id=0069")).unwrap().framing.message,
            MessageType::Fixed(alloc::vec![0x00, 0x69])
        );
        assert!(AckEndpoint::from_setting(Some("any,unknown=1")).is_none());
        assert!(AckEndpoint::from_setting(Some("any,type=x")).is_none());
        assert!(AckEndpoint::from_setting(Some(":31000")).is_none());
    }

    #[test]
    fn hex_is_read_with_or_without_separators() {
        assert_eq!(parse_hex("00ff"), Some(vec![0, 255]));
        assert_eq!(parse_hex("00 ff"), Some(vec![0, 255]));
        assert_eq!(parse_hex("00-ff"), Some(vec![0, 255]));
        assert_eq!(parse_hex(""), Some(vec![]));

        assert_eq!(parse_hex("0"), None);
        assert_eq!(parse_hex("zz"), None);
    }

    #[test]
    fn the_billing_gateway_keeps_its_own_answer() {
        let endpoint = AckEndpoint::new(CaptureAddress::Any, Framing::default());

        assert!(endpoint.accepts("socket", "anywhere", 1));
        assert!(!endpoint.accepts("billsocket", "anywhere", 1));
    }

    #[test]
    fn only_the_named_address_is_answered() {
        let endpoint = AckEndpoint::new(CaptureAddress::HostPort("210.222.18.25".to_string(), 31000), Framing::default());

        assert!(endpoint.accepts("socket", "210.222.18.25", 31000));
        assert!(!endpoint.accepts("socket", "210.222.18.25", 31001));
    }
}
