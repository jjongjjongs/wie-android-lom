//! Wiring that loads the user-supplied firmware BIOS, binds its imports, and
//! (P2) drives its init so the real firmware code begins running.
//!
//! The firmware is a BIOS: proprietary, never committed, supplied by the user
//! at runtime under the reference's own filename. Its absence is the default and
//! leaves the emulator entirely on the Rust platform. When present, this maps
//! the firmware beside the game, binds every import (the C-runtime primitives to
//! real Rust handlers, the rest to traceable stubs), and runs the firmware's
//! constructors and `MH_sysHalInit`. Init failures are logged, not fatal, so a
//! bring-up crash never stops the game from running on the Rust platform.

use alloc::{collections::BTreeMap, format, string::String, sync::Arc, vec, vec::Vec};

use spin::Mutex;

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, ResultWriter, SvcId};
use wie_util::{ByteWrite, Result, read_generic, write_generic};
use wie_wipi_c::api::graphics;

use super::SVC_CATEGORY_FIRMWARE_MDA;
use super::firmware::{FirmwareImage, ImportResolver, load_firmware};
use super::firmware_libc::{FirmwareImportNames, register_firmware_libc_handler};

/// The reference's own name for the firmware image. The user supplies this file.
pub const BIOS_FILENAME: &str = "libarm32_lgt_system.so";

/// Where the firmware is mapped. The image is linked at 0, so it needs a base
/// clear of the game module and the allocator heap. A collision would surface in
/// the load log as a mapping error.
const FIRMWARE_BASE: u32 = 0x6000_0000;

/// Size of the placeholder ctype data tables. Zero-filled for now; the init
/// trace will show if the firmware needs real classification data.
const CTYPE_TABLE_SIZE: u32 = 512;

/// The firmware's data-object imports, resolved to allocated tables rather than
/// code stubs. `_ctype_` and the case tables are the standard C library data.
fn is_ctype_data(name: &str) -> bool {
    matches!(name, "_ctype_" | "_tolower_tab_" | "_toupper_tab_")
}

/// Binds every firmware import: ctype data to allocated tables, and everything
/// else (functions) to the firmware-libc category, which implements the needed
/// C-runtime primitives and traces the rest. It never returns `None`, so the
/// load leaves no import unbound.
///
/// Firmware imports go through their own category rather than the game's
/// `stdlib` category, which is not registered until the game module loads -
/// after firmware init has already started running.
struct FirmwareResolver {
    names: FirmwareImportNames,
    data_tables: BTreeMap<String, u32>,
}

impl ImportResolver for FirmwareResolver {
    fn resolve(&mut self, core: &mut ArmCore, name: &str) -> Result<Option<u32>> {
        if is_ctype_data(name) {
            if let Some(&addr) = self.data_tables.get(name) {
                return Ok(Some(addr));
            }
            let addr = Allocator::alloc(core, CTYPE_TABLE_SIZE)?;
            core.write_bytes(addr, &vec![0u8; CTYPE_TABLE_SIZE as usize])?;
            self.data_tables.insert(name.into(), addr);
            return Ok(Some(addr));
        }

        // Everything else: intern the name (its index is the trampoline id) and
        // mint a firmware-libc stub.
        let index = {
            let mut names = self.names.lock();
            let index = names.len() as u32;
            names.push(name.into());
            index
        };
        Ok(Some(core.make_svc_stub(super::SVC_CATEGORY_FIRMWARE_LIBC, index)?))
    }
}

/// Whether to route the game's media calls to the firmware.
///
/// OFF while the firmware audio path is still being brought up. Routing only
/// `MC_mdaClipCreate`/`PutData`/`AllocPlayer` to the firmware (as we do) hands
/// the game a *firmware* clip, but the game's play call still lands on wie's
/// Rust `media::play`, which reads the clip as a Rust `MdaClip` and fails with
/// `InvalidHandle` - so mixed routing produces no firmware sound *and* breaks
/// the Rust audio path that works for every other game. The device trace has
/// served its purpose (the media path reaches `getWipiMediaManager`, which needs
/// a JNI-backed Java audio layer); until that bridge actually produces PCM,
/// keep routing off so all games keep their working Rust audio. The firmware
/// still loads and boots (P1/P2) and the synthetic JNIEnv is installed, all
/// dormant, so bring-up can continue behind this flag.
const ENABLE_MDA_ROUTING: bool = false;

