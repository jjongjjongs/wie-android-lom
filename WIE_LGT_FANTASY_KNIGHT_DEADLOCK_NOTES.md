# WIE_LGT — Fantasy Knight / Battle Monster worker deadlock (g3 / g6)

Investigation handoff. Two LGT titles (Fantasy Knight `00026F54`, Battle Monster
`00025C2B`) boot to a black screen. Both run on the WPP reference firmware
(`liblgt_system.so`), so the game bytecode is correct and the gap is in our
runtime. This documents how far the diagnosis got and the exact next step.

## Fixed already (committed)

**Own-field resolution when `field_offsets` is the last `.bss` array.**
`field_offset_capacity()` returned 0 when the compiled class's `field_offsets`
output array is the highest one in `.bss` (no neighbour to bound it), so
`resolve_own_fields` never filled the trailing rows. Fantasy Knight reads row 45
of that array — the slot index for the boolean field `Game.r` — and with it
left 0 the compiled `ldrsh [meta+0x5a]` read slot 0 instead (a live `Display`
reference that is never null), so the worker's `while(field != 0) wait()` could
never terminate. Fixed by bounding the walk with the containing image section's
end (`context.image_ranges`) when there is no neighbour array. See
`wie_lgt/src/runtime/init.rs`, the `JavaLoadClasses` handler (`field_end`).
Result: the worker now reads the correct field. Regression suite green (73).

## The remaining deadlock (NOT fixed)

After the field fix the worker reads the right field but still parks forever.

### Thread / control model (Fantasy Knight, headless, `Game` = MClass Jlet)

- `Game.<init>` (code `0x2074`) unconditionally sets `Game.r = 1` (field slot 2,
  resolved via `meta+0x5a`; `meta` = `.bss` `0x1500a30`). It also calls
  `BackLight.alwaysOn()`. `r = 1` is the correct initial state.
- `Game.startApp` (code `0x220c`) is minimal — its real SVC sequence is:
  `getClass → Runtime.getRuntime → Runtime.gc → Math.min×2 → new Random()
   → Display.getDefaultDisplay() → new Thread(this) → Thread.start()`.
  It does **not** push a card, register a listener, or set up event delivery.
- Worker `Game.run()` (code `0x262c`) is the only game thread:
  `synchronized(this){ try { while(Game.r != 0) wait(); … } }`
  (the `wait()` returns to `lr = 0x2740`; the `id 0x21` setjmp calls are the
  try-block, returning 0 is correct).
- Thread 1 (main / startApp) ends. Thread 3 = `net.wie.EventLoopRunner`, idle.

### Root cause pinned down

`Game.r` is cleared to 0 by exactly one method: **`Game.b(Lw;I)V`** (code
`0x2900`, dispatch slot 34 / vtable offset `0x88`). It takes a card (`w` =
`Game`'s `Card` subclass, `w extends org/kwis/msp/lcdui/Card`) and an int,
stores the event params into `field[0x52]/[0x50]`, and sets `Game.r = 0` (at
`0x29f8`, with `r5 = 0` from `0x2954`) when a guard `fn(this)==0` holds. It is
the game's per-frame / per-event wake callback.

**`Game.b` is never invoked in our runtime** (verified by tracing SVCs whose
`lr` falls inside `0x2900..0x2954`). The descriptor `(Lw;I)V` appears only in
`Game`'s own method table (no other class imports it), so it is an own-virtual
call the game makes from a card-event path — a path that never starts because:

- No game card (`g extends w extends Card`, code roots `0x1401688` etc.) is
  created or shown at bootstrap (the worker would do that only after passing the
  `r` wait — a bootstrap chicken-and-egg).
- The game's own `org.kwis.msp.lcdui.EventQueue(Game)` (created in startApp)
  is never polled — the worker that would poll it is parked, and our
  `EventLoopRunner` polls `net.wie.EventQueue` and dispatches to `CardCanvas`
  (which has zero cards), so no dispatch reaches game code.

Poking `Game.r = 0` once does not self-sustain (the worker renders once, sets
`r = 1`, re-parks; `Game.b` still never fires). Poking it continuously corrupts
state (the event params `Game.b` would set are missing) and the game spins in an
init loop, never rendering.

### The wake chain (fully traced)

`Game.r` is cleared through a fixed game-internal wrapper chain, all reached
only from the game's event/callback handlers:

```
platform event  ->  <handler>()  ->  M(card,int)  ->  Game.b(card,int)  ->  Game.r = 0  ->  worker wakes
                        (7 sites)      0x288c           0x2900
```

- `Game.b(Lw;I)V` `0x2900` — sets `Game.r=0` (only r=0 writer; confirmed the two
  other `meta+0x5a` sites `0x36098`/`0xbbc68` are *method-slot* dispatches in
  other classes, not r writes).
- `M` `0x288c` — fetches the game singleton (`0x2030`), and if it exists calls
  `Game.b(card,int)`. Private helper, no `.data` table entry.
- `M` is loaded (via literal-pool pointer, not `bl`) and called from **7 game
  methods**: `0x13a8`, `0x1045c`, `0x11cac`, `0x7d9f8`, `0xb46a4`, `0xb4dd0`,
  `0xd2570`. These are the game's event handlers.
- `0x13a8` is a named method `b()V` on the class at classptr `0x140004c` (holds
  the screen cards as static fields `a:Lk; b:Lv; c:Lg; d:Le;` and has
  `<init>/a()/pauseApp/resumeApp`). It reads a card from a field and calls
  `M(card,-1)`.

None of the 7 handlers runs in our runtime, because the platform never delivers
an event to the game's card/handler (no card is registered at bootstrap, and
our `EventLoopRunner` dispatches to an empty `net.wie.CardCanvas`). So the whole
chain never fires and `Game.r` stays 1.

