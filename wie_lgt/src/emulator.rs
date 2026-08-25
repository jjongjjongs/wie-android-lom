use core::pin::Pin;

use alloc::{borrow::ToOwned, boxed::Box, collections::BTreeMap, format, string::String, vec::Vec};

use jvm::runtime::{JavaIoInputStream, JavaLangClassLoader};

use wie_backend::{Emulator, Event, Options, Platform, System, TaskRunner, extract_zip};
use wie_core_arm::{Allocator, ArmCore};
use wie_jvm_support::{JvmSupport, RustJavaJvmImplementation};
use wie_util::{Result, WieError};

use crate::runtime::init::load_native;

struct LgtTaskRunner {
    core: ArmCore,
}

#[async_trait::async_trait]
impl TaskRunner for LgtTaskRunner {
    async fn run(&self, future: Pin<Box<dyn Future<Output = Result<()>> + Send>>) -> Result<()> {
        self.core.run_in_thread(async move || future.await)?.await
    }
}

pub struct LgtEmulator {
    core: ArmCore,
    system: System,
}

impl LgtEmulator {
    pub fn from_archive(platform: Box<dyn Platform>, files: BTreeMap<String, Vec<u8>>, options: Options) -> Result<Self> {
        let app_info = files
            .get("app_info")
            .ok_or_else(|| WieError::FatalError("Missing app_info in LGT archive".into()))?;
        let app_info = LgtAppInfo::parse(app_info);

        tracing::info!("Loading app {}, pid {}, mclass {}", app_info.aid, app_info.pid, app_info.mclass);

        let jar_filename = Self::find_jar(&files, &app_info.aid)?;

        Self::load(
            platform,
            &jar_filename,
            &app_info.pid,
            &app_info.aid,
            Some(app_info.mclass),
            &files,
            options,
        )
    }

    /// The jar an archive's descriptor names, or the one it actually contains.
    ///
    /// Repacked archives do not always agree with themselves: SEED's `app_info`
    /// declares `AID:00027565` while the jar beside it is `00025A84.jar`. When
    /// the declared name is absent and there is exactly one jar, that jar is
    /// what the descriptor meant.
    fn find_jar(files: &BTreeMap<String, Vec<u8>>, aid: &str) -> Result<String> {
        let declared = format!("{aid}.jar");
        if files.contains_key(&declared) {
            return Ok(declared);
        }

        let mut jars = files.keys().filter(|x| x.ends_with(".jar"));

        match (jars.next(), jars.next()) {
            (Some(jar), None) => {
                tracing::warn!("LGT archive declares {declared} but contains {jar}; using it");

                Ok(jar.clone())
            }
            (Some(_), Some(_)) => Err(WieError::FatalError(format!(
                "LGT archive declares {declared}, which is missing, and holds several jars"
            ))),
            _ => Err(WieError::FatalError(format!("LGT archive holds no jar; expected {declared}"))),
        }
    }

    pub fn from_jar(
        platform: Box<dyn Platform>,
        jar_filename: &str,
        jar: Vec<u8>,
        pid: &str,
        aid: &str,
        main_class_name: Option<String>,
        options: Options,
    ) -> Result<Self> {
        let files = [(jar_filename.to_owned(), jar)].into_iter().collect();

        Self::load(platform, jar_filename, pid, aid, main_class_name, &files, options)
    }

    pub fn loadable_archive(files: &BTreeMap<String, Vec<u8>>) -> bool {
        files.contains_key("app_info")
    }

    pub fn loadable_jar(jar: &[u8]) -> bool {
        let Ok(files) = extract_zip(jar) else {
            return false;
        };

        files.contains_key("binary.mod")
    }

    fn load(
        platform: Box<dyn Platform>,
        jar_filename: &str,
        pid: &str,
        aid: &str,
        main_class_name: Option<String>,
        files: &BTreeMap<String, Vec<u8>>,
        mut options: Options,
    ) -> Result<Self> {
        let mut core = ArmCore::new(options.enable_gdbserver, options.profile.take())?;
        let system = System::new(platform, pid, aid, LgtTaskRunner { core: core.clone() });

        for (filename, data) in files {
            let filename = filename.trim_start_matches("P/");
            system.filesystem().add_virtual(filename, data.clone())
        }

        Allocator::init(&mut core)?;

        let main_class_name = main_class_name.map(|x| x.replace('.', "/"));

        let mut core_clone = core.clone();
        let mut system_clone = system.clone();
        let main_class_name_clone = main_class_name.clone();
        let jar_filename = jar_filename.to_owned();
        let aid = aid.to_owned();

        system.spawn(async move || Self::do_start(&mut core_clone, &mut system_clone, jar_filename, aid, main_class_name_clone).await);

        Ok(Self { core, system })
    }

