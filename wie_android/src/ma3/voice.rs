//! One sounding note.
//!
//! An operator is a sine (or one of thirty one other shapes) read out of a
//! table, an envelope on its amplitude, and a phase that another operator can
//! bend. The voice's algorithm says which operators are heard and which only
//! bend the next; between two and four of them make a note.
//!
//! The arithmetic is the chip's: phase is Q32, the envelope Q31, decay a Q30
//! multiplier applied once a sample, and every operator's output is rounded to
//! sixteen bits before the next one sees it. Doing this in floating point
//! would sound close but not the same, and the rounding is a good part of why
//! a handset's music has the timbre it does.

use crate::ma3::{
    tables::{
        ATTACK_RATE_Q31, DECAY_RATE_Q30, FEEDBACK_GAIN, FEEDBACK_SHIFT, KEYLEVEL_Q15, LEVEL_Q15, LFO_STEP_Q20, PHASE_DETUNE, PHASE_KEY_CODE,
        SUSTAIN_Q31, lfo_level_q15, lfo_pitch_q20, wave_sample,
    },
    tone::{OPERATORS, Operator, Tone},
};

/// The envelope runs from nothing to `1 << 31`.
const ENV_MAX_Q31: u64 = 1 << 31;

/// Phase is a full 32 bit turn, so it wraps on its own.
const PHASE_MAX: u64 = u32::MAX as u64;

/// A frequency ratio of one, in the Q20 the vibrato table is in.
const PITCH_UNITY_Q20: i64 = 1 << 20;

/// Amplitude of one, in the Q15 the tremolo table is in.
const LEVEL_UNITY_Q15: i32 = 1 << 15;

/// Sixteen bit full scale as the chip counts it, one short of the usual.
const FULL_SCALE: i32 = 32767;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Off,
    Release,
    KeyOn,
    Attack,
    Decay1,
    Decay2,
}

/// Where a note sits, in the forms the chip wanted it in. A note's pitch
/// decides not only how fast the phase turns but how quickly the envelope runs
/// and how much the key scaling quietens it, so all four come from one place.
#[derive(Clone, Copy)]
struct Pitch {
    /// Bent frequency, used only when the exact step could not be built.
    hz: f64,
    /// Four bit code the envelope rates are offset by.
    env_rate: i32,
    /// Seven bit code the key scaling curves are read at.
    keylevel_code: i32,
    /// Five bit code the detune table is read at.
    detune_key: i32,
    /// The phase step itself, before an operator's multiple and detune.
    step_q32: u64,
}

impl Pitch {
    fn new(note: u8, bend: u16, bend_range: u8, sample_rate: u32) -> Self {
        let note = note.min(127) as i32;
        let plain = note_hz(note, None, 2);
        let bent = note_hz(note, Some(bend), bend_range);

        let (shift, mantissa) = pitch_tail(plain, sample_rate);
        let shift_capped = shift.min(7);

        Self {
            hz: bent,
            env_rate: ((mantissa >> 9) & 1) | (shift_capped << 1),
            keylevel_code: ((mantissa >> 3) & 120) | shift_capped,
            detune_key: PHASE_KEY_CODE[((mantissa >> 6) & 15) as usize] + shift_capped * 4,
            step_q32: base_step(mantissa, shift, bend_scale_q16(plain, bent)),
        }
    }
}

/// Equal temperament from note 69 = A440, bent by the wheel when it is given.
fn note_hz(note: i32, bend: Option<u16>, bend_range: u8) -> f64 {
    let plain = 2f64.powf((note as f64 - 69.0) / 12.0) * 440.0;

    // A range past two octaves is a misread rather than an intention.
    let range = if bend_range > 24 { 2 } else { bend_range } as f64;
    let Some(bend) = bend else {
        return plain;
    };
    if range == 0.0 {
        return plain;
    }

    plain * 2f64.powf(((bend as f64 - 8192.0) * range / 8192.0) / 12.0)
}

