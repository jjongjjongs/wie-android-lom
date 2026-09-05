//! `OracleMmfRenderer` - the reference LGT player's real-time streaming MA-3
//! renderer. Unlike the batch [`super::analysis::render`], which renders each
//! note in isolation, this drives a bank of 32 FM voice slots and 8 wave slots
//! from the analysis's note/control/audio event streams in tick order, applying
//! mid-note control changes, voice stealing and mono handling exactly as the
//! reference player does. Ported verbatim.

use super::analysis::{Analysis, AudioEvent, ControlEvent, NoteEvent, ToneDef, ticks_to_frames};
use super::audio::{DecodedAudioSample, resampled_frame_count};
use super::bus;
use super::synth::{StreamingVoice, VoiceRuntime, release_frames};
use super::wave::WaveRuntime;

const MAX_POLYPHONY: usize = 32;
const WAVE_SLOTS: usize = 8;
const MAX_CHANNELS: usize = 16;
const MAX_TRACKS: usize = 16;

fn safe_track(v: i32) -> i32 {
    if v < 0 { 0 } else { v.min(15) }
}

fn safe_channel(v: i32) -> i32 {
    if v < 0 { 0 } else { v.min(15) }
}

fn clamp_i(v: i32, lo: i32, hi: i32) -> i32 {
    if v >= lo { v.min(hi) } else { lo }
}

fn clamp_d(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo { lo } else { v.min(hi) }
}

fn controller_gain(a: i32, b: i32) -> f64 {
    (clamp_i(a, 0, 127) * clamp_i(b, 0, 127)) as f64 / 16129.0
}

// ---- StreamingVoice factories (reference `OracleMa3Synth.create*`) ----

fn create_streaming_voice(note: &NoteEvent, rate: i32, dur_tick: i32, fixed127: bool) -> Option<StreamingVoice> {
    let tone = note.tone.as_ref()?;
    let compact = tone.compact.as_ref()?;
    if !compact.valid {
        return None;
    }
    let r = if rate <= 0 { 48000 } else { rate };
    let gate_frames = ticks_to_frames(dur_tick, r).max(1);
    let velocity = if fixed127 {
        127
    } else {
        (note.velocity * note.combined_velocity.max(1) / 127).max(1)
    };
    let mut voice = VoiceRuntime::new(
        compact,
        note.render_midi_key,
        velocity,
        note.pan,
        note.bend14,
        note.bend_range,
        note.modulation,
        r,
    );
    if note.fm_pitch_tail_valid {
        voice.set_pitch_tail(note.fm_pitch_tail_shift, note.fm_pitch_tail_mantissa);
    }
    let mut total = release_frames(voice.max_release_sec(), r) + gate_frames;
    let floor = r / 100;
    if total < floor {
        total = floor;
    }
    Some(StreamingVoice::new(voice, gate_frames, total))
}

fn create_runtime_streaming_voice(note: &NoteEvent, rate: i32, dur_tick: i32) -> Option<StreamingVoice> {
    create_streaming_voice(note, rate, dur_tick, true)
}

fn create_streaming_voice_default(note: &NoteEvent, rate: i32) -> Option<StreamingVoice> {
    create_streaming_voice(note, rate, note.duration_tick, false)
}

#[allow(clippy::too_many_arguments)]
fn create_fixed_streaming_voice(
    tone: &ToneDef,
    midi_key: i32,
    _key6: i32,
    velocity: i32,
    pan: i32,
    bend14: i32,
    bend_range: i32,
    modulation: i32,
    rate: i32,
    tail_shift: i32,
    tail_mantissa: i32,
) -> Option<StreamingVoice> {
    let compact = tone.compact.as_ref()?;
    if !compact.valid {
        return None;
    }
    let r = if rate <= 0 { 48000 } else { rate };
    let mut voice = VoiceRuntime::new(compact, midi_key, velocity, pan, bend14, bend_range, modulation, r);
    voice.set_pitch_tail(tail_shift, tail_mantissa);
    let total = 536870911.max(release_frames(voice.max_release_sec(), r));
    Some(StreamingVoice::new(voice, 536870911, total))
}

// ---- Slot / audio state ----

struct RuntimeVoiceSlot {
    active: bool,
    age: i64,
    bend14: i32,
    bend_range: i32,
    channel: i32,
    duplicate_count: i32,
    expression: i32,
    fixed: bool,
    gate_frames: i32,
    generation: i64,
    host_active: bool,
    index: i32,
    left_gain_q15: i32,
    linked_next: i32,
    master_volume: i32,
    midi_key: i32,
    modulation: i32,
    mono: bool,
    note: Option<NoteEvent>,
    pan: i32,
    render_key6: i32,
    render_midi_key: i32,
    right_gain_q15: i32,
    sustain: i32,
    synth_key6: i32,
    synth_midi_key: i32,
    track: i32,
    velocity: i32,
    velocity_uses_ctrl_table: bool,
    voice: Option<StreamingVoice>,
    volume: i32,
}

