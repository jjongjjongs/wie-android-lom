//! Wiring that loads the user-supplied firmware BIOS and reports what it took.
//!
//! This is the P1 entry point: if the user has supplied the firmware image, map
//! it beside the game, bind the imports our existing Rust HLE already covers,
//! and log a summary plus the exact list of imports still unbound. Nothing here
//! redirects the game's platform calls into firmware code yet - that is P3 - so
//! with the BIOS present this only maps memory and prints a diagnostic, and
//! with it absent it does nothing at all.
//!
//! The firmware is a BIOS: proprietary, never committed, supplied by the user
//! at runtime under the reference's own filename. Dropping it into the game
//! archive (or the app's per-title storage) turns this path on; its absence is
//! the default and leaves the emulator entirely on the Rust platform.

use alloc::{vec, vec::Vec};

use wie_backend::System;
use wie_core_arm::ArmCore;
use wie_util::Result;

use super::SVC_CATEGORY_STDLIB;
use super::firmware::{FirmwareImage, ImportResolver, load_firmware};
use super::svc_ids::StdlibSvcId;

/// The reference's own name for the firmware image. The user supplies this file.
pub const BIOS_FILENAME: &str = "libarm32_lgt_system.so";

/// Where the firmware is mapped. The image is linked at 0, so it needs a base
/// clear of the game module and the allocator heap. This is a first-pass choice
/// to be confirmed on device once the real address map is known (a collision
/// would surface in the load log as a mapping error).
const FIRMWARE_BASE: u32 = 0x6000_0000;

/// Binds a firmware import name to one of the existing Rust HLE handlers.
///
/// The firmware's C-runtime imports overlap the game's `stdlib` category, so
/// the handlers that already serve the game (`memcpy`, `strlen`, `printf`, …)
/// serve the firmware too: each maps to an SVC trampoline into the same
/// dispatch. Names with no handler yet (libm, the allocator, POSIX threads, …)
/// return `None` and are recorded as unresolved for a later phase to fill in.
struct StdlibImportResolver;

/// Maps a firmware import name to the `stdlib` HLE id that serves it, or `None`
/// when no handler exists yet. Split out from the resolver so the mapping - the
/// part that can drift as handlers are added - is testable without a fully
/// registered SVC core.
fn stdlib_import_id(name: &str) -> Option<StdlibSvcId> {
    let id = match name {
        "printf" => StdlibSvcId::Printf,
        "sprintf" => StdlibSvcId::Sprintf,
        "atoi" => StdlibSvcId::Atoi,
        "strcpy" => StdlibSvcId::Strcpy,
        "strncpy" => StdlibSvcId::Strncpy,
        "strcat" => StdlibSvcId::Strcat,
        "strcmp" => StdlibSvcId::Strcmp,
        "strncmp" => StdlibSvcId::Strncmp,
        "strstr" => StdlibSvcId::Strstr,
        "strlen" => StdlibSvcId::Strlen,
        "memcpy" => StdlibSvcId::Memcpy,
        "memmove" => StdlibSvcId::Memmove,
        "memcmp" => StdlibSvcId::Memcmp,
        "memset" => StdlibSvcId::Memset,
        _ => return None,
    };
    Some(id)
}

impl ImportResolver for StdlibImportResolver {
    fn resolve(&mut self, core: &mut ArmCore, name: &str) -> Result<Option<u32>> {
        match stdlib_import_id(name) {
            Some(id) => Ok(Some(core.make_svc_stub(SVC_CATEGORY_STDLIB, id)?)),
            None => Ok(None),
        }
    }
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

/// Loads the firmware BIOS if the user supplied it, logging what was mapped and
/// which imports still need a host handler. Returns the loaded image, or `None`
/// when no BIOS is present.
pub async fn try_load_bios(core: &mut ArmCore, system: &System) -> Result<Option<FirmwareImage>> {
    if !system.filesystem().exists(BIOS_FILENAME).await {
        tracing::debug!("No firmware BIOS ({BIOS_FILENAME}); staying on the Rust platform");
        return Ok(None);
    }

    let size = system.filesystem().size(BIOS_FILENAME).await.unwrap_or(0);
    let data = read_bios(system, size).await;
    tracing::info!("Loading firmware BIOS {BIOS_FILENAME} ({} bytes) at base {FIRMWARE_BASE:#x}", data.len());

    let mut resolver = StdlibImportResolver;
    let image = load_firmware(core, &data, FIRMWARE_BASE, &mut resolver)?;

    tracing::info!(
        "Firmware mapped: base {:#x}, entry {:#x}, {} export(s), {} unresolved import(s)",
        image.base,
        image.entry,
        image.exports.len(),
        image.unresolved_imports.len()
    );

    // The unresolved list is the P1 work item: exactly which imports still need
    // an HLE handler. Log it as one line so it is easy to lift out of logcat.
    if !image.unresolved_imports.is_empty() {
        let names: Vec<&str> = image.unresolved_imports.iter().map(|u| u.name.as_str()).collect();
        tracing::info!("Firmware imports still unbound ({}): {}", names.len(), names.join(", "));
    }

    // Sanity check a few known firmware internals against the re-derived export
    // table, so the log confirms the symbol map lines up with real code.
    for name in ["MH_sysHalInit", "dlet_start", "InitPCSAutomata", "AND_mdaInit"] {
        match image.export(name) {
            Some(addr) => tracing::info!("Firmware export {name} -> {addr:#x}"),
            None => tracing::info!("Firmware export {name} not found"),
        }
    }

    Ok(Some(image))
}

/// The import names this resolver currently binds. Kept beside the mapping so
/// the two do not drift; used only by tests today.
#[cfg(test)]
const BOUND_IMPORTS: [&str; 14] = [
    "printf", "sprintf", "atoi", "strcpy", "strncpy", "strcat", "strcmp", "strncmp", "strstr", "strlen", "memcpy", "memmove", "memcmp", "memset",
];

#[cfg(test)]
mod tests {
    use super::{BOUND_IMPORTS, stdlib_import_id};

    #[test]
    fn binds_known_c_runtime_imports_to_distinct_ids() {
        let mut ids = alloc::vec::Vec::new();
        for name in BOUND_IMPORTS {
            let id = stdlib_import_id(name);
            assert!(id.is_some(), "{name} should map to an HLE id");
            ids.push(id.unwrap() as u32);
        }

        // Each import maps to its own distinct handler id.
        ids.sort_unstable();
        let distinct = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), distinct, "each bound import needs a distinct id");
    }

    #[test]
    fn leaves_libm_allocator_and_threads_unbound() {
        for name in ["cos", "sin", "pow", "sqrt", "malloc", "la_cal", "la_mal", "lafr", "pthread_mutex_init", "sem_wait", "dlopen"] {
            assert!(stdlib_import_id(name).is_none(), "{name} has no handler yet");
        }
    }
}
