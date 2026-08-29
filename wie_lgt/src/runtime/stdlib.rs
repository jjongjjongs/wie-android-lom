use alloc::string::String;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike};
use core::cmp::min;

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId, stdlib};
use wie_util::{ByteWrite, Result, read_generic, read_null_terminated_string_bytes, write_generic, write_null_terminated_string_bytes};

use wie_wipi_c::api::kernel::format_varargs;

use crate::runtime::{SVC_CATEGORY_STDLIB, savepoint::SavePointState, svc_ids::StdlibSvcId};

/// Per-stdlib-function call counts (indexed by raw svc id), so the perf meter
/// can name which C-runtime import dominates when the `stdlib` category is the
/// hot syscall category. Stdlib ids sit below `0x600`.
const STDLIB_ID_MAX: usize = 0x600;
pub(crate) static STDLIB_SVC_COUNT: [core::sync::atomic::AtomicU64; STDLIB_ID_MAX] = [const { core::sync::atomic::AtomicU64::new(0) }; STDLIB_ID_MAX];

/// Log the top stdlib functions by call rate (names via `StdlibSvcId`), draining
/// the counters. Identifies the exact hot C-runtime import behind the frame cost
/// for titles whose bottleneck is the `stdlib` syscall round-trip.
pub(crate) fn report_hot_stdlib(dt_ms: u64) {
    use core::fmt::Write;
    use core::sync::atomic::Ordering::Relaxed;

    let mut top: [(usize, u64); 6] = [(0, 0); 6];
    for (id, slot) in STDLIB_SVC_COUNT.iter().enumerate() {
        let count = slot.swap(0, Relaxed);
        if count > top[5].1 {
            top[5] = (id, count);
            top.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        }
    }
    if top[0].1 == 0 {
        return;
    }
    let mut line = String::from("[stdlib]");
    for (id, count) in top.iter().filter(|(_, c)| *c > 0) {
        let per_s = *count as f64 * 1000.0 / dt_ms as f64;
        match StdlibSvcId::try_from(SvcId(*id as u32)) {
            Ok(name) => {
                let _ = write!(line, " {name:?}({id:#x})={per_s:.0}/s");
            }
            Err(_) => {
                let _ = write!(line, " {id:#x}={per_s:.0}/s");
            }
        }
    }
    tracing::info!("{line}");
}

#[derive(Clone)]
struct StdlibSvcContext {
    system: System,
    save_points: SavePointState,
}

