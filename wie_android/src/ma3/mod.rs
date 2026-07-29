//! A Yamaha MA-3, in software.
//!
//! A `.mmf` holds two things: recordings, which Android will play, and a
//! sequence, which is only a list of notes. The chip that turned that list
//! into sound was an MA-3, and Android has nothing like it, so the sequence
//! used to be dropped and titles whose music is entirely sequenced played in
//! silence.
//!
//! This is that chip. It is four operator FM, the operators and the wiring
//! between them taken from the voices the file itself carries, using the
//! handset's own tables for waveforms, envelope rates, key scaling, detune,
//! the low frequency oscillator and the output attenuation. What comes out is
//! the title's music with its own instruments, not an impression of them.
//!
//! What is missing is the part of a handset that lived in ROM: the melodic
//! bank a file falls back on when it defines no voice of its own, and the
//! recorded drums behind twenty one of the kit's keys. Those get stand ins,
//! marked as such where they are built.
//!
//! - [`tone`] reads voices out of the file's system exclusive.
//! - [`voice`] is one sounding note.
//! - [`bus`] is the output stage, which attenuates rather than multiplies.
//! - [`tables`] is the chip's own data.

mod bus;
mod tables;
mod tone;
mod voice;

use crate::ma3::{
    bus::{clamp_i16, mix_q15, stereo_gain_q15},
    tone::Bank,
    voice::Voice,
};

/// Rate the rendered stream runs at. FM puts out harmonics well above the
/// note, so a rate this side of the chip's own would fold them back audibly.
pub const SAMPLE_RATE: u32 = 44100;

/// Channels in the rendered stream. A sequence pans its parts, and the chip's
/// output stage is stereo, so folding it down would throw that away.
pub const CHANNELS: usize = 2;

/// Notes at once, which is what the chip could hold.
const MAX_VOICES: usize = 16;

/// MIDI channels a sequence can address.
const MIDI_CHANNELS: usize = 16;

/// Controllers this synthesiser acts on.
const CONTROL_MODULATION: u8 = 1;
const CONTROL_VOLUME: u8 = 7;
const CONTROL_PAN: u8 = 10;
const CONTROL_EXPRESSION: u8 = 11;
const CONTROL_ALL_SOUND_OFF: u8 = 120;
const CONTROL_RESET_CONTROLLERS: u8 = 121;
const CONTROL_ALL_NOTES_OFF: u8 = 123;

/// What a channel is set to. A sequence changes these between notes, and a
/// note takes its copy when it starts.
#[derive(Clone, Copy)]
struct Channel {
    program: u8,
    volume: u8,
    expression: u8,
    pan: u8,
    modulation: u8,
    bend: u16,
    bend_range: u8,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            program: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            modulation: 0,
            bend: 8192,
            bend_range: 2,
        }
    }
}

/// How far the wheel has been moved, in the four steps the chip recognised.
fn modulation_depth(value: u8) -> usize {
    match value {
        0 => 0,
        1..=31 => 1,
        32..=63 => 2,
        64..=95 => 3,
        _ => 4,
    }
}

struct Slot {
    channel: u8,
    note: u8,
    velocity: u8,
    voice: Voice,
    left_q15: i32,
    right_q15: i32,
}

pub struct Synth {
    channels: [Channel; MIDI_CHANNELS],
    voices: Vec<Slot>,
    bank: Bank,
    /// Voices the running file has defined, so a title playing on stand ins
    /// rather than its own instruments is visible in the log.
    reported_voices: usize,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}

