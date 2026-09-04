use std::{
    collections::VecDeque,
    fmt::Write as _,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use wie_backend::{Emulator, Event, KeyCode, Options, extract_zip};
use wie_j2me::J2MEEmulator;
use wie_ktf::KtfEmulator;
use wie_lgt::LgtEmulator;
use wie_skt::SktEmulator;

use crate::platform::{AndroidHandsetInformation, AndroidPlatform, Frame, Shared};

/// Feature phone LCD the WIPI/MIDP APIs are written against. Games query this
/// through `getDisplayInfo`, so it has to stay fixed rather than follow the
/// Android display; `GameView` letterboxes it to whatever the panel is.
pub const SCREEN_WIDTH: u32 = 240;
pub const SCREEN_HEIGHT: u32 = 320;

/// The LGT WIPI platform hands its clets a taller 240x400 LCD, not the 240x320
/// the other stacks use. The reference player's `FrameSurfaceView.initBitmap`
/// allocates a 240x400 framebuffer for every portrait WIPI title, and the clet
/// sizes its whole scene from what `MC_grpGetDisplayInfo` reports - so a 320
/// panel squeezes the bottom HUD/menu (bars overlap, the level label falls off
/// the edge). Reporting 400 lays the title out exactly as the device does; the
/// on-screen keys and `GameView` letterbox absorb the extra height.
pub const LGT_SCREEN_HEIGHT: u32 = 400;

/// Whether `data` is a title `build_emulator` would hand to `LgtEmulator`, so
/// the LCD can be sized before the platform (and thus the screen) is built. The
/// checks mirror `build_emulator`'s order exactly: an earlier-matching stack
/// wins, and the bare-jar formats are only reached once every archive format is
/// ruled out.
fn is_lgt_title(data: &[u8]) -> bool {
    if let Ok(files) = extract_zip(data) {
        if KtfEmulator::loadable_archive(&files) {
            return false;
        }
        if LgtEmulator::loadable_archive(&files) {
            return true;
        }
        if SktEmulator::loadable_archive(&files) {
            return false;
        }
    }
    if KtfEmulator::loadable_jar(data) {
        return false;
    }
    LgtEmulator::loadable_jar(data)
}

/// Key indexes as laid out by `MainActivity`'s keypad and D-pad.
fn key_code(index: i32) -> Option<KeyCode> {
    Some(match index {
        0 => KeyCode::UP,
        1 => KeyCode::DOWN,
        2 => KeyCode::LEFT,
        3 => KeyCode::RIGHT,
        4 => KeyCode::OK,
        5 => KeyCode::LEFT_SOFT_KEY,
        6 => KeyCode::RIGHT_SOFT_KEY,
        7 => KeyCode::CLEAR,
        8 => KeyCode::NUM0,
        9 => KeyCode::NUM1,
        10 => KeyCode::NUM2,
        11 => KeyCode::NUM3,
        12 => KeyCode::NUM4,
        13 => KeyCode::NUM5,
        14 => KeyCode::NUM6,
        15 => KeyCode::NUM7,
        16 => KeyCode::NUM8,
        17 => KeyCode::NUM9,
        18 => KeyCode::STAR,
        19 => KeyCode::HASH,
        20 => KeyCode::CALL,
        21 => KeyCode::HANGUP,
        _ => return None,
    })
}

/// Java has no name to give us; `nativeStart` only receives the archive bytes,
/// so the app id is derived from the content. It has to be stable across
/// launches or the game loses its save data, and distinct per game or two
/// games would share one.
fn content_id(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

struct Instance {
    emulator: Box<dyn Emulator + Send>,
    shared: Shared,
    /// Input arrives on the UI thread and is drained by the emulator thread,
    /// so a touch never blocks behind a tick.
    pending_input: VecDeque<Event>,
}

pub struct Runner {
    instance: Option<Instance>,
    last_error: String,
}

static RUNNER: Mutex<Runner> = Mutex::new(Runner {
    instance: None,
    last_error: String::new(),
});

pub fn with_runner<T>(f: impl FnOnce(&mut Runner) -> T) -> T {
    let mut runner = RUNNER.lock().unwrap_or_else(|x| x.into_inner());

    f(&mut runner)
}

impl Runner {
    /// Loads `data` and spawns the emulator. Returns the error message to show
    /// in the player, or an empty string on success.
    pub fn start(&mut self, data: Vec<u8>, runtime_dir: PathBuf, handset_information: AndroidHandsetInformation) -> String {
        self.stop();

        let shared = Shared::default();
        let screen_height = if is_lgt_title(&data) { LGT_SCREEN_HEIGHT } else { SCREEN_HEIGHT };
        let platform = Box::new(AndroidPlatform::new(
            runtime_dir,
            SCREEN_WIDTH,
            screen_height,
            shared.clone(),
            handset_information,
        ));

        let options = Options {
            enable_gdbserver: false,
            profile: None,
        };

        match build_emulator(platform, &data, options) {
            Ok(emulator) => {
                self.instance = Some(Instance {
                    emulator,
                    shared,
                    pending_input: VecDeque::new(),
                });
                self.last_error.clear();

                String::new()
            }
            Err(error) => {
                self.last_error = error.clone();

                error
            }
        }
    }

    pub fn stop(&mut self) {
        // Otherwise whatever the sequence was holding goes on sounding after
        // the game it belongs to has gone.
        if let Some(instance) = self.instance.as_ref() {
            instance.shared.mixer().silence();
        }

        self.instance = None;
    }

    pub fn is_running(&self) -> bool {
        self.instance.is_some()
    }

    pub fn last_error(&self) -> String {
        self.last_error.clone()
    }

    /// Runs the emulator for up to `budget`. Returns a status line for the
    /// player, empty while everything is fine.
    pub fn tick(&mut self, budget: Duration) -> String {
        let Some(instance) = self.instance.as_mut() else {
            return String::new();
        };

        for event in instance.pending_input.drain(..) {
            instance.emulator.handle_event(event);
        }

        if instance.shared.take_redraw_request() {
            instance.emulator.handle_event(Event::Redraw);
        }

        // The synthesiser has to keep producing between the bursts a sequence
        // arrives in, so it is pumped once a tick rather than from the sink.
        instance.shared.render_synth();

        let deadline = Instant::now() + budget;
        loop {
            if let Err(error) = instance.emulator.tick() {
                let message = error.to_string();
                tracing::error!("Emulator stopped: {message}");

                self.last_error = message.clone();
                instance.shared.mixer().silence();
                self.instance = None;

                return message;
            }

            if instance.shared.has_exited() {
                tracing::info!("Application exited");

                self.last_error.clear();
                self.instance = None;

                return String::new();
            }

            // Nothing runnable until a timer fires: stop rather than busy-wait
            // the rest of the budget. The host's inter-tick delay then acts as a
            // real sleep. A CPU-bound title never reports idle, so it keeps
            // running to the full budget — which is what lifts its duty cycle
            // once the host delay is short.
            if instance.emulator.is_idle() {
                return String::new();
            }

            if Instant::now() >= deadline {
                return String::new();
            }
        }
    }

    pub fn key(&mut self, index: i32, pressed: bool) {
        let Some(instance) = self.instance.as_mut() else {
            return;
        };
        let Some(key_code) = key_code(index) else {
            tracing::warn!("Unknown key index {index}");
            return;
        };

        instance
            .pending_input
            .push_back(if pressed { Event::Keydown(key_code) } else { Event::Keyup(key_code) });
    }

    pub fn take_frame(&mut self) -> Option<Frame> {
        self.instance.as_ref()?.shared.take_frame()
    }

    pub fn take_audio(&mut self) -> Option<Vec<u8>> {
        self.instance.as_ref()?.shared.take_audio()
    }

    pub fn take_backlight_mode(&mut self) -> u8 {
        self.instance.as_ref().map(|instance| instance.shared.take_backlight_mode()).unwrap_or(0)
    }

    pub fn take_phone_call(&mut self) -> Option<String> {
        self.instance.as_ref()?.shared.take_phone_call()
    }

    pub fn take_browser_url(&mut self) -> Option<String> {
        self.instance.as_ref()?.shared.take_browser_url()
    }
}

/// The LGT firmware BIOS, bundled into the app so an LGT title can run real
/// firmware code. Injected as a virtual file under the reference's own filename,
/// so `wie_lgt`'s `try_load_bios` finds it - the game archive is never touched.
/// It is proprietary and lives only in this private repository.
const FIRMWARE_BIOS: &[u8] = include_bytes!("../firmware/libarm32_lgt_system.so");
const FIRMWARE_BIOS_NAME: &str = "libarm32_lgt_system.so";

fn build_emulator(platform: Box<AndroidPlatform>, data: &[u8], options: Options) -> Result<Box<dyn Emulator + Send>, String> {
    let mut files = extract_zip(data).map_err(|x| format!("압축을 열 수 없습니다: {x}"))?;

    // Handset archives are detected by their descriptor. A jar carries no
    // descriptor, so it is only considered once all three archive formats have
    // been ruled out - an apk or jar is itself a zip and would otherwise be
    // mistaken for one.
    if KtfEmulator::loadable_archive(&files) {
        return KtfEmulator::from_archive(platform, files, options)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("KTF 아카이브를 실행할 수 없습니다: {x}"));
    }
    if LgtEmulator::loadable_archive(&files) {
        // Ride the firmware in as a virtual file so the emulator's filesystem
        // exposes it to try_load_bios.
        files.insert(FIRMWARE_BIOS_NAME.to_owned(), FIRMWARE_BIOS.to_vec());
        return LgtEmulator::from_archive(platform, files, options)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("LGT 아카이브를 실행할 수 없습니다: {x}"));
    }
    if SktEmulator::loadable_archive(&files) {
        return SktEmulator::from_archive(platform, files)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("SKT 아카이브를 실행할 수 없습니다: {x}"));
    }

    let id = content_id(data);
    let jar_filename = format!("{id}.jar");
    let jar = data.to_vec();

    if KtfEmulator::loadable_jar(&jar) {
        KtfEmulator::from_jar(platform, &jar_filename, jar, &id, &id, None, options)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("KTF jar를 실행할 수 없습니다: {x}"))
    } else if LgtEmulator::loadable_jar(&jar) {
        LgtEmulator::from_jar(platform, &jar_filename, jar, &id, &id, None, options)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("LGT jar를 실행할 수 없습니다: {x}"))
    } else if SktEmulator::loadable_jar(&jar) {
        SktEmulator::from_jar(platform, &jar_filename, jar, &id, None)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("SKT jar를 실행할 수 없습니다: {x}"))
    } else {
        J2MEEmulator::from_jar(platform, &jar_filename, jar)
            .map(|x| Box::new(x) as Box<dyn Emulator + Send>)
            .map_err(|x| format!("지원하지 않는 형식입니다: {x}"))
    }
}

