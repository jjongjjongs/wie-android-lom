//! The chip's output stage.
//!
//! Volume, expression, velocity and pan do not multiply on the MA-3. Each is
//! looked up as an attenuation in decibel-ish units, the four are added, and
//! the sum is turned back into a gain by one table. Adding attenuations is not
//! the same as multiplying gains once the sum saturates, and it is why a
//! handset's music thins out rather than fading evenly as a track is turned
//! down.
//!
//! Voices are then summed with a clamp to sixteen bits at every step, so a
//! loud passage clips the way the chip clipped rather than being scaled to
//! fit.

/// Attenuation past which nothing is heard.
const SILENT: u32 = 192;

const CTRL_ATT: [u16; 128] = [
    192, 168, 144, 130, 120, 112, 106, 101, 96, 92, 88, 85, 82, 79, 77, 74, 72, 70, 68, 66, 64, 63, 61, 59, 58, 56, 55, 54, 53, 51, 50, 49, 48, 47,
    46, 45, 44, 43, 42, 41, 40, 39, 38, 38, 37, 36, 35, 35, 34, 33, 32, 32, 31, 30, 30, 29, 28, 28, 27, 27, 26, 25, 25, 24, 24, 23, 23, 22, 22, 21,
    21, 20, 20, 19, 19, 18, 18, 17, 17, 16, 16, 16, 15, 15, 14, 14, 14, 13, 13, 12, 12, 12, 11, 11, 10, 10, 10, 9, 9, 9, 8, 8, 8, 7, 7, 7, 6, 6, 6,
    5, 5, 5, 4, 4, 4, 3, 3, 3, 3, 2, 2, 2, 1, 1, 1, 1, 0, 0,
];

const VELOCITY_ATT: [u16; 128] = [
    192, 192, 72, 65, 60, 56, 53, 50, 48, 46, 44, 42, 41, 40, 38, 37, 36, 35, 34, 33, 32, 31, 30, 30, 29, 28, 28, 27, 26, 26, 25, 24, 24, 23, 23, 22,
    22, 21, 21, 21, 20, 20, 19, 19, 18, 18, 18, 17, 17, 17, 16, 16, 16, 15, 15, 15, 14, 14, 14, 13, 13, 13, 12, 12, 12, 12, 11, 11, 11, 11, 10, 10,
    10, 10, 9, 9, 9, 9, 8, 8, 8, 8, 8, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1,
    1, 1, 1, 1, 0, 0, 0, 0,
];

const PAN_LEFT: [u16; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3,
    3, 3, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8, 9, 9, 9, 9, 10, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15,
    15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 21, 21, 22, 23, 24, 24, 25, 26, 27, 28, 29, 31, 32, 33, 35, 36, 38, 40, 43, 45, 48, 52, 57, 64, 76, 192,
];