fn step_from_hz(hz: f64, sample_rate: u32) -> u64 {
    if hz <= 0.0 || sample_rate == 0 {
        return 0;
    }

    (hz * 4294967296.0 / sample_rate as f64).round().clamp(0.0, PHASE_MAX as f64) as u64
}

/// Splits a frequency into the mantissa and shift the chip held it as. The
/// envelope and key scaling are indexed off these rather than off the
/// frequency, so they have to be derived the same way.
fn pitch_tail(hz: f64, sample_rate: u32) -> (i32, i32) {
    let mut step = step_from_hz(hz, sample_rate) >> 12;
    if step == 0 {
        step = 1;
    }
    step = step.min(130944);

    let mut shift = 0;
    while step > 1023 && shift < 7 {
        step >>= 1;
        shift += 1;
    }

    (shift, step.min(1023) as i32)
}

fn base_step(mantissa: i32, shift: i32, scale_q16: i64) -> u64 {
    let base = ((mantissa as u64) << (shift & 31)) << 12;
    let scale = if scale_q16 == 0 { 65536 } else { scale_q16 } as u64;

    ((base * scale) >> 16).min(PHASE_MAX)
}

fn bend_scale_q16(plain: f64, bent: f64) -> i64 {
    if plain <= 0.0 || bent <= 0.0 {
        return 65536;
    }

    (bent / plain * 65536.0).clamp(1.0, 262144.0).round() as i64
}

/// Applies an operator's own multiple and detune to the note's phase step.
fn step_with_multiple(base: u64, multiple: u8, detune: u8, detune_key: i32) -> u64 {
    let mut step = base & PHASE_MAX;

    if detune & 7 != 0 {
        let mut offset = PHASE_DETUNE[((detune & 3) as usize) * 32 + (detune_key & 31) as usize];
        if detune & 4 != 0 {
            offset = -offset;
        }

        // Detune is a fixed number of cents at a reference rate, so it is
        // applied as a ratio around that rate rather than to the step itself.
        let reference = (((step >> 16) * 48000) & PHASE_MAX).max(65536) as i64;
        let detuned = reference + offset;
        step = if detuned <= 0 {
            0
        } else {
            ((detuned as u128 * step as u128) / reference as u128) as u64 & PHASE_MAX
        };
    }

    match multiple & 15 {
        // Zero means half, which is the one ratio below unity.
        0 => step >> 1,
        multiple => (step * multiple as u64) & PHASE_MAX,
    }
}

/// Which envelope rate row a rate lands on, once the note has moved it.
fn rate_index(rate: u8, rate_scaling: bool, env_rate: i32) -> usize {
    let base = ((rate & 15) as i32) << 2;
    if base == 0 {
        return 0;
    }

    let offset = if rate_scaling { env_rate } else { env_rate >> 2 };

    (base + offset).clamp(0, 63) as usize
}

/// How long a release takes, in seconds, so a voice can be given a tail.
fn release_seconds(rate: u8, rate_scaling: bool, env_rate: i32, sample_rate: u32) -> f64 {
    let multiplier = DECAY_RATE_Q30[rate_index(rate, rate_scaling, env_rate)] as f64 / 1073741824.0;

    // A multiplier of one never decays, which the caller reads as "forever".
    if multiplier >= 0.999999999 {
        return 1.0e9;
    }
    if multiplier <= 0.0 {
        return 1.0 / sample_rate as f64;
    }

    -1.0 / (sample_rate as f64 * multiplier.ln())
}

fn output_level_q15(base: i32, keylevel: u8, keylevel_code: i32) -> i32 {
    let scale = match keylevel & 3 {
        0 => LEVEL_UNITY_Q15,
        selector => KEYLEVEL_Q15[((selector - 1) & 3) as usize * 128 + (keylevel_code & 127) as usize],
    };

    ((base * scale) >> 15).min(FULL_SCALE)
}

fn clip(value: i32) -> i32 {
    value.clamp(-FULL_SCALE, FULL_SCALE)
}

