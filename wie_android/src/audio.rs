//! Audio and vibration are pushed to Java as opaque byte commands, drained by
//! `NativeBridge.nativePollOutput` and decoded in `AndroidAudioOutput`.
//!
//! Every command starts with a one byte opcode; all multi-byte fields are
//! little endian.
//!
//! | opcode | layout                                                              |
//! |--------|---------------------------------------------------------------------|
//! | 1      | `channel:u8`, `sample_rate:u32`, `sample_count:u32`, `samples:i16[]` |
//! | 8      | `intensity:u8`, `duration_ms:u64`                                   |
//!
//! MIDI is not represented: Android has no general MIDI synth to hand it to,
//! so those events are dropped rather than silently mistranslated.

use crate::platform::Shared;

const OPCODE_PLAY_WAVE: u8 = 1;
const OPCODE_VIBRATE: u8 = 8;

/// Header length shared by both commands; `AndroidAudioOutput` rejects
/// anything shorter.
const HEADER_LEN: usize = 10;

pub fn vibrate_command(duration_ms: u64, intensity: u8) -> Vec<u8> {
    let mut command = Vec::with_capacity(HEADER_LEN);

    command.push(OPCODE_VIBRATE);
    command.push(intensity);
    command.extend_from_slice(&duration_ms.to_le_bytes());

    command
}

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

pub struct AndroidAudioSink {
    shared: Shared,
}

impl AndroidAudioSink {
    pub fn new(shared: Shared) -> Self {
        Self { shared }
    }
}

impl wie_backend::AudioSink for AndroidAudioSink {
    fn play_wave(&self, channel: u8, sampling_rate: u32, wave_data: &[i16]) {
        if wave_data.is_empty() {
            return;
        }

        self.shared.push_audio(play_wave_command(channel, sampling_rate, wave_data));
    }

    fn midi_note_on(&self, channel_id: u8, note: u8, velocity: u8) {
        tracing::debug!("midi_note_on({channel_id}, {note}, {velocity}) - no synth on Android");
    }

    fn midi_note_off(&self, channel_id: u8, note: u8, velocity: u8) {
        tracing::debug!("midi_note_off({channel_id}, {note}, {velocity}) - no synth on Android");
    }

    fn midi_program_change(&self, channel_id: u8, program: u8) {
        tracing::debug!("midi_program_change({channel_id}, {program}) - no synth on Android");
    }

    fn midi_control_change(&self, channel_id: u8, control: u8, value: u8) {
        tracing::debug!("midi_control_change({channel_id}, {control}, {value}) - no synth on Android");
    }

    fn midi_pitch_bend(&self, channel_id: u8, value: u16) {
        tracing::debug!("midi_pitch_bend({channel_id}, {value}) - no synth on Android");
    }

    fn midi_sysex(&self, data: &[u8]) {
        tracing::debug!("midi_sysex({} bytes) - no synth on Android", data.len());
    }
}

#[cfg(test)]
mod tests {
    use super::{HEADER_LEN, play_wave_command, vibrate_command};

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
    fn vibrate_layout_matches_java_decoder() {
        let command = vibrate_command(250, 200);

        assert_eq!(command[0], 8);
        assert_eq!(command[1], 200);
        assert_eq!(u64::from_le_bytes(command[2..10].try_into().unwrap()), 250);
        assert_eq!(command.len(), HEADER_LEN);
    }
}
