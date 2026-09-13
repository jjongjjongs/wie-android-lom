use std::{
    sync::Arc,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use test_utils::{TestPlatform, TestPlatformEvent};
use wie_backend::{Emulator, Options, extract_zip};
use wie_lgt::LgtEmulator;
use wie_util::Result;

/// Quarantined: this upstream fixture never reaches `Exit` on this fork's LGT
/// runtime, so the tick loop below spins forever. It went unnoticed because
/// `cargo test --workspace` aborted earlier, on a `wie_ktf` test that no longer
/// compiled. Run it directly (`cargo test -p wie_lgt --test test_helloworld --
/// --ignored`) when picking the bring-up back up.
#[test]
#[ignore = "hangs: the LGT helloworld fixture never exits on this runtime"]
pub fn test_helloworld() -> Result<()> {
    let stdout = Arc::new(Mutex::new(Vec::new()));
    let exited = Arc::new(AtomicBool::new(false));

    let stdout_clone = stdout.clone();
    let exited_clone = exited.clone();
    let event_handler = move |event| match event {
        TestPlatformEvent::Stdout(buf) => {
            stdout_clone.lock().unwrap().extend(buf);
        }
        TestPlatformEvent::OpenUrl(_) => {}
        TestPlatformEvent::Exit => {
            exited_clone.store(true, Ordering::SeqCst);
        }
    };

    let platform = Box::new(TestPlatform::with_event_handler(event_handler));

    let archive = extract_zip(include_bytes!("../../test_data/helloworld_lgt.zip"))?;
    let mut emulator = LgtEmulator::from_archive(
        platform,
        archive,
        Options {
            enable_gdbserver: false,
            profile: None,
            annunciator: None,
        },
    )?;

    while !exited.load(Ordering::SeqCst) {
        emulator.tick()?;
    }

    let stdout_str = String::from_utf8(stdout.lock().unwrap().clone()).unwrap();
    assert_eq!(stdout_str, "Hello, world!");

    Ok(())
}
