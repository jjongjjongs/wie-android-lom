//! Note extraction, ported from the reference `OracleMmfAnalysis`.
//!
//! The collector walks each track's event stream, keeps the per-channel MIDI
//! state (bank, program, volume, pan, bend, ...), resolves each note-on to a
//! tone through the same bank/program/registry logic the reference uses, and
//! emits a [`NoteEvent`] carrying everything the ported [`super::synth`] needs
//! to render it. The streamed-audio path - recorded samples, wave percussion,
//! PCM note banks - is not walked here yet; tone-driven FM voices, including FM
//! percussion from [`super::rhythm`], are.
//!
//! Integer widths follow the reference exactly, so the bit expressions, nested
//! chunk checks and explicit casts mirror it one to one; the style lints that
//! flag those are allowed here rather than reshaped.
#![allow(clippy::too_many_arguments, clippy::collapsible_if, clippy::precedence, clippy::unnecessary_cast)]

use super::audio::{DecodedAudioSample, decode_yamaha_adpcm4_mono, mix_audio_event, resampled_frame_count};
use super::rhythm;
use super::smaf::{EventInfo, Smaf, ToneEntry};
use super::synth::{self, CompactTone};

// ----- small helpers -----

fn clamp7(v: i32) -> i32 {
    v.clamp(0, 127)
}

fn clamp_midi(v: i32) -> i32 {
    v.clamp(0, 127)
}

fn combined_gain7(a: i32, b: i32) -> i32 {
    (clamp7(a) * clamp7(b) / 127).min(127)
}

fn safe_channel(v: i32) -> i32 {
    if !(0..16).contains(&v) { 0 } else { v }
}

fn safe_track(v: i32) -> i32 {
    if !(0..16).contains(&v) { 0 } else { v }
}

fn effective_bank(channel: i32, bank_msb: i32, bank_lsb: i32) -> i32 {
    let msb = bank_msb & 0xFF;
    let lsb = bank_lsb & 255;
    if msb == 0 && lsb == 0 {
        if channel == 9 { 128 } else { 0 }
    } else if msb == 124 {
        lsb
    } else if msb == 125 {
        lsb + 128 & 0xFF
    } else {
        0
    }
}

fn mobile_bank_remap(value: i32) -> i32 {
    let v = value & 127;
    if v < 10 { v + 1 } else { 0 }
}

fn put_le32(dst: &mut [u8], at: usize, value: i64) {
    dst[at] = value as u8;
    dst[at + 1] = (value >> 8) as u8;
    dst[at + 2] = (value >> 16) as u8;
    dst[at + 3] = (value >> 24) as u8;
}

/// `expandDirectType2Wave` - the wave header a type-2 tone carries.
fn expand_direct_type2_wave(src: &[u8], param5: i32) -> Option<[u8; 40]> {
    if src.len() < 14 {
        return None;
    }
    let mut o = [0u8; 40];
    o[0] = ((src[0] as i32 & 255) >> 3) as u8;
    o[2] = (src[0] & 1) as u8;
    let b1 = src[1] as i32;
    o[3] = ((b1 & 255) >> 6) as u8;
    o[4] = (b1 & 3) as u8;
    let b2 = src[2] as i32;
    o[9] = ((b2 & 255) >> 4) as u8;
    o[5] = (b2 >> 3 & 1) as u8;
    o[7] = (b2 >> 1 & 1) as u8;
    o[8] = (b2 & 1) as u8;
    let b3 = src[3] as i32;
    o[10] = ((b3 & 255) >> 4) as u8;
    o[11] = (b3 & 15) as u8;
    let b4 = src[4] as i32;
    o[12] = ((b4 & 255) >> 4) as u8;
    o[13] = (b4 & 15) as u8;
    let b5 = src[5] as i32;
    o[14] = ((b5 & 255) >> 2) as u8;
    o[15] = (b5 & 3) as u8;
    let b6 = src[6] as i32;
    o[18] = (b6 >> 5 & 3) as u8;
    o[19] = (b6 >> 4 & 1) as u8;
    o[17] = (b6 & 1) as u8;
    o[16] = (b6 >> 1 & 3) as u8;
    put_le32(&mut o, 20, ((src[7] as i32 & 255) << 8 | src[8] as i32 & 255) as i64);
    put_le32(&mut o, 24, ((src[9] as i32 & 255) << 8 | src[10] as i32 & 255) as i64);
    put_le32(&mut o, 28, ((src[11] as i32 & 255) << 8 | src[12] as i32 & 255) as i64);
    o[32] = src[13];
    put_le32(&mut o, 36, (((param5 as i64 & 65535) << 11) / 48000 + 1 >> 1) << 6);
    Some(o)
}

/// `unpack7` - undo the seven-bit high-bit packing.
fn unpack7(src: &[u8], off: i32, len: i32, cap: i32) -> Vec<u8> {
    if off < 0 || len <= 0 || off as usize >= src.len() {
        return Vec::new();
    }
    let end = (src.len() as i32).min(off + len);
    let cap = if cap <= 0 { len.max(0) } else { cap };
    let mut out = Vec::with_capacity(cap as usize);
    let mut pos = off;
    while pos < end && (out.len() as i32) < cap {
        let high = src[pos as usize] as i32;
        let mut bit = 0;
        pos += 1;
        while bit < 7 && pos < end && (out.len() as i32) < cap {
            let low = src[pos as usize] as i32;
            bit += 1;
            out.push((low & 127 | (high & 255) << bit & 128) as u8);
            pos += 1;
        }
    }
    out
}

// ----- tone definition -----

/// One resolvable tone, as the reference's `ToneDef`.
#[derive(Clone, Default)]
pub struct ToneDef {
    pub track: i32,
    pub ordinal: i32,
    pub format: i32,
    pub program: i32,
    pub bank_msb: i32,
    pub bank_lsb: i32,
    pub compact: Option<CompactTone>,
    pub builtin_rhythm: bool,
    pub tone_type: i32,
    pub param5: i32,
    pub params: Vec<u8>,
    pub registry_record: Vec<u8>,
    pub wave_record: Vec<u8>,
    pub wave_valid: bool,
}

impl ToneDef {
    fn from_tone_entry(entry: &ToneEntry) -> Self {
        let program = if !entry.params.is_empty() {
            entry.params[0] as i32
        } else {
            entry.tone_no
        };
        let bank_msb = if entry.params.len() > 1 { entry.params[1] as i32 } else { 0 };
        let bank_lsb = if entry.params.len() > 2 { entry.params[2] as i32 } else { 0 };
        let params = entry.params.clone();
        let mut def = ToneDef {
            track: entry.track_id,
            ordinal: entry.ordinal,
            format: entry.format,
            program,
            bank_msb,
            bank_lsb,
            compact: entry.decoded_tone.clone(),
            builtin_rhythm: false,
            ..Default::default()
        };
        if entry.format == 5 && params.len() >= 20 && params[5] as i32 & 255 == 14 {
            def.tone_type = 2;
            let param5 = (params[3] as i32 & 255) << 8 | params[4] as i32 & 255;
            def.param5 = param5;
            let record = params[6..20].to_vec();
            let wave = expand_direct_type2_wave(&record, param5);
            def.wave_valid = wave.is_some();
            def.wave_record = wave.map(|w| w.to_vec()).unwrap_or_default();
            def.registry_record = record;
        } else {
            def.tone_type = 1;
        }
        def.params = params;
        def
    }

    fn direct_type1(
        track: i32,
        ordinal: i32,
        bank_msb: i32,
        program: i32,
        param5: i32,
        record: &[u8],
        record_len: i32,
        compact: Option<CompactTone>,
    ) -> Self {
        let program = program & 127;
        let bank_msb = bank_msb & 0xFF;
        let mut params = vec![0u8; (record_len + 7) as usize];
        params[0] = program as u8;
        params[1] = bank_msb as u8;
        params[2] = program as u8;
        params[3] = param5 as u8;
        params[4] = record_len as u8;
        let n = record.len().min(record_len as usize);
        params[5..5 + n].copy_from_slice(&record[..n]);
        params[(record_len + 5) as usize] = if bank_msb >= 128 { 125 } else { 124 };
        params[(record_len + 6) as usize] = (bank_msb & 127) as u8;
        ToneDef {
            track,
            ordinal,
            format: 4,
            program,
            bank_msb,
            bank_lsb: program,
            compact,
            builtin_rhythm: false,
            tone_type: 1,
            param5: param5 & 65535,
            params,
            ..Default::default()
        }
    }

    fn direct_type2(track: i32, ordinal: i32, bank_msb: i32, program: i32, param5: i32, record: &[u8]) -> Self {
        let program = program & 127;
        let bank_msb = bank_msb & 0xFF;
        let param5 = param5 & 65535;
        let mut rec = record.to_vec();
        rec.resize(14, 0);
        let wave = expand_direct_type2_wave(&rec, param5);
        ToneDef {
            track,
            ordinal,
            format: 5,
            program,
            bank_msb,
            bank_lsb: program,
            compact: None,
            builtin_rhythm: false,
            tone_type: 2,
            param5,
            registry_record: rec,
            wave_valid: wave.is_some(),
            wave_record: wave.map(|w| w.to_vec()).unwrap_or_default(),
            ..Default::default()
        }
    }

