use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    best_pixels: Vec<u8>,
    // The *last* painted frame's signature, so a screen that advances to a
    // simpler view is not masked by the busiest-frame heuristic.
    last_sig: u64,
    last_colors: usize,
    last_pixels: Vec<u8>,
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

        let frame_number = captured.frames;
        let width = image.width();
        let height = image.height();

        let mut colors = BTreeMap::new();
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        let mut sig: u64 = 1469598103934665603;

        for color in image.colors() {
            let packed = ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32;
            *colors.entry(packed).or_default() += 1;

            pixels.push(color.r);
            pixels.push(color.g);
            pixels.push(color.b);

            // FNV-1a over the pixels: a cheap fingerprint of the exact frame.
            sig ^= packed as u64;
            sig = sig.wrapping_mul(1099511628211);
        }

        captured.last_sig = sig;
        captured.last_colors = colors.len();
        captured.last_pixels = pixels.clone();

        // Keep the busiest frame, not the last: a screen that draws and then
        // clears would otherwise look like it never rendered.
        if colors.len() >= captured.colors.len() {
            captured.colors = colors;
            captured.best_frame = frame_number;
            captured.best_pixels = pixels;
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
    clock: Arc<AtomicU64>,
}

impl Platform for CapturePlatform {
    fn screen(&self) -> &dyn Screen {
        &self.screen
    }
    /// A clock that advances a millisecond on every read.
    ///
    /// The wall clock does not work here. `Executor::tick` runs until eight
    /// milliseconds have passed *or* every task is asleep, and an idle
    /// emulator hits the second condition at once - so a loop that calls
    /// `tick` as fast as it can burns thousands of iterations inside a single
    /// wall millisecond and the application's `sleep(16)` almost never
    /// expires. Emulated time then crawls, and a title that draws once per
    /// frame looks like a title that never draws.
    ///
    /// Advancing on read is what the executor's own tests do, and it ties
    /// emulated time to work done rather than to how fast the host is.
    fn now(&self) -> Instant {
        Instant::from_epoch_millis(self.clock.fetch_add(1, Ordering::SeqCst))
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
    fn set_backlight_mode(&self, mode: u8) {
        self.inner.set_backlight_mode(mode)
    }
}

/// Maps a `WIE_KEY` name to a backend key code, so a probe can target any key.
fn key_by_name(name: &str) -> Option<wie_backend::KeyCode> {
    use wie_backend::KeyCode::*;
    Some(match name.to_ascii_uppercase().as_str() {
        "UP" => UP,
        "DOWN" => DOWN,
        "LEFT" => LEFT,
        "RIGHT" => RIGHT,
        "OK" | "FIRE" => OK,
        "LEFT_SOFT" | "LSK" => LEFT_SOFT_KEY,
        "RIGHT_SOFT" | "RSK" => RIGHT_SOFT_KEY,
        "CLEAR" | "CLR" => CLEAR,
        "CALL" | "SEND" => CALL,
        "HANGUP" | "END" => HANGUP,
        "NUM0" => NUM0,
        "NUM1" => NUM1,
        "NUM2" => NUM2,
        "NUM3" => NUM3,
        "NUM4" => NUM4,
        "NUM5" => NUM5,
        "NUM6" => NUM6,
        "NUM7" => NUM7,
        "NUM8" => NUM8,
        "NUM9" => NUM9,
        "HASH" | "POUND" => HASH,
        "STAR" => STAR,
        _ => return None,
    })
}

/// Runs an archive for a while and reports the frames it painted.
fn run(label: &str, archive: &[u8], ticks_limit: u32) {
    // Diagnosing a blank screen means reading the runtime's own log, so honour
    // `RUST_LOG` here the way `wie_cli` does.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let screen = CaptureScreen::default();

    let platform = Box::new(CapturePlatform {
        inner: TestPlatform::with_event_handler(move |event| match event {
            TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
            TestPlatformEvent::OpenUrl(url) => eprintln!("[open-url] {url}"),
            TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
        }),
        screen: screen.clone(),
        clock: Arc::new(AtomicU64::new(0)),
    });

    let files = match extract_zip(archive) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("[{label}] not readable as an archive: {error}");
            return;
        }
    };

    let options = Options {
        enable_gdbserver: false,
        profile: None,
    };

    // The same two ways in the player has: a descriptor with a jar beside it,
    // or a bare jar that is the whole application. An archive stripped of its
    // `app_info` is still a title, and takes the second path.
    let emulator = if wie_lgt::LgtEmulator::loadable_archive(&files) {
        wie_lgt::LgtEmulator::from_archive(platform, files, options)
    } else {
        wie_lgt::LgtEmulator::from_jar(platform, label, archive.to_vec(), label, label, None, options)
    };

    let mut emulator = match emulator {
        Ok(emulator) => emulator,
        Err(error) => {
            eprintln!("[{label}] did not load: {error}");
            return;
        }
    };

    // Press OK once past the initial "press any key" notice. `WIE_INTRO_TICK`
    // overrides when that key lands (default 300).
    let intro_tick: u32 = std::env::var("WIE_INTRO_TICK").ok().and_then(|x| x.parse().ok()).unwrap_or(300);
    // A single probe key pressed at `WIE_PRESS_TICK`, named by `WIE_KEY`. This
    // is how we find which key a specific screen actually reacts to.
    let probe_key = std::env::var("WIE_KEY").ok().and_then(|name| key_by_name(&name));
    let press_tick: u32 = std::env::var("WIE_PRESS_TICK").ok().and_then(|x| x.parse().ok()).unwrap_or(u32::MAX);

    let mut ticks = 0;
    while !exited.load(Ordering::SeqCst) && ticks < ticks_limit {
        if ticks % 40 == 0 {
            emulator.handle_event(Event::Redraw);
        }
        if ticks == intro_tick {
            eprintln!("[{label}] pressing OK (intro) at tick {ticks}");
            emulator.handle_event(Event::Keydown(wie_backend::KeyCode::OK));
        }
        if ticks == intro_tick.saturating_add(20) {
            emulator.handle_event(Event::Keyup(wie_backend::KeyCode::OK));
        }

        // Leave the title screen with OK so probes land on the screen after it.
        let title_tick: u32 = std::env::var("WIE_TITLE_TICK").ok().and_then(|x| x.parse().ok()).unwrap_or(u32::MAX);
        if ticks == title_tick {
            eprintln!("[{label}] pressing OK (title) at tick {ticks}");
            emulator.handle_event(Event::Keydown(wie_backend::KeyCode::OK));
        }
        if ticks == title_tick.saturating_add(20) {
            emulator.handle_event(Event::Keyup(wie_backend::KeyCode::OK));
        }

        if let Some(key) = probe_key {
            if ticks == press_tick {
                eprintln!("[{label}] pressing probe key {:?} at tick {ticks}", key);
                emulator.handle_event(Event::Keydown(key));
            }
            if ticks == press_tick.saturating_add(20) {
                emulator.handle_event(Event::Keyup(key));
            }
        }

        // Optional repeated scroll: press DOWN `WIE_SCROLL_N` times, spaced 25
        // ticks apart, starting at `press_tick`, before any confirm probe.
        let scroll_n: u32 = std::env::var("WIE_SCROLL_N").ok().and_then(|x| x.parse().ok()).unwrap_or(0);
        for k in 0..scroll_n {
            let down = press_tick.saturating_add(40).saturating_add(k * 25);
            if ticks == down {
                emulator.handle_event(Event::Keydown(wie_backend::KeyCode::DOWN));
            }
            if ticks == down.saturating_add(10) {
                emulator.handle_event(Event::Keyup(wie_backend::KeyCode::DOWN));
            }
        }

        // An optional second probe key, so a "scroll then confirm" sequence
        // can be exercised (WIE_KEY2 pressed at WIE_PRESS_TICK2).
        if let Some(key2) = std::env::var("WIE_KEY2").ok().and_then(|n| key_by_name(&n)) {
            let t2: u32 = std::env::var("WIE_PRESS_TICK2").ok().and_then(|x| x.parse().ok()).unwrap_or(u32::MAX);
            if ticks == t2 {
                eprintln!("[{label}] pressing probe key2 {:?} at tick {ticks}", key2);
                emulator.handle_event(Event::Keydown(key2));
            }
            if ticks == t2.saturating_add(20) {
                emulator.handle_event(Event::Keyup(key2));
            }
        }

        // Report the exact last frame periodically so an advance to a simpler
        // screen is visible even when the busiest-frame heuristic would hide it.
        if ticks % 500 == 0 {
            let c = screen.captured.lock().unwrap();
            eprintln!("[{label}] tick {ticks}: last frame sig={:016x} colors={}", c.last_sig, c.last_colors);
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

    if let Ok(path) = std::env::var("WIE_LAST_PPM") {
        if !captured.last_pixels.is_empty() {
            let mut ppm = format!("P6\n{} {}\n255\n", captured.width, captured.height).into_bytes();
            ppm.extend_from_slice(&captured.last_pixels);
            let _ = std::fs::write(&path, ppm);
            eprintln!("[{label}] wrote LAST frame (sig={:016x}) to {path}", captured.last_sig);
        }
    }

    if let Ok(path) = std::env::var("LOM_CAPTURE_PATH") {
        if !captured.best_pixels.is_empty() {
            let mut ppm = format!("P6\n{} {}\n255\n", captured.width, captured.height).into_bytes();
            ppm.extend_from_slice(&captured.best_pixels);

            match std::fs::write(&path, ppm) {
                Ok(()) => eprintln!("[{label}] wrote busiest frame {} to {path}", captured.best_frame),
                Err(error) => {
                    eprintln!("[{label}] failed to write busiest frame to {path}: {error}")
                }
            }
        } else {
            eprintln!("[{label}] no captured pixels to write");
        }
    }
}

/// Reports what an application actually puts on screen, which is not
/// something a pass/fail test can capture while LGT support is still being
/// built out. Run it with `cargo test -p wie_lgt --test screen_capture --
/// --ignored --nocapture`.
#[test]
#[ignore = "diagnostic"]
fn capture_legend_of_master() {
    run("LoM", include_bytes!("../../test_games/legend_of_master.zip"), 48000);
}

/// Drives a title through a scripted key sequence, so a screen deep in the
/// game (a menu, the tutorial, an inventory) can be reached and captured.
///
/// `WIE_SCRIPT` is a comma-separated list of `tick:KEY` presses, e.g.
/// `300:OK,5200:OK,39000:OK`; each holds the key for 20 ticks. `WIE_TICKS`
/// caps the run (default 60000). `WIE_SHOT_DIR`, when set, gets a `step_<n>.ppm`
/// written ~300 ticks after each press so every step's screen is visible, plus
/// `final.ppm` at the end.
fn run_scripted(label: &str, archive: &[u8], ticks_limit: u32, script: &[(u32, wie_backend::KeyCode)]) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let screen = CaptureScreen::default();

    let platform = Box::new(CapturePlatform {
        inner: TestPlatform::with_event_handler(move |event| match event {
            TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
            TestPlatformEvent::OpenUrl(url) => eprintln!("[open-url] {url}"),
            TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
        }),
        screen: screen.clone(),
        clock: Arc::new(AtomicU64::new(0)),
    });

    let files = extract_zip(archive).expect("archive");
    let options = Options {
        enable_gdbserver: false,
        profile: None,
    };
    let mut emulator = if wie_lgt::LgtEmulator::loadable_archive(&files) {
        wie_lgt::LgtEmulator::from_archive(platform, files, options)
    } else {
        wie_lgt::LgtEmulator::from_jar(platform, label, archive.to_vec(), label, label, None, options)
    }
    .expect("load");

    let shot_dir = std::env::var("WIE_SHOT_DIR").ok();
    let write_ppm = |path: &str, screen: &CaptureScreen| {
        let c = screen.captured.lock().unwrap();
        if c.last_pixels.is_empty() {
            return;
        }
        let mut ppm = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
        ppm.extend_from_slice(&c.last_pixels);
        let _ = std::fs::write(path, ppm);
    };

    let mut ticks = 0u32;
    while !exited.load(Ordering::SeqCst) && ticks < ticks_limit {
        if ticks % 40 == 0 {
            emulator.handle_event(Event::Redraw);
        }

        for (step, &(at, key)) in script.iter().enumerate() {
            if ticks == at {
                eprintln!("[{label}] step {step}: press {key:?} at tick {ticks}");
                emulator.handle_event(Event::Keydown(key));
            }
            if ticks == at + 20 {
                emulator.handle_event(Event::Keyup(key));
            }
            if let Some(dir) = &shot_dir {
                if ticks == at + 300 {
                    write_ppm(&format!("{dir}/step_{step}.ppm"), &screen);
                    eprintln!("[{label}] step {step}: shot at tick {ticks}");
                }
            }
        }

        if let Err(error) = emulator.tick() {
            eprintln!("[{label}] stopped after {ticks} ticks: {error}");
            break;
        }
        ticks += 1;
    }

    if let Some(dir) = &shot_dir {
        write_ppm(&format!("{dir}/final.ppm"), &screen);
    }
    if let Ok(path) = std::env::var("WIE_LAST_PPM") {
        write_ppm(&path, &screen);
    }
    let c = screen.captured.lock().unwrap();
    eprintln!("[{label}] {ticks} ticks, {} frames, last sig={:016x}", c.frames, c.last_sig);
}

