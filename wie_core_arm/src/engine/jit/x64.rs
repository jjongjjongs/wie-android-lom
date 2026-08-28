//! x86-64 code generator for the JIT.
//!
//! Each Thumb basic block is compiled to a `extern "C" fn(*mut JitCtx) -> u32`
//! following the SysV AMD64 convention: `rdi` holds the context pointer on
//! entry, moved into the callee-saved `rbx` for the block's lifetime; guest
//! register `i` lives at `[rbx + i*4]`, CPSR at `[rbx + 64]`. The function
//! returns an `exit::*` code and always leaves `regs[15]` (`[rbx + 60]`) at the
//! next guest PC. Guest state lives entirely in the context array (not host
//! registers), so helper calls need only preserve `rbx`.

use dynasmrt::{AssemblyOffset, DynasmApi, DynasmLabelApi, ExecutableBuffer, dynasm};

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

type Asm = dynasmrt::x64::Assembler;

pub(crate) fn compile_block(ops: &[FastOp], pc: u32) -> Option<(Code, usize)> {
    let mut a = Asm::new().ok()?;
    let entry = a.offset();
    dynasm!(a
        ; .arch x64
        ; push rbx
        ; mov rbx, rdi
    );

    let mut cur = pc;
    let mut compiled = 0usize;
    let mut ended_with_branch = false;
    for op in ops {
        if !emit_op(&mut a, *op, cur) {
            break;
        }
        compiled += 1;
        cur = cur.wrapping_add(2);
        if matches!(op, FastOp::CondBranch { .. } | FastOp::Branch { .. }) {
            ended_with_branch = true;
            break;
        }
    }
    if compiled == 0 {
        return None;
    }

    // Fall-through end (block did not end in a branch): set PC past the last
    // compiled instruction and return CONTINUE.
    if !ended_with_branch {
        dynasm!(a ; mov DWORD [rbx + PC], cur as i32 ; mov eax, exit::CONTINUE as i32);
    }
    // Shared epilogue.
    dynasm!(a
        ; ->epilogue:
        ; pop rbx
        ; ret
    );

    let buf = a.finalize().ok()?;
    Some((Code { buf, entry }, compiled))
}

