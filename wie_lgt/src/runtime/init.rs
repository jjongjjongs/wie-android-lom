use alloc::{collections::BTreeMap, format, sync::Arc, vec::Vec};
use core::mem::size_of;

use elf::{ElfBytes, endian::AnyEndian};

use jvm::Jvm;
use spin::Mutex;
use wipi_types::lgt::{InitParam1, InitParam2, InitStruct};

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId};
use wie_util::{ByteRead, ByteWrite, Result, WieError, read_generic, write_generic};

use super::{
    SVC_CATEGORY_INIT, SVC_CATEGORY_STDLIB, SVC_CATEGORY_WIPIC,
    java::{
        app_classes::{self, AppClass},
        class_table::ClassTable,
        compiled_class, get_java_interface_method,
        handles::JavaHandles,
        interface::{
            ArrayClasses, DISPATCH_TABLE_SLOTS, JAVA_DIAG_SVC_BASE, JAVA_METHOD_SVC_LIMIT, JAVA_RESERVED_SLOT_SVC_BASE, JAVA_STATIC_METHOD_SVC_BASE,
            JAVA_UNKNOWN_SLOT_SVC_BASE, JAVA_VIRTUAL_METHOD_SVC_BASE, REFERENCE_SIZE, bridge_class_chain, java_import_11, java_import_23,
            java_load_classes, java_unk0, java_unk9, java_unk11, java_unk12, primitive_element_size, vm_get_constant_string, vm_instantiate_array,
        },
        method_bridge::{self, ResolvedMember},
    },
    stdlib::register_stdlib_svc_handler,
    svc_ids::InitSvcId,
    wipi_c::register_wipic_svc_handler,
};

type JavaClassTables = Arc<Mutex<BTreeMap<u32, (u32, u32)>>>;
/// Compiled classes the application registered, published by import `0x07`.
type AppClasses = Arc<Mutex<Vec<AppClass>>>;
type JavaActivatedClasses = Arc<Mutex<BTreeMap<u32, u32>>>;
/// Platform classes the application imports, published by import `0x14`.
type ImportedClasses = Arc<Mutex<Option<ClassTable>>>;

/// Main class of the one application the ahead-of-time compiled Java runtime
/// has been reverse engineered against so far.
///
/// The handlers guarded by this are not a compatibility implementation: they
/// poke absolute addresses (class roots, the runtime dispatch table at
/// `0x015009e4`, the object layout at `0x01500e40`) that were read out of
/// *this* `binary.mod`. Running them against any other LGT application writes
/// over unrelated memory, so they stay off unless the descriptor names `Lm`.
const LM_EXPERIMENT_MAIN_CLASS: &str = "Lm";

/// Bytes one `vm_alloc_save_point` entry takes, which is the stride of the
/// pool the platform hands them out of.
const SAVE_POINT_SIZE: u32 = 0x10c;

/// Where a class's metadata keeps its dispatch table and how many slots that
/// table has.
const CLASS_DISPATCH_TABLE: u32 = 0x0c;
const CLASS_DISPATCH_SLOTS: u32 = 0x26;

#[derive(Clone)]
struct InitSvcContext {
    wipic_category: u32,
    stdlib_category: u32,
    jvm: Jvm,
    java_handles: JavaHandles,
    imported_classes: ImportedClasses,
    app_classes: AppClasses,
    image_ranges: ImageRanges,
    java_class_tables: JavaClassTables,
    java_activated_classes: JavaActivatedClasses,
    /// Array classes handed out by `vm_get_array_class`, to the size of one of
    /// their elements.
    array_classes: ArrayClasses,
    lm_experiment: bool,
}

fn register_init_svc_handler(core: &mut ArmCore, jvm: &Jvm, lm_experiment: bool, image_ranges: ImageRanges) -> Result<()> {
    let java_handles = JavaHandles::new(core.clone());

    core.register_svc_handler(
        SVC_CATEGORY_INIT,
        handle_init_svc,
        &InitSvcContext {
            wipic_category: SVC_CATEGORY_WIPIC,
            stdlib_category: SVC_CATEGORY_STDLIB,
            jvm: jvm.clone(),
            java_handles,
            imported_classes: Default::default(),
            app_classes: Default::default(),
            image_ranges,
            java_class_tables: Default::default(),
            java_activated_classes: Default::default(),
            array_classes: Default::default(),
            lm_experiment,
        },
    )
}

