//! ARM (A32) instruction frontend for the JIT.
//!
//! Where [`super::fast`] decodes Thumb, this decodes the ARM instruction set the
//! JIT compiles. The games run large stretches in ARM state, which the Thumb JIT
//! cannot touch, so those stretches were interpreted; this frontend lets the same
//! machine-code backends compile the common ARM subset too.
//!
//! Decoding follows `arm32_cpu`'s exact match order, because ARM encodings
//! overlap (e.g. an MRS/MSR word also matches the data-processing mask): a type
//! earlier in the order must win. Unsupported types return `None`, so the engine
//! falls back to one interpreter step for them, exactly as the Thumb path does.

/// A decoded ARM operation the backends can compile. Register fields index the
/// flat `[u32; 16]` guest file. `cond` is the 4-bit condition (0xE/0xF = always).
#[derive(Clone, Copy)]
pub(crate) enum ArmOp {
    /// Data processing (the 16 ALU ops). `rd == 15` is a computed branch and
    /// ends the trace. Never has PC as a source operand (those fall back).
    DataProc {
        cond: u8,
        opcode: u8,
        s: bool,
        rd: u8,
        rn: u8,
        op2: Op2,
    },
    /// Single word/byte load or store. `rn == 15` uses a constant PC base.
    LoadStore {
        cond: u8,
        load: bool,
        byte: bool,
        rd: u8,
        rn: u8,
        pre: bool,
        up: bool,
        wb: bool,
        base_pc: Option<u32>,
        offset: Off,
    },
    /// Block transfer (LDM/STM), S bit clear. A load including r15 ends the
    /// trace.
    Block {
        cond: u8,
        load: bool,
        rn: u8,
        pre: bool,
        up: bool,
        wb: bool,
        reglist: u16,
    },
    /// B / BL / BLX(immediate). `target`/`ret` pre-computed; `to_thumb` for BLX.
    Branch {
        cond: u8,
        target: u32,
        link: bool,
        ret: u32,
        to_thumb: bool,
    },
    /// BX register: interworking branch.
    BranchEx { cond: u8, rm: u8 },
}

/// Data-processing operand 2.
#[derive(Clone, Copy)]
pub(crate) enum Op2 {
    /// Fully constant immediate; `carry` is the shifter carry-out.
    Imm { val: u32, carry: u32 },
    /// Register `rm` shifted by an immediate amount.
    ShiftImm { rm: u8, ty: u8, amount: u8 },
    /// Register `rm` shifted by the low byte of register `rs`.
    ShiftReg { rm: u8, ty: u8, rs: u8 },
}

/// Load/store offset.
#[derive(Clone, Copy)]
pub(crate) enum Off {
    Imm(u32),
    ShiftImm { rm: u8, ty: u8, amount: u8 },
}

/// Whether a compiled op writes a non-linear PC and must end the trace.
pub(crate) fn arm_ends_trace(op: &ArmOp) -> bool {
    match op {
        ArmOp::Branch { .. } | ArmOp::BranchEx { .. } => true,
        ArmOp::DataProc { rd, .. } => *rd == 15,
        ArmOp::Block { load: true, reglist, .. } => reglist & (1 << 15) != 0,
        _ => false,
    }
}

#[inline]
fn bits(inst: u32, off: u32, len: u32) -> u32 {
    (inst >> off) & ((1 << len) - 1)
}

