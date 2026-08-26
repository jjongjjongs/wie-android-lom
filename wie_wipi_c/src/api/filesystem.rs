use alloc::{collections::BTreeMap, string::String, sync::Arc};

use spin::Mutex;
use wie_util::{Result, read_null_terminated_string_bytes};
use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub mode: i32,
    pub cursor: usize,
}

pub struct FilesystemState {
    next_fd: i32,
    entries: BTreeMap<i32, FileEntry>,
}

impl Default for FilesystemState {
    fn default() -> Self {
        Self {
            // Native DFS `_dfs_fd_seq` starts at 1 and advances after every
            // successful dfs_open allocation.
            next_fd: 1,
            entries: BTreeMap::new(),
        }
    }
}

pub type SharedFilesystemState = Arc<Mutex<FilesystemState>>;

pub fn new_state() -> SharedFilesystemState {
    Arc::new(Mutex::new(FilesystemState::default()))
}

impl FilesystemState {
    fn register(&mut self, path: String, mode: i32, cursor: usize) -> i32 {
        let fd = self.next_fd;
        self.next_fd = self.next_fd.wrapping_add(1);
        if self.next_fd <= 0 {
            self.next_fd = 1;
        }

        self.entries.insert(fd, FileEntry { path, mode, cursor });
        fd
    }

    fn entry(&self, fd: i32) -> Option<&FileEntry> {
        self.entries.get(&fd)
    }

    fn advance_cursor(&mut self, fd: i32, count: usize) -> bool {
        let Some(entry) = self.entries.get_mut(&fd) else {
            return false;
        };

        entry.cursor = entry.cursor.wrapping_add(count);
        true
    }
}

