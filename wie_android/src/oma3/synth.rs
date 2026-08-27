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
    lfo_level_q15, wave_sample_i16, DETUNE_CENTS, FEEDBACK_GAIN, FEEDBACK_SHIFT, KEYLEVEL_Q15, LEVEL_Q15, MA3_FREQ_BASE, MULTIPLE, PHASE_DETUNE,
    PHASE_KEY_CODE,
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
    let mut v = (to_hz / from_hz) * 65536.0;
    if v < 1.0 {
        v = 1.0;
    }
    if v > 262144.0 {
        v = 262144.0;
    }
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
