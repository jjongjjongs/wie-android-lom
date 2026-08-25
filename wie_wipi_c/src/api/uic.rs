use wie_util::{Result, read_generic, read_null_terminated_string_bytes, write_generic};

use wipi_types::wipic::{WIPICIndirectPtr, WIPICWord};

use crate::{api::kernel, context::WIPICContext};

pub async fn create_application_context(_context: &mut dyn WIPICContext) -> Result<WIPICIndirectPtr> {
    tracing::warn!("stub MC_uicCreateApplicationContext");

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

pub async fn get_menu_item(_context: &mut dyn WIPICContext, cc: WIPICWord, idx: u32, psz: WIPICWord, buflen: i32, img: WIPICWord) -> Result<i32> {
    tracing::warn!("stub MC_uicGetMenuItem({cc:#x}, {idx}, {psz:#x}, {buflen}, {img:#x})");

    Ok(0)
}


#[cfg(test)]
mod tests {
    use wie_util::{ByteRead, ByteWrite, read_generic, write_generic};

    use crate::context::test::TestContext;

    use super::{configure, delete_text, insert_text};

    const COMPONENT: u32 = 0x1000;

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