fn to_sample(value: i32) -> f64 {
    clip(value) as f64 / FULL_SCALE as f64
}

fn to_i16(value: f64) -> i32 {
    (value * FULL_SCALE as f64).round() as i32
}

/// One operator, mid note.
struct Slot {
    // Fixed for the life of the note.
    attack: u8,
    decay1: u8,
    decay2: u8,
    release: u8,
    rate_scaling: bool,
    sustain_q31: u64,
    base_level_q15: i32,
    keylevel: u8,
    multiple: u8,
    detune: u8,
    waveform: usize,
    feedback_gain: f64,
    feedback_shift: u32,
    tremolo: bool,
    tremolo_depth: usize,
    vibrato: bool,
    vibrato_depth: u8,
    /// Key off does not release this operator.
    hold: bool,
    carrier: bool,
    sample_rate: u32,
    fallback_hz: f64,

    // Recomputed whenever the note's pitch moves.
    step_q32: u64,
    attack_add_q31: u64,
    decay1_mul_q30: u64,
    decay2_mul_q30: u64,
    release_mul_q30: u64,
    level_q15: i32,
    release_seconds: f64,

    // Running state.
    phase_q32: u32,
    env_q31: u64,
    stage: Stage,
    prev: f64,
    prev2: f64,
    prev_i16: i32,
    prev2_i16: i32,
    last_i16: i32,
}

impl Slot {
    fn new(operator: &Operator, carrier: bool, pitch: &Pitch, sample_rate: u32) -> Self {
        let mut slot = Self {
            attack: operator.attack,
            decay1: operator.decay1,
            decay2: operator.decay2,
            release: operator.release,
            rate_scaling: operator.rate_scaling,
            sustain_q31: SUSTAIN_Q31[(operator.sustain & 15) as usize],
            base_level_q15: LEVEL_Q15[(operator.level & 63) as usize],
            keylevel: operator.keylevel,
            multiple: operator.multiple,
            detune: operator.detune,
            waveform: operator.waveform as usize,
            feedback_gain: FEEDBACK_GAIN[(operator.feedback & 7) as usize],
            feedback_shift: FEEDBACK_SHIFT[(operator.feedback & 7) as usize],
            tremolo: operator.tremolo,
            tremolo_depth: operator.tremolo_depth as usize,
            vibrato: operator.vibrato,
            vibrato_depth: operator.vibrato_depth,
            hold: operator.hold,
            carrier,
            sample_rate,
            fallback_hz: pitch.hz,

            step_q32: 0,
            attack_add_q31: 0,
            decay1_mul_q30: 0,
            decay2_mul_q30: 0,
            release_mul_q30: 0,
            level_q15: 0,
            release_seconds: 0.0,

            phase_q32: 0,
            env_q31: 0,
            stage: Stage::KeyOn,
            prev: 0.0,
            prev2: 0.0,
            prev_i16: 0,
            prev2_i16: 0,
            last_i16: 0,
        };
        slot.set_pitch(pitch);

        slot
    }

    fn set_pitch(&mut self, pitch: &Pitch) {
        let step = if pitch.step_q32 != 0 {
            step_with_multiple(pitch.step_q32, self.multiple, self.detune, pitch.detune_key)
        } else {
            let hz = if pitch.hz > 0.0 { pitch.hz } else { self.fallback_hz };

            step_from_hz(hz * self.ratio(pitch), self.sample_rate)
        };
        self.step_q32 = step;

        self.attack_add_q31 = ATTACK_RATE_Q31[rate_index(self.attack, self.rate_scaling, pitch.env_rate)];
        self.decay1_mul_q30 = DECAY_RATE_Q30[rate_index(self.decay1, self.rate_scaling, pitch.env_rate)];
        self.decay2_mul_q30 = if self.decay2 != 0 {
            DECAY_RATE_Q30[rate_index(self.decay2, self.rate_scaling, pitch.env_rate)]
        } else {
            1 << 30
        };
        self.release_mul_q30 = DECAY_RATE_Q30[rate_index(self.release, self.rate_scaling, pitch.env_rate)];
        self.level_q15 = output_level_q15(self.base_level_q15, self.keylevel, pitch.keylevel_code);
        self.release_seconds = release_seconds(self.release, self.rate_scaling, pitch.env_rate, self.sample_rate);
    }

