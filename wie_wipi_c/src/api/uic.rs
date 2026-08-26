use alloc::{boxed::Box, vec::Vec};

use wie_util::{Result, WieError, read_generic, read_null_terminated_string_bytes, write_generic};

use wipi_types::wipic::{WIPICIndirectPtr, WIPICWord};

use crate::{
    WIPICResult,
    api::{graphics, kernel},
    context::WIPICContext,
    method::MethodBody,
};

const UIC_DRAW_MARKER_BASE: WIPICWord = 0xffff_f100;
const UIC_TIMER_MARKER_TIME: WIPICWord = 0xffff_f201;
const UIC_TIMER_MARKER_TEXT: WIPICWord = 0xffff_f202;

fn uic_draw_marker(component_type: WIPICWord) -> WIPICWord {
    UIC_DRAW_MARKER_BASE + component_type
}

async fn uic_dispatch_draw(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    graphics_context: WIPICWord,
) -> Result<()> {
    let draw: WIPICWord = read_generic(context, component + 0x24)?;
    if (UIC_DRAW_MARKER_BASE + 1..=UIC_DRAW_MARKER_BASE + 5).contains(&draw) {
        // MC_uicCreate stores provider-private WPUic_Draw* function pointers here.
        // Generic WIE cannot execute those LGT .so addresses, so native-created
        // components use an internal marker until those private draw routines are
        // ported independently. Application-installed callbacks still execute.
        return Ok(());
    }

    if draw != 0 {
        context
            .call_function(draw, &[component, graphics_context])
            .await?;
    }

    Ok(())
}

fn uic_schedule_component_timer(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    component_type: WIPICWord,
    delay: u64,
) -> Result<()> {
    struct UicTimerCallback {
        component: WIPICWord,
        component_type: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for UicTimerCallback {
        async fn call(
            &self,
            context: &mut dyn WIPICContext,
            _: Box<[WIPICWord]>,
        ) -> Result<WIPICResult> {
            match self.component_type {
                2 => uic_datetime_timer_callback(context, self.component).await?,
                3 => uic_text_timer_callback(context, self.component).await?,
                _ => {}
            }

            Ok(WIPICResult { results: Vec::new() })
        }
    }

    let timer = match component_type {
        2 => component + 0x98,
        3 => component + 0x54,
        _ => return Ok(()),
    };
    let due = context.system().platform().now() + delay;
    context.set_timer(
        timer,
        due,
        Box::new(UicTimerCallback {
            component,
            component_type,
        }),
    );

    Ok(())
}

/// LGT `MC_uicCreateApplicationContext` (WIPI-C service 0x320).
///
/// Native is a two-instruction constant return and always yields NULL.
pub async fn create_application_context(_context: &mut dyn WIPICContext) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_uicCreateApplicationContext");

    Ok(WIPICIndirectPtr(0))
}

/// LGT `MC_uicGetClass` (WIPI-C service 0x321).
///
/// Native returns component class ids 1..=5 for the five exact class names
/// and -1 for NULL or an unknown class name.
pub async fn get_class(
    context: &mut dyn WIPICContext,
    psz: WIPICWord,
) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_uicGetClass({psz:#x})");

    if psz == 0 {
        return Ok(WIPICIndirectPtr(u32::MAX));
    }

    let name = read_null_terminated_string_bytes(context, psz)?;
    let class = match name.as_slice() {
        b"MenuComponent" => 1,
        b"DateTimeComponent" => 2,
        b"TextComponent" => 3,
        b"LabelComponent" => 4,
        b"ListComponent" => 5,
        _ => u32::MAX,
    };

    Ok(WIPICIndirectPtr(class))
}

/// LGT `MC_uicCreate` (WIPI-C service 0x322).
///
/// The provider ignores `pac`, accepts class ids 1..=5, allocates a zeroed
/// type-specific object, copies the default graphics-context fields into the
/// common header, and then initializes the subtype state. Invalid classes
/// return -1; allocation failure returns -17.
pub async fn create(
    context: &mut dyn WIPICContext,
    pac: WIPICWord,
    cls: WIPICWord,
) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_uicCreate({pac:#x}, {cls:#x})");

    let size = match cls {
        1 | 5 => 92,
        2 => 168,
        3 => 108,
        4 => 76,
        _ => return Ok(WIPICIndirectPtr(u32::MAX)),
    };

    let memory = match context.alloc(size) {
        Ok(memory) => memory,
        Err(_) => return Ok(WIPICIndirectPtr((-17i32) as u32)),
    };
    let component = context.data_ptr(memory)?;
    context.write_bytes(component, &alloc::vec![0; size as usize])?;

    let font = graphics::get_font(context, 0, 0, 0).await? as u32;

    write_generic(context, component, cls)?;
    write_generic(context, component + 0x04, 0u32)?;
    write_generic(context, component + 0x08, 0u32)?;
    write_generic(context, component + 0x0c, 0x7fffu32)?;
    write_generic(context, component + 0x10, 0x7fffu32)?;
    write_generic(context, component + 0x14, font)?;
    write_generic(context, component + 0x18, 0u32)?;
    write_generic(context, component + 0x1c, 0x00ff_ffffu32)?;
    write_generic(context, component + 0x20, 0u32)?;
    write_generic(context, component + 0x24, uic_draw_marker(cls))?;
    write_generic(context, component + 0x28, 0u32)?;
    write_generic(context, component + 0x2c, 0u32)?;
    write_generic(context, component + 0x30, 0u32)?;
    write_generic(context, component + 0x34, 0u32)?;
    write_generic(context, component + 0x38, 0u32)?;
    write_generic(context, component + 0x3c, 0u32)?;
    write_generic(context, component + 0x40, 0u32)?;

    match cls {
        1 | 5 => {
            write_generic(context, component + 0x44, 0u32)?;
            write_generic(context, component + 0x48, -1i32)?;
            write_generic(context, component + 0x4c, 0u32)?;
            write_generic(context, component + 0x50, 0u32)?;
            write_generic(context, component + 0x54, 0u32)?;
            write_generic(context, component + 0x58, 0u32)?;
        }
        2 => {
            write_generic(context, component + 0x44, 3u32)?;
            write_generic(context, component + 0x94, 0u32)?;
            write_generic(context, component + 0x98, UIC_TIMER_MARKER_TIME)?;
            write_generic(context, component + 0x9c, 1u32)?;
            write_generic(context, component + 0xa0, 0u32)?;
            write_generic(context, component + 0xa4, 0u32)?;

            let fields =
                uic_kst_tm_from_epoch_millis(context.system().platform().now().raw());
            for (index, value) in fields.iter().enumerate() {
                write_generic(context, component + 0x48 + (index as u32) * 4, *value)?;
            }

            let formatted = uic_format_datetime(3, &fields);
            context.write_bytes(component + 0x74, &formatted)?;
            context.write_bytes(component + 0x74 + formatted.len() as u32, &[0])?;
        }
        3 => {
            let text = match context.alloc_raw(256) {
                Ok(address) => address,
                Err(_) => {
                    context.free(memory)?;
                    return Ok(WIPICIndirectPtr((-17i32) as u32));
                }
            };
            context.write_bytes(text, &alloc::vec![0; 256])?;

            write_generic(context, component + 0x44, text)?;
            write_generic(context, component + 0x48, 256u32)?;
            write_generic(context, component + 0x4c, 0u32)?;
            write_generic(context, component + 0x50, 0u32)?;
            write_generic(context, component + 0x54, UIC_TIMER_MARKER_TEXT)?;
            write_generic(context, component + 0x58, 1u32)?;
            write_generic(context, component + 0x5c, 0u32)?;
            write_generic(context, component + 0x60, 0u32)?;
            write_generic(context, component + 0x64, 0u32)?;
            write_generic(context, component + 0x68, 0u32)?;
        }
        4 => {
            write_generic(context, component + 0x44, 0u32)?;
            write_generic(context, component + 0x48, 2u32)?;
        }
        _ => unreachable!(),
    }

    Ok(memory)
}

/// LGT `MC_uicDestroy` (WIPI-C service 0x323).
///
/// Native ignores NULL/invalid components. For a valid component it first invokes
/// the optional destroy callback at +0x2c as `(component, 0, +0x38 context)`,
/// releases subtype-owned resources, clears the class word, then frees the
/// component itself.
///
/// Menu/List item storage is a raw pointer table at +0x50. Each table entry points
/// to a raw block laid out as `[image: u32][NUL-terminated label bytes]`.
pub async fn destroy(context: &mut dyn WIPICContext, component: WIPICWord) -> Result<()> {
    tracing::debug!("MC_uicDestroy({component:#x})");

    if component == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(());
    }

    let callback: WIPICWord = read_generic(context, component + 0x2c)?;
    if callback != 0 {
        let callback_context: WIPICWord = read_generic(context, component + 0x38)?;
        context
            .call_function(callback, &[component, 0, callback_context])
            .await?;
    }

    match component_type {
        1 | 5 => {
            let count: u32 = read_generic(context, component + 0x44)?;
            let table: WIPICWord = read_generic(context, component + 0x50)?;

            if table != 0 {
                for index in 0..count {
                    let item: WIPICWord = read_generic(context, table + index * 4)?;
                    if item == 0 {
                        continue;
                    }

                    let image: WIPICWord = read_generic(context, item)?;
                    if image != 0 {
                        graphics::destroy_image(context, WIPICIndirectPtr(image)).await?;
                    }

                    let label_len = uic_read_c_string(context, item + 4)?.len() as u32;
                    context.free_raw(item, label_len + 5)?;
                }

                context.free_raw_unsized(table)?;
            }
        }
        2 => {
            let enabled: u32 = read_generic(context, component + 0x20)?;
            if enabled != 0 {
                kernel::unset_timer(context, component + 0x98).await?;
            }
        }
        3 => {
            let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
            let capacity: u32 = read_generic(context, component + 0x48)?;
            if text_ptr != 0 && capacity != 0 {
                context.free_raw(text_ptr, capacity)?;
            }

            let enabled: u32 = read_generic(context, component + 0x20)?;
            if enabled != 0 {
                kernel::unset_timer(context, component + 0x54).await?;
            }
        }
        4 => {
            let label: WIPICWord = read_generic(context, component + 0x44)?;
            if label != 0 {
                context.free_raw_unsized(label)?;
            }
        }
        _ => unreachable!(),
    }

    write_generic(context, component, 0u32)?;
    context.free(WIPICIndirectPtr(component))?;

    Ok(())
}

fn uic_repaint_rect(
    component_x: i32,
    component_y: i32,
    component_width: i32,
    component_height: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> (i32, i32, i32, i32) {
    let width = if width == -1 { component_width } else { width };
    let height = if height == -1 { component_height } else { height };

    let left = x.wrapping_add(component_x);
    let top = y.wrapping_add(component_y);
    let right = left.wrapping_sub(1).wrapping_add(width);
    let bottom = top.wrapping_sub(1).wrapping_add(height);

    (left, top, right, bottom)
}

/// LGT `MC_uicRepaint` (WIPI-C service 0x324).
///
/// Native silently ignores NULL/invalid components. For a valid component,
/// width/height values of -1 select the component's own dimensions. The
/// requested offset is translated by the component origin and converted to an
/// inclusive rectangle before tail-calling `MC_grpRepaint(1, left, top, right,
/// bottom)`.
pub async fn repaint(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<()> {
    tracing::debug!("MC_uicRepaint({component:#x}, {x}, {y}, {width}, {height})");

    if component == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(());
    }

    let component_x: i32 = read_generic(context, component + 0x04)?;
    let component_y: i32 = read_generic(context, component + 0x08)?;
    let component_width: i32 = read_generic(context, component + 0x0c)?;
    let component_height: i32 = read_generic(context, component + 0x10)?;

    let (left, top, right, bottom) = uic_repaint_rect(
        component_x,
        component_y,
        component_width,
        component_height,
        x,
        y,
        width,
        height,
    );

    graphics::repaint(context, 1, left, top, right, bottom).await
}

/// LGT/KTF `MC_uicConfigure`.
///
/// Native flags use bit 0 for position and bit 1 for dimensions.
/// Position is written unconditionally when selected. Dimensions are
/// written only when both width and height are positive.
pub async fn configure(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    flags: WIPICWord,
) -> Result<()> {
    tracing::debug!(
        "MC_uicConfigure({component:#x}, {x}, {y}, {width}, {height}, {flags:#x})"
    );

    if component == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(());
    }

    if flags & 1 != 0 {
        write_generic(context, component + 0x04, x)?;
        write_generic(context, component + 0x08, y)?;
    }

    if flags & 2 != 0 && width > 0 && height > 0 {
        write_generic(context, component + 0x0c, width)?;
        write_generic(context, component + 0x10, height)?;
    }

    Ok(())
}

/// LGT `MC_uicGetGeometry` (WIPI-C service 0x32a).
///
/// Native validates the component, then independently writes x, y, width,
/// and height to each non-NULL output pointer. NULL output pointers are
/// ignored, and invalid/NULL components leave all outputs untouched.
pub async fn get_geometry(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    x: WIPICWord,
    y: WIPICWord,
    width: WIPICWord,
    height: WIPICWord,
) -> Result<()> {
    tracing::debug!(
        "MC_uicGetGeometry({component:#x}, {x:#x}, {y:#x}, {width:#x}, {height:#x})"
    );

    if component == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(());
    }

    if x != 0 {
        let value: i32 = read_generic(context, component + 0x04)?;
        write_generic(context, x, value)?;
    }
    if y != 0 {
        let value: i32 = read_generic(context, component + 0x08)?;
        write_generic(context, y, value)?;
    }
    if width != 0 {
        let value: i32 = read_generic(context, component + 0x0c)?;
        write_generic(context, width, value)?;
    }
    if height != 0 {
        let value: i32 = read_generic(context, component + 0x10)?;
        write_generic(context, height, value)?;
    }

    Ok(())
}

/// LGT `MC_uicSetCallback` (WIPI-C service 0x32c).
///
/// Native selectors 1..=3 address the three common callback/context pairs.
/// Selector 4 is subtype-specific: Menu/List use +0x54/+0x58, DateTime uses
/// +0xa0/+0xa4, and Text uses +0x5c/+0x64. Selector 5 exists only for Text
/// and uses +0x60/+0x68. The previous callback pointer is returned; invalid
/// components, selectors, and unsupported subtype/selector pairs return 0
/// without modifying the component.
pub async fn set_callback(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    selector: WIPICWord,
    callback: WIPICWord,
    callback_context: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!(
        "MC_uicSetCallback({component:#x}, {selector}, {callback:#x}, {callback_context:#x})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    let (callback_offset, context_offset) = match selector {
        1 => (0x2c, 0x38),
        2 => (0x30, 0x3c),
        3 => (0x34, 0x40),
        4 => match component_type {
            1 | 5 => (0x54, 0x58),
            2 => (0xa0, 0xa4),
            3 => (0x5c, 0x64),
            _ => return Ok(0),
        },
        5 if component_type == 3 => (0x60, 0x68),
        _ => return Ok(0),
    };

    let previous: WIPICWord = read_generic(context, component + callback_offset)?;
    write_generic(context, component + context_offset, callback_context)?;
    write_generic(context, component + callback_offset, callback)?;

    Ok(previous)
}

