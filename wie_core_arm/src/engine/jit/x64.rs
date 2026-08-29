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

use crate::engine::fast::{FastOp, ends_trace};

use super::arm_frontend::{ArmOp, Off, Op2, arm_ends_trace};
use super::{JitCtx, exit, jit_alu_shift, jit_arm_shift, jit_cond_met, jit_load8, jit_load16, jit_load32, jit_store8, jit_store16, jit_store32};

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
const SCRATCH: i32 = 100;
/// Offset of `JitCtx::pages` (base of the guest page table).
const PAGES: i32 = 104;

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
            // Control-flow ops that write a dynamic/far PC end the trace: emit
            // them, then let the epilogue follow (they jump to it themselves).
            t @ (FastOp::BranchExchange { .. }
            | FastOp::BranchLink { .. }
            | FastOp::MovPc { .. }
            | FastOp::PushPop { load: true, extra: true, .. }) => {
                emit_op(&mut a, t, pc);
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

// ---------------------------------------------------------------------------
// ARM (A32) code generation. Shares the block/exit protocol and helpers with the
// Thumb path above; only the instruction encoding differs.
// ---------------------------------------------------------------------------

/// Whether `emit_arm`/`emit_arm_terminator` can compile this op.
fn arm_supported(_op: &ArmOp) -> bool {
    true
}

/// Compile a linear ARM trace (straight-line ops through the first terminator)
/// into one native function. No in-trace linking yet: each trace is one basic
/// block, charged to the budget once, and every terminator sets PC and exits.
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
    let block_len = limit as i32;

    let mut a = Asm::new().ok()?;
    let entry = a.offset();
    dynasm!(a
        ; .arch x64
        ; push rbx
        ; mov rbx, rdi
        // Whole-trace budget: yield if spent, else charge every op once.
        ; mov ecx, [rbx + BUDGET]
        ; test ecx, ecx
        ; jg >run
        ; mov DWORD [rbx + PC], start_pc as i32
        ; mov eax, exit::CONTINUE as i32
        ; jmp ->epilogue
        ; run:
        ; sub DWORD [rbx + BUDGET], block_len
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
        dynasm!(a ; mov DWORD [rbx + PC], end_pc as i32 ; mov eax, exit::CONTINUE as i32);
    }
    dynasm!(a ; ->epilogue: ; pop rbx ; ret);

    let buf = a.finalize().ok()?;
    Some((Code { buf, entry }, limit))
}

/// Emit a guard that jumps to the local `>skip` label when `cond` is *not* met.
/// Returns whether a guard (and thus a `skip:` the caller must place) was
/// emitted. `cond` 0xE (AL) needs none.
fn emit_arm_guard(a: &mut Asm, cond: u8) -> bool {
    let single: Option<(i32, bool)> = match cond {
        0x0 => Some(((1 << 30), true)),              // EQ: Z==1
        0x1 => Some(((1 << 30), false)),             // NE
        0x2 => Some(((1 << 29), true)),              // CS: C==1
        0x3 => Some(((1 << 29), false)),             // CC
        0x4 => Some((((1u32 << 31) as i32), true)),  // MI: N==1
        0x5 => Some((((1u32 << 31) as i32), false)), // PL
        0x6 => Some(((1 << 28), true)),              // VS: V==1
        0x7 => Some(((1 << 28), false)),             // VC
        _ => None,
    };
    match single {
        Some((mask, take_when_set)) => {
            dynasm!(a ; test DWORD [rbx + CPSR], mask);
            if take_when_set {
                dynasm!(a ; jz >skip); // bit clear -> not taken
            } else {
                dynasm!(a ; jnz >skip);
            }
            true
        }
        None if cond >= 0xe => false, // AL / unconditional
        None => {
            // Compound condition: defer to the interpreter's exact predicate.
            dynasm!(a
                ; mov edi, cond as i32
                ; mov esi, [rbx + CPSR]
                ; mov rax, QWORD jit_cond_met as *const () as i64
                ; call rax
                ; test eax, eax
                ; jz >skip
            );
            true
        }
    }
}

