//! A block-caching Thumb execution engine — the frontend of the JIT.
//!
//! The stock [`Arm32CpuEngine`](super::Arm32CpuEngine) re-fetches, re-decodes
//! and re-dispatches every guest instruction through a banked register file and
//! checks loop terminators once per instruction. Profiling the games' software
//! sprite blitters (the on-device CPU bottleneck) showed the cost spread across
//! exactly that per-instruction overhead rather than any single hotspot.
//!
//! This engine attacks it structurally. A run of straight-line Thumb
//! instructions is decoded **once** into [`FastOp`]s with their operand fields
//! pre-extracted, cached by start PC, and executed against a flat `[u32; 16]`
//! register file, checking loop terminators once per *block*. Instruction
//! semantics are kept bit-identical to the interpreter by reusing arm32_cpu's
//! own flag/shift primitives, and anything outside the fast set (exotic Thumb,
//! ARM mode, SVC, multi-register transfers) falls back to a real
//! `arm32_cpu::Cpu` step, so the engine is correct by construction and merely
//! *faster* on the hot paths. Correctness is pinned by differential tests
//! against `Arm32CpuEngine` (see `bench.rs`).
//!
//! It is the substrate a machine-code backend plugs into later: replace
//! "interpret this block's `FastOp`s" with "emit and run native code for them".

use alloc::{boxed::Box, format, vec::Vec};
use core::array;

use arm32_cpu::{
    Cpu, Mode, reg,
    util::{
        arm::{arg_shift, arg_shift0, build_flags, cond_met},
        bit::BitUtilExt,
    },
};

use wie_util::{Result, WieError};

