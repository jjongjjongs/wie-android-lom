#![no_std]
extern crate alloc;

mod allocator;
mod binary_patches;
mod context;
mod core;
mod engine;
mod function;
pub mod stdlib;
mod thread;
mod thread_wrapper;

#[cfg(not(target_arch = "wasm32"))]
mod gdb;

pub type ThreadId = usize;

/// Total guest instructions executed by the engine, for a coarse MIPS meter.
/// Incremented in batches (once per `ArmEngine::run` call) to keep the hot loop
/// free of atomics; readers sample it against wall-clock time.
pub static EXECUTED_INSTRUCTIONS: ::core::sync::atomic::AtomicU64 = ::core::sync::atomic::AtomicU64::new(0);

/// Statistical PC histogram bucketed by 64 KiB region (`pc >> 16`), sampled
/// every few dozen instructions. Reveals whether the hot code is the firmware
/// (`0x6000_0000+`) or the game clet, and which region — pointing at what to
/// re-implement natively. 65536 buckets cover the whole 4 GiB space (256 KiB).
pub static PC_SAMPLES: [::core::sync::atomic::AtomicU32; 65536] = [const { ::core::sync::atomic::AtomicU32::new(0) }; 65536];

/// Total guest SVCs handled and total `ArmEngine::run` calls, for the perf
/// meter. A high SVC rate means the frame time is spent in the syscall
/// round-trip (return, handler dispatch, context restore) rather than in raw
/// instruction execution — which a faster CPU JIT cannot help.
pub static SVC_COUNT: ::core::sync::atomic::AtomicU64 = ::core::sync::atomic::AtomicU64::new(0);
pub static RUN_CALLS: ::core::sync::atomic::AtomicU64 = ::core::sync::atomic::AtomicU64::new(0);

/// Total interpreter fallbacks from the JIT, for the perf meter — a non-zero
/// rate confirms the JIT engine is active, and the magnitude shows how much
/// still misses compiled code.
pub static JIT_FALLBACKS: ::core::sync::atomic::AtomicU64 = ::core::sync::atomic::AtomicU64::new(0);

pub use self::{
    allocator::Allocator,
    binary_patches::install_binary_patches,
    context::ArmCoreContext,
    core::{ArmCore, RUN_FUNCTION_LR, RunFunctionResult},
    function::{EmulatedFunction, EmulatedFunctionParam, RegisteredFunction, RegisteredFunctionHolder, ResultWriter, SvcId},
};
