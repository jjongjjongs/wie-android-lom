use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use wie_backend::{AudioSink, DatabaseRepository, Filesystem, Instant, Platform, Screen, canvas::Image};
use wie_util::Result;

use crate::{audio::AndroidAudioSink, database::AndroidDatabaseRepository, filesystem::AndroidFilesystem};

/// A frame handed to `Screen::paint`, kept until Java collects it.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// ARGB_8888, the layout `Bitmap.setPixels` expects.
    pub pixels: Vec<i32>,
}

/// Everything the JNI layer reads out of a running emulator. The emulator
/// thread writes; the UI thread only ever reads `frame` and `audio`.
#[derive(Clone, Default)]
pub struct Shared {
    frame: Arc<Mutex<Option<Frame>>>,
    audio: Arc<Mutex<VecDeque<Vec<u8>>>>,
    redraw_requested: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}

/// Bound on queued audio commands. A game that pushes samples faster than
/// Java drains them would otherwise grow this without limit; dropping the
/// oldest keeps playback current instead of drifting further behind.
const MAX_QUEUED_AUDIO: usize = 64;

impl Shared {
    pub fn take_frame(&self) -> Option<Frame> {
        self.frame.lock().unwrap_or_else(|x| x.into_inner()).take()
    }

    pub fn take_audio(&self) -> Option<Vec<u8>> {
        self.audio.lock().unwrap_or_else(|x| x.into_inner()).pop_front()
    }

    pub fn push_audio(&self, command: Vec<u8>) {
        let mut queue = self.audio.lock().unwrap_or_else(|x| x.into_inner());

        while queue.len() >= MAX_QUEUED_AUDIO {
            queue.pop_front();
        }
        queue.push_back(command);
    }

    /// Consumes a pending `Screen::request_redraw`, so the caller knows to
    /// feed `Event::Redraw` back into the emulator.
    pub fn take_redraw_request(&self) -> bool {
        self.redraw_requested.swap(false, Ordering::SeqCst)
    }

    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
}

pub struct AndroidPlatform {
    screen: AndroidScreen,
    shared: Shared,
    filesystem: AndroidFilesystem,
    database_repository: AndroidDatabaseRepository,
}

impl AndroidPlatform {
    pub fn new(runtime_dir: PathBuf, width: u32, height: u32, shared: Shared) -> Self {
        Self {
            screen: AndroidScreen {
                width,
                height,
                shared: shared.clone(),
            },
            filesystem: AndroidFilesystem::new(runtime_dir.join("fs")),
            database_repository: AndroidDatabaseRepository::new(runtime_dir.join("db")),
            shared,
        }
    }
}

impl Platform for AndroidPlatform {
    fn screen(&self) -> &dyn Screen {
        &self.screen
    }

    fn now(&self) -> Instant {
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();

        Instant::from_epoch_millis(since_epoch.as_millis() as _)
    }

    fn database_repository(&self) -> &dyn DatabaseRepository {
        &self.database_repository
    }

    fn filesystem(&self) -> &dyn Filesystem {
        &self.filesystem
    }

    fn audio_sink(&self) -> Box<dyn AudioSink> {
        Box::new(AndroidAudioSink::new(self.shared.clone()))
    }

    fn write_stdout(&self, buf: &[u8]) {
        tracing::info!("{}", String::from_utf8_lossy(buf));
    }

    fn write_stderr(&self, buf: &[u8]) {
        tracing::warn!("{}", String::from_utf8_lossy(buf));
    }

    fn exit(&self) {
        self.shared.exited.store(true, Ordering::SeqCst);
    }

    fn vibrate(&self, duration_ms: u64, intensity: u8) {
        self.shared.push_audio(crate::audio::vibrate_command(duration_ms, intensity));
    }
}

struct AndroidScreen {
    width: u32,
    height: u32,
    shared: Shared,
}

impl Screen for AndroidScreen {
    fn request_redraw(&self) -> Result<()> {
        self.shared.redraw_requested.store(true, Ordering::SeqCst);

        Ok(())
    }

    fn paint(&self, image: &dyn Image) {
        let pixels = image
            .colors()
            .iter()
            .map(|x| (((x.a as u32) << 24) | ((x.r as u32) << 16) | ((x.g as u32) << 8) | (x.b as u32)) as i32)
            .collect::<Vec<_>>();

        let frame = Frame {
            width: image.width(),
            height: image.height(),
            pixels,
        };

        // Only the newest frame matters; Java polls slower than games paint.
        *self.shared.frame.lock().unwrap_or_else(|x| x.into_inner()) = Some(frame);
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}
