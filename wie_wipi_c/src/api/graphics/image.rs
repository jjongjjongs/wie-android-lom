use alloc::vec;
use alloc::vec::Vec;

use wie_backend::canvas::{PixelType, Rgb565Pixel, VecImageBuffer, decode_image};
use wie_util::Result;

use wipi_types::wipic::{WIPICImage, WIPICIndirectPtr, WIPICWord};

use crate::{api::graphics::framebuffer::FrameBuffer, context::WIPICContext};

pub fn create_wipi_image(context: &mut dyn WIPICContext, buf: WIPICIndirectPtr, offset: WIPICWord, len: WIPICWord) -> Result<WIPICImage> {
    let ptr_image_data = context.data_ptr(buf)?;

    let mut data = vec![0; len as _];
    context.read_bytes(ptr_image_data + offset, &mut data)?;
    let image = decode_image(&data)?;

    // A title that blits straight out of image memory reads it at the display's
    // own depth - 16bpp RGB565, the depth MC_grpGetFrameBpp reports - so the
    // colour plane is stored there. Handing back a 32bpp buffer to be read at
    // that stride is what turned HYBRID 1's sprites into noise. RGB565 cannot
    // carry the decoder's 8-bit alpha, so the full ARGB is kept in the mask
    // plane, from which MC_grpDrawImage composites transparency; that path is
    // unchanged.
    let width = image.width();
    let height = image.height();
    let colors = image.colors();
    let has_alpha = colors.iter().any(|color| color.a != 0xff);
    let raw: Vec<u16> = colors.iter().map(|color| Rgb565Pixel::from_color(*color)).collect();
    let rgb565 = VecImageBuffer::<Rgb565Pixel>::from_raw(width, height, raw);

    let img_framebuffer = FrameBuffer::from_image(context, &rgb565)?;
    // Only an image that carries alpha needs the full ARGB kept in the mask
    // plane for MC_grpDrawImage to composite; a fully opaque image composites
    // straight from the 16bpp colour plane and is spared the second copy.
    let mask_framebuffer = if has_alpha {
        FrameBuffer::from_image(context, &*image)?
    } else {
        FrameBuffer::empty()
    };

    Ok(WIPICImage {
        img: img_framebuffer.0,
        mask: mask_framebuffer.0,
        loop_count: 0,
        delay: 0,
        animated: 0,
        buf,
        offset,
        current: 0,
        len,
    })
}
