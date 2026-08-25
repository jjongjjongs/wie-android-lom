use alloc::string::String;

use wipi_types::wipic::WIPICWord;

use wie_util::{Result, read_generic, read_null_terminated_string_bytes};

use crate::context::WIPICContext;

/// `MC_sysExecute` for the LGT WIPI-C runtime.
///
/// Native ABI: `(ignored, command, argv)`.
///
/// The LGT HAL implements `WAPBROWSER`. For that command `argv[0]` is
/// interpreted as the NUL-terminated URL and passed to
/// `WipiPlayer.startWAPBrowser(String)`. Unsupported commands reach
/// `LGTH_sysExecute` and return -16.
///
/// `MMSSEND` and `APPCALL` have permission checks in the outer native wrapper.
/// WIE does not model that per-DLET APM permission state, so these commands
/// follow the native permission-allowed path and ultimately return -16.
pub async fn execute(
    context: &mut dyn WIPICContext,
    _ignored: WIPICWord,
    command: WIPICWord,
    argv: WIPICWord,
) -> Result<i32> {
    let command = read_null_terminated_string_bytes(context, command)?;

    tracing::debug!("MC_sysExecute({})", String::from_utf8_lossy(&command));

    if command != b"WAPBROWSER" {
        return Ok(-16);
    }

    let url_ptr: u32 = read_generic(context, argv)?;
    let url_bytes = read_null_terminated_string_bytes(context, url_ptr)?;
    let url = encoding_rs::EUC_KR.decode(&url_bytes).0.into_owned();

    Ok(if context.system().platform().open_url(&url) {
        0
    } else {
        -1
    })
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

    use spin::Mutex;
    use test_utils::{TestPlatform, TestPlatformEvent};
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::ByteWrite;

    use crate::context::test::TestContext;

    use super::execute;

    fn test_context(events: Arc<Mutex<Vec<String>>>) -> TestContext {
        let platform = TestPlatform::with_event_handler(move |event| {
            if let TestPlatformEvent::OpenUrl(url) = event {
                events.lock().push(url);
            }
        });
        let system = System::new(
            Box::new(platform),
            "test-pid",
            "test-aid",
            DefaultTaskRunner,
        );
        TestContext::with_system(system)
    }

    #[futures_test::test]
    async fn lgt_sys_execute_wapbrowser_reads_argv_zero_and_dispatches_url() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut context = test_context(events.clone());

        context.write_bytes(0x1000, b"WAPBROWSER\0").unwrap();
        context.write_bytes(0x1100, &0x1200u32.to_le_bytes()).unwrap();
        context.write_bytes(0x1200, b"https://example.com/a?b=c\0").unwrap();

        assert_eq!(execute(&mut context, 0xdead_beef, 0x1000, 0x1100).await.unwrap(), 0);
        assert_eq!(
            events.lock().as_slice(),
            ["https://example.com/a?b=c"]
        );
    }

    #[futures_test::test]
    async fn lgt_sys_execute_unsupported_command_returns_minus_16_without_using_argv() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut context = test_context(events);

        context.write_bytes(0x1000, b"UNKNOWN\0").unwrap();

        assert_eq!(execute(&mut context, 0xdead_beef, 0x1000, 0).await.unwrap(), -16);
    }

    #[futures_test::test]
    async fn lgt_sys_execute_permission_gated_commands_follow_allowed_hal_result() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut context = test_context(events);

        context.write_bytes(0x1000, b"MMSSEND\0").unwrap();
        context.write_bytes(0x1100, b"APPCALL\0").unwrap();

        assert_eq!(execute(&mut context, 0, 0x1000, 0).await.unwrap(), -16);
        assert_eq!(execute(&mut context, 0, 0x1100, 0).await.unwrap(), -16);
    }
}