use super::arm32_cpu::EmulatedMemory;
use crate::engine::{ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

/// Longest straight-line run decoded into a single block.
const MAX_BLOCK_LEN: usize = 256;

/// A decoded Thumb operation with operand fields pre-extracted. Register fields
/// are indices into the flat `[u32; 16]` file. Straight-line variants advance
/// PC by 2; the two branch variants are always a block's final op and set PC.
#[derive(Clone, Copy)]
pub(crate) enum FastOp {
    /// Thumb `Shifted` (LSL/LSR/ASR immediate); sets NZC.
    Shift { op: u8, rd: u8, rs: u8, shift: u32 },
    /// `AddSub` with a register second operand; sets NZCV.
    AddSubReg { sub: bool, rd: u8, rs: u8, rn: u8 },
    /// `AddSub` with a 3-bit immediate second operand; sets NZCV.
    AddSubImm { sub: bool, rd: u8, rs: u8, imm: u32 },
    /// `ImmOp` (MOV/CMP/ADD/SUB with 8-bit immediate); sets NZCV.
    ImmOp { op: u8, rd: u8, imm: u32 },
    /// `AluOp` (the 16 data-processing ops).
    AluOp { op: u8, rd: u8, rs: u8 },
    /// High-register ADD/CMP/MOV where the destination is not PC (op 0/1/2).
    HiReg { op: u8, crd: u8, crs: u8 },
    /// `ldr rd, [pc, #imm]`.
    PcLoad { rd: u8, offset: u32 },
    /// `add rd, pc/sp, #imm` (`LoadAddr`).
    LoadAddr { sp: bool, rd: u8, imm: u32 },
    /// `add/sub sp, #imm` (`SpAdd`).
    SpAdd { sub: bool, imm: u32 },
    /// Halfword load/store, immediate offset (`HwXferI`).
    HwXferI { load: bool, rb: u8, rd: u8, offset: u32 },
    /// Word/byte load/store, immediate offset (`SingleXferI`).
    SingleXferI { load: bool, byte: bool, rb: u8, rd: u8, offset: u32 },
    /// Word/byte load/store, register offset (`SingleXferR`).
    SingleXferR { load: bool, byte: bool, ro: u8, rb: u8, rd: u8 },
    /// Halfword/signed load/store, register offset (`HwSgnXfer`).
    HwSgnXfer { s: bool, h: bool, ro: u8, rb: u8, rd: u8 },
    /// SP-relative word load/store (`SpXfer`).
    SpXfer { load: bool, rd: u8, offset: u32 },
    /// Conditional branch: if `cond` holds go to `target`, else `next`.
    CondBranch { cond: u8, target: u32, next: u32 },
    /// Unconditional short branch.
    Branch { target: u32 },
    /// `push`/`pop` (`PushPop`). `extra` is the R bit: LR for a push, PC for a
    /// pop. A pop that includes PC (`load && extra`) writes a dynamic PC and so
    /// ends the trace. Produced only by the JIT frontend (`decode` declines it).
    PushPop { load: bool, extra: bool, rlist: u8 },
    /// `bx`/`blx` register (`HiRegBx` op 3): jump to `rm`, switching ARM/Thumb
    /// from bit 0; `link` (BLX) also sets LR. Dynamic PC, so it ends the trace.
    /// Produced only by the JIT frontend.
    BranchExchange { link: bool, rm: u8 },
    /// Long branch with link (`bl`/`blx` immediate). `target`/`ret` are the
    /// pre-computed jump target and return address; `exchange` (BLX) clears the
    /// Thumb bit. A 32-bit instruction, so it advances PC by 4. Ends the trace.
    /// Produced only by the JIT frontend.
    BranchLink { exchange: bool, target: u32, ret: u32 },
}

/// Whether a decoded op continues the block or ends it (a branch).
pub(crate) enum Decoded {
    Straight(FastOp),
    Terminator(FastOp),
}

/// Whether a compiled op writes a non-linear (dynamic or far) PC and so must be
/// the last op in a JIT trace. `CondBranch` is excluded: it has a fall-through
/// and an in-range target can be linked inside the trace.
pub(crate) fn ends_trace(op: &FastOp) -> bool {
    matches!(
        op,
        FastOp::Branch { .. } | FastOp::BranchExchange { .. } | FastOp::BranchLink { .. } | FastOp::PushPop { load: true, extra: true, .. }
    )
}

/// Number of direct-mapped block-cache slots (power of two). A guest PC hashes
/// to `(pc >> 1) & (SLOTS - 1)`; hot loops touch only a handful of blocks, which
/// map to distinct slots and hit every time.
const CACHE_SLOTS: usize = 1 << 9;
const CACHE_MASK: u32 = (CACHE_SLOTS - 1) as u32;

/// One direct-mapped cache slot. Validity is `gen == engine.generation`, which
/// lets a cache flush be an O(1) generation bump instead of clearing every slot.
struct Slot {
    gen_tag: u32,
    pc: u32,
    ops: Vec<FastOp>,
}

impl Default for Slot {
    fn default() -> Self {
        Slot {
            gen_tag: 0,
            pc: 0,
            ops: Vec::new(),
        }
    }
}

pub struct FastCpuEngine {
    /// Authoritative CPU: holds banked register/mode state and executes any
    /// instruction the fast path declines. The flat register file below is a
    /// working copy loaded from / stored back to this around fast runs.
    cpu: Cpu,
    mem: EmulatedMemory,
    /// Direct-mapped decoded-block cache.
    cache: Box<[Slot; CACHE_SLOTS]>,
    /// Current cache generation; slots with an older `gen` are stale.
    generation: u32,
    /// Which 64 KiB regions currently hold cached blocks, so a guest store into
    /// one can invalidate the cache (self-modifying code) cheaply.
    code_pages: Box<[bool; 65536]>,
}

impl FastCpuEngine {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            mem: EmulatedMemory::new(),
            cache: Box::new(array::from_fn(|_| Slot::default())),
            generation: 1,
            code_pages: Box::new([false; 65536]),
        }
    }

    fn flush_blocks(&mut self) {
        // O(1): bump the generation so every cached slot reads as stale. On the
        // (astronomically rare) wrap, actually clear so gen 0 slots don't alias.
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            for slot in self.cache.iter_mut() {
                *slot = Slot::default();
            }
            self.generation = 1;
        }
        self.code_pages.fill(false);
    }
}

/// Flat working-register execution context for a fast run. `r[15]` is PC; `cpsr`
/// is kept separate. A memory fault records the offending address in `fault`,
/// mirroring the interpreter's post-instruction `memory_error` check.
struct Ctx<'a> {
    r: [u32; 16],
    cpsr: u32,
    mem: &'a mut EmulatedMemory,
    fault: Option<u32>,
}

