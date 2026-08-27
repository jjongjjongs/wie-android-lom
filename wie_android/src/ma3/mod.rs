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
//! The other half of a handset lived in ROM as recordings. The drums behind
//! twenty one of the kit's keys are those recordings, decoded and played back
//! by [`wave`]; the melodic bank a file falls back on when it defines no voice
//! of its own is still a stand in, marked as such where it is built.
//!
//! - [`tone`] reads voices out of the file's system exclusive.
//! - [`voice`] is one synthesised note.
//! - [`wave`] plays a note back from one of the handset's ROM recordings.
//! - [`bus`] is the output stage, which attenuates rather than multiplies.
//! - [`tables`] is the chip's own data.

mod bus;
pub(crate) mod tables;
mod tone;
mod voice;
mod wave;

use std::collections::BTreeMap;

use crate::ma3::{
    bus::{clamp_i16, mix_q15, saturate_pcm, soft_limit, stereo_gain_q15},
    tone::{Bank, DRUM_CHANNEL},
    voice::Voice,
    wave::WaveVoice,
};

/// Where a slot's sound comes from: a synthesised voice, or a recording the
/// chip played back. Both hand the mixer the same normalised mono sample, so
/// the rest of the synthesiser does not care which a note turned out to be.
enum Source {
    Fm(Voice),
    Pcm(WaveVoice),
}

impl Source {
    fn sample(&mut self) -> f64 {
        match self {
            Source::Fm(voice) => voice.sample(),
            Source::Pcm(voice) => voice.sample(),
        }
    }

    fn audible(&self) -> bool {
        match self {
            Source::Fm(voice) => voice.audible(),
            Source::Pcm(voice) => voice.audible(),
        }
    }

    fn loudness(&self) -> f64 {
        match self {
            Source::Fm(voice) => voice.loudness(),
            Source::Pcm(voice) => voice.loudness(),
        }
    }

    fn release(&mut self) {
        match self {
            Source::Fm(voice) => voice.release(),
            Source::Pcm(voice) => voice.release(),
        }
    }

    fn all_sound_off(&mut self) {
        match self {
            Source::Fm(voice) => voice.all_sound_off(),
            Source::Pcm(voice) => voice.all_sound_off(),
        }
    }

    /// The wheel only bends a synthesised voice; a recording carries its own
    /// pitch and ignores it.
    fn set_modulation(&mut self, depth: usize) {
        if let Source::Fm(voice) = self {
            voice.set_modulation(depth);
        }
    }

    fn set_pitch(&mut self, note: u8, bend: u16, bend_range: u8, sample_rate: u32) {
        if let Source::Fm(voice) = self {
            voice.set_pitch(note, bend, bend_range, sample_rate);
        }
    }
}

/// Rate the rendered stream runs at. FM puts out harmonics well above the
/// note, so a rate this side of the chip's own would fold them back audibly.
pub const SAMPLE_RATE: u32 = 44100;

/// Channels in the rendered stream. A sequence pans its parts, and the chip's
/// output stage is stereo, so folding it down would throw that away.
pub const CHANNELS: usize = 2;

/// Notes at once. The reference renderer runs a single file up to thirty one
/// simultaneous voices - its own logs show rich sequences peaking there - so a
/// lower cap steals notes the reference keeps, thinning the music away from how
/// it should sound. Matched to that ceiling.
const MAX_VOICES: usize = 31;

/// Source samples over which a recorded effect is ramped to silence at its end.
/// The recordings stop at whatever level the last sample held rather than
/// decaying, so cutting straight to zero clicks on every tail; a few
/// milliseconds of fade lands them quietly instead. Only the tail is ramped -
/// the attack is left untouched so a hit stays sharp.
const WAVE_FADE_OUT_SAMPLES: usize = 64;

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
    source: Source,
    left_q15: i32,
    right_q15: i32,
}

pub struct Synth {
    channels: [Channel; MIDI_CHANNELS],
    voices: Vec<Slot>,
    bank: Bank,
    master_volume: u8,
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
            master_volume: 127,
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

