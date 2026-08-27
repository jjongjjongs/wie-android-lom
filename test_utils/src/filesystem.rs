use alloc::{
    format,
    collections::BTreeSet,
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::cmp::min;

use hashbrown::HashMap;
use spin::Mutex;

use wie_backend::{
    Filesystem, FilesystemMkdirError, FilesystemRenameError, FilesystemRmDirError,
    FilesystemSetModeError,
};

/// In-memory `Filesystem` implementation for tests.
#[derive(Default)]
pub struct MemoryFilesystem {
    files: Mutex<HashMap<(String, String), Vec<u8>>>,
    directories: Mutex<BTreeSet<(String, String)>>,
}

impl MemoryFilesystem {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl Filesystem for MemoryFilesystem {
    async fn exists(&self, aid: &str, path: &str) -> bool {
        self.files.lock().contains_key(&(aid.to_string(), path.to_string()))
    }

    async fn size(&self, aid: &str, path: &str) -> Option<usize> {
        self.files.lock().get(&(aid.to_string(), path.to_string())).map(|v| v.len())
    }

    async fn read(&self, aid: &str, path: &str, offset: usize, count: usize, buf: &mut [u8]) -> Option<usize> {
        let files = self.files.lock();
        let data = files.get(&(aid.to_string(), path.to_string()))?;

        if offset >= data.len() {
            return Some(0);
        }

        let size_to_read = min(count, data.len() - offset);
        buf[..size_to_read].copy_from_slice(&data[offset..offset + size_to_read]);
        Some(size_to_read)
    }

    async fn write(&self, aid: &str, path: &str, offset: usize, data: &[u8]) -> usize {
        let mut files = self.files.lock();
        let file = files.entry((aid.to_string(), path.to_string())).or_default();
        if file.len() < offset + data.len() {
            file.resize(offset + data.len(), 0);
        }
        file[offset..offset + data.len()].copy_from_slice(data);

        data.len()
    }

    async fn truncate(&self, aid: &str, path: &str, len: usize) {
        let mut files = self.files.lock();
        let file = files.entry((aid.to_string(), path.to_string())).or_default();
        file.resize(len, 0);
    }

    async fn remove(&self, aid: &str, path: &str) -> bool {
        self.files.lock().remove(&(aid.to_string(), path.to_string())).is_some()
    }

    async fn mkdir(
        &self,
        aid: &str,
        path: &str,
    ) -> core::result::Result<(), FilesystemMkdirError> {
        if path.is_empty() {
            return Err(FilesystemMkdirError::AlreadyExists);
        }

        let aid_owned = aid.to_string();
        let path_owned = path.to_string();

        {
            let files = self.files.lock();
            let directories = self.directories.lock();

            if files.contains_key(&(aid_owned.clone(), path_owned.clone()))
                || directories.contains(&(aid_owned.clone(), path_owned.clone()))
            {
                return Err(FilesystemMkdirError::AlreadyExists);
            }

            let parent = path.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
            if !parent.is_empty() {
                if files.contains_key(&(aid_owned.clone(), parent.to_string())) {
                    return Err(FilesystemMkdirError::Other);
                }

                let mut parent_prefix = parent.to_string();
                parent_prefix.push('/');

                let parent_exists =
                    directories.contains(&(aid_owned.clone(), parent.to_string()))
                    || directories.iter().any(|(entry_aid, entry_path)| {
                        entry_aid == aid && entry_path.starts_with(&parent_prefix)
                    })
                    || files.keys().any(|(entry_aid, entry_path)| {
                        entry_aid == aid && entry_path.starts_with(&parent_prefix)
                    });

                if !parent_exists {
                    return Err(FilesystemMkdirError::NotFound);
                }
            }
        }

        self.directories.lock().insert((aid_owned, path_owned));
        Ok(())
    }

    async fn rmdir(
        &self,
        aid: &str,
        path: &str,
    ) -> core::result::Result<(), FilesystemRmDirError> {
        if path.is_empty() {
            return Err(FilesystemRmDirError::Other);
        }

        let aid_owned = aid.to_string();
        let path_owned = path.to_string();
        let key = (aid_owned.clone(), path_owned.clone());

        if self.files.lock().contains_key(&key) {
            return Err(FilesystemRmDirError::Other);
        }

        let mut prefix = path_owned.clone();
        prefix.push('/');

        let has_file_child = self.files.lock().keys().any(|(entry_aid, entry_path)| {
            entry_aid == aid && entry_path.starts_with(&prefix)
        });

        let mut directories = self.directories.lock();
        let exists = directories.contains(&key);
        let has_directory_child = directories.iter().any(|(entry_aid, entry_path)| {
            entry_aid == aid && entry_path.starts_with(&prefix)
        });

        if has_file_child || has_directory_child {
            return Err(FilesystemRmDirError::NotEmpty);
        }

        if !exists {
            return Err(FilesystemRmDirError::NotFound);
        }

        directories.remove(&key);
        Ok(())
    }

    async fn rename(
        &self,
        aid: &str,
        from: &str,
        to: &str,
    ) -> core::result::Result<(), FilesystemRenameError> {
        if from == to {
            return Ok(());
        }

        let mut files = self.files.lock();
        let aid_owned = aid.to_string();
        let from_owned = from.to_string();
        let to_owned = to.to_string();

        if let Some(data) = files.remove(&(aid_owned.clone(), from_owned.clone())) {
            let mut to_prefix = to_owned.clone();
            to_prefix.push('/');

            if files.keys().any(|(entry_aid, entry_path)| {
                entry_aid == aid && entry_path.starts_with(&to_prefix)
            }) {
                files.insert((aid_owned, from_owned), data);
                return Err(FilesystemRenameError::Other);
            }

            // POSIX rename replaces an existing regular file destination.
            files.insert((aid.to_string(), to_owned), data);
            return Ok(());
        }

        let mut from_prefix = from_owned.clone();
        from_prefix.push('/');

        let subtree = files
            .keys()
            .filter(|(entry_aid, entry_path)| {
                entry_aid == aid && entry_path.starts_with(&from_prefix)
            })
            .cloned()
            .collect::<Vec<_>>();

        if subtree.is_empty() {
            return Err(FilesystemRenameError::NotFound);
        }

        if files.contains_key(&(aid.to_string(), to_owned.clone())) {
            return Err(FilesystemRenameError::Other);
        }

        let mut to_prefix = to_owned.clone();
        to_prefix.push('/');
        if files.keys().any(|(entry_aid, entry_path)| {
            entry_aid == aid && entry_path.starts_with(&to_prefix)
        }) {
            return Err(FilesystemRenameError::CrossDeviceOrNotEmpty);
        }

        let mut moved = Vec::new();
        for key in subtree {
            let suffix = key
                .1
                .strip_prefix(&from_prefix)
                .unwrap()
                .to_string();
            let data = files.remove(&key).unwrap();
            moved.push(((aid.to_string(), format!("{to_prefix}{suffix}")), data));
        }

        for (key, data) in moved {
            files.insert(key, data);
        }

        Ok(())
    }

    async fn set_mode(
        &self,
        aid: &str,
        path: &str,
        _mode: u32,
    ) -> core::result::Result<(), FilesystemSetModeError> {
        if self.files.lock().contains_key(&(aid.to_string(), path.to_string())) {
            Ok(())
        } else {
            Err(FilesystemSetModeError::NotFound)
        }
    }

    async fn total_space(&self, _aid: &str) -> Option<u64> {
        Some(32 * 1024 * 1024)
    }

    async fn available_space(&self, _aid: &str) -> Option<u64> {
        Some(16 * 1024 * 1024)
    }

    async fn list(&self, aid: &str, path: &str) -> Option<Vec<String>> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let files = self.files.lock();
        let directories = self.directories.lock();
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();

        let mut directory_exists =
            path.is_empty() || directories.contains(&(aid.to_string(), path.to_string()));

        for ((entry_aid, entry_path), _) in files.iter() {
            if entry_aid != aid {
                continue;
            }
            let Some(rest) = entry_path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }

            directory_exists = true;

            let child = rest.split('/').next().unwrap_or(rest).to_string();
            if seen.insert(child.clone()) {
                entries.push(child);
            }
        }

        for (entry_aid, entry_path) in directories.iter() {
            if entry_aid != aid {
                continue;
            }

            let Some(rest) = entry_path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }

            directory_exists = true;

            let child = rest.split('/').next().unwrap_or(rest).to_string();
            if seen.insert(child.clone()) {
                entries.push(child);
            }
        }

        if directory_exists { Some(entries) } else { None }
    }
}
