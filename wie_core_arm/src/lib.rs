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

pub use self::{
    allocator::Allocator,
    binary_patches::install_binary_patches,
    context::ArmCoreContext,
    core::{ArmCore, RUN_FUNCTION_LR, RunFunctionResult},
    function::{EmulatedFunction, EmulatedFunctionParam, RegisteredFunction, RegisteredFunctionHolder, ResultWriter, SvcId},
};