        // Percussion the handset recorded is played back rather than
        // synthesised; everything else is a voice, whether the file's own or a
        // stand in.
        let source = if channel == DRUM_CHANNEL {
            match WaveVoice::drum(note, velocity, SAMPLE_RATE) {
                Some(voice) => Source::Pcm(voice),
                None => Source::Fm(self.fm_voice(channel, &state, note)),
            }
        } else {
            Source::Fm(self.fm_voice(channel, &state, note))
        };

        let (left_q15, right_q15) = gains(&state, velocity, self.master_volume);
        let slot = Slot {
            channel,
            note,
            velocity,
            source,
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
            .min_by(|(_, left), (_, right)| left.source.loudness().total_cmp(&right.source.loudness()))
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.voices[quietest] = slot;
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        for slot in &mut self.voices {
            if slot.channel == channel && slot.note == note {
                slot.source.release();
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
                    slot.source.set_modulation(depth);
                }
                return;
            }
            CONTROL_VOLUME => state.volume = value,
            CONTROL_PAN => state.pan = value,
            CONTROL_EXPRESSION => state.expression = value,
            CONTROL_ALL_SOUND_OFF => {
                for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
                    slot.source.all_sound_off();
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
                    slot.source.release();
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
            slot.source.set_pitch(slot.note, state.bend, state.bend_range, SAMPLE_RATE);
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

    pub fn set_master_volume(&mut self, volume: u8) {
        self.master_volume = ((u16::from(volume.min(100)) * 127) / 100) as u8;

        for channel in 0..MIDI_CHANNELS {
            self.refresh_gains(channel as u8);
        }
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
                let sample = slot.source.sample();
                left = mix_q15(left, sample, slot.left_q15);
                right = mix_q15(right, sample, slot.right_q15);
            }

            // A voice that has fallen silent gives its slot back, whether it
            // was released or simply ran out of envelope, which is how
            // percussion ends.
            self.voices.retain(|x| x.source.audible());

            frame[0] = clamp_i16(left as i64) as i16;
            frame[1] = clamp_i16(right as i64) as i16;
        }

        Some(output)
    }

    /// Builds a synthesised voice for a note, taking its tone from the file's
    /// bank or a stand in.
    fn fm_voice(&self, channel: u8, state: &Channel, note: u8) -> Voice {
        let (tone, pitch) = self.bank.tone_for(channel, state.program, note);
        Voice::new(
            &tone,
            pitch,
            state.bend,
            state.bend_range,
            modulation_depth(state.modulation),
            SAMPLE_RATE,
        )
    }

    fn channel(&mut self, channel: u8) -> Option<&mut Channel> {
        self.channels.get_mut(channel as usize)
    }

    fn refresh_gains(&mut self, channel: u8) {
        let Some(state) = self.channels.get(channel as usize).copied() else {
            return;
        };
        let master_volume = self.master_volume;

        for slot in self.voices.iter_mut().filter(|x| x.channel == channel) {
            let (left, right) = gains(&state, slot.velocity, master_volume);
            slot.left_q15 = left;
            slot.right_q15 = right;
        }
    }
}

fn gains(state: &Channel, velocity: u8, master_volume: u8) -> (i32, i32) {
    stereo_gain_q15(state.volume, state.expression, velocity, master_volume, state.pan)
}

/// A set of independent [`Synth`] voices, one per playing clip, summed to one
/// output.
///
/// A title plays several clips at once - a looping background track under short
/// effects - and each is its own sequence on its own channels, every file
/// numbering channels from zero. Rendered through one shared synth they collide:
/// an effect's program change overwrites the track's instrument on the same
/// channel, and each clip's end-of-sequence "all notes off" silences whatever
/// else is sounding there. The reference avoids this by giving every clip its
/// own renderer and mixing the results; this does the same. `open` hands a clip
/// an isolated voice, its events go only to that voice, and `render` sums them.
pub struct SynthMixer {
    voices: BTreeMap<u32, MixerVoice>,
    next_id: u32,
    master_volume: u8,
    /// One-shot recorded waves (a hit, a door, the logo voice), mixed into the
    /// same output stream as the sequenced voices. Titles fire these through the
    /// wave path; routing them here plays them on the one AudioTrack that works,
    /// rather than a per-clip static track the device would not sound.
    pcm: Vec<PcmOneShot>,
    /// A whole `.mmf` rendered up front by the faithful [`crate::oma3`] port and
    /// mixed in here as one interleaved-stereo stream, so background music sounds
    /// exactly as the reference plays it instead of being re-sequenced live.
    song: Option<SongPlayback>,
}

