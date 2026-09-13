# Gap map

What this runtime does not implement yet, and what the reference emulator says
about whether it matters.

Two sources feed it. The first is our own unimplemented surface, which is
certain: a stub is a stub whatever anyone else does. The second is the
reference emulator (WIPI-X 0.1.7), which is evidence about which of those gaps
real titles actually reach.

Keep it honest: a row here is either measured or marked as not.

## What the reference is, and what it is not

The reference ships a build manifest (`assets/WIPI-X-BUILD-INFO.json`) listing
431 source paths and 105 patches with ids like `wipix-lgt-util-ntohs`. That
list is its **bug history**, not a specification. Three things follow:

- It is not a backlog. A patch id says a symptom class existed in their
  implementation, not that the same symptom exists here. We have fixed several
  of them independently - `wipix-lgt-util-ntohs` is `wie_wipi_c/src/api/util.rs`,
  sign extension and all, arrived at from 붉은보석's billing reply.
- Ten of the 105 are their architecture only: their frontend, the ebiten engine
  they embed, their Android audio and input paths.
- The rest are worth reading *when a title misbehaves*, as a list of places to
  look. Working from the list instead of from a symptom is how a week gets
  spent on something the reference turns out not to do at all - see the
  collector note below.

Its platform packages map to ours: `internal/ktf` is KTF, `internal/raptor`
plus `internal/wipi` is LGT, `skvm`/`skvmhost` is SK-VM, `cpu/interpreter` is
the ARM core.

## Settled: KTF Java objects are never collected

Not a gap. The reference builds KTF Java objects in guest memory exactly as
this runtime does and has no collector for them: the only `collectGarbage` in
its binary belongs to its SK-VM, there is no free, destroy or reclaim of a KTF
Java object anywhere in it, and its one KTF root-visitor covers strings for
state snapshots. Its LGT-equivalent *does* collect
(`CollectUnusedJavaStrings`, `DestroyRaptorJava`), which is the same split this
runtime has.

So `JavaClassInstance::destroy` freeing nothing for KTF is the design, and a
KTF title's heap growing is what the reference does too.

## Certain gaps: our own unimplemented surface

| area | count | what a call does today |
|---|---|---|
| KTF WIPI-C table slots | 77 | `WieError::Unimplemented` - kills the title |
| `wie_wipi_c` stubs | 17 | logs and answers benignly |
| WIPI-Java stubs | 53 | logs and answers benignly |
| SK-VM / SKT stubs | 20 | logs and answers benignly |

The KTF row is the serious one, because those abort rather than answer.

### KTF: 65 were already implemented, and are now wired

`wie_wipi_c` implemented these and LGT wired them; KTF's table did not, so a
title calling one died on the call. Wiring was mechanical - each aborting slot
already carried the name of the function it should call, e.g.
`gen_stub(3, "MC_netSocketConnect")` beside `net::socket_connect`.

| module | implemented | KTF wired | KTF wires now |
|---|---|---|---|
| net | 35 | 3 | 30 |
| uic | 43 | 10 | 43 |
| util | 6 | 1 | 6 |
| graphics | 44 | 32 | 37 |

Five more graphics calls turned out to be the same case and are wired too:
`MC_grpGetContext`, `MC_grpGetUnicodeStringWidth`, `MC_grpDecodeNextImage`,
`MC_grpFillPolygon`, `MC_grpDrawPolygon`.

Three of the remaining families have since been written and wired:
`MC_knlCreateSharedBuf` and its four companions (`wie_wipi_c/src/api/shared_buf.rs`),
the three `MC_miscGetLedCount`/`SetLed`/`GetLed` calls, which answer that this
handset has no LEDs, and the ten program-control calls below.

**Kernel program control** (`MC_knlExecute`, `MC_knlMExecute`, `MC_knlLoad`,
`MC_knlMLoad`, `MC_knlProgramStop`, `MC_knlGetExecNames`,
`MC_knlGetProgramInfo`, `MC_knlGetParentProgramID`, `MC_knlGetAppManagerID`,
`MC_knlGetAccessLevel`) starts and stops sibling programs, which this runtime
cannot do: one title is loaded and nothing can install or start another. So they
answer the world as it is - one program, id 1, no parent and no application
manager - and say "no such program" to everything else, without touching the
buffers they are handed, since no title we have pins their shapes down.
`MC_knlProgramStop` on the title's own id is the exception that does something:
it is a request to quit, and is honoured like `MC_knlExit`.

`MC_knlGetAccessLevel` is the one where a wrong answer could have a title refuse
itself work, so it reports what the title's own `__adf__` declares in `SLvl`
(`00142F9C` in 투스워즈, `00142F1C` in 드래곤하트) rather than a level nobody
wrote down. The reference does not implement this family at all - no
`knlExecute`, `knlLoad`, `ProgramStop` or `AccessLevel` symbol appears in its KTF
package - which is the measure of how rarely a title reaches it.

