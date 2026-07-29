# LGT Platform Architecture

LGT (LG Telecom) is a carrier, and LGT devices shipped with their own WIPI implementation. Currently only **Clet** (C native apps) execution is implemented. Java app support is not yet implemented.

## App Structure

An LGT app consists of:
- A JAR containing:
  - `binary.mod` — an ARM ELF executable
  - App resources
- An `app_info` file (separate from the JAR) — app descriptor (AID, PID, MClass)

### Clets

C native apps compiled as standard ARM ELF binaries. Unlike KTF's raw binary format, LGT uses proper ELF with section headers, allowing standard loading at specified addresses.

### Java Apps

The jar of a Java LGT title contains **no `.class` files at all** - only
`binary.mod` and resources. LGT's toolchain compiles the application's Java
source ahead of time into ARM code, so what would be bytecode elsewhere is
just more of `binary.mod`, and `MClass` in `app_info` names a class that only
exists there. Legend of Master (`MClass:Lm`) is such a title.

What survives compilation is a description of the boundary between the
application and the platform, exchanged over import table `0x64`. See
[Compiled Java classes](#compiled-java-classes).

## Platform Interfaces

LGT uses its own WIPI-side import-table mechanism instead of KTF's direct callback approach.

### Import Table System

During initialization, the native binary receives platform callbacks for import resolution:
- one callback identifies an import table
- another resolves a function pointer from a table ID and function index

The binary uses these callbacks to resolve each platform function it needs. Known tables:

| Table ID | Purpose |
|----------|---------|
| `0x1fb`  | WIPI C functions (kernel, graphics, etc.) |
| `0x64`   | Java interface functions |
| `0x1`    | C standard library (memcpy, strlen, etc.) |

### WIPI C Interface

Provides the LGT-side WIPI C surface (kernel, graphics, database, timer, etc.), but delivered through the import table rather than a named interface pointer.

### Compiled Java classes

Two tables describe the class model, and they run in opposite directions.

#### What the application imports (import `0x14`)

Called during `fn_init` with eleven pointers. Six are read-only tables in the
image; five are zeroed arrays in `.bss` that the **platform fills in**, which
is how the compiled code reaches anything outside itself.

| read-only table     | contents                                       |
|---------------------|------------------------------------------------|
| `classes`           | `u32` count, then one 24 byte entry per class   |
| `fields`            | `(name, descriptor)` string pointer pairs       |
| `static_fields`     | same                                            |
| `virtual_methods`   | same                                            |
| `interface_methods` | same                                            |
| `static_methods`    | same                                            |

A class entry is a name pointer followed by five `(start, count)` `u16` pairs
slicing those tables:

```text
+0x00 u32 name
+0x04 u16 field_start,            +0x06 u16 field_count
+0x08 u16 static_field_start,     +0x0a u16 static_field_count
+0x0c u16 virtual_method_start,   +0x0e u16 virtual_method_count
+0x10 u16 interface_method_start, +0x12 u16 interface_method_count
+0x14 u16 static_method_start,    +0x16 u16 static_method_count
```

Legend of Master imports 31 classes this way - `org/kwis/msp/lcdui/Card`,
`java/lang/String`, `org/kwis/msp/media/Clip` and so on - with 104 static and
38 virtual methods. These are exactly the classes `wie_wipi_java` implements,
so each row can be resolved against the JVM.

The output arrays are indexed by the same flat row index as the table they
correspond to, but their element widths differ, because they hold different
things:

| output array              | width | contents                              |
|---------------------------|-------|---------------------------------------|
| `static_method_offsets`   | `u32` | address to call                        |
| `virtual_method_offsets`  | `u16` | slot to index the receiver's vtable    |
| `field_offsets`           | `u16` | byte offset within the instance        |
| `static_field_offsets`    | `u16` | -                                      |

`wie_lgt` writes an SVC stub address into each static row and dispatches it
through `method_bridge`, which converts arguments using the row's descriptor
(AAPCS: `r0`-`r3` then stack, `long` and `double` taking two slots) and calls
the JVM. Objects cross the boundary as handles, since the instance itself
lives on the Rust side.

Rows the application leaves blank are skipped. It reserves two at the head of
every class's static method block; what belongs in them is not yet known.

#### What the application brings (import `0x07`)

The application registers its own compiled classes as
`{ u32 count, u32 pad, u32 root[count] }`. Import `0x0e` resolves one by index
and returns its root. Each root is preceded by a 76 byte metadata block and
followed inline by its member table:

```text
metadata:
  +0x00 u32 flags
  +0x08 u32 name
  +0x10 u32 superclass name, zero for none
  +0x18 u16 member count
  +0x28 u32 interface table, zero for none
  +0x38 u32 method table, zero for none

root (immediately after the metadata):
  +0x00 runtime slots, zero in the image
  +0x08 u32 metadata
  +0x0c u32 flags
  +0x10 field table
```

Fields and methods live in **two separate tables**. The fields start at
`root+0x10` and run, 20 bytes each, right up to the method table. The method
table opens with a `u32` count and continues with rows of 28 bytes. Both row
kinds begin with the owning class's root, which is what makes the boundary
checkable.

```text
field:
  +0x00 u32 owner root
  +0x04 u32 name
  +0x08 u32 descriptor
  +0x0c u32 flags
  +0x10 u32 slot

method:
  +0x00 u32 owner root
  +0x04 u32 name
  +0x08 u32 descriptor
  +0x0c u16 access flags, u16 argument words (`this` included)
  +0x14 u32 ARM entry point
```

The interface table at `+0x28` has the same shape - a `u32` count, then a name
pointer each. Legend of Master's `f` implements `java/lang/Runnable`, `k` and
`org/kwis/msp/media/PlayListener`.

**The member count is not the size of either table.** `f` declares 425 and has
409 fields and 372 methods. Reading only as many rows as the count names stops
sixteen methods into the method table, which is exactly far enough to hide
every method the class overrides - `paint` sits 350 rows further on. That is
worth stating plainly because it produced a wrong conclusion that stood for a
while: see "How an override is found" below.

Most application classes carry no method table at all (`+0x38` is zero),
because nothing outside the compiled code ever calls them by name. Those parse
as a class with no members, which is correct rather than a failure.

Two things about the class an application starts from:

- **It is not in the registered table.** Legend of Master registers 18 classes,
  `a` through `r`, and `Lm` is not among them. It is found by scanning the
  loaded image for the root shape, which on that title finds all 23 classes and
  nothing else.
- **Its name comes from the argument vector**, not the descriptor. Import
  `0x83` calls `org/kwis/msp/lcdui/Main.main(["Lm", "", "true", "true"])`, the
  same shape KTF uses.

`Lm` resolves to seven members, six of them the Jlet lifecycle:

| method       | descriptor               | entry     |
|--------------|--------------------------|-----------|
| `<init>`     | `()V`                    | `0x10c8`  |
| `startApp`   | `([Ljava/lang/String;)V` | `0x1118`  |
| `pauseApp`   | `()V`                    | `0x1248`  |
| `resumeApp`  | `()V`                    | `0x12d0`  |
| `destroyApp` | `(Z)V`                   | `0x1358`  |

`wie_lgt` registers a JVM class carrying those entry points, so `Main` can
construct and drive the application's Jlet.

#### The object model

The compiled code builds an object in four steps, and the platform supplies
each one:

```text
p     = alloc(n)                  ; a scratch buffer
token = <class>.reserved_row_0(p) ; the class token
obj   = vm_instantiate(token)     ; an instance, with its dispatch table
        <class>.<init>(obj, ...)  ; the constructor, on that instance
```

The two rows every class reserves at the head of its static method block are
called, not skipped: a constructor calls its own class's first reserved row
before the superclass constructor, which is how a superclass gets initialised.
Leaving them null turns that into a branch to address zero.

A constructor row is therefore **not a factory**. `this` arrives in the first
argument word and the object it names is what the caller goes on to use, so
`<init>` initialises rather than creates. When the object is already bound to
an instance, the call is a subclass running its superclass constructor and is
dispatched with `invoke_special`; the superclass is frequently abstract and
could not be constructed anyway.

Virtual calls go through the receiver:

```text
ldrsh r2, [r8, #row * 2]   ; slot, from virtual_method_offsets (signed)
ldr   r3, [r5]             ; the receiver's dispatch table, at its word 0
add   r3, r3, r2, lsl #2
ldr   ip, [r3, #4]         ; the entry, one word past the slot
bx    ip
```

So every instance carries its class's dispatch table in word 0, and the table
has a leading word before its entries. `wie_lgt` builds one per class at load
time, filled with stubs that dispatch into the JVM.

**The slot is not always read from `virtual_method_offsets`.** The compiled
code also emits fixed slot numbers for methods the platform is expected to
provide but the class table never declares - Battle Monster branches through
slot 13 of a `java/lang/Runtime` that declares no virtual methods at all, and
slot 10 of a `java/lang/Thread` immediately after constructing it from a
`Runnable`.

Every table is therefore the same size whatever its class declares, and the
slots a class does not account for hold stubs that report what was called
rather than a zero to branch to. Objects of a class the application never
declared get a fallback table for the same reason.

Which method a fixed slot means has to be worked out from what the caller does
with it. `KNOWN_DISPATCH_SLOTS` records the ones identified so far; the rest
are reported and return zero.

#### Bridging the application's own classes

An application class has no bytecode, but the platform still has to construct
it and call its methods - the Jlet machinery drives the main class, and a
`Card` subclass has `paint` called from the display. Each one is registered as
a JVM class whose methods trampoline into the compiled code at the entry
points its member table lists.

Method bodies are built from that table at runtime, so they cannot be ordinary
Rust functions with fixed arities. `JavaMethodProto` takes a boxed `MethodBody`
directly, which is what `compiled_class::CompiledMethod` implements: it
converts the JVM's arguments to words, runs the entry point, and converts the
result back using the descriptor. Objects cross as the address the compiled
code already knows them by.

`vm_instantiate` therefore handles two kinds of token. A platform class token
becomes a fresh allocation with that class's dispatch table installed. An
application class handle - produced by class activation, imports `0x0b` and
`0x0c` - is left as the address the application already owns, with a JVM
instance bound to it, since the application lays out its own objects.

One thing to watch: none of these locks may be held across an `await`. A call
into the JVM re-enters the runtime through the SVC handler and wants the same
tables, so every handler lifts what it needs out - a `ResolvedMember`, a class
definition - and lets go before running anything.

#### Instance layout

Disassembling the field access settled the rest of the object:

```text
ldr   ip, [r7, #8]           ; the field array, at the instance's +0x08
ldrsh r3, [r2, #row * 2]     ; r2 = field_offsets
str   r4, [ip, r3, lsl #2]   ; fields[slot] = value
```

So an instance is

```text
+0x00 dispatch table
+0x04 unused so far
+0x08 word array holding the fields
```

and `field_offsets` holds **word indices** into that array, not byte offsets.

Its rows are not the platform's own field table - Legend of Master reads row
413, and the platform declares one field in total - so they must be the
application's own fields, and nothing says which field a row means.

It does not have to. The application only ever reaches a field through this
array, so any assignment it agrees with itself on works, and giving every row
its own word is the assignment that cannot collide. The row count is taken
from the gap to whichever output array follows in `.bss`, which on Legend of
Master gives 416.

#### How an override is found

By name. The compiler obfuscates an application's own methods into `a`, `b`,
`c`, but it leaves the name of anything it overrides alone: Legend of Master's
`f` declares `paint`, `run`, `keyNotify` and `playUpdate` under exactly those
names, alongside 368 obfuscated ones.

This was got wrong once, and the way it went wrong is worth keeping. Reading
only as many member rows as the metadata's count names stopped sixteen methods
into `f`'s method table, so `paint` was not among the methods found. The
sixteen that were found included one lone `(Lorg/kwis/msp/lcdui/Graphics;)V`,
so matching overrides by descriptor - which looked like the only option once
names appeared to be obfuscated - picked it, and every redraw called `f.a`, a
helper that draws a message box, in place of `paint`. Two bugs, and the second
made the first look like a solved problem.

The fix for the first removed the need for the second. There is no descriptor
matching now.

#### Arrays

An array is an instance like any other, and what its `+0x08` points at is:

```text
+0x00 element count
+0x04 elements
```

Every bounds check the compiled code makes reads that count directly -
`ldr r3, [r0]; cmp index, r3` - so an array whose block does not start with
its length reads as empty and every access throws.

Import `0x10` is `new <class>[length]`: the compiled code resolves the element
class through import `0x0e` first and passes its root with the length. It used
to return zero, so `f.<init>` threw on the first element it touched and
returned without finishing. It now allocates, and `f.<init>` builds 48 arrays
and gets as far as opening `/res/gData.dat`.

#### Where it stops

Legend of Master boots and paints. `startApp` runs to completion: it creates an
`AnnunciatorComponent` and calls `show()` through the dispatch table, gets the
default display, initialises and instantiates its own classes, constructs a
`java/util/Random`, and finishes by pushing its `Card` onto the display.
`f.paint` then runs on every redraw and `f.keyNotify` on every key, both
without faulting.

`paint` calls `System.currentTimeMillis()`, `Graphics.translate`,
`Graphics.setColor` and `Graphics.fillRect`, then dispatches on a state field
through a 22 entry jump table. The state is zero, whose case does nothing, and
the clear runs as `setColor(0)` / `fillRect(0, 0, 0, 0)` - so the screen stays
one colour.

What has not run is `f.run()`. The class implements `java/lang/Runnable`, and
the last thing `f.<init>` does is construct a `java/lang/Thread` around itself
and call `start()` through slot 10:

```text
ldr ip, [r6, #0xd4]   ; static import 53, Thread.<init>(Ljava/lang/Runnable;)V
bx  ip
str r7, [r3, r2, lsl #2]
ldr r3, [r8]          ; the thread's dispatch table
ldr ip, [r3, #0x2c]   ; slot 10, Thread.start()V
bx  ip
```

`f.<init>` does not reach it. Three things are in the way, and the two that
have been fixed moved the stopping point further down each time:

1. **Arrays** did not exist. Fixed, above.
2. **An application class's objects had no dispatch table**, so a virtual call
   on one read a zero and branched through it. They now get the fallback
   table, whose every slot reports what was called and returns zero.
   Borrowing the nearest platform superclass's table was tried and is worse:
   `f` extends `Card`, so slot 1 answered `Card.getHeight()` and the
   application dereferenced 320 as an object.
3. **Stdlib import `0x32`** is not implemented. It used to end the run; it now
   reports and returns zero like the unknown WIPI-C and Java imports.

What the run now asks for, and nothing yet answers, is a short list: dispatch
slots 1, 2, 10, 11, 12, 13 and 14 on application classes, slot 18 on
`java/lang/StringBuffer`, and stdlib import `0x32`. Giving application classes
real dispatch tables is the next piece, and the slot numbers above are what
they have to satisfy.

#### Clets, by contrast, render

An LGT application whose `app_info` says `MClass:Clet` needs none of the
above: it registers through `clet_register` and runs as a `net/wie/CletWrapper`
Jlet. That path works, and commercial Clet titles paint real screens.

Running eleven retail archives through `screen_capture` turned up four things
worth fixing, none of them in the Clet path itself:

- **An unimplemented WIPI-C function ended the run.** Two titles stopped on
  database id `0x19c` before drawing anything. An unknown id is now reported
  and returns zero, the way unknown Java imports already did; both titles then
  render.
- **An archive need not agree with itself.** SEED's `app_info` declares
  `AID:00027565` while the jar beside it is `00025A84.jar`. The declared name
  is now a preference, not a requirement, when the archive holds one jar.
- **Korean archives carry a bad Unicode Path extra field.** The name its
  checksum was computed over is not the one in the header, which a strict
  reader treats as a corrupt archive. `extract_zip` retries with those fields
  renamed so they are skipped.
- **`binary.mod` was loaded with `unwrap`**, so a missing one panicked instead
  of reporting.

#### Application class hierarchies

A class's metadata points at its superclass either by name, when that is a
platform class, or **by root**, when it is another of the application's own -
Battle Monster's `Game` extends `a`, which extends `org/kwis/msp/lcdui/Jlet`.
Reading the pointer only as a name left `Game` extending `java/lang/Object`,
which meant the `Jlet` constructor could not find its own field on it.

Registering one class therefore means registering everything it inherits from,
parents first, up to the first platform class.

### Standard Library

LGT-specific: provides C standard library functions (memcpy, strlen, etc.) that the native binary expects from the platform. KTF binaries include these in their own binary; LGT imports them.

## Initialization Sequence

1. Platform parses `binary.mod` as ELF, loads sections into memory at their specified addresses
2. Calls the ELF entrypoint with platform-owned initialization blocks
   - one of these blocks contains the import-resolution callbacks
3. The binary stores the import-resolution callbacks and uses them on demand when platform functions are needed
4. The binary returns a pointer to a structure containing its initialization entry
5. Platform calls that initialization entry to start the app

## Key Differences from KTF

| Aspect | KTF WIPI | LGT WIPI |
|--------|----------|----------|
| Binary format | Raw ARM (`client.bin`) | ELF (`binary.mod`) |
| Function binding | Direct callback pointers | Import table lookup |
| Java integration | AOT-compiled into ARM binary | AOT-compiled; platform imports bridged, application classes not yet |
| C stdlib | Included in binary | Provided by platform |

## How We Emulate This

- **ARM execution**: Same `wie_core_arm::ArmCore` as KTF.
- **ELF loading**: Uses the `elf` crate to parse sections and load them at their specified addresses.
- **Import table**: Rust callbacks map `(table_id, function_index)` pairs to registered function addresses for WIPI C, Java interface, and stdlib functions.
- **JVM**: Uses `RustJavaJvmImplementation` (pure Rust JVM). The classes an application imports are resolved against it through `wie_lgt::runtime::java::method_bridge`; the application's own compiled classes have no JVM representation yet.
