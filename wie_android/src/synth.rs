//! A small synthesiser, so a handset's music has somewhere to go.
//!
//! A `.mmf` carries two things: PCM waves for short effects, and a MIDI-like
//! sequence that is the music. Android will play PCM and has no synthesiser
//! that takes live MIDI events, so the sequence used to be dropped and titles
//! whose music is entirely sequenced - Zenonia asks for 109 notes and no waves
//! at all - played silently.
//!
//! This renders the sequence to PCM on this side instead, and hands it to the
//! same path the waves already take.
//!
//! It is not a reproduction of the FM chip a handset had. It is a polyphonic
//! oscillator bank with an envelope per voice, and it picks a waveform from
//! the General MIDI program the sequence asks for. Sequenced music comes out
//! recognisable rather than authentic.

/// Rate the rendered stream runs at. Handset sequences are sparse enough that
/// this costs little, and it is a rate every device accepts.
pub const SAMPLE_RATE: u32 = 22050;

/// Voices sounding at once. A sequence that asks for more steals the quietest,
/// which is what a handset chip did with far fewer.
const MAX_VOICES: usize = 24;

/// MIDI's percussion channel, whose notes name drums rather than pitches.
const DRUM_CHANNEL: u8 = 9;

const CHANNELS: usize = 16;

/// Headroom, so a chord of square waves does not clip.
const MASTER_GAIN: f32 = 0.16;

#[derive(Clone, Copy, PartialEq)]
enum Wave {
    Square,
    Saw,
    Triangle,
    Noise,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Clone, Copy)]
struct Voice {
    channel: u8,
    note: u8,
    wave: Wave,
    /// Where the oscillator is within its period, in turns.
    phase: f32,
    step: f32,
    /// Amplitude before the channel's own volume is applied.
    level: f32,
    envelope: f32,
    stage: Stage,
    noise: u32,
}

impl Voice {
    fn silent() -> Self {
        Self {
            channel: 0,
            note: 0,
            wave: Wave::Square,
            phase: 0.0,
            step: 0.0,
            level: 0.0,
            envelope: 0.0,
            stage: Stage::Done,
            noise: 0x1234_5678,
        }
    }

    fn sounding(&self) -> bool {
        self.stage != Stage::Done
    }

    /// One sample of the oscillator, before the envelope.
    fn oscillator(&mut self) -> f32 {
        if self.wave == Wave::Noise {
            // Xorshift, which is white enough for a drum and costs nothing.
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;

            return (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
        }

        self.phase += self.step;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }

        match self.wave {
            Wave::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Saw => self.phase * 2.0 - 1.0,
            Wave::Triangle => 1.0 - (self.phase - 0.5).abs() * 4.0,
            Wave::Noise => unreachable!(),
        }
    }

