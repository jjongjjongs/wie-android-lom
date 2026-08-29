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

use arm32_cpu::{Cpu, Mode, reg, util::bit::BitUtilExt};

use wie_util::{Result, WieError};

use super::arm32_cpu::EmulatedMemory;
use super::fast::{Decoded, FastOp, decode, ends_trace};
use crate::engine::{ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

mod arm_frontend;
use arm_frontend::{ArmOp, arm_ends_trace, decode_arm};

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

/// Granularity of self-modifying-code tracking, as `log2(bytes)`. A store into a
/// region that holds compiled code invalidates every block, so this must be fine
/// enough that a title's data/stack writes do not alias its code: at the guest's
/// own 4 KiB page size, code and data (which the loader maps in separate pages)
/// no longer collide. A coarse 64 KiB granularity made a hot render loop whose
/// stack shared the code's 64 KiB region flush and recompile the whole cache on
/// every store, collapsing throughput to interpreter levels.
const CODE_PAGE_BITS: u32 = 12;
/// One flag per `2^CODE_PAGE_BITS` bytes of the 32-bit address space.
const CODE_PAGE_COUNT: usize = 1 << (32 - CODE_PAGE_BITS);

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
    /// Per-`2^CODE_PAGE_BITS`-region "has cached code" flags, for SMC detection.
    /// Offset 88.
    pub code_pages: *const bool,
    /// Remaining instruction budget for this run. Compiled traces decrement it
    /// per basic block and yield to the dispatcher when it reaches zero; the
    /// dispatcher reads the delta back as the retired-instruction count.
    /// Offset 96.
    pub budget: i32,
    /// Scratch word for compiled code needing a value to survive helper calls
    /// (the base address of an ldm/stm, whose base register may be overwritten
    /// mid-transfer). Offset 100.
    pub scratch: u32,
    /// Base of the guest page table: an array of one `*const u8` per 64 KiB of
    /// address space (null = unmapped), indexed by `addr >> 16`. Compiled code
    /// reads it to service the common in-bounds, aligned load/store inline
    /// instead of calling a helper. Offset 104.
    pub pages: *const *const u8,
    /// Addresses of the stores that hit a code page in the current block (see
    /// `smc`), so the dispatcher can invalidate only the block(s) that actually
    /// cover them. A multi-register store (`push`/`stm`) writes several distinct
    /// words before its single SMC check, so more than one is buffered. `smc_n`
    /// is how many are valid; `smc_n > SMC_ADDR_MAX` means the buffer overflowed
    /// and the dispatcher flushes everything. Only the Rust helpers touch these,
    /// so the code generator needs no offsets for them.
    pub smc_addrs: [u32; SMC_ADDR_MAX],
    pub smc_n: u32,
}

/// Capacity of `JitCtx::smc_addrs`: enough for the widest multi-register store
/// (`push`/`pop`/`ldm`/`stm`, at most 16 registers).
pub(crate) const SMC_ADDR_MAX: usize = 16;

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
            scratch: 0,
            pages: core::ptr::null(),
            smc_addrs: [0; SMC_ADDR_MAX],
            smc_n: 0,
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
const FALLBACK_NAMES: [&str; 13] = [
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
    "other",        // 11: undefined, hints, …
    "arm-mode",     // 12: guest running in ARM state (the JIT is Thumb-only)
];

