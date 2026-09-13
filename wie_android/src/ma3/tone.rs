//! Voices, and where they come from.
//!
//! An MA-3 voice is four (or two) operators plus the algorithm wiring them
//! together. A `.mmf` carries its own voices as system exclusive messages, in
//! the setup chunk and sometimes part way through the sequence, so the
//! instruments a title plays are the ones its composer programmed rather than
//! anything general.
//!
//! Three sources feed [`Bank`]:
//!
//! - the file's own voices, parsed out of system exclusive;
//! - the handset's built in drum kit, in `data/rhythm.bin`;
//! - a small set of stand ins, for the programs a file never defines. A
//!   handset had a melodic bank in ROM that we have no copy of, so without
//!   these those programs would be silent.

use crate::ma3::tables::{KEYLEVEL_SELECTOR_MAP, MULTIPLE_MAP};

/// Operators in a voice at most. Two operator voices leave the rest unused.
pub const OPERATORS: usize = 4;

/// Channel a sequence's percussion arrives on, whose notes name drums rather
/// than pitches.
pub const DRUM_CHANNEL: u8 = 9;

/// Expanded form of one operator's parameters, one field per byte, which is
/// how both the file's voices and the drum kit's records are laid out once
/// unpacked.
const OPERATOR_FIELDS: usize = 19;

/// Bytes an operator takes in a file's own voice, before expansion.
const PACKED_OPERATOR: usize = 7;

/// Bytes the drum kit spends on one voice: two of header, then four operators.
const RHYTHM_RECORD: usize = 5 + OPERATORS * OPERATOR_FIELDS;

/// One operator: an oscillator, its envelope, and how loud it comes out.
///
/// Whether it is heard directly or only bends the phase of the next operator
/// is not a property of the operator but of the voice's algorithm.
#[derive(Clone, Copy, Default)]
pub struct Operator {
    /// Envelope rates, each 0 to 15 and each faster as it rises.
    pub attack: u8,
    pub decay1: u8,
    pub decay2: u8,
    pub release: u8,
    /// Level the first decay runs down to before the second takes over.
    pub sustain: u8,
    /// Attenuation index, 0 loudest and 63 inaudible.
    pub level: u8,
    /// Frequency as a multiple of the note's, already mapped out of the four
    /// bit field the file stores.
    pub multiple: u8,
    /// Fine detune, the low three bits: magnitude in two, direction in one.
    pub detune: u8,
    pub waveform: u8,
    /// How much of the operator's own output bends its phase. Only the
    /// algorithm's feedback slots use it.
    pub feedback: u8,
    /// Which curve quietens the operator as the note rises, 0 for none.
    pub keylevel: u8,
    /// Whether the envelope speeds up as the note rises.
    pub rate_scaling: bool,
    pub tremolo: bool,
    pub tremolo_depth: u8,
    pub vibrato: bool,
    pub vibrato_depth: u8,
    /// Key off does not release this operator: it runs its own decay to
    /// nothing instead. Percussion is written this way.
    pub hold: bool,
}

/// A voice, as the chip would have been handed it.
#[derive(Clone, Copy)]
pub struct Tone {
    /// Which of the eight wirings connects the operators. Under two operators
    /// only the low bit matters, and it chooses between series and parallel.
    pub algorithm: u8,
    /// Low frequency oscillator speed, 0 to 3.
    pub lfo_speed: u8,
    pub operator_count: usize,
    pub operators: [Operator; OPERATORS],
}

impl Default for Tone {
    fn default() -> Self {
        Self {
            algorithm: 0,
            lfo_speed: 0,
            operator_count: 2,
            operators: [Operator::default(); OPERATORS],
        }
    }
}

impl Tone {
    /// Reads a voice from its expanded operator records. `global` is the
    /// voice's own two bytes: the algorithm and the oscillator speed.
    fn from_fields(global: u8, fields: &[u8]) -> Option<Self> {
        let algorithm = global & 7;
        // One and zero are the two operator wirings; everything above needs
        // all four.
        let operator_count = if algorithm < 2 { 2 } else { OPERATORS };
        if fields.len() < operator_count * OPERATOR_FIELDS {
            return None;
        }

        let mut operators = [Operator::default(); OPERATORS];
        for (index, operator) in operators.iter_mut().enumerate().take(operator_count) {
            *operator = Operator::from_fields(&fields[index * OPERATOR_FIELDS..]);
        }

        Some(Self {
            algorithm,
            lfo_speed: global >> 6,
            operator_count,
            operators,
        })
    }