    #[tracing::instrument(name = "start", skip_all)]
    async fn do_start(core: &mut ArmCore, system: &mut System, jar_filename: String, aid: String, main_class_name: Option<String>) -> Result<()> {
        let protos = [wie_midp::get_protos().into(), wie_wipi_java::get_protos().into()];
        let jvm = JvmSupport::new_jvm(system, Some(&jar_filename), Box::new(protos), &[], RustJavaJvmImplementation).await?; // TODO use lgt's java implementation

        let class_loader = match jvm.current_class_loader().await {
            Ok(class_loader) => class_loader,
            Err(error) => return Err(JvmSupport::to_wie_err(&jvm, error).await),
        };

        let stream = match JavaLangClassLoader::get_resource_as_stream(&jvm, &class_loader, "binary.mod").await {
            Ok(Some(stream)) => stream,
            Ok(None) => return Err(WieError::FatalError(format!("{jar_filename} has no binary.mod"))),
            Err(error) => return Err(JvmSupport::to_wie_err(&jvm, error).await),
        };

        let mut binary_mod = match JavaIoInputStream::read_until_end(&jvm, &stream).await {
            Ok(binary_mod) => binary_mod,
            Err(error) => return Err(JvmSupport::to_wie_err(&jvm, error).await),
        };

        apply_offline_auth_patch(&aid, &mut binary_mod);

        // If the user supplied the firmware BIOS, map and bind it, drive its
        // init as its own task so a bring-up crash cannot corrupt the game
        // thread (P2), and route the game's media imports to the firmware's
        // MC_mda* exports (P3). Dormant without the BIOS; the game runs on the
        // Rust platform either way.
        let mda_routes = if let Some(image) = crate::runtime::firmware_link::try_load_bios(core, system).await? {
            let routes = crate::runtime::firmware_link::build_mda_routes(&image);
            let plan = crate::runtime::firmware_link::FirmwareInitPlan::from_image(&image);
            let mut firmware_core = core.clone();
            system.spawn(async move || {
                if let Err(error) = crate::runtime::firmware_link::run_firmware_init(&mut firmware_core, &plan).await {
                    tracing::error!("Firmware init failed (continuing on the Rust platform): {error:?}");
                }
                Ok(())
            });
            routes
        } else {
            BTreeMap::new()
        };

        load_native(core, system, &jvm, &binary_mod, &jar_filename, main_class_name.as_deref(), mda_routes).await?;

        Ok(())
    }
}

/// Super Action Hero 3 (`00028E74`) checks in with an authentication server
/// before it will play. WipiPlayer flips the one event the check dispatches so
/// the title takes its offline branch instead; do the same to the module we
/// load. The instruction is `cmp r3, #0x15; bne; movs r0, #0x15; movs r1,
/// #0x2d; bl <auth>` - the auth event id 0x15 handed to the dispatcher. Turning
/// it into 0x18 is the whole patch WipiPlayer verified by hash.
///
/// It is applied only to that title, and only when the exact instruction window
/// is present exactly once, so nothing else can be touched by accident.
fn apply_offline_auth_patch(aid: &str, binary_mod: &mut [u8]) {
    if !aid.eq_ignore_ascii_case("00028E74") {
        return;
    }

    // cmp r3,#0x15 ; bne +.. ; movs r0,#0x15 ; movs r1,#0x2d
    const WINDOW: [u8; 8] = [0x15, 0x2b, 0x05, 0xd1, 0x15, 0x20, 0x2d, 0x21];
    // Offset within the window of the `movs r0, #0x15` immediate byte.
    const IMM_OFFSET: usize = 4;
    const PATCHED_IMM: u8 = 0x18;

    let mut site = None;
    let mut count = 0usize;
    for (index, window) in binary_mod.windows(WINDOW.len()).enumerate() {
        if window == WINDOW {
            count += 1;
            site.get_or_insert(index);
        }
    }

    match (count, site) {
        (1, Some(start)) => {
            let target = start + IMM_OFFSET;
            binary_mod[target] = PATCHED_IMM;
            tracing::info!("Applied Super Action Hero 3 offline authentication patch at binary.mod offset {target}");
        }
        (0, _) => tracing::warn!("Super Action Hero 3 auth patch site not found; leaving binary.mod unchanged"),
        (n, _) => tracing::warn!("Super Action Hero 3 auth pattern is ambiguous ({n} sites); leaving binary.mod unchanged"),
    }
}

impl Emulator for LgtEmulator {
    fn handle_event(&mut self, event: Event) {
        self.system.event_queue().push(event)
    }

    fn tick(&mut self) -> Result<()> {
        self.system.tick().map_err(|x| {
            let reg_stack = self.core.dump_reg_stack(0x1000); // TODO: hardcode
            match x {
                WieError::FatalError(msg) => WieError::FatalError(format!("{msg}\n{reg_stack}")),
                _ => WieError::FatalError(format!("{x}\n{reg_stack}")),
            }
        })
    }
}

// almost similar to KtfAdf.. can we merge these?
struct LgtAppInfo {
    aid: String,
    pid: String,
    mclass: String,
}

impl LgtAppInfo {
    pub fn parse(data: &[u8]) -> Self {
        let mut aid = String::new();
        let mut pid = String::new();
        let mut mclass = String::new();

        let mut lines = data.split(|x| *x == b'\n');

        for line in &mut lines {
            if line.starts_with(b"AID:") {
                aid = String::from_utf8_lossy(&line[4..]).into();
            } else if line.starts_with(b"PID:") {
                pid = String::from_utf8_lossy(&line[4..]).into();
            } else if line.starts_with(b"MClass:") {
                mclass = String::from_utf8_lossy(&line[7..]).into();
            }
            // TODO load name, it's in euc-kr..
        }

        Self { aid, pid, mclass }
    }
}
