//! A headless probe for KTF archives.
//!
//! KTF had no way to run a real title without a window, so a title that dies on
//! its first frame could only be diagnosed by guesswork. This runs an archive
//! the way `wie_cli` would, counts the frames it paints, and reports the error
//! it stopped on.
//!
//! It is driven by the environment so any archive can be pointed at it without
//! touching the tree:
//!
//! - `WIE_KTF_ARCHIVE` - path to the archive to run. Unset, the probe is a
//!   no-op, which is what keeps it out of the way of an ordinary `cargo test`.
//! - `WIE_TICKS` - how many ticks to run (default 20000).
//! - `WIE_SHOT` - where to write the last painted frame, as a binary PPM.
//! - `WIE_KEY`/`WIE_PRESS_TICK` - one key press, to get past a title's notice.
//! - `WIE_SCRIPT` - a walk into the title instead: `tick:KEY` pairs separated
//!   by commas, e.g. `1500:OK,3000:OK,4500:NUM1`. Each press is held 20 ticks.
//! - `WIE_SHOT_DIR` - a frame written ~400 ticks after each scripted press, so
//!   every step of the walk is visible rather than only where it ended.
//! - `WIE_SCRIPT2` - launch the archive twice over one handset's storage,
//!   `WIE_SCRIPT` driving the first launch and this the second. A title that
//!   installs itself on its first run and asks to be started again needs this
//!   to be reachable at all: 드래곤하트 paints
//!   `게임이 설치 되었습니다. 다시 실행해 주세요.` and goes no further, whatever
//!   it is sent. Only the second launch is captured.
//! - `WIE_TICKS2` - the second launch's tick budget, when it needs a different
//!   one from the first (default: the same).

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use test_utils::{TestPlatform, TestPlatformEvent, TestPlatformState};
use wie_backend::{
    AudioSink, DatabaseRepository, Emulator, Event, Filesystem, Instant, KeyCode, Options, Platform, Screen, canvas::Image, extract_zip,
};
use wie_ktf::KtfEmulator;
use wie_util::Result;

#[derive(Default)]
struct Captured {
    frames: u32,
    width: u32,
    height: u32,
    /// How many distinct colours the last frame held - a blank screen is one.
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

        let mut colors = std::collections::BTreeSet::new();
        let mut pixels = Vec::with_capacity((image.width() * image.height() * 3) as usize);
        for color in image.colors() {
            colors.insert(((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32);
            pixels.push(color.r);
            pixels.push(color.g);
            pixels.push(color.b);
        }

        captured.last_colors = colors.len();
        captured.last_pixels = pixels;
    }

    fn width(&self) -> u32 {
        self.captured.lock().unwrap().width.max(240)
    }

    fn height(&self) -> u32 {
        self.captured.lock().unwrap().height.max(320)
    }
}

struct CapturePlatform {
    inner: TestPlatform,
    screen: CaptureScreen,
    /// A clock the probe advances itself: a title that waits on the wall clock
    /// never gets anywhere on a clock that does not move.
    clock: Arc<AtomicU64>,
}

impl Platform for CapturePlatform {
    fn screen(&self) -> &dyn Screen {
        &self.screen
    }

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

    fn system_information(&self, key: &str) -> Option<String> {
        self.inner.system_information(key)
    }

