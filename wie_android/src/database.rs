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
