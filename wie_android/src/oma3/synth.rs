//! The FM voice engine, ported from the reference `OracleMa3Synth`.
//!
//! This step ports the self-contained arithmetic the operator and voice runtime
//! build on: fixed-point pitch, wave sampling, envelope-rate lookup, output
//! level and the accumulator helpers. The operator and voice runtimes that call
//! these follow. Every function mirrors the reference name and its exact
//! integer widths - Java `int` is [`i32`], `long` is [`i64`], `>>>` is an
//! unsigned shift, and `& 0xFFFF_FFFF` narrows to a thirty-two-bit value held in
//! a wider signed one - because the output has to match the reference bit for
//! bit, not merely sound similar.

use super::tables::{
    lfo_level_q15, wave_sample_i16, DETUNE_CENTS, FEEDBACK_GAIN, FEEDBACK_SHIFT, KEYLEVEL_Q15, LEVEL_Q15, LFO_STEP_Q20, MA3_FREQ_BASE, MULTIPLE,
    PHASE_DETUNE, PHASE_KEY_CODE,
};

/// `OracleMa3Synth.clamp(int, int, int)`.
pub fn clamp(value: i32, lo: i32, hi: i32) -> i32 {
    if value < lo {
        lo
    } else {
        value.min(hi)
    }
}

/// `OracleMa3Synth.clamp(double, double, double)`.
pub fn clamp_f(value: f64, lo: f64, hi: f64) -> f64 {
    if value < lo {
        lo
    } else {
        value.min(hi)
    }
}

/// `clipI16Accum` - the accumulator saturates at +/-32767, not 32768.
pub fn clip_i16_accum(value: i32) -> i32 {
    value.clamp(-32767, 32767)
}

/// `firstNonZero`.
pub fn first_non_zero(a: i32, b: i32) -> i32 {
    if a != 0 {
        a
    } else {
        b
    }
}

/// `isCarrier` - whether an operator is heard directly under an algorithm.
pub fn is_carrier(op_count: i32, algorithm: i32, index: i32) -> bool {
    if op_count <= 2 || algorithm < 2 {
        return algorithm & 1 != 0 || index == 1;
    }
    match algorithm & 7 {
        2 => true,
        3 | 4 => index == 3,
        5 => index == 1 || index == 3,
        6 => index == 0 || index == 3,
        7 => index == 0 || index == 2 || index == 3,
        _ => index == op_count - 1,
    }
}

/// `usesOp2Feedback`.
pub fn uses_op2_feedback(algorithm: i32) -> bool {
    let a = algorithm & 7;
    a == 2 || a == 5
}

/// `phaseKeyCodeFromParam`.
pub fn phase_key_code_from_param(value: i32) -> i32 {
    value & 31
}

/// `hasRecord` - a device operator record is present if any byte is non-zero.
pub fn has_record(data: &[u8]) -> bool {
    data.iter().any(|&b| b != 0)
}

// ----- accumulator / modulation helpers -----

/// `phaseModFromI16`.
pub fn phase_mod_from_i16(value: i32) -> f64 {
    clip_i16_accum(value) as f64 / 32767.0
}

/// `carrierAccumToSample`.
pub fn carrier_accum_to_sample(value: i32) -> f64 {
    clip_i16_accum(value) as f64 / 32767.0
}

/// `sum2I16`.
pub fn sum2_i16(a: i32, b: i32) -> i32 {
    clip_i16_accum(a + b)
}

/// `sum3I16`.
pub fn sum3_i16(a: i32, b: i32, c: i32) -> i32 {
    clip_i16_accum(sum2_i16(a, b) + c)
}

/// `sum4I16`.
pub fn sum4_i16(a: i32, b: i32, c: i32, d: i32) -> i32 {
    clip_i16_accum(sum2_i16(a, b) + sum2_i16(c, d))
}

/// `modSampleToI16`.
pub fn mod_sample_to_i16(value: f64) -> i32 {
    (value * 32767.0).round() as i32
}

// ----- wave sampling (`OracleMa3WaveTables`) -----

/// `OracleMa3WaveTables.waveIndex(long, double)`.
pub fn wave_index(phase_q32: u32, modulation: f64) -> i32 {
    let j2 = phase_q32 as u64;
    if modulation != 0.0 {
        (((mod_sample_to_i16(modulation) >> 1) + (j2 >> 20) as i32) >> 2) & 1023
    } else {
        ((j2 >> 22) & 1023) as i32
    }
}

/// `OracleMa3WaveTables.sampleI16(int, long, double)`.
pub fn wave_sample(waveform: i32, phase_q32: u32, modulation: f64) -> i32 {
    wave_sample_i16((waveform & 31) as usize, wave_index(phase_q32, modulation) as usize)
}

// ----- output level -----

/// `keylevelQ15FromSelector`.
pub fn keylevel_q15_from_selector(selector: i32, key: i32) -> i32 {
    let selector = selector & 3;
    if selector == 0 {
        return 32768;
    }
    KEYLEVEL_Q15[(((selector - 1) & 3) * 128 + (key & 127)) as usize]
}

/// `outputLevelQ15`.
pub fn output_level_q15(base_level_q15: i32, selector: i32, key: i32) -> i32 {
    let scaled = (base_level_q15 * keylevel_q15_from_selector(selector, key)) >> 15;
    scaled.min(32767)
}

/// `dllScaledSampleI16RawInt` - applies the envelope, the operator level and the
/// tremolo, all in the reference's order and precision.
pub fn dll_scaled_sample_i16_raw_int(sample: i32, envelope_q31: i64, level_q15: i32, tremolo: i32) -> i32 {
    let level = ((level_q15 as i64 & 0xffff) * (tremolo as i64 & 0xffff)) >> 15;
    let gain = ((envelope_q31 >> 16) * level) >> 15;
    let scaled = (clamp(sample, -32767, 32767) as i64 * gain) >> 15;
    scaled.clamp(-32767, 32767) as i32
}

