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
and returns its root. A root is:

```text
+0x00 runtime slots, zero in the image
+0x08 u32 metadata
+0x0c u32 flags
+0x10 field descriptors, 20 bytes each:
      { owner root, name, descriptor, flags, slot }
```

Legend of Master registers 18 classes. Its `Lm` root carries static fields
`a:LLm;` and `b:Ljava/lang/Thread;`, and the metadata block holds a superclass
name and entry point addresses.

Reaching `startApp` needs these roots turned into JVM classes whose methods
trampoline into the compiled code. That is not implemented: the application
loads, resolves its imports and completes `fn_init`, then stops. Imports
`0x0b`, `0x0c`, `0x0d` (class initialisation and activation) and `0x0f`
(instantiation) are the remaining unknowns.

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