    /// The operator's frequency as a ratio of the note's, for the path that
    /// has to work in hertz rather than in phase steps.
    fn ratio(&self, pitch: &Pitch) -> f64 {
        let detuned = if self.detune & 7 != 0 {
            let mut offset = PHASE_DETUNE[((self.detune & 3) as usize) * 32 + (pitch.detune_key & 31) as usize] as f64;
            if self.detune & 4 != 0 {
                offset = -offset;
            }

            let reference = (pitch.hz * 48000.0).max(65536.0);

            ((reference + offset) / reference).clamp(0.25, 2.5)
        } else {
            1.0
        };

        detuned * if self.multiple & 15 == 0 { 0.5 } else { (self.multiple & 15) as f64 }
    }

    fn audible(&self) -> bool {
        self.stage != Stage::Off && self.env_q31 > 0
    }

    fn release_now(&mut self) {
        if self.stage == Stage::Off || self.hold {
            return;
        }

        self.stage = Stage::Release;
    }

    fn all_sound_off(&mut self) {
        self.stage = Stage::Off;
        self.env_q31 = 0;
        self.reset_edges();
    }

    fn reset_edges(&mut self) {
        self.phase_q32 = 0;
        self.prev = 0.0;
        self.prev2 = 0.0;
        self.prev_i16 = 0;
        self.prev2_i16 = 0;
        self.last_i16 = 0;
    }

    fn add_attack(&mut self, current: u64) -> u64 {
        let next = current + self.attack_add_q31;
        if next < ENV_MAX_Q31 {
            return next;
        }

        self.stage = Stage::Decay1;

        ENV_MAX_Q31
    }

    fn step_envelope(&mut self) -> u64 {
        let current = self.env_q31;

        let next = match self.stage {
            Stage::Off => {
                self.env_q31 = 0;
                return 0;
            }
            Stage::Release => {
                // An operator written to ignore key off decays on its own
                // curve rather than on the release one.
                let multiplier = if self.hold { self.decay2_mul_q30 } else { self.release_mul_q30 };
                let next = (current * multiplier) >> 30;
                if next == 0 {
                    self.stage = Stage::Off;
                }

                next
            }
            Stage::KeyOn => {
                self.phase_q32 = 0;
                self.stage = Stage::Attack;

                self.add_attack(current)
            }
            Stage::Attack => self.add_attack(current),
            Stage::Decay1 => {
                let next = (current * self.decay1_mul_q30) >> 30;
                if next == 0 {
                    self.stage = Stage::Off;
                } else if next <= self.sustain_q31 {
                    self.stage = Stage::Decay2;
                }

                next
            }
            Stage::Decay2 => {
                let next = (current * self.decay2_mul_q30) >> 30;
                if next == 0 {
                    self.stage = Stage::Off;
                }

                next
            }
        };

        self.env_q31 = next.min(ENV_MAX_Q31);

        self.env_q31
    }

