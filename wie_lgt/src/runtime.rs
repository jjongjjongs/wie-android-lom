pub mod firmware;
pub mod firmware_libc;
pub mod firmware_link;
pub mod init;
mod java;
mod savepoint;
mod stdlib;
mod svc_ids;
mod wipi_c;

const SVC_CATEGORY_INIT: u32 = 1;
const SVC_CATEGORY_WIPIC: u32 = 3;
const SVC_CATEGORY_STDLIB: u32 = 5;
/// SVC category for the firmware's own C-runtime imports (libc/libm/allocator),
/// distinct from the game's `stdlib` category so firmware calls are traceable.
const SVC_CATEGORY_FIRMWARE_LIBC: u32 = 6;
/// SVC category for the media-route trace shim: each routed `MC_mda*` import can
/// be sent through a stub that logs its args, invokes the firmware export, and
/// logs the return, making the game's silent media call sequence visible.
const SVC_CATEGORY_FIRMWARE_MDA: u32 = 7;
