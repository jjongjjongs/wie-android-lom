//! PCM voices, for the parts of a title the chip played back rather than
//! synthesised.
//!
//! An MA-3 handset kept a small bank of recordings in ROM: a melodic set a
//! file could name instead of programming a voice, and the drums behind
//! twenty one keys of the kit. Those were 4-bit Yamaha ADPCM, decoded to
//! eight kilohertz mono and pitched by playing them back faster or slower.
//!
//! This module is that path. The banks and the per-key records are the
//! handset's own, in `data/adpcm_banks.bin` and `data/rhythm_wave.bin`; the
//! envelope, key scaling and low frequency oscillator tables are the same ones
//! [`super::tables`] holds for the synthesised voices, because the chip shared
//! them. What comes out is the recording, resampled and enveloped, in the same
//! normalised form a synthesised [`super::voice::Voice`] returns, so the mixer
//! treats the two alike.

use std::sync::{Arc, OnceLock};

use crate::ma3::tables::{
    ATTACK_RATE_Q31, DECAY_RATE_Q30, KEYLEVEL_Q15, KEYLEVEL_SELECTOR_MAP, LEVEL_Q15, LFO_STEP_Q20, SUSTAIN_Q31, lfo_level_q15, lfo_pitch_q20,
};

/// One WAVE record, as the kit and the file's sample voices carry it.
pub const RECORD_LEN: usize = 40;

/// Rate the handset's own player rendered at, which the record's playback step
/// is given against. The step is rescaled to whatever rate the synthesiser
/// runs at so a recording keeps its pitch.
const REFERENCE_RATE: f64 = 48000.0;

/// The four bit ADPCM step-size multipliers, Q14.
const ADPCM_STEP: [i64; 8] = [14720, 14720, 14720, 14720, 19648, 26176, 32768, 39296];

/// Decodes 4-bit Yamaha ADPCM, two mono samples to the byte, low nibble first.
pub fn decode_adpcm4_mono(src: &[u8]) -> Vec<i16> {
    let mut out = Vec::with_capacity(src.len() * 2);
    let mut step: i64 = 127;
    let mut predictor: i64 = 0;

    for &byte in src {
        for nibble in [byte & 0x0f, byte >> 4] {
            let code = nibble as i64;

            let mut delta = step >> 3;
            if code & 1 != 0 {
                delta += step >> 2;
            }
            if code & 2 != 0 {
                delta += step >> 1;
            }
            if code & 4 != 0 {
                delta += step;
            }
            if code & 8 != 0 {
                delta = -delta;
            }

            predictor = (predictor + delta).clamp(-32768, 32767);
            out.push(predictor as i16);

            step = ((ADPCM_STEP[(code & 7) as usize] * step) >> 14).clamp(127, 24576);
        }
    }

    out
}

/// The built in banks, keyed by id (128..=134), decoded once on first use.
struct Banks {
    banks: Vec<(u32, Arc<[i16]>)>,
}

static BANKS: OnceLock<Banks> = OnceLock::new();

static ADPCM_BANKS: &[u8] = include_bytes!("data/adpcm_banks.bin");

fn banks() -> &'static Banks {
    BANKS.get_or_init(|| {
        let data = ADPCM_BANKS;
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

        // Header: a count, then an (id, length) pair per bank, then the blobs.
        let mut ids = Vec::with_capacity(count);
        let mut offset = 4;
        for _ in 0..count {
            let id = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
            let len = u32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]) as usize;
            ids.push((id, len));
            offset += 8;
        }

        let mut banks = Vec::with_capacity(count);
        for (id, len) in ids {
            let pcm = decode_adpcm4_mono(&data[offset..offset + len]);
            banks.push((id, Arc::from(pcm.into_boxed_slice())));
            offset += len;
        }

        Banks { banks }
    })
}

/// The decoded recording for a bank id, or nothing if the id is unknown.
fn pcm_for_bank(id: u32) -> Option<Arc<[i16]>> {
    banks().banks.iter().find(|(bank, _)| *bank == id).map(|(_, pcm)| Arc::clone(pcm))
}

/// The kit's WAVE records, one per key, forty bytes each. A key with no
/// recording is all zeroes.
static RHYTHM_WAVE: &[u8; 128 * RECORD_LEN] = include_bytes!("data/rhythm_wave.bin");

/// The WAVE record a drum key names, if the kit plays it from a recording.
pub fn drum_wave_record(key: u8) -> Option<&'static [u8]> {
    let record = &RHYTHM_WAVE[(key & 127) as usize * RECORD_LEN..][..RECORD_LEN];
    record.iter().any(|&byte| byte != 0).then_some(record)
}

fn le32(bytes: &[u8], at: usize) -> i64 {
    i64::from(u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]))
}

/// Where a note sits in the chip's key scaling, from its playback step. The
/// faster a recording plays the higher it is taken to sound.
fn key_scale(step_q16: i32) -> i32 {
    let value = step_q16.clamp(2048, 65536);
    let mut octave = 0;
    let mut fraction = 0;
    while octave < 8 {
        let candidate = (value >> (octave + 1)) - 1024;
        fraction = candidate;
        if (0..1024).contains(&candidate) {
            break;
        }
        octave += 1;
    }
    ((fraction >> 9) | (octave << 1)) & 255
}

