use wie_util::{Result, read_null_terminated_string_bytes};

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

/// `MC_utilInetAddrInt` - parse a dotted IPv4 address into the native
/// little-endian integer representation used by the LGT WIPI runtime.
///
/// This deliberately follows the native parser rather than a strict IPv4
/// parser: exactly three dots are required, empty components are accepted,
/// and each component accumulates in an 8-bit byte (wrapping modulo 256).
/// A null pointer or any non-digit/non-dot character returns 0xffffffff.
pub async fn inet_addr_int(context: &mut dyn WIPICContext, address: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_utilInetAddrInt({address:#x})");

    if address == 0 {
        return Ok(u32::MAX as _);
    }

    let input = read_null_terminated_string_bytes(context, address)?;
    let mut octets = [0u8; 4];
    let mut index = 0usize;

    for byte in input {
        match byte {
            b'0'..=b'9' => {
                octets[index] = octets[index].wrapping_mul(10).wrapping_add(byte - b'0');
            }
            b'.' => {
                index += 1;
                if index > 3 {
                    return Ok(u32::MAX as _);
                }
            }
            _ => return Ok(u32::MAX as _),
        }
    }

    if index != 3 {
        return Ok(u32::MAX as _);
    }

    Ok(u32::from_le_bytes(octets) as _)
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;

    use test_utils::TestPlatform;
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::ByteWrite;

    use crate::context::test::TestContext;

    use super::inet_addr_int;

    #[futures_test::test]
    async fn lgt_inet_addr_int_matches_native_byte_order() {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);

        context.write_bytes(0x1000, b"1.2.3.4\0").unwrap();
        assert_eq!(inet_addr_int(&mut context, 0x1000).await.unwrap(), 0x0403_0201);
    }

    #[futures_test::test]
    async fn lgt_inet_addr_int_wraps_each_component_to_u8() {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);

        context.write_bytes(0x1000, b"256.511.258.257\0").unwrap();
        assert_eq!(inet_addr_int(&mut context, 0x1000).await.unwrap(), 0x0102_ff00);
    }

    #[futures_test::test]
    async fn lgt_inet_addr_int_accepts_empty_components_like_native() {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);

        context.write_bytes(0x1000, b".1.2.\0").unwrap();
        assert_eq!(inet_addr_int(&mut context, 0x1000).await.unwrap(), 0x0002_0100);
    }

    #[futures_test::test]
    async fn lgt_inet_addr_int_rejects_null_bad_characters_and_wrong_dot_count() {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        let mut context = TestContext::with_system(system);

        assert_eq!(inet_addr_int(&mut context, 0).await.unwrap(), u32::MAX);

        context.write_bytes(0x1000, b"1.2.3\0").unwrap();
        assert_eq!(inet_addr_int(&mut context, 0x1000).await.unwrap(), u32::MAX);

        context.write_bytes(0x1100, b"1.2.3.4.5\0").unwrap();
        assert_eq!(inet_addr_int(&mut context, 0x1100).await.unwrap(), u32::MAX);

        context.write_bytes(0x1200, b"1.2.x.4\0").unwrap();
        assert_eq!(inet_addr_int(&mut context, 0x1200).await.unwrap(), u32::MAX);
    }
}