/// Decode one ARM word at `pc`, or `None` if the JIT does not compile it (the
/// caller then single-steps it in the interpreter).
pub(crate) fn decode_arm(inst: u32, pc: u32) -> Option<ArmOp> {
    let cond = bits(inst, 28, 4) as u8;
    let m = |mask: u32, test: u32| inst & mask == test;

    // Order mirrors arm32_cpu's INST_MATCH_ORDER. Unsupported types that appear
    // before a supported one must still be recognized here (returning None), or
    // their words would be mis-decoded as the later, wider pattern.

    // BranchEx (BX)
    if m(0x0fff_fff0, 0x012f_ff10) {
        return Some(ArmOp::BranchEx {
            cond,
            rm: (inst & 0xf) as u8,
        });
    }
    // Branch / BL / BLX(imm)
    if m(0x0e00_0000, 0x0a00_0000) {
        let offset = bits(inst, 0, 24);
        let s_offset = ((offset << 8) as i32 >> 6) as u32;
        if cond == 0xf {
            let h = bits(inst, 24, 1);
            let target = pc.wrapping_add(s_offset | (h << 1)).wrapping_add(8);
            return Some(ArmOp::Branch {
                cond: 0xe,
                target,
                link: true,
                ret: pc.wrapping_add(4),
                to_thumb: true,
            });
        }
        let l = bits(inst, 24, 1) != 0;
        return Some(ArmOp::Branch {
            cond,
            target: pc.wrapping_add(s_offset).wrapping_add(8),
            link: l,
            ret: pc.wrapping_add(4),
            to_thumb: false,
        });
    }
    if m(0x0fff_0ff0, 0x016f_0f10) {
        return None; // Clz
    }
    if m(0x0fb0_0ff0, 0x0100_0090) {
        return None; // Swap
    }
    if m(0x0fb0_0000, 0x0320_0000) {
        return None; // MSR immediate
    }
    if m(0x0f90_0ff0, 0x0100_0000) {
        return None; // MRS / MSR register (PSR)
    }
    // Data processing: DataProc0 (reg, shift-by-imm), DataProc1 (reg,
    // shift-by-reg), DataProc2 (immediate). The unconditional space (cond 0xF)
    // for these is the extension space, not data processing, so fall back.
    if m(0x0e00_0010, 0x0000_0000) || m(0x0e00_0090, 0x0000_0010) || m(0x0e00_0000, 0x0200_0000) {
        if cond == 0xf {
            return None;
        }
        return decode_data_proc(inst, pc);
    }
    if m(0x0fc0_00f0, 0x0000_0090) {
        return None; // Multiply
    }
    if m(0x0f80_00f0, 0x0080_0090) {
        return None; // MulLong
    }
    // Single data transfer (immediate or scaled-register offset).
    if m(0x0e00_0000, 0x0400_0000) || m(0x0e00_0010, 0x0600_0000) {
        if cond == 0xf {
            return None;
        }
        return decode_single_xfer(inst, pc);
    }
    if m(0x0e40_0f90, 0x0000_0090) {
        return None; // HwSgnXferR
    }
    if m(0x0e40_0090, 0x0040_0090) {
        return None; // HwSgnXferI
    }
    // Block data transfer (LDM/STM).
    if m(0x0e00_0000, 0x0800_0000) {
        if cond == 0xf {
            return None;
        }
        return decode_block(inst);
    }
    None // SWI, coprocessor, undefined
}

/// Applies an immediate ARM shift to a compile-time-constant value, returning
/// `(result, shifter_carry)`. Used to fold a PC operand (whose value is the
/// constant `pc + 8`) into an immediate. `RRX` (ROR #0) needs the live carry, so
/// it is reported as unfoldable.
fn shift_const(val: u32, ty: u8, amount: u8) -> Option<(u32, u32)> {
    Some(match ty {
        0 => {
            // LSL
            if amount == 0 {
                (val, 0)
            } else {
                (val << amount, (val >> (32 - amount)) & 1)
            }
        }
        1 => {
            // LSR (#0 encodes #32)
            if amount == 0 {
                (0, (val >> 31) & 1)
            } else {
                (val >> amount, (val >> (amount - 1)) & 1)
            }
        }
        2 => {
            // ASR (#0 encodes #32)
            if amount == 0 {
                let filled = ((val as i32) >> 31) as u32;
                (filled, (val >> 31) & 1)
            } else {
                (((val as i32) >> amount) as u32, (val >> (amount - 1)) & 1)
            }
        }
        _ => {
            // ROR; #0 is RRX (needs the live carry), so decline that one.
            if amount == 0 {
                return None;
            }
            (val.rotate_right(amount as u32), (val >> (amount - 1)) & 1)
        }
    })
}