/// `feedbackShiftFromSeed`.
pub fn feedback_shift_from_seed(seed: i32) -> i32 {
    FEEDBACK_SHIFT[(seed & 7) as usize]
}

/// `feedbackGain` lookup used in the operator constructor.
pub fn feedback_gain(seed: i32) -> f64 {
    FEEDBACK_GAIN[clamp(seed, 0, 7) as usize]
}

// ----- pitch / phase step -----

/// `unsignedMulDivToUint32` - `(a * b) / c` in unsigned 64-bit, narrowed to 32.
pub fn unsigned_mul_div_to_uint32(a: i64, b: i64, c: i64) -> i64 {
    if c <= 0 {
        return 0;
    }
    // a and b are already within 32 bits and non-negative here in the reference.
    (((a as u64).wrapping_mul(b as u64)) / c as u64) as i64 & 0xffff_ffff
}

/// `basePhaseStepFromTail`.
pub fn base_phase_step_from_tail(mantissa: i32, shift: i32, mut scale: i32) -> i64 {
    let j = ((mantissa as i64) << (shift & 31)) << 12;
    if scale == 0 {
        scale = 65536;
    }
    let j2 = (j * scale as i64) >> 16;
    if j2 > 4_294_967_295 {
        4_294_967_295
    } else {
        j2
    }
}

/// `phaseStepFromFieldsExact`.
pub fn phase_step_from_fields_exact(base_step: i64, phase_mul: i32, phase_det: i32, key_code: i32) -> i64 {
    let mut j2 = base_step & 0xffff_ffff;
    if phase_det & 7 != 0 {
        let mut offset = PHASE_DETUNE[((phase_det & 3) * 32 + phase_key_code_from_param(key_code)) as usize];
        if phase_det & 4 != 0 {
            offset = -offset;
        }
        let j3 = ((j2 >> 16) * 48000) & 0xffff_ffff;
        let j4 = if j3 < 65536 { 65536 } else { j3 };
        let j5 = j4 + offset;
        j2 = if j5 <= 0 { 0 } else { unsigned_mul_div_to_uint32(j5, j2, j4) };
    }
    let mul = phase_mul & 15;
    if mul != 0 {
        (j2 * mul as i64) & 0xffff_ffff
    } else {
        j2 >> 1
    }
}

/// `phaseStepQ32FromHz`.
pub fn phase_step_q32_from_hz(hz: f64, sample_rate: i32) -> i64 {
    if hz <= 0.0 || sample_rate <= 0 {
        return 0;
    }
    let v = (hz * 4_294_967_296.0) / sample_rate as f64;
    if v < 0.0 {
        0
    } else if v > 4_294_967_295.0 {
        4_294_967_295
    } else {
        v.round() as i64
    }
}

/// `pitchHz(int, int, int)`.
pub fn pitch_hz(note: i32, bend: i32, mut range: i32) -> f64 {
    let base = 2f64.powf((note as f64 - 69.0) / 12.0) * 440.0;
    if range > 24 {
        range = 2;
    }
    if bend < 0 || range == 0 {
        base
    } else {
        base * 2f64.powf((((bend - 8192) * range) as f64 / 8192.0) / 12.0)
    }
}

/// `pitchScaleQ16FromBend`.
pub fn pitch_scale_q16_from_bend(from_hz: f64, to_hz: f64) -> i32 {
    if from_hz <= 0.0 || to_hz <= 0.0 {
        return 65536;
    }
    let v = ((to_hz / from_hz) * 65536.0).clamp(1.0, 262144.0);
    v.round() as i32
}

/// `pitchTailFromHz` - returns `(shift, mantissa)`.
pub fn pitch_tail_from_hz(hz: f64, sample_rate: i32) -> (i32, i32) {
    let mut v = (phase_step_q32_from_hz(hz, sample_rate) as u64 >> 12) as i64;
    if v == 0 {
        v = 1;
    }
    if v > 130944 {
        v = 130944;
    }
    let mut shift = 0;
    while v > 1023 && shift < 7 {
        v >>= 1;
        shift += 1;
    }
    if v > 1023 {
        v = 1023;
    }
    (shift, v as i32)
}

/// `hzFromPitchTail`.
pub fn hz_from_pitch_tail(mantissa: i32, shift: i32, sample_rate: i32) -> f64 {
    (base_phase_step_from_tail(mantissa, shift, 65536) * sample_rate as i64) as f64 / 4_294_967_296.0
}

/// `envRateParamFromTail`.
pub fn env_rate_param_from_tail(mantissa: i32, shift: i32) -> i32 {
    ((mantissa >> 9) & 1) | (clamp(shift, 0, 7) << 1)
}

/// `keylevelCodeFromTail`.
pub fn keylevel_code_from_tail(mantissa: i32, shift: i32) -> i32 {
    ((mantissa >> 3) & 120) | clamp(shift, 0, 7)
}

/// `phaseKeyCodeFromTail`.
pub fn phase_key_code_from_tail(mantissa: i32, shift: i32) -> i32 {
    PHASE_KEY_CODE[((mantissa >> 6) & 15) as usize] + clamp(shift, 0, 7) * 4
}

