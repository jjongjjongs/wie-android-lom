//! Audio and vibration are pushed to Java as opaque byte commands, drained by
//! `NativeBridge.nativePollOutput` and decoded in `AndroidAudioOutput`.
//!
//! Every command starts with a one byte opcode; all multi-byte fields are
//! little endian.
//!
//! | opcode | layout                                                              |
//! |--------|---------------------------------------------------------------------|
//! | 1      | `channel:u8`, `sample_rate:u32`, `sample_count:u32`, `samples:i16[]` |
//! | 2      | `channels:u8`, `sample_rate:u32`, `sample_count:u32`, `samples:i16[]` |
//! | 8      | `intensity:u8`, `duration_ms:u64`                                   |
//!
//! Opcode 1 is a clip, always mono: Java fires one track at it and forgets it.
//! Opcode 2 is the synthesiser's continuous output, which needs a track that
//! stays open between chunks or every seam would be a click; its samples are
//! interleaved across however many channels the header names, and its count is
//! of samples rather than of frames.
//!
//! MIDI never reaches Java. Android has no synthesiser that takes live MIDI
//! events, so [`crate::ma3`] renders the sequence here and it leaves as
//! opcode 2.

use crate::{
    ma3::{CHANNELS, SAMPLE_RATE},
    platform::Shared,
};
use std::{
    ffi::c_void,
    sync::atomic::{AtomicPtr, AtomicU8, Ordering},
};

/// Opcode 1 (a one-shot wave) is no longer emitted - recorded waves are mixed
/// into the synth stream instead - but the wire format is still exercised by a
/// layout test, so the constant and its builder live under `cfg(test)`.
#[cfg(test)]
const OPCODE_PLAY_WAVE: u8 = 1;
const OPCODE_STREAM: u8 = 2;
const OPCODE_VIBRATE: u8 = 8;

/// Header length shared by both commands; `AndroidAudioOutput` rejects
/// anything shorter.
const HEADER_LEN: usize = 10;

type WaveCallback = unsafe extern "C" fn(u8, u32, *const i16, usize) -> u8;

static WAVE_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Installs an optional host-side low-latency wave handler.
///
/// The stable symbol lets the Android audio helper register without patching
/// a build-specific instruction address inside this library.
#[unsafe(no_mangle)]
pub extern "C" fn wie_set_wave_callback(callback: *mut c_void) {
    WAVE_CALLBACK.store(callback, Ordering::Release);
}

fn wave_callback_consumed(channel: u8, sampling_rate: u32, wave_data: &[i16]) -> bool {
    let callback = WAVE_CALLBACK.load(Ordering::Acquire);
    if callback.is_null() {
        return false;
    }

    let callback: WaveCallback = unsafe { std::mem::transmute(callback) };
    unsafe { callback(channel, sampling_rate, wave_data.as_ptr(), wave_data.len()) != 0 }
}

pub fn vibrate_command(duration_ms: u64, intensity: u8) -> Vec<u8> {
    let mut command = Vec::with_capacity(HEADER_LEN);

    command.push(OPCODE_VIBRATE);
    command.push(intensity);
    command.extend_from_slice(&duration_ms.to_le_bytes());

    command
}

fn scale_wave_volume(wave_data: &[i16], volume: u8) -> Vec<i16> {
    let volume = volume.min(100);

    wave_data
        .iter()
        .map(|sample| ((*sample as i32 * i32::from(volume)) / 100) as i16)
        .collect()
}

#[cfg(test)]
fn play_wave_command(channel: u8, sampling_rate: u32, wave_data: &[i16]) -> Vec<u8> {
    let mut command = Vec::with_capacity(HEADER_LEN + wave_data.len() * 2);

    command.push(OPCODE_PLAY_WAVE);
    command.push(channel);
    command.extend_from_slice(&sampling_rate.to_le_bytes());
    command.extend_from_slice(&(wave_data.len() as u32).to_le_bytes());
    for sample in wave_data {
        command.extend_from_slice(&sample.to_le_bytes());
    }

    command
}

/// Packs a chunk of the synthesiser's output for the track Java keeps open.
pub fn stream_command(samples: &[i16]) -> Vec<u8> {
    let mut command = Vec::with_capacity(HEADER_LEN + samples.len() * 2);

    command.push(OPCODE_STREAM);
    command.push(CHANNELS as u8);
    command.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    command.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    for sample in samples {
        command.extend_from_slice(&sample.to_le_bytes());
    }

    command
}

pub struct AndroidAudioSink {
    shared: Shared,
    master_volume: AtomicU8,
}

impl AndroidAudioSink {
    pub fn new(shared: Shared) -> Self {
        Self {
            shared,
            master_volume: AtomicU8::new(100),
        }
    }
}

impl wie_backend::AudioSink for AndroidAudioSink {
    fn set_master_volume(&self, volume: u8) {
        let volume = volume.min(100);
        self.master_volume.store(volume, Ordering::Relaxed);
        self.shared.mixer().set_master_volume(volume);
    }

    fn open_midi_voice(&self) -> u32 {
        self.shared.mixer().open()
    }

