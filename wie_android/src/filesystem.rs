use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

use wie_backend::Filesystem;

/// Persistent filesystem rooted at the app-private directory Java passes to
/// `nativeStart`, laid out as `<base>/<aid>/<path>`.
///
/// Guest paths are attacker-controlled as far as this process is concerned, so
/// traversal out of the per-app directory is rejected rather than clamped.
pub struct AndroidFilesystem {
    base_path: PathBuf,
}

impl AndroidFilesystem {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
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

        Some(self.base_path.join(&sanitized_aid).join(normalized))
    }
}

#[async_trait::async_trait]
impl Filesystem for AndroidFilesystem {
    async fn exists(&self, aid: &str, path: &str) -> bool {
        self.path_for(aid, path).and_then(|x| x.metadata().ok()).is_some_and(|x| x.is_file())
    }

    async fn size(&self, aid: &str, path: &str) -> Option<usize> {
        let metadata = self.path_for(aid, path)?.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }

        Some(metadata.len() as usize)
    }

    async fn read(&self, aid: &str, path: &str, offset: usize, count: usize, buf: &mut [u8]) -> Option<usize> {
        let disk_path = self.path_for(aid, path)?;

        let mut file = match OpenOptions::new().read(true).open(&disk_path) {
            Ok(file) => file,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(aid, path, error = %error, "read: open failed");
                }
                return None;
            }
        };

        let size = file.metadata().map(|x| x.len() as usize).unwrap_or(0);
        if offset >= size {
            return Some(0);
        }

        if let Err(error) = file.seek(SeekFrom::Start(offset as u64)) {
            tracing::warn!(aid, path, error = %error, "read: seek failed");
            return Some(0);
        }

        let to_read = core::cmp::min(count, size - offset);
        match file.read_exact(&mut buf[..to_read]) {
            Ok(()) => Some(to_read),
            Err(error) => {
                tracing::warn!(aid, path, error = %error, "read: IO error");
                Some(0)
            }
        }
    }

    async fn write(&self, aid: &str, path: &str, offset: usize, data: &[u8]) -> usize {
        let Some(disk_path) = self.path_for(aid, path) else {
            return 0;
        };

        if let Some(parent) = disk_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            tracing::warn!(aid, path, error = %error, "write: create parent dir failed");
            return 0;
        }

        let mut file = match OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&disk_path) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(aid, path, error = %error, "write: open failed");
                return 0;
            }
        };

        let current_size = file.metadata().map(|x| x.len() as usize).unwrap_or(0);
        if offset > current_size
            && let Err(error) = file.set_len(offset as u64)
        {
            tracing::warn!(aid, path, error = %error, "write: set_len extend failed");
            return 0;
        }

        if let Err(error) = file.seek(SeekFrom::Start(offset as u64)) {
            tracing::warn!(aid, path, error = %error, "write: seek failed");
            return 0;
        }

        match file.write_all(data) {
            Ok(()) => data.len(),
            Err(error) => {
                tracing::warn!(aid, path, error = %error, "write: write_all failed");
                0
            }
        }
    }

    async fn truncate(&self, aid: &str, path: &str, len: usize) {
        let Some(disk_path) = self.path_for(aid, path) else {
            return;
        };

        if let Some(parent) = disk_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            tracing::warn!(aid, path, error = %error, "truncate: create parent dir failed");
            return;
        }

        let file = match OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&disk_path) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(aid, path, error = %error, "truncate: open failed");
                return;
            }
        };

        if let Err(error) = file.set_len(len as u64) {
            tracing::warn!(aid, path, error = %error, "truncate: set_len failed");
        }
    }

    async fn remove(&self, aid: &str, path: &str) -> bool {
        let Some(disk_path) = self.path_for(aid, path) else {
            return false;
        };

        fs::remove_file(disk_path).is_ok()
    }

    async fn list(&self, aid: &str, path: &str) -> Option<Vec<String>> {
        let sanitized_aid: String = aid.chars().filter(|c| !matches!(c, '/' | '\\' | '\0')).collect();
        if sanitized_aid.is_empty() || sanitized_aid == "." || sanitized_aid == ".." {
            tracing::error!(aid, path, "list: invalid aid");
            return None;
        }

        let disk_path = if path.is_empty() {
            self.base_path.join(sanitized_aid)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::AndroidFilesystem;

    #[test]
    fn path_stays_inside_app_directory() {
        let filesystem = AndroidFilesystem::new(PathBuf::from("/data/fs"));

        assert_eq!(filesystem.path_for("app", "save/1.dat"), Some(PathBuf::from("/data/fs/app/save/1.dat")));
        assert_eq!(filesystem.path_for("app", "../../etc/passwd"), None);
        assert_eq!(filesystem.path_for("app", "/etc/passwd"), None);
        assert_eq!(filesystem.path_for("..", "save.dat"), None);
    }
}