/// WIPI-C MC_fsOpen (service 0x190).
///
/// Native LGT modes:
/// - 1: read-only; existing file required
/// - 2: write/append; create when missing
/// - 4: write/truncate; create when missing
/// - 8: read/write; create when missing
pub async fn open(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    mode: i32,
    access: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_fsOpen({path:#x}, {mode}, {access})");

    // Native calls WPFS_IsPossibleAccess(mode, access) before path handling.
    // Access 1 and 100 are unconditional. Access 2/3 depend on a per-DLET
    // permission property that WIE does not currently model, so recognized
    // values 2/3 use the same compatibility adaptation as MC_fsList.
    if !matches!(access, 1 | 2 | 3 | 100) {
        return Ok(-24); // M_E_ACCESS
    }

    if path == 0 {
        return Ok(-3); // M_E_BADFILENAME
    }

    let path_bytes = read_null_terminated_string_bytes(context, path)?;
    if path_bytes.len() > 128 {
        return Ok(-11); // M_E_LONGNAME
    }
    let path = encoding_rs::EUC_KR.decode(&path_bytes).0.into_owned();

    // MC_fsOpen performs the mode switch only after WPFS_MakeFullPathName.
    if !matches!(mode, 1 | 2 | 4 | 8) {
        return Ok(-9); // M_E_INVALID
    }

    let filesystem = context.system().filesystem().clone();
    let exists = filesystem.exists(&path).await;

    let cursor = match mode {
        1 => {
            if !exists {
                return Ok(-12); // M_E_NOENT
            }
            0
        }
        2 => {
            // Native O_WRONLY|O_APPEND, falling back to O_WRONLY|O_CREAT.
            // O_APPEND does not move the initial file offset to EOF; it
            // redirects each write to EOF when that write occurs.
            if !exists {
                filesystem.truncate(&path, 0).await;
                if !filesystem.exists(&path).await {
                    return Ok(-1);
                }
            }
            0
        }
        4 => {
            // Native O_WRONLY|O_TRUNC, falling back to
            // O_WRONLY|O_TRUNC|O_CREAT.
            filesystem.truncate(&path, 0).await;
            if filesystem.size(&path).await != Some(0) {
                return Ok(-1);
            }
            0
        }
        8 => {
            // Native O_RDWR, falling back to O_RDWR|O_CREAT.
            if !exists {
                filesystem.truncate(&path, 0).await;
                if !filesystem.exists(&path).await {
                    return Ok(-1);
                }
            }
            0
        }
        _ => unreachable!(),
    };

    Ok(context.filesystem_state().lock().register(path, mode, cursor))
}

/// WIPI-C MC_fsRead (service 0x191).
///
/// Native contract:
/// - null buffer or signed size <= 0 => -1
/// - invalid fd => -2
/// - EOF => -23
/// - other read failure => -1
/// - successful short/full read returns its byte count
pub async fn read(
    context: &mut dyn WIPICContext,
    fd: i32,
    buffer: WIPICWord,
    size: i32,
) -> Result<i32> {
    tracing::debug!("MC_fsRead({fd}, {buffer:#x}, {size})");

    if buffer == 0 || size <= 0 {
        return Ok(-1);
    }

    let entry = {
        let state = context.filesystem_state();
        let state = state.lock();
        state.entry(fd).cloned()
    };

    let Some(entry) = entry else {
        return Ok(-2);
    };

    // Native modes 2 and 4 are opened O_WRONLY. Their POSIX read fails
    // with EBADF; the native wrapper ultimately maps that path to -1.
    if matches!(entry.mode, 2 | 4) {
        return Ok(-1);
    }

    let size = size as usize;
    let mut data = alloc::vec![0u8; size];

    let filesystem = context.system().filesystem().clone();
    let Some(read) = filesystem
        .read(&entry.path, entry.cursor, size, &mut data)
        .await
    else {
        return Ok(-1);
    };

    if read == 0 {
        return Ok(-23);
    }

    context.write_bytes(buffer, &data[..read])?;

    let state = context.filesystem_state();
    let mut state = state.lock();
    if !state.advance_cursor(fd, read) {
        return Ok(-2);
    }

    Ok(read as i32)
}

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

    use super::{list, open, read};

    fn filesystem_test_context() -> TestContext {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        TestContext::with_system(system)
    }

    #[futures_test::test]
    async fn lgt_fs_open_native_modes_and_fd_sequence() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/existing.dat", 0, b"abc")
            .await;

        context.write_bytes(0x1000, b"save/existing.dat\0").unwrap();
        context.write_bytes(0x1100, b"save/append.dat\0").unwrap();
        context.write_bytes(0x1200, b"save/truncate.dat\0").unwrap();
        context.write_bytes(0x1300, b"save/readwrite.dat\0").unwrap();

        context
            .system()
            .filesystem()
            .write("save/truncate.dat", 0, b"old")
            .await;

        let fd1 = open(&mut context, 0x1000, 1, 1).await.unwrap();
        let fd2 = open(&mut context, 0x1100, 2, 1).await.unwrap();
        let fd3 = open(&mut context, 0x1200, 4, 1).await.unwrap();
        let fd4 = open(&mut context, 0x1300, 8, 1).await.unwrap();

        assert_eq!((fd1, fd2, fd3, fd4), (1, 2, 3, 4));
        assert_eq!(
            context.system().filesystem().size("save/append.dat").await,
            Some(0)
        );
        assert_eq!(
            context.system().filesystem().size("save/truncate.dat").await,
            Some(0)
        );
        assert_eq!(
            context.system().filesystem().size("save/readwrite.dat").await,
            Some(0)
        );

        let state = context.filesystem_state();
        let state = state.lock();
        let e1 = state.entry(fd1).unwrap();
        let e2 = state.entry(fd2).unwrap();
        let e3 = state.entry(fd3).unwrap();
        let e4 = state.entry(fd4).unwrap();

        assert_eq!((e1.path.as_str(), e1.mode, e1.cursor), ("save/existing.dat", 1, 0));
        assert_eq!((e2.path.as_str(), e2.mode, e2.cursor), ("save/append.dat", 2, 0));
        assert_eq!((e3.path.as_str(), e3.mode, e3.cursor), ("save/truncate.dat", 4, 0));
        assert_eq!((e4.path.as_str(), e4.mode, e4.cursor), ("save/readwrite.dat", 8, 0));
    }

    #[futures_test::test]
    async fn lgt_fs_open_append_starts_at_zero_offset() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/data.bin", 0, b"12345")
            .await;
        context.write_bytes(0x1000, b"save/data.bin\0").unwrap();

        let fd = open(&mut context, 0x1000, 2, 1).await.unwrap();
        let state = context.filesystem_state();
        let state = state.lock();

        assert_eq!(state.entry(fd).unwrap().cursor, 0);
    }

    #[futures_test::test]
    async fn lgt_fs_open_rejects_native_error_cases() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"save/missing.dat\0").unwrap();

        // Access validation precedes path/mode processing.
        assert_eq!(open(&mut context, 0, 3, 0).await.unwrap(), -24);
        assert_eq!(open(&mut context, 0, 3, 1).await.unwrap(), -3);

        // Mode validation follows path construction.
        assert_eq!(open(&mut context, 0x1000, 3, 1).await.unwrap(), -9);
        assert_eq!(open(&mut context, 0x1000, 1, 1).await.unwrap(), -12);
    }

    #[futures_test::test]
    async fn lgt_fs_read_returns_data_and_advances_cursor() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/read.dat", 0, b"abcdef")
            .await;
        context.write_bytes(0x1000, b"save/read.dat\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 8]).unwrap();

        let fd = open(&mut context, 0x1000, 1, 1).await.unwrap();
        assert_eq!(fd, 1);

        assert_eq!(read(&mut context, fd, 0x2000, 4).await.unwrap(), 4);

        let mut first = [0u8; 8];
        context.read_bytes(0x2000, &mut first).unwrap();
        assert_eq!(&first[..4], b"abcd");
        assert_eq!(&first[4..], &[0xcc; 4]);

        {
            let state = context.filesystem_state();
            let state = state.lock();
            assert_eq!(state.entry(fd).unwrap().cursor, 4);
        }

        context.write_bytes(0x2100, &[0xcc; 8]).unwrap();
        assert_eq!(read(&mut context, fd, 0x2100, 4).await.unwrap(), 2);

        let mut second = [0u8; 8];
        context.read_bytes(0x2100, &mut second).unwrap();
        assert_eq!(&second[..2], b"ef");
        assert_eq!(&second[2..], &[0xcc; 6]);

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 6);
    }

    #[futures_test::test]
    async fn lgt_fs_read_eof_returns_minus_23_without_advancing() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/eof.dat", 0, b"x")
            .await;
        context.write_bytes(0x1000, b"save/eof.dat\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 4]).unwrap();

        let fd = open(&mut context, 0x1000, 1, 1).await.unwrap();

        assert_eq!(read(&mut context, fd, 0x2000, 1).await.unwrap(), 1);
        assert_eq!(read(&mut context, fd, 0x2001, 1).await.unwrap(), -23);

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 1);
    }

    #[futures_test::test]
    async fn lgt_fs_read_matches_native_validation_and_mode_errors() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/write-only.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/write-only.dat\0").unwrap();

        // MC_fsRead checks buffer/size before dfs_read validates fd.
        assert_eq!(read(&mut context, 77, 0, 1).await.unwrap(), -1);
        assert_eq!(read(&mut context, 77, 0x2000, 0).await.unwrap(), -1);
        assert_eq!(read(&mut context, 77, 0x2000, -1).await.unwrap(), -1);

        assert_eq!(read(&mut context, 0, 0x2000, 1).await.unwrap(), -2);
        assert_eq!(read(&mut context, 77, 0x2000, 1).await.unwrap(), -2);

        let append_fd = open(&mut context, 0x1000, 2, 1).await.unwrap();
        let trunc_fd = open(&mut context, 0x1000, 4, 1).await.unwrap();

        // Both native modes are O_WRONLY and read ultimately maps to -1.
        assert_eq!(read(&mut context, append_fd, 0x2000, 1).await.unwrap(), -1);
        assert_eq!(read(&mut context, trunc_fd, 0x2000, 1).await.unwrap(), -1);
    }

    #[futures_test::test]
    async fn lgt_fs_read_supports_virtual_read_only_files() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .add_virtual("res/data.bin", b"virtual".to_vec());

        context.write_bytes(0x1000, b"res/data.bin\0").unwrap();
        context.write_bytes(0x2000, &[0; 7]).unwrap();

        let fd = open(&mut context, 0x1000, 1, 1).await.unwrap();
        assert_eq!(read(&mut context, fd, 0x2000, 7).await.unwrap(), 7);

        let mut actual = [0u8; 7];
        context.read_bytes(0x2000, &mut actual).unwrap();
        assert_eq!(&actual, b"virtual");
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