/// Whether to route each media call through a trace shim instead of straight to
/// the firmware. ON logs every `MC_mda*` the game makes - name, args, and return
/// value - so the otherwise-silent media call sequence (which jumps directly
/// into firmware code) becomes visible in the device log. It adds a call layer
/// per media call, so it is a diagnostic switch, not the shipping path.
const ENABLE_MDA_TRACE: bool = true;

/// SVC-id -> (import name, firmware export address) for the media trace shim,
/// shared between `build_mda_routes` (which fills it, one entry per route) and
/// the trace handler (which reads it to name the call and find the firmware
/// function to invoke).
type FirmwareMdaRoutes = Arc<Mutex<Vec<(String, u32)>>>;

/// Registers the media-route trace handler. Each routed call arrives as an SVC
/// whose id indexes `routes`; the handler logs the call, invokes the firmware
/// export with the same arguments, logs the result, and returns it to the game.
fn register_firmware_mda_handler(core: &mut ArmCore, routes: FirmwareMdaRoutes) -> Result<()> {
    #[derive(Clone)]
    struct MdaTraceContext {
        routes: FirmwareMdaRoutes,
    }

    async fn handle_mda_svc(core: &mut ArmCore, context: &mut MdaTraceContext, id: SvcId) -> Result<()> {
        let (_, lr) = core.read_pc_lr()?;
        let index = id.0 as usize;
        let (name, addr) = context
            .routes
            .lock()
            .get(index)
            .cloned()
            .unwrap_or_else(|| (format!("mda_route#{index}"), 0));

        let a0 = core.read_param(0)?;
        let a1 = core.read_param(1)?;
        let a2 = core.read_param(2)?;
        let a3 = core.read_param(3)?;
        tracing::info!("[mda] {name}(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) -> firmware {addr:#x}");

        if addr == 0 {
            return 0u32.write(core, lr);
        }

        // Invoke the firmware export with the game's arguments. `run_function`
        // saves and restores the game's register context around the nested call
        // on success, so it is transparent to the game apart from r0.
        //
        // On a *fault* inside the firmware, `run_function` propagates the error
        // without restoring the caller context, leaving the core in the
        // firmware's faulted state. We snapshot the game's context first so we
        // can log the firmware fault (its own registers/stack) and then restore
        // the game and hand it a 0, letting the trace show the whole media call
        // sequence instead of stopping at the first firmware-internal fault.
        let saved = core.save_context();
        match core.run_function::<u32>(addr, &[a0, a1, a2, a3]).await {
            Ok(ret) => {
                tracing::info!("[mda] {name} returned {ret:#x}");
                ret.write(core, lr)
            }
            Err(error) => {
                tracing::error!("[mda] {name} faulted inside firmware: {error:?}\n{}", core.dump_reg_stack(0x40));
                core.restore_context(&saved);
                0u32.write(core, lr)
            }
        }
    }

    core.register_svc_handler(SVC_CATEGORY_FIRMWARE_MDA, handle_mda_svc, &MdaTraceContext { routes })
}

/// The game's WIPI-C media import indices (table `0x1fb`) paired with the
/// firmware export that implements each. Routing these to the firmware is the
/// P3 audio cutover: the game's clip lifecycle runs real firmware code.
const MDA_ROUTES: &[(u32, &str)] = &[
    (0x4b0, "MC_mdaClipCreate"),
    (0x4b1, "MC_mdaClipFree"),
    (0x4b3, "MC_mdaClipPutData"),
    (0x4b6, "MC_mdaClipControl"), // the game's play/control call (unk15)
    (0x4b9, "MC_mdaClipSetVolume"),
    (0x4c5, "MC_mdaClipAllocPlayer"),
    (0x4c6, "MC_mdaClipFreePlayer"),
];