    fn builtin_rhythm(track: i32, program: i32, compact: CompactTone) -> Self {
        let program = program & 127;
        ToneDef {
            track,
            ordinal: -1,
            format: -1,
            program,
            bank_msb: 128,
            bank_lsb: program,
            compact: Some(compact),
            builtin_rhythm: true,
            tone_type: 1,
            ..Default::default()
        }
    }
}

// ----- registry -----

#[derive(Clone, Default)]
struct RegistrySlot {
    address: i32,
    bank: i32,
    param5: i32,
    program: i32,
    size: i32,
    slot_id: i32,
    tone: Option<ToneDef>,
    tone_type: i32,
    used: bool,
}

impl RegistrySlot {
    fn new(slot_id: i32) -> Self {
        RegistrySlot {
            address: 65535,
            slot_id,
            ..Default::default()
        }
    }
}

struct ToneRegistry {
    key_to_slot: Vec<i32>,
    memory_used: i32,
    next_addr: i32,
    slots: Vec<RegistrySlot>,
}

impl ToneRegistry {
    fn new() -> Self {
        ToneRegistry {
            key_to_slot: vec![-1; 65536],
            memory_used: 0,
            next_addr: 16384,
            slots: (0..256).map(RegistrySlot::new).collect(),
        }
    }

    fn reset(&mut self) {
        self.key_to_slot.iter_mut().for_each(|v| *v = -1);
        self.next_addr = 16384;
        self.memory_used = 0;
        self.slots = (0..256).map(RegistrySlot::new).collect();
    }

    /// The reference copies the record into a fixed 24576-byte arena; only the
    /// returned address matters here, so the arena is tracked by a cursor.
    fn copy(&mut self, len: i32) -> i32 {
        if len > 0 {
            let addr = self.next_addr;
            if addr >= 16384 {
                let next = addr + len;
                if next <= 24576 {
                    self.next_addr = next + 1 & -2;
                    return addr;
                }
            }
        }
        -1
    }

    fn registry_key(&self, bank: i32, program: i32) -> i32 {
        let b = bank & 0xFF;
        if bank & 127 > 15 {
            return -1;
        }
        let base = if b < 128 { b } else { b + 400 };
        65535 & (base * 128 + (program & 127))
    }

    fn register(&mut self, bank: i32, program: i32, tone_type: i32, param5: i32, address: i32, size: i32, tone: Option<ToneDef>) {
        if !(16384..24576).contains(&address) {
            return;
        }
        let key = self.registry_key(bank, program);
        if key < 0 {
            return;
        }
        if self.key_to_slot[key as usize] != -1 {
            return;
        }
        let program_lsb = program & 127;
        let preferred = (bank & 128) + program_lsb;
        let mut slot = -1;
        if (preferred as usize) < self.slots.len() && !self.slots[preferred as usize].used {
            slot = preferred;
        }
        if slot < 0 {
            for (i, s) in self.slots.iter().enumerate() {
                if !s.used {
                    slot = i as i32;
                    break;
                }
            }
        }
        if slot < 0 {
            return;
        }
        let s = &mut self.slots[slot as usize];
        s.used = true;
        s.slot_id = slot;
        s.bank = bank & 0xFF;
        s.program = program_lsb;
        s.tone_type = tone_type & 0xFF;
        s.param5 = param5 & 65535;
        s.address = address & 65535;
        s.size = size & 65535;
        s.tone = tone;
        self.key_to_slot[key as usize] = slot;
    }

    fn unregister(&mut self, bank: i32, program: i32) {
        let key = self.registry_key(bank, program);
        if key < 0 {
            return;
        }
        let slot = self.key_to_slot[key as usize];
        if slot >= 0 && (slot as usize) < self.slots.len() {
            self.key_to_slot[key as usize] = -1;
            let s = &mut self.slots[slot as usize];
            s.used = false;
            s.address = 65535;
            s.tone = None;
        }
    }

    fn register_sysex_record(&mut self, bank: i32, program: i32, tone_type: i32, param5: i32, record_len: i32, tone: Option<ToneDef>) {
        let size = record_len;
        let addr = self.copy(size);
        if addr >= 0 {
            self.unregister(bank, program);
            self.register(bank, program, tone_type, param5, addr, size, tone);
        }
    }

    fn build_from_tones(&mut self, tones: &[ToneDef]) {
        self.reset();
        for def in tones {
            if def.tone_type == 2 {
                if def.wave_valid && def.registry_record.len() == 14 {
                    let addr = self.copy(def.registry_record.len() as i32);
                    if addr >= 0 {
                        self.unregister(def.bank_msb, def.bank_lsb);
                        self.register(
                            def.bank_msb,
                            def.bank_lsb,
                            2,
                            def.param5,
                            addr,
                            def.registry_record.len() as i32,
                            Some(def.clone()),
                        );
                    }
                }
            } else if let Some(compact) = &def.compact {
                if compact.valid {
                    let addr = self.copy(30);
                    if addr >= 0 {
                        self.unregister(def.bank_msb, def.bank_lsb);
                        self.register(def.bank_msb, def.bank_lsb, 1, def.program, addr, 30, Some(def.clone()));
                        if def.format != 4 {
                            self.unregister(1, def.program);
                            self.register(1, def.program, 1, 0, addr, 30, Some(def.clone()));
                        }
                    }
                }
            }
        }
    }

    fn lookup(&self, bank: i32, program: i32) -> Option<usize> {
        let key = self.registry_key(bank, program);
        if key < 0 {
            return None;
        }
        let slot = self.key_to_slot[key as usize];
        if slot >= 0 && (slot as usize) < self.slots.len() && self.slots[slot as usize].used {
            Some(slot as usize)
        } else {
            None
        }
    }

    fn resolve_tone(&self, slot: usize) -> Option<ToneDef> {
        let s = &self.slots[slot];
        if !s.used {
            return None;
        }
        let tone = s.tone.as_ref()?;
        if !(16384..24576).contains(&s.address) || s.size <= 0 || s.address + s.size > 24575 {
            return None;
        }
        if s.tone_type == 2 {
            return if tone.wave_valid { Some(tone.clone()) } else { None };
        }
        match &tone.compact {
            Some(c) if c.valid => Some(tone.clone()),
            _ => None,
        }
    }
}

/// The outcome of a tone lookup, as the reference's `ToneLookup`.
#[derive(Clone, Default)]
struct ToneLookup {
    tone: Option<ToneDef>,
    slot: Option<RegistrySlot>,
    builtin_wave_key: i32,
}

impl ToneLookup {
    fn empty() -> Self {
        ToneLookup {
            tone: None,
            slot: None,
            builtin_wave_key: -1,
        }
    }

    fn of(tone: ToneDef) -> Self {
        ToneLookup {
            tone: Some(tone),
            slot: None,
            builtin_wave_key: -1,
        }
    }

    fn builtin_wave(key: i32) -> Self {
        ToneLookup {
            tone: None,
            slot: None,
            builtin_wave_key: key & 127,
        }
    }

    fn found(&self) -> bool {
        self.tone.is_some()
    }
}

// ----- channel state -----

#[derive(Clone)]
struct ChannelState {
    all_notes_off: i32,
    all_sound_off: i32,
    bank_lsb: i32,
    bank_msb: i32,
    bank_source_lsb: i32,
    bend14: i32,
    bend_range: i32,
    expression: i32,
    fm_fixed_slot_mode: bool,
    fm_reset_env: bool,
    lookup_bank: i32,
    modulation: i32,
    mono_mode: i32,
    pan: i32,
    program: i32,
    rpn_lsb: i32,
    rpn_msb: i32,
    sustain: i32,
    volume: i32,
}

impl ChannelState {
    fn new(channel: i32) -> Self {
        ChannelState {
            all_notes_off: 0,
            all_sound_off: 0,
            bank_lsb: 0,
            bank_msb: 0,
            bank_source_lsb: 0,
            bend14: 8192,
            bend_range: 2,
            expression: 127,
            fm_fixed_slot_mode: false,
            fm_reset_env: true,
            lookup_bank: if channel == 9 { 128 } else { 0 },
            modulation: 0,
            mono_mode: 0,
            pan: 64,
            program: 0,
            rpn_lsb: 127,
            rpn_msb: 127,
            sustain: 0,
            volume: 100,
        }
    }
}

// ----- the note the synth renders -----

/// One note ready to render, as the reference's `NoteEvent`.
#[derive(Clone, Default)]
pub struct NoteEvent {
    pub order: i32,
    pub start_tick: i32,
    pub duration_tick: i32,
    pub track: i32,
    pub channel: i32,
    pub key6: i32,
    pub midi_key: i32,
    pub render_key6: i32,
    pub render_midi_key: i32,
    pub velocity: i32,
    pub combined_velocity: i32,
    pub pan: i32,
    pub volume: i32,
    pub expression: i32,
    pub modulation: i32,
    pub bend14: i32,
    pub bend_range: i32,
    pub sustain: i32,
    pub mono_mode: i32,
    pub bank_msb: i32,
    pub bank_lsb: i32,
    pub lookup_bank: i32,
    pub program: i32,
    pub host_transpose: i32,
    pub registry_slot: i32,
    pub tone_addr: i32,
    pub tone_size: i32,
    pub tone_type: i32,
    pub fm_reset_env: bool,
    pub fm_fixed_slot_mode: bool,
    pub fm_fixed_slot: i32,
    pub attenuation_master_volume: i32,
    pub attenuation_velocity_ctrl_table: bool,
    pub fm_pitch_tail_valid: bool,
    pub fm_pitch_tail_shift: i32,
    pub fm_pitch_tail_mantissa: i32,
    pub tone: Option<ToneDef>,
}