impl RuntimeVoiceSlot {
    fn new(index: i32) -> Self {
        RuntimeVoiceSlot {
            active: false,
            age: 0,
            bend14: 8192,
            bend_range: 2,
            channel: 0,
            duplicate_count: 0,
            expression: 127,
            fixed: false,
            gate_frames: 0,
            generation: 0,
            host_active: false,
            index,
            left_gain_q15: 0,
            linked_next: -2,
            master_volume: 76,
            midi_key: 0,
            modulation: 0,
            mono: false,
            note: None,
            pan: 64,
            render_key6: 0,
            render_midi_key: 0,
            right_gain_q15: 0,
            sustain: 0,
            synth_key6: 0,
            synth_midi_key: 0,
            track: 0,
            velocity: 127,
            velocity_uses_ctrl_table: false,
            voice: None,
            volume: 100,
        }
    }

    fn clear(&mut self) {
        self.active = false;
        self.host_active = false;
        self.fixed = false;
        self.mono = false;
        self.note = None;
        self.voice = None;
        self.gate_frames = 0;
        self.left_gain_q15 = 0;
        self.right_gain_q15 = 0;
        self.linked_next = -2;
        self.duplicate_count = 0;
    }

    fn force_off(&mut self) {
        if let Some(voice) = self.voice.as_mut() {
            voice.all_sound_off();
        }
        self.clear();
    }

    fn release(&mut self) {
        if self.active && self.host_active {
            self.host_active = false;
            if let Some(voice) = self.voice.as_mut() {
                voice.release_now();
            }
        }
    }
}

struct MonoOff {
    frame: i32,
    slot: i32,
    generation: i64,
}

struct ActiveAudio {
    attenuation_master_volume: i32,
    attenuation_velocity_ctrl_table: bool,
    channel: i32,
    expression: i32,
    pan: i32,
    pcm_master_softened: bool,
    pcm_stream: bool,
    position_frames: i32,
    start_frame: i32,
    stereo: Vec<i16>,
    stop_frame: i32,
    track: i32,
    velocity: i32,
    volume: i32,
}

impl ActiveAudio {
    fn new(start_frame: i32, stop_frame: i32, event: &AudioEvent, stereo: Vec<i16>) -> Self {
        ActiveAudio {
            attenuation_master_volume: event.attenuation_master_volume,
            attenuation_velocity_ctrl_table: event.attenuation_velocity_ctrl_table,
            channel: event.channel,
            expression: event.expression,
            pan: event.pan,
            pcm_master_softened: event.pcm_master_softened,
            pcm_stream: event.pcm_stream,
            position_frames: 0,
            start_frame,
            stereo,
            stop_frame,
            track: event.track,
            velocity: event.velocity,
            volume: event.volume,
        }
    }

    fn is_finished(&self) -> bool {
        !(self.position_frames < self.stereo.len() as i32 / 2 && self.start_frame + self.position_frames < self.stop_frame)
    }

    fn stop_now(&mut self) {
        self.stop_frame = self.start_frame + self.position_frames;
    }

    fn render(&mut self, buf: &mut [f32], offset: i32, count: i32) {
        let available = if self.stop_frame > self.start_frame {
            self.stop_frame - self.start_frame
        } else {
            0
        };
        let total_frames = self.stereo.len() as i32 / 2;
        let frames = count.min((total_frames - self.position_frames).min((available - self.position_frames).max(0)));
        let mut src = (self.position_frames * 2) as usize;
        let mut dst = (offset * 2) as usize;
        let (gain, left, right) = if self.pcm_stream {
            (
                std::f64::consts::SQRT_2,
                bus::pcm_stream_left_gain(
                    self.attenuation_master_volume,
                    self.velocity,
                    self.attenuation_velocity_ctrl_table,
                    self.pcm_master_softened,
                    self.pan,
                ),
                bus::pcm_stream_right_gain(
                    self.attenuation_master_volume,
                    self.velocity,
                    self.attenuation_velocity_ctrl_table,
                    self.pcm_master_softened,
                    self.pan,
                ),
            )
        } else {
            let g = self.velocity.max(1) as f64 / 127.0 * controller_gain(self.volume, self.expression);
            let position = clamp_d((self.pan as f64 - 64.0) / 64.0, -1.0, 1.0);
            (g, ((1.0 - position) * 0.5).sqrt(), ((position + 1.0) * 0.5).sqrt())
        };
        let mut i = 0;
        while i < frames {
            let l = self.stereo[src] as f64 / 32768.0 * gain * left;
            buf[dst] += l as f32;
            let r = self.stereo[src + 1] as f64 / 32768.0 * gain * right;
            buf[dst + 1] += r as f32;
            src += 2;
            dst += 2;
            self.position_frames += 1;
            i += 1;
        }
    }
}

struct SlotPick {
    slot: usize,
    high_bit_steal: bool,
}

pub struct Renderer {
    analysis: Analysis,
    sample_rate: i32,
    frame_count: i32,
    position_frames: i32,
    event_frame: i32,
    note_cursor: usize,
    control_cursor: usize,
    audio_cursor: usize,
    age_counter: i64,
    generation_counter: i64,
    slots: Vec<RuntimeVoiceSlot>,
    wave_slots: Vec<WaveRuntime>,
    audios: Vec<ActiveAudio>,
    mono_offs: Vec<MonoOff>,
    note_heads: [[i32; MAX_CHANNELS]; MAX_TRACKS],
}

fn has_playable(analysis: &Analysis) -> bool {
    if !analysis.audio_events.is_empty() {
        return true;
    }
    analysis
        .notes
        .iter()
        .any(|n| n.tone.as_ref().and_then(|t| t.compact.as_ref()).map(|c| c.valid).unwrap_or(false))
}

