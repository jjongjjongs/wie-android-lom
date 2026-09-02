use core::pin::Pin;

use alloc::{borrow::ToOwned, boxed::Box, collections::BTreeMap, format, string::String, vec::Vec};

use jvm::runtime::{JavaIoInputStream, JavaLangClassLoader};

use wie_backend::{Emulator, Event, Options, Platform, System, TaskRunner, extract_zip};
use wie_core_arm::{Allocator, ArmCore, EXECUTED_INSTRUCTIONS, PC_SAMPLES};
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
    /// Coarse MIPS/tick meter state: wall-clock ms and instruction count at the
    /// last report, and ticks since. Lets one game run reveal whether the title
    /// is CPU-bound (interpreter flat out) or waiting elsewhere.
    perf_last_ms: u64,
    perf_last_instr: u64,
    perf_ticks: u32,
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
            if is_incompatible_bundled_save(aid, filename) {
                tracing::info!("skipping incompatible bundled save {filename:?}; the title will start fresh");
                continue;
            }
            system.filesystem().add_virtual(filename, data.clone())
        }

        // Certification/auth files are sometimes packaged inside a folder (e.g.
        // "인증파일/certification"), but a title reads them from its data-dir
        // root. Expose such a file under its bare name too so the read resolves
        // and the title's offline auth check passes instead of falling through
        // to an online billing handshake that cannot complete. A real root copy
        // always takes precedence.
        const AUTH_FILES: [&str; 3] = ["certification", "cert.c2s", "certi.pzx"];
        for (filename, data) in files {
            let filename = filename.trim_start_matches("P/");
            if let Some((_, base)) = filename.rsplit_once('/')
                && AUTH_FILES.contains(&base)
                && !files.keys().any(|k| k.trim_start_matches("P/") == base)
            {
                tracing::info!("exposing nested auth file {filename:?} at data-dir root as {base:?}");
                system.filesystem().add_virtual(base, data.clone());
            }
        }

        seed_consent_savefile(&system, aid);

        Allocator::init(&mut core)?;

        let main_class_name = main_class_name.map(|x| x.replace('.', "/"));

        let mut core_clone = core.clone();
        let mut system_clone = system.clone();
        let main_class_name_clone = main_class_name.clone();
        let jar_filename = jar_filename.to_owned();
        let aid = aid.to_owned();

        system.spawn(async move || Self::do_start(&mut core_clone, &mut system_clone, jar_filename, aid, main_class_name_clone).await);

        Ok(Self {
            core,
            system,
            perf_last_ms: 0,
            perf_last_instr: 0,
            perf_ticks: 0,
        })
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
        apply_gamevil_baseball_auth_patch(&aid, &mut binary_mod);

        // If the user supplied the firmware BIOS, map and bind it, drive its
        // init as its own task so a bring-up crash cannot corrupt the game
        // thread (P2), and route the game's media imports to the firmware's
        // MC_mda* exports (P3). Dormant without the BIOS; the game runs on the
        // Rust platform either way.
        let mda_routes = if let Some(image) = crate::runtime::firmware_link::try_load_bios(core, system).await? {
            let routes = crate::runtime::firmware_link::build_mda_routes(core, system, &image)?;
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

/// GAMEVIL's 프로야구 baseball titles (2009 `0002E749`, 2010 `0002C663`) gate
/// play on an offline subscriber check: the title decrypts the phone number its
/// copy was licensed to from its own data and only starts if `PHONENUMBER`
/// equals it (or the licensed number is the SDK master `19988999999`). Every
/// copy is licensed to the original buyer's SIM, and that per-copy number is not
/// stored in plaintext anywhere in the archive, so under emulation we cannot
/// report it and the title parks on the ez-i "인증에 실패하였습니다" screen
/// (2010 hangs on a blank screen before it, until its stale bundled saves are
/// skipped) and never reaches its menu.
///
/// The verifier is the same routine in both titles, only compiled to different
/// addresses:
///
/// ```text
///   bl strncmp(PHONENUMBER, licensed) ; cmp r0,#0 ; beq ok
///   ldr r0,[pc,#N] ; adds r1,r4,#0 ; movs r2,#0xc
///   bl strncmp(master, licensed)      ; cmp r0,#0 ; bne fail
///   ...                                              ; ok: movs r0,#1 (success)
/// ```
///
/// Turning the first `beq ok` into an unconditional branch makes the SIM
/// comparison always take the success path, so any copy authenticates
/// regardless of the reported number - the same offline-auth bypass approach as
/// the Super Action Hero 3 patch above (and the reference player's own auth
/// shim). The two `bl` targets and the `ldr` literal offset differ between the
/// titles, so those bytes are wildcards; the surrounding two `cmp r0,#0` /
/// branch pairs make the 18-byte window specific to this verifier.
///
/// Applied only to those titles, only when the window is present exactly once
/// and the branch byte is the expected `beq` (`0xd0`), so nothing else can be
/// touched by accident.
fn apply_gamevil_baseball_auth_patch(aid: &str, binary_mod: &mut [u8]) {
    if !matches!(aid.to_ascii_uppercase().as_str(), "0002E749" | "0002C663") {
        return;
    }

    // cmp r0,#0 ; beq +9 ; ldr r0,[pc,#N] ; adds r1,r4,#0 ; movs r2,#0xc ;
    // bl strncmp ; cmp r0,#0 ; bne fail. `None` masks the per-title `bl` target
    // and the `ldr` literal-pool offset (the low byte of `?? 48`).
    const PATTERN: [Option<u8>; 18] = [
        Some(0x00),
        Some(0x28),
        Some(0x09),
        Some(0xd0),
        None,
        Some(0x48),
        Some(0x21),
        Some(0x1c),
        Some(0x0c),
        Some(0x22),
        None,
        None,
        None,
        None,
        Some(0x00),
        Some(0x28),
        Some(0x06),
        Some(0xd1),
    ];
    // Offset within the window of the `beq` branch's high byte.
    const BRANCH_OFFSET: usize = 3;
    const BEQ_HI: u8 = 0xd0; // 1101 = B<EQ>
    const B_HI: u8 = 0xe0; // 11100 = unconditional B (same +9-halfword offset)

    let matches = |window: &[u8]| window.iter().zip(PATTERN).all(|(&byte, expect)| expect.is_none_or(|e| e == byte));

    let mut site = None;
    let mut count = 0usize;
    for (index, window) in binary_mod.windows(PATTERN.len()).enumerate() {
        if matches(window) {
            count += 1;
            site.get_or_insert(index);
        }
    }

    match (count, site) {
        (1, Some(start)) => {
            let target = start + BRANCH_OFFSET;
            debug_assert_eq!(binary_mod[target], BEQ_HI);
            binary_mod[target] = B_HI;
            tracing::info!("Applied GAMEVIL 프로야구 offline authentication patch at binary.mod offset {target}");
        }
        (0, _) => tracing::warn!("GAMEVIL 프로야구 auth patch site not found; leaving binary.mod unchanged"),
        (n, _) => tracing::warn!("GAMEVIL 프로야구 auth pattern is ambiguous ({n} sites); leaving binary.mod unchanged"),
    }
}

/// Whether a bundled data-dir file is a save the title itself rejects, so
/// seeding it would brick start-up rather than restore progress.
///
/// GAMEVIL's 프로야구 titles ship foreign saves (dumped from another handset)
/// that their own start-up integrity check rejects:
///
/// - 2009 프로야구 (`0002E749`) reads `SaveFile.dat`, runs a pure,
///   phone-number-independent check over the bytes (verified against
///   PHONENUMBER/PHONEMODEL - neither changes the outcome), and on failure draws
///   "Error code :: -1005" and calls `MC_knlExit`.
/// - 2010 프로야구 (`0002C663`) reads its `game_*.sav` set and, on a bad save,
///   never leaves a blank screen (it loops before it even reaches its ez-i auth
///   step).
///
/// In both, the title's *own* freshly written save re-validates cleanly, so
/// this is stale data, not an emulation fault. With no save present the title
/// opens the file for create instead of read and starts fresh, exactly as a
/// clean install does - so drop the incompatible bundled copy and let it do
/// that. A real save the player later writes lives in the platform layer and is
/// unaffected.
///
/// Scoped to each title's aid and its own save filenames, so nothing else is
/// touched.
fn is_incompatible_bundled_save(aid: &str, filename: &str) -> bool {
    match aid.to_ascii_uppercase().as_str() {
        "0002E749" => filename.eq_ignore_ascii_case("SaveFile.dat"),
        "0002C663" => {
            let lower = filename.to_ascii_lowercase();
            lower.starts_with("game_") && lower.ends_with(".sav")
        }
        _ => false,
    }
}

/// SEED (`00027565`) opens `SEED_OP.dat` on launch to decide whether it has
/// already recorded the player's SMS-billing consent. Without that file the
/// title parks on the 수신동의 (consent) screen, whose YES branch drives an
/// online SMS handshake that cannot complete under emulation, so the game never
/// reaches its own menu.
///
/// The save is a fixed 64-byte options block; the consent record is the pair of
/// 32-bit words at offset 32 and 36, and the title treats "both words non-zero"
/// as "consent already given" and skips straight to its title menu. Seed exactly
/// that — a zeroed block with the consent words set — into the *virtual* layer,
/// so a real save the player later writes shadows it (platform reads win over
/// the virtual layer) and every other option stays at its zero default.
///
/// Scoped to SEED's aid, so no other title is touched.
fn seed_consent_savefile(system: &System, aid: &str) {
    if !aid.eq_ignore_ascii_case("00027565") {
        return;
    }

    // 64-byte options block: zeroed but for the consent words at offset 32/36.
    const CONSENT_OFFSET: usize = 32;
    let mut save = alloc::vec![0u8; 64];
    save[CONSENT_OFFSET..CONSENT_OFFSET + 8].fill(1);

    tracing::info!("Seeding SEED consent save so the SMS-consent screen is skipped");
    system.filesystem().add_virtual("SEED_OP.dat", save);
}

/// Log the hottest 64 KiB PC regions from the sampler as a share of samples.
/// A firmware address (`0x6000_0000+`) points at a firmware routine worth
/// re-implementing natively; a low game-clet address means the cost is the
/// title's own code, which only a recompiler can speed up.
fn report_hot_regions() {
    use core::fmt::Write;
    use core::sync::atomic::Ordering::Relaxed;

    let mut top: [(usize, u32); 6] = [(0, 0); 6];
    let mut total: u64 = 0;
    for (index, slot) in PC_SAMPLES.iter().enumerate() {
        let count = slot.load(Relaxed);
        if count == 0 {
            continue;
        }
        total += count as u64;
        if count > top[5].1 {
            top[5] = (index, count);
            top.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        }
    }
    if total == 0 {
        return;
    }

    let mut line = String::from("[hot]");
    for (index, count) in top.iter().filter(|(_, count)| *count > 0) {
        let region = (*index as u32) << 16;
        let pct = *count as f64 * 100.0 / total as f64;
        let _ = write!(line, " {region:#010x}:{pct:.0}%");
    }
    tracing::info!("{line}");
}

/// Name the LGT SVC category, so the hot-syscall line is readable.
fn svc_category_name(category: usize) -> &'static str {
    match category {
        1 => "init",
        3 => "wipi-c",
        5 => "stdlib",
        6 => "libc",
        7 => "mda",
        8 => "jni",
        _ => "?",
    }
}