/// A pre-rendered song mixed into the output, optionally looping.
struct SongPlayback {
    /// Interleaved stereo at the output rate, as [`crate::oma3`] renders it.
    samples: Vec<i16>,
    /// Read position into `samples`.
    position: usize,
    repeat: bool,
}

struct MixerVoice {
    synth: Synth,
    /// The clip has stopped feeding this voice; keep rendering until its sound
    /// decays (release tails finished), then `render` drops it, so a stop does
    /// not cut the tail.
    closing: bool,
}

/// A recorded wave playing back into the mix, resampled from its own rate to the
/// output rate on the fly.
struct PcmOneShot {
    samples: Vec<i16>,
    /// Read position in `samples`, in source samples.
    position: f64,
    /// Source samples advanced per output frame (source_rate / output_rate).
    step: f64,
}

impl PcmOneShot {
    /// Advances one output frame and returns the (linearly interpolated) mono
    /// sample, or `None` once the wave has played out.
    fn next_sample(&mut self) -> Option<i16> {
        let index = self.position as usize;
        if index >= self.samples.len() {
            return None;
        }
        let current = self.samples[index] as f64;
        let next = self.samples.get(index + 1).copied().unwrap_or(self.samples[index]) as f64;
        let frac = self.position - index as f64;
        let value = current + (next - current) * frac;
        self.position += self.step;

        // Ramp the last few samples down to zero so the effect does not cut off
        // at a non-zero level and click; everything before the tail is untouched.
        let remaining = self.samples.len() - 1 - index;
        let value = if remaining < WAVE_FADE_OUT_SAMPLES {
            value * remaining as f64 / WAVE_FADE_OUT_SAMPLES as f64
        } else {
            value
        };

        Some(value as i16)
    }
}

impl Default for SynthMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl SynthMixer {
    pub fn new() -> Self {
        Self {
            voices: BTreeMap::new(),
            next_id: 1,
            master_volume: 100,
            pcm: Vec::new(),
            song: None,
        }
    }

    /// Installs a pre-rendered song as the music stream, replacing any previous
    /// one. `samples` is interleaved stereo at [`SAMPLE_RATE`].
    pub fn set_song(&mut self, samples: Vec<i16>, repeat: bool) {
        self.song = Some(SongPlayback {
            samples,
            position: 0,
            repeat,
        });
    }

    /// Stops the pre-rendered song, if one is playing.
    pub fn stop_song(&mut self) {
        self.song = None;
    }

    /// Queues a recorded wave to play into the mix, resampled from `source_rate`
    /// to the output rate.
    pub fn push_pcm(&mut self, samples: Vec<i16>, source_rate: u32) {
        if samples.is_empty() || source_rate == 0 {
            return;
        }
        let step = f64::from(source_rate) / f64::from(SAMPLE_RATE);
        self.pcm.push(PcmOneShot {
            samples,
            position: 0.0,
            step,
        });
    }

    /// Opens an isolated voice and returns its id. Id 0 is never handed out, so
    /// a caller can use it as "no voice".
    pub fn open(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut synth = Synth::new();
        synth.set_master_volume(self.master_volume);
        self.voices.insert(id, MixerVoice { synth, closing: false });
        id
    }

    /// Marks a voice's clip as finished; the voice keeps rendering until its
    /// sound decays, then `render` drops it.
    pub fn close(&mut self, voice: u32) {
        if let Some(entry) = self.voices.get_mut(&voice) {
            entry.closing = true;
        }
    }

