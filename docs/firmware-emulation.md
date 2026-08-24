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

### The reference is a multi-binary ARM32-on-ARM64 stack

The reference APK ships six native libraries in `lib/arm64-v8a/` — all of them
**aarch64** host code except the firmware, which is **ARM32**. The whole point of
the stack is to run that one ARM32 module inside a 64-bit process:

| Reference library | Arch | Role |
|---|---|---|
| `libarm32_lgt_system.so` (5.7 MB) | **ARM32** | The firmware — the entire WIPI/J2ME/MIDP middleware as native code. |
| `liblgt_system.so` (282 KB) | aarch64 | JNI bridge + the `a32_*` **ARM32 interpreter/JIT** (`a32_blk_run`, `a32_run`, `host_call`) + `elf_load`/`elf_relocate`. |
| `libarm32_raptor.so` (350 KB) | aarch64 | ARM32 **module loader** (`load_module`, `_reloc_arm`, `setup_raptor_module`) + WIPI kernel services (`_MC_knlAlloc/Calloc/Free/CurrentTime/DefTimer/SetTimer`). |
| `libarm32_raptor_er.so` (12 KB) | aarch64 | ELF-load helper (`mmap`/`mprotect`/`dlopen`). |
| `libla.so` (6 KB) | aarch64 | `LegacyAddressCompat` — reserves a low 32-bit host address region so the ARM32 module's 32-bit pointers are valid pointers in the 64-bit process. |
| `libbinary.so`, `libcheat.so` | aarch64 | Small kernel overrides (`CUSTOM_MC_grpCreateImage`, timers) and cheat hooks. |

The reference cannot execute ARM32 natively from a 64-bit process, so
`liblgt_system.so` carries its own **ARM32 interpreter/JIT** (`a32_*`). Interpreted
ARM32 code reaches native aarch64 functions (libc, kernel services) through
`host_call`, an emutls-dispatched call bridge.

### The crucial simplification: we already own the two hardest pieces

Mapped onto our tree, most of the reference stack is plumbing **we do not need**,
because our host is itself an ARM emulator rather than a 64-bit process
smuggling ARM32 in:

| Reference component | Our equivalent |
|---|---|
| `a32_*` ARM32 interpreter/JIT | **`wie_core_arm`** — we already interpret ARM. |
| `host_call` interpreted-ARM→native bridge | **SVC + `binary_patches`** HLE dispatch — we already trap ARM→Rust. |
| `libla.so` low-32-bit-address reservation | **not needed** — our emulator has its own 32-bit address space. |
| `libarm32_raptor.so` loader + `_MC_knl*` | `load_executable`/`apply_relocations` + `wie_lgt` kernel stubs (partial). |
| `raptor_er` ELF helper | our ELF loader. |

So the work is not "build an ARM interpreter and a call bridge" (the reference's
two biggest components) — those exist. It is "load and relocate one more ARM32
ELF into the address space we already run, bind its imports to the HLE we already
dispatch, drive its init, and tap its output."

### Firmware image (`libarm32_lgt_system.so`)

- ARM 32-bit ELF, `ET_DYN`, entry `0xe4c38`.
- Two LOAD segments: `R E` text (~3.4 MB) at vaddr 0, `RW` data/bss
  (memsz ~1.6 MB) at `0x348ad4`. ~5 MB memory image, based at 0.
- ~25,880 relocations, almost all `R_ARM_RELATIVE` (`RELCOUNT` 25,875), plus a
  small PLT (`JMPREL`, 1,032 bytes).
- `NEEDED`: `libla.so`, `libdl.so`, `libc.so`, `libm.so`, `libstdc++.so`.
- Init/entry symbols of interest: `MH_sysHalInit`, `LGTH_themeInitialize`,
  `LGTH_zoopInit`, `InitPCSAutomata`, `dlet_start` (the DLET/clet entry).
- It is the whole middleware: alongside the platform C API it exports the entire
  CLDC/MIDP class library as native code with JNI-style names —
  `Java_java_lang_StringBuffer_*`, `Java_java_io_ByteArrayOutputStream_*`,
  `Java_com_lgt_MediaDeviceManager_mda*` (audio), `Java_com_velox_*` (networking),
  `Java_com_sun_cldc_io_*`. Reimplementing all of this by hand is the open-ended
  chase; running it is the point of this plan.

### What the firmware needs from the host (the HAL)

Confirmed against the binary: **134** undefined `FUNC` imports, and every one but
three is standard C runtime:

- `__aeabi_*` soft-float / integer-division helpers,
- libm (`sin`, `cos`, `pow`, `floor`, `sqrt`, `atan2`, …),
- libc (`memcpy`, `snprintf`, `open`/`read`/`write`, `opendir`, `mkdir`,
  `__android_log_print`, `dlopen`/`dlsym`, …),
- POSIX mutexes and semaphores (`pthread_mutex_*`, `sem_*`, `pthread_self`),
- setjmp/longjmp, `srand48`/`lrand48`.

