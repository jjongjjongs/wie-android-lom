use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

use directories::ProjectDirs;

use wie_backend::{Filesystem, FilesystemMkdirError, FilesystemRenameError, FilesystemRmDirError, FilesystemSetModeError};

/// `errno` values std does not expose as an `ErrorKind`, matched via
/// `raw_os_error` so the trait's dedicated error variants stay distinct from
/// the catch-all. Both are POSIX-stable on the hosts `wie_cli` targets.
const ENAMETOOLONG: i32 = 36;
const EXDEV: i32 = 18;

/// Persistent filesystem backed by `std::fs` under `<base>/<aid>/fs/<path>`.
/// Any I/O error or rejected path returns the trait's failure value.
pub struct CliFilesystem {
    base_path: PathBuf,
}

impl CliFilesystem {
    pub fn new() -> Self {
        let base_dir = ProjectDirs::from("net", "dlunch", "wie").unwrap();
        Self {
            base_path: base_dir.data_dir().to_owned(),
        }
    }

    fn path_for(&self, aid: &str, path: &str) -> Option<PathBuf> {
        let sanitized_aid: String = aid.chars().filter(|c| !matches!(c, '/' | '\\' | '\0')).collect();
        if sanitized_aid.is_empty() || sanitized_aid == "." || sanitized_aid == ".." {
            tracing::error!(aid, path, "rejected: invalid aid");
            return None;
        }

        let mut normalized = PathBuf::new();
        for component in Path::new(path).components() {
            match component {
                Component::Normal(c) => normalized.push(c),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    tracing::error!(aid, path, "path traversal attempt rejected");
                    return None;
                }
            }
        }

        if normalized.as_os_str().is_empty() {
            tracing::error!(aid, path, "rejected: empty normalized path");
            return None;
        }

        Some(self.base_path.join(&sanitized_aid).join("fs").join(normalized))
    }
}

impl Default for CliFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Filesystem for CliFilesystem {
    async fn exists(&self, aid: &str, path: &str) -> bool {
        let Some(disk_path) = self.path_for(aid, path) else {
            return false;
        };

        match disk_path.metadata() {
            Ok(md) => md.is_file(),
            Err(_) => false,
        }
    }

    async fn size(&self, aid: &str, path: &str) -> Option<usize> {
        let disk_path = self.path_for(aid, path)?;
        let md = disk_path.metadata().ok()?;
        if !md.is_file() {
            return None;
        }
        Some(md.len() as usize)
    }

