//! Local bring-up harness for firmware init (P2).
//!
//! Loads a real game archive with the firmware BIOS injected as a virtual file,
//! then ticks the emulator so the firmware's constructors and `MH_sysHalInit`
//! run as their own task. The firmware-libc stubs log every import the init
//! touches, so this answers "does the firmware execute under our interpreter,
//! and what does its init call?".
//!
//! Gated on `WIE_FIRMWARE`, so it is a no-op in CI (the firmware is a BIOS and
//! is never committed). Run it with:
//!   WIE_FIRMWARE=/path/to/libarm32_lgt_system.so \
//!     cargo test -p wie_lgt --test firmware_init -- --nocapture

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use test_utils::{TestPlatform, TestPlatformEvent};
use wie_backend::{Emulator, Options, extract_zip};
use wie_lgt::LgtEmulator;

#[test]
fn drives_firmware_init_when_supplied() {
    let Ok(path) = std::env::var("WIE_FIRMWARE") else {
        eprintln!("WIE_FIRMWARE not set; skipping firmware init harness");
        return;
    };

    // Print the firmware log (including the init trace) to stderr.
    let _ = tracing_subscriber::fmt().with_test_writer().with_max_level(tracing::Level::INFO).try_init();

    let firmware = std::fs::read(&path).expect("read firmware file");

    let exited = Arc::new(AtomicBool::new(false));
    let exited_clone = exited.clone();
    let platform = Box::new(TestPlatform::with_event_handler(move |event| {
        if let TestPlatformEvent::Exit = event {
            exited_clone.store(true, Ordering::SeqCst);
        }
    }));

    // Inject the firmware into the game archive as a virtual file so
    // try_load_bios finds it.
    let mut archive = extract_zip(include_bytes!("../../test_data/helloworld_lgt.zip")).unwrap();
    archive.insert("libarm32_lgt_system.so".into(), firmware);

    let mut emulator = LgtEmulator::from_archive(
        platform,
        archive,
        Options {
            enable_gdbserver: false,
            profile: None,
        },
    )
    .expect("build emulator");

    // Keep ticking past the game's own exit so the spawned firmware init task
    // has time to run MH_sysHalInit and log. A firmware fault is swallowed
    // inside its own task, so it does not error out of tick.
    let mut ticks = 0;
    while ticks < 40000 {
        if let Err(error) = emulator.tick() {
            eprintln!("emulator tick error at tick {ticks}: {error:?}");
            break;
        }
        ticks += 1;
    }

    eprintln!("firmware init harness finished after {ticks} ticks (game exited={})", exited.load(Ordering::SeqCst));
}
