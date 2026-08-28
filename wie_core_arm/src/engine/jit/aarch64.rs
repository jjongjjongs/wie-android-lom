//! AArch64 code generator for the JIT.
//!
//! A faithful mirror of the x86-64 backend (`x64.rs`): same trace/linking/budget
//! structure and the same instruction *selection* (via the shared `decode`
//! frontend), only the *encoding* differs. It cannot be executed on the x86-64
//! CI host, so it is deliberately kept simple — every conditional branch calls
//! the interpreter's `cond_met` rather than inlining flag tests — and its
//! correctness is validated by running the same differential tests
//! (`engine::jit::tests`) on AArch64 hardware.
//!
//! Convention (AAPCS64): `x0` holds the context pointer on entry, saved into the
//! callee-saved `x19` for the block; guest register `i` lives at `[x19, #i*4]`,
//! CPSR at `[x19, #64]`. The function returns an `exit::*` code in `w0` and
//! leaves `regs[15]` (`[x19, #60]`) at the next guest PC.

use alloc::collections::{BTreeMap, BTreeSet};

use dynasmrt::{AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer, dynasm};

use crate::engine::fast::FastOp;

use super::{JitCtx, exit, jit_cond_met, jit_load8, jit_load16, jit_load32, jit_store8, jit_store16, jit_store32};

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
        | FastOp::CondBranch { .. }
        | FastOp::Branch { .. } => true,
        FastOp::HiReg { op, .. } => op != 3,
        FastOp::AluOp { op, .. } => matches!(op, 0x0 | 0x1 | 0xC | 0xE | 0xF | 0x8 | 0xA | 0xB | 0x9),
        FastOp::Shift { .. } | FastOp::HwSgnXfer { .. } => false,
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
        if matches!(op, FastOp::Branch { .. }) {
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
        FastOp::CondBranch { .. } | FastOp::Branch { .. } | FastOp::Shift { .. } | FastOp::HwSgnXfer { .. } => {
            unreachable!("handled by compile_block or unsupported")
        }
    }
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
    addr(a);
    emit_call(a, helper);
    a64!(a ; mov w8, w0);
    if signed {
        match size {
            8 => a64!(a ; sxtb w8, w8),
            16 => a64!(a ; sxth w8, w8),
            _ => {}
        }
    }
    // Write the destination before the fault check (interpreter parity).
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