impl Synth {
    pub fn new() -> Self {
        Self {
            channels: [Channel::default(); MIDI_CHANNELS],
            voices: Vec::with_capacity(MAX_VOICES),
            bank: Bank::new(),
            reported_voices: 0,
        }
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        // A note on with no velocity is how a lot of sequences release a note.
        if velocity == 0 {
            self.note_off(channel, note);
            return;
        }

        let Some(state) = self.channel(channel) else {
            return;
        };
        let state = *state;

        let (tone, pitch) = self.bank.tone_for(channel, state.program, note);
        let voice = Voice::new(
            &tone,
            pitch,
            state.bend,
            state.bend_range,
            modulation_depth(state.modulation),
            SAMPLE_RATE,
        );

        let (left_q15, right_q15) = gains(&state, velocity);
        let slot = Slot {
            channel,
            note,
            velocity,
            voice,
            left_q15,
            right_q15,
        };

        if self.voices.len() < MAX_VOICES {
            self.voices.push(slot);
            return;
        }

        // Nothing is free, so the quietest voice goes: stealing is then heard
        // as a note ending early rather than one being cut off.
        let quietest = self
            .voices
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| left.voice.loudness().total_cmp(&right.voice.loudness()))
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.voices[quietest] = slot;
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        for slot in &mut self.voices {
            if slot.channel == channel && slot.note == note {
                slot.voice.release();
            }
        }
    }

    pub fn program_change(&mut self, channel: u8, program: u8) {
        if let Some(state) = self.channel(channel) {
            state.program = program & 127;
        }
    }

    pub fn control_change(&mut self, channel: u8, control: u8, value: u8) {
        let Some(state) = self.channel(channel) else {
            return;
        };

        match control {
            CONTROL_MODULATION => {
                state.modulation = value;
                let depth = modulation_depth(value);
                for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
                    slot.voice.set_modulation(depth);
                }
                return;
            }
            CONTROL_VOLUME => state.volume = value,
            CONTROL_PAN => state.pan = value,
            CONTROL_EXPRESSION => state.expression = value,
            CONTROL_ALL_SOUND_OFF => {
                for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
                    slot.voice.all_sound_off();
                }
                return;
            }
            CONTROL_RESET_CONTROLLERS | CONTROL_ALL_NOTES_OFF => {
                if control == CONTROL_RESET_CONTROLLERS {
                    let program = state.program;
                    *state = Channel {
                        program,
                        ..Channel::default()
                    };
                }
                for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
                    slot.voice.release();
                }
            }
            _ => return,
        }

        self.refresh_gains(channel);
    }

    /// The wheel is centred at `0x2000`, and its range is the two semitones a
    /// sequence gets unless it says otherwise.
    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        let Some(state) = self.channel(channel) else {
            return;
        };
        state.bend = value.min(16383);
        let state = *state;

        for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
            slot.voice.set_pitch(slot.note, state.bend, state.bend_range, SAMPLE_RATE);
        }
    }

    /// Takes a voice out of a system exclusive message.
    ///
    /// This is where a title's instruments come from: the chip had no melodic
    /// bank a sequence could rely on, so a file that wants to be heard as
    /// itself sends its own voices, in the setup chunk and sometimes part way
    /// through the sequence.
    pub fn sysex(&mut self, message: &[u8]) {
        if !self.bank.accept_sysex(message) {
            return;
        }

        if self.bank.len() != self.reported_voices {
            self.reported_voices = self.bank.len();
            tracing::debug!("The sequence has defined {} of its own voices", self.reported_voices);
        }
    }

    pub fn silence(&mut self) {
        self.voices.clear();
        self.channels = [Channel::default(); MIDI_CHANNELS];
        self.bank.clear();
        self.reported_voices = 0;
    }

    pub fn sounding(&self) -> bool {
        !self.voices.is_empty()
    }

    /// Renders `frames` frames of interleaved stereo, or nothing at all when
    /// no voice is sounding: silence is better left unqueued than pushed as
    /// zeroes.
    pub fn render(&mut self, frames: usize) -> Option<Vec<i16>> {
        if !self.sounding() {
            return None;
        }

        let mut output = vec![0i16; frames * CHANNELS];

        for frame in output.chunks_exact_mut(CHANNELS) {
            let mut left = 0;
            let mut right = 0;

            for slot in &mut self.voices {
                let sample = slot.voice.sample();
                left = mix_q15(left, sample, slot.left_q15);
                right = mix_q15(right, sample, slot.right_q15);
            }

            // A voice that has fallen silent gives its slot back, whether it
            // was released or simply ran out of envelope, which is how
            // percussion ends.
            self.voices.retain(|x| x.voice.audible());

            frame[0] = clamp_i16(left as i64) as i16;
            frame[1] = clamp_i16(right as i64) as i16;
        }

        Some(output)
    }

    fn channel(&mut self, channel: u8) -> Option<&mut Channel> {
        self.channels.get_mut(channel as usize)
    }

    fn refresh_gains(&mut self, channel: u8) {
        let Some(state) = self.channels.get(channel as usize).copied() else {
            return;
        };

        for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
            let (left, right) = gains(&state, slot.velocity);
            slot.left_q15 = left;
            slot.right_q15 = right;
        }
    }
}

