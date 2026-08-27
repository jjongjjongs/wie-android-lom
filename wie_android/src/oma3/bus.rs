//! `OracleMa3BusGains` - the streaming renderer's per-voice gain bus. It turns a
//! voice's volume/expression/velocity/master/pan controllers into a Q15 gain
//! through the reference's attenuation and period tables, and mixes FM and PCM
//! samples into the output accumulator. Ported verbatim from the reference.

use std::sync::LazyLock;

fn decode_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

fn decode_words(hex: &str) -> Vec<u16> {
    (0..hex.len() / 4)
        .map(|i| u16::from_str_radix(&hex[i * 4..i * 4 + 4], 16).unwrap())
        .collect()
}

static CTRL_ATT: LazyLock<Vec<u8>> = LazyLock::new(|| {
    decode_bytes(
        "C0A8908278706A65605C5855524F4D4A48464442403F3D3B3A38373635333231302F2E2D2C2B2A292827262625242323222120201F1E1E1D1C1C1B1B1A1919181817171616151514141313121211111010100F0F0E0E0E0D0D0C0C0C0B0B0A0A0A09090908080807070706060605050504040403030303020202010101010000",
    )
});

static PAN_LEFT: LazyLock<Vec<u8>> = LazyLock::new(|| {
    decode_bytes(
        "0000000000000000000000000000000000000000010101010101010101010101010102020202020202020203030303030303040404040404050505050505060606060707070708080808090909090A0A0A0B0B0B0C0C0D0D0D0E0E0F0F101011111212131314151516171818191A1B1C1D1F2021232426282B2D303439404CC0",
    )
});

static PAN_RIGHT: LazyLock<Vec<u8>> = LazyLock::new(|| {
    decode_bytes(
        "C04C403934302D2B2826242321201F1D1C1B1A191818171615151413131212111110100F0F0E0E0D0D0D0C0C0B0B0B0A0A0A090909090808080807070707060606060505050505050404040404040303030303030302020202020202020201010101010101010101010101010101000000000000000000000000000000000000",
    )
});

static PERIOD: LazyLock<Vec<u16>> = LazyLock::new(|| {
    decode_words(
        "800078D772156BB365AD5FFD5A9E558C50C34C3F47FB43F440273C90392D35FA32F5301B2D6B2AE0287A26372413220F20271E5B1CA81B0D198A181C16C3157D1449132712151112101D0F360E5D0D8F0CCD0C150B680AC50A2B09990910088E081407A0073306CC066A060E05B80566051904D0048B044A040C03D2039C03680337030902DE02B5028E026902470226020701EA01CF01B5019D01850170015B0148013501240114010400F600E800DB00CF00C300B800AE00A4009B0092008A0082007B0074006E00680062005C00570052004E004900450041003E003A003700340031002E002C00290027002500230021001F001D001C001A001900170016001500140012001100100010000F000E000D000C000C000B000A000A000900090008000800070007000700060006000600050005000500040004000400040003000300030003000300030002000200020002000200020002000200020001000100010001000100010001000100010001000100010001000100010001000100010000",
    )
});

static VELOCITY_ATT: LazyLock<Vec<u8>> = LazyLock::new(|| {
    decode_bytes(
        "C0C048413C383532302E2C2A2928262524232221201F1E1E1D1C1C1B1A1A19181817171616151515141413131212121111111010100F0F0F0E0E0E0D0D0D0C0C0C0C0B0B0B0B0A0A0A0A090909090808080808070707070706060606060505050505050404040404030303030303020202020202020101010101010100000000",
    )
});

static WAVE_FIXED_PAN: LazyLock<Vec<u8>> = LazyLock::new(|| {
    decode_bytes("000000000001010101020203030405060607090A0B0D0E101215181C212834C0C03428211C181512100E0D0B0A09070606050403030202010101010000000000")
});

fn clamp7(v: i32) -> i32 {
    if v < 0 { 0 } else { v.min(127) }
}

fn clamp_i16(v: i32) -> i32 {
    if v > 32767 { 32767 } else { v.max(-32768) }
}

fn to_unit(v: i32) -> f64 {
    v as f64 / 32768.0
}

