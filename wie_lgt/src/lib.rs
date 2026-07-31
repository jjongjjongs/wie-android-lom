#![no_std]
extern crate alloc;

mod relocation;

mod emulator;
mod runtime;

pub use emulator::LgtEmulator;
