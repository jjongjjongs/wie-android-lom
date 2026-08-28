//! A machine-code JIT execution engine.
//!
//! [`FastCpuEngine`](super::FastCpuEngine) proved that decoding Thumb into an IR
//! and caching it by block is *not* enough to beat the reference interpreter —
//! the per-operation ALU/flag/memory work dominates, and interpreting that IR
//! costs the same as interpreting the original instructions. This engine removes
//! that cost by **compiling each basic block to native code**: the block's
//! [`FastOp`]s become a straight run of host instructions operating on a flat
//! guest register array ([`JitCtx`]), with guest memory reached through small
//! `extern "C"` helpers that share the interpreter's page table and fault
//! semantics. A prototype of this shape ran the hot sprite-blit loop ~19x faster
//! than the interpreter.
//!
//! Instruction *selection* is shared across host architectures via the reused
//! [`decode`] frontend; only the *encoding* is per-arch (`x64`, `aarch64`).
//! Anything the compiler declines (exotic Thumb, ARM mode, SVC, multi-register
//! transfers) ends the block, and the engine falls back to a real
//! `arm32_cpu::Cpu` step for that one instruction — so the engine is correct by
//! construction and merely faster on the hot paths. Correctness is pinned by
//! differential tests against `Arm32CpuEngine`.

use alloc::{boxed::Box, format};

use arm32_cpu::{Cpu, Mode, reg};

use wie_util::{Result, WieError};

