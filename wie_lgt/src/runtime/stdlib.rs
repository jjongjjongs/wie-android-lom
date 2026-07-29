use alloc::string::String;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike};
use core::cmp::min;

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId, stdlib};
use wie_util::{ByteWrite, Result, read_generic, read_null_terminated_string_bytes, write_generic, write_null_terminated_string_bytes};

use crate::runtime::{SVC_CATEGORY_STDLIB, svc_ids::StdlibSvcId};

pub fn register_stdlib_svc_handler(core: &mut ArmCore, system: &System) -> Result<()> {
    async fn handle_stdlib_svc(core: &mut ArmCore, system: &mut System, id: SvcId) -> Result<()> {
        let (_, lr) = core.read_pc_lr()?;

        match id.0 {
            x if x == StdlibSvcId::Printf as u32 => EmulatedFunction::call(&printf, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Atoi as u32 => EmulatedFunction::call(&atoi, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcpy as u32 => EmulatedFunction::call(&stdlib::strcpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncpy as u32 => EmulatedFunction::call(&strncpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcat as u32 => EmulatedFunction::call(&strcat, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcmp as u32 => EmulatedFunction::call(&strcmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncmp as u32 => EmulatedFunction::call(&strncmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strstr as u32 => EmulatedFunction::call(&strstr, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strlen as u32 => EmulatedFunction::call(&stdlib::strlen, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memcpy as u32 => EmulatedFunction::call(&stdlib::memcpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memset as u32 => EmulatedFunction::call(&stdlib::memset, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Time as u32 => EmulatedFunction::call(&time, core, system).await?.write(core, lr),
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

    core.register_svc_handler(SVC_CATEGORY_STDLIB, handle_stdlib_svc, system)
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

async fn strncmp(core: &mut ArmCore, _: &mut (), ptr_str1: u32, ptr_str2: u32, size: u32) -> Result<u32> {
    tracing::debug!("strncmp({ptr_str1:#x}, {ptr_str2:#x}, {size})");

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