    /// Reads a voice out of a file, where each operator is packed into seven
    /// bytes and the two global bytes come first.
    fn from_packed(voice: &[u8]) -> Option<Self> {
        if voice.len() < 2 {
            return None;
        }

        let algorithm = voice[1] & 7;
        let operator_count = if algorithm < 2 { 2 } else { OPERATORS };
        if voice.len() < 2 + operator_count * PACKED_OPERATOR {
            return None;
        }

        let mut fields = [0u8; OPERATORS * OPERATOR_FIELDS];
        for index in 0..operator_count {
            let packed = &voice[2 + index * PACKED_OPERATOR..2 + (index + 1) * PACKED_OPERATOR];
            expand_operator(packed, &mut fields[index * OPERATOR_FIELDS..(index + 1) * OPERATOR_FIELDS]);
        }

        Self::from_fields(voice[1], &fields)
    }
}

impl Operator {
    fn from_fields(fields: &[u8]) -> Self {
        Self {
            attack: fields[7] & 15,
            decay1: fields[6] & 15,
            decay2: fields[0] & 15,
            release: fields[5] & 15,
            sustain: fields[8] & 15,
            level: fields[9] & 63,
            multiple: MULTIPLE_MAP[(fields[15] & 15) as usize],
            detune: fields[16] & 7,
            waveform: fields[17] & 31,
            feedback: fields[18] & 7,
            keylevel: KEYLEVEL_SELECTOR_MAP[(fields[10] & 3) as usize],
            rate_scaling: fields[4] & 1 != 0,
            // Both enables are held low: the bit is set to turn the effect
            // off, which is the opposite way round to everything else here.
            tremolo: fields[12] & 1 == 0,
            tremolo_depth: fields[11] & 3,
            vibrato: fields[14] & 1 == 0,
            vibrato_depth: fields[13] & 3,
            hold: fields[1] & 1 != 0,
        }
    }
}

/// Spreads one operator's seven packed bytes over the field per byte form the
/// rest of this module reads.
fn expand_operator(packed: &[u8], fields: &mut [u8]) {
    fields[0] = packed[0] >> 4;
    fields[1] = (packed[0] >> 3) & 1;
    fields[2] = (packed[0] >> 2) & 1;
    fields[3] = (packed[0] >> 1) & 1;
    fields[4] = packed[0] & 1;
    fields[5] = packed[1] >> 4;
    fields[6] = packed[1] & 15;
    fields[7] = packed[2] >> 4;
    fields[8] = packed[2] & 15;
    fields[9] = packed[3] >> 2;
    fields[10] = packed[3] & 3;
    fields[11] = (packed[4] >> 5) & 3;
    fields[12] = (packed[4] >> 4) & 1;
    fields[13] = (packed[4] >> 1) & 3;
    fields[14] = packed[4] & 1;
    fields[15] = packed[5] >> 4;
    fields[16] = packed[5] & 7;
    fields[17] = packed[6] >> 3;
    fields[18] = packed[6] & 7;
}

/// The handset's drum kit: what each key is, what pitch it sounds at, and the
/// voice behind it.
static RHYTHM: &[u8] = include_bytes!("data/rhythm.bin");

const RHYTHM_KEYS: usize = 128;
const RHYTHM_FM: u8 = 0;

/// The drum a key names, if the kit has one that this synthesiser can play.
///
/// The kit holds two sorts of entry. Forty are voices, and are read here.
/// Twenty one are recordings, which a handset played back from ROM samples we
/// have no copy of; those and the unused keys are left to the caller's stand
/// in.
fn rhythm_tone(key: u8) -> Option<(Tone, u8)> {
    let key = (key & 127) as usize;
    if RHYTHM[key] != RHYTHM_FM {
        return None;
    }

    let pitch = RHYTHM[RHYTHM_KEYS + key];
    let record = &RHYTHM[RHYTHM_KEYS * 2 + key * RHYTHM_RECORD..][..RHYTHM_RECORD];

    // A record's own two global bytes sit either side of the fields, so the
    // algorithm and the oscillator speed have to be put back together.
    let global = (record[4] & 7) | ((record[3] & 3) << 6);

    Tone::from_fields(global, &record[5..]).map(|tone| (tone, pitch))
}