    fn synth(&mut self, voice: u32) -> Option<&mut Synth> {
        self.voices.get_mut(&voice).map(|entry| &mut entry.synth)
    }

    pub fn note_on(&mut self, voice: u32, channel: u8, note: u8, velocity: u8) {
        if let Some(synth) = self.synth(voice) {
            synth.note_on(channel, note, velocity);
        }
    }

    pub fn note_off(&mut self, voice: u32, channel: u8, note: u8) {
        if let Some(synth) = self.synth(voice) {
            synth.note_off(channel, note);
        }
    }

    pub fn program_change(&mut self, voice: u32, channel: u8, program: u8) {
        if let Some(synth) = self.synth(voice) {
            synth.program_change(channel, program);
        }
    }

    pub fn control_change(&mut self, voice: u32, channel: u8, control: u8, value: u8) {
        if let Some(synth) = self.synth(voice) {
            synth.control_change(channel, control, value);
        }
    }

    pub fn pitch_bend(&mut self, voice: u32, channel: u8, value: u16) {
        if let Some(synth) = self.synth(voice) {
            synth.pitch_bend(channel, value);
        }
    }

    pub fn sysex(&mut self, voice: u32, message: &[u8]) {
        if let Some(synth) = self.synth(voice) {
            synth.sysex(message);
        }
    }

    pub fn set_master_volume(&mut self, volume: u8) {
        self.master_volume = volume;
        for entry in self.voices.values_mut() {
            entry.synth.set_master_volume(volume);
        }
    }

    /// Stops every voice at once (the game paused or gone).
    pub fn silence(&mut self) {
        self.voices.clear();
        self.pcm.clear();
    }

    /// Renders `frames` from every voice and sums them, dropping any closed
    /// voice that has fallen silent. Returns `None` when nothing sounded, so the
    /// caller queues no chunk - the same contract as [`Synth::render`].
    pub fn render(&mut self, frames: usize) -> Option<Vec<i16>> {
        let mut accumulator: Option<Vec<i32>> = None;
        let mut finished: Vec<u32> = Vec::new();
        let mut active_voices = 0usize;

        for (id, entry) in &mut self.voices {
            match entry.synth.render(frames) {
                Some(samples) => {
                    active_voices += 1;
                    let acc = accumulator.get_or_insert_with(|| vec![0i32; frames * CHANNELS]);
                    for (slot, sample) in acc.iter_mut().zip(samples.iter()) {
                        *slot += i32::from(*sample);
                    }
                }
                None => {
                    if entry.closing {
                        finished.push(*id);
                    }
                }
            }
        }

        for id in finished {
            self.voices.remove(&id);
        }

        // Peak of everything the sequenced voices produced, measured before the
        // recorded waves are added, so a diagnostic can tell whether a wave is
        // being buried by loud concurrent music in the single summed stream.
        let synth_peak = accumulator
            .as_ref()
            .map(|acc| acc.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0))
            .unwrap_or(0);

        // Mix the one-shot recorded waves (mono) into both output channels,
        // resampled to the output rate, and drop any that have played out.
        let pcm_active = self.pcm.len();
        let mut pcm_peak = 0u32;
        if !self.pcm.is_empty() {
            let acc = accumulator.get_or_insert_with(|| vec![0i32; frames * CHANNELS]);
            self.pcm.retain_mut(|wave| {
                for frame in acc.chunks_exact_mut(CHANNELS) {
                    match wave.next_sample() {
                        Some(sample) => {
                            let sample = saturate_pcm(i32::from(sample));
                            pcm_peak = pcm_peak.max(sample.unsigned_abs());
                            for slot in frame.iter_mut() {
                                *slot += sample;
                            }
                        }
                        None => return false,
                    }
                }
                true
            });
        }

