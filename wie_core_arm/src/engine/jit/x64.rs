//! x86-64 code generator for the JIT.
//!
//! Each Thumb basic block is compiled to a `extern "C" fn(*mut JitCtx) -> u32`
//! following the SysV AMD64 convention: `rdi` holds the context pointer on
//! entry, moved into the callee-saved `rbx` for the block's lifetime; guest
//! register `i` lives at `[rbx + i*4]`, CPSR at `[rbx + 64]`. The function
//! returns an `exit::*` code and always leaves `regs[15]` (`[rbx + 60]`) at the
//! next guest PC. Guest state lives entirely in the context array (not host
//! registers), so helper calls need only preserve `rbx`.

use alloc::collections::{BTreeMap, BTreeSet};

use dynasmrt::{AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer, dynasm};

use crate::engine::fast::FastOp;

use super::{JitCtx, exit, jit_cond_met, jit_load8, jit_load16, jit_load32, jit_store8, jit_store16, jit_store32};

/// A compiled block owning its executable buffer (stable pointers; a flush drops
/// it) plus the entry offset.
pub(crate) struct Code {
    buf: ExecutableBuffer,
    entry: AssemblyOffset,
}

pub(crate) fn run_block(code: &Code, ctx: &mut JitCtx) -> u32 {
    let f: extern "C" fn(*mut JitCtx) -> u32 = unsafe { core::mem::transmute(code.buf.ptr(code.entry)) };
    f(ctx as *mut JitCtx)
}

/// Byte offset of guest register `r` in `JitCtx`.
#[inline(always)]
fn ro(r: u8) -> i32 {
    r as i32 * 4
}
const CPSR: i32 = 64;
const PC: i32 = 15 * 4;
const FAULTED: i32 = 72;
const SMC: i32 = 76;
const BUDGET: i32 = 96;

type Asm = dynasmrt::x64::Assembler;

/// Whether `emit_op` can compile this op (else the trace ends before it and the
/// interpreter handles it). Kept in lockstep with `emit_op`.
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
        | FastOp::CondBranch { .. }
        | FastOp::Branch { .. } => true,
        FastOp::HiReg { op, .. } => op != 3,
        FastOp::AluOp { op, .. } => matches!(op, 0x0 | 0x1 | 0xC | 0xE | 0xF | 0x8 | 0xA | 0xB | 0x9),
        FastOp::HwSgnXfer { .. } => false,
    }
}

