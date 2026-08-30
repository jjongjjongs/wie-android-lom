//! Audio and vibration are pushed to Java as opaque byte commands, drained by
//! `NativeBridge.nativePollOutput` and decoded in `AndroidAudioOutput`.
//!
//! Every command starts with a one byte opcode; all multi-byte fields are
//! little endian.
//!
//! | opcode | layout                                                              |
//! |--------|---------------------------------------------------------------------|
//! | 2      | `channels:u8`, `sample_rate:u32`, `sample_count:u32`, `samples:i16[]` |
//! | 8      | `intensity:u8`, `duration_ms:u64`                                   |
//!
//! Opcode 2 is the synthesiser's continuous output, which needs a track that
//! stays open between chunks or every seam would be a click; its samples are
//! interleaved across however many channels the header names, and its count is
//! of samples rather than of frames.
//!
//! MIDI never reaches Java. Android has no synthesiser that takes live MIDI
//! events, so [`crate::ma3`] renders the sequence here and it leaves as
//! opcode 2. A file's own recorded PCM effects are mixed into that same stream
//! (see [`AndroidAudioSink::play_wave`]) rather than sent as a separate clip.

use crate::{
    ma3::{CHANNELS, SAMPLE_RATE},
    platform::Shared,
};

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
}

impl AndroidAudioSink {
    pub fn new(shared: Shared) -> Self {
        Self { shared }
    }
}

impl wie_backend::AudioSink for AndroidAudioSink {
    fn play_wave(&self, _channel: u8, sampling_rate: u32, wave_data: &[i16]) {
        if wave_data.is_empty() {
            return;
        }

        // A file's recorded effects are mixed into the synthesiser's stream
        // rather than sent as their own clip: the stream is the one output path
        // already proven on the device, so folding them in plays them where a
        // per-clip track stayed silent.
        self.shared.synth().queue_wave(sampling_rate, wave_data);
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

    /// System exclusive is where a file sends the voices it wants played, so
    /// this is what makes a title sound like itself rather than like a set of
    /// stand ins.
    fn midi_sysex(&self, data: &[u8]) {
        self.shared.synth().sysex(data);
    }
}

#[cfg(test)]
mod tests {
    use super::{HEADER_LEN, vibrate_command};

    #[test]
    fn vibrate_layout_matches_java_decoder() {
        let command = vibrate_command(250, 200);

        assert_eq!(command[0], 8);
        assert_eq!(command[1], 200);
        assert_eq!(u64::from_le_bytes(command[2..10].try_into().unwrap()), 250);
        assert_eq!(command.len(), HEADER_LEN);
    }
}
