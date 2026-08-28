use alloc::{borrow::ToOwned, boxed::Box, collections::BTreeSet, string::String, sync::Arc, vec::Vec};
use core::cmp::min;

use hashbrown::HashMap;
use spin::Mutex;

use crate::platform::Platform;

/// Normalize a guest-supplied path so both overlay layers see the same key.
///
/// - Leading `/` are stripped (archive paths often carry them).
/// - `.` segments are dropped.
/// - `..` segments, trailing `/`, backslashes, and empty results all
///   return `None`.
fn normalize_guest_path(path: &str) -> Option<String> {
    if path.contains('\\') {
        return None;
    }

    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.ends_with('/') {
        return None;
    }

    let mut out = String::new();
    for seg in trimmed.split('/') {
        match seg {
            "" => continue,
            "." => continue,
            ".." => return None,
            normal => {
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(normal);
            }
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

/// Normalize a guest directory path.
///
/// Unlike file paths, the root directory is valid and is represented as the
/// empty string. A trailing slash is accepted because the LGT filesystem layer
/// strips it before opening the directory.
fn normalize_guest_directory(path: &str) -> Option<String> {
    if path.contains('\\') {
        return None;
    }

    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }

    let mut out = String::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return None,
            normal => {
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(normal);
            }
        }
    }

    Some(out)
}

/// Unified filesystem view exposed by `System::filesystem()`.
///
/// Wraps the persistent `Platform::filesystem()` backend and an in-memory
/// virtual layer holding archive resources. Writes always hit the platform
/// backend; reads prefer the platform backend and fall back to the virtual
/// layer. Paths are normalized internally so callers pass raw guest paths.
#[derive(Clone)]
pub struct FilesystemOverlay {
    platform: Arc<Box<dyn Platform>>,
    virtual_files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    aid: Arc<str>,
}

impl FilesystemOverlay {
    pub fn new(platform: Arc<Box<dyn Platform>>, aid: &str) -> Self {
        Self {
            platform,
            virtual_files: Arc::new(Mutex::new(HashMap::new())),
            aid: Arc::from(aid),
        }
    }

    pub fn add_virtual(&self, path: &str, data: Vec<u8>) {
        let key = normalize_guest_path(path).unwrap_or_else(|| path.trim_start_matches('/').to_owned());
        self.virtual_files.lock().insert(key, data);
    }

    pub async fn exists(&self, path: &str) -> bool {
        let Some(normalized) = normalize_guest_path(path) else {
            return false;
        };

        if self.platform.filesystem().exists(&self.aid, &normalized).await {
            return true;
        }
        self.virtual_files.lock().contains_key(&normalized)
    }

    pub async fn size(&self, path: &str) -> Option<usize> {
        let normalized = normalize_guest_path(path)?;

        if let Some(size) = self.platform.filesystem().size(&self.aid, &normalized).await {
            return Some(size);
        }
        self.virtual_files.lock().get(&normalized).map(|d| d.len())
    }

    pub async fn read(&self, path: &str, offset: usize, count: usize, buf: &mut [u8]) -> Option<usize> {
        let normalized = normalize_guest_path(path)?;

        let plat_fs = self.platform.filesystem();
        if plat_fs.exists(&self.aid, &normalized).await {
            return plat_fs.read(&self.aid, &normalized, offset, count, buf).await;
        }

        let files = self.virtual_files.lock();
        let data = files.get(&normalized)?;
        if offset >= data.len() {
            return Some(0);
        }
        let n = min(count, data.len() - offset);
        buf[..n].copy_from_slice(&data[offset..offset + n]);
        Some(n)
    }

    pub async fn write(&self, path: &str, offset: usize, data: &[u8]) -> usize {
        let Some(normalized) = normalize_guest_path(path) else {
            return 0;
        };
        self.platform.filesystem().write(&self.aid, &normalized, offset, data).await
    }

    pub async fn truncate(&self, path: &str, len: usize) {
        let Some(normalized) = normalize_guest_path(path) else {
            return;
        };
        self.platform.filesystem().truncate(&self.aid, &normalized, len).await;
    }

    pub async fn remove(&self, path: &str) -> bool {
        let Some(normalized) = normalize_guest_path(path) else {
            return false;
        };

        self.platform.filesystem().remove(&self.aid, &normalized).await
    }

