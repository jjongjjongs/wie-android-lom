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
    AudioSink, DatabaseRepository, Filesystem, Instant, Network, Platform, Screen,
    canvas::{Image, PixelType, Rgb565Pixel},
};
use wie_util::Result;

use crate::{audio::AndroidAudioSink, database::AndroidDatabaseRepository, filesystem::AndroidFilesystem, ma3::SynthMixer, network::AndroidNetwork};

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
    mixer: Arc<Mutex<SynthMixer>>,
    redraw_requested: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    backlight_mode: Arc<AtomicU8>,
    phone_call: Arc<Mutex<Option<String>>>,
    browser_url: Arc<Mutex<Option<String>>>,
}

/// Bound on queued audio commands. A game that pushes samples faster than
/// Java drains them would otherwise grow this without limit; dropping the
/// oldest keeps playback current instead of drifting further behind.
const MAX_QUEUED_AUDIO: usize = 64;

impl Shared {
    pub fn mixer(&self) -> std::sync::MutexGuard<'_, SynthMixer> {
        self.mixer.lock().unwrap_or_else(|x| x.into_inner())
    }

    /// A handle to the mixer for the audio pump, which pulls chunks from it on
    /// its own thread rather than through the game loop.
    pub fn mixer_handle(&self) -> Arc<Mutex<SynthMixer>> {
        self.mixer.clone()
    }

    /// Audio is now pulled from the mixer by the Java audio thread, clocked by
    /// its AudioTrack (see [`crate::audio::render_audio_bytes`]), so the mixer
    /// must not also be advanced from the game loop here - doing both would
    /// consume the stream twice and play it at double speed. Kept as a no-op so
    /// the tick loop's call site is undisturbed.
    pub fn render_synth(&self) {}

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

    pub fn push_phone_call(&self, number: String) {
        *self.phone_call.lock().unwrap_or_else(|x| x.into_inner()) = Some(number);
    }

    pub fn take_phone_call(&self) -> Option<String> {
        self.phone_call.lock().unwrap_or_else(|x| x.into_inner()).take()
    }

    pub fn push_browser_url(&self, url: String) {
        *self.browser_url.lock().unwrap_or_else(|x| x.into_inner()) = Some(url);
    }

    pub fn take_browser_url(&self) -> Option<String> {
        self.browser_url.lock().unwrap_or_else(|x| x.into_inner()).take()
    }
}

/// Immutable host handset/HAL information captured when a game starts.
///
/// This is intentionally separate from the public WIPI system-property API.
/// Only values actually supplied by the Android host are represented.
#[derive(Clone)]
pub struct AndroidHandsetInformation {
    phone_model: String,
}

impl AndroidHandsetInformation {
    pub fn new(phone_model: String) -> Self {
        Self { phone_model }
    }

    fn get(&self, key: &str) -> Option<String> {
        match key {
            "PHONEMODEL" => Some(self.phone_model.clone()),
            _ => None,
        }
    }
}

pub struct AndroidPlatform {
    screen: AndroidScreen,
    shared: Shared,
    filesystem: AndroidFilesystem,
    database_repository: AndroidDatabaseRepository,
    network: AndroidNetwork,
    handset_information: AndroidHandsetInformation,
}

impl AndroidPlatform {
    pub fn new(
        runtime_dir: PathBuf,
        width: u32,
        height: u32,
        shared: Shared,
        handset_information: AndroidHandsetInformation,
    ) -> Self {
        Self {
            screen: AndroidScreen {
                width,
                height,
                shared: shared.clone(),
            },
            filesystem: AndroidFilesystem::new(runtime_dir.join("fs")),
            database_repository: AndroidDatabaseRepository::new(runtime_dir.join("db")),
            network: AndroidNetwork::new(),
            handset_information,
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

    fn network(&self) -> Option<&dyn Network> {
        Some(&self.network)
    }

    fn system_information(&self, key: &str) -> Option<String> {
        self.handset_information.get(key)
    }

    fn call_place(&self, number: &str) -> bool {
        self.shared.push_phone_call(number.to_string());
        true
    }

    fn open_url(&self, url: &str) -> bool {
        self.shared.push_browser_url(url.to_string());
        true
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


#[cfg(test)]
mod handset_information_tests {
    use super::AndroidHandsetInformation;

    #[test]
    fn android_handset_information_exposes_only_supplied_hal_values() {
        let configured =
            AndroidHandsetInformation::new("SM-S948N".to_owned());

        assert_eq!(configured.get("PHONEMODEL").as_deref(), Some("SM-S948N"));

        for unavailable in [
            "MDN",
            "CURRENTCH",
            "SID",
            "NID",
            "BASEID",
            "BESTPN",
        ] {
            assert_eq!(configured.get(unavailable), None, "{unavailable}");
        }
    }
}
