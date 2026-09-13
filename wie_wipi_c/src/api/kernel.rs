mod sprintf;

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::iter;

use bytemuck::{Pod, Zeroable};

use wipi_types::wipic::{WIPICIndirectPtr, WIPICWord};

use wie_util::{
    Result, WieError, descriptor_value, read_generic, read_null_terminated_string_bytes, write_generic, write_null_terminated_string_bytes,
};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};

pub use self::sprintf::format as format_varargs;
use self::sprintf::sprintf;

#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct WIPICTimer {
    fn_callback: WIPICWord,
}

pub async fn current_time(context: &mut dyn WIPICContext) -> Result<u64> {
    tracing::debug!("MC_knlCurrentTime()");

    Ok(context.system().platform().now().raw())
}

pub async fn get_system_property(context: &mut dyn WIPICContext, ptr_id: WIPICWord, p_out: WIPICWord, buf_size: WIPICWord) -> Result<i32> {
    let id_bytes = read_null_terminated_string_bytes(context, ptr_id)?;
    let id = encoding_rs::EUC_KR.decode(&id_bytes).0;

    // Logged at info: start-up/auth checks read these, and a wrong value is a
    // common reason a title bails, so the query and its result belong in a
    // normal log capture.
    //
    // `recovered` backs the subscriber-number arms; it lives out here so the
    // owned string it holds outlives the borrow the `match` yields.
    let recovered;
    let value = match id.as_ref() {
        "RSSILEVEL" => "30",
        "BATTERYLEVEL" => "100",
        "PHONEMODEL" => "Emulator",
        // The handset's volume step count, answered the same here as through
        // `HandsetProperty.getSystemProperty` so a title that reads it by
        // either door is told the same handset. See that arm for what a title
        // does with it.
        "VOLUMELEVEL" => "5",
        "MAXSERIALNUM" | "MAXSOCKETNUM" => "4",
        // LGT ez-i cert.c2s DRM keys on the subscriber phone number: cert.c2s
        // encodes "<appID><phoneNumber><checksums>" encrypted with the phone
        // number, and the title reads PHONENUMBER, decrypts, and rejects a
        // mismatch (error 3100, observed in 이노티아 연대기 2). We recover the
        // exact number the certificate was issued for from cert.c2s itself, so
        // an unmodified title authenticates without a per-game value. When no
        // certificate is recoverable we fall back to a valid placeholder.
        "PHONENUMBER" => {
            recovered = subscriber_number(context).await;
            recovered.as_str()
        }
        // MIN carries the same number, so a title that cross-checks the two
        // agrees with itself - unless its own certificate names a MIN, which
        // outranks it. 액션퍼즐패밀리4 GS2 ships a cert.c2s encrypted with a key
        // of its own whose first field is the MIN it was issued for, reads MIN
        // here, and stops at error 5001 when the two differ; the certificate
        // names an empty one, and that is what the handset it was issued for
        // reported.
        "MIN" => {
            recovered = match handset_identity(context).await {
                Some(identity) => identity.min,
                None => subscriber_number(context).await,
            };
            recovered.as_str()
        }
        // The media types the handset can play, which a title reads to decide
        // whether to load its music at all: 판타지포에버3 asks for this and looks
        // for `Yamaha_MA3` in the answer, and got `-9` - so it created no clip,
        // played nothing and ran silent. The reference asks the handset and
        // reports what its chip does; ours reports what its audio path does,
        // which is SMAF, so it names the SMAF profiles and nothing else. A
        // title that finds one of these here goes on to hand us SMAF data,
        // which is exactly what `MC_mdaClipPutData` decodes.
        "MEDIADEVICES" => "Yamaha_MA1,Yamaha_MA2,Yamaha_MA3,Yamaha_MA5,Yamaha_SMAF",
        "ANNUN_CALL" => "0",
        "ANNUN_SMS" => "0",
        "ANNUN_SILENT" => "0",
        "ANNUN_ALARM" => "0",
        "ANNUN_SECURITY" => "0",
        "CURRENTCH" => "0",
        "AIRPLANE_MODE" => "0",
        "ROAMING_AREA" => "0",
        "DS_LOCK" => "0",
        _ => {
            tracing::info!("MC_knlGetSystemProperty({id:?}) -> -9 (unknown property)");
            return Ok(-9); // M_E_INVALID
        }
    };

    let bytes = value.as_bytes();
    if bytes.len() + 1 > buf_size as usize {
        tracing::info!("MC_knlGetSystemProperty({id:?}) -> -18 (buffer {buf_size} too small for {:?})", value);
        return Ok(-18); // M_E_SHORTBUF
    }

    tracing::info!("MC_knlGetSystemProperty({id:?}) -> {value:?}");
    write_null_terminated_string_bytes(context, p_out, value.as_bytes())?;

    Ok(0)
}