/// One control-change snapshot the streaming renderer replays, as the
/// reference's `OracleMmfAnalysis.ControlEvent`. Every control event carries the
/// full post-change channel state so the renderer can re-derive gains and pitch.
#[derive(Clone, Default)]
pub struct ControlEvent {
    pub order: i32,
    pub tick: i32,
    pub track: i32,
    pub channel: i32,
    pub volume: i32,
    pub expression: i32,
    pub pan: i32,
    pub modulation: i32,
    pub bend14: i32,
    pub bend_range: i32,
    pub sustain: i32,
    pub mono_mode: i32,
    pub all_sound_off: i32,
    pub all_notes_off: i32,
    pub changed_control: i32,
    pub attenuation_master_volume: i32,
    pub attenuation_velocity_ctrl_table: bool,
    pub fm_fixed_cmd: i32,
    pub fm_fixed_slot: i32,
    pub fm_fixed_pitch_raw: i32,
    pub fm_fixed_velocity: i32,
    pub fm_fixed_tone: Option<ToneDef>,
    pub fm_fixed_lookup_bank: i32,
    pub fm_fixed_midi_key: i32,
    pub fm_fixed_key6: i32,
    pub fm_fixed_synth_midi_key: i32,
    pub fm_fixed_synth_key6: i32,
    pub fm_fixed_pan: i32,
    pub fm_fixed_reset_env: bool,
}

struct Collector<'a> {
    smaf: &'a Smaf,
    key_base: i32,
    channels: Vec<Vec<ChannelState>>,
    registry: ToneRegistry,
    tones: Vec<ToneDef>,
    notes: Vec<NoteEvent>,
    audio_events: Vec<AudioEvent>,
    controls: Vec<ControlEvent>,
    decoded_audio_samples: Vec<DecodedAudioSample>,
    setup_bulk_audio_samples: Vec<DecodedAudioSample>,
    builtin_audio_samples: Vec<DecodedAudioSample>,
    order: i32,
    host_transpose: i32,
    fm_reset_env: bool,
    fm_fixed_slot_mode: bool,
    attenuation_master_volume: i32,
    attenuation_velocity_ctrl_table: bool,
    pcm_sample_pan: [i32; 32],
}

impl<'a> Collector<'a> {
    fn new(smaf: &'a Smaf, key_base: i32) -> Self {
        Collector {
            smaf,
            key_base,
            channels: (0..16).map(|_| (0..16).map(ChannelState::new).collect()).collect(),
            registry: ToneRegistry::new(),
            tones: Vec::new(),
            notes: Vec::new(),
            audio_events: Vec::new(),
            controls: Vec::new(),
            decoded_audio_samples: Vec::new(),
            setup_bulk_audio_samples: Vec::new(),
            builtin_audio_samples: Vec::new(),
            order: 0,
            host_transpose: 0,
            fm_reset_env: true,
            fm_fixed_slot_mode: false,
            attenuation_master_volume: 76,
            attenuation_velocity_ctrl_table: false,
            pcm_sample_pan: [-1; 32],
        }
    }

    fn key6_to_midi(&self, key: i32) -> i32 {
        clamp_midi(key + self.key_base)
    }

    fn midi_to_key6(&self, key: i32) -> i32 {
        (key - self.key_base).clamp(0, 63)
    }

    fn apply_setup_attenuation_mode(&mut self, mode: i32) {
        if mode == 0 {
            self.attenuation_master_volume = 45;
            self.attenuation_velocity_ctrl_table = true;
        } else if mode == 1 {
            self.attenuation_master_volume = 91;
            self.attenuation_velocity_ctrl_table = true;
        } else {
            self.attenuation_master_volume = 76;
            self.attenuation_velocity_ctrl_table = false;
        }
    }

    fn find_compact_local_selector(&self, track_id: i32, bank_msb: i32, bank_lsb: i32) -> Option<ToneDef> {
        let mut fallback = None;
        for def in &self.tones {
            if def.bank_msb == (bank_msb & 0xFF) && def.bank_lsb == (bank_lsb & 0xFF) {
                if def.track == safe_track(track_id) {
                    return Some(def.clone());
                }
                if fallback.is_none() {
                    fallback = Some(def.clone());
                }
            }
        }
        fallback
    }

    fn find_tone_by_bank_program(&self, track: i32, bank: i32, program: i32) -> Option<ToneDef> {
        let mut fallback = None;
        for def in &self.tones {
            if def.bank_msb == (bank & 0xFF) && def.bank_lsb == (program & 127) {
                if def.track == track {
                    return Some(def.clone());
                }
                if fallback.is_none() {
                    fallback = Some(def.clone());
                }
            }
        }
        fallback
    }

    fn create_builtin_rhythm_tone(&self, track: i32, key: i32) -> Option<ToneDef> {
        let tone = rhythm::fm_tone(key)?;
        if tone.valid {
            Some(ToneDef::builtin_rhythm(track, key, tone))
        } else {
            None
        }
    }

    fn lookup_by_bank_program(&self, track: i32, bank: i32, program: i32) -> ToneLookup {
        if let Some(slot) = self.registry.lookup(bank, program) {
            if let Some(tone) = self.registry.resolve_tone(slot) {
                return ToneLookup {
                    tone: Some(tone),
                    slot: Some(self.registry.slots[slot].clone()),
                    builtin_wave_key: -1,
                };
            }
        }
        match self.find_tone_by_bank_program(track, bank, program) {
            Some(def) => ToneLookup::of(def),
            None => ToneLookup::empty(),
        }
    }

    fn find_tone(&self, track: i32, channel: i32, state: &ChannelState, compact: bool, key: i32) -> ToneLookup {
        let mut bank = state.lookup_bank;
        if bank == 0 && channel == 9 && state.bank_msb == 0 && state.bank_lsb == 0 {
            bank = 128;
        }

        if bank & 128 != 0 {
            let program = if compact { state.program & 127 } else { key & 127 };
            let hit = self.lookup_by_bank_program(track, bank, program);
            if hit.found() {
                return hit;
            }
            if rhythm::has_wave_record(program) {
                return ToneLookup::builtin_wave(program);
            }
            return match self.create_builtin_rhythm_tone(track, program) {
                Some(def) => ToneLookup::of(def),
                None => ToneLookup::empty(),
            };
        }

        let by_bank = self.lookup_by_bank_program(track, bank, state.program);
        if by_bank.found() {
            return by_bank;
        }
        let by_one = self.lookup_by_bank_program(track, 1, state.program);
        if by_one.found() {
            return by_one;
        }
        if bank != 0 {
            let by_zero = self.lookup_by_bank_program(track, 0, state.program);
            if by_zero.found() {
                return by_zero;
            }
        }
        let by_raw = self.lookup_by_bank_program(track, state.bank_msb, state.bank_lsb);
        if by_raw.found() {
            return by_raw;
        }

        // Fall back to a linear scan, preferring same-track and bank/program
        // matches exactly as the reference.
        let mut bank_match: Option<ToneDef> = None;
        let mut any_track: Option<ToneDef> = None;
        let mut program_same_track: Option<ToneDef> = None;
        let mut program_any: Option<ToneDef> = None;
        for def in &self.tones {
            if any_track.is_none() && def.track == track {
                any_track = Some(def.clone());
            }
            let bank_program_hit = def.bank_msb == (bank & 0xFF) && def.bank_lsb == (state.program & 127);
            let raw_hit = def.bank_msb == (state.bank_msb & 0xFF) && def.bank_lsb == (state.bank_lsb & 127);
            if bank_program_hit || raw_hit {
                if def.track == track {
                    return ToneLookup::of(def.clone());
                }
                if bank_match.is_none() {
                    bank_match = Some(def.clone());
                }
            }
            if def.program == (state.program & 127) {
                if def.track == track && program_same_track.is_none() {
                    program_same_track = Some(def.clone());
                }
                if program_any.is_none() {
                    program_any = Some(def.clone());
                }
            }
        }
        if let Some(def) = bank_match {
            return ToneLookup::of(def);
        }
        if let Some(def) = program_same_track {
            return ToneLookup::of(def);
        }
        if let Some(def) = program_any {
            return ToneLookup::of(def);
        }
        if let Some(def) = any_track {
            return ToneLookup::of(def);
        }
        match self.tones.first() {
            Some(def) => ToneLookup::of(def.clone()),
            None => ToneLookup::empty(),
        }
    }

