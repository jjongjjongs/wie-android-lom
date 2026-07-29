//! Audio and vibration are pushed to Java as opaque byte commands, drained by
//! `NativeBridge.nativePollOutput` and decoded in `AndroidAudioOutput`.
//!
//! Every command starts with a one byte opcode; all multi-byte fields are
//! little endian.
//!
//! | opcode | layout                                                              |
//! |--------|---------------------------------------------------------------------|
//! | 1      | `channel:u8`, `sample_rate:u32`, `sample_count:u32`, `samples:i16[]` |
//! | 2      | `pad:u8`, `sample_rate:u32`, `sample_count:u32`, `samples:i16[]`    |
//! | 8      | `intensity:u8`, `duration_ms:u64`                                   |
//!
//! Opcode 1 is a clip: Java fires one track at it and forgets it. Opcode 2 is
//! the synthesiser's continuous output, which needs a track that stays open
//! between chunks or every seam would be a click.
//!
//! MIDI never reaches Java. Android has no synthesiser that takes live MIDI
//! events, so [`crate::synth`] renders the sequence here and it leaves as
//! opcode 2.

use crate::{platform::Shared, synth::SAMPLE_RATE};

const OPCODE_PLAY_WAVE: u8 = 1;
const OPCODE_STREAM: u8 = 2;
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

/// Packs a chunk of the synthesiser's output for the track Java keeps open.
pub fn stream_command(samples: &[i16]) -> Vec<u8> {
    let mut command = Vec::with_capacity(HEADER_LEN + samples.len() * 2);

    command.push(OPCODE_STREAM);
    command.push(0);
    command.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    command.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    for sample in samples {
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
        self.shared.synth().note_on(channel_id, note, velocity);
    }

    fn midi_note_off(&self, channel_id: u8, note: u8, _velocity: u8) {
        self.shared.synth().note_off(channel_id, note);
    }

    fn midi_program_change(&self, channel_id: u8, program: u8) {
        self.shared.synth().program_change(channel_id, program);
    }

    fn midi_control_change(&self, channel_id: u8, control: u8, value: u8) {
        self.shared.synth().control_change(channel_id, control, value);
    }

    fn midi_pitch_bend(&self, channel_id: u8, value: u16) {
        self.shared.synth().pitch_bend(channel_id, value);
    }

    /// System exclusive is where a handset's own chip was configured, which
    /// this synthesiser has no equivalent for.
    fn midi_sysex(&self, data: &[u8]) {
        tracing::trace!("midi_sysex({} bytes), which this synthesiser has no use for", data.len());
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