fn compute_frame_count(analysis: &Analysis, rate: i32) -> i32 {
    if !has_playable(analysis) {
        return 0;
    }
    let mut frames = ticks_to_frames(analysis.total_ticks, rate);
    for note in &analysis.notes {
        if note.tone.as_ref().and_then(|t| t.compact.as_ref()).map(|c| c.valid).unwrap_or(false) {
            if let Some(voice) = create_streaming_voice_default(note, rate) {
                frames = frames.max(ticks_to_frames(note.start_tick, rate) + voice.total_frames());
            }
        }
    }
    for audio in &analysis.audio_events {
        let resampled = resampled_frame_count(audio.sample.pcm_mono.len(), audio.sample.sample_rate, rate);
        frames = frames.max(ticks_to_frames(audio.start_tick, rate) + resampled);
    }
    frames
}

impl Renderer {
    pub fn prepare(analysis: Analysis, rate: i32) -> Self {
        let sample_rate = if rate <= 0 { 48000 } else { rate };
        let frame_count = compute_frame_count(&analysis, sample_rate);
        let mut renderer = Renderer {
            analysis,
            sample_rate,
            frame_count,
            position_frames: 0,
            event_frame: 0,
            note_cursor: 0,
            control_cursor: 0,
            audio_cursor: 0,
            age_counter: 0,
            generation_counter: 0,
            slots: (0..MAX_POLYPHONY as i32).map(RuntimeVoiceSlot::new).collect(),
            wave_slots: (0..WAVE_SLOTS as i32).map(WaveRuntime::new).collect(),
            audios: Vec::new(),
            mono_offs: Vec::new(),
            note_heads: [[-1; MAX_CHANNELS]; MAX_TRACKS],
        };
        renderer.clear_note_heads();
        renderer
    }

    pub fn frame_count(&self) -> i32 {
        self.frame_count
    }

    fn clear_note_heads(&mut self) {
        for row in self.note_heads.iter_mut() {
            for cell in row.iter_mut() {
                *cell = -1;
            }
        }
    }

    fn ticks_to_frame(&self, ticks: i32) -> i32 {
        ticks_to_frames(ticks, self.sample_rate)
    }

    fn is_finished(&self) -> bool {
        if self.position_frames < self.frame_count {
            return false;
        }
        if self.slots.iter().any(|s| s.active) {
            return false;
        }
        if self.wave_slots.iter().any(|w| w.is_active()) {
            return false;
        }
        self.audios.is_empty()
    }

    pub fn render(&mut self, buf: &mut [f32], count: i32) -> i32 {
        self.render_mix(buf, 0, count, 1.0, 1.0)
    }

    pub fn render_mix(&mut self, buf: &mut [f32], offset: i32, count: i32, gain_l: f32, gain_r: f32) -> i32 {
        if offset < 0 || count < 0 || ((offset + count) * 2) as usize > buf.len() {
            return -1;
        }
        if count == 0 || self.is_finished() {
            return -1;
        }
        let frames = count.min((self.frame_count - self.position_frames).max(0));
        if frames <= 0 {
            return -1;
        }
        for s in &mut buf[(offset * 2) as usize..((offset + frames) * 2) as usize] {
            *s = 0.0;
        }
        let mut done = 0;
        while done < frames {
            let frame = self.position_frames + done;
            self.process_events_at(frame);
            let chunk = (self.next_event_frame(frame, self.position_frames + frames) - frame).max(1);
            let at = offset + done;
            self.render_slots(buf, at, frame, chunk);
            self.render_wave_slots(buf, at, frame, chunk);
            self.render_active_audio(buf, at, frame, chunk);
            done += chunk;
        }
        if gain_l != 1.0 || gain_r != 1.0 {
            apply_gain(buf, offset, frames, gain_l, gain_r);
        }
        self.position_frames += frames;
        frames
    }

    fn next_event_frame(&self, from: i32, limit: i32) -> i32 {
        let mut result = limit;
        if self.control_cursor < self.analysis.controls.len() {
            let f = self.ticks_to_frame(self.analysis.controls[self.control_cursor].tick);
            if f > from {
                result = result.min(f);
            }
        }
        if self.note_cursor < self.analysis.notes.len() {
            let f = self.ticks_to_frame(self.analysis.notes[self.note_cursor].start_tick);
            if f > from {
                result = result.min(f);
            }
        }
        if self.audio_cursor < self.analysis.audio_events.len() {
            let f = self.ticks_to_frame(self.analysis.audio_events[self.audio_cursor].start_tick);
            if f > from {
                result = result.min(f);
            }
        }
        result
    }

    fn process_events_at(&mut self, frame: i32) {
        self.event_frame = frame;
        while self.control_cursor < self.analysis.controls.len() && self.ticks_to_frame(self.analysis.controls[self.control_cursor].tick) <= frame {
            let control = self.analysis.controls[self.control_cursor].clone();
            self.control_cursor += 1;
            self.apply_control(&control);
        }
        while self.note_cursor < self.analysis.notes.len() && self.ticks_to_frame(self.analysis.notes[self.note_cursor].start_tick) <= frame {
            let note = self.analysis.notes[self.note_cursor].clone();
            self.note_cursor += 1;
            self.start_note(&note);
        }
        while self.audio_cursor < self.analysis.audio_events.len()
            && self.ticks_to_frame(self.analysis.audio_events[self.audio_cursor].start_tick) <= frame
        {
            let event = self.analysis.audio_events[self.audio_cursor].clone();
            self.audio_cursor += 1;
            self.activate_audio(&event, frame);
        }
    }

