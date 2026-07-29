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
        captured.colors.clear();
        for color in image.colors() {
            let packed = ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32;
            *captured.colors.entry(packed).or_default() += 1;
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
    fn now(&self) -> Instant {
        self.inner.now()
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
        if let Err(error) = emulator.tick() {
            eprintln!("[{label}] stopped after {ticks} ticks: {error}");
            break;
        }
        ticks += 1;
    }

    let captured = screen.captured.lock().unwrap();
    eprintln!(
        "[{label}] {ticks} ticks, {} frames painted, {}x{}, {} distinct colours",
        captured.frames,
        captured.width,
        captured.height,
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
    run("LoM", include_bytes!("../../test_games/legend_of_master.zip"), 600);
}
