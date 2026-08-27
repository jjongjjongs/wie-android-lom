//! The SMAF/MMF container parser, ported from the reference `OracleSmaf`.
//!
//! It walks the `MMMD` chunk tree, and for each score track (`MTR`) it pulls
//! out the header, the per-channel status, the tone bank (decoded through
//! [`super::tone`]) and the event sequence - both the compact and the
//! MIDI-like encodings. The [`super::analysis`] collector turns those events
//! into notes. The streamed-audio (`ATR`) path is walked past but not decoded
//! here; the FM score is what the port needs first.
//!
//! Offsets are held as [`i32`] to mirror the reference's arithmetic exactly; a
//! byte read is masked to `0..=255`.
//!
//! The nested chunk-validation checks and the wide tone-parse signatures follow
//! the reference one to one, so the two style lints that flag them are allowed
//! here rather than reshaped.
#![allow(clippy::collapsible_if, clippy::too_many_arguments)]

use super::synth::CompactTone;
use super::tone::{decode_compact_tone, FormatError};

type Result<T> = core::result::Result<T, FormatError>;

fn err(message: &str) -> FormatError {
    FormatError(message.to_string())
}

// ----- public parse result -----

/// One parsed event, as the reference's `EventInfo`.
#[derive(Clone, Default)]
pub struct EventInfo {
    pub kind: i32,
    pub track_id: i32,
    pub tick: i32,
    pub delta: i32,
    pub offset_in_sequence: i32,
    pub compact: bool,
    pub channel: i32,
    pub key: i32,
    pub key_is_midi: bool,
    pub gate: i32,
    pub control: i32,
    pub value: i32,
    pub meta_type: i32,
    pub payload_offset_in_sequence: i32,
    pub payload_size: i32,
    pub sysex_family: i32,
    pub sysex_type: i32,
    pub sysex_event_code: i32,
    pub sysex_value: i32,
    pub sysex_arg: i32,
    pub payload: Vec<u8>,
    pub raw: Vec<u8>,
}

/// A parsed event stream, as the reference's `SequenceInfo`.
#[derive(Clone, Default)]
pub struct SequenceInfo {
    pub offset: i32,
    pub size: i32,
    pub ticks: i32,
    pub event_count: i32,
    pub has_end_marker: bool,
    pub events: Vec<EventInfo>,
}

/// One tone-bank entry, as the reference's `ToneEntry`.
#[derive(Clone)]
pub struct ToneEntry {
    pub track_id: i32,
    pub ordinal: i32,
    pub format: i32,
    pub tone_no: i32,
    pub params: Vec<u8>,
    pub raw_offset: i32,
    pub raw_size: i32,
    pub decoded_tone: Option<CompactTone>,
    pub decode_error: Option<String>,
}

/// A setup-bulk block, as the reference's `SetupBulkEntry`.
#[derive(Clone)]
pub struct SetupBulkEntry {
    pub track_id: i32,
    pub block_id: i32,
    pub packed_data: Vec<u8>,
}

/// The start/stop points a track's text chunk names, as `TextInfo`.
#[derive(Clone, Copy, Default)]
pub struct TextInfo {
    pub has_st: bool,
    pub st: i64,
    pub has_sp: bool,
    pub sp: i64,
}

/// One score track, as the reference's `TrackInfo`.
#[derive(Clone, Default)]
pub struct TrackInfo {
    pub id: i32,
    pub offset: i32,
    pub size: i32,
    pub format_type: i32,
    pub sequence_type: i32,
    pub timebase_d: i32,
    pub timebase_g: i32,
    pub channel_status: Vec<u8>,
    pub mspi_offset: i32,
    pub mspi_size: i32,
    pub mtsu_offset: i32,
    pub mtsu_size: i32,
    pub mtsp_offset: i32,
    pub mtsp_size: i32,
    pub mspi: TextInfo,
    pub sequence: SequenceInfo,
    pub tones: Vec<ToneEntry>,
    pub setup_bulk_entries: Vec<SetupBulkEntry>,
}

/// The parsed file: the tracks and the longest track length in ticks.
pub struct Smaf {
    pub total_ticks: i32,
    pub tracks: Vec<TrackInfo>,
}

pub fn parse(data: &[u8]) -> Result<Smaf> {
    if data.is_empty() {
        return Err(err("SMAF data is empty"));
    }
    Parser::new(data).parse()
}

// ----- static helpers -----

fn is_alpha(v: i32) -> bool {
    (65..=90).contains(&v) || (97..=122).contains(&v)
}

fn is_digit(v: i32) -> bool {
    (48..=57).contains(&v)
}

fn midi_modulation_bucket(v: i32) -> i32 {
    if v == 0 {
        0
    } else if v < 32 {
        1
    } else if v < 64 {
        2
    } else if v > 95 {
        4
    } else {
        3
    }
}

fn compact_modulation_bucket(v: i32) -> i32 {
    if v == 0 {
        0
    } else if v < 17 {
        1
    } else if v < 49 {
        2
    } else if v > 80 {
        4
    } else {
        3
    }
}

fn mtr_status_len_for_format(format: i32) -> i32 {
    if format != 0 && format != 4 {
        16
    } else {
        2
    }
}

fn read_compact_gate(byte: i32, low: i32, has_extra: bool) -> i32 {
    if byte & 128 != 0 {
        let low = if has_extra { low } else { 0 };
        ((byte & 127) + 1) * 128 + low
    } else {
        byte
    }
}

/// `velocityMap` - the reference's velocity remap table.
const VELOCITY_MAP: [i32; 128] = [
    0, 0, 16, 20, 23, 25, 28, 30, 32, 34, 36, 38, 39, 40, 43, 44, 45, 46, 48, 49, 51, 52, 54, 54, 55, 57, 57, 58, 60, 60, 62, 64, 64, 66, 66, 67, 67,
    69, 69, 69, 71, 71, 74, 74, 76, 76, 76, 78, 78, 78, 80, 80, 80, 82, 82, 82, 85, 85, 85, 87, 87, 87, 90, 90, 90, 90, 93, 93, 93, 93, 95, 95, 95,
    95, 98, 98, 98, 98, 101, 101, 101, 101, 101, 104, 104, 104, 104, 104, 107, 107, 107, 107, 107, 110, 110, 110, 110, 110, 110, 113, 113, 113, 113,
    113, 116, 116, 116, 116, 116, 116, 120, 120, 120, 120, 120, 120, 120, 123, 123, 123, 123, 123, 123, 123, 127, 127, 127, 127,
];

fn velocity_map(v: i32) -> i32 {
    VELOCITY_MAP[(v & 127) as usize]
}

/// The short-control value table for compact short events (control 11).
const COMPACT_CTRL11: [i32; 16] = [0, 0, 31, 39, 47, 55, 63, 71, 79, 87, 95, 103, 111, 119, 127, 127];
/// The short-modulation value table for compact short events (control 129).
const COMPACT_MOD: [i32; 16] = [0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4];

// ----- chunk / VLQ scaffolding -----