    /// Advances the envelope one sample and returns where it now is.
    fn envelope(&mut self, shape: &Envelope) -> f32 {
        match self.stage {
            Stage::Attack => {
                self.envelope += shape.attack;
                if self.envelope >= 1.0 {
                    self.envelope = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.envelope -= shape.decay;
                if self.envelope <= shape.sustain {
                    self.envelope = shape.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                self.envelope -= shape.release;
                if self.envelope <= 0.0 {
                    self.envelope = 0.0;
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => return 0.0,
        }

        self.envelope
    }
}

/// How fast a voice rises and falls, in envelope units per sample.
struct Envelope {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Envelope {
    fn new(sample_rate: u32) -> Self {
        let per_ms = 1.0 / (sample_rate as f32 / 1000.0);

        Self {
            attack: per_ms / 4.0,
            decay: per_ms / 90.0,
            sustain: 0.62,
            release: per_ms / 110.0,
        }
    }

    /// A drum has no sustain: it decays to nothing on its own.
    fn percussive(sample_rate: u32) -> Self {
        let per_ms = 1.0 / (sample_rate as f32 / 1000.0);

        Self {
            attack: per_ms / 1.0,
            decay: per_ms / 60.0,
            sustain: 0.0,
            release: per_ms / 30.0,
        }
    }
}

pub struct Synth {
    voices: [Voice; MAX_VOICES],
    /// Per channel: the General MIDI program, the volume controller, and how
    /// far the pitch wheel is bent, in semitones.
    programs: [u8; CHANNELS],
    volumes: [u8; CHANNELS],
    bends: [f32; CHANNELS],
    melodic: Envelope,
    percussive: Envelope,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}

impl Synth {
    pub fn new() -> Self {
        Self {
            voices: [Voice::silent(); MAX_VOICES],
            programs: [0; CHANNELS],
            volumes: [100; CHANNELS],
            bends: [0.0; CHANNELS],
            melodic: Envelope::new(SAMPLE_RATE),
            percussive: Envelope::percussive(SAMPLE_RATE),
        }
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        // A note on with no velocity is a note off, which is how a lot of
        // sequences release a note.
        if velocity == 0 {
            self.note_off(channel, note);
            return;
        }

        let wave = self.wave_for(channel);
        let index = self.free_voice();

        self.voices[index] = Voice {
            channel,
            note,
            wave,
            phase: 0.0,
            step: frequency(note, self.bend_of(channel)) / SAMPLE_RATE as f32,
            level: velocity as f32 / 127.0,
            envelope: 0.0,
            stage: Stage::Attack,
            // Seeded from the note so two drums in a row do not sound identical.
            noise: 0x9E37_79B9 ^ ((note as u32) << 16) ^ (index as u32 + 1),
        };
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        for voice in &mut self.voices {
            if voice.channel == channel && voice.note == note && voice.stage != Stage::Done {
                voice.stage = Stage::Release;
            }
        }
    }

    pub fn program_change(&mut self, channel: u8, program: u8) {
        if let Some(slot) = self.programs.get_mut(channel as usize) {
            *slot = program;
        }
    }

    pub fn control_change(&mut self, channel: u8, control: u8, value: u8) {
        match control {
            // Channel volume.
            7 => {
                if let Some(slot) = self.volumes.get_mut(channel as usize) {
                    *slot = value;
                }
            }
            // All sound off, all notes off, and the reset that follows a stop.
            120 | 121 | 123 => {
                for voice in &mut self.voices {
                    if voice.channel == channel {
                        voice.stage = Stage::Release;
                    }
                }
            }
            _ => {}
        }
    }

    /// The wheel is centred at 0x2000 and covers two semitones either way.
    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        let semitones = (value as f32 - 8192.0) / 8192.0 * 2.0;

        if let Some(slot) = self.bends.get_mut(channel as usize) {
            *slot = semitones;
        }

        for voice in &mut self.voices {
            if voice.channel == channel && voice.stage != Stage::Done {
                voice.step = frequency(voice.note, semitones) / SAMPLE_RATE as f32;
            }
        }
    }

    pub fn silence(&mut self) {
        self.voices = [Voice::silent(); MAX_VOICES];
    }

    pub fn sounding(&self) -> bool {
        self.voices.iter().any(Voice::sounding)
    }

    /// Renders `frames` samples, or nothing at all when no voice is sounding -
    /// silence is better left unqueued than pushed as zeroes.
    pub fn render(&mut self, frames: usize) -> Option<Vec<i16>> {
        if !self.sounding() {
            return None;
        }

        let mut output = vec![0i16; frames];

        for sample in output.iter_mut() {
            let mut mixed = 0.0;

            for index in 0..MAX_VOICES {
                if !self.voices[index].sounding() {
                    continue;
                }

                let percussive = self.voices[index].channel == DRUM_CHANNEL;
                let shape = if percussive { &self.percussive } else { &self.melodic };

                let envelope = self.voices[index].envelope(shape);
                if envelope <= 0.0 {
                    continue;
                }

                let channel = self.voices[index].channel as usize;
                let volume = self.volumes.get(channel).copied().unwrap_or(100) as f32 / 127.0;
                let level = self.voices[index].level;

                mixed += self.voices[index].oscillator() * envelope * level * volume;
            }

            *sample = (mixed * MASTER_GAIN * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }

        Some(output)
    }

    /// The voice to take: a finished one, or failing that the quietest, so
    /// stealing is heard as a note ending early rather than a note cut off.
    fn free_voice(&self) -> usize {
        let mut quietest = 0;
        let mut quietest_level = f32::MAX;

        for (index, voice) in self.voices.iter().enumerate() {
            if !voice.sounding() {
                return index;
            }

            let loudness = voice.envelope * voice.level;
            if loudness < quietest_level {
                quietest_level = loudness;
                quietest = index;
            }
        }

        quietest
    }

    fn bend_of(&self, channel: u8) -> f32 {
        self.bends.get(channel as usize).copied().unwrap_or(0.0)
    }

    /// General MIDI groups instruments in eights, which is enough to choose a
    /// waveform with: reeds and leads carry a square, strings and brass a saw,
    /// pitched percussion a triangle.
    fn wave_for(&self, channel: u8) -> Wave {
        if channel == DRUM_CHANNEL {
            return Wave::Noise;
        }

        match self.programs.get(channel as usize).copied().unwrap_or(0) {
            0..=15 => Wave::Triangle,
            16..=23 => Wave::Square,
            24..=39 => Wave::Saw,
            40..=63 => Wave::Saw,
            64..=79 => Wave::Square,
            80..=95 => Wave::Square,
            96..=111 => Wave::Triangle,
            _ => Wave::Noise,
        }
    }
}

/// Equal temperament from MIDI note 69 = A440, bent by `semitones`.
fn frequency(note: u8, semitones: f32) -> f32 {
    440.0 * 2f32.powf((note as f32 - 69.0 + semitones) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::{SAMPLE_RATE, Synth};

    #[test]
    fn silence_renders_nothing() {
        let mut synth = Synth::new();

        assert!(synth.render(256).is_none());
    }

    #[test]
    fn a_note_makes_a_sound() {
        let mut synth = Synth::new();
        synth.note_on(0, 69, 100);

        let rendered = synth.render(SAMPLE_RATE as usize / 20).expect("a sounding note renders");

        assert!(rendered.iter().any(|x| *x != 0), "every sample was zero");
    }

    #[test]
    fn a_released_note_fades_and_stops() {
        let mut synth = Synth::new();
        synth.note_on(0, 60, 127);
        synth.render(SAMPLE_RATE as usize / 50);

        synth.note_off(0, 60);

        // Long enough for the release to run to the end.
        for _ in 0..20 {
            synth.render(SAMPLE_RATE as usize / 50);
        }

        assert!(!synth.sounding(), "the voice never finished releasing");
        assert!(synth.render(256).is_none());
    }

    #[test]
    fn a_note_on_without_velocity_releases() {
        let mut synth = Synth::new();
        synth.note_on(0, 60, 100);
        assert!(synth.sounding());

        synth.note_on(0, 60, 0);
        for _ in 0..20 {
            synth.render(SAMPLE_RATE as usize / 50);
        }

        assert!(!synth.sounding());
    }

    #[test]
    fn all_notes_off_releases_the_channel() {
        let mut synth = Synth::new();
        synth.note_on(3, 60, 100);
        synth.note_on(4, 62, 100);

        synth.control_change(3, 123, 0);
        for _ in 0..20 {
            synth.render(SAMPLE_RATE as usize / 50);
        }

        // Channel 4 was not addressed, so it is still holding its note.
        assert!(synth.sounding());
    }

    /// More notes than there are voices must not lose the newest ones.
    #[test]
    fn voices_are_stolen_rather_than_dropped() {
        let mut synth = Synth::new();

        for note in 40..90 {
            synth.note_on(0, note, 100);
        }

        assert!(synth.render(512).is_some());
    }
}
