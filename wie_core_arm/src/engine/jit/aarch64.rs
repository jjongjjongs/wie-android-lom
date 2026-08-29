//! AArch64 code generator for the JIT.
//!
//! A faithful mirror of the x86-64 backend (`x64.rs`): same trace/linking/budget
//! structure and the same instruction *selection* (via the shared `decode`
//! frontend), only the *encoding* differs. It is deliberately kept simple —
//! every conditional branch calls the interpreter's `cond_met` rather than
//! inlining flag tests. Its correctness is pinned by the same differential
//! tests (`engine::jit::tests`), which pass on AArch64 hardware and under
//! `qemu-aarch64`:
//!
//! ```text
//! CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
//! CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER=qemu-aarch64-static \
//! QEMU_LD_PREFIX=/usr/aarch64-linux-gnu \
//! cargo test -p wie_core_arm --features jit --target aarch64-unknown-linux-gnu engine::jit
//! ```
//!
//! Convention (AAPCS64): `x0` holds the context pointer on entry, saved into the
//! callee-saved `x19` for the block; guest register `i` lives at `[x19, #i*4]`,
//! CPSR at `[x19, #64]`. The function returns an `exit::*` code in `w0` and
//! leaves `regs[15]` (`[x19, #60]`) at the next guest PC.

use alloc::collections::{BTreeMap, BTreeSet};

use dynasmrt::{AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer, dynasm};

use crate::engine::fast::{FastOp, ends_trace};

use super::arm_frontend::{ArmOp, Off, Op2, arm_ends_trace};
use super::{JitCtx, exit, jit_alu_shift, jit_arm_shift, jit_cond_met, jit_load8, jit_load16, jit_load32, jit_store8, jit_store16, jit_store32};

/// `dynasm!` selects the target architecture per invocation, defaulting to x64;
/// this wrapper prepends `.arch aarch64` so every code-emitting helper assembles
/// AArch64 without repeating the directive.
macro_rules! a64 {
    ($ops:expr ; $($t:tt)*) => {
        dynasm!($ops ; .arch aarch64 ; $($t)*)
    };
}

pub(crate) struct Code {
    buf: ExecutableBuffer,
    entry: AssemblyOffset,
}

pub(crate) fn run_block(code: &Code, ctx: &mut JitCtx) -> u32 {
    let f: extern "C" fn(*mut JitCtx) -> u32 = unsafe { core::mem::transmute(code.buf.ptr(code.entry)) };
    f(ctx as *mut JitCtx)
}

#[inline(always)]
fn ro(r: u8) -> u32 {
    r as u32 * 4
}
const CPSR: u32 = 64;
const PC: u32 = 15 * 4;
const FAULTED: u32 = 72;
const SMC: u32 = 76;
const BUDGET: u32 = 96;
const SCRATCH: u32 = 100;
/// Offset of `JitCtx::pages` (base of the guest page table).
const PAGES: u32 = 104;

type Asm = dynasmrt::aarch64::Assembler;

fn supported(op: FastOp) -> bool {
    match op {
        FastOp::AddSubImm { .. }
        | FastOp::AddSubReg { .. }
        | FastOp::ImmOp { .. }
        | FastOp::PcLoad { .. }
        | FastOp::LoadAddr { .. }
        | FastOp::SpAdd { .. }
        | FastOp::HwXferI { .. }
        | FastOp::SingleXferI { .. }
        | FastOp::SpXfer { .. }
        | FastOp::SingleXferR { .. }
        | FastOp::Shift { .. }
        | FastOp::HwSgnXfer { .. }
        | FastOp::CondBranch { .. }
        | FastOp::Branch { .. }
        | FastOp::PushPop { .. }
        | FastOp::BranchExchange { .. }
        | FastOp::BranchLink { .. }
        | FastOp::MovPc { .. }
        | FastOp::BlockXfer { .. } => true,
        FastOp::HiReg { op, .. } => op != 3,
        FastOp::AluOp { op, .. } => matches!(op, 0x0 | 0x1 | 0x2 | 0x3 | 0x4 | 0x7 | 0xC | 0xE | 0xF | 0x8 | 0xA | 0xB | 0x9 | 0xD),
    }
}

