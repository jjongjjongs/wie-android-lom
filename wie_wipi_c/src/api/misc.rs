use wie_util::Result;

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

pub async fn back_light(
    _context: &mut dyn WIPICContext,
    id: WIPICWord,
    on_off: WIPICWord,
    color: WIPICWord,
    timeout: WIPICWord,
) -> Result<WIPICWord> {
    tracing::warn!("stub MC_miscBackLight({id}, {on_off}, {color}, {timeout})");

    Ok(0)
}

/// `MC_miscGetLedCount` - how many indicator LEDs the handset has.
///
/// There is no handset here and nothing to light, so the honest answer is
/// none. It is also the useful one: a title reads the count first and, told
/// zero, does not go on to address an LED that is not there. That turns what
/// used to abort the title into a question it can act on.
pub async fn get_led_count(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::debug!("MC_miscGetLedCount()");

    Ok(0)
}

/// `MC_miscSetLed` - light one of the LEDs `MC_miscGetLedCount` counted.
///
/// There are none, so every id is out of range. Reported as failure rather
/// than as a silent success, so a title that checks is told the truth.
pub async fn set_led(_context: &mut dyn WIPICContext, id: WIPICWord, on_off: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_miscSetLed({id}, {on_off})");

    Ok(-1)
}

/// `MC_miscGetLed` - read one back. As above, there is none to read.
pub async fn get_led(_context: &mut dyn WIPICContext, id: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_miscGetLed({id})");

    Ok(-1)
}

#[cfg(test)]
mod tests {
    use crate::context::test::TestContext;

    use super::{get_led, get_led_count, set_led};

    /// A handset with no LEDs answers none, and refuses to address one. The
    /// point is that it answers at all - these used to abort the title.
    #[futures_test::test]
    async fn there_are_no_leds_to_light() {
        let mut context = TestContext::new();

        assert_eq!(get_led_count(&mut context).await.unwrap(), 0);
        assert_eq!(set_led(&mut context, 0, 1).await.unwrap(), -1);
        assert_eq!(get_led(&mut context, 0).await.unwrap(), -1);
    }
}