    fn apply_control(&mut self, event: &EventInfo, track: i32, channel: i32) {
        let mut state = self.channels[track as usize][channel as usize].clone();
        self.apply_control_state(&mut state, event);
        self.channels[track as usize][channel as usize] = state;
    }

    fn apply_control_state(&self, state: &mut ChannelState, event: &EventInfo) {
        let value = event.value;
        match event.control {
            0 => {
                if event.compact {
                    state.program = value & 127;
                    state.bank_lsb = value & 127;
                    state.bank_source_lsb = state.bank_lsb;
                    let bank_msb = state.bank_msb;
                    let bank_lsb = state.bank_lsb;
                    if let Some(def) = self.find_compact_local_selector(event.track_id, bank_msb, bank_lsb) {
                        state.lookup_bank = 1;
                        state.program = def.ordinal;
                    }
                } else {
                    state.bank_source_lsb = value & 0xFF;
                    state.bank_lsb = state.bank_source_lsb;
                }
            }
            1 => {
                state.bank_msb = value & 0xFF;
                if event.compact {
                    state.lookup_bank = state.bank_msb;
                }
            }
            2 => {
                if !event.compact {
                    state.program = value & 127;
                    if state.bank_msb == 124 {
                        state.bank_lsb = mobile_bank_remap(state.bank_source_lsb);
                    } else if state.bank_msb == 125 {
                        state.bank_lsb = mobile_bank_remap(state.program);
                    }
                    let ch = safe_channel(event.channel);
                    state.lookup_bank = effective_bank(ch, state.bank_msb, state.bank_lsb);
                }
            }
            4 => state.bend14 = (value & 127) << 7,
            14 => {
                state.bend14 = if event.gate != 0 { event.gate & 16383 } else { (value & 127) << 7 };
            }
            64 => state.sustain = if value <= 63 { 0 } else { 1 },
            123 => state.all_notes_off += 1,
            129 => state.modulation = value.min(4),
            6 => {
                if state.rpn_msb == 0 && state.rpn_lsb == 0 && value < 25 {
                    state.bend_range = value;
                }
            }
            7 => state.volume = clamp7(value),
            10 => state.pan = clamp7(value),
            11 => state.expression = clamp7(value),
            100 => state.rpn_lsb = value & 0xFF,
            101 => state.rpn_msb = value & 0xFF,
            120 => state.all_sound_off += 1,
            121 => {
                if value == 0 {
                    state.expression = 127;
                    state.pan = 64;
                    state.bend14 = 8192;
                    state.bend_range = 2;
                    state.rpn_msb = 127;
                    state.rpn_lsb = 127;
                    state.sustain = 0;
                    state.modulation = 0;
                    state.mono_mode = 0;
                }
            }
            126 => {
                if value == 1 {
                    state.mono_mode = 1;
                }
            }
            127 => {
                if value == 0 {
                    state.mono_mode = 0;
                }
            }
            _ => {}
        }
    }

    fn apply_initial_channel_status(&mut self, track_index: usize) {
        let track = &self.smaf.tracks[track_index];
        let track_no = safe_track(track.id);
        let midi_like = track.format_type != 0 && track.format_type != 4;
        let count = if midi_like { track.channel_status.len() as i32 } else { 4 };
        let limit = count.min(16);
        for ch in 0..limit {
            let status = if midi_like {
                if ch as usize >= track.channel_status.len() {
                    continue;
                }
                track.channel_status[ch as usize] as i32
            } else {
                let byte_index = (ch / 2) as usize;
                if byte_index >= track.channel_status.len() {
                    continue;
                }
                let byte = track.channel_status[byte_index] as i32;
                if ch & 1 != 0 { byte & 15 } else { (byte & 255) >> 4 }
            };
            if status & 3 == 3 {
                let s = &mut self.channels[track_no as usize][ch as usize];
                s.bank_msb = 128;
                s.lookup_bank = 128;
            }
        }
    }

    /// `recordControl` - snapshot a channel's post-change state as a control
    /// event for the streaming renderer. `track`/`channel` index the channel
    /// grid directly (already track-/channel-safe).
    fn record_control(&mut self, tick: i32, track: i32, channel: i32, changed_control: i32) {
        let order = self.order;
        self.order += 1;
        let state = &self.channels[track as usize][channel as usize];
        let control = ControlEvent {
            order,
            tick,
            track,
            channel,
            volume: state.volume,
            expression: state.expression,
            pan: state.pan,
            modulation: state.modulation,
            bend14: state.bend14,
            bend_range: state.bend_range,
            sustain: state.sustain,
            mono_mode: state.mono_mode,
            all_sound_off: state.all_sound_off,
            all_notes_off: state.all_notes_off,
            changed_control,
            attenuation_master_volume: self.attenuation_master_volume,
            attenuation_velocity_ctrl_table: self.attenuation_velocity_ctrl_table,
            fm_fixed_cmd: 0,
            fm_fixed_slot: 0,
            fm_fixed_pitch_raw: 0,
            fm_fixed_velocity: 0,
            fm_fixed_tone: None,
            fm_fixed_lookup_bank: 0,
            fm_fixed_midi_key: 0,
            fm_fixed_key6: 0,
            fm_fixed_synth_midi_key: 0,
            fm_fixed_synth_key6: 0,
            fm_fixed_pan: 64,
            fm_fixed_reset_env: state.fm_reset_env,
        };
        self.controls.push(control);
    }

    /// `recordFixedSlotControl` - the FM fixed-slot note-on/off carried by a
    /// family-3 sample sysex while fixed-slot mode is active.
    fn record_fixed_slot_control(&mut self, event: &EventInfo) {
        let track = safe_track(event.track_id);
        let slot = event.sysex_value & 15;
        if slot >= 16 {
            return;
        }
        let velocity = if event.sysex_type == 6 && event.payload.len() >= 10 {
            event.payload[9] as i32 & 127
        } else {
            0
        };
        let synth_key = self.key6_to_midi(0);
        let lookup = self.find_tone_for_fixed_slot(track, slot, &self.channels[track as usize][slot as usize].clone());
        let state = &self.channels[track as usize][slot as usize];
        let order = self.order;
        self.order += 1;
        let control = ControlEvent {
            order,
            tick: event.tick,
            track,
            channel: safe_channel(slot),
            volume: state.volume,
            expression: state.expression,
            pan: state.pan,
            modulation: state.modulation,
            bend14: state.bend14,
            bend_range: state.bend_range,
            sustain: state.sustain,
            mono_mode: state.mono_mode,
            all_sound_off: state.all_sound_off,
            all_notes_off: state.all_notes_off,
            changed_control: (event.sysex_type & 0xFF) | 256,
            attenuation_master_volume: self.attenuation_master_volume,
            attenuation_velocity_ctrl_table: self.attenuation_velocity_ctrl_table,
            fm_fixed_cmd: event.sysex_type,
            fm_fixed_slot: safe_channel(slot),
            fm_fixed_pitch_raw: event.sysex_arg & 65535,
            fm_fixed_velocity: velocity & 127,
            fm_fixed_tone: lookup.tone.clone(),
            fm_fixed_lookup_bank: state.lookup_bank & 0xFF,
            fm_fixed_midi_key: synth_key & 127,
            fm_fixed_key6: 0,
            fm_fixed_synth_midi_key: synth_key & 127,
            fm_fixed_synth_key6: 0,
            fm_fixed_pan: state.pan,
            fm_fixed_reset_env: self.fm_reset_env,
        };
        self.controls.push(control);
    }

    /// `findToneForFixedSlot` - the tone a fixed slot plays, given the channel's
    /// bank state, falling back to bank 0 then a built-in rhythm tone.
    fn find_tone_for_fixed_slot(&self, track: i32, slot: i32, state: &ChannelState) -> ToneLookup {
        let mut bank = state.lookup_bank;
        if bank == 0 && slot == 9 && state.bank_msb == 0 && state.bank_lsb == 0 {
            bank = 128;
        }
        if bank < 128 {
            let found = self.lookup_by_bank_program(track, bank, state.program);
            if found.found() {
                found
            } else {
                self.lookup_by_bank_program(track, 0, state.program)
            }
        } else {
            let found = self.lookup_by_bank_program(track, bank, 0);
            if found.found() {
                found
            } else {
                match self.create_builtin_rhythm_tone(track, 0) {
                    Some(tone) => ToneLookup::of(tone),
                    None => ToneLookup::empty(),
                }
            }
        }
    }

