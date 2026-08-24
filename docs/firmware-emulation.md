# Running the real LGT firmware

## Why

Our reference players (WipiPlayer Plus and the like) sound and behave exactly
like a handset because they do not reimplement the platform: they load the real
LGT firmware, `libarm32_lgt_system.so`, and run it under an ARM interpreter. The
firmware is the whole WIPI middleware — the Yamaha MA-3 audio engine, the SMAF
and `Mwa` decoders, the graphics stack, the database and filesystem — so every
one of the 2,678 platform entry points is correct by construction.

`wie_lgt` today reimplements that platform in Rust. It is a clean-room effort and
covers a lot, but each subsystem is an approximation with its own gaps (the
bundled Zenonia recordings exist to paper over exactly this in audio). Matching
the reference by continuing to reimplement is an open-ended chase.

This document plans the other route: load and run the real firmware, the way the
reference does, and keep our Rust platform only where we choose to. The goal is
handset-accurate behaviour first, and a base to build past the reference second.

## The firmware is treated as a BIOS

`libarm32_lgt_system.so` is proprietary. It is **never committed**. The app
requires the user to supply it at runtime — dumped from their own device or from
a player they already have — exactly as a console emulator requires the user's
own BIOS. The repository stays clean and public; the firmware lives only on the
user's device. Once the firmware produces sound, the bundled recordings and the
`ZenoniaAudioOverride` come out entirely.

## What the reverse engineering found

### Firmware image (`libarm32_lgt_system.so`)

- ARM 32-bit ELF, `ET_DYN`, entry `0xe4c38`.
- Two LOAD segments: `R E` text (~3.4 MB) at vaddr 0, `RW` data/bss
  (memsz ~1.6 MB) at `0x348ad4`. ~5 MB memory image, based at 0.
- ~25,880 relocations, almost all `R_ARM_RELATIVE` (`RELCOUNT` 25,875), plus a
  small PLT (`JMPREL`, 1,032 bytes).
- `NEEDED`: `libla.so`, `libdl.so`, `libc.so`, `libm.so`, `libstdc++.so`.
- Init/entry symbols of interest: `MH_sysHalInit`, `LGTH_themeInitialize`,
  `LGTH_zoopInit`, `InitPCSAutomata`, `dlet_start` (the DLET/clet entry).

### What the firmware needs from the host (the HAL)

Of 131 undefined `FUNC` imports, all but three are standard C runtime:

- `__aeabi_*` soft-float / integer division helpers,
- libm (`sin`, `cos`, `pow`, `floor`, …),
- libc (`malloc`, `memcpy`, `close`, `dlopen`, `__android_log_print`, …),
- POSIX threads and semaphores (`pthread_mutex_*`, `sem_*`).

The three custom ones are the LGT allocator, from `libla.so`: **`la_cal`**,
**`la_mal`**, **`lafr`** (calloc / malloc / free).

So the HAL is "provide a C runtime to emulated ARM code, plus three allocator
functions." This is high-level emulation of libc: when the firmware calls
`malloc` or `cos`, the host runs its own. We already do this for the game binary,
so most of the surface exists.

### How the firmware's output reaches Android (in the reference)

- Audio: the firmware's `MH_mdaWriteData` fills a sound buffer that the host
  bridge hands to Java as `setSoundBuffer(byte[])`, which feeds an `AudioTrack`.
- Video: the host bridge exposes `FrameSurfaceView` (`android_lgt_wipi`), i.e.
  the firmware renders into a framebuffer the bridge blits to a `SurfaceView`.

These are the two output hooks we must provide.

### Where we already are

`wie_lgt` is much closer to this than it looks:

- It has a full ARM ELF loader and relocator (`load_executable`,
  `apply_relocations`, `load_native` in `runtime/init.rs`).
- **It already carries the firmware's symbol map**: `runtime/java/platform_metadata.rs`
  lists platform methods with their firmware addresses (e.g. `cos → 0x00160178`,
  `mdaSetDevInfo → 0x001bb32c`). The hard part — extracting name→address for the
  whole platform — is done.
- A game import today resolves to a firmware address, and an injected SVC / binary
  patch at that address traps into a Rust reimplementation
  (`wie_core_arm` SVC + `binary_patches`).

The firmware binary itself is **not** loaded; only its address map is used, with
Rust standing in at every address.

## The pivot

Load the firmware at its own addresses so real code is present, then:

1. resolve the firmware's own imports (libc/libm/threads + `la_cal`/`la_mal`/`lafr`)
   to host (Rust) HLE handlers, at its PLT;
2. run the firmware's init (`MH_sysHalInit` → … → ready for `dlet_start`);
3. let the game's platform calls run the **real** firmware code at those addresses
   instead of trapping to Rust;