fn decode_data_proc(inst: u32, pc: u32) -> Option<ArmOp> {
    let i = bits(inst, 25, 1);
    let r = bits(inst, 4, 1);
    let opcode = bits(inst, 21, 4) as u8;
    let s = bits(inst, 20, 1) != 0;
    let rn = bits(inst, 16, 4) as u8;
    let rd = bits(inst, 12, 4) as u8;

    // A flag-setting write to PC restores CPSR from SPSR (exception return):
    // banked and rare, so fall back.
    if rd == 15 && s {
        return None;
    }

    // The value r15 reads as, in ARM state, is this instruction's address + 8.
    let pc_val = pc.wrapping_add(8);

    let op2 = if i == 1 {
        // Rotated immediate: value and shifter carry are both constant.
        let rot = bits(inst, 8, 4) * 2;
        let imm = bits(inst, 0, 8);
        let val = imm.rotate_right(rot);
        let carry = if rot == 0 { 0 } else { (imm >> ((rot - 1) & 31)) & 1 };
        Op2::Imm { val, carry }
    } else {
        let rm = bits(inst, 0, 4) as u8;
        let ty = bits(inst, 5, 2) as u8;
        if r == 0 {
            let amount = bits(inst, 7, 5) as u8;
            if rm == 15 {
                // PC as the shifted operand is a compile-time constant - the
                // `mov lr, pc` / `add rd, rx, pc` call and address idioms. Fold
                // it to an immediate rather than fall back.
                let (val, carry) = shift_const(pc_val, ty, amount)?;
                Op2::Imm { val, carry }
            } else {
                Op2::ShiftImm { rm, ty, amount }
            }
        } else {
            let rs = bits(inst, 8, 4) as u8;
            if rm == 15 || rs == 15 {
                return None; // PC shifted by a register is not constant
            }
            Op2::ShiftReg { rm, ty, rs }
        }
    };

    // PC as the first ALU operand: its value is the constant `pc + 8`. When the
    // second operand is also constant, fold the whole instruction to a `mov rd,
    // #result` - the `add rd, pc, #imm` (ADR) address idiom and friends. MOV/MVN
    // ignore rn, so PC there is harmless. Other rn == 15 forms (register operand,
    // or a carry/flag-only op) stay a fall back.
    if rn == 15 {
        if opcode == 0xD || opcode == 0xF {
            // MOV/MVN ignore the first operand.
            return Some(ArmOp::DataProc {
                cond: bits(inst, 28, 4) as u8,
                opcode,
                s,
                rd,
                rn: 0,
                op2,
            });
        }
        if s {
            return None; // folding the flags of a constant op is not worth it
        }
        let Op2::Imm { val: b, .. } = op2 else {
            return None;
        };
        let a = pc_val;
        let result = match opcode {
            0x0 => a & b,             // AND
            0x1 => a ^ b,             // EOR
            0x2 => a.wrapping_sub(b), // SUB
            0x3 => b.wrapping_sub(a), // RSB
            0x4 => a.wrapping_add(b), // ADD
            0xC => a | b,             // ORR
            0xE => a & !b,            // BIC
            _ => return None,         // ADC/SBC/RSC (carry) and CMP/TST/etc.
        };
        return Some(ArmOp::DataProc {
            cond: bits(inst, 28, 4) as u8,
            opcode: 0xD, // MOV
            s: false,
            rd,
            rn: 0,
            op2: Op2::Imm { val: result, carry: 0 },
        });
    }

    Some(ArmOp::DataProc {
        cond: bits(inst, 28, 4) as u8,
        opcode,
        s,
        rd,
        rn,
        op2,
    })
}

fn decode_single_xfer(inst: u32, pc: u32) -> Option<ArmOp> {
    let i = bits(inst, 25, 1); // 1 = register offset
    let pre = bits(inst, 24, 1) != 0;
    let up = bits(inst, 23, 1) != 0;
    let byte = bits(inst, 22, 1) != 0;
    let wb = bits(inst, 21, 1) != 0;
    let load = bits(inst, 20, 1) != 0;
    let rn = bits(inst, 16, 4) as u8;
    let rd = bits(inst, 12, 4) as u8;

    // rd == PC (interworking load / pc+12 store) and writeback into PC: rare,
    // fall back.
    if rd == 15 {
        return None;
    }
    let base_pc = if rn == 15 {
        // A PC base only works without any writeback (post-index always writes
        // back, so require a pre-index with the W bit clear).
        if wb || !pre {
            return None;
        }
        Some(pc.wrapping_add(8))
    } else {
        None
    };

    let offset = if i == 0 {
        Off::Imm(bits(inst, 0, 12))
    } else {
        let rm = bits(inst, 0, 4) as u8;
        if rm == 15 {
            return None;
        }
        Off::ShiftImm {
            rm,
            ty: bits(inst, 5, 2) as u8,
            amount: bits(inst, 7, 5) as u8,
        }
    };

    Some(ArmOp::LoadStore {
        cond: bits(inst, 28, 4) as u8,
        load,
        byte,
        rd,
        rn,
        pre,
        up,
        wb,
        base_pc,
        offset,
    })
}

fn decode_block(inst: u32) -> Option<ArmOp> {
    let s = bits(inst, 22, 1) != 0;
    if s {
        return None; // user-bank / SPSR-restoring forms: fall back
    }
    let rn = bits(inst, 16, 4) as u8;
    if rn == 15 {
        return None;
    }
    Some(ArmOp::Block {
        cond: bits(inst, 28, 4) as u8,
        load: bits(inst, 20, 1) != 0,
        rn,
        pre: bits(inst, 24, 1) != 0,
        up: bits(inst, 23, 1) != 0,
        wb: bits(inst, 21, 1) != 0,
        reglist: bits(inst, 0, 16) as u16,
    })
}