/// Builds the WIPI-C-index -> firmware-address map for the media functions the
/// loaded firmware provides. Indices whose export is missing are skipped, so a
/// firmware that lacks one simply keeps the Rust stub for it.
pub fn build_mda_routes(core: &mut ArmCore, _system: &System, image: &FirmwareImage) -> Result<BTreeMap<u32, u32>> {
    let mut routes = BTreeMap::new();
    if !ENABLE_MDA_ROUTING {
        tracing::info!("Firmware media routing disabled (needs a current dprocess first); game keeps the Rust audio path");
        return Ok(routes);
    }

    // When tracing, each route resolves to an SVC trace stub (which logs and
    // then invokes the firmware) rather than the firmware address directly.
    let trace: Option<FirmwareMdaRoutes> = if ENABLE_MDA_TRACE {
        let table: FirmwareMdaRoutes = Arc::new(Mutex::new(Vec::new()));
        register_firmware_mda_handler(core, table.clone())?;
        Some(table)
    } else {
        None
    };

    for (index, name) in MDA_ROUTES {
        match image.export(name) {
            Some(addr) => {
                let target = if let Some(table) = &trace {
                    let stub_id = {
                        let mut table = table.lock();
                        let id = table.len() as u32;
                        table.push(((*name).into(), addr));
                        id
                    };
                    let stub = core.make_svc_stub(SVC_CATEGORY_FIRMWARE_MDA, stub_id)?;
                    tracing::info!("Routing WIPI-C media import {index:#x} -> [traced] firmware {name} {addr:#x} via stub {stub:#x}");
                    stub
                } else {
                    tracing::info!("Routing WIPI-C media import {index:#x} -> firmware {name} {addr:#x}");
                    addr
                };
                routes.insert(*index, target);
            }
            None => tracing::warn!("Firmware has no {name}; leaving WIPI-C import {index:#x} on the Rust stub"),
        }
    }
    Ok(routes)
}

/// Reads the whole BIOS file out of the filesystem overlay.
async fn read_bios(system: &System, size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    let mut read = 0;
    while read < size {
        match system.filesystem().read(BIOS_FILENAME, read, size - read, &mut data[read..]).await {
            Some(0) | None => break,
            Some(n) => read += n,
        }
    }
    data.truncate(read);
    data
}

/// Loads the firmware BIOS if the user supplied it, binding every import and
/// logging what was mapped. Returns the loaded image, or `None` when no BIOS is
/// present. Does not run the firmware; call `run_firmware_init` for that.
pub async fn try_load_bios(core: &mut ArmCore, system: &System) -> Result<Option<FirmwareImage>> {
    if !system.filesystem().exists(BIOS_FILENAME).await {
        tracing::info!("Firmware BIOS support present; no {BIOS_FILENAME} supplied, staying on the Rust platform");
        return Ok(None);
    }

    let size = system.filesystem().size(BIOS_FILENAME).await.unwrap_or(0);
    let data = read_bios(system, size).await;
    tracing::info!("Loading firmware BIOS {BIOS_FILENAME} ({} bytes) at base {FIRMWARE_BASE:#x}", data.len());

    // The handset's own bitmap face lives in this image, and drawing text with
    // it needs nothing else from the firmware - so take it before the mapping,
    // and a title gets the handset's glyphs even if the rest of the load does
    // not come up. See `wie_wipi_c::api::graphics::install_bios_font`.
    if graphics::install_bios_font(&data) {
        tracing::info!("Firmware bitmap face installed; text is drawn from the handset's own glyphs");
    } else {
        tracing::warn!("No bitmap face found in {BIOS_FILENAME}; text stays on the outline font");
    }

    let names: FirmwareImportNames = Arc::new(Mutex::new(Vec::new()));
    register_firmware_libc_handler(core, system, names.clone())?;

    let mut resolver = FirmwareResolver {
        names,
        data_tables: BTreeMap::new(),
    };
    let image = load_firmware(core, &data, FIRMWARE_BASE, &mut resolver)?;

    tracing::info!(
        "Firmware mapped: base {:#x}, entry {:#x}, {} export(s), {} unresolved import(s), {} init fn(s)",
        image.base,
        image.entry,
        image.exports.len(),
        image.unresolved_imports.len(),
        image.init_array.len() + usize::from(image.init.is_some())
    );

    for name in ["MH_sysHalInit", "dlet_start", "InitPCSAutomata", "AND_mdaInit", "media_manager_init"] {
        match image.export(name) {
            Some(addr) => tracing::info!("Firmware export {name} -> {addr:#x}"),
            None => tracing::info!("Firmware export {name} not found"),
        }
    }

    // The firmware's media manager is a JNI object: it fetches the current
    // JNIEnv and calls Java through it. There is no ART here, so getJNIEnv
    // returns null and the media path jumps through a null table slot. Install a
    // synthetic JNIEnv into the firmware's kernel_jni_env global so those calls
    // land on serviceable (for now, traced) stubs instead of crashing.
    match image.export("kernel_jni_env") {
        Some(addr) => {
            super::firmware_jni::install_firmware_jni_env(core, system, addr)?;
        }
        None => tracing::warn!("Firmware has no kernel_jni_env export; media JNI path will not have an env"),
    }

    Ok(Some(image))
}