    fn apply_sysex(&mut self, event: &EventInfo, track: i32) {
        let track_no = safe_track(event.track_id);
        if event.sysex_family == 2 && event.sysex_type == 0 {
            self.fm_reset_env = event.sysex_value != 1;
            self.fm_fixed_slot_mode = event.sysex_value == 1;
            self.apply_setup_attenuation_mode(event.sysex_value);
            for ch in 0..16 {
                let s = &mut self.channels[track_no as usize][ch];
                s.fm_reset_env = self.fm_reset_env;
                s.fm_fixed_slot_mode = self.fm_fixed_slot_mode;
            }
            for ch in 0..16 {
                self.record_control(event.tick, track_no, ch, -1);
            }
        } else if event.sysex_family == 2 && event.sysex_type == 4 {
            self.apply_type4_tone_sysex(track_no, &event.payload);
        } else if event.sysex_family == 2 && (event.sysex_type == 5 || event.sysex_type == 6) {
            // Setup-bulk sample sysex - streamed audio, handled elsewhere.
        } else if event.sysex_family == 3 && (event.sysex_type == 5 || event.sysex_type == 6) && self.fm_fixed_slot_mode {
            self.record_fixed_slot_control(event);
        } else if event.sysex_family == 1 && event.sysex_type == 33 && event.sysex_event_code == 33 && event.sysex_value == 1 {
            let transpose = (event.sysex_arg & 0xFF) - 12;
            if (-12..=12).contains(&transpose) {
                self.host_transpose = transpose;
            }
        } else if event.sysex_family == 1 && event.sysex_type == 11 && event.sysex_event_code == 22 {
            let idx = event.sysex_value;
            if (idx as usize) < self.pcm_sample_pan.len() {
                self.pcm_sample_pan[idx as usize] = event.sysex_arg & 0xFF;
            }
        } else if event.sysex_family == 1 && event.sysex_type == 0 && event.sysex_event_code == 23 {
            self.attenuation_master_volume = clamp7(event.sysex_value);
            for ch in 0..16 {
                self.record_control(event.tick, track_no, ch, -1);
            }
        }
        let _ = track;
    }

    fn apply_type4_tone_sysex(&mut self, track: i32, payload: &[u8]) {
        if payload.len() < 17 || payload[payload.len() - 1] != 0xF7 {
            return;
        }
        let u = |i: usize| payload[i] as i32;
        if !(u(0) == 67 && u(1) == 121 && u(2) == 6 && u(3) == 124 && u(4) == 33 && u(5) == 4) {
            return;
        }
        let bank = (if payload[6] & 1 != 0 { 128 } else { 0 }) + (u(7) & 127);
        let program = u(8) & 127;
        let kind = u(9) & 255;
        let b10 = payload[10] as i32;
        let b9 = payload[11] as i32;
        let b12 = payload[12] as i32;
        let param5 = (u(13) & 127) << 7 | (b10 & 15) << 28 | (b9 & 127) << 21 | (b12 & 127) << 14 | (u(14) & 127);
        let record_len = if kind == 1 {
            30
        } else if kind == 2 {
            14
        } else {
            0
        };
        if record_len == 0 {
            return;
        }
        let record = unpack7(payload, 15, payload.len() as i32 - 16, record_len);
        if (record.len() as i32) < record_len {
            return;
        }
        let def = if kind == 1 {
            self.add_direct_type1_tone(track, bank, program, param5, &record, record_len)
        } else {
            self.add_direct_type2_tone(track, bank, program, param5, &record)
        };
        self.registry.register_sysex_record(bank, program, kind, param5 & 65535, record_len, def);
    }

    fn add_direct_type1_tone(&mut self, track: i32, bank: i32, program: i32, param5: i32, record: &[u8], record_len: i32) -> Option<ToneDef> {
        // Build the format-4 voice the reference decodes.
        let program7 = program & 127;
        let bank_ff = bank & 0xFF;
        let mut voice = vec![0u8; (record_len + 7) as usize];
        voice[0] = program7 as u8;
        voice[1] = bank_ff as u8;
        voice[2] = program7 as u8;
        voice[3] = param5 as u8;
        voice[4] = record_len as u8;
        voice[5..(5 + record_len) as usize].copy_from_slice(&record[..record_len as usize]);
        voice[(record_len + 5) as usize] = if bank_ff >= 128 { 125 } else { 124 };
        voice[(record_len + 6) as usize] = (bank_ff & 127) as u8;
        let compact = super::tone::decode_compact_tone(4, &voice).ok()?;
        let ordinal = (self.tones.len() as i32 & 31) + 224;
        let def = ToneDef::direct_type1(track, ordinal, bank, program, param5, record, record_len, Some(compact));
        self.tones.push(def.clone());
        Some(def)
    }

    fn add_direct_type2_tone(&mut self, track: i32, bank: i32, program: i32, param5: i32, record: &[u8]) -> Option<ToneDef> {
        let ordinal = (self.tones.len() as i32 & 31) + 224;
        let def = ToneDef::direct_type2(track, ordinal, bank, program, param5, record);
        if !def.wave_valid {
            return None;
        }
        self.tones.push(def.clone());
        Some(def)
    }

    /// `replaceSample` - overwrite a sample with the same audio and sample id,
    /// or append it.
    fn replace_sample(list: &mut Vec<DecodedAudioSample>, sample: DecodedAudioSample) {
        if let Some(slot) = list.iter_mut().find(|s| s.audio_id == sample.audio_id && s.sample_id == sample.sample_id) {
            *slot = sample;
        } else {
            list.push(sample);
        }
    }

    /// `findAudioSampleExact` - the decoded sample for an audio and sample id.
    fn find_audio_sample_exact(list: &[DecodedAudioSample], audio_id: i32, sample_id: i32) -> Option<DecodedAudioSample> {
        list.iter().find(|s| s.audio_id == audio_id && s.sample_id == sample_id).cloned()
    }

    /// `findSetupBulkTone` - the type-2 wave tone in a track whose record names a
    /// setup-bulk block.
    fn find_setup_bulk_tone(&self, track: i32, block: i32) -> Option<&ToneDef> {
        self.tones.iter().find(|t| {
            t.track == track && t.tone_type == 2 && t.wave_valid && t.registry_record.len() == 14 && (t.registry_record[13] as i32 & 255) == block
        })
    }

    /// `findSetupBulkSample` - the setup-bulk recording a type-2 tone references.
    fn find_setup_bulk_sample(&self, tone: &ToneDef) -> Option<DecodedAudioSample> {
        if tone.registry_record.len() != 14 {
            return None;
        }
        let block = tone.registry_record[13] as i32 & 255;
        Self::find_audio_sample_exact(&self.setup_bulk_audio_samples, 255, block)
    }

    /// `builtinWaveSample` - the recording for a built-in wave key or a type-2
    /// wave record, cached by sample id.
    fn builtin_wave_sample(&mut self, key: i32, record: &[u8]) -> Option<DecodedAudioSample> {
        let key = key & 127;
        if let Some(existing) = Self::find_audio_sample_exact(&self.builtin_audio_samples, -1, key) {
            return Some(existing);
        }
        let sample = rhythm::wave_sample_for_record(key, record)?;
        self.builtin_audio_samples.push(sample.clone());
        Some(sample)
    }

    /// Decode every recorded sample a file carries - the audio tracks, the MTSP
    /// streams and the setup-bulk blocks - into [`Self::decoded_audio_samples`],
    /// exactly as the reference's `collect` does before it walks the events.
    fn decode_all_audio_samples(&mut self) {
        for audio in &self.smaf.audios {
            let rate = if audio.sample_rate == 0 { 4000 } else { audio.sample_rate };
            for s in &audio.samples {
                let pcm = decode_yamaha_adpcm4_mono(&s.data, 0, s.data.len());
                self.decoded_audio_samples.push(DecodedAudioSample {
                    audio_id: audio.id,
                    sample_id: s.id,
                    sample_rate: rate,
                    pcm_mono: pcm,
                });
            }
        }
        for track in &self.smaf.tracks {
            for m in &track.mtsp_samples {
                let pcm = if m.codec == 1 {
                    decode_yamaha_adpcm4_mono(&m.data, 0, m.data.len())
                } else {
                    m.data
                        .iter()
                        .map(|&b| {
                            let v = if m.codec == 2 { (b as i32 & 255) - 128 } else { b as i32 & 255 };
                            (v << 8) as i16
                        })
                        .collect()
                };
                self.decoded_audio_samples.push(DecodedAudioSample {
                    audio_id: m.track_id,
                    sample_id: m.sample_id,
                    sample_rate: m.sample_rate,
                    pcm_mono: pcm,
                });
            }
        }
        self.load_setup_bulk_samples();
    }

    /// `loadSetupBulkSamples` - decode each setup-bulk block its type-2 tone
    /// claims into a 48 kHz sample under audio id 255.
    fn load_setup_bulk_samples(&mut self) {
        for track_index in 0..self.smaf.tracks.len() {
            let entries = self.smaf.tracks[track_index].setup_bulk_entries.clone();
            for entry in &entries {
                let Some(tone) = self.find_setup_bulk_tone(entry.track_id, entry.block_id) else {
                    continue;
                };
                let record = &tone.registry_record;
                let codec = record[1] as i32 & 3;
                let hi = record[11] as i32;
                let count = record[12] as i32 & 255 | (hi & 0xFF) << 8;
                if codec != 0 && codec != 2 && codec != 3 {
                    continue;
                }
                let packed = &entry.packed_data;
                let cap = 1.max(packed.len() as i32 - packed.len() as i32 / 8 + 1);
                let unpacked = unpack7(packed, 0, packed.len() as i32, cap);
                let take = if codec == 0 { count + 1 >> 1 } else { count };
                let end = take.max(0).min(unpacked.len() as i32);
                if end == 0 {
                    continue;
                }
                let pcm = if codec == 0 {
                    decode_yamaha_adpcm4_mono(&unpacked, 0, end as usize)
                } else {
                    (0..end as usize)
                        .map(|i| {
                            let v = if codec == 2 {
                                (unpacked[i] as i32 & 255) - 128
                            } else {
                                unpacked[i] as i32 & 255
                            };
                            (v << 8) as i16
                        })
                        .collect()
                };
                let sample = DecodedAudioSample {
                    audio_id: 255,
                    sample_id: entry.block_id,
                    sample_rate: 48000,
                    pcm_mono: pcm,
                };
                Self::replace_sample(&mut self.decoded_audio_samples, sample.clone());
                Self::replace_sample(&mut self.setup_bulk_audio_samples, sample);
            }
        }
    }