/// Master volume is not something a sequence sets, so it stays wide open and
/// the mix is left to the file's own levels.
const MASTER_VOLUME: u8 = 127;

fn gains(state: &Channel, velocity: u8) -> (i32, i32) {
    stereo_gain_q15(state.volume, state.expression, velocity, MASTER_VOLUME, state.pan)
}

/// Plays a whole `.mmf` through the synthesiser and returns the result as a
/// WAV, for listening to a change rather than only measuring it.
///
/// Only [`mod@tests`] calls this, but it lives out here so the sequence walk it
/// does is written once: it is the same walk `wie_backend` does at runtime,
/// with the sleeps taken out.
#[cfg(test)]
fn render_file(smaf: &[u8]) -> Vec<u8> {
    use smaf_player::SmafEvent;

    /// The sequence gives times in milliseconds, and stops this long after the
    /// last event so a final chord is not cut off.
    const TAIL_MS: usize = 3000;

    let events = smaf_player::parse_smaf(smaf);
    println!("{} events", events.len());

    let mut synth = Synth::new();
    let mut samples = Vec::new();
    let mut rendered_ms = 0;

    let end = events.iter().map(|(time, _)| *time).max().unwrap_or(0) + TAIL_MS;
    for (time, event) in events {
        while rendered_ms < time.min(end) {
            let step = (time - rendered_ms).min(10);
            samples.extend(synth.render(SAMPLE_RATE as usize * step / 1000).unwrap_or_default());
            rendered_ms += step;
        }

        match event {
            SmafEvent::MidiNoteOn { channel, note, velocity } => synth.note_on(channel, note, velocity),
            SmafEvent::MidiNoteOff { channel, note, .. } => synth.note_off(channel, note),
            SmafEvent::MidiProgramChange { channel, program } => synth.program_change(channel, program),
            SmafEvent::MidiControlChange { channel, control, value } => synth.control_change(channel, control, value),
            SmafEvent::MidiPitchBend { channel, value } => synth.pitch_bend(channel, value),
            SmafEvent::MidiSysEx(data) => synth.sysex(&data),
            // Recordings already have somewhere to go and are not this
            // synthesiser's business.
            SmafEvent::Wave { .. } | SmafEvent::End => {}
        }
    }

    while rendered_ms < end {
        samples.extend(synth.render(SAMPLE_RATE as usize / 100).unwrap_or_default());
        rendered_ms += 10;
    }

    wav(&samples)
}