/// The two names a title's stored data sits under.
///
/// Record stores are keyed by the product id and the writable filesystem by the
/// application id, and an archive's descriptor gives different values for the
/// two - Legend of Master saves under `PD127080` but writes files under
/// `0002A4B1`. Both are needed to collect everything a title has kept.
pub struct SaveIds {
    pub records: String,
    pub files: String,
}

/// Where `data`'s saves would be, without running it.
///
/// This has to agree with what the emulators pass to `System::new`, or an
/// export would quietly come back empty.
pub fn save_ids(data: &[u8]) -> Option<SaveIds> {
    let files = extract_zip(data).ok()?;

    // The descriptor names both ids for the two archive formats that have one.
    for descriptor in ["app_info", "__adf__"] {
        let Some(contents) = files.get(descriptor) else {
            continue;
        };

        let mut product = String::new();
        let mut application = String::new();
        for line in contents.split(|x| *x == b'\n') {
            let line = String::from_utf8_lossy(line);
            let line = line.trim();

            if let Some(value) = line.strip_prefix("PID:") {
                product = value.trim().to_owned();
            }
            if let Some(value) = line.strip_prefix("AID:") {
                application = value.trim().to_owned();
            }
        }

        if !product.is_empty() || !application.is_empty() {
            return Some(SaveIds {
                records: product.clone(),
                files: if application.is_empty() { product } else { application },
            });
        }
    }

    // An SKT archive names itself in its descriptor, or failing that in the
    // descriptor's own filename, and uses the one name for both.
    if let Some((name, contents)) = files.iter().find(|(name, _)| name.ends_with(".msd")) {
        let declared = contents
            .split(|x| *x == b'\n')
            .map(|line| String::from_utf8_lossy(line).trim().to_owned())
            .find_map(|line| line.strip_prefix("DD-ProgName:").map(|x| x.trim().to_owned()));

        let id = declared.unwrap_or_else(|| name.split('.').next().unwrap_or(name).to_owned());
        if !id.is_empty() {
            return Some(SaveIds {
                records: id.clone(),
                files: id,
            });
        }
    }

    // Anything else runs as a bare jar, which has no id but the one derived
    // from its contents.
    let id = content_id(data);

    Some(SaveIds {
        records: id.clone(),
        files: id,
    })
}

