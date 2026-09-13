use core::mem;

use bytemuck::{Pod, Zeroable};

use wipi_types::wipic::WIPICWord;

use crate::{WIPICContext, method::ParamConverter};

/// A title's drawing state, laid out the way the reference lays it out.
///
/// A clet does not only pass this struct to the `MC_grp*` calls - one with its
/// own blitter reads the fields straight out of it, to clip and to pick up the
/// colours it draws with - so the layout is part of the ABI, not an internal
/// detail. `MC_grpSetContext` (@0x1aaba8) and `MC_grpGetContext` (@0x1a9e94)
/// give it exactly: every field one 32-bit word, the clip rectangle first, and
/// `MC_grpInitContext` (@0x1abc0c) fills the same 0x38 bytes.
///
/// The shape carried here before had a leading word the reference does not have
/// and packed the clip into 16-bit halves, which put every field from the
/// foreground colour on at the wrong offset - so a title reading its own
/// context back got another field's value, and a clet clipping its blits by
/// hand clipped against nonsense.
///
/// Op 3 (`TransPixelIdx`) has no field: the reference neither stores nor
/// reports it.
#[repr(C)]
#[derive(Default, Clone, Copy, Pod, Zeroable)]
pub struct WIPICGraphicsContext {
    /// Top-left x, y and bottom-right x, y - the corner stored decremented, as
    /// the reference stores it, and re-incremented when reported back.
    pub clip: [WIPICWord; 4],
    pub fgpxl: WIPICWord,
    pub bgpxl: WIPICWord,
    pub alpha: WIPICWord,
    /// `MC_GrpPixelOpProc`, which the reference also plants for XOR mode.
    pub pixel_op_func_ptr: WIPICWord,
    pub param1: WIPICWord,
    pub font: WIPICWord,
    pub style: WIPICWord,
    /// Whether XOR mode is on, which op 9 turns on and off.
    pub xor_mode: WIPICWord,
    /// x, y
    pub offset: [WIPICWord; 2],
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum WIPICGraphicsContextIdx {
    ClipIdx = 0,
    FgPixelIdx = 1,
    BgPixelIdx = 2,
    TransPixelIdx = 3,
    AlphaIdx = 4,
    PixelopIdx = 5,
    PixelParam1Idx = 6,
    FontIdx = 7,
    StyleIdx = 8,
    XorModeIdx = 9,
    OffsetIdx = 10,
    OutlineIdx = 11,

    /// Unknown values are mapped to this enum value.
    /// Note that this field doesn't exist in WIPI and the choice of this ordinal is arbitrary.
    Invalid = 0xff,
}

impl ParamConverter<WIPICGraphicsContextIdx> for WIPICGraphicsContextIdx {
    fn convert(_context: &mut dyn WIPICContext, raw: WIPICWord) -> WIPICGraphicsContextIdx {
        if raw >= (Self::ClipIdx as WIPICWord) && raw <= (Self::OutlineIdx as WIPICWord) {
            // SAFETY: WIPICGraphicsContextIdx has CWord repr and is unit only.
            let x: Self = unsafe { mem::transmute(raw) };
            x
        } else {
            Self::Invalid
        }
    }
}