/// Compute operand2 into `r10d` (value) and `r11d` (shifter carry, for logical
/// flag-setting ops).
fn emit_arm_op2(a: &mut Asm, op2: Op2) {
    match op2 {
        Op2::Imm { val, carry } => {
            dynasm!(a ; mov r10d, val as i32 ; mov r11d, carry as i32);
        }
        Op2::ShiftImm { rm, ty, amount } => {
            emit_arm_shift_call(a, rm, ty, amount as i32, false, None);
        }
        Op2::ShiftReg { rm, ty, rs } => {
            emit_arm_shift_call(a, rm, ty, 0, true, Some(rs));
        }
    }
}

/// Call `jit_arm_shift(reg[rm], ty, amount, reg_shift, c)`, leaving value in
/// `r10d` and carry in `r11d`. For a register shift, `amount` comes from
/// `reg[rs] & 0xff`.
fn emit_arm_shift_call(a: &mut Asm, rm: u8, ty: u8, amount_imm: i32, reg_shift: bool, rs: Option<u8>) {
    dynasm!(a
        ; mov edi, [rbx + ro(rm)]
        ; mov esi, ty as i32
    );
    if let Some(rs) = rs {
        dynasm!(a ; mov edx, [rbx + ro(rs)] ; and edx, 0xff);
    } else {
        dynasm!(a ; mov edx, amount_imm);
    }
    dynasm!(a
        ; mov ecx, reg_shift as i32
        ; mov r8d, [rbx + CPSR]
        ; shr r8d, 29
        ; and r8d, 1
        ; mov rax, QWORD jit_arm_shift as *const () as i64
        ; call rax
        ; mov r10d, eax
        ; shr rax, 32
        ; mov r11d, eax
    );
}

/// Emit a straight-line (non-terminator) ARM op.
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
            // Writeback (result in r8d) unless TST/TEQ/CMP/CMN.
            if !matches!(opcode, 0x8..=0xB) {
                dynasm!(a ; mov [rbx + ro(rd)], r8d);
            }
            if guarded {
                dynasm!(a ; skip:);
            }
        }
        ArmOp::LoadStore { .. } => emit_arm_load_store(a, op, pc),
        ArmOp::Block { .. } => {
            let guarded = emit_arm_guard(a, block_cond(op));
            emit_arm_block_body(a, op, pc);
            if guarded {
                dynasm!(a ; skip:);
            }
        }
        _ => unreachable!("terminator in straight slot"),
    }
}

/// The ALU core: operand2 in `r10d`, shifter carry in `r11d`; leaves the result
/// in `r8d` and, when `s`, updates CPSR.
fn emit_arm_alu(a: &mut Asm, opcode: u8, s: bool, rn: u8) {
    let logical = matches!(opcode, 0x0 | 0x1 | 0x8 | 0x9 | 0xC | 0xD | 0xE | 0xF);
    match opcode {
        0x0 | 0x8 => dynasm!(a ; mov eax, [rbx + ro(rn)] ; and eax, r10d),
        0x1 | 0x9 => dynasm!(a ; mov eax, [rbx + ro(rn)] ; xor eax, r10d),
        0xC => dynasm!(a ; mov eax, [rbx + ro(rn)] ; or eax, r10d),
        0xE => dynasm!(a ; mov eax, r10d ; not eax ; and eax, [rbx + ro(rn)]),
        0xD => dynasm!(a ; mov eax, r10d),
        0xF => dynasm!(a ; mov eax, r10d ; not eax),
        0x2 | 0xA => dynasm!(a ; mov eax, [rbx + ro(rn)] ; sub eax, r10d),
        0x3 => dynasm!(a ; mov eax, r10d ; sub eax, [rbx + ro(rn)]),
        0x4 | 0xB => dynasm!(a ; mov eax, [rbx + ro(rn)] ; add eax, r10d),
        0x5 => dynasm!(a
            ; mov eax, [rbx + ro(rn)]
            ; mov ecx, [rbx + CPSR] ; shr ecx, 29 ; and ecx, 1 ; neg ecx // CF = C
            ; adc eax, r10d),
        0x6 => dynasm!(a
            ; mov eax, [rbx + ro(rn)]
            ; mov ecx, [rbx + CPSR] ; shr ecx, 29 ; and ecx, 1 ; xor ecx, 1 ; neg ecx // CF = 1-C
            ; sbb eax, r10d),
        0x7 => dynasm!(a
            ; mov eax, r10d
            ; mov ecx, [rbx + CPSR] ; shr ecx, 29 ; and ecx, 1 ; xor ecx, 1 ; neg ecx
            ; sbb eax, [rbx + ro(rn)]),
        _ => unreachable!(),
    }
    if logical {
        dynasm!(a ; mov r8d, eax);
        if s {
            // N,Z from result, C = shifter carry, V preserved.
            dynasm!(a ; mov ecx, r11d);
            emit_flags_nzc(a);
        }
    } else if s {
        let is_add = matches!(opcode, 0x4 | 0x5 | 0xB);
        emit_flags_nzcv(a, is_add);
    } else {
        dynasm!(a ; mov r8d, eax);
    }
}