/// Describes an archive without running it, for `nativeInspect`. Only used for
/// diagnostics, so every failure is reported as text rather than an error.
pub fn inspect(data: &[u8]) -> String {
    let mut report = String::new();

    let _ = writeln!(report, "size: {} bytes", data.len());
    let _ = writeln!(report, "id: {}", content_id(data));

    let files = match extract_zip(data) {
        Ok(files) => files,
        Err(error) => {
            let _ = writeln!(report, "not a zip: {error}");
            return report;
        }
    };

    let format = if KtfEmulator::loadable_archive(&files) {
        "KTF archive"
    } else if LgtEmulator::loadable_archive(&files) {
        "LGT archive"
    } else if SktEmulator::loadable_archive(&files) {
        "SKT archive"
    } else if KtfEmulator::loadable_jar(data) {
        "KTF jar"
    } else if LgtEmulator::loadable_jar(data) {
        "LGT jar"
    } else if SktEmulator::loadable_jar(data) {
        "SKT jar"
    } else {
        "J2ME jar (assumed)"
    };
    let _ = writeln!(report, "format: {format}");

    // The descriptor is the only place the app id, product id and main class
    // come from, and a malformed one is the usual reason a zip will not run.
    for descriptor in ["app_info", "__adf__"] {
        let Some(contents) = files.get(descriptor) else {
            continue;
        };

        let _ = writeln!(report, "--- {descriptor} ---");
        for line in contents.split(|x| *x == b'\n') {
            let line = String::from_utf8_lossy(line);
            let line = line.trim();
            if line.starts_with("AID:") || line.starts_with("PID:") || line.starts_with("MClass:") || line.starts_with("Ver:") {
                let _ = writeln!(report, "{line}");
            }
        }
    }

    let _ = writeln!(report, "--- entries ({}) ---", files.len());
    for name in files.keys().take(32) {
        let _ = writeln!(report, "{name}");
    }

    report
}

