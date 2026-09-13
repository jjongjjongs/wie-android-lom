use std::{fs, path::PathBuf};

use wie_backend::RecordId;

/// Record stores rooted at the app-private directory, laid out as
/// `<base>/<app_id>/<name>/<record id>`.
pub struct AndroidDatabaseRepository {
    base_path: PathBuf,
}

impl AndroidDatabaseRepository {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    fn path_for_database(&self, name: &str, app_id: &str) -> PathBuf {
        let sanitized_app_id: String = app_id.chars().filter(|c| !matches!(c, '/' | '\\' | '\0')).collect();
        let app_id = if sanitized_app_id.is_empty() || sanitized_app_id == "." || sanitized_app_id == ".." {
            "_"
        } else {
            &sanitized_app_id
        };

        // Guest database names are free-form and routinely contain a leading
        // slash, so they are normalized into a relative path instead of being
        // rejected - a rejected name would lose the game's save data.
        let name: String = name.chars().map(|c| if matches!(c, '\\' | '\0') { '_' } else { c }).collect();
        let mut normalized_name = PathBuf::new();
        for segment in name.trim_start_matches('/').split('/') {
            match segment {
                "" | "." => {}
                ".." => normalized_name.push("_"),
                segment => normalized_name.push(segment),
            }
        }
        if normalized_name.as_os_str().is_empty() {
            normalized_name.push("_");
        }

        self.base_path.join(app_id).join(normalized_name)
    }

    fn list_databases(&self, app_id: &str) -> Vec<String> {
        let sanitized_app_id: String = app_id.chars().filter(|c| !matches!(c, '/' | '\\' | '\0')).collect();
        let app_id = if sanitized_app_id.is_empty() || sanitized_app_id == "." || sanitized_app_id == ".." {
            "_"
        } else {
            &sanitized_app_id
        };

        let root = self.base_path.join(app_id);
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };

        let mut names = Vec::new();

        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // A direct database directory contains numeric record files.
            // This rejects an intermediate directory belonging only to a
            // nested logical name such as "foo/bar".
            let has_record = fs::read_dir(&path)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|record| record.ok())
                .any(|record| record.path().is_file() && record.file_name().to_str().is_some_and(|name| name.parse::<RecordId>().is_ok()));

            if !has_record {
                continue;
            }

            if let Ok(name) = entry.file_name().into_string() {
                names.push(name);
            }
        }

        names.sort();
        names
    }
}

#[async_trait::async_trait]
impl wie_backend::DatabaseRepository for AndroidDatabaseRepository {
    async fn open(&self, name: &str, app_id: &str) -> Box<dyn wie_backend::Database> {
        let path = self.path_for_database(name, app_id);

        if let Err(error) = fs::create_dir_all(&path) {
            tracing::warn!("Failed to create database at {path:?}: {error}");
        }

        Box::new(AndroidDatabase { base_path: path })
    }

    async fn exists(&self, name: &str, app_id: &str) -> bool {
        self.path_for_database(name, app_id).exists()
    }

    async fn delete(&self, name: &str, app_id: &str) -> bool {
        let path = self.path_for_database(name, app_id);

        match fs::remove_dir_all(&path) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                tracing::warn!("Failed to delete database at {path:?}: {error}");
                false
            }
        }
    }

    async fn list(&self, app_id: &str) -> Vec<String> {
        self.list_databases(app_id)
    }
}

struct AndroidDatabase {
    base_path: PathBuf,
}

impl AndroidDatabase {
    fn find_empty_record_id(&self) -> RecordId {
        let mut record_id = 1; // XXX midp requires first record to be 1

        while self.path_for_record(record_id).exists() {
            record_id += 1;
        }

        record_id
    }

    fn path_for_record(&self, id: RecordId) -> PathBuf {
        self.base_path.join(id.to_string())
    }
}

#[async_trait::async_trait]
impl wie_backend::Database for AndroidDatabase {
    async fn next_id(&self) -> RecordId {
        self.find_empty_record_id()
    }

    async fn add(&mut self, data: &[u8]) -> RecordId {
        let id = self.find_empty_record_id();

        if let Err(error) = fs::write(self.path_for_record(id), data) {
            tracing::warn!("Failed to add record {id} to {:?}: {error}", self.base_path);
        }

        id
    }

    async fn get(&self, id: RecordId) -> Option<Vec<u8>> {
        fs::read(self.path_for_record(id)).ok()
    }

    async fn set(&mut self, id: RecordId, data: &[u8]) -> bool {
        fs::write(self.path_for_record(id), data).is_ok()
    }

    async fn delete(&mut self, id: RecordId) -> bool {
        fs::remove_file(self.path_for_record(id)).is_ok()
    }

    async fn get_record_ids(&self) -> Vec<RecordId> {
        let Ok(entries) = fs::read_dir(&self.base_path) else {
            return Vec::new();
        };

        entries
            .filter_map(|x| x.ok())
            .filter(|x| x.path().is_file())
            .filter_map(|x| x.file_name().to_str()?.parse().ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::AndroidDatabaseRepository;

    #[test]
    fn database_list_returns_direct_database_and_skips_nested_parent() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("wie_android_db_list_{}_{}", std::process::id(), unique));

        let repository = AndroidDatabaseRepository::new(base.clone());

        // Canonical LGT databases contain reserved metadata record 0.
        let direct = base.join("test-aid").join("root");
        fs::create_dir_all(&direct).unwrap();
        fs::write(direct.join("0"), b"metadata").unwrap();

        // This represents logical name "parent/child". The root-level
        // "parent" directory is only a path component, not a database.
        let nested = base.join("test-aid").join("parent").join("child");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("0"), b"metadata").unwrap();

        let names = repository.list_databases("test-aid");
        assert_eq!(names, vec!["root".to_string()]);

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn database_path_normalizes_guest_names() {
        let repository = AndroidDatabaseRepository::new(PathBuf::from("/data/db"));

        assert_eq!(
            repository.path_for_database("records", "game123"),
            PathBuf::from("/data/db/game123/records")
        );
        assert_eq!(
            repository.path_for_database("/save0.dat", "PD140106"),
            PathBuf::from("/data/db/PD140106/save0.dat")
        );
        assert!(
            repository
                .path_for_database("/../save0.dat", "PD140106")
                .starts_with(PathBuf::from("/data/db/PD140106"))
        );
    }
}