/// The subscriber number to report for PHONENUMBER / MIN.
///
/// Recovered from the archive by [`wie_backend::subscriber`], which the
/// WIPI-Java `HandsetProperty` path uses too so the two always agree.
pub(crate) async fn subscriber_number(context: &mut dyn WIPICContext) -> String {
    let cert = context.read_resource("cert.c2s").await.ok();
    let certification = context.read_resource("certification").await.ok();
    let app_info = context.read_resource("app_info").await.ok();

    wie_backend::subscriber::subscriber_number(cert.as_deref(), certification.as_deref(), app_info.as_deref())
}

/// The handset identity a title's own-key `cert.c2s` was issued for, or `None`
/// when the archive has no such certificate.
async fn handset_identity(context: &mut dyn WIPICContext) -> Option<wie_backend::subscriber::HandsetIdentity> {
    let cert = context.read_resource("cert.c2s").await.ok()?;

    wie_backend::subscriber::identity_from_cert(&cert)
}

pub async fn set_system_property(context: &mut dyn WIPICContext, ptr_id: WIPICWord, ptr_value: WIPICWord) -> Result<()> {
    // Decoded and logged at info: a title that sets a property and reads it back
    // expects the value to survive, so seeing what it set (and not persisting it
    // yet) helps explain a later mismatch. Persistence is a follow-up.
    let id = encoding_rs::EUC_KR
        .decode(&read_null_terminated_string_bytes(context, ptr_id)?)
        .0
        .into_owned();
    let value = if ptr_value != 0 {
        encoding_rs::EUC_KR
            .decode(&read_null_terminated_string_bytes(context, ptr_value)?)
            .0
            .into_owned()
    } else {
        String::new()
    };
    tracing::info!("MC_knlSetSystemProperty({id:?}, {value:?}) [not persisted]");

    Ok(())
}

pub async fn def_timer(context: &mut dyn WIPICContext, ptr_timer: WIPICWord, fn_callback: WIPICWord) -> Result<()> {
    tracing::debug!("MC_knlDefTimer({ptr_timer:#x}, {fn_callback:#x})");

    let timer = WIPICTimer { fn_callback };

    write_generic(context, ptr_timer, timer)?;

    Ok(())
}

