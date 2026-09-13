use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use test_utils::{TestPlatform, TestPlatformEvent, TestPlatformState};
use wie_backend::{
    AudioSink, DatabaseRepository, Emulator, Event, Filesystem, Instant, Network, NetworkError, NetworkPoll, Options, Platform, Screen,
    canvas::Image, extract_zip,
};
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
    /// The panel the archive under capture names for itself, when it names one.
    native_size: Option<(u32, u32)>,
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

    /// The LCD the title is told it has: the panel the archive names for itself
    /// where it names one, and 240x320 otherwise - the same choice the player
    /// app makes, so a capture shows what a player would see.
    ///
    /// `WIE_SCR_W`/`WIE_SCR_H` override both so a layout that a title derives
    /// from the screen size can be swept: how a splash or a HUD element moves as
    /// the reported size changes says whether the title centred it, anchored it,
    /// or placed it at a fixed offset.
    fn width(&self) -> u32 {
        std::env::var("WIE_SCR_W")
            .ok()
            .and_then(|x| x.parse().ok())
            .unwrap_or_else(|| self.native_size.map_or(240, |(width, _)| width))
    }

    fn height(&self) -> u32 {
        std::env::var("WIE_SCR_H")
            .ok()
            .and_then(|x| x.parse().ok())
            .unwrap_or_else(|| self.native_size.map_or(320, |(_, height)| height))
    }
}

struct CapturePlatform {
    inner: TestPlatform,
    screen: CaptureScreen,
    clock: Arc<AtomicU64>,
    network: CaptureNetwork,
}

/// A network that hands out sockets and refuses every operation on them.
///
/// The runtime answers some connections in process - a local endpoint, or the
/// LGT billing gateway - but it still asks the platform for the socket those
/// connections are carried on, and `Platform::network` is `None` by default.
/// A capture over that default never reaches the in-process paths at all:
/// 던파귀검사편 asks for its billing socket, `MC_netSocketConnect` answers
/// `M_E_NOTCONN` before either path is consulted, and the title puts up
/// `현재 서버에 접속할 수 없습니다` without a byte having been written.
///
/// So this hands out descriptors and nothing else. Everything that would reach
/// a host fails, which is what a capture wants: what it records came from the
/// answer this run gives rather than from somewhere off the machine.
#[derive(Default)]
struct CaptureNetwork {
    next: AtomicU64,
}

impl Network for CaptureNetwork {
    fn socket(&self, _family: i32, _socket_type: i32) -> std::result::Result<i32, NetworkError> {
        Ok(self.next.fetch_add(1, Ordering::SeqCst) as i32 + 1)
    }

    fn connect(&self, _socket: i32, _address: u32, _port: u16) -> NetworkPoll<()> {
        NetworkPoll::Ready(Err(NetworkError::HostUnreachable))
    }

    fn bind(&self, _socket: i32, _address: u32, _port: u16) -> std::result::Result<(), NetworkError> {
        Err(NetworkError::Unsupported)
    }

    fn read(&self, _socket: i32, _buf: &mut [u8]) -> std::result::Result<usize, NetworkError> {
        Err(NetworkError::NotConnected)
    }

    fn write(&self, _socket: i32, _buf: &[u8]) -> std::result::Result<usize, NetworkError> {
        Err(NetworkError::NotConnected)
    }

    fn send_to(&self, _socket: i32, _buf: &[u8], _address: u32, _port: u16) -> std::result::Result<usize, NetworkError> {
        Err(NetworkError::NotConnected)
    }

    fn recv_from(&self, _socket: i32, _buf: &mut [u8]) -> std::result::Result<(usize, u32, u16), NetworkError> {
        Err(NetworkError::NotConnected)
    }

    fn close(&self, _socket: i32) -> std::result::Result<(), NetworkError> {
        Ok(())
    }

    fn resolve_host(&self, _host: &str, _query_id: u32) {}

    fn poll_event(&self) -> Option<wie_backend::NetworkEvent> {
        None
    }
}