### Class hierarchy (resolved)

Names read from the class metadata (`name` at `metadata+0x08`,
`super` at `metadata+0x10`, `metadata = root - 0x4c`):

- `Game` (root `0x140004c`) **extends** `b` (root `0x1400fdc`) extends platform
  Jlet. `Game` holds the screen cards as static fields (`a:Lk; b:Lv; c:Lg;
  d:Le;`).
- `b` implements `java/lang/Runnable`; it owns `run()` `0x262c` (the worker),
  the `r` field, `startApp` `0x220c`, and `b(Lw;I)V` `0x2900` (the r=0 setter),
  `c(Lw;I)V` `0x2a58` (present).
- Cards `e/g/k/v` (roots `0x1401330/0x1401688/0x14022ec/0x1402a2c`) extend
  `w` (`0x1402b9c`) extends platform `Card`.

So the worker runs `b.run()`, the `r` field is `b.r`, and its r=0 setter is
`b.b(Lw;I)V`. The 7 wake handlers, by owning class:

| handler          | class | note |
|------------------|-------|------|
| `Game.b()V` `0x13a8`  | Game  | **bootstrap candidate** — the only non-card handler |
| `e.a()V/c(I)V`        | card  | per-card, needs the card shown |
| `g.c(I)V`            | card  | " |
| `k.a()V/c(I)V`       | card  | " |
| `v.c(I)V`            | card  | " |

### Reference dispatch mechanism (found)

The reference `EventQueue.dispatchEvent_v0` (`0x1f95fc`) is a full event-type
switch on the event fields (`getDefaultDisplay` at `0x1ef180` into `ip`, then
`switch(event subtype r4)`), NOT the thin delegate our `org.kwis.EventQueue`
uses. On a **repaint event (subtype `0x29`=41, handler `0x1f9928`)** it does:

```
0x1f9950  mov r0, ip            ; ip = getDefaultDisplay()
0x1f9954  ldr r3, [ip]
0x1f9960  ldr pc, [r3, #0x88]   ; call display.vtable entry at byte offset 0x88
```

The compiled dispatch uses `vtable[slot*4 + 4]` (a one-word header), so byte
offset `0x88` is **slot 33 = `Display.serviceRepaints(Z)`** (confirmed against
`platform_metadata.rs`, which reads the reference dt_ tables; slot 34 =
`repaint(Card)` at offset `0x8c`, and `Display.repaint_v0` `0x1efb80` is a no-op
for a null card). So the reference services the repaint synchronously
(`serviceRepaints(false)`), painting the current displayable — the game's own
card/Jlet code — which reaches `M -> Game.b -> b.r = 0`.

CAVEAT: painting an *empty* surface would not wake anything, so the bootstrap
first-wake (before any card is shown) is still not fully pinned. Either the
reference's default current displayable for a Jlet routes paints to the Jlet's
own handler, or the first wake comes from a different event path in
`dispatchEvent` (its type/subtype switch, e.g. the `getActiveJlet` +
`display.vtable[0x4c]` = slot 18 branch at `0x1f9644`, still to be decoded). The
static reference RE here is error-prone (layered wrappers, runtime-built vtables
whose raw dt_ words are descriptors, not code); the offset/slot mapping must be
recomputed with the `*4+4` header each time.

Our path is `net.wie.EventQueue.dispatchEvent -> RepaintEvent ->
Display.handlePaintEvent -> net.wie.CardCanvas.paint`, and `CardCanvas` has zero
cards, so nothing game-side runs. **That is the divergence**: the reference
paints the game's own displayable; we paint an empty wrapper. The game never
`setCurrent`s a card of its own (its startApp is minimal), so on the reference
the *current displayable for a Jlet must default to the Jlet/its main surface*,
whose `vtable[34]` is the game paint/handler.