    /// One sample. `modulation` is what the operator ahead of this one put
    /// out, as a fraction of full scale; `feedback` says whether this slot
    /// also bends its own phase.
    fn run(&mut self, modulation: f64, feedback: bool, channel_modulation: usize, lfo_phase: usize, released: bool) {
        if released && !self.hold && self.stage != Stage::Off && self.stage != Stage::Release {
            self.stage = Stage::Release;
        }

        let envelope = self.step_envelope();

        let own = if !feedback {
            0.0
        } else if self.feedback_shift != 0 {
            ((self.prev_i16 + self.prev2_i16) >> self.feedback_shift) as f64 / FULL_SCALE as f64
        } else {
            (self.prev + self.prev2) * 0.5 * self.feedback_gain
        };

        // Vibrato depth is the operator's own, pushed up by however far the
        // channel's modulation wheel has been moved.
        let vibrato = if channel_modulation != 0 && self.vibrato {
            Some(((self.vibrato_depth & 3) as usize + channel_modulation - 1).min(3))
        } else {
            None
        };
        let pitch_q20 = vibrato.map(|depth| lfo_pitch_q20(depth, lfo_phase)).unwrap_or(PITCH_UNITY_Q20);
        let level_q15 = if self.tremolo {
            lfo_level_q15(self.tremolo_depth, lfo_phase).min(0xFFFF)
        } else {
            LEVEL_UNITY_Q15
        };

        let bend = modulation + own;
        let sample = wave_sample(self.waveform, wave_index(self.phase_q32, bend));
        let out = scale_sample(sample, envelope, self.level_q15, level_q15);

        self.prev2 = self.prev;
        self.prev = out as f64 / FULL_SCALE as f64;
        self.prev2_i16 = self.prev_i16;
        self.prev_i16 = out;
        self.last_i16 = out;

        let mut step = self.step_q32;
        if pitch_q20 != PITCH_UNITY_Q20 {
            step = ((step as i64 * pitch_q20) >> 20).clamp(0, PHASE_MAX as i64) as u64;
        }
        self.phase_q32 = self.phase_q32.wrapping_add(step as u32);
    }
}

/// Where in its period a wave is read, once the phase has been bent.
fn wave_index(phase_q32: u32, modulation: f64) -> usize {
    if modulation == 0.0 {
        return ((phase_q32 >> 22) & 1023) as usize;
    }

    let bent = ((to_i16(modulation) >> 1) + (phase_q32 >> 20) as i32) >> 2;

    (bent & 1023) as usize
}

/// Applies the envelope, the operator's level and the tremolo, all in the
/// order and the precision the chip applied them in.
fn scale_sample(sample: i32, envelope: u64, level_q15: i32, tremolo_q15: i32) -> i32 {
    let level = ((level_q15 as i64 & 0xFFFF) * (tremolo_q15 as i64 & 0xFFFF)) >> 15;
    let gain = (((envelope >> 16) as i64) * level) >> 15;

    (((clip(sample) as i64) * gain) >> 15).clamp(-FULL_SCALE as i64, FULL_SCALE as i64) as i32
}

/// Whether an operator is heard directly under a given algorithm, rather than
/// only bending the phase of the next.
fn is_carrier(operator_count: usize, algorithm: u8, index: usize) -> bool {
    if operator_count <= 2 || algorithm < 2 {
        return algorithm & 1 != 0 || index == 1;
    }

    match algorithm & 7 {
        2 => true,
        3 | 4 => index == 3,
        5 => index == 1 || index == 3,
        6 => index == 0 || index == 3,
        7 => index == 0 || index == 2 || index == 3,
        _ => index == operator_count - 1,
    }
}

/// The algorithms where the third operator feeds back as well as the first.
fn feeds_back_at_two(algorithm: u8) -> bool {
    matches!(algorithm & 7, 2 | 5)
}

/// A note in flight.
///
/// The voice is mono: velocity, volume and pan are not its business but the
/// output stage's, which attenuates rather than multiplies. See [`crate::ma3::bus`].
pub struct Voice {
    algorithm: u8,
    operator_count: usize,
    slots: Vec<Slot>,
    lfo_phase_q20: u32,
    lfo_step_q20: u32,
    channel_modulation: usize,
    released: bool,
    /// Samples since key off, so an operator written never to release still
    /// gives its voice back. A handset had the sequence's own end to stop it;
    /// played live there is nothing to bound it but this.
    since_release: u32,
    tail_frames: u32,
}

