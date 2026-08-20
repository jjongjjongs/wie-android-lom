use alloc::{collections::BTreeMap, format, sync::Arc, vec, vec::Vec};
use core::mem::size_of;

use elf::{ElfBytes, endian::AnyEndian};

use jvm::Jvm;
use spin::Mutex;
use wipi_types::lgt::{InitParam1, InitParam2, InitStruct};

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId};
use wie_util::{ByteWrite, Result, WieError, read_generic, write_generic};

use crate::relocation::{
    R_ARM_ABS32, R_ARM_CALL, R_ARM_JUMP24, R_ARM_NONE, R_ARM_PC24, R_ARM_RABS32, R_ARM_RBASE, R_ARM_REL32, R_ARM_RPC24, R_ARM_RREL32, R_ARM_THM_CALL,
    R_ARM_THM_JUMP24, arm_abs32, arm_pc24, arm_rel32, raptor_rabs32, raptor_rpc24, raptor_rrel32, thumb_pc22,
};

use super::{
    SVC_CATEGORY_INIT, SVC_CATEGORY_STDLIB, SVC_CATEGORY_WIPIC,
    java::{
        app_classes::{self, AppClass},
        class_table::ClassTable,
        compiled_class, get_java_interface_method,
        handles::JavaHandles,
        interface::{
            ArrayClassInfo, ArrayClasses, DISPATCH_TABLE_SLOTS, JAVA_DIAG_SVC_BASE, JAVA_METHOD_SVC_LIMIT, JAVA_RESERVED_SLOT_SVC_BASE,
            JAVA_STATIC_METHOD_SVC_BASE, JAVA_UNKNOWN_SLOT_SVC_BASE, JAVA_VIRTUAL_METHOD_SVC_BASE, REFERENCE_SIZE, bridge_class_chain,
            java_import_11, java_import_23, java_load_classes, java_unk0, java_unk9, java_unk11, java_unk12, primitive_element_size,
            vm_get_constant_string, vm_instantiate_array,
        },
        method_bridge::{self, ResolvedMember},
        platform_slots::platform_method,
    },
    stdlib::register_stdlib_svc_handler,
    svc_ids::InitSvcId,
    wipi_c::register_wipic_svc_handler,
};

type JavaClassTables = Arc<Mutex<BTreeMap<u32, (u32, u32)>>>;
/// Compiled classes the application registered, published by import `0x07`.
type AppClasses = Arc<Mutex<Vec<AppClass>>>;
type JavaActivatedClasses = Arc<Mutex<BTreeMap<u32, u32>>>;
type ImportFunctionCache = Arc<Mutex<BTreeMap<(u32, u32), u32>>>;
type UnresolvedImportCallCounts = Arc<Mutex<BTreeMap<(u32, u32), u64>>>;
/// Platform classes the application imports, published by import `0x14`.
type ImportedClasses = Arc<Mutex<Option<ClassTable>>>;

const UNRESOLVED_IMPORT_SVC_BASE: u32 = 0x1000_0000;
const UNRESOLVED_IMPORT_FIELD_MASK: u32 = 0x0fff;

/// Bytes one `vm_alloc_save_point` entry takes, which is the stride of the
/// pool the platform hands them out of.
const SAVE_POINT_SIZE: u32 = 0x10c;

/// Where a class's metadata keeps its dispatch table and how many slots that
/// table has.
const CLASS_DISPATCH_TABLE: u32 = 0x0c;
const CLASS_DISPATCH_SLOTS: u32 = 0x26;

/// Guard on how far a class hierarchy is walked looking for a platform class.
const MAX_SUPERCLASS_DEPTH: usize = 32;

#[derive(Clone)]
struct InitSvcContext {
    wipic_category: u32,
    stdlib_category: u32,
    system: System,
    jvm: Jvm,
    java_handles: JavaHandles,
    imported_classes: ImportedClasses,
    app_classes: AppClasses,
    image_ranges: ImageRanges,
    java_class_tables: JavaClassTables,
    java_activated_classes: JavaActivatedClasses,
    import_function_cache: ImportFunctionCache,
    unresolved_import_call_counts: UnresolvedImportCallCounts,
    /// Array classes handed out by `vm_get_array_class`, to the size of one of
    /// their elements.
    array_classes: ArrayClasses,
}