/// Log the SVCs-per-second broken down by category, so the dominant syscall
/// (the frame-time cost, since the round-trip — not raw execution — is the wall)
/// is identified by name. Counters are drained each period.
fn report_hot_svc(dt_ms: u64) {
    use core::fmt::Write;
    use core::sync::atomic::Ordering::Relaxed;

    let mut top: [(usize, u64); 4] = [(0, 0); 4];
    for (category, slot) in wie_core_arm::SVC_CATEGORY_COUNT.iter().enumerate() {
        let count = slot.swap(0, Relaxed);
        if count > top[3].1 {
            top[3] = (category, count);
            top.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        }
    }
    if top[0].1 == 0 {
        return;
    }
    let mut line = String::from("[svc]");
    for (category, count) in top.iter().filter(|(_, c)| *c > 0) {
        let per_s = *count as f64 * 1000.0 / dt_ms as f64;
        let _ = write!(line, " {}({category})={per_s:.0}/s", svc_category_name(*category));
    }
    tracing::info!("{line}");
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
        })?;

        // Report a coarse MIPS/tick rate about once a second. Compared against
        // the device's ceiling this shows whether raw interpretation is the
        // bottleneck (worth a JIT) or the title is waiting on something else.
        let now_ms = self.system.platform().now().raw();
        self.perf_ticks += 1;
        if self.perf_last_ms == 0 {
            self.perf_last_ms = now_ms;
            self.perf_last_instr = EXECUTED_INSTRUCTIONS.load(core::sync::atomic::Ordering::Relaxed);
        } else if now_ms.saturating_sub(self.perf_last_ms) >= 1000 {
            let dt_ms = now_ms - self.perf_last_ms;
            let instr = EXECUTED_INSTRUCTIONS.load(core::sync::atomic::Ordering::Relaxed);
            let executed = instr.saturating_sub(self.perf_last_instr);
            let ticks = self.perf_ticks;
            let mips = executed as f64 / dt_ms as f64 / 1000.0;
            let tps = ticks as f64 * 1000.0 / dt_ms as f64;
            // SVCs and run() calls per second: a high SVC rate points the frame
            // cost at the syscall round-trip rather than raw execution.
            let svc = wie_core_arm::SVC_COUNT.swap(0, core::sync::atomic::Ordering::Relaxed);
            let runs = wie_core_arm::RUN_CALLS.swap(0, core::sync::atomic::Ordering::Relaxed);
            let fb = wie_core_arm::JIT_FALLBACKS.swap(0, core::sync::atomic::Ordering::Relaxed);
            let svc_per_s = svc as f64 * 1000.0 / dt_ms as f64;
            let runs_per_s = runs as f64 * 1000.0 / dt_ms as f64;
            let fb_per_s = fb as f64 * 1000.0 / dt_ms as f64;
            // Live/peak guest threads: each owns a 1 MB native stack, so a
            // climbing live count is what exhausts the heap (see the allocation
            // failure that stops LGT titles with many concurrent threads).
            let live_threads = wie_core_arm::LIVE_THREADS.load(core::sync::atomic::Ordering::Relaxed);
            let peak_threads = wie_core_arm::PEAK_THREADS.load(core::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                "[perf] {mips:.1} MIPS, {tps:.1} tick/s, {svc_per_s:.0} svc/s, {runs_per_s:.0} run/s, {fb_per_s:.0} fallback/s, threads={live_threads} (peak {peak_threads}) ({executed} insn / {ticks} ticks in {dt_ms} ms)"
            );
            report_hot_regions();
            report_hot_svc(dt_ms);
            crate::runtime::wipi_c::report_hot_wipic(dt_ms);
            crate::runtime::stdlib::report_hot_stdlib(dt_ms);
            crate::runtime::init::report_hot_init(dt_ms);
            crate::runtime::java::method_bridge::report_hot_java(dt_ms);
            self.perf_last_ms = now_ms;
            self.perf_last_instr = instr;
            self.perf_ticks = 0;
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::{apply_gamevil_baseball_auth_patch, is_incompatible_bundled_save};

    // The 2009 verifier window (2010 differs only in the wildcard `bl`/`ldr`
    // bytes); `09 d0` at index 2-3 is the `beq` to flip.
    const BASEBALL_2009_AUTH_WINDOW: [u8; 18] = [
        0x00, 0x28, 0x09, 0xd0, 0x0e, 0x48, 0x21, 0x1c, 0x0c, 0x22, 0x3e, 0xf0, 0x29, 0xfc, 0x00, 0x28, 0x06, 0xd1,
    ];
    // The 2010 verifier window: different `bl` target and `ldr` literal offset.
    const BASEBALL_2010_AUTH_WINDOW: [u8; 18] = [
        0x00, 0x28, 0x09, 0xd0, 0x0f, 0x48, 0x21, 0x1c, 0x0c, 0x22, 0x62, 0xf0, 0x94, 0xfb, 0x00, 0x28, 0x06, 0xd1,
    ];

    #[test]
    fn baseball_auth_patch_flips_both_titles_branch() {
        // Both gated titles flip the beq (0xd0) into an unconditional b (0xe0).
        for (aid, window) in [("0002E749", BASEBALL_2009_AUTH_WINDOW), ("0002C663", BASEBALL_2010_AUTH_WINDOW)] {
            let mut binary = window.to_vec();
            apply_gamevil_baseball_auth_patch(aid, &mut binary);
            assert_eq!(binary[3], 0xe0, "aid {aid} branch not flipped");
        }

        // A different title leaves the same bytes untouched.
        let mut other = BASEBALL_2009_AUTH_WINDOW.to_vec();
        apply_gamevil_baseball_auth_patch("00027565", &mut other);
        assert_eq!(other, BASEBALL_2009_AUTH_WINDOW);

        // No site present: nothing changes.
        let mut empty = alloc::vec![0u8; 64];
        let snapshot = empty.clone();
        apply_gamevil_baseball_auth_patch("0002E749", &mut empty);
        assert_eq!(empty, snapshot);
    }

    #[test]
    fn drops_only_the_baseball_saves() {
        // 2009 프로야구's foreign SaveFile.dat is dropped so it starts fresh.
        assert!(is_incompatible_bundled_save("0002E749", "SaveFile.dat"));
        // aid match is case-insensitive, as app_info casing varies.
        assert!(is_incompatible_bundled_save("0002e749", "SaveFile.dat"));
        // Other files in that title are kept.
        assert!(!is_incompatible_bundled_save("0002E749", "level.dat"));

        // 2010 프로야구's foreign game_*.sav set is dropped.
        assert!(is_incompatible_bundled_save("0002C663", "game_o.sav"));
        assert!(is_incompatible_bundled_save("0002C663", "game_vs.sav"));
        // Its other data-dir files are kept.
        assert!(!is_incompatible_bundled_save("0002C663", "speed.dat"));
        assert!(!is_incompatible_bundled_save("0002C663", "audio.adt"));
        // 2010's rule does not apply to 2009 (which has no game_*.sav anyway).
        assert!(!is_incompatible_bundled_save("0002E749", "game_o.sav"));

        // Other titles keep their SaveFile.dat.
        assert!(!is_incompatible_bundled_save("00027565", "SaveFile.dat"));
    }
}