### Ground-truth runtime trace (our emulator, g3 `00026F54.jar`)

Ran g3 headless (`screen_capture::capture_archives`, 1200 ticks,
`RUST_LOG=wie_wipi_java=debug,wie_midp=debug`). The trace confirms the whole
diagnosis end-to-end and pins the divergence in *our* code, not just the
reference:

1. `Game.<init>` → `Jlet.<init>` → `Display.<init>` (creates a `net.wie.CardCanvas`
   and `MidpDisplay.setCurrent(cardCanvas)`) → `EventQueue.<init>`.
2. `Game.startApp` calls only `getDefaultDisplay` ×2, `Jlet.getActiveJlet`,
   `getDisplay(null)`. **No `pushCard`, no `addJletEventListener`, no card
   created** — the "minimal startApp" the static RE predicted, now confirmed at
   runtime. (The worker `Thread` is started and parks on `b.r` immediately.)
3. From then on the only activity is `net.wie.EventLoopRunner` pumping
   `net.wie.EventQueue.getNextEvent/dispatchEvent` → the **MIDP**
   `Display.handlePaintEvent` → `Canvas.handlePaintEvent` → `net.wie.CardCanvas.paint`.
   `CardCanvas` holds **zero** cards, so every frame paints one flat colour
   (30 frames, "busiest frame … with 1 distinct colour" = black).
4. The intro OK key reaches `Canvas.handleKeyEvent → CardCanvas.keyPressed(148)`
   and dies there (no card to receive it).

The decisive observation: **the game's own `org.kwis.msp.lcdui.EventQueue(Game)`
is never pumped by anyone**, and the `net.wie` pump never routes to any
`org.kwis` game handler — it only paints the MIDP `CardCanvas`. The game's
worker (which is the only code that would ever `getNextEvent`/`dispatchEvent` on
the game queue, or push the first card) is parked before it runs a single
instruction. Chicken-and-egg, confirmed empirically.

Contrast with card-based titles (Seed, Zenonia), which **do** `pushCard` during
their own startup, so the `CardCanvas` has a card and painting it runs game
code. g3/g6 push nothing, so nothing game-side ever runs.

### The worker is a pure consumer — instrumented dispatch census

Instrumented every LGT dispatch path in `init.rs` (imported static/virtual/
interface, unknown-slot, object-self) and ran g3 for 400 ticks. The entire
platform-call census the game makes is:

- **Start-up only (once):** `Runtime.getRuntime`, `Runtime.gc` (slot 13),
  `Math.min` ×2, `Random.<init>`, `Display.getDefaultDisplay` ×2, `Jlet.<init>`,
  `BackLight.alwaysOn`, `Thread.<init>`, `Thread.start` (slot 10). That is the
  minimal `startApp`, and it confirms **the worker thread is actually created
  and started** (`Thread.start` fires).
- **Steady state (forever after):** `Game.getClass()` (Object slot 1) ×2, then
  `Game.wait()` (**Object slot 9**) ×~81 and nothing else. No WIPI-C SVC of any
  kind (`wipic svc` count = 0), no `EventQueue`/`Display`/`Card` virtual, no
  event-array poll from the game side.

