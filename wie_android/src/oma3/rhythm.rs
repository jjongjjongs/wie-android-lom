//! The built-in rhythm bank, ported from the reference `OracleMa3BuiltinRhythm`.
//!
//! Percussion keys map to a fixed pitch and either an FM voice (type 0) or a
//! recorded wave (type 1). The FM voices are decoded here through the same
//! [`super::tone::decode_ma3_dll81_tone`] the reference uses, so tone-driven
//! drums render through the ported synth like any other note. The wave records
//! are recorded percussion and belong to the streamed-audio path, which the
//! port does not decode yet.

use super::synth::CompactTone;
use super::tone::decode_ma3_dll81_tone;

static FIXED_KEY: &[u8; 128] = include_bytes!("data/rhythm_fixedkey.bin");
static TYPE: &[u8; 128] = include_bytes!("data/rhythm_type.bin");
/// 128 records, each a two-byte little-endian length then that many bytes; only
/// the type-0 (FM) keys carry a record.
static FM_RECORDS: &[u8] = include_bytes!("data/rhythm_fm.bin");

/// `fixedKey` - the pitch a percussion key always plays at.
pub fn fixed_key(key: i32) -> i32 {
    FIXED_KEY[(key & 127) as usize] as i32
}

/// `type` - 0 is an FM voice, 1 a recorded wave, otherwise unused.
pub fn kind(key: i32) -> i32 {
    TYPE[(key & 127) as usize] as i32
}

fn fm_record(key: usize) -> &'static [u8] {
    let mut offset = 0;
    for k in 0..128 {
        let len = FM_RECORDS[offset] as usize | (FM_RECORDS[offset + 1] as usize) << 8;
        let start = offset + 2;
        if k == key {
            return &FM_RECORDS[start..start + len];
        }
        offset = start + len;
    }
    &[]
}

/// `fmTone` - the FM voice for a type-0 percussion key, or `None`.
pub fn fm_tone(key: i32) -> Option<CompactTone> {
    let key = (key & 127) as usize;
    if kind(key as i32) != 0 {
        return None;
    }
    let record = fm_record(key);
    if record.len() < 81 {
        return None;
    }
    decode_ma3_dll81_tone(key as i32, record).ok()
}

/// Whether a percussion key carries a recorded wave (deferred audio path).
pub fn has_wave_record(key: i32) -> bool {
    kind(key) == 1
}