/// The key scaling attenuation for the level, as Q15, or unity when the record
/// selects no curve.
fn key_scale_level(step_q16: i32, selector: u8) -> i32 {
    if selector == 0 {
        return 32768;
    }

    let value = step_q16.clamp(2048, 65536);
    let mut octave = 0;
    let mut fraction = 0;
    while octave < 8 {
        let candidate = (value >> (octave + 1)) - 1024;
        fraction = candidate;
        if (0..1024).contains(&candidate) {
            break;
        }
        octave += 1;
    }

    let index = (((fraction >> 6) & 15) << 3) | (octave & 7);
    KEYLEVEL_Q15[(((selector as usize - 1) & 3) * 128) + (index as usize & 127)]
}

/// The envelope rate table index, from a record's raw rate and the key scale.
fn rate_index(raw: u8, key_scale: i32) -> usize {
    let mut index = (raw as i32 & 255) * 4;
    if index != 0 {
        index += key_scale;
    }
    index.min(63) as usize
}

/// Envelope stages, matching the order the chip stepped through them.
const ENV_RELEASE: u8 = 1;
const ENV_START: u8 = 2;
const ENV_ATTACK: u8 = 3;
const ENV_DECAY1: u8 = 4;
const ENV_DECAY2: u8 = 5;

/// One sounding recording.
pub struct WaveVoice {
    pcm: Arc<[i16]>,
    position: f64,
    step: f64,
    sample_end: usize,
    loop_start: usize,
    loops: bool,

    env_state: u8,
    env_q31: u64,
    attack_q31: u64,
    decay1_q30: u64,
    decay2_q30: u64,
    release_q30: u64,
    sustain_q31: u64,
    level_q15: i32,

    amp_lfo: bool,
    amp_depth: usize,
    pitch_lfo: bool,
    pitch_depth: usize,
    lfo_phase_q20: u32,
    lfo_step_q20: u32,

    active: bool,
}

impl WaveVoice {
    /// Builds a voice for a drum key, if the key is one the kit recorded.
    pub fn drum(key: u8, velocity: u8, sample_rate: u32) -> Option<Self> {
        let record = drum_wave_record(key)?;
        Self::from_record(record, velocity, sample_rate)
    }

    /// Builds a voice from a forty byte WAVE record and the recording its bank
    /// field names.
    pub fn from_record(record: &[u8], velocity: u8, sample_rate: u32) -> Option<Self> {
        if record.len() < RECORD_LEN {
            return None;
        }

        // The bank id is a single byte at offset 32.
        let pcm = pcm_for_bank(record[32] as u32)?;
        if pcm.len() < 2 {
            return None;
        }
        let _ = velocity;

        let mut sample_end = le32(record, 28) as usize;
        if sample_end == 0 || sample_end >= pcm.len() {
            sample_end = pcm.len() - 1;
        }

        let mut loop_start = le32(record, 24) as usize;
        if loop_start > sample_end {
            loop_start = sample_end.saturating_sub(1);
        }
        let loops = loop_start < sample_end;

        // The record's playback step is Q16 samples per reference frame; rescale
        // it to this synthesiser's rate so the recording keeps its pitch.
        let mut base_step_q16 = le32(record, 36) as i32;
        if base_step_q16 == 0 {
            base_step_q16 = 65536;
        }
        let step = (base_step_q16 as f64 / 65536.0) * (REFERENCE_RATE / sample_rate as f64);

        let key_scale = key_scale(base_step_q16);
        let level_q15 =
            (LEVEL_Q15[(record[14] & 63) as usize] * key_scale_level(base_step_q16, KEYLEVEL_SELECTOR_MAP[(record[15] & 3) as usize])) >> 15;

        Some(Self {
            pcm,
            position: 0.0,
            step,
            sample_end,
            loop_start,
            loops,

            env_state: ENV_START,
            env_q31: 0,
            attack_q31: ATTACK_RATE_Q31[rate_index(record[12], key_scale)],
            decay1_q30: DECAY_RATE_Q30[rate_index(record[11], key_scale)],
            decay2_q30: DECAY_RATE_Q30[rate_index(record[9], key_scale)],
            release_q30: DECAY_RATE_Q30[rate_index(record[10], key_scale)],
            sustain_q31: SUSTAIN_Q31[(record[13] & 15) as usize],
            level_q15,

            amp_lfo: record[19] != 0,
            amp_depth: (record[18] & 3) as usize,
            pitch_lfo: record[7] != 0,
            pitch_depth: (record[6] & 3) as usize,
            lfo_phase_q20: 0,
            lfo_step_q20: LFO_STEP_Q20[(record[3] & 3) as usize],

            active: true,
        })
    }