    // ---- control application ----

    fn apply_control(&mut self, control: &ControlEvent) {
        if control.fm_fixed_cmd != 0 {
            self.apply_fixed_slot_control(control);
        }
        for i in 0..self.slots.len() {
            if self.slots[i].active && self.slots[i].track == control.track && self.slots[i].channel == control.channel {
                let had_sustain = self.slots[i].sustain != 0;
                {
                    let slot = &mut self.slots[i];
                    slot.volume = control.volume;
                    slot.expression = control.expression;
                    slot.pan = control.pan;
                    slot.master_volume = control.attenuation_master_volume;
                    slot.velocity_uses_ctrl_table = control.attenuation_velocity_ctrl_table;
                    slot.sustain = control.sustain;
                    slot.mono = control.mono_mode != 0;
                }
                self.update_slot_gains(i);
                if self.slots[i].modulation != control.modulation {
                    self.slots[i].modulation = control.modulation;
                    let m = self.slots[i].modulation;
                    if let Some(voice) = self.slots[i].voice.as_mut() {
                        voice.set_modulation(m);
                    }
                }
                if self.slots[i].bend14 != control.bend14 || self.slots[i].bend_range != control.bend_range {
                    self.slots[i].bend14 = control.bend14;
                    self.slots[i].bend_range = control.bend_range;
                    self.update_voice_pitch(i);
                }
                if control.changed_control == 120 {
                    self.force_off_slot(i);
                } else if control.changed_control == 123 || (had_sustain && control.sustain == 0 && !self.slots[i].host_active) {
                    self.release_slot(i);
                }
            }
        }
        for audio in &mut self.audios {
            if audio.track == control.track && audio.channel == control.channel {
                audio.volume = control.volume;
                audio.expression = control.expression;
                audio.pan = control.pan;
                if control.changed_control == 120 || control.changed_control == 123 {
                    audio.stop_now();
                }
            }
        }
        for w in 0..self.wave_slots.len() {
            self.wave_slots[w].apply_control(control);
        }
    }

    fn apply_fixed_slot_control(&mut self, control: &ControlEvent) {
        let slot_idx = control.fm_fixed_slot;
        if slot_idx < 0 || slot_idx as usize >= self.slots.len() {
            return;
        }
        let i = slot_idx as usize;
        let needs_new = (!self.slots[i].active || !self.slots[i].host_active)
            && control.fm_fixed_cmd == 6
            && control.fm_fixed_tone.as_ref().map(|t| t.compact.is_some()).unwrap_or(false);
        if needs_new {
            let tone = control.fm_fixed_tone.as_ref().unwrap();
            let voice = create_fixed_streaming_voice(
                tone,
                control.fm_fixed_synth_midi_key,
                control.fm_fixed_synth_key6,
                127,
                control.fm_fixed_pan,
                control.bend14,
                control.bend_range,
                control.modulation,
                self.sample_rate,
                (control.fm_fixed_pitch_raw as u32 >> 10) as i32,
                control.fm_fixed_pitch_raw & 1023,
            );
            let Some(voice) = voice else {
                return;
            };
            {
                let slot = &mut self.slots[i];
                slot.clear();
                slot.active = true;
                slot.host_active = true;
                slot.fixed = true;
                slot.track = control.track;
                slot.channel = slot_idx;
                slot.voice = Some(voice);
                slot.volume = control.volume;
                slot.expression = control.expression;
                slot.pan = control.pan;
                slot.master_volume = control.attenuation_master_volume;
                slot.velocity_uses_ctrl_table = control.attenuation_velocity_ctrl_table;
                slot.modulation = control.modulation;
                slot.bend14 = control.bend14;
                slot.bend_range = control.bend_range;
                slot.sustain = control.sustain;
                slot.gate_frames = 536870911;
                slot.velocity = control.fm_fixed_velocity.max(1);
            }
            self.age_counter += 1;
            self.slots[i].age = self.age_counter;
            self.update_slot_gains(i);
            return;
        }
        if self.slots[i].active && self.slots[i].host_active {
            let shift = (control.fm_fixed_pitch_raw as u32 >> 10) as i32 & 7;
            let mantissa = control.fm_fixed_pitch_raw & 1023;
            if let Some(voice) = self.slots[i].voice.as_mut() {
                voice.set_pitch_tail(shift, mantissa);
            }
            if control.fm_fixed_cmd == 5 {
                self.release_slot(i);
            } else if control.fm_fixed_cmd == 6 {
                if control.fm_fixed_velocity != 0 {
                    self.slots[i].velocity = control.fm_fixed_velocity;
                }
                self.age_counter += 1;
                self.slots[i].age = self.age_counter;
                self.update_slot_gains(i);
            }
        }
    }