struct Chunk {
    stream_no: i32,
    ty: i32,
    payload_offset: i32,
    payload_size: i32,
}

impl Chunk {
    fn base_type(&self) -> i32 {
        self.ty & 0xFF
    }
}

struct ReadValue {
    value: i32,
    next: i32,
}

struct SequenceWindow {
    start: i32,
    size: i32,
}

/// Mutable event under construction, as the reference's `EventBuilder`.
#[derive(Default)]
struct EventBuilder {
    kind: i32,
    track_id: i32,
    tick: i32,
    delta: i32,
    offset_in_sequence: i32,
    compact: bool,
    channel: i32,
    key: i32,
    key_is_midi: bool,
    gate: i32,
    control: i32,
    value: i32,
    meta_type: i32,
    payload_offset_in_sequence: i32,
    payload_size: i32,
    sysex_family: i32,
    sysex_type: i32,
    sysex_event_code: i32,
    sysex_value: i32,
    sysex_arg: i32,
    payload: Vec<u8>,
    raw: Vec<u8>,
}

impl EventBuilder {
    fn new(track_id: i32, tick: i32, delta: i32, offset_in_sequence: i32) -> Self {
        EventBuilder {
            kind: 7,
            track_id,
            tick,
            delta,
            offset_in_sequence,
            channel: 255,
            ..Default::default()
        }
    }

    fn build(self) -> EventInfo {
        EventInfo {
            kind: self.kind,
            track_id: self.track_id,
            tick: self.tick,
            delta: self.delta,
            offset_in_sequence: self.offset_in_sequence,
            compact: self.compact,
            channel: self.channel,
            key: self.key,
            key_is_midi: self.key_is_midi,
            gate: self.gate,
            control: self.control,
            value: self.value,
            meta_type: self.meta_type,
            payload_offset_in_sequence: self.payload_offset_in_sequence,
            payload_size: self.payload_size,
            sysex_family: self.sysex_family,
            sysex_type: self.sysex_type,
            sysex_event_code: self.sysex_event_code,
            sysex_value: self.sysex_value,
            sysex_arg: self.sysex_arg,
            payload: self.payload,
            raw: self.raw,
        }
    }
}

