use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use test_utils::{TestPlatform, TestPlatformEvent};
use wie_backend::{
    AudioSink, DatabaseRepository, Emulator, Event, Filesystem, Instant, KeyCode, Options, Platform, Screen, canvas::Image, extract_zip,
};

/// Ticks between one key press and the next.
const KEY_PERIOD: u32 = 200;
use wie_util::Result;

#[derive(Default)]
struct Captured {
    frames: u32,
    best_frame: u32,
    width: u32,
    height: u32,
    colors: BTreeMap<u32, u32>,
    /// The busiest frame's pixels, kept only when somewhere to write them was
    /// asked for.
    pixels: Vec<u8>,
    /// The last frame's, which is where the title got to rather than the best
    /// it managed.
    last_pixels: Vec<u8>,
}

/// Counts what an application asks the audio sink for.
///
/// A `.mmf` carries both PCM waves and a MIDI-like sequence, and only the
/// waves reach a device: `AndroidAudioSink` drops every MIDI event because
/// there is no synth on the other side. So which of the two a title's music
/// is decides whether it makes a sound at all.
#[derive(Default)]
struct AudioTally {
    waves: u32,
    wave_samples: u64,
    notes: u32,
    other_midi: u32,
}

#[derive(Default, Clone)]
struct CaptureAudio {
    tally: Arc<Mutex<AudioTally>>,
}

impl AudioSink for CaptureAudio {
    fn play_wave(&self, _channel: u8, _sampling_rate: u32, wave_data: &[i16]) {
        let mut tally = self.tally.lock().unwrap();
        tally.waves += 1;
        tally.wave_samples += wave_data.len() as u64;
    }

    fn midi_note_on(&self, _channel: u8, _note: u8, _velocity: u8) {
        self.tally.lock().unwrap().notes += 1;
    }

    fn midi_note_off(&self, _channel: u8, _note: u8, _velocity: u8) {}

    fn midi_program_change(&self, _channel: u8, _program: u8) {
        self.tally.lock().unwrap().other_midi += 1;
    }

    fn midi_control_change(&self, _channel: u8, _control: u8, _value: u8) {
        self.tally.lock().unwrap().other_midi += 1;
    }

    fn midi_pitch_bend(&self, _channel: u8, _value: u16) {
        self.tally.lock().unwrap().other_midi += 1;
    }

    fn midi_sysex(&self, _data: &[u8]) {
        self.tally.lock().unwrap().other_midi += 1;
    }
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

            if std::env::var_os("WIE_CAPTURE_DIR").is_some() {
                captured.pixels = image.colors().into_iter().flat_map(|color| [color.r, color.g, color.b]).collect();
            }
        }

        if std::env::var_os("WIE_CAPTURE_DIR").is_some() {
            captured.last_pixels = image.colors().into_iter().flat_map(|color| [color.r, color.g, color.b]).collect();
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
    audio: CaptureAudio,
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
        Box::new(self.audio.clone())
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
    // Diagnosing a blank screen means reading the runtime's own log, so honour
    // `RUST_LOG` here the way `wie_cli` does.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let screen = CaptureScreen::default();
    let audio = CaptureAudio::default();

    let platform = Box::new(CapturePlatform {
        inner: TestPlatform::with_event_handler(move |event| match event {
            TestPlatformEvent::Stdout(buf) => eprint!("[stdout] {}", String::from_utf8_lossy(&buf)),
            TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
        }),
        screen: screen.clone(),
        audio: audio.clone(),
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

    let keys = match std::env::var("WIE_KEYS") {
        Ok(names) => names.split(',').filter(|x| !x.is_empty()).map(KeyCode::parse).collect::<Vec<_>>(),
        Err(_) => vec![KeyCode::OK, KeyCode::NUM1, KeyCode::LEFT_SOFT_KEY, KeyCode::NUM5],
    };

    let mut ticks = 0;
    while !exited.load(Ordering::SeqCst) && ticks < ticks_limit {
        if ticks % 40 == 0 {
            emulator.handle_event(Event::Redraw);
        }
        // Titles wait on input, and not always the same key: Legend of Master's
        // first screen ends with "press any key", and SEED 2 opens on a consent
        // screen that wants `1`. Nothing here knows which, so they take turns.
        //
        // `$WIE_KEYS` narrows it to one when a title is being looked at on its
        // own: `WIE_KEYS=NUM1` answers a consent screen and nothing else.
        let key = keys[(ticks / KEY_PERIOD) as usize % keys.len()];

        if ticks % KEY_PERIOD == KEY_PERIOD / 2 {
            emulator.handle_event(Event::Keydown(key));
        }
        if ticks % KEY_PERIOD == KEY_PERIOD / 2 + 20 {
            emulator.handle_event(Event::Keyup(key));
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

    // A histogram says a title draws; only the picture says what it drew, and
    // only the last frame says whether it got anywhere.
    if let Some(directory) = std::env::var_os("WIE_CAPTURE_DIR") {
        for (suffix, pixels) in [("", &captured.pixels), ("-final", &captured.last_pixels)] {
            if pixels.is_empty() {
                continue;
            }

            let path = std::path::Path::new(&directory).join(format!("{label}{suffix}.ppm"));
            let header = format!("P6\n{} {}\n255\n", captured.width, captured.height);

            let mut file = header.into_bytes();
            file.extend_from_slice(pixels);

            match std::fs::write(&path, file) {
                Ok(()) => eprintln!("[{label}] wrote {}", path.display()),
                Err(error) => eprintln!("[{label}] could not write {}: {error}", path.display()),
            }
        }
    }

    let tally = audio.tally.lock().unwrap();
    eprintln!(
        "[{label}] audio: {} waves ({} samples), {} notes, {} other midi",
        tally.waves, tally.wave_samples, tally.notes, tally.other_midi
    );
    drop(tally);

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