fn block_cond(op: &ArmOp) -> u8 {
    match op {
        ArmOp::Block { cond, .. } => *cond,
        _ => 0xe,
    }
}

/// Emit an ARM single load/store (word/byte), all index modes. Writeback follows
/// the interpreter: post-index or the W bit updates `rn` (skipped for a load into
/// `rn`), and a store captures its value before writeback so `str rX,[rX],#n`
/// stores the original.
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

    // Offset -> edx (may call the shifter, which clobbers scratch, so do it
    // before reading the base).
    match offset {
        Off::Imm(v) => dynasm!(a ; mov edx, v as i32),
        Off::ShiftImm { rm, ty, amount } => {
            emit_arm_shift_call(a, rm, ty, amount as i32, false, None);
            dynasm!(a ; mov edx, r10d);
        }
    }
    // Base -> ecx.
    match base_pc {
        Some(v) => dynasm!(a ; mov ecx, v as i32),
        None => dynasm!(a ; mov ecx, [rbx + ro(rn)]),
    }
    // post = up ? base+off : base-off (eax); addr = pre ? post : base (esi).
    dynasm!(a ; mov eax, ecx);
    if up {
        dynasm!(a ; add eax, edx);
    } else {
        dynasm!(a ; sub eax, edx);
    }
    if pre {
        dynasm!(a ; mov esi, eax);
    } else {
        dynasm!(a ; mov esi, ecx);
    }

    let do_wb = (!pre || wb) && base_pc.is_none() && (!load || rd != rn);

    if load {
        if do_wb {
            dynasm!(a ; mov [rbx + ro(rn)], eax); // rn = post (eax dies in the call)
        }
        let helper = if byte { jit_load8 } else { jit_load32 } as *const () as i64;
        dynasm!(a
            ; mov rdi, rbx
            ; mov rax, QWORD helper
            ; call rax
            ; mov [rbx + ro(rd)], eax   // write before the fault check (interpreter parity)
            ; mov ecx, [rbx + FAULTED]
            ; test ecx, ecx
            ; jz >nofault
            ; mov DWORD [rbx + PC], pc.wrapping_add(4) as i32
            ; mov eax, exit::FAULT as i32
            ; jmp ->epilogue
            ; nofault:
        );
    } else {
        // Capture the store value before writeback so a store of rn is correct.
        dynasm!(a ; mov edi, [rbx + ro(rd)]);
        if do_wb {
            dynasm!(a ; mov [rbx + ro(rn)], eax);
        }
        let helper = if byte { jit_store8 } else { jit_store32 } as *const () as i64;
        dynasm!(a
            ; mov edx, edi              // value
            ; mov rdi, rbx
            ; mov rax, QWORD helper
            ; call rax
            ; mov ecx, [rbx + FAULTED]
            ; test ecx, ecx
            ; jz >nofault
            ; mov DWORD [rbx + PC], pc.wrapping_add(4) as i32
            ; mov eax, exit::FAULT as i32
            ; jmp ->epilogue
            ; nofault:
            ; mov ecx, [rbx + SMC]
            ; test ecx, ecx
            ; jz >nosmc
            ; mov DWORD [rbx + PC], pc.wrapping_add(4) as i32
            ; mov eax, exit::SMC as i32
            ; jmp ->epilogue
            ; nosmc:
        );
    }
    if guarded {
        dynasm!(a ; skip:);
    }
}

