use std::{
    io::Cursor,
    sync::{Arc, Mutex, OnceLock},
};

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass},
    sys::{JNI_FALSE, JNI_TRUE, jbyteArray},
};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

const SAMPLE_RATE: i32 = 44_100;
const SOUND_FONT: &[u8] = include_bytes!("../soundfont.sf2");

struct MidiState {
    synthesizer: Synthesizer,
    started: bool,
}

static STATE: OnceLock<Mutex<Option<MidiState>>> = OnceLock::new();

fn state() -> &'static Mutex<Option<MidiState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

fn initialize() -> Option<MidiState> {
    let mut reader = Cursor::new(SOUND_FONT);
    let sound_font = Arc::new(SoundFont::new(&mut reader).ok()?);
    let settings = SynthesizerSettings::new(SAMPLE_RATE);
    let synthesizer = Synthesizer::new(&sound_font, &settings).ok()?;
    Some(MidiState { synthesizer, started: false })
}

fn render_block(state: &mut MidiState, milliseconds: i32) -> Option<Vec<u8>> {
    if !state.started {
        return None;
    }

    let milliseconds = milliseconds.clamp(5, 50) as usize;
    let frame_count = (milliseconds * SAMPLE_RATE as usize) / 1_000;
    if frame_count == 0 {
        return None;
    }

    let mut left = vec![0.0f32; frame_count];
    let mut right = vec![0.0f32; frame_count];
    state.synthesizer.render(&mut left, &mut right);

    let sample_count = frame_count * 2;
    let mut output = Vec::with_capacity(10 + sample_count * 2);
    output.push(2); // Existing AndroidAudioOutput streaming PCM opcode.
    output.push(2); // Stereo.
    output.extend_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    output.extend_from_slice(&(sample_count as u32).to_le_bytes());
    for (left, right) in left.into_iter().zip(right) {
        output.extend_from_slice(&float_to_pcm16(left * 3.0).to_le_bytes());
        output.extend_from_slice(&float_to_pcm16(right * 3.0).to_le_bytes());
    }
    Some(output)
}

fn apply_event(synthesizer: &mut Synthesizer, command: &[u8]) {
    let Some(&opcode) = command.first() else {
        return;
    };
    match opcode {
        2 if command.len() >= 4 => {
            let velocity = ((command[3] as i32 * 3) / 2).min(127);
            synthesizer.note_on(command[1] as i32, command[2] as i32, velocity)
        }
        3 if command.len() >= 4 => synthesizer.note_off(command[1] as i32, command[2] as i32),
        4 if command.len() >= 3 => synthesizer.process_midi_message(command[1] as i32, 0xC0, command[2] as i32, 0),
        5 if command.len() >= 4 => synthesizer.process_midi_message(command[1] as i32, 0xB0, command[2] as i32, command[3] as i32),
        6 if command.len() >= 4 => {
            let value = u16::from_le_bytes([command[2], command[3]]);
            synthesizer.process_midi_message(command[1] as i32, 0xE0, (value & 0x7F) as i32, ((value >> 7) & 0x7F) as i32);
        }
        _ => {}
    }
}

fn float_to_pcm16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jjongjjongs_wiemobile_MidiSynthBridge_nativeHandle(env: JNIEnv, _class: JClass, command: JByteArray) -> jbyteArray {
    let Ok(command) = env.convert_byte_array(command) else {
        return std::ptr::null_mut();
    };

    let mut guard = state().lock().unwrap();
    if guard.is_none() {
        *guard = initialize();
    }
    let Some(state) = guard.as_mut() else {
        return std::ptr::null_mut();
    };

    apply_event(&mut state.synthesizer, &command);
    state.started = true;

    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jjongjjongs_wiemobile_MidiSynthBridge_nativeRender(env: JNIEnv, _class: JClass, milliseconds: i32) -> jbyteArray {
    let mut guard = state().lock().unwrap();
    let Some(state) = guard.as_mut() else {
        return std::ptr::null_mut();
    };

    match render_block(state, milliseconds).and_then(|data| env.byte_array_from_slice(&data).ok()) {
        Some(array) => array.into_raw(),
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jjongjjongs_wiemobile_MidiSynthBridge_nativeReset(_env: JNIEnv, _class: JClass) {
    *state().lock().unwrap() = None;
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jjongjjongs_wiemobile_MidiSynthBridge_nativeAvailable(_env: JNIEnv, _class: JClass) -> u8 {
    if initialize().is_some() { JNI_TRUE } else { JNI_FALSE }
}