impl Ctx<'_> {
    #[inline(always)]
    fn set_flags(&mut self, res: u32, v: u32, c: u32) {
        let z = (res == 0) as u32;
        let n = u32::is_neg(res) as u32;
        self.cpsr = (self.cpsr & !0xf000_0000) | (build_flags(v, c, z, n) << 28);
    }

    #[inline(always)]
    fn c_flag(&self) -> u32 {
        self.cpsr.get_bit(29)
    }
    #[inline(always)]
    fn v_flag(&self) -> u32 {
        self.cpsr.get_bit(28)
    }

    // Fault-checked memory accessors: on an unmapped page they record the fault
    // and yield 0 / drop the write, exactly as `Arm32CpuMemory` does before the
    // run loop aborts the instruction.
    #[inline(always)]
    fn r8(&mut self, addr: u32) -> u32 {
        match self.mem.load_u8(addr) {
            Some(v) => v as u32,
            None => {
                self.fault = Some(addr);
                0
            }
        }
    }
    #[inline(always)]
    fn r16(&mut self, addr: u32) -> u32 {
        match self.mem.load_u16(addr) {
            Some(v) => v as u32,
            None => {
                self.fault = Some(addr);
                0
            }
        }
    }
    #[inline(always)]
    fn r32(&mut self, addr: u32) -> u32 {
        // Replicate AlignmentWrapper: read the aligned word and rotate for a
        // misaligned address.
        let a = addr & !3;
        match self.mem.load_u32(a) {
            Some(v) => {
                if a == addr {
                    v
                } else {
                    v.rotate_right((addr & 3) * 8)
                }
            }
            None => {
                // The interpreter accesses (and thus faults at) the aligned word.
                self.fault = Some(a);
                0
            }
        }
    }
    #[inline(always)]
    fn w8(&mut self, addr: u32, val: u8) {
        if self.mem.store_u8(addr, val).is_none() {
            self.fault = Some(addr);
        }
    }
    #[inline(always)]
    fn w16(&mut self, addr: u32, val: u16) {
        if self.mem.store_u16(addr, val).is_none() {
            self.fault = Some(addr);
        }
    }
    #[inline(always)]
    fn w32(&mut self, addr: u32, val: u32) {
        // AlignmentWrapper stores to (and faults at) the down-aligned address.
        let a = addr & !3;
        if self.mem.store_u32(a, val).is_none() {
            self.fault = Some(a);
        }
    }

    /// Execute one straight-line op. `pc` is the instruction's own address;
    /// `r[15]` has already been set to `pc + 2` so PC-relative reads match the
    /// interpreter's pipeline offset. Returns whether a store hit a code page
    /// (self-modifying code) so the caller can invalidate.
    #[inline]
    fn exec_straight(&mut self, op: FastOp) {
        match op {
            FastOp::Shift { op, rd, rs, shift } => {
                let val = self.r[rs as usize];
                let c = self.c_flag();
                let (res, new_c) = if shift == 0 {
                    arg_shift0(val, op as u32, c)
                } else {
                    arg_shift(val, shift, op as u32)
                };
                self.r[rd as usize] = res;
                self.set_flags(res, self.v_flag(), new_c);
            }
            FastOp::AddSubReg { sub, rd, rs, rn } => {
                let a = self.r[rs as usize];
                let b = self.r[rn as usize];
                let (res, v, c) = if sub { a.sub_flags(b, 0) } else { a.add_flags(b, 0) };
                self.r[rd as usize] = res;
                self.set_flags(res, v, c);
            }
            FastOp::AddSubImm { sub, rd, rs, imm } => {
                let a = self.r[rs as usize];
                let (res, v, c) = if sub { a.sub_flags(imm, 0) } else { a.add_flags(imm, 0) };
                self.r[rd as usize] = res;
                self.set_flags(res, v, c);
            }
            FastOp::ImmOp { op, rd, imm } => {
                let (res, v, c) = match op {
                    0 => (imm, self.v_flag(), self.c_flag()),
                    1 | 3 => self.r[rd as usize].sub_flags(imm, 0),
                    2 => self.r[rd as usize].add_flags(imm, 0),
                    _ => unreachable!(),
                };
                if op != 1 {
                    self.r[rd as usize] = res;
                }
                self.set_flags(res, v, c);
            }
            FastOp::AluOp { op, rd, rs } => {
                let c = self.c_flag();
                let v = self.v_flag();
                let vals = self.r[rs as usize];
                let vald = self.r[rd as usize];
                let (res, new_v, new_c) = match op {
                    0x0 | 0x8 => (vald & vals, v, c),
                    0x1 => (vald ^ vals, v, c),
                    0x2 | 0x3 | 0x4 | 0x7 => {
                        let shift = vals & 0xff;
                        let (res, new_c) = if shift == 0 {
                            (vald, c)
                        } else {
                            let shift_type = ((op >> 1) & 2) | (op & 1);
                            arg_shift(vald, shift, shift_type as u32)
                        };
                        (res, v, new_c)
                    }
                    0x5 => vald.add_flags(vals, c),
                    0x6 => vald.sub_flags(vals, 1 - c),
                    0x9 => 0u32.sub_flags(vals, 0),
                    0xA => vald.sub_flags(vals, 0),
                    0xB => vald.add_flags(vals, 0),
                    0xC => (vald | vals, v, c),
                    0xD => (vald.wrapping_mul(vals), v, 0),
                    0xE => (vald & !vals, v, c),
                    0xF => (!vals, v, c),
                    _ => unreachable!(),
                };
                match op {
                    0x8 | 0xA | 0xB => (),
                    _ => self.r[rd as usize] = res,
                }
                self.set_flags(res, new_v, new_c);
            }
            FastOp::HiReg { op, crd, crs } => {
                let vals = self.r[crs as usize].wrapping_add(((crs == reg::PC) as u32) * 2);
                let vald = self.r[crd as usize].wrapping_add(((crd == reg::PC) as u32) * 2);
                match op {
                    0 => self.r[crd as usize] = vald.wrapping_add(vals),
                    1 => {
                        let (res, v, c) = vald.sub_flags(vals, 0);
                        self.set_flags(res, v, c);
                    }
                    2 => self.r[crd as usize] = vals,
                    _ => unreachable!(),
                }
            }
            FastOp::PcLoad { rd, offset } => {
                let addr = self.r[15].wrapping_add(2).wrapping_add(offset * 4) & !3;
                self.r[rd as usize] = self.r32(addr);
            }
            FastOp::LoadAddr { sp, rd, imm } => {
                let base = if sp { self.r[reg::SP as usize] } else { self.r[15].wrapping_add(2) & !2 };
                self.r[rd as usize] = base.wrapping_add(imm * 4);
            }
            FastOp::SpAdd { sub, imm } => {
                let sp = self.r[reg::SP as usize];
                self.r[reg::SP as usize] = if sub { sp.wrapping_sub(imm) } else { sp.wrapping_add(imm) };
            }
            FastOp::HwXferI { load, rb, rd, offset } => {
                let addr = self.r[rb as usize].wrapping_add(offset * 2) & !1;
                if load {
                    self.r[rd as usize] = self.r16(addr);
                } else {
                    self.w16(addr, self.r[rd as usize] as u16);
                }
            }
            FastOp::SingleXferI { load, byte, rb, rd, offset } => {
                if byte {
                    let addr = self.r[rb as usize].wrapping_add(offset);
                    if load {
                        self.r[rd as usize] = self.r8(addr);
                    } else {
                        self.w8(addr, self.r[rd as usize] as u8);
                    }
                } else {
                    let addr = self.r[rb as usize].wrapping_add(offset * 4);
                    if load {
                        self.r[rd as usize] = self.r32(addr);
                    } else {
                        self.w32(addr, self.r[rd as usize]);
                    }
                }
            }
            FastOp::SingleXferR { load, byte, ro, rb, rd } => {
                let addr = self.r[rb as usize].wrapping_add(self.r[ro as usize]);
                match (load, byte) {
                    (false, false) => self.w32(addr, self.r[rd as usize]),
                    (false, true) => self.w8(addr, self.r[rd as usize] as u8),
                    (true, false) => self.r[rd as usize] = self.r32(addr),
                    (true, true) => self.r[rd as usize] = self.r8(addr),
                }
            }
            FastOp::HwSgnXfer { s, h, ro, rb, rd } => {
                let addr = self.r[rb as usize].wrapping_add(self.r[ro as usize]);
                match (s, h) {
                    (false, false) => self.w16(addr & !1, self.r[rd as usize] as u16),
                    (false, true) => self.r[rd as usize] = self.r16(addr & !1),
                    (true, false) => self.r[rd as usize] = self.r8(addr) as u8 as i8 as u32,
                    (true, true) => self.r[rd as usize] = self.r16(addr & !1) as u16 as i16 as u32,
                }
            }
            FastOp::SpXfer { load, rd, offset } => {
                let addr = self.r[reg::SP as usize].wrapping_add(offset);
                if load {
                    self.r[rd as usize] = self.r32(addr);
                } else {
                    self.w32(addr, self.r[rd as usize]);
                }
            }
            FastOp::CondBranch { .. }
            | FastOp::Branch { .. }
            // The JIT-only control-flow ops are never produced by `decode` (used
            // by this engine), so they never reach here.
            | FastOp::PushPop { .. }
            | FastOp::BranchExchange { .. }
            | FastOp::BranchLink { .. } => unreachable!("branch in straight-line slot"),
        }
    }
}