impl Platform for CapturePlatform {
    /// `WIE_LOCAL_NET_ACK` answers a title's requests in process rather than
    /// letting the connection fail: `1` for every connection it opens, or
    /// `host:port` for one, then any of `len=`, `type=`, `status=`, `prefix=`
    /// to describe its framing. See `wie_backend::AckEndpoint`.
    ///
    /// `WIE_LOCAL_NET_CAPTURE` takes the connection the same way but records
    /// what the title sends instead of answering it, which is how a protocol
    /// gets read in the first place. A recorded connection never answers, so a
    /// title waiting on a reply waits.
    fn local_endpoints(&self) -> Vec<Box<dyn wie_backend::LocalEndpoint>> {
        let mut endpoints: Vec<Box<dyn wie_backend::LocalEndpoint>> = Vec::new();

        // The approving endpoint first: a run that sets both wants its requests
        // answered, with the recorder behind it for whatever it does not take.
        let ack = std::env::var("WIE_LOCAL_NET_ACK").ok();
        if let Some(endpoint) = wie_backend::AckEndpoint::from_setting(ack.as_deref()) {
            endpoints.push(Box::new(endpoint));
        }

        let capture = std::env::var("WIE_LOCAL_NET_CAPTURE").ok();
        if let Some(endpoint) = wie_backend::CaptureEndpoint::from_setting(capture.as_deref()) {
            endpoints.push(Box::new(endpoint));
        }

        endpoints
    }

    fn screen(&self) -> &dyn Screen {
        &self.screen
    }