impl Voice {
    pub fn new(tone: &Tone, note: u8, bend: u16, bend_range: u8, modulation: usize, sample_rate: u32) -> Self {
        let pitch = Pitch::new(note, bend, bend_range, sample_rate);
        let algorithm = tone.algorithm & 7;
        let operator_count = tone.operator_count.clamp(2, OPERATORS);

        let slots = (0..operator_count)
            .map(|index| {
                let carrier = is_carrier(operator_count, algorithm, index);

                Slot::new(&tone.operators[index], carrier, &pitch, sample_rate)
            })
            .collect::<Vec<_>>();

        let tail = slots.iter().map(|x| x.release_seconds).fold(0.05f64, f64::max);

        Self {
            algorithm,
            operator_count,
            slots,
            lfo_phase_q20: 0,
            lfo_step_q20: LFO_STEP_Q20[(tone.lfo_speed & 63) as usize],
            channel_modulation: modulation.min(4),
            released: false,
            since_release: 0,
            tail_frames: tail_frames(tail, sample_rate),
        }
    }

    pub fn set_modulation(&mut self, modulation: usize) {
        self.channel_modulation = modulation.min(4);
    }

    pub fn set_pitch(&mut self, note: u8, bend: u16, bend_range: u8, sample_rate: u32) {
        let pitch = Pitch::new(note, bend, bend_range, sample_rate);

        for slot in &mut self.slots {
            slot.set_pitch(&pitch);
        }
    }

    pub fn release(&mut self) {
        self.released = true;
        self.since_release = 0;

        for slot in &mut self.slots {
            slot.release_now();
        }
    }

    pub fn all_sound_off(&mut self) {
        for slot in &mut self.slots {
            slot.all_sound_off();
        }
    }

    pub fn audible(&self) -> bool {
        self.slots.iter().any(|x| x.carrier && x.audible())
    }

    /// Roughly how loud the voice is, for choosing which to steal.
    pub fn loudness(&self) -> f64 {
        let envelope = self.slots.iter().filter(|x| x.carrier).map(|x| x.env_q31).max().unwrap_or(0);

        envelope as f64 / ENV_MAX_Q31 as f64
    }

    /// One sample, before the output stage.
    pub fn sample(&mut self) -> f64 {
        if self.released {
            self.since_release = self.since_release.saturating_add(1);
            if self.since_release > self.tail_frames {
                self.all_sound_off();

                return 0.0;
            }
        }

        let lfo_phase = ((self.lfo_phase_q20 >> 20) & 4095) as usize;

        let mixed = if self.operator_count <= 2 || self.algorithm < 2 {
            self.two_operator(lfo_phase)
        } else {
            self.four_operator(lfo_phase)
        };

        self.lfo_phase_q20 = self.lfo_phase_q20.wrapping_add(self.lfo_step_q20);

        mixed
    }

    fn two_operator(&mut self, lfo_phase: usize) -> f64 {
        self.run(0, 0.0, true, lfo_phase);

        // The low bit of the algorithm is the whole difference: clear and the
        // first operator bends the second, set and they are simply summed.
        if self.algorithm & 1 == 0 {
            let modulation = self.modulation_of(0);
            self.run(1, modulation, false, lfo_phase);

            to_sample(self.slots[1].last_i16)
        } else {
            self.run(1, 0.0, false, lfo_phase);

            to_sample(clip(self.slots[0].last_i16 + self.slots[1].last_i16))
        }
    }