    pub async fn mkdir(&self, path: &str) -> core::result::Result<(), crate::platform::FilesystemMkdirError> {
        use crate::platform::FilesystemMkdirError;

        let Some(normalized) = normalize_guest_path(path) else {
            return Err(FilesystemMkdirError::Other);
        };

        // Packaged virtual objects are visible filesystem entries but are
        // read-only. Creating a directory over one therefore behaves as an
        // existing-path collision.
        if self.size(&normalized).await.is_some() || self.list(&normalized).await.is_some() {
            return Err(FilesystemMkdirError::AlreadyExists);
        }

        self.platform.filesystem().mkdir(&self.aid, &normalized).await
    }

    pub async fn rmdir(&self, path: &str) -> core::result::Result<(), crate::platform::FilesystemRmDirError> {
        use crate::platform::FilesystemRmDirError;

        let Some(normalized) = normalize_guest_path(path) else {
            return Err(FilesystemRmDirError::Other);
        };

        // A directory visible only through the packaged archive is read-only.
        // Do not turn that into a persistent-layer ENOENT.
        if self.platform.filesystem().list(&self.aid, &normalized).await.is_none() && self.list(&normalized).await.is_some() {
            return Err(FilesystemRmDirError::Other);
        }

        self.platform.filesystem().rmdir(&self.aid, &normalized).await
    }

    pub async fn rename(&self, from: &str, to: &str) -> core::result::Result<(), crate::platform::FilesystemRenameError> {
        use crate::platform::FilesystemRenameError;

        let Some(from) = normalize_guest_path(from) else {
            return Err(FilesystemRenameError::Other);
        };
        let Some(to) = normalize_guest_path(to) else {
            return Err(FilesystemRenameError::Other);
        };

        // Platform objects shadow virtual files. A source that exists only in
        // the archive is read-only and cannot be renamed.
        if !self.platform.filesystem().exists(&self.aid, &from).await && self.platform.filesystem().list(&self.aid, &from).await.is_none() {
            if self.virtual_files.lock().contains_key(&from) || {
                let mut prefix = from.clone();
                prefix.push('/');
                self.virtual_files.lock().keys().any(|key| key.starts_with(&prefix))
            } {
                return Err(FilesystemRenameError::Other);
            }
        }

        self.platform.filesystem().rename(&self.aid, &from, &to).await
    }

    pub async fn set_mode(&self, path: &str, mode: u32) -> core::result::Result<(), crate::platform::FilesystemSetModeError> {
        use crate::platform::FilesystemSetModeError;

        let Some(normalized) = normalize_guest_path(path) else {
            return Err(FilesystemSetModeError::Other);
        };

        // A packaged virtual file is visible through the overlay but has no
        // writable persistent object whose host permissions can be changed.
        if !self.platform.filesystem().exists(&self.aid, &normalized).await && self.virtual_files.lock().contains_key(&normalized) {
            return Err(FilesystemSetModeError::Other);
        }

        self.platform.filesystem().set_mode(&self.aid, &normalized, mode).await
    }

    pub async fn total_space(&self) -> Option<u64> {
        self.platform.filesystem().total_space(&self.aid).await
    }

    pub async fn available_space(&self) -> Option<u64> {
        self.platform.filesystem().available_space(&self.aid).await
    }

    /// Lists the direct children visible through the overlay.
    ///
    /// Platform entries come first in their native enumeration order. Virtual
    /// archive entries that are not shadowed by the platform follow. Virtual
    /// directories are implicit in archive paths and are exposed by their
    /// first path component.
    pub async fn list(&self, path: &str) -> Option<Vec<String>> {
        let normalized = normalize_guest_directory(path)?;

        let platform_entries = self.platform.filesystem().list(&self.aid, &normalized).await;
        let platform_exists = platform_entries.is_some();
        let mut entries = platform_entries.unwrap_or_default();

        let mut seen = BTreeSet::new();
        for entry in &entries {
            seen.insert(entry.clone());
        }

        let prefix = if normalized.is_empty() {
            String::new()
        } else {
            let mut prefix = normalized.clone();
            prefix.push('/');
            prefix
        };

        for key in self.virtual_files.lock().keys() {
            let Some(rest) = key.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }

            let child = rest.split('/').next().unwrap_or(rest);
            if seen.insert(child.to_owned()) {
                entries.push(child.to_owned());
            }
        }

        if entries.is_empty() {
            let virtual_dir_exists = normalized.is_empty() || self.virtual_files.lock().keys().any(|key| key.starts_with(&prefix));

            if !virtual_dir_exists && !platform_exists {
                return None;
            }
        }