/// Materialize a 32-bit constant into `w<reg>` (movz + movk).
macro_rules! mov_imm32 {
    ($a:expr, $reg:tt, $v:expr) => {{
        let v: u32 = $v;
        a64!($a ; movz $reg, #(v & 0xffff) ; movk $reg, #((v >> 16) & 0xffff), lsl #16);
    }};
}

/// Emit a call to `addr` (movz/movk into x16, then blr).
fn emit_call(a: &mut Asm, addr: u64) {
    let a0 = (addr & 0xffff) as u32;
    let a1 = ((addr >> 16) & 0xffff) as u32;
    let a2 = ((addr >> 32) & 0xffff) as u32;
    let a3 = ((addr >> 48) & 0xffff) as u32;
    a64!(a
        ; movz x16, #a0
        ; movk x16, #a1, lsl #16
        ; movk x16, #a2, lsl #32
        ; movk x16, #a3, lsl #48
        ; blr x16
    );
}

pub(crate) fn compile_block(ops: &[FastOp], start_pc: u32) -> Option<(Code, usize)> {
    let op_pc = |i: usize| start_pc.wrapping_add(2 * i as u32);

    let mut limit = ops.len();
    for (i, op) in ops.iter().enumerate() {
        if !supported(*op) {
            limit = i;
            break;
        }
        if ends_trace(op) {
            limit = i + 1;
            break;
        }
    }
    if limit == 0 {
        return None;
    }
    let end_pc = op_pc(limit);
    let in_range = |t: u32| t >= start_pc && t < end_pc && (t.wrapping_sub(start_pc)) & 1 == 0;

    let mut a = Asm::new().ok()?;

    let mut labels: BTreeMap<u32, DynamicLabel> = BTreeMap::new();
    for op in &ops[..limit] {
        let t = match op {
            FastOp::CondBranch { target, .. } | FastOp::Branch { target } => *target,
            _ => continue,
        };
        if in_range(t) {
            labels.entry(t).or_insert_with(|| a.new_dynamic_label());
        }
    }

    let mut boundaries: BTreeSet<u32> = BTreeSet::new();
    boundaries.insert(start_pc);
    for &t in labels.keys() {
        boundaries.insert(t);
    }
    for (i, op) in ops[..limit].iter().enumerate() {
        if matches!(op, FastOp::CondBranch { .. } | FastOp::Branch { .. }) {
            let after = op_pc(i) + 2;
            if after < end_pc {
                boundaries.insert(after);
            }
        }
    }
    let seg_len = |b: u32| -> u32 {
        let next = boundaries.range((b + 1)..).next().copied().unwrap_or(end_pc);
        (next - b) / 2
    };

    let entry = a.offset();
    a64!(a
        ; stp x19, x30, [sp, #-16]!
        ; mov x19, x0
    );

    let mut ended_unconditional = false;
    for (i, op) in ops[..limit].iter().enumerate() {
        let pc = op_pc(i);
        if let Some(label) = labels.get(&pc) {
            a64!(a ; => *label);
        }
        if boundaries.contains(&pc) {
            let len = seg_len(pc);
            a64!(a
                ; ldr w9, [x19, #BUDGET]
                ; cmp w9, #0
                ; b.gt >cont
            );
            mov_imm32!(a, w10, pc);
            a64!(a
                ; str w10, [x19, #PC]
                ; movz w0, #exit::CONTINUE
                ; b ->epilogue
                ; cont:
                ; sub w9, w9, #len
                ; str w9, [x19, #BUDGET]
            );
        }
        match *op {
            FastOp::CondBranch { cond, target, next } => {
                let _ = next;
                let cond = cond as u32;
                a64!(a
                    ; movz w0, #cond
                    ; ldr w1, [x19, #CPSR]
                );
                emit_call(&mut a, jit_cond_met as *const () as u64);
                if in_range(target) {
                    a64!(a ; cbnz w0, => *labels.get(&target).unwrap());
                } else {
                    a64!(a ; cbz w0, >notaken);
                    mov_imm32!(a, w10, target);
                    a64!(a
                        ; str w10, [x19, #PC]
                        ; movz w0, #exit::CONTINUE
                        ; b ->epilogue
                        ; notaken:
                    );
                }
            }
            FastOp::Branch { target } => {
                if in_range(target) {
                    a64!(a ; b => *labels.get(&target).unwrap());
                } else {
                    mov_imm32!(a, w10, target);
                    a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::CONTINUE ; b ->epilogue);
                }
                ended_unconditional = true;
            }
            // Control-flow ops that write a dynamic/far PC end the trace.
            t @ (FastOp::BranchExchange { .. }
            | FastOp::BranchLink { .. }
            | FastOp::MovPc { .. }
            | FastOp::PushPop { load: true, extra: true, .. }) => {
                emit_op(&mut a, t, pc);
                ended_unconditional = true;
            }
            other => emit_op(&mut a, other, pc),
        }
    }

    if !ended_unconditional {
        mov_imm32!(a, w10, end_pc);
        a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::CONTINUE);
    }
    a64!(a
        ; ->epilogue:
        ; ldp x19, x30, [sp], #16
        ; ret
    );

    let buf = a.finalize().ok()?;
    Some((Code { buf, entry }, limit))
}

// ---------------------------------------------------------------------------
// ARM (A32) code generation — mirror of the x86-64 ARM path.
// ---------------------------------------------------------------------------

fn arm_supported(_op: &ArmOp) -> bool {
    true
}

pub(crate) fn compile_arm_block(ops: &[ArmOp], start_pc: u32) -> Option<(Code, usize)> {
    let mut limit = ops.len();
    for (i, op) in ops.iter().enumerate() {
        if !arm_supported(op) {
            limit = i;
            break;
        }
        if arm_ends_trace(op) {
            limit = i + 1;
            break;
        }
    }
    if limit == 0 {
        return None;
    }
    let end_pc = start_pc.wrapping_add(4 * limit as u32);
    let block_len = limit as u32;

    let mut a = Asm::new().ok()?;
    let entry = a.offset();
    a64!(a
        ; stp x19, x30, [sp, #-16]!
        ; mov x19, x0
        ; ldr w9, [x19, #BUDGET]
        ; cmp w9, #0
        ; b.gt >run
    );
    mov_imm32!(a, w10, start_pc);
    a64!(a
        ; str w10, [x19, #PC]
        ; movz w0, #exit::CONTINUE
        ; b ->epilogue
        ; run:
        ; sub w9, w9, #block_len
        ; str w9, [x19, #BUDGET]
    );

    let mut ended = false;
    for (i, op) in ops[..limit].iter().enumerate() {
        let pc = start_pc.wrapping_add(4 * i as u32);
        if arm_ends_trace(op) {
            emit_arm_terminator(&mut a, op, pc, end_pc);
            ended = true;
        } else {
            emit_arm(&mut a, op, pc);
        }
    }
    if !ended {
        mov_imm32!(a, w10, end_pc);
        a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::CONTINUE);
    }
    a64!(a ; ->epilogue: ; ldp x19, x30, [sp], #16 ; ret);

    let buf = a.finalize().ok()?;
    Some((Code { buf, entry }, limit))
}

/// Emit a guard that branches to `>skip` when `cond` is not met. Returns whether
/// a guard was emitted (AL needs none).
fn emit_arm_guard(a: &mut Asm, cond: u8) -> bool {
    // (bit, take_when_set)
    let single: Option<(u32, bool)> = match cond {
        0x0 => Some((30, true)),
        0x1 => Some((30, false)),
        0x2 => Some((29, true)),
        0x3 => Some((29, false)),
        0x4 => Some((31, true)),
        0x5 => Some((31, false)),
        0x6 => Some((28, true)),
        0x7 => Some((28, false)),
        _ => None,
    };
    match single {
        Some((bit, take_when_set)) => {
            a64!(a ; ldr w9, [x19, #CPSR]);
            if take_when_set {
                a64!(a ; tbz w9, #bit, >skip);
            } else {
                a64!(a ; tbnz w9, #bit, >skip);
            }
            true
        }
        None if cond >= 0xe => false,
        None => {
            a64!(a ; movz w0, #(cond as u32) ; ldr w1, [x19, #CPSR]);
            emit_call(a, jit_cond_met as *const () as u64);
            a64!(a ; cbz w0, >skip);
            true
        }
    }
}

/// operand2 -> value in `w10`, shifter carry in `w11`.
fn emit_arm_op2(a: &mut Asm, op2: Op2) {
    match op2 {
        Op2::Imm { val, carry } => {
            mov_imm32!(a, w10, val);
            a64!(a ; movz w11, #(carry & 0xffff));
        }
        Op2::ShiftImm { rm, ty, amount } => emit_arm_shift_call(a, rm, ty, amount as u32, false, None),
        Op2::ShiftReg { rm, ty, rs } => emit_arm_shift_call(a, rm, ty, 0, true, Some(rs)),
    }
}

fn emit_arm_shift_call(a: &mut Asm, rm: u8, ty: u8, amount_imm: u32, reg_shift: bool, rs: Option<u8>) {
    a64!(a ; ldr w0, [x19, #ro(rm)] ; movz w1, #(ty as u32));
    if let Some(rs) = rs {
        a64!(a ; ldr w2, [x19, #ro(rs)] ; and w2, w2, #0xff);
    } else {
        a64!(a ; movz w2, #amount_imm);
    }
    a64!(a
        ; movz w3, #(reg_shift as u32)
        ; ldr w4, [x19, #CPSR]
        ; ubfx w4, w4, #29, #1
    );
    emit_call(a, jit_arm_shift as *const () as u64);
    a64!(a ; mov w10, w0 ; lsr x11, x0, #32);
}

fn emit_arm(a: &mut Asm, op: &ArmOp, pc: u32) {
    match *op {
        ArmOp::DataProc {
            cond,
            opcode,
            s,
            rd,
            rn,
            op2,
        } => {
            let guarded = emit_arm_guard(a, cond);
            emit_arm_op2(a, op2);
            emit_arm_alu(a, opcode, s, rn);
            if !matches!(opcode, 0x8..=0xB) {
                a64!(a ; str w8, [x19, #ro(rd)]);
            }
            if guarded {
                a64!(a ; skip:);
            }
        }
        ArmOp::LoadStore { .. } => emit_arm_load_store(a, op, pc),
        ArmOp::Block { cond, .. } => {
            let guarded = emit_arm_guard(a, cond);
            emit_arm_block_body(a, op, pc);
            if guarded {
                a64!(a ; skip:);
            }
        }
        _ => unreachable!("terminator in straight slot"),
    }
}

/// operand2 in `w10`, shifter carry in `w11`; result -> `w8`, updates CPSR when
/// `s`.
fn emit_arm_alu(a: &mut Asm, opcode: u8, s: bool, rn: u8) {
    let logical = matches!(opcode, 0x0 | 0x1 | 0x8 | 0x9 | 0xC | 0xD | 0xE | 0xF);
    match opcode {
        0x0 | 0x8 => a64!(a ; ldr w9, [x19, #ro(rn)] ; and w0, w9, w10),
        0x1 | 0x9 => a64!(a ; ldr w9, [x19, #ro(rn)] ; eor w0, w9, w10),
        0xC => a64!(a ; ldr w9, [x19, #ro(rn)] ; orr w0, w9, w10),
        0xE => a64!(a ; ldr w9, [x19, #ro(rn)] ; bic w0, w9, w10),
        0xD => a64!(a ; mov w0, w10),
        0xF => a64!(a ; mvn w0, w10),
        0x2 | 0xA => a64!(a ; ldr w9, [x19, #ro(rn)] ; subs w0, w9, w10),
        0x3 => a64!(a ; ldr w9, [x19, #ro(rn)] ; subs w0, w10, w9),
        0x4 | 0xB => a64!(a ; ldr w9, [x19, #ro(rn)] ; adds w0, w9, w10),
        0x5 => a64!(a
            ; ldr w9, [x19, #ro(rn)]
            ; ldr w12, [x19, #CPSR] ; ubfx w12, w12, #29, #1 ; cmp w12, #1 // C flag = ARM C
            ; adcs w0, w9, w10),
        0x6 => a64!(a
            ; ldr w9, [x19, #ro(rn)]
            ; ldr w12, [x19, #CPSR] ; ubfx w12, w12, #29, #1 ; cmp w12, #1
            ; sbcs w0, w9, w10),
        0x7 => a64!(a
            ; ldr w9, [x19, #ro(rn)]
            ; ldr w12, [x19, #CPSR] ; ubfx w12, w12, #29, #1 ; cmp w12, #1
            ; sbcs w0, w10, w9),
        _ => unreachable!(),
    }
    if logical {
        a64!(a ; mov w8, w0);
        if s {
            a64!(a ; mov w1, w11);
            emit_flags_nzc(a);
        }
    } else if s {
        emit_flags_nzcv(a);
    } else {
        a64!(a ; mov w8, w0);
    }
}

fn emit_arm_load_store(a: &mut Asm, op: &ArmOp, pc: u32) {
    let ArmOp::LoadStore {
        cond,
        load,
        byte,
        rd,
        rn,
        pre,
        up,
        wb,
        base_pc,
        offset,
    } = *op
    else {
        unreachable!()
    };
    let guarded = emit_arm_guard(a, cond);

    // offset -> w2
    match offset {
        Off::Imm(v) => mov_imm32!(a, w2, v),
        Off::ShiftImm { rm, ty, amount } => {
            emit_arm_shift_call(a, rm, ty, amount as u32, false, None);
            a64!(a ; mov w2, w10);
        }
    }
    // base -> w9
    match base_pc {
        Some(v) => mov_imm32!(a, w9, v),
        None => a64!(a ; ldr w9, [x19, #ro(rn)]),
    }
    // post -> w10 ; addr -> w1
    if up {
        a64!(a ; add w10, w9, w2);
    } else {
        a64!(a ; sub w10, w9, w2);
    }
    if pre {
        a64!(a ; mov w1, w10);
    } else {
        a64!(a ; mov w1, w9);
    }
    let do_wb = (!pre || wb) && base_pc.is_none() && (!load || rd != rn);

    if load {
        if do_wb {
            a64!(a ; str w10, [x19, #ro(rn)]);
        }
        a64!(a ; mov x0, x19);
        emit_call(a, if byte { jit_load8 } else { jit_load32 } as *const () as u64);
        a64!(a
            ; str w0, [x19, #ro(rd)]
            ; ldr w9, [x19, #FAULTED]
            ; cbz w9, >nofault
        );
        mov_imm32!(a, w12, pc.wrapping_add(4));
        a64!(a ; str w12, [x19, #PC] ; movz w0, #exit::FAULT ; b ->epilogue ; nofault:);
    } else {
        a64!(a ; ldr w2, [x19, #ro(rd)]); // capture value before writeback
        if do_wb {
            a64!(a ; str w10, [x19, #ro(rn)]);
        }
        a64!(a ; mov x0, x19);
        emit_call(a, if byte { jit_store8 } else { jit_store32 } as *const () as u64);
        a64!(a ; ldr w9, [x19, #FAULTED] ; cbz w9, >nofault);
        mov_imm32!(a, w12, pc.wrapping_add(4));
        a64!(a ; str w12, [x19, #PC] ; movz w0, #exit::FAULT ; b ->epilogue ; nofault:);
        a64!(a ; ldr w9, [x19, #SMC] ; cbz w9, >nosmc);
        mov_imm32!(a, w12, pc.wrapping_add(4));
        a64!(a ; str w12, [x19, #PC] ; movz w0, #exit::SMC ; b ->epilogue ; nosmc:);
    }
    if guarded {
        a64!(a ; skip:);
    }
}

fn emit_arm_block_body(a: &mut Asm, op: &ArmOp, pc: u32) {
    let ArmOp::Block {
        load,
        rn,
        pre,
        up,
        wb,
        reglist,
        ..
    } = *op
    else {
        unreachable!()
    };
    let regs: alloc::vec::Vec<u8> = (0..16u8).filter(|&r| reglist & (1 << r) != 0).collect();
    let total = regs.len() as i32;
    a64!(a ; ldr w0, [x19, #ro(rn)] ; str w0, [x19, #SCRATCH]);
    if wb {
        let delta = if up { total * 4 } else { -total * 4 };
        a64!(a ; ldr w0, [x19, #SCRATCH]);
        if delta >= 0 {
            a64!(a ; add w0, w0, #(delta as u32));
        } else {
            a64!(a ; sub w0, w0, #((-delta) as u32));
        }
        a64!(a ; str w0, [x19, #ro(rn)]);
    }
    let addr_base = if up { 0 } else { -total * 4 };
    let pre_incr = (pre == up) as i32;
    for (i, &r) in regs.iter().enumerate() {
        let off = addr_base + (i as i32 + pre_incr) * 4;
        a64!(a ; mov x0, x19 ; ldr w1, [x19, #SCRATCH]);
        if off >= 0 {
            a64!(a ; add w1, w1, #(off as u32));
        } else {
            a64!(a ; sub w1, w1, #((-off) as u32));
        }
        if load {
            emit_call(a, jit_load32 as *const () as u64);
            a64!(a ; str w0, [x19, #ro(r)]);
        } else {
            if r == 15 {
                mov_imm32!(a, w2, pc.wrapping_add(12));
            } else if r == rn && wb && i == 0 {
                a64!(a ; ldr w2, [x19, #SCRATCH]);
            } else {
                a64!(a ; ldr w2, [x19, #ro(r)]);
            }
            emit_call(a, jit_store32 as *const () as u64);
        }
    }
    a64!(a ; ldr w9, [x19, #FAULTED] ; cbz w9, >nofault);
    mov_imm32!(a, w12, pc.wrapping_add(4));
    a64!(a ; str w12, [x19, #PC] ; movz w0, #exit::FAULT ; b ->epilogue ; nofault:);
    if !load {
        a64!(a ; ldr w9, [x19, #SMC] ; cbz w9, >nosmc);
        mov_imm32!(a, w12, pc.wrapping_add(4));
        a64!(a ; str w12, [x19, #PC] ; movz w0, #exit::SMC ; b ->epilogue ; nosmc:);
    }
}

fn emit_arm_terminator(a: &mut Asm, op: &ArmOp, pc: u32, end_pc: u32) {
    let cond = match op {
        ArmOp::Branch { cond, .. } | ArmOp::BranchEx { cond, .. } | ArmOp::DataProc { cond, .. } | ArmOp::Block { cond, .. } => *cond,
        _ => 0xe,
    };
    let guarded = emit_arm_guard(a, cond);

    match *op {
        ArmOp::Branch {
            target, link, ret, to_thumb, ..
        } => {
            if link {
                mov_imm32!(a, w10, ret);
                a64!(a ; str w10, [x19, #ro(14)]);
            }
            if to_thumb {
                a64!(a ; ldr w10, [x19, #CPSR] ; orr w10, w10, #0x20 ; str w10, [x19, #CPSR]);
            }
            mov_imm32!(a, w10, target);
            a64!(a ; str w10, [x19, #PC]);
        }
        ArmOp::BranchEx { rm, .. } => {
            if rm == 15 {
                mov_imm32!(a, w0, pc.wrapping_add(8));
            } else {
                a64!(a ; ldr w0, [x19, #ro(rm)]);
            }
            a64!(a ; and w9, w0, #1);
            mov_imm32!(a, w10, 0xffff_fffc);
            a64!(a
                ; orr w10, w10, w9, lsl #1
                ; and w0, w0, w10
                ; str w0, [x19, #PC]
                ; ldr w10, [x19, #CPSR]
                ; movz w11, #0x20
                ; bic w10, w10, w11
                ; orr w10, w10, w9, lsl #5
                ; str w10, [x19, #CPSR]
            );
        }
        ArmOp::DataProc { opcode, rn, op2, .. } => {
            emit_arm_op2(a, op2);
            emit_arm_alu(a, opcode, false, rn);
            a64!(a ; str w8, [x19, #PC]);
        }
        ArmOp::Block { .. } => {
            emit_arm_block_body(a, op, pc);
        }
        _ => unreachable!(),
    }
    a64!(a ; movz w0, #exit::CONTINUE ; b ->epilogue);
    if guarded {
        a64!(a ; skip:);
        mov_imm32!(a, w10, end_pc);
        a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::CONTINUE ; b ->epilogue);
    }
}

fn emit_op(a: &mut Asm, op: FastOp, pc: u32) {
    match op {
        FastOp::AddSubImm { sub, rd, rs, imm } => {
            a64!(a ; ldr w0, [x19, #ro(rs)]);
            if sub {
                a64!(a ; subs w0, w0, #imm);
            } else {
                a64!(a ; adds w0, w0, #imm);
            }
            emit_flags_nzcv(a);
            a64!(a ; str w8, [x19, #ro(rd)]);
        }
        FastOp::AddSubReg { sub, rd, rs, rn } => {
            a64!(a ; ldr w0, [x19, #ro(rs)] ; ldr w1, [x19, #ro(rn)]);
            if sub {
                a64!(a ; subs w0, w0, w1);
            } else {
                a64!(a ; adds w0, w0, w1);
            }
            emit_flags_nzcv(a);
            a64!(a ; str w8, [x19, #ro(rd)]);
        }
        FastOp::ImmOp { op, rd, imm } => match op {
            0 => {
                mov_imm32!(a, w8, imm);
                a64!(a ; str w8, [x19, #ro(rd)]);
                emit_flags_nz(a);
            }
            1 => {
                a64!(a ; ldr w0, [x19, #ro(rd)]);
                mov_imm32!(a, w1, imm);
                a64!(a ; subs w0, w0, w1);
                emit_flags_nzcv(a);
            }
            2 => {
                a64!(a ; ldr w0, [x19, #ro(rd)]);
                mov_imm32!(a, w1, imm);
                a64!(a ; adds w0, w0, w1);
                emit_flags_nzcv(a);
                a64!(a ; str w8, [x19, #ro(rd)]);
            }
            3 => {
                a64!(a ; ldr w0, [x19, #ro(rd)]);
                mov_imm32!(a, w1, imm);
                a64!(a ; subs w0, w0, w1);
                emit_flags_nzcv(a);
                a64!(a ; str w8, [x19, #ro(rd)]);
            }
            _ => unreachable!(),
        },
        FastOp::AluOp { op, rd, rs } => emit_alu(a, op, rd, rs),
        FastOp::HiReg { op, crd, crs } => {
            // Load crs value (PC pipeline offset folded in as a constant) into w0.
            if crs == 15 {
                mov_imm32!(a, w0, pc.wrapping_add(4));
            } else {
                a64!(a ; ldr w0, [x19, #ro(crs)]);
            }
            match op {
                0 => {
                    a64!(a ; ldr w1, [x19, #ro(crd)] ; add w1, w1, w0 ; str w1, [x19, #ro(crd)]);
                }
                2 => {
                    a64!(a ; str w0, [x19, #ro(crd)]);
                }
                1 => {
                    a64!(a ; ldr w1, [x19, #ro(crd)] ; subs w0, w1, w0);
                    emit_flags_nzcv(a);
                }
                _ => unreachable!(),
            }
        }
        FastOp::PcLoad { rd, offset } => {
            let addr = pc.wrapping_add(4).wrapping_add(offset * 4) & !3;
            emit_load(a, 32, false, rd, |a| mov_imm32!(a, w1, addr), pc);
        }
        FastOp::LoadAddr { sp, rd, imm } => {
            if sp {
                let imm4 = imm * 4;
                a64!(a ; ldr w0, [x19, #ro(13)] ; add w0, w0, #imm4 ; str w0, [x19, #ro(rd)]);
            } else {
                let v = (pc.wrapping_add(4) & !2).wrapping_add(imm * 4);
                mov_imm32!(a, w0, v);
                a64!(a ; str w0, [x19, #ro(rd)]);
            }
        }
        FastOp::SpAdd { sub, imm } => {
            a64!(a ; ldr w0, [x19, #ro(13)]);
            if sub {
                a64!(a ; sub w0, w0, #imm);
            } else {
                a64!(a ; add w0, w0, #imm);
            }
            a64!(a ; str w0, [x19, #ro(13)]);
        }
        FastOp::HwXferI { load, rb, rd, offset } => {
            let off = offset * 2;
            let addr = move |a: &mut Asm| a64!(a ; ldr w1, [x19, #ro(rb)] ; add w1, w1, #off ; and w1, w1, #0xffff_fffe);
            if load {
                emit_load(a, 16, false, rd, addr, pc);
            } else {
                emit_store(a, 16, rd, addr, pc);
            }
        }
        FastOp::SingleXferI { load, byte, rb, rd, offset } => {
            let scaled = if byte { offset } else { offset * 4 };
            let addr = move |a: &mut Asm| a64!(a ; ldr w1, [x19, #ro(rb)] ; add w1, w1, #scaled);
            let size = if byte { 8 } else { 32 };
            if load {
                emit_load(a, size, false, rd, addr, pc);
            } else {
                emit_store(a, size, rd, addr, pc);
            }
        }
        FastOp::SpXfer { load, rd, offset } => {
            let addr = move |a: &mut Asm| a64!(a ; ldr w1, [x19, #ro(13)] ; add w1, w1, #offset);
            if load {
                emit_load(a, 32, false, rd, addr, pc);
            } else {
                emit_store(a, 32, rd, addr, pc);
            }
        }
        FastOp::SingleXferR {
            load,
            byte,
            ro: roff,
            rb,
            rd,
        } => {
            let addr = move |a: &mut Asm| a64!(a ; ldr w1, [x19, #ro(rb)] ; ldr w9, [x19, #ro(roff)] ; add w1, w1, w9);
            let size = if byte { 8 } else { 32 };
            if load {
                emit_load(a, size, false, rd, addr, pc);
            } else {
                emit_store(a, size, rd, addr, pc);
            }
        }
        FastOp::Shift { op, rd, rs, shift } => emit_shift(a, op, rd, rs, shift),
        FastOp::HwSgnXfer { s, h, ro: roff, rb, rd } => {
            let hw_addr = move |a: &mut Asm| a64!(a ; ldr w1, [x19, #ro(rb)] ; ldr w9, [x19, #ro(roff)] ; add w1, w1, w9 ; and w1, w1, #0xffff_fffe);
            let byte_addr = move |a: &mut Asm| a64!(a ; ldr w1, [x19, #ro(rb)] ; ldr w9, [x19, #ro(roff)] ; add w1, w1, w9);
            match (s, h) {
                (false, false) => emit_store(a, 16, rd, hw_addr, pc),      // strh
                (false, true) => emit_load(a, 16, false, rd, hw_addr, pc), // ldrh
                (true, false) => emit_load(a, 8, true, rd, byte_addr, pc), // ldrsb
                (true, true) => emit_load(a, 16, true, rd, hw_addr, pc),   // ldrsh
            }
        }
        FastOp::PushPop { load, extra, rlist } => emit_push_pop(a, load, extra, rlist, pc),
        FastOp::BlockXfer { load, rb, rlist } => emit_block_xfer(a, load, rb, rlist, pc),
        FastOp::BranchExchange { link, rm } => emit_bx(a, link, rm, pc),
        FastOp::MovPc { rm } => emit_mov_pc(a, rm),
        FastOp::BranchLink { exchange, target, ret } => emit_bl(a, exchange, target, ret),
        FastOp::CondBranch { .. } | FastOp::Branch { .. } => {
            unreachable!("handled by compile_block")
        }
    }
}

/// Emit `push`/`pop`, fully unrolled from the compile-time register list. Every
/// access runs (each helper records a fault but continues, matching the
/// interpreter completing the instruction) and SP is updated unconditionally;
/// the fault/SMC check is deferred to the end. A `pop {..,pc}` writes a dynamic
/// PC and ends the trace; the other forms fall through to the next op.
fn emit_push_pop(a: &mut Asm, load: bool, extra: bool, rlist: u8, pc: u32) {
    let mut regs: alloc::vec::Vec<u8> = (0..8u8).filter(|&r| rlist & (1 << r) != 0).collect();
    if extra {
        regs.push(if load { 15 } else { 14 });
    }
    let total = regs.len() as u32;
    let total4 = total * 4;
    let pop_pc = load && extra;

    if load {
        // POP: addr = SP, load ascending, then SP += total*4.
        for (i, &r) in regs.iter().enumerate() {
            let off = i as u32 * 4;
            a64!(a ; mov x0, x19 ; ldr w1, [x19, #ro(13)] ; add w1, w1, #off);
            emit_call(a, jit_load32 as *const () as u64);
            if r == 15 {
                // POP {..,pc}: T from bit 0, PC = val & !1.
                a64!(a
                    ; mov w8, w0
                    ; and w9, w8, #1
                    ; ldr w10, [x19, #CPSR]
                    ; movz w11, #0x20
                    ; bic w10, w10, w11
                    ; orr w10, w10, w9, lsl #5
                    ; str w10, [x19, #CPSR]
                    ; and w8, w8, #0xffff_fffe
                    ; str w8, [x19, #PC]
                );
            } else {
                a64!(a ; str w0, [x19, #ro(r)]);
            }
        }
        a64!(a ; ldr w9, [x19, #ro(13)] ; add w9, w9, #total4 ; str w9, [x19, #ro(13)]);
    } else {
        // PUSH: addr = SP - total*4, store ascending, then SP = addr.
        for (i, &r) in regs.iter().enumerate() {
            let off = total4 - i as u32 * 4; // amount subtracted from SP
            a64!(a ; mov x0, x19 ; ldr w1, [x19, #ro(13)] ; sub w1, w1, #off ; ldr w2, [x19, #ro(r)]);
            emit_call(a, jit_store32 as *const () as u64);
        }
        a64!(a ; ldr w9, [x19, #ro(13)] ; sub w9, w9, #total4 ; str w9, [x19, #ro(13)]);
    }

    // Deferred fault check. A pop-with-pc has already written the popped PC.
    a64!(a ; ldr w9, [x19, #FAULTED] ; cbz w9, >nofault);
    if !pop_pc {
        mov_imm32!(a, w10, pc.wrapping_add(2));
        a64!(a ; str w10, [x19, #PC]);
    }
    a64!(a ; movz w0, #exit::FAULT ; b ->epilogue ; nofault:);
    if !load {
        a64!(a ; ldr w9, [x19, #SMC] ; cbz w9, >nosmc);
        mov_imm32!(a, w10, pc.wrapping_add(2));
        a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::SMC ; b ->epilogue ; nosmc:);
    }
    if pop_pc {
        a64!(a ; movz w0, #exit::CONTINUE ; b ->epilogue);
    }
}

/// Emit `ldmia`/`stmia rb!, {rlist}`, fully unrolled. The base is stashed in the
/// context scratch word so it survives helper calls even when `rb` is in the list
/// and gets overwritten. Writeback happens first (matching the interpreter), so
/// an LDM reloading `rb` overrides it and an STM of `rb` above the lowest slot
/// stores the written-back value. Fault/SMC checked once at the end.
fn emit_block_xfer(a: &mut Asm, load: bool, rb: u8, rlist: u8, pc: u32) {
    let regs: alloc::vec::Vec<u8> = (0..8u8).filter(|&r| rlist & (1 << r) != 0).collect();
    let total4 = regs.len() as u32 * 4;
    a64!(a
        ; ldr w0, [x19, #ro(rb)]
        ; str w0, [x19, #SCRATCH]      // scratch = base
        ; add w0, w0, #total4
        ; str w0, [x19, #ro(rb)]        // writeback rb = base + total*4
    );
    for (i, &r) in regs.iter().enumerate() {
        let off = i as u32 * 4;
        if load {
            a64!(a ; mov x0, x19 ; ldr w1, [x19, #SCRATCH] ; add w1, w1, #off);
            emit_call(a, jit_load32 as *const () as u64);
            a64!(a ; str w0, [x19, #ro(r)]);
        } else {
            a64!(a ; mov x0, x19 ; ldr w1, [x19, #SCRATCH] ; add w1, w1, #off);
            if r == rb && i == 0 {
                a64!(a ; ldr w2, [x19, #SCRATCH]); // lowest slot stores the original base
            } else {
                a64!(a ; ldr w2, [x19, #ro(r)]);
            }
            emit_call(a, jit_store32 as *const () as u64);
        }
    }
    a64!(a ; ldr w9, [x19, #FAULTED] ; cbz w9, >nofault);
    mov_imm32!(a, w10, pc.wrapping_add(2));
    a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::FAULT ; b ->epilogue ; nofault:);
    if !load {
        a64!(a ; ldr w9, [x19, #SMC] ; cbz w9, >nosmc);
        mov_imm32!(a, w10, pc.wrapping_add(2));
        a64!(a ; str w10, [x19, #PC] ; movz w0, #exit::SMC ; b ->epilogue ; nosmc:);
    }
}

/// Emit `bx`/`blx` register: read rm (PC folded to a constant), switch ARM/Thumb
/// from bit 0, optionally set LR, and end the trace.
fn emit_bx(a: &mut Asm, link: bool, rm: u8, pc: u32) {
    if rm == 15 {
        mov_imm32!(a, w0, pc.wrapping_add(4));
    } else {
        a64!(a ; ldr w0, [x19, #ro(rm)]);
    }
    a64!(a ; and w9, w0, #1); // new T bit
    // PC = vals & (new_t ? !1 : !3) = vals & (0xFFFFFFFC | new_t<<1)
    mov_imm32!(a, w10, 0xffff_fffc);
    a64!(a
        ; orr w10, w10, w9, lsl #1
        ; and w0, w0, w10
        ; str w0, [x19, #PC]
        ; ldr w10, [x19, #CPSR]
        ; movz w11, #0x20
        ; bic w10, w10, w11
        ; orr w10, w10, w9, lsl #5
        ; str w10, [x19, #CPSR]
    );
    if link {
        mov_imm32!(a, w10, pc.wrapping_add(2) | 1);
        a64!(a ; str w10, [x19, #ro(14)]);
    }
    a64!(a ; movz w0, #exit::CONTINUE ; b ->epilogue);
}

/// Emit `mov pc, rm` (`rm != PC`). Unlike `bx`, this does not interwork: the
/// target is `rm & !1` and the Thumb state is unchanged, so no CPSR write.
fn emit_mov_pc(a: &mut Asm, rm: u8) {
    a64!(a
        ; ldr w0, [x19, #ro(rm)]
        ; movz w9, #1
        ; bic w0, w0, w9
        ; str w0, [x19, #PC]
        ; movz w0, #exit::CONTINUE
        ; b ->epilogue
    );
}

/// Emit `bl`/`blx` immediate: set LR to the return address and PC to the
/// pre-computed target; BLX also clears the Thumb bit. Ends the trace.
fn emit_bl(a: &mut Asm, exchange: bool, target: u32, ret: u32) {
    mov_imm32!(a, w10, ret);
    a64!(a ; str w10, [x19, #ro(14)]);
    mov_imm32!(a, w10, target);
    a64!(a ; str w10, [x19, #PC]);
    if exchange {
        a64!(a
            ; ldr w10, [x19, #CPSR]
            ; movz w11, #0x20
            ; bic w10, w10, w11
            ; str w10, [x19, #CPSR]
        );
    }
    a64!(a ; movz w0, #exit::CONTINUE ; b ->epilogue);
}

/// Thumb `Shifted` (LSL/LSR/ASR by a 5-bit immediate). Updates N, Z and C,
/// preserving V; the shift amount is a compile-time constant.
fn emit_shift(a: &mut Asm, op: u8, rd: u8, rs: u8, shift: u32) {
    if shift == 0 {
        match op {
            0 => {
                // LSL #0: unchanged, C preserved -> only N,Z change.
                a64!(a ; ldr w8, [x19, #ro(rs)] ; str w8, [x19, #ro(rd)]);
                emit_flags_nz(a);
                return;
            }
            1 => {
                // LSR #32: result 0, C = bit 31.
                a64!(a ; ldr w2, [x19, #ro(rs)] ; ubfx w1, w2, #31, #1 ; mov w0, wzr);
            }
            2 => {
                // ASR #32: sign-extend, C = bit 31.
                a64!(a ; ldr w2, [x19, #ro(rs)] ; ubfx w1, w2, #31, #1 ; asr w0, w2, #31);
            }
            _ => unreachable!(),
        }
    } else {
        a64!(a ; ldr w2, [x19, #ro(rs)]);
        match op {
            0 => {
                let lsb = 32 - shift;
                a64!(a ; lsl w0, w2, #shift ; ubfx w1, w2, #lsb, #1);
            }
            1 => {
                let lsb = shift - 1;
                a64!(a ; lsr w0, w2, #shift ; ubfx w1, w2, #lsb, #1);
            }
            2 => {
                let lsb = shift - 1;
                a64!(a ; asr w0, w2, #shift ; ubfx w1, w2, #lsb, #1);
            }
            _ => unreachable!(),
        }
    }
    // res in w0, carry (0/1) in w1.
    a64!(a ; str w0, [x19, #ro(rd)]);
    emit_flags_nzc(a);
}

/// Pack N, Z (from the result in `w0`) and C (0/1 in `w1`) into CPSR, preserving
/// V. The result must already be stored.
fn emit_flags_nzc(a: &mut Asm) {
    a64!(a
        ; tst w0, w0
        ; cset w9, eq
        ; cset w10, mi
        ; ldr w13, [x19, #CPSR]
        ; and w13, w13, #0x1fff_ffff
        ; orr w13, w13, w10, lsl #31
        ; orr w13, w13, w9, lsl #30
        ; orr w13, w13, w1, lsl #29
        ; str w13, [x19, #CPSR]
    );
}

fn emit_alu(a: &mut Asm, op: u8, rd: u8, rs: u8) {
    match op {
        0x0 => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; and w8, w0, w1 ; str w8, [x19, #ro(rd)]);
            emit_flags_nz(a);
        }
        0x1 => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; eor w8, w0, w1 ; str w8, [x19, #ro(rd)]);
            emit_flags_nz(a);
        }
        0xC => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; orr w8, w0, w1 ; str w8, [x19, #ro(rd)]);
            emit_flags_nz(a);
        }
        0xE => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; bic w8, w0, w1 ; str w8, [x19, #ro(rd)]);
            emit_flags_nz(a);
        }
        0xF => {
            a64!(a ; ldr w1, [x19, #ro(rs)] ; mvn w8, w1 ; str w8, [x19, #ro(rd)]);
            emit_flags_nz(a);
        }
        0x8 => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; and w8, w0, w1);
            emit_flags_nz(a);
        }
        0xA => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; subs w0, w0, w1);
            emit_flags_nzcv(a);
        }
        0xB => {
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w1, [x19, #ro(rs)] ; adds w0, w0, w1);
            emit_flags_nzcv(a);
        }
        0x9 => {
            a64!(a ; ldr w1, [x19, #ro(rs)] ; subs w0, wzr, w1);
            emit_flags_nzcv(a);
            a64!(a ; str w8, [x19, #ro(rd)]);
        }
        0xD => {
            // MUL: rd * rs (low 32 bits); N,Z from result, C forced to 0, V kept.
            a64!(a ; ldr w0, [x19, #ro(rd)] ; ldr w2, [x19, #ro(rs)] ; mul w0, w0, w2 ; str w0, [x19, #ro(rd)] ; mov w1, wzr);
            emit_flags_nzc(a);
        }
        0x2 | 0x3 | 0x4 | 0x7 => {
            // Shift-by-register (LSL/LSR/ASR/ROR): rd shifted by rs & 0xff. N,Z
            // from the result, C from the shifted-out bit, V preserved. The
            // interpreter's exact shift semantics are reused via `jit_alu_shift`.
            let shift_type = (((op >> 1) & 2) | (op & 1)) as u32;
            a64!(a
                ; ldr w0, [x19, #ro(rd)]       // val
                ; ldr w1, [x19, #ro(rs)]       // amount
                ; movz w2, #shift_type          // shift type
                ; ldr w3, [x19, #CPSR]
                ; ubfx w3, w3, #29, #1          // carry-in
            );
            emit_call(a, jit_alu_shift as *const () as u64);
            a64!(a
                ; lsr x1, x0, #32              // carry (0/1)
                ; str w0, [x19, #ro(rd)]       // res (low 32)
            );
            emit_flags_nzc(a);
        }
        _ => unreachable!(),
    }
}

/// Flags set by a preceding `adds`/`subs` with the result in `w0`: save it to
/// `w8` and pack ARM NZCV into CPSR. AArch64's C flag matches ARM's for both add
/// and subtract, so no add/sub distinction is needed.
fn emit_flags_nzcv(a: &mut Asm) {
    a64!(a
        ; mov w8, w0
        ; cset w9, eq          // Z
        ; cset w10, mi         // N
        ; cset w11, cs         // C
        ; cset w12, vs         // V
        ; ldr w13, [x19, #CPSR]
        ; and w13, w13, #0x0fff_ffff
        ; orr w13, w13, w10, lsl #31
        ; orr w13, w13, w9, lsl #30
        ; orr w13, w13, w11, lsl #29
        ; orr w13, w13, w12, lsl #28
        ; str w13, [x19, #CPSR]
    );
}

/// Update only N and Z from the result in `w8`, preserving C and V.
fn emit_flags_nz(a: &mut Asm) {
    a64!(a
        ; tst w8, w8
        ; cset w9, eq
        ; cset w10, mi
        ; ldr w13, [x19, #CPSR]
        ; and w13, w13, #0x3fff_ffff
        ; orr w13, w13, w10, lsl #31
        ; orr w13, w13, w9, lsl #30
        ; str w13, [x19, #CPSR]
    );
}

fn emit_load(a: &mut Asm, size: u8, signed: bool, rd: u8, addr: impl FnOnce(&mut Asm), pc: u32) {
    let helper = match size {
        8 => jit_load8 as *const () as u64,
        16 => jit_load16 as *const () as u64,
        _ => jit_load32 as *const () as u64,
    };
    a64!(a ; mov x0, x19);
    addr(a); // sets w1 = effective address
    // Inline fast path: a mapped, naturally aligned access reads the guest page
    // directly (host is little-endian, like the guest), skipping the helper call.
    // A null page (unmapped) or a misaligned access takes the slow path, whose
    // helper reproduces the fault / unaligned-rotate semantics exactly.
    a64!(a
        ; lsr w9, w1, #16          // 64 KiB page index
        ; lsl w9, w9, #3           // *8 bytes per page pointer
        ; ldr x10, [x19, #PAGES]
        ; ldr x10, [x10, w9, uxtw] // page base (null = unmapped)
        ; cbz x10, >slow
    );
    match size {
        16 => a64!(a ; tst w1, #1 ; b.ne >slow),
        32 => a64!(a ; tst w1, #3 ; b.ne >slow),
        _ => {}
    }
    a64!(a ; and w9, w1, #0xffff); // in-page offset
    match (size, signed) {
        (8, true) => a64!(a ; ldrsb w8, [x10, w9, uxtw]),
        (8, false) => a64!(a ; ldrb w8, [x10, w9, uxtw]),
        (16, true) => a64!(a ; ldrsh w8, [x10, w9, uxtw]),
        (16, false) => a64!(a ; ldrh w8, [x10, w9, uxtw]),
        _ => a64!(a ; ldr w8, [x10, w9, uxtw]),
    }
    a64!(a ; b >done ; slow:);
    emit_call(a, helper);
    a64!(a ; mov w8, w0);
    if signed {
        match size {
            8 => a64!(a ; sxtb w8, w8),
            16 => a64!(a ; sxth w8, w8),
            _ => {}
        }
    }
    a64!(a ; done:);
    // Write the destination before the fault check (interpreter parity). The
    // fast path never faults (page mapped), so FAULTED stays 0 and it skips out.
    a64!(a
        ; str w8, [x19, #ro(rd)]
        ; ldr w9, [x19, #FAULTED]
        ; cbz w9, >nofault
    );
    mov_imm32!(a, w10, pc.wrapping_add(2));
    a64!(a
        ; str w10, [x19, #PC]
        ; movz w0, #exit::FAULT
        ; b ->epilogue
        ; nofault:
    );
}

fn emit_store(a: &mut Asm, size: u8, rd: u8, addr: impl FnOnce(&mut Asm), pc: u32) {
    let helper = match size {
        8 => jit_store8 as *const () as u64,
        16 => jit_store16 as *const () as u64,
        _ => jit_store32 as *const () as u64,
    };
    a64!(a ; mov x0, x19);
    addr(a);
    a64!(a ; ldr w2, [x19, #ro(rd)]);
    emit_call(a, helper);
    a64!(a ; ldr w9, [x19, #FAULTED] ; cbz w9, >nofault);
    mov_imm32!(a, w10, pc.wrapping_add(2));
    a64!(a
        ; str w10, [x19, #PC]
        ; movz w0, #exit::FAULT
        ; b ->epilogue
        ; nofault:
        ; ldr w9, [x19, #SMC]
        ; cbz w9, >nosmc
    );
    mov_imm32!(a, w10, pc.wrapping_add(2));
    a64!(a
        ; str w10, [x19, #PC]
        ; movz w0, #exit::SMC
        ; b ->epilogue
        ; nosmc:
    );
}