    fn four_operator(&mut self, lfo_phase: usize) -> f64 {
        match self.algorithm & 7 {
            3 => {
                self.run(0, 0.0, true, lfo_phase);
                self.run(1, 0.0, false, lfo_phase);
                let from_one = self.modulation_of(1);
                self.run(2, from_one, false, lfo_phase);
                let paired = to_sample(clip(self.slots[0].last_i16 + self.slots[2].last_i16));
                self.run(3, paired, false, lfo_phase);

                to_sample(self.slots[3].last_i16)
            }
            4 => {
                self.run(0, 0.0, true, lfo_phase);
                for index in 1..4 {
                    let modulation = self.modulation_of(index - 1);
                    self.run(index, modulation, false, lfo_phase);
                }

                to_sample(self.slots[3].last_i16)
            }
            5 => {
                self.run(0, 0.0, true, lfo_phase);
                let from_zero = self.modulation_of(0);
                self.run(1, from_zero, false, lfo_phase);
                self.run(2, 0.0, true, lfo_phase);
                let from_two = self.modulation_of(2);
                self.run(3, from_two, false, lfo_phase);

                to_sample(clip(self.slots[1].last_i16 + self.slots[3].last_i16))
            }
            6 => {
                self.run(0, 0.0, true, lfo_phase);
                self.run(1, 0.0, false, lfo_phase);
                let from_one = self.modulation_of(1);
                self.run(2, from_one, false, lfo_phase);
                let from_two = self.modulation_of(2);
                self.run(3, from_two, false, lfo_phase);

                to_sample(clip(self.slots[0].last_i16 + self.slots[3].last_i16))
            }
            7 => {
                self.run(0, 0.0, true, lfo_phase);
                self.run(1, 0.0, false, lfo_phase);
                let from_one = self.modulation_of(1);
                self.run(2, from_one, false, lfo_phase);
                self.run(3, 0.0, false, lfo_phase);

                let paired = clip(self.slots[0].last_i16 + self.slots[2].last_i16);

                to_sample(clip(paired + self.slots[3].last_i16))
            }
            // Four operators side by side, the first and third feeding back.
            _ => {
                self.run(0, 0.0, true, lfo_phase);
                self.run(1, 0.0, false, lfo_phase);
                self.run(2, 0.0, true, lfo_phase);
                self.run(3, 0.0, false, lfo_phase);

                let first = clip(self.slots[0].last_i16 + self.slots[1].last_i16);
                let second = clip(self.slots[2].last_i16 + self.slots[3].last_i16);

                to_sample(clip(first + second))
            }
        }
    }

    fn run(&mut self, index: usize, modulation: f64, feedback: bool, lfo_phase: usize) {
        // Only the first and, under two of the algorithms, the third operator
        // are wired to feed back into themselves.
        let feedback = feedback && (index == 0 || (index == 2 && feeds_back_at_two(self.algorithm)));
        let channel_modulation = self.channel_modulation;
        let released = self.released;

        self.slots[index].run(modulation, feedback, channel_modulation, lfo_phase, released);
    }

    fn modulation_of(&self, index: usize) -> f64 {
        to_sample(self.slots[index].last_i16)
    }
}