/// The firmware runtime boot sequence, in call order. `dprocess_init` and
/// `dthread_init` stand up the DTHREAD runtime (memory pools, config) so
/// `dprocess_get_current` starts returning a live process; `MH_sysHalInit` is
/// the HAL init that needs that context. Each is called with no arguments,
/// matching their disassembly.
///
/// `media_manager_init` (not the leaf `AND_mdaInit`) is the real media bring-up:
/// from RE at 0x1926b4 it `dlink_init`s the clip list head - the global
/// `media_create_clip_ex` inserts every clip into - allocates and parses the
/// device descriptor table, registers each audio device, and calls `AND_mdaInit`
/// itself at the right point. Calling `AND_mdaInit` alone (as we did) only
/// memset the MH device-instance table and left the clip list a zeroed .bss
/// global, so the game's first `MC_mdaClipCreate` operated on an uninitialised
/// list and crashed. It reads the statically-relocated media-manager slot
/// (`[GOT+0xa68]`), so it needs no argument, but it allocates, so the current
/// process must already be installed (it runs after `dprocess_init`).
const BOOT_SEQUENCE: &[&str] = &[
    "dmempage_init",
    "dmemory_init",
    "dprocess_init",
    "dthread_init",
    "MH_sysHalInit",
    "WPKnl_Init",
    "media_manager_init",
];

/// The firmware entry points needed to drive init, lifted out of the image so
/// they can move into an isolated task.
pub struct FirmwareInitPlan {
    pub init: Option<u32>,
    pub init_array: Vec<u32>,
    /// `(name, guest address)` for each boot step present in the image.
    pub boot_steps: Vec<(String, u32)>,
    /// Load base, for computing the firmware's GOT-relative globals.
    pub base: u32,
    /// Addresses for establishing the current process the media path needs.
    pub dmemory_initheap_ex: Option<u32>,
    pub dprocess_get_current: Option<u32>,
    pub dmemory_alloc: Option<u32>,
}

impl FirmwareInitPlan {
    pub fn from_image(image: &FirmwareImage) -> Self {
        let boot_steps = BOOT_SEQUENCE
            .iter()
            .filter_map(|&name| image.export(name).map(|addr| (name.into(), addr)))
            .collect();
        Self {
            init: image.init,
            init_array: image.init_array.clone(),
            boot_steps,
            base: image.base,
            dmemory_initheap_ex: image.export("dmemory_initheap_ex"),
            dprocess_get_current: image.export("dprocess_get_current"),
            dmemory_alloc: image.export("dmemory_alloc"),
        }
    }
}

/// Firmware GOT layout for `dprocess_get_current` (from RE at `0xecb18`): the
/// GOT base is `base + GOT_BASE_OFFSET`, and it reads the init flag and the
/// current-process holder through two GOT slots.
const GOT_BASE_OFFSET: u32 = 0x34a0e8;
const INIT_FLAG_GOT_SLOT: u32 = 0x8d4;
const CURRENT_HOLDER_GOT_SLOT: u32 = 0x2f0;
/// The `"os"` allocator-type string in firmware rodata, the type
/// `create_process` passes to `dmemory_initheap_ex` for a process heap.
const OS_ALLOC_TYPE_OFFSET: u32 = 0x29d158;
/// The media clip list head, a firmware global that `media_create_clip_ex`
/// (behind `MC_mdaClipCreate`) inserts each clip into via `dlink_insert_prev`.
/// `AND_mdaInit` initialises it as an empty circular list (head->next =
/// head->prev = head); if it ran without an allocator/current process the head
/// is left zeroed and the first insert corrupts state. Logged after boot so the
/// device trace shows whether init took. From RE: `r8(GOT) + 0x18411c`.
const MEDIA_CLIP_LIST_HEAD_OFFSET: u32 = GOT_BASE_OFFSET + 0x18411c;
/// Layout of the process struct used by `dmemory_alloc`: it reads the allocator
/// at `+0x24`; `dmemory_initheap_ex` initialises the heap header at `+0x1c`, and
/// the process name lives at `+148`.
const PROCESS_STRUCT_SIZE: u32 = 404;
const PROCESS_HEAP_HEADER_OFFSET: u32 = 0x1c;
const PROCESS_NAME_OFFSET: u32 = 148;
/// Size of the heap handed to the synthetic process.
const PROCESS_HEAP_SIZE: u32 = 512 * 1024;

