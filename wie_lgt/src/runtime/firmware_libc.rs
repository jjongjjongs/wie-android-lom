//! HLE for the firmware's own C-runtime imports (libc / libm / allocator).
//!
//! P2 starts by binding every firmware import to a single traceable stub so the
//! firmware's init can actually run: each import gets an SVC trampoline whose id
//! indexes a shared name table, and the handler logs the call and returns zero.
//! That turns "does the firmware even execute under our interpreter, and which
//! imports does its init touch?" into something visible in the log, before any
//! real handler is written.
//!
//! Real implementations replace the stub one import at a time, keyed by name, as
//! the init trace shows what is actually exercised.

use alloc::{format, string::String, sync::Arc, vec, vec::Vec};

use spin::Mutex;

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, ResultWriter, SvcId, stdlib};
use wie_util::{ByteWrite, Result};

use super::SVC_CATEGORY_FIRMWARE_LIBC;

/// Import names in trampoline-id order: the SVC id minted for an import is its
/// index here, so the handler can name the call it is servicing.
pub type FirmwareImportNames = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
struct FirmwareLibcContext {
    names: FirmwareImportNames,
    #[allow(dead_code)]
    system: System,
}

/// Registers the firmware C-runtime SVC handler. `names` is shared with the
/// resolver, which appends an entry for each import it binds.
pub fn register_firmware_libc_handler(core: &mut ArmCore, system: &System, names: FirmwareImportNames) -> Result<()> {
    async fn handle_firmware_libc_svc(core: &mut ArmCore, context: &mut FirmwareLibcContext, id: SvcId) -> Result<()> {
        let (_, lr) = core.read_pc_lr()?;

        let index = id.0 as usize;
        let name = context
            .names
            .lock()
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("firmware_import#{index}"));

        let a0 = core.read_param(0)?;
        let a1 = core.read_param(1)?;
        let a2 = core.read_param(2)?;
        let a3 = core.read_param(3)?;

        tracing::debug!("firmware libc dispatch {name}(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");

        // The allocator and the memory/string primitives must be real from the
        // first run, or init corrupts memory / dereferences null. Everything
        // else is a traceable stub until the init log shows it is needed.
        match name.as_str() {
            // Allocator (LGT libla): la_mal/la_cal/lafr = malloc/calloc/free.
            "la_mal" => {
                let ptr = Allocator::alloc(core, a0.max(1))?;
                return ptr.write(core, lr);
            }
            "la_cal" => {
                let total = a0.saturating_mul(a1).max(1);
                let ptr = Allocator::alloc(core, total)?;
                core.write_bytes(ptr, &vec![0u8; total as usize])?;
                return ptr.write(core, lr);
            }
            // Our allocator's free needs the size, which the C free ABI does not
            // carry, so leak for now (a bring-up cost, not a shipped leak).
            "lafr" => return 0u32.write(core, lr),

            // Memory primitives. The C ABI returns the destination pointer.
            "memcpy" => {
                stdlib::memcpy(core, &mut (), a0, a1, a2).await?;
                return a0.write(core, lr);
            }
            "memmove" => {
                stdlib::memmove(core, &mut (), a0, a1, a2).await?;
                return a0.write(core, lr);
            }
            "memset" => {
                stdlib::memset(core, &mut (), a0, a1, a2).await?;
                return a0.write(core, lr);
            }
            "memcmp" => {
                let result = stdlib::memcmp(core, &mut (), a0, a1, a2).await?;
                return result.write(core, lr);
            }
            "strcpy" => {
                stdlib::strcpy(core, &mut (), a0, a1).await?;
                return a0.write(core, lr);
            }
            "strlen" => {
                let len = stdlib::strlen(core, &mut (), a0).await?;
                return len.write(core, lr);
            }
            // Threading/sync primitives. We are cooperatively single-threaded,
            // so these never block, but callers store and later dereference the
            // handles, so they must be non-null. sem_init(sem=a0, ...) writes a
            // sentinel into the caller's semaphore slot; pthread_self returns a
            // non-zero thread id.
            "sem_init" => {
                if a0 != 0 {
                    core.write_bytes(a0, &1u32.to_le_bytes())?;
                }
                return 0u32.write(core, lr);
            }
            "pthread_self" => {
                return 1u32.write(core, lr);
            }
            _ => {}
        }

        tracing::warn!("firmware libc stub {name}(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) -> 0");

        0u32.write(core, lr)
    }

    core.register_svc_handler(
        SVC_CATEGORY_FIRMWARE_LIBC,
        handle_firmware_libc_svc,
        &FirmwareLibcContext {
            names,
            system: system.clone(),
        },
    )
}