/// Emit the register-list transfer of an ARM LDM/STM (S bit clear), fully
/// unrolled. On a load that includes r15 the caller ends the trace afterwards.
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
    // orig base -> scratch; writeback rn = post first.
    dynasm!(a ; mov eax, [rbx + ro(rn)] ; mov [rbx + SCRATCH], eax);
    let post_delta = if up { total * 4 } else { -total * 4 };
    if wb {
        dynasm!(a ; mov eax, [rbx + SCRATCH] ; add eax, post_delta ; mov [rbx + ro(rn)], eax);
    }
    let addr_base = if up { 0 } else { -total * 4 };
    let pre_incr = (pre == up) as i32;
    for (i, &r) in regs.iter().enumerate() {
        let off = addr_base + (i as i32 + pre_incr) * 4;
        if load {
            dynasm!(a
                ; mov rdi, rbx
                ; mov esi, [rbx + SCRATCH]
                ; add esi, off
                ; mov rax, QWORD jit_load32 as *const () as i64
                ; call rax
                ; mov [rbx + ro(r)], eax
            );
        } else {
            dynasm!(a ; mov rdi, rbx ; mov esi, [rbx + SCRATCH] ; add esi, off);
            if r == 15 {
                dynasm!(a ; mov edx, pc.wrapping_add(12) as i32);
            } else if r == rn && wb && i == 0 {
                dynasm!(a ; mov edx, [rbx + SCRATCH]); // lowest reg + writeback -> original base
            } else {
                dynasm!(a ; mov edx, [rbx + ro(r)]);
            }
            dynasm!(a ; mov rax, QWORD jit_store32 as *const () as i64 ; call rax);
        }
    }
    // Deferred fault (+ SMC for stores) check.
    dynasm!(a
        ; mov ecx, [rbx + FAULTED]
        ; test ecx, ecx
        ; jz >nofault
        ; mov DWORD [rbx + PC], pc.wrapping_add(4) as i32
        ; mov eax, exit::FAULT as i32
        ; jmp ->epilogue
        ; nofault:
    );
    if !load {
        dynasm!(a
            ; mov ecx, [rbx + SMC]
            ; test ecx, ecx
            ; jz >nosmc
            ; mov DWORD [rbx + PC], pc.wrapping_add(4) as i32
            ; mov eax, exit::SMC as i32
            ; jmp ->epilogue
            ; nosmc:
        );
    }
}

/// Emit an ARM trace terminator (branch / bx / computed-PC data-proc / ldm-pc).
/// A conditional terminator falls through to `end_pc` when not taken.
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
                dynasm!(a ; mov DWORD [rbx + ro(14)], ret as i32);
            }
            if to_thumb {
                dynasm!(a ; or DWORD [rbx + CPSR], (1 << 5));
            }
            dynasm!(a ; mov DWORD [rbx + PC], target as i32);
        }
        ArmOp::BranchEx { rm, .. } => {
            if rm == 15 {
                dynasm!(a ; mov eax, pc.wrapping_add(8) as i32);
            } else {
                dynasm!(a ; mov eax, [rbx + ro(rm)]);
            }
            dynasm!(a
                ; mov ecx, eax
                ; and ecx, 1
                ; mov edx, ecx
                ; shl edx, 1
                ; or edx, -4
                ; and eax, edx
                ; mov [rbx + PC], eax
                ; mov edx, [rbx + CPSR]
                ; and edx, !(1 << 5)
                ; mov r8d, ecx
                ; shl r8d, 5
                ; or edx, r8d
                ; mov [rbx + CPSR], edx
            );
        }
        ArmOp::DataProc { opcode, rn, op2, .. } => {
            // rd == 15: compute the result and use it as the branch target (no
            // flags: the S form fell back at decode).
            emit_arm_op2(a, op2);
            emit_arm_alu(a, opcode, false, rn);
            dynasm!(a ; mov [rbx + PC], r8d);
        }
        ArmOp::Block { .. } => {
            emit_arm_block_body(a, op, pc);
            // The loaded r15 is already in regs[15]; fall through to the exit.
        }
        _ => unreachable!(),
    }
    dynasm!(a ; mov eax, exit::CONTINUE as i32 ; jmp ->epilogue);
    if guarded {
        dynasm!(a
            ; skip:
            ; mov DWORD [rbx + PC], end_pc as i32
            ; mov eax, exit::CONTINUE as i32
            ; jmp ->epilogue
        );
    }
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
        FastOp::HwSgnXfer { s, h, ro: roff, rb, rd } => {
            // Halfword accesses align the address; the signed-byte load does not.
            let hw_addr = move |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(rb)] ; add esi, [rbx + ro(roff)] ; and esi, !1);
            let byte_addr = move |a: &mut Asm| dynasm!(a ; mov esi, [rbx + ro(rb)] ; add esi, [rbx + ro(roff)]);
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