So the worker's whole life is `synchronized(this){ while(this.r != 0)
this.wait(); … }` — a pure spin on `Game.wait()`, timed-bounded to 20 ms by our
runtime, re-reading `Game.r` (always 1) each time. **It never reaches an event
poll and never calls a single platform method beyond the wait.** The
`VmInstantiateArray`/`aastore` (~16/s) seen in the init-SVC stats come from the
`net.wie.EventLoopRunner` painting the empty `CardCanvas`, not from the worker.

Two hard conclusions from this census:

1. **`Display.vtable[34]` — and every `Display`/`Card`/`EventQueue` dispatch slot
   — is never invoked in our runtime.** The game issues no virtual dispatch on
   any platform lcdui object at all; the worker only spins on `Game.wait()`. So
   the runtime answer to "what does `Display.vtable[34]` route to" is: *it is
   never reached* (our metadata would route slot 34 → `repaint(Card)`, but the
   game never gets there). The divergence is upstream of any Display call.
2. The worker is a **pure consumer**. The `Game.r = 0` **producer** (`Game.b` →
   `notify`) is never the worker's own job — on the reference it is driven by the
   platform's event thread calling *into* the game. Our `net.wie.EventLoopRunner`
   is that thread, but it only paints the empty `CardCanvas` and has no channel
   that invokes the game's Jlet/card handler. **That absent channel is the whole
   bug**; there is no game-side code path that could self-start.

### `Game.b()V` (`0x13a8`) is invoked purely by runtime virtual dispatch

Scanned the whole `binary.mod` (BL targets + literal-pool words, bias
`vaddr = file + 0xFCC`) for references to `0x13a8`: the **only** hit is its own
28-byte method-descriptor entry in `.data` at vaddr `0x14000e8`
(`+0x00`=classptr `0x140004c` Game, `+0x04`=name, `+0x08`=desc `()V`,
`+0x0c`=flags `0x10011`, `+0x14`=code `0x13a8`). There is **no `bl 0x13a8` and no
literal `0x13a8`** anywhere else, so `Game.b()V` is reached *only* through a
compiled `ldr pc,[recv,#slot*4+4]` whose code word is filled from this table at
class-activation time. That is why an xref search can never find its caller, and
why the "who calls the bootstrap handler" question is unanswerable by static
xref alone — it needs the activation/slot-assignment logic simulated.

Corollary, checked against `platform_metadata.rs`: **`Jlet` and `EventQueue`
have *no* virtual dispatch methods** (`DISPATCH_METHODS_257`/`_246` are empty;
`startApp`/`pauseApp`/`resumeApp` are non-virtual). So `Game.b()V` is **not** a
platform `Jlet` lifecycle override the platform could call directly — it is a
*game-internal* virtual the game invokes from its own event-dispatch path. The
reference `EventQueue.dispatchEvent` (`0x1f95fc`) opens with an **unconditional**
`r4 = eventqueue.field@8.field@0x10; lr=pc; ldr pc,[r4_class,#0x4c]` (slot 18)
*before* the event-type switch — a per-dispatch call into some object every time
an event is delivered. That, or the repaint→`serviceRepaints`(slot 33) path, is
where the game's own dispatch runs and eventually hits `Game.b()V`. The reframed
conclusion: **the fix is not "paint the empty CardCanvas differently"; it is
that dispatched events must reach the active Jlet's own `org.kwis` dispatch
(which then virtual-dispatches into game code), which our `net.wie`→MIDP pump
never does.**

### `Game.run()` (`0x262c`) decoded — the present/re-arm model

The worker is not a bare `while(r!=0) wait()`; it is an event loop with the
wait nested inside:

```
run():
  0x263c  call import id 0xf                 ; setup (monitor/exec-env)
  0x2650  b 0x2834
  0x2834  s = singleton(0x2030); if s.field@0x20 == 0 -> return   ; outer gate
  0x2654  <event-poll body: several .data-thunk imports>
          ... if poll yields nothing (r4==0) -> 0x2740
  0x2740  if Game.r (meta+0x5a, slot 2) != 0 -> 0x271c
  0x271c  this.wait()  via Object vtable slot 9 (offset 0x28)     ; TIMED wait
          (our runtime bounds a bare wait() to LGT_BARE_WAIT_BOUND_MS=20ms and
           returns — it does NOT block until notify, so the worker *spins*,
           re-checking Game.r every ~quantum)
  when Game.r == 0 (0x2764+):
    r7 = Game.field[meta+0x52]                 ; card param b() stored
    r4 = Game.field[meta+0x50]                 ; int  param b() stored
    Game.c(this, r7, r4)   ; c(Lw;I)V = 0x2a58 = present / buffer-swap
    Game.field[meta+0x5a] = 1                  ; re-arm: Game.r = 1
    ... loop back to 0x2834
```

So the confirmed cycle is: **wait until `Game.r==0` → present the card/params
that `Game.b` stored → set `Game.r=1` → wait again.** `Game.b(Lw;I)V` (`0x2900`)
is the producer (stores card→`field@0x52`, int→`field@0x50`, sets `Game.r=0`,
notifies); `run()` is the consumer. A single successful `Game.b` therefore
bootstraps exactly one present; sustained animation needs `Game.b` to fire once
per frame, which only happens if events keep reaching the game's handlers.

Two consequences for the fix:
- The worker's `Object.wait()` is already timed in our runtime, so the worker is
  *live and spinning*, not hard-blocked. It re-reads `Game.r` continuously. The
  missing piece is purely the `Game.r=0` **producer** side (`Game.b`), i.e. an
  event reaching a game handler.
- The worker's own outer loop (`0x2654`) polls events through **`.data` import
  thunks patched at load**, not through `org.kwis.EventQueue.getNextEvent` — the
  g3 runtime trace shows the worker never calls our Rust `getNextEvent`. So the
  game's event source in the compiled model is a lower-level import, distinct
  from the `net.wie`/`org.kwis` `EventQueue` our pump drives. Pinning which
  import that poll is (runtime trace of the worker thread's platform calls) is
  the next concrete instrumentation step.

### Firmware-linked mode is media-only (not an event-path reference)

`wie_lgt` can load the real BIOS (`libarm32_lgt_system.so`) when it is present
in the title's filesystem (`firmware_link::try_load_bios`), but that path only
binds the firmware's `MC_mda*` media exports and runs firmware init as a
side task — **the game still runs its org.kwis/event code on the Rust platform
either way** (`emulator.rs`: "the game runs on the Rust platform either way").
So there is no supported mode that executes the game's event model against the
real firmware; the deadlock is squarely in our Rust org.kwis/net.wie shim, and
the reference `liblgt_system.so` remains a *static* spec, not a runnable oracle
for the event path.

### The open question (next step)

Reproduce the reference's current-displayable routing for these Jlet titles:
determine what the reference registers as the current displayable when a Jlet
calls `getDefaultDisplay()` (peel reference `getDefaultDisplay` `0x1ef180` and
`activateCurrentDisplay0` `0x1f53d8`), and make our repaint dispatch invoke the
active Jlet's `vtable[34]` (the game paint/handler) rather than the empty
`net.wie.CardCanvas`. That is the concrete platform fix; keep it scoped to the
Jlet-without-pushed-card case so card-based titles (Seed, Zenonia) are
unaffected.

On the reference the worker's first `Game.r = 0` must come from somewhere — most
likely the reference's `Thread.start` / monitor / `EventQueue` bootstrap
delivers an initial event (or a `notify` + `r = 0`) to the game without a
pre-existing card. Our `net.wie` shim (`WIPIMIDlet` → `CardCanvas` →
`EventLoopRunner`, in `wie_midp`/`wie_wipi_java`) does not reproduce that.

**Concrete continuation:** disassemble the reference `liblgt_system.so`:
- `Java_java_lang_Thread_*` / the DTHREAD scheduler around `Thread.start`,
- `Java_org_kwis_msp_lcdui_EventQueue_*` (getNextEvent / dispatchEvent),
- `Java_org_kwis_msp_lcdui_Display_getDefaultDisplay_s0` (`0x1ef180`) and the
  Jlet activation (`Java_org_kwis_msp_lcdui_Jlet__4init_4_s0` `0x20d1a8`),

to find what path delivers the first event to a cardless Jlet and clears its
repaint/ready flag, then mirror it in our org.kwis event/card model. This is the
`vtable synthesis` / `port reference gaps` work, and must be reference-driven —
guessing at platform behaviour risks regressing titles that already run
(Seed, Zenonia).

## Reference entry points already mapped (`liblgt_system.so`, vaddr == file off)

The reference is heavily layered: nearly every `Java_*` symbol is a thin
save-point / generic-dispatch wrapper (`0x145130`, `0x198f38`, `0x138fb0`,
`0xef214`, `0x144fd0`) that forwards to a core. Peel via the `bl` chains.

- `Java_org_kwis_msp_lcdui_JletWrapper_startApp_s0` `0x20d7fc` — the reference's
  launcher (our `net.wie.Launcher` equivalent); thin dispatcher, calls
  `ldr pc,[r3,#0x30]` on a manager object with the Jlet as arg.
- `Java_org_kwis_msp_lcdui_Jlet_startApp_v0` `0x20d07c` → core `0x20e368`
  (forwards through generic dispatch id `0xc9`).
- `Java_org_kwis_msp_lcdui_Display_activateCurrentDisplay_v0` `0x1eecdc` →
  `_s0` `0x1ef594` → `Display_activateCurrentDisplay0` `0x1f53d8` (real logic).
  Prime suspect for what posts the initial paint/event to the active Jlet.
- `Java_java_lang_Thread_start_v0` `0x1613c8`, `Thread_run_v0` `0x1611ac`,
  `Java_android_lgt_wipi_JavaThread_runN` `0x178174`.
- Scheduler: `dscheduler_main` `0xeddf8`, `dscheduler_init` `0xedf40`,
  `dprocess_find_dthread_by_pthread_id` `0xeccb0`,
  `dscheduler_add_timer_queue` `0xee03c` (frame/timer pump candidate).
- `Java_org_kwis_msp_lcdui_Card_serviceRepaints_v0` `0x1ed5bc`,
  `Card_repaint_v0` `0x1ed740`, `Card_paint_v0` `0x1eeb00`.
- `Java_org_kwis_msp_lcdui_Display_getDefaultDisplay_s0` `0x1ef180`.

Note: even on the reference there is no card at bootstrap (the game's startApp
creates none), so `Game.b`'s *first* call is not a normal card paint — the
open question is which reference path (display activation / scheduler frame
pump / an initial posted event) invokes it (or a `notify` + `Game.r=0`) without
a pre-existing card. That is the crux to resolve next.

## Useful addresses (Fantasy Knight `g3mod/binary.mod`)

- `.text` vaddr `0x1000` = file offset `0x34` (so `file = vaddr - 0xFCC`).
- `.data` vaddr `0x1400000` (off `0xea0fc`), `.bss` vaddr `0x1500000` (0xa90).
- `Game` methods: `<init>`=`0x2074`, `startApp`=`0x220c`, `run`=`0x262c`,
  `b(Lw;I)V`=`0x2900` (wakes worker), `c(Lw;I)V`=`0x2a58` (present/bufswap).
- `Game.r`: field slot 2, index at class meta `0x1500a30 + 0x5a`.
- Method table entries are 28 bytes: `+0x04`=name ptr, `+0x08`=descriptor ptr,
  `+0x0c`=flags (`arg_words<<16 | link marker`), `+0x14`=code.

## Fix direction B — reproduce the platform event channel (chosen)

Option B (mirror the platform's uniform event-delivery channel, additively,
gated on the cardless-Jlet case, leaving the card path untouched) is the chosen
direction: it is game-agnostic (no per-title obfuscated-handler knowledge) and,
unlike a one-shot `Game.r=0` poke, it drives *sustained* frames, not just the
first one.

### Reference delivery channels mapped (`EventQueue.dispatchEvent` `0x1f95fc`)

- Opens with an **unconditional** call, every dispatch, before the type switch:
  `r4 = this.field@8 . field@0x10; r4.vtable[slot 18]` (`ldr pc,[r3,#0x4c]`).
- For `event[0] > 3` it reads `r4 = event[1]` (subtype) and, after
  `bl 0x1f4dd4` (`Display.get_raw_class`) and `bl 0x1eecb8`
  (`activateCurrentDisplay_v0`) **on every dispatch**, `ip = getDefaultDisplay()`
  then switches on the subtype: `0x29`=41 repaint → `0x1f9928`; `1`,`2`,`0x3e9`
  (1001), `0xbb8` (3000), `0x3ef` (1007)… Several arms end in
  `getDefaultDisplay().vtable[slot 36] = eventNotify(III)V` (`ldr pc,[ip,#0x94]`)
  or the repaint arm's `getDefaultDisplay().vtable[0x88]` (slot 33
  `serviceRepaints(Z)` under the `*4+4` convention, or slot 34 `repaint(Card)`
  under the raw-array convention — the one-word-header off-by-one is unresolved
  statically and must be settled by runtime probe).
- For `event[0] <= 3` it routes through the low-level event-manager singleton
  (`global[0x152210 + 0xa94]`, its `vtable[0x16c]/[0x178]`), not the display.
- The event array is `(type=event[0], subtype=event[1], p1, p2)`. **Our
  `net.wie.EventQueue` uses a flat `(code, subcode, key, 0)` encoding that does
  not mirror this two-dimensional (type, subtype) shape.**

### Why the three obvious org.kwis channels are all empty for g3

Confirmed at runtime and from metadata:
- **No card**: g3 never `pushCard`s (nothing in `net.wie.CardCanvas`).
- **No JletEventListener**: g3 never calls `addJletEventListener`, and its class
  chain `Game -> b -> Jlet` implements only `java/lang/Runnable` (read from the
  game's class metadata interface table) — it does **not** implement
  `JletEventListener`/`SystemEventListener`, so `Display.eventNotify ->
  JletEventListener.notifyEvent` reaches nothing.
- **No worker poll**: the worker only spins on `Game.wait()`.

So g3's bootstrap delivery is something the platform does **automatically** when
a Jlet's display is activated (the per-dispatch `activateCurrentDisplay_v0`
`0x1eecb8` / core `0x1f53d8` is the prime suspect), not something the game
registers. This is the piece static RE cannot finish (firmware plumbing,
runtime-built vtables, the `*4+4` off-by-one).

### Concrete B plan (empirical resolution of the delivery target)

1. Settle the delivery target by **runtime probe**, not static RE: in our
   runtime, for the cardless-Jlet case, drive the active Jlet (`Game`) through
   candidate receivers and watch for the worker waking (`Game.r -> 0`, a
   non-empty painted frame). Candidates, in order: the game's own bootstrap
   handler `Game.b()V` (`0x13a8`) invoked by its resolved own-virtual slot;
   `Display.eventNotify(III)`; the reference's per-dispatch
   `activateCurrentDisplay`. Our runtime already calls compiled game methods
   (startApp/run) — reuse that entry to make the incoming call.
2. Once the receiver is confirmed, implement it as an **additive** branch in the
   org.kwis/net.wie event path taken only when the active Jlet has no shown card,
   so `net.wie.CardCanvas`/MIDP (Seed, Zenonia, LoM) is untouched.
3. Regression-gate on Seed / Zenonia / LoM plus the `wie_lgt` suite; keep g3/g6
   as the positive cases.

### Empirical probe result: a direct handler poke is not enough

Ran a temporary experiment: from the event pump (`net.wie.EventQueue.dispatchEvent`,
the `EventLoopRunner` thread), on every dispatch invoke the active Jlet's own
bootstrap handler `Game.b()V` via `jvm.invoke_virtual(currentJlet, "b", "()V")`.

Observed: the call succeeds **once** (`invoked activeJlet.b()V OK`), and then the
title goes from 30 painted frames to **1** — the pump stalls after the single
invoke. So a direct handler poke is *not* a no-op (it reaches game code) but it
does **not** cleanly wake the title; it stalls it. Two (non-exclusive) causes,
both meaning the poke is the wrong shape for B:

- **No card yet.** `Game.b()V` reads the current card from a `Game` field and
  calls `M(card, -1)`; at bootstrap that field is null (no `Card` is ever
  instantiated — confirmed in the dispatch census), so it drives `Game.b(null,…)`
  and the worker's subsequent `Game.c(null,…)` present into a bad state.
- **Wrong thread / monitor.** `Game.b()V` opens with import `0xb` (a monitor/
  save-point primitive) and the worker's wake is a `synchronized`/`notify`
  handshake; calling it from the `EventLoopRunner` thread rather than a real
  event-delivery context can wedge the native monitor.

Conclusion: **B must reproduce the platform's *proper* event-context delivery —
the right thread plus the card-bootstrap path — not just call a game handler.**
The crux narrows to: *what platform action creates/authors the Jlet's first card
(or its equivalent surface) so that a delivered event has something to present?*
No `Card` is instantiated anywhere in g3's startup, so that authoring is
platform-driven on Jlet activation and is the specific piece still to pin.

### (a) classes.dex decompiled — the bootstrap has no Java-side trigger

Decompiled the reference WPP player's Android glue (`android/lgt/wipi/*` in
`ref/classes*.dex`, via androguard) to see how it drives the game. Result: it
does **not** author the first card or post any bootstrap event — that is all
native.