The three custom ones are the LGT allocator: **`la_cal`**, **`la_mal`**, **`lafr`**
(calloc / malloc / free). They are `UND` in the firmware too — the reference's
loader binds them to the raptor kernel allocator (`_MC_knl*`). For us they are
three more HLE handlers over a host allocator.

So the HAL is "provide a C runtime to emulated ARM code, plus three allocator
functions." This is high-level emulation of libc: when the firmware calls
`malloc` or `cos`, the host runs its own. We already do this for the game binary,
so most of the surface exists.

### How the firmware's output reaches Android (in the reference)

- Audio: the firmware's MA-3 path (exports `AND_mdaInit`,
  `Java_com_lgt_MediaDeviceManager_mdaSetDevInfo0`, …) fills a PCM buffer; the
  aarch64 bridge hands it to Java via `GetMethodID` + `CallVoidMethod` (the bridge
  registers only a handful of JNI natives — `startWipiN`, `pltEventN`,
  `pltChangeStateN`, `JavaThread_runN`, `FrameSurfaceView_*` — and pushes audio
  and frames *back up* into Java through these callbacks). That feeds an
  `AudioTrack`.
- Video: the bridge exposes `FrameSurfaceView` (`android_lgt_wipi`) — the firmware
  renders into a framebuffer the bridge blits to a `SurfaceView`.

The reference's output tap is a Java up-call; **ours is simpler** — we tap the
same firmware buffer straight into `AndroidAudioSink` / `GameView` in Rust, with
no JNI round-trip. The exact firmware-side buffer address and fill call are the
one thing that must be pinned on device (see open questions).

### Where we already are

`wie_lgt` is much closer to this than it looks:

- It has a full ARM ELF loader and relocator (`load_executable`,
  `apply_relocations`, `load_native` in `runtime/init.rs`).
- **It carries a platform address map**: `runtime/java/platform_metadata.rs`
  lists platform methods with firmware addresses (e.g. `cos → 0x00160178`,
  `mdaSetDevInfo → 0x001bb32c`). This is a strong head start on name→address for
  the whole platform — but it is **not** the firmware's export table and must be
  re-derived against the exact loaded binaries (see the findings section: some of
  these addresses are PLT stubs or raptor addresses, not firmware internals).
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
- Where the firmware's `la_cal`/`la_mal`/`lafr` allocator should point. In the
  reference these resolve into the raptor kernel allocator (`_MC_knl*`); for us
  they are simplest as three HLE handlers over a host allocator. (`libla.so`
  itself is `LegacyAddressCompat`, a low-32-bit-address reservation for the
  reference's 64-bit process — not the allocator, and not something we need.)

These are all answerable from the reference's own `liblgt_system.so` bridge,
which contains the other side of every one of these interfaces.

## Findings from the RE passes (complications)

Two static RE passes over the reference libraries sharpened the plan and turned
up where it is *not* plug-and-play, despite the "symbol map for free" hope above:

- **The platform is more than one binary** (corrected). The stack is
  game → **`libarm32_raptor.so`** (loader + `_MC_knl*` kernel services) →
  **`libarm32_lgt_system.so`** (firmware), all under the `a32_*` interpreter in
  `liblgt_system.so`. (An earlier note called the middle layer a
  `raptor-carrier.mod`; it is the `libarm32_raptor.so` above.) The firmware's
  `la_cal`/`la_mal`/`lafr` and its `_MC_knl*` calls resolve *into the raptor
  layer*, so a loader has to place and link the raptor alongside the firmware, not
  the firmware alone.
- **`platform_metadata` addresses do not map 1:1 to the extracted firmware.**
  `cos` (metadata `0x160178`) is `UND` in the firmware (a libm import / PLT stub),
  and `mdaSetDevInfo` (metadata `0x1bb32c`) is not a firmware export at all
  (the export is `Java_com_lgt_MediaDeviceManager_mdaSetDevInfo0`). Firmware-
  internal symbols like `MH_sysHalInit` (`0x18529c`), `dlet_start`, and
  `InitPCSAutomata` *do* match. So the metadata addresses are a mix (firmware
  internals, PLT stubs, and probably raptor addresses) tied to a specific
  reference build's combined image, not to any single binary. They cannot be
  assumed to be the firmware's export table, and must be re-derived against the
  exact binaries that get loaded.

None of this changes the direction — and the second pass strengthened it by
confirming we already own the reference's two hardest components (the ARM
interpreter and the HLE-call bridge; see "the crucial simplification" above). But
the loader/link step needs the raptor and the firmware placed together and their
versions reconciled, and the metadata table re-derived against the exact binaries
that get loaded. That is genuinely multi-binary, version-sensitive reverse
engineering, and it can only be brought up and verified by building and running
the emulator on a device with the real firmware present — not from static
analysis alone. This document is therefore the implementation spec; the bring-up
loop (load → relocate → init → tap audio → diff against the reference) is the
on-device work it hands off to.
