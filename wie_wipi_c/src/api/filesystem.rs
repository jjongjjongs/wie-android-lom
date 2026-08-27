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

    fn set_cursor(&mut self, fd: i32, cursor: usize) -> bool {
        let Some(entry) = self.entries.get_mut(&fd) else {
            return false;
        };

        entry.cursor = cursor;
        true
    }

    fn rename_path(&mut self, from: &str, to: &str) {
        let mut prefix = String::from(from);
        prefix.push('/');

        for entry in self.entries.values_mut() {
            if entry.path == from {
                entry.path = String::from(to);
            } else if let Some(suffix) = entry.path.strip_prefix(&prefix) {
                let mut renamed = String::from(to);
                renamed.push('/');
                renamed.push_str(suffix);
                entry.path = renamed;
            }
        }
    }

    fn close(&mut self, fd: i32) -> bool {
        self.entries.remove(&fd).is_some()
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

/// WIPI-C MC_fsWrite (service 0x192).
///
/// Native contract:
/// - null buffer or signed size < 0 => -9
/// - size == 0 is a valid zero-byte write and returns 0
/// - invalid fd => -2
/// - ENOSPC => -13
/// - other write failure => -1
/// - successful write returns the byte count
pub async fn write(
    context: &mut dyn WIPICContext,
    fd: i32,
    buffer: WIPICWord,
    size: i32,
) -> Result<i32> {
    tracing::debug!("MC_fsWrite({fd}, {buffer:#x}, {size})");

    if buffer == 0 || size < 0 {
        return Ok(-9);
    }

    let entry = {
        let state = context.filesystem_state();
        let state = state.lock();
        state.entry(fd).cloned()
    };

    let Some(entry) = entry else {
        return Ok(-2);
    };

    // Native mode 1 is O_RDONLY. POSIX write fails with EBADF and the
    // native wrapper collapses that path to the generic -1 result.
    if entry.mode == 1 {
        return Ok(-1);
    }

    // POSIX write(fd, ..., 0) succeeds with 0. Preserve that distinction
    // before calling WIE's backend, whose 0 return also denotes failure.
    if size == 0 {
        return Ok(0);
    }

    let size = size as usize;
    let mut data = alloc::vec![0u8; size];
    context.read_bytes(buffer, &mut data)?;

    let filesystem = context.system().filesystem().clone();

    // Mode 2 was opened O_APPEND. Every write therefore starts at the
    // current EOF regardless of a preceding seek. Other writable modes use
    // the descriptor's current file offset.
    let offset = if entry.mode == 2 {
        let Some(file_size) = filesystem.size(&entry.path).await else {
            return Ok(-1);
        };
        file_size
    } else {
        entry.cursor
    };

    let written = filesystem.write(&entry.path, offset, &data).await;
    if written == 0 {
        // WIE's Filesystem contract currently collapses disk-full,
        // permission and other backend failures to 0, so ENOSPC cannot be
        // distinguished here. The native generic failure is -1.
        return Ok(-1);
    }

    let state = context.filesystem_state();
    let mut state = state.lock();
    let updated = if entry.mode == 2 {
        state.set_cursor(fd, offset.wrapping_add(written))
    } else {
        state.advance_cursor(fd, written)
    };

    if !updated {
        return Ok(-2);
    }

    Ok(written as i32)
}

/// WIPI-C MC_fsClose (service 0x193).
///
/// Native dfs_close removes the descriptor object only after the underlying
/// close callback succeeds. WIE's filesystem abstraction has no persistent
/// host handle, so a valid synthetic descriptor closes by removing its state.
///
/// Native contract:
/// - valid fd => 0
/// - invalid/non-positive/already-closed fd => -2
/// - other low-level close failure => -1
pub async fn close(context: &mut dyn WIPICContext, fd: i32) -> Result<i32> {
    tracing::debug!("MC_fsClose({fd})");

    if fd <= 0 {
        return Ok(-2);
    }

    if !context.filesystem_state().lock().close(fd) {
        return Ok(-2);
    }

    Ok(0)
}

/// WIPI-C MC_fsSeek (service 0x194).
///
/// Native origins:
/// - 0: SEEK_SET
/// - 1: SEEK_CUR
/// - 2: SEEK_END
///
/// Native additionally forbids seeking beyond EOF. On that failure the host
/// implementation restores the previous position.
///
/// Return contract:
/// - origin 0 with negative offset => -4
/// - origin 2 with positive offset => -4
/// - invalid origin => -9
/// - invalid fd => -2
/// - negative resulting position / generic seek failure => -1
/// - resulting position beyond EOF => -4
/// - success => new absolute file offset
pub async fn seek(
    context: &mut dyn WIPICContext,
    fd: i32,
    offset: i32,
    origin: i32,
) -> Result<i32> {
    tracing::debug!("MC_fsSeek({fd}, {offset}, {origin})");

    // These checks happen in MC_fsSeek before dfs_seek validates the fd.
    if origin == 0 && offset < 0 {
        return Ok(-4);
    }
    if origin == 2 && offset > 0 {
        return Ok(-4);
    }
    if !matches!(origin, 0 | 1 | 2) {
        return Ok(-9);
    }

    let entry = {
        let state = context.filesystem_state();
        let state = state.lock();
        state.entry(fd).cloned()
    };

    let Some(entry) = entry else {
        return Ok(-2);
    };

    // Native MH/AND_fileSeek obtains EOF before performing the requested
    // seek and rejects positions past it.
    let filesystem = context.system().filesystem().clone();
    let Some(file_size) = filesystem.size(&entry.path).await else {
        return Ok(-1);
    };

    let current = entry.cursor as i64;
    let end = file_size as i64;
    let offset = offset as i64;

    let target = match origin {
        0 => offset,
        1 => current + offset,
        2 => end + offset,
        _ => unreachable!(),
    };

    // A negative lseek target fails at the host layer and is collapsed to
    // the native generic filesystem error.
    if target < 0 {
        return Ok(-1);
    }

    // MH/AND_fileSeek explicitly restores the old offset when the requested
    // position lies beyond EOF.
    if target > end {
        return Ok(-4);
    }

    if target > i32::MAX as i64 {
        return Ok(-1);
    }

    let target = target as usize;
    if !context.filesystem_state().lock().set_cursor(fd, target) {
        return Ok(-2);
    }

    Ok(target as i32)
}

/// WIPI-C MC_fsTell (canonical service 0x19f).
///
/// Native flow is `dfs_control(fd, 4, 0, 0, 0)`. The filesystem fcontrol
/// callback handles command 4 by issuing `lseek(fd, 0, SEEK_CUR)`.
///
/// Native externally visible contract:
/// - valid descriptor => current absolute file offset
/// - invalid, nonpositive, or closed descriptor => -2
/// - any other tell failure => -1
///
/// WIE synthetic descriptors already store their current offset directly.
pub async fn tell(context: &mut dyn WIPICContext, fd: i32) -> Result<i32> {
    tracing::debug!("MC_fsTell({fd})");

    let state = context.filesystem_state();
    let state = state.lock();

    let Some(entry) = state.entry(fd) else {
        return Ok(-2);
    };

    Ok(i32::try_from(entry.cursor).unwrap_or(i32::MAX))
}

/// WIPI-C MC_fsFileAttribute (service 0x195).
///
/// Native output is three 32-bit words:
/// - word 0: 1 for a directory, 0 for a regular file
/// - word 1: timestamp/attribute slot; LGT's HAL path supplies 0
/// - word 2: regular-file size, or 0 for a directory
///
/// Native error contract:
/// - inaccessible access selector => -24
/// - bad/null filename => -3
/// - path longer than 128 bytes => -11
/// - null output pointer => -1
/// - missing/stat failure => -1
pub async fn file_attribute(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    output: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_fsFileAttribute({path:#x}, {output:#x}, {access})");

    // Native WPFS_IsPossibleAccess(1, access) precedes path processing.
    // As with the other LGT filesystem services, WIE accepts recognized
    // access values 2/3 because the per-DLET permission bitmask is not
    // currently represented.
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

    // Native checks the destination only after full-path construction.
    if output == 0 {
        return Ok(-1);
    }

    let filesystem = context.system().filesystem().clone();

    let (is_directory, size) = if let Some(size) = filesystem.size(&path).await {
        (0u32, size as u32)
    } else if filesystem.list(&path).await.is_some() {
        (1u32, 0u32)
    } else {
        return Ok(-1);
    };

    context.write_bytes(output, &is_directory.to_le_bytes())?;
    context.write_bytes(output + 4, &0u32.to_le_bytes())?;
    context.write_bytes(output + 8, &size.to_le_bytes())?;

    Ok(0)
}

/// WIPI-C MC_fsRemove (service 0x196).
///
/// Native flow:
/// WPFS_IsPossibleAccess(2, access) -> path construction -> dfs_stat ->
/// dfs_unlink.
///
/// The stat preflight is significant: a missing path returns -12 before the
/// unlink operation is attempted.
///
/// Native contract representable by WIE's filesystem abstraction:
/// - inaccessible access selector => -24
/// - bad/null filename => -3
/// - path longer than 128 bytes => -11
/// - missing path => -12
/// - directory / generic unlink failure => -1
/// - successful regular-file removal => 0
///
/// Native also distinguishes a busy-file unlink failure as -8, but WIE's
/// boolean backend remove contract does not expose the underlying failure
/// reason.
pub async fn remove(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_fsRemove({path:#x}, {access})");

    // Native uses WPFS_IsPossibleAccess(2, access). WIE does not model the
    // per-DLET permission bitmask, so recognized access selectors 2/3 use
    // the same compatibility adaptation as the other LGT filesystem APIs.
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

    let filesystem = context.system().filesystem().clone();

    // MC_fsRemove performs dfs_stat before dfs_unlink. Preserve the native
    // missing-path distinction rather than collapsing it into remove failure.
    let is_file = filesystem.size(&path).await.is_some();
    let is_directory = if is_file {
        false
    } else {
        filesystem.list(&path).await.is_some()
    };

    if !is_file && !is_directory {
        return Ok(-12);
    }

    // POSIX unlink does not remove directories. Native therefore reaches its
    // generic unlink failure path for a directory.
    if is_directory {
        return Ok(-1);
    }

    // Virtual archive files are visible through size(), but remove() writes
    // only to the persistent platform layer. That correctly leaves them
    // read-only and produces the native generic unlink failure.
    if !filesystem.remove(&path).await {
        return Ok(-1);
    }

    Ok(0)
}

/// WIPI-C MC_fsMkDir (service 0x198).
///
/// Native flow:
/// WPFS_IsPossibleAccess(2, access) -> WPFS_MakeFullPathName -> dfs_mkdir.
///
/// The LGT/Android HAL calls POSIX mkdir(path, 0755), so creation is strictly
/// one level: missing parents are not created automatically.
///
/// Native return mapping:
/// - inaccessible access selector => -24
/// - bad/null filename => -3
/// - path longer than 128 bytes => -11
/// - existing file or directory => -5
/// - missing parent => -12
/// - generic mkdir failure => -1
/// - success => 0
pub async fn mkdir(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    use wie_backend::FilesystemMkdirError;

    tracing::debug!("MC_fsMkDir({path:#x}, {access})");

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

    match context.system().filesystem().mkdir(&path).await {
        Ok(()) => Ok(0),
        Err(FilesystemMkdirError::AlreadyExists) => Ok(-5),
        Err(FilesystemMkdirError::NotFound) => Ok(-12),
        Err(FilesystemMkdirError::NameTooLong) => Ok(-11),
        Err(FilesystemMkdirError::Other) => Ok(-1),
    }
}

/// WIPI-C MC_fsRmDir (service 0x199).
///
/// Native flow:
/// WPFS_IsPossibleAccess(2, access) -> WPFS_MakeFullPathName -> dfs_stat ->
/// dfs_rmdir.
///
/// The stat preflight is significant: any stat failure is returned as -12
/// before rmdir is attempted.
///
/// Native return mapping represented here:
/// - inaccessible access selector => -24
/// - bad/null filename => -3
/// - path longer than 128 bytes => -11
/// - missing/stat failure => -12
/// - non-empty directory => -15
/// - regular file or generic rmdir failure => -1
/// - success => 0
pub async fn rmdir(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    use wie_backend::FilesystemRmDirError;

    tracing::debug!("MC_fsRmDir({path:#x}, {access})");

    // Native WPFS_IsPossibleAccess(2, access) is the first semantic check.
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

    let filesystem = context.system().filesystem().clone();

    // Native dfs_stat precedes dfs_rmdir.
    let is_file = filesystem.size(&path).await.is_some();
    let is_directory = if is_file {
        false
    } else {
        filesystem.list(&path).await.is_some()
    };

    if !is_file && !is_directory {
        return Ok(-12);
    }

    // POSIX rmdir on a regular file fails generically (typically ENOTDIR),
    // which the native HAL collapses to -1.
    if is_file {
        return Ok(-1);
    }

    match filesystem.rmdir(&path).await {
        Ok(()) => Ok(0),
        Err(FilesystemRmDirError::NotFound) => Ok(-12),
        Err(FilesystemRmDirError::NotEmpty) => Ok(-15),
        Err(FilesystemRmDirError::NameTooLong) => Ok(-11),
        Err(FilesystemRmDirError::Other) => Ok(-1),
    }
}

/// WIPI-C MC_fsRename (service 0x197).
///
/// Native flow:
/// WPFS_IsPossibleAccess(2, access) -> source full path -> destination full
/// path -> dfs_move. For paths on the same mount, dfs_move dispatches to the
/// filesystem rename callback, which reaches POSIX rename on LGT/Android.
///
/// Native return mapping represented here:
/// - inaccessible access selector => -24
/// - bad/null source or destination => -3
/// - path longer than 128 bytes => -11
/// - source/destination parent missing => -12
/// - destination conflict mapped from EEXIST => -8
/// - cross-device/non-empty-directory class => -5
/// - generic rename failure => -1
/// - success => 0
pub async fn rename(
    context: &mut dyn WIPICContext,
    from: WIPICWord,
    to: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    use wie_backend::FilesystemRenameError;

    tracing::debug!("MC_fsRename({from:#x}, {to:#x}, {access})");

    // Native WPFS_IsPossibleAccess(2, access) precedes both path conversions.
    // As with the other LGT filesystem calls, selectors 2/3 are accepted
    // because WIE does not model the native per-DLET permission bitmask.
    if !matches!(access, 1 | 2 | 3 | 100) {
        return Ok(-24);
    }

    if from == 0 {
        return Ok(-3);
    }

    let from_bytes = read_null_terminated_string_bytes(context, from)?;
    if from_bytes.len() > 128 {
        return Ok(-11);
    }
    let from = encoding_rs::EUC_KR.decode(&from_bytes).0.into_owned();

    if to == 0 {
        return Ok(-3);
    }

    let to_bytes = read_null_terminated_string_bytes(context, to)?;
    if to_bytes.len() > 128 {
        return Ok(-11);
    }
    let to = encoding_rs::EUC_KR.decode(&to_bytes).0.into_owned();

    // dfs_move explicitly rejects identical refined source/destination paths.
    if from == to {
        return Ok(-1);
    }

    let filesystem = context.system().filesystem().clone();

    match filesystem.rename(&from, &to).await {
        Ok(()) => {
            // Native open file descriptors continue referring to the same
            // renamed filesystem object. WIE descriptors store paths rather
            // than persistent host handles, so retarget them after success.
            context.filesystem_state().lock().rename_path(&from, &to);
            Ok(0)
        }
        Err(FilesystemRenameError::NotFound) => Ok(-12),
        Err(FilesystemRenameError::AlreadyExists) => Ok(-8),
        Err(FilesystemRenameError::CrossDeviceOrNotEmpty) => Ok(-5),
        Err(FilesystemRenameError::NameTooLong) => Ok(-11),
        Err(FilesystemRenameError::Other) => Ok(-1),
    }
}

/// WIPI-C MC_fsGetCounts (canonical service 0x19e).
///
/// Native flow:
/// - WPFS_IsPossibleAccess(1, access);
/// - access 1/2/3: WPFS_MakeFullPathName(path, access);
/// - access 100: bypass WPFS_MakeFullPathName and pass the original path
///   directly to the "wipi root" filesystem;
/// - dfs_control_dev(command 2);
/// - MH/AND_fileGetCounts uses opendir/readdir and counts every direct entry
///   except "." and "..".
///
/// Native return behavior represented here:
/// - inaccessible/unknown access selector => -24;
/// - null filename => -3;
/// - access 1/2/3 path longer than 128 bytes => -11;
/// - missing/non-directory/open failure => -1;
/// - existing empty directory => 0;
/// - otherwise => number of direct child entries.
///
/// Access 100 intentionally does not inherit the 128-byte
/// WPFS_MakeFullPathName limit because the native wrapper bypasses that helper.
pub async fn get_counts(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    access: WIPICWord,
) -> Result<i32> {
    tracing::debug!("MC_fsGetCounts({path:#x}, {access})");

    // Native WPFS_IsPossibleAccess(1, access) executes before path handling.
    // WIE does not model the per-DLET permission bitmap used for selectors
    // 2/3, so all recognized selectors use the established compatibility
    // adaptation used by the neighboring LGT filesystem services.
    if !matches!(access, 1 | 2 | 3 | 100) {
        return Ok(-24);
    }

    if path == 0 {
        return Ok(-3);
    }

    let path_bytes = read_null_terminated_string_bytes(context, path)?;

    // Only selectors 1/2/3 call WPFS_MakeFullPathName. Selector 100 passes
    // the original path directly to dfs_control_dev("wipi root", 2, path).
    if access != 100 && path_bytes.len() > 128 {
        return Ok(-11);
    }

    let path = encoding_rs::EUC_KR.decode(&path_bytes).0.into_owned();

    let Some(entries) = context.system().filesystem().list(&path).await else {
        // MH/AND_fileGetCounts returns -1 when opendir fails. The enclosing
        // DFS/WIPI-C translation leaves that class as the generic -1 result.
        return Ok(-1);
    };

    Ok(i32::try_from(entries.len()).unwrap_or(i32::MAX))
}

/// WIPI-C MC_fsSetMode (canonical service 0x19d).
///
/// Native flow:
/// WPFS_IsPossibleAccess(1, access) -> WPFS_MakeFullPathName ->
/// public mode translation -> dfs_chmod.
///
/// Public modes map to owner permission bits:
/// - 0x10 -> 0400
/// - 0x20 -> 0200
/// - 0x30 -> 0600
///
/// Native return contract represented here:
/// - inaccessible access selector => -24
/// - bad/null filename => -3
/// - path longer than 128 bytes => -11
/// - invalid public mode => -9
/// - missing path or directory => -12
/// - generic chmod failure => -1
/// - success => 0
pub async fn set_mode(
    context: &mut dyn WIPICContext,
    path: WIPICWord,
    mode: i32,
    access: WIPICWord,
) -> Result<i32> {
    use wie_backend::FilesystemSetModeError;

    tracing::debug!("MC_fsSetMode({path:#x}, {mode:#x}, {access})");

    // Native WPFS_IsPossibleAccess(1, access) is evaluated first.
    // WIE does not model the per-DLET permission bitmap, so recognized
    // selectors 2/3 retain the compatibility adaptation used by the other
    // canonical LGT filesystem services.
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

    // MC_fsSetMode performs this switch only after path construction.
    let host_mode = match mode {
        0x10 => 0o400,
        0x20 => 0o200,
        0x30 => 0o600,
        _ => return Ok(-9),
    };

    let filesystem = context.system().filesystem().clone();

    // The native HAL stat() preflight accepts only a regular file.
    let is_file = filesystem.size(&path).await.is_some();
    let is_directory = if is_file {
        false
    } else {
        filesystem.list(&path).await.is_some()
    };

    if !is_file || is_directory {
        return Ok(-12);
    }

    match filesystem.set_mode(&path, host_mode).await {
        Ok(()) => Ok(0),
        Err(FilesystemSetModeError::NotFound) => Ok(-12),
        Err(FilesystemSetModeError::NameTooLong) => Ok(-11),
        Err(FilesystemSetModeError::Other) => Ok(-1),
    }
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
    use alloc::string::String;
    use alloc::{boxed::Box, vec, vec::Vec};

    use test_utils::TestPlatform;
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::{ByteRead, ByteWrite};

    use crate::context::{WIPICContext, test::TestContext};

    use super::{
        close, file_attribute, get_counts, list, mkdir, open, read, remove, rename, rmdir, seek,
        set_mode, tell, write,
    };

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
    async fn lgt_fs_write_writes_at_cursor_and_advances() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/rw.dat", 0, b"abcdef")
            .await;
        context.write_bytes(0x1000, b"save/rw.dat\0").unwrap();
        context.write_bytes(0x2000, b"XYZ").unwrap();

        let fd = open(&mut context, 0x1000, 8, 1).await.unwrap();
        assert_eq!(write(&mut context, fd, 0x2000, 3).await.unwrap(), 3);

        let mut actual = [0u8; 6];
        assert_eq!(
            context
                .system()
                .filesystem()
                .read("save/rw.dat", 0, 6, &mut actual)
                .await,
            Some(6)
        );
        assert_eq!(&actual, b"XYZdef");

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 3);
    }

    #[futures_test::test]
    async fn lgt_fs_write_append_uses_eof_and_sets_cursor_to_new_eof() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/append.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/append.dat\0").unwrap();
        context.write_bytes(0x2000, b"XY").unwrap();

        let fd = open(&mut context, 0x1000, 2, 1).await.unwrap();

        // The descriptor begins at offset zero, but O_APPEND redirects the
        // write itself to EOF.
        {
            let state = context.filesystem_state();
            let state = state.lock();
            assert_eq!(state.entry(fd).unwrap().cursor, 0);
        }

        assert_eq!(write(&mut context, fd, 0x2000, 2).await.unwrap(), 2);

        let mut actual = [0u8; 5];
        assert_eq!(
            context
                .system()
                .filesystem()
                .read("save/append.dat", 0, 5, &mut actual)
                .await,
            Some(5)
        );
        assert_eq!(&actual, b"abcXY");

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 5);
    }

    #[futures_test::test]
    async fn lgt_fs_write_matches_native_validation_and_mode_errors() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/readonly.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/readonly.dat\0").unwrap();
        context.write_bytes(0x2000, b"Z").unwrap();

        // MC_fsWrite validates buffer and signed size before dfs_write.
        assert_eq!(write(&mut context, 77, 0, 1).await.unwrap(), -9);
        assert_eq!(write(&mut context, 77, 0x2000, -1).await.unwrap(), -9);

        // A zero-sized write is valid, but fd validation still happens first.
        assert_eq!(write(&mut context, 77, 0x2000, 0).await.unwrap(), -2);

        assert_eq!(write(&mut context, 0, 0x2000, 1).await.unwrap(), -2);
        assert_eq!(write(&mut context, 77, 0x2000, 1).await.unwrap(), -2);

        let readonly_fd = open(&mut context, 0x1000, 1, 1).await.unwrap();
        assert_eq!(write(&mut context, readonly_fd, 0x2000, 1).await.unwrap(), -1);
        assert_eq!(write(&mut context, readonly_fd, 0x2000, 0).await.unwrap(), -1);

        let rw_fd = open(&mut context, 0x1000, 8, 1).await.unwrap();
        assert_eq!(write(&mut context, rw_fd, 0x2000, 0).await.unwrap(), 0);

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(rw_fd).unwrap().cursor, 0);
    }

    #[futures_test::test]
    async fn lgt_fs_write_truncate_mode_starts_from_zero() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/truncate-write.dat", 0, b"old-data")
            .await;
        context.write_bytes(0x1000, b"save/truncate-write.dat\0").unwrap();
        context.write_bytes(0x2000, b"new").unwrap();

        let fd = open(&mut context, 0x1000, 4, 1).await.unwrap();
        assert_eq!(write(&mut context, fd, 0x2000, 3).await.unwrap(), 3);

        let mut actual = [0u8; 3];
        assert_eq!(
            context
                .system()
                .filesystem()
                .read("save/truncate-write.dat", 0, 3, &mut actual)
                .await,
            Some(3)
        );
        assert_eq!(&actual, b"new");

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 3);
    }

    #[futures_test::test]
    async fn lgt_fs_close_removes_descriptor_and_invalidates_future_io() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/close.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/close.dat\0").unwrap();
        context.write_bytes(0x2000, b"Z").unwrap();

        let fd = open(&mut context, 0x1000, 8, 1).await.unwrap();
        assert_eq!(fd, 1);

        assert_eq!(close(&mut context, fd).await.unwrap(), 0);

        {
            let state = context.filesystem_state();
            let state = state.lock();
            assert!(state.entry(fd).is_none());
        }

        assert_eq!(read(&mut context, fd, 0x2000, 1).await.unwrap(), -2);
        assert_eq!(write(&mut context, fd, 0x2000, 1).await.unwrap(), -2);
        assert_eq!(close(&mut context, fd).await.unwrap(), -2);
    }

    #[futures_test::test]
    async fn lgt_fs_close_rejects_nonpositive_and_unknown_descriptors() {
        let mut context = filesystem_test_context();

        assert_eq!(close(&mut context, 0).await.unwrap(), -2);
        assert_eq!(close(&mut context, -1).await.unwrap(), -2);
        assert_eq!(close(&mut context, 77).await.unwrap(), -2);
    }

    #[futures_test::test]
    async fn lgt_fs_close_does_not_reuse_native_fd_sequence() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/a.dat", 0, b"a")
            .await;
        context
            .system()
            .filesystem()
            .write("save/b.dat", 0, b"b")
            .await;

        context.write_bytes(0x1000, b"save/a.dat\0").unwrap();
        context.write_bytes(0x1100, b"save/b.dat\0").unwrap();

        let first = open(&mut context, 0x1000, 1, 1).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(close(&mut context, first).await.unwrap(), 0);

        let second = open(&mut context, 0x1100, 1, 1).await.unwrap();
        assert_eq!(second, 2);
    }

    #[futures_test::test]
    async fn lgt_fs_seek_set_cur_and_end_return_absolute_offset() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/seek.dat", 0, b"abcdef")
            .await;
        context.write_bytes(0x1000, b"save/seek.dat\0").unwrap();

        let fd = open(&mut context, 0x1000, 8, 1).await.unwrap();

        assert_eq!(seek(&mut context, fd, 3, 0).await.unwrap(), 3);
        assert_eq!(seek(&mut context, fd, 2, 1).await.unwrap(), 5);
        assert_eq!(seek(&mut context, fd, -2, 2).await.unwrap(), 4);
        assert_eq!(seek(&mut context, fd, 0, 2).await.unwrap(), 6);

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 6);
    }

    #[futures_test::test]
    async fn lgt_fs_seek_matches_native_validation_order() {
        let mut context = filesystem_test_context();

        // MC_fsSeek performs these argument checks before dfs_seek gets a
        // chance to reject the fd.
        assert_eq!(seek(&mut context, 77, -1, 0).await.unwrap(), -4);
        assert_eq!(seek(&mut context, 77, 1, 2).await.unwrap(), -4);
        assert_eq!(seek(&mut context, 77, 0, 3).await.unwrap(), -9);
        assert_eq!(seek(&mut context, 77, 0, -1).await.unwrap(), -9);

        assert_eq!(seek(&mut context, 0, 0, 0).await.unwrap(), -2);
        assert_eq!(seek(&mut context, -1, 0, 0).await.unwrap(), -2);
        assert_eq!(seek(&mut context, 77, 0, 0).await.unwrap(), -2);
    }

    #[futures_test::test]
    async fn lgt_fs_seek_rejects_beyond_eof_and_preserves_cursor() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/seek-bound.dat", 0, b"abcdef")
            .await;
        context.write_bytes(0x1000, b"save/seek-bound.dat\0").unwrap();

        let fd = open(&mut context, 0x1000, 8, 1).await.unwrap();
        assert_eq!(seek(&mut context, fd, 3, 0).await.unwrap(), 3);

        // Native MH/AND_fileSeek restores the prior cursor if the requested
        // result lies past EOF.
        assert_eq!(seek(&mut context, fd, 7, 0).await.unwrap(), -4);
        assert_eq!(seek(&mut context, fd, 4, 1).await.unwrap(), -4);

        {
            let state = context.filesystem_state();
            let state = state.lock();
            assert_eq!(state.entry(fd).unwrap().cursor, 3);
        }

        // A negative resulting SEEK_CUR position is a host lseek failure,
        // which reaches MC_fsSeek as the generic -1 path.
        assert_eq!(seek(&mut context, fd, -4, 1).await.unwrap(), -1);

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 3);
    }

    #[futures_test::test]
    async fn lgt_fs_tell_returns_current_cursor_after_seek_read_and_write() {
        const BUFFER: u32 = 0x1000;

        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .truncate("save/tell.dat", 6)
            .await;
        context
            .system()
            .filesystem()
            .write("save/tell.dat", 0, &[1, 2, 3, 4, 5, 6])
            .await;

        let fd = context
            .filesystem_state()
            .lock()
            .register(String::from("save/tell.dat"), 8, 0);

        assert_eq!(tell(&mut context, fd).await.unwrap(), 0);

        assert_eq!(seek(&mut context, fd, 3, 0).await.unwrap(), 3);
        assert_eq!(tell(&mut context, fd).await.unwrap(), 3);

        assert_eq!(read(&mut context, fd, BUFFER, 2).await.unwrap(), 2);
        assert_eq!(tell(&mut context, fd).await.unwrap(), 5);

        context.write_bytes(BUFFER, &[9]).unwrap();
        assert_eq!(write(&mut context, fd, BUFFER, 1).await.unwrap(), 1);
        assert_eq!(tell(&mut context, fd).await.unwrap(), 6);
    }

    #[futures_test::test]
    async fn lgt_fs_tell_invalid_nonpositive_and_closed_descriptors_return_minus_2() {
        let mut context = filesystem_test_context();

        assert_eq!(tell(&mut context, 0).await.unwrap(), -2);
        assert_eq!(tell(&mut context, -1).await.unwrap(), -2);
        assert_eq!(tell(&mut context, 12345).await.unwrap(), -2);

        let fd = context
            .filesystem_state()
            .lock()
            .register(String::from("save/tell.dat"), 1, 4);

        assert_eq!(tell(&mut context, fd).await.unwrap(), 4);
        assert_eq!(close(&mut context, fd).await.unwrap(), 0);
        assert_eq!(tell(&mut context, fd).await.unwrap(), -2);
    }

    #[futures_test::test]
    async fn lgt_fs_seek_append_cursor_changes_but_write_still_uses_eof() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/seek-append.dat", 0, b"abcde")
            .await;
        context.write_bytes(0x1000, b"save/seek-append.dat\0").unwrap();
        context.write_bytes(0x2000, b"XY").unwrap();

        let fd = open(&mut context, 0x1000, 2, 1).await.unwrap();

        assert_eq!(seek(&mut context, fd, 1, 0).await.unwrap(), 1);
        {
            let state = context.filesystem_state();
            let state = state.lock();
            assert_eq!(state.entry(fd).unwrap().cursor, 1);
        }

        // O_APPEND ignores that cursor for placement.
        assert_eq!(write(&mut context, fd, 0x2000, 2).await.unwrap(), 2);

        let mut actual = [0u8; 7];
        assert_eq!(
            context
                .system()
                .filesystem()
                .read("save/seek-append.dat", 0, 7, &mut actual)
                .await,
            Some(7)
        );
        assert_eq!(&actual, b"abcdeXY");

        let state = context.filesystem_state();
        let state = state.lock();
        assert_eq!(state.entry(fd).unwrap().cursor, 7);
    }

    #[futures_test::test]
    async fn lgt_fs_file_attribute_reports_regular_file_size() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/attr.dat", 0, b"abcdef")
            .await;
        context.write_bytes(0x1000, b"save/attr.dat\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 12]).unwrap();

        assert_eq!(
            file_attribute(&mut context, 0x1000, 0x2000, 1)
                .await
                .unwrap(),
            0
        );

        let mut actual = [0u8; 12];
        context.read_bytes(0x2000, &mut actual).unwrap();

        assert_eq!(u32::from_le_bytes(actual[0..4].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(actual[4..8].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(actual[8..12].try_into().unwrap()), 6);
    }

    #[futures_test::test]
    async fn lgt_fs_file_attribute_reports_directory_and_virtual_directory() {
        let mut context = filesystem_test_context();

        context
            .system()
            .filesystem()
            .write("save/child.dat", 0, b"x")
            .await;
        context
            .system()
            .filesystem()
            .add_virtual("archive/sub/data.bin", b"x".to_vec());

        context.write_bytes(0x1000, b"save\0").unwrap();
        context.write_bytes(0x1100, b"archive/sub\0").unwrap();

        for path in [0x1000, 0x1100] {
            context.write_bytes(0x2000, &[0xcc; 12]).unwrap();

            assert_eq!(
                file_attribute(&mut context, path, 0x2000, 1)
                    .await
                    .unwrap(),
                0
            );

            let mut actual = [0u8; 12];
            context.read_bytes(0x2000, &mut actual).unwrap();

            assert_eq!(u32::from_le_bytes(actual[0..4].try_into().unwrap()), 1);
            assert_eq!(u32::from_le_bytes(actual[4..8].try_into().unwrap()), 0);
            assert_eq!(u32::from_le_bytes(actual[8..12].try_into().unwrap()), 0);
        }
    }

    #[futures_test::test]
    async fn lgt_fs_file_attribute_reports_virtual_file_size() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .add_virtual("res/attr.bin", b"virtual".to_vec());

        context.write_bytes(0x1000, b"res/attr.bin\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 12]).unwrap();

        assert_eq!(
            file_attribute(&mut context, 0x1000, 0x2000, 100)
                .await
                .unwrap(),
            0
        );

        let mut actual = [0u8; 12];
        context.read_bytes(0x2000, &mut actual).unwrap();

        assert_eq!(u32::from_le_bytes(actual[0..4].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(actual[4..8].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(actual[8..12].try_into().unwrap()), 7);
    }

    #[futures_test::test]
    async fn lgt_fs_file_attribute_matches_native_validation_and_missing_errors() {
        let mut context = filesystem_test_context();

        context.write_bytes(0x1000, b"missing.dat\0").unwrap();
        context.write_bytes(0x1100, b"existing.dat\0").unwrap();
        context
            .system()
            .filesystem()
            .write("existing.dat", 0, b"x")
            .await;

        // Access validation occurs first.
        assert_eq!(
            file_attribute(&mut context, 0, 0, 0).await.unwrap(),
            -24
        );

        assert_eq!(
            file_attribute(&mut context, 0, 0x2000, 1).await.unwrap(),
            -3
        );

        // The output pointer is checked only after path processing.
        assert_eq!(
            file_attribute(&mut context, 0x1100, 0, 1)
                .await
                .unwrap(),
            -1
        );

        assert_eq!(
            file_attribute(&mut context, 0x1000, 0x2000, 1)
                .await
                .unwrap(),
            -1
        );

        for access in [1, 2, 3, 100] {
            assert_eq!(
                file_attribute(&mut context, 0x1100, 0x2000, access)
                    .await
                    .unwrap(),
                0
            );
        }
    }

    #[futures_test::test]
    async fn lgt_fs_file_attribute_root_is_directory() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"/\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 12]).unwrap();

        assert_eq!(
            file_attribute(&mut context, 0x1000, 0x2000, 1)
                .await
                .unwrap(),
            0
        );

        let mut actual = [0u8; 12];
        context.read_bytes(0x2000, &mut actual).unwrap();
        assert_eq!(u32::from_le_bytes(actual[0..4].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(actual[4..8].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(actual[8..12].try_into().unwrap()), 0);
    }

    #[futures_test::test]
    async fn lgt_fs_remove_deletes_regular_file() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/remove.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/remove.dat\0").unwrap();

        assert!(context.system().filesystem().exists("save/remove.dat").await);

        assert_eq!(remove(&mut context, 0x1000, 2).await.unwrap(), 0);

        assert!(!context.system().filesystem().exists("save/remove.dat").await);
        assert_eq!(context.system().filesystem().size("save/remove.dat").await, None);
    }

    #[futures_test::test]
    async fn lgt_fs_remove_missing_path_returns_minus_12_before_unlink() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"save/missing.dat\0").unwrap();

        assert_eq!(remove(&mut context, 0x1000, 2).await.unwrap(), -12);
    }

    #[futures_test::test]
    async fn lgt_fs_remove_rejects_directory_like_native_unlink() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/sub/child.dat", 0, b"x")
            .await;
        context.write_bytes(0x1000, b"save/sub\0").unwrap();

        assert!(context.system().filesystem().list("save/sub").await.is_some());
        assert_eq!(remove(&mut context, 0x1000, 2).await.unwrap(), -1);
        assert!(context.system().filesystem().exists("save/sub/child.dat").await);
    }

    #[futures_test::test]
    async fn lgt_fs_remove_virtual_file_is_visible_but_read_only() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .add_virtual("res/remove.bin", b"virtual".to_vec());
        context.write_bytes(0x1000, b"res/remove.bin\0").unwrap();

        assert_eq!(
            context.system().filesystem().size("res/remove.bin").await,
            Some(7)
        );
        assert_eq!(remove(&mut context, 0x1000, 2).await.unwrap(), -1);
        assert_eq!(
            context.system().filesystem().size("res/remove.bin").await,
            Some(7)
        );
    }

    #[futures_test::test]
    async fn lgt_fs_remove_matches_native_validation_order() {
        let mut context = filesystem_test_context();

        // Access validation precedes filename validation.
        assert_eq!(remove(&mut context, 0, 0).await.unwrap(), -24);
        assert_eq!(remove(&mut context, 0, 2).await.unwrap(), -3);

        context
            .system()
            .filesystem()
            .write("save/access.dat", 0, b"x")
            .await;
        context.write_bytes(0x1000, b"save/access.dat\0").unwrap();

        // Recognized selectors are accepted under WIE's compatibility
        // adaptation for the unmodelled per-DLET permission mask.
        for access in [1, 2, 3, 100] {
            context
                .system()
                .filesystem()
                .write("save/access.dat", 0, b"x")
                .await;

            assert_eq!(remove(&mut context, 0x1000, access).await.unwrap(), 0);
        }
    }

    #[futures_test::test]
    async fn lgt_fs_remove_open_descriptor_survives_but_future_io_sees_missing_file() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/open-remove.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/open-remove.dat\0").unwrap();
        context.write_bytes(0x2000, &[0; 1]).unwrap();

        let fd = open(&mut context, 0x1000, 8, 1).await.unwrap();
        assert_eq!(remove(&mut context, 0x1000, 2).await.unwrap(), 0);

        // WIE has no persistent host handle behind its synthetic descriptor,
        // so unlike POSIX an already-open descriptor cannot retain the
        // unlinked inode. Preserve the descriptor itself, while subsequent
        // backend IO reports the missing path.
        {
            let state = context.filesystem_state();
            let state = state.lock();
            assert!(state.entry(fd).is_some());
        }

        assert_eq!(read(&mut context, fd, 0x2000, 1).await.unwrap(), -1);
    }

    #[futures_test::test]
    async fn lgt_fs_mkdir_creates_empty_directory_visible_to_list_and_attribute() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"created\0").unwrap();

        assert_eq!(mkdir(&mut context, 0x1000, 2).await.unwrap(), 0);
        assert_eq!(
            context.system().filesystem().list("created").await,
            Some(Vec::new())
        );

        context.write_bytes(0x2000, &[0xaa; 12]).unwrap();
        assert_eq!(
            file_attribute(&mut context, 0x1000, 0x2000, 1)
                .await
                .unwrap(),
            0
        );

        let mut output = [0u8; 12];
        context.read_bytes(0x2000, &mut output).unwrap();
        assert_eq!(
            output,
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[futures_test::test]
    async fn lgt_fs_mkdir_is_non_recursive_and_allows_existing_parent() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"missing/child\0").unwrap();

        assert_eq!(mkdir(&mut context, 0x1000, 2).await.unwrap(), -12);

        context.write_bytes(0x1100, b"parent\0").unwrap();
        assert_eq!(mkdir(&mut context, 0x1100, 2).await.unwrap(), 0);

        context.write_bytes(0x1200, b"parent/child\0").unwrap();
        assert_eq!(mkdir(&mut context, 0x1200, 2).await.unwrap(), 0);
        assert!(context.system().filesystem().list("parent/child").await.is_some());
    }

    #[futures_test::test]
    async fn lgt_fs_mkdir_existing_file_directory_and_virtual_path_return_minus_5() {
        let mut context = filesystem_test_context();

        context.system().filesystem().write("file.dat", 0, b"x").await;
        context.write_bytes(0x1000, b"file.dat\0").unwrap();
        assert_eq!(mkdir(&mut context, 0x1000, 2).await.unwrap(), -5);

        context.write_bytes(0x1100, b"dir\0").unwrap();
        assert_eq!(mkdir(&mut context, 0x1100, 2).await.unwrap(), 0);
        assert_eq!(mkdir(&mut context, 0x1100, 2).await.unwrap(), -5);

        context
            .system()
            .filesystem()
            .add_virtual("virtual/item.bin", b"x".to_vec());
        context.write_bytes(0x1200, b"virtual\0").unwrap();
        assert_eq!(mkdir(&mut context, 0x1200, 2).await.unwrap(), -5);
    }

    #[futures_test::test]
    async fn lgt_fs_mkdir_matches_native_validation_order() {
        let mut context = filesystem_test_context();

        assert_eq!(mkdir(&mut context, 0, 0).await.unwrap(), -24);
        assert_eq!(mkdir(&mut context, 0, 2).await.unwrap(), -3);

        let long = vec![b'a'; 129];
        context.write_bytes(0x1000, &long).unwrap();
        context.write_bytes(0x1000 + 129, &[0]).unwrap();

        assert_eq!(mkdir(&mut context, 0x1000, 2).await.unwrap(), -11);
    }

    #[futures_test::test]
    async fn lgt_fs_rmdir_removes_empty_directory() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"empty\0").unwrap();

        assert_eq!(mkdir(&mut context, 0x1000, 2).await.unwrap(), 0);
        assert_eq!(rmdir(&mut context, 0x1000, 2).await.unwrap(), 0);
        assert_eq!(context.system().filesystem().list("empty").await, None);
    }

    #[futures_test::test]
    async fn lgt_fs_rmdir_nonempty_directory_returns_minus_15_and_is_non_recursive() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"parent\0").unwrap();
        context.write_bytes(0x1100, b"parent/child\0").unwrap();

        assert_eq!(mkdir(&mut context, 0x1000, 2).await.unwrap(), 0);
        assert_eq!(mkdir(&mut context, 0x1100, 2).await.unwrap(), 0);

        assert_eq!(rmdir(&mut context, 0x1000, 2).await.unwrap(), -15);
        assert!(context.system().filesystem().list("parent").await.is_some());

        assert_eq!(rmdir(&mut context, 0x1100, 2).await.unwrap(), 0);
        assert_eq!(rmdir(&mut context, 0x1000, 2).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_fs_rmdir_regular_file_is_generic_failure() {
        let mut context = filesystem_test_context();
        context.system().filesystem().write("file.dat", 0, b"x").await;
        context.write_bytes(0x1000, b"file.dat\0").unwrap();

        assert_eq!(rmdir(&mut context, 0x1000, 2).await.unwrap(), -1);
        assert_eq!(context.system().filesystem().size("file.dat").await, Some(1));
    }

    #[futures_test::test]
    async fn lgt_fs_rmdir_missing_and_virtual_directory_match_native_read_only_model() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"missing\0").unwrap();

        assert_eq!(rmdir(&mut context, 0x1000, 2).await.unwrap(), -12);

        context
            .system()
            .filesystem()
            .add_virtual("virtual/item.bin", b"x".to_vec());
        context.write_bytes(0x1100, b"virtual\0").unwrap();

        // Stat sees the virtual directory, but the packaged layer is read-only.
        assert_eq!(rmdir(&mut context, 0x1100, 2).await.unwrap(), -1);
        assert!(context.system().filesystem().list("virtual").await.is_some());
    }

    #[futures_test::test]
    async fn lgt_fs_rmdir_matches_native_validation_order() {
        let mut context = filesystem_test_context();

        assert_eq!(rmdir(&mut context, 0, 0).await.unwrap(), -24);
        assert_eq!(rmdir(&mut context, 0, 2).await.unwrap(), -3);

        let long = vec![b'a'; 129];
        context.write_bytes(0x1000, &long).unwrap();
        context.write_bytes(0x1000 + 129, &[0]).unwrap();

        assert_eq!(rmdir(&mut context, 0x1000, 2).await.unwrap(), -11);
    }

    #[futures_test::test]
    async fn lgt_fs_rename_moves_regular_file_and_preserves_data() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/old.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/old.dat\0").unwrap();
        context.write_bytes(0x1100, b"save/new.dat\0").unwrap();

        assert_eq!(rename(&mut context, 0x1000, 0x1100, 2).await.unwrap(), 0);
        assert_eq!(context.system().filesystem().size("save/old.dat").await, None);
        assert_eq!(context.system().filesystem().size("save/new.dat").await, Some(3));

        let mut actual = [0u8; 3];
        assert_eq!(
            context
                .system()
                .filesystem()
                .read("save/new.dat", 0, 3, &mut actual)
                .await,
            Some(3)
        );
        assert_eq!(&actual, b"abc");
    }

    #[futures_test::test]
    async fn lgt_fs_rename_overwrites_existing_regular_file_like_posix() {
        let mut context = filesystem_test_context();
        context.system().filesystem().write("old.dat", 0, b"source").await;
        context.system().filesystem().write("new.dat", 0, b"dest").await;
        context.write_bytes(0x1000, b"old.dat\0").unwrap();
        context.write_bytes(0x1100, b"new.dat\0").unwrap();

        assert_eq!(rename(&mut context, 0x1000, 0x1100, 2).await.unwrap(), 0);
        assert_eq!(context.system().filesystem().size("old.dat").await, None);
        assert_eq!(context.system().filesystem().size("new.dat").await, Some(6));

        let mut actual = [0u8; 6];
        assert_eq!(
            context
                .system()
                .filesystem()
                .read("new.dat", 0, 6, &mut actual)
                .await,
            Some(6)
        );
        assert_eq!(&actual, b"source");
    }

    #[futures_test::test]
    async fn lgt_fs_rename_moves_implicit_directory_subtree() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("old/a.dat", 0, b"a")
            .await;
        context
            .system()
            .filesystem()
            .write("old/sub/b.dat", 0, b"b")
            .await;
        context.write_bytes(0x1000, b"old\0").unwrap();
        context.write_bytes(0x1100, b"new\0").unwrap();

        assert_eq!(rename(&mut context, 0x1000, 0x1100, 2).await.unwrap(), 0);
        assert_eq!(context.system().filesystem().size("old/a.dat").await, None);
        assert_eq!(context.system().filesystem().size("new/a.dat").await, Some(1));
        assert_eq!(
            context.system().filesystem().size("new/sub/b.dat").await,
            Some(1)
        );
    }

    #[futures_test::test]
    async fn lgt_fs_rename_missing_virtual_and_same_path_errors() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"missing.dat\0").unwrap();
        context.write_bytes(0x1100, b"new.dat\0").unwrap();

        assert_eq!(rename(&mut context, 0x1000, 0x1100, 2).await.unwrap(), -12);

        context
            .system()
            .filesystem()
            .add_virtual("virtual.dat", b"x".to_vec());
        context.write_bytes(0x1200, b"virtual.dat\0").unwrap();

        assert_eq!(rename(&mut context, 0x1200, 0x1100, 2).await.unwrap(), -1);

        context.system().filesystem().write("same.dat", 0, b"x").await;
        context.write_bytes(0x1300, b"same.dat\0").unwrap();

        assert_eq!(rename(&mut context, 0x1300, 0x1300, 2).await.unwrap(), -1);
    }

    #[futures_test::test]
    async fn lgt_fs_rename_matches_native_validation_order() {
        let mut context = filesystem_test_context();

        // Access check happens before either filename is processed.
        assert_eq!(rename(&mut context, 0, 0, 0).await.unwrap(), -24);
        assert_eq!(rename(&mut context, 0, 0, 2).await.unwrap(), -3);

        context.write_bytes(0x1000, b"old.dat\0").unwrap();
        assert_eq!(rename(&mut context, 0x1000, 0, 2).await.unwrap(), -3);
    }

    #[futures_test::test]
    async fn lgt_fs_rename_updates_open_descriptor_path() {
        let mut context = filesystem_test_context();
        context.system().filesystem().write("old.dat", 0, b"abc").await;
        context.write_bytes(0x1000, b"old.dat\0").unwrap();
        context.write_bytes(0x1100, b"new.dat\0").unwrap();
        context.write_bytes(0x2000, &[0; 3]).unwrap();

        let fd = open(&mut context, 0x1000, 8, 1).await.unwrap();

        assert_eq!(rename(&mut context, 0x1000, 0x1100, 2).await.unwrap(), 0);
        assert_eq!(seek(&mut context, fd, 0, 0).await.unwrap(), 0);
        assert_eq!(read(&mut context, fd, 0x2000, 3).await.unwrap(), 3);

        let mut actual = [0u8; 3];
        context.read_bytes(0x2000, &mut actual).unwrap();
        assert_eq!(&actual, b"abc");
    }

    #[futures_test::test]
    async fn lgt_fs_get_counts_counts_direct_children_and_empty_directory() {
        let mut context = filesystem_test_context();

        context.system().filesystem().mkdir("save").await.unwrap();
        context.system().filesystem().mkdir("save/empty").await.unwrap();
        context.system().filesystem().mkdir("save/sub").await.unwrap();
        context.system().filesystem().write("save/a.dat", 0, b"a").await;
        context.system().filesystem().write("save/b.dat", 0, b"b").await;
        context.system().filesystem().write("save/sub/nested.dat", 0, b"n").await;

        context.write_bytes(0x1000, b"save\0").unwrap();
        context.write_bytes(0x1100, b"save/empty\0").unwrap();
        context.write_bytes(0x1200, b"save/sub\0").unwrap();

        // Direct children only: a.dat, b.dat, empty, sub.
        assert_eq!(get_counts(&mut context, 0x1000, 1).await.unwrap(), 4);
        assert_eq!(get_counts(&mut context, 0x1100, 1).await.unwrap(), 0);
        assert_eq!(get_counts(&mut context, 0x1200, 1).await.unwrap(), 1);
    }

    #[futures_test::test]
    async fn lgt_fs_get_counts_matches_native_validation_and_access_100_path_rule() {
        let mut context = filesystem_test_context();

        context.write_bytes(0x1000, b"missing\0").unwrap();

        // Access validation precedes all path handling.
        assert_eq!(get_counts(&mut context, 0, 0).await.unwrap(), -24);
        assert_eq!(get_counts(&mut context, 0, 1).await.unwrap(), -3);
        assert_eq!(get_counts(&mut context, 0x1000, 1).await.unwrap(), -1);

        // Selectors 1/2/3 pass through WPFS_MakeFullPathName and enforce its
        // 128-byte input limit. Selector 100 bypasses that helper.
        let long_dir = "a".repeat(129);
        let long_file = alloc::format!("{long_dir}/child.dat");
        context.system().filesystem().write(&long_file, 0, b"x").await;

        let mut long_path = long_dir.as_bytes().to_vec();
        long_path.push(0);
        context.write_bytes(0x2000, &long_path).unwrap();

        assert_eq!(get_counts(&mut context, 0x2000, 1).await.unwrap(), -11);
        assert_eq!(get_counts(&mut context, 0x2000, 2).await.unwrap(), -11);
        assert_eq!(get_counts(&mut context, 0x2000, 3).await.unwrap(), -11);
        assert_eq!(get_counts(&mut context, 0x2000, 100).await.unwrap(), 1);
    }

    #[futures_test::test]
    async fn lgt_fs_set_mode_matches_native_validation_order_and_modes() {
        let mut context = filesystem_test_context();
        context
            .system()
            .filesystem()
            .write("save/mode.dat", 0, b"abc")
            .await;
        context.write_bytes(0x1000, b"save/mode.dat\0").unwrap();

        // Access validation precedes path construction and mode validation.
        assert_eq!(set_mode(&mut context, 0, 0x77, 0).await.unwrap(), -24);
        assert_eq!(set_mode(&mut context, 0, 0x77, 1).await.unwrap(), -3);

        // Public mode validation occurs after successful path construction.
        assert_eq!(set_mode(&mut context, 0x1000, 0x77, 1).await.unwrap(), -9);

        assert_eq!(set_mode(&mut context, 0x1000, 0x10, 1).await.unwrap(), 0);
        assert_eq!(set_mode(&mut context, 0x1000, 0x20, 1).await.unwrap(), 0);
        assert_eq!(set_mode(&mut context, 0x1000, 0x30, 1).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_fs_set_mode_missing_and_directory_return_minus_12() {
        let mut context = filesystem_test_context();
        context.write_bytes(0x1000, b"save/missing.dat\0").unwrap();
        context.write_bytes(0x1100, b"save/dir\0").unwrap();

        assert_eq!(set_mode(&mut context, 0x1000, 0x10, 1).await.unwrap(), -12);

        context.system().filesystem().mkdir("save").await.unwrap();
        context.system().filesystem().mkdir("save/dir").await.unwrap();
        assert_eq!(set_mode(&mut context, 0x1100, 0x10, 1).await.unwrap(), -12);
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

    #[futures_test::test]
    async fn lgt_fs_list_null_output_and_missing_directory_match_native_errors() {
        let mut context = filesystem_test_context();

        context.write_bytes(0x1000, b"/\0").unwrap();
        assert_eq!(list(&mut context, 0x1000, 0, 16, 1).await.unwrap(), -3);

        context.write_bytes(0x1100, b"missing\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 16]).unwrap();
        assert_eq!(list(&mut context, 0x1100, 0x2000, 16, 1).await.unwrap(), -1);

        let mut untouched = [0u8; 16];
        context.read_bytes(0x2000, &mut untouched).unwrap();
        assert_eq!(untouched, [0xcc; 16]);
    }

    #[futures_test::test]
    async fn lgt_fs_list_requires_one_extra_byte_for_final_empty_string() {
        let mut context = filesystem_test_context();

        context.system().filesystem().write("save/first", 0, &[1]).await;
        context.write_bytes(0x1000, b"save\0").unwrap();
        context.write_bytes(0x2000, &[0xcc; 16]).unwrap();

        // "first\0" is six bytes. Native MH_fileList requires a seventh byte
        // for the final empty-string terminator, so capacity 6 fails before
        // copying the entry.
        assert_eq!(list(&mut context, 0x1000, 0x2000, 6, 1).await.unwrap(), -18);

        let mut failed = [0u8; 7];
        context.read_bytes(0x2000, &mut failed).unwrap();
        assert_eq!(failed, [0xcc; 7]);

        assert_eq!(list(&mut context, 0x1000, 0x2000, 7, 1).await.unwrap(), 0);

        let mut exact = [0u8; 7];
        context.read_bytes(0x2000, &mut exact).unwrap();
        assert_eq!(&exact, b"first\0\0");
    }
}