/// Emit `push`/`pop`. The register list and count are compile-time constants, so
/// the transfer is fully unrolled. Every access runs (each helper records a
/// fault but continues, matching the interpreter completing the instruction) and
/// SP is updated unconditionally; the fault/SMC check is deferred to the end. A
/// `pop {..,pc}` writes a dynamic PC and ends the trace with CONTINUE; the other
/// forms fall through to the next op.
fn emit_push_pop(a: &mut Asm, load: bool, extra: bool, rlist: u8, pc: u32) {
    // Ascending register order; the extra register (LR for push, PC for pop) is
    // r14/r15 and so comes last.
    let mut regs: alloc::vec::Vec<u8> = (0..8u8).filter(|&r| rlist & (1 << r) != 0).collect();
    if extra {
        regs.push(if load { 15 } else { 14 });
    }
    let total = regs.len() as i32;
    let pop_pc = load && extra;

    if load {
        // POP: addr = SP, load ascending, then SP += total*4.
        for (i, &r) in regs.iter().enumerate() {
            let off = i as i32 * 4;
            dynasm!(a
                ; mov rdi, rbx
                ; mov esi, [rbx + ro(13)]
                ; add esi, off
                ; mov rax, QWORD jit_load32 as *const () as i64
                ; call rax
            );
            if r == 15 {
                // POP {..,pc}: T from bit 0, PC = val & !1.
                dynasm!(a
                    ; mov r8d, eax
                    ; mov ecx, r8d
                    ; and ecx, 1
                    ; mov edx, [rbx + CPSR]
                    ; and edx, !(1 << 5)
                    ; mov r9d, ecx
                    ; shl r9d, 5
                    ; or edx, r9d
                    ; mov [rbx + CPSR], edx
                    ; and r8d, !1
                    ; mov [rbx + PC], r8d
                );
            } else {
                dynasm!(a ; mov [rbx + ro(r)], eax);
            }
        }
        dynasm!(a ; add DWORD [rbx + ro(13)], total * 4);
    } else {
        // PUSH: addr = SP - total*4, store ascending, then SP = addr.
        for (i, &r) in regs.iter().enumerate() {
            let off = i as i32 * 4 - total * 4;
            dynasm!(a
                ; mov rdi, rbx
                ; mov esi, [rbx + ro(13)]
                ; add esi, off
                ; mov edx, [rbx + ro(r)]
                ; mov rax, QWORD jit_store32 as *const () as i64
                ; call rax
            );
        }
        dynasm!(a ; sub DWORD [rbx + ro(13)], total * 4);
    }

    // Deferred fault check. A pop-with-pc has already written the popped PC; the
    // other forms report the instruction's own next PC.
    dynasm!(a ; mov ecx, [rbx + FAULTED] ; test ecx, ecx ; jz >nofault);
    if !pop_pc {
        dynasm!(a ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32);
    }
    dynasm!(a ; mov eax, exit::FAULT as i32 ; jmp ->epilogue ; nofault:);
    if !load {
        // A push can hit a page holding compiled code (self-modifying code).
        dynasm!(a
            ; mov ecx, [rbx + SMC]
            ; test ecx, ecx
            ; jz >nosmc
            ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32
            ; mov eax, exit::SMC as i32
            ; jmp ->epilogue
            ; nosmc:
        );
    }
    if pop_pc {
        dynasm!(a ; mov eax, exit::CONTINUE as i32 ; jmp ->epilogue);
    }
}