use super::arm32_cpu::EmulatedMemory;
use super::fast::{Decoded, FastOp, decode};
use crate::engine::{ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

#[cfg(target_arch = "x86_64")]
mod x64;
#[cfg(target_arch = "x86_64")]
use x64 as backend;
#[cfg(target_arch = "aarch64")]
mod aarch64;
#[cfg(target_arch = "aarch64")]
use aarch64 as backend;

/// Longest straight-line run compiled into one block.
const MAX_BLOCK_LEN: usize = 256;

const CACHE_SLOTS: usize = 1 << 13;
const CACHE_MASK: u32 = (CACHE_SLOTS - 1) as u32;

/// Flat guest state a compiled block operates on. `#[repr(C)]` with a fixed
/// layout the code generator hard-codes offsets into (see `backend::off`).
#[repr(C)]
pub(crate) struct JitCtx {
    /// r0..r15; r15 is PC. Offset 0.
    pub regs: [u32; 16],
    /// CPSR (only NZCV and the mode/T bits matter here). Offset 64.
    pub cpsr: u32,
    /// Address of the last memory fault. Offset 68.
    pub fault_addr: u32,
    /// Non-zero if a memory access faulted. Offset 72.
    pub faulted: u32,
    /// Non-zero if a store hit a page holding cached code (self-modifying code).
    /// Offset 76.
    pub smc: u32,
    /// Guest memory (borrowed from the engine for the duration of a run).
    /// Offset 80.
    pub mem: *mut EmulatedMemory,
    /// Per-64 KiB-region "has cached code" flags, for SMC detection. Offset 88.
    pub code_pages: *const bool,
    /// Remaining instruction budget for this run. Compiled traces decrement it
    /// per basic block and yield to the dispatcher when it reaches zero; the
    /// dispatcher reads the delta back as the retired-instruction count.
    /// Offset 96.
    pub budget: i32,
}

impl JitCtx {
    fn new() -> Self {
        JitCtx {
            regs: [0; 16],
            cpsr: 0,
            fault_addr: 0,
            faulted: 0,
            smc: 0,
            mem: core::ptr::null_mut(),
            code_pages: core::ptr::null(),
            budget: 0,
        }
    }
}

/// Exit codes a compiled block returns in the host return register. The low bits
/// carry the reason; the block always leaves `regs[15]` at the next PC.
pub(crate) mod exit {
    /// Ran to the block's end / a branch; continue at `regs[15]`.
    pub const CONTINUE: u32 = 0;
    /// A memory access faulted (`faulted`/`fault_addr` set).
    pub const FAULT: u32 = 1;
    /// A store hit a code page; flush and re-compile from `regs[15]`.
    pub const SMC: u32 = 2;
}

/// A compiled block plus the metadata the dispatcher needs.
struct CompiledBlock {
    code: backend::Code,
    /// Guest PC just past the block (for the end-in-block check).
    end_pc: u32,
}

#[derive(Default)]
struct Slot {
    gen_tag: u32,
    pc: u32,
    block: Option<CompiledBlock>,
}

/// Reasons the JIT declines an instruction and falls back to the interpreter,
/// for the fallback profile. Order matches `FALLBACK_NAMES`.
const FALLBACK_NAMES: [&str; 12] = [
    "shift",        // 0: LSL/LSR/ASR immediate
    "shift-by-reg", // 1: ALU LSL/LSR/ASR/ROR by register
    "adc",          // 2
    "sbc",          // 3
    "mul",          // 4
    "ldrsb/ldrsh",  // 5: signed halfword transfers
    "bx/blx",       // 6
    "push/pop",     // 7
    "ldm/stm",      // 8
    "bl/b.w",       // 9: long / wide branches
    "svc",          // 10
    "other",        // 11: ARM mode, undefined, hints, …
];

/// Classify why the instruction at `inst` is not JIT-compilable, as an index
/// into `FALLBACK_NAMES`.
fn fallback_category(inst: u16) -> usize {
    match decode(inst, 0) {
        Some(Decoded::Straight(op)) | Some(Decoded::Terminator(op)) => match op {
            FastOp::Shift { .. } => 0,
            FastOp::AluOp { op, .. } => match op {
                0x2 | 0x3 | 0x4 | 0x7 => 1,
                0x5 => 2,
                0x6 => 3,
                0xD => 4,
                _ => 11,
            },
            FastOp::HwSgnXfer { .. } => 5,
            _ => 11,
        },
        None => {
            let i = inst as u32;
            if i & 0xff00 == 0x4700 {
                6 // BX/BLX
            } else if i & 0xf600 == 0xb400 {
                7 // PUSH/POP
            } else if i & 0xf000 == 0xc000 {
                8 // LDMIA/STMIA
            } else if i & 0xf000 == 0xf000 || i & 0xf800 == 0xe800 {
                9 // BL / BLX / B.W
            } else if i & 0xff00 == 0xdf00 {
                10 // SVC
            } else {
                11
            }
        }
    }
}

pub struct JitEngine {
    cpu: Cpu,
    mem: Box<EmulatedMemory>,
    ctx: Box<JitCtx>,
    cache: Box<[Slot; CACHE_SLOTS]>,
    generation: u32,
    code_pages: Box<[bool; 65536]>,
    /// Per-category count of interpreter fallbacks, and the running total, for
    /// the periodic fallback profile (see `note_fallback`).
    fallbacks: [u64; 12],
    fallback_total: u64,
}

// The raw pointers in `JitCtx` are refreshed at the start of every `run` and
// only used during it, so the engine is safe to move between threads (it always
// lives behind a mutex in `ArmCoreInner`).
unsafe impl Send for JitEngine {}

impl JitEngine {
    pub fn new() -> Self {
        JitEngine {
            cpu: Cpu::new(),
            mem: Box::new(EmulatedMemory::new()),
            ctx: Box::new(JitCtx::new()),
            cache: Box::new(core::array::from_fn(|_| Slot::default())),
            generation: 1,
            code_pages: Box::new([false; 65536]),
            fallbacks: [0; 12],
            fallback_total: 0,
        }
    }

    /// Record an interpreter fallback for the instruction at `pc` and, at coarse
    /// milestones, log the top reasons so on-device profiling shows which
    /// instructions to teach the compiler next.
    fn note_fallback(&mut self, pc: u32) {
        let inst = self.mem.peek_u16(pc).unwrap_or(0);
        self.fallbacks[fallback_category(inst)] += 1;
        self.fallback_total += 1;
        if self.fallback_total.is_power_of_two() && self.fallback_total >= 1 << 20 {
            let mut idx: [usize; 12] = core::array::from_fn(|i| i);
            idx.sort_unstable_by_key(|&i| core::cmp::Reverse(self.fallbacks[i]));
            let mut top = alloc::string::String::new();
            for &i in idx.iter().take(6) {
                if self.fallbacks[i] == 0 {
                    break;
                }
                use core::fmt::Write;
                let _ = write!(top, "{}={} ", FALLBACK_NAMES[i], self.fallbacks[i]);
            }
            tracing::info!("[jit] {} fallbacks; top: {}", self.fallback_total, top.trim_end());
        }
    }

    fn flush_blocks(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            for slot in self.cache.iter_mut() {
                *slot = Slot::default();
            }
            self.generation = 1;
        }
        self.code_pages.fill(false);
    }

    #[inline(always)]
    fn slot_index(pc: u32) -> usize {
        ((pc >> 1) & CACHE_MASK) as usize
    }

    /// Decode the block at `pc` into `FastOp`s. `None` if the first instruction
    /// is not fast-decodable (caller falls back).
    fn decode_block(&self, start: u32) -> Option<(alloc::vec::Vec<FastOp>, u32)> {
        let mut ops = alloc::vec::Vec::new();
        let mut pc = start;
        while ops.len() < MAX_BLOCK_LEN {
            let inst = self.mem.peek_u16(pc)?;
            match decode(inst, pc) {
                Some(Decoded::Straight(op)) => {
                    ops.push(op);
                    pc = pc.wrapping_add(2);
                }
                Some(Decoded::Terminator(op)) => {
                    ops.push(op);
                    pc = pc.wrapping_add(2);
                    // A conditional branch has a fall-through path, so keep
                    // decoding linearly (its target is linked inside the trace if
                    // in range). An unconditional branch has no fall-through.
                    if matches!(op, FastOp::Branch { .. }) {
                        break;
                    }
                }
                None => break,
            }
        }
        if ops.is_empty() {
            return None;
        }
        Some((ops, pc))
    }

    /// Ensure a compiled block for `pc` occupies its cache slot. Returns `false`
    /// if the instruction there cannot start a compiled block (caller falls
    /// back for one instruction).
    fn ensure_block(&mut self, pc: u32) -> bool {
        let idx = Self::slot_index(pc);
        if self.cache[idx].gen_tag == self.generation && self.cache[idx].pc == pc && self.cache[idx].block.is_some() {
            return true;
        }
        let Some((ops, end_pc)) = self.decode_block(pc) else {
            return false;
        };
        let Some((code, compiled_ops)) = backend::compile_block(&ops, pc) else {
            return false;
        };
        // `compiled_ops` may be shorter than `ops` if the compiler stopped early
        // at an op it declined; recompute the real end PC.
        let end_pc = if compiled_ops == ops.len() {
            end_pc
        } else {
            pc.wrapping_add(2 * compiled_ops as u32)
        };
        self.code_pages[(pc >> 16) as usize] = true;
        let cur_gen = self.generation;
        let slot = &mut self.cache[idx];
        slot.gen_tag = cur_gen;
        slot.pc = pc;
        slot.block = Some(CompiledBlock { code, end_pc });
        true
    }

    fn store_back(&mut self, mode: Mode) {
        self.cpu.reg_set(mode, reg::CPSR, self.ctx.cpsr);
        for i in 0..16 {
            self.cpu.reg_set(mode, i as u8, self.ctx.regs[i]);
        }
    }

    fn load_regs(&mut self, mode: Mode) {
        for i in 0..16 {
            self.ctx.regs[i] = self.cpu.reg_get(mode, i as u8);
        }
        self.ctx.cpsr = self.cpu.reg_get(mode, reg::CPSR);
    }

    fn read_svc_result(&mut self) -> Result<EngineRunResult> {
        let lr = self.cpu.reg_get(Mode::Supervisor, reg::LR);
        let spsr = self.cpu.reg_get(Mode::Supervisor, reg::SPSR);
        let svc_address = lr.checked_sub(2).ok_or(WieError::InvalidMemoryAccess(lr))?;
        let mut svc_bytes = [0u8; 2];
        self.mem.read_range(svc_address, 2, &mut svc_bytes)?;
        let instruction = u16::from_le_bytes(svc_bytes);
        if instruction & 0xff00 != 0xdf00 {
            return Err(WieError::FatalError(format!("Invalid Thumb SVC instruction {instruction:#06x}")));
        }
        Ok(EngineRunResult::Svc {
            category: instruction as u32 & 0xff,
            lr,
            spsr,
        })
    }
}