    fn apply_mono_offs(&mut self, frame: i32) {
        while !self.mono_offs.is_empty() && self.mono_offs[0].frame <= frame {
            let mono = self.mono_offs.remove(0);
            if mono.slot < 0 {
                continue;
            }
            let i = mono.slot as usize;
            if i >= self.slots.len() {
                continue;
            }
            if self.slots[i].active && self.slots[i].host_active && self.slots[i].mono && self.slots[i].generation == mono.generation {
                if self.slots[i].duplicate_count > 0 {
                    self.slots[i].duplicate_count -= 1;
                }
                if self.slots[i].duplicate_count == 0 {
                    self.release_slot(i);
                }
            }
        }
    }

    // ---- rendering ----

    fn render_slots(&mut self, buf: &mut [f32], offset: i32, base_frame: i32, count: i32) {
        let mut base = (offset * 2) as usize;
        for f in 0..count {
            self.apply_mono_offs(base_frame + f);
            let mut left = 0;
            let mut right = 0;
            for i in 0..self.slots.len() {
                if !self.slots[i].active || self.slots[i].voice.is_none() {
                    continue;
                }
                let pos = self.slots[i].voice.as_ref().unwrap().position_frames();
                if self.slots[i].host_active && self.slots[i].sustain != 0 && pos >= self.slots[i].gate_frames {
                    let g = pos + 1;
                    self.slots[i].voice.as_mut().unwrap().set_gate_frames(g);
                } else if self.slots[i].host_active && pos >= self.slots[i].gate_frames {
                    self.release_slot(i);
                }
                let sample = self.slots[i].voice.as_mut().unwrap().render_sample();
                left = bus::mix_fm_q15(left, sample, self.slots[i].left_gain_q15);
                right = bus::mix_fm_q15(right, sample, self.slots[i].right_gain_q15);
                let finished = self.slots[i].voice.as_ref().unwrap().is_finished();
                if !finished && (self.slots[i].host_active || self.slots[i].voice.as_ref().unwrap().is_audible()) {
                    continue;
                }
                self.unlink_slot(i);
                self.slots[i].clear();
            }
            buf[base] = bus::add_output_i16(buf[base], left);
            buf[base + 1] = bus::add_output_i16(buf[base + 1], right);
            base += 2;
        }
    }

    fn render_wave_slots(&mut self, buf: &mut [f32], offset: i32, base_frame: i32, count: i32) {
        for w in 0..self.wave_slots.len() {
            self.wave_slots[w].render(buf, offset, base_frame, count);
        }
    }

    fn render_active_audio(&mut self, buf: &mut [f32], offset: i32, base_frame: i32, count: i32) {
        let mut i = 0;
        while i < self.audios.len() {
            let start = (self.audios[i].start_frame - base_frame).max(0);
            if start < count {
                self.audios[i].render(buf, offset + start, count - start);
            }
            if self.audios[i].is_finished() {
                self.audios.remove(i);
            } else {
                i += 1;
            }
        }
    }

    // ---- note start / slot management ----

    fn clip_duration_at_control(&self, note: &NoteEvent) -> i32 {
        let note_end = note.start_tick + note.duration_tick.max(1);
        let mut limit = note_end;
        if note.sustain != 0 {
            limit = if self.analysis.total_ticks > note_end {
                self.analysis.total_ticks
            } else {
                note_end
            };
            for control in &self.analysis.controls {
                if control.tick >= note_end
                    && control.track == note.track
                    && control.channel == note.channel
                    && control.changed_control == 64
                    && control.sustain == 0
                {
                    limit = control.tick;
                    break;
                }
            }
        }
        for control in &self.analysis.controls {
            if control.tick > note.start_tick
                && control.tick < limit
                && control.track == note.track
                && control.channel == note.channel
                && (control.changed_control == 120 || control.changed_control == 123)
            {
                return (control.tick - note.start_tick).max(1);
            }
        }
        (limit - note.start_tick).max(1)
    }

    fn note_mono_slot(&self, note: &NoteEvent) -> Option<usize> {
        let head = self.note_heads[safe_track(note.track) as usize][safe_channel(note.channel) as usize];
        if head < 0 || head as usize >= self.slots.len() {
            return None;
        }
        let i = head as usize;
        if self.slots[i].active && self.slots[i].host_active && self.slots[i].mono {
            Some(i)
        } else {
            None
        }
    }

    fn start_note(&mut self, note: &NoteEvent) {
        let valid = note.tone.as_ref().and_then(|t| t.compact.as_ref()).map(|c| c.valid).unwrap_or(false);
        if !valid {
            return;
        }
        let frames = (self.sample_rate / 100).max(ticks_to_frames(self.clip_duration_at_control(note), self.sample_rate));
        if note.fm_fixed_slot_mode && note.tone_type != 2 {
            let slot_idx = note.fm_fixed_slot;
            if (slot_idx as usize) < self.slots.len() {
                let i = note.fm_fixed_slot.max(0) as usize;
                if self.slots[i].active && self.slots[i].host_active {
                    self.update_slot_from_note(i, note, frames, false);
                } else {
                    self.start_slot(i, note, frames, true, 1);
                }
                return;
            }
        }
        let mono_slot = if note.mono_mode != 0 { self.note_mono_slot(note) } else { None };
        if let Some(i) = mono_slot {
            self.update_slot_from_note(i, note, frames, false);
            self.slots[i].mono = true;
            let target = self.event_frame + frames;
            self.schedule_mono_off(i, target);
            return;
        }
        let pick = self.find_slot();
        let i = pick.slot;
        if self.slots[i].active {
            self.prepare_slot_for_reuse(i);
        }
        let key_state = if pick.high_bit_steal { 2 } else { 1 };
        self.start_slot(i, note, frames, false, key_state);
    }