/// Decode one Thumb instruction word at `pc` into a fast op, or `None` if it is
/// outside the fast set (caller falls back to the interpreter for it).
pub(crate) fn decode(inst: u16, pc: u32) -> Option<Decoded> {
    let i = inst as u32;
    let hi = inst >> 8;
    // Mirror arm32_cpu's decode groups, but only for the fast subset.
    if hi & 0xe0 == 0x00 && hi & 0x18 != 0x18 {
        // Shifted (LSL/LSR/ASR imm). 0b000xx, excluding 0b00011 (AddSub).
        let op = i.extract(11, 2) as u8;
        return Some(Decoded::Straight(FastOp::Shift {
            op,
            rd: i.extract(0, 3) as u8,
            rs: i.extract(3, 3) as u8,
            shift: i.extract(6, 5),
        }));
    }
    if i & 0xf800 == 0x1800 {
        // AddSub
        let sub = i.get_bit(9) == 1;
        let rd = i.extract(0, 3) as u8;
        let rs = i.extract(3, 3) as u8;
        let rn = i.extract(6, 3);
        return Some(Decoded::Straight(if i.get_bit(10) == 0 {
            FastOp::AddSubReg { sub, rd, rs, rn: rn as u8 }
        } else {
            FastOp::AddSubImm { sub, rd, rs, imm: rn }
        }));
    }
    if i & 0xe000 == 0x2000 {
        // ImmOp
        return Some(Decoded::Straight(FastOp::ImmOp {
            op: i.extract(11, 2) as u8,
            rd: i.extract(8, 3) as u8,
            imm: i.extract(0, 8),
        }));
    }
    if i & 0xfc00 == 0x4000 {
        // AluOp
        return Some(Decoded::Straight(FastOp::AluOp {
            op: i.extract(6, 4) as u8,
            rd: i.extract(0, 3) as u8,
            rs: i.extract(3, 3) as u8,
        }));
    }
    if i & 0xfc00 == 0x4400 {
        // HiRegBx. Fast only for ADD/CMP/MOV (op 0/1/2) with destination != PC;
        // BX (op 3) and PC-writing forms fall back (control flow).
        let op = i.extract(8, 2) as u8;
        let crs = ((i.get_bit(6) * 8) + i.extract(3, 3)) as u8;
        let crd = ((i.get_bit(7) * 8) + i.extract(0, 3)) as u8;
        if op != 3 && crd != reg::PC {
            return Some(Decoded::Straight(FastOp::HiReg { op, crd, crs }));
        }
        return None;
    }
    if i & 0xf800 == 0x4800 {
        return Some(Decoded::Straight(FastOp::PcLoad {
            rd: i.extract(8, 3) as u8,
            offset: i.extract(0, 8),
        }));
    }
    if i & 0xf200 == 0x5000 {
        // SingleXferR
        return Some(Decoded::Straight(FastOp::SingleXferR {
            load: i.get_bit(11) == 1,
            byte: i.get_bit(10) == 1,
            ro: i.extract(6, 3) as u8,
            rb: i.extract(3, 3) as u8,
            rd: i.extract(0, 3) as u8,
        }));
    }
    if i & 0xf200 == 0x5200 {
        // HwSgnXfer
        return Some(Decoded::Straight(FastOp::HwSgnXfer {
            h: i.get_bit(11) == 1,
            s: i.get_bit(10) == 1,
            ro: i.extract(6, 3) as u8,
            rb: i.extract(3, 3) as u8,
            rd: i.extract(0, 3) as u8,
        }));
    }
    if i & 0xe000 == 0x6000 {
        // SingleXferI (word/byte); bit 12 selects byte, bit 11 load.
        return Some(Decoded::Straight(FastOp::SingleXferI {
            load: i.get_bit(11) == 1,
            byte: i.get_bit(12) == 1,
            rb: i.extract(3, 3) as u8,
            rd: i.extract(0, 3) as u8,
            offset: i.extract(6, 5),
        }));
    }
    if i & 0xf000 == 0x8000 {
        // HwXferI
        return Some(Decoded::Straight(FastOp::HwXferI {
            load: i.get_bit(11) == 1,
            rb: i.extract(3, 3) as u8,
            rd: i.extract(0, 3) as u8,
            offset: i.extract(6, 5),
        }));
    }
    if i & 0xf000 == 0x9000 {
        // SpXfer
        return Some(Decoded::Straight(FastOp::SpXfer {
            load: i.get_bit(11) == 1,
            rd: i.extract(8, 3) as u8,
            offset: i.extract(0, 8) * 4,
        }));
    }
    if i & 0xf000 == 0xa000 {
        // LoadAddr
        return Some(Decoded::Straight(FastOp::LoadAddr {
            sp: i.get_bit(11) == 1,
            rd: i.extract(8, 3) as u8,
            imm: i.extract(0, 8),
        }));
    }
    if i & 0xff00 == 0xb000 {
        // SpAdd
        return Some(Decoded::Straight(FastOp::SpAdd {
            sub: i.get_bit(7) == 1,
            imm: i.extract(0, 7) * 4,
        }));
    }
    if i & 0xf000 == 0xd000 {
        // CondBranch (0xdf00 SWI and 0xde00 undefined are excluded).
        let cond = i.extract(8, 4) as u8;
        if cond == 0xe || cond == 0xf {
            return None; // undefined / SWI encodings
        }
        let offset = i.extract(0, 8) as i8 as u32;
        let target = pc.wrapping_add(4).wrapping_add(offset << 1);
        return Some(Decoded::Terminator(FastOp::CondBranch {
            cond,
            target,
            next: pc.wrapping_add(2),
        }));
    }
    if i & 0xf800 == 0xe000 {
        // Unconditional Branch
        let offset = (i.extract(0, 11) << 1).sign_extend(12);
        return Some(Decoded::Terminator(FastOp::Branch {
            target: pc.wrapping_add(4).wrapping_add(offset),
        }));
    }
    None
}