impl ArmEngine for JitEngine {
    fn run(&mut self, end: u32, count: u32) -> Result<EngineRunResult> {
        struct Batch(u64);
        impl Drop for Batch {
            fn drop(&mut self) {
                crate::EXECUTED_INSTRUCTIONS.fetch_add(self.0, ::core::sync::atomic::Ordering::Relaxed);
            }
        }
        let mut batch = Batch(0);
        let mut budget = count as u64;

        let mut mode = self.cpu.mode();
        self.load_regs(mode);

        // Point the context at this engine's memory / SMC flags for the run.
        self.ctx.mem = &mut *self.mem as *mut EmulatedMemory;
        self.ctx.code_pages = self.code_pages.as_ptr();

        let result = loop {
            let pc = self.ctx.regs[15];

            if pc == 0x08 && (self.ctx.cpsr & 0x1f) == 0x13 {
                self.store_back(mode);
                return self.read_svc_result();
            }
            if pc < 0x1000 {
                break Err(WieError::InvalidMemoryAccess(pc));
            }
            if pc == end {
                break Ok(EngineRunResult::End);
            }
            if budget == 0 {
                break Ok(EngineRunResult::CountExhausted);
            }

            let thumb = self.ctx.cpsr & (1 << 5) != 0;
            // `end` mid-block: fall back to single-stepping so we stop exactly at
            // it. In normal control flow `end` is a return sentinel reached only
            // at block starts, so this is the rare path.
            let can_jit = thumb && self.ensure_block(pc) && {
                let b = self.cache[Self::slot_index(pc)].block.as_ref().unwrap();
                !(end > pc && end < b.end_pc)
            };

            if can_jit {
                crate::PC_SAMPLES[(pc >> 16) as usize].fetch_add(1, ::core::sync::atomic::Ordering::Relaxed);
                self.ctx.faulted = 0;
                self.ctx.smc = 0;
                // The compiled trace decrements `budget` per basic block and
                // yields when it reaches zero, so the delta is the retired count.
                let start_budget = budget.min(i32::MAX as u64) as i32;
                self.ctx.budget = start_budget;
                let reason = {
                    let block = self.cache[Self::slot_index(pc)].block.as_ref().unwrap();
                    backend::run_block(&block.code, &mut self.ctx)
                };
                let retired = (start_budget - self.ctx.budget).max(0) as u64;
                batch.0 += retired;
                budget = budget.saturating_sub(retired);
                match reason {
                    exit::CONTINUE => {}
                    exit::FAULT => break Err(WieError::InvalidMemoryAccess(self.ctx.fault_addr)),
                    exit::SMC => self.flush_blocks(),
                    other => break Err(WieError::FatalError(format!("bad JIT exit {other}"))),
                }
            } else {
                // Fallback: one interpreter step on the real CPU.
                self.note_fallback(pc);
                self.store_back(mode);
                let mut wrapper = self.mem.as_arm32cpu_memory();
                let ok = self.cpu.step(&mut wrapper);
                let mem_err = wrapper.memory_error();
                mode = self.cpu.mode();
                self.load_regs(mode);
                if !ok {
                    let mut bytes = [0u8; 4];
                    let _ = self.mem.read_range(pc, 4, &mut bytes);
                    let opcode = u32::from_le_bytes(bytes);
                    break Err(WieError::FatalError(format!("Undefined instruction at {pc:#x}: opcode {opcode:#010x}")));
                }
                batch.0 += 1;
                budget = budget.saturating_sub(1);
                if let Some(addr) = mem_err {
                    break Err(WieError::InvalidMemoryAccess(addr));
                }
            }
        };

        self.store_back(mode);
        result
    }

