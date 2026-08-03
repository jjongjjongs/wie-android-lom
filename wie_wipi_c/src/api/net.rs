use alloc::{boxed::Box, vec::Vec};

use wipi_types::wipic::WIPICWord;

use wie_util::{Result, WieError};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};

/// `ENOTCONN` - the emulator has no network, so a connection attempt reports
/// that there is none rather than a generic failure. Titles that tell an
/// offline device apart from a transient error branch on this: a generic error
/// (`M_E_ERROR`, -1) is retried, while `ENOTCONN` is taken as "no network here"
/// and sends the title down its offline path. WipiPlayer relies on the same
/// distinction - it fast-fails `WipiSocket.connect`/`send` and the LGT billing
/// sockets with exactly this value.
const M_E_NOTCONN: u32 = -14i32 as u32;

pub async fn connect(context: &mut dyn WIPICContext, cb: WIPICWord, param: WIPICWord) -> Result<i32> {
    tracing::warn!("stub MC_netConnect({cb:#x}, {param:#x})");

    struct ConnectCallback {
        cb: WIPICWord,
        param: WIPICWord,
    }

    #[async_trait::async_trait]
    impl MethodBody<WieError> for ConnectCallback {
        #[tracing::instrument(name = "timer", skip_all)]
        async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
            context.system().sleep(1).await; // simulate some delay

            context.call_function(self.cb, &[M_E_NOTCONN, self.param]).await?; // callback with ENOTCONN

            Ok(WIPICResult { results: Vec::new() })
        }
    }

    context.spawn(Box::new(ConnectCallback { cb, param }))?;

    Ok(0)
}

pub async fn close(_context: &mut dyn WIPICContext) -> Result<()> {
    tracing::warn!("stub MC_netClose()");

    Ok(())
}

pub async fn socket_close(_context: &mut dyn WIPICContext, fd: i32) -> Result<i32> {
    tracing::warn!("stub MC_netSocketClose({fd})");

    Ok(-1) // M_E_ERROR
}