#[cfg(test)]
mod tests {
    use wie_backend::KeyCode;

    use super::{content_id, extract_zip, inspect, key_code, save_ids};

    #[test]
    fn key_indexes_match_the_java_keypad() {
        assert!(matches!(key_code(0), Some(KeyCode::UP)));
        assert!(matches!(key_code(4), Some(KeyCode::OK)));
        assert!(matches!(key_code(8), Some(KeyCode::NUM0)));
        assert!(matches!(key_code(17), Some(KeyCode::NUM9)));
        assert!(matches!(key_code(19), Some(KeyCode::HASH)));
        // The keypad's call key, which a handset's games treat as save.
        assert!(matches!(key_code(20), Some(KeyCode::CALL)));
        assert!(key_code(22).is_none());
        assert!(key_code(-1).is_none());
    }

    #[test]
    fn content_id_is_stable_and_distinct() {
        assert_eq!(content_id(b"abc"), content_id(b"abc"));
        assert_ne!(content_id(b"abc"), content_id(b"abd"));
    }

    #[test]
    fn inspect_reports_lgt_archive() {
        let report = inspect(include_bytes!("../../test_data/helloworld_lgt.zip"));

        assert!(report.contains("format: LGT archive"), "{report}");
        assert!(report.contains("--- app_info ---"), "{report}");
    }

    #[test]
    fn inspect_reports_non_zip() {
        let report = inspect(b"not a zip at all");

        assert!(report.contains("not a zip"), "{report}");
    }

    /// The two ids differ, and reading only one of them would miss half of
    /// what a title has kept.
    #[test]
    fn save_ids_come_from_the_descriptor() {
        for archive in [
            include_bytes!("../../test_data/helloworld_lgt.zip").as_slice(),
            include_bytes!("../../test_data/helloworld_ktf.zip").as_slice(),
        ] {
            let ids = save_ids(archive).expect("this archive has a descriptor");

            assert!(!ids.records.is_empty(), "no product id");
            assert!(!ids.files.is_empty(), "no application id");
            assert!(
                inspect(archive).contains(&format!("PID:{}", ids.records)),
                "the product id is not the descriptor's"
            );
        }
    }

    /// A zip with no descriptor runs as a bare jar, whose id is its content
    /// hash - the same one `build_emulator` would have used.
    #[test]
    fn save_ids_fall_back_to_the_content_id() {
        // A zip that holds nothing a loader recognises.
        let bare = include_bytes!("../../test_data/helloworld_lgt.zip");
        let jar = extract_zip(bare)
            .expect("the archive opens")
            .remove("00000000.jar")
            .expect("it holds a jar");

        let ids = save_ids(&jar).expect("a jar still has an id");

        assert_eq!(ids.records, content_id(&jar));
        assert_eq!(ids.files, ids.records);
    }

    /// A file that cannot be opened has nowhere to have saved to either, and
    /// saying so is what lets the export report it rather than write an empty
    /// zip.
    #[test]
    fn save_ids_reject_what_cannot_be_loaded() {
        assert!(save_ids(b"not a zip at all").is_none());
    }
}