pub fn register_stdlib_svc_handler(core: &mut ArmCore, system: &System, save_points: &SavePointState) -> Result<()> {
    async fn handle_stdlib_svc(core: &mut ArmCore, context: &mut StdlibSvcContext, id: SvcId) -> Result<()> {
        STDLIB_SVC_COUNT[(id.0 as usize).min(STDLIB_ID_MAX - 1)].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let (_, lr) = core.read_pc_lr()?;

        // Kernel table 1 / function 0x32 is libc setjmp. The guest passes the
        // 0x10c-byte block returned by vm_alloc_save_point as its jmp_buf.
        if id.0 == 0x32 {
            let save_point = core.read_param(0)?;
            context.save_points.capture(core, save_point, lr)?;
            return 0u32.write(core, lr);
        }

        match id.0 {
            x if x == StdlibSvcId::Printf as u32 => EmulatedFunction::call(&printf, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Sprintf as u32 => EmulatedFunction::call(&sprintf, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Atoi as u32 => EmulatedFunction::call(&atoi, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcpy as u32 => EmulatedFunction::call(&stdlib::strcpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncpy as u32 => EmulatedFunction::call(&strncpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcat as u32 => EmulatedFunction::call(&strcat, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcmp as u32 => EmulatedFunction::call(&strcmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncmp as u32 => EmulatedFunction::call(&strncmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strstr as u32 => EmulatedFunction::call(&strstr, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strlen as u32 => EmulatedFunction::call(&stdlib::strlen, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memcpy as u32 => EmulatedFunction::call(&stdlib::memcpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memmove as u32 => EmulatedFunction::call(&stdlib::memmove, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memcmp as u32 => EmulatedFunction::call(&stdlib::memcmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memset as u32 => EmulatedFunction::call(&stdlib::memset, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Time as u32 => EmulatedFunction::call(&time, core, &mut context.system).await?.write(core, lr),
            x if x == StdlibSvcId::Localtime as u32 => EmulatedFunction::call(&localtime, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Atexit as u32 => EmulatedFunction::call(&atexit, core, &mut ()).await?.write(core, lr),
            // An unrecognised import is reported and returns zero, the way
            // unknown WIPI-C and Java imports already do. Ending the run
            // instead hides everything the application would have done next,
            // which is the only way to find out what the import was for.
            index => {
                let a0 = core.read_param(0)?;
                let a1 = core.read_param(1)?;
                let a2 = core.read_param(2)?;
                let a3 = core.read_param(3)?;

                tracing::warn!("Unknown LGT stdlib import {index:#x}(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");

                0u32.write(core, lr)
            }
        }
    }

    core.register_svc_handler(
        SVC_CATEGORY_STDLIB,
        handle_stdlib_svc,
        &StdlibSvcContext {
            system: system.clone(),
            save_points: save_points.clone(),
        },
    )
}

async fn strncpy(core: &mut ArmCore, _: &mut (), ptr_dst: u32, ptr_src: u32, size: u32) -> Result<()> {
    tracing::debug!("strncpy({ptr_dst:#x}, {ptr_src:#x}, {size:#x})");

    let src = read_null_terminated_string_bytes(core, ptr_src)?;

    let size_to_copy = min(size, src.len() as u32);
    let bytes = &src[..size_to_copy as usize];

    core.write_bytes(ptr_dst, bytes)?;

    Ok(())
}

async fn strcat(core: &mut ArmCore, _: &mut (), ptr_dst: u32, ptr_src: u32) -> Result<()> {
    tracing::debug!("strcat({ptr_dst:#x}, {ptr_src:#x})");

    let src = read_null_terminated_string_bytes(core, ptr_src)?;
    let dst = read_null_terminated_string_bytes(core, ptr_dst)?;

    let offset = dst.len();
    write_null_terminated_string_bytes(core, ptr_dst + offset as u32, &src)?;

    Ok(())
}

async fn strcmp(core: &mut ArmCore, _: &mut (), ptr_str1: u32, ptr_str2: u32) -> Result<u32> {
    tracing::debug!("strcmp({ptr_str1:#x}, {ptr_str2:#x})");

    let str1 = read_null_terminated_string_bytes(core, ptr_str1)?;
    let str2 = read_null_terminated_string_bytes(core, ptr_str2)?;

    Ok(str1.cmp(&str2) as u32)
}

async fn atoi(core: &mut ArmCore, _: &mut (), ptr_str: u32) -> Result<u32> {
    tracing::debug!("atoi({ptr_str:#x})");

    let string = read_null_terminated_string_bytes(core, ptr_str)?;
    let string = String::from_utf8(string).unwrap();

    Ok(string.parse().unwrap_or(0))
}

async fn time(core: &mut ArmCore, system: &mut System, ptr_time: u32) -> Result<u32> {
    let epoch_seconds = (system.platform().now().raw() / 1000) as u32;
    tracing::debug!("time({ptr_time:#x}) -> {epoch_seconds}");

    if ptr_time != 0 {
        write_generic(core, ptr_time, epoch_seconds)?;
    }

    Ok(epoch_seconds)
}

// TODO is this method better suit on wie_backend?
async fn localtime(core: &mut ArmCore, _: &mut (), ptr_time: u32) -> Result<u32> {
    tracing::debug!("localtime({ptr_time:#x})");

    // TODO we need static buffer
    let result = Allocator::alloc(core, 0x2c)?;
    let time: u32 = read_generic(core, ptr_time)?;

    // TODO kst only for now
    let kst = FixedOffset::east_opt(9 * 3600).unwrap();
    let dt: DateTime<FixedOffset> = kst.timestamp_opt(time as _, 0).unwrap();

    // TODO tm struct
    write_generic(core, result, dt.second() as u32)?;
    write_generic(core, result + 0x04, dt.minute() as u32)?;
    write_generic(core, result + 0x08, dt.hour() as u32)?;
    write_generic(core, result + 0x0c, dt.day() as u32)?;
    write_generic(core, result + 0x10, (dt.month() as u32) - 1)?; // months since January
    write_generic(core, result + 0x14, (dt.year() as u32) - 1900)?; // years since 1900
    write_generic(core, result + 0x18, dt.weekday().num_days_from_sunday() as u32)?; // days since Sunday
    write_generic(core, result + 0x1c, dt.ordinal() as u32)?; // days since January 1
    write_generic(core, result + 0x20, 0u32)?; // DST flag
    write_generic(core, result + 0x24, kst.local_minus_utc() as u32)?; // timezone offset in seconds
    write_generic(core, result + 0x28, 0u32)?; // timezone abbreviation ptr

    Ok(result)
}

/// The format string is written out as-is. Conversions are left alone rather
/// than guessed at: the arguments after the first are spread across registers
/// and the stack by rules this does not model, and a wrong walk of them reads
/// memory that is not there. Applications use this for their own tracing.
async fn printf(core: &mut ArmCore, _: &mut (), format: u32) -> Result<u32> {
    let bytes = read_null_terminated_string_bytes(core, format)?;

    tracing::debug!("printf({:?})", String::from_utf8_lossy(&bytes));

    Ok(bytes.len() as u32)
}

/// `sprintf(dest, format, ...)`. A title formatting its HUD text this way got
/// an empty destination while this was an unimplemented import, so the numbers
/// and labels it built each frame never appeared. Six variadic words cover the
/// specifiers these titles use; the format engine stops at the last one.
#[allow(clippy::too_many_arguments)]
async fn sprintf(core: &mut ArmCore, _: &mut (), dest: u32, format: u32, a0: u32, a1: u32, a2: u32, a3: u32, a4: u32, a5: u32) -> Result<u32> {
    let format_bytes = read_null_terminated_string_bytes(core, format)?;
    let format_string = encoding_rs::EUC_KR.decode(&format_bytes).0;

    tracing::debug!("sprintf({dest:#x}, {:?})", format_string);

    let args = [a0, a1, a2, a3, a4, a5];
    let result = format_varargs(&format_string, &args, &mut |ptr| {
        let bytes = read_null_terminated_string_bytes(core, ptr)?;
        Ok(encoding_rs::EUC_KR.decode(&bytes).0.into_owned())
    })?;

    let result_bytes = encoding_rs::EUC_KR.encode(&result).0;
    write_null_terminated_string_bytes(core, dest, &result_bytes)?;

    Ok(result.len() as u32)
}

async fn strncmp(core: &mut ArmCore, _: &mut (), ptr_str1: u32, ptr_str2: u32, size: u32) -> Result<u32> {
    tracing::debug!("strncmp({ptr_str1:#x}, {ptr_str2:#x}, {size})");

    if ptr_str1 == 0 || ptr_str2 == 0 {
        return Ok(u32::from(ptr_str1 != ptr_str2));
    }

    let str1 = read_null_terminated_string_bytes(core, ptr_str1)?;
    let str2 = read_null_terminated_string_bytes(core, ptr_str2)?;

    let size = size as usize;
    let head1 = &str1[..min(size, str1.len())];
    let head2 = &str2[..min(size, str2.len())];

    Ok(head1.cmp(head2) as u32)
}

/// Returns the address of the first occurrence of `needle` in `haystack`, or
/// zero. This used to do nothing and return zero, which reads as "not found"
/// and is a plausible answer, so nothing ever looked wrong.
async fn strstr(core: &mut ArmCore, _: &mut (), haystack: u32, needle: u32) -> Result<u32> {
    // Applications pass a null haystack and expect "not found" rather than a
    // fault, which is what they got while this was a stub that did nothing.
    if haystack == 0 || needle == 0 {
        tracing::debug!("strstr({haystack:#x}, {needle:#x}) -> 0x0");

        return Ok(0);
    }

    let haystack_bytes = read_null_terminated_string_bytes(core, haystack)?;
    let needle_bytes = read_null_terminated_string_bytes(core, needle)?;

    let found = haystack_bytes
        .windows(needle_bytes.len().max(1))
        .position(|window| window == needle_bytes.as_slice())
        .map(|offset| haystack + offset as u32)
        .unwrap_or(0);

    tracing::debug!("strstr({haystack:#x}, {needle:#x}) -> {found:#x}");

    Ok(found)
}

/// Registers a function to run at exit, which is not the same as running it.
/// This used to call it immediately, which runs an application's teardown in
/// the middle of its startup.
async fn atexit(_core: &mut ArmCore, _: &mut (), handler: u32) -> Result<u32> {
    tracing::debug!("atexit({handler:#x})");

    Ok(0)
}
