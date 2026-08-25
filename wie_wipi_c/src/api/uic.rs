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

pub async fn get_menu_item(_context: &mut dyn WIPICContext, cc: WIPICWord, idx: u32, psz: WIPICWord, buflen: i32, img: WIPICWord) -> Result<i32> {
    tracing::warn!("stub MC_uicGetMenuItem({cc:#x}, {idx}, {psz:#x}, {buflen}, {img:#x})");

    Ok(0)
}


#[cfg(test)]
mod tests {
    use wie_util::{read_generic, write_generic};

    use crate::context::test::TestContext;

    use super::configure;

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
