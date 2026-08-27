//! The streamed-audio primitives, ported from the reference `OracleSmaf` and
//! `OracleMa3Synth`: the Yamaha 4-bit ADPCM decode, the mono->stereo resample
//! with equal-power pan and velocity, and the per-event mix. These render the
//! recorded parts a title carries - percussion, effects, streamed loops -
//! alongside the FM voices, so a file plays as the reference does rather than
//! as its FM layer alone.

/// A decoded recording, as the reference's `DecodedAudioSample`. `pcm_mono` is
/// signed sixteen-bit at `sample_rate`.
#[derive(Clone)]
pub struct DecodedAudioSample {
    pub audio_id: i32,
    pub sample_id: i32,
    pub sample_rate: i32,
    pub pcm_mono: Vec<i16>,
}

impl DecodedAudioSample {
    pub fn resample_to_stereo(&self, out_rate: i32, pan: i32, velocity: i32) -> Vec<i16> {
        resample_mono_to_stereo(&self.pcm_mono, self.sample_rate, out_rate, pan, velocity)
    }
}

fn clamp_short(value: i64) -> i16 {
    if value > 32767 {
        32767
    } else if value < -32768 {
        -32768
    } else {
        value as i16
    }
}

/// `decodeYamahaAdpcm4Mono` - Yamaha 4-bit ADPCM to signed sixteen-bit mono.
pub fn decode_yamaha_adpcm4_mono(data: &[u8], offset: usize, len: usize) -> Vec<i16> {
    if offset > data.len() || len > data.len() - offset {
        return Vec::new();
    }
    const STEP_MUL: [i64; 8] = [14720, 14720, 14720, 14720, 19648, 26176, 32768, 39296];
    let mut out = vec![0i16; len * 2];
    let mut step = 127i32;
    let mut predictor = 0i32;
    let mut write = 0usize;
    for &byte in &data[offset..offset + len] {
        for nibble_index in 0..2 {
            let nibble = if nibble_index == 0 { byte as i32 & 15 } else { (byte as i32 & 255) >> 4 };
            let mut delta = step >> 3;
            if nibble & 1 != 0 {
                delta += step >> 2;
            }
            if nibble & 2 != 0 {
                delta += step >> 1;
            }
            if nibble & 4 != 0 {
                delta += step;
            }
            if nibble & 8 != 0 {
                delta = -delta;
            }
            let sample = (predictor + delta).clamp(-32768, 32767);
            let next_step = (STEP_MUL[(nibble & 7) as usize] * step as i64 >> 14).clamp(127, 24576);
            step = next_step as i32;
            out[write] = sample as i16;
            write += 1;
            predictor = sample;
        }
    }
    out
}

/// `resampledFrameCount` - how many output frames a recording yields at
/// `out_rate`.
pub fn resampled_frame_count(len: usize, src_rate: i32, out_rate: i32) -> i32 {
    if len <= 1 || src_rate <= 0 || out_rate <= 0 {
        return 0;
    }
    let target = 1.max((len as f64 * out_rate as f64 / src_rate as f64).ceil() as i32);
    let step = src_rate as f64 / out_rate as f64;
    let mut pos = 0.0;
    let mut count = 0;
    while count < target && pos < (len - 1) as f64 {
        count += 1;
        pos += step;
    }
    count
}

/// `resampleMonoToStereo` - resample a mono recording to stereo at `out_rate`,
/// applying the equal-power pan and the velocity gain.
pub fn resample_mono_to_stereo(mono: &[i16], src_rate: i32, out_rate: i32, pan: i32, velocity: i32) -> Vec<i16> {
    if mono.is_empty() || src_rate <= 0 || out_rate <= 0 {
        return Vec::new();
    }
    let frame_count = resampled_frame_count(mono.len(), src_rate, out_rate);
    if frame_count <= 0 {
        return Vec::new();
    }
    let mut out = vec![0i16; frame_count as usize * 2];
    let step = src_rate as f64 / out_rate as f64;
    let pan_norm = (((pan - 64) as f64) / 64.0).clamp(-1.0, 1.0);
    let gain_l = ((1.0 - pan_norm) * 0.5).sqrt();
    let gain_r = ((pan_norm + 1.0) * 0.5).sqrt();
    let vel_gain = velocity.clamp(0, 127) as f64 / 127.0;
    let mut pos = 0.0;
    let mut i = 0;
    while i < frame_count && pos < (mono.len() - 1) as f64 {
        let idx = pos as usize;
        let frac = pos - idx as f64;
        let s0 = mono[idx] as f64;
        let s1 = mono[idx + 1] as f64;
        let interp = (s0 * (1.0 - frac) + s1 * frac) * vel_gain;
        let out_idx = (i * 2) as usize;
        out[out_idx] = clamp_short((interp * gain_l).round() as i64);
        out[out_idx + 1] = clamp_short((interp * gain_r).round() as i64);
        i += 1;
        pos += step;
    }
    if i == frame_count {
        out
    } else {
        out.truncate((i * 2) as usize);
        out
    }
}

/// `mixAudioEvent` - resample one recording to stereo and add it into the
/// float buffer at `start` frames.
pub fn mix_audio_event(buffer: &mut [f32], frames: i32, start: i32, sample: &DecodedAudioSample, pan: i32, velocity: i32, sample_rate: i32) {
    if start < 0 || start >= frames {
        return;
    }
    let resampled = sample.resample_to_stereo(sample_rate, pan, velocity);
    let count = (resampled.len() / 2).min((frames - start) as usize);
    let base = (start * 2) as usize;
    for i in 0..count {
        buffer[base + i * 2] += resampled[i * 2] as f32 / 32768.0;
        buffer[base + i * 2 + 1] += resampled[i * 2 + 1] as f32 / 32768.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yamaha ADPCM decode must match the reference bit for bit.
    #[test]
    fn adpcm_matches_reference() {
        let adpcm = [0x1f, 0x8a, 0x37, 0xc2, 0x55, 0xe1, 0x09, 0xf0, 0x42, 0x7b];
        let got = decode_yamaha_adpcm4_mono(&adpcm, 0, adpcm.len());
        let want: [i16; 20] = [
            -236, -122, -292, -322, 90, 549, 844, 367, 1065, 2179, 2663, 774, -97, 164, 398, -2760, -235, 3848, 40, 7370,
        ];
        assert_eq!(got, want);
    }

    /// The mono->stereo resample (pan + velocity) must match the reference.
    #[test]
    fn resample_matches_reference() {
        let mono: Vec<i16> = (0..20).map(|i| (i * 3000 - 15000) as i16).collect();
        let got = resample_mono_to_stereo(&mono, 8000, 44100, 96, 100);
        assert_eq!(got.len(), 210, "stereo sample count");
        let want: [i16; 39] = [
            -5906, -10229, -5691, -9858, -5477, -9486, -5263, -9115, -5048, -8744, -4834, -8373, -4620, -8002, -4406, -7631, -4191, -7260, -3977,
            -6889, -3763, -6518, -3549, -6146, -3334, -5775, -3120, -5404, -2906, -5033, -2692, -4662, -2477, -4291, -2263, -3920, -2049, -3549,
            -1835,
        ];
        for (i, &w) in want.iter().enumerate() {
            assert_eq!(got[i], w, "sample {i}");
        }
    }
}