    fn reg_write(&mut self, r: ArmRegister, value: u32) {
        if r == ArmRegister::PC && value % 2 == 1 {
            self.cpu.reg_set(Mode::User, r.into_armv4t(), value - 1);
            let cpsr = self.cpu.reg_get(Mode::User, reg::CPSR);
            self.cpu.reg_set(Mode::User, reg::CPSR, cpsr | (1 << 5));
            return;
        }
        self.cpu.reg_set(Mode::User, r.into_armv4t(), value);
    }

    fn reg_read(&self, r: ArmRegister) -> u32 {
        self.cpu.reg_get(Mode::User, r.into_armv4t())
    }

    fn mem_map(&mut self, address: u32, size: usize, _permission: MemoryPermission) {
        self.mem.map(address, size);
        self.flush_blocks();
    }

    fn mem_write(&mut self, address: u32, data: &[u8]) -> Result<()> {
        self.mem.write_range(address, data)?;
        self.flush_blocks();
        Ok(())
    }

    fn mem_read(&mut self, address: u32, size: usize, result: &mut [u8]) -> Result<usize> {
        self.mem.read_range(address, size, result)
    }

    fn is_mapped(&self, address: u32, size: usize) -> bool {
        self.mem.is_mapped(address, size)
    }
}

// ---------------------------------------------------------------------------
// Memory helpers called from compiled code. All take the context pointer so a
// fault or SMC hit can be recorded. They mirror `engine::fast::Ctx`'s accessors
// (alignment, fault addresses) exactly so the JIT and interpreter agree.
// ---------------------------------------------------------------------------

