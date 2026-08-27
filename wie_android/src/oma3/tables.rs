//! Data tables for the faithful port of the reference MA-3 renderer
//! (`com.keitaiwiki.music.OracleMa3Synth`).
//!
//! The envelope, level, sustain, key-level and phase-detune tables are
//! byte-for-byte identical to the ones the existing [`crate::ma3`] engine
//! already carries, so they are re-exported rather than transcribed again. The
//! tables that differ - the low-frequency oscillator, the operator frequency
//! multiples and their fine detune in cents, and the note-to-hertz base - are
//! defined here from the reference's own values, because the current engine
//! either stores them in a different form or does not use them at all.

// Verified identical to the reference `OracleMa3Synth` arrays of the same name.
pub use crate::ma3::tables::{
    ATTACK_RATE_Q31, DECAY_RATE_Q30, KEYLEVEL_Q15, LEVEL_Q15, LFO_STEP_Q20, PHASE_DETUNE, SUSTAIN_Q31,
};

/// Wave table: 32 recordings of 1024 samples each, sixteen bit. Identical to
/// the recordings [`crate::ma3`] carries.
pub const WAVE_COUNT: usize = 32;
pub const WAVE_SIZE: usize = 1024;
static WAVES: &[u8; WAVE_COUNT * WAVE_SIZE * 2] = include_bytes!("data/waves.bin");

/// One sample of `waveform`, at `index` within its period, as the reference's
/// `OracleMa3WaveTables.sampleI16AtIndex`.
pub fn wave_sample_i16(waveform: usize, index: usize) -> i32 {
    let offset = ((waveform & (WAVE_COUNT - 1)) * WAVE_SIZE + (index & (WAVE_SIZE - 1))) * 2;
    i16::from_le_bytes([WAVES[offset], WAVES[offset + 1]]) as i32
}

/// Low frequency oscillator, four depths of 4096 phase steps. The reference
/// stores the level as an unsigned sixteen-bit value and the pitch as a signed
/// thirty-two-bit one, which is why these differ from the current engine's.
const LFO_DEPTHS: usize = 4;
const LFO_TABLE_SIZE: usize = 4096;
static LFO_LEVEL: &[u8; LFO_DEPTHS * LFO_TABLE_SIZE * 2] = include_bytes!("data/lfo_level.bin");
static LFO_PITCH: &[u8; LFO_DEPTHS * LFO_TABLE_SIZE * 4] = include_bytes!("data/lfo_pitch.bin");

/// `OracleMa3LfoTables.levelQ15(depth, phase)` - the tremolo depth, unsigned.
pub fn lfo_level_q15(depth: usize, phase: usize) -> i32 {
    let offset = ((depth & (LFO_DEPTHS - 1)) * LFO_TABLE_SIZE + (phase & (LFO_TABLE_SIZE - 1))) * 2;
    u16::from_le_bytes([LFO_LEVEL[offset], LFO_LEVEL[offset + 1]]) as i32
}

/// `OracleMa3LfoTables.pitchQ20(depth, phase)` - the vibrato depth, signed.
pub fn lfo_pitch_q20(depth: usize, phase: usize) -> i64 {
    let offset = ((depth & (LFO_DEPTHS - 1)) * LFO_TABLE_SIZE + (phase & (LFO_TABLE_SIZE - 1))) * 4;
    i32::from_le_bytes([LFO_PITCH[offset], LFO_PITCH[offset + 1], LFO_PITCH[offset + 2], LFO_PITCH[offset + 3]]) as i64
}

/// Operator frequency as a multiple of the note's, from the reference's
/// `MULTIPLE`. Fractional entries (0.5, 10.5, 13.5) are the reason the current
/// engine's integer `MULTIPLE_MAP` sounds wrong.
pub const MULTIPLE: [f64; 16] = [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 13.5, 15.0];

/// Fine detune per operator, in cents, from the reference's `DETUNE_CENTS`.
pub const DETUNE_CENTS: [f64; 16] = [-15.0, -11.0, -7.0, -3.0, 0.0, 3.0, 7.0, 11.0, 15.0, -24.0, -18.0, -12.0, 12.0, 18.0, 24.0, 0.0];

/// Feedback gain per level, from the reference's `FEEDBACK_GAIN`.
pub const FEEDBACK_GAIN: [f64; 8] = [0.0, 0.026, 0.052, 0.092, 0.145, 0.22, 0.32, 0.47];

/// Feedback shift per seed, from `feedbackShiftFromSeed`.
pub const FEEDBACK_SHIFT: [i32; 8] = [0, 8, 7, 6, 5, 4, 3, 2];

/// The four-step modulation wheel depths and the phase key-code table the
/// reference reads in `phaseKeyCodeFromTail`.
pub const PHASE_KEY_CODE: [i32; 16] = [0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3];

/// Note number to hertz, from the reference's `MA3_FREQ_BASE` (128 entries).
pub const MA3_FREQ_BASE: [f64; 128] = [
    8.2, 8.7, 9.2, 9.7, 10.3, 10.9, 11.6, 12.3, 13.0, 13.8, 14.6, 15.4, 16.4, 17.3, 18.4, 19.4, 20.6, 21.8, 23.1, 24.5, 26.0, 27.5, 29.1, 30.9, 32.7,
    34.7, 36.7, 38.9, 41.2, 43.7, 46.3, 49.0, 51.9, 55.0, 58.3, 61.7, 65.4, 69.3, 73.4, 77.8, 82.4, 87.3, 92.5, 98.0, 103.8, 110.0, 116.6, 123.5,
    130.8, 138.6, 146.9, 155.6, 164.8, 174.6, 185.0, 196.0, 207.7, 220.0, 233.1, 247.0, 261.6, 277.2, 293.7, 311.1, 329.6, 349.2, 370.0, 392.0, 415.3,
    440.0, 466.2, 493.9, 523.2, 554.4, 587.4, 622.2, 659.2, 698.4, 740.0, 784.0, 830.6, 880.0, 932.4, 987.8, 1046.4, 1108.8, 1174.8, 1244.4, 1318.4,
    1396.8, 1480.0, 1568.0, 1661.2, 1760.0, 1864.8, 1975.6, 2092.8, 2217.6, 2349.6, 2488.8, 2636.8, 2793.6, 2960.0, 3136.0, 3322.4, 3520.0, 3729.6,
    3951.2, 4185.6, 4435.2, 4699.2, 4977.6, 5273.6, 5587.2, 5920.0, 6272.0, 6644.8, 7040.0, 7459.2, 7902.4, 8371.2, 8870.4, 9398.4, 9955.2, 10547.2,
    11174.4, 11840.0, 12544.0,
];