    fn start_slot(&mut self, i: usize, note: &NoteEvent, frames: i32, fixed: bool, key_state: i32) {
        let voice = create_runtime_streaming_voice(note, self.sample_rate, note.duration_tick.max(1));
        let Some(voice) = voice else {
            return;
        };
        self.unlink_slot(i);
        {
            let slot = &mut self.slots[i];
            slot.clear();
            slot.active = true;
            slot.host_active = true;
            slot.fixed = fixed;
            slot.mono = note.mono_mode != 0;
            slot.track = note.track;
            slot.channel = note.channel;
            slot.midi_key = note.midi_key;
            slot.render_midi_key = note.render_midi_key;
            slot.render_key6 = note.render_key6;
            slot.synth_midi_key = note.render_midi_key;
            slot.synth_key6 = note.render_key6;
            slot.note = Some(note.clone());
            slot.gate_frames = frames;
            slot.velocity = note.velocity.max(1);
            slot.volume = note.volume;
            slot.expression = note.expression;
            slot.pan = note.pan;
            slot.master_volume = note.attenuation_master_volume;
            slot.velocity_uses_ctrl_table = note.attenuation_velocity_ctrl_table;
            slot.modulation = note.modulation;
            slot.bend14 = note.bend14;
            slot.bend_range = note.bend_range;
            slot.sustain = note.sustain;
        }
        self.age_counter += 1;
        self.slots[i].age = self.age_counter;
        self.generation_counter += 1;
        self.slots[i].generation = self.generation_counter;
        self.slots[i].duplicate_count = 1;
        // The voice is set before gains/pitch so update_voice_pitch can drive it.
        self.slots[i].voice = Some(voice);
        self.update_slot_gains(i);
        self.update_voice_pitch(i);
        let reset_env = note.fm_reset_env;
        {
            let slot = &mut self.slots[i];
            if let Some(v) = slot.voice.as_mut() {
                v.set_gate_frames(frames);
                v.key_state(key_state, reset_env);
            }
        }
        self.link_slot(i);
        if self.slots[i].mono {
            let target = self.event_frame + frames;
            self.schedule_mono_off(i, target);
        }
    }

    fn update_slot_from_note(&mut self, i: usize, note: &NoteEvent, frames: i32, key_on: bool) {
        self.slots[i].host_active = true;
        if self.slots[i].duplicate_count < 255 {
            self.slots[i].duplicate_count += 1;
        }
        let pos = self.slots[i].voice.as_ref().map(|v| v.position_frames()).unwrap_or(0);
        {
            let slot = &mut self.slots[i];
            slot.note = Some(note.clone());
            slot.midi_key = note.midi_key;
            slot.render_midi_key = note.render_midi_key;
            slot.render_key6 = note.render_key6;
            slot.synth_midi_key = note.render_midi_key;
            slot.synth_key6 = note.render_key6;
            slot.gate_frames = pos + frames;
            slot.velocity = note.velocity.max(1);
            slot.volume = note.volume;
            slot.expression = note.expression;
            slot.pan = note.pan;
            slot.master_volume = note.attenuation_master_volume;
            slot.velocity_uses_ctrl_table = note.attenuation_velocity_ctrl_table;
            slot.modulation = note.modulation;
            slot.bend14 = note.bend14;
            slot.bend_range = note.bend_range;
            slot.sustain = note.sustain;
        }
        self.age_counter += 1;
        self.slots[i].age = self.age_counter;
        self.update_slot_gains(i);
        self.update_voice_pitch(i);
        let gate = self.slots[i].gate_frames;
        let modulation = self.slots[i].modulation;
        let reset_env = note.fm_reset_env;
        if let Some(v) = self.slots[i].voice.as_mut() {
            v.set_velocity(127);
            v.set_modulation(modulation);
            v.set_gate_frames(gate);
            if key_on {
                v.key_state(2, reset_env);
            }
        }
    }

    fn update_slot_gains(&mut self, i: usize) {
        let slot = &mut self.slots[i];
        slot.left_gain_q15 = bus::left_gain_q15(
            slot.volume,
            slot.expression,
            slot.velocity,
            slot.master_volume,
            slot.velocity_uses_ctrl_table,
            slot.pan,
        );
        slot.right_gain_q15 = bus::right_gain_q15(
            slot.volume,
            slot.expression,
            slot.velocity,
            slot.master_volume,
            slot.velocity_uses_ctrl_table,
            slot.pan,
        );
    }

    fn update_voice_pitch(&mut self, i: usize) {
        let tail = self.slots[i]
            .note
            .as_ref()
            .filter(|n| n.fm_pitch_tail_valid)
            .map(|n| (n.fm_pitch_tail_shift, n.fm_pitch_tail_mantissa));
        let synth_midi_key = self.slots[i].synth_midi_key;
        let synth_key6 = self.slots[i].synth_key6;
        let bend14 = self.slots[i].bend14;
        let bend_range = self.slots[i].bend_range;
        if let Some(v) = self.slots[i].voice.as_mut() {
            if let Some((shift, mantissa)) = tail {
                v.set_pitch_tail(shift, mantissa);
            } else {
                v.set_pitch(synth_midi_key, synth_key6, bend14, bend_range);
            }
        }
    }