- `WipiPlayer$WipiThread.run()` just calls `WipiPlayer.startWipiN(aid, null)`
  (a native method). `startWipiN` runs the whole WIPI VM (the game's
  `Main.main` → Jlet lifecycle → `startApp` → worker) and blocks until exit.
- The only things the Java layer posts into native are `pltEventN(int, Object)`
  — **key** (`WipiEventManager.onKeyEvent`, `DirectionKeyOverlay.send`),
  **touch** (`onTouchEvent`), **exit** (`exitPlatform` posts type 0), and
  **new-intent** (type 34) — and `pltChangeStateN(int)` lifecycle transitions
  (`onResume`/`onPause`/dialogs), where `pltChangeStateN(1)` fires **only** when
  resuming from a prior pause (`mBpaused`), never at first launch.
- Rendering is a native→Java callback (`flushBitmap([S,I,I,I,I)` into
  `FrameSurfaceView`); the game only flushes once its worker has produced a
  frame.

So **there is no Android-side "activate / initial paint / surface-ready" event
at bootstrap**: the native VM authors the game's first card and starts its
render loop autonomously inside `startWipiN`. That autonomy lives in the native
event/scheduler system (the event-manager singleton `global[0x152210 + 0xa94]`
seen in `getNextEvent`), which our Rust platform reimplements incompletely.

