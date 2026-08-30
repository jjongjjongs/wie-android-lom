use wie_util::{Result, WieError};

pub(crate) const R_ARM_NONE: u32 = 0;
pub(crate) const R_ARM_PC24: u32 = 1;
pub(crate) const R_ARM_ABS32: u32 = 2;
pub(crate) const R_ARM_REL32: u32 = 3;
pub(crate) const R_ARM_THM_CALL: u32 = 10;
pub(crate) const R_ARM_CALL: u32 = 28;
pub(crate) const R_ARM_JUMP24: u32 = 29;
pub(crate) const R_ARM_THM_JUMP24: u32 = 30;

// Legacy Raptor ER private relocation numbers recovered from the reference
// loader. These occupy the processor-specific high end of the ELF32 r_type
// byte and are not part of the standard ARM ELF ABI.
pub(crate) const R_ARM_RREL32: u32 = 252;
pub(crate) const R_ARM_RABS32: u32 = 253;
pub(crate) const R_ARM_RPC24: u32 = 254;
pub(crate) const R_ARM_RBASE: u32 = 255;

pub(crate) fn arm_abs32(addend: u32, symbol: u32) -> u32 {
    symbol.wrapping_add(addend)
}

pub(crate) fn arm_rel32(place: u32, addend: u32, symbol: u32) -> u32 {
    symbol.wrapping_add(addend).wrapping_sub(place)
}

pub(crate) fn arm_pc24(instruction: u32, place: u32, symbol: u32, addend: i32) -> Result<u32> {
    let target = symbol.wrapping_add_signed(addend);
    let displacement = target.wrapping_sub(place.wrapping_add(8)) as i32;
    if displacement & 3 != 0 {
        return Err(WieError::FatalError(alloc::format!(
            "unaligned ARM branch relocation: place={place:#x}, target={target:#x}"
        )));
    }
    if !(-0x0200_0000..=0x01ff_fffc).contains(&displacement) {
        return Err(WieError::FatalError(alloc::format!(
            "ARM branch relocation out of range: place={place:#x}, target={target:#x}"
        )));
    }

    Ok((instruction & 0xff00_0000) | (((displacement >> 2) as u32) & 0x00ff_ffff))
}

/// Apply Raptor ER's private absolute relocation. `target_bias` is the
/// difference between the selected segment's loaded and linked addresses.
pub(crate) fn raptor_rabs32(addend: u32, target_bias: i32) -> u32 {
    addend.wrapping_add_signed(target_bias)
}

/// Apply Raptor ER's private relative relocation.
pub(crate) fn raptor_rrel32(addend: u32, target_bias: i32, place_bias: i32) -> u32 {
    addend.wrapping_add_signed(target_bias.wrapping_sub(place_bias))
}

/// Rebase an existing ARM B/BL instruction used by R_ARM_RPC24.
///
/// Unlike standard R_ARM_PC24, the addend is already encoded in the branch.
/// Raptor ER therefore adjusts only the difference between target and place
/// segment load biases.
pub(crate) fn raptor_rpc24(instruction: u32, target_bias: i32, place_bias: i32) -> Result<u32> {
    if instruction & 0x0e00_0000 != 0x0a00_0000 {
        return Err(WieError::FatalError(alloc::format!(
            "R_ARM_RPC24 is not an ARM branch: {instruction:#010x}"
        )));
    }

    let imm24 = (instruction & 0x00ff_ffff) as i32;
    let displacement = (imm24 << 8) >> 6;
    let adjusted = displacement.wrapping_add(target_bias.wrapping_sub(place_bias));
    if adjusted & 3 != 0 {
        return Err(WieError::FatalError(alloc::format!("unaligned R_ARM_RPC24 displacement: {adjusted}")));
    }
    if !(-0x0200_0000..=0x01ff_fffc).contains(&adjusted) {
        return Err(WieError::FatalError(alloc::format!("R_ARM_RPC24 target is out of range: {adjusted}")));
    }

    Ok((instruction & 0xff00_0000) | (((adjusted >> 2) as u32) & 0x00ff_ffff))
}

/// Apply the classic ARM ELF R_ARM_THM_CALL/R_ARM_THM_PC22 encoding used by
/// legacy LGT modules. `upper` and `lower` are the two Thumb halfwords in
/// memory order.
pub(crate) fn thumb_pc22(upper: u16, lower: u16, place: u32, symbol: u32, addend: i32) -> Result<(u16, u16)> {
    let target = symbol.wrapping_add_signed(addend);
    let displacement = target.wrapping_sub(place.wrapping_add(4)) as i32;
    if displacement & 1 != 0 {
        return Err(WieError::FatalError(alloc::format!(
            "unaligned Thumb branch relocation: place={place:#x}, target={target:#x}"
        )));
    }
    if !(-0x0040_0000..=0x003f_fffe).contains(&displacement) {
        return Err(WieError::FatalError(alloc::format!(
            "Thumb branch relocation out of range: place={place:#x}, target={target:#x}"
        )));
    }

    let encoded = displacement as u32;
    let new_upper = (upper & 0xf800) | ((encoded >> 12) as u16 & 0x07ff);
    let new_lower = (lower & 0xf800) | ((encoded >> 1) as u16 & 0x07ff);
    Ok((new_upper, new_lower))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_and_rel_are_wrapping_arm32_values() {
        assert_eq!(arm_abs32(0xffff_fff0, 0x20), 0x10);
        assert_eq!(arm_rel32(0x1000, 4, 0x2000), 0x1004);
    }

    #[test]
    fn arm_pc24_preserves_opcode_and_encodes_branch() {
        let result = arm_pc24(0xeb00_0000, 0x1000, 0x2000, 0).unwrap();
        assert_eq!(result & 0xff00_0000, 0xeb00_0000);
        assert_eq!(result & 0x00ff_ffff, (0xff8u32 >> 2) & 0x00ff_ffff);
    }

    #[test]
    fn thumb_pc22_rejects_unaligned_target() {
        assert!(thumb_pc22(0xf000, 0xf800, 0x1000, 0x2001, 0).is_err());
    }

    #[test]
    fn raptor_er_abs_and_rel_apply_segment_biases() {
        assert_eq!(raptor_rabs32(0x1000, 0x200), 0x1200);
        assert_eq!(raptor_rrel32(0x1000, 0x300, 0x100), 0x1200);
    }

    #[test]
    fn raptor_er_pc24_preserves_branch_opcode() {
        let result = raptor_rpc24(0xeb00_0001, 0x100, 0).unwrap();
        assert_eq!(result & 0xff00_0000, 0xeb00_0000);
        assert_eq!(result & 0x00ff_ffff, 0x41);
    }

    #[test]
    fn raptor_er_pc24_rejects_non_branch_instruction() {
        assert!(raptor_rpc24(0xe1a0_0000, 0, 0).is_err());
    }
}
