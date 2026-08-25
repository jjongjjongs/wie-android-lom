//! JNI bridge behind `com.jjongjjongs.wiemobile.NativeBridge`.
//!
//! Java owns the loop: a single background thread calls [`nativeStart`] once
//! and then [`nativeTick`] every 16ms, collecting rendered frames with
//! [`nativeFrame`] and queued audio with [`nativePollOutput`]. Input arrives
//! separately, on the UI thread, through [`nativeKey`].
//!
//! Every entry point catches panics: an unwind across the JNI boundary is
//! undefined behaviour, and an emulator panic should surface in the player as
//! a message rather than take the process down.

mod audio;
mod database;
mod filesystem;
mod logging;
mod ma3;
mod network;
mod platform;
mod runner;

use std::{panic::AssertUnwindSafe, path::PathBuf, time::Duration};

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JShortArray, JString},
    sys::{jbyteArray, jint, jshortArray, jstring},
};

use crate::runner::with_runner;

/// Upper bound on a single `nativeTick` call, whatever Java asks for. The tick
/// thread also drives input and frame delivery, so letting it run away would
/// stall both.
const MAX_TICK_BUDGET: Duration = Duration::from_millis(200);

/// Returns an empty Java string. Used when a call fails so badly that we
/// cannot even build the error message.
fn empty_string(env: &JNIEnv) -> jstring {
    env.new_string("").map(|x| x.into_raw()).unwrap_or(std::ptr::null_mut())
}

fn to_java_string(env: &JNIEnv, value: &str) -> jstring {
    match env.new_string(value) {
        Ok(value) => value.into_raw(),
        Err(error) => {
            tracing::error!("Failed to allocate Java string: {error}");
            std::ptr::null_mut()
        }
    }
}

/// Runs `f`, converting a panic into a message rather than unwinding into the
/// JVM.
fn guard_string(env: &JNIEnv, f: impl FnOnce() -> String) -> jstring {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => to_java_string(env, &value),
        Err(_) => {
            let message = "에뮬레이터 내부 오류가 발생했습니다.";
            tracing::error!("{message}");

            with_runner(|runner| runner.stop());

            to_java_string(env, message)
        }
    }
}

fn guard(f: impl FnOnce()) {
    if std::panic::catch_unwind(AssertUnwindSafe(f)).is_err() {
        tracing::error!("Panic caught at the JNI boundary");

        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| with_runner(|runner| runner.stop())));
    }
}

/// `nativeStart(byte[] archive, String runtimeDir) -> String`
///
/// Loads `archive` and starts the emulator. Returns an empty string on
/// success, otherwise the message to show in the player.
///
/// # Safety
/// Called by the JVM with valid `env`, `archive` and `runtime_dir` references.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeStart(
    mut env: JNIEnv,
    _class: JClass,
    archive: JByteArray,
    runtime_dir: JString,
) -> jstring {
    logging::init();
    // The saved log covers one run of one game, so it starts over here.
    logging::reset();

    let data = match env.convert_byte_array(&archive) {
        Ok(data) => data,
        Err(error) => return to_java_string(&env, &format!("게임 파일을 읽을 수 없습니다: {error}")),
    };

    let runtime_dir: String = match env.get_string(&runtime_dir) {
        Ok(value) => value.into(),
        Err(error) => return to_java_string(&env, &format!("저장 경로를 읽을 수 없습니다: {error}")),
    };

    tracing::info!("nativeStart: {} bytes, runtime dir {runtime_dir}", data.len());

    guard_string(&env, || with_runner(|runner| runner.start(data, PathBuf::from(runtime_dir))))
}

/// `nativeTick(int budgetMs) -> String`
///
/// Runs the emulator for up to `budget_ms`. Returns an empty string while the
/// game is healthy, otherwise the message that stopped it.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeTick(env: JNIEnv, _class: JClass, budget_ms: jint) -> jstring {
    let budget = Duration::from_millis(budget_ms.clamp(0, MAX_TICK_BUDGET.as_millis() as jint) as u64);

    guard_string(&env, || with_runner(|runner| runner.tick(budget)))
}

/// `nativeStop()`
///
/// Tears the emulator down. Safe to call when nothing is running.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeStop(_env: JNIEnv, _class: JClass) {
    tracing::info!("nativeStop");

    guard(|| with_runner(|runner| runner.stop()));
}

/// `nativeRunning() -> int`
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeRunning(_env: JNIEnv, _class: JClass) -> jint {
    std::panic::catch_unwind(AssertUnwindSafe(|| with_runner(|runner| runner.is_running())))
        .unwrap_or(false)
        .into()
}

/// `nativeLastError() -> String`
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeLastError(env: JNIEnv, _class: JClass) -> jstring {
    guard_string(&env, || with_runner(|runner| runner.last_error()))
}