/// Establishes a "current process" so the firmware's media path works.
///
/// `MC_mda*` reach `dmemory_alloc`, which calls
/// `dprocess_get_current()->allocator[+4]`. Without a current process that is
/// null. Rather than `dprocess_create` (which also spawns a JNI thread via
/// `startThread` - a whole runtime layer the audio path does not need), we build
/// a minimal process struct and initialise just its heap/allocator with
/// `dmemory_initheap_ex` (the same call `create_process` uses), then point the
/// current-process global at it. The media path only needs the allocator.
async fn setup_current_process(core: &mut ArmCore, plan: &FirmwareInitPlan) -> Result<()> {
    let Some(dmemory_initheap_ex) = plan.dmemory_initheap_ex else {
        tracing::warn!("Firmware has no dmemory_initheap_ex; media path will not have a current process");
        return Ok(());
    };

    // A minimal process struct and its heap.
    let process = Allocator::alloc(core, PROCESS_STRUCT_SIZE)?;
    core.write_bytes(process, &vec![0u8; PROCESS_STRUCT_SIZE as usize])?;
    core.write_bytes(process + PROCESS_NAME_OFFSET, b"wie\0")?;
    let heap = Allocator::alloc(core, PROCESS_HEAP_SIZE)?;

    // dmemory_initheap_ex(alloc_out = process+0x1c, type = "os", 1,
    //                     name = process+148, heap_base, heap_size).
    let alloc_out = process + PROCESS_HEAP_HEADER_OFFSET;
    let os_type = plan.base.wrapping_add(OS_ALLOC_TYPE_OFFSET);
    let name_ptr = process + PROCESS_NAME_OFFSET;
    let rc: u32 = match core
        .run_function(dmemory_initheap_ex, &[alloc_out, os_type, 1, name_ptr, heap, PROCESS_HEAP_SIZE])
        .await
    {
        Ok(rc) => rc,
        Err(error) => {
            tracing::error!("dmemory_initheap_ex faulted: {error:?}\n{}", core.dump_reg_stack(0x40));
            return Err(error);
        }
    };
    if (rc as i32) < 0 {
        tracing::warn!("dmemory_initheap_ex returned {rc}; process heap not initialised");
        return Ok(());
    }
    let allocator: u32 = read_generic(core, process + 0x24)?;
    tracing::info!("Synthetic process {process:#x}: heap {heap:#x}, allocator@+0x24 = {allocator:#x}");

    // Install it as the current process: write it into the holder global and
    // set the init flag, both reached through the GOT slots.
    let got = plan.base.wrapping_add(GOT_BASE_OFFSET);
    let holder: u32 = read_generic(core, got + CURRENT_HOLDER_GOT_SLOT)?;
    write_generic(core, holder + 4, process)?;
    let flag_global: u32 = read_generic(core, got + INIT_FLAG_GOT_SLOT)?;
    write_generic(core, flag_global, 1u32)?;
    tracing::info!("Current process installed: holder {holder:#x}+4 = {process:#x}, flag {flag_global:#x} = 1");

    // Verify by asking the firmware itself.
    if let Some(get_current) = plan.dprocess_get_current {
        let current: u32 = core.run_function(get_current, &[]).await?;
        if current == process {
            tracing::info!("dprocess_get_current() -> {current:#x} (matches); media path has a current process");
        } else {
            tracing::warn!("dprocess_get_current() -> {current:#x}, expected {process:#x}; current-process wiring is off");
        }
    }

    // Confirm the allocator works end to end: dmemory_alloc is exactly the call
    // the media functions crashed on before a current process existed.
    if let Some(dmemory_alloc) = plan.dmemory_alloc {
        match core.run_function::<u32>(dmemory_alloc, &[64]).await {
            Ok(ptr) if ptr != 0 => tracing::info!("dmemory_alloc(64) -> {ptr:#x}; the media allocator path works"),
            Ok(_) => tracing::warn!("dmemory_alloc(64) -> 0; allocator returned null"),
            Err(error) => tracing::error!("dmemory_alloc(64) faulted: {error:?}"),
        }
    }

    Ok(())
}