        // Mix the pre-rendered song, scaled by the master volume, looping or
        // finishing at its end.
        let master_volume = i32::from(self.master_volume);
        let mut song_ended = false;
        if let Some(song) = &mut self.song {
            // A one-shot that has already played out contributes nothing and
            // must not force an otherwise-silent buffer into existence.
            let exhausted = (song.samples.is_empty() || !song.repeat) && song.position >= song.samples.len();
            if exhausted {
                song_ended = true;
            } else {
                let acc = accumulator.get_or_insert_with(|| vec![0i32; frames * CHANNELS]);
                for slot in acc.iter_mut() {
                    if song.position >= song.samples.len() {
                        if song.repeat && !song.samples.is_empty() {
                            song.position = 0;
                        } else {
                            song_ended = true;
                            break;
                        }
                    }
                    *slot += i32::from(song.samples[song.position]) * master_volume / 100;
                    song.position += 1;
                }
            }
        }
        if song_ended {
            self.song = None;
        }

        accumulator.map(|acc| {
            let total_peak = acc.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            // Past full scale the limiter rounds the peak off rather than
            // clipping it; the log notes when it had to so the lift can be read
            // against how often it reaches there.
            let limited = total_peak > i16::MAX as u32;
            if pcm_active > 0 {
                tracing::info!(
                    "[mix] pcm={pcm_active} pcm_peak={pcm_peak} voices={active_voices} synth_peak={synth_peak} total_peak={total_peak} limited={limited}"
                );
            }
            acc.iter().map(|sample| soft_limit(i64::from(*sample)) as i16).collect()
        })
    }
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
            SmafEvent::MidiNoteOn { channel, note, velocity } => {
                if std::env::var("WIE_DUMP_NOTES").is_ok() {
                    println!("[note] t={time} ch={channel} note={note} vel={velocity}");
                }
                synth.note_on(channel, note, velocity)
            }
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
    use super::{CHANNELS, Channel, MAX_VOICES, SAMPLE_RATE, Synth, SynthMixer, modulation_depth};

    #[test]
    fn song_mixes_then_finishes() {
        let mut mixer = SynthMixer::new();
        // Two stereo frames.
        mixer.set_song(vec![100, 100, 200, 200], false);
        // One frame at a time: the song plays out, then the mixer goes silent.
        let first = mixer.render(1).expect("song frame");
        assert_eq!(first, vec![100, 100]);
        let second = mixer.render(1).expect("song frame");
        assert_eq!(second, vec![200, 200]);
        assert!(mixer.render(1).is_none(), "a one-shot song stops at its end");
    }

    #[test]
    fn song_loops_when_repeating() {
        let mut mixer = SynthMixer::new();
        mixer.set_song(vec![100, 100], true);
        assert_eq!(mixer.render(1), Some(vec![100, 100]));
        assert_eq!(mixer.render(1), Some(vec![100, 100]), "a repeating song wraps");
        mixer.stop_song();
        assert!(mixer.render(1).is_none(), "stopping clears the song");
    }

    #[test]
    fn song_scales_by_master_volume() {
        let mut mixer = SynthMixer::new();
        mixer.set_master_volume(50);
        mixer.set_song(vec![100, 100], false);
        assert_eq!(mixer.render(1), Some(vec![50, 50]));
    }

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
    fn master_volume_scales_wipi_range_without_rewriting_channel_volume() {
        let mut muted = Synth::new();
        muted.set_master_volume(0);
        muted.note_on(0, 69, 100);
        let muted_audio = run(&mut muted, 20);
        assert!(muted_audio.iter().all(|x| *x == 0));

        let mut quiet = Synth::new();
        quiet.set_master_volume(50);
        quiet.note_on(0, 69, 100);
        let quiet_audio = run(&mut quiet, 20);

        let mut loud = Synth::new();
        loud.set_master_volume(100);
        loud.note_on(0, 69, 100);
        let loud_audio = run(&mut loud, 20);

        let quiet_peak = quiet_audio.iter().map(|x| x.unsigned_abs()).max().unwrap_or(0);
        let loud_peak = loud_audio.iter().map(|x| x.unsigned_abs()).max().unwrap_or(0);

        assert!(quiet_peak > 0);
        assert!(quiet_peak < loud_peak);
        assert_eq!(quiet.channels[0].volume, Channel::default().volume);
        assert_eq!(loud.channels[0].volume, Channel::default().volume);
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
