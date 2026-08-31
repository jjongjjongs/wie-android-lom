use alloc::string::String;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike};
use core::cmp::min;

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId, stdlib};
use wie_util::{ByteRead, ByteWrite, Result, read_generic, read_null_terminated_string_bytes, write_generic, write_null_terminated_string_bytes};

use wie_wipi_c::api::kernel::format_varargs;

use crate::runtime::{SVC_CATEGORY_STDLIB, savepoint::SavePointState, svc_ids::StdlibSvcId};

/// Per-stdlib-function call counts (indexed by raw svc id), so the perf meter
/// can name which C-runtime import dominates when the `stdlib` category is the
/// hot syscall category. Stdlib ids sit below `0x600`.
const STDLIB_ID_MAX: usize = 0x600;
pub(crate) static STDLIB_SVC_COUNT: [core::sync::atomic::AtomicU64; STDLIB_ID_MAX] = [const { core::sync::atomic::AtomicU64::new(0) }; STDLIB_ID_MAX];

/// `rand`/`srand` state. The reference links the platform libc's `rand`; games
/// use it for variety rather than a specific sequence, so a standard POSIX
/// linear-congruential generator is enough - and far better than the constant
/// zero an unimplemented import returned, which froze everything random.
static RAND_STATE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

fn c_rand() -> u32 {
    use core::sync::atomic::Ordering::Relaxed;

    let next = RAND_STATE.load(Relaxed).wrapping_mul(1103515245).wrapping_add(12345);
    RAND_STATE.store(next, Relaxed);

    // POSIX: return (next / 65536) % 32768, i.e. 0..=RAND_MAX (0x7fff).
    (next >> 16) & 0x7fff
}

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

        // Character classification/conversion. Each takes one int and returns
        // one int, so they are dispatched here directly rather than through an
        // EmulatedFunction wrapper.
        if let Some(result) = c_ctype(id.0, core.read_param(0)?) {
            return result.write(core, lr);
        }

        match id.0 {
            x if x == StdlibSvcId::Printf as u32 => EmulatedFunction::call(&printf, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Sprintf as u32 => EmulatedFunction::call(&sprintf, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Atoi as u32 => EmulatedFunction::call(&atoi, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Atol as u32 => EmulatedFunction::call(&atoi, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strtol as u32 => EmulatedFunction::call(&strtol, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strtoul as u32 => EmulatedFunction::call(&strtol, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Rand as u32 => {
                let value = c_rand();
                tracing::debug!("rand() -> {value}");
                value.write(core, lr)
            }
            x if x == StdlibSvcId::Srand as u32 => {
                let seed = core.read_param(0)?;
                RAND_STATE.store(seed, core::sync::atomic::Ordering::Relaxed);
                tracing::debug!("srand({seed})");
                0u32.write(core, lr)
            }
            x if x == StdlibSvcId::Strcpy as u32 => EmulatedFunction::call(&stdlib::strcpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncpy as u32 => EmulatedFunction::call(&strncpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcat as u32 => EmulatedFunction::call(&strcat, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncat as u32 => EmulatedFunction::call(&strncat, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcmp as u32 => EmulatedFunction::call(&strcmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strncmp as u32 => EmulatedFunction::call(&strncmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Stricmp as u32 => EmulatedFunction::call(&stricmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strnicmp as u32 => EmulatedFunction::call(&strnicmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strchr as u32 => EmulatedFunction::call(&strchr, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strrchr as u32 => EmulatedFunction::call(&strrchr, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strspn as u32 => EmulatedFunction::call(&strspn, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strcspn as u32 => EmulatedFunction::call(&strcspn, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strpbrk as u32 => EmulatedFunction::call(&strpbrk, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strstr as u32 => EmulatedFunction::call(&strstr, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Strlen as u32 => EmulatedFunction::call(&stdlib::strlen, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memcpy as u32 => EmulatedFunction::call(&stdlib::memcpy, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memmove as u32 => EmulatedFunction::call(&stdlib::memmove, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memcmp as u32 => EmulatedFunction::call(&stdlib::memcmp, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memchr as u32 => EmulatedFunction::call(&memchr, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Memset as u32 => EmulatedFunction::call(&stdlib::memset, core, &mut ()).await?.write(core, lr),
            x if x == StdlibSvcId::Snprintf as u32 => EmulatedFunction::call(&snprintf, core, &mut ()).await?.write(core, lr),
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

    let bytes = read_null_terminated_string_bytes(core, ptr_str)?;

    Ok(c_atoi(&bytes) as u32)
}

/// C `atoi`: skip leading whitespace, take an optional sign, then the run of
/// leading decimal digits, stopping at the first non-digit. `String::parse`
/// (the previous implementation) instead required the *whole* string to be a
/// number, so a title reading a number followed by other bytes on the line -
/// or a negative number, which never fits `u32` - got 0. That fed a bad size
/// into a later `calloc` (크로이센 computed a negative element count and asked
/// for ~4 GiB, crashing with an allocation failure).
fn c_atoi(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    let sign = match bytes.get(i) {
        Some(b'-') => {
            i += 1;
            -1i64
        }
        Some(b'+') => {
            i += 1;
            1
        }
        _ => 1,
    };

    let mut value = 0i64;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value * 10 + i64::from(bytes[i] - b'0');
        // Saturate rather than wrap; C leaves overflow undefined but titles do
        // not rely on a specific wrapped value, and this keeps the result sane.
        value = value.min(i64::from(i32::MAX) + 1);
        i += 1;
    }

    (sign * value) as i32
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

/// The ctype family (`isalnum`..`toupper`, ids `0x3e9`..=`0x3f5`). Each takes a
/// single int and returns an int, so they are answered here without an
/// `EmulatedFunction` wrapper. Returns `None` for any other id so the caller
/// falls through to its main dispatch. Classification follows the C locale: a
/// byte outside ASCII classifies as false, matching what the reference libc
/// returns for these titles' 7-bit inputs.
fn c_ctype(id: u32, arg: u32) -> Option<u32> {
    let byte = (arg & 0xff) as u8;
    let result = match id {
        x if x == StdlibSvcId::Isalnum as u32 => byte.is_ascii_alphanumeric() as u32,
        x if x == StdlibSvcId::Isalpha as u32 => byte.is_ascii_alphabetic() as u32,
        x if x == StdlibSvcId::Iscntrl as u32 => byte.is_ascii_control() as u32,
        x if x == StdlibSvcId::Isdigit as u32 => byte.is_ascii_digit() as u32,
        x if x == StdlibSvcId::Isgraph as u32 => byte.is_ascii_graphic() as u32,
        x if x == StdlibSvcId::Islower as u32 => byte.is_ascii_lowercase() as u32,
        x if x == StdlibSvcId::Isprint as u32 => (byte.is_ascii_graphic() || byte == b' ') as u32,
        x if x == StdlibSvcId::Ispunct as u32 => byte.is_ascii_punctuation() as u32,
        // Rust's `is_ascii_whitespace` omits the vertical tab that C's isspace counts.
        x if x == StdlibSvcId::Isspace as u32 => (byte.is_ascii_whitespace() || byte == 0x0b) as u32,
        x if x == StdlibSvcId::Isupper as u32 => byte.is_ascii_uppercase() as u32,
        x if x == StdlibSvcId::Isxdigit as u32 => byte.is_ascii_hexdigit() as u32,
        x if x == StdlibSvcId::Tolower as u32 => byte.to_ascii_lowercase() as u32,
        x if x == StdlibSvcId::Toupper as u32 => byte.to_ascii_uppercase() as u32,
        _ => return None,
    };

    Some(result)
}

/// `strtol`/`strtoul(nptr, endptr, base)`. Skips leading whitespace, an optional
/// sign, and an optional `0x` prefix (for base 16 or auto-detect), then parses
/// digits in `base`. Writes the first unparsed address through `endptr` when it
/// is non-null, as C requires. `strtoul` shares this: the bit pattern returned
/// in r0 is read back either way.
async fn strtol(core: &mut ArmCore, _: &mut (), nptr: u32, endptr: u32, base: u32) -> Result<u32> {
    if nptr == 0 {
        if endptr != 0 {
            write_generic(core, endptr, nptr)?;
        }
        return Ok(0);
    }

    let bytes = read_null_terminated_string_bytes(core, nptr)?;
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    let mut negative = false;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        negative = bytes[i] == b'-';
        i += 1;
    }

    let mut base = base;
    if (base == 0 || base == 16) && i + 1 < bytes.len() && bytes[i] == b'0' && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
        base = 16;
        i += 2;
    } else if base == 0 && i < bytes.len() && bytes[i] == b'0' {
        base = 8;
    } else if base == 0 {
        base = 10;
    }

    let digits_start = i;
    let mut accumulator: u64 = 0;
    while i < bytes.len() {
        let digit = match bytes[i] {
            b'0'..=b'9' => (bytes[i] - b'0') as u32,
            b'a'..=b'z' => (bytes[i] - b'a' + 10) as u32,
            b'A'..=b'Z' => (bytes[i] - b'A' + 10) as u32,
            _ => break,
        };
        if digit >= base {
            break;
        }
        accumulator = accumulator.wrapping_mul(base as u64).wrapping_add(digit as u64);
        i += 1;
    }

    // C sets endptr to the original nptr when no digits were converted.
    let end = if i == digits_start { nptr } else { nptr + i as u32 };
    if endptr != 0 {
        write_generic(core, endptr, end)?;
    }

    let value = if negative {
        (accumulator as i64).wrapping_neg() as u32
    } else {
        accumulator as u32
    };
    tracing::debug!("strtol({nptr:#x}, {endptr:#x}, {base}) -> {value:#x}");

    Ok(value)
}

async fn strncat(core: &mut ArmCore, _: &mut (), ptr_dst: u32, ptr_src: u32, size: u32) -> Result<u32> {
    tracing::debug!("strncat({ptr_dst:#x}, {ptr_src:#x}, {size:#x})");

    let src = read_null_terminated_string_bytes(core, ptr_src)?;
    let dst = read_null_terminated_string_bytes(core, ptr_dst)?;

    let take = min(size as usize, src.len());
    write_null_terminated_string_bytes(core, ptr_dst + dst.len() as u32, &src[..take])?;

    Ok(ptr_dst)
}

async fn stricmp(core: &mut ArmCore, _: &mut (), ptr_str1: u32, ptr_str2: u32) -> Result<u32> {
    tracing::debug!("stricmp({ptr_str1:#x}, {ptr_str2:#x})");

    let mut str1 = read_null_terminated_string_bytes(core, ptr_str1)?;
    let mut str2 = read_null_terminated_string_bytes(core, ptr_str2)?;
    str1.make_ascii_lowercase();
    str2.make_ascii_lowercase();

    Ok(str1.cmp(&str2) as u32)
}

async fn strnicmp(core: &mut ArmCore, _: &mut (), ptr_str1: u32, ptr_str2: u32, size: u32) -> Result<u32> {
    tracing::debug!("strnicmp({ptr_str1:#x}, {ptr_str2:#x}, {size})");

    let mut str1 = read_null_terminated_string_bytes(core, ptr_str1)?;
    let mut str2 = read_null_terminated_string_bytes(core, ptr_str2)?;
    str1.make_ascii_lowercase();
    str2.make_ascii_lowercase();

    let size = size as usize;
    let head1 = &str1[..min(size, str1.len())];
    let head2 = &str2[..min(size, str2.len())];

    Ok(head1.cmp(head2) as u32)
}

/// `strchr(s, c)`. Returns the address of the first `c` in `s`, or zero. A zero
/// `c` matches the terminator, per C.
async fn strchr(core: &mut ArmCore, _: &mut (), ptr_str: u32, ch: u32) -> Result<u32> {
    if ptr_str == 0 {
        return Ok(0);
    }

    let bytes = read_null_terminated_string_bytes(core, ptr_str)?;
    let needle = (ch & 0xff) as u8;

    if let Some(offset) = bytes.iter().position(|&b| b == needle) {
        return Ok(ptr_str + offset as u32);
    }
    if needle == 0 {
        // The terminator is not part of `bytes`; it sits just past it.
        return Ok(ptr_str + bytes.len() as u32);
    }

    Ok(0)
}

/// `strrchr(s, c)`. Returns the address of the last `c` in `s`, or zero.
async fn strrchr(core: &mut ArmCore, _: &mut (), ptr_str: u32, ch: u32) -> Result<u32> {
    if ptr_str == 0 {
        return Ok(0);
    }

    let bytes = read_null_terminated_string_bytes(core, ptr_str)?;
    let needle = (ch & 0xff) as u8;

    if needle == 0 {
        return Ok(ptr_str + bytes.len() as u32);
    }
    if let Some(offset) = bytes.iter().rposition(|&b| b == needle) {
        return Ok(ptr_str + offset as u32);
    }

    Ok(0)
}

async fn strspn(core: &mut ArmCore, _: &mut (), ptr_str: u32, ptr_accept: u32) -> Result<u32> {
    if ptr_str == 0 {
        return Ok(0);
    }

    let bytes = read_null_terminated_string_bytes(core, ptr_str)?;
    let accept = read_null_terminated_string_bytes(core, ptr_accept)?;

    let count = bytes.iter().take_while(|b| accept.contains(b)).count();

    Ok(count as u32)
}

async fn strcspn(core: &mut ArmCore, _: &mut (), ptr_str: u32, ptr_reject: u32) -> Result<u32> {
    if ptr_str == 0 {
        return Ok(0);
    }

    let bytes = read_null_terminated_string_bytes(core, ptr_str)?;
    let reject = read_null_terminated_string_bytes(core, ptr_reject)?;

    let count = bytes.iter().take_while(|b| !reject.contains(b)).count();

    Ok(count as u32)
}

/// `strpbrk(s, accept)`. Returns the address of the first byte of `s` that
/// appears in `accept`, or zero.
async fn strpbrk(core: &mut ArmCore, _: &mut (), ptr_str: u32, ptr_accept: u32) -> Result<u32> {
    if ptr_str == 0 {
        return Ok(0);
    }

    let bytes = read_null_terminated_string_bytes(core, ptr_str)?;
    let accept = read_null_terminated_string_bytes(core, ptr_accept)?;

    if let Some(offset) = bytes.iter().position(|b| accept.contains(b)) {
        return Ok(ptr_str + offset as u32);
    }

    Ok(0)
}

/// `memchr(s, c, n)`. Returns the address of the first `c` within the first `n`
/// bytes of `s`, or zero.
async fn memchr(core: &mut ArmCore, _: &mut (), ptr: u32, ch: u32, size: u32) -> Result<u32> {
    if ptr == 0 || size == 0 {
        return Ok(0);
    }

    let mut buffer = alloc::vec![0u8; size as usize];
    core.read_bytes(ptr, &mut buffer)?;
    let needle = (ch & 0xff) as u8;

    if let Some(offset) = buffer.iter().position(|&b| b == needle) {
        return Ok(ptr + offset as u32);
    }

    Ok(0)
}

/// `snprintf(dest, size, format, ...)`. Formats like `sprintf` but writes at
/// most `size - 1` bytes plus a terminator. Returns the length the full result
/// would have, as C does. Five variadic words cover the specifiers these titles
/// use.
#[allow(clippy::too_many_arguments)]
async fn snprintf(core: &mut ArmCore, _: &mut (), dest: u32, size: u32, format: u32, a0: u32, a1: u32, a2: u32, a3: u32, a4: u32) -> Result<u32> {
    let format_bytes = read_null_terminated_string_bytes(core, format)?;
    let format_string = encoding_rs::EUC_KR.decode(&format_bytes).0;

    tracing::debug!("snprintf({dest:#x}, {size}, {:?})", format_string);

    let args = [a0, a1, a2, a3, a4];
    let result = format_varargs(&format_string, &args, &mut |ptr| {
        let bytes = read_null_terminated_string_bytes(core, ptr)?;
        Ok(encoding_rs::EUC_KR.decode(&bytes).0.into_owned())
    })?;

    let result_bytes = encoding_rs::EUC_KR.encode(&result).0;

    if size > 0 && dest != 0 {
        let capacity = (size - 1) as usize;
        let take = min(capacity, result_bytes.len());
        write_null_terminated_string_bytes(core, dest, &result_bytes[..take])?;
    }

    Ok(result.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::c_atoi;

    #[test]
    fn c_atoi_parses_leading_number_and_stops() {
        assert_eq!(c_atoi(b"1156"), 1156);
        // Stops at the first non-digit instead of failing the whole parse.
        assert_eq!(c_atoi(b"1156 42"), 1156);
        assert_eq!(c_atoi(b"1156\nrest"), 1156);
        assert_eq!(c_atoi(b"42abc"), 42);
    }

    #[test]
    fn c_atoi_handles_sign_and_whitespace() {
        assert_eq!(c_atoi(b"-1156"), -1156);
        assert_eq!(c_atoi(b"+7"), 7);
        assert_eq!(c_atoi(b"   -8xyz"), -8);
    }

    #[test]
    fn c_atoi_non_numeric_is_zero() {
        assert_eq!(c_atoi(b""), 0);
        assert_eq!(c_atoi(b"abc"), 0);
        assert_eq!(c_atoi(b"-"), 0);
    }
}