/// Java `Math.round(float)` - round half up to the nearest `i32`.
fn round_f32(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

pub fn add_output(acc: f32, value: f64) -> f32 {
    to_unit(clamp_i16(round_f32(acc * 32768.0) + round_f32((value * 32768.0) as f32))) as f32
}

pub fn add_output_i16(acc: f32, value: i32) -> f32 {
    to_unit(clamp_i16(round_f32(acc * 32768.0) + value)) as f32
}

fn gain_q15(volume: i32, expression: i32, velocity: i32, master: i32, ctrl_table: bool, pan_att: i32) -> i32 {
    let sel = if ctrl_table { &*CTRL_ATT } else { &*VELOCITY_ATT };
    let vel_att = sel[clamp7(velocity.max(1)) as usize] as i32;
    let ctrl = &*CTRL_ATT;
    let sum = 192
        .min((ctrl[clamp7(volume) as usize] as i32) + (ctrl[clamp7(expression) as usize] as i32) + vel_att + (ctrl[clamp7(master) as usize] as i32));
    PERIOD[192.min(sum + pan_att) as usize] as i32
}

pub fn left_gain_q15(volume: i32, expression: i32, velocity: i32, master: i32, ctrl_table: bool, pan: i32) -> i32 {
    gain_q15(volume, expression, velocity, master, ctrl_table, PAN_LEFT[clamp7(pan) as usize] as i32)
}

pub fn right_gain_q15(volume: i32, expression: i32, velocity: i32, master: i32, ctrl_table: bool, pan: i32) -> i32 {
    gain_q15(volume, expression, velocity, master, ctrl_table, PAN_RIGHT[clamp7(pan) as usize] as i32)
}

pub fn mix_fm_q15(acc: i32, sample: f32, gain_q15: i32) -> i32 {
    let s = clamp_i16(round_f32(sample * 32768.0));
    let prod = clamp_i16(((s as i64 * gain_q15 as i64) >> 15) as i32);
    clamp_i16(acc + prod)
}

fn pcm_stream_gain(master: i32, velocity: i32, ctrl_table: bool, softened: bool, pan: i32, is_right: bool) -> f64 {
    let ctrl = &*CTRL_ATT;
    let base = ctrl[clamp7(master) as usize] as i32;
    let sel = if ctrl_table { &*CTRL_ATT } else { &*VELOCITY_ATT };
    let mut sum = base + (sel[clamp7(velocity) as usize] as i32);
    if softened {
        sum = if sum < 25 { 0 } else { sum - 24 };
    }
    sum = 192.min(sum + (ctrl[127] as i32) * 2);
    let mut pan_att = 0;
    if pan < 128 {
        let p = if is_right { &*PAN_RIGHT } else { &*PAN_LEFT };
        pan_att = p[clamp7(pan) as usize] as i32;
    }
    PERIOD[192.min(sum + pan_att) as usize] as f64 / 32768.0
}

pub fn pcm_stream_left_gain(master: i32, velocity: i32, ctrl_table: bool, softened: bool, pan: i32) -> f64 {
    pcm_stream_gain(master, velocity, ctrl_table, softened, pan, false)
}

pub fn pcm_stream_right_gain(master: i32, velocity: i32, ctrl_table: bool, softened: bool, pan: i32) -> f64 {
    pcm_stream_gain(master, velocity, ctrl_table, softened, pan, true)
}

fn wave_gain(volume: i32, expression: i32, velocity: i32, master: i32, ctrl_table: bool, pan_att: i32) -> f64 {
    let sel = if ctrl_table { &*CTRL_ATT } else { &*VELOCITY_ATT };
    let vel_att = sel[clamp7(velocity.max(1)) as usize] as i32;
    let ctrl = &*CTRL_ATT;
    let mut m = (ctrl[clamp7(master) as usize] as i32) + vel_att;
    if m < 25 {
        m = 0;
    } else {
        m -= 24;
    }
    let sum = 192.min((ctrl[clamp7(volume) as usize] as i32) + (ctrl[clamp7(expression) as usize] as i32) + m);
    PERIOD[192.min(sum + pan_att) as usize] as f64 / 32768.0
}

pub fn wave_left_gain(fixed_pan: bool, wave_index: i32, volume: i32, expression: i32, velocity: i32, master: i32, ctrl_table: bool, pan: i32) -> f64 {
    let pan_att = if fixed_pan {
        WAVE_FIXED_PAN[(wave_index & 31) as usize] as i32
    } else {
        PAN_LEFT[clamp7(pan) as usize] as i32
    };
    wave_gain(volume, expression, velocity, master, ctrl_table, pan_att)
}

pub fn wave_right_gain(
    fixed_pan: bool,
    wave_index: i32,
    volume: i32,
    expression: i32,
    velocity: i32,
    master: i32,
    ctrl_table: bool,
    pan: i32,
) -> f64 {
    let pan_att = if fixed_pan {
        WAVE_FIXED_PAN[((wave_index & 31) + 32) as usize] as i32
    } else {
        PAN_RIGHT[clamp7(pan) as usize] as i32
    };
    wave_gain(volume, expression, velocity, master, ctrl_table, pan_att)
}