/// Drives the firmware's init: the C/C++ constructors (`DT_INIT` + init array)
/// and then `MH_sysHalInit`. This is the P2 experiment - it shows whether the
/// firmware executes under our interpreter and which imports its init touches.
///
/// Meant to run as its own task (see `do_start`) so a bring-up crash here cannot
/// corrupt the game thread. Errors are returned to the caller, which logs them.
pub async fn run_firmware_init(core: &mut ArmCore, plan: &FirmwareInitPlan) -> Result<()> {
    if let Some(init) = plan.init {
        tracing::info!("Running firmware DT_INIT at {init:#x}");
        let _: u32 = core.run_function(init, &[]).await?;
    }

    for (index, ctor) in plan.init_array.iter().enumerate() {
        tracing::info!("Running firmware init_array[{index}] at {ctor:#x}");
        let _: u32 = core.run_function(*ctor, &[]).await?;
    }

    // The current process must exist *before* the media/kernel init runs.
    // `AND_mdaInit` (and the kernel init before it) allocate and initialise the
    // media clip list head and device structures via `dmemory_alloc`, which
    // dereferences `dprocess_get_current()->allocator`. Run it too early - as we
    // did when `setup_current_process` came last - and those allocations return
    // null, leaving the clip list head (a firmware global) uninitialised; the
    // game's first `MC_mdaClipCreate` then inserts into that garbage list and
    // corrupts state (observed: the GOT-base register clobbered, a wild branch
    // into game code). So we establish the process the moment the memory and
    // process subsystems are up (right after `dprocess_init`) and before the
    // media init that needs it.
    let mut current_process_ready = false;
    for (name, addr) in &plan.boot_steps {
        tracing::info!("Running firmware {name} at {addr:#x}");
        match core.run_function::<u32>(*addr, &[]).await {
            Ok(result) => tracing::info!("{name} returned {result:#x}"),
            Err(error) => {
                // Dump the guest state at the fault so the bring-up log shows
                // exactly where the boot sequence broke.
                tracing::error!("{name} faulted: {error:?}\n{}", core.dump_reg_stack(0x40));
                return Err(error);
            }
        }

        // `dprocess_init` allocates the current-process holder global and
        // `dmemory_init` registers the "os" allocator type - both of which
        // `setup_current_process` needs - so install the process here, before
        // the media/kernel init that allocates against it.
        if name == PROCESS_SUBSYSTEM_READY_STEP {
            setup_current_process(core, plan).await?;
            current_process_ready = true;
        }
    }

    // Fallback: if the sequence did not include the expected step (e.g. a
    // firmware variant renames it), still establish the process so the media
    // allocator has a context, even if some earlier init missed it.
    if !current_process_ready {
        setup_current_process(core, plan).await?;
    }

    // Diagnostic: report whether the media init built a valid (circular) clip
    // list head. A healthy empty list has next == prev == the head's own
    // address; zero/garbage means the init did not take and the first
    // `MC_mdaClipCreate` will corrupt state.
    let head = plan.base.wrapping_add(MEDIA_CLIP_LIST_HEAD_OFFSET);
    let next: Result<u32> = read_generic(core, head);
    let prev: Result<u32> = read_generic(core, head + 4);
    match (next, prev) {
        (Ok(next), Ok(prev)) => {
            let healthy = next == head && prev == head;
            tracing::info!(
                "Media clip list head {head:#x}: next={next:#x} prev={prev:#x} ({})",
                if healthy {
                    "circular/empty - OK"
                } else {
                    "NOT initialised as a circular list"
                }
            );
        }
        _ => tracing::warn!("Media clip list head {head:#x}: could not read"),
    }

    Ok(())
}