**Input method** (`MC_imHandleInput`, `MC_imSetCurrentMode`,
`MC_imGetCurrentMode`, `MC_imGetSupportModeCount`, `MC_imGetSupportedModes`) is
now shared in `wie_wipi_c/src/api/im.rs` and wired into both platforms. An
earlier note here said LGT's versions would not port because two of them take
`(a0, a1, a2, a3)` - that was wrong, and reading them settled it: those four
words are LGT's dispatch scaffolding and native reads none of them, so the calls
are nullary, which is a shape, not an unknown. The other three were already
pinned down - `MC_imHandleInput` takes `(key, event, output0, output0_len,
output1, output1_len)` - and LGT native implements the standard calls rather
than anything of its own.

The four modes (`EN/S`, `EN/L`, `N123`, `KO`) are confirmed twice over: the
reference's string table carries exactly `EN/S\0EN/L\0N123\0KO\0` beside its
`setCurrentMode(I)Z`, and the shared UIC text component KTF already runs cycles
modes modulo 4.

**Database** (`MC_dbGetAccessMode`, `MC_dbGetNumberOfRecords`,
`MC_dbGetRecordSize`, `MC_dbSortRecords`, `MC_dbListDataBase`) has `*_lgt`
variants, but they read LGT's own `.idx`-equivalent metadata, which a KTF
database does not have - they would answer -1 for every KTF handle. So these are
`*_ktf` variants written against KTF's own model, which is one record read and
written as a byte stream: the record count is the one `MC_dbListRecords` would
list, the record size is how many bytes that stream holds, and sorting one record
succeeds without a comparator because one record is already sorted.
`MC_dbGetAccessMode` takes either an open handle or a name - both arrive as one
word, and a handle is recognised by the magic this runtime writes at the front of
it, the same trick KTF's slot 6 already needed - so neither reading of the ABI
has to be guessed at.

**Graphics** (`MC_grpDrawUnicodeString`, `MC_grpEncodeImage`) was the real work
of the four groups. The first is `MC_grpDrawString` reading UCS-2 instead of
EUC-KR; both now share one drawing path, and `MC_grpGetUnicodeStringWidth` was
already there to measure with.

`MC_grpEncodeImage`'s contract came out of the reference emulator's
`ktf.ktfWIPICGraphicsEncodeImage`, disassembled: six arguments
`(src, x, y, w, h, out_len)`; `*out_len` cleared before anything else and written
again only on success; `x`/`y` not negative, `w`/`h` positive, and `x + w` /
`y + h` inside the framebuffer; `image/bmp` as the encoding; empty or past 32 MiB
refused; and on success a freshly allocated guest buffer whose address is the
return value. The BMP is 24-bit bottom-up BGR by the same rules as
`org.kwis.msp.lcdui.Graphics.encodeImage`, which was derived from the same native
encoder - so a title that saves a screenshot through either door gets the same
file.

What is left in KTF's table is OEM extensions (`OEMC_knl*`, `OEMC_grp*`) and
slots whose names nobody has recovered - `MC_knlReserved2..13`, `MC_dbUnk13..15`,
`MC_mdaUnk*`. Every standard call in it that has a name is answered; the rest
cannot be written until a title reaches one and says what it wanted.

The reference implements the same APIs - its shared WIPI runtime dispatches
`dispatchUIC`, `dispatchNetwork` and `dispatchUtility` by index - so these are
live APIs, not dead table space.

Caveat worth stating: no KTF title we have reaches these slots, so wiring them
is verified by tests, not by a title. KTF's own ABI decides which numeric slot
is which function; the slot labels in the table are what encode that, and they
are what the wiring must follow.

## Reference patches worth reading when a title misbehaves

Not a to-do list. Each names a symptom class the reference had to handle.

KTF: `wipix-ktf-paint-capacity`, `wipix-ktf-stale-card-repaint`,
`wipix-ktf-handset-key-repeat`, `wipix-ktf-java-input-method` (16 sites),
`wipix-ktf-c-text-component` (11), `wipix-ktf-clip-completion-listener`,
`wipix-ktf-local-star-purchase`, `ktf-wipic-put-count`,
`wipix-ktf-input-effect-work-budget`.

LGT Java ABI: `wipix-lgt-java-application-class-layout`,
`wipix-lgt-java-array-type-abi`, `wipix-lgt-java-platform-virtual-slots`,
`wipix-lgt-fixed-platform-vtable-precedence`,
`wipix-lgt-java-superclass-materialization`,
`wipix-raptor-java-string-char-array-slot`,
`wipix-raptor-java-wide-store-word-order`,
`wipix-raptor-java-zero-layout-class`.

