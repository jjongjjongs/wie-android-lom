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

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};

use spin::Mutex;

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore};
use wie_util::{ByteWrite, Result};

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
pub fn build_mda_routes(image: &FirmwareImage) -> BTreeMap<u32, u32> {
    let mut routes = BTreeMap::new();
    for (index, name) in MDA_ROUTES {
        match image.export(name) {
            Some(addr) => {
                tracing::info!("Routing WIPI-C media import {index:#x} -> firmware {name} {addr:#x}");
                routes.insert(*index, addr);
            }
            None => tracing::warn!("Firmware has no {name}; leaving WIPI-C import {index:#x} on the Rust stub"),
        }
    }
    routes
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

    for name in ["MH_sysHalInit", "dlet_start", "InitPCSAutomata", "AND_mdaInit"] {
        match image.export(name) {
            Some(addr) => tracing::info!("Firmware export {name} -> {addr:#x}"),
            None => tracing::info!("Firmware export {name} not found"),
        }
    }

    Ok(Some(image))
}

/// The firmware runtime boot sequence, in call order. `dprocess_init` and
/// `dthread_init` stand up the DTHREAD runtime (memory pools, config) so
/// `dprocess_get_current` starts returning a live process; `MH_sysHalInit` is
/// the HAL init that needs that context. Each is called with no arguments,
/// matching their disassembly.
const BOOT_SEQUENCE: &[&str] = &[
    "dmempage_init",
    "dmemory_init",
    "dprocess_init",
    "dthread_init",
    "MH_sysHalInit",
    "WPKnl_Init",
    "AND_mdaInit",
];

/// The firmware entry points needed to drive init, lifted out of the image so
/// they can move into an isolated task.
pub struct FirmwareInitPlan {
    pub init: Option<u32>,
    pub init_array: Vec<u32>,
    /// `(name, guest address)` for each boot step present in the image.
    pub boot_steps: Vec<(String, u32)>,
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
        }
    }
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
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_ctype_data;

    /// The complete import list the real firmware exposes (from the loader smoke
    /// test), so this asserts every one classifies to a binding.
    const FIRMWARE_IMPORTS: &[&str] = &[
        "__aeabi_d2f", "__aeabi_d2iz", "__aeabi_d2lz", "__aeabi_dadd", "__aeabi_dcmpeq", "__aeabi_dcmpge", "__aeabi_dcmpgt", "__aeabi_dcmplt",
        "__aeabi_ddiv", "__aeabi_dmul", "__aeabi_dsub", "__aeabi_f2d", "__aeabi_f2iz", "__aeabi_f2lz", "__aeabi_fadd", "__aeabi_fcmpgt",
        "__aeabi_fcmplt", "__aeabi_fdiv", "__aeabi_fmul", "__aeabi_fsub", "__aeabi_i2d", "__aeabi_i2f", "__aeabi_idiv", "__aeabi_idivmod",
        "__aeabi_l2d", "__aeabi_l2f", "__aeabi_ldivmod", "__aeabi_uidiv", "__aeabi_uidivmod", "__aeabi_uldivmod", "__android_log_print",
        "__errno", "_ctype_", "_tolower_tab_", "_toupper_tab_", "access", "acos", "asctime", "asin", "atan", "atan2", "atoi", "atol", "atoll",
        "ceil", "chmod", "close", "closedir", "cos", "cosh", "ctime", "dlclose", "dlerror", "dlopen", "dlsym", "exit", "exp", "floor", "fmod",
        "frexp", "ftime", "la_cal", "la_mal", "lafr", "ldexp", "log", "log10", "longjmp", "lrand48", "lseek", "memchr", "memcmp", "memcpy",
        "memmove", "memset", "mkdir", "modf", "mprotect", "open", "opendir", "perror", "pow", "printf", "pthread_mutex_destroy",
        "pthread_mutex_init", "pthread_mutex_trylock", "pthread_mutex_unlock", "pthread_self", "puts", "read", "readdir", "rename", "rmdir",
        "sem_destroy", "sem_init", "sem_post", "sem_wait", "setjmp", "sin", "sinh", "sleep", "snprintf", "sprintf", "sqrt", "srand48", "stat",
        "statfs", "strcat", "strchr", "strcmp", "strcpy", "strcspn", "strlen", "strncat", "strncmp", "strncpy", "strpbrk", "strrchr", "strspn",
        "strstr", "strtod", "strtok", "strtol", "strtoul", "tan", "tanh", "tolower", "unlink", "usleep", "vsnprintf", "vsprintf", "write",
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