    fn network(&self) -> Option<&dyn Network> {
        Some(&self.network)
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
        // Milliseconds added per read. The default of 1 ties emulated time to
        // work done (see above). A title with a real-time splash/pause loop
        // (poll currentTimeMillis + sleep) needs emulated time to advance faster
        // than one ms per read or it never elapses; set WIE_CLOCK_MS to test
        // whether such a title is genuinely stuck or just clock-limited here.
        static STEP: std::sync::LazyLock<u64> = std::sync::LazyLock::new(|| {
            std::env::var("WIE_CLOCK_MS")
                .ok()
                .and_then(|x| x.parse().ok())
                .filter(|&x| x >= 1)
                .unwrap_or(1)
        });
        Instant::from_epoch_millis(self.clock.fetch_add(*STEP, Ordering::SeqCst))
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

/// A sampling profile writer for `WIE_PROFILE_OUT`, in the flamegraph-folded
/// form `wie_cli` writes: one line per sample, the call stack outermost-first.
/// Diagnostic - it is what says which compiled routine a stuck title is looping
/// in, which nothing else in the log does.
fn profile_from_env() -> Option<wie_backend::ProfileCallback> {
    let path = std::env::var("WIE_PROFILE_OUT").ok()?;
    let file = std::fs::File::create(path).expect("profile output");
    let writer = std::sync::Mutex::new(std::io::BufWriter::new(file));

    Some(Box::new(move |batch: Vec<wie_backend::ProfileSample>| {
        use std::io::Write;

        let mut writer = writer.lock().unwrap();
        for sample in batch {
            let folded: Vec<String> = sample.stack.iter().rev().map(|pc| format!("{pc:#x}")).collect();
            let _ = writeln!(writer, "{} {}", folded.join(";"), sample.count);
        }
        let _ = writer.flush();
    }))
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
    let screen = CaptureScreen {
        native_size: wie_lgt::LgtEmulator::screen_size(archive),
        ..Default::default()
    };

    let platform = Box::new(CapturePlatform {
        inner: TestPlatform::with_event_handler(move |event| match event {
            TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
            TestPlatformEvent::OpenUrl(url) => eprintln!("[open-url] {url}"),
            TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
        }),
        screen: screen.clone(),
        clock: Arc::new(AtomicU64::new(0)),
        network: CaptureNetwork::default(),
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
        profile: profile_from_env(),
        // `WIE_ANNUN` forces the handset's status strip on (`0` forces it off),
        // so a title can be captured under either state rather than the one its
        // aid selects.
        annunciator: std::env::var("WIE_ANNUN").ok().map(|x| x != "0"),
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

        // Every `WIE_FDUMP_EVERY` ticks, the exact frame as a PPM under
        // `WIE_FDUMP_DIR`. The single last/busiest frame catches one screen;
        // a title that walks through several (a splash sequence, an intro,
        // then a menu) needs the whole run to compare each against a
        // reference capture.
        if let Ok(dir) = std::env::var("WIE_FDUMP_DIR") {
            let every: u32 = std::env::var("WIE_FDUMP_EVERY").ok().and_then(|x| x.parse().ok()).unwrap_or(25);
            if ticks % every == 0 {
                let c = screen.captured.lock().unwrap();
                if !c.last_pixels.is_empty() {
                    let mut ppm = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
                    ppm.extend_from_slice(&c.last_pixels);
                    let _ = std::fs::write(format!("{dir}/t{ticks:06}.ppm"), ppm);
                }
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
    run_scripted_over(label, archive, ticks_limit, script, saved_state());
}

/// The same run, over storage that may already hold what an earlier run wrote,
/// and handing that storage back for the run after this one.
fn run_scripted_over(
    label: &str,
    archive: &[u8],
    ticks_limit: u32,
    script: &[(u32, wie_backend::KeyCode)],
    state: TestPlatformState,
) -> TestPlatformState {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let screen = CaptureScreen {
        native_size: wie_lgt::LgtEmulator::screen_size(archive),
        ..Default::default()
    };

    let inner = TestPlatform::with_state_and_event_handler(state, move |event| match event {
        TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
        TestPlatformEvent::OpenUrl(url) => eprintln!("[open-url] {url}"),
        TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
    });
    let state = inner.state();

    let platform = Box::new(CapturePlatform {
        inner,
        screen: screen.clone(),
        clock: Arc::new(AtomicU64::new(0)),
        network: CaptureNetwork::default(),
    });

    let files = extract_zip(archive).expect("archive");
    let options = Options {
        enable_gdbserver: false,
        profile: profile_from_env(),
        // `WIE_ANNUN` forces the handset's status strip on (`0` forces it off),
        // so a title can be captured under either state rather than the one its
        // aid selects.
        annunciator: std::env::var("WIE_ANNUN").ok().map(|x| x != "0"),
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

    // The same periodic frame dump `run` takes: a scripted walk is exactly the
    // case where the screens between the key presses are what needs comparing.
    let frame_dump = std::env::var("WIE_FDUMP_DIR").ok();
    let frame_dump_every: u32 = std::env::var("WIE_FDUMP_EVERY").ok().and_then(|x| x.parse().ok()).unwrap_or(25);

    // How long each scripted press is held. A title that polls a key-state flag
    // once a game loop rather than acting on the event can miss a press shorter
    // than its own loop, so the hold has to be settable.
    let hold: u32 = std::env::var("WIE_HOLD").ok().and_then(|x| x.parse().ok()).unwrap_or(20);

    let mut ticks = 0u32;
    while !exited.load(Ordering::SeqCst) && ticks < ticks_limit {
        if ticks % 40 == 0 {
            emulator.handle_event(Event::Redraw);
        }

        if let Some(dir) = &frame_dump
            && ticks % frame_dump_every == 0
        {
            write_ppm(&format!("{dir}/t{ticks:06}.ppm"), &screen);
        }

        for (step, &(at, key)) in script.iter().enumerate() {
            if ticks == at {
                eprintln!("[{label}] step {step}: press {key:?} at tick {ticks}");
                emulator.handle_event(Event::Keydown(key));
            }
            if ticks == at + hold {
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

    state
}

/// The storage a capture starts over, seeded from `WIE_SAVE_ZIP` when one is
/// set and empty when none is.
///
/// A title only misbehaves somewhere, and driving it there from nothing costs
/// a script for every screen in between - 엑시온2's NPCs go missing after a
/// story sequence no key schedule is going to sit through. An exported save is
/// the state itself, so this reads one back in: the export lays a handset's
/// files out as `fs/<application id>/<path>` and its databases as
/// `db/<product id>/<name>/<record id>`, which is what the importer on the
/// handset reads too.
fn saved_state() -> TestPlatformState {
    let state = TestPlatformState::default();

    let Ok(path) = std::env::var("WIE_SAVE_ZIP") else {
        return state;
    };

    let save = std::fs::read(&path).expect("save archive");
    let entries = extract_zip(&save).expect("save archive contents");

    let mut files = 0;
    let mut records = 0;
    for (name, data) in entries {
        // An export that carries both kinds keeps its `fs`/`db` prefixes. One
        // that carries only files has `fs` as its single root directory, and
        // reading the archive strips a shared root, so those entries arrive
        // already inside it - hence the prefix being optional here. A record's
        // own name is the last component and is always a number, which is what
        // tells the two stripped shapes apart.
        let parts: Vec<&str> = name.split('/').collect();
        let (kind, rest) = match parts.as_slice() {
            ["fs", rest @ ..] => (Some(false), rest),
            ["db", rest @ ..] => (Some(true), rest),
            rest => (None, rest),
        };

        let record = kind.unwrap_or(rest.len() == 3 && rest[2].parse::<u32>().is_ok());

        match (record, rest) {
            (false, [aid, path @ ..]) if !path.is_empty() => {
                state.preload_file(aid, &path.join("/"), data);
                files += 1;
            }
            (true, [pid, name, id]) => {
                state.preload_record(pid, name, id.parse().expect("record id"), data);
                records += 1;
            }
            _ => eprintln!("[save] ignoring {name}"),
        }
    }

    eprintln!("[save] {files} files and {records} records from {path}");

    state
}

/// Parses `WIE_SCRIPT` (`tick:KEY,tick:KEY,...`) into the press schedule
/// `run_scripted` takes.
fn script_from_env() -> Vec<(u32, wie_backend::KeyCode)> {
    script_from_env_named("WIE_SCRIPT")
}

/// The same, out of whichever variable names the schedule.
fn script_from_env_named(name: &str) -> Vec<(u32, wie_backend::KeyCode)> {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("set {name}=tick:KEY,tick:KEY,..."))
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|pair| {
            let (tick, key) = pair.split_once(':').expect("tick:KEY");
            (tick.trim().parse().expect("tick"), key_by_name(key.trim()).expect("key name"))
        })
        .collect()
}

/// Drives the archive at `WIE_ARCHIVE` with the `WIE_SCRIPT` key sequence, so a
/// title that parks on a notice or a menu can be walked past from a test.
/// Diagnostic; retail archives are not in the repository.
#[test]
#[ignore = "diagnostic"]
fn capture_scripted_archive() {
    let Ok(path) = std::env::var("WIE_ARCHIVE") else {
        eprintln!("Set WIE_ARCHIVE to an archive and WIE_SCRIPT to tick:KEY,...");
        return;
    };
    let archive = std::fs::read(&path).expect("archive");
    let ticks: u32 = std::env::var("WIE_TICKS").ok().and_then(|x| x.parse().ok()).unwrap_or(60000);
    let label = std::path::Path::new(&path)
        .file_stem()
        .map_or("archive", |x| x.to_str().unwrap_or("archive"));

    run_scripted(label, &archive, ticks, &script_from_env());
}

/// Launches the archive at `WIE_ARCHIVE` twice over one handset's storage, so
/// a title that only gets going on its second run can be captured.
///
/// 던파귀검사편 is the case this exists for: its first run builds a cache, parks
/// on `속도 최적화를 위해 종료 후 재실행 해주시기 바랍니다`, and goes no
/// further; the run that finds that cache is the one that reaches `사용자 인증`
/// and opens its billing socket. `WIE_SCRIPT` drives the first launch and
/// `WIE_SCRIPT2` the second, and only the second launch's frames are dumped.
#[test]
#[ignore = "diagnostic"]
fn capture_scripted_archive_twice() {
    let Ok(path) = std::env::var("WIE_ARCHIVE") else {
        eprintln!("Set WIE_ARCHIVE to an archive, WIE_SCRIPT/WIE_SCRIPT2 to tick:KEY,... (WIE_RUNS for more than two)");
        return;
    };
    let archive = std::fs::read(&path).expect("archive");
    let ticks: u32 = std::env::var("WIE_TICKS").ok().and_then(|x| x.parse().ok()).unwrap_or(4000);
    let ticks2: u32 = std::env::var("WIE_TICKS2").ok().and_then(|x| x.parse().ok()).unwrap_or(ticks);
    let label = std::path::Path::new(&path)
        .file_stem()
        .map_or("archive", |x| x.to_str().unwrap_or("archive"));

    let first = script_from_env();
    let later = std::env::var("WIE_SCRIPT2").map_or_else(|_| first.clone(), |_| script_from_env_named("WIE_SCRIPT2"));

    // `WIE_RUNS` launches more than twice, which is how a title that should
    // only ask something once - 던파귀검사편's 사용자 인증 says it runs on the
    // first launch alone - is held to it.
    let runs: u32 = std::env::var("WIE_RUNS").ok().and_then(|x| x.parse().ok()).unwrap_or(2).max(2);

    // Only the last launch's frames are what this is for, so the dump
    // directories are kept back until it.
    let dump = std::env::var("WIE_FDUMP_DIR").ok();
    let shot = std::env::var("WIE_SHOT_DIR").ok();
    unsafe {
        std::env::remove_var("WIE_FDUMP_DIR");
        std::env::remove_var("WIE_SHOT_DIR");
    }

    let mut state = saved_state();
    for run in 1..=runs {
        if run == runs {
            unsafe {
                if let Some(dump) = &dump {
                    std::env::set_var("WIE_FDUMP_DIR", dump);
                }
                if let Some(shot) = &shot {
                    std::env::set_var("WIE_SHOT_DIR", shot);
                }
            }
        }

        let (ticks, script) = if run == 1 { (ticks, &first) } else { (ticks2, &later) };

        state = run_scripted_over(&format!("{label} run {run}"), &archive, ticks, script, state);
    }
}

/// Drives LoM with a scripted key sequence from `WIE_SCRIPT` (`tick:KEY,...`),
/// so the tutorial and inventory can be reached from a test. Diagnostic.
#[test]
#[ignore = "diagnostic"]
fn capture_lom_scripted() {
    let script = script_from_env();
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
