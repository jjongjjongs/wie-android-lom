use wie_util::{Result, read_null_terminated_string_bytes};
use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

/// WIPI-C MC_fsList.
///
/// The LGT implementation returns direct child basenames as NUL-separated
/// local-code strings followed by an additional terminating NUL.
pub async fn list(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    output: WIPICWord,
    output_size: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_fsList({path:#x}, {output:#x}, {output_size}, {access})");

    // Native WPFS_IsPossibleAccess(1, access):
    // - 1 and 100 are always allowed.
    // - 2 and 3 depend on the current DLET access-permission bitmask.
    //
    // WIE does not currently model LGT's per-DLET permission property
    // (native property 202 / APM fallback), so recognized access modes 2/3
    // are accepted as a compatibility adaptation rather than rejected
    // without the permission state needed to make the native decision.
    if !matches!(access, 1 | 2 | 3 | 100) {
        return Ok(-24);
    }

    if path == 0 {
        return Ok(-3);
    }

    let path_bytes = read_null_terminated_string_bytes(context, path)?;
    if path_bytes.len() > 128 {
        return Ok(-11);
    }
    let path = encoding_rs::EUC_KR.decode(&path_bytes).0.into_owned();

    if output == 0 {
        return Ok(-3);
    }
    // MH_fileList receives this as a signed 32-bit capacity.
    if output_size as i32 <= 0 {
        return Ok(-18);
    }

    let Some(entries) = context.system().filesystem().list(&path).await else {
        return Ok(-1);
    };

    let mut cursor = 0usize;
    for entry in entries {
        let (encoded, _, _) = encoding_rs::EUC_KR.encode(&entry);
        let entry_len = encoded.len() + 1;

        // MH_fileList checks `buffer_size <= used + entry_len` before copying
        // the current entry, reserving one byte for the final empty string.
        // Entries copied before a later short-buffer failure remain visible.
        if output_size as usize <= cursor + entry_len {
            return Ok(-18);
        }

        context.write_bytes(output + cursor as u32, encoded.as_ref())?;
        context.write_bytes(output + cursor as u32 + encoded.len() as u32, &[0])?;
        cursor += entry_len;
    }

    // Empty directory => one NUL. Non-empty list => the second NUL after the
    // final entry, producing name1\0name2\0...\0\0.
    context.write_bytes(output + cursor as u32, &[0])?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, vec};

    use test_utils::TestPlatform;
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::{ByteRead, ByteWrite};

    use crate::context::{WIPICContext, test::TestContext};

    use super::list;

    fn filesystem_test_context() -> TestContext {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        TestContext::with_system(system)
    }

    #[futures_test::test]
    async fn lgt_fs_list_serializes_platform_and_virtual_entries_with_double_nul() {
        let mut context = filesystem_test_context();

        context.system().filesystem().write("save/platform.dat", 0, &[1]).await;
        context.system().filesystem().add_virtual("save/virtual.dat", vec![2]);

        context.write_bytes(0x1000, b"save\0").unwrap();
        context.write_bytes(0x2000, &[0xCC; 64]).unwrap();

        assert_eq!(list(&mut context, 0x1000, 0x2000, 64, 1).await.unwrap(), 0);

        let expected = b"platform.dat\0virtual.dat\0\0";
        let mut actual = [0u8; 30];
        context.read_bytes(0x2000, &mut actual).unwrap();

        assert_eq!(&actual[..expected.len()], expected);
        assert_eq!(actual[expected.len()], 0xCC);
    }

    #[futures_test::test]
    async fn lgt_fs_list_short_buffer_keeps_prior_complete_entries() {
        let mut context = filesystem_test_context();

        context.system().filesystem().write("save/first", 0, &[1]).await;
        context.system().filesystem().add_virtual("save/second", vec![2]);

        context.write_bytes(0x1000, b"save\0").unwrap();
        context.write_bytes(0x2000, &[0xCC; 32]).unwrap();

        // "first\0" consumes 6 bytes. A 12-byte buffer cannot fit
        // "second\0" while also reserving the final terminating NUL.
        assert_eq!(list(&mut context, 0x1000, 0x2000, 12, 1).await.unwrap(), -18);

        let mut actual = [0u8; 12];
        context.read_bytes(0x2000, &mut actual).unwrap();

        assert_eq!(&actual[..6], b"first\0");
        assert_eq!(&actual[6..], &[0xCC; 6]);
    }

    #[futures_test::test]
    async fn lgt_fs_list_empty_directory_writes_single_nul() {
        let mut context = filesystem_test_context();

        // Materialize the directory through a child and remove the child;
        // MemoryFilesystem models directories implicitly, so root is the
        // portable empty-directory case for this test harness.
        context.write_bytes(0x1000, b"/\0").unwrap();
        context.write_bytes(0x2000, &[0xCC; 4]).unwrap();

        assert_eq!(list(&mut context, 0x1000, 0x2000, 4, 1).await.unwrap(), 0);

        let mut actual = [0u8; 4];
        context.read_bytes(0x2000, &mut actual).unwrap();
        assert_eq!(actual, [0, 0xCC, 0xCC, 0xCC]);
    }

    #[futures_test::test]
    async fn lgt_fs_list_rejects_invalid_access_and_zero_capacity() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"/\0").unwrap();

        assert_eq!(list(&mut context, 0x1000, 0x2000, 16, 0).await.unwrap(), -24);
        assert_eq!(list(&mut context, 0x1000, 0x2000, 0, 1).await.unwrap(), -18);
        assert_eq!(list(&mut context, 0x1000, 0x2000, 0x8000_0000, 1).await.unwrap(), -18);
    }
}