/// Drives LoM with a scripted key sequence from `WIE_SCRIPT` (`tick:KEY,...`),
/// so the tutorial and inventory can be reached from a test. Diagnostic.
#[test]
#[ignore = "diagnostic"]
fn capture_lom_scripted() {
    let script: Vec<(u32, wie_backend::KeyCode)> = std::env::var("WIE_SCRIPT")
        .expect("set WIE_SCRIPT=tick:KEY,tick:KEY,...")
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|pair| {
            let (tick, key) = pair.split_once(':').expect("tick:KEY");
            (tick.trim().parse().expect("tick"), key_by_name(key.trim()).expect("key name"))
        })
        .collect();
    let ticks: u32 = std::env::var("WIE_TICKS").ok().and_then(|x| x.parse().ok()).unwrap_or(60000);
    run_scripted("LoM", include_bytes!("../../test_games/legend_of_master.zip"), ticks, &script);
}

/// Runs every archive under `$WIE_ARCHIVES`, which can be a directory or a
/// list of paths separated by `:`.
///
/// Retail archives are not in the repository, so the one title that is has its
/// own test above and this one does nothing without the variable. Comparing a
/// batch is how a change that helps one title and breaks four gets noticed.
#[test]
#[ignore = "diagnostic"]
fn capture_archives() {
    let Ok(paths) = std::env::var("WIE_ARCHIVES") else {
        eprintln!("Set WIE_ARCHIVES to a directory or a ':' separated list of archives");
        return;
    };

    let ticks = std::env::var("WIE_TICKS").ok().and_then(|x| x.parse().ok()).unwrap_or(2000);

    let mut archives = Vec::new();
    for path in paths.split(':').filter(|x| !x.is_empty()).map(std::path::PathBuf::from) {
        if path.is_dir() {
            let mut entries = std::fs::read_dir(&path)
                .unwrap()
                .filter_map(|x| x.ok().map(|x| x.path()))
                .filter(|x| x.is_file())
                .collect::<Vec<_>>();
            entries.sort();
            archives.extend(entries);
        } else {
            archives.push(path);
        }
    }

    for archive in archives {
        let label = archive.file_name().unwrap_or_default().to_string_lossy().into_owned();

        // One archive that stops the runtime must not take the batch with it.
        let data = std::fs::read(&archive).unwrap();
        let result = std::panic::catch_unwind(|| run(&label, &data, ticks));

        if result.is_err() {
            eprintln!("[{label}] panicked");
        }
    }
}