    fn close_midi_voice(&self, voice: u32) {
        self.shared.mixer().close(voice);
    }

    fn play_wave(&self, channel: u8, sampling_rate: u32, wave_data: &[i16]) {
        if wave_data.is_empty() {
            return;
        }

        let volume = self.master_volume.load(Ordering::Relaxed);
        if volume == 0 {
            return;
        }

        // A disabled per-title override leaves this a no-op, but keep the hook so
        // it can still substitute a wave when enabled.
        if wave_callback_consumed(channel, sampling_rate, wave_data) {
            return;
        }

        // Mix the wave into the synthesiser's output stream rather than firing a
        // one-shot AudioTrack. The device sounds the streamed synth output but
        // not the per-clip static tracks the old opcode-1 path opened, so routing
        // recorded effects through the same stream is what makes them audible.
        let samples = if volume == 100 {
            wave_data.to_vec()
        } else {
            scale_wave_volume(wave_data, volume)
        };
        self.shared.mixer().push_pcm(samples, sampling_rate);
        tracing::info!(
            "[wave] mixed into synth stream: rate={sampling_rate} samples={} volume={volume}",
            wave_data.len()
        );
    }

    fn midi_note_on(&self, voice: u32, channel_id: u8, note: u8, velocity: u8) {
        self.shared.mixer().note_on(voice, channel_id, note, velocity);
    }

    fn midi_note_off(&self, voice: u32, channel_id: u8, note: u8, _velocity: u8) {
        self.shared.mixer().note_off(voice, channel_id, note);
    }

    fn midi_program_change(&self, voice: u32, channel_id: u8, program: u8) {
        self.shared.mixer().program_change(voice, channel_id, program);
    }

    fn midi_control_change(&self, voice: u32, channel_id: u8, control: u8, value: u8) {
        self.shared.mixer().control_change(voice, channel_id, control, value);
    }

    fn midi_pitch_bend(&self, voice: u32, channel_id: u8, value: u16) {
        self.shared.mixer().pitch_bend(voice, channel_id, value);
    }

    /// System exclusive is where a file sends the voices it wants played, so
    /// this is what makes a title sound like itself rather than like a set of
    /// stand ins.
    fn midi_sysex(&self, voice: u32, data: &[u8]) {
        self.shared.mixer().sysex(voice, data);
    }

    /// Renders the whole file through the faithful [`crate::oma3`] port and
    /// installs it as the music stream, so FM titles sound exactly as the
    /// reference plays them. A file with no FM notes (a bare recorded-wave clip)
    /// is declined so the live wave path keeps handling it.
    fn play_smaf(&self, id: u32, data: &[u8], repeat: bool) -> Option<u32> {
        let smaf = crate::oma3::smaf::parse(data).ok()?;
        let notes = crate::oma3::analysis::analyze(&smaf);
        if notes.is_empty() {
            return None;
        }
        let pcm = crate::oma3::analysis::render(&notes, smaf.total_ticks, SAMPLE_RATE as i32);
        let samples: Vec<i16> = pcm.iter().map(|&s| (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16).collect();
        let frames = (samples.len() / CHANNELS) as u64;
        let duration_ms = (frames * 1000 / u64::from(SAMPLE_RATE)) as u32;
        tracing::info!(
            "[smaf] oma3 rendered {} notes, {frames} frames ({duration_ms}ms), repeat={repeat}, id={id}",
            notes.len()
        );
        self.shared.mixer().set_song(id, samples, repeat);
        Some(duration_ms)
    }

    fn stop_smaf(&self, id: u32) {
        self.shared.mixer().stop_song(id);
    }
}

#[cfg(test)]
mod tests {
    use super::{HEADER_LEN, play_wave_command, scale_wave_volume, vibrate_command};

    #[test]
    fn play_wave_layout_matches_java_decoder() {
        let command = play_wave_command(1, 22050, &[-2, 1]);

        assert_eq!(command[0], 1);
        assert_eq!(command[1], 1);
        assert_eq!(u32::from_le_bytes(command[2..6].try_into().unwrap()), 22050);
        // Java multiplies this by two to get the byte count of the PCM body.
        assert_eq!(u32::from_le_bytes(command[6..10].try_into().unwrap()), 2);
        assert_eq!(command.len(), HEADER_LEN + 4);
        assert_eq!(i16::from_le_bytes(command[10..12].try_into().unwrap()), -2);
    }

    #[test]
    fn wave_volume_preserves_wipi_endpoints_and_scales_middle() {
        let input = [-30000, -10000, 10000, 30000];

        assert_eq!(scale_wave_volume(&input, 0), [0, 0, 0, 0]);
        assert_eq!(scale_wave_volume(&input, 50), [-15000, -5000, 5000, 15000]);
        assert_eq!(scale_wave_volume(&input, 100), input);
    }

    #[test]
    fn vibrate_layout_matches_java_decoder() {
        let command = vibrate_command(250, 200);

        assert_eq!(command[0], 8);
        assert_eq!(command[1], 200);
        assert_eq!(u64::from_le_bytes(command[2..10].try_into().unwrap()), 250);
        assert_eq!(command.len(), HEADER_LEN);
    }
}