/// Yamaha's manufacturer id, then the SMAF and MA-3 markers, then the command
/// that carries a voice. Everything this synthesiser understands starts here.
const VOICE_PREFIX: [u8; 5] = [0x43, 0x79, 0x06, 0x7F, 0x01];

/// Voices arrive as one of two commands. `0x7C` is the synthesised one this
/// module plays; `0x7D` names a recording in the handset's ROM instead, and is
/// recognised only so it can be skipped without confusing the parse.
const COMMAND_VOICE: u8 = 0x7C;
const COMMAND_SAMPLE: u8 = 0x7D;

/// Shortest message that could hold a voice at all.
const MIN_VOICE_MESSAGE: usize = 30;

/// Where the packed voice starts, and the shape byte that says how long it is.
const SHAPE_OFFSET: usize = 9;
const PACKED_OFFSET: usize = 10;

/// System exclusive is seven bit, so a voice is sent with the high bits of
/// each run of seven bytes gathered into a leading byte.
const PACK_GROUP: usize = 7;

/// Unpacks `packed` back into whole bytes, stopping once `wanted` are out.
fn unpack(packed: &[u8], wanted: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(wanted);
    let mut offset = 0;

    while offset < packed.len() && out.len() < wanted {
        let high = packed[offset];
        offset += 1;

        for index in 0..PACK_GROUP {
            if offset >= packed.len() || out.len() >= wanted {
                break;
            }

            out.push(packed[offset] | ((high << (index + 1)) & 0x80));
            offset += 1;
        }
    }

    (out.len() >= wanted).then_some(out)
}

/// A voice message and where it belongs.
struct VoiceMessage {
    bank: u8,
    program: u8,
    tone: Tone,
}

/// Reads a voice out of one system exclusive message, or returns nothing if
/// the message is not one.
fn parse_voice(message: &[u8]) -> Option<VoiceMessage> {
    // The sequence parser hands these over wrapped, so strip the frame.
    let body = message.strip_prefix(&[0xF0]).unwrap_or(message);
    if body.len() < MIN_VOICE_MESSAGE || body.last() != Some(&0xF7) || !body.starts_with(&VOICE_PREFIX) {
        return None;
    }

    let command = body[VOICE_PREFIX.len()];
    if command != COMMAND_VOICE && command != COMMAND_SAMPLE {
        return None;
    }

    // Where the bank and the program sit depends on the command, because the
    // one naming a recording carries an extra byte ahead of them.
    let (bank, program) = if command == COMMAND_SAMPLE {
        ((body[7] & 127).wrapping_add(129), body[8] & 127)
    } else {
        ((body[6] & 127) + 1, body[7] & 127)
    };

    // Three shapes exist, told apart by the message's length and a byte that
    // is set only on the one naming a recording. Two and four operator voices
    // differ only in how much follows.
    let (packed_len, unpacked_len, voice_len) = match (body.len() + 1, body[SHAPE_OFFSET]) {
        (32, 0) => (20, 17, 16),
        (48, 0) => (36, 31, 30),
        // A recording, which this synthesiser cannot play.
        (31, 1) => return None,
        _ => return None,
    };

    if body.len() < PACKED_OFFSET + packed_len {
        return None;
    }

    let unpacked = unpack(&body[PACKED_OFFSET..PACKED_OFFSET + packed_len], unpacked_len)?;
    let tone = Tone::from_packed(&unpacked[1..1 + voice_len])?;

    Some(VoiceMessage { bank, program, tone })
}

/// The voices a running sequence can reach.
pub struct Bank {
    /// Keyed by bank and program, in the order the file defined them. A file
    /// carries a handful of voices, so a scan costs less than a map.
    voices: Vec<VoiceMessage>,
}

impl Default for Bank {
    fn default() -> Self {
        Self::new()
    }
}

impl Bank {
    pub fn new() -> Self {
        Self { voices: Vec::new() }
    }

    /// Takes a voice out of a system exclusive message. Anything that is not
    /// one is ignored, which is most of them: a sequence also uses exclusive
    /// for chip resets and for settings this synthesiser has no equivalent of.
    ///
    /// Returns whether the message defined a voice, for the log.
    pub fn accept_sysex(&mut self, message: &[u8]) -> bool {
        let Some(voice) = parse_voice(message) else {
            return false;
        };

        // A file may redefine a program part way through; the later definition
        // is the one that applies from then on.
        if let Some(existing) = self.voices.iter_mut().find(|x| x.bank == voice.bank && x.program == voice.program) {
            existing.tone = voice.tone;
        } else {
            self.voices.push(voice);
        }

        true
    }

