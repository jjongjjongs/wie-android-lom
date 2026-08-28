//! Local, host-runnable micro-benchmark and correctness harness for the ARM
//! engine.
//!
//! The on-device slowness is dominated by games' own software sprite blitters —
//! tight Thumb loops that read a source pixel, test it against a transparent
//! colour, and conditionally store it. This harness reproduces that shape as a
//! hand-assembled Thumb program so the interpreter's throughput can be measured
//! here on the x86-64 host, with no device, display, or game assets required.
//!
//! Run the benchmark (release is essential for meaningful numbers):
//!
//! ```text
//! cargo test -p wie_core_arm --release blit_throughput -- --ignored --nocapture
//! ```
//!
//! The same fixture doubles as a correctness oracle: `run_blit` returns the
//! destination buffer, so a future engine can be diffed against
//! `Arm32CpuEngine` byte-for-byte on identical input.

extern crate std;

use alloc::vec;
use alloc::vec::Vec;

use crate::engine::{ArmEngine, ArmRegister, EngineRunResult, MemoryPermission};

use super::Arm32CpuEngine;

const CODE_ADDR: u32 = 0x1000;
const SRC_ADDR: u32 = 0x0010_0000;
const DST_ADDR: u32 = 0x0020_0000;
const TRANSPARENT: u32 = 0x0000;

/// Hand-assembled Thumb sprite-blit loop (see `bench.rs` docs). Copies
/// `pixels_per_frame` 16-bit pixels from `SRC_ADDR` to `DST_ADDR`, skipping any
/// pixel equal to the transparent colour, repeated `frames` times.
///
/// ```asm
/// outer: adds r0, r6, #0     ; r0 = src base
///        adds r1, r7, #0     ; r1 = dst base
///        mov  r2, r9         ; r2 = pixels per frame
/// inner: ldrh r3, [r0]       ; pixel = *src
///        cmp  r3, r4         ; transparent?
///        beq  skip
///        strh r3, [r1]       ; *dst = pixel
/// skip:  adds r0, r0, #2
///        adds r1, r1, #2
///        subs r2, r2, #1
///        bne  inner
///        subs r5, r5, #1
///        bne  outer
///        nop                 ; <- `end` breakpoint
/// ```
#[rustfmt::skip]
const BLIT_CODE: [u8; 28] = [
    0x30, 0x1c, 0x39, 0x1c, 0x4a, 0x46, 0x03, 0x88,
    0xa3, 0x42, 0x00, 0xd0, 0x0b, 0x80, 0x80, 0x1c,
    0x89, 0x1c, 0x52, 0x1e, 0xf7, 0xd1, 0x6d, 0x1e,
    0xf2, 0xd1, 0xc0, 0x46,
];

/// PC of the trailing `nop`, used as the engine's `end` breakpoint.
const BLIT_END: u32 = CODE_ADDR + 0x1a;

/// Build a source buffer with a representative mix of opaque and transparent
/// pixels (every fourth pixel transparent), returning it plus the expected
/// destination contents after a blit that skips transparent pixels.
fn blit_fixture(pixels: usize) -> (Vec<u8>, Vec<u8>) {
    let mut src = vec![0u8; pixels * 2];
    let mut expected_dst = vec![0u8; pixels * 2];
    for i in 0..pixels {
        let pixel: u16 = if i % 4 == 0 {
            TRANSPARENT as u16
        } else {
            // Some deterministic non-transparent colour.
            (0x1000 + (i as u16 & 0x0fff)) | 0x8000
        };
        src[i * 2..i * 2 + 2].copy_from_slice(&pixel.to_le_bytes());
        if pixel as u32 != TRANSPARENT {
            expected_dst[i * 2..i * 2 + 2].copy_from_slice(&pixel.to_le_bytes());
        }
    }
    (src, expected_dst)
}

/// Run the blit program on the given engine and return the destination buffer.
///
/// The engine is created fresh, memory mapped, program and source loaded, and
/// the program run to its `nop` breakpoint. Returns `(dst_bytes, executed)`
/// where `executed` is the delta of the global instruction counter.
fn run_blit<E: ArmEngine>(mut engine: E, pixels_per_frame: usize, frames: u32, src: &[u8]) -> (Vec<u8>, u64) {
    // Map code, source and destination pages.
    engine.mem_map(CODE_ADDR & !0xffff, 0x10000, MemoryPermission::ReadExecute);
    engine.mem_map(SRC_ADDR, src.len().next_multiple_of(0x10000), MemoryPermission::ReadWrite);
    engine.mem_map(DST_ADDR, (pixels_per_frame * 2).next_multiple_of(0x10000), MemoryPermission::ReadWrite);

    engine.mem_write(CODE_ADDR, &BLIT_CODE).unwrap();
    engine.mem_write(SRC_ADDR, src).unwrap();

    engine.reg_write(ArmRegister::R4, TRANSPARENT);
    engine.reg_write(ArmRegister::R5, frames);
    engine.reg_write(ArmRegister::R6, SRC_ADDR);
    engine.reg_write(ArmRegister::R7, DST_ADDR);
    engine.reg_write(ArmRegister::SB, pixels_per_frame as u32); // r9
    // Enter Thumb mode at CODE_ADDR (odd address sets the T bit).
    engine.reg_write(ArmRegister::PC, CODE_ADDR | 1);

    let before = crate::EXECUTED_INSTRUCTIONS.load(::core::sync::atomic::Ordering::Relaxed);
    loop {
        match engine.run(BLIT_END, u32::MAX).unwrap() {
            EngineRunResult::End => break,
            EngineRunResult::CountExhausted => continue,
            EngineRunResult::Svc { .. } => panic!("unexpected SVC in blit benchmark"),
        }
    }
    let executed = crate::EXECUTED_INSTRUCTIONS.load(::core::sync::atomic::Ordering::Relaxed) - before;

    let mut dst = vec![0u8; pixels_per_frame * 2];
    engine.mem_read(DST_ADDR, dst.len(), &mut dst).unwrap();
    (dst, executed)
}

#[test]
fn blit_correctness() {
    let pixels = 4096;
    let (src, expected) = blit_fixture(pixels);
    let (dst, _) = run_blit(Arm32CpuEngine::new(), pixels, 1, &src);
    assert_eq!(dst, expected, "Arm32CpuEngine blit produced wrong pixels");
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture --release"]
fn blit_throughput() {
    let pixels = 4096;
    // Overridable so a profiler (e.g. callgrind) can be handed a small,
    // guest-dominated run without rebuilding.
    let frames: u32 = std::env::var("WIE_BENCH_FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
    let (src, _) = blit_fixture(pixels);

    // Report the best of several reps: the fastest run is the one least
    // perturbed by scheduler/contention noise on this shared host, so it is the
    // most stable basis for comparing engine changes.
    let reps: u32 = std::env::var("WIE_BENCH_REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(7);
    let mut best = f64::INFINITY;
    let mut executed = 0u64;
    for _ in 0..reps.max(1) {
        let start = std::time::Instant::now();
        let (_, e) = run_blit(Arm32CpuEngine::new(), pixels, frames, &src);
        best = best.min(start.elapsed().as_secs_f64());
        executed = e;
    }

    let mips = executed as f64 / best / 1.0e6;
    std::eprintln!("[bench] Arm32CpuEngine blit: {executed} insns, best of {reps} = {best:.3}s = {mips:.1} MIPS");
}