### Refined B target: the worker's own event poll returns "no event"

Combining the census, the `run()` decode, and this dex result: the worker's
outer loop (`0x2654`) polls the native event system through `.data` import
thunks and, in our runtime, gets nothing back, so it falls to the `Game.r` wait
every time. The clean B is therefore to make **the worker's own poll return the
bootstrap event** (an initial repaint/activate the native VM would have
generated), so the game handles it **on the worker's own thread and monitor
context** — авторing its first card and clearing `Game.r` itself. This avoids
the cross-thread monitor wedge the direct-poke experiment hit. The remaining
task is to identify that poll import (instrument the init-SVC / event-manager
calls the worker makes at `0x2654`–`0x2684`) and give it the initial event.

### Correction: the worker has NO event poll — it is a pure consumer

Probed which SVCs the worker makes with `lr` in `run()`'s `0x2654`–`0x26c4`
range. The result is only **four SVCs, once each**: `0x23`
(`VmCheckStackOverflow`) at `0x2650`, `0x25` (`VmMonitorEnter`) at `0x2674`,
and `0x21` (`VmAllocSavePoint`/setjmp) at `0x2684` and `0x26c4`. Those are the
`synchronized(this){ try { … } }` prologue, **not** an event poll. After them
the worker only loops on `Game.wait()`.

