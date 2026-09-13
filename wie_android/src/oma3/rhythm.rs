//! The built-in rhythm bank, ported from the reference `OracleMa3BuiltinRhythm`.
//!
//! Percussion keys map to a fixed pitch and either an FM voice (type 0) or a
//! recorded wave (type 1). The FM voices are decoded here through the same
//! [`super::tone::decode_ma3_dll81_tone`] the reference uses, so tone-driven
//! drums render through the ported synth like any other note. The wave records
//! name one of seven recorded percussion banks and a length; the banks are
//! Yamaha ADPCM, decoded through [`super::audio`] into the recording a wave
//! rhythm key plays back.

use super::audio::{DecodedAudioSample, decode_yamaha_adpcm4_mono};
use super::synth::CompactTone;
use super::tone::decode_ma3_dll81_tone;

static FIXED_KEY: &[u8; 128] = include_bytes!("data/rhythm_fixedkey.bin");
static TYPE: &[u8; 128] = include_bytes!("data/rhythm_type.bin");
/// 128 records, each a two-byte little-endian length then that many bytes; only
/// the type-0 (FM) keys carry a record.
static FM_RECORDS: &[u8] = include_bytes!("data/rhythm_fm.bin");
/// 128 records, each a two-byte little-endian length then that many bytes; only
/// the type-1 (wave) keys carry a record. Byte 32 names the bank, bytes 28..32
/// the playback length.
static WAVE_RECORDS: &[u8] = include_bytes!("data/rhythm_wave.bin");
/// Seven ADPCM percussion banks (ids 128..=134), each a four-byte
/// little-endian length then that many bytes.
static WAVE_BANKS: &[u8] = include_bytes!("data/wave_banks.bin");
const BANK_IDS: [i32; 7] = [128, 129, 130, 131, 132, 133, 134];

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

/// Whether a percussion key carries a recorded wave.
pub fn has_wave_record(key: i32) -> bool {
    kind(key) == 1
}

/// One length-prefixed record out of a `[len_bytes][data]` blob.
fn indexed_record(blob: &[u8], key: usize, len_bytes: usize) -> &[u8] {
    let mut offset = 0;
    for k in 0..128 {
        let mut len = 0usize;
        for (i, &b) in blob[offset..offset + len_bytes].iter().enumerate() {
            len |= (b as usize) << (8 * i);
        }
        let start = offset + len_bytes;
        if k == key {
            return &blob[start..start + len];
        }
        offset = start + len;
        if offset >= blob.len() {
            break;
        }
    }
    &[]
}

/// `waveRecord` - the raw wave record for a type-1 percussion key, or empty.
fn wave_record(key: usize) -> &'static [u8] {
    indexed_record(WAVE_RECORDS, key, 2)
}

/// `waveRecord` for an arbitrary key value, for the analysis's built-in
/// wave-rhythm event path.
pub fn wave_record_for_key(key: i32) -> Vec<u8> {
    wave_record((key & 127) as usize).to_vec()
}

fn read_le32(data: &[u8], at: usize) -> i32 {
    (data[at] as i32 & 0xFF) | (data[at + 1] as i32 & 0xFF) << 8 | (data[at + 2] as i32 & 0xFF) << 16 | (data[at + 3] as i32 & 0xFF) << 24
}

/// `pcmForBank` - the decoded PCM of one recorded percussion bank.
fn pcm_for_bank(bank_id: i32) -> Vec<i16> {
    let Some(index) = BANK_IDS.iter().position(|&id| id == bank_id) else {
        return Vec::new();
    };
    let mut offset = 0;
    for i in 0..7 {
        let len = read_le32(WAVE_BANKS, offset) as usize;
        let start = offset + 4;
        if i == index {
            return decode_yamaha_adpcm4_mono(&WAVE_BANKS[start..start + len], 0, len);
        }
        offset = start + len;
    }
    Vec::new()
}

/// `OracleMa3BuiltinWaveSamples.sampleForRecord` - the recording a wave rhythm
/// key plays: the named bank's PCM, truncated to the record's length.
fn sample_for_record(key: i32, record: &[u8]) -> Option<DecodedAudioSample> {
    if record.len() < 33 {
        return None;
    }
    let pcm = pcm_for_bank(record[32] as i32 & 255);
    if pcm.is_empty() {
        return None;
    }
    let length = read_le32(record, 28);
    let pcm = if length >= 0 {
        let end = (length + 1).max(0) as usize;
        if end > 0 && end < pcm.len() { pcm[..end].to_vec() } else { pcm }
    } else {
        pcm
    };
    Some(DecodedAudioSample {
        audio_id: -1,
        sample_id: key & 127,
        sample_rate: 8000,
        pcm_mono: pcm,
    })
}

/// `OracleMa3BuiltinWaveSamples.sampleForRecord` applied to an arbitrary wave
/// record - the recording a direct type-2 wave tone plays: the record's named
/// bank truncated to the record's length.
pub fn wave_sample_for_record(key: i32, record: &[u8]) -> Option<DecodedAudioSample> {
    sample_for_record(key & 127, record)
}

/// The recording a wave rhythm key plays back, or `None` if the key carries no
/// wave.
pub fn wave_sample(key: i32) -> Option<DecodedAudioSample> {
    let key = key & 127;
    if kind(key) != 1 {
        return None;
    }
    sample_for_record(key, wave_record(key as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in wave-rhythm key must decode to the same recording the
    /// reference produces: same rate, length, and leading samples.
    #[test]
    fn wave_samples_match_the_reference() {
        let fixture = include_str!("data/wave_sample_vectors.txt");
        let mut checked = 0;
        for line in fixture.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let mut cols = line.split_whitespace();
            let key: i32 = cols.next().unwrap().parse().unwrap();
            let rate: i32 = cols.next().unwrap().parse().unwrap();
            let len: usize = cols.next().unwrap().parse().unwrap();
            let first: Vec<i16> = cols.next().unwrap().split(',').map(|v| v.parse().unwrap()).collect();
            let sample = wave_sample(key).unwrap_or_else(|| panic!("key {key}: no wave sample"));
            assert_eq!(sample.sample_rate, rate, "key {key} rate");
            assert_eq!(sample.pcm_mono.len(), len, "key {key} length");
            for (i, &f) in first.iter().enumerate() {
                assert_eq!(sample.pcm_mono[i], f, "key {key} sample {i}");
            }
            checked += 1;
        }
        assert_eq!(checked, 21, "all type-1 keys");
    }
}