/// Compile a decoded linear trace (which may contain conditional branches) into
/// one native function, linking in-trace branch targets with internal jumps so
/// hot loops stay in compiled code. Returns the code and how many ops were
/// compiled (the caller ends the block there).
pub(crate) fn compile_block(ops: &[FastOp], start_pc: u32) -> Option<(Code, usize)> {
    let op_pc = |i: usize| start_pc.wrapping_add(2 * i as u32);

    // How far the trace compiles: up to the first unsupported op, or through a
    // terminating unconditional branch.
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

    // A dynamic label for each in-range branch target.
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

    // Basic-block boundaries (for exact per-block budget accounting): the trace
    // start, every branch target, and the instruction after every branch.
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
    dynasm!(a ; .arch x64 ; push rbx ; mov rbx, rdi);

    let mut ended_unconditional = false;
    for (i, op) in ops[..limit].iter().enumerate() {
        let pc = op_pc(i);
        if let Some(label) = labels.get(&pc) {
            dynasm!(a ; => *label);
        }
        if boundaries.contains(&pc) {
            let len = seg_len(pc) as i32;
            // Yield to the dispatcher if the budget is spent; otherwise charge
            // this block's instructions.
            dynasm!(a
                ; mov ecx, [rbx + BUDGET]
                ; test ecx, ecx
                ; jg >cont
                ; mov DWORD [rbx + PC], pc as i32
                ; mov eax, exit::CONTINUE as i32
                ; jmp ->epilogue
                ; cont:
                ; sub DWORD [rbx + BUDGET], len
            );
        }
        match *op {
            FastOp::CondBranch { cond, target, next } => {
                let _ = next;
                // Single-flag conditions test a CPSR bit inline (mask, take-when-
                // set); compound ones fall back to the interpreter's cond_met.
                let single: Option<(u32, bool)> = match cond {
                    0x0 => Some((1 << 30, true)),  // EQ: Z==1
                    0x1 => Some((1 << 30, false)), // NE: Z==0
                    0x2 => Some((1 << 29, true)),  // CS: C==1
                    0x3 => Some((1 << 29, false)), // CC: C==0
                    0x4 => Some((1 << 31, true)),  // MI: N==1
                    0x5 => Some((1 << 31, false)), // PL: N==0
                    0x6 => Some((1 << 28, true)),  // VS: V==1
                    0x7 => Some((1 << 28, false)), // VC: V==0
                    _ => None,
                };
                if let Some((mask, take_when_set)) = single {
                    dynasm!(a ; test DWORD [rbx + CPSR], mask as i32);
                    // After test, ZF==1 iff the bit is clear.
                    if in_range(target) {
                        let l = *labels.get(&target).unwrap();
                        if take_when_set {
                            dynasm!(a ; jnz => l);
                        } else {
                            dynasm!(a ; jz => l);
                        }
                    } else {
                        // Jump over the taken-exit when the branch is not taken.
                        if take_when_set {
                            dynasm!(a ; jz >notaken);
                        } else {
                            dynasm!(a ; jnz >notaken);
                        }
                        dynasm!(a
                            ; mov DWORD [rbx + PC], target as i32
                            ; mov eax, exit::CONTINUE as i32
                            ; jmp ->epilogue
                            ; notaken:
                        );
                    }
                } else {
                    dynasm!(a
                        ; mov edi, cond as i32
                        ; mov esi, [rbx + CPSR]
                        ; mov rax, QWORD jit_cond_met as *const () as i64
                        ; call rax
                        ; test eax, eax
                    );
                    if in_range(target) {
                        dynasm!(a ; jnz => *labels.get(&target).unwrap());
                    } else {
                        dynasm!(a
                            ; jz >notaken
                            ; mov DWORD [rbx + PC], target as i32
                            ; mov eax, exit::CONTINUE as i32
                            ; jmp ->epilogue
                            ; notaken:
                        );
                    }
                }
            }
            FastOp::Branch { target } => {
                if in_range(target) {
                    dynasm!(a ; jmp => *labels.get(&target).unwrap());
                } else {
                    dynasm!(a ; mov DWORD [rbx + PC], target as i32 ; mov eax, exit::CONTINUE as i32 ; jmp ->epilogue);
                }
                ended_unconditional = true;
            }
            other => {
                emit_op(&mut a, other, pc);
            }
        }
    }

    if !ended_unconditional {
        dynasm!(a ; mov DWORD [rbx + PC], end_pc as i32 ; mov eax, exit::CONTINUE as i32);
    }
    dynasm!(a ; ->epilogue: ; pop rbx ; ret);

    let buf = a.finalize().ok()?;
    Some((Code { buf, entry }, limit))
}