/// LGT `MC_uicSetEventHandler` (WIPI-C service 0x32d).
///
/// Native validates the component, returns the previous handler from +0x28,
/// and stores the new handler there. NULL or invalid components return 0 and
/// leave memory untouched.
pub async fn set_event_handler(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    handler: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_uicSetEventHandler({component:#x}, {handler:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    let previous: WIPICWord = read_generic(context, component + 0x28)?;
    write_generic(context, component + 0x28, handler)?;

    Ok(previous)
}

/// LGT `MC_uicSetFont` (WIPI-C service 0x32e).
///
/// Native validates the component, returns the previous font value from +0x14,
/// and stores the new value there. NULL or invalid components return 0 and
/// leave memory untouched.
pub async fn set_font(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    font: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_uicSetFont({component:#x}, {font:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    let previous: WIPICWord = read_generic(context, component + 0x14)?;
    write_generic(context, component + 0x14, font)?;

    Ok(previous)
}

/// LGT `MC_uicGetFont` (WIPI-C service 0x32f).
///
/// Native validates the component and returns the font value stored at +0x14.
/// NULL or invalid components return 0.
pub async fn get_font(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_uicGetFont({component:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    read_generic(context, component + 0x14)
}

fn uic_color_to_rgb565(color: WIPICWord) -> WIPICWord {
    ((color >> 8) & 0xf800) | ((color >> 5) & 0x07e0) | ((color >> 3) & 0x001f)
}

/// LGT `MC_uicSetFgColor` (WIPI-C service 0x330).
///
/// Native validates the component, converts the WIPI 0xRRGGBB color through
/// the active display's color-to-pixel operation, stores the resulting RGB565
/// pixel at +0x18, and returns that pixel value. NULL or invalid components
/// return 0 and leave memory untouched.
pub async fn set_fg_color(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    color: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_uicSetFgColor({component:#x}, {color:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    let pixel = uic_color_to_rgb565(color);
    write_generic(context, component + 0x18, pixel)?;

    Ok(pixel)
}

/// LGT `MC_uicSetBgColor` (WIPI-C service 0x331).
///
/// Native validates the component, converts the WIPI 0xRRGGBB color through
/// the active display's color-to-pixel operation, stores the resulting RGB565
/// pixel at +0x1c, and returns that pixel value. NULL or invalid components
/// return 0 and leave memory untouched.
pub async fn set_bg_color(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    color: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_uicSetBgColor({component:#x}, {color:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    let pixel = uic_color_to_rgb565(color);
    write_generic(context, component + 0x1c, pixel)?;

    Ok(pixel)
}

/// LGT `MC_uicSetLabel` (WIPI-C service 0x332).
///
/// Native first validates the component. For a valid component, NULL labels
/// and non-Label component types return the validator's success value (1)
/// without changing memory. LabelComponent stores a private NUL-terminated
/// copy at +0x44.
///
/// Native `dmemory_realloc` keeps an existing allocation when the new
/// `strlen(label)+1` request fits its actual allocator block capacity.
/// WIE mirrors that using `raw_alloc_size`; growth allocates a replacement
/// before releasing the old block. After copying `strlen+1` bytes, native
/// also writes one extra NUL byte immediately after the requested region.
pub async fn set_label(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    label: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_uicSetLabel({component:#x}, {label:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    if label == 0 || component_type != 4 {
        return Ok(1);
    }

    let label_bytes = uic_read_c_string(context, label)?;
    let request_size = label_bytes.len() as u32 + 1;
    let old_label: WIPICWord = read_generic(context, component + 0x44)?;

    let new_label = if old_label != 0 && request_size <= context.raw_alloc_size(old_label)? {
        old_label
    } else {
        let address = match context.alloc_raw(request_size) {
            Ok(address) => address,
            Err(WieError::AllocationFailure) => return Ok(0),
            Err(error) => return Err(error),
        };

        if old_label != 0 {
            context.free_raw_unsized(old_label)?;
        }

        write_generic(context, component + 0x44, address)?;
        address
    };

    context.write_bytes(new_label, &label_bytes)?;
    context.write_bytes(new_label + label_bytes.len() as u32, &[0])?;
    context.write_bytes(new_label + request_size, &[0])?;

    Ok(new_label)
}

/// LGT `MC_uicGetLabel` (WIPI-C service 0x333).
///
/// Native validates the component and accepts only LabelComponent (type 4).
/// Invalid/NULL and non-Label components return NULL. A non-NULL +0x44 label
/// pointer is returned unchanged. When +0x44 is NULL, native returns a
/// provider-static empty string; WIE mirrors that stable pointer identity in
/// reserved guest global-data memory.
pub async fn get_label(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_uicGetLabel({component:#x})");

    if component == 0 {
        return Ok(WIPICIndirectPtr(0));
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if component_type != 4 {
        return Ok(WIPICIndirectPtr(0));
    }

    let label: WIPICWord = read_generic(context, component + 0x44)?;
    if label != 0 {
        return Ok(WIPICIndirectPtr(label));
    }

    context.write_bytes(UIC_EMPTY_LABEL, &[0])?;
    Ok(WIPICIndirectPtr(UIC_EMPTY_LABEL))
}

/// LGT `MC_uicSetLabelAlignment` (WIPI-C service 0x334).
///
/// Native validates the component first. NULL/invalid components return 0.
/// Valid non-Label components and alignment values outside unsigned 0..=2
/// return -9 without modifying memory. For LabelComponent, the previous
/// alignment at +0x48 is returned and the new value is stored there.
pub async fn set_label_alignment(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    alignment: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicSetLabelAlignment({component:#x}, {alignment:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    if component_type != 4 || alignment > 2 {
        return Ok(-9);
    }

    let old_alignment: i32 = read_generic(context, component + 0x48)?;
    write_generic(context, component + 0x48, alignment)?;

    Ok(old_alignment)
}

/// LGT `MC_uicSetTimeMask` (WIPI-C service 0x335).
///
/// Native validates the component first. NULL/invalid components return 0,
/// while valid non-DateTime components return -9. The mask is rejected with
/// -9 when either of its low two bits is set (`mask & 3 != 0`).
///
/// On success, native returns the previous +0x44 mask, stores the new mask,
/// immediately regenerates the DateTime text at +0x74 through
/// `WPUic_SetTimeStr`, and invokes the +0xa0 callback only when the rendered
/// text actually changes.
pub async fn set_time_mask(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    mask: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicSetTimeMask({component:#x}, {mask:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 2 || mask & 3 != 0 {
        return Ok(-9);
    }

    let old_mask: i32 = read_generic(context, component + 0x44)?;
    let old_text = uic_read_c_string(context, component + 0x74)?;

    write_generic(context, component + 0x44, mask)?;

    let mut fields = [0i32; 9];
    for (index, field) in fields.iter_mut().enumerate() {
        *field = read_generic(context, component + 0x48 + index as u32 * 4)?;
    }

    let formatted = uic_format_datetime(mask, &fields);
    context.write_bytes(component + 0x74, &formatted)?;
    context.write_bytes(component + 0x74 + formatted.len() as u32, &[0])?;

    if formatted != old_text {
        let callback: WIPICWord = read_generic(context, component + 0xa0)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0xa4)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(old_mask)
}

/// LGT `MC_uicGetTime` (WIPI-C service 0x338).
///
/// Native first validates the component. NULL/invalid components return 0.
/// For a valid component, a NULL output pointer or a non-DateTime component
/// returns the validator success value 1 without writing output.
///
/// For DateTimeComponent, native copies exactly 44 bytes from component +0x48
/// to the caller-provided tm buffer, then calls `WPUic_SetTimeStr`. The getter
/// therefore also regenerates the component text and invokes +0xa0 only when
/// the rendered string changes. Its final return follows the same
/// strcmp/callback contract as `MC_uicSetTime`.
pub async fn get_time(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    tm: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicGetTime({component:#x}, {tm:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    if tm == 0 || component_type != 2 {
        return Ok(1);
    }

    let mut tm_bytes = [0u8; 44];
    context.read_bytes(component + 0x48, &mut tm_bytes)?;
    context.write_bytes(tm, &tm_bytes)?;

    let old_text = uic_read_c_string(context, component + 0x74)?;
    let mask: WIPICWord = read_generic(context, component + 0x44)?;

    let mut fields = [0i32; 9];
    for (index, field) in fields.iter_mut().enumerate() {
        *field = read_generic(context, component + 0x48 + index as u32 * 4)?;
    }

    let formatted = uic_format_datetime(mask, &fields);
    context.write_bytes(component + 0x74, &formatted)?;
    context.write_bytes(component + 0x74 + formatted.len() as u32, &[0])?;

    let compare = match old_text.as_slice().cmp(formatted.as_slice()) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    };

    if compare != 0 {
        let callback: WIPICWord = read_generic(context, component + 0xa0)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0xa4)?;
            return Ok(
                context
                    .call_function(callback, &[component, 0, callback_context])
                    .await? as i32,
            );
        }
    }

    Ok(compare)
}

/// LGT `MC_uicSetTime` (WIPI-C service 0x336).
///
/// Native first runs `WPUic_CheckValidComp`. NULL/invalid components therefore
/// return 0. For an otherwise valid component, a NULL `tm` pointer or a
/// non-DateTime component returns the validator success value 1 without
/// modifying component memory.
///
/// A valid DateTimeComponent copies exactly 44 bytes from the caller's tm
/// structure to +0x48, then tail-calls `WPUic_SetTimeStr`. The rendered string
/// is selected by the current +0x44 mask. `WPUic_SetTimeStr` compares the old
/// and new strings, invokes +0xa0(component, 0, +0xa4) only when they differ,
/// and leaves that callback result in r0. Without a callback, WIE mirrors the
/// native strcmp result as -1/0/1.
pub async fn set_time(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    tm: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicSetTime({component:#x}, {tm:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    if tm == 0 || component_type != 2 {
        return Ok(1);
    }

    let mut tm_bytes = [0u8; 44];
    context.read_bytes(tm, &mut tm_bytes)?;
    context.write_bytes(component + 0x48, &tm_bytes)?;

    let old_text = uic_read_c_string(context, component + 0x74)?;
    let mask: WIPICWord = read_generic(context, component + 0x44)?;

    let mut fields = [0i32; 11];
    for (index, field) in fields.iter_mut().enumerate() {
        *field = read_generic(context, component + 0x48 + index as u32 * 4)?;
    }

    let format_fields: [i32; 9] = fields[..9].try_into().unwrap();
    let formatted = uic_format_datetime(mask, &format_fields);
    context.write_bytes(component + 0x74, &formatted)?;
    context.write_bytes(component + 0x74 + formatted.len() as u32, &[0])?;

    let compare = match old_text.as_slice().cmp(formatted.as_slice()) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    };

    if compare != 0 {
        let callback: WIPICWord = read_generic(context, component + 0xa0)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0xa4)?;
            return Ok(
                context
                    .call_function(callback, &[component, 0, callback_context])
                    .await? as i32,
            );
        }
    }

    Ok(compare)
}

/// LGT `MC_uicSetTimeLong` (WIPI-C service 0x337).
///
/// Native validates the component and returns 0 for NULL/invalid components.
/// A valid non-DateTime component returns the validator success value 1.
///
/// For DateTimeComponent, the second argument is stored as a 32-bit `time_t`.
/// `dlib_localtime` reads it as signed (`asr #31` in `gmtime_internal`) and
/// applies configuration ID 40, which resolves to +32400 seconds (KST).
/// Its 44-byte tm result is copied to +0x48 and then `WPUic_SetTimeStr` is
/// called. The final return value therefore follows the same string/callback
/// contract as `MC_uicSetTime`.
pub async fn set_time_long(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    time: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicSetTimeLong({component:#x}, {time:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 2 {
        return Ok(1);
    }

    let fields = uic_kst_tm_from_epoch_seconds((time as i32) as i64);
    for (index, value) in fields.iter().enumerate() {
        write_generic(context, component + 0x48 + index as u32 * 4, *value)?;
    }
    write_generic(context, component + 0x6c, 9 * 3600i32)?;
    write_generic(context, component + 0x70, 0u32)?;

    let old_text = uic_read_c_string(context, component + 0x74)?;
    let mask: WIPICWord = read_generic(context, component + 0x44)?;
    let formatted = uic_format_datetime(mask, &fields);

    context.write_bytes(component + 0x74, &formatted)?;
    context.write_bytes(component + 0x74 + formatted.len() as u32, &[0])?;

    let compare = match old_text.as_slice().cmp(formatted.as_slice()) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    };

    if compare != 0 {
        let callback: WIPICWord = read_generic(context, component + 0xa0)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0xa4)?;
            return Ok(
                context
                    .call_function(callback, &[component, 0, callback_context])
                    .await? as i32,
            );
        }
    }

    Ok(compare)
}

/// LGT/KTF `MC_uicSetEnable`.
///
/// Native component types are:
/// 1 Menu, 2 DateTime, 3 Text, 4 Label, 5 List.
/// Invalid/null components are ignored. The enable value is normalized to
/// 0/1 and stored at +0x20. DateTime and Text components additionally
/// start/stop their internal timers.
pub async fn set_enable(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    enable: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_uicSetEnable({component:#x}, {enable:#x})");

    if component == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(());
    }

    let enabled = u32::from(enable != 0);
    write_generic(context, component + 0x20, enabled)?;

    match (component_type, enabled) {
        (2, 1) => {
            // DateTimeComponent: WPUic_TimeTimerCB timer.
            write_generic(context, component + 0x9c, 1u32)?;
            let timer = component + 0x98;
            if !context.system().event_queue().has_timer(timer) {
                let callback: WIPICWord = read_generic(context, timer)?;
                if callback == UIC_TIMER_MARKER_TIME {
                    uic_schedule_component_timer(context, component, 2, 1000)?;
                } else {
                    kernel::set_timer(context, timer, 1000, 0, component).await?;
                }
            }
        }
        (2, 0) => {
            kernel::unset_timer(context, component + 0x98).await?;
        }
        (3, 1) => {
            // TextComponent: WPUic_TextTimerCB timer.
            write_generic(context, component + 0x58, 1u32)?;
            let timer = component + 0x54;
            if !context.system().event_queue().has_timer(timer) {
                let callback: WIPICWord = read_generic(context, timer)?;
                if callback == UIC_TIMER_MARKER_TEXT {
                    uic_schedule_component_timer(context, component, 3, 500)?;
                } else {
                    kernel::set_timer(context, timer, 500, 0, component).await?;
                }
            }
        }
        (3, 0) => {
            kernel::unset_timer(context, component + 0x54).await?;
        }
        _ => {}
    }

    Ok(())
}

/// LGT/KTF `MC_uicInsertText`.
///
/// TextComponent layout used here:
/// +0x44 text buffer pointer, +0x48 buffer capacity, +0x4c cursor,
/// +0x5c change callback, +0x64 callback context.
pub async fn insert_text(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: i32,
    source: WIPICWord,
    length: i32,
) -> Result<i32> {
    tracing::debug!(
        "MC_uicInsertText({component:#x}, {position}, {source:#x}, {length})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 3 {
        return Ok(-9);
    }
    if source == 0 || length <= 0 {
        return Ok(0);
    }

    let text: WIPICWord = read_generic(context, component + 0x44)?;
    if text == 0 {
        return Ok(0);
    }

    let mut old_len = 0u32;
    loop {
        let byte: u8 = read_generic(context, text + old_len)?;
        if byte == 0 {
            break;
        }
        old_len = old_len.wrapping_add(1);
    }

    let insert_len = length as u32;
    let new_len = old_len.wrapping_add(insert_len);
    let capacity: u32 = read_generic(context, component + 0x48)?;
    if capacity <= new_len {
        return Ok(-17);
    }

    let insert_pos = if position < 0 {
        0
    } else {
        (position as u32).min(old_len)
    };

    let mut old = alloc::vec![0u8; old_len as usize];
    if old_len != 0 {
        context.read_bytes(text, &mut old)?;
    }

    let mut inserted = alloc::vec![0u8; insert_len as usize];
    context.read_bytes(source, &mut inserted)?;

    if insert_pos != 0 {
        context.write_bytes(text, &old[..insert_pos as usize])?;
    }
    context.write_bytes(text + insert_pos, &inserted)?;
    if insert_pos < old_len {
        context.write_bytes(
            text + insert_pos + insert_len,
            &old[insert_pos as usize..],
        )?;
    }
    context.write_bytes(text + new_len, &[0])?;

    let cursor: u32 = read_generic(context, component + 0x4c)?;
    if insert_pos <= cursor {
        write_generic(context, component + 0x4c, insert_pos + insert_len)?;
    }

    let changed = {
        let old_c_len = old.iter().position(|&byte| byte == 0).unwrap_or(old.len());

        let mut current = alloc::vec![0u8; new_len as usize];
        if new_len != 0 {
            context.read_bytes(text, &mut current)?;
        }
        let current_c_len = current.iter().position(|&byte| byte == 0).unwrap_or(current.len());

        old[..old_c_len] != current[..current_c_len]
    };

    if changed {
        let callback: WIPICWord = read_generic(context, component + 0x5c)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x64)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(length)
}

/// LGT/KTF `MC_uicDeleteText`.
///
/// Native accepts `length == -1` (delete to end) or a positive length.
/// If a positive deletion reaches or passes the end, it is also treated
/// as delete-to-end. Invalid/null components and invalid ranges are no-ops.
pub async fn delete_text(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: i32,
    length: i32,
) -> Result<()> {
    tracing::debug!("MC_uicDeleteText({component:#x}, {position}, {length})");

    if component == 0 || length < -1 || length == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) || component_type != 3 {
        return Ok(());
    }

    let text: WIPICWord = read_generic(context, component + 0x44)?;
    if text == 0 {
        return Ok(());
    }

    let mut old_len = 0u32;
    loop {
        let byte: u8 = read_generic(context, text + old_len)?;
        if byte == 0 {
            break;
        }
        old_len = old_len.wrapping_add(1);
    }

    if position < 0 || position as u32 > old_len {
        return Ok(());
    }
    let delete_pos = position as u32;

    let mut old = alloc::vec![0u8; old_len as usize];
    if old_len != 0 {
        context.read_bytes(text, &mut old)?;
    }

    let delete_to_end = if length == -1 {
        true
    } else {
        let end = (position as i64) + (length as i64);
        end >= old_len as i64
    };

    if delete_to_end {
        context.write_bytes(text + delete_pos, &[0])?;
        write_generic(context, component + 0x4c, delete_pos)?;
    } else {
        let delete_len = length as u32;
        let src = delete_pos + delete_len;
        let tail_len = old_len - src;

        if tail_len != 0 {
            let mut tail = alloc::vec![0u8; tail_len as usize];
            context.read_bytes(text + src, &mut tail)?;
            context.write_bytes(text + delete_pos, &tail)?;
        }

        let new_len = old_len - delete_len;
        context.write_bytes(text + new_len, &[0])?;

        let cursor: i32 = read_generic(context, component + 0x4c)?;
        let cursor = cursor.saturating_sub(length).max(0);
        write_generic(context, component + 0x4c, cursor)?;
    }

    let changed = {
        let mut current = alloc::vec![0u8; old_len as usize];
        if old_len != 0 {
            context.read_bytes(text, &mut current)?;
        }

        let old_c_len = old.iter().position(|&byte| byte == 0).unwrap_or(old.len());
        let current_c_len = current.iter().position(|&byte| byte == 0).unwrap_or(current.len());
        old[..old_c_len] != current[..current_c_len]
    };

    if changed {
        let callback: WIPICWord = read_generic(context, component + 0x5c)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x64)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(())
}

/// LGT `MC_uicGetMaxTextSize` (WIPI-C service 0x340).
///
/// Native validates the component first. NULL/invalid components return 0,
/// valid non-Text components return -9, and valid Text components return the
/// 32-bit max-text-size/capacity field stored at +0x48 unchanged.
pub async fn get_max_text_size(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicGetMaxTextSize({component:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 3 {
        return Ok(-9);
    }

    read_generic(context, component + 0x48)
}

/// LGT `MC_uicGetTextSize` (WIPI-C service 0x342).
///
/// Native first validates the component. NULL/invalid components return 0 and
/// valid non-Text components return -9.
///
/// For a valid Text component, +0x44 is the NUL-terminated text buffer pointer.
/// A NULL text pointer returns 0. Otherwise native tail-calls `strlen` and
/// returns the current text length, excluding the terminating NUL. The +0x48
/// capacity and +0x4c cursor fields are not consulted.
pub async fn get_text_size(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicGetTextSize({component:#x})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 3 {
        return Ok(-9);
    }

    let text: WIPICWord = read_generic(context, component + 0x44)?;
    if text == 0 {
        return Ok(0);
    }

    Ok(uic_read_c_string(context, text)?.len() as i32)
}

/// LGT `MC_uicGetText` (WIPI-C service 0x343).
///
/// Native argument order is `(component, position, output, buflen)`.
///
/// Validation order is significant:
/// - NULL/invalid component => 0
/// - NULL output or `buflen <= 0` => 0
/// - valid non-Text component => -9
/// - NULL Text buffer at +0x44 => 0
///
/// A negative position is clamped to 0 with the native ARM
/// `bic position, position, position, asr #31` sequence.
///
/// Native does not treat `buflen` as a copy-count limit. After obtaining the
/// source `strlen`, it requires:
///
/// `source_len >= position` and `source_len < position + buflen`
///
/// using signed ARM comparisons and 32-bit wrapping addition. If either test
/// fails it returns 0 without writing the destination.
///
/// On success it copies the complete suffix `source[position..source_len]`,
/// appends NUL, then tail-calls `strlen(output)`. Therefore the successful
/// return value is the suffix length.
pub async fn get_text(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: i32,
    output: WIPICWord,
    buflen: i32,
) -> Result<i32> {
    tracing::debug!(
        "MC_uicGetText({component:#x}, {position}, {output:#x}, {buflen})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    if output == 0 || buflen <= 0 {
        return Ok(0);
    }

    if component_type != 3 {
        return Ok(-9);
    }

    let text: WIPICWord = read_generic(context, component + 0x44)?;
    if text == 0 {
        return Ok(0);
    }

    let source = uic_read_c_string(context, text)?;
    let source_len = source.len() as i32;
    let position = if position < 0 { 0 } else { position };

    if source_len < position {
        return Ok(0);
    }

    let end = position.wrapping_add(buflen);
    if source_len >= end {
        return Ok(0);
    }

    let copy_len = source_len.wrapping_sub(position) as usize;
    let start = position as usize;

    if copy_len != 0 {
        context.write_bytes(output, &source[start..start + copy_len])?;
    }
    context.write_bytes(output.wrapping_add(copy_len as u32), &[0])?;

    Ok(uic_read_c_string(context, output)?.len() as i32)
}

/// LGT/KTF `MC_uicSetMaxTextSize`.
///
/// Native returns the previous capacity on success. Invalid/null components
/// and negative sizes return 0, non-Text components return -9, and realloc
/// failure returns -17. For a positive new capacity, the final byte is always
/// forced to NUL.
///
/// The WIE allocator has no realloc primitive, so changed-size reallocations
/// are reproduced as allocate/copy/free using the old +0x48 capacity as the
/// exact allocation size required by `free_raw`.
pub async fn set_max_text_size(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    size: i32,
) -> Result<i32> {
    tracing::debug!("MC_uicSetMaxTextSize({component:#x}, {size})");

    if component == 0 || size < 0 {
        return Ok(0);
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 3 {
        return Ok(-9);
    }

    let old_text: WIPICWord = read_generic(context, component + 0x44)?;
    let old_capacity: u32 = read_generic(context, component + 0x48)?;
    let new_capacity = size as u32;

    // Native dmemory_realloc(ptr, 0) frees a non-null old block and returns
    // NULL. MC_uicSetMaxTextSize then reports -17 without updating +0x44/+0x48.
    if new_capacity == 0 {
        if old_text != 0 && old_capacity != 0 {
            context.free_raw(old_text, old_capacity)?;
        }
        return Ok(-17);
    }

    // Native realloc keeps an existing block when the requested size does not
    // exceed its allocator size. For the ordinary TextComponent case,
    // requesting the same capacity therefore preserves the pointer.
    if old_text != 0 && new_capacity == old_capacity {
        context.write_bytes(old_text + new_capacity - 1, &[0])?;
        return Ok(old_capacity as i32);
    }

    let new_text = match context.alloc_raw(new_capacity) {
        Ok(address) => address,
        Err(WieError::AllocationFailure) => return Ok(-17),
        Err(error) => return Err(error),
    };

    if old_text != 0 && old_capacity != 0 {
        let copy_len = old_capacity.min(new_capacity);
        if copy_len != 0 {
            let mut data = alloc::vec![0u8; copy_len as usize];
            context.read_bytes(old_text, &mut data)?;
            context.write_bytes(new_text, &data)?;
        }
        context.free_raw(old_text, old_capacity)?;
    }

    write_generic(context, component + 0x48, new_capacity)?;
    write_generic(context, component + 0x44, new_text)?;
    context.write_bytes(new_text + new_capacity - 1, &[0])?;

    Ok(old_capacity as i32)
}

const UIC_CLASS_NAME_MENU: WIPICWord = 0x7fff_1010;
const UIC_CLASS_NAME_DATETIME: WIPICWord = 0x7fff_1020;
const UIC_CLASS_NAME_TEXT: WIPICWord = 0x7fff_1038;
const UIC_CLASS_NAME_LABEL: WIPICWord = 0x7fff_1048;
const UIC_CLASS_NAME_LIST: WIPICWord = 0x7fff_1058;
const UIC_EMPTY_LABEL: WIPICWord = 0x7fff_1068;

fn uic_class_name(component_type: WIPICWord) -> Option<(WIPICWord, &'static [u8])> {
    match component_type {
        1 => Some((UIC_CLASS_NAME_MENU, b"MenuComponent\0")),
        2 => Some((UIC_CLASS_NAME_DATETIME, b"DateTimeComponent\0")),
        3 => Some((UIC_CLASS_NAME_TEXT, b"TextComponent\0")),
        4 => Some((UIC_CLASS_NAME_LABEL, b"LabelComponent\0")),
        5 => Some((UIC_CLASS_NAME_LIST, b"ListComponent\0")),
        _ => None,
    }
}

/// LGT `MC_uicGetClassName` (WIPI-C service 0x326).
///
/// Native validates the component and returns one of five provider-static
/// NUL-terminated class-name pointers. Invalid/NULL components return NULL.
/// WIE mirrors the native table's stable pointer identity in reserved guest
/// global-data memory, preserving the native relative spacing between strings.
pub async fn get_class_name(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_uicGetClassName({component:#x})");

    if component == 0 {
        return Ok(WIPICIndirectPtr(0));
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    let Some((address, name)) = uic_class_name(component_type) else {
        return Ok(WIPICIndirectPtr(0));
    };

    context.write_bytes(address, name)?;
    Ok(WIPICIndirectPtr(address))
}

/// LGT `MC_uicIsInstance` (WIPI-C service 0x327).
///
/// Native validates the component, rejects a NULL class-name pointer, then
/// compares the supplied NUL-terminated string with the exact provider class
/// name for component types 1..=5. It returns 1 only for an exact `strcmp`
/// match and 0 otherwise.
pub async fn is_instance(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    psz: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicIsInstance({component:#x}, {psz:#x})");

    if component == 0 || psz == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    let Some((_, native_name)) = uic_class_name(component_type) else {
        return Ok(0);
    };

    let supplied = read_null_terminated_string_bytes(context, psz)?;
    let native_name = &native_name[..native_name.len() - 1];

    Ok(i32::from(supplied.as_slice() == native_name))
}

/// LGT `MC_uicPaint` (WIPI-C service 0x325).
///
/// Native contract:
/// - NULL or invalid component types (outside 1..=5) are silent no-ops.
/// - NULL graphics context is a silent no-op.
/// - property 148 selects screen framebuffer 0 or 3 before draw dispatch.
/// - component +0x24 identifies the provider-private type-specific draw routine.
/// - callback +0x30, when present, is then invoked as
///   `(component, 0, component+0x3c context)`.
///
/// Native stores LGT-internal WPUic_Draw* addresses at +0x24. Components created
/// by generic WIE store internal markers instead so those provider addresses are
/// never mistaken for application ARM callbacks.
pub async fn paint(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    graphics_context: WIPICWord,
) -> Result<()> {
    tracing::debug!("MC_uicPaint({component:#x}, {graphics_context:#x})");

    if component == 0 {
        return Ok(());
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) || graphics_context == 0 {
        return Ok(());
    }

    // Native first selects framebuffer 0 or 3 from dlet property 148.
    // WIPICContext does not expose that LGT property table, so use the
    // native fallback surface (0). The type-specific native draw routine
    // performs the same framebuffer selection before actual rendering.
    let _ = graphics::get_screen_framebuffer(context, 0).await?;

    uic_dispatch_draw(context, component, graphics_context).await?;

    let callback: WIPICWord = read_generic(context, component + 0x30)?;
    if callback != 0 {
        let callback_context: WIPICWord = read_generic(context, component + 0x3c)?;
        context
            .call_function(callback, &[component, 0, callback_context])
            .await?;
    }

    Ok(())
}


fn uic_read_c_string(context: &dyn WIPICContext, address: WIPICWord) -> Result<alloc::vec::Vec<u8>> {
    let mut result = alloc::vec::Vec::new();
    let mut offset = 0u32;

    loop {
        let byte: u8 = read_generic(context, address.wrapping_add(offset))?;
        if byte == 0 {
            break;
        }
        result.push(byte);
        offset = offset.wrapping_add(1);
    }

    Ok(result)
}

fn uic_signed_byte_width(byte: u8) -> u32 {
    if byte & 0x80 != 0 { 2 } else { 1 }
}

fn uic_text_line_step(context: &dyn WIPICContext, component: WIPICWord) -> Result<i32> {
    let width: i32 = read_generic(context, component + 0x0c)?;

    // Native WGrText_Draw:
    // s_MaxLineCharNum = (s_X2 + 1 - s_X1) / (font_height >> 1).
    // WPUic_DrawText sets [s_X1, s_X2] to [x + 4, x + width - 4].
    // The current WIE MC_grpGetFontHeight implementation returns 12.
    Ok((width - 7) / 6)
}

fn uic_text_vertical_position(
    text_len: i32,
    cursor: i32,
    step: i32,
    direction: i32,
) -> Option<i32> {
    if step <= 0 || text_len <= step {
        return None;
    }

    match direction {
        0 => {
            if step < cursor {
                let next = cursor - step;
                if next >= 0 {
                    Some(next)
                } else {
                    None
                }
            } else {
                None
            }
        }
        1 => {
            if text_len / step <= cursor / step {
                return None;
            }
            let next = cursor + step;
            if text_len >= next {
                Some(next)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn uic_skip_time_separator(text: &[u8], cursor: usize, forward: bool) -> usize {
    if cursor >= text.len() || !matches!(text[cursor], b':' | b'/' | b'\n') {
        return cursor;
    }

    if forward {
        cursor.saturating_add(1)
    } else {
        cursor.saturating_sub(1)
    }
}

fn uic_get_active_item_pos(selected: i32, count: i32, scroll: i32) -> Option<(i32, i32)> {
    if selected == -1 {
        return Some((scroll, scroll));
    }
    if selected >= count {
        return None;
    }
    if selected > 0 {
        let top = selected.saturating_mul(17);
        Some((top, top.saturating_add(17)))
    } else {
        Some((0, 17))
    }
}

async fn uic_repaint_component(context: &mut dyn WIPICContext, component: WIPICWord) -> Result<()> {
    repaint(context, component, 0, 0, -1, -1).await
}

async fn uic_selection_changed(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    old_selected: i32,
) -> Result<()> {
    let selected: i32 = read_generic(context, component + 0x48)?;
    if selected == old_selected {
        return Ok(());
    }

    let callback: WIPICWord = read_generic(context, component + 0x54)?;
    if callback != 0 {
        let callback_context: WIPICWord = read_generic(context, component + 0x58)?;
        context
            .call_function(callback, &[component, selected as u32, callback_context])
            .await?;
    }

    Ok(())
}

async fn uic_handle_menu(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    key: i32,
) -> Result<u32> {
    let count: i32 = read_generic(context, component + 0x44)?;
    if count <= 0 {
        return Ok(0);
    }

    let old_selected: i32 = read_generic(context, component + 0x48)?;

    match key {
        -1 => {
            let selected = if old_selected == -1 {
                0
            } else {
                (count - 1 + old_selected).rem_euclid(count)
            };
            write_generic(context, component + 0x48, selected)?;
            uic_selection_changed(context, component, old_selected).await?;
            Ok(1)
        }
        -2 => {
            let selected = if old_selected == -1 {
                0
            } else {
                (old_selected + 1).rem_euclid(count)
            };
            write_generic(context, component + 0x48, selected)?;
            uic_selection_changed(context, component, old_selected).await?;
            Ok(1)
        }
        -5 => {
            let callback: WIPICWord = read_generic(context, component + 0x34)?;
            if callback != 0 {
                let callback_context: WIPICWord = read_generic(context, component + 0x40)?;
                context
                    .call_function(
                        callback,
                        &[component, old_selected as u32, callback_context],
                    )
                    .await?;
            }
            uic_selection_changed(context, component, old_selected).await?;
            Ok(1)
        }
        _ => Ok(0),
    }
}

async fn uic_handle_list(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    key: i32,
) -> Result<u32> {
    let count: i32 = read_generic(context, component + 0x44)?;
    if count <= 0 {
        return Ok(0);
    }

    let old_selected: i32 = read_generic(context, component + 0x48)?;

    if key == -5 {
        let callback: WIPICWord = read_generic(context, component + 0x34)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x40)?;
            context
                .call_function(
                    callback,
                    &[component, old_selected as u32, callback_context],
                )
                .await?;
        }
        uic_selection_changed(context, component, old_selected).await?;
        return Ok(1);
    }

    if !matches!(key, -1 | -2) {
        return Ok(0);
    }

    let height: i32 = read_generic(context, component + 0x10)?;
    let mut scroll: i32 = read_generic(context, component + 0x4c)?;

    let Some((top, bottom)) = uic_get_active_item_pos(old_selected, count, scroll) else {
        return Ok(0);
    };

    if key == -1 {
        if scroll <= top {
            if old_selected > 0 {
                let selected = old_selected - 1;
                write_generic(context, component + 0x48, selected)?;

                if let Some((new_top, _new_bottom)) =
                    uic_get_active_item_pos(selected, count, scroll)
                {
                    if scroll > new_top {
                        let span = bottom.saturating_add(1).saturating_sub(new_top);
                        scroll = if span <= height {
                            new_top
                        } else {
                            bottom.saturating_sub(height.saturating_sub(1))
                        };
                        write_generic(context, component + 0x4c, scroll)?;
                    }
                }
            }
        } else if scroll <= height {
            write_generic(context, component + 0x4c, 0i32)?;
        } else {
            let candidate = scroll.saturating_sub(height);
            scroll = if candidate < top {
                top
            } else {
                (candidate / 17) * 17
            };
            write_generic(context, component + 0x4c, scroll)?;
        }
    } else {
        let edge = scroll.saturating_add(height);
        if edge > bottom {
            if old_selected + 1 < count {
                let selected = old_selected + 1;
                write_generic(context, component + 0x48, selected)?;

                if let Some((new_top, new_bottom)) =
                    uic_get_active_item_pos(selected, count, scroll)
                {
                    if scroll.saturating_add(height.saturating_sub(1)) < new_bottom {
                        let span = new_bottom.saturating_add(1).saturating_sub(new_top);
                        scroll = if span <= height {
                            new_bottom.saturating_sub(height.saturating_sub(1))
                        } else {
                            new_top
                        };
                        write_generic(context, component + 0x4c, scroll)?;
                    }
                }
            }
        } else {
            scroll = (edge / 17) * 17;
            write_generic(context, component + 0x4c, scroll)?;
        }
    }

    uic_selection_changed(context, component, old_selected).await?;
    Ok(1)
}

fn uic_is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn uic_days_before_month(year: i32, month: i32) -> i32 {
    let table = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut result = table[(month - 1) as usize];
    if month > 2 && uic_is_leap_year(year) {
        result += 1;
    }
    result
}

fn uic_days_since_1900_03_01(year: i32, month: i32, day: i32) -> i64 {
    let mut y = year as i64;
    let mut m = month as i64;
    if m <= 2 {
        y -= 1;
        m += 12;
    }

    let y0 = y - 1900;
    365 * y0
        + y0.div_euclid(4)
        - y0.div_euclid(100)
        + y0.div_euclid(400)
        + ((153 * (m - 3) + 2) / 5)
        + (day as i64 - 1)
}

fn uic_civil_from_days(days: i64) -> (i32, i32, i32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096)
            / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = month_param + if month_param < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };

    (year as i32, month as i32, day as i32)
}

fn uic_kst_tm_from_epoch_seconds(epoch_seconds: i64) -> [i32; 9] {
    let seconds = epoch_seconds + 9 * 3600;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);

    let (year, month, day) = uic_civil_from_days(days);
    let hour = (seconds_of_day / 3600) as i32;
    let minute = ((seconds_of_day % 3600) / 60) as i32;
    let second = (seconds_of_day % 60) as i32;
    let yday = uic_days_before_month(year, month) + day - 1;
    let wday = (days + 4).rem_euclid(7) as i32;

    [
        second,
        minute,
        hour,
        day,
        month - 1,
        year - 1900,
        wday,
        yday,
        0,
    ]
}

fn uic_kst_tm_from_epoch_millis(epoch_millis: u64) -> [i32; 9] {
    uic_kst_tm_from_epoch_seconds((epoch_millis / 1000) as i64)
}

async fn uic_datetime_timer_callback(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<()> {
    let blink: u32 = read_generic(context, component + 0x9c)?;
    write_generic(context, component + 0x9c, u32::from(blink == 0))?;

    let fields = uic_kst_tm_from_epoch_millis(context.system().platform().now().raw());
    for (index, value) in fields.iter().enumerate() {
        write_generic(context, component + 0x48 + (index as u32) * 4, *value)?;
    }

    let mask: u32 = read_generic(context, component + 0x44)?;
    let formatted = uic_format_datetime(mask, &fields);
    let text_ptr = component + 0x74;
    context.write_bytes(text_ptr, &formatted)?;
    context.write_bytes(text_ptr + formatted.len() as u32, &[0])?;

    // WPUic_TimeTimerCB performs the component draw, invokes the paint
    // callback, repaints the component, and re-arms the 1000 ms timer.
    // Generic WIE has no persistent native stack graphics context, so use
    // the component's normal paint path with an allocated temporary context.
    let gctx_size =
        core::mem::size_of::<wipi_types::wipic::WIPICGraphicsContext>() as u32;
    let gctx = context.alloc_raw(gctx_size)?;
    graphics::init_context(context, gctx).await?;

    uic_dispatch_draw(context, component, gctx).await?;

    let callback: WIPICWord = read_generic(context, component + 0x30)?;
    if callback != 0 {
        let callback_context: WIPICWord = read_generic(context, component + 0x3c)?;
        context
            .call_function(callback, &[component, 0, callback_context])
            .await?;
    }

    context.free_raw(gctx, gctx_size)?;

    uic_repaint_component(context, component).await?;

    let timer = component + 0x98;
    let callback: WIPICWord = read_generic(context, timer)?;
    if callback == UIC_TIMER_MARKER_TIME {
        uic_schedule_component_timer(context, component, 2, 1000)?;
    } else {
        kernel::set_timer(context, timer, 1000, 0, component).await?;
    }
    Ok(())
}

async fn uic_text_timer_callback(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<()> {
    let blink: u32 = read_generic(context, component + 0x58)?;
    write_generic(context, component + 0x58, u32::from(blink == 0))?;

    let gctx_size =
        core::mem::size_of::<wipi_types::wipic::WIPICGraphicsContext>() as u32;
    let gctx = context.alloc_raw(gctx_size)?;
    graphics::init_context(context, gctx).await?;

    uic_dispatch_draw(context, component, gctx).await?;

    let callback: WIPICWord = read_generic(context, component + 0x30)?;
    if callback != 0 {
        let callback_context: WIPICWord = read_generic(context, component + 0x3c)?;
        context
            .call_function(callback, &[component, 0, callback_context])
            .await?;
    }

    context.free_raw(gctx, gctx_size)?;
    uic_repaint_component(context, component).await?;

    let timer = component + 0x54;
    let callback: WIPICWord = read_generic(context, timer)?;
    if callback == UIC_TIMER_MARKER_TEXT {
        uic_schedule_component_timer(context, component, 3, 500)?;
    } else {
        kernel::set_timer(context, timer, 500, 0, component).await?;
    }

    Ok(())
}

async fn uic_datetime_finish_edit(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<u32> {
    write_generic(context, component + 0x9c, 0u32)?;
    kernel::unset_timer(context, component + 0x98).await?;
    uic_datetime_timer_callback(context, component).await?;
    Ok(1)
}

fn uic_format_datetime(mask: u32, fields: &[i32; 9]) -> alloc::vec::Vec<u8> {
    let sec = fields[0];
    let min = fields[1];
    let hour = fields[2];
    let mday = fields[3];
    let mon = fields[4] + 1;
    let year = fields[5] + 1900;

    let text = if mask == 3 {
        alloc::format!(
            "{year:04}/{mon:02}/{mday:02} {hour:02}:{min:02}:{sec:02}"
        )
    } else if mask == 1 {
        alloc::format!("{hour:02}:{min:02}:{sec:02}")
    } else {
        alloc::format!("{year:04}/{mon:02}/{mday:02}")
    };

    text.into_bytes()
}

fn uic_parse_datetime(text: &[u8]) -> Option<(i32, i32, i32, i32, i32, i32)> {
    if text.len() < 19 {
        return None;
    }
    if text[4] != b'/'
        || text[7] != b'/'
        || text[10] != b' '
        || text[13] != b':'
        || text[16] != b':'
    {
        return None;
    }

    let d = |a: usize, b: usize| -> Option<i32> {
        let x = *text.get(a)?;
        let y = *text.get(b)?;
        if !x.is_ascii_digit() || !y.is_ascii_digit() {
            return None;
        }
        Some(((x - b'0') as i32) * 10 + (y - b'0') as i32)
    };

    if !text[0..4].iter().all(u8::is_ascii_digit) {
        return None;
    }

    let year = ((text[0] - b'0') as i32) * 1000
        + ((text[1] - b'0') as i32) * 100
        + ((text[2] - b'0') as i32) * 10
        + ((text[3] - b'0') as i32);

    Some((
        year,
        d(5, 6)?,
        d(8, 9)?,
        d(11, 12)?,
        d(14, 15)?,
        d(17, 18)?,
    ))
}

async fn uic_handle_datetime(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    key: i32,
) -> Result<u32> {
    let text_ptr = component + 0x74;
    let mut text = uic_read_c_string(context, text_ptr)?;
    let mut cursor: u32 = read_generic(context, component + 0x94)?;

    match key {
        -3 => {
            if cursor > 0 {
                cursor -= 1;
                if (cursor as usize) < text.len()
                    && matches!(text[cursor as usize], b':' | b'/' | b'\n')
                {
                    cursor = cursor.saturating_sub(1);
                }
                write_generic(context, component + 0x94, cursor)?;
            }
            return uic_datetime_finish_edit(context, component).await;
        }
        -4 => {
            if cursor < text.len().saturating_sub(1) as u32 {
                cursor += 1;
                if (cursor as usize) < text.len()
                    && matches!(text[cursor as usize], b':' | b'/' | b'\n')
                {
                    cursor += 1;
                }
                write_generic(context, component + 0x94, cursor)?;
            }
            return uic_datetime_finish_edit(context, component).await;
        }
        -16 => {
            if (cursor as usize) < text.len() {
                text[cursor as usize] = b'0';
                context.write_bytes(text_ptr, &text)?;
            }
            if cursor < text.len().saturating_sub(1) as u32 {
                cursor += 1;
                if (cursor as usize) < text.len()
                    && matches!(text[cursor as usize], b':' | b'/' | b'\n')
                {
                    cursor += 1;
                }
                write_generic(context, component + 0x94, cursor)?;
            }
            return uic_datetime_finish_edit(context, component).await;
        }
        _ => {}
    }

    if !(48..=57).contains(&key) {
        return uic_datetime_finish_edit(context, component).await;
    }

    if cursor as usize >= text.len() {
        return Ok(1);
    }

    let old = text[cursor as usize];
    text[cursor as usize] = key as u8;
    context.write_bytes(text_ptr, &text)?;

    let mask: u32 = read_generic(context, component + 0x44)?;
    if mask & 2 != 0 {
        let valid = if let Some((year, month, day, hour, minute, second)) =
            uic_parse_datetime(&text)
        {
            (1901..=2999).contains(&year)
                && (1..=12).contains(&month)
                && (1..=31).contains(&day)
                && (0..=23).contains(&hour)
                && (0..=59).contains(&minute)
                && (0..=59).contains(&second)
        } else {
            false
        };

        if !valid {
            context.write_bytes(text_ptr + cursor, &[old])?;
        } else if let Some((year, month, day, hour, minute, second)) =
            uic_parse_datetime(&text)
        {
            let days_in_month = match month {
                2 if uic_is_leap_year(year) => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };

            if day > days_in_month {
                context.write_bytes(text_ptr + cursor, &[old])?;
            } else {
                let tm_year = year - 1900;
                let tm_mon = month - 1;
                let tm_yday = uic_days_before_month(year, month) + day - 1;
                let days = uic_days_since_1900_03_01(year, month, day);
                let tm_wday = (days + 4).rem_euclid(7) as i32;

                write_generic(context, component + 0x48, second)?;
                write_generic(context, component + 0x4c, minute)?;
                write_generic(context, component + 0x50, hour)?;
                write_generic(context, component + 0x54, day)?;
                write_generic(context, component + 0x58, tm_mon)?;
                write_generic(context, component + 0x5c, tm_year)?;
                write_generic(context, component + 0x60, tm_wday)?;
                write_generic(context, component + 0x64, tm_yday)?;

                let fields = [
                    second,
                    minute,
                    hour,
                    day,
                    tm_mon,
                    tm_year,
                    tm_wday,
                    tm_yday,
                    0,
                ];
                let formatted = uic_format_datetime(mask, &fields);
                context.write_bytes(text_ptr, &formatted)?;
                context.write_bytes(text_ptr + formatted.len() as u32, &[0])?;

                let callback: WIPICWord = read_generic(context, component + 0xa0)?;
                if callback != 0 {
                    let callback_context: WIPICWord =
                        read_generic(context, component + 0xa4)?;
                    context
                        .call_function(callback, &[component, 0, callback_context])
                        .await?;
                }
            }
        }
    }

    let len = uic_read_c_string(context, text_ptr)?.len() as u32;
    cursor = read_generic(context, component + 0x94)?;
    if cursor < len.saturating_sub(1) {
        cursor += 1;
        let current = uic_read_c_string(context, text_ptr)?;
        cursor = uic_skip_time_separator(&current, cursor as usize, true) as u32;
        write_generic(context, component + 0x94, cursor)?;
    }

    uic_datetime_finish_edit(context, component).await
}

async fn uic_text_changed_callback(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<()> {
    let callback: WIPICWord = read_generic(context, component + 0x5c)?;
    if callback != 0 {
        let callback_context: WIPICWord = read_generic(context, component + 0x64)?;
        context
            .call_function(callback, &[component, 0, callback_context])
            .await?;
    }
    Ok(())
}

fn uic_text_delete_raw(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: usize,
    length: usize,
) -> Result<()> {
    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    if text_ptr == 0 {
        return Ok(());
    }

    let mut text = uic_read_c_string(context, text_ptr)?;
    if position >= text.len() || length == 0 {
        return Ok(());
    }

    let end = position.saturating_add(length).min(text.len());
    text.drain(position..end);
    context.write_bytes(text_ptr, &text)?;
    context.write_bytes(text_ptr + text.len() as u32, &[0])?;
    write_generic(context, component + 0x4c, position as u32)?;
    Ok(())
}

fn uic_text_insert_raw(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: usize,
    bytes: &[u8],
) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }

    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    let capacity: u32 = read_generic(context, component + 0x48)?;
    if text_ptr == 0 {
        return Ok(());
    }

    let mut text = uic_read_c_string(context, text_ptr)?;
    if text.len().saturating_add(bytes.len()) >= capacity as usize {
        return Ok(());
    }

    let position = position.min(text.len());
    text.splice(position..position, bytes.iter().copied());
    context.write_bytes(text_ptr, &text)?;
    context.write_bytes(text_ptr + text.len() as u32, &[0])?;
    write_generic(
        context,
        component + 0x4c,
        (position + bytes.len()) as u32,
    )?;
    Ok(())
}

async fn uic_text_delete_internal(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: usize,
    length: usize,
) -> Result<()> {
    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    if text_ptr == 0 {
        return Ok(());
    }

    let old = uic_read_c_string(context, text_ptr)?;
    uic_text_delete_raw(context, component, position, length)?;
    let current = uic_read_c_string(context, text_ptr)?;

    if old != current {
        uic_text_changed_callback(context, component).await?;
    }

    Ok(())
}

async fn uic_text_insert_internal(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    position: usize,
    bytes: &[u8],
) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }

    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    if text_ptr == 0 {
        return Ok(());
    }

    let old = uic_read_c_string(context, text_ptr)?;
    uic_text_insert_raw(context, component, position, bytes)?;
    let current = uic_read_c_string(context, text_ptr)?;

    if old != current {
        uic_text_changed_callback(context, component).await?;
    }

    Ok(())
}

async fn uic_text_insert_output(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    output: &[u8],
) -> Result<()> {
    if output.is_empty() {
        return Ok(());
    }

    let cursor: u32 = read_generic(context, component + 0x4c)?;
    uic_text_insert_internal(context, component, cursor as usize, output).await
}

async fn uic_text_remove_composition_internal(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    composition_size: usize,
) -> Result<()> {
    if composition_size == 0 {
        return Ok(());
    }

    let cursor: u32 = read_generic(context, component + 0x4c)?;
    let position = (cursor as usize).saturating_sub(composition_size);
    uic_text_delete_internal(context, component, position, composition_size).await
}

async fn uic_text_apply_ime_output(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    output0: &[u8],
    output1: &[u8],
    old_composition_size: usize,
) -> Result<()> {
    uic_text_remove_composition_internal(context, component, old_composition_size).await?;

    uic_text_insert_output(context, component, output0).await?;
    uic_text_insert_output(context, component, output1).await?;

    context
        .system()
        .set_input_composition_size(output1.len());
    Ok(())
}

async fn uic_text_process_default_input(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    key: i8,
) -> Result<()> {
    let old_composition_size = context.system().input_composition_size();
    let output = context.system().handle_input_method(key, 2);

    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    let capacity: u32 = read_generic(context, component + 0x48)?;
    let text_len = uic_read_c_string(context, text_ptr)?.len();

    // Native 0x1b9af8 computes:
    // growth = output0_len + output1_len - s_CompoSize.
    // If growth is positive and capacity <= strlen(text) + growth, it does
    // not alter the displayed composition. It flushes the IME with key 157
    // and clears s_CompoSize, effectively committing the existing bytes
    // in place.
    let growth = output
        .output0_len
        .saturating_add(output.output1_len) as isize
        - old_composition_size as isize;

    if growth > 0
        && capacity as usize <= text_len.saturating_add(growth as usize)
    {
        let _ = context.system().handle_input_method(-99, 2);
        context.system().set_input_composition_size(0);
        return Ok(());
    }

    uic_text_apply_ime_output(
        context,
        component,
        &output.output0[..output.output0_len],
        &output.output1[..output.output1_len],
        old_composition_size,
    )
    .await
}

async fn uic_text_flush_removed_composition(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<()> {
    let old_composition_size = context.system().input_composition_size();
    if old_composition_size == 0 {
        context.system().set_input_composition_size(0);
        return Ok(());
    }

    uic_text_remove_composition_internal(context, component, old_composition_size).await?;

    // Native LEFT/RIGHT_SOFT/RIGHT composition paths remove the visible
    // composition first, call MC_imHandleInput(157, 502), then insert the
    // returned committed text. Provider output1 is normally zero after
    // this flush but preserve the complete output contract.
    let output = context.system().handle_input_method(-99, 2);
    uic_text_insert_output(
        context,
        component,
        &output.output0[..output.output0_len],
    )
    .await?;
    uic_text_insert_output(
        context,
        component,
        &output.output1[..output.output1_len],
    )
    .await?;

    context
        .system()
        .set_input_composition_size(output.output1_len);
    Ok(())
}

async fn uic_text_clear(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<()> {
    let composition_size = context.system().input_composition_size();
    let cursor: u32 = read_generic(context, component + 0x4c)?;

    if composition_size != 0 {
        // Native intentionally uses public MC_uicDeleteText here rather
        // than WPUic_DeleteText, so preserve its callback behavior.
        let position = (cursor as usize).saturating_sub(composition_size);
        delete_text(
            context,
            component,
            position as i32,
            composition_size as i32,
        )
        .await?;

        let output = context.system().handle_input_method(-16, 2);

        uic_text_insert_output(
            context,
            component,
            &output.output0[..output.output0_len],
        )
        .await?;
        uic_text_insert_output(
            context,
            component,
            &output.output1[..output.output1_len],
        )
        .await?;

        context
            .system()
            .set_input_composition_size(output.output1_len);
        return Ok(());
    }

    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    let text = uic_read_c_string(context, text_ptr)?;

    if cursor == 0 || cursor as usize > text.len() {
        context.system().set_input_composition_size(0);
        return Ok(());
    }

    let previous = (cursor - 1) as usize;
    let width = if text[previous] & 0x80 != 0 { 2 } else { 1 };
    let position = cursor.saturating_sub(width) as i32;

    // Native no-composition CLEAR also uses public MC_uicDeleteText.
    delete_text(context, component, position, width as i32).await?;
    context.system().set_input_composition_size(0);
    Ok(())
}

async fn uic_handle_text(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    key: i32,
) -> Result<u32> {
    let text_ptr: WIPICWord = read_generic(context, component + 0x44)?;
    if text_ptr == 0 {
        return Ok(0);
    }

    let old = uic_read_c_string(context, text_ptr)?;

    match key {
        -16 => {
            uic_text_clear(context, component).await?;
        }
        -7 => {
            if context.system().input_composition_size() != 0 {
                uic_text_flush_removed_composition(context, component).await?;
            }

            let mode = context.system().current_input_mode();
            let next_mode = if mode + 1 >= 4 { 0 } else { mode + 1 };
            context.system().set_current_input_mode(next_mode);
            context.system().set_input_composition_size(0);
        }
        -3 => {
            if context.system().input_composition_size() != 0 {
                uic_text_flush_removed_composition(context, component).await?;
            }

            let cursor: u32 = read_generic(context, component + 0x4c)?;
            if cursor > 0 {
                let text = uic_read_c_string(context, text_ptr)?;
                let previous = (cursor - 1) as usize;
                let width = if text[previous] & 0x80 != 0 { 2 } else { 1 };
                write_generic(
                    context,
                    component + 0x4c,
                    cursor.saturating_sub(width),
                )?;
            }

            context.system().set_input_composition_size(0);
        }
        -4 => {
            if context.system().input_composition_size() != 0 {
                uic_text_flush_removed_composition(context, component).await?;
            }

            let mut cursor: u32 = read_generic(context, component + 0x4c)?;
            let text = uic_read_c_string(context, text_ptr)?;

            if cursor as usize == text.len() {
                uic_text_insert_internal(context, component, text.len(), b" ").await?;
            } else if (cursor as usize) < text.len() {
                cursor += uic_signed_byte_width(text[cursor as usize]);
                write_generic(
                    context,
                    component + 0x4c,
                    cursor.min(text.len() as u32),
                )?;
            }

            context.system().set_input_composition_size(0);
        }
        -1 => {
            let cursor: u32 = read_generic(context, component + 0x4c)?;
            let text = uic_read_c_string(context, text_ptr)?;
            let step = uic_text_line_step(context, component)?;

            if let Some(next) =
                uic_text_vertical_position(text.len() as i32, cursor as i32, step, 0)
            {
                write_generic(context, component + 0x4c, next as u32)?;
            }

            // Native special-key common path stores the zero-initialized
            // output1_len into s_CompoSize.
            context.system().set_input_composition_size(0);
        }
        -2 => {
            let cursor: u32 = read_generic(context, component + 0x4c)?;
            let text = uic_read_c_string(context, text_ptr)?;
            let step = uic_text_line_step(context, component)?;

            if let Some(next) =
                uic_text_vertical_position(text.len() as i32, cursor as i32, step, 1)
            {
                write_generic(context, component + 0x4c, next as u32)?;
            }

            context.system().set_input_composition_size(0);
        }
        _ => {
            uic_text_process_default_input(
                context,
                component,
                key as u8 as i8,
            )
            .await?;
        }
    }

    let current = uic_read_c_string(context, text_ptr)?;
    if old != current {
        let callback: WIPICWord = read_generic(context, component + 0x5c)?;
        if callback != 0 {
            let callback_context: WIPICWord =
                read_generic(context, component + 0x64)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    let callback: WIPICWord = read_generic(context, component + 0x60)?;
    if callback != 0 {
        let callback_context: WIPICWord =
            read_generic(context, component + 0x68)?;
        context
            .call_function(callback, &[component, 0, callback_context])
            .await?;
    }

    Ok(1)
}

/// LGT `MC_uicHandleEvent` (WIPI-C service 0x328).
///
/// Native first invokes the optional component event handler at +0x28. A
/// handler return value of 1 consumes the event immediately. Built-in UIC
/// processing runs only for WIPI key events 502 (press) and 504 (release).
/// Recognized built-in changes repaint the component before returning 1.
pub async fn handle_event(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    event: WIPICWord,
    key: i32,
    extra: WIPICWord,
) -> Result<u32> {
    tracing::debug!(
        "MC_uicHandleEvent({component:#x}, {event}, {key}, {extra:#x})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: u32 = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }

    let enabled: u32 = read_generic(context, component + 0x20)?;
    if enabled == 0 {
        return Ok(0);
    }

    let mut result = 0;
    let handler: WIPICWord = read_generic(context, component + 0x28)?;
    if handler != 0 {
        result = context
            .call_function(
                handler,
                &[component, event, key as u32, extra],
            )
            .await?;
        if result == 1 {
            return Ok(1);
        }
    }

    if !matches!(event, 502 | 504) {
        return Ok(result);
    }

    let builtin = match component_type {
        1 => uic_handle_menu(context, component, key).await?,
        2 => uic_handle_datetime(context, component, key).await?,
        3 => uic_handle_text(context, component, key).await?,
        4 => 0,
        5 => uic_handle_list(context, component, key).await?,
        _ => 0,
    };

    if builtin != 0 {
        uic_repaint_component(context, component).await?;
        Ok(builtin)
    } else if result != 0 {
        uic_repaint_component(context, component).await?;
        Ok(result)
    } else {
        Ok(0)
    }
}

/// LGT `MC_uicAddMenuItem` (WIPI-C service 0x339).
///
/// Native is a thin wrapper over `WPUic_AddItem` with required component type 1.
/// NULL/invalid components return 0 and valid non-Menu components return -9.
///
/// Menu/List common item state is:
/// - +0x44 item count
/// - +0x48 active index
/// - +0x50 pointer table
///
/// The pointer table contains one 32-bit item pointer per entry. Each item is a
/// zero-initialized raw block laid out as `[image: u32][NUL-terminated label]`.
/// A NULL label therefore produces a 5-byte block whose byte at +4 is NUL.
///
/// Native allocates the first 4-byte table with `calloc`, and grows an existing
/// table to `(count + 1) * 4` with `realloc`. WIPICContext has no raw realloc
/// primitive, so the successful realloc path is mirrored by allocating a new
/// table, preserving the existing pointer words, releasing the old table, and
/// then replacing +0x50. Allocation failure returns 0 and does not increment
/// the count. On success the return value is the old count, i.e. the new item's
/// zero-based index.
pub async fn add_menu_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    label: WIPICWord,
    image: WIPICWord,
) -> Result<i32> {
    tracing::debug!(
        "MC_uicAddMenuItem({component:#x}, {label:#x}, {image:#x})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 1 {
        return Ok(-9);
    }

    let count: WIPICWord = read_generic(context, component + 0x44)?;

    let table = if count == 0 {
        let table = match context.alloc_raw(4) {
            Ok(address) => address,
            Err(_) => return Ok(0),
        };
        context.write_bytes(table, &[0; 4])?;
        write_generic(context, component + 0x50, table)?;
        table
    } else {
        let old_table: WIPICWord = read_generic(context, component + 0x50)?;
        let old_size = count.wrapping_mul(4);
        let new_size = count.wrapping_add(1).wrapping_mul(4);

        let new_table = match context.alloc_raw(new_size) {
            Ok(address) => address,
            Err(_) => return Ok(0),
        };

        let mut old_entries = alloc::vec![0u8; old_size as usize];
        context.read_bytes(old_table, &mut old_entries)?;
        context.write_bytes(new_table, &old_entries)?;
        context.free_raw_unsized(old_table)?;
        write_generic(context, component + 0x50, new_table)?;
        new_table
    };

    let label_bytes = if label == 0 {
        alloc::vec::Vec::new()
    } else {
        uic_read_c_string(context, label)?
    };
    let item_size = (label_bytes.len() as u32).wrapping_add(5);

    let item = match context.alloc_raw(item_size) {
        Ok(address) => address,
        Err(_) => return Ok(0),
    };
    context.write_bytes(item, &alloc::vec![0; item_size as usize])?;

    write_generic(context, table + count.wrapping_mul(4), item)?;
    write_generic(context, item, image)?;

    if label != 0 {
        context.write_bytes(item + 4, &label_bytes)?;
        context.write_bytes(item + 4 + label_bytes.len() as u32, &[0])?;
    }

    write_generic(context, component + 0x44, count.wrapping_add(1))?;

    Ok(count as i32)
}

/// LGT `MC_uicAddListItem` (WIPI-C service 0x344).
///
/// Native is a thin wrapper over `WPUic_AddItem` with required component type 5.
/// Its storage and return contract are identical to `MC_uicAddMenuItem`:
/// +0x44 is the item count and +0x50 is the raw pointer table. Each table entry
/// points to a zero-initialized `[image: u32][NUL-terminated label]` block.
///
/// NULL/invalid components return 0 and valid non-List components return -9.
/// A NULL label is accepted and produces an empty string. The first table is a
/// four-byte allocation; later additions grow it to `(count + 1) * 4`.
/// WIPICContext has no raw realloc primitive, so WIE mirrors the successful
/// native realloc path by allocating a replacement table, copying the existing
/// pointer words, freeing the old table, and storing the replacement at +0x50.
///
/// Allocation failure returns 0 without incrementing +0x44. Successful addition
/// returns the previous count, which is the new item's zero-based index.
pub async fn add_list_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    label: WIPICWord,
    image: WIPICWord,
) -> Result<i32> {
    tracing::debug!(
        "MC_uicAddListItem({component:#x}, {label:#x}, {image:#x})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 5 {
        return Ok(-9);
    }

    let count: WIPICWord = read_generic(context, component + 0x44)?;

    let table = if count == 0 {
        let table = match context.alloc_raw(4) {
            Ok(address) => address,
            Err(_) => return Ok(0),
        };
        context.write_bytes(table, &[0; 4])?;
        write_generic(context, component + 0x50, table)?;
        table
    } else {
        let old_table: WIPICWord = read_generic(context, component + 0x50)?;
        let old_size = count.wrapping_mul(4);
        let new_size = count.wrapping_add(1).wrapping_mul(4);

        let new_table = match context.alloc_raw(new_size) {
            Ok(address) => address,
            Err(_) => return Ok(0),
        };

        let mut old_entries = alloc::vec![0u8; old_size as usize];
        context.read_bytes(old_table, &mut old_entries)?;
        context.write_bytes(new_table, &old_entries)?;
        context.free_raw_unsized(old_table)?;
        write_generic(context, component + 0x50, new_table)?;
        new_table
    };

    let label_bytes = if label == 0 {
        alloc::vec::Vec::new()
    } else {
        uic_read_c_string(context, label)?
    };
    let item_size = (label_bytes.len() as u32).wrapping_add(5);

    let item = match context.alloc_raw(item_size) {
        Ok(address) => address,
        Err(_) => return Ok(0),
    };
    context.write_bytes(item, &alloc::vec![0; item_size as usize])?;

    write_generic(context, table + count.wrapping_mul(4), item)?;
    write_generic(context, item, image)?;

    if label != 0 {
        context.write_bytes(item + 4, &label_bytes)?;
        context.write_bytes(item + 4 + label_bytes.len() as u32, &[0])?;
    }

    write_generic(context, component + 0x44, count.wrapping_add(1))?;

    Ok(count as i32)
}

/// LGT `MC_uicRemoveListItem` (WIPI-C service 0x346).
///
/// Native is a thin wrapper over `WPUic_RemoveItem` with required component type 5.
/// Its removal semantics are otherwise identical to `MC_uicRemoveMenuItem`.
///
/// NULL/invalid components and valid non-List components return 0.
///
/// List item state uses +0x44 as the item count, +0x48 as the active index,
/// +0x50 as the pointer table, +0x54 as the active-item callback, and +0x58
/// as that callback's context.
///
/// Native's range check is unsigned `count < index`. Thus only an index strictly
/// greater than the count is rejected; `index == count` proceeds into the native
/// out-of-bounds access path.
///
/// On the ordinary successful path, native destroys the stored image, frees the
/// item block, decrements +0x44, shifts following table entries one slot left,
/// and leaves the pointer table allocation itself unchanged. +0x48 is not adjusted.
///
/// If the removed index exactly equals the active index and +0x54 is nonzero,
/// native calls `callback(component, 0, +0x58 context)`. The callback result is
/// discarded. Successful removal returns 1.
pub async fn remove_list_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    index: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicRemoveListItem({component:#x}, {index})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) || component_type != 5 {
        return Ok(0);
    }

    let count: WIPICWord = read_generic(context, component + 0x44)?;
    if count < index {
        return Ok(0);
    }

    let table: WIPICWord = read_generic(context, component + 0x50)?;
    let slot = table.wrapping_add(index.wrapping_mul(4));
    let item: WIPICWord = read_generic(context, slot)?;

    let image: WIPICWord = read_generic(context, item)?;
    if image != 0 {
        graphics::destroy_image(context, WIPICIndirectPtr(image)).await?;
    }
    context.free_raw_unsized(item)?;

    let new_count = count.wrapping_sub(1);
    write_generic(context, component + 0x44, new_count)?;

    if (index as i32) < (new_count as i32) {
        let mut source = slot.wrapping_add(4);
        let mut current = index.wrapping_add(1);

        while (current as i32) < (new_count as i32) {
            let next: WIPICWord = read_generic(context, source)?;
            write_generic(context, source.wrapping_sub(4), next)?;

            current = current.wrapping_add(1);
            source = source.wrapping_add(4);
        }

        if (current as i32) == (new_count as i32) {
            let next: WIPICWord = read_generic(context, source)?;
            write_generic(context, source.wrapping_sub(4), next)?;
        }
    }

    let active: WIPICWord = read_generic(context, component + 0x48)?;
    if index == active {
        let callback: WIPICWord = read_generic(context, component + 0x54)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x58)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(1)
}

/// LGT `MC_uicRemoveMenuItem` (WIPI-C service 0x33b).
///
/// Native is a thin wrapper over `WPUic_RemoveItem` with required component type 1.
/// NULL/invalid components and valid non-Menu components all return 0.
///
/// Menu item state uses +0x44 as the item count, +0x48 as the active index,
/// +0x50 as the pointer table, +0x54 as the active-item callback, and +0x58
/// as that callback's context.
///
/// Native's initial range check is unsigned `count < index`, not `index >= count`.
/// Consequently `index == count` is not rejected before the table access; WIE
/// preserves that control-flow shape rather than adding a stricter range rule.
///
/// The selected item image is destroyed, the item block is freed, and +0x44 is
/// decremented. The pointer table itself is neither shrunk nor reallocated.
/// Entries after the removed index are shifted one slot left when the removed
/// index is before the new count; the stale final table word is left untouched.
///
/// Native does not adjust +0x48 after removal. If the removed index exactly equals
/// the active index and +0x54 is nonzero, it invokes
/// `callback(component, 0, +0x58 context)`. The callback result is discarded and
/// successful removal returns 1.
pub async fn remove_menu_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    index: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicRemoveMenuItem({component:#x}, {index})");

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) || component_type != 1 {
        return Ok(0);
    }

    let count: WIPICWord = read_generic(context, component + 0x44)?;
    if count < index {
        return Ok(0);
    }

    let table: WIPICWord = read_generic(context, component + 0x50)?;
    let slot = table.wrapping_add(index.wrapping_mul(4));
    let item: WIPICWord = read_generic(context, slot)?;

    let image: WIPICWord = read_generic(context, item)?;
    if image != 0 {
        graphics::destroy_image(context, WIPICIndirectPtr(image)).await?;
    }
    context.free_raw_unsized(item)?;

    let new_count = count.wrapping_sub(1);
    write_generic(context, component + 0x44, new_count)?;

    if (index as i32) < (new_count as i32) {
        let mut source = slot.wrapping_add(4);
        let mut current = index.wrapping_add(1);

        while (current as i32) < (new_count as i32) {
            let next: WIPICWord = read_generic(context, source)?;
            write_generic(context, source.wrapping_sub(4), next)?;

            current = current.wrapping_add(1);
            source = source.wrapping_add(4);
        }

        if (current as i32) == (new_count as i32) {
            let next: WIPICWord = read_generic(context, source)?;
            write_generic(context, source.wrapping_sub(4), next)?;
        }
    }

    let active: WIPICWord = read_generic(context, component + 0x48)?;
    if index == active {
        let callback: WIPICWord = read_generic(context, component + 0x54)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x58)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(1)
}

/// LGT `MC_uicSetActiveListItem` (WIPI-C service 0x347).
///
/// Native is a thin wrapper over `WPUic_SetActiveItem` with required component type 5.
/// Its behavior is otherwise identical to `MC_uicSetActiveMenuItem`.
///
/// NULL/invalid components return -1, while valid non-List components return -9.
///
/// List state uses +0x44 as the item count, +0x48 as the active index, +0x54 as the
/// active-item callback, and +0x58 as its callback context.
///
/// The value -1 is always accepted and clears the active item. Every other value is
/// accepted only when the signed comparison `selected < count` succeeds. Therefore
/// native also accepts values below -1 when they are less than the signed count.
///
/// On success native returns the previous +0x48 value. It stores the new value and,
/// only when the value changed and +0x54 is nonzero, invokes
/// `callback(component, 0, +0x58 context)`. The callback result is discarded.
pub async fn set_active_list_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    selected: i32,
) -> Result<i32> {
    tracing::debug!("MC_uicSetActiveListItem({component:#x}, {selected})");

    if component == 0 {
        return Ok(-1);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(-1);
    }
    if component_type != 5 {
        return Ok(-9);
    }

    let old_selected: i32 = read_generic(context, component + 0x48)?;

    if selected != -1 {
        let count: i32 = read_generic(context, component + 0x44)?;
        if selected >= count {
            return Ok(-1);
        }
    }

    write_generic(context, component + 0x48, selected)?;

    if selected != old_selected {
        let callback: WIPICWord = read_generic(context, component + 0x54)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x58)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(old_selected)
}

/// LGT `MC_uicSetActiveMenuItem` (WIPI-C service 0x33c).
///
/// Native is a thin wrapper over `WPUic_SetActiveItem` with required component type 1.
/// NULL/invalid components return -1, while valid non-Menu components return -9.
///
/// Menu state uses +0x44 as the item count, +0x48 as the active index, +0x54 as the
/// active-item callback, and +0x58 as its callback context.
///
/// The value -1 is always accepted and clears the active item. Every other value is
/// accepted only when the signed comparison `selected < count` succeeds. This means
/// negative values below -1 are also accepted by the native code rather than rejected.
///
/// On success native returns the previous +0x48 value. After storing the new value it
/// invokes `callback(component, 0, +0x58 context)` only when the active value changed
/// and +0x54 is nonzero. The callback return value is discarded.
pub async fn set_active_menu_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    selected: i32,
) -> Result<i32> {
    tracing::debug!("MC_uicSetActiveMenuItem({component:#x}, {selected})");

    if component == 0 {
        return Ok(-1);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(-1);
    }
    if component_type != 1 {
        return Ok(-9);
    }

    let old_selected: i32 = read_generic(context, component + 0x48)?;

    if selected != -1 {
        let count: i32 = read_generic(context, component + 0x44)?;
        if selected >= count {
            return Ok(-1);
        }
    }

    write_generic(context, component + 0x48, selected)?;

    if selected != old_selected {
        let callback: WIPICWord = read_generic(context, component + 0x54)?;
        if callback != 0 {
            let callback_context: WIPICWord = read_generic(context, component + 0x58)?;
            context
                .call_function(callback, &[component, 0, callback_context])
                .await?;
        }
    }

    Ok(old_selected)
}

/// LGT `MC_uicGetActiveListItem` (WIPI-C service 0x348).
///
/// Native is a thin wrapper over `WPUic_GetActiveItem` with required component type 5.
///
/// `WPUic_CheckValidComp` failure is converted from 0 to -1. A valid component of
/// any type other than List returns -9. For a valid List component, native simply
/// returns the signed 32-bit active-item value stored at +0x48 unchanged.
pub async fn get_active_list_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicGetActiveListItem({component:#x})");

    if component == 0 {
        return Ok(-1);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(-1);
    }
    if component_type != 5 {
        return Ok(-9);
    }

    read_generic(context, component + 0x48)
}

/// LGT `MC_uicGetActiveMenuItem` (WIPI-C service 0x33d).
///
/// Native is a thin wrapper over `WPUic_GetActiveItem` with required component type 1.
///
/// `WPUic_CheckValidComp` failure is converted from 0 to -1. A valid component of
/// any type other than Menu returns -9. For a valid Menu component, native simply
/// returns the signed 32-bit active-item value stored at +0x48 unchanged.
pub async fn get_active_menu_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_uicGetActiveMenuItem({component:#x})");

    if component == 0 {
        return Ok(-1);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(-1);
    }
    if component_type != 1 {
        return Ok(-9);
    }

    read_generic(context, component + 0x48)
}

/// LGT `MC_uicGetListItem` (WIPI-C service 0x345).
///
/// Native is a thin wrapper over `WPUic_GetItem` with required component type 5.
/// Apart from the required List type, its behavior is identical to
/// `MC_uicGetMenuItem`.
///
/// List item state uses +0x44 as the item count and +0x50 as the pointer table.
/// Each table entry points to `[image: u32][NUL-terminated label]`.
///
/// NULL/invalid components return 0 and valid non-List components return -9.
/// The native range test is signed `count <= index`; an out-of-range index
/// returns 0.
///
/// Before writing either optional output, native requires
/// `strlen(label) + 1 <= buflen`. If the buffer is too small it returns -18,
/// even when the label output pointer itself is NULL. On success a non-NULL
/// image output receives the stored image word, a non-NULL label output receives
/// the complete NUL-terminated string, and the function returns 1.
pub async fn get_list_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    index: i32,
    label: WIPICWord,
    buflen: i32,
    image: WIPICWord,
) -> Result<i32> {
    tracing::debug!(
        "MC_uicGetListItem({component:#x}, {index}, {label:#x}, {buflen}, {image:#x})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 5 {
        return Ok(-9);
    }

    let count: i32 = read_generic(context, component + 0x44)?;
    if count <= index {
        return Ok(0);
    }

    let table: WIPICWord = read_generic(context, component + 0x50)?;
    let item: WIPICWord =
        read_generic(context, table.wrapping_add((index as u32).wrapping_mul(4)))?;

    let label_bytes = uic_read_c_string(context, item + 4)?;
    let required = (label_bytes.len() as i32).wrapping_add(1);
    if required > buflen {
        return Ok(-18);
    }

    if image != 0 {
        let stored_image: WIPICWord = read_generic(context, item)?;
        write_generic(context, image, stored_image)?;
    }

    if label != 0 {
        context.write_bytes(label, &label_bytes)?;
        context.write_bytes(label + label_bytes.len() as u32, &[0])?;
    }

    Ok(1)
}

/// LGT `MC_uicGetMenuItem` (WIPI-C service 0x33a).
///
/// Native is a thin wrapper over `WPUic_GetItem` with required component type 1.
/// NULL/invalid components return 0 and valid non-Menu components return -9.
///
/// Menu item state uses +0x44 as the item count and +0x50 as the pointer table.
/// Each table entry points to `[image: u32][NUL-terminated label]`.
///
/// The native range test is signed `count <= index`; an out-of-range index returns 0.
/// It then requires `strlen(label) + 1 <= buflen`, otherwise returning -18 even when
/// the caller did not request the label output. On success, a non-NULL image output
/// receives the stored image word first, then a non-NULL string output receives the
/// complete NUL-terminated label. Success returns 1.
pub async fn get_menu_item(
    context: &mut dyn WIPICContext,
    component: WIPICWord,
    index: i32,
    label: WIPICWord,
    buflen: i32,
    image: WIPICWord,
) -> Result<i32> {
    tracing::debug!(
        "MC_uicGetMenuItem({component:#x}, {index}, {label:#x}, {buflen}, {image:#x})"
    );

    if component == 0 {
        return Ok(0);
    }

    let component_type: WIPICWord = read_generic(context, component)?;
    if !(1..=5).contains(&component_type) {
        return Ok(0);
    }
    if component_type != 1 {
        return Ok(-9);
    }

    let count: i32 = read_generic(context, component + 0x44)?;
    if count <= index {
        return Ok(0);
    }

    let table: WIPICWord = read_generic(context, component + 0x50)?;
    let item: WIPICWord =
        read_generic(context, table.wrapping_add((index as u32).wrapping_mul(4)))?;

    let label_bytes = uic_read_c_string(context, item + 4)?;
    let required = (label_bytes.len() as i32).wrapping_add(1);
    if required > buflen {
        return Ok(-18);
    }

    if image != 0 {
        let stored_image: WIPICWord = read_generic(context, item)?;
        write_generic(context, image, stored_image)?;
    }

    if label != 0 {
        context.write_bytes(label, &label_bytes)?;
        context.write_bytes(label + label_bytes.len() as u32, &[0])?;
    }

    Ok(1)
}


#[cfg(test)]
mod tests {
    use wie_util::{ByteRead, ByteWrite, read_generic, write_generic};

    use crate::context::{WIPICContext, test::TestContext};

    use super::{
        UIC_DRAW_MARKER_BASE, UIC_EMPTY_LABEL, UIC_TIMER_MARKER_TEXT, configure, create,
        delete_text,
        add_list_item, add_menu_item, destroy, get_class, get_class_name, get_font, get_geometry, get_label,
        get_active_list_item, get_active_menu_item, get_list_item, get_menu_item, get_time, insert_text, is_instance,
        remove_list_item, remove_menu_item,
        repaint, set_active_list_item, set_active_menu_item, set_bg_color, set_callback,
        set_enable,
        set_event_handler,
        get_max_text_size, get_text, get_text_size, set_fg_color, set_font, set_label,
        set_label_alignment, set_max_text_size,
        set_time, set_time_long, set_time_mask, uic_color_to_rgb565, uic_read_c_string,
        uic_repaint_rect,
        uic_skip_time_separator,
    };

    const COMPONENT: u32 = 0x1000;

    #[futures_test::test]
    async fn lgt_uic_get_class_matches_native_class_name_table() {
        let mut context = TestContext::new();

        for (address, name, expected) in [
            (0x3000u32, b"MenuComponent\0".as_slice(), 1u32),
            (0x3040u32, b"DateTimeComponent\0".as_slice(), 2u32),
            (0x3080u32, b"TextComponent\0".as_slice(), 3u32),
            (0x30c0u32, b"LabelComponent\0".as_slice(), 4u32),
            (0x3100u32, b"ListComponent\0".as_slice(), 5u32),
        ] {
            context.write_bytes(address, name).unwrap();
            let result = get_class(&mut context, address).await.unwrap();
            assert!(result.0 == expected);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_class_returns_minus_one_for_null_and_unknown() {
        let mut context = TestContext::new();

        let result = get_class(&mut context, 0).await.unwrap();
        assert!(result.0 == u32::MAX);

        context.write_bytes(0x3000, b"menucomponent\0").unwrap();
        let result = get_class(&mut context, 0x3000).await.unwrap();
        assert!(result.0 == u32::MAX);

        context.write_bytes(0x3040, b"UnknownComponent\0").unwrap();
        let result = get_class(&mut context, 0x3040).await.unwrap();
        assert!(result.0 == u32::MAX);
    }

    #[futures_test::test]
    async fn lgt_uic_get_class_name_returns_native_names_at_stable_guest_pointers() {
        for (class, expected_ptr, expected_name) in [
            (1u32, 0x7fff_1010u32, b"MenuComponent\0".as_slice()),
            (2u32, 0x7fff_1020u32, b"DateTimeComponent\0".as_slice()),
            (3u32, 0x7fff_1038u32, b"TextComponent\0".as_slice()),
            (4u32, 0x7fff_1048u32, b"LabelComponent\0".as_slice()),
            (5u32, 0x7fff_1058u32, b"ListComponent\0".as_slice()),
        ] {
            let mut context = TestContext::new();
            context.write_bytes(COMPONENT, &class.to_le_bytes()).unwrap();

            let first = get_class_name(&mut context, COMPONENT).await.unwrap();
            let second = get_class_name(&mut context, COMPONENT).await.unwrap();

            assert_eq!(first.0, expected_ptr);
            assert_eq!(second.0, expected_ptr);

            let mut actual = alloc::vec![0u8; expected_name.len()];
            context.read_bytes(expected_ptr, &mut actual).unwrap();
            assert_eq!(actual.as_slice(), expected_name);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_class_name_returns_null_for_null_and_invalid_components() {
        let mut context = TestContext::new();

        assert_eq!(get_class_name(&mut context, 0).await.unwrap().0, 0);

        context.write_bytes(COMPONENT, &0u32.to_le_bytes()).unwrap();
        assert_eq!(get_class_name(&mut context, COMPONENT).await.unwrap().0, 0);

        context.write_bytes(COMPONENT, &6u32.to_le_bytes()).unwrap();
        assert_eq!(get_class_name(&mut context, COMPONENT).await.unwrap().0, 0);
    }

    #[futures_test::test]
    async fn lgt_uic_is_instance_matches_exact_native_class_names() {
        for (class, name) in [
            (1u32, b"MenuComponent\0".as_slice()),
            (2u32, b"DateTimeComponent\0".as_slice()),
            (3u32, b"TextComponent\0".as_slice()),
            (4u32, b"LabelComponent\0".as_slice()),
            (5u32, b"ListComponent\0".as_slice()),
        ] {
            let mut context = TestContext::new();
            context.write_bytes(COMPONENT, &class.to_le_bytes()).unwrap();
            context.write_bytes(0x3000, name).unwrap();

            assert_eq!(is_instance(&mut context, COMPONENT, 0x3000).await.unwrap(), 1);

            context.write_bytes(0x3040, b"UnknownComponent\0").unwrap();
            assert_eq!(is_instance(&mut context, COMPONENT, 0x3040).await.unwrap(), 0);

            let mut lower = name[..name.len() - 1].to_vec();
            lower[0] = lower[0].to_ascii_lowercase();
            lower.push(0);
            context.write_bytes(0x3080, &lower).unwrap();
            assert_eq!(is_instance(&mut context, COMPONENT, 0x3080).await.unwrap(), 0);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_is_instance_returns_zero_for_null_and_invalid_inputs() {
        let mut context = TestContext::new();
        context.write_bytes(0x3000, b"MenuComponent\0").unwrap();

        assert_eq!(is_instance(&mut context, 0, 0x3000).await.unwrap(), 0);

        context.write_bytes(COMPONENT, &1u32.to_le_bytes()).unwrap();
        assert_eq!(is_instance(&mut context, COMPONENT, 0).await.unwrap(), 0);

        context.write_bytes(COMPONENT, &0u32.to_le_bytes()).unwrap();
        assert_eq!(is_instance(&mut context, COMPONENT, 0x3000).await.unwrap(), 0);

        context.write_bytes(COMPONENT, &6u32.to_le_bytes()).unwrap();
        assert_eq!(is_instance(&mut context, COMPONENT, 0x3000).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_uic_create_rejects_invalid_class_like_native() {
        let mut context = TestContext::new();

        assert_eq!(create(&mut context, 0x12345678, 0).await.unwrap().0, u32::MAX);
        assert_eq!(create(&mut context, 0x12345678, 6).await.unwrap().0, u32::MAX);
    }

    #[futures_test::test]
    async fn lgt_uic_create_initializes_menu_list_label_and_text_layouts() {
        for class in [1u32, 3, 4, 5] {
            let mut context = TestContext::new();
            let component = create(&mut context, 0xdeadbeef, class).await.unwrap().0;

            assert_ne!(component, 0);
            assert_eq!(read_generic::<u32, _>(&context, component).unwrap(), class);
            assert_eq!(read_generic::<u32, _>(&context, component + 0x04).unwrap(), 0);
            assert_eq!(read_generic::<u32, _>(&context, component + 0x08).unwrap(), 0);
            assert_eq!(read_generic::<u32, _>(&context, component + 0x0c).unwrap(), 0x7fff);
            assert_eq!(read_generic::<u32, _>(&context, component + 0x10).unwrap(), 0x7fff);
            assert_eq!(read_generic::<u32, _>(&context, component + 0x14).unwrap(), 0);
            assert_eq!(read_generic::<u32, _>(&context, component + 0x18).unwrap(), 0);
            assert_eq!(
                read_generic::<u32, _>(&context, component + 0x1c).unwrap(),
                0x00ff_ffff
            );
            assert_eq!(
                read_generic::<u32, _>(&context, component + 0x24).unwrap(),
                UIC_DRAW_MARKER_BASE + class
            );
            for offset in [0x20u32, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40] {
                assert_eq!(read_generic::<u32, _>(&context, component + offset).unwrap(), 0);
            }

            match class {
                1 | 5 => {
                    assert_eq!(read_generic::<u32, _>(&context, component + 0x44).unwrap(), 0);
                    assert_eq!(read_generic::<i32, _>(&context, component + 0x48).unwrap(), -1);
                    for offset in [0x4cu32, 0x50, 0x54, 0x58] {
                        assert_eq!(
                            read_generic::<u32, _>(&context, component + offset).unwrap(),
                            0
                        );
                    }
                }
                3 => {
                    let text_ptr =
                        read_generic::<u32, _>(&context, component + 0x44).unwrap();
                    assert_ne!(text_ptr, 0);
                    assert_eq!(read_generic::<u8, _>(&context, text_ptr).unwrap(), 0);
                    assert_eq!(
                        read_generic::<u32, _>(&context, component + 0x48).unwrap(),
                        256
                    );
                    assert_eq!(
                        read_generic::<u32, _>(&context, component + 0x54).unwrap(),
                        UIC_TIMER_MARKER_TEXT
                    );
                    assert_eq!(
                        read_generic::<u32, _>(&context, component + 0x58).unwrap(),
                        1
                    );
                    for offset in [0x4cu32, 0x50, 0x5c, 0x60, 0x64, 0x68] {
                        assert_eq!(
                            read_generic::<u32, _>(&context, component + offset).unwrap(),
                            0
                        );
                    }
                }
                4 => {
                    assert_eq!(read_generic::<u32, _>(&context, component + 0x44).unwrap(), 0);
                    assert_eq!(read_generic::<u32, _>(&context, component + 0x48).unwrap(), 2);
                }
                _ => unreachable!(),
            }

        }
    }

    #[futures_test::test]
    async fn lgt_uic_create_initializes_datetime_layout() {
        let system = wie_backend::System::new(
            alloc::boxed::Box::new(test_utils::TestPlatform::new()),
            "test-pid",
            "test-aid",
            wie_backend::DefaultTaskRunner,
        );
        let mut context = TestContext::with_system(system);
        let component = create(&mut context, 0x13572468, 2).await.unwrap().0;

        assert_ne!(component, 0);
        assert_eq!(read_generic::<u32, _>(&context, component).unwrap(), 2);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x04).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x08).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x0c).unwrap(), 0x7fff);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x10).unwrap(), 0x7fff);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x14).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x18).unwrap(), 0);
        assert_eq!(
            read_generic::<u32, _>(&context, component + 0x1c).unwrap(),
            0x00ff_ffff
        );
        assert_eq!(
            read_generic::<u32, _>(&context, component + 0x24).unwrap(),
            UIC_DRAW_MARKER_BASE + 2
        );
        assert_eq!(read_generic::<u32, _>(&context, component + 0x44).unwrap(), 3);
        assert_eq!(read_generic::<u32, _>(&context, component + 0x94).unwrap(), 0);
        assert_eq!(
            read_generic::<u32, _>(&context, component + 0x98).unwrap(),
            super::UIC_TIMER_MARKER_TIME
        );
        assert_eq!(read_generic::<u32, _>(&context, component + 0x9c).unwrap(), 1);
        assert_eq!(read_generic::<u32, _>(&context, component + 0xa0).unwrap(), 0);
        assert_eq!(read_generic::<u32, _>(&context, component + 0xa4).unwrap(), 0);

        let mut formatted = [0u8; 32];
        context.read_bytes(component + 0x74, &mut formatted).unwrap();
        assert_ne!(formatted[0], 0);
    }

    #[futures_test::test]
    async fn lgt_uic_destroy_ignores_null_and_invalid_components() {
        let mut context = TestContext::new();

        destroy(&mut context, 0).await.unwrap();

        context.write_bytes(COMPONENT, &0u32.to_le_bytes()).unwrap();
        destroy(&mut context, COMPONENT).await.unwrap();

        context.write_bytes(COMPONENT, &6u32.to_le_bytes()).unwrap();
        destroy(&mut context, COMPONENT).await.unwrap();
        assert_eq!(read_generic::<u32, _>(&context, COMPONENT).unwrap(), 6);
    }

    #[futures_test::test]
    async fn lgt_uic_destroy_clears_created_component_type() {
        for class in [1u32, 3, 4, 5] {
            let mut context = TestContext::new();
            let component = create(&mut context, 0xfeedface, class).await.unwrap().0;

            destroy(&mut context, component).await.unwrap();

            assert_eq!(read_generic::<u32, _>(&context, component).unwrap(), 0);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_destroy_unsets_enabled_datetime_timer() {
        let system = wie_backend::System::new(
            alloc::boxed::Box::new(test_utils::TestPlatform::new()),
            "test-pid",
            "test-aid",
            wie_backend::DefaultTaskRunner,
        );
        let mut context = TestContext::with_system(system);
        let component = create(&mut context, 0, 2).await.unwrap().0;

        set_enable(&mut context, component, 1).await.unwrap();
        assert!(context.system().event_queue().has_timer(component + 0x98));

        destroy(&mut context, component).await.unwrap();

        assert!(!context.system().event_queue().has_timer(component + 0x98));
        assert_eq!(read_generic::<u32, _>(&context, component).unwrap(), 0);
    }

    #[test]
    fn lgt_uic_repaint_rect_matches_native_translation_and_inclusive_edges() {
        assert_eq!(
            uic_repaint_rect(10, 20, 100, 50, 3, 4, 7, 9),
            (13, 24, 19, 32)
        );
        assert_eq!(
            uic_repaint_rect(10, 20, 100, 50, 0, 0, -1, -1),
            (10, 20, 109, 69)
        );
    }

    #[futures_test::test]
    async fn lgt_uic_repaint_ignores_null_and_invalid_components() {
        let mut context = TestContext::new();

        repaint(&mut context, 0, 1, 2, 3, 4).await.unwrap();

        context.write_bytes(COMPONENT, &0u32.to_le_bytes()).unwrap();
        repaint(&mut context, COMPONENT, 1, 2, 3, 4).await.unwrap();

        context.write_bytes(COMPONENT, &6u32.to_le_bytes()).unwrap();
        repaint(&mut context, COMPONENT, 1, 2, 3, 4).await.unwrap();
        assert_eq!(read_generic::<u32, _>(&context, COMPONENT).unwrap(), 6);
    }

    #[test]
    fn lgt_uic_datetime_separator_skip_matches_native_single_step() {
        assert_eq!(uic_skip_time_separator(b"12:34", 2, true), 3);
        assert_eq!(uic_skip_time_separator(b"12/34", 2, true), 3);
        assert_eq!(uic_skip_time_separator(b"12\n34", 2, true), 3);

        // Native skips at most one separator per cursor move.
        assert_eq!(uic_skip_time_separator(b"12::34", 2, true), 3);

        // Space is part of the formatted DateTime string but is not one of
        // the native separator bytes checked by MC_uicHandleEvent.
        assert_eq!(uic_skip_time_separator(b"12 34", 2, true), 2);

        assert_eq!(uic_skip_time_separator(b"12:34", 2, false), 1);
        assert_eq!(uic_skip_time_separator(b"12 34", 2, false), 2);
    }

    fn read_i32(context: &TestContext, offset: u32) -> i32 {
        read_generic(context, COMPONENT + offset).unwrap()
    }

    fn init_component(context: &mut TestContext, component_type: u32) {
        write_generic(context, COMPONENT, component_type).unwrap();
        write_generic(context, COMPONENT + 0x04, 10i32).unwrap();
        write_generic(context, COMPONENT + 0x08, 20i32).unwrap();
        write_generic(context, COMPONENT + 0x0c, 30i32).unwrap();
        write_generic(context, COMPONENT + 0x10, 40i32).unwrap();
    }

    fn init_text_component(context: &mut TestContext, text: &[u8], capacity: u32, cursor: u32) {
        write_generic(context, COMPONENT, 3u32).unwrap();
        write_generic(context, COMPONENT + 0x44, 0x2000u32).unwrap();
        write_generic(context, COMPONENT + 0x48, capacity).unwrap();
        write_generic(context, COMPONENT + 0x4c, cursor).unwrap();
        write_generic(context, COMPONENT + 0x5c, 0u32).unwrap();
        write_generic(context, COMPONENT + 0x64, 0u32).unwrap();

        context.write_bytes(0x2000, text).unwrap();
    }

    fn read_text(context: &TestContext, max: usize) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec![0u8; max];
        context.read_bytes(0x2000, &mut buf).unwrap();
        let end = buf.iter().position(|&byte| byte == 0).unwrap_or(buf.len());
        buf.truncate(end);
        buf
    }

    #[futures_test::test]
    async fn lgt_uic_get_max_text_size_returns_native_capacity() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 0x1234_5678, 5);

        assert_eq!(
            get_max_text_size(&mut context, COMPONENT).await.unwrap(),
            0x1234_5678
        );

        write_generic(&mut context, COMPONENT + 0x48, 0xffff_fffeu32).unwrap();
        assert_eq!(
            get_max_text_size(&mut context, COMPONENT).await.unwrap(),
            -2
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_max_text_size_matches_native_validation_and_type_contract() {
        let mut context = TestContext::new();

        assert_eq!(get_max_text_size(&mut context, 0).await.unwrap(), 0);

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x1122_3344u32).unwrap();

            assert_eq!(
                get_max_text_size(&mut context, COMPONENT).await.unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
                0x1122_3344
            );
        }

        for component_type in [1u32, 2, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x5566_7788u32).unwrap();

            assert_eq!(
                get_max_text_size(&mut context, COMPONENT).await.unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
                0x5566_7788
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_text_size_returns_native_strlen() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 0x1234_5678, 5);

        assert_eq!(get_text_size(&mut context, COMPONENT).await.unwrap(), 6);

        context.write_bytes(0x2000, b"a\0cdef\0").unwrap();
        assert_eq!(get_text_size(&mut context, COMPONENT).await.unwrap(), 1);

        context.write_bytes(0x2000, b"\0").unwrap();
        assert_eq!(get_text_size(&mut context, COMPONENT).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_uic_get_text_size_matches_native_validation_and_null_text_contract() {
        let mut context = TestContext::new();

        assert_eq!(get_text_size(&mut context, 0).await.unwrap(), 0);

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x2000u32).unwrap();
            context.write_bytes(0x2000, b"ignored\0").unwrap();

            assert_eq!(
                get_text_size(&mut context, COMPONENT).await.unwrap(),
                0
            );
        }

        for component_type in [1u32, 2, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x2000u32).unwrap();
            context.write_bytes(0x2000, b"ignored\0").unwrap();

            assert_eq!(
                get_text_size(&mut context, COMPONENT).await.unwrap(),
                -9
            );
        }

        init_component(&mut context, 3);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x48, 0x5566_7788u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x4c, 0x1122_3344u32).unwrap();

        assert_eq!(
            get_text_size(&mut context, COMPONENT).await.unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
            0x5566_7788
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x4c).unwrap(),
            0x1122_3344
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_text_copies_native_suffix_and_returns_strlen() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 16, 5);

        context.write_bytes(0x3000, &[0xcc; 16]).unwrap();

        assert_eq!(
            get_text(&mut context, COMPONENT, 2, 0x3000, 5)
                .await
                .unwrap(),
            4
        );

        let mut actual = [0u8; 8];
        context.read_bytes(0x3000, &mut actual).unwrap();
        assert_eq!(&actual[..5], b"cdef\0");

        context.write_bytes(0x3000, &[0xcc; 16]).unwrap();

        assert_eq!(
            get_text(&mut context, COMPONENT, -123, 0x3000, 7)
                .await
                .unwrap(),
            6
        );

        context.read_bytes(0x3000, &mut actual).unwrap();
        assert_eq!(&actual[..7], b"abcdef\0");
    }

    #[futures_test::test]
    async fn lgt_uic_get_text_matches_native_range_and_capacity_contract() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 16, 5);

        context.write_bytes(0x3000, &[0xcc; 16]).unwrap();

        // position 2 leaves four bytes. Native requires position + buflen
        // to be strictly greater than strlen, so buflen == 4 fails.
        assert_eq!(
            get_text(&mut context, COMPONENT, 2, 0x3000, 4)
                .await
                .unwrap(),
            0
        );

        let mut unchanged = [0u8; 16];
        context.read_bytes(0x3000, &mut unchanged).unwrap();
        assert_eq!(unchanged, [0xcc; 16]);

        assert_eq!(
            get_text(&mut context, COMPONENT, 7, 0x3000, 16)
                .await
                .unwrap(),
            0
        );

        // position == strlen passes the first comparison, copies zero bytes,
        // writes a NUL, and returns strlen(output) == 0.
        context.write_bytes(0x3000, &[0xcc; 16]).unwrap();
        assert_eq!(
            get_text(&mut context, COMPONENT, 6, 0x3000, 1)
                .await
                .unwrap(),
            0
        );
        assert_eq!(read_generic::<u8, _>(&context, 0x3000).unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_uic_get_text_matches_native_validation_order() {
        let mut context = TestContext::new();

        assert_eq!(
            get_text(&mut context, 0, 0, 0x3000, 16).await.unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            assert_eq!(
                get_text(&mut context, COMPONENT, 0, 0x3000, 16)
                    .await
                    .unwrap(),
                0
            );
        }

        // Output/buffer validation occurs before the Text type check.
        for component_type in [1u32, 2, 4, 5] {
            init_component(&mut context, component_type);

            assert_eq!(
                get_text(&mut context, COMPONENT, 0, 0, 16)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                get_text(&mut context, COMPONENT, 0, 0x3000, 0)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                get_text(&mut context, COMPONENT, 0, 0x3000, -1)
                    .await
                    .unwrap(),
                0
            );

            assert_eq!(
                get_text(&mut context, COMPONENT, 0, 0x3000, 16)
                    .await
                    .unwrap(),
                -9
            );
        }

        init_component(&mut context, 3);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        assert_eq!(
            get_text(&mut context, COMPONENT, 0, 0x3000, 16)
                .await
                .unwrap(),
            0
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_max_text_size_matches_native_resize_and_return_contract() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 16, 5);

        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, 5).await.unwrap(),
            16
        );
        assert_eq!(read_i32(&context, 0x48), 5);

        let text_ptr: u32 = read_generic(&context, COMPONENT + 0x44).unwrap();
        let mut bytes = [0u8; 5];
        context.read_bytes(text_ptr, &mut bytes).unwrap();
        assert_eq!(&bytes, b"abcd\0");

        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, 12).await.unwrap(),
            5
        );
        assert_eq!(read_i32(&context, 0x48), 12);

        let text_ptr: u32 = read_generic(&context, COMPONENT + 0x44).unwrap();
        let last: u8 = read_generic(&context, text_ptr + 11).unwrap();
        assert_eq!(last, 0);
    }

    #[futures_test::test]
    async fn lgt_uic_set_max_text_size_matches_native_same_size_and_validation() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 16, 5);

        let old_ptr: u32 = read_generic(&context, COMPONENT + 0x44).unwrap();
        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, 16).await.unwrap(),
            16
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            old_ptr
        );
        let last: u8 = read_generic(&context, old_ptr + 15).unwrap();
        assert_eq!(last, 0);

        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, -1).await.unwrap(),
            0
        );

        write_generic(&mut context, COMPONENT, 4u32).unwrap();
        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, 8).await.unwrap(),
            -9
        );

        write_generic(&mut context, COMPONENT, 6u32).unwrap();
        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, 8).await.unwrap(),
            0
        );

        assert_eq!(set_max_text_size(&mut context, 0, 8).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_uic_set_max_text_size_preserves_native_zero_size_quirk() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcd\0", 16, 2);

        let old_ptr: u32 = read_generic(&context, COMPONENT + 0x44).unwrap();

        assert_eq!(
            set_max_text_size(&mut context, COMPONENT, 0).await.unwrap(),
            -17
        );

        // Native realloc(ptr, 0) frees the allocation but MC_uicSetMaxTextSize
        // leaves both component fields untouched after the NULL return.
        assert_eq!(read_i32(&context, 0x48), 16);
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            old_ptr
        );
    }

    #[futures_test::test]
    async fn lgt_uic_delete_text_matches_native_partial_and_to_end_rules() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 16, 5);

        delete_text(&mut context, COMPONENT, 2, 2).await.unwrap();
        assert_eq!(read_text(&context, 16), b"abef");
        assert_eq!(read_i32(&context, 0x4c), 3);

        context.write_bytes(0x2000, b"abcdef\0").unwrap();
        write_generic(&mut context, COMPONENT + 0x4c, 5i32).unwrap();
        delete_text(&mut context, COMPONENT, 3, -1).await.unwrap();
        assert_eq!(read_text(&context, 16), b"abc");
        assert_eq!(read_i32(&context, 0x4c), 3);

        context.write_bytes(0x2000, b"abcdef\0").unwrap();
        write_generic(&mut context, COMPONENT + 0x4c, 5i32).unwrap();
        delete_text(&mut context, COMPONENT, 2, 99).await.unwrap();
        assert_eq!(read_text(&context, 16), b"ab");
        assert_eq!(read_i32(&context, 0x4c), 2);
    }

    #[futures_test::test]
    async fn lgt_uic_delete_text_matches_native_cursor_clamp_and_validation() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcdef\0", 16, 1);

        delete_text(&mut context, COMPONENT, 3, 2).await.unwrap();
        assert_eq!(read_text(&context, 16), b"abcf");
        assert_eq!(read_i32(&context, 0x4c), 0);

        context.write_bytes(0x2000, b"abcdef\0").unwrap();
        write_generic(&mut context, COMPONENT + 0x4c, 4i32).unwrap();

        for (position, length) in [(-1, 1), (7, 1), (0, 0), (0, -2)] {
            delete_text(&mut context, COMPONENT, position, length)
                .await
                .unwrap();
            assert_eq!(read_text(&context, 16), b"abcdef");
            assert_eq!(read_i32(&context, 0x4c), 4);
        }

        write_generic(&mut context, COMPONENT, 4u32).unwrap();
        delete_text(&mut context, COMPONENT, 0, 1).await.unwrap();
        assert_eq!(read_text(&context, 16), b"abcdef");

        write_generic(&mut context, COMPONENT, 6u32).unwrap();
        delete_text(&mut context, COMPONENT, 0, 1).await.unwrap();
        assert_eq!(read_text(&context, 16), b"abcdef");

        delete_text(&mut context, 0, 0, 1).await.unwrap();
    }

    #[futures_test::test]
    async fn lgt_uic_insert_text_inserts_and_clamps_position_like_native() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcd\0", 16, 3);
        context.write_bytes(0x3000, b"XY").unwrap();

        assert_eq!(insert_text(&mut context, COMPONENT, 2, 0x3000, 2).await.unwrap(), 2);
        assert_eq!(read_text(&context, 16), b"abXYcd");
        assert_eq!(read_i32(&context, 0x4c), 4);

        context.write_bytes(0x2000, b"abcd\0").unwrap();
        write_generic(&mut context, COMPONENT + 0x4c, 1u32).unwrap();
        assert_eq!(insert_text(&mut context, COMPONENT, -99, 0x3000, 2).await.unwrap(), 2);
        assert_eq!(read_text(&context, 16), b"XYabcd");
        assert_eq!(read_i32(&context, 0x4c), 2);

        context.write_bytes(0x2000, b"abcd\0").unwrap();
        write_generic(&mut context, COMPONENT + 0x4c, 1u32).unwrap();
        assert_eq!(insert_text(&mut context, COMPONENT, 99, 0x3000, 2).await.unwrap(), 2);
        assert_eq!(read_text(&context, 16), b"abcdXY");
        assert_eq!(read_i32(&context, 0x4c), 1);
    }

    #[futures_test::test]
    async fn lgt_uic_insert_text_matches_native_validation_and_capacity_rule() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcd\0", 5, 2);
        context.write_bytes(0x3000, b"X").unwrap();

        // Native fails when capacity <= old_len + insert_len.
        assert_eq!(insert_text(&mut context, COMPONENT, 2, 0x3000, 1).await.unwrap(), -17);
        assert_eq!(read_text(&context, 8), b"abcd");

        write_generic(&mut context, COMPONENT + 0x48, 6u32).unwrap();
        assert_eq!(insert_text(&mut context, COMPONENT, 2, 0x3000, 1).await.unwrap(), 1);
        assert_eq!(read_text(&context, 8), b"abXcd");

        write_generic(&mut context, COMPONENT, 4u32).unwrap();
        assert_eq!(insert_text(&mut context, COMPONENT, 0, 0x3000, 1).await.unwrap(), -9);

        write_generic(&mut context, COMPONENT, 6u32).unwrap();
        assert_eq!(insert_text(&mut context, COMPONENT, 0, 0x3000, 1).await.unwrap(), 0);

        assert_eq!(insert_text(&mut context, 0, 0, 0x3000, 1).await.unwrap(), 0);

        write_generic(&mut context, COMPONENT, 3u32).unwrap();
        assert_eq!(insert_text(&mut context, COMPONENT, 0, 0, 1).await.unwrap(), 0);
        assert_eq!(insert_text(&mut context, COMPONENT, 0, 0x3000, 0).await.unwrap(), 0);
        assert_eq!(insert_text(&mut context, COMPONENT, 0, 0x3000, -1).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_uic_insert_text_preserves_native_embedded_nul_behavior() {
        let mut context = TestContext::new();
        init_text_component(&mut context, b"abcd\0", 16, 4);
        context.write_bytes(0x3000, &[0, b'X']).unwrap();

        assert_eq!(insert_text(&mut context, COMPONENT, 4, 0x3000, 2).await.unwrap(), 2);

        // Raw bytes were inserted, but as a C string the visible text remains "abcd".
        assert_eq!(read_text(&context, 16), b"abcd");
        assert_eq!(read_i32(&context, 0x4c), 6);
    }

    #[futures_test::test]
    async fn lgt_uic_set_bg_color_matches_native_conversion_store_and_return() {
        for component_type in 1u32..=5 {
            let mut context = TestContext::new();
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x1c, 0xdead_beefu32).unwrap();

            assert_eq!(
                set_bg_color(&mut context, COMPONENT, 0x0012_3456)
                    .await
                    .unwrap(),
                0x11aa
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x1c).unwrap(),
                0x11aa
            );
        }

        let mut context = TestContext::new();
        assert_eq!(
            set_bg_color(&mut context, 0, 0x00ff_0000)
                .await
                .unwrap(),
            0
        );

        init_component(&mut context, 6);
        write_generic(&mut context, COMPONENT + 0x1c, 0xaabb_ccddu32).unwrap();
        assert_eq!(
            set_bg_color(&mut context, COMPONENT, 0x00ff_0000)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x1c).unwrap(),
            0xaabb_ccdd
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_label_matches_native_validation_and_copy_contract() {
        let mut context = TestContext::new();

        assert_eq!(set_label(&mut context, 0, 0x3000).await.unwrap(), 0);

        init_component(&mut context, 6);
        context.write_bytes(0x3000, b"invalid\0").unwrap();
        assert_eq!(set_label(&mut context, COMPONENT, 0x3000).await.unwrap(), 0);

        init_component(&mut context, 1);
        write_generic(&mut context, COMPONENT + 0x44, 0xdead_beefu32).unwrap();
        assert_eq!(set_label(&mut context, COMPONENT, 0).await.unwrap(), 1);
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            0xdead_beef
        );

        context.write_bytes(0x3000, b"menu\0").unwrap();
        assert_eq!(set_label(&mut context, COMPONENT, 0x3000).await.unwrap(), 1);
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            0xdead_beef
        );

        init_component(&mut context, 4);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        context.write_bytes(0x3000, b"Label\0").unwrap();
        let result = set_label(&mut context, COMPONENT, 0x3000).await.unwrap();
        assert_ne!(result, 0);
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            result
        );

        let mut copied = [0u8; 7];
        context.read_bytes(result, &mut copied).unwrap();
        assert_eq!(&copied[..6], b"Label\0");
        assert_eq!(copied[6], 0);
    }

    #[futures_test::test]
    async fn lgt_uic_set_label_preserves_or_replaces_pointer_by_actual_capacity() {
        let mut context = TestContext::new();
        init_component(&mut context, 4);

        context.write_bytes(0x3000, b"abcdefghij\0").unwrap();
        let first = set_label(&mut context, COMPONENT, 0x3000).await.unwrap();
        assert_ne!(first, 0);

        context.write_bytes(0x3040, b"abc\0").unwrap();
        let smaller = set_label(&mut context, COMPONENT, 0x3040).await.unwrap();
        assert_eq!(smaller, first);

        context
            .write_bytes(0x3080, b"abcdefghijklmnop\0")
            .unwrap();
        let larger = set_label(&mut context, COMPONENT, 0x3080).await.unwrap();
        assert_ne!(larger, 0);
        assert_ne!(larger, first);
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            larger
        );

        let mut copied = [0u8; 18];
        context.read_bytes(larger, &mut copied).unwrap();
        assert_eq!(&copied[..17], b"abcdefghijklmnop\0");
        assert_eq!(copied[17], 0);
    }

    #[futures_test::test]
    async fn lgt_uic_get_label_returns_native_label_pointer_or_static_empty_string() {
        let mut context = TestContext::new();

        init_component(&mut context, 4);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();

        let empty_first = get_label(&mut context, COMPONENT).await.unwrap();
        let empty_second = get_label(&mut context, COMPONENT).await.unwrap();
        assert_eq!(empty_first.0, UIC_EMPTY_LABEL);
        assert_eq!(empty_second.0, UIC_EMPTY_LABEL);

        let mut empty = [0xffu8; 1];
        context.read_bytes(UIC_EMPTY_LABEL, &mut empty).unwrap();
        assert_eq!(empty, [0]);

        context.write_bytes(0x3000, b"native label\0").unwrap();
        write_generic(&mut context, COMPONENT + 0x44, 0x3000u32).unwrap();

        assert_eq!(
            get_label(&mut context, COMPONENT).await.unwrap().0,
            0x3000
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_label_returns_null_for_null_invalid_and_non_label_components() {
        let mut context = TestContext::new();

        assert_eq!(get_label(&mut context, 0).await.unwrap().0, 0);

        init_component(&mut context, 0);
        write_generic(&mut context, COMPONENT + 0x44, 0x3000u32).unwrap();
        assert_eq!(get_label(&mut context, COMPONENT).await.unwrap().0, 0);

        init_component(&mut context, 6);
        assert_eq!(get_label(&mut context, COMPONENT).await.unwrap().0, 0);

        for component_type in [1u32, 2, 3, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x3000u32).unwrap();
            assert_eq!(get_label(&mut context, COMPONENT).await.unwrap().0, 0);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_label_alignment_returns_previous_and_updates_native_slot() {
        for (new_alignment, expected_old) in [(0u32, 2i32), (1u32, 0i32), (2u32, 1i32)] {
            let mut context = TestContext::new();
            init_component(&mut context, 4);
            write_generic(&mut context, COMPONENT + 0x48, expected_old).unwrap();

            assert_eq!(
                set_label_alignment(&mut context, COMPONENT, new_alignment)
                    .await
                    .unwrap(),
                expected_old
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
                new_alignment
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_label_alignment_matches_native_validation_and_range_errors() {
        let mut context = TestContext::new();

        assert_eq!(
            set_label_alignment(&mut context, 0, 1).await.unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x1122_3344u32).unwrap();
            assert_eq!(
                set_label_alignment(&mut context, COMPONENT, 1)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
                0x1122_3344
            );
        }

        for component_type in [1u32, 2, 3, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x5566_7788u32).unwrap();
            assert_eq!(
                set_label_alignment(&mut context, COMPONENT, 1)
                    .await
                    .unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
                0x5566_7788
            );
        }

        init_component(&mut context, 4);
        for alignment in [3u32, u32::MAX] {
            write_generic(&mut context, COMPONENT + 0x48, 2u32).unwrap();
            assert_eq!(
                set_label_alignment(&mut context, COMPONENT, alignment)
                    .await
                    .unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x48).unwrap(),
                2
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_mask_returns_previous_updates_mask_and_reformats() {
        for mask in [0u32, 4, 8, 0xffff_fffc] {
            let mut context = TestContext::new();
            init_component(&mut context, 2);

            write_generic(&mut context, COMPONENT + 0x44, 3u32).unwrap();

            let fields = [
                5i32,   // sec
                4,      // min
                3,      // hour
                2,      // mday
                0,      // mon: January
                124,    // year: 2024
                2,
                1,
                0,
            ];
            for (index, value) in fields.iter().enumerate() {
                write_generic(
                    &mut context,
                    COMPONENT + 0x48 + index as u32 * 4,
                    *value,
                )
                .unwrap();
            }

            context
                .write_bytes(COMPONENT + 0x74, b"2024/01/02 03:04:05\0")
                .unwrap();

            assert_eq!(
                set_time_mask(&mut context, COMPONENT, mask).await.unwrap(),
                3
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                mask
            );

            let rendered = uic_read_c_string(&context, COMPONENT + 0x74).unwrap();
            assert_eq!(rendered, b"2024/01/02");
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_mask_matches_native_validation_and_mask_rule() {
        let mut context = TestContext::new();

        assert_eq!(set_time_mask(&mut context, 0, 0).await.unwrap(), 0);

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x1122_3344u32).unwrap();

            assert_eq!(
                set_time_mask(&mut context, COMPONENT, 0).await.unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0x1122_3344
            );
        }

        for component_type in [1u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x5566_7788u32).unwrap();

            assert_eq!(
                set_time_mask(&mut context, COMPONENT, 0).await.unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0x5566_7788
            );
        }

        init_component(&mut context, 2);
        for mask in [1u32, 2, 3, 5, u32::MAX] {
            write_generic(&mut context, COMPONENT + 0x44, 0x1234_5678u32).unwrap();

            assert_eq!(
                set_time_mask(&mut context, COMPONENT, mask).await.unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0x1234_5678
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_add_list_item_builds_native_pointer_table_and_item_blocks() {
        let mut context = TestContext::new();
        init_component(&mut context, 5);

        context.write_bytes(0x3000, b"First list item\0").unwrap();

        assert_eq!(
            add_list_item(&mut context, COMPONENT, 0x3000, 0x1122_3344)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            1
        );

        let first_table: u32 =
            read_generic(&context, COMPONENT + 0x50).unwrap();
        assert_ne!(first_table, 0);

        let first_item: u32 = read_generic(&context, first_table).unwrap();
        assert_ne!(first_item, 0);
        assert_eq!(
            read_generic::<u32, _>(&context, first_item).unwrap(),
            0x1122_3344
        );
        assert_eq!(
            uic_read_c_string(&context, first_item + 4).unwrap(),
            b"First list item"
        );

        assert_eq!(
            add_list_item(&mut context, COMPONENT, 0, 0xaabb_ccdd)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            2
        );

        let second_table: u32 =
            read_generic(&context, COMPONENT + 0x50).unwrap();
        assert_ne!(second_table, 0);

        assert_eq!(
            read_generic::<u32, _>(&context, second_table).unwrap(),
            first_item
        );

        let second_item: u32 =
            read_generic(&context, second_table + 4).unwrap();
        assert_ne!(second_item, 0);
        assert_eq!(
            read_generic::<u32, _>(&context, second_item).unwrap(),
            0xaabb_ccdd
        );
        assert_eq!(
            uic_read_c_string(&context, second_item + 4).unwrap(),
            b""
        );
    }

    #[futures_test::test]
    async fn lgt_uic_add_list_item_matches_native_validation_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            add_list_item(&mut context, 0, 0, 0).await.unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x1234u32).unwrap();
            write_generic(&mut context, COMPONENT + 0x50, 0x5678u32).unwrap();

            assert_eq!(
                add_list_item(&mut context, COMPONENT, 0, 0)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0x1234
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x50).unwrap(),
                0x5678
            );
        }

        for component_type in [1u32, 2, 3, 4] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
            write_generic(&mut context, COMPONENT + 0x50, 0u32).unwrap();

            assert_eq!(
                add_list_item(&mut context, COMPONENT, 0, 0)
                    .await
                    .unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x50).unwrap(),
                0
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_add_menu_item_builds_native_pointer_table_and_item_blocks() {
        let mut context = TestContext::new();
        init_component(&mut context, 1);

        context.write_bytes(0x3000, b"First item\0").unwrap();

        assert_eq!(
            add_menu_item(&mut context, COMPONENT, 0x3000, 0x1122_3344)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            1
        );

        let first_table: u32 =
            read_generic(&context, COMPONENT + 0x50).unwrap();
        assert_ne!(first_table, 0);
        let first_item: u32 = read_generic(&context, first_table).unwrap();
        assert_ne!(first_item, 0);
        assert_eq!(
            read_generic::<u32, _>(&context, first_item).unwrap(),
            0x1122_3344
        );
        assert_eq!(
            uic_read_c_string(&context, first_item + 4).unwrap(),
            b"First item"
        );

        assert_eq!(
            add_menu_item(&mut context, COMPONENT, 0, 0xaabb_ccdd)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            2
        );

        let second_table: u32 =
            read_generic(&context, COMPONENT + 0x50).unwrap();
        assert_ne!(second_table, 0);

        let preserved_first: u32 =
            read_generic(&context, second_table).unwrap();
        assert_eq!(preserved_first, first_item);

        let second_item: u32 =
            read_generic(&context, second_table + 4).unwrap();
        assert_ne!(second_item, 0);
        assert_eq!(
            read_generic::<u32, _>(&context, second_item).unwrap(),
            0xaabb_ccdd
        );
        assert_eq!(
            uic_read_c_string(&context, second_item + 4).unwrap(),
            b""
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_list_item_reads_native_item_layout_and_optional_outputs() {
        let mut context = TestContext::new();
        init_component(&mut context, 5);

        context.write_bytes(0x3000, b"List label\0").unwrap();
        assert_eq!(
            add_list_item(&mut context, COMPONENT, 0x3000, 0x1122_3344)
                .await
                .unwrap(),
            0
        );

        context.write_bytes(0x3100, &[0xaa; 32]).unwrap();
        write_generic(&mut context, 0x3200, 0xdead_beefu32).unwrap();

        assert_eq!(
            get_list_item(&mut context, COMPONENT, 0, 0x3100, 32, 0x3200)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            uic_read_c_string(&context, 0x3100).unwrap(),
            b"List label"
        );
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3200).unwrap(),
            0x1122_3344
        );

        context.write_bytes(0x3100, &[0xbb; 32]).unwrap();
        write_generic(&mut context, 0x3200, 0u32).unwrap();

        assert_eq!(
            get_list_item(&mut context, COMPONENT, 0, 0, 32, 0x3200)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3200).unwrap(),
            0x1122_3344
        );

        assert_eq!(
            get_list_item(&mut context, COMPONENT, 0, 0x3100, 32, 0)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            uic_read_c_string(&context, 0x3100).unwrap(),
            b"List label"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_list_item_matches_native_validation_range_and_buffer_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            get_list_item(&mut context, 0, 0, 0x3100, 32, 0x3200)
                .await
                .unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            assert_eq!(
                get_list_item(&mut context, COMPONENT, 0, 0x3100, 32, 0x3200)
                    .await
                    .unwrap(),
                0
            );
        }

        for component_type in [1u32, 2, 3, 4] {
            init_component(&mut context, component_type);
            assert_eq!(
                get_list_item(&mut context, COMPONENT, 0, 0x3100, 32, 0x3200)
                    .await
                    .unwrap(),
                -9
            );
        }

        init_component(&mut context, 5);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x48, -1i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x50, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x54, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x58, 0u32).unwrap();

        context.write_bytes(0x3000, b"abcd\0").unwrap();
        assert_eq!(
            add_list_item(&mut context, COMPONENT, 0x3000, 0xaabb_ccdd)
                .await
                .unwrap(),
            0
        );

        context.write_bytes(0x3100, &[0xcc; 16]).unwrap();
        write_generic(&mut context, 0x3200, 0x5566_7788u32).unwrap();

        assert_eq!(
            get_list_item(&mut context, COMPONENT, 1, 0x3100, 16, 0x3200)
                .await
                .unwrap(),
            0
        );

        assert_eq!(
            get_list_item(&mut context, COMPONENT, 0, 0, 4, 0)
                .await
                .unwrap(),
            -18
        );
        assert_eq!(
            get_list_item(&mut context, COMPONENT, 0, 0x3100, 4, 0x3200)
                .await
                .unwrap(),
            -18
        );

        let mut unchanged = [0u8; 16];
        context.read_bytes(0x3100, &mut unchanged).unwrap();
        assert_eq!(unchanged, [0xcc; 16]);
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3200).unwrap(),
            0x5566_7788
        );

        assert_eq!(
            get_list_item(&mut context, COMPONENT, 0, 0x3100, 5, 0x3200)
                .await
                .unwrap(),
            1
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_menu_item_reads_native_item_layout_and_optional_outputs() {
        let mut context = TestContext::new();
        init_component(&mut context, 1);

        context.write_bytes(0x3000, b"Menu label\0").unwrap();
        assert_eq!(
            add_menu_item(&mut context, COMPONENT, 0x3000, 0x1122_3344)
                .await
                .unwrap(),
            0
        );

        context.write_bytes(0x3100, &[0xaa; 32]).unwrap();
        write_generic(&mut context, 0x3200, 0xdead_beefu32).unwrap();

        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 0, 0x3100, 32, 0x3200)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            uic_read_c_string(&context, 0x3100).unwrap(),
            b"Menu label"
        );
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3200).unwrap(),
            0x1122_3344
        );

        context.write_bytes(0x3100, &[0xbb; 32]).unwrap();
        write_generic(&mut context, 0x3200, 0u32).unwrap();

        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 0, 0, 32, 0x3200)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3200).unwrap(),
            0x1122_3344
        );

        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 0, 0x3100, 32, 0)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            uic_read_c_string(&context, 0x3100).unwrap(),
            b"Menu label"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_remove_list_item_shifts_native_table_and_preserves_active_index() {
        let mut context = TestContext::new();
        init_component(&mut context, 5);

        for (address, label, image) in [
            (0x3000u32, b"zero\0".as_slice(), 0u32),
            (0x3040u32, b"one\0".as_slice(), 0u32),
            (0x3080u32, b"two\0".as_slice(), 0u32),
        ] {
            context.write_bytes(address, label).unwrap();
            assert_eq!(
                add_list_item(&mut context, COMPONENT, address, image)
                    .await
                    .unwrap(),
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap() as i32 - 1
            );
        }

        write_generic(&mut context, COMPONENT + 0x48, 2i32).unwrap();

        let table: u32 = read_generic(&context, COMPONENT + 0x50).unwrap();
        let first: u32 = read_generic(&context, table).unwrap();
        let second: u32 = read_generic(&context, table + 4).unwrap();
        let third: u32 = read_generic(&context, table + 8).unwrap();

        assert_ne!(first, second);
        assert_ne!(second, third);

        assert_eq!(
            remove_list_item(&mut context, COMPONENT, 1).await.unwrap(),
            1
        );

        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            2
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            2
        );

        assert_eq!(
            read_generic::<u32, _>(&context, table).unwrap(),
            first
        );
        assert_eq!(
            read_generic::<u32, _>(&context, table + 4).unwrap(),
            third
        );
        assert_eq!(
            read_generic::<u32, _>(&context, table + 8).unwrap(),
            third
        );

        assert_eq!(
            uic_read_c_string(&context, third + 4).unwrap(),
            b"two"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_remove_list_item_matches_native_validation_and_range_rule() {
        let mut context = TestContext::new();

        assert_eq!(
            remove_list_item(&mut context, 0, 0).await.unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 3u32).unwrap();
            write_generic(&mut context, COMPONENT + 0x50, 0x5678u32).unwrap();

            assert_eq!(
                remove_list_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                3
            );
        }

        for component_type in [1u32, 2, 3, 4] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 3u32).unwrap();

            assert_eq!(
                remove_list_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                3
            );
        }

        init_component(&mut context, 5);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x48, -1i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x50, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x54, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x58, 0u32).unwrap();

        context.write_bytes(0x3000, b"only\0").unwrap();
        assert_eq!(
            add_list_item(&mut context, COMPONENT, 0x3000, 0)
                .await
                .unwrap(),
            0
        );

        let table: u32 = read_generic(&context, COMPONENT + 0x50).unwrap();
        let only: u32 = read_generic(&context, table).unwrap();

        assert_eq!(
            remove_list_item(&mut context, COMPONENT, 2)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            1
        );
        assert_eq!(
            read_generic::<u32, _>(&context, table).unwrap(),
            only
        );
    }

    #[futures_test::test]
    async fn lgt_uic_remove_menu_item_shifts_native_table_and_preserves_active_index() {
        let mut context = TestContext::new();
        init_component(&mut context, 1);

        for (address, label, image) in [
            (0x3000u32, b"zero\0".as_slice(), 0u32),
            (0x3040u32, b"one\0".as_slice(), 0u32),
            (0x3080u32, b"two\0".as_slice(), 0u32),
        ] {
            context.write_bytes(address, label).unwrap();
            assert_eq!(
                add_menu_item(&mut context, COMPONENT, address, image)
                    .await
                    .unwrap(),
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap() as i32 - 1
            );
        }

        write_generic(&mut context, COMPONENT + 0x48, 2i32).unwrap();

        let table: u32 = read_generic(&context, COMPONENT + 0x50).unwrap();
        let first: u32 = read_generic(&context, table).unwrap();
        let second: u32 = read_generic(&context, table + 4).unwrap();
        let third: u32 = read_generic(&context, table + 8).unwrap();

        assert_ne!(first, second);
        assert_ne!(second, third);

        assert_eq!(
            remove_menu_item(&mut context, COMPONENT, 1).await.unwrap(),
            1
        );

        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            2
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            2
        );

        assert_eq!(
            read_generic::<u32, _>(&context, table).unwrap(),
            first
        );
        assert_eq!(
            read_generic::<u32, _>(&context, table + 4).unwrap(),
            third
        );
        assert_eq!(
            read_generic::<u32, _>(&context, table + 8).unwrap(),
            third
        );

        assert_eq!(
            uic_read_c_string(&context, third + 4).unwrap(),
            b"two"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_remove_menu_item_matches_native_validation_and_range_rule() {
        let mut context = TestContext::new();

        assert_eq!(
            remove_menu_item(&mut context, 0, 0).await.unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 3u32).unwrap();
            write_generic(&mut context, COMPONENT + 0x50, 0x5678u32).unwrap();

            assert_eq!(
                remove_menu_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                3
            );
        }

        for component_type in [2u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 3u32).unwrap();

            assert_eq!(
                remove_menu_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                0
            );
        }

        init_component(&mut context, 1);
        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x48, -1i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x50, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x54, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x58, 0u32).unwrap();

        context.write_bytes(0x3000, b"only\0").unwrap();
        assert_eq!(
            add_menu_item(&mut context, COMPONENT, 0x3000, 0)
                .await
                .unwrap(),
            0
        );

        let table: u32 = read_generic(&context, COMPONENT + 0x50).unwrap();
        let only: u32 = read_generic(&context, table).unwrap();

        assert_eq!(
            remove_menu_item(&mut context, COMPONENT, 2)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
            1
        );
        assert_eq!(
            read_generic::<u32, _>(&context, table).unwrap(),
            only
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_active_list_item_matches_native_validation_and_type_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            get_active_list_item(&mut context, 0).await.unwrap(),
            -1
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x1234_5678i32).unwrap();

            assert_eq!(
                get_active_list_item(&mut context, COMPONENT)
                    .await
                    .unwrap(),
                -1
            );
        }

        for component_type in [1u32, 2, 3, 4] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 7i32).unwrap();

            assert_eq!(
                get_active_list_item(&mut context, COMPONENT)
                    .await
                    .unwrap(),
                -9
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_active_list_item_returns_native_slot_unchanged() {
        let mut context = TestContext::new();
        init_component(&mut context, 5);

        for expected in [-1i32, 0, 2, -2, i32::MIN, i32::MAX] {
            write_generic(&mut context, COMPONENT + 0x48, expected).unwrap();

            assert_eq!(
                get_active_list_item(&mut context, COMPONENT)
                    .await
                    .unwrap(),
                expected
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_active_menu_item_matches_native_validation_and_type_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            get_active_menu_item(&mut context, 0).await.unwrap(),
            -1
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x1234_5678i32).unwrap();

            assert_eq!(
                get_active_menu_item(&mut context, COMPONENT)
                    .await
                    .unwrap(),
                -1
            );
        }

        for component_type in [2u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 7i32).unwrap();

            assert_eq!(
                get_active_menu_item(&mut context, COMPONENT)
                    .await
                    .unwrap(),
                -9
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_active_menu_item_returns_native_slot_unchanged() {
        let mut context = TestContext::new();
        init_component(&mut context, 1);

        for expected in [-1i32, 0, 2, -2, i32::MIN, i32::MAX] {
            write_generic(&mut context, COMPONENT + 0x48, expected).unwrap();

            assert_eq!(
                get_active_menu_item(&mut context, COMPONENT)
                    .await
                    .unwrap(),
                expected
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_active_list_item_matches_native_validation_and_type_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            set_active_list_item(&mut context, 0, 0).await.unwrap(),
            -1
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x1234i32).unwrap();

            assert_eq!(
                set_active_list_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                -1
            );
            assert_eq!(
                read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
                0x1234
            );
        }

        for component_type in [1u32, 2, 3, 4] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 7i32).unwrap();

            assert_eq!(
                set_active_list_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
                7
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_active_list_item_returns_old_value_and_matches_signed_range_rule() {
        let mut context = TestContext::new();
        init_component(&mut context, 5);

        write_generic(&mut context, COMPONENT + 0x44, 3i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x48, -1i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x54, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x58, 0u32).unwrap();

        assert_eq!(
            set_active_list_item(&mut context, COMPONENT, 1)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            1
        );

        assert_eq!(
            set_active_list_item(&mut context, COMPONENT, 1)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            1
        );

        assert_eq!(
            set_active_list_item(&mut context, COMPONENT, -1)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            -1
        );

        assert_eq!(
            set_active_list_item(&mut context, COMPONENT, 3)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            -1
        );

        assert_eq!(
            set_active_list_item(&mut context, COMPONENT, -2)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            -2
        );

        assert_eq!(
            set_active_list_item(&mut context, COMPONENT, i32::MIN)
                .await
                .unwrap(),
            -2
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            i32::MIN
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_active_menu_item_matches_native_validation_and_type_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            set_active_menu_item(&mut context, 0, 0).await.unwrap(),
            -1
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 0x1234i32).unwrap();

            assert_eq!(
                set_active_menu_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                -1
            );
            assert_eq!(
                read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
                0x1234
            );
        }

        for component_type in [2u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x48, 7i32).unwrap();

            assert_eq!(
                set_active_menu_item(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
                7
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_active_menu_item_returns_old_value_and_matches_signed_range_rule() {
        let mut context = TestContext::new();
        init_component(&mut context, 1);

        write_generic(&mut context, COMPONENT + 0x44, 3i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x48, -1i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x54, 0u32).unwrap();
        write_generic(&mut context, COMPONENT + 0x58, 0u32).unwrap();

        assert_eq!(
            set_active_menu_item(&mut context, COMPONENT, 1)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            1
        );

        assert_eq!(
            set_active_menu_item(&mut context, COMPONENT, 1)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            1
        );

        assert_eq!(
            set_active_menu_item(&mut context, COMPONENT, -1)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            -1
        );

        assert_eq!(
            set_active_menu_item(&mut context, COMPONENT, 3)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            -1
        );

        assert_eq!(
            set_active_menu_item(&mut context, COMPONENT, -2)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            read_generic::<i32, _>(&context, COMPONENT + 0x48).unwrap(),
            -2
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_menu_item_matches_native_validation_range_and_buffer_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            get_menu_item(&mut context, 0, 0, 0x3100, 32, 0x3200)
                .await
                .unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            assert_eq!(
                get_menu_item(&mut context, COMPONENT, 0, 0x3100, 32, 0x3200)
                    .await
                    .unwrap(),
                0
            );
        }

        for component_type in [2u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            assert_eq!(
                get_menu_item(&mut context, COMPONENT, 0, 0x3100, 32, 0x3200)
                    .await
                    .unwrap(),
                -9
            );
        }

        init_component(&mut context, 1);
        context.write_bytes(0x3000, b"abcd\0").unwrap();
        assert_eq!(
            add_menu_item(&mut context, COMPONENT, 0x3000, 0xaabb_ccdd)
                .await
                .unwrap(),
            0
        );

        context.write_bytes(0x3100, &[0xcc; 16]).unwrap();
        write_generic(&mut context, 0x3200, 0x5566_7788u32).unwrap();

        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 1, 0x3100, 16, 0x3200)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 0, 0, 4, 0)
                .await
                .unwrap(),
            -18
        );
        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 0, 0x3100, 4, 0x3200)
                .await
                .unwrap(),
            -18
        );

        let mut unchanged = [0u8; 16];
        context.read_bytes(0x3100, &mut unchanged).unwrap();
        assert_eq!(unchanged, [0xcc; 16]);
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3200).unwrap(),
            0x5566_7788
        );

        assert_eq!(
            get_menu_item(&mut context, COMPONENT, 0, 0x3100, 5, 0x3200)
                .await
                .unwrap(),
            1
        );
    }

    #[futures_test::test]
    async fn lgt_uic_add_menu_item_matches_native_validation_contract() {
        let mut context = TestContext::new();

        assert_eq!(
            add_menu_item(&mut context, 0, 0, 0).await.unwrap(),
            0
        );

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0x1234u32).unwrap();
            write_generic(&mut context, COMPONENT + 0x50, 0x5678u32).unwrap();

            assert_eq!(
                add_menu_item(&mut context, COMPONENT, 0, 0)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0x1234
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x50).unwrap(),
                0x5678
            );
        }

        for component_type in [2u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
            write_generic(&mut context, COMPONENT + 0x50, 0u32).unwrap();

            assert_eq!(
                add_menu_item(&mut context, COMPONENT, 0, 0)
                    .await
                    .unwrap(),
                -9
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x44).unwrap(),
                0
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x50).unwrap(),
                0
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_time_copies_exact_native_tm_and_reformats() {
        let mut context = TestContext::new();
        init_component(&mut context, 2);

        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();

        let fields = [
            5i32,
            4,
            3,
            2,
            0,
            124,
            2,
            1,
            0,
            0x1122_3344u32 as i32,
            0x5566_7788u32 as i32,
        ];
        for (index, value) in fields.iter().enumerate() {
            write_generic(
                &mut context,
                COMPONENT + 0x48 + index as u32 * 4,
                *value,
            )
            .unwrap();
        }

        context
            .write_bytes(COMPONENT + 0x74, b"old datetime\0")
            .unwrap();
        context.write_bytes(0x3000, &[0xaa; 44]).unwrap();

        assert_ne!(
            get_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
            0
        );

        for (index, expected) in fields.iter().enumerate() {
            assert_eq!(
                read_generic::<i32, _>(&context, 0x3000 + index as u32 * 4).unwrap(),
                *expected
            );
        }

        assert_eq!(
            uic_read_c_string(&context, COMPONENT + 0x74).unwrap(),
            b"2024/01/02"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_time_returns_zero_when_rendered_text_is_unchanged() {
        let mut context = TestContext::new();
        init_component(&mut context, 2);

        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();

        let fields = [5i32, 4, 3, 2, 0, 124, 2, 1, 0, 0, 0];
        for (index, value) in fields.iter().enumerate() {
            write_generic(
                &mut context,
                COMPONENT + 0x48 + index as u32 * 4,
                *value,
            )
            .unwrap();
        }

        context
            .write_bytes(COMPONENT + 0x74, b"2024/01/02\0")
            .unwrap();

        assert_eq!(
            get_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
            0
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_time_matches_native_validation_contract() {
        let mut context = TestContext::new();

        assert_eq!(get_time(&mut context, 0, 0x3000).await.unwrap(), 0);

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            context.write_bytes(0x3000, &[0xaa; 44]).unwrap();

            assert_eq!(
                get_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
                0
            );

            let mut unchanged = [0u8; 44];
            context.read_bytes(0x3000, &mut unchanged).unwrap();
            assert_eq!(unchanged, [0xaa; 44]);
        }

        for component_type in 1u32..=5 {
            init_component(&mut context, component_type);
            assert_eq!(
                get_time(&mut context, COMPONENT, 0).await.unwrap(),
                1
            );
        }

        for component_type in [1u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            context.write_bytes(0x3000, &[0xbb; 44]).unwrap();

            assert_eq!(
                get_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
                1
            );

            let mut unchanged = [0u8; 44];
            context.read_bytes(0x3000, &mut unchanged).unwrap();
            assert_eq!(unchanged, [0xbb; 44]);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_copies_native_tm_and_reformats() {
        let mut context = TestContext::new();
        init_component(&mut context, 2);

        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        context
            .write_bytes(COMPONENT + 0x74, b"old datetime\0")
            .unwrap();

        let fields = [
            5i32,   // sec
            4,      // min
            3,      // hour
            2,      // mday
            0,      // mon
            124,    // year
            2,      // wday
            1,      // yday
            0,      // isdst
            0x1122_3344u32 as i32,
            0x5566_7788u32 as i32,
        ];

        for (index, value) in fields.iter().enumerate() {
            write_generic(&mut context, 0x3000 + index as u32 * 4, *value).unwrap();
        }

        let result = set_time(&mut context, COMPONENT, 0x3000).await.unwrap();
        assert_ne!(result, 0);

        for (index, expected) in fields.iter().enumerate() {
            assert_eq!(
                read_generic::<i32, _>(
                    &context,
                    COMPONENT + 0x48 + index as u32 * 4
                )
                .unwrap(),
                *expected
            );
        }

        assert_eq!(
            uic_read_c_string(&context, COMPONENT + 0x74).unwrap(),
            b"2024/01/02"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_returns_zero_when_rendered_text_is_unchanged() {
        let mut context = TestContext::new();
        init_component(&mut context, 2);

        write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
        context
            .write_bytes(COMPONENT + 0x74, b"2024/01/02\0")
            .unwrap();

        let fields = [5i32, 4, 3, 2, 0, 124, 2, 1, 0, 0, 0];
        for (index, value) in fields.iter().enumerate() {
            write_generic(&mut context, 0x3000 + index as u32 * 4, *value).unwrap();
        }

        assert_eq!(
            set_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
            0
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_matches_native_validation_and_null_tm_contract() {
        let mut context = TestContext::new();

        assert_eq!(set_time(&mut context, 0, 0x3000).await.unwrap(), 0);

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            context
                .write_bytes(COMPONENT + 0x48, &[0xaa; 44])
                .unwrap();

            assert_eq!(
                set_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
                0
            );

            let mut unchanged = [0u8; 44];
            context
                .read_bytes(COMPONENT + 0x48, &mut unchanged)
                .unwrap();
            assert_eq!(unchanged, [0xaa; 44]);
        }

        for component_type in 1u32..=5 {
            init_component(&mut context, component_type);
            context
                .write_bytes(COMPONENT + 0x48, &[0xbb; 44])
                .unwrap();

            assert_eq!(
                set_time(&mut context, COMPONENT, 0).await.unwrap(),
                1
            );

            let mut unchanged = [0u8; 44];
            context
                .read_bytes(COMPONENT + 0x48, &mut unchanged)
                .unwrap();
            assert_eq!(unchanged, [0xbb; 44]);
        }

        for component_type in [1u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            context
                .write_bytes(COMPONENT + 0x48, &[0xcc; 44])
                .unwrap();
            context.write_bytes(0x3000, &[0x11; 44]).unwrap();

            assert_eq!(
                set_time(&mut context, COMPONENT, 0x3000).await.unwrap(),
                1
            );

            let mut unchanged = [0u8; 44];
            context
                .read_bytes(COMPONENT + 0x48, &mut unchanged)
                .unwrap();
            assert_eq!(unchanged, [0xcc; 44]);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_long_converts_signed_epoch_seconds_to_native_kst_tm() {
        for (time, expected) in [
            (
                0u32,
                [0i32, 0, 9, 1, 0, 70, 4, 0, 0],
            ),
            (
                u32::MAX,
                [59i32, 59, 8, 1, 0, 70, 4, 0, 0],
            ),
            (
                0x7fff_ffffu32,
                [7i32, 14, 12, 19, 0, 138, 2, 18, 0],
            ),
        ] {
            let mut context = TestContext::new();
            init_component(&mut context, 2);
            write_generic(&mut context, COMPONENT + 0x44, 0u32).unwrap();
            context
                .write_bytes(COMPONENT + 0x74, b"old datetime\0")
                .unwrap();

            assert_ne!(
                set_time_long(&mut context, COMPONENT, time).await.unwrap(),
                0
            );

            for (index, value) in expected.iter().enumerate() {
                assert_eq!(
                    read_generic::<i32, _>(
                        &context,
                        COMPONENT + 0x48 + index as u32 * 4
                    )
                    .unwrap(),
                    *value
                );
            }

            assert_eq!(
                read_generic::<i32, _>(&context, COMPONENT + 0x6c).unwrap(),
                32_400
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x70).unwrap(),
                0
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_long_formats_using_current_mask() {
        let mut context = TestContext::new();
        init_component(&mut context, 2);

        write_generic(&mut context, COMPONENT + 0x44, 3u32).unwrap();
        context.write_bytes(COMPONENT + 0x74, b"old\0").unwrap();

        assert_ne!(
            set_time_long(&mut context, COMPONENT, 0).await.unwrap(),
            0
        );
        assert_eq!(
            uic_read_c_string(&context, COMPONENT + 0x74).unwrap(),
            b"1970/01/01 09:00:00"
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_time_long_matches_native_validation_contract() {
        let mut context = TestContext::new();

        assert_eq!(set_time_long(&mut context, 0, 0).await.unwrap(), 0);

        for component_type in [0u32, 6] {
            init_component(&mut context, component_type);
            context
                .write_bytes(COMPONENT + 0x48, &[0xaa; 44])
                .unwrap();

            assert_eq!(
                set_time_long(&mut context, COMPONENT, 0).await.unwrap(),
                0
            );

            let mut unchanged = [0u8; 44];
            context
                .read_bytes(COMPONENT + 0x48, &mut unchanged)
                .unwrap();
            assert_eq!(unchanged, [0xaa; 44]);
        }

        for component_type in [1u32, 3, 4, 5] {
            init_component(&mut context, component_type);
            context
                .write_bytes(COMPONENT + 0x48, &[0xbb; 44])
                .unwrap();

            assert_eq!(
                set_time_long(&mut context, COMPONENT, u32::MAX)
                    .await
                    .unwrap(),
                1
            );

            let mut unchanged = [0u8; 44];
            context
                .read_bytes(COMPONENT + 0x48, &mut unchanged)
                .unwrap();
            assert_eq!(unchanged, [0xbb; 44]);
        }
    }

    #[test]
    fn lgt_uic_color_to_rgb565_matches_native_mh_fb_make_pixel() {
        for (color, expected) in [
            (0x0000_0000u32, 0x0000u32),
            (0x00ff_ffffu32, 0xffffu32),
            (0x00ff_0000u32, 0xf800u32),
            (0x0000_ff00u32, 0x07e0u32),
            (0x0000_00ffu32, 0x001fu32),
            (0x0012_3456u32, 0x11aau32),
            (0xff12_3456u32, 0x11aau32),
        ] {
            assert_eq!(uic_color_to_rgb565(color), expected);
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_fg_color_matches_native_conversion_store_and_return() {
        for component_type in 1u32..=5 {
            let mut context = TestContext::new();
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x18, 0xdead_beefu32).unwrap();

            assert_eq!(
                set_fg_color(&mut context, COMPONENT, 0x0012_3456)
                    .await
                    .unwrap(),
                0x11aa
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x18).unwrap(),
                0x11aa
            );
        }

        let mut context = TestContext::new();
        assert_eq!(
            set_fg_color(&mut context, 0, 0x00ff_0000)
                .await
                .unwrap(),
            0
        );

        init_component(&mut context, 6);
        write_generic(&mut context, COMPONENT + 0x18, 0xaabb_ccddu32).unwrap();
        assert_eq!(
            set_fg_color(&mut context, COMPONENT, 0x00ff_0000)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x18).unwrap(),
            0xaabb_ccdd
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_font_returns_native_slot_for_all_component_types() {
        for component_type in 1u32..=5 {
            let mut context = TestContext::new();
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x14, 0x1234_0000u32 + component_type)
                .unwrap();

            assert_eq!(
                get_font(&mut context, COMPONENT).await.unwrap(),
                0x1234_0000u32 + component_type
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_get_font_returns_zero_for_null_and_invalid_components() {
        let mut context = TestContext::new();

        assert_eq!(get_font(&mut context, 0).await.unwrap(), 0);

        init_component(&mut context, 0);
        write_generic(&mut context, COMPONENT + 0x14, 0xaaaa_bbbbu32).unwrap();
        assert_eq!(get_font(&mut context, COMPONENT).await.unwrap(), 0);

        write_generic(&mut context, COMPONENT, 6u32).unwrap();
        assert_eq!(get_font(&mut context, COMPONENT).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_uic_set_font_returns_previous_and_replaces_native_slot() {
        for component_type in 1u32..=5 {
            let mut context = TestContext::new();
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x14, 0x1234_5678u32).unwrap();

            assert_eq!(
                set_font(&mut context, COMPONENT, 0x8765_4321)
                    .await
                    .unwrap(),
                0x1234_5678
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x14).unwrap(),
                0x8765_4321
            );

            assert_eq!(
                set_font(&mut context, COMPONENT, 0).await.unwrap(),
                0x8765_4321
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x14).unwrap(),
                0
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_font_returns_zero_for_invalid_components() {
        let mut context = TestContext::new();

        assert_eq!(set_font(&mut context, 0, 0x1111_2222).await.unwrap(), 0);

        init_component(&mut context, 6);
        write_generic(&mut context, COMPONENT + 0x14, 0xaabb_ccddu32).unwrap();

        assert_eq!(
            set_font(&mut context, COMPONENT, 0x1111_2222)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x14).unwrap(),
            0xaabb_ccdd
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_event_handler_returns_previous_and_replaces_native_slot() {
        for component_type in 1u32..=5 {
            let mut context = TestContext::new();
            init_component(&mut context, component_type);
            write_generic(&mut context, COMPONENT + 0x28, 0x1234_5678u32).unwrap();

            assert_eq!(
                set_event_handler(&mut context, COMPONENT, 0x8765_4321)
                    .await
                    .unwrap(),
                0x1234_5678
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x28).unwrap(),
                0x8765_4321
            );

            assert_eq!(
                set_event_handler(&mut context, COMPONENT, 0)
                    .await
                    .unwrap(),
                0x8765_4321
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + 0x28).unwrap(),
                0
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_event_handler_returns_zero_for_invalid_components() {
        let mut context = TestContext::new();

        assert_eq!(
            set_event_handler(&mut context, 0, 0x1111_2222)
                .await
                .unwrap(),
            0
        );

        init_component(&mut context, 6);
        write_generic(&mut context, COMPONENT + 0x28, 0xaabb_ccddu32).unwrap();

        assert_eq!(
            set_event_handler(&mut context, COMPONENT, 0x1111_2222)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x28).unwrap(),
            0xaabb_ccdd
        );
    }

    #[futures_test::test]
    async fn lgt_uic_set_callback_matches_native_common_selector_slots_and_return_value() {
        let mut context = TestContext::new();
        init_component(&mut context, 4);

        for (selector, callback_offset, context_offset) in [
            (1u32, 0x2cu32, 0x38u32),
            (2u32, 0x30u32, 0x3cu32),
            (3u32, 0x34u32, 0x40u32),
        ] {
            let old = 0x1100_0000u32 + selector;
            let new = 0x2200_0000u32 + selector;
            let user = 0x3300_0000u32 + selector;

            write_generic(&mut context, COMPONENT + callback_offset, old).unwrap();
            write_generic(&mut context, COMPONENT + context_offset, 0xdead_beefu32).unwrap();

            assert_eq!(
                set_callback(&mut context, COMPONENT, selector, new, user)
                    .await
                    .unwrap(),
                old
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + callback_offset).unwrap(),
                new
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + context_offset).unwrap(),
                user
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_callback_matches_native_subtype_specific_slots() {
        for (component_type, selector, callback_offset, context_offset) in [
            (1u32, 4u32, 0x54u32, 0x58u32),
            (5u32, 4u32, 0x54u32, 0x58u32),
            (2u32, 4u32, 0xa0u32, 0xa4u32),
            (3u32, 4u32, 0x5cu32, 0x64u32),
            (3u32, 5u32, 0x60u32, 0x68u32),
        ] {
            let mut context = TestContext::new();
            init_component(&mut context, component_type);

            write_generic(&mut context, COMPONENT + callback_offset, 0x1234_5678u32).unwrap();

            assert_eq!(
                set_callback(
                    &mut context,
                    COMPONENT,
                    selector,
                    0x8765_4321,
                    0x1357_2468,
                )
                .await
                .unwrap(),
                0x1234_5678
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + callback_offset).unwrap(),
                0x8765_4321
            );
            assert_eq!(
                read_generic::<u32, _>(&context, COMPONENT + context_offset).unwrap(),
                0x1357_2468
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_set_callback_rejects_invalid_selector_and_subtype_pairs() {
        let mut context = TestContext::new();

        assert_eq!(
            set_callback(&mut context, 0, 1, 0x1111, 0x2222)
                .await
                .unwrap(),
            0
        );

        init_component(&mut context, 4);
        write_generic(&mut context, COMPONENT + 0x2c, 0xaaaa_bbbbu32).unwrap();

        assert_eq!(
            set_callback(&mut context, COMPONENT, 0, 0x1111, 0x2222)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            set_callback(&mut context, COMPONENT, 6, 0x1111, 0x2222)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            set_callback(&mut context, COMPONENT, 4, 0x1111, 0x2222)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            set_callback(&mut context, COMPONENT, 5, 0x1111, 0x2222)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            read_generic::<u32, _>(&context, COMPONENT + 0x2c).unwrap(),
            0xaaaa_bbbb
        );

        write_generic(&mut context, COMPONENT, 6u32).unwrap();
        assert_eq!(
            set_callback(&mut context, COMPONENT, 1, 0x1111, 0x2222)
                .await
                .unwrap(),
            0
        );
    }

    #[futures_test::test]
    async fn lgt_uic_get_geometry_writes_each_non_null_output_like_native() {
        let mut context = TestContext::new();
        init_component(&mut context, 4);

        write_generic(&mut context, COMPONENT + 0x04, -11i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x08, 22i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x0c, 333i32).unwrap();
        write_generic(&mut context, COMPONENT + 0x10, 444i32).unwrap();

        for address in [0x3000u32, 0x3004, 0x3008, 0x300c] {
            write_generic(&mut context, address, 0x1357_2468u32).unwrap();
        }

        get_geometry(
            &mut context,
            COMPONENT,
            0x3000,
            0,
            0x3008,
            0x300c,
        )
        .await
        .unwrap();

        assert_eq!(read_generic::<i32, _>(&context, 0x3000).unwrap(), -11);
        assert_eq!(
            read_generic::<u32, _>(&context, 0x3004).unwrap(),
            0x1357_2468
        );
        assert_eq!(read_generic::<i32, _>(&context, 0x3008).unwrap(), 333);
        assert_eq!(read_generic::<i32, _>(&context, 0x300c).unwrap(), 444);

        get_geometry(
            &mut context,
            COMPONENT,
            0,
            0x3004,
            0,
            0,
        )
        .await
        .unwrap();
        assert_eq!(read_generic::<i32, _>(&context, 0x3004).unwrap(), 22);
    }

    #[futures_test::test]
    async fn lgt_uic_get_geometry_leaves_outputs_untouched_for_invalid_components() {
        let mut context = TestContext::new();

        for address in [0x3000u32, 0x3004, 0x3008, 0x300c] {
            write_generic(&mut context, address, 0x2468_1357u32).unwrap();
        }

        get_geometry(&mut context, 0, 0x3000, 0x3004, 0x3008, 0x300c)
            .await
            .unwrap();

        for address in [0x3000u32, 0x3004, 0x3008, 0x300c] {
            assert_eq!(
                read_generic::<u32, _>(&context, address).unwrap(),
                0x2468_1357
            );
        }

        init_component(&mut context, 6);
        get_geometry(
            &mut context,
            COMPONENT,
            0x3000,
            0x3004,
            0x3008,
            0x300c,
        )
        .await
        .unwrap();

        for address in [0x3000u32, 0x3004, 0x3008, 0x300c] {
            assert_eq!(
                read_generic::<u32, _>(&context, address).unwrap(),
                0x2468_1357
            );
        }
    }

    #[futures_test::test]
    async fn lgt_uic_configure_flags_match_native_geometry_updates() {
        let mut context = TestContext::new();
        init_component(&mut context, 1);

        configure(&mut context, COMPONENT, -11, -22, 33, 44, 1)
            .await
            .unwrap();
        assert_eq!(read_i32(&context, 0x04), -11);
        assert_eq!(read_i32(&context, 0x08), -22);
        assert_eq!(read_i32(&context, 0x0c), 30);
        assert_eq!(read_i32(&context, 0x10), 40);

        configure(&mut context, COMPONENT, 111, 222, 55, 66, 2)
            .await
            .unwrap();
        assert_eq!(read_i32(&context, 0x04), -11);
        assert_eq!(read_i32(&context, 0x08), -22);
        assert_eq!(read_i32(&context, 0x0c), 55);
        assert_eq!(read_i32(&context, 0x10), 66);

        configure(&mut context, COMPONENT, 7, 8, 9, 10, 3)
            .await
            .unwrap();
        assert_eq!(read_i32(&context, 0x04), 7);
        assert_eq!(read_i32(&context, 0x08), 8);
        assert_eq!(read_i32(&context, 0x0c), 9);
        assert_eq!(read_i32(&context, 0x10), 10);
    }

    #[futures_test::test]
    async fn lgt_uic_configure_rejects_nonpositive_size_and_invalid_component() {
        let mut context = TestContext::new();
        init_component(&mut context, 5);

        configure(&mut context, COMPONENT, 1, 2, 0, 99, 2)
            .await
            .unwrap();
        assert_eq!(read_i32(&context, 0x0c), 30);
        assert_eq!(read_i32(&context, 0x10), 40);

        configure(&mut context, COMPONENT, 1, 2, 99, -1, 2)
            .await
            .unwrap();
        assert_eq!(read_i32(&context, 0x0c), 30);
        assert_eq!(read_i32(&context, 0x10), 40);

        write_generic(&mut context, COMPONENT, 6u32).unwrap();
        configure(&mut context, COMPONENT, 100, 200, 300, 400, 3)
            .await
            .unwrap();
        assert_eq!(read_i32(&context, 0x04), 10);
        assert_eq!(read_i32(&context, 0x08), 20);
        assert_eq!(read_i32(&context, 0x0c), 30);
        assert_eq!(read_i32(&context, 0x10), 40);

        configure(&mut context, 0, 100, 200, 300, 400, 3)
            .await
            .unwrap();
    }
}