/// `opMultiple` - the non-device path multiplies the fractional multiple by the
/// detune in cents; the device path uses `dllPhaseRatioFromFields`.
pub fn op_multiple(mul: i32, detune: i32, device: bool, rt_mul: i32, rt_det: i32, key_code: i32, base_hz: f64) -> f64 {
    if device {
        dll_phase_ratio_from_fields(rt_mul & 15, rt_det & 7, key_code, base_hz)
    } else {
        clamp_f(
            MULTIPLE[clamp(mul & 15, 0, 15) as usize] * 2f64.powf(DETUNE_CENTS[(detune & 15) as usize] / 1200.0),
            0.125,
            15.0,
        )
    }
}

/// `dllPhaseRatioFromFields`.
pub fn dll_phase_ratio_from_fields(mul: i32, det: i32, key_code: i32, base_hz: f64) -> f64 {
    let ratio = if det & 7 != 0 {
        let mut offset = PHASE_DETUNE[((det & 3) * 32 + (key_code & 31)) as usize] as f64;
        if det & 4 != 0 {
            offset = -offset;
        }
        let mut base = base_hz * 48000.0;
        if base < 65536.0 {
            base = 65536.0;
        }
        clamp_f((offset + base) / base, 0.25, 2.5)
    } else {
        1.0
    };
    let m = mul & 15;
    ratio * if m != 0 { m as f64 } else { 0.5 }
}

/// `dllRateIndex`.
pub fn dll_rate_index(rate: i32, ksr: i32, mut key_code: i32) -> i32 {
    let base = (rate & 15) << 2;
    if ksr == 0 {
        key_code >>= 2;
    }
    if base == 0 {
        return 0;
    }
    clamp(base + key_code, 0, 63)
}