    fn step_envelope(&mut self) {
        match self.env_state {
            ENV_RELEASE => {
                self.env_q31 = (self.release_q30 * self.env_q31) >> 30;
                if self.env_q31 == 0 {
                    self.active = false;
                }
            }
            ENV_START => {
                self.position = 0.0;
                self.env_state = ENV_ATTACK;
                self.env_q31 += self.attack_q31;
                if self.env_q31 > i32::MAX as u64 {
                    self.env_q31 = 1 << 31;
                    self.env_state = ENV_DECAY1;
                }
            }
            ENV_ATTACK => {
                self.env_q31 += self.attack_q31;
                if self.env_q31 > i32::MAX as u64 {
                    self.env_q31 = 1 << 31;
                    self.env_state = ENV_DECAY1;
                }
            }
            ENV_DECAY1 => {
                self.env_q31 = (self.decay1_q30 * self.env_q31) >> 30;
                if self.env_q31 <= self.sustain_q31 {
                    self.env_state = ENV_DECAY2;
                }
            }
            ENV_DECAY2 => {
                self.env_q31 = (self.decay2_q30 * self.env_q31) >> 30;
                if self.env_q31 == 0 {
                    self.active = false;
                }
            }
            _ => self.active = false,
        }
    }

    fn normalize_position(&mut self) {
        if self.position < self.sample_end as f64 {
            return;
        }

        if self.loops {
            let span = (self.sample_end - self.loop_start) as f64;
            if span <= 0.0 {
                self.active = false;
                return;
            }
            while self.position >= self.sample_end as f64 {
                self.position -= span;
            }
            if self.position < self.loop_start as f64 {
                self.position = self.loop_start as f64;
            }
        } else {
            // A one shot recording has no note off to wait for and no duration
            // to run against here, so it ends when it runs out rather than
            // holding its last sample under a slow envelope forever.
            self.position = self.sample_end as f64;
            self.active = false;
        }
    }

    /// One mono sample, normalised to roughly minus one to one, the same form a
    /// synthesised voice returns so the mixer can pan and mix them alike.
    pub fn sample(&mut self) -> f64 {
        if !self.active {
            return 0.0;
        }

        self.step_envelope();
        if !self.active {
            return 0.0;
        }
        self.normalize_position();
        if !self.active {
            return 0.0;
        }

        let index = self.position as usize;
        let next = if index + 1 > self.sample_end {
            if self.loops { self.loop_start } else { index }
        } else {
            index + 1
        };

        let here = self.pcm[index] as f64;
        let there = self.pcm[next] as f64;
        let interpolated = here + (there - here) * (self.position - index as f64);

        let mut level = self.level_q15;
        if self.amp_lfo {
            level = (level * lfo_level_q15(self.amp_depth, (self.lfo_phase_q20 >> 20) as usize)) >> 15;
        }
        let env_level = (((self.env_q31 >> 16) * level as u64) >> 15) as f64;
        let output = (interpolated / 32768.0) * (env_level / 32768.0);

        let mut advance = self.step;
        if self.pitch_lfo {
            advance *= lfo_pitch_q20(self.pitch_depth, (self.lfo_phase_q20 >> 20) as usize) as f64 / 1_048_576.0;
        }
        self.position += advance;
        self.lfo_phase_q20 = self.lfo_phase_q20.wrapping_add(self.lfo_step_q20);

        output
    }

    pub fn audible(&self) -> bool {
        self.active
    }

    pub fn loudness(&self) -> f64 {
        self.env_q31 as f64
    }

    pub fn release(&mut self) {
        if self.env_state != ENV_RELEASE {
            self.env_state = ENV_RELEASE;
        }
    }

    pub fn all_sound_off(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{WaveVoice, banks, decode_adpcm4_mono, drum_wave_record};
    use crate::ma3::SAMPLE_RATE;

    #[test]
    fn adpcm_decodes_two_samples_per_byte_in_range() {
        let pcm = decode_adpcm4_mono(&[0x00, 0x88, 0x4c, 0xff]);
        assert_eq!(pcm.len(), 8);
        assert!(pcm.iter().all(|&s| (i16::MIN..=i16::MAX).contains(&s)));
    }

    #[test]
    fn all_seven_banks_decode_to_audio() {
        for (id, pcm) in &banks().banks {
            assert!((128..=134).contains(id), "unexpected bank id {id}");
            assert!(pcm.len() > 2, "bank {id} decoded to nothing");
        }
        assert_eq!(banks().banks.len(), 7);
    }

    #[test]
    fn twenty_one_drum_keys_are_recorded() {
        let recorded = (0..128u8).filter(|&key| drum_wave_record(key).is_some()).count();
        assert_eq!(recorded, 21, "the kit records twenty one keys");
    }

    #[test]
    fn a_recorded_drum_makes_a_sound_and_ends() {
        // Key 36 is the bass drum in the kit.
        let mut voice = WaveVoice::drum(36, 100, SAMPLE_RATE).expect("key 36 is recorded");

        let mut heard = false;
        for _ in 0..SAMPLE_RATE as usize {
            let sample = voice.sample();
            if sample.abs() > 0.0 {
                heard = true;
            }
            if !voice.audible() {
                break;
            }
        }

        assert!(heard, "the drum never made a sound");
        assert!(!voice.audible(), "the drum never finished");
    }
}