So the earlier reading of `0x2654` as an "event-poll body" was wrong — **the
worker never polls events**. It is purely `synchronized(this){ try {
while(Game.r != 0) wait(); … } }`. This retracts the "make the worker's own poll
return the bootstrap event" idea: there is no poll to feed.

### Consequence for B: the producer must be an external thread that authors the card

The wake producer (`Game.b` → `Game.r = 0` + `notify`) can only come from a
**separate thread** that (1) holds `this`'s monitor, (2) has a card to hand
`Game.b`, and (3) delivers on a real event context. On the reference that thread
is the native VM's event/scheduler running inside `startWipiN`, which authors
the game's first card autonomously and delivers into the game's own dispatch.
Our `net.wie.EventLoopRunner` is the counterpart thread, but it delivers to an
empty `net.wie.CardCanvas` and has no game-side receiver for a cardless Jlet.

Both reference sources are now exhausted for the *card-authoring* step:
- **classes.dex**: no Java-side bootstrap trigger (native-autonomous).
- **native RE**: the authoring lives in the firmware event-manager/scheduler
  (`global[0x152210 + 0xa94]`, `dscheduler_*`), behind runtime-built vtables and
  the `*4+4` off-by-one — the documented error-prone zone.

So B's remaining step — *reproduce the native VM's autonomous first-card
authoring for a cardless Jlet* — is a sizeable firmware-scheduler port, not a
small additive branch. It is the honest boundary of this investigation. The
recommended options are (1) commit to porting the firmware event-manager's
autonomous bootstrap, (2) accept g3/g6 as fully-diagnosed and deferred and move
to other failing titles (g4 crash, g5 class-0x0), or (3) revisit once the
generic vtable-synthesis work (task) makes the game's own dispatch reachable.