impl FastCpuEngine {
    #[inline(always)]
    fn slot_index(pc: u32) -> usize {
        ((pc >> 1) & CACHE_MASK) as usize
    }

    /// Ensure a decoded block for `pc` occupies its cache slot, building it if
    /// the slot is stale or holds a different PC. Returns `false` if the
    /// instruction there is not fast-decodable (caller falls back for one
    /// instruction).
    fn ensure_block(&mut self, pc: u32) -> bool {
        let idx = Self::slot_index(pc);
        if self.cache[idx].gen_tag == self.generation && self.cache[idx].pc == pc {
            return true;
        }
        match self.build_block(pc) {
            Some(ops) => {
                self.code_pages[(pc >> 16) as usize] = true;
                let cur_gen = self.generation;
                let slot = &mut self.cache[idx];
                slot.gen_tag = cur_gen;
                slot.pc = pc;
                slot.ops = ops;
                true
            }
            None => false,
        }
    }

    fn build_block(&self, start: u32) -> Option<Vec<FastOp>> {
        let mut ops = Vec::new();
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
                    break;
                }
                None => break, // fall-through to a fallback instruction
            }
        }
        if ops.is_empty() {
            return None;
        }
        Some(ops)
    }
}

impl ArmEngine for FastCpuEngine {
    fn run(&mut self, end: u32, count: u32) -> Result<EngineRunResult> {
        struct Batch(u64);
        impl Drop for Batch {
            fn drop(&mut self) {
                crate::EXECUTED_INSTRUCTIONS.fetch_add(self.0, ::core::sync::atomic::Ordering::Relaxed);
            }
        }
        let mut batch = Batch(0);
        let mut budget = count as u64;

        // Load the flat working register file from the authoritative CPU using
        // its *current* bank: the guest may run in Supervisor/IRQ/etc. mode, in
        // which SP, LR and (for FIQ) r8-r12 are banked. Fast ops never change
        // the mode bits, so this bank stays valid for the whole fast run; only
        // a fallback instruction can switch modes, after which it is refreshed.
        let mut mode = self.cpu.mode();
        let mut r = [0u32; 16];
        for (i, slot) in r.iter_mut().enumerate() {
            *slot = self.cpu.reg_get(mode, i as u8);
        }
        let mut cpsr = self.cpu.reg_get(mode, reg::CPSR);

        let result = loop {
            let pc = r[15];

            if pc == 0x08 && (cpsr & 0x1f) == 0x13 {
                // SVC vector reached in supervisor mode — store back and let the
                // interpreter engine's SVC path read the result.
                self.store_back(mode, &r, cpsr);
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

            // Thumb only in the fast path: an ARM-mode PC (T bit clear) falls
            // back for its whole run of instructions.
            let thumb = cpsr & (1 << 5) != 0;
            let has_block = thumb && self.ensure_block(pc);

            if has_block {
                crate::PC_SAMPLES[(pc >> 16) as usize].fetch_add(1, ::core::sync::atomic::Ordering::Relaxed);
                // Borrow the disjoint fields directly so the cached block (in
                // `cache`) can be read while `mem` is mutated by loads/stores.
                let (fault, hit_end, retired, smc) = {
                    let FastCpuEngine { cache, mem, code_pages, .. } = &mut *self;
                    let block = &cache[Self::slot_index(pc)];
                    // Only watch for `end` per-instruction when it actually falls
                    // inside this block's PC span. In normal control flow `end`
                    // (a return sentinel) is only ever reached as a branch target,
                    // i.e. at a block start, so the hot path skips this check.
                    let block_span = pc.wrapping_add(2 * block.ops.len() as u32);
                    let end_in_block = end >= pc && end < block_span && end & 1 == pc & 1;
                    let mut ctx = Ctx { r, cpsr, mem, fault: None };
                    let mut cur = pc;
                    let mut smc = false;
                    let mut hit_end = false;
                    for &op in &block.ops {
                        // Stop before executing the instruction at `end`, even
                        // mid-block: a straight-line block can span the stop
                        // address (e.g. it extends into not-yet-branch code).
                        if end_in_block && cur == end {
                            hit_end = true;
                            break;
                        }
                        ctx.r[15] = cur.wrapping_add(2);
                        match op {
                            FastOp::CondBranch { cond, target, next } => {
                                ctx.r[15] = if cond_met(cond as u32, ctx.cpsr) { target } else { next };
                            }
                            FastOp::Branch { target } => ctx.r[15] = target,
                            other => {
                                ctx.exec_straight(other);
                                // A store into a page holding cached blocks is
                                // self-modifying code: flag for invalidation.
                                if ctx.fault.is_none()
                                    && is_store(other)
                                    && let Some(addr) = store_addr(other, &ctx.r, cur)
                                    && code_pages[(addr >> 16) as usize]
                                {
                                    smc = true;
                                }
                            }
                        }
                        if ctx.fault.is_some() {
                            break;
                        }
                        cur = cur.wrapping_add(2);
                        if smc {
                            // The store just retired modified a code page. Stop
                            // before running any further pre-decoded (now stale)
                            // ops in this block; the outer loop re-decodes from
                            // the next PC against the modified memory.
                            break;
                        }
                    }
                    let fault = ctx.fault;
                    if hit_end {
                        ctx.r[15] = end;
                    }
                    r = ctx.r;
                    cpsr = ctx.cpsr;
                    // Instructions retired: those fully executed (`cur` advanced
                    // past each), plus the faulting one (which still counts, like
                    // the interpreter). `end` stops before executing, so 0 extra.
                    let retired = (cur.wrapping_sub(pc)) / 2 + fault.is_some() as u32;
                    (fault, hit_end, retired, smc)
                };
                batch.0 += retired as u64;
                budget = budget.saturating_sub(retired as u64);

                if let Some(addr) = fault {
                    break Err(WieError::InvalidMemoryAccess(addr));
                }
                if hit_end {
                    break Ok(EngineRunResult::End);
                }
                if smc {
                    self.flush_blocks();
                }
            } else {
                // Fallback: execute exactly one instruction on the real CPU.
                self.store_back(mode, &r, cpsr);
                let mut wrapper = self.mem.as_arm32cpu_memory();
                let ok = self.cpu.step(&mut wrapper);
                let mem_err = wrapper.memory_error();
                // `step` may have taken an exception (undefined instruction, SVC)
                // that changed the mode and PC. Refresh the bank and reload the
                // flat registers from the post-step state before deciding how to
                // exit, so every outcome matches the interpreter's final state.
                mode = self.cpu.mode();
                for (i, slot) in r.iter_mut().enumerate() {
                    *slot = self.cpu.reg_get(mode, i as u8);
                }
                cpsr = self.cpu.reg_get(mode, reg::CPSR);
                if !ok {
                    // Undefined instruction: like the interpreter, the taken
                    // exception state is left in place (already reloaded above).
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

        // Publish the flat registers back on every structured exit (Ok or Err),
        // so the authoritative CPU reflects the state at the stopping point —
        // matching the interpreter, which updates its registers in place. (The
        // SVC path returned earlier, after its own store-back.)
        self.store_back(mode, &r, cpsr);
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

impl FastCpuEngine {
    fn store_back(&mut self, mode: Mode, r: &[u32; 16], cpsr: u32) {
        // Write CPSR first so the register bank matches before GP writes. `mode`
        // is the bank the flat file was loaded from; fast ops never change the
        // mode bits, so it still matches `cpsr`'s mode.
        self.cpu.reg_set(mode, reg::CPSR, cpsr);
        for (i, &v) in r.iter().enumerate() {
            self.cpu.reg_set(mode, i as u8, v);
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

/// Whether the op writes memory (for self-modifying-code detection).
#[inline(always)]
fn is_store(op: FastOp) -> bool {
    matches!(
        op,
        FastOp::HwXferI { load: false, .. }
            | FastOp::SingleXferI { load: false, .. }
            | FastOp::SingleXferR { load: false, .. }
            | FastOp::HwSgnXfer { s: false, .. }
            | FastOp::SpXfer { load: false, .. }
    )
}

/// The destination address of a store op, given the (already updated) register
/// file and the instruction's own pc. Used only for SMC page-invalidation.
#[inline(always)]
fn store_addr(op: FastOp, r: &[u32; 16], _pc: u32) -> Option<u32> {
    Some(match op {
        FastOp::HwXferI { rb, offset, .. } => r[rb as usize].wrapping_add(offset * 2) & !1,
        FastOp::SingleXferI { byte, rb, offset, .. } => {
            if byte {
                r[rb as usize].wrapping_add(offset)
            } else {
                r[rb as usize].wrapping_add(offset * 4)
            }
        }
        FastOp::SingleXferR { ro, rb, .. } => r[rb as usize].wrapping_add(r[ro as usize]),
        FastOp::HwSgnXfer { ro, rb, .. } => r[rb as usize].wrapping_add(r[ro as usize]) & !1,
        FastOp::SpXfer { offset, .. } => r[reg::SP as usize].wrapping_add(offset),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::vec;
    use alloc::vec::Vec;

    use super::{Decoded, FastCpuEngine, decode};
    use crate::engine::{Arm32CpuEngine, ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

    const CODE: u32 = 0x1000;
    const DATA: u32 = 0x0010_0000;
    const DATA_SIZE: usize = 0x0010_0000;

    /// Map `ArmRegister` for a flat index 0..=15.
    fn reg_of(i: usize) -> ArmRegister {
        use ArmRegister::*;
        [R0, R1, R2, R3, R4, R5, R6, R7, R8, SB, SL, FP, IP, SP, LR, PC][i]
    }

    /// A comparable summary of a run's outcome.
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
        e.reg_write(ArmRegister::PC, CODE | 1); // enter Thumb at CODE
        e
    }

    /// Read r0..=r15 and CPSR.
    fn snapshot<E: ArmEngine>(e: &E) -> [u32; 17] {
        let mut s = [0u32; 17];
        for (i, slot) in s.iter_mut().enumerate().take(16) {
            *slot = e.reg_read(reg_of(i));
        }
        s[16] = e.reg_read(ArmRegister::Cpsr);
        s
    }

    /// Drive one engine to `end` (or a stop), returning outcome + register
    /// snapshot + the data region contents.
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

    /// Run identical setup through both engines and assert bit-for-bit identical
    /// outcome, registers and data memory.
    fn assert_same(code: &[u8], regs: &[u32; 15], end: u32) {
        // Drive each engine to completion sequentially (not both live at once):
        // each holds a 512 KiB inline page table, so keeping only one on the
        // stack at a time avoids overflowing the test thread. The odd budget
        // exercises the run/resume boundary.
        let (fo, fr, fm) = drive(setup(FastCpuEngine::new(), code, regs), end, 37);
        let (so, sr, sm) = drive(setup(Arm32CpuEngine::new(), code, regs), end, 37);
        assert_eq!(so, fo, "outcome differs (interp {so:?} vs fast {fo:?})");
        if sr != fr {
            for i in 0..17 {
                if sr[i] != fr[i] {
                    panic!("reg[{i}] differs: interp {:#010x} vs fast {:#010x}", sr[i], fr[i]);
                }
            }
        }
        assert!(sm == fm, "data memory differs between engines");
    }

    #[test]
    fn mixed_fallback_and_branch() {
        // movs r0,#0x12 / push {r0} / adds r0,#1 / pop {r1} / adds r1,r1,r0 /
        // b end / movs r2,#0xff (skipped) / nop. Exercises the interpreter
        // fallback (push/pop), a block-ending branch, and register sync.
        #[rustfmt::skip]
        let code = [0x12,0x20, 0x01,0xb4, 0x40,0x1c, 0x02,0xbc, 0x09,0x18, 0x00,0xe0, 0xff,0x22, 0xc0,0x46];
        let mut regs = [0u32; 15];
        regs[13] = DATA + 0x8000; // sp
        assert_same(&code, &regs, CODE + 0xe);
    }

    /// Tiny deterministic PRNG (xorshift64).
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
    fn debug_find_bug() {
        // Mirror `fuzz_straight_line`'s program and register generation so a
        // failure there can be reproduced and bisected here.
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
            // Bisect on program length: find the smallest K where running just
            // the first K instructions already diverges.
            for k in 1..=OPS {
                let end = CODE + (k as u32) * 2;
                let (fo, fr, _) = drive(setup(FastCpuEngine::new(), &code, &regs), end, 1_000_000);
                let (so, sr, _) = drive(setup(Arm32CpuEngine::new(), &code, &regs), end, 1_000_000);
                if fo != so || fr != sr {
                    let w = u16::from_le_bytes([code[(k - 1) * 2], code[(k - 1) * 2 + 1]]);
                    std::eprintln!("seed {seed}: diverges at op #{} word {w:#06x}; outcome interp {so:?} fast {fo:?}", k - 1);
                    if fr != sr {
                        for i in 0..17 {
                            if fr[i] != sr[i] {
                                std::eprintln!("   reg[{i}] interp {:#010x} fast {:#010x}", sr[i], fr[i]);
                            }
                        }
                    }
                    return;
                }
            }
        }
        std::eprintln!("no divergence found");
    }

    #[test]
    fn fuzz_straight_line() {
        // For many seeds, build a straight-line program of random instructions
        // drawn only from the fast set's *straight* ops (no branches, so it runs
        // linearly to `end`), with registers seeded to point into the data
        // region. Any divergence in final registers, memory, or fault behaviour
        // between the fast engine and the interpreter fails the test.
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
                // Mix of bases: most near the data-region start (accesses stay
                // mapped), some small or near a page boundary to stress the
                // alignment and fault edges. Whatever the outcome — completion or
                // an identical fault — both engines must agree.
                *r = match rng.u32() & 3 {
                    0 => DATA + (rng.u32() & 0x3ff) * 4 + (i as u32) * 4,
                    1 => rng.u32() & 0xff,
                    2 => DATA + 0xfff8 + (rng.u32() & 0xf),
                    _ => rng.u32(),
                };
            }
            regs[13] = DATA + 0x8000; // sp
            let end = CODE + (OPS as u32) * 2;
            assert_same(&code, &regs, end);
        }
    }
}
