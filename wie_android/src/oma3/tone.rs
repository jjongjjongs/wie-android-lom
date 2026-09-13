//! Tone decoding, ported from the reference `OracleSmaf`.
//!
//! These functions turn a raw voice record - the bytes a track carries for a
//! program - into the [`CompactTone`] the synth plays. The reference reaches
//! them from several formats (the two compact `4302`/`4303` layouts, the direct
//! MA-3 type-1 voice and the runtime 19-byte-per-operator voice); each is
//! reproduced here bit for bit, since the operator records they produce are
//! what the ported [`super::synth`] runtime reads.
//!
//! Every function mirrors the reference name and its exact integer widths: Java
//! `int` is [`i32`] and a byte read is masked to `0..=255`.

use super::synth::{CompactOperator, CompactTone};

/// A malformed record, as the reference's `FormatException`.
#[derive(Debug, Clone)]
pub struct FormatError(pub String);

type Result<T> = core::result::Result<T, FormatError>;

fn format_error(message: &str) -> FormatError {
    FormatError(message.to_string())
}

/// `OracleSmaf.u8` - one byte as an unsigned `0..=255`.
fn u8(data: &[u8], index: i32) -> i32 {
    data[index as usize] as i32
}

/// The `MULTIPLE`-index remap the reference applies to the raw multiple nibble.
const MUL_REMAP: [i32; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10, 12, 12, 15, 15];
/// The key-level-selector remap `{0,2,1,3}`.
const KEYLEVEL_REMAP: [i32; 4] = [0, 2, 1, 3];

/// Accumulates one tone as the reference's `ToneBuilder` does, so the field
/// order and the `dllVoice` truncation match exactly.
struct ToneBuilder {
    valid: bool,
    program: i32,
    bank_msb: i32,
    bank_lsb: i32,
    global0: i32,
    global1: i32,
    feedback: i32,
    algorithm: i32,
    operators: Vec<CompactOperator>,
    dll_voice: [u8; 32],
    dll_voice_length: i32,
    dll_operator_count: i32,
}

impl ToneBuilder {
    fn new() -> Self {
        ToneBuilder {
            valid: false,
            program: 0,
            bank_msb: 0,
            bank_lsb: 0,
            global0: 0,
            global1: 0,
            feedback: 0,
            algorithm: 0,
            operators: Vec::new(),
            dll_voice: [0; 32],
            dll_voice_length: 0,
            dll_operator_count: 0,
        }
    }

    fn build(self) -> CompactTone {
        let length = self.dll_voice_length.clamp(0, 32) as usize;
        CompactTone {
            valid: self.valid,
            program: self.program,
            bank_msb: self.bank_msb,
            bank_lsb: self.bank_lsb,
            global0: self.global0,
            global1: self.global1,
            algorithm: self.algorithm,
            feedback: self.feedback,
            operators: self.operators,
            dll_voice: self.dll_voice[..length].to_vec(),
            dll_voice_length: self.dll_voice_length,
            dll_operator_count: self.dll_operator_count,
        }
    }
}

