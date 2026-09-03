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
0x1f9960  ldr pc, [r3, #0x88]   ; call display.vtable[slot 34]  (0x88/4)
```

i.e. it dispatches the repaint to `display.vtable[34]`, which routes to the
current displayable's paint/handler — the game's own card/Jlet code — and that
reaches `M -> Game.b -> b.r = 0` (the first wake, before the worker renders).

Our path is `net.wie.EventQueue.dispatchEvent -> RepaintEvent ->
Display.handlePaintEvent -> net.wie.CardCanvas.paint`, and `CardCanvas` has zero
cards, so nothing game-side runs. **That is the divergence**: the reference
paints the game's own displayable; we paint an empty wrapper. The game never
`setCurrent`s a card of its own (its startApp is minimal), so on the reference
the *current displayable for a Jlet must default to the Jlet/its main surface*,
whose `vtable[34]` is the game paint/handler.

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
