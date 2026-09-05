use wie_util::{Result, read_null_terminated_string_bytes};

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

/// WIPI-C MC_phnCallPlace.
///
/// The LGT reference forwards one NUL-terminated local-code string to
/// WipiPlayer.callPlace(String). Its Android host implementation launches
/// ACTION_CALL with a `tel:` URI. A host-dispatch failure is reported as -1;
/// successful dispatch returns 0.
pub async fn call_place(context: &mut dyn WIPICContext, phone_number: WIPICWord) -> Result<i32> {
    let bytes = read_null_terminated_string_bytes(context, phone_number)?;
    let number = encoding_rs::EUC_KR.decode(&bytes).0.into_owned();

    tracing::debug!("MC_phnCallPlace({number})");

    Ok(if context.system().platform().call_place(&number) { 0 } else { -1 })
}

/// LGT `MC_phnSmsSend`.
///
/// The native implementation does not inspect any API arguments. It checks
/// SMS permission bit 0x8000, discards that result, and unconditionally
/// returns -1 without invoking the handset SMS backend.
pub async fn sms_send(_context: &mut dyn WIPICContext) -> Result<i32> {
    tracing::debug!("MC_phnSmsSend() -> -1");
    Ok(-1)
}

#[cfg(test)]
mod tests {
    use crate::context::test::TestContext;

    use super::sms_send;

    #[futures_test::test]
    async fn lgt_phn_sms_send_matches_native_unconditional_failure() {
        let mut context = TestContext::new();

        assert_eq!(sms_send(&mut context).await.unwrap(), -1);
    }
}
