#![no_std]
extern crate alloc;

mod relocation;

mod emulator;
mod runtime;

pub use emulator::LgtEmulator;

// The firmware loader (P1 of docs/firmware-emulation.md). Exposed at the crate
// root so it is reachable as public API while it is still dormant - it is not
// yet wired into startup and changes no existing behaviour.
pub use runtime::firmware::{FirmwareImage, ImportResolver, UnresolvedImport, load_firmware};
