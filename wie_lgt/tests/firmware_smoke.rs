//! Local bring-up harness for the firmware loader.
//!
//! This runs the real firmware image through `wie_lgt::load_firmware` so the
//! P1 loader can be validated against the actual binary rather than only the
//! synthetic fixture in the unit tests. It is gated on the `WIE_FIRMWARE`
//! environment variable pointing at a firmware file, so it is a no-op in CI
//! (the firmware is a BIOS and is never committed).
//!
//! Run it with, e.g.:
//!   WIE_FIRMWARE=/path/to/libarm32_lgt_system.so \
//!     cargo test -p wie_lgt --test firmware_smoke -- --nocapture

use wie_core_arm::{Allocator, ArmCore};
use wie_lgt::{ImportResolver, load_firmware};
use wie_util::Result;

/// A resolver that binds nothing, so every firmware import is reported. This is
/// the fastest way to see the complete import surface the HLE must eventually
/// cover.
struct ReportAll;

impl ImportResolver for ReportAll {
    fn resolve(&mut self, _core: &mut ArmCore, _name: &str) -> Result<Option<u32>> {
        Ok(None)
    }
}

#[test]
fn loads_real_firmware_when_supplied() {
    let Ok(path) = std::env::var("WIE_FIRMWARE") else {
        eprintln!("WIE_FIRMWARE not set; skipping real-firmware smoke test");
        return;
    };

    let data = std::fs::read(&path).expect("read firmware file");
    println!("firmware: {} ({} bytes)", path, data.len());

    let mut core = ArmCore::new(false, None).unwrap();
    Allocator::init(&mut core).unwrap();

    let base = 0x6000_0000;
    let image = load_firmware(&mut core, &data, base, &mut ReportAll).expect("load firmware");

    println!("base        = {:#x}", image.base);
    println!("entry       = {:#x}", image.entry);
    println!("segments    = {}", image.segments.len());
    for (addr, size) in &image.segments {
        println!("  segment {addr:#x}..{:#x} ({size} bytes)", addr + size);
    }
    println!("exports     = {}", image.exports.len());
    println!("unresolved  = {}", image.unresolved_imports.len());

    // A few known firmware internals must resolve as exports.
    for name in ["MH_sysHalInit", "dlet_start", "InitPCSAutomata", "AND_mdaInit"] {
        match image.export(name) {
            Some(addr) => println!("  export {name} -> {addr:#x}"),
            None => println!("  export {name} NOT FOUND"),
        }
    }

    // libm names are imports, so they should not appear as exports.
    for name in ["cos", "malloc", "la_cal"] {
        assert!(image.export(name).is_none(), "{name} should be an import, not an export");
    }

    // Print the full unbound import list so the HLE work item is explicit.
    let mut names: Vec<&str> = image.unresolved_imports.iter().map(|u| u.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    println!("imports ({}):", names.len());
    println!("  {}", names.join(", "));

    // Sanity: the real firmware exports thousands of symbols and imports the C
    // runtime, so both tables must be substantial.
    assert!(image.exports.len() > 1000, "expected a large export table, got {}", image.exports.len());
    assert!(!image.unresolved_imports.is_empty(), "expected C-runtime imports");
    assert!(image.export("MH_sysHalInit").is_some(), "MH_sysHalInit must be a firmware export");
}