    /// `wavePitchRatioQ16` - the Q16 resample ratio a recorded note plays at,
    /// keyed by its bank context.
    fn wave_pitch_ratio_q16(&self, note: &NoteEvent, lookup: &ToneLookup) -> i32 {
        let tone_bank_msb = lookup.tone.as_ref().map(|t| t.bank_msb);
        let slot_bank = lookup.slot.as_ref().map(|s| s.bank);
        let mut ctx = 128;
        if note.lookup_bank & 128 == 0 && note.bank_msb != 128 {
            if tone_bank_msb == Some(128) {
                return synth::wave_pitch_ratio_for_context(128, note.render_midi_key, 0, note.host_transpose);
            }
            if let Some(bank) = slot_bank {
                if bank & 128 != 0 {
                    return synth::wave_pitch_ratio_for_context(128, note.render_midi_key, 0, note.host_transpose);
                }
            }
            if note.render_key6 & 128 != 0 {
                ctx = 128;
            } else if (115..=128).contains(&note.lookup_bank) {
                ctx = note.lookup_bank;
            } else if note.bank_msb == 124 && (115..=128).contains(&note.bank_lsb) {
                ctx = note.bank_lsb;
            } else if note.bank_msb == 125 {
                ctx = note.bank_lsb + 128 & 0xFF;
            } else {
                ctx = note.render_midi_key;
            }
        }
        synth::wave_pitch_ratio_for_context(ctx, note.render_midi_key, 0, note.host_transpose)
    }

    /// `addBuiltinWaveRhythmEvent` - emit the recording a built-in wave-rhythm
    /// key plays, or return false if it carries none.
    fn add_builtin_wave_rhythm_event(
        &mut self,
        event: &EventInfo,
        track: i32,
        channel: i32,
        state: &ChannelState,
        note: &NoteEvent,
        key: i32,
        q16: i32,
    ) -> bool {
        let record = rhythm::wave_record_for_key(key);
        if record.is_empty() {
            return false;
        }
        let Some(sample) = self.builtin_wave_sample(key, &record) else {
            return false;
        };
        self.audio_events.push(AudioEvent {
            order: self.order,
            start_tick: event.tick,
            duration_tick: if event.gate != 0 { event.gate } else { 1 },
            audio_id: track,
            sample_id: key,
            velocity: note.velocity.max(1),
            pan: state.pan,
            sample,
            track,
            channel,
            volume: state.volume,
            expression: state.expression,
            builtin_wave: true,
            wave_pitch_ratio_q16: q16,
            attenuation_master_volume: self.attenuation_master_volume,
            attenuation_velocity_ctrl_table: self.attenuation_velocity_ctrl_table,
            pcm_stream: false,
            pcm_master_softened: false,
            wave_record: record,
        });
        self.order += 1;
        true
    }

    /// `addDirectType2WaveEvent` - emit the recording a direct type-2 wave tone
    /// plays: its setup-bulk block if it names one, else its built-in bank.
    fn add_direct_type2_wave_event(
        &mut self,
        event: &EventInfo,
        track: i32,
        channel: i32,
        state: &ChannelState,
        note: &NoteEvent,
        lookup: &ToneLookup,
        q16: i32,
    ) -> bool {
        let tone = match &lookup.tone {
            Some(t) if t.tone_type == 2 && t.wave_valid => t.clone(),
            _ => return false,
        };
        let bank_id = tone.wave_record[32] as i32 & 255;
        let sample = match self.find_setup_bulk_sample(&tone) {
            Some(s) => s,
            None => match self.builtin_wave_sample(bank_id, &tone.wave_record) {
                Some(s) => s,
                None => return false,
            },
        };
        self.audio_events.push(AudioEvent {
            order: self.order,
            start_tick: event.tick,
            duration_tick: if event.gate != 0 { event.gate } else { 1 },
            audio_id: -1,
            sample_id: bank_id,
            velocity: note.velocity.max(1),
            pan: state.pan,
            sample,
            track,
            channel,
            volume: state.volume,
            expression: state.expression,
            builtin_wave: true,
            wave_pitch_ratio_q16: q16,
            attenuation_master_volume: self.attenuation_master_volume,
            attenuation_velocity_ctrl_table: self.attenuation_velocity_ctrl_table,
            pcm_stream: false,
            pcm_master_softened: false,
            wave_record: tone.wave_record.clone(),
        });
        self.order += 1;
        true
    }

    fn copy_tone_metadata(&self, note: &mut NoteEvent, lookup: &ToneLookup) {
        let slot = lookup.slot.as_ref();
        note.registry_slot = slot.map_or(-1, |s| s.slot_id);
        note.tone_addr = slot.map_or(65535, |s| s.address);
        note.tone_size = slot.map_or(0, |s| s.size);
        note.tone_type = if let Some(s) = slot {
            s.tone_type
        } else if lookup.tone.as_ref().and_then(|t| t.compact.as_ref()).is_some() {
            1
        } else {
            0
        };
    }

    fn set_render_pitch(&self, note: &mut NoteEvent) {
        note.render_midi_key = note.midi_key;
        note.render_key6 = note.key6;
        if note.lookup_bank & 128 == 0 {
            // Melodic voice: render key is the note's own key, and its FM pitch
            // tail comes from the neutral (bank 0, mode 2) context.
            let tail = synth::fm_pitch_tail_for_context(0, note.midi_key, 2, note.host_transpose);
            note.fm_pitch_tail_valid = true;
            note.fm_pitch_tail_shift = (tail >> 16) & 7;
            note.fm_pitch_tail_mantissa = tail & 1023;
            return;
        }
        if let Some(tone) = &note.tone {
            if tone.compact.is_some() {
                let mut key = if tone.builtin_rhythm {
                    rhythm::fixed_key(tone.program)
                } else {
                    tone.bank_lsb
                };
                if key > 127 {
                    key = note.program & 127;
                }
                note.render_midi_key = key;
                note.render_key6 = self.midi_to_key6(key);
                if note.tone_type == 1 {
                    let (bank_arg, key_arg) = if tone.format == 4 && tone.params.len() > 3 {
                        (note.lookup_bank, tone.params[3] as i32 & 255)
                    } else {
                        (0, note.midi_key)
                    };
                    let tail = synth::fm_pitch_tail_for_context(bank_arg, key_arg, 1, note.host_transpose);
                    note.fm_pitch_tail_valid = true;
                    note.fm_pitch_tail_shift = (tail >> 16) & 7;
                    note.fm_pitch_tail_mantissa = tail & 1023;
                }
            }
        }
    }

    fn collect_event(&mut self, event: &EventInfo) {
        let track = safe_track(event.track_id);
        let channel = safe_channel(event.channel);
        match event.kind {
            2 => {
                self.apply_control(event, track, channel);
                self.record_control(event.tick, track, channel, event.control);
            }
            6 => {
                let state = &mut self.channels[track as usize][channel as usize];
                if event.control == 11 {
                    state.expression = clamp7(event.value);
                    self.record_control(event.tick, track, channel, event.control);
                } else if event.control == 129 {
                    state.modulation = event.value.min(4);
                    self.record_control(event.tick, track, channel, event.control);
                }
            }
            3 => self.apply_sysex(event, track),
            1 => self.collect_note(event, track, channel),
            _ => {}
        }
    }