    async fn read(&self, aid: &str, path: &str, offset: usize, count: usize, buf: &mut [u8]) -> Option<usize> {
        let disk_path = self.path_for(aid, path)?;

        let mut file = match OpenOptions::new().read(true).open(&disk_path) {
            Ok(f) => f,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    return None;
                }
                tracing::warn!(aid, path, error = %err, "read: open failed");
                return None;
            }
        };

        let size = file.metadata().map(|m| m.len() as usize).unwrap_or(0);
        if offset >= size {
            return Some(0);
        }

        if let Err(err) = file.seek(SeekFrom::Start(offset as u64)) {
            tracing::warn!(aid, path, error = %err, "read: seek failed");
            return Some(0);
        }

        let to_read = core::cmp::min(count, size - offset);
        let slice = &mut buf[..to_read];
        // read_exact so short reads surface to the caller only at EOF, not
        // on signal interruption.
        match file.read_exact(slice) {
            Ok(()) => Some(to_read),
            Err(err) => {
                tracing::warn!(aid, path, error = %err, "read: IO error");
                Some(0)
            }
        }
    }

    async fn write(&self, aid: &str, path: &str, offset: usize, data: &[u8]) -> usize {
        let Some(disk_path) = self.path_for(aid, path) else {
            return 0;
        };

        if let Some(parent) = disk_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            tracing::warn!(aid, path, error = %err, "write: create parent dir failed");
            return 0;
        }

        let mut file = match OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&disk_path) {
            Ok(f) => f,
            Err(err) => {
                tracing::warn!(aid, path, error = %err, "write: open failed");
                return 0;
            }
        };

        let current_size = file.metadata().map(|m| m.len() as usize).unwrap_or(0);

        if offset > current_size {
            // OS sparse extend avoids host allocation for the gap; POSIX
            // ftruncate and Windows SetEndOfFile both zero-fill the
            // newly-created region.
            if let Err(err) = file.set_len(offset as u64) {
                tracing::warn!(aid, path, error = %err, "write: set_len extend failed");
                return 0;
            }
        }

        if let Err(err) = file.seek(SeekFrom::Start(offset as u64)) {
            tracing::warn!(aid, path, error = %err, "write: seek failed");
            return 0;
        }

        match file.write_all(data) {
            Ok(()) => data.len(),
            Err(err) => {
                tracing::warn!(aid, path, error = %err, "write: write_all failed");
                0
            }
        }
    }

    async fn truncate(&self, aid: &str, path: &str, len: usize) {
        let Some(disk_path) = self.path_for(aid, path) else {
            return;
        };

        if let Some(parent) = disk_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            tracing::warn!(aid, path, error = %err, "truncate: create parent dir failed");
            return;
        }

        let file = match OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&disk_path) {
            Ok(f) => f,
            Err(err) => {
                tracing::warn!(aid, path, error = %err, "truncate: open failed");
                return;
            }
        };

        if let Err(err) = file.set_len(len as u64) {
            tracing::warn!(aid, path, error = %err, "truncate: set_len failed");
        }
    }

    async fn remove(&self, aid: &str, path: &str) -> bool {
        let Some(disk_path) = self.path_for(aid, path) else {
            return false;
        };

        fs::remove_file(disk_path).is_ok()
    }

    async fn mkdir(&self, aid: &str, path: &str) -> core::result::Result<(), FilesystemMkdirError> {
        let Some(disk_path) = self.path_for(aid, path) else {
            return Err(FilesystemMkdirError::Other);
        };

        // Exactly one directory: the trait forbids creating missing parents.
        match fs::create_dir(&disk_path) {
            Ok(()) => Ok(()),
            Err(err) => Err(match err.kind() {
                std::io::ErrorKind::AlreadyExists => FilesystemMkdirError::AlreadyExists,
                std::io::ErrorKind::NotFound => FilesystemMkdirError::NotFound,
                _ if err.raw_os_error() == Some(ENAMETOOLONG) => FilesystemMkdirError::NameTooLong,
                _ => {
                    tracing::warn!(aid, path, error = %err, "mkdir failed");
                    FilesystemMkdirError::Other
                }
            }),
        }
    }

    async fn rmdir(&self, aid: &str, path: &str) -> core::result::Result<(), FilesystemRmDirError> {
        let Some(disk_path) = self.path_for(aid, path) else {
            return Err(FilesystemRmDirError::Other);
        };

        // Non-recursive: `remove_dir` fails on a non-empty directory.
        match fs::remove_dir(&disk_path) {
            Ok(()) => Ok(()),
            Err(err) => Err(match err.kind() {
                std::io::ErrorKind::NotFound => FilesystemRmDirError::NotFound,
                std::io::ErrorKind::DirectoryNotEmpty => FilesystemRmDirError::NotEmpty,
                _ if err.raw_os_error() == Some(ENAMETOOLONG) => FilesystemRmDirError::NameTooLong,
                _ => {
                    tracing::warn!(aid, path, error = %err, "rmdir failed");
                    FilesystemRmDirError::Other
                }
            }),
        }
    }

    async fn rename(&self, aid: &str, from: &str, to: &str) -> core::result::Result<(), FilesystemRenameError> {
        let (Some(from_path), Some(to_path)) = (self.path_for(aid, from), self.path_for(aid, to)) else {
            return Err(FilesystemRenameError::Other);
        };

        // The host rename replaces an existing file and removes the source; it
        // does not create destination parents.
        match fs::rename(&from_path, &to_path) {
            Ok(()) => Ok(()),
            Err(err) => Err(match err.kind() {
                std::io::ErrorKind::NotFound => FilesystemRenameError::NotFound,
                std::io::ErrorKind::DirectoryNotEmpty => FilesystemRenameError::CrossDeviceOrNotEmpty,
                _ if err.raw_os_error() == Some(EXDEV) => FilesystemRenameError::CrossDeviceOrNotEmpty,
                _ if err.raw_os_error() == Some(ENAMETOOLONG) => FilesystemRenameError::NameTooLong,
                _ => {
                    tracing::warn!(aid, from, to, error = %err, "rename failed");
                    FilesystemRenameError::Other
                }
            }),
        }
    }

    async fn set_mode(&self, aid: &str, path: &str, mode: u32) -> core::result::Result<(), FilesystemSetModeError> {
        let Some(disk_path) = self.path_for(aid, path) else {
            return Err(FilesystemSetModeError::Other);
        };

        let metadata = match disk_path.metadata() {
            Ok(md) if md.is_file() => md,
            Ok(_) => return Err(FilesystemSetModeError::NotFound),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(FilesystemSetModeError::NotFound),
            Err(err) if err.raw_os_error() == Some(ENAMETOOLONG) => return Err(FilesystemSetModeError::NameTooLong),
            Err(err) => {
                tracing::warn!(aid, path, error = %err, "set_mode: stat failed");
                return Err(FilesystemSetModeError::Other);
            }
        };

        // Only the owner read/write bits are meaningful to the WIPI-C caller.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = metadata.permissions();
            permissions.set_mode(mode & 0o600);
            if let Err(err) = fs::set_permissions(&disk_path, permissions) {
                tracing::warn!(aid, path, error = %err, "set_mode: chmod failed");
                return Err(FilesystemSetModeError::Other);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (metadata, mode);
        }
        Ok(())
    }

    async fn total_space(&self, _aid: &str) -> Option<u64> {
        // The host disk dwarfs a feature-phone quota; report the same fixed
        // capacity the in-memory backend does so titles see a sane figure.
        Some(32 * 1024 * 1024)
    }

    async fn available_space(&self, _aid: &str) -> Option<u64> {
        Some(16 * 1024 * 1024)
    }

    async fn list(&self, aid: &str, path: &str) -> Option<Vec<String>> {
        let disk_path = if path.is_empty() {
            self.path_for(aid, ".")?
        } else {
            self.path_for(aid, path)?
        };

        let entries = match fs::read_dir(disk_path) {
            Ok(entries) => entries,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(aid, path, error = %error, "list: read_dir failed");
                }
                return None;
            }
        };

        Some(
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect(),
        )
    }
}