/// Emit `ldmia`/`stmia rb!, {rlist}`, fully unrolled. The base is stashed in the
/// context scratch word so it survives the helper calls even when `rb` is in the
/// list and gets overwritten. Writeback happens first (matching the interpreter),
/// so an LDM that reloads `rb` overrides it and an STM of `rb` other than at the
/// lowest slot stores the written-back value. Fault/SMC is checked once at the
/// end. No PC transfer (rlist is r0..r7), so it is straight-line.
fn emit_block_xfer(a: &mut Asm, load: bool, rb: u8, rlist: u8, pc: u32) {
    let regs: alloc::vec::Vec<u8> = (0..8u8).filter(|&r| rlist & (1 << r) != 0).collect();
    let total = regs.len() as i32;
    dynasm!(a
        ; mov eax, [rbx + ro(rb)]
        ; mov [rbx + SCRATCH], eax   // scratch = base
        ; add eax, total * 4
        ; mov [rbx + ro(rb)], eax    // writeback rb = base + total*4
    );
    for (i, &r) in regs.iter().enumerate() {
        let off = i as i32 * 4;
        if load {
            dynasm!(a
                ; mov rdi, rbx
                ; mov esi, [rbx + SCRATCH]
                ; add esi, off
                ; mov rax, QWORD jit_load32 as *const () as i64
                ; call rax
                ; mov [rbx + ro(r)], eax
            );
        } else {
            dynasm!(a ; mov rdi, rbx ; mov esi, [rbx + SCRATCH] ; add esi, off);
            if r == rb && i == 0 {
                dynasm!(a ; mov edx, [rbx + SCRATCH]); // lowest slot stores the original base
            } else {
                dynasm!(a ; mov edx, [rbx + ro(r)]);
            }
            dynasm!(a ; mov rax, QWORD jit_store32 as *const () as i64 ; call rax);
        }
    }
    dynasm!(a
        ; mov ecx, [rbx + FAULTED]
        ; test ecx, ecx
        ; jz >nofault
        ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32
        ; mov eax, exit::FAULT as i32
        ; jmp ->epilogue
        ; nofault:
    );
    if !load {
        dynasm!(a
            ; mov ecx, [rbx + SMC]
            ; test ecx, ecx
            ; jz >nosmc
            ; mov DWORD [rbx + PC], pc.wrapping_add(2) as i32
            ; mov eax, exit::SMC as i32
            ; jmp ->epilogue
            ; nosmc:
        );
    }
}

/// Emit `bx`/`blx` register: read rm (PC folded to a constant), switch ARM/Thumb
/// from bit 0, optionally set LR, and end the trace.
fn emit_bx(a: &mut Asm, link: bool, rm: u8, pc: u32) {
    if rm == 15 {
        dynasm!(a ; mov eax, pc.wrapping_add(4) as i32);
    } else {
        dynasm!(a ; mov eax, [rbx + ro(rm)]);
    }
    dynasm!(a
        ; mov ecx, eax
        ; and ecx, 1                 // new T bit
        // PC = vals & (new_t ? !1 : !3) = vals & (0xFFFFFFFC | new_t<<1)
        ; mov edx, ecx
        ; shl edx, 1
        ; or edx, -4
        ; and eax, edx
        ; mov [rbx + PC], eax
        ; mov edx, [rbx + CPSR]
        ; and edx, !(1 << 5)
        ; mov r8d, ecx
        ; shl r8d, 5
        ; or edx, r8d
        ; mov [rbx + CPSR], edx
    );
    if link {
        dynasm!(a ; mov DWORD [rbx + ro(14)], (pc.wrapping_add(2) | 1) as i32);
    }
    dynasm!(a ; mov eax, exit::CONTINUE as i32 ; jmp ->epilogue);
}

/// Emit `mov pc, rm` (`rm != PC`). Unlike `bx`, this does not interwork: the
/// target is `rm & !1` and the Thumb state is unchanged, so no CPSR write.
fn emit_mov_pc(a: &mut Asm, rm: u8) {
    dynasm!(a
        ; mov eax, [rbx + ro(rm)]
        ; and eax, !1
        ; mov [rbx + PC], eax
        ; mov eax, exit::CONTINUE as i32
        ; jmp ->epilogue
    );
}