/// `lfoLevelQ15` clamped as the operator does before scaling the sample.
pub fn lfo_tremolo(am_enable: bool, am_depth: i32, phase: i32) -> i32 {
    if am_enable {
        lfo_level_q15(am_depth as usize, phase as usize).min(65535)
    } else {
        32768
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden values captured from the reference `OracleMa3Synth` running as an
    /// oracle, so the fixed-point port is exact rather than merely plausible.
    #[test]
    fn matches_reference_math() {
        assert_eq!(pitch_hz(69, -1, 2), 440.0);
        assert_eq!(pitch_hz(81, 8192, 2), 880.0);
        assert_eq!(phase_step_q32_from_hz(440.0, 44100), 42852281);
        assert_eq!(base_phase_step_from_tail(500, 3, 65536), 16384000);
        assert_eq!(phase_step_from_fields_exact(1000000, 3, 5, 20), 2950848);
        assert_eq!(dll_scaled_sample_i16_raw_int(20000, 1000000000, 20000, 40000), 6938);
        assert_eq!(keylevel_q15_from_selector(2, 60), 12139);
        assert_eq!(output_level_q15(16000, 2, 60), 5927);
        assert_eq!(dll_rate_index(10, 1, 40), 63);
        assert_eq!(env_rate_param_from_tail(700, 3), 7);
        assert_eq!(keylevel_code_from_tail(700, 3), 83);
        assert_eq!(phase_key_code_from_tail(700, 3), 15);
        assert!((dll_phase_ratio_from_fields(2, 5, 20, 440.0) - 1.9988829545454545).abs() < 1e-12);
        assert_eq!(wave_index(0x4000_0000, 0.3), 460);
        assert_eq!(wave_sample(3, 0x4000_0000, 0.3), 0);
        assert_eq!(pitch_scale_q16_from_bend(440.0, 466.2), 69438);
    }
}

/// `dllDecaySec` - the release time in seconds, used only to size the tail.
pub fn dll_decay_sec(rr: i32, ksr: i32, env_rate_param: i32, sample_rate: i32) -> f64 {
    let ratio = super::tables::DECAY_RATE_Q30[dll_rate_index(rr, ksr, env_rate_param) as usize] as f64 / 1.073741824e9;
    if ratio >= 0.999999999 {
        return 1.0e9;
    }
    let (d, log) = if ratio <= 0.0 {
        (1.0, sample_rate as f64)
    } else {
        (-1.0, sample_rate as f64 * ratio.ln())
    };
    d / log
}

/// One operator's voice parameters, as the reference `OracleSmaf.CompactOperator`
/// carries them: the legacy fields, the two raw device records (`dll`, `ma3`)
/// and the decoded runtime (`rt_*`) set. The parser fills these; the runtime
/// below chooses between the device and legacy paths exactly as the reference.
#[derive(Clone, Default)]
pub struct CompactOperator {
    pub ar: i32,
    pub attack: i32,
    pub d1r: i32,
    pub decay: i32,
    pub d2r: i32,
    pub rr: i32,
    pub release: i32,
    pub sl: i32,
    pub sustain: i32,
    pub tl: i32,
    pub level: i32,
    pub mul: i32,
    pub multiple: i32,
    pub ksr: i32,
    pub waveform: i32,
    pub dt1: i32,
    pub dt2: i32,
    pub detune: i32,
    pub am: i32,
    pub am_enable: i32,
    pub ksl: i32,
    pub vib: i32,
    pub dll: Vec<u8>,
    pub ma3: Vec<u8>,
    pub rt_ar: i32,
    pub rt_d1r: i32,
    pub rt_d2r: i32,
    pub rt_rr: i32,
    pub rt_sl: i32,
    pub rt_level_index: i32,
    pub rt_mul: i32,
    pub rt_ksr: i32,
    pub rt_waveform: i32,
    pub rt_det: i32,
    pub rt_keylevel_sel: i32,
    pub rt_feedback: i32,
    pub rt_am_enable: i32,
    pub rt_am_depth: i32,
    pub rt_vib_enable: i32,
    pub rt_vib_depth: i32,
    pub rt_keyoff_inhibit: i32,
}

const MASK32: i64 = 0xffff_ffff;

/// One running operator, ported from the reference `OperatorRuntime`. The
/// constructor chooses device (`rt_*`) or legacy fields exactly as the
/// reference, and `run` produces one sample while advancing the phase and
/// stepping the envelope.
pub struct OperatorRuntime {
    waveform: i32,
    base_hz_fallback: f64,
    sample_rate: i32,
    phase_mul: i32,
    phase_det: i32,
    env_ar: i32,
    env_d1r: i32,
    env_d2r: i32,
    env_rr: i32,
    env_ksr: i32,
    sustain_q31: i64,
    base_level_q15: i32,
    keylevel_selector: i32,
    pub carrier: bool,
    feedback_gain: f64,
    feedback_shift: i32,
    am_enable: bool,
    am_depth: i32,
    vib_enable: bool,
    vib_depth: i32,
    keyoff_inhibit: bool,
    release_use_decay2: bool,
    phase_step_q32: i64,
    pub release_sec: f64,
    attack_add_q31: i64,
    decay_mul_q30: i64,
    decay2_mul_q30: i64,
    release_mul_q30: i64,
    level_q15: i32,
    phase_q32: i64,
    env_q31: i64,
    env_stage: i32,
    prev: f64,
    prev2: f64,
    pub prev_i16: i32,
    pub prev2_i16: i32,
    pub last_i16: i32,
}

impl OperatorRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        op: Option<&CompactOperator>,
        base_hz: f64,
        sample_rate: i32,
        env_rate_param: i32,
        keylevel_code: i32,
        detune_key_code: i32,
        base_step_q32: i64,
        feedback_seed: i32,
        carrier: bool,
    ) -> Self {
        let device = op.map(|o| has_record(&o.dll) || has_record(&o.ma3)).unwrap_or(false);

        let env_ar = op.map_or(14, |o| if device { o.rt_ar } else { first_non_zero(o.ar, o.attack) });
        let env_d1r = op.map_or(8, |o| if device { o.rt_d1r } else { first_non_zero(o.d1r, o.decay) });
        let env_d2r = op.map_or(0, |o| if device { o.rt_d2r } else { o.d2r });
        let env_rr = op.map_or(7, |o| if device { o.rt_rr } else { first_non_zero(o.rr, o.release) });
        let sl = op.map_or(5, |o| if device { o.rt_sl } else { first_non_zero(o.sl, o.sustain) });
        let level_index = op.map_or(0, |o| {
            if device {
                o.rt_level_index & 63
            } else if o.tl != 0 {
                o.tl
            } else {
                o.level * 4
            }
        });
        let phase_mul = op.map_or(1, |o| if device { o.rt_mul } else { first_non_zero(o.mul, o.multiple) });
        let env_ksr = op.map_or(0, |o| if device { o.rt_ksr } else { o.ksr });
        let waveform = op.map_or(0, |o| (if device { o.rt_waveform } else { o.waveform }) & 31);

        let op_mul = op.map_or(1.0, |o| {
            op_multiple(phase_mul, o.detune, device, o.rt_mul, o.rt_det, detune_key_code, base_hz)
        });
        let phase_det = op.map_or(0, |o| (if device { o.rt_det } else { o.dt2 }) & 7);
        let keylevel_selector = op.map_or(0, |o| (if device { o.rt_keylevel_sel } else { o.dt1 }) & 3);
        let feedback_seed = match op {
            Some(o) if device => o.rt_feedback,
            _ => feedback_seed,
        };

        let (am_enable, am_depth, vib_enable, vib_depth, keyoff_inhibit, release_use_decay2) = match op {
            None => (false, 0, false, 0, false, false),
            Some(o) => {
                let am_enable = if device {
                    o.rt_am_enable & 1 != 0
                } else {
                    o.am_enable != 0 || o.am != 0
                };
                let am_depth = (if device { o.rt_am_depth } else { o.ksl }) & 3;
                let vib_enable = !(if device { o.rt_vib_enable & 1 == 0 } else { o.vib == 0 });
                let vib_depth = if device {
                    o.rt_vib_depth & 3
                } else if o.ma3.len() > 13 {
                    (o.ma3[13] as i32) & 3
                } else {
                    0
                };
                let keyoff_inhibit = !(if device {
                    o.rt_keyoff_inhibit & 1 == 0
                } else {
                    o.ma3.len() <= 1 || (o.ma3[1] as i32) & 1 == 0
                });
                let release_use_decay2 = if o.ma3.len() > 1 && o.ma3[1] != 0 {
                    true
                } else {
                    !o.dll.is_empty() && (o.dll[0] as i32) & 8 != 0
                };
                (am_enable, am_depth, vib_enable, vib_depth, keyoff_inhibit, release_use_decay2)
            }
        };

        let mut runtime = OperatorRuntime {
            waveform,
            base_hz_fallback: base_hz,
            sample_rate: sample_rate.max(1),
            phase_mul,
            phase_det,
            env_ar,
            env_d1r,
            env_d2r,
            env_rr,
            env_ksr,
            sustain_q31: super::tables::SUSTAIN_Q31[clamp(sl, 0, 15) as usize] as i64,
            base_level_q15: super::tables::LEVEL_Q15[clamp(level_index, 0, 63) as usize],
            keylevel_selector,
            carrier,
            feedback_gain: feedback_gain(feedback_seed),
            feedback_shift: feedback_shift_from_seed(feedback_seed),
            am_enable,
            am_depth,
            vib_enable,
            vib_depth,
            keyoff_inhibit,
            release_use_decay2,
            phase_step_q32: 0,
            release_sec: 0.0,
            attack_add_q31: 0,
            decay_mul_q30: 0,
            decay2_mul_q30: 0,
            release_mul_q30: 0,
            level_q15: 0,
            phase_q32: 0,
            env_q31: 0,
            // The reference constructor keys the operator on: it ends with
            // `envStage = 2`, the attack stage, so the note sounds without a
            // separate key-on call.
            env_stage: 2,
            prev: 0.0,
            prev2: 0.0,
            prev_i16: 0,
            prev2_i16: 0,
            last_i16: 0,
        };
        runtime.set_pitch(base_hz * op_mul, env_rate_param, keylevel_code, detune_key_code, base_step_q32);
        runtime
    }

    pub fn set_pitch(&mut self, hz: f64, env_rate_param: i32, keylevel_code: i32, detune_key_code: i32, base_step_q32: i64) {
        self.phase_step_q32 = if base_step_q32 != 0 {
            phase_step_from_fields_exact(base_step_q32, self.phase_mul, self.phase_det, detune_key_code)
        } else {
            let hz = if hz > 0.0 { hz } else { self.base_hz_fallback };
            phase_step_q32_from_hz(hz, self.sample_rate)
        };
        self.release_sec = dll_decay_sec(self.env_rr, self.env_ksr, env_rate_param, self.sample_rate);
        self.attack_add_q31 = super::tables::ATTACK_RATE_Q31[dll_rate_index(self.env_ar, self.env_ksr, env_rate_param) as usize] as i64;
        self.decay_mul_q30 = super::tables::DECAY_RATE_Q30[dll_rate_index(self.env_d1r, self.env_ksr, env_rate_param) as usize] as i64;
        self.decay2_mul_q30 = if self.env_d2r != 0 {
            super::tables::DECAY_RATE_Q30[dll_rate_index(self.env_d2r, self.env_ksr, env_rate_param) as usize] as i64
        } else {
            1_073_741_824
        };
        self.release_mul_q30 = super::tables::DECAY_RATE_Q30[dll_rate_index(self.env_rr, self.env_ksr, env_rate_param) as usize] as i64;
        self.level_q15 = output_level_q15(self.base_level_q15, self.keylevel_selector, keylevel_code);
    }

    fn add_attack(&mut self, value: i64) -> i64 {
        let value = value + self.attack_add_q31;
        if value >= 2_147_483_648 {
            self.env_stage = 4;
            2_147_483_648
        } else {
            value
        }
    }

    fn reset_edge_state(&mut self) {
        self.phase_q32 = 0;
        self.prev = 0.0;
        self.prev2 = 0.0;
        self.prev_i16 = 0;
        self.prev2_i16 = 0;
        self.last_i16 = 0;
    }

    pub fn all_sound_off(&mut self) {
        self.env_stage = 0;
        self.env_q31 = 0;
        self.prev = 0.0;
        self.prev2 = 0.0;
        self.prev_i16 = 0;
        self.prev2_i16 = 0;
        self.last_i16 = 0;
    }

    pub fn is_audible(&self) -> bool {
        self.env_stage != 0 && self.env_q31 > 0
    }

    pub fn key_state(&mut self, state: i32, reset_env: bool) {
        if state == 255 {
            self.env_stage = 0;
            self.env_q31 = 0;
            self.reset_edge_state();
        } else if state == 1 || state == 2 {
            self.env_stage = 2;
            if reset_env {
                self.env_q31 = 0;
            }
            self.reset_edge_state();
        } else if state == 0 && self.env_stage != 0 && !self.keyoff_inhibit {
            self.env_stage = 1;
        }
    }

    pub fn release_now(&mut self) {
        if self.env_stage != 0 && !self.keyoff_inhibit {
            self.env_stage = 1;
        }
    }

    fn step_envelope_q31(&mut self, position: i32, gate: i32) -> i64 {
        if position >= gate && !self.keyoff_inhibit {
            let stage = self.env_stage;
            if stage != 0 && stage != 1 {
                self.env_stage = 1;
            }
        }
        let mut env = self.env_q31;
        let stage = self.env_stage;
        let mut out = 0i64;
        if stage != 0 {
            if stage == 1 {
                let mul = if self.release_use_decay2 {
                    self.decay2_mul_q30
                } else {
                    self.release_mul_q30
                };
                env = (env * mul) >> 30;
                out = env;
                if env == 0 {
                    self.env_stage = 0;
                }
            } else if stage == 2 {
                self.phase_q32 = 0;
                self.env_stage = 3;
                out = self.add_attack(env);
            } else if stage == 3 {
                out = self.add_attack(env);
            } else if stage == 4 {
                env = (env * self.decay_mul_q30) >> 30;
                if env != 0 {
                    out = env;
                    if env <= self.sustain_q31 {
                        self.env_stage = 5;
                    }
                } else {
                    self.env_stage = 0;
                    out = env;
                }
            } else if stage == 5 {
                env = (env * self.decay2_mul_q30) >> 30;
                out = env;
                if env == 0 {
                    self.env_stage = 0;
                }
            } else if stage == 6 {
                self.phase_q32 = 0;
                out = self.add_attack(env);
                self.env_stage = 1;
            } else {
                self.env_stage = 0;
            }
        }
        if out > 2_147_483_648 {
            out = 2_147_483_648;
        }
        self.env_q31 = out;
        out
    }

    /// One sample. `modulation` is the phase modulation from the previous
    /// operator; `feedback` runs this operator's own output back into its phase;
    /// `channel_modulation` and `lfo_phase` drive vibrato and tremolo.
    pub fn run(&mut self, position: i32, gate: i32, modulation: f64, feedback: bool, channel_modulation: i32, lfo_phase: i32) -> f64 {
        let env = self.step_envelope_q31(position, gate);
        let (feedback_sample, feedback_i16) = if feedback {
            if self.feedback_shift != 0 {
                let i = (self.prev_i16 + self.prev2_i16) >> self.feedback_shift;
                (i as f64 / 32767.0, i)
            } else {
                let d = (self.prev + self.prev2) * 0.5 * self.feedback_gain;
                (d, mod_sample_to_i16(d))
            }
        } else {
            (0.0, 0)
        };

        let (vib_index, vib_on) = if channel_modulation != 0 && self.vib_enable {
            (((self.vib_depth & 3) + channel_modulation - 1).min(3), true)
        } else {
            (0, false)
        };
        let pitch_q20 = if vib_on {
            super::tables::lfo_pitch_q20(vib_index as usize, lfo_phase as usize) & MASK32
        } else {
            1_048_576
        };
        let tremolo = if self.am_enable {
            super::tables::lfo_level_q15(self.am_depth as usize, lfo_phase as usize).min(65535)
        } else {
            32768
        };

        let modulation = modulation + feedback_sample;
        let sample_i16 = wave_sample(self.waveform, self.phase_q32 as u32, modulation);
        let scaled = dll_scaled_sample_i16_raw_int(sample_i16, env, self.level_q15, tremolo);
        let out = scaled as f64 / 32767.0;

        self.prev2 = self.prev;
        self.prev = out;
        self.prev2_i16 = self.prev_i16;
        self.prev_i16 = scaled;
        self.last_i16 = scaled;
        let _ = feedback_i16;

        let mut step = self.phase_step_q32;
        if pitch_q20 != 1_048_576 {
            step = ((step.wrapping_mul(pitch_q20)) >> 20).min(MASK32);
        }
        self.phase_q32 = (self.phase_q32 + step) & MASK32;
        out
    }
}