4. hook the firmware's output buffers (sound buffer → our Android audio sink;
   framebuffer → our `GameView`).

The address map we already have becomes the linker table for free.

## Incremental strategy — do not cut over all at once

We do not have to move all 2,678 functions in one step. The firmware can be
loaded while most platform calls keep trapping to Rust; we move **one subsystem
at a time** by removing the Rust stand-ins for that subsystem's addresses and
letting the real firmware run there.

**First subsystem: audio.** It is the user's immediate goal, it is self-contained,
and it is where our reimplementation is weakest.

- Load the firmware.
- Let the `mda*` / sound path run real firmware code (the genuine MA-3 synth, the
  SMAF and `Mwa` decoders — everything we have been reimplementing).
- Hook the firmware's sound-buffer output to `AndroidAudioSink`.
- Keep graphics, input, DB, FS, etc. on the existing Rust path untouched.

If audio then matches the reference, the bundled recordings and the override are
deleted, and later subsystems (graphics, …) move over the same way, each verified
on device before the next.

## Phase plan

- **P1 — Loader + HLE runtime (spike).** Load and relocate the firmware ELF into
  the ARM address space beside the game. Stand up the libc/libm/thread/`libla`
  HLE handlers the firmware imports. Reach a state where the firmware image is
  mapped and its imports resolve. *(No behaviour change yet.)*
- **P2 — Firmware init.** Drive `MH_sysHalInit` and the rest of the boot sequence
  to a ready state, working out the host-call ABI (`__emutls host_call`,
  `a32_blk`) and any init structures the firmware expects.
- **P3 — Audio cutover.** Route the audio subsystem's addresses to the real
  firmware; hook its sound-buffer output to `AndroidAudioSink`. Verify Zenonia on
  device against the reference. Remove the override + bundled recordings.
- **P4 — Wider cutover.** Move graphics and the rest over subsystem by subsystem,
  verifying each on device.
- **P5 — Beyond the reference.** Save states, performance, display scaling, and
  other features the reference does not have.

## Integration points in the tree

- `wie_lgt/src/runtime/init.rs` — `load_executable` / `apply_relocations` are the
  templates for a firmware loader; the SVC/patch dispatch is where a subsystem is
  switched from Rust stand-in to real firmware code.
- `wie_lgt/src/runtime/java/platform_metadata.rs` — the name→address map that
  becomes the game↔firmware link table.
- `wie_core_arm` — ARM execution, SVC, and `binary_patches` hooks; the HLE host
  calls for the firmware's libc imports attach here.
- `wie_android/src/audio.rs` — `AndroidAudioSink`; the firmware sound-buffer hook
  ends here.

## Open questions to resolve during P1/P2

- The exact host-call ABI the firmware↔bridge use (`__emutls_v.host_call.*`,
  `a32_blk_note_write`): how an emulated `bl` to an imported symbol lands in a
  host handler and returns.
- The firmware's boot/init order and the structures `MH_sysHalInit` expects.
- How the firmware learns where its sound and frame buffers are (does the bridge
  pass them in at init, or does the firmware allocate and the bridge read a known
  export?).
- Whether `libla.so` (6 KB in the reference) is worth loading as ARM code or is
  simplest reimplemented as three host functions.

These are all answerable from the reference's own `liblgt_system.so` bridge,
which contains the other side of every one of these interfaces.

## Findings from the first RE pass (complications)

An initial pass at P1/P2 turned up that the loader is not as plug-and-play as
the "symbol map for free" note above hoped:

- **The platform is more than one binary.** Besides `libarm32_lgt_system.so`
  there is `raptor-carrier.mod` (~1 MB, ~4,980 functions), and the carrier
  *imports* WIPI symbols such as `MC_mdaClipCreate` from the firmware. The stack
  layers game → carrier → firmware; a loader has to place and link all of them.
- **`platform_metadata` addresses do not map 1:1 to the extracted firmware.**
  `cos` (metadata `0x160178`) is `UND` in the firmware (a libm import, likely a
  PLT stub at that address), and `mdaSetDevInfo` (metadata `0x1bb32c`) is not a
  firmware export at all. Firmware-internal symbols like `MH_sysHalInit`
  (`0x18529c`), `dlet_start`, and `InitPCSAutomata` *do* match. So the metadata
  addresses are a mix (firmware internals, PLT stubs, and probably carrier
  addresses) and may be tied to a specific reference build's combined image, not
  to any single binary. They cannot be assumed to be the firmware's export table.

The direction still stands, but the loader/link step needs the carrier and the
firmware placed together and their versions reconciled, and the metadata table
re-derived against the exact binaries that get loaded. This is genuinely
multi-binary, version-sensitive reverse engineering, and it can only be brought
up and verified by building and running the emulator on a device with the real
firmware present — not from static analysis alone.