/// `FALLBACK_NAMES` index for ARM-state execution.
const ARM_MODE: usize = 12;
const NUM_FALLBACK: usize = FALLBACK_NAMES.len();

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
    /// Compiled ARM (A32) blocks, kept apart from the Thumb `cache` so a PC that
    /// is only ever one ISA never aliases the other.
    arm_cache: Box<[Slot; CACHE_SLOTS]>,
    generation: u32,
    code_pages: Box<[bool]>,
    /// Per-category count of interpreter fallbacks, and the running total, for
    /// the periodic fallback profile (see `note_fallback`).
    fallbacks: [u64; NUM_FALLBACK],
    fallback_total: u64,
    /// Count of full block-cache flushes (ambiguous multi-address SMC), and of
    /// precise single-block invalidations, for the periodic profile. A store that
    /// hits a code page but lands on data (the common false alarm) bumps neither.
    smc_flushes: u64,
    smc_invalidations: u64,
    /// Histogram of the un-compilable Thumb opcodes that land in the catch-all
    /// `other` category, keyed by the instruction's high byte (`inst >> 8`),
    /// which selects the Thumb opcode group. When `other` dominates the profile
    /// this pinpoints the exact instruction the compiler should learn next,
    /// without another guess-and-check round trip on device.
    other_hist: [u64; 256],
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
            arm_cache: Box::new(core::array::from_fn(|_| Slot::default())),
            generation: 1,
            // Heap-built (a 1 MiB array must not go through the stack).
            code_pages: alloc::vec![false; CODE_PAGE_COUNT].into_boxed_slice(),
            fallbacks: [0; NUM_FALLBACK],
            fallback_total: 0,
            other_hist: [0; 256],
            smc_flushes: 0,
            smc_invalidations: 0,
        }
    }

    /// Record one interpreter fallback in `category` and, at coarse milestones,
    /// log the top reasons so on-device profiling shows which instructions to
    /// teach the compiler next.
    fn record_fallback(&mut self, category: usize) {
        self.fallbacks[category] += 1;
        self.fallback_total += 1;
        crate::JIT_FALLBACKS.fetch_add(1, ::core::sync::atomic::Ordering::Relaxed);
        if self.fallback_total.is_power_of_two() && self.fallback_total >= 1 << 16 {
            let mut idx: [usize; NUM_FALLBACK] = core::array::from_fn(|i| i);
            idx.sort_unstable_by_key(|&i| core::cmp::Reverse(self.fallbacks[i]));
            let mut top = alloc::string::String::new();
            for &i in idx.iter().take(6) {
                if self.fallbacks[i] == 0 {
                    break;
                }
                use core::fmt::Write;
                let _ = write!(top, "{}={} ", FALLBACK_NAMES[i], self.fallbacks[i]);
            }
            // When the catch-all `other` bucket is significant, name the Thumb
            // opcode groups behind it (by high byte) so the next log says exactly
            // which instruction to teach the compiler.
            let mut other_top = alloc::string::String::new();
            if self.fallbacks[11] > 0 {
                let mut hi: [usize; 256] = core::array::from_fn(|i| i);
                hi.sort_unstable_by_key(|&i| core::cmp::Reverse(self.other_hist[i]));
                use core::fmt::Write;
                for &h in hi.iter().take(4) {
                    if self.other_hist[h] == 0 {
                        break;
                    }
                    let _ = write!(other_top, " {:#04x}xx={}", h, self.other_hist[h]);
                }
            }
            if other_top.is_empty() {
                tracing::info!(
                    "[jit] {} fallbacks (smc-flushes={} inval={}); top: {}",
                    self.fallback_total,
                    self.smc_flushes,
                    self.smc_invalidations,
                    top.trim_end()
                );
            } else {
                tracing::info!(
                    "[jit] {} fallbacks (smc-flushes={} inval={}); top: {}; other:{}",
                    self.fallback_total,
                    self.smc_flushes,
                    self.smc_invalidations,
                    top.trim_end(),
                    other_top
                );
            }
        }
    }

    /// Classify and record a Thumb-mode fallback for the instruction at `pc`.
    fn note_fallback(&mut self, pc: u32) {
        let inst = self.mem.peek_u16(pc).unwrap_or(0);
        let category = fallback_category(inst);
        if category == 11 {
            self.other_hist[(inst >> 8) as usize] += 1;
        }
        self.record_fallback(category);
    }

    fn flush_blocks(&mut self) {
        self.smc_flushes += 1;
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            for slot in self.cache.iter_mut() {
                *slot = Slot::default();
            }
            for slot in self.arm_cache.iter_mut() {
                *slot = Slot::default();
            }
            self.generation = 1;
        }
        self.code_pages.fill(false);
    }

    /// Handle a store that flagged a code page (`exit::SMC`). If two distinct
    /// addresses were flagged in the run the culprit is ambiguous, so flush
    /// everything; otherwise invalidate only the block(s) actually covering the
    /// written address — a store to data that merely shares a page with code
    /// (the common case) then invalidates nothing and the hot blocks survive.
    fn handle_smc(&mut self) {
        let n = self.ctx.smc_n as usize;
        if n > SMC_ADDR_MAX {
            self.flush_blocks();
            return;
        }
        // Precisely invalidate the block(s) covering each flagged store. A store
        // to data that merely shares a page with code matches nothing and leaves
        // the hot blocks intact; this is the common case for a `push`/`stm` to a
        // stack that happens to sit in the code's page.
        let addrs = self.ctx.smc_addrs;
        let mut any = false;
        for &addr in &addrs[..n] {
            any |= self.invalidate_at(addr);
        }
        if any {
            self.smc_invalidations += 1;
        }
    }

    /// Invalidate every cached block whose compiled instruction range contains
    /// `addr`. Returns whether any block was dropped. Only block starts within one
    /// maximum block span before `addr` can cover it, so the search is bounded.
    fn invalidate_at(&mut self, addr: u32) -> bool {
        let cur_gen = self.generation;
        let mut hit = false;

        // A Thumb block is at most `MAX_BLOCK_LEN` ops, and an op is up to 4
        // bytes (the 32-bit `bl`/`blx`), so its byte span is bounded by
        // `MAX_BLOCK_LEN * 4`; any block covering `addr` starts within that span.
        let mut pc = (addr.saturating_sub(MAX_BLOCK_LEN as u32 * 4)) & !1;
        while pc <= addr {
            let idx = Self::slot_index(pc);
            let slot = &self.cache[idx];
            if slot.gen_tag == cur_gen && slot.pc == pc && slot.block.as_ref().is_some_and(|b| addr < b.end_pc) {
                self.cache[idx] = Slot::default();
                hit = true;
            }
            pc = pc.wrapping_add(2);
        }

        // ARM blocks span at most `MAX_BLOCK_LEN` 4-byte ops.
        let mut pc = (addr.saturating_sub(MAX_BLOCK_LEN as u32 * 4)) & !3;
        while pc <= addr {
            let idx = Self::slot_index(pc);
            let slot = &self.arm_cache[idx];
            if slot.gen_tag == cur_gen && slot.pc == pc && slot.block.as_ref().is_some_and(|b| addr < b.end_pc) {
                self.arm_cache[idx] = Slot::default();
                hit = true;
            }
            pc = pc.wrapping_add(4);
        }

        hit
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
                None => {
                    // The shared frontend declined it; try the JIT-only
                    // control-flow ops (push/pop, bx/blx, bl) before giving up.
                    match self.decode_cf(inst, pc) {
                        Some((op, len)) => {
                            ops.push(op);
                            pc = pc.wrapping_add(len);
                            if ends_trace(&op) {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
        if ops.is_empty() {
            return None;
        }
        Some((ops, pc))
    }

    /// Decode the control-flow Thumb ops the shared frontend declines, into their
    /// JIT-only `FastOp` variants, returning the op and its byte length (2, or 4
    /// for the 32-bit long branch). `None` if still not compilable, so the caller
    /// ends the trace and single-steps it in the interpreter.
    fn decode_cf(&self, inst: u16, pc: u32) -> Option<(FastOp, u32)> {
        let i = inst as u32;
        // BX/BLX register (HiRegBx op 3): H1 (bit 7) is the link bit, the source
        // register is (H2:Rs) = bit6<<3 | bits[5:3].
        if i & 0xff00 == 0x4700 {
            let link = i.get_bit(7) == 1;
            let rm = ((i.get_bit(6) * 8) + i.extract(3, 3)) as u8;
            return Some((FastOp::BranchExchange { link, rm }, 2));
        }
        // MOV PC, Rm (HiRegBx op 2 with destination PC): a computed branch that
        // stays in Thumb (target `rm & !1`, no interworking). Destination is PC
        // (H1=1, Rd=7); source rm = (H2:Rs). rm==PC is degenerate and left to the
        // interpreter. The shared frontend declines every crd==PC hi-reg form.
        if i & 0xff00 == 0x4600 {
            let rm = ((i.get_bit(6) * 8) + i.extract(3, 3)) as u8;
            let crd = ((i.get_bit(7) * 8) + i.extract(0, 3)) as u8;
            if crd == 15 && rm != 15 {
                return Some((FastOp::MovPc { rm }, 2));
            }
            return None;
        }
        // PUSH/POP.
        if i & 0xf600 == 0xb400 {
            return Some((
                FastOp::PushPop {
                    load: i.get_bit(11) == 1,
                    extra: i.get_bit(8) == 1,
                    rlist: i.extract(0, 8) as u8,
                },
                2,
            ));
        }
        // LDMIA/STMIA (BlockXfer).
        if i & 0xf000 == 0xc000 {
            return Some((
                FastOp::BlockXfer {
                    load: i.get_bit(11) == 1,
                    rb: i.extract(8, 3) as u8,
                    rlist: i.extract(0, 8) as u8,
                },
                2,
            ));
        }
        // Long branch (BL / BLX immediate). A 32-bit instruction: this is only
        // the prefix halfword; the suffix selects the form. The Thumb-2 wide-
        // branch (B.W) forms are left to the interpreter.
        if i & 0xf800 == 0xf000 {
            let inst2 = self.mem.peek_u16(pc.wrapping_add(2))? as u32;
            let offset_hi = i.extract(0, 11);
            let offset_lo = inst2.extract(0, 11);
            let base = pc.wrapping_add(4).wrapping_add((offset_hi << 12).sign_extend(23));
            let ret = pc.wrapping_add(4) | 1;
            if inst2 & 0xf800 == 0xf800 {
                // BL: stay in Thumb.
                let target = base.wrapping_add(offset_lo << 1);
                return Some((
                    FastOp::BranchLink {
                        exchange: false,
                        target,
                        ret,
                    },
                    4,
                ));
            } else if inst2 & 0xf801 == 0xe800 {
                // BLX: switch to ARM (target word-aligned, Thumb bit cleared).
                let target = base.wrapping_add(offset_lo << 1) & !3;
                return Some((FastOp::BranchLink { exchange: true, target, ret }, 4));
            }
            return None;
        }
        None
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
        self.code_pages[(pc >> CODE_PAGE_BITS) as usize] = true;
        let cur_gen = self.generation;
        let slot = &mut self.cache[idx];
        slot.gen_tag = cur_gen;
        slot.pc = pc;
        slot.block = Some(CompiledBlock { code, end_pc });
        true
    }

    /// Decode the ARM block at `start` into `ArmOp`s. `None` if the first
    /// instruction is not compilable (caller falls back). ARM instructions are a
    /// fixed 4 bytes, and the trace ends after the first terminator.
    fn decode_arm_block(&self, start: u32) -> Option<(alloc::vec::Vec<ArmOp>, u32)> {
        let mut ops = alloc::vec::Vec::new();
        let mut pc = start;
        while ops.len() < MAX_BLOCK_LEN {
            let inst = self.mem.load_u32(pc)?;
            match decode_arm(inst, pc) {
                Some(op) => {
                    ops.push(op);
                    pc = pc.wrapping_add(4);
                    if arm_ends_trace(&op) {
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

    /// Ensure a compiled ARM block for `pc` occupies its cache slot. `false` if
    /// the instruction there cannot start a compiled ARM block.
    fn ensure_arm_block(&mut self, pc: u32) -> bool {
        let idx = Self::slot_index(pc);
        if self.arm_cache[idx].gen_tag == self.generation && self.arm_cache[idx].pc == pc && self.arm_cache[idx].block.is_some() {
            return true;
        }
        let Some((ops, end_pc)) = self.decode_arm_block(pc) else {
            return false;
        };
        let Some((code, compiled_ops)) = backend::compile_arm_block(&ops, pc) else {
            return false;
        };
        let end_pc = if compiled_ops == ops.len() {
            end_pc
        } else {
            pc.wrapping_add(4 * compiled_ops as u32)
        };
        self.code_pages[(pc >> CODE_PAGE_BITS) as usize] = true;
        let cur_gen = self.generation;
        let slot = &mut self.arm_cache[idx];
        slot.gen_tag = cur_gen;
        slot.pc = pc;
        slot.block = Some(CompiledBlock { code, end_pc });
        true
    }

    /// Copy the flat working registers into the interpreter CPU. In the
    /// User/System bank this is a slice copy (r0..r15 at raw[0..16], CPSR at
    /// raw[16]); other banks use the per-register path. Fast because it runs on
    /// every interpreter fallback.
    fn store_back(&mut self, mode: Mode) {
        if self.cpu.cur_bank() == 0 {
            let dst = self.cpu.raw_regs_mut();
            dst[0..16].copy_from_slice(&self.ctx.regs);
            dst[16] = self.ctx.cpsr;
        } else {
            self.cpu.reg_set(mode, reg::CPSR, self.ctx.cpsr);
            for i in 0..16 {
                self.cpu.reg_set(mode, i as u8, self.ctx.regs[i]);
            }
        }
    }

    fn load_regs(&mut self, mode: Mode) {
        if self.cpu.cur_bank() == 0 {
            let src = self.cpu.raw_regs();
            self.ctx.regs.copy_from_slice(&src[0..16]);
            self.ctx.cpsr = src[16];
        } else {
            for i in 0..16 {
                self.ctx.regs[i] = self.cpu.reg_get(mode, i as u8);
            }
            self.ctx.cpsr = self.cpu.reg_get(mode, reg::CPSR);
        }
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
        self.ctx.pages = self.mem.pages_base();

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

            if !thumb {
                // ARM state. Prefer a compiled ARM block; when the instruction
                // here is not compilable, run the interpreter until it reaches one
                // (or Thumb, or a stop), syncing once around that batch instead of
                // per instruction.
                let arm_jit = self.ensure_arm_block(pc);
                let end_in_block = arm_jit && {
                    let b = self.arm_cache[Self::slot_index(pc)].block.as_ref().unwrap();
                    end > pc && end < b.end_pc
                };

                if arm_jit && !end_in_block {
                    crate::PC_SAMPLES[(pc >> 16) as usize].fetch_add(1, ::core::sync::atomic::Ordering::Relaxed);
                    self.ctx.faulted = 0;
                    self.ctx.smc = 0;
                    self.ctx.smc_n = 0;
                    let start_budget = budget.min(i32::MAX as u64) as i32;
                    self.ctx.budget = start_budget;
                    let reason = {
                        let block = self.arm_cache[Self::slot_index(pc)].block.as_ref().unwrap();
                        backend::run_block(&block.code, &mut self.ctx)
                    };
                    let retired = (start_budget - self.ctx.budget).max(0) as u64;
                    batch.0 += retired;
                    budget = budget.saturating_sub(retired);
                    match reason {
                        exit::CONTINUE => {}
                        exit::FAULT => break Err(WieError::InvalidMemoryAccess(self.ctx.fault_addr)),
                        exit::SMC => self.handle_smc(),
                        other => break Err(WieError::FatalError(format!("bad JIT exit {other}"))),
                    }
                    continue;
                }

                // Interpreter path for uncompiled ARM (or an `end` inside the
                // compiled block). `yield_to_jit` is off in the end-in-block case
                // so a single step makes progress instead of bouncing back to the
                // JIT it just declined.
                let yield_to_jit = !end_in_block;
                self.store_back(mode);
                let outcome: Option<Result<EngineRunResult>> = loop {
                    let apc = self.cpu.reg_get(Mode::User, reg::PC);
                    let acpsr = self.cpu.reg_get(Mode::User, reg::CPSR);
                    if apc == 0x08 && (acpsr & 0x1f) == 0x13 {
                        return self.read_svc_result();
                    }
                    if apc < 0x1000 {
                        break Some(Err(WieError::InvalidMemoryAccess(apc)));
                    }
                    if apc == end {
                        break Some(Ok(EngineRunResult::End));
                    }
                    if budget == 0 {
                        break Some(Ok(EngineRunResult::CountExhausted));
                    }
                    if acpsr & (1 << 5) != 0 {
                        break None; // back in Thumb; resume the JIT
                    }
                    if yield_to_jit && self.mem.load_u32(apc).is_some_and(|w| decode_arm(w, apc).is_some()) {
                        break None; // a compilable ARM op starts here; let the JIT take it
                    }
                    self.record_fallback(ARM_MODE);
                    let mut wrapper = self.mem.as_arm32cpu_memory();
                    let ok = self.cpu.step(&mut wrapper);
                    let mem_err = wrapper.memory_error();
                    batch.0 += 1;
                    budget = budget.saturating_sub(1);
                    if !ok {
                        let mut bytes = [0u8; 4];
                        let _ = self.mem.read_range(apc, 4, &mut bytes);
                        let opcode = u32::from_le_bytes(bytes);
                        break Some(Err(WieError::FatalError(format!(
                            "Undefined instruction at {apc:#x}: opcode {opcode:#010x}"
                        ))));
                    }
                    if let Some(addr) = mem_err {
                        break Some(Err(WieError::InvalidMemoryAccess(addr)));
                    }
                    if !yield_to_jit {
                        // Single-step mode: stop after exactly one instruction.
                        break None;
                    }
                };
                mode = self.cpu.mode();
                self.load_regs(mode);
                match outcome {
                    Some(r) => break r,
                    None => continue,
                }
            }

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
                self.ctx.smc_n = 0;
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
                    exit::SMC => self.handle_smc(),
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

/// Barrel-shifter for ARM operand2 / load-store register offset. `reg_shift` is
/// the ARM `r` bit (1 = shift amount from a register, 0 = immediate); `amount` is
/// the resolved shift amount (immediate, or `rs & 0xff`). Reuses the
/// interpreter's `arg_shift`/`arg_shift0` so the shifted value and its carry-out
/// match bit-for-bit. Returns the value in the low 32 bits, carry in bit 32.
pub(crate) extern "C" fn jit_arm_shift(val: u32, shift_type: u32, amount: u32, reg_shift: u32, c_in: u32) -> u64 {
    use arm32_cpu::util::arm::{arg_shift, arg_shift0};
    let (v, carry) = if reg_shift == 0 && amount == 0 {
        arg_shift0(val, shift_type, c_in)
    } else if amount != 0 {
        arg_shift(val, amount, shift_type)
    } else {
        (val, c_in)
    };
    (v as u64) | ((carry as u64) << 32)
}

/// Register-amount shift for the Thumb ALU LSL/LSR/ASR/ROR-by-register ops
/// (`AluOp` 0x2/0x3/0x4/0x7). Mirrors `engine::fast`'s `exec_straight` exactly: a
/// zero shift (low 8 bits of the amount register) leaves the value and carry
/// untouched, otherwise the interpreter's own `arg_shift` applies. Returns the
/// result in the low 32 bits and the new carry (0/1) in bit 32, so a single
/// return register carries both.
pub(crate) extern "C" fn jit_alu_shift(val: u32, amount: u32, shift_type: u32, c_in: u32) -> u64 {
    let shift = amount & 0xff;
    let (res, new_c) = if shift == 0 {
        (val, c_in)
    } else {
        arm32_cpu::util::arm::arg_shift(val, shift, shift_type)
    };
    (res as u64) | ((new_c as u64) << 32)
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
    // Flag a store into a page that holds compiled blocks. It only *might* be
    // self-modifying code: the page also holds data, so the dispatcher confirms
    // by checking whether `addr` actually falls in a compiled block's byte range.
    // Record the address for that precise check. Two distinct flagged addresses
    // in one run (`smc == 2`) leave the exact culprit ambiguous, so the
    // dispatcher then flushes everything.
    if !ctx.code_pages.is_null() {
        let page = (addr >> CODE_PAGE_BITS) as usize;
        if unsafe { *ctx.code_pages.add(page) } {
            ctx.smc = 1;
            let n = ctx.smc_n as usize;
            if n < SMC_ADDR_MAX {
                ctx.smc_addrs[n] = addr;
            }
            // Saturates past the buffer; the dispatcher reads `smc_n > MAX` as
            // "overflowed, flush everything" (a store op wider than 16 words,
            // which the ARM/Thumb encodings cannot produce, so never in practice).
            ctx.smc_n += 1;
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

    /// Assemble Thumb halfwords into a little-endian byte program.
    fn thumb(words: &[u16]) -> Vec<u8> {
        let mut v = Vec::with_capacity(words.len() * 2);
        for &w in words {
            v.extend_from_slice(&w.to_le_bytes());
        }
        v
    }

    /// Assemble ARM words into a little-endian byte program.
    fn arm(words: &[u32]) -> Vec<u8> {
        let mut v = Vec::with_capacity(words.len() * 4);
        for &w in words {
            v.extend_from_slice(&w.to_le_bytes());
        }
        v
    }

    /// Like `setup`, but enter in ARM state (PC even, T bit clear).
    fn setup_arm<E: ArmEngine>(mut e: E, code: &[u8], regs: &[u32; 15]) -> E {
        e.mem_map(0, 0x10000, MemoryPermission::ReadExecute);
        e.mem_map(DATA, DATA_SIZE, MemoryPermission::ReadWrite);
        e.mem_write(CODE, code).unwrap();
        for (i, &v) in regs.iter().enumerate() {
            e.reg_write(reg_of(i), v);
        }
        e.reg_write(ArmRegister::PC, CODE);
        e
    }

    fn assert_same_arm(code: &[u8], regs: &[u32; 15], end: u32) {
        let (jo, jr, jm) = drive(setup_arm(JitEngine::new(), code, regs), end, 37);
        let (so, sr, sm) = drive(setup_arm(Arm32CpuEngine::new(), code, regs), end, 37);
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

    #[test]
    fn jit_arm_then_thumb() {
        // ARM: mov r0,#0x42; add r1,r0,#1; bx r2 -> Thumb: movs r3,#0x55; b next.
        // Exercises the ARM batch, the ARM->Thumb handoff into compiled code, and
        // the final register sync.
        let mut code = arm(&[0xe3a00042, 0xe2801001, 0xe12fff12, 0xe1a00000]);
        code.extend_from_slice(&thumb(&[0x2355, B_NEXT]));
        let mut regs = [0u32; 15];
        regs[2] = (CODE + 0x10) | 1; // Thumb target for bx r2
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 0x14);
    }

    #[test]
    fn jit_arm_fault() {
        // ARM ldr r0,[r5] with r5 unmapped: both engines must fault identically.
        let code = arm(&[0xe5950000]);
        let mut regs = [0u32; 15];
        regs[5] = 0x0005_0000; // unmapped
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 0x100);
    }

    #[test]
    fn jit_arm_loop_to_end() {
        // A short ARM loop that decrements r0 and branches back, then reaches the
        // `end` breakpoint via fall-through. Exercises repeated ARM stepping and
        // exact stop-at-end inside the batch.
        // CODE+0: subs r0,r0,#1 (0xe2500001); CODE+4: bne CODE+0 (0x1afffffd).
        let code = arm(&[0xe2500001, 0x1afffffd]);
        let mut regs = [0u32; 15];
        regs[0] = 5;
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 8);
    }

    // `0xe7ff` is `b .` to the following halfword: a trailing one gives a trace a
    // clean block boundary exactly at `end`, so the ops before it are actually
    // JIT-compiled rather than single-stepped by the end-in-block guard.
    const B_NEXT: u16 = 0xe7ff;

    #[test]
    fn jit_mov_pc_register() {
        // mov pc, r2 (HiRegBx op 2, dest PC): a computed branch that, unlike bx,
        // does not interwork — target is r2 & !1 and execution stays in Thumb.
        // The target here is even, so a wrongly-cleared Thumb bit would diverge.
        let code = thumb(&[
            0x4697, // CODE+0: mov pc, r2
            0x0000, 0x0000, 0x0000, // padding to CODE+8
            0x2011, // CODE+8: movs r0, #0x11
            B_NEXT, // CODE+10 -> CODE+12
        ]);
        let mut regs = [0u32; 15];
        regs[2] = CODE + 8; // even target: mov pc masks bit 0 and keeps Thumb
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + 12);
    }

    #[test]
    fn jit_mov_pc_register_odd_target() {
        // Same, but the target register carries a set low bit; mov pc masks it off
        // (result identical to the even case) rather than interworking.
        let code = thumb(&[
            0x469f, // CODE+0: mov pc, r3
            0x0000, 0x0000, 0x0000, // padding to CODE+8
            0x2011, // CODE+8: movs r0, #0x11
            B_NEXT, // CODE+10 -> CODE+12
        ]);
        let mut regs = [0u32; 15];
        regs[3] = (CODE + 8) | 1;
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + 12);
    }

    #[test]
    fn jit_self_modifying_code() {
        // A counted loop patches its own first instruction, then re-enters the
        // block at that start. The compiler must invalidate the stale block so the
        // re-entry runs the patched instruction; without invalidation it would
        // keep running the pre-patch `adds r0, #1`. Differential against the
        // interpreter (which never caches): with correct invalidation both give
        // r0 = 1 + 0x10 + 0x10 = 0x21; a miss yields r0 = 3.
        let code = thumb(&[
            0x3001, // CODE+0: adds r0, #1     (patched -> adds r0, #0x10)
            0x6011, // CODE+2: str r1, [r2]    (r2 = CODE; rewrites CODE+0)
            0x3c01, // CODE+4: subs r4, #1
            0xd1fb, // CODE+6: bne CODE+0
            B_NEXT, // CODE+8: b CODE+10 (exit) — keeps the loop block a full JIT block
        ]);
        let mut regs = [0u32; 15];
        regs[1] = 0x6011_3010; // low: adds r0,#0x10 at CODE+0; high: str unchanged at CODE+2
        regs[2] = CODE; // patch target = block start
        regs[4] = 3; // iteration count
        regs[13] = DATA + 0x8000;
        // `end` at CODE+10 (a block boundary), so the loop body runs as a compiled
        // block rather than being single-stepped — which is what exercises the
        // stale-block invalidation.
        assert_same(&code, &regs, CODE + 10);
    }

    #[test]
    fn jit_push_into_code_page_is_not_smc() {
        // A `push` whose stack sits in the same page as code writes several words
        // to code-page addresses, flagging SMC for each. None overlap a compiled
        // block's instruction bytes, so nothing must be invalidated and execution
        // must match the interpreter. This exercises the multi-address SMC buffer.
        let code = thumb(&[
            0xb407, // CODE+0: push {r0,r1,r2}  (three code-page stack writes)
            0x2342, // CODE+2: movs r3, #0x42
            B_NEXT, // CODE+4 -> CODE+6
        ]);
        let mut regs = [0u32; 15];
        regs[0] = 0x1111;
        regs[1] = 0x2222;
        regs[2] = 0x3333;
        regs[13] = CODE + 0x800; // stack in the code's 64 KiB region, above the code
        assert_same(&code, &regs, CODE + 6);
    }

    #[test]
    fn jit_push_pop_roundtrip() {
        // push {r0,r1}; pop {r2,r3}; b next
        let code = thumb(&[0xb403, 0xbc0c, B_NEXT]);
        let mut regs = [0u32; 15];
        regs[0] = 0x1111_2222;
        regs[1] = 0x3333_4444;
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + code.len() as u32);
    }

    #[test]
    fn jit_push_pop_many() {
        // push {r0,r2,r4,r7,lr}; pop {r1,r3,r5,r6,pc}. The pop's pc lands on the
        // movs below (set via lr), which then branches to end.
        let code = thumb(&[
            0xb495,                // push {r0,r2,r4,r7,lr}
            0xbc00 | 0x100 | 0x6a, // pop {r1,r3,r5,r6,pc} = 0xbdea
            0x0000,
            0x0000, // padding at CODE+4, CODE+6
            0x2055, // CODE+8: movs r0, #0x55
            B_NEXT, // CODE+10 -> CODE+12
        ]);
        let mut regs = [0u32; 15];
        for (i, r) in regs.iter_mut().enumerate().take(13) {
            *r = 0x1000_0000u32.wrapping_add((i as u32) * 0x11);
        }
        regs[14] = (CODE + 8) | 1; // lr -> the movs (Thumb)
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + 12);
    }

    #[test]
    fn jit_pop_pc_return() {
        // push {r0}; pop {pc} (jumps to r0); ...; movs r1; b next.
        let code = thumb(&[
            0xb401, // CODE+0: push {r0}
            0xbd00, // CODE+2: pop {pc}
            0x0000, 0x0000, 0x0000, // padding to CODE+8
            0x2155, // CODE+8: movs r1, #0x55
            B_NEXT, // CODE+10 -> CODE+12
        ]);
        let mut regs = [0u32; 15];
        regs[0] = (CODE + 8) | 1; // return address (Thumb)
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + 12);
    }

    #[test]
    fn jit_bl_bx_call_return() {
        // bl func; movs r1; b next  |  func: movs r0; bx lr
        let code = thumb(&[
            0xf000, 0xf806, // CODE+0: bl CODE+0x10
            0x2133, // CODE+4: movs r1, #0x33
            B_NEXT, // CODE+6 -> CODE+8 (end)
            0x0000, 0x0000, 0x0000, 0x0000, // CODE+8..0xe padding
            0x2042, // CODE+0x10: movs r0, #0x42
            0x4770, // CODE+0x12: bx lr
        ]);
        let mut regs = [0u32; 15];
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + 8);
    }

    #[test]
    fn jit_bx_register() {
        // bx r2 (jumps to r2); ...; movs r0; b next
        let code = thumb(&[
            0x4710, // CODE+0: bx r2
            0x0000, 0x0000, 0x0000, // padding to CODE+8
            0x2011, // CODE+8: movs r0, #0x11
            B_NEXT, // CODE+10 -> CODE+12
        ]);
        let mut regs = [0u32; 15];
        regs[2] = (CODE + 8) | 1; // Thumb target
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + 12);
    }

    #[test]
    fn jit_ldm_stm_roundtrip() {
        // stmia r6!,{r0,r1,r2}; ldmia r7!,{r3,r4,r5}; b next (r6,r7 share a base).
        let code = thumb(&[0xc607, 0xcf38, B_NEXT]);
        let mut regs = [0u32; 15];
        regs[0] = 0x1111_1111;
        regs[1] = 0x2222_2222;
        regs[2] = 0x3333_3333;
        regs[6] = DATA + 0x100;
        regs[7] = DATA + 0x100;
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + code.len() as u32);
    }

    #[test]
    fn jit_stm_base_in_list() {
        // stmia r0!,{r0,r1}: r0 is the lowest reg, so slot 0 stores the original
        // base, not the written-back value.
        let code = thumb(&[0xc003, B_NEXT]);
        let mut regs = [0u32; 15];
        regs[0] = DATA + 0x200;
        regs[1] = 0x9999_9999;
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + code.len() as u32);
    }

    #[test]
    fn jit_ldm_base_in_list() {
        // Seed memory with stmia r5!,{r2,r3}, then ldmia r0!,{r0,r1} where r0 is
        // both base and in the list: the loaded value must win over writeback.
        let code = thumb(&[0xc50c, 0xc803, B_NEXT]);
        let mut regs = [0u32; 15];
        regs[0] = DATA + 0x300;
        regs[5] = DATA + 0x300;
        regs[2] = 0x0abc_0abc;
        regs[3] = 0x0def_0def;
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + code.len() as u32);
    }

    #[test]
    fn jit_stm_fault() {
        // stmia r6!,{r0,r1} onto an unmapped base: writeback still happens and
        // both engines fault at the same (last) address.
        let code = thumb(&[0xc603, B_NEXT]);
        let mut regs = [0u32; 15];
        regs[0] = 0xdead_beef;
        regs[1] = 0xfeed_face;
        regs[6] = 0x0005_0000; // unmapped
        regs[13] = DATA + 0x8000;
        assert_same(&code, &regs, CODE + code.len() as u32);
    }

    #[test]
    fn jit_push_fault() {
        // push {r0,r1} onto an unmapped stack: both engines must fault at the
        // same address with the same post-instruction register state.
        let code = thumb(&[0xb403, B_NEXT]);
        let mut regs = [0u32; 15];
        regs[0] = 0xaaaa_aaaa;
        regs[1] = 0xbbbb_bbbb;
        regs[13] = 0x0005_0000; // unmapped
        assert_same(&code, &regs, CODE + code.len() as u32);
    }

    // `b .+4` (branch to the following ARM instruction): gives an ARM trace a
    // clean end so the ops before it are JIT-compiled.
    const B_NEXT_ARM: u32 = 0xeaff_ffff;

    #[test]
    fn jit_arm_bl_return() {
        // bl func; mov r1,#0x33; b next | func: mov r0,#0x42; mov pc,lr
        let code = arm(&[
            0xeb00_0002, // CODE+0: bl CODE+0x10
            0xe3a0_1033, // CODE+4: mov r1,#0x33
            B_NEXT_ARM,  // CODE+8 -> CODE+0xC (end)
            0x0000_0000, // CODE+0xC pad
            0xe3a0_0042, // CODE+0x10: mov r0,#0x42
            0xe1a0_f00e, // CODE+0x14: mov pc,lr
        ]);
        let mut regs = [0u32; 15];
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 0xc);
    }

    #[test]
    fn jit_arm_conditional() {
        // movs r0,#0 (Z=1); addeq r1 (taken); addne r2 (skipped); b next
        let code = arm(&[0xe3b0_0000, 0x0281_1001, 0x1282_2001, B_NEXT_ARM]);
        let mut regs = [0u32; 15];
        regs[1] = 0x10;
        regs[2] = 0x20;
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 0x10);
    }

    #[test]
    fn jit_arm_stm_ldm_pc_return() {
        // stmfd sp!,{r4,lr}; ldmfd sp!,{r4,pc} (returns to lr); mov r5; b next
        let code = arm(&[
            0xe92d_4010, // CODE+0: stmfd sp!,{r4,lr}
            0xe8bd_8010, // CODE+4: ldmfd sp!,{r4,pc}
            0x0000_0000, // CODE+8 pad
            0x0000_0000, // CODE+0xC pad
            0xe3a0_5055, // CODE+0x10: mov r5,#0x55
            B_NEXT_ARM,  // CODE+0x14 -> CODE+0x18
        ]);
        let mut regs = [0u32; 15];
        regs[4] = 0xaaaa_aaaa;
        regs[14] = CODE + 0x10; // return address (ARM)
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 0x18);
    }

    #[test]
    fn jit_arm_ldr_str() {
        // str r0,[r4]; ldr r1,[r4]; str r2,[r4,#4]!; ldr r3,[r4],#-4; b next
        let code = arm(&[
            0xe584_0000, // str r0,[r4]
            0xe594_1000, // ldr r1,[r4]
            0xe5a4_2004, // str r2,[r4,#4]!  (pre, wb)
            0xe414_3004, // ldr r3,[r4],#-4  (post, down)
            B_NEXT_ARM,
        ]);
        let mut regs = [0u32; 15];
        regs[0] = 0x1234_5678;
        regs[2] = 0x9abc_def0;
        regs[4] = DATA + 0x400;
        regs[13] = DATA + 0x8000;
        assert_same_arm(&code, &regs, CODE + 0x14);
    }

    #[test]
    fn jit_arm_fuzz_straight_line() {
        use super::arm_frontend::{arm_ends_trace, decode_arm};
        // Random AL-condition ARM ops the JIT compiles (data-processing, single
        // and block transfers), run linearly to a trailing branch that ends the
        // block exactly at `end` so the trace is compiled rather than
        // single-stepped. Any divergence from the interpreter fails.
        const OPS: usize = 40;
        const B_NEXT_ARM: u32 = 0xeaff_ffff; // b .+4 (the following instruction)
        for seed in 1..=3000u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut words: Vec<u32> = Vec::with_capacity(OPS + 1);
            while words.len() < OPS {
                let w = (rng.u32() & 0x0fff_ffff) | 0xe000_0000; // force cond = AL
                let pc = CODE + (words.len() as u32) * 4;
                if let Some(op) = decode_arm(w, pc) {
                    if !arm_ends_trace(&op) {
                        words.push(w);
                    }
                }
            }
            words.push(B_NEXT_ARM);
            let mut code = Vec::with_capacity(words.len() * 4);
            for w in &words {
                code.extend_from_slice(&w.to_le_bytes());
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
            let end = CODE + (words.len() as u32) * 4;
            assert_same_arm(&code, &regs, end);
        }
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
