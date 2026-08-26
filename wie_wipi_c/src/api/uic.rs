use wie_util::{Result, WieError, read_generic, read_null_terminated_string_bytes, write_generic};

use wipi_types::wipic::{WIPICIndirectPtr, WIPICWord};

use crate::{api::{graphics, kernel}, context::WIPICContext};

/// LGT `MC_uicCreateApplicationContext` (WIPI-C service 0x320).
///
/// Native is a two-instruction constant return and always yields NULL.
pub async fn create_application_context(_context: &mut dyn WIPICContext) -> Result<WIPICIndirectPtr> {
    tracing::debug!("MC_uicCreateApplicationContext");

    Ok(WIPICIndirectPtr(0))
}

pub async fn get_class(context: &mut dyn WIPICContext, psz: WIPICWord) -> Result<WIPICIndirectPtr> {
    let name_bytes = read_null_terminated_string_bytes(context, psz)?;
    let name = encoding_rs::EUC_KR.decode(&name_bytes).0;
    tracing::warn!("stub MC_uicGetClass({name})");

    Ok(WIPICIndirectPtr(0))
}

pub async fn create(_context: &mut dyn WIPICContext, pac: WIPICWord, cls: WIPICWord) -> Result<WIPICIndirectPtr> {
    tracing::warn!("stub MC_uicCreate({pac:#x}, {cls:#x})");

    Ok(WIPICIndirectPtr(0))
}

pub async fn destroy(_context: &mut dyn WIPICContext, cc: WIPICWord) -> Result<()> {
    tracing::warn!("stub MC_uicDestroy({cc:#x})");

    Ok(())
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
                kernel::set_timer(context, timer, 1000, 0, component).await?;
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
                kernel::set_timer(context, timer, 500, 0, component).await?;
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

/// LGT `MC_uicPaint` (WIPI-C service 0x325).
///
/// Native contract:
/// - NULL or invalid component types (outside 1..=5) are silent no-ops.
/// - NULL graphics context is a silent no-op.
/// - property 148 selects screen framebuffer 0 or 3 before draw dispatch.
/// - component +0x24 is invoked as `(component, graphics_context)`.
/// - callback +0x30, when present, is then invoked as
///   `(component, 0, component+0x3c context)`.
///
/// `MC_uicCreate` remains a separate API: it is responsible for populating
/// the type-specific +0x24 draw function pointer.
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

    let draw: WIPICWord = read_generic(context, component + 0x24)?;
    context
        .call_function(draw, &[component, graphics_context])
        .await?;

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
    let width: i32 = read_generic(context, component + 0x0c)?;
    let height: i32 = read_generic(context, component + 0x10)?;
    graphics::repaint(context, 0, 0, 0, width, height).await
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

fn uic_kst_tm_from_epoch_millis(epoch_millis: u64) -> [i32; 9] {
    let seconds = (epoch_millis / 1000) as i64 + 9 * 3600;
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

    let draw: WIPICWord = read_generic(context, component + 0x24)?;
    if draw != 0 {
        context.call_function(draw, &[component, gctx]).await?;
    }

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
    kernel::set_timer(context, timer, 1000, 0, component).await?;
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

pub async fn get_menu_item(_context: &mut dyn WIPICContext, cc: WIPICWord, idx: u32, psz: WIPICWord, buflen: i32, img: WIPICWord) -> Result<i32> {
    tracing::warn!("stub MC_uicGetMenuItem({cc:#x}, {idx}, {psz:#x}, {buflen}, {img:#x})");

    Ok(0)
}


#[cfg(test)]
mod tests {
    use wie_util::{ByteRead, ByteWrite, read_generic, write_generic};

    use crate::context::test::TestContext;

    use super::{
        configure, delete_text, insert_text, set_max_text_size, uic_skip_time_separator,
    };

    const COMPONENT: u32 = 0x1000;

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