/// `nativeKey(int index, int pressed)`
///
/// Called from the UI thread; the event is queued and applied on the next
/// tick.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeKey(_env: JNIEnv, _class: JClass, index: jint, pressed: jint) {
    guard(|| with_runner(|runner| runner.key(index, pressed != 0)));
}

/// `nativeFrame() -> short[]`
///
/// Returns `null` when nothing new has been painted, otherwise
/// `{width, height, RGB565 pixels...}`.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeFrame(env: JNIEnv, _class: JClass) -> jshortArray {
    let frame = match std::panic::catch_unwind(AssertUnwindSafe(|| with_runner(|runner| runner.take_frame()))) {
        Ok(Some(frame)) => frame,
        Ok(None) => return std::ptr::null_mut(),
        Err(_) => {
            tracing::error!("Panic while collecting a frame");
            return std::ptr::null_mut();
        }
    };

    let length = frame.pixels.len() + 2;
    let array = match env.new_short_array(length as jint) {
        Ok(array) => array,
        Err(error) => {
            tracing::error!("Failed to allocate a {length} short frame: {error}");
            return std::ptr::null_mut();
        }
    };

    let header = [frame.width as i16, frame.height as i16];
    if let Err(error) = env.set_short_array_region(&array, 0, &header) {
        tracing::error!("Failed to write the frame header: {error}");
        return std::ptr::null_mut();
    }

    if let Err(error) = env.set_short_array_region(&array, 2, &frame.pixels) {
        tracing::error!("Failed to write RGB565 frame pixels: {error}");
        return std::ptr::null_mut();
    }

    JShortArray::into_raw(array)
}

/// `nativePollOutput() -> byte[]`
///
/// Returns the next queued audio or vibration command, or `null` when the
/// queue is empty. See [`audio`] for the encoding.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativePollOutput(env: JNIEnv, _class: JClass) -> jbyteArray {
    let command = match std::panic::catch_unwind(AssertUnwindSafe(|| with_runner(|runner| runner.take_audio()))) {
        Ok(Some(command)) => command,
        Ok(None) => return std::ptr::null_mut(),
        Err(_) => {
            tracing::error!("Panic while collecting audio output");
            return std::ptr::null_mut();
        }
    };

    match env.byte_array_from_slice(&command) {
        Ok(array) => JByteArray::into_raw(array),
        Err(error) => {
            tracing::error!("Failed to allocate an audio command: {error}");
            std::ptr::null_mut()
        }
    }
}

/// `nativePollBacklightMode() -> int`
///
/// Returns zero when there is no pending change. Non-zero values are handset
/// backlight modes supplied by the emulated runtime.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativePollBacklightMode(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        with_runner(|runner| runner.take_backlight_mode())
    }))
    .unwrap_or(0) as jint
}

/// `nativeInspect(byte[] archive) -> String`
///
/// Describes an archive without running it.
///
/// # Safety
/// Called by the JVM with valid `env` and `archive` references.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeInspect(env: JNIEnv, _class: JClass, archive: JByteArray) -> jstring {
    logging::init();

    let Ok(data) = env.convert_byte_array(&archive) else {
        return empty_string(&env);
    };

    guard_string(&env, || runner::inspect(&data))
}

/// `nativeSaveIds(byte[] archive) -> String`
///
/// The two directory names, newline separated, that an archive's saved data
/// sits under inside the runtime directory: the record store id first, then the
/// filesystem id. Empty when the archive cannot be read at all.
///
/// Java needs these to collect a title's saves without the emulator running,
/// and only the loader knows how an archive names itself.
///
/// # Safety
/// Called by the JVM with valid `env` and `archive` references.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeSaveIds(env: JNIEnv, _class: JClass, archive: JByteArray) -> jstring {
    logging::init();

    let Ok(data) = env.convert_byte_array(&archive) else {
        return empty_string(&env);
    };

    guard_string(&env, || match runner::save_ids(&data) {
        Some(ids) => format!("{}\n{}", ids.records, ids.files),
        None => String::new(),
    })
}

/// `nativeLog() -> String`
///
/// Everything logged since the running game started, which is what the
/// player's log button saves. Reading it does not clear it, so it can be taken
/// more than once during a long run.
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeLog(env: JNIEnv, _class: JClass) -> jstring {
    logging::init();

    guard_string(&env, logging::snapshot)
}

/// `nativeVersion() -> String`
///
/// # Safety
/// Called by the JVM with a valid `env` reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_jjongjjongs_wiemobile_NativeBridge_nativeVersion(env: JNIEnv, _class: JClass) -> jstring {
    to_java_string(&env, env!("CARGO_PKG_VERSION"))
}
