//! `OracleMa3WaveRuntime` - a streaming PCM/wave voice used by the MMF renderer
//! for built-in wave rhythms and direct type-2 wave tones. It plays a decoded
//! recording with its own MA-3 amplitude envelope, key-scaled level, optional
//! amplitude/pitch LFO and pitch bend, looping as the record directs. Ported
//! verbatim from the reference.

use super::analysis::{AudioEvent, ControlEvent, ticks_to_frames};
use super::bus;
use super::tables::{self, ATTACK_RATE_Q31, DECAY_RATE_Q30, LEVEL_Q15, LFO_STEP_Q20, SUSTAIN_Q31};

const Q31_ONE: i64 = 2_147_483_648;

pub struct WaveRuntime {
    active: bool,
    age: i64,
    allocation_stop_frame: i32,
    amp_lfo: bool,
    amp_lfo_depth: i32,
    attack_q31: i64,
    base_step_q16: i32,
    bend14: i32,
    bend_range: i32,
    channel: i32,
    decay1_q30: i64,
    decay2_q30: i64,
    effective_level_q15: i32,
    env_q31: i64,
    env_state: i32,
    expression: i32,
    fixed_pan: bool,
    fixed_pan_index: i32,
    index: i32,
    lfo_phase_q20: i32,
    lfo_step_q20: i32,
    loop_start: i32,
    loops: bool,
    master_volume: i32,
    pan: i32,
    pcm: Vec<i16>,
    pitch_lfo: bool,
    pitch_lfo_depth: i32,
    position: f64,
    release_q30: i64,
    sample_end: i32,
    state_bits: i32,
    step_q16: i32,
    stop_frame: i32,
    sustain_q31: i64,
    track: i32,
    velocity: i32,
    velocity_uses_ctrl_table: bool,
    volume: i32,
}

fn clamp_q16(v: i32, lo: i32, hi: i32) -> i32 {
    if v >= lo { v.min(hi) } else { lo }
}

fn apply_pitch_scale(base: i32, scale: i32) -> i32 {
    clamp_q16(((((base & 131071) as i64) * scale as i64 + 32768) >> 16) as i32, 2048, 262144)
}

fn pitch_scale_q16(bend: i32, mut range: i32) -> i32 {
    if range > 24 {
        range = 2;
    }
    if bend >= 0 && range != 0 && bend != 8192 {
        clamp_q16(
            (2f64.powf((bend - 8192) as f64 / 8192.0 * range as f64 / 12.0) * 65536.0).round() as i32,
            1,
            262144,
        )
    } else {
        65536
    }
}

fn rate_index(rate: i32, ksr: i32) -> i32 {
    let base = (rate & 0xFF) * 4;
    let v = if base != 0 { base + ksr } else { base };
    v.min(63)
}

fn key_scale(base_step: i32, mode: i32) -> i32 {
    let clamped = clamp_q16(base_step, 2048, 65536);
    let mut slot = 0;
    let mut frac = 0;
    while slot < 8 {
        frac = (((clamped as u32) >> (slot + 1)) as i32) - 1024;
        if (0..1024).contains(&frac) {
            break;
        }
        slot += 1;
    }
    let code = (((frac as u32 >> 9) as i32) | (slot << 1)) & 0xFF;
    if mode == 0 { (code as u32 >> 2) as i32 } else { code }
}

fn keyscale_level(base_step: i32, mode: i32) -> i32 {
    if mode == 0 {
        return 32768;
    }
    let clamped = clamp_q16(base_step, 2048, 65536);
    let mut slot = 0;
    let mut frac = 0;
    while slot < 8 {
        frac = (((clamped as u32) >> (slot + 1)) as i32) - 1024;
        if (0..1024).contains(&frac) {
            break;
        }
        slot += 1;
    }
    let index = ((mode - 1) & 3) * 128 + (((((frac as u32 >> 6) as i32 & 15) << 3) | (slot & 7)) & 127);
    tables::wave_keyscale_q15(index as usize)
}

fn le32(data: &[u8], at: usize) -> i32 {
    (data[at] as i32 & 0xFF) | (data[at + 1] as i32 & 0xFF) << 8 | (data[at + 2] as i32 & 0xFF) << 16 | (data[at + 3] as i32 & 0xFF) << 24
}

/// `m(int)` - the reference's unsigned 32-to-64 widening.
fn u32_to_u64(v: i32) -> u64 {
    v as u32 as u64
}

impl WaveRuntime {
    pub fn new(index: i32) -> Self {
        WaveRuntime {
            active: false,
            age: 0,
            allocation_stop_frame: 0,
            amp_lfo: false,
            amp_lfo_depth: 0,
            attack_q31: 0,
            base_step_q16: 0,
            bend14: 8192,
            bend_range: 2,
            channel: 0,
            decay1_q30: 0,
            decay2_q30: 0,
            effective_level_q15: 0,
            env_q31: 0,
            env_state: 0,
            expression: 127,
            fixed_pan: false,
            fixed_pan_index: 0,
            index,
            lfo_phase_q20: 0,
            lfo_step_q20: 0,
            loop_start: 0,
            loops: false,
            master_volume: 76,
            pan: 64,
            pcm: Vec::new(),
            pitch_lfo: false,
            pitch_lfo_depth: 0,
            position: 0.0,
            release_q30: 0,
            sample_end: 0,
            state_bits: 0,
            step_q16: 0,
            stop_frame: 0,
            sustain_q31: 0,
            track: 0,
            velocity: 0,
            velocity_uses_ctrl_table: false,
            volume: 100,
        }
    }