/// A compact tone (one voice), as the reference `OracleSmaf.CompactTone`. The
/// parser fills it; the runtime reads the algorithm, feedback seed, per-operator
/// records and the device voice bytes.
#[derive(Clone, Default)]
pub struct CompactTone {
    pub valid: bool,
    pub algorithm: i32,
    pub feedback: i32,
    pub operators: Vec<CompactOperator>,
    pub dll_voice: Vec<u8>,
}

/// `PitchState.from` - the note's base hertz, envelope-rate/key-level codes and
/// the base phase step, all derived from the midi key and bend.
struct PitchState {
    base_hz: f64,
    env_rate_param: i32,
    keylevel_code: i32,
    detune_key_code: i32,
    base_step_q32: i64,
}

impl PitchState {
    fn from(midi_key: i32, bend: i32, bend_range: i32, sample_rate: i32) -> Self {
        let note = clamp(midi_key, 0, 127);
        let unbent = pitch_hz(note, -1, 2);
        let bent = pitch_hz(note, bend, bend_range);
        let scale = pitch_scale_q16_from_bend(unbent, bent);
        let (shift, mantissa) = pitch_tail_from_hz(unbent, sample_rate);
        PitchState {
            base_hz: bent,
            env_rate_param: env_rate_param_from_tail(mantissa, shift),
            keylevel_code: keylevel_code_from_tail(mantissa, shift),
            detune_key_code: phase_key_code_from_tail(mantissa, shift),
            base_step_q32: base_phase_step_from_tail(mantissa, shift, scale),
        }
    }
}

