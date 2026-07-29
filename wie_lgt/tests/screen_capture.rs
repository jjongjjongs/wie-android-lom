use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use test_utils::{TestPlatform, TestPlatformEvent};
use wie_backend::{AudioSink, DatabaseRepository, Emulator, Event, Filesystem, Instant, Options, Platform, Screen, canvas::Image, extract_zip};
use wie_util::Result;

#[derive(Default)]
struct Captured {
    frames: u32,
    best_frame: u32,
    width: u32,
    height: u32,
    colors: BTreeMap<u32, u32>,
}

#[derive(Default, Clone)]
struct CaptureScreen {
    captured: Arc<Mutex<Captured>>,
}

impl Screen for CaptureScreen {
    fn request_redraw(&self) -> Result<()> {
        Ok(())
    }

    fn paint(&self, image: &dyn Image) {
        let mut captured = self.captured.lock().unwrap();

        captured.frames += 1;
        captured.width = image.width();
        captured.height = image.height();

        let mut colors = BTreeMap::new();
        for color in image.colors() {
            let packed = ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32;
            *colors.entry(packed).or_default() += 1;
        }

        // Keep the busiest frame, not the last: a title that draws and then
        // clears would otherwise look like it never drew.
        if colors.len() >= captured.colors.len() {
            captured.colors = colors;
            captured.best_frame = captured.frames;
        }
    }

    fn width(&self) -> u32 {
        240
    }

    fn height(&self) -> u32 {
        320
    }
}

struct CapturePlatform {
    inner: TestPlatform,
    screen: CaptureScreen,
}

impl Platform for CapturePlatform {
    fn screen(&self) -> &dyn Screen {
        &self.screen
    }
    /// Real time, not the fake clock the shared test platform steps by hand: a
    /// title that waits on the clock never advances under that one.
    fn now(&self) -> Instant {
        let since_epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();

        Instant::from_epoch_millis(since_epoch.as_millis() as _)
    }
    fn database_repository(&self) -> &dyn DatabaseRepository {
        self.inner.database_repository()
    }
    fn filesystem(&self) -> &dyn Filesystem {
        self.inner.filesystem()
    }
    fn audio_sink(&self) -> Box<dyn AudioSink> {
        self.inner.audio_sink()
    }
    fn write_stdout(&self, buf: &[u8]) {
        self.inner.write_stdout(buf)
    }
    fn write_stderr(&self, buf: &[u8]) {
        self.inner.write_stderr(buf)
    }
    fn exit(&self) {
        self.inner.exit()
    }
    fn vibrate(&self, duration_ms: u64, intensity: u8) {
        self.inner.vibrate(duration_ms, intensity)
    }
}

/// Runs an archive for a while and reports the frames it painted.
fn run(label: &str, archive: &[u8], ticks_limit: u32) {
    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let screen = CaptureScreen::default();

    let platform = Box::new(CapturePlatform {
        inner: TestPlatform::with_event_handler(move |event| match event {
            TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
            TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
        }),
        screen: screen.clone(),
    });

    let files = extract_zip(archive).unwrap();
    let mut emulator = wie_lgt::LgtEmulator::from_archive(
        platform,
        files,
        Options {
            enable_gdbserver: false,
            profile: None,
        },
    )
    .unwrap();

    let mut ticks = 0;
    while !exited.load(Ordering::SeqCst) && ticks < ticks_limit {
        if ticks % 40 == 0 {
            emulator.handle_event(Event::Redraw);
        }
        // Titles wait on input: Legend of Master's first screen ends with
        // "press any key".
        if ticks % 200 == 100 {
            emulator.handle_event(Event::Keydown(wie_backend::KeyCode::OK));
        }
        if ticks % 200 == 120 {
            emulator.handle_event(Event::Keyup(wie_backend::KeyCode::OK));
        }
        if let Err(error) = emulator.tick() {
            eprintln!("[{label}] stopped after {ticks} ticks: {error}");
            break;
        }
        ticks += 1;
    }

    let captured = screen.captured.lock().unwrap();
    eprintln!(
        "[{label}] {ticks} ticks, {} frames painted, {}x{}, busiest frame {} with {} distinct colours",
        captured.frames,
        captured.width,
        captured.height,
        captured.best_frame,
        captured.colors.len()
    );

    let mut top: Vec<_> = captured.colors.iter().collect();
    top.sort_by_key(|(_, count)| core::cmp::Reverse(**count));
    for (color, count) in top.into_iter().take(6) {
        eprintln!("[{label}]   #{color:06x} x{count}");
    }
}

/// Reports what an application actually puts on screen, which is not
/// something a pass/fail test can capture while LGT support is still being
/// built out. Run it with `cargo test -p wie_lgt --test screen_capture --
/// --ignored --nocapture`.
#[test]
#[ignore = "diagnostic"]
fn capture_legend_of_master() {
    run("LoM", include_bytes!("../../test_games/legend_of_master.zip"), 2000);
}