struct Parser<'a> {
    data: &'a [u8],
    total_ticks: i32,
    tracks: Vec<TrackInfo>,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Parser {
            data,
            total_ticks: 0,
            tracks: Vec::new(),
        }
    }

    fn u8(&self, index: i32) -> i32 {
        self.data[index as usize] as i32
    }

    fn be24(&self, at: i32) -> i32 {
        self.u8(at + 2) | self.u8(at) << 16 | self.u8(at + 1) << 8
    }

    fn be32(&self, at: i32) -> i64 {
        (self.u8(at) as i64) << 24 | (self.u8(at + 1) as i64) << 16 | (self.u8(at + 2) as i64) << 8 | self.u8(at + 3) as i64
    }

    fn be32_int(&self, at: i32) -> i32 {
        self.u8(at + 3) | self.u8(at) << 24 | self.u8(at + 1) << 16 | self.u8(at + 2) << 8
    }

    fn checked_end(&self, offset: i32, size: i32, limit: i32) -> Result<i32> {
        if offset >= 0 && size >= 0 && offset <= limit && size <= limit - offset {
            Ok(offset + size)
        } else {
            Err(err("SMAF chunk extends past parent"))
        }
    }

    fn checked_payload_size(&self, at: i32) -> Result<i32> {
        let v = self.be32(at);
        if v <= 2_147_483_647 {
            Ok(v as i32)
        } else {
            Err(err("SMAF chunk is too large"))
        }
    }

    fn copy_bytes(&self, at: i32, len: i32) -> Vec<u8> {
        self.data[at as usize..(at + len) as usize].to_vec()
    }

    fn copy_raw(&self, at: i32, len: i32) -> Vec<u8> {
        let len = len.min(16);
        self.data[at as usize..(at + len) as usize].to_vec()
    }

    fn fourcc(&self, at: i32, tag: &str) -> bool {
        let t = tag.as_bytes();
        self.data[at as usize] == t[0]
            && self.data[(at + 1) as usize] == t[1]
            && self.data[(at + 2) as usize] == t[2]
            && self.data[(at + 3) as usize] == t[3]
    }

    fn fourcc_value(&self, tag: &str) -> i32 {
        let t = tag.as_bytes();
        t[3] as i32 | (t[0] as i32) << 24 | (t[1] as i32) << 16 | (t[2] as i32) << 8
    }

    fn threecc_value(&self, tag: &str) -> i32 {
        let t = tag.as_bytes();
        t[2] as i32 | (t[0] as i32) << 16 | (t[1] as i32) << 8
    }

    fn is_plausible_fourcc(&self, at: i32) -> bool {
        if is_alpha(self.u8(at)) && is_alpha(self.u8(at + 1)) && is_alpha(self.u8(at + 2)) {
            let last = self.u8(at + 3);
            if is_alpha(last) || is_digit(last) || last < 16 {
                return true;
            }
        }
        false
    }

    fn read_compact_vlq(&self, at: i32, end: i32) -> Result<ReadValue> {
        if at < end {
            let b = self.u8(at);
            if b & 128 != 0 {
                if at + 1 >= end {
                    return Err(err("Compact VLQ is truncated"));
                }
                Ok(ReadValue {
                    value: ((b & 127) + 1) * 128 + self.u8(at + 1),
                    next: at + 2,
                })
            } else {
                Ok(ReadValue { value: b, next: at + 1 })
            }
        } else {
            Err(err("Compact VLQ is truncated"))
        }
    }

    fn read_mobile_vlq(&self, at: i32, end: i32) -> Result<ReadValue> {
        let mut value = 0;
        let mut cursor = at;
        for _ in 0..4 {
            if cursor >= end {
                return Err(err("Mobile VLQ is truncated"));
            }
            let byte = self.u8(cursor);
            let next = cursor + 1;
            value = value << 7 | (byte & 127);
            if byte & 128 == 0 {
                return Ok(ReadValue { value, next });
            }
            cursor = next;
        }
        Ok(ReadValue { value, next: cursor })
    }

    fn identify_chunk(&self, expect: i32, at: i32, avail: i32) -> Result<Chunk> {
        if avail < 9 {
            return Err(err("SMAF chunk is truncated"));
        }
        let four = self.be32_int(at);
        let three = self.be24(at);
        let stream = self.u8(at + 3);
        let ty;
        if four == self.fourcc_value("MMMD") {
            if expect != 0 {
                return Err(err("SMAF chunk type does not match parent"));
            }
            ty = 1;
        } else {
            // Each named chunk requires a particular parent state.
            let named: [(&str, i32, i32); 10] = [
                ("CNTI", 1, 2),
                ("OPDA", 2, 3),
                ("MSTR", 2, 7),
                ("MspI", 3, 9),
                ("Mtsu", 3, 10),
                ("Mtsq", 3, 11),
                ("Mtsp", 3, 12),
                ("AspI", 6, 14),
                ("Atsu", 6, 15),
                ("Atsq", 6, 16),
            ];
            let mut resolved = None;
            for (tag, parent, code) in named {
                if four == self.fourcc_value(tag) {
                    if expect != parent {
                        return Err(err("SMAF chunk type does not match parent"));
                    }
                    resolved = Some(code);
                    break;
                }
            }
            if let Some(code) = resolved {
                ty = code;
            } else {
                let three_named: [(&str, i32, i32); 6] = [("MTR", 2, 4), ("ATR", 2, 5), ("GTR", 2, 6), ("Dch", 5, 8), ("Mwa", 4, 13), ("Awa", 6, 17)];
                let mut resolved3 = None;
                for (tag, parent, code) in three_named {
                    if three == self.threecc_value(tag) {
                        if expect != parent {
                            return Err(err("SMAF chunk type does not match parent"));
                        }
                        resolved3 = Some(stream << 8 | code);
                        break;
                    }
                }
                ty = resolved3.unwrap_or(65535);
            }
        }
        let payload_size = self.checked_payload_size(at + 4)?;
        let payload_offset = at + 8;
        self.checked_end(payload_offset, payload_size, avail + at)?;
        Ok(Chunk {
            stream_no: stream,
            ty,
            payload_offset,
            payload_size,
        })
    }

    fn try_identify_chunk(&self, expect: i32, at: i32, avail: i32) -> Option<Chunk> {
        self.identify_chunk(expect, at, avail).ok()
    }

    fn identify_root_child(&self, at: i32, avail: i32) -> Result<Option<Chunk>> {
        if avail < 9 {
            return Ok(None);
        }
        if self.fourcc(at, "CNTI") {
            return self.identify_chunk(1, at, avail).map(Some);
        }
        for expect in [2, 6, 4] {
            if let Some(chunk) = self.try_identify_chunk(expect, at, avail) {
                if chunk.base_type() != 65535 {
                    return Ok(Some(chunk));
                }
            }
        }
        Ok(None)
    }

    fn mtr_child_at(&self, base: i32, size: i32, at: i32) -> bool {
        if at + 8 <= size {
            let pos = base + at;
            if self.is_plausible_fourcc(pos) && self.try_identify_chunk(3, pos, size - at).is_some() {
                return true;
            }
        }
        false
    }

    fn update_total_ticks(&mut self, sequence: &SequenceInfo) {
        if sequence.ticks > self.total_ticks {
            self.total_ticks = sequence.ticks;
        }
    }

    fn parse(mut self) -> Result<Smaf> {
        let root = self.identify_chunk(0, 0, self.data.len() as i32)?;
        if root.base_type() != 1 {
            return Err(err("SMAF root chunk is not MMMD"));
        }
        let end = self.checked_end(8, root.payload_size, self.data.len() as i32)?;
        let mut pos = 8;
        while pos + 8 <= end {
            let lead = self.data[pos as usize] as i32;
            if lead != 44 && lead != 0 {
                if let Some(chunk) = self.identify_root_child(pos, end - pos)? {
                    if chunk.base_type() != 65535 {
                        match chunk.base_type() {
                            3 => {
                                let child_end = self.checked_end(chunk.payload_offset, chunk.payload_size, self.data.len() as i32)?;
                                self.collect_opda_tracks(chunk.payload_offset, child_end)?;
                            }
                            4 if chunk.stream_no < 16 && self.tracks.len() < 16 => {
                                let mut track = TrackInfo {
                                    id: chunk.stream_no,
                                    offset: chunk.payload_offset,
                                    size: chunk.payload_size,
                                    ..Default::default()
                                };
                                self.parse_mtr(&mut track)?;
                                let seq = track.sequence.clone();
                                self.tracks.push(track);
                                self.update_total_ticks(&seq);
                            }
                            // Streamed-audio chunks are walked past; the FM path
                            // does not decode them yet.
                            _ => {}
                        }
                        pos += chunk.payload_size + 8;
                        continue;
                    }
                }
            }
            pos += 1;
        }
        Ok(Smaf {
            total_ticks: self.total_ticks,
            tracks: self.tracks,
        })
    }

    fn collect_opda_tracks(&mut self, mut at: i32, end: i32) -> Result<()> {
        while at + 8 <= end {
            let lead = self.data[at as usize] as i32;
            if lead != 44 && lead != 0 && self.is_plausible_fourcc(at) {
                if let Some(chunk) = self.try_identify_chunk(2, at, end - at) {
                    if chunk.base_type() != 65535 {
                        self.checked_end(chunk.payload_offset, chunk.payload_size, self.data.len() as i32)?;
                        if chunk.base_type() == 4 && chunk.stream_no < 16 && self.tracks.len() < 16 {
                            let mut track = TrackInfo {
                                id: chunk.stream_no,
                                offset: chunk.payload_offset,
                                size: chunk.payload_size,
                                ..Default::default()
                            };
                            self.parse_mtr(&mut track)?;
                            let seq = track.sequence.clone();
                            self.tracks.push(track);
                            self.update_total_ticks(&seq);
                        }
                        at += chunk.payload_size + 8;
                        continue;
                    }
                }
            }
            at += 1;
        }
        Ok(())
    }

    fn parse_text_info(&self, at: i32, size: i32) -> TextInfo {
        let (st_found, st) = self.parse_be_after_tag(at, size, "st:");
        let (sp_found, sp) = self.parse_be_after_tag(at, size, "sp:");
        TextInfo {
            has_st: st_found,
            st,
            has_sp: sp_found,
            sp,
        }
    }

    fn parse_be_after_tag(&self, at: i32, size: i32, tag: &str) -> (bool, i64) {
        let needle = tag.as_bytes();
        let end = at + size;
        let mut pos = at;
        loop {
            if needle.len() as i32 + pos + 4 > end {
                return (false, 0);
            }
            let mut matched = true;
            for (i, &b) in needle.iter().enumerate() {
                if self.data[(pos + i as i32) as usize] != b {
                    matched = false;
                    break;
                }
            }
            if matched {
                return (true, self.be32(pos + needle.len() as i32));
            }
            pos += 1;
        }
    }

    fn sequence_window(&self, text: &TextInfo, size: i32) -> Result<SequenceWindow> {
        let start = if text.has_st { text.st } else { 0 };
        let stop = if text.has_sp { text.sp } else { size as i64 };
        if start >= 0 && stop >= start && stop <= size as i64 {
            Ok(SequenceWindow {
                start: start as i32,
                size: (stop - start) as i32,
            })
        } else {
            Err(err("SMAF sequence window is outside its payload"))
        }
    }

    fn parse_mtr(&self, track: &mut TrackInfo) -> Result<()> {
        if track.size < 6 {
            return Err(err("MTR chunk is truncated"));
        }
        let base = track.offset;
        // The header is four bytes when a child chunk follows the status region
        // directly; only the two alternate-header layouts keep the six-byte
        // header, exactly as the reference's `var3`.
        let mut header_len = 6;
        track.format_type = self.u8(base);
        track.sequence_type = self.u8(base + 1);
        track.timebase_d = self.u8(base + 2);
        track.timebase_g = self.u8(base + 3);
        let mut status_len = mtr_status_len_for_format(track.format_type);
        if !self.mtr_child_at(base, track.size, status_len + 4) {
            let alt_format = self.u8(base + 2);
            let alt_status = mtr_status_len_for_format(alt_format);
            if self.mtr_child_at(base, track.size, 6) {
                track.format_type = alt_format;
                track.sequence_type = self.u8(base + 3);
                track.timebase_d = self.u8(base + 4);
                track.timebase_g = self.u8(base + 5);
                status_len = 0;
            } else if self.mtr_child_at(base, track.size, alt_status + 6) {
                track.format_type = alt_format;
                track.sequence_type = self.u8(base + 3);
                track.timebase_d = self.u8(base + 4);
                track.timebase_g = self.u8(base + 5);
                status_len = alt_status;
            } else {
                header_len = 4;
            }
        } else {
            header_len = 4;
        }

        let mut status = status_len;
        if status > track.size - header_len {
            status = track.size - header_len;
        }
        let mut pos = header_len;
        if status > 0 {
            track.channel_status = self.data[(base + header_len) as usize..(base + header_len + status) as usize].to_vec();
            pos = header_len + status;
        }

        while pos + 8 <= track.size {
            let at = base + pos;
            if self.data[at as usize] != 44 && self.is_plausible_fourcc(at) {
                if let Some(chunk) = self.try_identify_chunk(3, at, track.size - pos) {
                    match chunk.base_type() {
                        9 => {
                            track.mspi_offset = chunk.payload_offset;
                            track.mspi_size = chunk.payload_size;
                            self.checked_end(track.mspi_offset, track.mspi_size, self.data.len() as i32)?;
                            track.mspi = self.parse_text_info(track.mspi_offset, track.mspi_size);
                        }
                        10 => {
                            track.mtsu_offset = chunk.payload_offset;
                            track.mtsu_size = chunk.payload_size;
                            self.checked_end(track.mtsu_offset, track.mtsu_size, self.data.len() as i32)?;
                            let tones = self.parse_tone_entries(track.mtsu_offset, track.mtsu_size, track.id)?;
                            track.tones.extend(tones);
                            let bulk = self.parse_setup_bulk_entries(track.mtsu_offset, track.mtsu_size, track.id)?;
                            track.setup_bulk_entries.extend(bulk);
                        }
                        11 => {
                            self.checked_end(chunk.payload_offset, chunk.payload_size, self.data.len() as i32)?;
                            let window = self.sequence_window(&track.mspi, chunk.payload_size)?;
                            track.sequence =
                                self.parse_sequence_for_format(chunk.payload_offset + window.start, window.size, track.id, track.format_type)?;
                        }
                        12 => {
                            track.mtsp_offset = chunk.payload_offset;
                            track.mtsp_size = chunk.payload_size;
                        }
                        _ => {}
                    }
                    pos += chunk.payload_size + 8;
                    continue;
                }
            }
            pos += 1;
        }
        Ok(())
    }

    fn parse_tone_entries(&self, at: i32, size: i32, track_id: i32) -> Result<Vec<ToneEntry>> {
        let mut entries = Vec::new();
        let end = size + at;
        let mut pos = at;
        let mut ordinal = 0;
        while pos < end {
            while pos < end && self.data[pos as usize] == 44 {
                pos += 1;
            }
            if pos >= end {
                break;
            }
            let remaining = end - pos;
            if remaining < 3 {
                return Err(err("Mtsu tone entry is truncated"));
            }
            if self.u8(pos) == 240 {
                let vlq = self.read_mobile_vlq(pos + 1, end)?;
                let len = vlq.value;
                let after = vlq.next;
                if len > end - after {
                    return Err(err("Mtsu direct Mobile voice is truncated"));
                }
                if let Some(entry) = self.try_parse_direct_mobile_tone(pos, after, len, track_id, ordinal)? {
                    entries.push(entry);
                    ordinal += 1;
                }
                pos = after + len;
            } else {
                if remaining < 6 {
                    return Err(err("Mtsu sysex tone entry is truncated"));
                }
                if self.u8(pos) == 255 && self.u8(pos + 1) == 240 {
                    let len_pos = pos + 2;
                    let len = self.u8(len_pos);
                    let consumed = len + 3;
                    if remaining < consumed {
                        return Err(err("Mtsu sysex tone payload is truncated"));
                    }
                    let entry = if len >= 13 && self.u8(pos + 3) == 67 && self.u8(pos + 4) == 2 {
                        let params_at = pos + 5;
                        Some(self.create_tone_entry(
                            track_id,
                            ordinal,
                            3,
                            self.u8(params_at),
                            self.copy_bytes(params_at, len - 3),
                            pos,
                            consumed,
                        ))
                    } else if len >= 4 && self.u8(pos + 3) == 67 && self.u8(pos + 4) == 3 {
                        let params_at = pos + 5;
                        Some(self.create_tone_entry(
                            track_id,
                            ordinal,
                            1,
                            self.u8(params_at),
                            self.copy_bytes(params_at, len - 3),
                            pos,
                            consumed,
                        ))
                    } else if len >= 5 && self.u8(pos + 3) == 67 && self.u8(pos + 4) == 4 && self.u8(pos + 5) == 1 && self.u8(len_pos + len) == 247 {
                        let params_at = pos + 6;
                        Some(self.create_tone_entry(
                            track_id,
                            ordinal,
                            2,
                            self.u8(params_at),
                            self.copy_bytes(params_at, len - 4),
                            pos,
                            consumed,
                        ))
                    } else {
                        None
                    };
                    if let Some(entry) = entry {
                        entries.push(entry);
                        ordinal += 1;
                    }
                    pos += consumed;
                } else {
                    pos += 1;
                }
            }
        }
        Ok(entries)
    }

    fn create_tone_entry(
        &self,
        track_id: i32,
        ordinal: i32,
        format: i32,
        tone_no: i32,
        params: Vec<u8>,
        raw_offset: i32,
        raw_size: i32,
    ) -> ToneEntry {
        let (decoded_tone, decode_error) = if format != 5 {
            match decode_compact_tone(format, &params) {
                Ok(tone) => (Some(tone), None),
                Err(e) => (None, Some(e.0)),
            }
        } else {
            (None, None)
        };
        ToneEntry {
            track_id,
            ordinal,
            format,
            tone_no,
            params,
            raw_offset,
            raw_size,
            decoded_tone,
            decode_error,
        }
    }

    fn parse_setup_bulk_entries(&self, at: i32, size: i32, track_id: i32) -> Result<Vec<SetupBulkEntry>> {
        let mut entries = Vec::new();
        let end = self.checked_end(at, size, self.data.len() as i32)?;
        let mut pos = at;
        while pos < end {
            if self.u8(pos) != 240 {
                pos += 1;
            } else {
                let vlq = self.read_mobile_vlq(pos + 1, end)?;
                let payload = vlq.next;
                if vlq.value > end - payload {
                    return Err(err("Mtsu bulk payload is truncated"));
                }
                if vlq.value >= 9
                    && self.u8(payload) == 67
                    && self.u8(payload + 1) == 121
                    && self.u8(payload + 2) == 6
                    && self.u8(payload + 3) == 127
                    && self.u8(payload + 4) == 3
                    && self.u8(vlq.value + payload - 1) == 247
                {
                    entries.push(SetupBulkEntry {
                        track_id,
                        block_id: 127 & self.u8(payload + 5),
                        packed_data: self.copy_bytes(payload + 7, vlq.value - 8),
                    });
                }
                pos = vlq.value + payload;
            }
        }
        Ok(entries)
    }

    fn parse_sequence_for_format(&self, at: i32, size: i32, track_id: i32, format: i32) -> Result<SequenceInfo> {
        if format != 1 && format != 2 && format != 6 {
            self.parse_compact_sequence(at, size, track_id)
        } else {
            self.parse_midi_like_sequence(at, size, track_id)
        }
    }

    fn parse_compact_sequence(&self, start: i32, size: i32, track_id: i32) -> Result<SequenceInfo> {
        let end = start + size;
        let mut transpose = [0i32; 4];
        let mut events = Vec::new();
        let mut pos = start;
        let mut running_tick = 0;
        let mut ended = false;
        let mut last_abs_tick = 0;

        while pos < end {
            let vlq = self.read_compact_vlq(pos, end)?;
            let next = vlq.next;
            let abs_tick = running_tick + vlq.value;
            last_abs_tick = abs_tick;
            if next >= end {
                return Err(err("Compact sequence is truncated"));
            }
            let b0 = self.u8(next);
            let mut eb = EventBuilder::new(track_id, abs_tick, vlq.value, next - start);
            eb.compact = true;
            let consumed;
            let mut ended_now = ended;

            if b0 == 0 {
                let remaining = end - next;
                if remaining == 1 {
                    eb.kind = 5;
                    ended_now = true;
                    consumed = 1;
                } else {
                    let b1 = self.u8(next + 1);
                    let sel = b1 & 48;
                    if sel == 48 {
                        if remaining < 3 {
                            return Err(err("Compact control event is truncated"));
                        }
                        eb.kind = 2;
                        eb.channel = b1 >> 6;
                        let ctrl = b1 & 15;
                        let val = self.u8(next + 2);
                        if ctrl == 3 && val < 128 {
                            eb.control = 129;
                            eb.value = compact_modulation_bucket(val);
                        } else {
                            eb.control = ctrl;
                            eb.value = val;
                            if ctrl == 2 {
                                let signed = if val >= 128 { -(val - 128) } else { val };
                                transpose[(eb.channel & 3) as usize] = signed;
                            }
                        }
                        consumed = 3;
                    } else {
                        if remaining < 2 {
                            return Err(err("Compact short event is truncated"));
                        }
                        eb.kind = 6;
                        eb.channel = b1 >> 6;
                        if sel == 0 {
                            eb.control = 11;
                            eb.value = COMPACT_CTRL11[(b1 & 15) as usize];
                        } else if sel == 32 {
                            eb.control = 129;
                            eb.value = COMPACT_MOD[(b1 & 15) as usize];
                        }
                        consumed = 2;
                    }
                }
            } else if b0 != 255 {
                let remaining = end - next;
                if remaining < 2 {
                    return Err(err("Compact note event is truncated"));
                }
                let gate_byte = self.u8(next + 1);
                let has_extra = gate_byte & 128 != 0;
                let need = if has_extra { 3 } else { 2 };
                if remaining < need {
                    return Err(err("Compact note gate is truncated"));
                }
                eb.kind = 1;
                eb.channel = b0 >> 6;
                let raw_key = (b0 >> 4 & 3) * 12 + 36 + (b0 & 15) + transpose[(eb.channel & 3) as usize] * 12;
                eb.key = raw_key.clamp(0, 127);
                eb.key_is_midi = true;
                eb.value = 127;
                let gate_low = if has_extra { self.u8(next + 2) } else { 0 };
                eb.gate = read_compact_gate(gate_byte, gate_low, has_extra);
                consumed = need;
            } else {
                let remaining = end - next;
                if remaining == 1 {
                    eb.kind = 5;
                    ended_now = true;
                    consumed = 1;
                } else {
                    if remaining < 2 {
                        return Err(err("Compact meta event is truncated"));
                    }
                    let meta_pos = next + 1;
                    if self.u8(meta_pos) != 240 {
                        eb.meta_type = self.u8(meta_pos);
                        eb.kind = 4;
                        if eb.meta_type == 0 {
                            let mut p = next + 2;
                            while p < end && self.u8(p) == 0 {
                                p += 1;
                            }
                            if p == end {
                                eb.kind = 5;
                                ended_now = true;
                            }
                        }
                        consumed = 2;
                    } else {
                        if remaining < 3 {
                            return Err(err("Compact sysex event is truncated"));
                        }
                        let sysex_len = self.u8(next + 2);
                        consumed = sysex_len + 3;
                        if remaining < consumed {
                            return Err(err("Compact sysex payload is truncated"));
                        }
                        eb.kind = 3;
                        let payload_at = next + 3;
                        eb.payload_offset_in_sequence = payload_at - start;
                        eb.payload_size = sysex_len;
                        eb.payload = self.copy_bytes(payload_at, sysex_len);
                        self.decode_yamaha_system_sysex(&mut eb, payload_at, sysex_len);
                    }
                }
            }

            eb.raw = self.copy_raw(next, consumed);
            events.push(eb.build());
            pos = next + consumed;
            running_tick = abs_tick;
            ended = ended_now;
            if ended {
                break;
            }
        }

        let event_count = events.len() as i32;
        Ok(SequenceInfo {
            offset: start,
            size,
            ticks: last_abs_tick,
            event_count,
            has_end_marker: ended,
            events,
        })
    }

    fn parse_midi_like_sequence(&self, start: i32, size: i32, track_id: i32) -> Result<SequenceInfo> {
        let end = start + size;
        let mut ch_vel = [90i32; 16];
        let mut events = Vec::new();
        let mut pos = start;
        let mut acc_tick = 0;
        let mut running_status = 0;
        let mut ended = false;

        loop {
            if pos >= end {
                return Ok(SequenceInfo {
                    offset: start,
                    size,
                    ticks: acc_tick,
                    event_count: events.len() as i32,
                    has_end_marker: ended,
                    events,
                });
            }
            let vlq = self.read_mobile_vlq(pos, end)?;
            let next = vlq.next;
            let abs_tick = acc_tick + vlq.value;
            if next >= end {
                return Err(err("MIDI-like sequence is truncated"));
            }
            let cursor = next + 1;
            let status_byte = self.u8(next);
            let opcode;
            let mut data_pos;
            let new_running_status;
            if status_byte < 128 {
                if running_status == 0 {
                    return Err(err("MIDI-like sequence has data byte without running status"));
                }
                opcode = running_status;
                data_pos = next;
                new_running_status = running_status;
            } else if status_byte < 240 {
                opcode = status_byte;
                data_pos = cursor;
                new_running_status = status_byte;
            } else {
                opcode = status_byte;
                data_pos = cursor;
                new_running_status = running_status;
            }

            let mut eb = EventBuilder::new(track_id, abs_tick, vlq.value, next - start);
            eb.channel = if opcode < 240 { opcode & 15 } else { 255 };
            let hi = opcode & 240;
            let event_end;

            if hi == 128 {
                if data_pos >= end {
                    return Err(err("MIDI-like note event is truncated"));
                }
                eb.key = self.u8(data_pos);
                let gate = self.read_mobile_vlq(data_pos + 1, end)?;
                event_end = gate.next;
                eb.kind = 1;
                eb.key_is_midi = true;
                eb.gate = if gate.value != 0 { gate.value } else { 1 };
                eb.value = ch_vel[(eb.channel & 15) as usize];
            } else if hi == 144 {
                if data_pos + 2 > end {
                    return Err(err("MIDI-like velocity note event is truncated"));
                }
                eb.key = self.u8(data_pos);
                let vel = velocity_map(self.u8(data_pos + 1));
                let gate = self.read_mobile_vlq(data_pos + 2, end)?;
                event_end = gate.next;
                ch_vel[(eb.channel & 15) as usize] = vel;
                eb.kind = 1;
                eb.key_is_midi = true;
                eb.gate = if gate.value != 0 { gate.value } else { 1 };
                eb.value = vel;
            } else if hi == 176 {
                if data_pos + 2 > end {
                    return Err(err("MIDI-like control event is truncated"));
                }
                let ctrl_num = self.u8(data_pos);
                let val = self.u8(data_pos + 1);
                event_end = data_pos + 2;
                eb.kind = 2;
                eb.value = val;
                eb.control = self.map_midi_control(ctrl_num, val, &mut eb);
            } else if hi == 160 {
                if data_pos + 2 > end {
                    return Err(err("MIDI-like poly pressure event is truncated"));
                }
                eb.kind = 4;
                eb.meta_type = 160;
                eb.key = self.u8(data_pos);
                eb.value = self.u8(data_pos + 1);
                event_end = data_pos + 2;
            } else if hi == 192 {
                if data_pos >= end {
                    return Err(err("MIDI-like program change event is truncated"));
                }
                eb.kind = 2;
                eb.control = 2;
                eb.value = self.u8(data_pos) & 127;
                event_end = data_pos + 1;
            } else if hi == 208 {
                if data_pos >= end {
                    return Err(err("MIDI-like channel pressure event is truncated"));
                }
                eb.kind = 4;
                eb.meta_type = 208;
                eb.value = self.u8(data_pos) & 127;
                event_end = data_pos + 1;
            } else if hi == 224 {
                if data_pos + 2 > end {
                    return Err(err("MIDI-like pitch bend event is truncated"));
                }
                let lo = self.u8(data_pos);
                let hi7 = self.u8(data_pos + 1) & 127;
                event_end = data_pos + 2;
                eb.kind = 2;
                eb.control = 14;
                eb.value = hi7;
                eb.gate = hi7 << 7 | (lo & 127);
            } else if opcode != 255 {
                if opcode == 240 {
                    let payload = self.read_mobile_vlq(data_pos, end)?;
                    let payload_at = payload.next;
                    if end - payload_at < payload.value {
                        return Err(err("MIDI-like sysex payload is truncated"));
                    }
                    eb.kind = 3;
                    eb.meta_type = opcode & 15;
                    eb.payload_offset_in_sequence = payload_at - start;
                    eb.payload_size = payload.value;
                    eb.payload = self.copy_bytes(payload_at, payload.value);
                    self.decode_yamaha_system_sysex(&mut eb, payload_at, payload.value);
                    event_end = payload_at + payload.value;
                } else {
                    eb.kind = 7;
                    event_end = data_pos;
                }
            } else {
                if data_pos >= end {
                    return Err(err("MIDI-like meta event is truncated"));
                }
                let meta_type = self.u8(data_pos);
                data_pos += 1;
                eb.meta_type = meta_type;
                if meta_type == 0 {
                    eb.kind = 4;
                    event_end = data_pos;
                } else if meta_type == 47 {
                    eb.kind = 5;
                    ended = true;
                    event_end = data_pos;
                } else {
                    let payload = self.read_mobile_vlq(data_pos, end)?;
                    let payload_at = payload.next;
                    if end - payload_at < payload.value {
                        return Err(err("MIDI-like meta payload is truncated"));
                    }
                    eb.payload_offset_in_sequence = payload_at - start;
                    eb.payload_size = payload.value;
                    event_end = payload_at + payload.value;
                    eb.kind = 4;
                }
            }

            eb.raw = self.copy_raw(next, event_end - next);
            events.push(eb.build());
            if ended {
                return Ok(SequenceInfo {
                    offset: start,
                    size,
                    ticks: abs_tick,
                    event_count: events.len() as i32,
                    has_end_marker: true,
                    events,
                });
            }
            running_status = new_running_status;
            acc_tick = abs_tick;
            pos = event_end;
        }
    }

    fn map_midi_control(&self, ctrl: i32, value: i32, eb: &mut EventBuilder) -> i32 {
        if ctrl == 0 {
            return 1;
        }
        if ctrl == 1 {
            eb.value = midi_modulation_bucket(value);
            return 129;
        }
        if ctrl == 32 {
            return 0;
        }
        if matches!(ctrl, 6 | 7 | 10 | 11 | 64 | 123 | 100 | 101 | 120 | 121 | 126 | 127) {
            return ctrl;
        }
        if ctrl < 16 {
            return ctrl | 128;
        }
        ctrl
    }

    fn decode_yamaha_system_sysex(&self, eb: &mut EventBuilder, at: i32, size: i32) {
        if size < 5 || self.u8(at) != 67 || self.u8(at + 1) != 121 || self.u8(at + 2) != 6 {
            return;
        }
        let family_byte = self.u8(at + 3);
        if family_byte == 124 && size >= 6 && self.u8(at + 4) == 33 {
            eb.sysex_family = 2;
            eb.sysex_type = self.u8(at + 5);
            eb.sysex_event_code = eb.sysex_type;
            if eb.sysex_type == 0 && size >= 7 {
                eb.sysex_value = self.u8(at + 6) & 127;
            } else if eb.sysex_type == 3 && size >= 10 {
                eb.sysex_value = self.u8(at + 7) & 127;
                let high = self.u8(at + 8);
                eb.sysex_arg = self.u8(at + 9) & 127 | (high & 127) << 7;
            } else if eb.sysex_type == 4 && size >= 9 {
                eb.sysex_value = self.u8(at + 8) & 127;
                let high = self.u8(at + 6);
                eb.sysex_arg = self.u8(at + 7) & 127 | (high & 1) << 8;
            } else if (eb.sysex_type == 5 || eb.sysex_type == 6) && size >= 12 {
                eb.sysex_value = self.u8(at + 7) & 31;
                eb.sysex_arg = self.u8(at + 8) & 3;
            }
            return;
        }
        if family_byte == 124 && size >= 6 && self.u8(at + 4) == 32 {
            eb.sysex_family = 3;
            eb.sysex_type = self.u8(at + 5);
            eb.sysex_event_code = eb.sysex_type;
            if (eb.sysex_type == 5 || eb.sysex_type == 6) && size >= 9 {
                eb.sysex_value = self.u8(at + 6) & 15;
                let high = self.u8(at + 7);
                eb.sysex_arg = self.u8(at + 8) & 127 | (high & 127) << 7;
            }
            return;
        }
        if family_byte != 127 {
            return;
        }
        eb.sysex_family = 1;
        eb.sysex_type = self.u8(at + 4);
        eb.sysex_event_code = 30;
        match eb.sysex_type {
            0 => {
                if size >= 6 {
                    eb.sysex_event_code = 23;
                    eb.sysex_value = self.u8(at + 5);
                }
            }
            8 => {
                if size >= 8 {
                    eb.sysex_value = self.u8(at + 5);
                    let high = self.u8(at + 6);
                    eb.sysex_arg = self.u8(at + 7) | high << 8;
                }
            }
            11 => {
                if size >= 8 {
                    let sel = at + 5;
                    if self.u8(sel) != 0 && self.u8(sel) < 33 {
                        eb.sysex_event_code = 22;
                        eb.sysex_value = self.u8(sel) - 1;
                        let mode = at + 6;
                        if self.u8(mode) == 1 {
                            eb.sysex_arg = 255;
                        } else if self.u8(mode) == 2 {
                            eb.sysex_arg = 128;
                        } else {
                            eb.sysex_arg = self.u8(at + 7);
                        }
                    }
                }
            }
            16 => {
                if size >= 6 {
                    let sel = at + 5;
                    if self.u8(sel) < 16 {
                        eb.sysex_event_code = 29;
                        eb.sysex_value = self.u8(sel);
                    }
                }
            }
            33 => {
                if size >= 7 {
                    eb.sysex_event_code = 33;
                    eb.sysex_value = self.u8(at + 5);
                    eb.sysex_arg = self.u8(at + 6);
                }
            }
            _ => {}
        }
    }

    fn try_parse_direct_mobile_tone(&self, header: i32, at: i32, len: i32, track_id: i32, ordinal: i32) -> Result<Option<ToneEntry>> {
        if let Some(entry) = self.try_parse_mtr6_direct_mobile_tone(header, at, len, track_id, ordinal) {
            return Ok(Some(entry));
        }
        if len < 30 {
            return Ok(None);
        }
        let payload_end = at + len;
        if self.u8(payload_end - 1) != 247
            || self.u8(at) != 67
            || self.u8(at + 1) != 121
            || self.u8(at + 2) != 6
            || self.u8(at + 3) != 127
            || self.u8(at + 4) != 1
        {
            return Ok(None);
        }
        let flag_pos = at + 5;
        if self.u8(flag_pos) != 124 && self.u8(flag_pos) != 125 {
            return Ok(None);
        }
        let total = len + 1;
        let mut nibble_bytes = 0;
        let mut out_len = 0;
        let mut voice_len = 16;
        if total == 31 && self.u8(at + 9) == 1 {
            nibble_bytes = 19;
            out_len = 16;
        } else if total == 32 && self.u8(at + 9) == 0 {
            nibble_bytes = 20;
            out_len = 17;
        } else if total == 48 && self.u8(at + 9) == 0 {
            nibble_bytes = 36;
            out_len = 31;
            voice_len = 30;
        }
        if nibble_bytes == 0 || len < nibble_bytes + 11 {
            return Ok(None);
        }

        let mut decoded = [0u8; 31];
        let mut src = at + 10;
        let src_end = nibble_bytes + src;
        let mut written = 0;
        while src < src_end && written < out_len {
            let high = self.u8(src);
            src += 1;
            let mut bit = 0;
            while bit < 7 && src < src_end && written < out_len {
                let low = self.u8(src);
                bit += 1;
                decoded[written as usize] = (low | high << bit & 128) as u8;
                src += 1;
                written += 1;
            }
        }
        if written < out_len {
            return Ok(None);
        }

        let program;
        let mut tone_no;
        if self.u8(flag_pos) == 125 {
            program = (self.u8(at + 7) & 127) + 129;
            tone_no = self.u8(at + 8);
        } else {
            program = (self.u8(at + 6) & 127) + 1;
            tone_no = self.u8(at + 7);
        }
        tone_no &= 127;

        if total == 31 && self.u8(at + 9) == 1 {
            let mut params = [0u8; 22];
            params[0] = tone_no as u8;
            params[1] = program as u8;
            params[2] = tone_no as u8;
            params[3] = decoded[0];
            params[4] = decoded[1];
            params[5] = 14;
            params[6..19].copy_from_slice(&decoded[2..15]);
            let tail = decoded[15];
            if tail & 128 != 0 {
                let idx = (tail & 127) as usize;
                if idx < 7 {
                    let value = [5120, 5590, 7076, 8840, 9460, 11122, 13522][idx];
                    params[13] = (value >> 8) as u8;
                    params[14] = value as u8;
                }
            }
            params[19] = tail;
            params[20] = self.data[flag_pos as usize];
            params[21] = (self.u8(at + 6) & 127) as u8;
            return Ok(Some(self.create_tone_entry(
                track_id,
                ordinal,
                5,
                tone_no,
                params.to_vec(),
                header,
                payload_end - header,
            )));
        }

        let mut params = vec![0u8; (voice_len + 7) as usize];
        params[0] = tone_no as u8;
        params[1] = program as u8;
        params[2] = tone_no as u8;
        params[3] = decoded[0];
        params[4] = voice_len as u8;
        params[5..(5 + voice_len) as usize].copy_from_slice(&decoded[1..(1 + voice_len) as usize]);
        params[(voice_len + 5) as usize] = self.data[flag_pos as usize];
        params[(voice_len + 6) as usize] = (self.u8(at + 6) & 127) as u8;
        Ok(Some(self.create_tone_entry(
            track_id,
            ordinal,
            4,
            tone_no,
            params,
            header,
            payload_end - header,
        )))
    }

    fn try_parse_mtr6_direct_mobile_tone(&self, header: i32, at: i32, len: i32, track_id: i32, ordinal: i32) -> Option<ToneEntry> {
        // The reference only takes this layout for track id 6.
        if track_id != 6 || len < 12 {
            return None;
        }
        let payload_end = at + len;
        if self.u8(payload_end - 1) != 247
            || self.u8(at) != 67
            || self.u8(at + 1) != 121
            || self.u8(at + 2) != 7
            || self.u8(at + 3) != 127
            || self.u8(at + 4) != 1
        {
            return None;
        }
        let flag_pos = at + 5;
        if self.u8(flag_pos) != 124 && self.u8(flag_pos) != 125 {
            return None;
        }
        let program;
        let tone_no;
        if self.u8(flag_pos) == 124 && self.u8(at + 6) < 10 {
            program = self.u8(at + 6) + 1;
            tone_no = self.u8(at + 7);
        } else if self.u8(flag_pos) == 125 && self.u8(at + 6) == 0 {
            if self.u8(at + 7) >= 10 {
                return None;
            }
            program = self.u8(at + 7) + 129;
            tone_no = self.u8(at + 8);
        } else {
            return None;
        }
        let tone_no = tone_no & 127;
        let flags = self.u8(at + 9);
        let record = at + 10;
        let remaining = len - 11;
        if flags & 2 != 0 {
            return None;
        }
        let raw_size = payload_end - header;
        if flags & 1 == 0 && remaining >= 3 {
            let voice_len = if self.u8(at + 12) & 7 < 2 { 16 } else { 30 };
            if remaining < voice_len + 1 {
                return None;
            }
            let mut params = vec![0u8; (voice_len + 7) as usize];
            params[0] = tone_no as u8;
            params[1] = program as u8;
            params[2] = tone_no as u8;
            params[3] = self.data[record as usize];
            params[4] = voice_len as u8;
            params[5..(5 + voice_len) as usize].copy_from_slice(&self.data[(at + 11) as usize..(at + 11 + voice_len) as usize]);
            params[(voice_len + 5) as usize] = self.data[flag_pos as usize];
            params[(voice_len + 6) as usize] = self.data[(at + 6) as usize];
            return Some(self.create_tone_entry(track_id, ordinal, 4, tone_no, params, header, raw_size));
        }
        if flags & 1 != 0 && remaining >= 16 {
            let mut params = [0u8; 22];
            params[0] = tone_no as u8;
            params[1] = program as u8;
            params[2] = tone_no as u8;
            params[3] = self.data[record as usize];
            params[4] = self.data[(at + 11) as usize];
            params[5] = 14;
            params[6..19].copy_from_slice(&self.data[(at + 12) as usize..(at + 25) as usize]);
            let tail = self.u8(at + 25);
            if tail & 128 != 0 {
                let idx = (tail & 127) as usize;
                if idx < 7 {
                    let value = [5120, 5590, 7076, 8840, 9460, 11122, 13522][idx];
                    params[13] = (value >> 8) as u8;
                    params[14] = value as u8;
                }
            }
            params[19] = tail as u8;
            params[20] = self.data[flag_pos as usize];
            params[21] = self.data[(at + 6) as usize];
            return Some(self.create_tone_entry(track_id, ordinal, 5, tone_no, params.to_vec(), header, raw_size));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    fn ev(e: &EventInfo) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            e.kind,
            e.tick,
            e.delta,
            e.channel,
            e.key,
            e.key_is_midi as i32,
            e.gate,
            e.control,
            e.value,
            e.meta_type,
            e.payload_offset_in_sequence,
            e.payload_size,
            e.sysex_family,
            e.sysex_type,
            e.sysex_event_code,
            e.sysex_value,
            e.sysex_arg,
            hex(&e.raw),
            hex(&e.payload),
        )
    }

    /// The same track fingerprint the oracle's `DumpSeq` emits.
    fn track_fingerprint(t: &TrackInfo) -> String {
        let mut s = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            t.id,
            t.format_type,
            t.sequence_type,
            t.timebase_d,
            t.timebase_g,
            hex(&t.channel_status),
            t.sequence.ticks,
            t.sequence.event_count,
            t.sequence.has_end_marker as i32,
            t.tones.len(),
            t.setup_bulk_entries.len(),
        );
        for te in &t.tones {
            s.push_str(&format!("|T{},{},{}", te.format, te.tone_no, hex(&te.params)));
        }
        for e in &t.sequence.events {
            s.push_str("||");
            s.push_str(&ev(e));
        }
        s
    }

    /// A compact-sequence and a MIDI-like MMF, parsed and checked field for
    /// field against the reference `OracleSmaf`. The fixtures and their
    /// fingerprints were captured from the oracle.
    #[test]
    fn parses_fixtures_like_the_reference() {
        let files: [(&str, &[u8]); 2] = [
            ("compact.mmf", include_bytes!("data/seq/compact.mmf")),
            ("midi_like.mmf", include_bytes!("data/seq/midi_like.mmf")),
        ];
        let vectors = include_str!("data/seq_vectors.txt");
        let mut expected: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
        for line in vectors.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            expected.entry(cols[0]).or_default().push(cols[2]);
        }
        for (name, bytes) in files {
            let smaf = super::parse(bytes).unwrap_or_else(|e| panic!("{name}: parse failed: {}", e.0));
            let got: Vec<String> = smaf.tracks.iter().map(track_fingerprint).collect();
            let want = &expected[name];
            assert_eq!(got.len(), want.len(), "{name}: track count");
            for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                assert_eq!(g, w, "{name} track {i}");
            }
        }
    }

    /// Parse every MMF in the corpus and check its track fingerprints against
    /// the reference `OracleSmaf` running as an oracle. Gated on the
    /// `OMA3_SEQ_DUMP` env var (a `DumpSeq` capture) so it only runs when the
    /// oracle data is present.
    #[test]
    fn parses_corpus_like_the_reference() {
        let dump_path = match std::env::var("OMA3_SEQ_DUMP") {
            Ok(p) => p,
            Err(_) => return,
        };
        let dump = std::fs::read_to_string(&dump_path).unwrap();
        // Group expected TRK fingerprints by file path, in order.
        let mut expected: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        for line in dump.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols[0] == "TRK" {
                let path = cols[1].to_string();
                if !expected.contains_key(&path) {
                    order.push(path.clone());
                }
                expected.entry(path).or_default().push(cols[3].to_string());
            }
        }
        let mut files = 0;
        let mut tracks = 0;
        for path in &order {
            let data = std::fs::read(path).unwrap();
            let smaf = super::parse(&data).unwrap_or_else(|e| panic!("{path}: parse failed: {}", e.0));
            let got: Vec<String> = smaf.tracks.iter().map(track_fingerprint).collect();
            let want = &expected[path];
            assert_eq!(got.len(), want.len(), "{path}: track count");
            for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                if g != w {
                    let diff = g.chars().zip(w.chars()).position(|(a, b)| a != b).unwrap_or(0);
                    let from = diff.saturating_sub(20);
                    panic!(
                        "{path} track {i} mismatch at {diff}:\n got ...{}\nwant ...{}",
                        &g[from..(from + 80).min(g.len())],
                        &w[from..(from + 80).min(w.len())],
                    );
                }
            }
            files += 1;
            tracks += got.len();
        }
        eprintln!("verified {files} files, {tracks} tracks");
        assert!(files >= 40);
    }
}
