use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};

use hashbrown::HashMap;
use spin::Mutex;
use wie_backend::{AudioSink, Database, DatabaseRepository, Filesystem, Instant, Platform, RecordId, Screen, canvas::Image};
use wie_util::Result;

use crate::filesystem::MemoryFilesystem;

static TEST_EPOCH: AtomicU64 = AtomicU64::new(0);

pub enum TestPlatformEvent {
    Stdout(Vec<u8>),
    OpenUrl(String),
    Exit,
}

pub struct TestPlatform {
    screen: TestScreen,
    event_handler: Option<Box<dyn Fn(TestPlatformEvent) + Sync + Send>>,
    fs: Arc<MemoryFilesystem>,
    db: Arc<MemoryDatabaseRepository>,
    system_information: HashMap<String, String>,
}

impl Default for TestPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl TestPlatform {
    pub fn new() -> Self {
        Self {
            screen: TestScreen,
            event_handler: None,
            fs: Arc::new(MemoryFilesystem::default()),
            db: Arc::new(MemoryDatabaseRepository::default()),
            system_information: HashMap::new(),
        }
    }

    pub fn with_event_handler<T>(event_handler: T) -> Self
    where
        T: Fn(TestPlatformEvent) + Sync + Send + 'static,
    {
        Self {
            screen: TestScreen,
            event_handler: Some(Box::new(event_handler)),
            fs: Arc::new(MemoryFilesystem::default()),
            db: Arc::new(MemoryDatabaseRepository::default()),
            system_information: HashMap::new(),
        }
    }

    pub fn with_system_information(mut self, key: &str, value: &str) -> Self {
        self.system_information
            .insert(key.to_string(), value.to_string());
        self
    }
}

impl Platform for TestPlatform {
    fn screen(&self) -> &dyn Screen {
        &self.screen
    }

    fn now(&self) -> Instant {
        let epoch = TEST_EPOCH.fetch_add(8, Ordering::SeqCst);
        Instant::from_epoch_millis(epoch) // TODO
    }

    fn database_repository(&self) -> &dyn DatabaseRepository {
        self.db.as_ref()
    }

    fn filesystem(&self) -> &dyn Filesystem {
        self.fs.as_ref()
    }

    fn audio_sink(&self) -> Box<dyn AudioSink> {
        Box::new(TestAudioSink)
    }

    fn system_information(&self, key: &str) -> Option<String> {
        self.system_information.get(key).cloned()
    }

    fn open_url(&self, url: &str) -> bool {
        if let Some(event_handler) = &self.event_handler {
            (event_handler)(TestPlatformEvent::OpenUrl(url.to_string()));
            true
        } else {
            false
        }
    }

    fn write_stdout(&self, buf: &[u8]) {
        if let Some(event_handler) = &self.event_handler {
            (event_handler)(TestPlatformEvent::Stdout(buf.to_vec()))
        }
    }

    fn write_stderr(&self, _buf: &[u8]) {}

    fn exit(&self) {
        if let Some(event_handler) = &self.event_handler {
            (event_handler)(TestPlatformEvent::Exit);
        }
    }

    fn vibrate(&self, _duration_ms: u64, _intensity: u8) {}

    fn set_backlight_mode(&self, _mode: u8) {}
}

type DatabaseKey = (String, String);
type DatabaseStore = HashMap<DatabaseKey, HashMap<RecordId, Vec<u8>>>;

#[derive(Default)]
struct MemoryDatabaseRepository {
    store: Arc<Mutex<DatabaseStore>>,
}

#[async_trait::async_trait]
impl DatabaseRepository for MemoryDatabaseRepository {
    async fn open(&self, name: &str, app_id: &str) -> Box<dyn Database> {
        let key = (app_id.to_string(), name.to_string());
        self.store.lock().entry(key.clone()).or_default();
        Box::new(MemoryDatabase {
            store: self.store.clone(),
            key,
        })
    }

    async fn exists(&self, name: &str, app_id: &str) -> bool {
        self.store.lock().contains_key(&(app_id.to_string(), name.to_string()))
    }

    async fn delete(&self, name: &str, app_id: &str) -> bool {
        self.store.lock().remove(&(app_id.to_string(), name.to_string())).is_some()
    }

    async fn list(&self, app_id: &str) -> Vec<String> {
        let store = self.store.lock();
        let mut names: Vec<String> = store
            .keys()
            .filter_map(|(stored_app_id, name)| {
                if stored_app_id != app_id {
                    return None;
                }

                // Android/CLI normalize a guest-leading slash away. Preserve
                // that observable storage model in the in-memory repository.
                let name = name.trim_start_matches('/');
                if name.is_empty() || name.contains('/') {
                    return None;
                }

                Some(name.to_string())
            })
            .collect();

        names.sort();
        names.dedup();
        names
    }
}

struct MemoryDatabase {
    store: Arc<Mutex<DatabaseStore>>,
    key: DatabaseKey,
}

#[async_trait::async_trait]
impl Database for MemoryDatabase {
    async fn next_id(&self) -> RecordId {
        let store = self.store.lock();
        let records = store.get(&self.key);
        let mut id = 1;
        while records.is_some_and(|records| records.contains_key(&id)) {
            id += 1;
        }
        id
    }

    async fn add(&mut self, data: &[u8]) -> RecordId {
        let id = self.next_id().await;
        self.set(id, data).await;
        id
    }

    async fn get(&self, id: RecordId) -> Option<Vec<u8>> {
        self.store.lock().get(&self.key)?.get(&id).cloned()
    }

    async fn set(&mut self, id: RecordId, data: &[u8]) -> bool {
        let mut store = self.store.lock();
        store.entry(self.key.clone()).or_default().insert(id, data.to_vec());
        true
    }

    async fn delete(&mut self, id: RecordId) -> bool {
        self.store.lock().get_mut(&self.key).is_some_and(|records| records.remove(&id).is_some())
    }

    async fn get_record_ids(&self) -> Vec<RecordId> {
        self.store
            .lock()
            .get(&self.key)
            .map(|records| records.keys().copied().collect())
            .unwrap_or_default()
    }
}

pub struct TestAudioSink;

/// Discards audio. It used to panic on every method, which meant any test
/// running a title that makes a sound died on the sound rather than on
/// whatever it was testing.
impl AudioSink for TestAudioSink {
    fn play_wave(&self, _channel: u8, _sampling_rate: u32, _wave_data: &[i16]) {}

    fn midi_note_on(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}

    fn midi_note_off(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}

    fn midi_program_change(&self, _voice: u32, _channel_id: u8, _program: u8) {}

    fn midi_control_change(&self, _voice: u32, _channel_id: u8, _control: u8, _value: u8) {}

    fn midi_pitch_bend(&self, _voice: u32, _channel_id: u8, _value: u16) {}

    fn midi_sysex(&self, _voice: u32, _data: &[u8]) {}
}

#[derive(Default)]
pub struct TestScreen;

impl Screen for TestScreen {
    fn request_redraw(&self) -> Result<()> {
        Ok(())
    }

    fn paint(&self, _image: &dyn Image) {}

    fn width(&self) -> u32 {
        320
    }

    fn height(&self) -> u32 {
        240
    }
}
