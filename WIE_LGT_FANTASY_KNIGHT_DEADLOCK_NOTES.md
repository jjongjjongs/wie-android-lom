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

### The open question (next step)

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

## Useful addresses (Fantasy Knight `g3mod/binary.mod`)

- `.text` vaddr `0x1000` = file offset `0x34` (so `file = vaddr - 0xFCC`).
- `.data` vaddr `0x1400000` (off `0xea0fc`), `.bss` vaddr `0x1500000` (0xa90).
- `Game` methods: `<init>`=`0x2074`, `startApp`=`0x220c`, `run`=`0x262c`,
  `b(Lw;I)V`=`0x2900` (wakes worker), `c(Lw;I)V`=`0x2a58` (present/bufswap).
- `Game.r`: field slot 2, index at class meta `0x1500a30 + 0x5a`.
- Method table entries are 28 bytes: `+0x04`=name ptr, `+0x08`=descriptor ptr,
  `+0x0c`=flags (`arg_words<<16 | link marker`), `+0x14`=code.