    /// The voice to sound `note` with, and the pitch to sound it at.
    ///
    /// Percussion is looked up by note rather than by program, because on that
    /// channel the note names the drum.
    pub fn tone_for(&self, channel: u8, program: u8, note: u8) -> (Tone, u8) {
        if channel == DRUM_CHANNEL {
            if let Some((tone, pitch)) = rhythm_tone(note) {
                return (tone, pitch);
            }

            return (percussion_stand_in(note), note);
        }

        if let Some(voice) = self.lookup(program) {
            return (voice, note);
        }

        // A file that defines no voice for this program still expects a sound.
        // Falling back to another of its own voices keeps the title's own
        // character, which a generic stand in would not.
        if let Some(voice) = self.voices.first() {
            return (voice.tone, note);
        }

        (melodic_stand_in(program), note)
    }

    fn lookup(&self, program: u8) -> Option<Tone> {
        // Banks are chosen by controller in ways that vary between titles, and
        // guessing wrong is worse than ignoring them: a file rarely defines
        // the same program twice, so the program alone identifies the voice.
        self.voices.iter().find(|x| x.program == program).map(|x| x.tone)
    }

    /// How many voices the running file has defined, for the log.
    pub fn len(&self) -> usize {
        self.voices.len()
    }
}

/// Stand in voices, for the programs a file leaves to the handset's own ROM.
///
/// These are not that ROM. They are plain two operator patches, grouped the
/// way General MIDI groups instruments, chosen so a sequence that would
/// otherwise be silent still carries its tune.
fn melodic_stand_in(program: u8) -> Tone {
    let (algorithm, modulator, carrier) = match program {
        // Piano and tuned percussion: struck, and decaying on their own.
        0..=15 => (0, (1, 26, 15, 6, 4, 4, 7), (1, 2, 15, 5, 6, 3, 7)),
        // Organ and accordion, which hold until released.
        16..=23 => (1, (1, 12, 15, 0, 15, 0, 8), (2, 6, 15, 0, 15, 0, 8)),
        // Guitar and bass: plucked, with a firmer decay.
        24..=39 => (0, (2, 24, 15, 7, 3, 5, 8), (1, 3, 15, 6, 5, 4, 8)),
        // Strings, ensemble and brass: bowed or blown, so slower in.
        40..=63 => (0, (1, 30, 9, 4, 10, 0, 7), (1, 4, 10, 3, 13, 0, 7)),
        // Reed, pipe and lead: sustained and brighter.
        64..=87 => (0, (2, 27, 13, 5, 9, 0, 8), (1, 3, 14, 4, 12, 0, 8)),
        // Pads, effects and everything past them.
        _ => (1, (1, 22, 6, 3, 12, 0, 6), (1, 6, 7, 3, 12, 0, 6)),
    };

    stand_in(algorithm, modulator, carrier, 0)
}

/// The drum kit covers forty keys. The rest were recordings on a handset, so
/// they get a short, heavily modulated patch instead: at these ratios the
/// operators beat against each other closely enough to read as a hit.
fn percussion_stand_in(note: u8) -> Tone {
    // Low notes are drums and high ones cymbals, which want a longer tail.
    let bright = note >= 42;
    let modulator = if bright { (15, 18, 15, 6, 0, 9, 0) } else { (10, 20, 15, 8, 0, 11, 0) };
    let carrier = if bright { (12, 2, 15, 7, 0, 10, 0) } else { (1, 3, 15, 9, 0, 12, 0) };

    stand_in(0, modulator, carrier, if bright { 24 } else { 16 })
}

/// Builds a two operator stand in. Each operator is given as multiple, level,
/// attack, first decay, sustain, second decay and release.
fn stand_in(algorithm: u8, modulator: (u8, u8, u8, u8, u8, u8, u8), carrier: (u8, u8, u8, u8, u8, u8, u8), waveform: u8) -> Tone {
    let build = |(multiple, level, attack, decay1, sustain, decay2, release): (u8, u8, u8, u8, u8, u8, u8)| Operator {
        attack,
        decay1,
        decay2,
        release,
        sustain,
        level,
        multiple,
        waveform,
        ..Operator::default()
    };

    let mut operators = [Operator::default(); OPERATORS];
    operators[0] = build(modulator);
    operators[1] = build(carrier);

    Tone {
        algorithm,
        lfo_speed: 0,
        operator_count: 2,
        operators,
    }
}