async fn handle_init_svc(core: &mut ArmCore, context: &mut InitSvcContext, id: SvcId) -> Result<()> {
    let wipic_category = &context.wipic_category;
    let stdlib_category = &context.stdlib_category;
    let jvm = &mut context.jvm;
    let (_, lr) = core.read_pc_lr()?;

    if id.0 >= JAVA_STATIC_METHOD_SVC_BASE && id.0 < JAVA_STATIC_METHOD_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let index = id.0 - JAVA_STATIC_METHOD_SVC_BASE;
        let result = invoke_imported_static(core, context, index).await?;

        return result.write(core, lr);
    }

    if id.0 >= JAVA_VIRTUAL_METHOD_SVC_BASE && id.0 < JAVA_VIRTUAL_METHOD_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let index = id.0 - JAVA_VIRTUAL_METHOD_SVC_BASE;
        let result = invoke_imported_virtual(core, context, index).await?;

        return result.write(core, lr);
    }

    if id.0 >= JAVA_UNKNOWN_SLOT_SVC_BASE && id.0 < JAVA_UNKNOWN_SLOT_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let encoded = id.0 - JAVA_UNKNOWN_SLOT_SVC_BASE;
        let result = call_unknown_slot(core, context, encoded / DISPATCH_TABLE_SLOTS, encoded % DISPATCH_TABLE_SLOTS).await?;

        return result.write(core, lr);
    }

    if id.0 >= JAVA_RESERVED_SLOT_SVC_BASE && id.0 < JAVA_RESERVED_SLOT_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let index = id.0 - JAVA_RESERVED_SLOT_SVC_BASE;
        let result = call_reserved_slot(core, context, index)?;

        return result.write(core, lr);
    }

    // Diagnostic fallback for Java-interface indices that do not yet have a
    // semantic implementation. Log the first four ABI parameters and return 0.
    // This is intentionally not a compatibility implementation; it lets us
    // discover the actual call sequence used by a game on a 64-bit host.
    if id.0 >= JAVA_DIAG_SVC_BASE && id.0 < JAVA_DIAG_SVC_BASE + 0x1000 {
        let function_index = id.0 - JAVA_DIAG_SVC_BASE;
        let a0 = core.read_param(0)?;
        let a1 = core.read_param(1)?;
        let a2 = core.read_param(2)?;
        let a3 = core.read_param(3)?;
        tracing::warn!("lgt_java_diag(index={function_index:#x}, a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");
        // `vm_check_stack_overflow(words)`, which the compiled code calls at
        // the head of every method with the frame it is about to use. It was
        // read as an allocator, which meant a heap allocation per call whose
        // result the caller then discarded. The emulated stack is the host's,
        // and it has room.
        if function_index == 0x54 {
            tracing::trace!("vm_check_stack_overflow({a0})");
            0u32.write(core, lr)?;
            return Ok(());
        }

        // `vm_alloc_save_point(depth)` takes an entry out of a per-thread pool
        // of 0x10c byte blocks and hands it back for the compiled code to
        // record its unwind state in; `vm_free_save_point` returns it. Zero
        // means the pool is exhausted, and an application that reads zero goes
        // down its out-of-memory path - which is what Legend of Master was
        // doing, several thousand instructions before the point where it
        // looked like something else had gone wrong.
        if function_index == 0x1f {
            let save_point = Allocator::alloc(core, SAVE_POINT_SIZE)?;
            core.write_bytes(save_point, &[0; SAVE_POINT_SIZE as usize])?;

            tracing::debug!("vm_alloc_save_point({a0}) -> {save_point:#x}");

            save_point.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x20 {
            tracing::debug!("vm_free_save_point({a0:#x})");
            0u32.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x0b {
            let root = a0;
            let meta: u32 = read_generic(core, root + 8)?;
            write_generic(core, meta + 0x1a, 3u16)?;

            tracing::warn!("LGT vm_initialize_class_shared(root={root:#x}, meta={meta:#x}) -> state=3");

            root.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x0c {
            let root = a0;

            if let Some(&activated) = context.java_activated_classes.lock().get(&root) {
                tracing::warn!("LGT vm_activate_class(root={root:#x}, table={a1:#x}) -> cached={activated:#x}");
                activated.write(core, lr)?;
                return Ok(());
            }

            let data = Allocator::alloc(core, 20)?;
            write_generic(core, data, 0u16)?;
            write_generic(core, data + 2, 0u16)?;
            write_generic(core, data + 4, 0u32)?;
            write_generic(core, data + 8, root)?;
            write_generic(core, data + 12, 0u32)?;
            write_generic(core, data + 16, 4u16)?;
            write_generic(core, data + 18, 0u16)?;

            let vtable = activate_dispatch_table(core, context, root)?;

            let activated = Allocator::alloc(core, 12)?;
            write_generic(core, activated, vtable)?;
            write_generic(core, activated + 4, 0u32)?;
            write_generic(core, activated + 8, data)?;

            context.java_activated_classes.lock().insert(root, activated);

            tracing::warn!("LGT vm_activate_class(root={root:#x}, table={a1:#x}) -> handle={activated:#x}, data={data:#x}, vtable={vtable:#x}");

            activated.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x0d && context.lm_experiment {
            tracing::warn!("LGT import 0x0d regs: a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}, lr={lr:#x}");
            let root: u32 = match lr {
                0x0000e6b8 => 0x014015dc,
                0x000e98a8 => 0x01406274,
                0x000f1ea0 => 0x0140649c,
                _ => {
                    tracing::warn!("LGT import 0x0d unknown call site: lr={lr:#x}");
                    a0.write(core, lr)?;
                    return Ok(());
                }
            };

            let meta_ptr: u32 = read_generic(core, root + 8)?;
            let init_state: u16 = read_generic(core, meta_ptr + 0x10)?;
            let guard_state: u16 = read_generic(core, meta_ptr + 0x1a)?;

            let mut meta_bytes = [0u8; 0x40];

            match core.read_bytes(meta_ptr, &mut meta_bytes) {
                Ok(read) => {
                    tracing::warn!(
                        "LGT import 0x0d meta bytes: root={root:#x}, meta={meta_ptr:#x}, read={read:#x}, bytes={:02x?}",
                        &meta_bytes[..read]
                    );
                }
                Err(error) => {
                    tracing::warn!("LGT import 0x0d meta bytes failed: root={root:#x}, meta={meta_ptr:#x}, error={error}");
                }
            }

            tracing::warn!(
                "LGT import 0x0d class: lr={lr:#x}, root={root:#x}, meta={meta_ptr:#x}, \
     init_state={init_state:#x}, guard_state={guard_state:#x}, callback={a1:#x}"
            );

            let activated_data: u32 = read_generic(core, a0 + 8)?;
            let state_before: u16 = read_generic(core, activated_data + 0x10)?;
            write_generic(core, activated_data + 0x10, 5u16)?;
            let state_after: u16 = read_generic(core, activated_data + 0x10)?;

            tracing::warn!(
                "LGT vm_initialize_class(handle={a0:#x}, data={activated_data:#x}, callback={a1:#x}) \
                 state {state_before:#x} -> {state_after:#x}"
            );

            a0.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x104 && context.lm_experiment {
            let mut original = [0u8; 16];
            match core.read_bytes(a0, &mut original) {
                Ok(read) => {
                    tracing::warn!(
                        "LGT callback object before 0x104: object={a0:#x}, read={read:#x}, bytes={:02x?}",
                        &original[..read]
                    );
                }
                Err(error) => {
                    tracing::warn!("LGT callback object before 0x104: object={a0:#x}, read failed: {error}");
                }
            }
            let mut class_meta = [0u8; 0x40];

            match core.read_bytes(0x01401590, &mut class_meta) {
                Ok(read) => {
                    tracing::warn!("LGT class meta runtime 0x1401590: read={read:#x}, bytes={:02x?}", &class_meta[..read]);
                }
                Err(error) => {
                    tracing::warn!("LGT class meta runtime 0x1401590 read failed: {error}");
                }
            }
            let original_index_0: u16 = read_generic(core, 0x01500e40 + 0x22)?;
            let original_index_1: u16 = read_generic(core, 0x01500e40 + 0x24)?;
            write_generic(core, 0x01500e40 + 0x22, 0u16)?;
            write_generic(core, 0x01500e40 + 0x24, 1u16)?;
            tracing::warn!("Lm original method indexes before patch: +0x22={original_index_0}, +0x24={original_index_1}");

            let vtable = Allocator::alloc(core, 12)?;
            let method_stub_0 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x105)?;
            let method_stub_1 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x106)?;

            write_generic(core, vtable, 0u32)?;
            write_generic(core, vtable + 4, method_stub_0)?;
            write_generic(core, vtable + 8, method_stub_1)?;
            write_generic(core, a0, vtable)?;

            // startApp가 사용하는 두 virtual-method offset을 서로 다른 슬롯으로 분리한다.
            // +0x22: index 17 -> vtable slot 0
            // +0x24: index 18 -> vtable slot 1
            let object_word: u32 = read_generic(core, a0)?;
            let vtable_word0: u32 = read_generic(core, vtable)?;
            let vtable_word1: u32 = read_generic(core, vtable + 4)?;
            let vtable_word2: u32 = read_generic(core, vtable + 8)?;

            tracing::warn!(
                "Lm runtime object readback: object[0]={object_word:#x}, \
     vtable[0]={vtable_word0:#x}, vtable[1]={vtable_word1:#x}, \
     vtable[2]={vtable_word2:#x}"
            );

            tracing::warn!(
                "Lm runtime object initialized: object={a0:#x}, \
         vtable={vtable:#x}, method0={method_stub_0:#x}, method1={method_stub_1:#x}"
            );

            a0.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x105 {
            tracing::warn!("Lm virtual method stub 0(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");

            let mut argument_object = [0u8; 16];
            match core.read_bytes(a1, &mut argument_object) {
                Ok(read) => tracing::warn!(
                    "Lm stub 0 argument object: object={a1:#x}, read={read:#x}, bytes={:02x?}",
                    &argument_object[..read]
                ),
                Err(error) => tracing::warn!("Lm stub 0 argument object: object={a1:#x}, read failed: {error}"),
            }

            match read_generic::<u32, _>(core, a1 + 8) {
                Ok(class_root) => {
                    let mut class_bytes = [0u8; 0x40];
                    match core.read_bytes(class_root, &mut class_bytes) {
                        Ok(read) => tracing::warn!(
                            "Lm stub 0 argument class: root={class_root:#x}, read={read:#x}, bytes={:02x?}",
                            &class_bytes[..read]
                        ),
                        Err(error) => tracing::warn!("Lm stub 0 argument class: root={class_root:#x}, read failed: {error}"),
                    }
                }
                Err(error) => tracing::warn!("Lm stub 0 argument class root read failed at {:#x}: {error}", a1 + 8),
            }

            a0.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x106 {
            tracing::warn!("Lm virtual method stub 1(a0={a0:#x})");
            a0.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0xfc && context.lm_experiment {
            let lm_class_handle = 0x014015dcu32;
            tracing::warn!("Lm class getter(a0={a0:#x}) -> {lm_class_handle:#x}");
            lm_class_handle.write(core, lr)?;
            return Ok(());
        }

        // Instantiation. The argument is the token a class's first reserved
        // row handed back, and the result is the object the constructor is
        // then called on, so it has to carry that class's dispatch table in
        // its first word.
        if function_index == 0x0f {
            let instance = instantiate(core, context, a0).await?;

            tracing::debug!("LGT vm_instantiate({a0:#x}) -> {instance:#x}");

            instance.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0xf0
            || function_index == 0xf8
            || function_index == 0xfc
            || function_index == 0x108
            || function_index == 0x110
            || function_index == 0x90
            || function_index == 0x98
            || function_index == 0x84
            || function_index == 0x8c
        {
            tracing::warn!("Lm runtime passthrough(index={function_index:#x}, a0={a0:#x})");
            a0.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x83 {
            tracing::warn!("LGT import 0x83 unimplemented: a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}, lr={lr:#x}");
            0u32.write(core, lr)?;
            return Ok(());
        }

        0u32.write(core, lr)?;
        return Ok(());
    }

    match InitSvcId::try_from(id)? {
        InitSvcId::GetImportTable => EmulatedFunction::call(&get_import_table, core, &mut ()).await?.write(core, lr),
        InitSvcId::GetImportFunction => get_import_function(core, *wipic_category, *stdlib_category, core.read_param(0)?, core.read_param(1)?)
            .await?
            .write(core, lr),
        InitSvcId::Unk0 => EmulatedFunction::call(&unk0, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk7 => EmulatedFunction::call(&java_unk7, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk1 => EmulatedFunction::call(&java_unk1, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk2 => EmulatedFunction::call(&java_unk2, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk3 => EmulatedFunction::call(&java_unk3, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaInterfaceUnk0 => EmulatedFunction::call(&java_unk0, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaInterfaceUnk12 => EmulatedFunction::call(&java_unk12, core, &mut ()).await?.write(core, lr),
        // Import 0x07. The application registers its *own* classes - the ones
        // its Java source was compiled into - as `{ u32 count, u32 pad, u32
        // root[count] }`. This is the other half of the class model: import
        // 0x14 declares what the application needs from the platform, this
        // declares what it brings.
        InitSvcId::JavaInterfaceUnk5 => {
            let classes = core.read_param(0)?;
            let runtime_table = core.read_param(1)?;
            let mut tables = context.java_class_tables.lock();
            let index = (0u32..).find(|index| !tables.contains_key(index)).unwrap();
            tables.insert(index, (classes, runtime_table));

            drop(tables);

            let parsed = app_classes::parse_registered_classes(core, classes)?;
            tracing::debug!(
                "java_register_classes(classes={classes:#x}, runtime_table={runtime_table:#x}) -> {index:#x}, {} classes",
                parsed.len()
            );
            for class in &parsed {
                tracing::debug!("  {}", app_classes::describe(class));
                for method in class.methods() {
                    tracing::trace!("    {}", app_classes::describe_member(class, method));
                }
            }

            context.app_classes.lock().extend(parsed);

            index.write(core, lr)
        }
        InitSvcId::JavaLoadClasses => {
            let arguments: Vec<u32> = (0..11).map(|index| core.read_param(index)).collect::<Result<_>>()?;

            let table = java_load_classes(
                core,
                &context.java_handles,
                arguments[0],
                arguments[1],
                arguments[2],
                arguments[3],
                arguments[4],
                arguments[5],
                arguments[6],
                arguments[7],
                arguments[8],
                arguments[9],
                arguments[10],
            )
            .await?;

            *context.imported_classes.lock() = Some(table);

            ().write(core, lr)
        }
        InitSvcId::JavaUnk9 => EmulatedFunction::call(&java_unk9, core, &mut ()).await?.write(core, lr),
        // Import 0x83: run a Java main class. The first two parameters name
        // the class, which is always `org/kwis/msp/lcdui/Main`; the argument
        // vector is what actually selects the application's Jlet.
        InitSvcId::JavaUnk11 => {
            let argc = core.read_param(2)?;
            let argv = core.read_param(3)?;

            let app_classes = context.app_classes.clone();
            let image_ranges = context.image_ranges.clone();
            let java_handles = context.java_handles.clone();

            java_unk11(core, jvm, &java_handles, &app_classes, &image_ranges, argc, argv)
                .await?
                .write(core, lr)
        }
        InitSvcId::JavaImport09 => {
            let chars = core.read_param(1)?;
            let length = core.read_param(2)?;
            let cache = core.read_param(3)?;

            let result = vm_get_constant_string(core, &context.java_handles, jvm, chars, length, cache).await?;

            result.write(core, lr)
        }
        InitSvcId::JavaImport0e => {
            let dimensions = core.read_param(0)?;
            let element_class = core.read_param(1)?;
            let atype = core.read_param(2)?;

            let class = get_array_class(core, context, dimensions, element_class, atype)?;

            class.write(core, lr)
        }
        InitSvcId::JavaImport10 => {
            let class = core.read_param(0)?;
            let length = core.read_param(1)?;

            vm_instantiate_array(&context.java_handles, &context.array_classes, class, length)
                .await?
                .write(core, lr)
        }
        InitSvcId::JavaImport11 => EmulatedFunction::call(&java_import_11, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaImport23 => EmulatedFunction::call(&java_import_23, core, &mut ()).await?.write(core, lr),
    }
}
/// Handles a call the compiled code made through `static_method_offsets`.
///
/// `index` is the row of the static method table, which is all the stub
/// carries; everything else comes from the table published at load time.
async fn invoke_imported_static(core: &mut ArmCore, context: &mut InitSvcContext, index: u32) -> Result<u32> {
    // The row is lifted out before the JVM runs: a call can re-enter the
    // runtime and want the table again, and this lock does not nest.
    let member = context
        .imported_classes
        .lock()
        .as_ref()
        .and_then(|table| ResolvedMember::static_method(table, index));

    let Some(member) = member else {
        return Err(WieError::FatalError(format!("Imported static method {index} has no descriptor")));
    };

    let handles = context.java_handles.clone();
    let jvm = context.jvm.clone();

    method_bridge::invoke(core, &jvm, &handles, &member, None).await
}

/// Creates the object `vm_instantiate` was asked for.
///
/// The token is either a platform class - one handed out by a class's first
/// reserved static row - or a handle to one of the application's own classes,
/// produced by class activation. Both end up as a guest allocation with a JVM
/// instance bound to it; only the platform case has a dispatch table to
/// install, since an application class dispatches within its own code.
/// The dispatch table an application class's instances go through.
///
/// A class carries its own at `metadata + 0x0c`, with the slot count at
/// `metadata + 0x26`. The layout is fixed and shared with the platform's own
/// classes, which is what lets the compiled code emit a slot number directly:
///
/// ```text
/// word 0  the class root
/// slot 0  <init>
/// slot 1  java/lang/Object.getClass
/// slot 2  java/lang/Object.hashCode
/// slot 3  java/lang/Object.equals
/// slot 4  java/lang/Object.toString
/// slot 5  java/lang/Object.notify
/// slot 6  java/lang/Object.notifyAll
/// slot 7  java/lang/Object.wait()
/// slot 8  java/lang/Object.wait(J)
/// slot 9  java/lang/Object.wait(JI)
/// slot 10 the superclass chain's virtual methods, then the class's own
/// ```
///
/// The application fills in the slots it has code for and leaves the rest
/// zero, because those are the platform's to provide - which is what
/// activation is for. The zero slots are filled here with the reporting stubs
/// the fallback table already carries, so a call to one says what it wanted
/// instead of branching to address zero.
///
/// A class with no table of its own gets the fallback table whole.
fn activate_dispatch_table(core: &mut ArmCore, context: &InitSvcContext, root: u32) -> Result<u32> {
    let fallback = context.java_handles.fallback_dispatch_table();

    let metadata: u32 = read_generic(core, root + 8)?;
    if metadata == 0 {
        return Ok(fallback);
    }

    let vtable: u32 = read_generic(core, metadata + CLASS_DISPATCH_TABLE)?;
    let slots: u16 = read_generic(core, metadata + CLASS_DISPATCH_SLOTS)?;

    if vtable == 0 {
        tracing::debug!("LGT class at {root:#x} carries no dispatch table; using the fallback");

        return Ok(fallback);
    }

    // Copied into a table of this runtime's own size rather than filled in
    // place. The application's table is exactly as long as it declares, and
    // the compiled code reaches past that end for methods the platform is
    // expected to provide - writing the stubs into the image would land them
    // on whatever follows the table, and leaving them out puts a zero where a
    // call goes.
    let slots = u32::from(slots).min(DISPATCH_TABLE_SLOTS);

    let installed = Allocator::alloc(core, (DISPATCH_TABLE_SLOTS + 1) * 4)?;
    write_generic(core, installed, root)?;

    let mut declared = 0;

    for slot in 0..DISPATCH_TABLE_SLOTS {
        let entry: u32 = if slot < slots { read_generic(core, vtable + 4 + slot * 4)? } else { 0 };

        let entry = if entry != 0 {
            declared += 1;
            entry
        } else {
            read_generic(core, fallback + 4 + slot * 4)?
        };

        write_generic(core, installed + 4 + slot * 4, entry)?;
    }

    tracing::debug!("LGT class at {root:#x} dispatches through {installed:#x}, {declared} of {slots} slots its own");

    Ok(installed)
}

/// `vm_get_array_class(dimensions, element_class, atype)`.
///
/// The platform builds the array's descriptor from these three - `dimensions`
/// leading `[`, then either the element class's name or the descriptor letter
/// its `atype` stands for - and looks the class up by that name. Nothing here
/// has classes to look up, so the call hands back a token that stands for the
/// array class, and what it has to carry is the size of one element:
/// `vm_instantiate_array` is given the token and a length and nothing else.
///
/// An element is a reference when the array is of objects or of arrays, and
/// otherwise as wide as the primitive `atype` names.
fn get_array_class(core: &mut ArmCore, context: &InitSvcContext, dimensions: u32, element_class: u32, atype: u32) -> Result<u32> {
    let element_size = if dimensions > 1 || element_class != 0 {
        REFERENCE_SIZE
    } else {
        match primitive_element_size(atype) {
            Some(size) => size,
            None => {
                tracing::warn!("vm_get_array_class({dimensions}, {element_class:#x}, {atype}) names no primitive type");

                REFERENCE_SIZE
            }
        }
    };

    // One token per shape, so an application that asks twice gets the same
    // class back, the way it would from a class table.
    let existing = context
        .array_classes
        .lock()
        .iter()
        .find(|(_, size)| **size == element_size)
        .map(|(class, _)| *class);

    if let Some(class) = existing {
        return Ok(class);
    }

    let class = Allocator::alloc(core, 8)?;
    write_generic(core, class, element_size)?;
    write_generic(core, class + 4, dimensions)?;

    context.array_classes.lock().insert(class, element_size);

    tracing::debug!("vm_get_array_class({dimensions}, {element_class:#x}, {atype}) -> {class:#x}, {element_size} bytes an element");

    Ok(class)
}

async fn instantiate(core: &mut ArmCore, context: &mut InitSvcContext, token: u32) -> Result<u32> {
    let known_platform_class = context
        .imported_classes
        .lock()
        .as_ref()
        .is_some_and(|table| table.class_of_object(token).is_some());

    if known_platform_class {
        return instantiate_imported_class(core, context, token);
    }

    instantiate_app_class(core, context, token).await
}

/// Instantiates one of the application's own classes.
///
/// The token is the handle class activation produced, which carries the class
/// root. Registering the class gives the JVM something to construct, and
/// binding it to a fresh allocation is what lets the superclass constructor
/// the compiled code calls next find an object instead of building one.
async fn instantiate_app_class(core: &mut ArmCore, context: &mut InitSvcContext, handle: u32) -> Result<u32> {
    let root = context
        .java_activated_classes
        .lock()
        .iter()
        .find(|(_, activated)| **activated == handle)
        .map(|(root, _)| *root);

    let Some(root) = root else {
        tracing::warn!("LGT vm_instantiate({handle:#x}) names neither a platform nor an application class");
        return Ok(handle);
    };

    let class = context.app_classes.lock().iter().find(|x| x.root == root).map(|x| x.name.clone());

    let Some(class) = class else {
        tracing::warn!("LGT application class at {root:#x} was never registered");
        return Ok(handle);
    };

    let app_classes = context.app_classes.clone();
    let image_ranges = context.image_ranges.clone();
    let jvm = context.jvm.clone();
    let handles = context.java_handles.clone();

    if !bridge_class_chain(&jvm, core, &handles, &app_classes, &image_ranges, &class).await {
        return Ok(handle);
    }

    let Some(instance) = compiled_class::instantiate(&jvm, &class).await else {
        return Ok(handle);
    };

    // The application allocates and lays out its own instances, so the handle
    // it already has is the object; only the JVM side is missing.
    context.java_handles.bind(handle, instance);

    tracing::debug!("LGT bound application class {class} to {handle:#x}");

    Ok(handle)
}

/// Allocates an instance of the class a token names, with its dispatch table
/// installed.
fn instantiate_imported_class(_core: &mut ArmCore, context: &mut InitSvcContext, class_object: u32) -> Result<u32> {
    let imported_classes = context.imported_classes.clone();
    let table = imported_classes.lock();

    let layout = table.as_ref().and_then(|table| {
        let index = table.class_of_object(class_object)?;
        let class = table.classes.get(index as usize)?;

        Some((class.name.clone(), *table.vtables.get(index as usize)?, class.field_count))
    });

    let Some((name, vtable, field_count)) = layout else {
        tracing::warn!("LGT vm_instantiate({class_object:#x}) does not name a class");
        return Ok(class_object);
    };

    drop(table);

    let _ = field_count;

    let instance = context.java_handles.allocate_instance(vtable)?;

    tracing::debug!("LGT new {name} instance at {instance:#x}, dispatch table {vtable:#x}");

    Ok(instance)
}

/// Reports a call through one of the reserved rows at the head of a class's
/// static method block, and returns the first argument.
///
/// Identity is a placeholder, not a semantic: these rows are called for their
/// effect, and what that effect is has not been worked out. Returning the
/// argument at least keeps a constructor that threads an allocation through
/// them going, instead of stopping at a branch to zero.
fn call_reserved_slot(core: &mut ArmCore, context: &mut InitSvcContext, index: u32) -> Result<u32> {
    let a0 = core.read_param(0)?;

    let imported_classes = context.imported_classes.clone();
    let table = imported_classes.lock();

    let Some(table) = table.as_ref() else {
        return Ok(a0);
    };
    let Some((class, slot)) = table.static_method_owner(index) else {
        tracing::warn!("LGT reserved static row {index} belongs to no class");
        return Ok(a0);
    };

    // Slot 0 hands back the class token. A constructor calls it and drops the
    // result, which is how a superclass gets initialized; `new` calls it and
    // passes the result to vm_instantiate.
    if slot == 0 {
        let class_object = table
            .classes
            .iter()
            .position(|x| core::ptr::eq(x, class))
            .and_then(|index| table.class_objects.get(index).copied())
            .unwrap_or(a0);

        tracing::debug!("LGT class object of {} -> {class_object:#x}", class.name);

        return Ok(class_object);
    }

    tracing::warn!("LGT reserved slot {slot} of {} called with a0={a0:#x}", class.name);

    Ok(a0)
}

/// Dispatch table slots the compiled code reaches by a fixed number, with the
/// method each one turns out to be.
///
/// A class does not declare these - the platform is expected to provide them -
/// so which method a slot means has to be worked out from what the caller does
/// with it. Battle Monster branches through slot 10 of a `java/lang/Thread`
/// immediately after constructing it from a `Runnable`, which is `start`, and
/// nothing else a game does with a fresh thread fits.
const KNOWN_DISPATCH_SLOTS: &[(&str, u32, &str, &str)] = &[("java/lang/Thread", 10, "start", "()V")];

/// Slots 1 to 9 of every dispatch table, which the platform fills in for the
/// class whatever the class is.
///
/// Read out of `liblgt_system.so`, where `dt_java_lang_Object` names them and
/// every other `dt_` repeats them in the same order - `dt_java_lang_Card` and
/// `dt_java_lang_StringBuffer` differ only where they override one. Slot 0 is
/// the class's own `<init>`, so it is not here.
const OBJECT_DISPATCH_SLOTS: &[(&str, &str)] = &[
    ("getClass", "()Ljava/lang/Class;"),
    ("hashCode", "()I"),
    ("equals", "(Ljava/lang/Object;)Z"),
    ("toString", "()Ljava/lang/String;"),
    ("notify", "()V"),
    ("notifyAll", "()V"),
    ("wait", "()V"),
    ("wait", "(J)V"),
    ("wait", "(JI)V"),
];

/// Reports a call through a dispatch table slot the class does not declare.
///
/// The compiled code emits fixed slot numbers for methods the platform is
/// expected to provide, and which method a given slot means is not yet known.
/// Returning zero at least keeps the caller going, and the log says what to
/// look for.
async fn call_unknown_slot(core: &mut ArmCore, context: &mut InitSvcContext, class_index: u32, slot: u32) -> Result<u32> {
    let this = core.read_param(0)?;

    // Slots 1 to 9 are `java/lang/Object`'s, in every dispatch table there
    // is, so they can be answered without knowing whose table this one is.
    if let Some((name, descriptor)) = OBJECT_DISPATCH_SLOTS.get((slot as usize).wrapping_sub(1)) {
        let member = ResolvedMember {
            class_name: "java/lang/Object".into(),
            name: (*name).into(),
            descriptor: (*descriptor).into(),
        };

        let handles = context.java_handles.clone();
        let jvm = context.jvm.clone();

        // An object the compiled code built for itself has no instance on the
        // JVM side to call these on.
        if handles.get(this).is_some() {
            return method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await;
        }

        tracing::debug!("LGT java/lang/Object.{name}{descriptor} on {this:#x}, which has no instance");

        return Ok(0);
    }

    let class = context
        .imported_classes
        .lock()
        .as_ref()
        .and_then(|table| table.classes.get(class_index as usize).map(|x| x.name.clone()));

    let Some(class) = class else {
        tracing::warn!("LGT undeclared dispatch slot {slot} called on {this:#x}");
        return Ok(0);
    };

    if let Some((_, _, name, descriptor)) = KNOWN_DISPATCH_SLOTS
        .iter()
        .find(|(known_class, known_slot, _, _)| *known_class == class && *known_slot == slot)
    {
        let member = ResolvedMember {
            class_name: class,
            name: (*name).into(),
            descriptor: (*descriptor).into(),
        };

        let handles = context.java_handles.clone();
        let jvm = context.jvm.clone();

        return method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await;
    }

    tracing::warn!("LGT undeclared dispatch slot {slot} of {class} called on {this:#x}");

    Ok(0)
}

/// Handles a call the compiled code made through a class's dispatch table.
async fn invoke_imported_virtual(core: &mut ArmCore, context: &mut InitSvcContext, index: u32) -> Result<u32> {
    let this = core.read_param(0)?;

    let member = context
        .imported_classes
        .lock()
        .as_ref()
        .and_then(|table| ResolvedMember::virtual_method(table, index));

    let Some(member) = member else {
        return Err(WieError::FatalError(format!("Imported virtual method {index} has no descriptor")));
    };

    let handles = context.java_handles.clone();
    let jvm = context.jvm.clone();

    method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await
}

pub async fn load_native(core: &mut ArmCore, system: &mut System, jvm: &Jvm, data: &[u8], main_class_name: Option<&str>) -> Result<()> {
    let lm_experiment = main_class_name == Some(LM_EXPERIMENT_MAIN_CLASS);
    if lm_experiment {
        tracing::warn!("Enabling experimental {LM_EXPERIMENT_MAIN_CLASS} runtime patches; these are specific to that binary.mod");
    }

    let (entrypoint, image_ranges) = load_executable(core, data)?;
    register_wipic_svc_handler(core, system, jvm)?;
    register_stdlib_svc_handler(core, system)?;
    register_init_svc_handler(core, jvm, lm_experiment, Arc::new(image_ranges))?;

    let ptr_init_param_1 = Allocator::alloc(core, size_of::<InitParam1>() as u32)?;
    let ptr_init_param_2 = Allocator::alloc(core, size_of::<InitParam2>() as u32)?;

    let init_param_1 = InitParam1 {
        unk1: [0; 512],
        unk2: [0; 20],
        ptr_init_struct: 0,
    };

    write_generic(core, ptr_init_param_1, init_param_1)?;

    let init_param_2 = InitParam2 {
        fn_get_import_table: core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::GetImportTable)?,
        fn_get_import_function: core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::GetImportFunction)?,
        fn_unk3: 0,
        fn_unk4: 0,
    };

    write_generic(core, ptr_init_param_2, init_param_2)?;

    tracing::debug!("ptr_init_param_1: {ptr_init_param_1:#x}");
    tracing::debug!("ptr_init_param_2: {ptr_init_param_2:#x}");

    tracing::debug!("Calling entrypoint {entrypoint:#x}");
    let _: () = core.run_function(entrypoint + 1, &[ptr_init_param_1, ptr_init_param_2, 0]).await?;

    let init_param_1: InitParam1 = read_generic(core, ptr_init_param_1)?;

    tracing::debug!("InitStruct: {:#x?}", init_param_1.ptr_init_struct);
    let init_struct: InitStruct = read_generic(core, init_param_1.ptr_init_struct)?;

    tracing::debug!("Calling initializer at {:#x}", init_struct.fn_init);
    let _: () = core.run_function(init_struct.fn_init, &[]).await?;

    Ok(())
}

async fn get_import_table(_core: &mut ArmCore, _: &mut (), import_table: u32) -> Result<u32> {
    tracing::debug!("get_import_table({import_table:#x})");

    Ok(import_table)
}

async fn get_import_function(core: &mut ArmCore, wipic_category: u32, stdlib_category: u32, import_table: u32, function_index: u32) -> Result<u32> {
    tracing::debug!("get_import_function({import_table:#x}, {function_index})");

    if import_table == 0x1fb {
        return core.make_svc_stub(wipic_category, function_index);
    } else if import_table == 0x64 {
        return get_java_interface_method(core, function_index);
    } else if import_table == 1 {
        return core.make_svc_stub(stdlib_category, function_index);
    }

    Ok(match (import_table, function_index) {
        (0x1f8, 0x16) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::Unk0)?,
        (0x1f8, 0x17) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk7)?,
        (0x1fc, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk1)?,
        (0x1ff, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk2)?,
        (0x201, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk3)?,
        _ => {
            return Err(WieError::FatalError(format!(
                "Unknown import function: {import_table:#x}, {function_index:#x}"
            )));
        }
    })
}

/// Loaded section ranges, used to find classes the application never
/// registered.
type ImageRanges = Arc<Vec<(u32, u32)>>;

fn load_executable(core: &mut ArmCore, data: &[u8]) -> Result<(u32, Vec<(u32, u32)>)> {
    let elf = ElfBytes::<AnyEndian>::minimal_parse(data).map_err(|x| WieError::FatalError(format!("Failed to parse ELF binary.mod: {x}")))?;

    if elf.ehdr.e_machine != elf::abi::EM_ARM {
        return Err(WieError::FatalError(format!("Invalid ELF machine type: {}", elf.ehdr.e_machine)));
    }
    if elf.ehdr.e_type != elf::abi::ET_EXEC {
        return Err(WieError::FatalError(format!("Invalid ELF file type: {}", elf.ehdr.e_type)));
    }
    if elf.ehdr.class != elf::file::Class::ELF32 {
        return Err(WieError::FatalError(format!("Invalid ELF class: {:?}", elf.ehdr.class)));
    }

    let (shdrs_opt, strtab_opt) = elf
        .section_headers_with_strtab()
        .map_err(|x| WieError::FatalError(format!("Failed to read ELF section headers: {x}")))?;
    let shdrs = shdrs_opt.ok_or_else(|| WieError::FatalError("ELF is missing section headers".into()))?;
    let strtab = strtab_opt.ok_or_else(|| WieError::FatalError("ELF is missing section name string table".into()))?;

    let mut ranges = Vec::new();

    for shdr in shdrs {
        let section_name = strtab
            .get(shdr.sh_name as usize)
            .map_err(|x| WieError::FatalError(format!("Invalid ELF section name index {}: {x}", shdr.sh_name)))?;

        if shdr.sh_addr != 0 {
            tracing::debug!("Section {section_name} at {:x}", shdr.sh_addr);

            let data = elf
                .section_data(&shdr)
                .map_err(|x| WieError::FatalError(format!("Failed to read ELF section {section_name}: {x}")))?
                .0;

            core.load(data, shdr.sh_addr as u32, shdr.sh_size as usize)?;
            ranges.push((shdr.sh_addr as u32, shdr.sh_size as u32));
        }
    }

    tracing::debug!("Entrypoint: {:#x}", elf.ehdr.e_entry);

    Ok((elf.ehdr.e_entry as u32, ranges))
}

async fn unk0(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<()> {
    tracing::warn!("clet_unk0({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    Ok(())
}

async fn java_unk1(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32) -> Result<()> {
    tracing::warn!("java_unk1({a0:#x}, {a1:#x}, {a2:#x})");

    Ok(())
}

async fn java_unk2(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32) -> Result<()> {
    tracing::warn!("java_unk2({a0:#x}, {a1:#x}, {a2:#x})");

    Ok(())
}

async fn java_unk3(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32) -> Result<()> {
    tracing::warn!("java_unk3({a0:#x}, {a1:#x}, {a2:#x})");

    Ok(())
}

async fn java_unk7(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32) -> Result<u32> {
    tracing::warn!("java_unk7({a0:#x}, {a1:#x}, {a2:#x})");

    Ok(0)
}