/// How long to let a voice ring after key off before taking it back.
fn tail_frames(seconds: f64, sample_rate: u32) -> u32 {
    /// An operator that never releases would otherwise hold its voice for as
    /// long as the game runs.
    const LONGEST_TAIL_SECONDS: f64 = 4.0;

    let seconds = if seconds.is_finite() {
        seconds.min(LONGEST_TAIL_SECONDS)
    } else {
        LONGEST_TAIL_SECONDS
    };

    (seconds * sample_rate as f64).max(sample_rate as f64 / 50.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{Voice, is_carrier, note_hz, rate_index, wave_index};

    const SAMPLE_RATE: u32 = 44100;

    fn plain_voice() -> Voice {
        // A stand in, so the test does not depend on any particular file.
        let (tone, note) = crate::ma3::tone::Bank::new().tone_for(0, 0, 69);

        Voice::new(&tone, note, 8192, 2, 0, SAMPLE_RATE)
    }

    #[test]
    fn a_note_is_at_the_pitch_it_names() {
        assert!((note_hz(69, None, 2) - 440.0).abs() < 0.001);
        assert!((note_hz(81, None, 2) - 880.0).abs() < 0.001);
        // The wheel centred leaves the pitch alone.
        assert!((note_hz(69, Some(8192), 2) - 440.0).abs() < 0.001);
        // Fully up is the whole range, two semitones by default.
        assert!((note_hz(69, Some(16383), 2) - 493.8).abs() < 0.5);
    }

    #[test]
    fn a_silent_rate_stays_at_the_slowest_row() {
        // Rate zero means the envelope does not move, whatever the note.
        assert_eq!(rate_index(0, true, 15), 0);
        // Scaling only applies when the operator asks for it.
        assert_eq!(rate_index(1, false, 15), 4 + 3);
        assert_eq!(rate_index(1, true, 15), 4 + 15);
    }

    #[test]
    fn an_unbent_phase_reads_straight_off_the_table() {
        assert_eq!(wave_index(0, 0.0), 0);
        assert_eq!(wave_index(u32::MAX, 0.0), 1023);
        // A bend moves the read, and never off the table.
        assert!(wave_index(0, -1.0) < 1024);
        assert!(wave_index(u32::MAX, 1.0) < 1024);
    }

    #[test]
    fn carriers_match_the_algorithms() {
        // Two operators in series: only the second is heard.
        assert!(!is_carrier(2, 0, 0));
        assert!(is_carrier(2, 0, 1));
        // Two in parallel: both.
        assert!(is_carrier(2, 1, 0));
        assert!(is_carrier(2, 1, 1));
        // Four in series: only the last.
        assert!(!is_carrier(4, 4, 0));
        assert!(is_carrier(4, 4, 3));
        // Four side by side: all of them.
        assert!((0..4).all(|index| is_carrier(4, 2, index)));
    }

    #[test]
    fn a_note_makes_a_sound() {
        let mut voice = plain_voice();
        let mut loudest = 0.0f64;

        for _ in 0..SAMPLE_RATE / 20 {
            loudest = loudest.max(voice.sample().abs());
        }

        assert!(loudest > 0.01, "the voice never rose above {loudest}");
        assert!(loudest <= 1.001, "the voice ran away to {loudest}");
    }

    /// Every algorithm has to route to something audible, or a file that uses
    /// one of the rarer wirings would go quiet with no other sign of trouble.
    #[test]
    fn every_algorithm_carries_the_note() {
        let (mut tone, _) = crate::ma3::tone::Bank::new().tone_for(0, 24, 69);
        tone.operator_count = 4;
        tone.operators[2] = tone.operators[0];
        tone.operators[3] = tone.operators[1];

        for algorithm in 0..8u8 {
            tone.algorithm = algorithm;
            let mut voice = Voice::new(&tone, 69, 8192, 2, 0, SAMPLE_RATE);
            let mut loudest = 0.0f64;

            for _ in 0..SAMPLE_RATE / 20 {
                loudest = loudest.max(voice.sample().abs());
            }

            assert!(loudest > 0.001, "algorithm {algorithm} never rose above {loudest}");
        }
    }

    #[test]
    fn a_released_note_gives_its_voice_back() {
        let mut voice = plain_voice();
        for _ in 0..SAMPLE_RATE / 20 {
            voice.sample();
        }

        voice.release();
        for _ in 0..SAMPLE_RATE * 5 {
            voice.sample();
        }

        assert!(!voice.audible(), "the voice was still sounding five seconds after key off");
    }

    #[test]
    fn bending_the_wheel_moves_the_pitch() {
        let (tone, note) = crate::ma3::tone::Bank::new().tone_for(0, 16, 69);
        let mut plain = Voice::new(&tone, note, 8192, 2, 0, SAMPLE_RATE);
        let mut bent = Voice::new(&tone, note, 16383, 2, 0, SAMPLE_RATE);

        // The two run at different rates, so they part company well inside a
        // tenth of a second even though they start together.
        let mut apart = 0.0f64;
        for _ in 0..SAMPLE_RATE / 10 {
            apart = apart.max((plain.sample() - bent.sample()).abs());
        }

        assert!(apart > 0.01, "the bent note tracked the plain one to within {apart}");
    }
}