/// Emit one op. Returns false if the op is not supported (caller ends the block
/// there and falls back to the interpreter for it). `pc` is the op's guest PC.
fn emit_op(a: &mut Asm, op: FastOp, pc: u32) -> bool {
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
            true
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
            true
        }
        FastOp::ImmOp { op, rd, imm } => match op {
            0 => {
                // MOV: N,Z from imm; C,V preserved.
                dynasm!(a ; mov eax, imm as i32 ; mov [rbx + ro(rd)], eax ; test eax, eax);
                emit_flags_nz(a);
                true
            }
            1 => {
                // CMP: rd - imm, full NZCV, no writeback.
                dynasm!(a ; mov eax, [rbx + ro(rd)] ; sub eax, imm as i32);
                emit_flags_nzcv(a, false);
                true
            }
            2 => {
                dynasm!(a ; mov eax, [rbx + ro(rd)] ; add eax, imm as i32);
                emit_flags_nzcv(a, true);
                dynasm!(a ; mov [rbx + ro(rd)], r8d);
                true
            }
            3 => {
                dynasm!(a ; mov eax, [rbx + ro(rd)] ; sub eax, imm as i32);
                emit_flags_nzcv(a, false);
                dynasm!(a ; mov [rbx + ro(rd)], r8d);
                true
            }
            _ => false,
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
                    true
                }
                2 => {
                    // MOV (no flags).
                    load_crs(a);
                    dynasm!(a ; mov [rbx + ro(crd)], eax);
                    true
                }
                1 => {
                    // CMP: crd - crs, full NZCV.
                    load_crs(a);
                    dynasm!(a ; mov ecx, [rbx + ro(crd)] ; sub ecx, eax ; mov eax, ecx);
                    emit_flags_nzcv(a, false);
                    true
                }
                _ => false,
            }
        }
        FastOp::PcLoad { rd, offset } => {
            let addr = pc.wrapping_add(4).wrapping_add(offset * 4) & !3;
            emit_load(a, 32, false, rd, |a| dynasm!(a ; mov esi, addr as i32), pc);
            true
        }
        FastOp::LoadAddr { sp, rd, imm } => {
            if sp {
                dynasm!(a ; mov eax, [rbx + ro(13)] ; add eax, (imm * 4) as i32 ; mov [rbx + ro(rd)], eax);
            } else {
                let v = (pc.wrapping_add(4) & !2).wrapping_add(imm * 4);
                dynasm!(a ; mov DWORD [rbx + ro(rd)], v as i32);
            }
            true
        }
        FastOp::SpAdd { sub, imm } => {
            if sub {
                dynasm!(a ; sub DWORD [rbx + ro(13)], imm as i32);
            } else {
                dynasm!(a ; add DWORD [rbx + ro(13)], imm as i32);
            }
            true
        }
        FastOp::HwXferI { load, rb, rd, offset } => {
            let off = (offset * 2) as i32;
            let addr = |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(rb)] ; add esi, off ; and esi, !1);
            if load {
                emit_load(a, 16, false, rd, addr, pc);
            } else {
                emit_store(a, 16, rd, addr, pc);
            }
            true
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
            true
        }
        FastOp::SpXfer { load, rd, offset } => {
            let addr = move |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(13)] ; add esi, offset as i32);
            if load {
                emit_load(a, 32, false, rd, addr, pc);
            } else {
                emit_store(a, 32, rd, addr, pc);
            }
            true
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
            true
        }
        FastOp::CondBranch { cond, target, next } => {
            // cond_met(cond, cpsr) via a helper (exact interpreter semantics).
            dynasm!(a
                ; mov edi, cond as i32
                ; mov esi, [rbx + CPSR]
                ; mov rax, QWORD jit_cond_met as *const () as i64
                ; call rax
                ; test eax, eax
                ; mov eax, target as i32       // if taken, PC = target
                ; mov ecx, next as i32         // else PC = next
                ; cmovz eax, ecx
                ; mov [rbx + PC], eax
                ; mov eax, exit::CONTINUE as i32
                ; jmp ->epilogue
            );
            true
        }
        FastOp::Branch { target } => {
            dynasm!(a
                ; mov DWORD [rbx + PC], target as i32
                ; mov eax, exit::CONTINUE as i32
                ; jmp ->epilogue
            );
            true
        }
        // Not yet supported: shifts (shifter carry), signed/complex ALU,
        // signed halfword transfers. End the block; the interpreter handles them.
        FastOp::Shift { .. } | FastOp::HwSgnXfer { .. } => false,
    }
}

/// The 16 Thumb data-processing ALU ops. Supports the logical and simple
/// arithmetic ones; declines carry-in / shift / multiply forms.
fn emit_alu(a: &mut Asm, op: u8, rd: u8, rs: u8) -> bool {
    match op {
        0x0 => {
            // AND (N,Z; C,V preserved)
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; and eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
            true
        }
        0x1 => {
            // EOR
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; xor eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
            true
        }
        0xC => {
            // ORR
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; or eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
            true
        }
        0xE => {
            // BIC: rd & !rs
            dynasm!(a ; mov eax, [rbx + ro(rs)] ; not eax ; and eax, [rbx + ro(rd)] ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
            true
        }
        0xF => {
            // MVN: !rs
            dynasm!(a ; mov eax, [rbx + ro(rs)] ; not eax ; mov [rbx + ro(rd)], eax ; test eax, eax);
            emit_flags_nz(a);
            true
        }
        0x8 => {
            // TST: rd & rs, N,Z only, no writeback
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; and eax, [rbx + ro(rs)] ; test eax, eax);
            emit_flags_nz(a);
            true
        }
        0xA => {
            // CMP: rd - rs, full NZCV, no writeback
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; sub eax, [rbx + ro(rs)]);
            emit_flags_nzcv(a, false);
            true
        }
        0xB => {
            // CMN: rd + rs, full NZCV, no writeback
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; add eax, [rbx + ro(rs)]);
            emit_flags_nzcv(a, true);
            true
        }
        0x9 => {
            // NEG: 0 - rs, full NZCV, writeback
            dynasm!(a ; xor eax, eax ; sub eax, [rbx + ro(rs)]);
            emit_flags_nzcv(a, false);
            dynasm!(a ; mov [rbx + ro(rd)], r8d);
            true
        }
        _ => false, // ADC/SBC/MUL/shift-by-reg
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