    fn find_slot(&self) -> SlotPick {
        let mut free: Option<usize> = None;
        let mut released: Option<usize> = None;
        let mut oldest_host: Option<usize> = None;
        let mut host_count = 0;
        for i in 0..self.slots.len() {
            let slot = &self.slots[i];
            if !slot.active && free.is_none() {
                free = Some(i);
            }
            if slot.host_active {
                host_count += 1;
                if oldest_host.map(|o| slot.age < self.slots[o].age).unwrap_or(true) {
                    oldest_host = Some(i);
                }
            } else if released.map(|r| slot.age < self.slots[r].age).unwrap_or(true) {
                released = Some(i);
            }
        }
        if host_count < self.slots.len() {
            if let Some(i) = released {
                return SlotPick {
                    slot: i,
                    high_bit_steal: false,
                };
            }
            if let Some(i) = free {
                return SlotPick {
                    slot: i,
                    high_bit_steal: false,
                };
            }
        }
        SlotPick {
            slot: oldest_host.unwrap_or(0),
            high_bit_steal: true,
        }
    }

    fn schedule_mono_off(&mut self, i: usize, frame: i32) {
        let mono = MonoOff {
            frame: frame.max(self.event_frame + 1),
            slot: self.slots[i].index,
            generation: self.slots[i].generation,
        };
        let mut pos = 0;
        while pos < self.mono_offs.len() && self.mono_offs[pos].frame <= mono.frame {
            pos += 1;
        }
        self.mono_offs.insert(pos, mono);
    }

    fn prepare_slot_for_reuse(&mut self, i: usize) {
        if !self.slots[i].active {
            return;
        }
        let host = self.slots[i].host_active;
        self.unlink_slot(i);
        if host {
            if let Some(v) = self.slots[i].voice.as_mut() {
                v.key_state(0, true);
            }
            self.slots[i].host_active = false;
            self.age_counter += 1;
            self.slots[i].age = self.age_counter;
        } else if let Some(v) = self.slots[i].voice.as_mut() {
            v.key_state(255, true);
        }
        self.slots[i].clear();
    }

    fn release_slot(&mut self, i: usize) {
        if self.slots[i].active && self.slots[i].host_active {
            self.unlink_slot(i);
            self.slots[i].release();
            self.age_counter += 1;
            self.slots[i].age = self.age_counter;
        }
    }

    fn force_off_slot(&mut self, i: usize) {
        self.unlink_slot(i);
        self.slots[i].force_off();
    }

    fn link_slot(&mut self, i: usize) {
        let track = safe_track(self.slots[i].track) as usize;
        let channel = safe_channel(self.slots[i].channel) as usize;
        self.slots[i].linked_next = self.note_heads[track][channel];
        self.note_heads[track][channel] = self.slots[i].index;
    }

    fn unlink_slot(&mut self, i: usize) {
        if self.slots[i].linked_next == -2 {
            return;
        }
        let track = safe_track(self.slots[i].track) as usize;
        let channel = safe_channel(self.slots[i].channel) as usize;
        let mut cur = self.note_heads[track][channel];
        let mut prev = -1;
        while cur >= 0 {
            if cur as usize >= self.slots.len() {
                break;
            }
            if cur == self.slots[i].index {
                if prev < 0 {
                    self.note_heads[track][channel] = self.slots[i].linked_next;
                } else {
                    self.slots[prev as usize].linked_next = self.slots[i].linked_next;
                }
                break;
            }
            let next = self.slots[cur as usize].linked_next;
            prev = cur;
            cur = next;
        }
        self.slots[i].linked_next = -2;
        self.slots[i].duplicate_count = 0;
    }

    // ---- audio activation ----

    fn activate_audio(&mut self, event: &AudioEvent, frame: i32) {
        if event.builtin_wave && event.wave_record.len() >= 40 {
            self.activate_builtin_wave(event, frame);
            return;
        }
        let stereo = event.sample.resample_to_stereo(self.sample_rate, 64, 127);
        if stereo.is_empty() {
            return;
        }
        let mut stop = ticks_to_frames(event.duration_tick, self.sample_rate).max(1);
        if event.builtin_wave && event.wave_record.len() > 5 && event.wave_record[5] != 0 {
            stop = 536870911;
        } else {
            stop += frame;
        }
        self.audios.push(ActiveAudio::new(frame, stop, event, stereo));
    }

    fn activate_builtin_wave(&mut self, event: &AudioEvent, frame: i32) {
        // Pick a wave slot: prefer a free/reclaimable one, else steal the
        // youngest by age, tracking the overall oldest as the last resort.
        let mut chosen: Option<usize> = None;
        let mut oldest = 0usize;
        for w in 0..self.wave_slots.len() {
            let reclaimable = !self.wave_slots[w].is_active() || self.wave_slots[w].is_reclaimable_at(frame);
            let better_than_chosen = chosen.map(|c| self.wave_slots[w].age() < self.wave_slots[c].age()).unwrap_or(true);
            if reclaimable && better_than_chosen {
                chosen = Some(w);
            }
            if self.wave_slots[w].age() < self.wave_slots[oldest].age() {
                oldest = w;
            }
        }
        let target = chosen.unwrap_or(oldest);
        let was_active = self.wave_slots[target].is_active();
        if was_active {
            self.wave_slots[target].hard_stop();
        }
        self.age_counter += 1;
        let age = self.age_counter;
        self.wave_slots[target].start(event, frame, age, was_active);
        let stop = frame + ticks_to_frames(event.duration_tick, self.sample_rate).max(1);
        self.wave_slots[target].set_allocation_stop_frame(stop);
        if self.wave_slots[target].is_active() && event.wave_record[5] == 0 {
            self.wave_slots[target].set_stop_frame(stop);
        }
    }
}