    fn collect_note(&mut self, event: &EventInfo, track: i32, channel: i32) {
        if event.key_is_midi && event.value == 0 {
            return;
        }
        let state = self.channels[track as usize][channel as usize].clone();
        // The recorded-sample note bank (bankMsb == 125) plays a decoded PCM
        // sample rather than an FM voice: emit a streamed-audio event and stop.
        // A key with no matching sample falls through to the FM note path.
        if event.key_is_midi && state.bank_msb == 125 {
            if let Some(sample) = Self::find_audio_sample_exact(&self.decoded_audio_samples, track, event.key & 127) {
                let velocity = if event.value != 0 { clamp7(event.value) } else { 127 };
                let key7 = event.key & 127;
                let pan = if (key7 as usize) < self.pcm_sample_pan.len() && self.pcm_sample_pan[key7 as usize] >= 0 {
                    self.pcm_sample_pan[key7 as usize]
                } else {
                    state.pan
                };
                let softened = self.attenuation_master_volume == 45;
                self.audio_events.push(AudioEvent {
                    order: self.order,
                    start_tick: event.tick,
                    duration_tick: if event.gate != 0 { event.gate } else { 1 },
                    audio_id: track,
                    sample_id: key7,
                    velocity,
                    pan,
                    sample,
                    track,
                    channel,
                    volume: state.volume,
                    expression: state.expression,
                    builtin_wave: false,
                    wave_pitch_ratio_q16: 65536,
                    attenuation_master_volume: self.attenuation_master_volume,
                    attenuation_velocity_ctrl_table: self.attenuation_velocity_ctrl_table,
                    pcm_stream: true,
                    pcm_master_softened: softened,
                    wave_record: Vec::new(),
                });
                self.order += 1;
                return;
            }
        }
        let mut note = NoteEvent {
            order: self.order,
            start_tick: event.tick,
            duration_tick: if event.gate != 0 { event.gate } else { 2 },
            track,
            channel,
            ..Default::default()
        };
        self.order += 1;
        note.midi_key = if event.key_is_midi { event.key } else { self.key6_to_midi(event.key) };
        note.key6 = if event.key_is_midi {
            self.midi_to_key6(note.midi_key)
        } else {
            event.key
        };
        note.velocity = if event.key_is_midi { 127 & event.value } else { 127 };
        note.combined_velocity = combined_gain7(state.volume, state.expression).max(1);
        note.pan = state.pan;
        note.volume = state.volume;
        note.expression = state.expression;
        note.modulation = state.modulation;
        note.bend14 = state.bend14;
        note.bend_range = state.bend_range;
        note.sustain = state.sustain;
        note.mono_mode = state.mono_mode;
        note.bank_msb = state.bank_msb;
        note.bank_lsb = state.bank_lsb;
        note.lookup_bank = state.lookup_bank;
        note.program = state.program;
        note.host_transpose = self.host_transpose;
        note.fm_reset_env = self.fm_reset_env;
        note.fm_fixed_slot_mode = self.fm_fixed_slot_mode;
        note.fm_fixed_slot = if event.compact {
            let base = if track > 0 { track - 1 } else { 0 };
            (base * 4 + (channel & 3)).max(0)
        } else {
            channel
        };
        note.attenuation_master_volume = self.attenuation_master_volume;
        note.attenuation_velocity_ctrl_table = self.attenuation_velocity_ctrl_table;

        let lookup = self.find_tone(track, channel, &state, event.compact, event.key);
        note.tone = lookup.tone.clone();
        self.copy_tone_metadata(&mut note, &lookup);
        self.set_render_pitch(&mut note);
        let q16 = self.wave_pitch_ratio_q16(&note, &lookup);

        // A built-in wave-rhythm key or a direct type-2 wave tone plays a
        // recording, not an FM voice: emit an audio event and stop. Anything
        // else is an FM note.
        if lookup.builtin_wave_key >= 0 && self.add_builtin_wave_rhythm_event(event, track, channel, &state, &note, lookup.builtin_wave_key, q16) {
            return;
        }
        if lookup.tone.as_ref().map(|t| t.tone_type) == Some(2)
            && self.add_direct_type2_wave_event(event, track, channel, &state, &note, &lookup, q16)
        {
            return;
        }
        self.notes.push(note);
    }

    fn collect(mut self) -> Analysis {
        self.apply_setup_attenuation_mode(2);
        if let Some(first) = self.smaf.tracks.first() {
            match first.format_type {
                0 | 4 => self.apply_setup_attenuation_mode(1),
                2 => self.apply_setup_attenuation_mode(0),
                _ => {}
            }
        }
        for track in &self.smaf.tracks {
            for entry in &track.tones {
                if entry.decoded_tone.is_some() || entry.format == 5 {
                    self.tones.push(ToneDef::from_tone_entry(entry));
                }
            }
        }
        let tones = self.tones.clone();
        self.registry.build_from_tones(&tones);
        self.decode_all_audio_samples();
        for i in 0..self.smaf.tracks.len() {
            self.apply_initial_channel_status(i);
            let track_no = safe_track(self.smaf.tracks[i].id);
            for ch in 0..16 {
                self.record_control(0, track_no, ch, -1);
            }
        }
        for i in 0..self.smaf.tracks.len() {
            let events = self.smaf.tracks[i].sequence.events.clone();
            for event in &events {
                self.collect_event(event);
            }
        }
        self.notes.sort_by(|a, b| a.start_tick.cmp(&b.start_tick).then(a.order.cmp(&b.order)));
        self.controls.sort_by(|a, b| a.tick.cmp(&b.tick).then(a.order.cmp(&b.order)));
        self.collect_audio_events();
        self.audio_events
            .sort_by(|a, b| a.start_tick.cmp(&b.start_tick).then(a.order.cmp(&b.order)));
        Analysis {
            notes: self.notes,
            audio_events: self.audio_events,
            controls: self.controls,
            total_ticks: self.smaf.total_ticks,
        }
    }

    /// `collectAudioEvents` - the recorded-audio track events. Each audio track
    /// keeps its own channel state and every note-on that names a decoded sample
    /// becomes a streamed-audio event.
    fn collect_audio_events(&mut self) {
        if self.decoded_audio_samples.is_empty() {
            return;
        }
        for audio_index in 0..self.smaf.audios.len() {
            let audio_id = self.smaf.audios[audio_index].id;
            let events = self.smaf.audios[audio_index].sequence.events.clone();
            let mut channels: Vec<ChannelState> = (0..16).map(ChannelState::new).collect();
            for event in &events {
                let ch = safe_channel(event.channel);
                if event.kind == 2 {
                    self.apply_control_state(&mut channels[ch as usize], event);
                } else if event.kind == 1 {
                    let key = if event.compact && !event.raw.is_empty() {
                        event.raw[0] as i32 & 63
                    } else {
                        event.key & 0xFF
                    };
                    let Some(sample) = Self::find_audio_sample_exact(&self.decoded_audio_samples, audio_id, key) else {
                        continue;
                    };
                    let state = &channels[ch as usize];
                    let velocity = if event.value != 0 { clamp7(event.value) } else { 127 };
                    self.audio_events.push(AudioEvent {
                        order: self.order,
                        start_tick: event.tick,
                        duration_tick: if event.gate != 0 { event.gate } else { 1 },
                        audio_id,
                        sample_id: key,
                        velocity,
                        pan: state.pan,
                        sample,
                        track: audio_id,
                        channel: ch,
                        volume: state.volume,
                        expression: state.expression,
                        builtin_wave: false,
                        wave_pitch_ratio_q16: 65536,
                        attenuation_master_volume: state.volume,
                        attenuation_velocity_ctrl_table: false,
                        pcm_stream: true,
                        pcm_master_softened: false,
                        wave_record: Vec::new(),
                    });
                    self.order += 1;
                }
            }
        }
    }
}

/// One recording to play back, as the reference's `AudioEvent`. `order` is the
/// analysis order for the tie-break in the sort.
#[derive(Clone)]
pub struct AudioEvent {
    pub order: i32,
    pub start_tick: i32,
    pub duration_tick: i32,
    pub audio_id: i32,
    pub sample_id: i32,
    pub velocity: i32,
    pub pan: i32,
    pub sample: DecodedAudioSample,
    pub track: i32,
    pub channel: i32,
    pub volume: i32,
    pub expression: i32,
    pub builtin_wave: bool,
    pub wave_pitch_ratio_q16: i32,
    pub attenuation_master_volume: i32,
    pub attenuation_velocity_ctrl_table: bool,
    pub pcm_stream: bool,
    pub pcm_master_softened: bool,
    pub wave_record: Vec<u8>,
}

/// The notes and recordings a file resolves to.
pub struct Analysis {
    pub notes: Vec<NoteEvent>,
    pub audio_events: Vec<AudioEvent>,
    pub controls: Vec<ControlEvent>,
    pub total_ticks: i32,
}

/// Analyze a parsed file into the notes and recordings it plays. `key_base` is
/// the reference's default of 24.
pub fn analyze(smaf: &Smaf) -> Analysis {
    Collector::new(smaf, 24).collect()
}

/// `ticksToFrames` - a tick is four milliseconds.
pub fn ticks_to_frames(ticks: i32, rate: i32) -> i32 {
    if ticks <= 0 {
        return 0;
    }
    (((ticks as i64) * 4 * rate as i64 + 999) / 1000).min(2_147_483_647) as i32
}

/// How many stereo frames [`render`] produces for an analysis, sizing the
/// buffer exactly as it does. Computing this is cheap - it never runs the synth
/// - so the play path can learn a clip's length without paying for the render.
pub fn rendered_frame_count(analysis: &Analysis, total_ticks: i32, rate: i32) -> i32 {
    let rate = if rate <= 0 { 48000 } else { rate };
    let mut max_tick = total_ticks.max(0);
    for note in &analysis.notes {
        max_tick = max_tick.max(note.start_tick + note.duration_tick.max(1));
    }
    for event in &analysis.audio_events {
        max_tick = max_tick.max(event.start_tick + event.duration_tick.max(1));
    }
    let frames = ticks_to_frames(max_tick, rate) + rate;
    let mut total = if frames <= 0 { rate } else { frames };
    // A recording can run past the tick-derived length; make room for its tail.
    for event in &analysis.audio_events {
        let end =
            ticks_to_frames(event.start_tick, rate) + resampled_frame_count(event.sample.pcm_mono.len(), event.sample.sample_rate, rate) + rate / 10;
        total = total.max(end);
    }
    total
}