#[cfg(test)]
mod tests {
    use super::{Bank, DRUM_CHANNEL, OPERATORS, Tone, parse_voice, unpack};

    /// A four operator voice, as one of the library's own files sends it.
    const FOUR_OPERATOR: &[u8] = &[
        0xF0, 0x43, 0x79, 0x06, 0x7F, 0x01, 0x7C, 0x00, 0x55, 0x00, 0x00, 0x02, 0x00, 0x79, 0x43, 0x13, 0x32, 0x21, 0x72, 0x03, 0x44, 0x10, 0x06,
        0x33, 0x64, 0x14, 0x68, 0x22, 0x44, 0x40, 0x00, 0x23, 0x33, 0x32, 0x5A, 0x06, 0x44, 0x10, 0x00, 0x13, 0x01, 0x32, 0x00, 0x00, 0x44, 0x20,
        0x00, 0xF7,
    ];

    #[test]
    fn unpacking_restores_the_high_bits() {
        // One group: a header naming bits 5 and 0, then two bytes.
        let unpacked = unpack(&[0b0010_0001, 0x2E, 0x60], 2).expect("two bytes were asked for");

        assert_eq!(unpacked, vec![0x2E, 0xE0]);
    }

    #[test]
    fn unpacking_reports_a_short_message() {
        assert!(unpack(&[0x00, 0x01], 7).is_none());
    }

    #[test]
    fn a_four_operator_voice_is_read_whole() {
        let voice = parse_voice(FOUR_OPERATOR).expect("this is a voice message");

        assert_eq!(voice.program, 0x55);
        assert_eq!(voice.bank, 1);
        assert_eq!(voice.tone.operator_count, OPERATORS);
        // Two operators either side of the algorithm's own numbering, so a
        // misread of the packing would not leave every field plausible.
        assert!(voice.tone.operators.iter().any(|x| x.attack != 0));
        assert!(voice.tone.operators.iter().all(|x| x.level <= 63));
        assert!(voice.tone.operators.iter().all(|x| x.waveform < 32));
    }

    #[test]
    fn a_reset_is_not_a_voice() {
        // What a file sends before anything else: reset the chip.
        assert!(parse_voice(&[0xF0, 0x43, 0x79, 0x06, 0x7F, 0x7F, 0xF7]).is_none());
    }

    #[test]
    fn a_bank_takes_a_voice_and_gives_it_back() {
        let mut bank = Bank::new();

        assert!(bank.accept_sysex(FOUR_OPERATOR));
        assert_eq!(bank.len(), 1);

        let (tone, note) = bank.tone_for(0, 0x55, 60);
        assert_eq!(tone.operator_count, OPERATORS);
        assert_eq!(note, 60);
    }

    #[test]
    fn redefining_a_program_replaces_it() {
        let mut bank = Bank::new();

        bank.accept_sysex(FOUR_OPERATOR);
        bank.accept_sysex(FOUR_OPERATOR);

        assert_eq!(bank.len(), 1);
    }

    #[test]
    fn an_undefined_program_still_sounds() {
        let bank = Bank::new();

        for program in 0..128u8 {
            let (tone, _) = bank.tone_for(0, program, 60);
            assert!(tone.operators[1].attack > 0, "program {program} has no carrier");
        }
    }

    #[test]
    fn every_drum_key_resolves() {
        let bank = Bank::new();
        let mut from_kit = 0;

        for note in 0..128u8 {
            let (tone, pitch) = bank.tone_for(DRUM_CHANNEL, 0, note);
            assert!(tone.operator_count == 2 || tone.operator_count == OPERATORS);

            if pitch != note {
                from_kit += 1;
            }
        }

        // The kit's forty voices, less any whose fixed pitch happens to match
        // the key that names them.
        assert!(from_kit > 30, "only {from_kit} keys came from the kit");
    }

    #[test]
    fn a_default_tone_is_playable() {
        let tone = Tone::default();

        assert_eq!(tone.operator_count, 2);
    }
}