fn apply_gain(buf: &mut [f32], offset: i32, count: i32, gain_l: f32, gain_r: f32) {
    let mut idx = (offset * 2) as usize;
    for _ in 0..count {
        buf[idx] *= gain_l;
        buf[idx + 1] *= gain_r;
        idx += 2;
    }
}

/// Renders `analysis` in 1024-frame chunks exactly as the reference player and
/// the `RefStreamF32` harness do, returning interleaved stereo `f32`. The 40s
/// cap mirrors the harness so captures stay a manageable size.
pub fn render_full(analysis: Analysis, rate: i32) -> Vec<f32> {
    let mut renderer = Renderer::prepare(analysis, rate);
    let cap = rate * 40;
    let mut out = Vec::new();
    let mut buf = vec![0.0f32; 1024 * 2];
    let mut total = 0;
    loop {
        let n = renderer.render(&mut buf, 1024);
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..(n * 2) as usize]);
        total += n;
        if total > cap {
            break;
        }
    }
    out
}

/// How many stereo frames the streaming renderer produces for `analysis`,
/// without rendering. Cheap enough for the play path to learn a song's length.
pub fn frame_count_of(analysis: &Analysis, rate: i32) -> i32 {
    compute_frame_count(analysis, if rate <= 0 { 48000 } else { rate })
}

/// Renders the whole song through the streaming renderer, returning interleaved
/// stereo `f32`. Unlike [`render_full`], there is no time cap: it renders every
/// frame the renderer reports, which is what the live play path wants.
pub fn render_all(analysis: Analysis, rate: i32) -> Vec<f32> {
    let mut renderer = Renderer::prepare(analysis, rate);
    let total = renderer.frame_count().max(0);
    let mut out = vec![0.0f32; total as usize * 2];
    let chunk = 4096;
    let mut pos = 0;
    while pos < total {
        let n = renderer.render_mix(&mut out, pos, (total - pos).min(chunk), 1.0, 1.0);
        if n <= 0 {
            break;
        }
        pos += n;
    }
    out.truncate(pos.max(0) as usize * 2);
    out
}

#[cfg(test)]
mod tests {
    /// Render a file through the streaming renderer and check every `f32`
    /// sample against a `RefStreamF32` capture, bit for bit. Gated on
    /// `OMA3_STREAM_MMF` and `OMA3_STREAM_F32`.
    #[test]
    fn streams_like_the_reference() {
        let (mmf_path, ref_path) = match (std::env::var("OMA3_STREAM_MMF"), std::env::var("OMA3_STREAM_F32")) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return,
        };
        let data = std::fs::read(&mmf_path).unwrap();
        let smaf = super::super::smaf::parse(&data).unwrap();
        let analysis = super::super::analysis::analyze(&smaf);
        let got = super::render_full(analysis, 44100);

        let raw = std::fs::read(&ref_path).unwrap();
        let want: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();

        assert_eq!(got.len(), want.len(), "sample count got {} want {}", got.len(), want.len());
        let mut mismatches = 0;
        let mut first = None;
        let mut max_abs = 0.0f32;
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            if g.to_bits() != w.to_bits() {
                mismatches += 1;
                if first.is_none() {
                    first = Some((i, *g, *w));
                }
                max_abs = max_abs.max((g - w).abs());
            }
        }
        assert!(
            mismatches == 0,
            "{} / {} samples differ; first {:?}; max abs diff {}",
            mismatches,
            got.len(),
            first,
            max_abs
        );
        eprintln!("matched {} samples exactly", got.len());
    }

    /// The live play path renders in 4096-frame chunks via [`super::render_all`];
    /// prove that produces the same samples as the reference's 1024-frame chunks,
    /// so the outer chunk size never changes the output. Gated the same way, and
    /// only meaningful for captures shorter than the harness's 40s cap.
    #[test]
    fn render_all_matches_the_reference() {
        let (mmf_path, ref_path) = match (std::env::var("OMA3_STREAM_MMF"), std::env::var("OMA3_STREAM_F32")) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return,
        };
        let data = std::fs::read(&mmf_path).unwrap();
        let smaf = super::super::smaf::parse(&data).unwrap();
        let analysis = super::super::analysis::analyze(&smaf);
        let got = super::render_all(analysis, 44100);

        let raw = std::fs::read(&ref_path).unwrap();
        let want: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        if want.len() >= 44100 * 40 * 2 {
            return; // ref hit the 40s cap; render_all is uncapped, lengths differ.
        }
        assert_eq!(got.len(), want.len(), "sample count got {} want {}", got.len(), want.len());
        let mismatches = got.iter().zip(want.iter()).filter(|(g, w)| g.to_bits() != w.to_bits()).count();
        assert!(mismatches == 0, "{} / {} samples differ", mismatches, got.len());
        eprintln!("render_all matched {} samples exactly", got.len());
    }
}
