use wie_util::Result;

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

/// `MC_utilHtonl` - host to network byte order, 32 bit. The emulated code always
/// runs little endian, so this is a byte swap to big endian.
pub async fn htonl(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_utilHtonl({val:#x})");

    Ok(val.to_be())
}

/// `MC_utilHtons` - host to network byte order, 16 bit.
pub async fn htons(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_utilHtons({val:#x})");

    Ok((val as u16).to_be() as _)
}

/// `MC_utilNtohl` - network to host byte order, 32 bit. Symmetric with `htonl`
/// on a little endian host.
pub async fn ntohl(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_utilNtohl({val:#x})");

    Ok(u32::from_be(val))
}

/// `MC_utilNtohs` - network to host byte order, 16 bit.
pub async fn ntohs(_context: &mut dyn WIPICContext, val: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_utilNtohs({val:#x})");

    Ok(u16::from_be(val as u16) as _)
}
