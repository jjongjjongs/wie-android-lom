use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use wie_backend::{
    AudioSink, DatabaseRepository, Filesystem, Instant, Platform, Screen,
    canvas::{Image, PixelType, Rgb565Pixel},
};
use wie_util::Result;

use crate::{audio::AndroidAudioSink, database::AndroidDatabaseRepository, filesystem::AndroidFilesystem, ma3::Synth};

/// A frame handed to `Screen::paint`, kept until Java collects it.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Native-endian RGB565 values consumed by an Android RGB_565 Bitmap.
    pub pixels: Vec<i16>,
}

/// Everything the JNI layer reads out of a running emulator. The emulator
/// thread writes; the UI thread only ever reads `frame` and `audio`.
#[derive(Clone, Default)]
pub struct Shared {
    frame: Arc<Mutex<Option<Frame>>>,
    audio: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Shared because `Platform::audio_sink` hands out a fresh sink whenever
    /// it is asked, and the voices have to outlive any one of them.
    synth: Arc<Mutex<Synth>>,
    last_render: Arc<Mutex<Option<std::time::Instant>>>,
    redraw_requested: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    backlight_mode: Arc<AtomicU8>,
}

/// Bound on queued audio commands. A game that pushes samples faster than
/// Java drains them would otherwise grow this without limit; dropping the
/// oldest keeps playback current instead of drifting further behind.
const MAX_QUEUED_AUDIO: usize = 64;

/// Longest gap the synthesiser will try to fill in one go. Past this the
/// stream has already broken, and rendering the whole backlog would only
/// queue a burst of stale music behind the live one.
const MAX_SYNTH_CATCHUP_MS: u32 = 120;

impl Shared {
    pub fn synth(&self) -> std::sync::MutexGuard<'_, Synth> {
        self.synth.lock().unwrap_or_else(|x| x.into_inner())
    }

    /// Renders however much of the synthesiser's output the wall clock says is
    /// owed, and queues it.
    ///
    /// Called once per tick rather than from the sink, because a sequence
    /// delivers its notes in bursts and the sound between them has to keep
    /// coming. Measuring the gap rather than assuming the tick interval is
    /// what keeps the stream level: ticks are scheduled with a fixed delay
    /// *after* the work, so they run slower than they are asked to, and a
    /// fixed amount per tick would underrun by exactly that much.
    pub fn render_synth(&self) {
        let now = std::time::Instant::now();
        let mut last = self.last_render.lock().unwrap_or_else(|x| x.into_inner());

        let elapsed = last.replace(now).map(|previous| now.saturating_duration_since(previous));
        drop(last);

        // A first call has nothing to measure, and a long stall - the app in
        // the background, a slow load - would ask for a chunk far too big to
        // be worth catching up on.
        let millis = elapsed.map(|x| x.as_millis() as u32).unwrap_or(0).clamp(0, MAX_SYNTH_CATCHUP_MS);
        if millis == 0 {
            return;
        }

        let frames = (crate::ma3::SAMPLE_RATE as usize * millis as usize) / 1000;

        if let Some(samples) = self.synth().render(frames) {
            self.push_audio(crate::audio::stream_command(&samples));
        }
    }

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

    pub fn set_backlight_mode(&self, mode: u8) {
        self.backlight_mode.store(mode, Ordering::SeqCst);
    }

    pub fn take_backlight_mode(&self) -> u8 {
        self.backlight_mode.swap(0, Ordering::SeqCst)
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

    fn set_backlight_mode(&self, mode: u8) {
        self.shared.set_backlight_mode(mode);
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
        let pixels = if image.bytes_per_pixel() == 2 {
            image
                .raw()
                .chunks_exact(2)
                .map(|bytes| i16::from_ne_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>()
        } else {
            image
                .colors()
                .into_iter()
                .map(|color| Rgb565Pixel::from_color(color) as i16)
                .collect::<Vec<_>>()
        };

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