/// Emit `bl`/`blx` immediate: set LR to the return address and PC to the
/// pre-computed target; BLX also clears the Thumb bit. Ends the trace.
fn emit_bl(a: &mut Asm, exchange: bool, target: u32, ret: u32) {
    dynasm!(a
        ; mov DWORD [rbx + ro(14)], ret as i32
        ; mov DWORD [rbx + PC], target as i32
    );
    if exchange {
        dynasm!(a ; and DWORD [rbx + CPSR], !(1 << 5));
    }
    dynasm!(a ; mov eax, exit::CONTINUE as i32 ; jmp ->epilogue);
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
        0xD => {
            // MUL: rd * rs (low 32 bits); N,Z from result, C forced to 0, V kept.
            dynasm!(a ; mov eax, [rbx + ro(rd)] ; imul eax, [rbx + ro(rs)] ; mov [rbx + ro(rd)], eax ; xor ecx, ecx);
            emit_flags_nzc(a);
        }
        0x2 | 0x3 | 0x4 | 0x7 => {
            // Shift-by-register (LSL/LSR/ASR/ROR): rd shifted by rs & 0xff. N,Z
            // from the result, C from the shifted-out bit, V preserved. The
            // interpreter's exact shift semantics (large amounts, ROR/RRX edges)
            // are reused via `jit_alu_shift` rather than re-encoded here.
            let shift_type = (((op >> 1) & 2) | (op & 1)) as i32;
            dynasm!(a
                ; mov edi, [rbx + ro(rd)]      // val
                ; mov esi, [rbx + ro(rs)]      // amount
                ; mov edx, shift_type          // shift type
                ; mov ecx, [rbx + CPSR]
                ; shr ecx, 29
                ; and ecx, 1                   // carry-in
                ; mov rax, QWORD jit_alu_shift as *const () as i64
                ; call rax
                ; mov edx, eax                 // res (low 32)
                ; shr rax, 32                  // carry (0/1)
                ; mov ecx, eax
                ; mov eax, edx
                ; mov [rbx + ro(rd)], eax
            );
            emit_flags_nzc(a);
        }
        _ => unreachable!(), // ADC/SBC
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
    addr(a); // sets esi = effective address
    // Inline fast path: a mapped, naturally aligned access reads the guest page
    // directly (host is little-endian, like the guest), skipping the helper call.
    // A null page (unmapped) or a misaligned access takes the slow path, whose
    // helper reproduces the fault / unaligned-rotate semantics exactly.
    dynasm!(a
        ; mov ecx, esi
        ; shr ecx, 16                     // 64 KiB page index
        ; mov rdx, [rbx + PAGES]
        ; mov rdx, [rdx + rcx*8]          // page base (null = unmapped)
        ; test rdx, rdx
        ; jz >slow
    );
    if size != 8 {
        let align = (size as i32 / 8) - 1; // 16->1, 32->3
        dynasm!(a ; test esi, align ; jnz >slow);
    }
    dynasm!(a ; mov ecx, esi ; and ecx, 0xffff); // in-page offset
    match (size, signed) {
        (8, true) => dynasm!(a ; movsx r8d, BYTE [rdx + rcx]),
        (8, false) => dynasm!(a ; movzx r8d, BYTE [rdx + rcx]),
        (16, true) => dynasm!(a ; movsx r8d, WORD [rdx + rcx]),
        (16, false) => dynasm!(a ; movzx r8d, WORD [rdx + rcx]),
        _ => dynasm!(a ; mov r8d, [rdx + rcx]),
    }
    dynasm!(a ; jmp >done);
    dynasm!(a
        ; slow:
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
    dynasm!(a ; done:);
    // Write the destination *before* checking for a fault: the interpreter's
    // load path stores the (dummy 0) value into rd even when the access faults,
    // then aborts, so we must too. The fast path never faults (page is mapped),
    // so it leaves FAULTED at 0 and skips the exit.
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