    pub fn index(&self) -> i32 {
        self.index
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn age(&self) -> i64 {
        self.age
    }

    pub fn is_reclaimable_at(&self, frame: i32) -> bool {
        self.active && self.allocation_stop_frame <= frame
    }

    pub fn set_allocation_stop_frame(&mut self, frame: i32) {
        self.allocation_stop_frame = frame;
    }

    pub fn set_stop_frame(&mut self, frame: i32) {
        self.stop_frame = frame;
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.state_bits = 0;
        self.pcm = Vec::new();
        self.env_state = 0;
    }

    pub fn hard_stop(&mut self) {
        self.state_bits = (self.state_bits | 44) & -2;
        self.active = false;
    }

    fn stop_silent(&mut self) {
        self.state_bits |= 8;
        self.active = false;
    }

    pub fn apply_control(&mut self, control: &ControlEvent) {
        if self.active && control.track == self.track && control.channel == self.channel {
            self.volume = control.volume;
            self.expression = control.expression;
            self.pan = control.pan;
            self.master_volume = control.attenuation_master_volume;
            self.velocity_uses_ctrl_table = control.attenuation_velocity_ctrl_table;
            if control.bend14 != self.bend14 || control.bend_range != self.bend_range {
                self.bend14 = control.bend14;
                self.bend_range = control.bend_range;
                self.step_q16 = apply_pitch_scale(self.base_step_q16, pitch_scale_q16(self.bend14, self.bend_range));
                self.state_bits |= 64;
            }
            if control.changed_control == 120 {
                self.hard_stop();
            } else if control.changed_control == 123 && self.env_state != 1 {
                self.env_state = 1;
                self.state_bits = (self.state_bits | 2) & -2;
            } else {
                self.state_bits |= 128;
            }
        }
    }

    fn step_envelope(&mut self) {
        match self.env_state {
            1 => {
                self.env_q31 = self.release_q30 * self.env_q31 >> 30;
                if self.env_q31 == 0 {
                    self.stop_silent();
                }
            }
            2 | 3 => {
                if self.env_state == 2 {
                    self.position = 0.0;
                    self.env_state = 3;
                }
                self.env_q31 += self.attack_q31;
                if self.env_q31 > 2_147_483_647 {
                    self.env_q31 = Q31_ONE;
                    self.env_state = 4;
                }
            }
            4 => {
                self.env_q31 = self.decay1_q30 * self.env_q31 >> 30;
                if self.env_q31 <= self.sustain_q31 {
                    self.env_state = 5;
                }
            }
            5 => {
                self.env_q31 = self.decay2_q30 * self.env_q31 >> 30;
                if self.env_q31 == 0 {
                    self.stop_silent();
                }
            }
            _ => self.stop_silent(),
        }
    }

    fn normalize_position(&mut self) {
        if self.position < self.sample_end as f64 {
            return;
        }
        if !self.loops {
            self.position = self.sample_end as f64;
            return;
        }
        let span = (self.sample_end - self.loop_start) as f64;
        if span <= 0.0 {
            self.stop_silent();
            return;
        }
        loop {
            if self.position < self.sample_end as f64 {
                if self.position < self.loop_start as f64 {
                    self.position = self.loop_start as f64;
                }
                return;
            }
            self.position -= span;
        }
    }

    pub fn render(&mut self, buf: &mut [f32], offset: i32, base_frame: i32, count: i32) {
        if !self.active {
            return;
        }
        let mut idx = (offset * 2) as usize;
        let left = bus::wave_left_gain(
            self.fixed_pan,
            self.fixed_pan_index,
            self.volume,
            self.expression,
            self.velocity,
            self.master_volume,
            self.velocity_uses_ctrl_table,
            self.pan,
        );
        let right = bus::wave_right_gain(
            self.fixed_pan,
            self.fixed_pan_index,
            self.volume,
            self.expression,
            self.velocity,
            self.master_volume,
            self.velocity_uses_ctrl_table,
            self.pan,
        );
        let mut i = 0;
        while i < count {
            if !self.active {
                return;
            }
            if base_frame + i >= self.stop_frame && self.env_state != 1 {
                self.env_state = 1;
                self.state_bits = (self.state_bits | 2) & -2;
            }
            self.step_envelope();
            if !self.active {
                return;
            }
            self.normalize_position();
            if !self.active {
                return;
            }
            let pos = self.position;
            let i0 = pos as i32;
            let mut i1 = i0 + 1;
            if i1 > self.sample_end {
                i1 = if self.loops { self.loop_start } else { i0 };
            }
            let s0 = self.pcm[i0 as usize] as i32;
            let delta = (self.pcm[i1 as usize] as i32 - s0) as f64;
            let mut level = self.effective_level_q15;
            if self.amp_lfo {
                level = level * tables::lfo_level_q15(self.amp_lfo_depth as usize, ((self.lfo_phase_q20 as u32) >> 20) as usize) >> 15;
            }
            level = (((self.env_q31 as u64 >> 16) * level as u64) >> 15) as i32;
            let sample = (s0 as f64 + delta * (pos - i0 as f64)) / 32768.0 * level as f64 / 32768.0;
            buf[idx] = bus::add_output(buf[idx], sample * left);
            buf[idx + 1] = bus::add_output(buf[idx + 1], sample * right);
            idx += 2;
            let mut step = self.step_q16 as f64 / 65536.0;
            if self.pitch_lfo {
                step *= tables::wave_pitch_q20(self.pitch_lfo_depth as usize, ((self.lfo_phase_q20 as u32) >> 20) as usize) as f64 / 1_048_576.0;
            }
            if step < 0.03125 {
                step = 0.03125;
            }
            if step > 4.0 {
                step = 4.0;
            }
            self.position += step;
            self.lfo_phase_q20 = self.lfo_phase_q20.wrapping_add(self.lfo_step_q20);
            i += 1;
        }
    }

    pub fn start(&mut self, event: &AudioEvent, frame: i32, age: i64, reclaimed: bool) {
        let record = &event.wave_record;
        if event.sample.pcm_mono.len() >= 2 && record.len() >= 40 {
            self.pcm = event.sample.pcm_mono.clone();
            let mut sample_end = le32(record, 28);
            if sample_end == 0 || sample_end >= self.pcm.len() as i32 {
                sample_end = self.pcm.len() as i32 - 1;
            }
            self.sample_end = sample_end;
            let mut loop_start = le32(record, 24);
            if loop_start > sample_end {
                loop_start = if sample_end > 0 { sample_end - 1 } else { 0 };
            }
            self.loop_start = loop_start;
            self.loops = self.loop_start < sample_end;
            self.track = event.track;
            self.channel = event.channel;
            self.volume = event.volume;
            self.expression = event.expression;
            self.pan = event.pan;
            self.velocity = event.velocity.max(1);
            self.master_volume = event.attenuation_master_volume;
            self.velocity_uses_ctrl_table = event.attenuation_velocity_ctrl_table;
            self.fixed_pan = (record[2] as i32 & 255) != 0;
            self.fixed_pan_index = record[0] as i32 & 31;
            let mut base = le32(record, 36);
            if base == 0 {
                base = 65536;
            }
            let scaled = clamp_q16(
                ((u32_to_u64(base) * u32_to_u64(event.wave_pitch_ratio_q16) + 32768) >> 16).min(2_147_483_647) as i32,
                2048,
                65536,
            );
            self.base_step_q16 = scaled;
            self.bend14 = 8192;
            self.bend_range = 2;
            self.step_q16 = apply_pitch_scale(scaled, pitch_scale_q16(8192, 2));
            let ksr = key_scale(self.base_step_q16, record[4] as i32 & 255);
            self.attack_q31 = ATTACK_RATE_Q31[rate_index(record[12] as i32 & 255, ksr) as usize] as i64;
            self.decay1_q30 = DECAY_RATE_Q30[rate_index(record[11] as i32 & 255, ksr) as usize] as i64;
            self.decay2_q30 = DECAY_RATE_Q30[rate_index(record[9] as i32 & 255, ksr) as usize] as i64;
            self.release_q30 = DECAY_RATE_Q30[rate_index(record[10] as i32 & 255, ksr) as usize] as i64;
            self.sustain_q31 = SUSTAIN_Q31[(record[13] as i32 & 15) as usize] as i64;
            let level_mode = [0, 2, 1, 3][(record[15] as i32 & 3) as usize];
            self.effective_level_q15 = LEVEL_Q15[(record[14] as i32 & 63) as usize] * keyscale_level(self.base_step_q16, level_mode) >> 15;
            self.amp_lfo = record[19] != 0;
            self.amp_lfo_depth = record[18] as i32 & 3;
            self.pitch_lfo = record[7] != 0;
            self.pitch_lfo_depth = record[6] as i32 & 3;
            self.lfo_step_q20 = LFO_STEP_Q20[(3 & record[3] as i32) as usize] as i32;
            let stop = frame + ticks_to_frames(event.duration_tick, 48000).max(1);
            self.allocation_stop_frame = stop;
            self.stop_frame = if record[5] != 0 { i32::MAX } else { stop };
            self.position = 0.0;
            self.env_q31 = 0;
            self.env_state = 2;
            self.lfo_phase_q20 = 0;
            self.state_bits = if reclaimed { 193 | 16 } else { 193 };
            self.age = age;
            self.active = true;
        } else {
            self.clear();
        }
    }
}