pub async fn set_timer(
    context: &mut dyn WIPICContext,
    ptr_timer: WIPICWord,
    timeout_low: WIPICWord,
    timeout_high: WIPICWord,
    param: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_knlSetTimer({ptr_timer:#x}, {timeout_low:#x}, {timeout_high:#x}, {param:#x})");

    struct TimerCallback {
        ptr_timer: WIPICWord,
        fn_callback: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for TimerCallback {
        #[tracing::instrument(name = "timer", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            context.call_function(self.fn_callback, &[self.ptr_timer, self.param]).await?;

            Ok(WIPICResult { results: Vec::new() })
        }
    }

    let now = context.system().platform().now();

    // Raptor represents the delay as a signed 64-bit value split into two
    // words. Legacy runtimes schedule a negative delay on the next scheduler
    // tick instead of interpreting it as a very large unsigned duration.
    let raw_timeout = ((timeout_high as u64) << 32) | timeout_low as u64;
    let timeout = if (raw_timeout as i64) < 0 { 1 } else { raw_timeout };

    let timer: WIPICTimer = read_generic(context, ptr_timer)?;

    context.set_timer(
        ptr_timer,
        now + timeout,
        Box::new(TimerCallback {
            ptr_timer,
            fn_callback: timer.fn_callback,
            param,
        }),
    );

    Ok(())
}

pub async fn unset_timer(context: &mut dyn WIPICContext, ptr_timer: WIPICWord) -> Result<()> {
    tracing::debug!("MC_knlUnsetTimer({ptr_timer:#x})");

    context.unset_timer(ptr_timer);

    Ok(())
}

pub async fn alloc(context: &mut dyn WIPICContext, size: WIPICWord) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_knlAlloc({size:#x})");

    if size == 0 {
        return Ok(WIPICIndirectPtr(0));
    }

    context.alloc(size)
}

pub async fn calloc(context: &mut dyn WIPICContext, size: WIPICWord) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_knlCalloc({size:#x})");

    // A zero-size request still returns a unique, freeable non-null pointer, as
    // the reference allocator does. A title's font loader callocs a
    // zero-length buffer and treats a null result as failure, unwinding into a
    // state it then dereferences through a -1 handle; handing back null there
    // faulted it. Allocate a minimal block so the pointer is non-null.
    let alloc_size = size.max(1);

    let memory = context.alloc(alloc_size)?;

    let zero = iter::repeat_n(0, alloc_size as _).collect::<Vec<_>>();
    context.write_bytes(context.data_ptr(memory)?, &zero)?;

    Ok(memory)
}

pub async fn free(context: &mut dyn WIPICContext, memory: WIPICIndirectPtr) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_knlFree({:#x})", memory.0);

    if memory.0 == 0 {
        return Ok(memory);
    }

    context.free(memory)?;

    Ok(memory)
}

pub async fn get_resource_id(context: &mut dyn WIPICContext, ptr_name: WIPICWord, ptr_size: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_knlGetResourceID({ptr_name:#x}, {ptr_size:#x})");

    let raw_name = read_null_terminated_string_bytes(context, ptr_name)?;
    let mut name = encoding_rs::EUC_KR.decode(&raw_name).0;
    tracing::debug!("  resource name: {name}");

    let mut size = context.get_resource_size(&name).await?;

    // A title can hand us a path that is not NUL-terminated: it copies the
    // characters into a scratch buffer and trusts the byte past the last one to
    // already be zero. That holds on the reference's heap, whose freshly handed
    // out (and internally recycled) blocks are zeroed, but not here, where the
    // block still carries the bytes an earlier use left - MapleStory 도적편
    // builds "png/mainmenu/menu.dat" over stale RGB565 pixels, so the read runs
    // on into garbage and the lookup misses. A resource path is plain ASCII, so
    // when the full read misses, retry with the leading printable-ASCII run,
    // which is exactly the path the title meant.
    if size.is_none() {
        let ascii_len = raw_name.iter().position(|&b| !(0x20..=0x7e).contains(&b)).unwrap_or(raw_name.len());
        if ascii_len < raw_name.len() && ascii_len > 0 {
            let trimmed = encoding_rs::EUC_KR.decode(&raw_name[..ascii_len]).0.into_owned();
            if let Some(found) = context.get_resource_size(&trimmed).await? {
                tracing::debug!("  resource name resolved to {trimmed:?} after trimming a non-path tail");
                size = Some(found);
                name = trimmed.into();
            }
        }
    }

    if size.is_none() {
        if ptr_size != 0 {
            write_generic(context, ptr_size, 0u32)?;
        }
        return Ok(-12); // M_E_NOENT
    }

    let size = size.unwrap();

    let name_bytes = name.as_bytes();
    let handle_size = name_bytes
        .len()
        .checked_add(1)
        .ok_or_else(|| WieError::FatalError("Resource name too long".to_string()))?;
    let ptr_handle = context.alloc_raw(handle_size as _)?;
    write_null_terminated_string_bytes(context, ptr_handle, name_bytes)?;
    write_generic(context, ptr_size, size as u32)?;

    tracing::debug!("  resource {name:?} is {size} bytes, handle {ptr_handle:#x}");

    Ok(ptr_handle as _)
}

pub async fn get_resource(context: &mut dyn WIPICContext, id: i32, buf: WIPICIndirectPtr, buf_size: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_knlGetResource({id}, {:#x}, {buf_size})", buf.0);

    // Both of the ways this refuses are ways a title's art quietly stops
    // existing: it draws whatever the buffer already held, or nothing, and
    // carries on without a word. 오셔너스 paints its own pixels into buffers it
    // owns - it calls no blit, image or string API at all - so a resource that
    // never arrives is invisible from every other angle. Say so here.
    if id < 0 {
        tracing::warn!("MC_knlGetResource: {id} is not a handle get_resource_id handed out");
        return Ok(-9); // M_E_INVALID
    }

    let name_bytes = read_null_terminated_string_bytes(context, id as _)?;
    // the handle was written by get_resource_id as utf-8, not guest-encoded
    let name = String::from_utf8_lossy(&name_bytes);

    let data = context.read_resource(&name).await?;

    if data.len() as u32 > buf_size {
        tracing::warn!(
            "MC_knlGetResource: {name:?} is {} bytes and was asked for into {buf_size}; refused, and the title is not told why",
            data.len()
        );
        return Ok(-1);
    }

    tracing::debug!("  resource {name:?} read, {} bytes", data.len());

    context.write_bytes(context.data_ptr(buf)?, &data)?;

    Ok(0)
}

pub async fn printk(context: &mut dyn WIPICContext, ptr_format: WIPICWord, a0: WIPICWord, a1: WIPICWord, a2: WIPICWord, a3: WIPICWord) -> Result<()> {
    tracing::debug!("MC_knlPrintk({ptr_format:#x}, {a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    let format_string = read_null_terminated_string_bytes(context, ptr_format)?;

    let result = sprintf(context, &format_string, &[a0, a1, a2, a3])?;

    // The one place the guest's bytes are read rather than handed back: this
    // goes to a log a person reads, so its EUC-KR becomes text here. Every
    // other caller writes the result straight back into guest memory and must
    // not.
    let text = encoding_rs::EUC_KR.decode(&result).0;

    context.system().platform().write_stdout(text.as_bytes());

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn sprintk(
    context: &mut dyn WIPICContext,
    dest: WIPICWord,
    ptr_format: WIPICWord,
    a0: WIPICWord,
    a1: WIPICWord,
    a2: WIPICWord,
    a3: WIPICWord,
    a4: WIPICWord,
    a5: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_knlSprintk({dest:#x}, {ptr_format:#x}, {a0}, {a1}, {a2}, {a3}, {a4}, {a5})",);

    let format_string = read_null_terminated_string_bytes(context, ptr_format)?;

    let result = sprintf(context, &format_string, &[a0, a1, a2, a3, a4, a5])?;

    write_null_terminated_string_bytes(context, dest, &result)?;

    // What `sprintf` returns is what it wrote, and both are the guest's own
    // bytes: the format goes in as it was read and the result goes back as it
    // was built, so a byte the guest passed is the byte it gets, and the count
    // is of those.
    Ok(result.len() as _)
}

pub async fn get_total_memory(_context: &mut dyn WIPICContext) -> Result<i32> {
    // A handset reported tens of MiB here; the old 1 MiB made memory-probing
    // titles believe the heap was already full and refuse to load.
    const TOTAL_MEMORY: i32 = 32 * 1024 * 1024;
    tracing::debug!("MC_knlGetTotalMemory() -> {TOTAL_MEMORY:#x}");

    Ok(TOTAL_MEMORY)
}

pub async fn get_free_memory(_context: &mut dyn WIPICContext) -> Result<i32> {
    const FREE_MEMORY: i32 = 24 * 1024 * 1024;
    tracing::debug!("MC_knlGetFreeMemory() -> {FREE_MEMORY:#x}");

    Ok(FREE_MEMORY)
}

pub async fn exit(context: &mut dyn WIPICContext, code: i32) -> Result<()> {
    tracing::debug!("MC_knlExit({code})");

    context.system().platform().exit();

    Ok(())
}

/// `MC_knlGetCurProgramID` - the id of the program asking.
///
/// One program runs here and its id is 1; see [`CURRENT_PROGRAM_ID`] for what
/// the rest of the program-control family makes of that.
pub async fn get_cur_program_id(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::debug!("MC_knlGetCurProgramID() -> {CURRENT_PROGRAM_ID}");

    Ok(CURRENT_PROGRAM_ID)
}

pub async fn get_program_name(context: &mut dyn WIPICContext, name_buf: WIPICWord, buf_size: i32) -> Result<i32> {
    tracing::debug!("MC_knlGetProgramName({name_buf:#x}, {buf_size})");

    let aid = context.system().aid().to_string();

    if buf_size < aid.len() as i32 + 1 {
        return Ok(-18); // M_E_SHORTBUF
    }

    let aid_bytes = aid.as_bytes();
    context.write_bytes(name_buf, aid_bytes)?;
    context.write_bytes(name_buf + aid_bytes.len() as u32, &[0])?;

    Ok(0)
}

/// The one program there is.
///
/// A handset runs a suite of programs and gives each one an id. This runtime
/// loads a single title and can neither install nor start another, so every
/// program-control call below answers against a world with exactly one program
/// in it - the one calling - and that program's id is 1, which is what
/// `MC_knlGetCurProgramID` has always reported here.
const CURRENT_PROGRAM_ID: WIPICWord = 1;

/// The name a program-control call was handed, for the log.
///
/// No title we have calls any of these, so their exact shapes are not pinned
/// down by anything, and the first word may not be a string pointer at all. An
/// unreadable one is reported as a number rather than turned into an error: a
/// guess about a call's shape must not be able to kill a title that was only
/// asking a question.
fn program_name_for_log(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> String {
    if ptr_name == 0 {
        return "(null)".to_string();
    }

    match read_null_terminated_string_bytes(context, ptr_name) {
        Ok(bytes) => encoding_rs::EUC_KR.decode(&bytes).0.into_owned(),
        Err(_) => format!("{ptr_name:#x}"),
    }
}

/// `MC_knlExecute` - start another program and hand it the screen.
///
/// There is no other program to start, so this says so. A title that gets here
/// is offering something outside itself - its suite's other games, a manual, the
/// operator's download page - and reads the failure as "not installed", which is
/// the truth.
///
/// Logged at info: it explains a menu item that does nothing, which is otherwise
/// invisible.
pub async fn execute(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<i32> {
    let name = program_name_for_log(context, ptr_name);
    tracing::info!("MC_knlExecute({name:?}) -> -12, nothing else is installed");

    Ok(-12) // M_E_NOENT
}

/// `MC_knlMExecute` - the same, for a program named by its own manager.
pub async fn mexecute(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<i32> {
    let name = program_name_for_log(context, ptr_name);
    tracing::info!("MC_knlMExecute({name:?}) -> -12, nothing else is installed");

    Ok(-12) // M_E_NOENT
}

/// `MC_knlLoad` - bring another program's code in without running it.
///
/// Loading is how a title reaches a shared library sitting beside it on the
/// handset. Nothing sits beside this one.
pub async fn load(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<i32> {
    let name = program_name_for_log(context, ptr_name);
    tracing::info!("MC_knlLoad({name:?}) -> -12, nothing else is installed");

    Ok(-12) // M_E_NOENT
}

/// `MC_knlMLoad` - the same, for a program named by its own manager.
pub async fn mload(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<i32> {
    let name = program_name_for_log(context, ptr_name);
    tracing::info!("MC_knlMLoad({name:?}) -> -12, nothing else is installed");

    Ok(-12) // M_E_NOENT
}

/// `MC_knlProgramStop` - stop a running program.
///
/// The only running program is the title itself, so asking to stop id 1 is
/// asking to quit, and that is honoured exactly as `MC_knlExit` is. Any other id
/// names a program that is not running, and gets told so.
pub async fn program_stop(context: &mut dyn WIPICContext, program_id: WIPICWord) -> Result<i32> {
    if program_id != CURRENT_PROGRAM_ID {
        tracing::info!("MC_knlProgramStop({program_id}) -> -12, no such program is running");

        return Ok(-12); // M_E_NOENT
    }

    tracing::info!("MC_knlProgramStop({program_id}) - the title asked to stop itself");

    context.system().platform().exit();

    Ok(0)
}

/// `MC_knlGetExecNames` - list the programs that can be started.
///
/// None can, so there is no list to write. This refuses without touching the
/// buffer it was given: a title that checks the result finds nothing to show,
/// and one that does not check reads whatever it had there, not something this
/// invented.
pub async fn get_exec_names(_context: &mut dyn WIPICContext) -> Result<i32> {
    tracing::info!("MC_knlGetExecNames() -> -12, nothing else is installed");

    Ok(-12) // M_E_NOENT
}

/// `MC_knlGetProgramInfo` - the handset's record of an installed program.
///
/// That record lives in the handset's program database, which this runtime does
/// not have: it knows the title's own descriptor and nothing about anything
/// else. The layout of the struct a caller passes is not established by any
/// title we have either, so this refuses rather than filling one in from a
/// guess.
pub async fn get_program_info(_context: &mut dyn WIPICContext) -> Result<i32> {
    tracing::info!("MC_knlGetProgramInfo() -> -12, there is no program database to read");

    Ok(-12) // M_E_NOENT
}

/// `MC_knlGetParentProgramID` - who launched this program.
///
/// Nobody did: a title starts at the top here rather than from a menu program,
/// so there is no parent, which is 0. LGT answers the same.
pub async fn get_parent_program_id(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::debug!("MC_knlGetParentProgramID() -> 0, nothing launched this");

    Ok(0)
}

/// `MC_knlGetAppManagerID` - the id of the handset's application manager.
///
/// There is no application manager; the title is the only program. A caller
/// usually wants this to hand to `MC_knlExecute` to get back to the menu, which
/// answers that there is nothing to go back to.
pub async fn get_app_manager_id(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::debug!("MC_knlGetAppManagerID() -> 0, there is no application manager");

    Ok(0)
}

/// The security level the title's own descriptor declares, if it declares one.
///
/// KTF archives carry `__adf__`, and its `SLvl` line is the level the handset
/// stored for this program - `SLvl:00142F9C` in 투스워즈, `00142F1C` in
/// 드래곤하트. It is written as eight hex digits and reads as a mask, the two
/// values above differing in one bit. Reading it back is the one answer here
/// that comes from the title rather than from us.
///
/// Read through the filesystem rather than as a resource, because `__adf__` sits
/// beside the jar in the archive rather than inside it. LGT titles have no such
/// file and so declare nothing.
async fn declared_access_level(context: &mut dyn WIPICContext) -> Option<u32> {
    let data = {
        let filesystem = context.system().filesystem();

        let size = filesystem.size("__adf__").await?;
        let mut data = vec![0u8; size];
        filesystem.read("__adf__", 0, size, &mut data).await?;

        data
    };

    for line in data.split(|x| *x == b'\n') {
        if let Some(value) = line.strip_prefix(b"SLvl:") {
            return u32::from_str_radix(&descriptor_value(value), 16).ok();
        }
    }

    None
}

/// `MC_knlGetAccessLevel` - how much a program is trusted.
///
/// Answered for the calling program whatever it asks about, because it is the
/// only program: an id it could have got from anywhere else names something that
/// does not exist here. That also keeps the answer right if the call turns out
/// to take no argument at all, which nothing we have settles.
///
/// The level is the one the title's own descriptor declares. What a title does
/// with it is not established by anything here - no title we have calls this -
/// so the risk of the alternative decided it: a made-up level could be lower
/// than a title's own expectation and have it refuse itself work this runtime
/// would have done. The declared value cannot be, because it is what the handset
/// the title shipped for reported.
pub async fn get_access_level(context: &mut dyn WIPICContext) -> Result<i32> {
    match declared_access_level(context).await {
        Some(level) => {
            tracing::info!("MC_knlGetAccessLevel() -> {level:#010x}, as declared by SLvl");

            Ok(level as i32)
        }
        None => {
            tracing::warn!("MC_knlGetAccessLevel() -> -12, this archive declares no security level");

            Ok(-12) // M_E_NOENT
        }
    }
}

#[cfg(test)]
mod test {
    use alloc::{boxed::Box, string::String, sync::Arc};
    use core::sync::atomic::{AtomicBool, Ordering};

    use test_utils::{TestPlatform, TestPlatformEvent};
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::{ByteRead, ByteWrite, Result, read_null_terminated_string_bytes, write_null_terminated_string_bytes};

    use crate::{WIPICContext, context::test::TestContext, method::MethodImpl};

    use super::{
        alloc, calloc, execute, free, get_access_level, get_app_manager_id, get_exec_names, get_parent_program_id, get_program_info, get_resource,
        get_resource_id, get_system_property, load, mexecute, mload, program_stop, sprintk,
    };

    #[futures_test::test]
    async fn test_sprintk() -> Result<()> {
        let mut context = TestContext::new();

        let sprintk = sprintk.into_body();

        let format = context.alloc_raw(10).unwrap();
        let dest = context.alloc_raw(10).unwrap();

        write_null_terminated_string_bytes(&mut context, format, "%d".as_bytes()).unwrap();
        sprintk
            .call(&mut context, Box::new([dest, format, 1234, 0, 0, 0, 0, 0, 0, 0]))
            .await
            .unwrap();
        let result = read_null_terminated_string_bytes(&context, dest).unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "1234");

        write_null_terminated_string_bytes(&mut context, format, "test %02d".as_bytes()).unwrap();
        sprintk
            .call(&mut context, Box::new([dest, format, 1, 0, 0, 0, 0, 0, 0, 0]))
            .await
            .unwrap();
        let result = read_null_terminated_string_bytes(&context, dest).unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "test 01");

        Ok(())
    }

    #[futures_test::test]
    async fn test_get_system_property_min() -> Result<()> {
        let mut context = TestContext::new();
        let id = context.alloc_raw(16).unwrap();
        let out = context.alloc_raw(16).unwrap();

        write_null_terminated_string_bytes(&mut context, id, b"MIN").unwrap();

        assert_eq!(get_system_property(&mut context, id, out, 16).await.unwrap(), 0);
        let result = read_null_terminated_string_bytes(&context, out).unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "01046119269");

        Ok(())
    }

    #[futures_test::test]
    async fn test_get_system_property_media_devices() -> Result<()> {
        // 판타지포에버3 reads this and looks for `Yamaha_MA3` before it will
        // create a clip at all, so the answer has to name the SMAF profiles the
        // audio path plays and fit the buffer a title hands over.
        let mut context = TestContext::new();
        let id = context.alloc_raw(16).unwrap();
        let out = context.alloc_raw(128).unwrap();

        write_null_terminated_string_bytes(&mut context, id, b"MEDIADEVICES").unwrap();

        assert_eq!(get_system_property(&mut context, id, out, 128).await.unwrap(), 0);
        let result = String::from_utf8(read_null_terminated_string_bytes(&context, out).unwrap()).unwrap();
        assert!(result.contains("Yamaha_MA3"), "{result}");

        Ok(())
    }

    #[futures_test::test]
    async fn test_phonenumber_from_certification_file() -> Result<()> {
        // A title whose `certification` file is the subscriber number as ASCII
        // digits should see PHONENUMBER report exactly that, so its own
        // strcmp(certification, PHONENUMBER) authentication passes.
        let mut context = TestContext::new().with_resource("certification", b"01000000000\0");
        let id = context.alloc_raw(16).unwrap();
        let out = context.alloc_raw(16).unwrap();

        write_null_terminated_string_bytes(&mut context, id, b"PHONENUMBER").unwrap();

        assert_eq!(get_system_property(&mut context, id, out, 16).await.unwrap(), 0);
        let result = read_null_terminated_string_bytes(&context, out).unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "01000000000");

        Ok(())
    }

    #[futures_test::test]
    async fn test_zero_size_memory_returns_null() -> Result<()> {
        let mut context = TestContext::new();

        assert_eq!(alloc(&mut context, 0).await.unwrap().0, 0);
        // calloc of zero returns a unique, freeable non-null pointer, matching
        // the reference allocator that a font loader relies on.
        let zero = calloc(&mut context, 0).await.unwrap();
        assert_ne!(zero.0, 0);
        free(&mut context, zero).await.unwrap();
        assert_eq!(free(&mut context, wipi_types::wipic::WIPICIndirectPtr(0)).await.unwrap().0, 0);

        Ok(())
    }

    #[futures_test::test]
    async fn test_get_system_property_non_utf8() -> Result<()> {
        let mut context = TestContext::new();
        let id = context.alloc_raw(16).unwrap();
        let out = context.alloc_raw(16).unwrap();

        // EUC-KR "한글"
        write_null_terminated_string_bytes(&mut context, id, &[0xc7, 0xd1, 0xb1, 0xdb]).unwrap();

        assert_eq!(get_system_property(&mut context, id, out, 16).await.unwrap(), -9);

        Ok(())
    }

    #[futures_test::test]
    async fn test_get_resource_id_non_utf8() -> Result<()> {
        let mut context = TestContext::new();
        let name = context.alloc_raw(16).unwrap();
        let size = context.alloc_raw(4).unwrap();

        write_null_terminated_string_bytes(&mut context, name, &[0xc7, 0xd1, 0xb1, 0xdb]).unwrap();

        assert_eq!(get_resource_id(&mut context, name, size).await.unwrap(), -12);

        Ok(())
    }

    #[futures_test::test]
    async fn test_get_resource_euc_kr_roundtrip() -> Result<()> {
        let data = [1u8, 2, 3, 4];
        let mut context = TestContext::new().with_resource("한글", &data);
        let name = context.alloc_raw(16).unwrap();
        let size = context.alloc_raw(4).unwrap();

        write_null_terminated_string_bytes(&mut context, name, &[0xc7, 0xd1, 0xb1, 0xdb]).unwrap();

        let handle = get_resource_id(&mut context, name, size).await.unwrap();
        assert!(handle >= 0);

        let mut size_bytes = [0; 4];
        context.read_bytes(size, &mut size_bytes).unwrap();
        assert_eq!(u32::from_le_bytes(size_bytes), 4);

        let buf = context.alloc(4).unwrap();
        assert_eq!(get_resource(&mut context, handle, buf, 4).await.unwrap(), 0);

        let mut result = [0; 4];
        context.read_bytes(context.data_ptr(buf).unwrap(), &mut result).unwrap();
        assert_eq!(result, data);

        Ok(())
    }

    #[futures_test::test]
    async fn test_missing_resource_clears_size() -> Result<()> {
        let mut context = TestContext::new();
        let name = context.alloc_raw(16).unwrap();
        let size = context.alloc_raw(4).unwrap();

        write_null_terminated_string_bytes(&mut context, name, b"missing").unwrap();
        context.write_bytes(size, &[0xff; 4]).unwrap();

        assert_eq!(get_resource_id(&mut context, name, size).await.unwrap(), -12);
        let mut result = [0; 4];
        context.read_bytes(size, &mut result).unwrap();
        assert_eq!(u32::from_le_bytes(result), 0);

        Ok(())
    }

    /// Every way a title can ask for another program is answered "no such
    /// program", because there is not one.
    #[futures_test::test]
    async fn nothing_else_is_installed() {
        let mut context = program_control_context(None);
        let name = context.alloc_raw(16).unwrap();
        write_null_terminated_string_bytes(&mut context, name, b"01032A4F").unwrap();

        assert_eq!(execute(&mut context, name).await.unwrap(), -12);
        assert_eq!(mexecute(&mut context, name).await.unwrap(), -12);
        assert_eq!(load(&mut context, name).await.unwrap(), -12);
        assert_eq!(mload(&mut context, name).await.unwrap(), -12);
        assert_eq!(get_exec_names(&mut context).await.unwrap(), -12);
        assert_eq!(get_program_info(&mut context).await.unwrap(), -12);
    }

    /// Nothing launched the title and there is no manager to go back to.
    #[futures_test::test]
    async fn there_is_no_parent_and_no_manager() {
        let mut context = program_control_context(None);

        assert_eq!(get_parent_program_id(&mut context).await.unwrap(), 0);
        assert_eq!(get_app_manager_id(&mut context).await.unwrap(), 0);
    }

    /// A name pointer that is not a name must not be able to kill the call: the
    /// shapes of these are not settled by any title, so a wrong first word is a
    /// thing that can happen, and it is a log detail either way.
    #[futures_test::test]
    async fn an_unreadable_name_is_still_answered() {
        let mut context = program_control_context(None);

        assert_eq!(execute(&mut context, 0).await.unwrap(), -12);
        assert_eq!(execute(&mut context, 0xdead_beef).await.unwrap(), -12);
    }

    /// Stopping the only program that runs is quitting, and is honoured.
    #[futures_test::test]
    async fn stopping_the_title_quits_it() {
        let exited = Arc::new(AtomicBool::new(false));
        let mut context = program_control_context(Some(exited.clone()));

        assert_eq!(program_stop(&mut context, 1).await.unwrap(), 0);
        assert!(exited.load(Ordering::Relaxed));
    }

    /// Stopping anything else stops nothing: an id that is not the title's names
    /// a program that is not running.
    #[futures_test::test]
    async fn stopping_anything_else_stops_nothing() {
        let exited = Arc::new(AtomicBool::new(false));
        let mut context = program_control_context(Some(exited.clone()));

        assert_eq!(program_stop(&mut context, 7).await.unwrap(), -12);
        assert!(!exited.load(Ordering::Relaxed));
    }

    /// The access level is the one the title's own `__adf__` declares - here
    /// 투스워즈's, read back as the hex it is written in.
    #[futures_test::test]
    async fn the_access_level_is_the_declared_one() {
        let mut context = program_control_context(None);
        context.system().filesystem().add_virtual(
            "__adf__",
            b"PID:PD005966\nAID:010100D2\nSLvl:00142F9C\nSLvl2:00000183\nMClass:MoApp\n".to_vec(),
        );

        assert_eq!(get_access_level(&mut context).await.unwrap(), 0x0014_2f9c);
    }

    /// An archive that declares no level is said to declare none, rather than
    /// answered with a number nobody wrote down.
    #[futures_test::test]
    async fn an_archive_without_a_declaration_has_no_level() {
        let mut context = program_control_context(None);
        context.system().filesystem().add_virtual("__adf__", b"AID:010100D2\n".to_vec());

        assert_eq!(get_access_level(&mut context).await.unwrap(), -12);

        // And neither does one with no descriptor at all, which is every LGT
        // archive.
        let mut context = program_control_context(None);
        assert_eq!(get_access_level(&mut context).await.unwrap(), -12);
    }

    /// A context whose platform records whether the title was asked to quit.
    fn program_control_context(exited: Option<Arc<AtomicBool>>) -> TestContext {
        let platform = TestPlatform::with_event_handler(move |event| {
            if let TestPlatformEvent::Exit = event
                && let Some(exited) = &exited
            {
                exited.store(true, Ordering::Relaxed);
            }
        });

        TestContext::with_system(System::new(Box::new(platform), "test-pid", "test-aid", DefaultTaskRunner))
    }
}
