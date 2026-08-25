use wie_util::{Result, read_null_terminated_string_bytes};

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

/// WIPI-C MC_phnCallPlace.
///
/// The LGT reference forwards one NUL-terminated local-code string to
/// WipiPlayer.callPlace(String). Its Android host implementation launches
/// ACTION_CALL with a `tel:` URI. A host-dispatch failure is reported as -1;
/// successful dispatch returns 0.
pub async fn call_place(
    context: &mut dyn WIPICContext,
    phone_number: WIPICWord,
) -> Result<i32> {
    let bytes = read_null_terminated_string_bytes(context, phone_number)?;
    let number = encoding_rs::EUC_KR.decode(&bytes).0.into_owned();

    tracing::debug!("MC_phnCallPlace({number})");

    Ok(if context.system().platform().call_place(&number) {
        0
    } else {
        -1
    })
}