/// Evaluate an ARM condition code against CPSR, reusing the interpreter's exact
/// logic so compiled conditional branches match bit-for-bit.
pub(crate) extern "C" fn jit_cond_met(cond: u32, cpsr: u32) -> u32 {
    arm32_cpu::util::arm::cond_met(cond, cpsr) as u32
}

/// SAFETY: `ctx` points at a live `JitCtx` whose `mem`/`code_pages` are valid for
/// the duration of the call (guaranteed by `run`).
pub(crate) unsafe extern "C" fn jit_load8(ctx: *mut JitCtx, addr: u32) -> u32 {
    let ctx = unsafe { &mut *ctx };
    let mem = unsafe { &*ctx.mem };
    match mem.load_u8(addr) {
        Some(v) => v as u32,
        None => {
            ctx.faulted = 1;
            ctx.fault_addr = addr;
            0
        }
    }
}

pub(crate) unsafe extern "C" fn jit_load16(ctx: *mut JitCtx, addr: u32) -> u32 {
    let ctx = unsafe { &mut *ctx };
    let mem = unsafe { &*ctx.mem };
    match mem.load_u16(addr) {
        Some(v) => v as u32,
        None => {
            ctx.faulted = 1;
            ctx.fault_addr = addr;
            0
        }
    }
}

pub(crate) unsafe extern "C" fn jit_load32(ctx: *mut JitCtx, addr: u32) -> u32 {
    let ctx = unsafe { &mut *ctx };
    let mem = unsafe { &*ctx.mem };
    let a = addr & !3;
    match mem.load_u32(a) {
        Some(v) => {
            if a == addr {
                v
            } else {
                v.rotate_right((addr & 3) * 8)
            }
        }
        None => {
            ctx.faulted = 1;
            ctx.fault_addr = a;
            0
        }
    }
}

