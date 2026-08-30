use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use test_utils::{TestPlatform, TestPlatformEvent};
use wie_backend::{Emulator, Options, extract_zip};
use wie_lgt::LgtEmulator;

#[test]
pub fn prefixed_archive_runs() {
    let stdout = Arc::new(Mutex::new(Vec::new()));
    let exited = Arc::new(AtomicBool::new(false));

    let stdout_clone = stdout.clone();
    let exited_clone = exited.clone();
    let platform = Box::new(TestPlatform::with_event_handler(move |event| match event {
        TestPlatformEvent::Stdout(buf) => stdout_clone.lock().unwrap().extend(buf),
        TestPlatformEvent::Exit => exited_clone.store(true, Ordering::SeqCst),
    }));

    // Same app as `helloworld_lgt.zip`, repacked with every entry below `P/`
    // the way handset dumps often are.
    let archive = extract_zip(include_bytes!("../../test_data/helloworld_lgt_prefixed.zip")).unwrap();
    assert!(LgtEmulator::loadable_archive(&archive), "prefixed archive not detected");

    let mut emulator = LgtEmulator::from_archive(
        platform,
        archive,
        Options {
            enable_gdbserver: false,
            profile: None,
        },
    )
    .unwrap();

    while !exited.load(Ordering::SeqCst) {
        emulator.tick().unwrap();
    }

    assert_eq!(String::from_utf8(stdout.lock().unwrap().clone()).unwrap(), "Hello, world!");
}