const PAN_RIGHT: [u16; 128] = [
    192, 76, 64, 57, 52, 48, 45, 43, 40, 38, 36, 35, 33, 32, 31, 29, 28, 27, 26, 25, 24, 24, 23, 22, 21, 21, 20, 19, 19, 18, 18, 17, 17, 16, 16, 15,
    15, 14, 14, 13, 13, 13, 12, 12, 11, 11, 11, 10, 10, 10, 9, 9, 9, 9, 8, 8, 8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 3, 3,
    3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const PERIOD_Q15: [u32; 193] = [
    32768, 30935, 29205, 27571, 26029, 24573, 23198, 21900, 20675, 19519, 18427, 17396, 16423, 15504, 14637, 13818, 13045, 12315, 11627, 10976,
    10362, 9783, 9235, 8719, 8231, 7771, 7336, 6925, 6538, 6172, 5827, 5501, 5193, 4903, 4629, 4370, 4125, 3894, 3677, 3471, 3277, 3093, 2920, 2757,
    2603, 2457, 2320, 2190, 2068, 1952, 1843, 1740, 1642, 1550, 1464, 1382, 1305, 1232, 1163, 1098, 1036, 978, 924, 872, 823, 777, 734, 693, 654,
    617, 583, 550, 519, 490, 463, 437, 413, 389, 368, 347, 328, 309, 292, 276, 260, 246, 232, 219, 207, 195, 184, 174, 164, 155, 146, 138, 130, 123,
    116, 110, 104, 98, 92, 87, 82, 78, 73, 69, 65, 62, 58, 55, 52, 49, 46, 44, 41, 39, 37, 35, 33, 31, 29, 28, 26, 25, 23, 22, 21, 20, 18, 17, 16,
    16, 15, 14, 13, 12, 12, 11, 10, 10, 9, 9, 8, 8, 7, 7, 7, 6, 6, 6, 5, 5, 5, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
];

fn at(table: &[u16; 128], value: u8) -> u32 {
    table[value.min(127) as usize] as u32
}

/// Gain for one side of one voice, as Q15.
///
/// `velocity_uses_control_curve` is a mode a sequence can ask for, where
/// velocity is attenuated on the gentler curve the controllers use.
fn gain_q15(volume: u8, expression: u8, velocity: u8, master: u8, velocity_uses_control_curve: bool, pan: u32) -> i32 {
    let velocity = at(if velocity_uses_control_curve { &CTRL_ATT } else { &VELOCITY_ATT }, velocity.max(1));
    let controls = at(&CTRL_ATT, volume) + at(&CTRL_ATT, expression) + velocity + at(&CTRL_ATT, master);

    PERIOD_Q15[(controls.min(SILENT) + pan).min(SILENT) as usize] as i32
}

/// Left and right gains for a voice, as Q15.
pub fn stereo_gain_q15(volume: u8, expression: u8, velocity: u8, master: u8, pan: u8) -> (i32, i32) {
    let position = pan.min(127) as usize;

    (
        gain_q15(volume, expression, velocity, master, false, PAN_LEFT[position] as u32),
        gain_q15(volume, expression, velocity, master, false, PAN_RIGHT[position] as u32),
    )
}

/// Adds one voice into an accumulator, clamping the way the chip did.
pub fn mix_q15(accumulator: i32, sample: f64, gain_q15: i32) -> i32 {
    let scaled = clamp_i16((sample * 32768.0).round() as i64) as i64;

    clamp_i16(accumulator as i64 + clamp_i16((scaled * gain_q15 as i64) >> 15) as i64)
}

pub fn clamp_i16(value: i64) -> i32 {
    value.clamp(i16::MIN as i64, i16::MAX as i64) as i32
}

/// Point below which the output limiter is exact, and the ceiling it bends up
/// to. Nine tenths of full scale leaves the music untouched - it rarely reaches
/// here - while giving the top a knee to bend through.
const LIMIT_KNEE: f64 = 29490.0;
const LIMIT_CEIL: f64 = i16::MAX as f64;

/// Turns a summed sample into sixteen bits, staying exact below the knee and
/// bending everything above it smoothly up to the ceiling instead of flat
/// topping it. A recorded effect can then be lifted well up the scale for
/// loudness while its rare peaks round off rather than clip, which is what a
/// hard clamp turned into a harsh edge.
pub fn soft_limit(value: i64) -> i32 {
    let magnitude = value.unsigned_abs() as f64;
    if magnitude <= LIMIT_KNEE {
        return value as i32;
    }

    let span = LIMIT_CEIL - LIMIT_KNEE;
    let limited = LIMIT_KNEE + span * ((magnitude - LIMIT_KNEE) / span).tanh();
    let limited = if value < 0 { -limited } else { limited };

    limited.round() as i32
}

/// How hard a recorded effect is driven into the saturator below. A recorded
/// effect carries its energy in short transients and stays faint between them,
/// so it reads as far quieter than a sustained synthesised note that peaks no
/// higher. Driving it well past full scale and folding it back lifts the quiet
/// body toward the peaks - the same thing a compressor does - so the effect
/// reads as loud as the music rather than as a distant tap. Set high enough to
/// match the sustained synthesised sounds; past here the fold starts to square
/// the body off and read as grit rather than as more level.
const PCM_DRIVE: f64 = 8.0;

/// Applies that drive and folds the result back to the ceiling with tanh, which
/// leaves the quiet body nearly linear at the driven level while easing the
/// loud part smoothly in rather than clipping it. Returns a sixteen-bit sample.
pub fn saturate_pcm(sample: i32) -> i32 {
    let driven = sample as f64 * PCM_DRIVE;

    (LIMIT_CEIL * (driven / LIMIT_CEIL).tanh()).round() as i32
}

#[cfg(test)]
mod tests {
    use super::{PERIOD_Q15, SILENT, clamp_i16, mix_q15, stereo_gain_q15};

    #[test]
    fn silence_is_the_end_of_the_table() {
        assert_eq!(PERIOD_Q15[SILENT as usize], 0);
    }

    #[test]
    fn everything_open_and_centred_is_near_unity() {
        let (left, right) = stereo_gain_q15(127, 127, 127, 127, 64);

        assert_eq!(left, right);
        // Centre is down a few decibels on hard over, which is the pan law the
        // table carries rather than anything applied on top of it.
        assert!(left > 20000, "centred and open came out at {left}");
    }

    #[test]
    fn hard_left_silences_the_right() {
        let (left, right) = stereo_gain_q15(127, 127, 127, 127, 0);

        assert!(left > 0);
        assert_eq!(right, 0);
    }

    #[test]
    fn turning_the_volume_down_quietens_it() {
        let (loud, _) = stereo_gain_q15(127, 127, 127, 127, 64);
        let (quiet, _) = stereo_gain_q15(40, 127, 127, 127, 64);

        assert!(quiet < loud);
        assert_eq!(stereo_gain_q15(0, 127, 127, 127, 64).0, 0);
    }

    #[test]
    fn the_mix_clamps_rather_than_wrapping() {
        let mut accumulator = 0;
        for _ in 0..8 {
            accumulator = mix_q15(accumulator, 1.0, 32767);
        }

        assert_eq!(accumulator, i16::MAX as i32);
        assert_eq!(clamp_i16(-100000), i16::MIN as i32);
    }
}