/// Emit one straight-line op (branches are handled by `compile_block`).
fn emit_op(a: &mut Asm, op: FastOp, pc: u32) {
    match op {
        FastOp::AddSubImm { sub, rd, rs, imm } => {
            dynasm!(a ; mov eax, [rbx + ro(rs)]);
            if sub {
                dynasm!(a ; sub eax, imm as i32);
            } else {
                dynasm!(a ; add eax, imm as i32);
            }
            emit_flags_nzcv(a, !sub);
            dynasm!(a ; mov [rbx + ro(rd)], r8d);
        }
        FastOp::AddSubReg { sub, rd, rs, rn } => {
            dynasm!(a ; mov eax, [rbx + ro(rs)]);
            if sub {
                dynasm!(a ; sub eax, [rbx + ro(rn)]);
            } else {
                dynasm!(a ; add eax, [rbx + ro(rn)]);
            }
            emit_flags_nzcv(a, !sub);
            dynasm!(a ; mov [rbx + ro(rd)], r8d);
        }
        FastOp::ImmOp { op, rd, imm } => match op {
            0 => {
                // MOV: N,Z from imm; C,V preserved.
                dynasm!(a ; mov eax, imm as i32 ; mov [rbx + ro(rd)], eax ; test eax, eax);
                emit_flags_nz(a);
            }
            1 => {
                // CMP: rd - imm, full NZCV, no writeback.
                dynasm!(a ; mov eax, [rbx + ro(rd)] ; sub eax, imm as i32);
                emit_flags_nzcv(a, false);
            }
            2 => {
                dynasm!(a ; mov eax, [rbx + ro(rd)] ; add eax, imm as i32);
                emit_flags_nzcv(a, true);
                dynasm!(a ; mov [rbx + ro(rd)], r8d);
            }
            3 => {
                dynasm!(a ; mov eax, [rbx + ro(rd)] ; sub eax, imm as i32);
                emit_flags_nzcv(a, false);
                dynasm!(a ; mov [rbx + ro(rd)], r8d);
            }
            _ => unreachable!(),
        },
        FastOp::AluOp { op, rd, rs } => emit_alu(a, op, rd, rs),
        FastOp::HiReg { op, crd, crs } => {
            // Operand values, with the PC pipeline offset folded in as a constant.
            let load_crs = |a: &mut Asm| {
                if crs == 15 {
                    dynasm!(a ; mov eax, (pc.wrapping_add(4)) as i32);
                } else {
                    dynasm!(a ; mov eax, [rbx + ro(crs)]);
                }
            };
            match op {
                0 => {
                    // ADD (no flags). crd != 15 guaranteed by decode.
                    load_crs(a);
                    dynasm!(a ; add [rbx + ro(crd)], eax);
                }
                2 => {
                    // MOV (no flags).
                    load_crs(a);
                    dynasm!(a ; mov [rbx + ro(crd)], eax);
                }
                1 => {
                    // CMP: crd - crs, full NZCV.
                    load_crs(a);
                    dynasm!(a ; mov ecx, [rbx + ro(crd)] ; sub ecx, eax ; mov eax, ecx);
                    emit_flags_nzcv(a, false);
                }
                _ => unreachable!(),
            }
        }
        FastOp::PcLoad { rd, offset } => {
            let addr = pc.wrapping_add(4).wrapping_add(offset * 4) & !3;
            emit_load(a, 32, false, rd, |a| dynasm!(a ; mov esi, addr as i32), pc);
        }
        FastOp::LoadAddr { sp, rd, imm } => {
            if sp {
                dynasm!(a ; mov eax, [rbx + ro(13)] ; add eax, (imm * 4) as i32 ; mov [rbx + ro(rd)], eax);
            } else {
                let v = (pc.wrapping_add(4) & !2).wrapping_add(imm * 4);
                dynasm!(a ; mov DWORD [rbx + ro(rd)], v as i32);
            }
        }
        FastOp::SpAdd { sub, imm } => {
            if sub {
                dynasm!(a ; sub DWORD [rbx + ro(13)], imm as i32);
            } else {
                dynasm!(a ; add DWORD [rbx + ro(13)], imm as i32);
            }
        }
        FastOp::HwXferI { load, rb, rd, offset } => {
            let off = (offset * 2) as i32;
            let addr = |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(rb)] ; add esi, off ; and esi, !1);
            if load {
                emit_load(a, 16, false, rd, addr, pc);
            } else {
                emit_store(a, 16, rd, addr, pc);
            }
        }
        FastOp::SingleXferI { load, byte, rb, rd, offset } => {
            let scaled = if byte { offset } else { offset * 4 } as i32;
            let addr = move |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(rb)] ; add esi, scaled);
            let size = if byte { 8 } else { 32 };
            if load {
                emit_load(a, size, false, rd, addr, pc);
            } else {
                emit_store(a, size, rd, addr, pc);
            }
        }
        FastOp::SpXfer { load, rd, offset } => {
            let addr = move |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(13)] ; add esi, offset as i32);
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
            let addr = move |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(rb)] ; add esi, [rbx + ro(roff)]);
            let size = if byte { 8 } else { 32 };
            if load {
                emit_load(a, size, false, rd, addr, pc);
            } else {
                emit_store(a, size, rd, addr, pc);
            }
        }
        FastOp::Shift { op, rd, rs, shift } => emit_shift(a, op, rd, rs, shift),
        FastOp::CondBranch { .. } | FastOp::Branch { .. } | FastOp::HwSgnXfer { .. } => {
            unreachable!("handled by compile_block or unsupported")
        }
    }
}

