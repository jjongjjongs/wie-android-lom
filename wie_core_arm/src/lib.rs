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

pub use self::{
    allocator::Allocator,
    binary_patches::install_binary_patches,
    context::ArmCoreContext,
    core::{ArmCore, RUN_FUNCTION_LR, RunFunctionResult},
    function::{EmulatedFunction, EmulatedFunctionParam, RegisteredFunction, RegisteredFunctionHolder, ResultWriter, SvcId},
};