        Some(entries)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        boxed::Box,
        string::{String, ToString},
        sync::Arc,
        vec,
        vec::Vec,
    };
    use alloc::{collections::BTreeSet, format};

    use hashbrown::HashMap;
    use spin::Mutex;

    use crate::{
        audio_sink::AudioSink,
        database::DatabaseRepository,
        platform::{Filesystem, FilesystemMkdirError, FilesystemRenameError, FilesystemRmDirError, Platform},
        screen::Screen,
        time::Instant,
    };

    use super::FilesystemOverlay;

    #[derive(Default)]
    struct StubFilesystem {
        files: Mutex<HashMap<(String, String), Vec<u8>>>,
        directories: Mutex<BTreeSet<(String, String)>>,
    }
    #[async_trait::async_trait]
    impl Filesystem for StubFilesystem {
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
            let n = core::cmp::min(count, data.len() - offset);
            buf[..n].copy_from_slice(&data[offset..offset + n]);
            Some(n)
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

        async fn mkdir(&self, aid: &str, path: &str) -> core::result::Result<(), FilesystemMkdirError> {
            if path.is_empty() {
                return Err(FilesystemMkdirError::AlreadyExists);
            }

            let mut directories = self.directories.lock();
            let key = (aid.to_string(), path.to_string());

            if directories.contains(&key) || self.files.lock().contains_key(&key) {
                return Err(FilesystemMkdirError::AlreadyExists);
            }

            let parent = path.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
            if !parent.is_empty() {
                let parent_key = (aid.to_string(), parent.to_string());
                let mut prefix = parent.to_string();
                prefix.push('/');

                let parent_exists = directories.contains(&parent_key)
                    || directories
                        .iter()
                        .any(|(entry_aid, entry_path)| entry_aid == aid && entry_path.starts_with(&prefix))
                    || self
                        .files
                        .lock()
                        .keys()
                        .any(|(entry_aid, entry_path)| entry_aid == aid && entry_path.starts_with(&prefix));

                if !parent_exists {
                    return Err(FilesystemMkdirError::NotFound);
                }
            }

            directories.insert(key);
            Ok(())
        }

        async fn rmdir(&self, aid: &str, path: &str) -> core::result::Result<(), FilesystemRmDirError> {
            if path.is_empty() {
                return Err(FilesystemRmDirError::Other);
            }

            let key = (aid.to_string(), path.to_string());

            if self.files.lock().contains_key(&key) {
                return Err(FilesystemRmDirError::Other);
            }

            let mut prefix = path.to_string();
            prefix.push('/');

            let has_file_child = self
                .files
                .lock()
                .keys()
                .any(|(entry_aid, entry_path)| entry_aid == aid && entry_path.starts_with(&prefix));

            let mut directories = self.directories.lock();
            let exists = directories.contains(&key);
            let has_directory_child = directories
                .iter()
                .any(|(entry_aid, entry_path)| entry_aid == aid && entry_path.starts_with(&prefix));

            if has_file_child || has_directory_child {
                return Err(FilesystemRmDirError::NotEmpty);
            }

            if !exists {
                return Err(FilesystemRmDirError::NotFound);
            }

            directories.remove(&key);
            Ok(())
        }

        async fn rename(&self, aid: &str, from: &str, to: &str) -> core::result::Result<(), FilesystemRenameError> {
            let mut files = self.files.lock();
            let Some(data) = files.remove(&(aid.to_string(), from.to_string())) else {
                return Err(FilesystemRenameError::NotFound);
            };
            files.insert((aid.to_string(), to.to_string()), data);
            Ok(())
        }

        async fn set_mode(&self, aid: &str, path: &str, _mode: u32) -> core::result::Result<(), crate::platform::FilesystemSetModeError> {
            if self.files.lock().contains_key(&(aid.to_string(), path.to_string())) {
                Ok(())
            } else {
                Err(crate::platform::FilesystemSetModeError::NotFound)
            }
        }

        async fn total_space(&self, _aid: &str) -> Option<u64> {
            Some(32 * 1024 * 1024)
        }

        async fn available_space(&self, _aid: &str) -> Option<u64> {
            Some(16 * 1024 * 1024)
        }

        async fn list(&self, aid: &str, path: &str) -> Option<Vec<String>> {
            let prefix = if path.is_empty() { String::new() } else { format!("{path}/") };
            let files = self.files.lock();
            let directories = self.directories.lock();
            let mut entries = Vec::new();
            let mut seen = BTreeSet::new();
            let mut directory_exists = path.is_empty() || directories.contains(&(aid.to_string(), path.to_string()));

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
                let child = rest.split('/').next().unwrap_or(rest).to_string();
                directory_exists = true;

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

    struct StubPlatform {
        fs: StubFilesystem,
    }
    impl Platform for StubPlatform {
        fn screen(&self) -> &dyn Screen {
            unimplemented!()
        }
        fn now(&self) -> Instant {
            Instant::from_epoch_millis(0)
        }
        fn database_repository(&self) -> &dyn DatabaseRepository {
            unimplemented!()
        }
        fn filesystem(&self) -> &dyn Filesystem {
            &self.fs
        }
        fn audio_sink(&self) -> Box<dyn AudioSink> {
            unimplemented!()
        }
        fn write_stdout(&self, _buf: &[u8]) {}
        fn write_stderr(&self, _buf: &[u8]) {}
        fn exit(&self) {}
        fn vibrate(&self, _duration_ms: u64, _intensity: u8) {}

        fn set_backlight_mode(&self, _mode: u8) {}
    }

    fn setup() -> FilesystemOverlay {
        let platform: Arc<Box<dyn Platform>> = Arc::new(Box::new(StubPlatform {
            fs: StubFilesystem::default(),
        }));
        FilesystemOverlay::new(platform, "test-aid")
    }

    #[futures_test::test]
    async fn add_then_read_virtual() {
        let fs = setup();
        fs.add_virtual("a.bin", vec![1, 2, 3, 4]);

        let mut buf = [0u8; 4];
        assert_eq!(fs.read("a.bin", 0, 4, &mut buf).await, Some(4));
        assert_eq!(buf, [1, 2, 3, 4]);
    }

    #[futures_test::test]
    async fn size_falls_through_to_virtual() {
        let fs = setup();
        fs.add_virtual("x", vec![0; 17]);

        assert_eq!(fs.size("x").await, Some(17));
        assert_eq!(fs.size("nope").await, None);
    }

    #[futures_test::test]
    async fn exists_checks_both_layers() {
        let fs = setup();
        fs.add_virtual("x", vec![1]);

        assert!(fs.exists("x").await);
        assert!(!fs.exists("y").await);

        fs.write("written", 0, &[9]).await;
        assert!(fs.exists("written").await);
    }

    #[futures_test::test]
    async fn leading_slash_normalized() {
        let fs = setup();
        fs.add_virtual("/a/b", vec![9]);

        assert!(fs.exists("a/b").await);
        assert!(fs.exists("/a/b").await);
    }

    #[futures_test::test]
    async fn read_past_eof_virtual_returns_some_zero() {
        let fs = setup();
        fs.add_virtual("a", vec![1, 2, 3]);

        let mut buf = [0u8; 4];
        assert_eq!(fs.read("a", 10, 4, &mut buf).await, Some(0));
    }

    #[futures_test::test]
    async fn read_missing_returns_none() {
        let fs = setup();
        let mut buf = [0u8; 4];
        assert_eq!(fs.read("nope", 0, 4, &mut buf).await, None);
    }

    #[futures_test::test]
    async fn platform_write_shadows_virtual() {
        let fs = setup();
        fs.add_virtual("cfg.dat", vec![0xAA, 0xBB, 0xCC]);
        fs.write("cfg.dat", 0, &[1, 2, 3, 4]).await;

        let mut buf = [0u8; 4];
        assert_eq!(fs.read("cfg.dat", 0, 4, &mut buf).await, Some(4));
        assert_eq!(buf, [1, 2, 3, 4]);
    }

    #[futures_test::test]
    async fn list_merges_platform_and_virtual_direct_children() {
        let fs = setup();

        fs.write("dir/platform.dat", 0, &[1]).await;
        fs.add_virtual("dir/virtual.dat", vec![2]);
        fs.add_virtual("dir/sub/deep.dat", vec![3]);
        fs.add_virtual("dir/platform.dat", vec![4]);

        let entries = fs.list("dir/").await.unwrap();

        assert_eq!(entries.first().map(String::as_str), Some("platform.dat"));
        assert_eq!(entries.iter().filter(|x| x.as_str() == "platform.dat").count(), 1);

        let mut virtual_tail = entries[1..].to_vec();
        virtual_tail.sort();
        assert_eq!(virtual_tail, vec!["sub".to_string(), "virtual.dat".to_string()]);
    }

    #[futures_test::test]
    async fn list_virtual_root_exposes_only_direct_children() {
        let fs = setup();

        fs.add_virtual("root.bin", vec![1]);
        fs.add_virtual("P/data.bin", vec![2]);
        fs.add_virtual("P/nested/deep.bin", vec![3]);

        let mut entries = fs.list("/").await.unwrap();
        entries.sort();

        assert_eq!(entries, vec!["P".to_string(), "root.bin".to_string()]);
    }
}