fn register_init_svc_handler(core: &mut ArmCore, system: &System, jvm: &Jvm, image_ranges: ImageRanges) -> Result<()> {
    let java_handles = JavaHandles::new(core.clone());

    core.register_svc_handler(
        SVC_CATEGORY_INIT,
        handle_init_svc,
        &InitSvcContext {
            wipic_category: SVC_CATEGORY_WIPIC,
            stdlib_category: SVC_CATEGORY_STDLIB,
            system: system.clone(),
            jvm: jvm.clone(),
            java_handles,
            imported_classes: Default::default(),
            app_classes: Default::default(),
            image_ranges,
            java_class_tables: Default::default(),
            java_activated_classes: Default::default(),
            import_function_cache: Default::default(),
            unresolved_import_call_counts: Default::default(),
            array_classes: Default::default(),
        },
    )
}

async fn handle_init_svc(core: &mut ArmCore, context: &mut InitSvcContext, id: SvcId) -> Result<()> {
    let wipic_category = &context.wipic_category;
    let stdlib_category = &context.stdlib_category;
    let jvm = &mut context.jvm;
    let (pc, lr) = core.read_pc_lr()?;

    if id.0 >= UNRESOLVED_IMPORT_SVC_BASE {
        let encoded = id.0 - UNRESOLVED_IMPORT_SVC_BASE;
        let import_table = (encoded >> 12) & UNRESOLVED_IMPORT_FIELD_MASK;
        let function_index = encoded & UNRESOLVED_IMPORT_FIELD_MASK;
        let a0 = core.read_param(0)?;
        let a1 = core.read_param(1)?;
        let a2 = core.read_param(2)?;
        let a3 = core.read_param(3)?;

        let key = (import_table, function_index);
        let count = {
            let mut counts = context.unresolved_import_call_counts.lock();
            let count = counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
            *count
        };

        // Record the first calls in detail, then exponentially sample a hot loop.
        // This preserves evidence of repeated imports without flooding Android logs.
        let should_log = count <= 8 || count.is_power_of_two();

        if should_log {
            tracing::warn!(
                "unresolved_lgt_import(                 table={import_table:#x},                  function={function_index:#x},                  count={count},                  pc={pc:#x},                  lr={lr:#x},                  r0={a0:#x},                  r1={a1:#x},                  r2={a2:#x},                  r3={a3:#x}                 ) -> 0"
            );
        }

        0u32.write(core, lr)?;
        return Ok(());
    }

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

        if function_index == 0x55 {
            tracing::trace!("vm_thread_reschedule()");
            context.system.yield_now().await;
            0u32.write(core, lr)?;
            return Ok(());
        }

        // `vm_monitor_enter(object)`. Seed1 calls this immediately before
        // java/lang/Object.wait(JI)V; without acquiring the JVM monitor,
        // Object.wait fails with IllegalMonitorStateException.
        if function_index == 0x56 {
            let Some(instance) = context.java_handles.get(a0) else {
                return Err(WieError::FatalError(format!("vm_monitor_enter({a0:#x}) names no JVM instance")));
            };

            tracing::trace!("vm_monitor_enter({a0:#x})");
            if let Err(error) = context.jvm.monitor_enter(&instance).await {
                return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
            }

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

            tracing::debug!("LGT vm_initialize_class_shared(root={root:#x}, meta={meta:#x}) -> state=3");

            root.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x0c {
            let root = a0;

            if let Some(&activated) = context.java_activated_classes.lock().get(&root) {
                tracing::debug!("LGT vm_activate_class(root={root:#x}, table={a1:#x}) -> cached={activated:#x}");
                activated.write(core, lr)?;
                return Ok(());
            }

            // Native vm_activate_class allocates a 20-byte class header plus
            // one word for each static field declared at metadata + 0x48.
            // Classes carrying metadata flag 0x2000 use only the header.
            let metadata: u32 = read_generic(core, root + 8)?;
            let flags: u16 = read_generic(core, metadata)?;
            let static_field_count: u16 = read_generic(core, metadata + 0x48)?;
            let data_size = if flags & 0x2000 != 0 {
                20
            } else {
                20 + u32::from(static_field_count) * 4
            };
            let data = Allocator::alloc(core, data_size)?;
            core.write_bytes(data, &vec![0; data_size as usize])?;

            write_generic(core, data, 0u16)?;
            write_generic(core, data + 2, 0u16)?;
            write_generic(core, data + 4, 0u32)?;
            write_generic(core, data + 8, root)?;
            write_generic(core, data + 12, 0u32)?;
            write_generic(core, data + 16, 4u16)?;
            write_generic(core, data + 18, 0u16)?;

            let instance_vtable = activate_dispatch_table(core, context, root)?;
            let class_vtable = context.java_handles.fallback_dispatch_table();

            // Keep the activated java/lang/Class-like object distinct from
            // instances of the application class. The original runtime gives
            // the activated handle java/lang/Class's table and reaches the
            // application instance table through the class metadata.
            write_generic(core, data + 12, instance_vtable)?;

            let activated = Allocator::alloc(core, 12)?;
            write_generic(core, activated, class_vtable)?;
            write_generic(core, activated + 4, 0u32)?;
            write_generic(core, activated + 8, data)?;

            context.java_activated_classes.lock().insert(root, activated);

            tracing::debug!(
                "LGT vm_activate_class(root={root:#x}, table={a1:#x}) -> handle={activated:#x}, data={data:#x}, class_vtable={class_vtable:#x}, instance_vtable={instance_vtable:#x}"
            );

            activated.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x0d {
            let activated_data: u32 = read_generic(core, a0 + 8)?;

            // Mark initialization in progress before entering guest code so a
            // recursive class lookup does not start the initializer again.
            write_generic(core, activated_data + 0x10, 5u16)?;

            if a1 != 0 {
                let _: u32 = core.run_function(a1, &[a0]).await?;
            }

            tracing::debug!("LGT vm_initialize_class(handle={a0:#x}, data={activated_data:#x}, callback={a1:#x})");

            a0.write(core, lr)?;
            return Ok(());
        }

        // Instantiation. The argument is the token a class's first reserved
        // row handed back, and the result is the object the constructor is
        // then called on, so it has to carry that class's dispatch table in
        // its first word.
        // Java-interface imports 0xfa and 0x61 store an object reference
        // into a guest reference array. Legend of Master uses 0xfa while
        // initializing the class-selection arrays.
        if function_index == 0xfa || function_index == 0x61 {
            let array = a0;
            let index = a1;
            let value = a2;

            if array == 0 {
                return Err(WieError::FatalError("LGT reference-array store received a null array".into()));
            }

            let data: u32 = read_generic(core, array + 8)?;
            let length: u32 = read_generic(core, data)?;

            if index >= length {
                return Err(WieError::FatalError(format!(
                    "LGT reference-array store index {index} is outside length {length}"
                )));
            }

            write_generic(core, data + 4 + index * REFERENCE_SIZE, value)?;

            0u32.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x0f {
            let instance = instantiate(core, context, a0).await?;

            tracing::debug!("LGT vm_instantiate({a0:#x}, lr={lr:#x}) -> {instance:#x}");

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
        InitSvcId::GetImportFunction => get_import_function(
            core,
            *wipic_category,
            *stdlib_category,
            &context.import_function_cache,
            core.read_param(0)?,
            core.read_param(1)?,
        )
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
        InitSvcId::JavaImportE1 => {
            let class = imported_class_token(context, "java/lang/String")
                .ok_or_else(|| WieError::FatalError("java/lang/String has no imported class token".into()))?;

            tracing::debug!("vm_get_string_class() -> {class:#x}");
            class.write(core, lr)
        }
        InitSvcId::JavaImportE2 => {
            let string_class = imported_class_token(context, "java/lang/String")
                .ok_or_else(|| WieError::FatalError("java/lang/String has no imported class token".into()))?;

            let class = get_array_class(core, context, 1, string_class, 0)?;
            tracing::debug!("vm_get_string_array_class() -> {class:#x}");
            class.write(core, lr)
        }
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

    // A class that overrides nothing ships no table, and inherits its
    // superclass's whole. Legend of Master's `f` is one, and reaches
    // `Card.showNotify` at slot 16 - `move`, `resize`, `getWidth`,
    // `getHeight`, `getX`, `getY`, `showNotify` being Card's first seven,
    // which land at slots 10 to 16.
    if vtable == 0 {
        let inherited = platform_superclass_dispatch_table(context, root);

        // Seed1's `o` declares a 30-slot instance table but leaves the
        // prebuilt table pointer null. Its compiler-emitted virtual-method
        // row 28 names o.a(II)V, which is the ninth instance method and
        // therefore occupies dispatch slot 18 after the ten reserved slots.
        //
        // Keep this narrowly scoped as a diagnostic until the generic
        // application-vtable synthesis rule is established.
        if root == 0x0140_14d8 && slots == 30 {
            const SEED1_O_A_II_ENTRY: u32 = 0x0001_6ca0;
            const SEED1_O_A_II_SLOT: u32 = 18;
            const SEED1_O_A_II_ROW: u32 = 28;

            let installed = Allocator::alloc(core, (DISPATCH_TABLE_SLOTS + 1) * 4)?;
            write_generic(core, installed, root)?;

            for slot in 0..DISPATCH_TABLE_SLOTS {
                let entry: u32 = read_generic(core, inherited + 4 + slot * 4)?;
                write_generic(core, installed + 4 + slot * 4, entry)?;
            }

            write_generic(core, installed + 4 + SEED1_O_A_II_SLOT * 4, SEED1_O_A_II_ENTRY)?;

            if let Some(table) = context.imported_classes.lock().as_ref() {
                write_generic(
                    core,
                    table.outputs.virtual_method_offsets + SEED1_O_A_II_ROW * 2,
                    SEED1_O_A_II_SLOT as u16,
                )?;
            }

            tracing::debug!(
                "Seed1 vtable compatibility: root={root:#x}, installed={installed:#x}, virtual_method[28] -> slot 18 -> {SEED1_O_A_II_ENTRY:#x}"
            );

            return Ok(installed);
        }

        tracing::debug!(
            "LGT class at {root:#x} carries no dispatch table; using {}",
            if inherited == fallback { "the fallback" } else { "its superclass's" }
        );

        return Ok(inherited);
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

/// The table of the nearest platform class an application class extends, or
/// the fallback when the chain does not reach one.
fn imported_class_token(context: &InitSvcContext, name: &str) -> Option<u32> {
    let imported = context.imported_classes.lock();
    let table = imported.as_ref()?;

    let index = table.classes.iter().position(|class| class.name == name)?;
    table.class_objects.get(index).copied()
}

fn platform_superclass_dispatch_table(context: &InitSvcContext, root: u32) -> u32 {
    let app_classes = context.app_classes.lock();
    let mut superclass = app_classes.iter().find(|x| x.root == root).and_then(|x| x.superclass.clone());

    for _ in 0..MAX_SUPERCLASS_DEPTH {
        let Some(name) = superclass else { break };

        let platform = context.imported_classes.lock().as_ref().and_then(|table| {
            let index = table.classes.iter().position(|x| x.name == name)?;

            table.vtables.get(index).copied()
        });

        if let Some(vtable) = platform {
            return vtable;
        }

        superclass = app_classes.iter().find(|x| x.name == name).and_then(|x| x.superclass.clone());
    }

    context.java_handles.fallback_dispatch_table()
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
        .find(|(_, info)| info.dimensions == dimensions && info.element_class == element_class && info.atype == atype)
        .map(|(class, _)| *class);

    if let Some(class) = existing {
        return Ok(class);
    }

    let class = Allocator::alloc(core, 8)?;
    write_generic(core, class, element_size)?;
    write_generic(core, class + 4, dimensions)?;

    context.array_classes.lock().insert(
        class,
        ArrayClassInfo {
            dimensions,
            element_class,
            atype,
            element_size,
        },
    );

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
        return instantiate_imported_class(core, context, token).await;
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

    // The activated class is not the object. Handing it back made every
    // instance of a class the same address, and gave all of them the twenty
    // byte block `vm_activate_class` allocates for the class itself as their
    // fields - so a class wrote its fields over its own statics, and a field
    // past the fifth landed in whatever allocation followed.
    //
    // Reading import 0x54 as an allocator is what made that look right: the
    // compiled code appeared to allocate the object itself and hand it to
    // `vm_instantiate`, so `vm_instantiate` had nothing left to do. It is
    // `vm_check_stack_overflow`, the object was never allocated, and
    // allocating it is the whole job.
    let activated_data: u32 = read_generic(core, handle + 8)?;
    let vtable: u32 = read_generic(core, activated_data + 12)?;

    let object = context.java_handles.allocate_instance(vtable)?;

    context.java_handles.bind(object, instance);

    tracing::debug!("LGT new {class} at {object:#x}, dispatch table {vtable:#x}");

    Ok(object)
}

/// Allocates an instance of the class a token names, with its dispatch table
/// installed.
async fn instantiate_imported_class(_core: &mut ArmCore, context: &mut InitSvcContext, class_object: u32) -> Result<u32> {
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

    let object = context.java_handles.allocate_instance(vtable)?;

    match context.jvm.instantiate_class(&name).await {
        Ok(instance) => {
            context.java_handles.bind(object, instance);
            tracing::debug!("LGT new {name} instance at {object:#x}, dispatch table {vtable:#x}, JVM instance bound");
        }
        Err(error) => {
            tracing::warn!("LGT could not create uninitialized JVM instance for {name} at {object:#x}: {error:?}");
        }
    }

    Ok(object)
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
const KNOWN_DISPATCH_SLOTS: &[(&str, u32, &str, &str)] = &[
    // `dt_org_kwis_msp_lcdui_Card` maps slot 0 to its canonical
    // no-argument constructor. Unused ARM argument registers may still
    // contain arbitrary values at this call site.
    ("org/kwis/msp/lcdui/Card", 0, "<init>", "()V"),
    ("java/lang/Thread", 10, "start", "()V"),
    // `dt_java_lang_StringBuffer` names this one. A platform class's slots
    // are numbered over *all* its virtual methods, and the table the
    // application registers lists only the ones it imports, so the two agree
    // on where a class's methods start and not on the order within.
    ("java/lang/StringBuffer", 18, "append", "(Ljava/lang/String;)Ljava/lang/StringBuffer;"),
    // Legend of Master reaches append(int) through platform dispatch slot 23.
    ("java/lang/StringBuffer", 23, "append", "(I)Ljava/lang/StringBuffer;"),
    // `dt_java_lang_String` maps slot 10 to String.length() and
    // slot 33 to String.trim().
    ("java/lang/String", 10, "length", "()I"),
    ("java/lang/String", 11, "charAt", "(I)C"),
    ("java/lang/String", 33, "trim", "()Ljava/lang/String;"),
    ("java/lang/Class", 16, "getResourceAsStream", "(Ljava/lang/String;)Ljava/io/InputStream;"),
    ("java/io/ByteArrayInputStream", 11, "read", "([B)I"),
    ("java/io/ByteArrayInputStream", 12, "read", "([BII)I"),
    ("java/io/ByteArrayInputStream", 14, "available", "()I"),
    ("java/io/ByteArrayInputStream", 15, "close", "()V"),
    // Clip inherits BaseClip's virtual methods after Object slots 1..9.
    ("org/kwis/msp/media/Clip", 12, "availableDataSize", "()I"),
];

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
    ("wait", "(J)V"),
    ("wait", "(JI)V"),
    ("wait", "()V"),
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

    // Resolve fixed native dispatch slots from the receiver's actual platform
    // class even when the application never imported that class or method.
    if let Some(receiver_class) = context.java_handles.get(this).map(|instance| instance.class_definition().name())
        && let Some((name, descriptor)) = platform_method(&receiver_class, slot)
    {
        let member = ResolvedMember {
            class_name: receiver_class.clone(),
            name: name.into(),
            descriptor: descriptor.into(),
        };

        let handles = context.java_handles.clone();
        let jvm = context.jvm.clone();

        return match method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await {
            Ok(result) => Ok(result),
            Err(error) => {
                tracing::warn!("LGT {receiver_class}.{name}{descriptor} at slot {slot} failed: {error}");
                Ok(0)
            }
        };
    }

    let imported_class = context
        .imported_classes
        .lock()
        .as_ref()
        .and_then(|table| table.classes.get(class_index as usize).map(|class| class.name.clone()));

    // Objects retained by JavaHandles may use the shared fallback table when
    // their JVM class was not present in the compiled application's import
    // table. Recover their actual runtime class before giving up.
    let class = imported_class.or_else(|| context.java_handles.get(this).map(|instance| instance.class_definition().name()));

    let Some(class) = class else {
        tracing::warn!("LGT undeclared dispatch slot {slot} called on {this:#x}, class_index={class_index}");
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

pub async fn load_native(core: &mut ArmCore, system: &mut System, jvm: &Jvm, data: &[u8], _main_class_name: Option<&str>) -> Result<()> {
    let (entrypoint, image_ranges) = load_executable(core, data)?;
    register_wipic_svc_handler(core, system, jvm)?;
    register_stdlib_svc_handler(core, system)?;
    register_init_svc_handler(core, system, jvm, Arc::new(image_ranges))?;

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

fn validate_resolved_import_address(import_table: u32, function_index: u32, address: u32) -> Result<u32> {
    if address < 0x100 {
        return Err(WieError::FatalError(format!(
            "Invalid resolved LGT import address:              table={import_table:#x},              function={function_index:#x},              address={address:#x}"
        )));
    }

    if address & 1 == 0 {
        return Err(WieError::FatalError(format!(
            "Resolved LGT import is not a Thumb address:              table={import_table:#x},              function={function_index:#x},              address={address:#x}"
        )));
    }

    Ok(address)
}

async fn get_import_function(
    core: &mut ArmCore,
    wipic_category: u32,
    stdlib_category: u32,
    cache: &ImportFunctionCache,
    import_table: u32,
    function_index: u32,
) -> Result<u32> {
    let key = (import_table, function_index);

    if let Some(&cached) = cache.lock().get(&key) {
        let cached = validate_resolved_import_address(import_table, function_index, cached)?;

        tracing::debug!("get_import_function({import_table:#x},              {function_index:#x}) -> cached {cached:#x}");

        return Ok(cached);
    }

    tracing::debug!("get_import_function({import_table:#x}, {function_index:#x})");

    let resolved = if import_table == 0x1fb {
        core.make_svc_stub(wipic_category, function_index)?
    } else if import_table == 0x64 {
        get_java_interface_method(core, function_index)?
    } else if import_table == 1 {
        core.make_svc_stub(stdlib_category, function_index)?
    } else {
        match (import_table, function_index) {
            (0x1f8, 0x16) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::Unk0)?,
            (0x1f8, 0x17) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk7)?,
            (0x1fc, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk1)?,
            (0x1ff, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk2)?,
            (0x201, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk3)?,
            _ => {
                if import_table > UNRESOLVED_IMPORT_FIELD_MASK || function_index > UNRESOLVED_IMPORT_FIELD_MASK {
                    return Err(WieError::FatalError(format!(
                        "Unknown import cannot be encoded:                          table={import_table:#x},                          function={function_index:#x}"
                    )));
                }

                let diagnostic_id = UNRESOLVED_IMPORT_SVC_BASE | (import_table << 12) | function_index;

                let stub = core.make_svc_stub(SVC_CATEGORY_INIT, diagnostic_id)?;

                tracing::warn!(
                    "Unknown import function:                      table={import_table:#x},                      function={function_index:#x};                      installed diagnostic stub {stub:#x}"
                );

                stub
            }
        }
    };

    let resolved = validate_resolved_import_address(import_table, function_index, resolved)?;

    cache.lock().insert(key, resolved);

    tracing::debug!("get_import_function({import_table:#x},          {function_index:#x}) -> resolved {resolved:#x}");

    Ok(resolved)
}

fn read_u32_le(data: &[u8], offset: usize, what: &str) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| WieError::FatalError(format!("truncated {what} at file offset {offset:#x}")))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn symbol_value(data: &[u8], symtab_offset: usize, symtab_size: usize, symtab_entsize: usize, symbol_index: usize) -> Result<(u32, u16)> {
    if symtab_entsize < 16 {
        return Err(WieError::FatalError(format!("invalid ELF32 symbol entry size: {symtab_entsize}")));
    }
    let entry = symtab_offset
        .checked_add(
            symbol_index
                .checked_mul(symtab_entsize)
                .ok_or_else(|| WieError::FatalError("symbol index overflow".into()))?,
        )
        .ok_or_else(|| WieError::FatalError("symbol offset overflow".into()))?;
    if entry + 16 > symtab_offset + symtab_size || entry + 16 > data.len() {
        return Err(WieError::FatalError(format!("ELF symbol index {symbol_index} is out of range")));
    }
    Ok((
        read_u32_le(data, entry + 4, "ELF symbol value")?,
        u16::from_le_bytes([data[entry + 14], data[entry + 15]]),
    ))
}

fn section_load_bias(section_headers: &[elf::section::SectionHeader], address: u32) -> Option<i32> {
    const SHF_ALLOC: u64 = 0x2;

    section_headers.iter().find_map(|section| {
        if section.sh_flags & SHF_ALLOC == 0 || section.sh_size == 0 {
            return None;
        }
        let start = section.sh_addr as u32;
        let end = start.checked_add(section.sh_size as u32)?;
        if (start..end).contains(&address) {
            // WIE currently maps allocatable sections at their linked virtual
            // addresses. Keeping this calculation explicit preserves Raptor
            // ER semantics when rebased section loading is added later.
            Some(0)
        } else {
            None
        }
    })
}

fn has_raptor_metadata(data: &[u8], section_headers: &[elf::section::SectionHeader]) -> bool {
    section_headers.iter().any(|section| {
        let offset = section.sh_offset as usize;
        let size = section.sh_size as usize;

        size >= 4 && data.get(offset..offset.saturating_add(4)).is_some_and(|magic| magic == b"RAPT")
    })
}

fn apply_relocations(core: &mut ArmCore, data: &[u8], section_headers: &[elf::section::SectionHeader]) -> Result<()> {
    const SHT_RELA: u32 = 4;
    const SHT_REL: u32 = 9;

    for (relocation_section_index, shdr) in section_headers.iter().enumerate() {
        if shdr.sh_type != SHT_REL && shdr.sh_type != SHT_RELA {
            continue;
        }

        // Raptor ER private relocation sections may not link a conventional
        // ELF symbol table. Resolve the table lazily only for standard ARM
        // relocations.
        let symtab = section_headers.get(shdr.sh_link as usize);
        let prelinked_raptor = symtab.is_none() && has_raptor_metadata(data, section_headers);

        if prelinked_raptor {
            tracing::warn!(
                "Raptor prelinked relocation section #{relocation_section_index}:                  invalid symtab link {}; preserving linked relocation values",
                shdr.sh_link
            );
        }

        let rel_entsize = if shdr.sh_entsize == 0 {
            if shdr.sh_type == SHT_RELA { 12 } else { 8 }
        } else {
            shdr.sh_entsize as usize
        };
        let minimum_entry_size = if shdr.sh_type == SHT_RELA { 12 } else { 8 };
        if rel_entsize < minimum_entry_size {
            return Err(WieError::FatalError(format!("invalid relocation entry size: {rel_entsize}")));
        }

        let rel_offset = shdr.sh_offset as usize;
        let rel_size = shdr.sh_size as usize;
        let count = rel_size / rel_entsize;
        tracing::debug!("Applying {count} relocation(s) from section #{relocation_section_index}");

        // Raptor ER uses private relocation types 252..255. R_ARM_RBASE
        // records bind a compact segment id (r_sym) to the segment containing
        // r_offset; following records select that segment id instead of a
        // normal ELF symbol.
        let mut raptor_segment_biases = BTreeMap::<usize, i32>::new();

        for index in 0..count {
            let entry_offset = rel_offset
                .checked_add(
                    index
                        .checked_mul(rel_entsize)
                        .ok_or_else(|| WieError::FatalError("relocation index overflow".into()))?,
                )
                .ok_or_else(|| WieError::FatalError("relocation offset overflow".into()))?;
            let place = read_u32_le(data, entry_offset, "ELF relocation offset")?;
            let info = read_u32_le(data, entry_offset + 4, "ELF relocation info")?;
            let relocation_type = info & 0xff;
            let symbol_index = (info >> 8) as usize;

            if relocation_type == R_ARM_RBASE {
                let Some(bias) = section_load_bias(section_headers, place) else {
                    tracing::warn!(
                        "Invalid R_ARM_RBASE segment {symbol_index} at relocation #{index}: address {place:#x} is outside allocatable sections"
                    );
                    continue;
                };
                raptor_segment_biases.insert(symbol_index, bias);
                tracing::debug!("Registered Raptor ER segment {symbol_index} at {place:#x} with load bias {bias:#x}");
                continue;
            }

            if matches!(relocation_type, R_ARM_RABS32 | R_ARM_RPC24 | R_ARM_RREL32) {
                let Some(&target_bias) = raptor_segment_biases.get(&symbol_index) else {
                    tracing::warn!(
                        "Raptor ER relocation type {relocation_type} at {place:#x} references unknown segment {symbol_index}; leaving original value unchanged"
                    );
                    continue;
                };
                let Some(place_bias) = section_load_bias(section_headers, place) else {
                    tracing::warn!("Raptor ER relocation place is outside allocatable sections: {place:#x}; leaving original value unchanged");
                    continue;
                };

                match relocation_type {
                    R_ARM_RABS32 => {
                        let addend = if shdr.sh_type == SHT_RELA {
                            read_u32_le(data, entry_offset + 8, "Raptor ER RELA addend")?
                        } else {
                            read_generic(core, place)?
                        };
                        write_generic(core, place, raptor_rabs32(addend, target_bias))?;
                    }
                    R_ARM_RREL32 => {
                        let addend = if shdr.sh_type == SHT_RELA {
                            read_u32_le(data, entry_offset + 8, "Raptor ER RELA addend")?
                        } else {
                            read_generic(core, place)?
                        };
                        write_generic(core, place, raptor_rrel32(addend, target_bias, place_bias))?;
                    }
                    R_ARM_RPC24 => {
                        let instruction: u32 = read_generic(core, place)?;
                        write_generic(core, place, raptor_rpc24(instruction, target_bias, place_bias)?)?;
                    }
                    _ => unreachable!(),
                }
                continue;
            }

            if prelinked_raptor {
                match relocation_type {
                    // These Raptor executable images are linked at their final
                    // virtual addresses. ABS32 values already point into the
                    // mapped text/data/bss sections, and PC24 instructions
                    // already encode their final branch targets.
                    R_ARM_ABS32 | R_ARM_PC24 | 15 => {
                        continue;
                    }
                    _ => {
                        return Err(WieError::FatalError(format!(
                            "Unsupported prelinked Raptor relocation:                              section={relocation_section_index},                              index={index},                              type={relocation_type},                              symbol={symbol_index:#x},                              place={place:#x}"
                        )));
                    }
                }
            }

            let symtab = symtab.ok_or_else(|| {
                WieError::FatalError(format!(
                    "relocation section {relocation_section_index} has invalid symtab link {}",
                    shdr.sh_link
                ))
            })?;
            let symtab_entsize = if symtab.sh_entsize == 0 { 16 } else { symtab.sh_entsize as usize };
            let (symbol, symbol_section_index) =
                symbol_value(data, symtab.sh_offset as usize, symtab.sh_size as usize, symtab_entsize, symbol_index)?;
            if symbol_index != 0 && symbol_section_index == 0 {
                tracing::warn!(
                    "Unresolved ELF symbol #{symbol_index} for relocation type {relocation_type} at {place:#x}; leaving original value unchanged"
                );
                continue;
            }

            match relocation_type {
                R_ARM_NONE => {}
                R_ARM_ABS32 => {
                    let addend = if shdr.sh_type == SHT_RELA {
                        read_u32_le(data, entry_offset + 8, "ELF RELA addend")?
                    } else {
                        read_generic(core, place)?
                    };
                    write_generic(core, place, arm_abs32(addend, symbol))?;
                }
                R_ARM_REL32 => {
                    let addend = if shdr.sh_type == SHT_RELA {
                        read_u32_le(data, entry_offset + 8, "ELF RELA addend")?
                    } else {
                        read_generic(core, place)?
                    };
                    write_generic(core, place, arm_rel32(place, addend, symbol))?;
                }
                R_ARM_PC24 | R_ARM_CALL | R_ARM_JUMP24 => {
                    let instruction: u32 = read_generic(core, place)?;
                    let addend = if shdr.sh_type == SHT_RELA {
                        read_u32_le(data, entry_offset + 8, "ELF RELA addend")? as i32
                    } else {
                        let imm24 = (instruction & 0x00ff_ffff) as i32;
                        (imm24 << 8) >> 6
                    };
                    write_generic(core, place, arm_pc24(instruction, place, symbol, addend)?)?;
                }
                R_ARM_THM_CALL | R_ARM_THM_JUMP24 => {
                    let upper: u16 = read_generic(core, place)?;
                    let lower: u16 = read_generic(core, place + 2)?;
                    let addend = if shdr.sh_type == SHT_RELA {
                        read_u32_le(data, entry_offset + 8, "ELF RELA addend")? as i32
                    } else {
                        let hi = (upper & 0x07ff) as i32;
                        let lo = (lower & 0x07ff) as i32;
                        (((hi << 12) | (lo << 1)) << 9) >> 9
                    };
                    let (new_upper, new_lower) = thumb_pc22(upper, lower, place, symbol, addend)?;
                    write_generic(core, place, new_upper)?;
                    write_generic(core, place + 2, new_lower)?;
                }
                unsupported => tracing::warn!(
                    "Unsupported ARM relocation type {unsupported} at {place:#x} (symbol #{symbol_index}={symbol:#x}); leaving original value unchanged"
                ),
            }
        }
    }

    Ok(())
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
    let section_headers: alloc::vec::Vec<_> = shdrs.iter().collect();

    let mut ranges = Vec::new();

    for shdr in &section_headers {
        let section_name = strtab
            .get(shdr.sh_name as usize)
            .map_err(|x| WieError::FatalError(format!("Invalid ELF section name index {}: {x}", shdr.sh_name)))?;

        if shdr.sh_addr != 0 {
            tracing::debug!("Section {section_name} at {:x}", shdr.sh_addr);

            if shdr.sh_type == 8 {
                let zeroes = vec![0u8; shdr.sh_size as usize];
                core.load(&zeroes, shdr.sh_addr as u32, zeroes.len())?;
            } else {
                let section_data = elf
                    .section_data(shdr)
                    .map_err(|x| WieError::FatalError(format!("Failed to read ELF section {section_name}: {x}")))?
                    .0;
                core.load(section_data, shdr.sh_addr as u32, shdr.sh_size as usize)?;
            }

            ranges.push((shdr.sh_addr as u32, shdr.sh_size as u32));
        }
    }

    apply_relocations(core, data, &section_headers)?;

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