/// One running voice, ported from the reference `VoiceRuntime`: up to four
/// operators wired by the tone's algorithm, with per-slot feedback.
pub struct VoiceRuntime {
    algorithm: i32,
    amp: f64,
    channel_modulation: i32,
    fb0_prev_i16: i32,
    fb0_prev2_i16: i32,
    fb2_prev_i16: i32,
    fb2_prev2_i16: i32,
    lfo_phase_q20: i32,
    lfo_step_q20: i32,
    op: Vec<OperatorRuntime>,
    op_count: usize,
    pan_l: f64,
    pan_r: f64,
}

impl VoiceRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn new(tone: &CompactTone, midi_key: i32, velocity: i32, pan: i32, bend: i32, bend_range: i32, modulation: i32, sample_rate: i32) -> Self {
        let op_count = clamp(if tone.operators.is_empty() { 2 } else { tone.operators.len() as i32 }, 2, 4) as usize;
        let algorithm = tone.algorithm & 7;
        let sample_rate = sample_rate.max(1);
        let channel_modulation = clamp(modulation, 0, 4);
        let lfo_seed = if tone.dll_voice.len() >= 2 {
            ((tone.dll_voice[1] as i32 & 255) >> 6) & 63
        } else {
            0
        };
        let lfo_step_q20 = LFO_STEP_Q20[(lfo_seed & 63) as usize] as i32;
        let pitch = PitchState::from(midi_key, bend, bend_range, sample_rate);

        let mut op = Vec::with_capacity(op_count);
        for i in 0..op_count {
            let operator = tone.operators.get(i);
            let carrier = is_carrier(op_count as i32, algorithm, i as i32);
            let seed = if i != 0 && (i != 2 || !uses_op2_feedback(algorithm)) {
                operator.map_or(0, |o| o.rt_feedback)
            } else {
                tone.feedback
            };
            op.push(OperatorRuntime::new(
                operator,
                pitch.base_hz,
                sample_rate,
                pitch.env_rate_param,
                pitch.keylevel_code,
                pitch.detune_key_code,
                pitch.base_step_q32,
                seed,
                carrier,
            ));
        }