## BREAKTHROUGH — the game can be brought fully alive

Experimentally, calling the game's own two methods after `startApp` makes g3
run:

- **`Game.a()V` (`0x1138`)** — the screen-setup method. Invoking it (via
  `jvm.invoke_virtual(currentJlet, "a", "()V")`) makes the game **create its
  cards** (`Card.<init>` for `k`/`e`/`v`/`g`), call `Display.setDockedCard`, and
  `Display.addJletEventListener` — i.e. it authors the first screen. Our runtime
  never calls this on its own, which is why no `Card` was ever instantiated.
- **`Game.b()V` (`0x13a8`)** — the wake. With cards now present, invoking it
  cleanly clears `Game.r` and the worker runs.

With `a()` then `b()` called after `startApp`, the init-SVC census flips from the
parked `java-unknown=53/s` (spinning on `wait`) to **`java-static=90/s
java-reserved=90/s VmThreadReschedule=90/s`** — the worker is now *running the
game*, drawing every frame. The deadlock is gone. (The earlier direct-`b()`
experiment stalled only because it ran before `a()` had authored a card.)

Both `a()V` and `b()V` are **slot-dispatched only** (no `bl`/literal xref in the
binary), so on the reference the native VM calls them through the game's Jlet
vtable slots — likely platform lifecycle callbacks the native VM issues after
`startApp` (a "setup/activate" and a "start" callback) that our `net.wie`
launcher does not.

### Two concrete engineering pieces remain

1. **Trigger `a()`/`b()` generically.** They are the game's overrides of
   platform Jlet lifecycle slots; our `net.wie.Launcher` runs only `startApp`.
   Identify which Jlet vtable slots the reference invokes after `startApp` and
   issue them (game-agnostic — no obfuscated-name hardcoding).
2. **Render the docked card.** The game shows its screen via
   `Display.setDockedCard`, but our `net.wie.CardCanvas.paint` iterates only
   *pushed* cards, so the docked card is never painted (screen stays black even
   though the game is running and drawing). `CardCanvas` must also paint the
   Display's `dockedCard`.

Both are additive and gate-able to the cardless-Jlet / docked-card cases, so the
working card-push titles (Seed, Zenonia, LoM) are unaffected. The `startApp`
self-call at `0x2288` reads `virtual_method_offsets[+0xb6]`, which resolves to
slot 1 (`getClass`) — a legitimate call, not the setup trigger — so the setup
trigger is a separate Jlet lifecycle slot, still to be pinned.