    fn open_url(&self, url: &str) -> bool {
        self.inner.open_url(url)
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

/// Maps a `WIE_KEY` name to a key code, so a probe can target any key.
fn key_by_name(name: &str) -> Option<KeyCode> {
    Some(match name.to_ascii_uppercase().as_str() {
        "OK" | "FIRE" => KeyCode::OK,
        "UP" => KeyCode::UP,
        "DOWN" => KeyCode::DOWN,
        "LEFT" => KeyCode::LEFT,
        "RIGHT" => KeyCode::RIGHT,
        "LSK" | "LEFT_SOFT_KEY" => KeyCode::LEFT_SOFT_KEY,
        "RSK" | "RIGHT_SOFT_KEY" => KeyCode::RIGHT_SOFT_KEY,
        "CLEAR" => KeyCode::CLEAR,
        "NUM0" => KeyCode::NUM0,
        "NUM1" => KeyCode::NUM1,
        "NUM2" => KeyCode::NUM2,
        "NUM3" => KeyCode::NUM3,
        "NUM4" => KeyCode::NUM4,
        "NUM5" => KeyCode::NUM5,
        "NUM6" => KeyCode::NUM6,
        "NUM7" => KeyCode::NUM7,
        "NUM8" => KeyCode::NUM8,
        "NUM9" => KeyCode::NUM9,
        "HASH" | "POUND" => KeyCode::HASH,
        "STAR" => KeyCode::STAR,
        _ => return None,
    })
}

/// Parses `WIE_SCRIPT`: `tick:KEY` pairs separated by commas.
fn parse_script(script: Option<&str>) -> Vec<(u32, KeyCode)> {
    let Some(script) = script else {
        return Vec::new();
    };

    script
        .split(',')
        .filter_map(|step| {
            let (tick, key) = step.trim().split_once(':')?;

            Some((tick.trim().parse().ok()?, key_by_name(key.trim())?))
        })
        .collect()
}

fn write_ppm(path: &str, screen: &CaptureScreen) {
    let captured = screen.captured.lock().unwrap();
    if captured.last_pixels.is_empty() {
        return;
    }

    let mut ppm = format!("P6\n{} {}\n255\n", captured.width, captured.height).into_bytes();
    ppm.extend_from_slice(&captured.last_pixels);
    let _ = std::fs::write(path, ppm);
}

#[test]
fn ktf_archive_probe() {
    let Ok(path) = std::env::var("WIE_KTF_ARCHIVE") else {
        eprintln!("WIE_KTF_ARCHIVE unset; nothing to probe");
        return;
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let ticks_limit: u32 = std::env::var("WIE_TICKS").ok().and_then(|x| x.parse().ok()).unwrap_or(20000);
    let archive = std::fs::read(&path).expect("archive");
    let files = extract_zip(&archive).expect("extract");
    eprintln!(
        "[probe] {path}: {} entries, loadable={}",
        files.len(),
        KtfEmulator::loadable_archive(&files)
    );

    let mut script = parse_script(std::env::var("WIE_SCRIPT").ok().as_deref());
    if let Some(key) = std::env::var("WIE_KEY").ok().and_then(|name| key_by_name(&name))
        && let Some(tick) = std::env::var("WIE_PRESS_TICK").ok().and_then(|x| x.parse().ok())
    {
        script.push((tick, key));
    }
    script.sort_by_key(|(tick, _)| *tick);

    // One handset's storage, so a title that installs itself on its first run
    // finds what it wrote when it is started again.
    let state = TestPlatformState::default();
    let second = std::env::var("WIE_SCRIPT2").ok();

    if let Some(second) = second {
        eprintln!("[probe] first launch");
        run_once(&files, &state, &script, ticks_limit, None, None);

        let mut second_script = parse_script(Some(second.as_str()));
        second_script.sort_by_key(|(tick, _)| *tick);
        let second_ticks: u32 = std::env::var("WIE_TICKS2").ok().and_then(|x| x.parse().ok()).unwrap_or(ticks_limit);

        eprintln!("[probe] second launch");
        run_once(
            &files,
            &state,
            &second_script,
            second_ticks,
            std::env::var("WIE_SHOT_DIR").ok().as_deref(),
            std::env::var("WIE_SHOT").ok().as_deref(),
        );

        return;
    }

    run_once(
        &files,
        &state,
        &script,
        ticks_limit,
        std::env::var("WIE_SHOT_DIR").ok().as_deref(),
        std::env::var("WIE_SHOT").ok().as_deref(),
    );
}

/// Runs the archive once over `state`, which is the handset's storage and
/// outlives the launch.
fn run_once(
    files: &BTreeMap<String, Vec<u8>>,
    state: &TestPlatformState,
    script: &[(u32, KeyCode)],
    ticks_limit: u32,
    shot_dir: Option<&str>,
    shot: Option<&str>,
) {
    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let screen = CaptureScreen::default();

    let platform = Box::new(CapturePlatform {
        inner: TestPlatform::with_state_and_event_handler(state.clone(), move |event| match event {
            TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
            TestPlatformEvent::OpenUrl(url) => eprintln!("[open-url] {url}"),
            TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
        }),
        screen: screen.clone(),
        clock: Arc::new(AtomicU64::new(0)),
    });

    let options = Options {
        enable_gdbserver: false,
        profile: None,
        annunciator: None,
    };

    let mut emulator = match KtfEmulator::from_archive(platform, files.clone(), options) {
        Ok(emulator) => emulator,
        Err(error) => {
            eprintln!("[probe] LOAD FAILED: {error:?}");
            return;
        }
    };

    // `request_redraw` only asks; the host is what paints. `wie_cli` turns the
    // request into a window redraw, so a probe that never feeds one back sees
    // a title paint nothing however well it runs.
    let mut ticks = 0;
    let mut stopped = None;
    while ticks < ticks_limit && !exited.load(Ordering::SeqCst) {
        if ticks % 40 == 0 {
            emulator.handle_event(Event::Redraw);
        }
        for (step, (tick, key)) in script.iter().enumerate() {
            if ticks == *tick {
                eprintln!("[probe] step {step}: pressing {key:?} at tick {ticks}");
                emulator.handle_event(Event::Keydown(*key));
            }
            if ticks == tick.saturating_add(20) {
                emulator.handle_event(Event::Keyup(*key));
            }
            // Far enough past the press that the screen it opened has settled.
            if let Some(dir) = shot_dir
                && ticks == tick.saturating_add(400)
            {
                write_ppm(&format!("{dir}/step_{step}.ppm"), &screen);
            }
        }

        if let Err(error) = emulator.tick() {
            stopped = Some(error);
            break;
        }
        ticks += 1;
    }

    let captured = screen.captured.lock().unwrap();
    eprintln!(
        "[probe] ticks={ticks} frames={} size={}x{} colors_in_last_frame={} exited={}",
        captured.frames,
        captured.width,
        captured.height,
        captured.last_colors,
        exited.load(Ordering::SeqCst),
    );
    match &stopped {
        Some(error) => eprintln!("[probe] STOPPED: {error:?}"),
        None => eprintln!("[probe] ran to the tick limit without stopping"),
    }

    if let Some(shot) = shot
        && !captured.last_pixels.is_empty()
    {
        let mut ppm = format!("P6\n{} {}\n255\n", captured.width, captured.height).into_bytes();
        ppm.extend_from_slice(&captured.last_pixels);
        std::fs::write(shot, ppm).expect("shot");
        eprintln!("[probe] wrote {shot}");
    }

    // Keep the probe from being mistaken for a passing assertion.
    let _ = Duration::from_secs(0);
}