        let mut runtime = VoiceRuntime {
            algorithm,
            amp: 0.0,
            channel_modulation,
            fb0_prev_i16: 0,
            fb0_prev2_i16: 0,
            fb2_prev_i16: 0,
            fb2_prev2_i16: 0,
            lfo_phase_q20: 0,
            lfo_step_q20,
            op,
            op_count,
            pan_l: 0.0,
            pan_r: 0.0,
        };
        runtime.set_velocity(velocity);
        let position = clamp_f((pan as f64 - 64.0) / 64.0, -1.0, 1.0);
        runtime.pan_l = ((1.0 - position) * 0.5).sqrt();
        runtime.pan_r = ((position + 1.0) * 0.5).sqrt();
        runtime
    }

    pub fn set_velocity(&mut self, velocity: i32) {
        self.amp = clamp(velocity, 0, 127).max(1) as f64 / 127.0;
    }

    pub fn max_release_sec(&self) -> f64 {
        self.op.iter().take(self.op_count).fold(0.05, |m, o| m.max(o.release_sec))
    }

    fn run_slot_feedback(&mut self, slot: usize, position: i32, gate: i32, modulation: f64, lfo_phase: i32) -> f64 {
        let cm = self.channel_modulation;
        let (prev, prev2) = if slot == 2 {
            (self.fb2_prev_i16, self.fb2_prev2_i16)
        } else {
            (self.fb0_prev_i16, self.fb0_prev2_i16)
        };
        let op = &mut self.op[slot];
        op.prev_i16 = prev;
        op.prev2_i16 = prev2;
        op.prev = prev as f64 / 32767.0;
        op.prev2 = prev2 as f64 / 32767.0;
        let out = op.run(position, gate, modulation, true, cm, lfo_phase);
        let (np, np2) = (op.prev_i16, op.prev2_i16);
        if slot == 2 {
            self.fb2_prev_i16 = np;
            self.fb2_prev2_i16 = np2;
        } else {
            self.fb0_prev_i16 = np;
            self.fb0_prev2_i16 = np2;
        }
        out
    }

    /// Runs operator `slot` with a plain phase modulation and returns nothing;
    /// the reference reads the operator's `lastI16` afterwards.
    fn run_op(&mut self, slot: usize, position: i32, gate: i32, modulation: f64, lfo_phase: i32) {
        let cm = self.channel_modulation;
        self.op[slot].run(position, gate, modulation, false, cm, lfo_phase);
    }

    fn last_mod(&self, slot: usize) -> f64 {
        phase_mod_from_i16(self.op[slot].last_i16)
    }

    fn sample_four_operator(&mut self, position: i32, gate: i32, lfo_phase: i32) -> f64 {
        match self.algorithm & 7 {
            3 => {
                self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
                self.run_op(1, position, gate, 0.0, lfo_phase);
                let m1 = self.last_mod(1);
                self.run_op(2, position, gate, m1, lfo_phase);
                let m = phase_mod_from_i16(sum2_i16(self.op[0].last_i16, self.op[2].last_i16));
                self.run_op(3, position, gate, m, lfo_phase);
                carrier_accum_to_sample(self.op[3].last_i16)
            }
            4 => {
                self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
                let m0 = self.last_mod(0);
                self.run_op(1, position, gate, m0, lfo_phase);
                let m1 = self.last_mod(1);
                self.run_op(2, position, gate, m1, lfo_phase);
                let m2 = self.last_mod(2);
                self.run_op(3, position, gate, m2, lfo_phase);
                carrier_accum_to_sample(self.op[3].last_i16)
            }
            5 => {
                self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
                let m0 = self.last_mod(0);
                self.run_op(1, position, gate, m0, lfo_phase);
                self.run_slot_feedback(2, position, gate, 0.0, lfo_phase);
                let m2 = self.last_mod(2);
                self.run_op(3, position, gate, m2, lfo_phase);
                carrier_accum_to_sample(sum2_i16(self.op[1].last_i16, self.op[3].last_i16))
            }
            6 => {
                self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
                self.run_op(1, position, gate, 0.0, lfo_phase);
                let m1 = self.last_mod(1);
                self.run_op(2, position, gate, m1, lfo_phase);
                let m2 = self.last_mod(2);
                self.run_op(3, position, gate, m2, lfo_phase);
                carrier_accum_to_sample(sum2_i16(self.op[0].last_i16, self.op[3].last_i16))
            }
            7 => {
                self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
                self.run_op(1, position, gate, 0.0, lfo_phase);
                let m1 = self.last_mod(1);
                self.run_op(2, position, gate, m1, lfo_phase);
                self.run_op(3, position, gate, 0.0, lfo_phase);
                carrier_accum_to_sample(sum3_i16(self.op[0].last_i16, self.op[2].last_i16, self.op[3].last_i16))
            }
            _ => {
                self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
                self.run_op(1, position, gate, 0.0, lfo_phase);
                self.run_slot_feedback(2, position, gate, 0.0, lfo_phase);
                self.run_op(3, position, gate, 0.0, lfo_phase);
                carrier_accum_to_sample(sum4_i16(
                    self.op[0].last_i16,
                    self.op[1].last_i16,
                    self.op[2].last_i16,
                    self.op[3].last_i16,
                ))
            }
        }
    }

    pub fn sample(&mut self, position: i32, gate: i32) -> f32 {
        let lfo_phase = ((self.lfo_phase_q20 as u32 >> 20) & 4095) as i32;
        let out = if self.op_count > 2 && self.algorithm >= 2 {
            self.sample_four_operator(position, gate, lfo_phase)
        } else {
            self.run_slot_feedback(0, position, gate, 0.0, lfo_phase);
            if self.algorithm & 1 == 0 {
                let m0 = self.last_mod(0);
                self.run_op(1, position, gate, m0, lfo_phase);
                carrier_accum_to_sample(self.op[1].last_i16)
            } else {
                self.run_op(1, position, gate, 0.0, lfo_phase);
                carrier_accum_to_sample(sum2_i16(self.op[0].last_i16, self.op[1].last_i16))
            }
        };
        self.lfo_phase_q20 = self.lfo_phase_q20.wrapping_add(self.lfo_step_q20);
        (out * self.amp) as f32
    }
}