/// `render` - mix every FM note and recording into a stereo float buffer, as
/// the reference's top-level `OracleMa3Synth.render`.
pub fn render(analysis: &Analysis, total_ticks: i32, rate: i32) -> Vec<f32> {
    let rate = if rate <= 0 { 48000 } else { rate };
    let notes = &analysis.notes;
    let total = rendered_frame_count(analysis, total_ticks, rate);
    let mut buffer = vec![0f32; (total * 2) as usize];
    for note in notes {
        if let Some(tone) = &note.tone {
            if tone.compact.as_ref().map(|c| c.valid) == Some(true) {
                let start = ticks_to_frames(note.start_tick, rate);
                let duration = ticks_to_frames(note.duration_tick, rate).max(1);
                let velocity = (note.velocity * note.combined_velocity.max(1) / 127).max(1);
                synth::render_note(
                    &mut buffer,
                    total,
                    start,
                    duration,
                    tone.compact.as_ref().unwrap(),
                    note.render_midi_key,
                    velocity,
                    note.pan,
                    note.bend14,
                    note.bend_range,
                    note.modulation,
                    rate,
                );
            }
        }
    }
    for event in &analysis.audio_events {
        let start = ticks_to_frames(event.start_tick, rate);
        mix_audio_event(&mut buffer, total, start, &event.sample, event.pan, event.velocity, rate);
    }
    buffer
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

    /// The same per-note fingerprint the oracle's `DumpNotes` emits.
    fn note_fingerprint(n: &NoteEvent) -> String {
        let dv = match n.tone.as_ref().and_then(|t| t.compact.as_ref()) {
            Some(c) => hex(&c.dll_voice),
            None => "-".to_string(),
        };
        let vel = (n.velocity * n.combined_velocity.max(1) / 127).max(1);
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            n.start_tick,
            n.duration_tick,
            n.track,
            n.channel,
            n.render_midi_key,
            n.render_key6,
            vel,
            n.velocity,
            n.combined_velocity,
            n.pan,
            n.bend14,
            n.bend_range,
            n.modulation,
            n.bank_msb,
            n.bank_lsb,
            n.lookup_bank,
            n.program,
            n.registry_slot,
            n.tone_addr,
            n.tone_size,
            n.tone_type,
            n.fm_fixed_slot,
            n.fm_reset_env as i32,
            n.fm_fixed_slot_mode as i32,
            dv,
        )
    }

    /// Analyze every MMF and check the FM note stream against the reference
    /// `OracleMmfAnalysis`. Gated on `OMA3_NOTES_DUMP` (a `DumpNotes` capture).
    #[test]
    fn notes_match_the_reference() {
        let dump_path = match std::env::var("OMA3_NOTES_DUMP") {
            Ok(p) => p,
            Err(_) => return,
        };
        let dump = std::fs::read_to_string(&dump_path).unwrap();
        let mut files = 0;
        let mut fm_files = 0;
        let mut notes = 0;
        for line in dump.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols[0] != "N" {
                continue;
            }
            let path = cols[1];
            let want: Vec<&str> = cols[4..].to_vec();
            let data = std::fs::read(path).unwrap();
            let smaf = super::super::smaf::parse(&data).unwrap_or_else(|e| panic!("{path}: parse: {}", e.0));
            let got: Vec<String> = super::analyze(&smaf).notes.iter().map(note_fingerprint).collect();
            assert_eq!(got.len(), want.len(), "{path}: note count (got {}, want {})", got.len(), want.len());
            for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                if g != *w {
                    let diff = g.chars().zip(w.chars()).position(|(a, b)| a != b).unwrap_or(0);
                    let from = diff.saturating_sub(15);
                    panic!(
                        "{path} note {i} mismatch at {diff}:\n got ...{}\nwant ...{}",
                        &g[from..(from + 60).min(g.len())],
                        &w[from..(from + 60).min(w.len())],
                    );
                }
            }
            files += 1;
            if !got.is_empty() {
                fm_files += 1;
            }
            notes += got.len();
        }
        eprintln!("verified {files} files ({fm_files} with FM notes, {notes} notes total)");
        assert!(files >= 40);
    }
}

#[cfg(test)]
mod wav_tests {
    fn to_i16(pcm: &[f32]) -> Vec<i16> {
        // The same float-to-i16 conversion the reference's WAV writer uses.
        pcm.iter().map(|&s| (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16).collect()
    }

    /// Render the committed compact-sequence fixture end to end and check the
    /// PCM where its first note sounds against samples captured from the
    /// reference renderer. The whole file matches sample for sample; a window
    /// keeps the fixture small.
    #[test]
    fn renders_fixture_pcm_like_the_reference() {
        let data = include_bytes!("data/seq/compact.mmf");
        let smaf = super::super::smaf::parse(data).unwrap();
        let analysis = super::analyze(&smaf);
        let pcm = super::render(&analysis, smaf.total_ticks, 44100);
        let got = to_i16(&pcm);

        // Reference samples from frame 1236 (where the first chord sounds).
        let start = 2472;
        let want: [i16; 24] = [
            4721, 4721, 9514, 9514, 17346, 17346, 22442, 22442, 30151, 30151, 31755, 31755, 32767, 32767, 32767, 32767, 30842, 30842, 31079, 31079,
            26975, 26975, 18104, 18104,
        ];
        for (i, &w) in want.iter().enumerate() {
            let g = got[start + i];
            assert!((g as i32 - w as i32).abs() <= 1, "sample {}: got {g}, want {w}", start + i);
        }
    }

    /// Render a pure-FM MMF end to end and check every PCM sample against the
    /// reference `RefRender` output. Gated on `OMA3_WAV_MMF` and `OMA3_WAV_REF`
    /// so it only runs when the oracle output is present.
    #[test]
    fn renders_pcm_like_the_reference() {
        let (mmf_path, ref_path) = match (std::env::var("OMA3_WAV_MMF"), std::env::var("OMA3_WAV_REF")) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return,
        };
        let data = std::fs::read(&mmf_path).unwrap();
        let smaf = super::super::smaf::parse(&data).unwrap();
        let analysis = super::analyze(&smaf);
        let got = to_i16(&super::render(&analysis, smaf.total_ticks, 44100));

        let wav = std::fs::read(&ref_path).unwrap();
        let ref_samples: Vec<i16> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();

        assert_eq!(got.len(), ref_samples.len(), "sample count");
        let mut max_diff = 0i32;
        for (g, r) in got.iter().zip(ref_samples.iter()) {
            max_diff = max_diff.max((*g as i32 - *r as i32).abs());
        }
        assert!(max_diff <= 1, "max sample diff {max_diff}");
        eprintln!("matched {} samples, max diff {max_diff}", got.len());
    }

    /// The full audio-event list a file resolves to - built-in rhythm, direct
    /// type-2 wave, streamed PCM notes and audio-track events - must match the
    /// reference field for field, in order, with the same decoded sample.
    /// Gated on `OMA3_AUDIO_EVENTS_DUMP` (a `DumpAudioEvents` capture).
    #[test]
    fn audio_events_match_the_reference() {
        let dump_path = match std::env::var("OMA3_AUDIO_EVENTS_DUMP") {
            Ok(p) => p,
            Err(_) => return,
        };
        let pcm_fp = |pcm: &[i16]| -> i64 {
            let mut h: i64 = 0;
            for &x in pcm {
                h = h.wrapping_mul(31).wrapping_add(x as u16 as i64);
            }
            h & 0xFFFF_FFFF
        };
        let dump = std::fs::read_to_string(&dump_path).unwrap();
        let mut files = 0;
        let mut events = 0;
        for line in dump.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let path = cols[0];
            let want: Vec<&str> = cols[1..].to_vec();
            let data = std::fs::read(path).unwrap();
            let smaf = super::super::smaf::parse(&data).unwrap();
            let got: Vec<String> = super::analyze(&smaf)
                .audio_events
                .iter()
                .map(|e| {
                    format!(
                        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
                        e.start_tick,
                        e.duration_tick,
                        e.audio_id,
                        e.sample_id,
                        e.velocity,
                        e.pan,
                        e.builtin_wave as i32,
                        e.wave_pitch_ratio_q16,
                        e.attenuation_master_volume,
                        e.attenuation_velocity_ctrl_table as i32,
                        e.pcm_stream as i32,
                        e.pcm_master_softened as i32,
                        e.volume,
                        e.expression,
                        e.track,
                        e.channel,
                        e.sample.audio_id,
                        e.sample.sample_id,
                        e.sample.sample_rate,
                        e.sample.pcm_mono.len(),
                        pcm_fp(&e.sample.pcm_mono),
                    )
                })
                .collect();
            assert_eq!(got.len(), want.len(), "{path}: audio event count");
            for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                assert_eq!(g, *w, "{path} event {i}");
            }
            files += 1;
            events += got.len();
        }
        eprintln!("verified {files} files, {events} audio events");
        assert!(files >= 30);
    }
}