/// The boot step after which the memory + process subsystems are up, so a
/// current process can be installed. Media/kernel init must run *after* this.
const PROCESS_SUBSYSTEM_READY_STEP: &str = "dprocess_init";

#[cfg(test)]
mod tests {
    use super::is_ctype_data;

    /// The complete import list the real firmware exposes (from the loader smoke
    /// test), so this asserts every one classifies to a binding.
    const FIRMWARE_IMPORTS: &[&str] = &[
        "__aeabi_d2f",
        "__aeabi_d2iz",
        "__aeabi_d2lz",
        "__aeabi_dadd",
        "__aeabi_dcmpeq",
        "__aeabi_dcmpge",
        "__aeabi_dcmpgt",
        "__aeabi_dcmplt",
        "__aeabi_ddiv",
        "__aeabi_dmul",
        "__aeabi_dsub",
        "__aeabi_f2d",
        "__aeabi_f2iz",
        "__aeabi_f2lz",
        "__aeabi_fadd",
        "__aeabi_fcmpgt",
        "__aeabi_fcmplt",
        "__aeabi_fdiv",
        "__aeabi_fmul",
        "__aeabi_fsub",
        "__aeabi_i2d",
        "__aeabi_i2f",
        "__aeabi_idiv",
        "__aeabi_idivmod",
        "__aeabi_l2d",
        "__aeabi_l2f",
        "__aeabi_ldivmod",
        "__aeabi_uidiv",
        "__aeabi_uidivmod",
        "__aeabi_uldivmod",
        "__android_log_print",
        "__errno",
        "_ctype_",
        "_tolower_tab_",
        "_toupper_tab_",
        "access",
        "acos",
        "asctime",
        "asin",
        "atan",
        "atan2",
        "atoi",
        "atol",
        "atoll",
        "ceil",
        "chmod",
        "close",
        "closedir",
        "cos",
        "cosh",
        "ctime",
        "dlclose",
        "dlerror",
        "dlopen",
        "dlsym",
        "exit",
        "exp",
        "floor",
        "fmod",
        "frexp",
        "ftime",
        "la_cal",
        "la_mal",
        "lafr",
        "ldexp",
        "log",
        "log10",
        "longjmp",
        "lrand48",
        "lseek",
        "memchr",
        "memcmp",
        "memcpy",
        "memmove",
        "memset",
        "mkdir",
        "modf",
        "mprotect",
        "open",
        "opendir",
        "perror",
        "pow",
        "printf",
        "pthread_mutex_destroy",
        "pthread_mutex_init",
        "pthread_mutex_trylock",
        "pthread_mutex_unlock",
        "pthread_self",
        "puts",
        "read",
        "readdir",
        "rename",
        "rmdir",
        "sem_destroy",
        "sem_init",
        "sem_post",
        "sem_wait",
        "setjmp",
        "sin",
        "sinh",
        "sleep",
        "snprintf",
        "sprintf",
        "sqrt",
        "srand48",
        "stat",
        "statfs",
        "strcat",
        "strchr",
        "strcmp",
        "strcpy",
        "strcspn",
        "strlen",
        "strncat",
        "strncmp",
        "strncpy",
        "strpbrk",
        "strrchr",
        "strspn",
        "strstr",
        "strtod",
        "strtok",
        "strtol",
        "strtoul",
        "tan",
        "tanh",
        "tolower",
        "unlink",
        "usleep",
        "vsnprintf",
        "vsprintf",
        "write",
    ];

    #[test]
    fn imports_route_to_data_tables_or_the_firmware_libc_category() {
        // Only the three ctype tables are data; every other import is a function
        // bound through the firmware-libc category (which always binds).
        let data_count = FIRMWARE_IMPORTS.iter().filter(|name| is_ctype_data(name)).count();
        assert_eq!(data_count, 3, "only _ctype_/_tolower_tab_/_toupper_tab_ are data");

        assert!(is_ctype_data("_ctype_"));
        assert!(!is_ctype_data("cos"), "cos is a function, routed to firmware-libc");
        assert!(!is_ctype_data("memset"), "memset is a function, routed to firmware-libc");
    }
}