/// Thumb `Shifted` (LSL/LSR/ASR by a 5-bit immediate). Updates N, Z and C
/// (from the shifted-out bit), preserving V — mirroring `arg_shift`/`arg_shift0`.
/// The shift amount is a compile-time constant, so each case is specialized.
fn emit_shift(a: &mut Asm, op: u8, rd: u8, rs: u8, shift: u32) {
    if shift == 0 {
        match op {
            0 => {
                // LSL #0: value unchanged, C preserved -> only N,Z change.
                dynasm!(a ; mov eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
                emit_flags_nz(a);
                return;
            }
            1 => {
                // LSR #0 == LSR #32: result 0, C = bit 31.
                dynasm!(a ; mov eax, [rbx + ro(rs)] ; mov ecx, eax ; shr ecx, 31 ; xor eax, eax);
            }
            2 => {
                // ASR #0 == ASR #32: result = sign-extend, C = bit 31.
                dynasm!(a ; mov eax, [rbx + ro(rs)] ; mov ecx, eax ; shr ecx, 31 ; sar eax, 31);
            }
            _ => unreachable!(),
        }
    } else {
        dynasm!(a ; mov eax, [rbx + ro(rs)] ; mov ecx, eax);
        match op {
            0 => dynasm!(a ; shr ecx, (32 - shift) as i8 ; and ecx, 1 ; shl eax, shift as i8),
            1 => dynasm!(a ; shr ecx, (shift - 1) as i8 ; and ecx, 1 ; shr eax, shift as i8),
            2 => dynasm!(a ; shr ecx, (shift - 1) as i8 ; and ecx, 1 ; sar eax, shift as i8),
            _ => unreachable!(),
        }
    }
    // res in eax, carry (0/1) in ecx.
    dynasm!(a ; mov [rbx + ro(rd)], eax);
    emit_flags_nzc(a);
}

/// Pack N, Z (from the result in `eax`) and C (0/1 in `ecx`) into CPSR,
/// preserving V. The result must already be stored.
fn emit_flags_nzc(a: &mut Asm) {
    dynasm!(a
        ; test eax, eax
        ; setz r9b
        ; sets r10b
        ; movzx r9d, r9b
        ; movzx r10d, r10b
        ; shl r10d, 31
        ; shl r9d, 30
        ; shl ecx, 29
        ; or r10d, r9d
        ; or r10d, ecx
        ; mov eax, [rbx + CPSR]
        ; and eax, 0x1fff_ffff  // clear N,Z,C; keep V and mode
        ; or eax, r10d
        ; mov [rbx + CPSR], eax
    );
}

/// The 16 Thumb data-processing ALU ops. Supports the logical and simple
/// arithmetic ones; declines carry-in / shift / multiply forms.
fn emit_alu(a: &mut Asm, op: u8, rd: u8, rs: u8) {
    match op {
        0x0 => {
            // AND (N,Z; C,V preserved)
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; and eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
        }
        0x1 => {
            // EOR
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; xor eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
        }
        0xC => {
            // ORR
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; or eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
        }
        0xE => {
            // BIC: rd & !rs
            dynasm!(a ; mov eax, [rbx + ro(rs)] ; not eax ; and eax, [rbx + ro(rd)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
        }
        0xF => {
            // MVN: !rs
            dynasm!(a ; mov eax, [rbx + ro(rs)] ; not eax ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
        }
        0x8 => {
            // TST: rd & rs, N,Z only, no writeback
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; and eax, [rbx + ro(rs)] ; test eax, eax);
            emit_flags_nz(a);
        }
        0xA => {
            // CMP: rd - rs, full NZCV, no writeback
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; sub eax, [rbx + ro(rs)]);
            emit_flags_nzcv(a, false);
        }
        0xB => {
            // CMN: rd + rs, full NZCV, no writeback
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; add eax, [rbx + ro(rs)]);
            emit_flags_nzcv(a, true);
        }
        0x9 => {
            // NEG: 0 - rs, full NZCV, writeback
            dynasm!(a ; xor eax, eax ; sub eax, [rbx + ro(rs)]);
            emit_flags_nzcv(a, false);
            dynasm!(a ; mov [rbx + ro(rd)], r8d);
        }
        _ => unreachable!(), // ADC/SBC/MUL/shift-by-reg
    }
}

/// After a `sub`/`add` on `eax` sets the host flags, save the result to `r8d`
/// and pack ARM NZCV into CPSR. `is_add` selects the carry mapping (ARM carry is
/// the add carry-out, but the inverse of the subtract borrow).
fn emit_flags_nzcv(a: &mut Asm, is_add: bool) {
    dynasm!(a
        ; mov r8d, eax          // save result (does not touch flags)
        ; setz r9b              // Z
        ; sets r10b             // N
        ; seto cl               // V
    );
    if is_add {
        dynasm!(a ; setc r11b);
    } else {
        dynasm!(a ; setnc r11b);
    }
    dynasm!(a
        ; movzx r9d, r9b
        ; movzx r10d, r10b
        ; movzx r11d, r11b
        ; movzx ecx, cl
        ; shl r10d, 31          // N -> bit31
        ; shl r9d, 30           // Z -> bit30
        ; shl r11d, 29          // C -> bit29
        ; shl ecx, 28           // V -> bit28
        ; or r10d, r9d
        ; or r10d, r11d
        ; or r10d, ecx
        ; mov eax, [rbx + CPSR]
        ; and eax, 0x0fff_ffff  // clear NZCV
        ; or eax, r10d
        ; mov [rbx + CPSR], eax
    );
}

/// After `test eax, eax` (or any op leaving ZF/SF for the result), update only
/// N and Z in CPSR, preserving C and V.
fn emit_flags_nz(a: &mut Asm) {
    dynasm!(a
        ; setz r9b
        ; sets r10b
        ; movzx r9d, r9b
        ; movzx r10d, r10b
        ; shl r10d, 31
        ; shl r9d, 30
        ; or r10d, r9d
        ; mov eax, [rbx + CPSR]
        ; and eax, 0x3fff_ffff  // clear only N,Z
        ; or eax, r10d
        ; mov [rbx + CPSR], eax
    );
}

/// Emit a guest load: compute the address (into esi) via `addr`, call the size's
/// helper, check for a fault, and on success store the (optionally sign-extended)
/// result into `rd`. `pc` is the op's PC for the fault exit.
fn emit_load(a: &mut Asm, size: u8, signed: bool, rd: u8, addr: impl FnOnce(&mut Asm), pc: u32) {
    let helper = match size {
        8 => jit_load8 as *const () as i64,
        16 => jit_load16 as *const () as i64,
        _ => jit_load32 as *const () as i64,
    };
    dynasm!(a ; mov rdi, rbx);
    addr(a);
    dynasm!(a
        ; mov rax, QWORD helper
        ; call rax
        ; mov r8d, eax                    // loaded value (0 on fault)
    );
    if signed {
        match size {
            8 => dynasm!(a ; movsx r8d, r8b),
            16 => dynasm!(a ; movsx r8d, r8w),
            _ => {}
        }
    }
    // Write the destination *before* checking for a fault: the interpreter's
    // load path stores the (dummy 0) value into rd even when the access faults,
    // then aborts, so we must too.
    dynasm!(a
        ; mov [rbx + ro(rd)], r8d
        ; mov ecx, [rbx + FAULTED]
        ; test ecx, ecx
        ; jz >nofault
        ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32
        ; mov eax, exit::FAULT as i32
        ; jmp ->epilogue
        ; nofault:
    );
}

/// Emit a guest store: compute the address (into esi) via `addr`, load the value
/// from `rd`, call the size's helper, then check fault and SMC.
fn emit_store(a: &mut Asm, size: u8, rd: u8, addr: impl FnOnce(&mut Asm), pc: u32) {
    let helper = match size {
        8 => jit_store8 as *const () as i64,
        16 => jit_store16 as *const () as i64,
        _ => jit_store32 as *const () as i64,
    };
    dynasm!(a ; mov rdi, rbx);
    addr(a);
    dynasm!(a
        ; mov edx, [rbx + ro(rd)]
        ; mov rax, QWORD helper
        ; call rax
        ; mov ecx, [rbx + FAULTED]
        ; test ecx, ecx
        ; jz >nofault
        ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32
        ; mov eax, exit::FAULT as i32
        ; jmp ->epilogue
        ; nofault:
        ; mov ecx, [rbx + SMC]
        ; test ecx, ecx
        ; jz >nosmc
        ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32
        ; mov eax, exit::SMC as i32
        ; jmp ->epilogue
        ; nosmc:
    );
}