#[cfg(test)]
fn wav(samples: &[i16]) -> Vec<u8> {
    let data = samples.len() * 2;
    let byte_rate = SAMPLE_RATE * CHANNELS as u32 * 2;
    let mut out = Vec::with_capacity(44 + data);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&((CHANNELS as u16) * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{CHANNELS, MAX_VOICES, SAMPLE_RATE, Synth, modulation_depth};

    /// A four operator voice, as one of the library's own files sends it.
    const VOICE: &[u8] = &[
        0xF0, 0x43, 0x79, 0x06, 0x7F, 0x01, 0x7C, 0x00, 0x55, 0x00, 0x00, 0x02, 0x00, 0x79, 0x43, 0x13, 0x32, 0x21, 0x72, 0x03, 0x44, 0x10, 0x06,
        0x33, 0x64, 0x14, 0x68, 0x22, 0x44, 0x40, 0x00, 0x23, 0x33, 0x32, 0x5A, 0x06, 0x44, 0x10, 0x00, 0x13, 0x01, 0x32, 0x00, 0x00, 0x44, 0x20,
        0x00, 0xF7,
    ];

    fn run(synth: &mut Synth, milliseconds: u32) -> Vec<i16> {
        let frames = (SAMPLE_RATE * milliseconds / 1000) as usize;

        synth.render(frames).unwrap_or_default()
    }

    #[test]
    fn silence_renders_nothing() {
        let mut synth = Synth::new();

        assert!(synth.render(256).is_none());
    }

    #[test]
    fn a_note_makes_a_sound() {
        let mut synth = Synth::new();
        synth.note_on(0, 69, 100);

        let rendered = run(&mut synth, 50);

        assert_eq!(rendered.len() % CHANNELS, 0);
        assert!(rendered.iter().any(|x| *x != 0), "every sample was zero");
    }

    #[test]
    fn a_titles_own_voice_is_used() {
        let mut synth = Synth::new();
        synth.sysex(VOICE);
        synth.program_change(0, 0x55);
        synth.note_on(0, 60, 100);

        assert!(run(&mut synth, 50).iter().any(|x| *x != 0));
    }

    #[test]
    fn a_released_note_fades_and_stops() {
        let mut synth = Synth::new();
        synth.note_on(0, 60, 127);
        run(&mut synth, 20);

        synth.note_off(0, 60);
        for _ in 0..60 {
            run(&mut synth, 100);
        }

        assert!(!synth.sounding(), "the voice never finished releasing");
        assert!(synth.render(256).is_none());
    }

    #[test]
    fn a_note_on_without_velocity_releases() {
        let mut synth = Synth::new();
        synth.note_on(0, 60, 100);
        run(&mut synth, 20);
        assert!(synth.sounding());

        synth.note_on(0, 60, 0);
        for _ in 0..60 {
            run(&mut synth, 100);
        }

        assert!(!synth.sounding());
    }

    #[test]
    fn all_sound_off_stops_only_its_channel() {
        let mut synth = Synth::new();
        synth.note_on(3, 60, 100);
        synth.note_on(4, 62, 100);
        run(&mut synth, 20);

        synth.control_change(3, 120, 0);
        run(&mut synth, 20);

        // Channel four was not addressed, so it is still holding its note.
        assert!(synth.sounding());
    }

    #[test]
    fn more_notes_than_voices_steals_rather_than_drops() {
        let mut synth = Synth::new();

        for note in 40..90 {
            synth.note_on(0, note, 100);
        }

        assert!(synth.render(512).is_some());
    }

    #[test]
    fn voices_are_not_leaked() {
        let mut synth = Synth::new();

        for round in 0..8 {
            for note in 40..90 {
                synth.note_on(0, note, 100);
            }
            run(&mut synth, 20);
            for note in 40..90 {
                synth.note_off(0, note);
            }
            run(&mut synth, 20);

            assert!(synth.voices.len() <= MAX_VOICES, "round {round} held {} voices", synth.voices.len());
        }
    }

    #[test]
    fn turning_a_channel_down_quietens_a_sounding_note() {
        let peak = |volume: u8| {
            let mut synth = Synth::new();
            synth.control_change(0, 7, volume);
            synth.note_on(0, 60, 100);

            run(&mut synth, 50).into_iter().map(|x| (x as i32).abs()).max().unwrap_or(0)
        };

        assert!(peak(30) < peak(127));
        assert_eq!(peak(0), 0);
    }

    #[test]
    fn panning_moves_the_note_across() {
        let mut synth = Synth::new();
        synth.control_change(0, 10, 0);
        synth.note_on(0, 60, 100);

        let rendered = run(&mut synth, 50);
        let left = rendered.iter().step_by(2).map(|x| (*x as i32).abs()).max().unwrap_or(0);
        let right = rendered.iter().skip(1).step_by(2).map(|x| (*x as i32).abs()).max().unwrap_or(0);

        assert!(left > 0);
        assert_eq!(right, 0);
    }

    #[test]
    fn the_wheel_moves_in_four_steps() {
        assert_eq!(modulation_depth(0), 0);
        assert_eq!(modulation_depth(1), 1);
        assert_eq!(modulation_depth(64), 3);
        assert_eq!(modulation_depth(127), 4);
    }

    /// A note has to come out at the frequency it names. Everything else here
    /// could be right while the phase step was wrong, and the result would
    /// still look like music in a level meter.
    #[test]
    fn a_note_comes_out_at_its_own_frequency() {
        use crate::ma3::tone::{OPERATORS, Operator, Tone};

        // A plain sine: one operator at the note's own frequency, held rather
        // than decaying, and a second turned down to nothing.
        let sine = Operator {
            attack: 15,
            level: 0,
            multiple: 1,
            release: 8,
            ..Operator::default()
        };
        let tone = Tone {
            algorithm: 1,
            lfo_speed: 0,
            operator_count: 2,
            operators: [sine, Operator { level: 63, ..sine }, Operator::default(), Operator::default()],
        };

        for (note, expected) in [(45u8, 110.0), (69, 440.0), (81, 880.0)] {
            let mut voice = crate::ma3::voice::Voice::new(&tone, note, 8192, 2, 0, SAMPLE_RATE);

            // Past the attack, so the envelope is not still moving.
            for _ in 0..SAMPLE_RATE / 100 {
                voice.sample();
            }

            let window = SAMPLE_RATE as usize / 2;
            let mut previous = voice.sample();
            let mut crossings = 0;
            for _ in 0..window {
                let sample = voice.sample();
                if previous <= 0.0 && sample > 0.0 {
                    crossings += 1;
                }
                previous = sample;
            }

            let measured = crossings as f64 / (window as f64 / SAMPLE_RATE as f64);
            let error = (measured - expected).abs() / expected;
            assert!(error < 0.02, "note {note} came out at {measured}Hz, not {expected}Hz");
        }

        assert_eq!(tone.operators.len(), OPERATORS);
    }

    #[test]
    fn percussion_sounds_without_a_file_defining_it() {
        let mut synth = Synth::new();

        // Bass drum, snare and a hat, which the kit carries as voices.
        for note in [36, 38, 42] {
            synth.note_on(9, note, 100);
        }

        assert!(run(&mut synth, 50).iter().any(|x| *x != 0));
    }

    /// Renders a `.mmf` to a `.wav`, so a change can be listened to rather
    /// than only measured. Not part of the suite: it needs a file, and titles
    /// are not in the repository.
    ///
    /// `WIE_MMF=path/to/music.mmf WIE_WAV=out.wav cargo test -p wie_android
    /// --lib render_a_file -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a .mmf to render"]
    fn render_a_file() {
        let input = std::env::var("WIE_MMF").expect("WIE_MMF names the file to render");
        let output = std::env::var("WIE_WAV").unwrap_or_else(|_| "rendered.wav".into());

        let smaf = std::fs::read(&input).expect("the named file could be read");
        let wav = super::render_file(&smaf);

        assert!(wav.len() > 44, "{input} rendered no audio at all");
        std::fs::write(&output, &wav).expect("the wav could be written");

        println!("{input} -> {output}, {} bytes", wav.len());
    }
}