/// `releaseFrames` - how many frames the note's tail needs.
fn release_frames(release_sec: f64, sample_rate: i32) -> i32 {
    let floor = (sample_rate / 20).max(1);
    if !release_sec.is_finite() || release_sec > 60.0 {
        return floor.max(sample_rate);
    }
    let frames = release_sec * sample_rate as f64;
    if frames >= 2_147_483_647.0 {
        return floor.max(sample_rate);
    }
    floor.max(frames.round() as i32)
}

/// `renderNote` - renders one note into the stereo float buffer, matching the
/// reference's `float` arithmetic. `frames` is total frames; `start`/`duration`
/// are in frames.
#[allow(clippy::too_many_arguments)]
pub fn render_note(
    buffer: &mut [f32],
    frames: i32,
    start: i32,
    duration: i32,
    tone: &CompactTone,
    midi_key: i32,
    velocity: i32,
    pan: i32,
    bend: i32,
    bend_range: i32,
    modulation: i32,
    sample_rate: i32,
) {
    if start < 0 || start >= frames {
        return;
    }
    let mut voice = VoiceRuntime::new(tone, midi_key, velocity, pan, bend, bend_range, modulation, sample_rate);
    let mut total = release_frames(voice.max_release_sec(), sample_rate) + duration;
    let floor = sample_rate / 100;
    if total < floor {
        total = floor;
    }
    if start + total > frames {
        total = frames - start;
    }
    for i in 0..total {
        let idx = ((start + i) * 2) as usize;
        let sample = voice.sample(i, duration);
        buffer[idx] += (voice.pan_l * sample as f64) as f32;
        buffer[idx + 1] += (sample as f64 * voice.pan_r) as f32;
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn op(rt: [i32; 17], dll: &[u8], ma3: &[u8]) -> CompactOperator {
        CompactOperator {
            rt_ar: rt[0],
            rt_d1r: rt[1],
            rt_d2r: rt[2],
            rt_rr: rt[3],
            rt_sl: rt[4],
            rt_level_index: rt[5],
            rt_mul: rt[6],
            rt_ksr: rt[7],
            rt_waveform: rt[8],
            rt_det: rt[9],
            rt_keylevel_sel: rt[10],
            rt_feedback: rt[11],
            rt_am_enable: rt[12],
            rt_am_depth: rt[13],
            rt_vib_enable: rt[14],
            rt_vib_depth: rt[15],
            rt_keyoff_inhibit: rt[16],
            dll: dll.to_vec(),
            ma3: ma3.to_vec(),
            ..Default::default()
        }
    }

    /// A whole note (device-path tone, algorithm 0) rendered through the port
    /// must match the reference `renderNote` sample for sample. Tone and the
    /// golden samples were captured from boss.mmf's first note via the oracle.
    #[test]
    fn render_note_matches_reference() {
        // rt_* order: ar,d1r,d2r,rr,sl,levelIndex,mul,ksr,waveform,det,keylevelSel,feedback,amEnable,amDepth,vibEnable,vibDepth,keyoffInhibit
        let op0 = op(
            [15, 6, 1, 1, 0, 0, 15, 0, 7, 0, 0, 7, 0, 2, 0, 2, 0],
            &[0x10, 0x16, 0xf0, 0x00, 0x44, 0xe0, 0x3f],
            &[0x01, 0, 0, 0, 0, 0x01, 0x06, 0x0f, 0, 0, 0, 0x02, 0, 0x02, 0, 0x0e, 0, 0x07, 0x07],
        );
        let op1 = op(
            [9, 15, 2, 2, 0, 0, 15, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0],
            &[0x21, 0x2f, 0x90, 0x00, 0x00, 0xf0, 0x30],
            &[0x02, 0, 0, 0, 0x01, 0x02, 0x0f, 0x09, 0, 0, 0, 0, 0, 0, 0, 0x0f, 0, 0x06, 0],
        );
        let tone = CompactTone {
            valid: true,
            algorithm: 0,
            feedback: 7,
            operators: vec![op0, op1],
            dll_voice: vec![
                0x81, 0x00, 0x10, 0x16, 0xf0, 0x00, 0x44, 0xe0, 0x3f, 0x21, 0x2f, 0x90, 0x00, 0x00, 0xf0, 0x30,
            ],
        };
        let total = 44100;
        let mut buf = vec![0f32; (total * 2) as usize];
        render_note(&mut buf, total, 0, 2470, &tone, 81, 81, 64, 8192, 2, 0, 44100);

        // Captured from the reference `renderNote` (stereo, center pan so the
        // two channels are equal), sample for sample.
        let golden: [f32; 16] = [
            -0.005450355,
            -0.005450355,
            0.01090071,
            0.01090071,
            -0.016378593,
            -0.016378593,
            0.021828948,
            0.021828948,
            -0.02730683,
            -0.02730683,
            0.032770947,
            0.032770947,
            -0.03824883,
            -0.03824883,
            -0.043712948,
            -0.043712948,
        ];
        for (i, &g) in golden.iter().enumerate() {
            assert!((buf[i] - g).abs() < 1e-6, "sample {i}: got {}, want {g}", buf[i]);
        }
    }
}