/// `decode4302` - the two-operator compact layout. Returns whether it applied.
fn decode_4302(src: &[u8], tone: &mut ToneBuilder) -> bool {
    let remap: [i32; 16] = [0, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
    if src.len() < 12 || u8(src, 0) & 248 != 0 {
        return false;
    }
    tone.dll_voice.fill(0);
    tone.dll_voice[0] = (u8(src, 3) & 3 | 128) as u8;
    tone.dll_voice[1] = 0x80;
    tone.dll_operator_count = 2;
    tone.dll_voice_length = 16;

    for op in 0..2 {
        let base = op * 4;
        let b8 = u8(src, base + 4);
        let b7 = u8(src, base + 5);
        let b6 = u8(src, base + 6);
        let b9 = u8(src, base + 7);
        let mut v2 = b8 >> 2 & 1;
        let mut v4 = (b8 & 3) << 2 | b7 >> 6;
        let mut v5 = b9 & 7;
        if b8 & 8 == 0 {
            v4 = remap[(v4 & 15) as usize];
            if v2 == 0 {
                v2 = 8;
            } else {
                v2 = v4;
            }
        } else {
            if v2 == 0 {
                v2 = remap[(v4 & 15) as usize];
            } else {
                v2 = 5;
            }
            v4 = 0;
        }
        if op != 0 {
            v5 = 0;
        }
        let dst = (op * 7) as usize;
        let sl_remap: [i32; 16] = [0, 1, 1, 1, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15];
        tone.dll_voice[dst + 2] = (v4 << 4) as u8;
        tone.dll_voice[dst + 3] = ((remap[(b7 >> 2 & 15) as usize] & 15) + v2 * 16) as u8;
        tone.dll_voice[dst + 4] = (sl_remap[((b7 & 3) << 2 | b6 >> 6) as usize] * 16 + (b6 >> 2 & 15)) as u8;
        tone.dll_voice[dst + 5] = (((b6 & 3) << 4 | b9 >> 4) << 2) as u8;
        tone.dll_voice[dst + 6] = ((b8 >> 4 & 1) + 4) as u8;
        tone.dll_voice[dst + 7] = (b8 >> 5 << 4) as u8;
        tone.dll_voice[dst + 8] = ((b9 >> 3 & 1) * 8 + v5) as u8;
    }
    true
}

/// `decode4303` - the compact layout that carries an explicit operator count.
fn decode_4303(src: &[u8], tone: &mut ToneBuilder) -> bool {
    if src.len() < 15 || u8(src, 0) & 240 != 0 {
        return false;
    }
    let b3 = u8(src, 3);
    let mut carry = b3 >> 3 & 7;
    let mut count_field = b3 & 7;
    let op_count: i32 = if count_field < 2 { 2 } else { 4 };
    if src.len() < (op_count * 5 + 5) as usize {
        return false;
    }
    tone.dll_voice.fill(0);
    tone.dll_voice[0] = (u8(src, 4) & 3 | 128) as u8;
    tone.dll_voice[1] = ((b3 & 192) + count_field) as u8;
    tone.dll_operator_count = op_count;
    tone.dll_voice_length = op_count * 7 + 2;

    let mut op = 0;
    while op < op_count {
        let base = op * 5;
        let b9 = u8(src, base + 5);
        let b10 = u8(src, base + 6);
        let b11 = u8(src, base + 8);
        let b7 = u8(src, base + 9);
        let mut b6 = b10 >> 4;
        if b9 & 4 != 0 {
            count_field = 0;
        } else {
            count_field = b6;
        }
        if b9 >> 1 & 1 != 0 {
            b6 = 4;
        }
        let dst = (op * 7) as usize;
        tone.dll_voice[dst + 2] = (count_field << 4 | b9 & 1) as u8;
        tone.dll_voice[dst + 3] = (b10 & 15 | b6 << 4) as u8;
        tone.dll_voice[dst + 4] = src[(base + 7) as usize];
        tone.dll_voice[dst + 5] = b11 as u8;
        tone.dll_voice[dst + 6] = ((((b7 >> 4 & 3) * 2 + (b7 >> 3 & 1)) * 8 + (b7 >> 6)) * 2 + (b9 >> 3 & 1)) as u8;
        tone.dll_voice[dst + 7] = (b9 & 240) as u8;
        tone.dll_voice[dst + 8] = ((b7 & 7) * 8 + carry) as u8;
        op += 1;
        carry = 0;
    }
    true
}

/// `decodeCompactOp5Dll` - one operator from its five-byte record and the
/// seven-byte device slice, producing both the raw `ma3` record and the decoded
/// runtime set the synth reads.
fn decode_compact_op5_dll(raw: &[u8], dll: &[u8], op_index: i32, op_count: i32) -> CompactOperator {
    let mut o = CompactOperator::default();
    let mut raw5 = vec![0u8; 5];
    let n = raw.len().min(5);
    raw5[..n].copy_from_slice(&raw[..n]);
    o.raw = raw5;

    let mut dll7 = vec![0u8; 7];
    let m = dll.len().min(7);
    dll7[..m].copy_from_slice(&dll[..m]);
    o.dll = dll7;
    o.op_index = op_index;

    let d = &o.dll;
    let mut ma3 = vec![0u8; 19];
    ma3[0] = (u8(d, 0) >> 4) as u8;
    ma3[1] = (u8(d, 0) >> 3 & 1) as u8;
    ma3[2] = (u8(d, 0) >> 2 & 1) as u8;
    ma3[3] = (u8(d, 0) >> 1 & 1) as u8;
    ma3[4] = (u8(d, 0) & 1) as u8;
    ma3[5] = (u8(d, 1) >> 4) as u8;
    ma3[6] = (u8(d, 1) & 15) as u8;
    ma3[7] = (u8(d, 2) >> 4) as u8;
    ma3[8] = (u8(d, 2) & 15) as u8;
    ma3[9] = (u8(d, 3) >> 2) as u8;
    ma3[10] = (u8(d, 3) & 3) as u8;
    ma3[11] = (u8(d, 4) >> 5 & 3) as u8;
    ma3[12] = (u8(d, 4) >> 4 & 1) as u8;
    ma3[13] = (u8(d, 4) >> 1 & 3) as u8;
    ma3[14] = (u8(d, 4) & 1) as u8;
    ma3[15] = (u8(d, 5) >> 4) as u8;
    ma3[16] = (u8(d, 5) & 7) as u8;
    ma3[17] = (u8(d, 6) >> 3) as u8;
    ma3[18] = (u8(d, 6) & 7) as u8;

    o.ar = u8(d, 2) >> 4;
    o.d1r = u8(d, 2) & 15;
    o.d2r = u8(d, 3) >> 4;
    o.rr = u8(d, 3) & 15;
    o.sl = u8(d, 4) >> 4;
    o.tl = (u8(d, 5) >> 2).min(63);

    o.rt_mul = MUL_REMAP[(u8(&ma3, 15) & 15) as usize];
    o.rt_det = u8(&ma3, 16) & 7;
    o.rt_waveform = u8(&ma3, 17) & 31;
    o.rt_feedback = u8(&ma3, 18) & 7;
    o.rt_level_index = u8(&ma3, 9) & 63;
    o.rt_keylevel_sel = KEYLEVEL_REMAP[(u8(&ma3, 10) & 3) as usize];
    o.rt_ar = u8(&ma3, 7) & 15;
    o.rt_d1r = u8(&ma3, 6) & 15;
    o.rt_d2r = u8(&ma3, 0) & 15;
    o.rt_rr = u8(&ma3, 5) & 15;
    o.rt_sl = u8(&ma3, 8) & 15;
    o.rt_ksr = u8(&ma3, 4) & 1;
    o.rt_ksl = u8(&ma3, 10) & 3;
    o.rt_keyoff_inhibit = u8(&ma3, 1) & 1;
    o.rt_am_enable = u8(&ma3, 12) & 1;
    o.rt_am_depth = u8(&ma3, 11) & 3;
    o.rt_vib_enable = u8(&ma3, 14) & 1;
    o.rt_vib_depth = u8(&ma3, 13) & 3;

    o.mul = MUL_REMAP[(u8(d, 1) >> 4 & 15) as usize];
    o.dt1 = KEYLEVEL_REMAP[(u8(d, 1) >> 1 & 3) as usize];
    o.dt2 = u8(d, 1) >> 1 & 7;
    o.ksr = u8(d, 0) >> 2 & 1;
    o.ksl = u8(d, 5) & 3;
    o.am = u8(d, 0) >> 1 & 1;
    o.vib = o.rt_vib_enable;
    o.attack = o.ar;
    o.decay = o.d1r;
    o.sustain = o.sl;
    o.release = o.rr;
    o.level = o.tl >> 2;
    o.multiple = o.mul;
    o.detune = o.dt1 & 7;
    o.rate_scale = if o.ksr <= 3 { o.ksr } else { 3 };
    o.flags = u8(d, 0);
    o.waveform = o.rt_waveform;
    o.key_scale = o.ksl + if o.ksr != 0 { 1 } else { 0 };
    o.am_enable = o.am;
    o.carrier_hint = if op_index >= op_count / 2 { 1 } else { 0 };

    o.ma3 = ma3;
    o
}

/// `decodeCompactOp5Legacy` - one operator from a bare five-byte legacy record.
fn decode_compact_op5_legacy(src: &[u8]) -> CompactOperator {
    let mut o = CompactOperator::default();
    let mut raw5 = vec![0u8; 5];
    let n = src.len().min(5);
    raw5[..n].copy_from_slice(&src[..n]);
    o.raw = raw5;
    let s = &o.raw;
    o.attack = u8(s, 0) >> 4;
    o.decay = u8(s, 0) & 15;
    o.sustain = u8(s, 1) >> 4;
    o.release = u8(s, 1) & 15;
    o.level = u8(s, 2) >> 4;
    o.multiple = u8(s, 2) & 15;
    o.detune = u8(s, 3) >> 5;
    o.rate_scale = u8(s, 3) >> 3 & 3;
    o.flags = u8(s, 4) & 31;
    o.waveform = u8(s, 4) >> 3 & 7;
    o.key_scale = o.rate_scale;
    o.am_enable = u8(s, 4) & 1;
    o.carrier_hint = 1;
    o
}

/// `decodeCompactTone` - the compact-format dispatcher (`format` 1, 3, 4, or the
/// auto-detecting default).
pub fn decode_compact_tone(format: i32, params: &[u8]) -> Result<CompactTone> {
    let mut tone = ToneBuilder::new();
    if format == 4 {
        decode_ma3_direct_type1_voice(params, &mut tone)?;
        return Ok(tone.build());
    }
    if params.len() < 12 {
        return Err(format_error("Compact tone is truncated"));
    }
    tone.valid = true;
    tone.program = u8(params, 0);
    tone.bank_msb = u8(params, 1);
    tone.bank_lsb = u8(params, 2);
    tone.global0 = u8(params, 3);
    tone.global1 = u8(params, 4);
    tone.feedback = u8(params, 3) >> 3 & 7;
    tone.algorithm = u8(params, 3) & 7;

    // `applied` is whether a layout matched; `use_4302_stride` selects the
    // record stride and is set only when the `4302` layout was the decoder -
    // the `4303` and auto paths both read the five-byte stride, exactly as the
    // reference's `var11`.
    let applied;
    let use_4302_stride;
    if format == 3 {
        applied = decode_4302(params, &mut tone);
        use_4302_stride = applied;
    } else if format == 1 {
        applied = decode_4303(params, &mut tone);
        use_4302_stride = false;
    } else if decode_4303(params, &mut tone) {
        applied = true;
        use_4302_stride = false;
    } else if params.len() >= 12 {
        applied = decode_4302(params, &mut tone);
        use_4302_stride = applied;
    } else {
        applied = false;
        use_4302_stride = false;
    }

    if applied && tone.dll_operator_count >= 2 {
        let op_count = tone.dll_operator_count.min(4);
        tone.operators.clear();
        for op in 0..op_count {
            let record: Vec<u8> = if use_4302_stride {
                let mut r = vec![0u8; 5];
                let start = (op * 4 + 4) as usize;
                r[..4].copy_from_slice(&params[start..start + 4]);
                r
            } else {
                let start = (op * 5 + 5) as usize;
                params[start..start + 5].to_vec()
            };
            let dv = (op * 7) as usize;
            let device: Vec<u8> = tone.dll_voice[dv + 2..dv + 9].to_vec();
            tone.operators.push(decode_compact_op5_dll(&record, &device, op, op_count));
        }
    } else {
        if params.len() < 15 {
            return Err(format_error("Compact tone format is invalid"));
        }
        tone.operators.clear();
        tone.operators.push(decode_compact_op5_legacy(&params[5..10]));
        tone.operators.push(decode_compact_op5_legacy(&params[10..15]));
    }
    Ok(tone.build())
}

/// `decodeMa3DirectType1Voice` - a direct MA-3 type-1 voice (`format` 4).
fn decode_ma3_direct_type1_voice(src: &[u8], tone: &mut ToneBuilder) -> Result<()> {
    if src.len() < 5 {
        return Err(format_error("Direct MA-3 voice is truncated"));
    }
    let voice_len = u8(src, 4);
    if voice_len != 16 && voice_len != 30 {
        return Err(format_error("Direct MA-3 voice length is unsupported"));
    }
    if src.len() < (voice_len + 5) as usize {
        return Err(format_error("Direct MA-3 voice payload is truncated"));
    }
    let algorithm = u8(src, 6) & 7;
    let op_count: i32 = if algorithm < 2 { 2 } else { 4 };
    if voice_len < op_count * 7 + 2 {
        return Err(format_error("Direct MA-3 voice operator data is truncated"));
    }
    tone.valid = true;
    tone.program = u8(src, 0) & 127;
    tone.bank_msb = u8(src, 1);
    tone.bank_lsb = u8(src, 2) & 127;
    tone.global0 = u8(src, 5);
    tone.global1 = u8(src, 6);
    tone.algorithm = algorithm;
    tone.feedback = u8(src, 13) & 7;
    tone.dll_operator_count = op_count;
    tone.dll_voice_length = voice_len;
    let copy = voice_len as usize;
    tone.dll_voice[..copy].copy_from_slice(&src[5..5 + copy]);
    tone.operators.clear();
    for op in 0..op_count {
        let base = (op * 7) as usize;
        let record = src[7 + base..14 + base].to_vec();
        tone.operators.push(decode_compact_op5_dll(&record, &record, op, op_count));
    }
    Ok(())
}

/// `decodeMa3Dll81Tone` - the eighty-one-byte DLL record; bank is forced to 128.
pub fn decode_ma3_dll81_tone(program: i32, record: &[u8]) -> Result<CompactTone> {
    if record.len() < 81 {
        return Err(format_error("DLL81 tone record is truncated"));
    }
    decode_ma3_runtime_voice_with_bank(program, record, 128)
}

/// `decodeMa3RuntimeVoice` - the 19-byte-per-operator runtime voice.
pub fn decode_ma3_runtime_voice(program: i32, record: &[u8]) -> Result<CompactTone> {
    decode_ma3_runtime_voice_with_bank(program, record, 0)
}

fn decode_ma3_runtime_voice_with_bank(program: i32, record: &[u8], bank_msb: i32) -> Result<CompactTone> {
    if record.len() < 5 {
        return Err(format_error("MA-3 runtime voice is truncated"));
    }
    let algorithm = u8(record, 4) & 7;
    let op_count: i32 = if algorithm < 2 { 2 } else { 4 };
    if record.len() < (op_count * 19 + 5) as usize {
        return Err(format_error("MA-3 runtime voice operator data is truncated"));
    }
    let mut tone = ToneBuilder::new();
    tone.valid = true;
    tone.program = program & 127;
    tone.bank_msb = bank_msb & 0xff;
    tone.bank_lsb = u8(record, 0) & 127;
    tone.global0 = u8(record, 0);
    tone.global1 = u8(record, 1);
    tone.algorithm = algorithm;
    tone.feedback = u8(record, 23) & 7;
    tone.dll_voice[0] = (u8(record, 2) & 3 | 128) as u8;
    tone.dll_voice[1] = (algorithm | (u8(record, 3) & 3) << 6) as u8;
    tone.dll_operator_count = op_count;
    tone.dll_voice_length = op_count * 7 + 2;

    for op in 0..op_count {
        let mut ma3 = vec![0u8; 19];
        let start = (op * 19 + 5) as usize;
        ma3.copy_from_slice(&record[start..start + 19]);
        let mut o = CompactOperator {
            op_index: op,
            ..Default::default()
        };
        o.rt_mul = MUL_REMAP[(u8(&ma3, 15) & 15) as usize];
        o.rt_det = u8(&ma3, 16) & 7;
        o.rt_waveform = u8(&ma3, 17) & 31;
        o.rt_feedback = u8(&ma3, 18) & 7;
        o.rt_level_index = u8(&ma3, 9) & 63;
        o.rt_keylevel_sel = KEYLEVEL_REMAP[(u8(&ma3, 10) & 3) as usize];
        o.rt_ar = u8(&ma3, 7) & 15;
        o.rt_d1r = u8(&ma3, 6) & 15;
        o.rt_d2r = u8(&ma3, 0) & 15;
        o.rt_rr = u8(&ma3, 5) & 15;
        o.rt_sl = u8(&ma3, 8) & 15;
        o.rt_ksr = u8(&ma3, 4) & 1;
        o.rt_ksl = u8(&ma3, 10) & 3;
        o.rt_keyoff_inhibit = u8(&ma3, 1) & 1;
        o.rt_am_enable = u8(&ma3, 12) & 1;
        o.rt_am_depth = u8(&ma3, 11) & 3;
        o.rt_vib_enable = u8(&ma3, 14) & 1;
        o.rt_vib_depth = u8(&ma3, 13) & 3;
        o.ar = o.rt_ar;
        o.d1r = o.rt_d1r;
        o.d2r = o.rt_d2r;
        o.rr = o.rt_rr;
        o.sl = o.rt_sl;
        o.tl = o.rt_level_index;
        o.level = o.rt_level_index;
        o.mul = o.rt_mul;
        o.multiple = o.rt_mul;
        o.dt1 = o.rt_det;
        o.detune = o.rt_det;
        o.ksr = o.rt_ksr;
        o.ksl = o.rt_ksl;
        o.rate_scale = o.rt_ksr;
        o.key_scale = o.rt_ksl;
        o.am = o.rt_am_enable;
        o.am_enable = o.rt_am_enable;
        o.vib = o.rt_vib_enable;
        o.waveform = o.rt_waveform;
        o.carrier_hint = if op >= op_count / 2 { 1 } else { 0 };
        o.ma3 = ma3;
        tone.operators.push(o);
    }
    Ok(tone.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// The same operator fingerprint the oracle's `DumpTones` emits.
    fn op_canon(o: &CompactOperator) -> String {
        let fields = [
            o.ar,
            o.attack,
            o.d1r,
            o.decay,
            o.d2r,
            o.rr,
            o.release,
            o.sl,
            o.sustain,
            o.tl,
            o.level,
            o.mul,
            o.multiple,
            o.ksr,
            o.waveform,
            o.dt1,
            o.dt2,
            o.detune,
            o.am,
            o.am_enable,
            o.ksl,
            o.vib,
            o.rt_ar,
            o.rt_d1r,
            o.rt_d2r,
            o.rt_rr,
            o.rt_sl,
            o.rt_level_index,
            o.rt_mul,
            o.rt_ksr,
            o.rt_waveform,
            o.rt_det,
            o.rt_keylevel_sel,
            o.rt_feedback,
            o.rt_am_enable,
            o.rt_am_depth,
            o.rt_vib_enable,
            o.rt_vib_depth,
            o.rt_keyoff_inhibit,
            o.rt_ksl,
            o.flags,
            o.rate_scale,
            o.key_scale,
            o.op_index,
            o.carrier_hint,
        ];
        let mut parts: Vec<String> = fields.iter().map(|v| v.to_string()).collect();
        parts.push(hex(&o.ma3));
        parts.push(hex(&o.dll));
        parts.push(hex(&o.raw));
        parts.join(",")
    }

    fn tone_canon(t: &CompactTone) -> String {
        let mut s = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            t.valid as i32,
            t.program,
            t.bank_msb,
            t.bank_lsb,
            t.global0,
            t.global1,
            t.algorithm,
            t.feedback,
            t.dll_voice_length,
            t.dll_operator_count,
            hex(&t.dll_voice),
            t.operators.len(),
        );
        for o in &t.operators {
            s.push_str("||");
            s.push_str(&op_canon(o));
        }
        s
    }

    /// Every tone the reference decoded from the corpus of real game files must
    /// decode identically here. The fixture is `format<TAB>params<TAB>canon`
    /// captured from `OracleSmaf` running as an oracle.
    #[test]
    fn decodes_every_tone_like_the_reference() {
        let fixture = include_str!("data/tone_vectors.txt");
        let mut checked = 0;
        for (line_no, line) in fixture.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut cols = line.split('\t');
            let format: i32 = cols.next().unwrap().parse().unwrap();
            let params = unhex(cols.next().unwrap());
            let expected = cols.next().unwrap();
            let tone = decode_compact_tone(format, &params).unwrap_or_else(|e| panic!("line {}: decode failed: {}", line_no + 1, e.0));
            let got = tone_canon(&tone);
            if got != expected {
                let diff = got.chars().zip(expected.chars()).position(|(a, b)| a != b).unwrap_or(0);
                let from = diff.saturating_sub(10);
                panic!(
                    "line {} (format {format}) mismatch at {}:\n got: ...{}\nwant: ...{}",
                    line_no + 1,
                    diff,
                    &got[from..(from + 60).min(got.len())],
                    &expected[from..(from + 60).min(expected.len())],
                );
            }
            checked += 1;
        }
        assert!(checked >= 40, "expected the full corpus, only checked {checked}");
    }
}