LGT graphics: `wipix-lgt-image-alpha-mask` (13 sites),
`wipix-lgt-platform-graphics-heap` (32), `wipix-wipi-lcd-flush-region` (16),
`wipix-lgt-pixel-operation-order`, `wipix-tempest-blend-pixel-operation`.

Audio: `wipix-smaf-mixed-track-streaming`, `wipix-smaf-setup-ram-pcm`,
`wipix-raptor-java-audio-source-mute`.

CPU: `wipix-cpu-native-branch-exchange`, `wipix-cpu-read-only-leaf-traps`,
`wipix-native-tlb-refill-classification`, `wipix-application-cpsr-memory-cache`.

## Not ours

`wipix-native-audio-probe`, `wipix-audio-underrun-rebuffer`,
`wipix-audio-sync-telemetry`, `wipix-audio-sample-cursor-jitter`,
`wipix-guest-vibration-gate`, `wipix-ownership-persistence-context`,
`wipix-native-playback-speed-audio`, `wipix-android-digital-triggers`,
`wipix-input-execution-isolation`, `wipix-android-audio-read-chunks` - their
frontend, their audio backend, the ebiten engine.

## Measured against another implementation's source

`wfeature` (MIT) is a Go runtime covering the same three platforms, and unlike
the binaries above it ships its source, its tests, and ~8,000 lines of written
findings. Everything below is a comparison that was run, not a reading.

**Its KTF LWC is mostly stubs** - `Component.getWidth`/`getHeight` answer zero,
`ContainerComponent.layout`/`validate` do nothing - because it routes text entry
to the host's own keyboard instead of drawing a widget. Ours is the fuller one.
It is the better reference for the platform *outside* the toolkit.

| compared | result |
|---|---|
| KTF class surface | no gap. Its `apiscan` reads the client image's name pool; ported and run over our five archives, each names 43-65 platform classes and the only one unpublished here is `PluginJlet`, which every archive names and none extends. |
| WIPI Java surface | no gap. All 36 reference classes are published and every reference method resolves. `Clip.setBuffer` was moved to `BaseClip` and made return `Z` independently on both sides. |
| C runtime hooks | no gap. memcpy/memset/strcpy/strlen are recognised in the image and replaced with native stubs on both sides; ours is `wie_core_arm/src/binary_patches`, applied over the whole KTF image. |
| a container answering zero children | not ours. Their adds and reads kept two types in one field; ours reads and writes `children`/`childCount` one way. |
| a card forwarding a key to its own text field | not ours. `TextComponent.keyNotify` already runs the key through the input method. |
| `Display` capability answers | **gap, fixed** - `isColor` and `numColors` were placeholders answering `false` and `0`. |
| multi-tap commit delay | **gap, fixed** - we had none, so the same letter twice could not be typed. |
| a frame loop asking for no frame period | **measured, not applicable** - see below. |

### A floor on the wait after a frame, and why it is not here

A title can put its whole frame loop on a guest thread - repaint,
serviceRepaints, sleep, round again - and ask for a sleep of nothing. Answered
literally, the loop redraws as fast as the executor allows and every redraw but
the last is thrown away. Their fix marks the thread that published a frame and
raises only that thread's next wait to a frame period; one of their archives was
publishing 4,807 frames for each one collected, and the fix took sixty rounds
from 2m 6.6s to 1.2s.

The mechanism is real and we have no equivalent. It was written, measured and
removed again, because on this corpus it changes nothing. Instrumenting
`System::sleep` over Bigi 미궁 - our slowest title at ~10ms a tick, and the only
one that drives its own frame loop - counted 232 frames published and 10,183
sleeps in 2,000 ticks:

| wait | follows a frame | count |
|---|---|---|
| 1ms | no | 8,908 |
| 16ms | no | 822 |
| 80ms | **yes** | 182 |
| 16ms | **yes** | 50 |

Every wait that follows a frame already asks for 16ms or more, so the floor
never fires; the waits that dominate the run do not follow a frame, and their
rule deliberately leaves those alone - a loader sleeping between chunks has
drawn nothing, and flooring it would cost it a frame per chunk. Adding the floor
would have put a lock on a path taken ten thousand times per two thousand ticks
to change nothing.

**What the same measurement did turn up** is that Bigi 미궁 asks for `sleep(1)`
8,908 times in 2,000 ticks - four and a half times a tick, from a task that
never draws. That turned out to be ours rather than the title's: 8,893 of them
came from the clip-completion watcher, which read a flag every millisecond that
the audio layer's own watcher only writes every fifty. Reading it at a frame
instead takes the same window from 8,893 polls to 195.