#[inline(always)]
unsafe fn note_store(ctx: &mut JitCtx, addr: u32) {
    // Flag self-modifying code: a store into a page that holds compiled blocks.
    if !ctx.code_pages.is_null() {
        let page = (addr >> 16) as usize;
        if unsafe { *ctx.code_pages.add(page) } {
            ctx.smc = 1;
        }
    }
}

pub(crate) unsafe extern "C" fn jit_store8(ctx: *mut JitCtx, addr: u32, val: u32) {
    let ctx = unsafe { &mut *ctx };
    let mem = unsafe { &mut *ctx.mem };
    if mem.store_u8(addr, val as u8).is_none() {
        ctx.faulted = 1;
        ctx.fault_addr = addr;
    } else {
        unsafe { note_store(ctx, addr) };
    }
}

pub(crate) unsafe extern "C" fn jit_store16(ctx: *mut JitCtx, addr: u32, val: u32) {
    let ctx = unsafe { &mut *ctx };
    let mem = unsafe { &mut *ctx.mem };
    if mem.store_u16(addr, val as u16).is_none() {
        ctx.faulted = 1;
        ctx.fault_addr = addr;
    } else {
        unsafe { note_store(ctx, addr) };
    }
}

pub(crate) unsafe extern "C" fn jit_store32(ctx: *mut JitCtx, addr: u32, val: u32) {
    let ctx = unsafe { &mut *ctx };
    let mem = unsafe { &mut *ctx.mem };
    let a = addr & !3;
    if mem.store_u32(a, val).is_none() {
        ctx.faulted = 1;
        ctx.fault_addr = a;
    } else {
        unsafe { note_store(ctx, a) };
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::vec;
    use alloc::vec::Vec;

    use super::super::fast::{Decoded, decode};
    use super::JitEngine;
    use crate::engine::{Arm32CpuEngine, ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

    const CODE: u32 = 0x1000;
    const DATA: u32 = 0x0010_0000;
    const DATA_SIZE: usize = 0x0010_0000;

    fn reg_of(i: usize) -> ArmRegister {
        use ArmRegister::*;
        [R0, R1, R2, R3, R4, R5, R6, R7, R8, SB, SL, FP, IP, SP, LR, PC][i]
    }

    #[derive(PartialEq, Eq, Debug)]
    enum Outcome {
        End,
        CountExhausted,
        Svc(u32),
        Fault(u32),
        Fatal,
    }

    fn outcome(r: super::Result<EngineRunResult>) -> Outcome {
        match r {
            Ok(EngineRunResult::End) => Outcome::End,
            Ok(EngineRunResult::CountExhausted) => Outcome::CountExhausted,
            Ok(EngineRunResult::Svc { category, .. }) => Outcome::Svc(category),
            Err(wie_util::WieError::InvalidMemoryAccess(a)) => Outcome::Fault(a),
            Err(_) => Outcome::Fatal,
        }
    }

    fn setup<E: ArmEngine>(mut e: E, code: &[u8], regs: &[u32; 15]) -> E {
        e.mem_map(0, 0x10000, MemoryPermission::ReadExecute);
        e.mem_map(DATA, DATA_SIZE, MemoryPermission::ReadWrite);
        e.mem_write(CODE, code).unwrap();
        for (i, &v) in regs.iter().enumerate() {
            e.reg_write(reg_of(i), v);
        }
        e.reg_write(ArmRegister::PC, CODE | 1);
        e
    }

    fn snapshot<E: ArmEngine>(e: &E) -> [u32; 17] {
        let mut s = [0u32; 17];
        for (i, slot) in s.iter_mut().enumerate().take(16) {
            *slot = e.reg_read(reg_of(i));
        }
        s[16] = e.reg_read(ArmRegister::Cpsr);
        s
    }

    fn drive<E: ArmEngine>(mut e: E, end: u32, count: u32) -> (Outcome, [u32; 17], Vec<u8>) {
        let out = loop {
            match e.run(end, count) {
                Ok(EngineRunResult::CountExhausted) => continue,
                other => break outcome(other),
            }
        };
        let regs = snapshot(&e);
        let mut mem = vec![0u8; DATA_SIZE];
        e.mem_read(DATA, DATA_SIZE, &mut mem).unwrap();
        (out, regs, mem)
    }

    fn assert_same(code: &[u8], regs: &[u32; 15], end: u32) {
        let (jo, jr, jm) = drive(setup(JitEngine::new(), code, regs), end, 37);
        let (so, sr, sm) = drive(setup(Arm32CpuEngine::new(), code, regs), end, 37);
        assert_eq!(so, jo, "outcome differs (interp {so:?} vs jit {jo:?})");
        if sr != jr {
            for i in 0..17 {
                if sr[i] != jr[i] {
                    panic!("reg[{i}] differs: interp {:#010x} vs jit {:#010x}", sr[i], jr[i]);
                }
            }
        }
        assert!(sm == jm, "data memory differs between engines");
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn u16(&mut self) -> u16 {
            self.next() as u16
        }
        fn u32(&mut self) -> u32 {
            self.next() as u32
        }
    }

    #[test]
    #[ignore = "debug helper"]
    fn jit_debug_find_bug() {
        const OPS: usize = 40;
        for seed in 1..=2000u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut code = Vec::with_capacity(OPS * 2);
            while code.len() < OPS * 2 {
                let w = rng.u16();
                let pc = CODE + code.len() as u32;
                if let Some(Decoded::Straight(_)) = decode(w, pc) {
                    code.extend_from_slice(&w.to_le_bytes());
                }
            }
            let mut regs = [0u32; 15];
            for (i, r) in regs.iter_mut().enumerate().take(13) {
                *r = match rng.u32() & 3 {
                    0 => DATA + (rng.u32() & 0x3ff) * 4 + (i as u32) * 4,
                    1 => rng.u32() & 0xff,
                    2 => DATA + 0xfff8 + (rng.u32() & 0xf),
                    _ => rng.u32(),
                };
            }
            regs[13] = DATA + 0x8000;
            for k in 1..=OPS {
                let end = CODE + (k as u32) * 2;
                let (jo, jr, _) = drive(setup(JitEngine::new(), &code, &regs), end, 1_000_000);
                let (so, sr, _) = drive(setup(Arm32CpuEngine::new(), &code, &regs), end, 1_000_000);
                if jo != so || jr != sr {
                    let w = u16::from_le_bytes([code[(k - 1) * 2], code[(k - 1) * 2 + 1]]);
                    std::eprintln!("seed {seed}: diverges at op #{} word {w:#06x}; outcome interp {so:?} jit {jo:?}", k - 1);
                    for i in 0..17 {
                        if jr[i] != sr[i] {
                            std::eprintln!("  reg[{i}] interp {:#010x} jit {:#010x}", sr[i], jr[i]);
                        }
                    }
                    return;
                }
            }
        }
        std::eprintln!("no divergence found");
    }

    #[test]
    fn jit_fuzz_straight_line() {
        const OPS: usize = 40;
        for seed in 1..=2000u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut code = Vec::with_capacity(OPS * 2);
            while code.len() < OPS * 2 {
                let w = rng.u16();
                let pc = CODE + code.len() as u32;
                if let Some(Decoded::Straight(_)) = decode(w, pc) {
                    code.extend_from_slice(&w.to_le_bytes());
                }
            }
            let mut regs = [0u32; 15];
            for (i, r) in regs.iter_mut().enumerate().take(13) {
                *r = match rng.u32() & 3 {
                    0 => DATA + (rng.u32() & 0x3ff) * 4 + (i as u32) * 4,
                    1 => rng.u32() & 0xff,
                    2 => DATA + 0xfff8 + (rng.u32() & 0xf),
                    _ => rng.u32(),
                };
            }
            regs[13] = DATA + 0x8000;
            let end = CODE + (OPS as u32) * 2;
            assert_same(&code, &regs, end);
        }
    }
}
