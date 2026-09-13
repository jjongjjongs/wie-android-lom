//! A synthetic `JNIEnv` for the firmware's media path.
//!
//! The firmware's `WipiMediaManager` is a JNI object: `getWipiMediaManager`
//! fetches the current `JNIEnv` (via `getJNIEnv` -> the kernel JNI env global)
//! and calls Java through its `JNINativeInterface` function table
//! (`ldr pc, [env_table + off]`). On real hardware the Android runtime hands the
//! firmware a real `JNIEnv` through `JNI_OnLoad`; here there is no ART, so
//! `getJNIEnv` returns null and the media path jumps through a null table slot
//! (observed: `MC_mdaClipSetVolume` faulting with `PC=0`).
//!
//! We supply a synthetic env instead: a `JNINativeInterface` table whose every
//! slot is an SVC stub, plus a one-word `JNIEnv` that points at it, written into
//! the firmware's `kernel_jni_env` global so `getJNIEnv` returns it. Bring-up:
//! every slot logs the JNI index it services and returns 0, so the device log
//! shows exactly which JNI functions the media manager calls - the list we then
//! implement to bridge the firmware's audio to wie's sink and tap its PCM.

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, ResultWriter, SvcId};
use wie_util::{Result, write_generic};

use super::SVC_CATEGORY_FIRMWARE_JNI;

/// Slots in the synthetic `JNINativeInterface` table. The real table has ~232
/// function pointers; we stub a bit more so any index the firmware reaches is a
/// logged, callable no-op rather than a wild jump.
const JNI_TABLE_SLOTS: u32 = 256;

/// The byte offset of a `JNINativeInterface` slot maps to a human name for the
/// standard JNI ABI, so the trace log reads as JNI calls rather than raw
/// offsets. Only the entries the media path is expected to touch are named; the
/// rest log as `JNI[+0xNN]`.
fn jni_slot_name(offset: u32) -> &'static str {
    // Standard JNINativeInterface: byte offset == JNI function index * 4 (the
    // first four slots are reserved, GetVersion is index 4 at 0x10).
    match offset {
        0x10 => "GetVersion",
        0x14 => "DefineClass",
        0x18 => "FindClass",
        0x1c => "FromReflectedMethod",
        0x28 => "GetSuperclass",
        0x2c => "IsAssignableFrom",
        0x54 => "NewGlobalRef",
        0x58 => "DeleteGlobalRef",
        0x5c => "DeleteLocalRef",
        0x70 => "NewObject",
        0x7c => "GetObjectClass",
        0x80 => "IsInstanceOf",
        0x84 => "GetMethodID",
        0x88 => "CallObjectMethod",
        0xc4 => "CallIntMethod",
        0xf4 => "CallVoidMethod",
        0x1c4 => "GetStaticMethodID",
        0x1c8 => "CallStaticObjectMethod",
        0x290 => "GetStaticFieldID",
        0x2a4 => "GetStaticIntField",
        0x35c => "RegisterNatives",
        0x364 => "MonitorEnter",
        0x368 => "MonitorExit",
        0x36c => "GetJavaVM",
        0x394 => "NewDirectByteBuffer",
        0x398 => "GetDirectBufferAddress",
        0x39c => "GetDirectBufferCapacity",
        _ => "JNI",
    }
}

#[derive(Clone)]
struct JniContext {
    #[allow(dead_code)]
    system: System,
}

/// Installs a synthetic `JNIEnv` into the firmware's `kernel_jni_env` global so
/// `getJNIEnv` returns it instead of null. `kernel_jni_env_addr` is the loaded
/// address of the firmware's `kernel_jni_env` export. Returns the `JNIEnv`
/// pointer for later use (e.g. tapping a PCM buffer it hands back).
pub fn install_firmware_jni_env(core: &mut ArmCore, system: &System, kernel_jni_env_addr: u32) -> Result<u32> {
    async fn handle_jni_svc(core: &mut ArmCore, _context: &mut JniContext, id: SvcId) -> Result<()> {
        let (_, lr) = core.read_pc_lr()?;
        let offset = id.0 * 4;
        let name = jni_slot_name(offset);

        let a0 = core.read_param(0)?;
        let a1 = core.read_param(1)?;
        let a2 = core.read_param(2)?;
        let a3 = core.read_param(3)?;
        tracing::info!("[jni] {name} (slot +{offset:#x}) (env={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) -> 0");

        // Bring-up: return 0 for every JNI call. getWipiMediaManager checks its
        // results and bails to a null manager on 0, so this trades the null-slot
        // crash for a clean trace of the JNI functions the media path needs.
        0u32.write(core, lr)
    }

    core.register_svc_handler(SVC_CATEGORY_FIRMWARE_JNI, handle_jni_svc, &JniContext { system: system.clone() })?;

    // Build the JNINativeInterface function table: one SVC stub per slot.
    let table = Allocator::alloc(core, JNI_TABLE_SLOTS * 4)?;
    for slot in 0..JNI_TABLE_SLOTS {
        let stub = core.make_svc_stub(SVC_CATEGORY_FIRMWARE_JNI, slot)?;
        write_generic(core, table + slot * 4, stub)?;
    }

    // A JNIEnv is a `const struct JNINativeInterface**`: a one-word cell holding
    // the table pointer. Install that JNIEnv into the kernel JNI env global, so
    // wipihal_get_kernel_jni_env (`*(kernel_jni_env)`) hands it to the firmware.
    let env = Allocator::alloc(core, 4)?;
    write_generic(core, env, table)?;
    write_generic(core, kernel_jni_env_addr, env)?;

    tracing::info!("Installed synthetic JNIEnv {env:#x} (table {table:#x}) into kernel_jni_env {kernel_jni_env_addr:#x}");

    Ok(env)
}
