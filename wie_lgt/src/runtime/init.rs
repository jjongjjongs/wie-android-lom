use alloc::{collections::{BTreeMap, BTreeSet}, format, sync::Arc, vec, vec::Vec};
use alloc::string::String;
use core::mem::size_of;

use elf::{ElfBytes, endian::AnyEndian};

use jvm::{JavaType, Jvm, runtime::JavaLangString};
use spin::Mutex;
use wipi_types::lgt::{InitParam1, InitParam2, InitStruct};

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId, ThreadId};
use wie_util::{
    ByteRead, ByteWrite, Result, WieError, read_generic, read_null_terminated_string_bytes, write_generic,
    write_null_terminated_string_bytes,
};

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
            ArrayClassInfo, ArrayClasses, DISPATCH_TABLE_SLOTS, JAVA_DIAG_SVC_BASE, JAVA_INTERFACE_METHOD_SVC_BASE, JAVA_METHOD_SVC_LIMIT,
            JAVA_RESERVED_SLOT_SVC_BASE, JAVA_STATIC_METHOD_SVC_BASE, JAVA_UNKNOWN_SLOT_SVC_BASE, JAVA_VIRTUAL_METHOD_SVC_BASE, REFERENCE_SIZE,
            bridge_class_chain,
            java_load_classes, java_resolve_one, vm_run_main_class,
            primitive_element_size,
            vm_get_constant_string, vm_instantiate_array,
        },
        method_bridge::{self, ResolvedMember},
        platform_metadata::platform_class,
    },
    savepoint::SavePointState,
    stdlib::register_stdlib_svc_handler,
    svc_ids::InitSvcId,
    wipi_c::register_wipic_svc_handler,
};

type JavaClassTables = Arc<Mutex<BTreeMap<u32, (u32, u32)>>>;
/// Compiled classes the application registered, published by import `0x07`.
type AppClasses = Arc<Mutex<Vec<AppClass>>>;
type JavaActivatedClasses = Arc<Mutex<BTreeMap<u32, u32>>>;
/// DLET process-local properties. Each entry is `(value, size)`, matching the
/// native local-data node's `+0x0c` value and `+0x10` size fields.
type DletProperties = Arc<Mutex<BTreeMap<u32, (u32, u32)>>>;

fn dlet_set_process_local_property(
    properties: &DletProperties,
    applet: u32,
    property: u32,
    value: u32,
    size: u32,
) -> Result<u32> {
    // LoM uses the process-local form (applet == 0) with scalar properties
    // (size == 0). Native dprocess_set_local_data stores `value` directly in
    // the node and returns zero.
    if applet != 0 || size != 0 {
        return Err(WieError::FatalError(format!(
            "Unsupported DLET property write: applet={applet:#x}, property={property}, value={value:#x}, size={size}"
        )));
    }

    properties.lock().insert(property, (value, size));

    Ok(0)
}

fn dlet_get_process_local_property(
    core: &mut ArmCore,
    properties: &DletProperties,
    applet: u32,
    property: u32,
    output: u32,
) -> Result<u32> {
    // Native dlet_get_property(0, ...) resolves the current process. LoM only
    // uses that process-local form.
    if applet != 0 || output == 0 {
        return Ok(u32::MAX);
    }

    match properties.lock().get(&property).copied() {
        Some((value, size)) => {
            write_generic(core, output, value)?;
            Ok(size)
        }
        // Native dprocess_get_local_data returns -2002 when no node exists for
        // the requested property.
        None => Ok((-2002i32) as u32),
    }
}

type ImportFunctionCache = Arc<Mutex<BTreeMap<(u32, u32), u32>>>;
type UnresolvedImportCallCounts = Arc<Mutex<BTreeMap<(u32, u32), u64>>>;
/// Platform classes the application imports, published by import `0x14`.
type ImportedClasses = Arc<Mutex<Option<ClassTable>>>;
type SyntheticClasses = Arc<Mutex<BTreeMap<u32, String>>>;
type HeavyLinkedClasses = Arc<Mutex<BTreeSet<u32>>>;
/// Native monitor ownership keyed by the guest object's address.
///
/// Native `vm_monitor_enter` stores the owning dthread and recursion depth in a
/// lazily allocated monitor attached to every object. The compiled application
/// can synchronize on guest-only objects such as primitive arrays, which have
/// no RustJava `ClassInstance`, so monitor identity must not depend on
/// `JavaHandles`.
type VmMonitors = Arc<Mutex<BTreeMap<u32, (ThreadId, u32)>>>;

const UNRESOLVED_IMPORT_SVC_BASE: u32 = 0x1000_0000;
const UNRESOLVED_IMPORT_FIELD_MASK: u32 = 0x0fff;

/// Where a class's metadata keeps its dispatch table and how many slots that
/// table has.
const CLASS_DISPATCH_TABLE: u32 = 0x0c;
const CLASS_DISPATCH_SLOTS: u32 = 0x26;

/// Guard on how far a class hierarchy is walked looking for a platform class.
const MAX_SUPERCLASS_DEPTH: usize = 32;

/// Native CLDC initializes both `the_vm_reschedule_count` and its threshold
/// from configuration 32, whose LGT value is 100.
const VM_RESCHEDULE_COUNT_THRESHOLD: u32 = 100;

/// `unit_sched_time` is configuration 31 (4 ms); a new native exec-env stores
/// five units in its scheduling interval, i.e. 20 ms.
const VM_RESCHEDULE_INTERVAL_MS: u64 = 20;

fn vm_thread_reschedule_due(count: &Mutex<u32>) -> bool {
    let mut count = count.lock();

    // Native compares the old value with zero, then decrements it with a
    // non-flag-setting SUB. Starting from 100, calls 1..=100 therefore
    // store 99..=0 and return; call 101 enters the slow path and reloads 100.
    if *count > 0 {
        *count -= 1;
        false
    } else {
        *count = VM_RESCHEDULE_COUNT_THRESHOLD;
        true
    }
}

fn vm_exec_env_thread_id(core: &ArmCore) -> ThreadId {
    // Match SavePointState: zero represents bootstrap/native-loader execution
    // before an ArmCoreThreadWrapper owns the register context.
    core.current_thread_id().unwrap_or(0)
}

fn vm_ensure_reschedule_deadline(
    deadlines: &Mutex<BTreeMap<ThreadId, u64>>,
    thread_id: ThreadId,
    now: u64,
) -> u64 {
    let mut deadlines = deadlines.lock();

    *deadlines
        .entry(thread_id)
        .or_insert_with(|| now.saturating_add(VM_RESCHEDULE_INTERVAL_MS))
}

/// Attempts one native-style monitor enter.
///
/// A missing entry is an unlocked object. Re-entering from the owning native
/// thread increments the recursion depth. A different owner leaves the state
/// untouched so the caller can yield and retry without blocking the executor.
fn vm_monitor_try_enter(monitors: &VmMonitors, object: u32, thread_id: ThreadId) -> bool {
    let mut monitors = monitors.lock();

    match monitors.get_mut(&object) {
        Some((owner, depth)) if *owner == thread_id => {
            *depth = depth.saturating_add(1);
            true
        }
        Some(_) => false,
        None => {
            monitors.insert(object, (thread_id, 1));
            true
        }
    }
}

/// Releases one recursion level if `thread_id` owns `object`.
///
/// Native clears the owner and unlocks only when the recursion depth reaches
/// zero. `false` is the native IllegalMonitorStateException condition.
fn vm_monitor_exit_owned(monitors: &VmMonitors, object: u32, thread_id: ThreadId) -> bool {
    let mut monitors = monitors.lock();

    let Some((owner, depth)) = monitors.get_mut(&object) else {
        return false;
    };
    if *owner != thread_id {
        return false;
    }

    if *depth > 1 {
        *depth -= 1;
    } else {
        monitors.remove(&object);
    }

    true
}

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
    dlet_properties: DletProperties,
    import_function_cache: ImportFunctionCache,
    unresolved_import_call_counts: UnresolvedImportCallCounts,
    /// Array classes handed out by `vm_get_array_class`, to the size of one of
    /// their elements.
    array_classes: ArrayClasses,
    save_points: SavePointState,
    /// Platform classes not explicitly imported by the application but whose
    /// identity must still cross the compiled/native boundary (notably VM-
    /// created exception subclasses).
    synthetic_classes: SyntheticClasses,
    /// Application roots whose native heavy method slots have already been
    /// linked and written back to their guest method rows.
    heavy_linked_classes: HeavyLinkedClasses,
    /// Native VM-global reschedule countdown.
    vm_reschedule_count: Arc<Mutex<u32>>,
    /// Per-native-thread millisecond deadline corresponding to exec-env
    /// +0x10/+0x14. An entry appears when that thread first needs an exec-env.
    vm_reschedule_deadlines_ms: Arc<Mutex<BTreeMap<ThreadId, u64>>>,
    /// Guest-object monitor owner and recursion depth.
    vm_monitors: VmMonitors,
}

fn register_init_svc_handler(
    core: &mut ArmCore,
    system: &System,
    jvm: &Jvm,
    image_ranges: ImageRanges,
    save_points: &SavePointState,
    dlet_properties: DletProperties,
) -> Result<()> {
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
            dlet_properties,
            import_function_cache: Default::default(),
            unresolved_import_call_counts: Default::default(),
            array_classes: Default::default(),
            save_points: save_points.clone(),
            synthetic_classes: Default::default(),
            heavy_linked_classes: Default::default(),
            vm_reschedule_count: Arc::new(Mutex::new(VM_RESCHEDULE_COUNT_THRESHOLD)),
            vm_reschedule_deadlines_ms: Default::default(),
            vm_monitors: Default::default(),
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

    if id.0 >= JAVA_INTERFACE_METHOD_SVC_BASE && id.0 < JAVA_INTERFACE_METHOD_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let index = id.0 - JAVA_INTERFACE_METHOD_SVC_BASE;
        let result = invoke_imported_interface(core, context, index).await?;

        return result.write(core, lr);
    }

    if id.0 >= JAVA_RESERVED_SLOT_SVC_BASE && id.0 < JAVA_RESERVED_SLOT_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let index = id.0 - JAVA_RESERVED_SLOT_SVC_BASE;
        let result = call_reserved_slot(core, context, index).await?;

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
        InitSvcId::DletSetProperty => dlet_set_process_local_property(
            &context.dlet_properties,
            core.read_param(0)?,
            core.read_param(1)?,
            core.read_param(2)?,
            core.read_param(3)?,
        )?
        .write(core, lr),
        InitSvcId::DletGetProperty => dlet_get_process_local_property(
            core,
            &context.dlet_properties,
            core.read_param(0)?,
            core.read_param(1)?,
            core.read_param(2)?,
        )?
        .write(core, lr),
        InitSvcId::VmClassIsAssignableTo => {
            let source = core.read_param(0)?;
            let target = core.read_param(1)?;
            let assignable = class_is_assignable_to(core, context, source, target).await?;
            u32::from(assignable).write(core, lr)
        }
        InitSvcId::VmCheckStackOverflow => {
            let words = core.read_param(0)?;
            tracing::trace!("vm_check_stack_overflow({words})");

            // Native vm_check_stack_overflow obtains the current exec-env.
            // Creating that exec-env initializes this thread's first deadline.
            let thread_id = vm_exec_env_thread_id(core);
            let now = context.system.platform().now().raw();
            vm_ensure_reschedule_deadline(
                &context.vm_reschedule_deadlines_ms,
                thread_id,
                now,
            );

            0u32.write(core, lr)
        }
        InitSvcId::VmThreadReschedule => {
            tracing::trace!("vm_thread_reschedule()");

            if vm_thread_reschedule_due(&context.vm_reschedule_count) {
                let thread_id = vm_exec_env_thread_id(core);
                let now = context.system.platform().now().raw();

                // Native slow path calls vm_get_exec_env for the current
                // dthread. A first use creates only this thread's now+20 ms
                // deadline before the comparison.
                let deadline = vm_ensure_reschedule_deadline(
                    &context.vm_reschedule_deadlines_ms,
                    thread_id,
                    now,
                );

                // Native vm_thread_reschedule returns without yielding while
                // the current time is at or before the exec-env deadline.
                if now > deadline {
                    context.system.yield_now().await;

                    // Native refreshes only the current exec-env deadline from
                    // a new time sample after dthread_yield returns.
                    let next_deadline = context
                        .system
                        .platform()
                        .now()
                        .raw()
                        .saturating_add(VM_RESCHEDULE_INTERVAL_MS);
                    context
                        .vm_reschedule_deadlines_ms
                        .lock()
                        .insert(thread_id, next_deadline);
                }
            }

            0u32.write(core, lr)
        }
        InitSvcId::VmMonitorEnter => {
            let object = core.read_param(0)?;
            if object == 0 {
                return Err(WieError::FatalError("vm_monitor_enter(null)".into()));
            }

            let thread_id = vm_exec_env_thread_id(core);
            tracing::trace!("vm_monitor_enter({object:#x}) thread={thread_id}");

            while !vm_monitor_try_enter(&context.vm_monitors, object, thread_id) {
                context.system.yield_now().await;
            }

            0u32.write(core, lr)
        }
        InitSvcId::VmMonitorExit => {
            let object = core.read_param(0)?;
            if object == 0 {
                return Err(WieError::FatalError("vm_monitor_exit(null)".into()));
            }

            let thread_id = vm_exec_env_thread_id(core);
            tracing::trace!("vm_monitor_exit({object:#x}) thread={thread_id}");

            if !vm_monitor_exit_owned(&context.vm_monitors, object, thread_id) {
                return Err(WieError::FatalError(format!(
                    "vm_monitor_exit({object:#x}) from non-owner thread {thread_id}"
                )));
            }

            0u32.write(core, lr)
        }
        InitSvcId::VmAllocSavePoint => {
            let depth = core.read_param(0)?;
            let save_point = context.save_points.alloc(core, depth)?;
            tracing::debug!("vm_alloc_save_point({depth}) -> {save_point:#x}");
            save_point.write(core, lr)
        }
        InitSvcId::VmFreeSavePoint => {
            let depth = core.read_param(0)?;
            let save_point = context.save_points.free(core, depth)?;
            tracing::debug!("vm_free_save_point({depth}) -> {save_point:#x}");
            save_point.write(core, lr)
        }
        InitSvcId::VmInitializeClassShared => {
            let root = core.read_param(0)?;
            let meta: u32 = read_generic(core, root + 8)?;
            write_generic(core, meta + 0x1a, 3u16)?;

            tracing::debug!(
                "LGT vm_initialize_class_shared(root={root:#x}, meta={meta:#x}) -> state=3"
            );
            root.write(core, lr)
        }
        InitSvcId::VmActivateClass => {
            let root = core.read_param(0)?;
            let table = core.read_param(1)?;

            if let Some(&activated) = context.java_activated_classes.lock().get(&root) {
                tracing::debug!(
                    "LGT vm_activate_class(root={root:#x}, table={table:#x}) -> cached={activated:#x}"
                );
                return activated.write(core, lr);
            }

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
            write_generic(core, data + 12, instance_vtable)?;

            let activated = Allocator::alloc(core, 12)?;
            write_generic(core, activated, class_vtable)?;
            write_generic(core, activated + 4, 0u32)?;
            write_generic(core, activated + 8, data)?;

            context.java_activated_classes.lock().insert(root, activated);

            tracing::debug!(
                "LGT vm_activate_class(root={root:#x}, table={table:#x}) -> handle={activated:#x}, data={data:#x}, class_vtable={class_vtable:#x}, instance_vtable={instance_vtable:#x}"
            );
            activated.write(core, lr)
        }
        InitSvcId::VmInitializeClass => {
            let handle = core.read_param(0)?;
            let callback = core.read_param(1)?;
            let activated_data: u32 = read_generic(core, handle + 8)?;

            write_generic(core, activated_data + 0x10, 5u16)?;

            if callback != 0 {
                let _: u32 = core.run_function(callback, &[handle]).await?;
            }

            tracing::debug!(
                "LGT vm_initialize_class(handle={handle:#x}, data={activated_data:#x}, callback={callback:#x})"
            );
            handle.write(core, lr)
        }
        InitSvcId::VmAastoreImpl | InitSvcId::VmAastoreImplFast => {
            let array = core.read_param(0)?;
            let index = core.read_param(1)?;
            let value = core.read_param(2)?;

            // Native vm_aastore_impl checks null; the fast variant intentionally
            // omits this check and immediately dereferences array+8.
            if id.0 == InitSvcId::VmAastoreImpl as u32 && array == 0 {
                return throw_vm_exception(
                    core,
                    context,
                    "java/lang/NullPointerException",
                )
                .await;
            }

            let data: u32 = read_generic(core, array + 8)?;
            let length: u32 = read_generic(core, data)?;

            if index >= length {
                return throw_vm_exception(
                    core,
                    context,
                    "java/lang/ArrayIndexOutOfBoundsException",
                )
                .await;
            }

            if value != 0 {
                let array_class = object_class_root(core, array)?;
                let target_class = array_component_class(&context.array_classes, array_class)?;
                let source_class = object_class_root(core, value)?;

                if !class_is_assignable_to(core, context, source_class, target_class).await? {
                    return throw_vm_exception(
                        core,
                        context,
                        "java/lang/ArrayStoreException",
                    )
                    .await;
                }
            }

            write_generic(core, data + 4 + index * REFERENCE_SIZE, value)?;
            0u32.write(core, lr)
        }
        InitSvcId::VmInstantiate => {
            let token = core.read_param(0)?;
            let instance = instantiate(core, context, token).await?;

            tracing::debug!(
                "LGT vm_instantiate({token:#x}, lr={lr:#x}) -> {instance:#x}"
            );
            instance.write(core, lr)
        }
        // Native WIPI-Java and LGTE activation publish their already
        // registered class tables into a process-local visibility table.
        // WIE has no equivalent visibility gate: the complete native platform
        // class set is always available through `platform_class`. Exact native
        // class-table comparison covers all 106 WIPI-Java and 17 LGTE nonzero
        // class slots, so no additional activation state is required here.
        InitSvcId::WipiJavaModuleActivate | InitSvcId::LgteModuleActivate => 0u32.write(core, lr),
        // Native bankon_lib_module_activate is itself a zero-return no-op.
        InitSvcId::BankonModuleActivate => 0u32.write(core, lr),
        // Native CLDC import 0x03 is `cldc_module_activate`. The native runtime
        // registers per-dprocess VM state here, then activates its built-in
        // classes. WIE already owns one JVM/context for this emulated process;
        // application class registration/loading follows through imports
        // 0x07/0x14, so no additional process-local state is required here.
        InitSvcId::CldcModuleActivate => 0u32.write(core, lr),
        // Older LGT-generated binaries split part of CLDC startup across
        // table-0x64 imports 0xfa and 0x61 before WIPI-Java/LGTE activation.
        // The exact native helper names are revision-specific, but their
        // observable contract here is bootstrap side effects only: unrelated
        // games use the same sequence and discard the wrapper return value.
        // WIE already provides the corresponding process/class visibility
        // through its single runtime context, so both compatibility hooks are
        // intentionally zero-return no-ops, matching CldcModuleActivate.
        InitSvcId::LegacyCldcBootstrapFa | InitSvcId::LegacyCldcBootstrap61 => 0u32.write(core, lr),
        // Native Java interface import 0x06 is
        // `vm_unregister_classes(index)`. Import 0x07 returns this index when
        // the application's compiled classes are registered.
        InitSvcId::VmUnregisterClasses => {
            let index = core.read_param(0)?;
            let registered = context.java_class_tables.lock().remove(&index);

            if let Some((classes, _runtime_table)) = registered {
                let roots: Vec<u32> = app_classes::parse_registered_classes(core, classes)?
                    .into_iter()
                    .map(|class| class.root)
                    .collect();

                context.app_classes.lock().retain(|class| !roots.contains(&class.root));
            context
                .heavy_linked_classes
                .lock()
                .retain(|root| !roots.contains(root));

                tracing::debug!(
                    "vm_unregister_classes({index:#x}) -> removed {} application classes",
                    roots.len()
                );
            } else {
                tracing::debug!("vm_unregister_classes({index:#x}) -> no registered class table");
            }

            ().write(core, lr)
        }
        // Import 0x07. The application registers its *own* classes - the ones
        // its Java source was compiled into - as `{ u32 count, u32 pad, u32
        // root[count] }`. This is the other half of the class model: import
        // 0x14 declares what the application needs from the platform, this
        // declares what it brings.
        InitSvcId::VmRegisterClasses => {
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
        InitSvcId::JavaResolveOne => {
            let arguments: Vec<u32> = (0..12).map(|index| core.read_param(index)).collect::<Result<_>>()?;

            ensure_heavy_method_slots_linked(core, context, arguments[1])?;

            java_resolve_one(
                core,
                &context.app_classes,
                context.image_ranges.as_ref(),
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
                arguments[11],
            )
            .await?;

            ().write(core, lr)
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
        // LoM table-0x64 import 0x0d is `vm_add_classpath(path)`. It receives
        // DLET property 200, which names the same JAR WIE already supplied to
        // `JvmSupport::new_jvm` as its class source before native startup.
        // Native stores this path in VM property 1007; duplicating that
        // bookkeeping is unnecessary in WIE, so activation succeeds here.
        InitSvcId::VmAddClasspath => 0u32.write(core, lr),
        // LoM table-0x64 import 0x82 runs the Java main class. The first two
        // parameters name the class, which is `org/kwis/msp/lcdui/Main`; the
        // argument vector selects the application's Jlet.
        InitSvcId::VmRunMainClass => {
            let argc = core.read_param(2)?;
            let argv = core.read_param(3)?;

            let app_classes = context.app_classes.clone();
            let image_ranges = context.image_ranges.clone();
            let java_handles = context.java_handles.clone();

            vm_run_main_class(core, jvm, &java_handles, &app_classes, &image_ranges, argc, argv)
                .await?
                .write(core, lr)
        }
        InitSvcId::VmGetConstantString => {
            let chars = core.read_param(1)?;
            let length = core.read_param(2)?;
            let cache = core.read_param(3)?;

            let result = vm_get_constant_string(core, &context.java_handles, jvm, chars, length, cache).await?;

            result.write(core, lr)
        }
        InitSvcId::VmGetArrayClass => {
            let dimensions = core.read_param(0)?;
            let element_class = core.read_param(1)?;
            let atype = core.read_param(2)?;

            let class = get_array_class(core, context, dimensions, element_class, atype)?;

            class.write(core, lr)
        }
        InitSvcId::VmInstantiateArray => {
            let class = core.read_param(0)?;
            let length = core.read_param(1)?;

            // Native vm_instantiate_array treats the requested length as signed
            // and throws before attempting any allocation when it is negative.
            if (length as i32) < 0 {
                return throw_vm_exception(
                    core,
                    context,
                    "java/lang/NegativeArraySizeException",
                )
                .await;
            }

            vm_instantiate_array(&context.java_handles, &context.array_classes, class, length)
                .await?
                .write(core, lr)
        }
        // Native Java-interface 0x11 is
        // vm_instantiate_multi_array(array_class, dimensions, dimension_count).
        // It validates every requested length before allocating anything, then
        // recursively fills each reference-array level with the next array class.
        InitSvcId::VmInstantiateMultiArray => {
            let class = core.read_param(0)?;
            let dimensions_ptr = core.read_param(1)?;
            let dimension_count = core.read_param(2)?;

            let mut lengths = Vec::with_capacity(dimension_count as usize);
            for index in 0..dimension_count {
                let length: u32 = read_generic(core, dimensions_ptr + index * 4)?;
                if (length as i32) < 0 {
                    let class_name = "java/lang/NegativeArraySizeException";
                    let vtable = synthetic_platform_vtable(core, context, class_name)?;
                    context.java_handles.set_dispatch_table(class_name, vtable);

                    let exception = match context.jvm.instantiate_class(class_name).await {
                        Ok(exception) => exception,
                        Err(error) => {
                            return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
                        }
                    };
                    let exception = context.java_handles.address_of(exception)?;

                    tracing::debug!(
                        "vm_instantiate_multi_array({class:#x}, {dimensions_ptr:#x}, {dimension_count}) -> NegativeArraySizeException {exception:#x}"
                    );
                    return context.save_points.throw(core, exception);
                }
                lengths.push(length);
            }

            if dimension_count == 0 {
                return Err(WieError::FatalError(
                    "vm_instantiate_multi_array received zero dimensions".into(),
                ));
            }

            let root = vm_instantiate_array(
                &context.java_handles,
                &context.array_classes,
                class,
                lengths[0],
            )
            .await?;

            let mut current_class = class;
            let mut parents = vec![root];

            for depth in 1..dimension_count {
                let info = context
                    .array_classes
                    .lock()
                    .get(&current_class)
                    .copied()
                    .ok_or_else(|| {
                        WieError::FatalError(format!(
                            "vm_instantiate_multi_array class {current_class:#x} has no array metadata"
                        ))
                    })?;

                if info.dimensions <= 1 {
                    return Err(WieError::FatalError(format!(
                        "vm_instantiate_multi_array exhausted class dimensions at depth {depth}"
                    )));
                }

                let child_class = get_array_class(
                    core,
                    context,
                    info.dimensions - 1,
                    info.element_class,
                    info.atype,
                )?;

                let mut children = Vec::new();
                for parent in parents {
                    let data: u32 = read_generic(core, parent + 8)?;
                    let parent_length: u32 = read_generic(core, data)?;

                    for index in 0..parent_length {
                        let child = vm_instantiate_array(
                            &context.java_handles,
                            &context.array_classes,
                            child_class,
                            lengths[depth as usize],
                        )
                        .await?;

                        write_generic(core, data + 4 + index * REFERENCE_SIZE, child)?;
                        children.push(child);
                    }
                }

                current_class = child_class;
                parents = children;
            }

            tracing::debug!(
                "vm_instantiate_multi_array({class:#x}, {dimensions_ptr:#x}, {dimension_count}) -> {root:#x}"
            );
            root.write(core, lr)
        }
        // Native Java-interface 0x21 is vm_throw_exception(exception).
        // The native routine frees save-point depth zero and longjmps with the
        // supplied exception object. A null exception is replaced with a new
        // java/lang/NullPointerException before the throw.
        InitSvcId::VmThrowException => {
            let exception = core.read_param(0)?;

            {
                let thread_id = vm_exec_env_thread_id(core);
                let now = context.system.platform().now().raw();
                vm_ensure_reschedule_deadline(
                    &context.vm_reschedule_deadlines_ms,
                    thread_id,
                    now,
                );
            }

            if exception != 0 {
                tracing::debug!("vm_throw_exception({exception:#x}) -> longjmp");
                return context.save_points.throw(core, exception);
            }

            let class_name = "java/lang/NullPointerException";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;
            context.java_handles.set_dispatch_table(class_name, vtable);

            let exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };
            let exception = context.java_handles.address_of(exception)?;

            tracing::debug!("vm_throw_exception(0) -> NullPointerException {exception:#x} -> longjmp");
            context.save_points.throw(core, exception)
        }
        // Native Java-interface 0x22 is
        // vm_throw_null_pointer_exception(message). The wrapper preserves r0
        // as its optional NUL-terminated message and forwards the NPE class to
        // throw_exception_with_class_and_message.
        InitSvcId::VmThrowNullPointerException => {
            let message = core.read_param(0)?;
            let class_name = "java/lang/NullPointerException";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;

            context.java_handles.set_dispatch_table(class_name, vtable);

            let mut exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };

            if message != 0 {
                let message_bytes = read_null_terminated_string_bytes(core, message)?;
                let message: String = message_bytes.iter().map(|&byte| char::from(byte)).collect();
                let java_message = match JavaLangString::from_rust_string(&context.jvm, &message).await {
                    Ok(message) => message,
                    Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
                };

                if let Err(error) = context
                    .jvm
                    .put_field(
                        &mut exception,
                        "detailMessage",
                        "Ljava/lang/String;",
                        java_message,
                    )
                    .await
                {
                    return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
                }
            }

            let exception = context.java_handles.address_of(exception)?;

            tracing::debug!(
                "vm_throw_null_pointer_exception({message:#x}) -> longjmp({exception:#x})"
            );
            context.save_points.throw(core, exception)
        }
        // Native Java-interface 0x25 is
        // vm_throw_arithmetic_exception(message), with the same message ABI.
        InitSvcId::VmThrowArithmeticException => {
            let message = core.read_param(0)?;
            let class_name = "java/lang/ArithmeticException";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;

            context.java_handles.set_dispatch_table(class_name, vtable);

            let mut exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };

            if message != 0 {
                let message_bytes = read_null_terminated_string_bytes(core, message)?;
                let message: String = message_bytes.iter().map(|&byte| char::from(byte)).collect();
                let java_message = match JavaLangString::from_rust_string(&context.jvm, &message).await {
                    Ok(message) => message,
                    Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
                };

                if let Err(error) = context
                    .jvm
                    .put_field(
                        &mut exception,
                        "detailMessage",
                        "Ljava/lang/String;",
                        java_message,
                    )
                    .await
                {
                    return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
                }
            }

            let exception = context.java_handles.address_of(exception)?;

            tracing::debug!(
                "vm_throw_arithmetic_exception({message:#x}) -> longjmp({exception:#x})"
            );
            context.save_points.throw(core, exception)
        }
        // Native Java-interface 0x23 is
        // vm_throw_array_index_out_of_bounds_exception(message). It constructs
        // a real AIOOBE object, pops the current save point, then
        // longjmp(save_point, exception_object).
        InitSvcId::VmThrowArrayIndexOutOfBoundsException => {
            let message = core.read_param(1)?;
            let class_name = "java/lang/ArrayIndexOutOfBoundsException";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;

            // `JavaHandles::insert` chooses a guest vtable by JVM class name.
            context.java_handles.set_dispatch_table(class_name, vtable);

            let mut exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };

            // Native vm_throw_array_index_out_of_bounds_exception preserves r1
            // as an optional NUL-terminated message. The common throw helper
            // instantiates a Java String and stores it directly in the
            // exception's first data slot; it does not run a Throwable
            // constructor. RustJava exposes that slot as detailMessage.
            if message != 0 {
                let message_bytes = read_null_terminated_string_bytes(core, message)?;
                // Native vm_instantiate_string zero-extends each input byte
                // directly into one UTF-16 Java char. It performs no UTF-8 or
                // local-code decoding; vm_instantiate_string_from_local_code is
                // the separate conversion path.
                let message: String = message_bytes.iter().map(|&byte| char::from(byte)).collect();
                let java_message = match JavaLangString::from_rust_string(&context.jvm, &message).await {
                    Ok(message) => message,
                    Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
                };

                if let Err(error) = context
                    .jvm
                    .put_field(
                        &mut exception,
                        "detailMessage",
                        "Ljava/lang/String;",
                        java_message,
                    )
                    .await
                {
                    return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
                }
            }

            let exception = context.java_handles.address_of(exception)?;

            tracing::debug!(
                "vm_throw_array_index_out_of_bounds_exception({message:#x}) -> longjmp({exception:#x})"
            );
            context.save_points.throw(core, exception)
        }
        // Native Java-interface 0x26 is
        // vm_throw_class_cast_exception(message). The wrapper preserves its
        // incoming r0 as the optional message and tail-calls the same native
        // exception helper used by the other VM throw wrappers.
        InitSvcId::VmThrowClassCastException => {
            let message = core.read_param(0)?;
            let class_name = "java/lang/ClassCastException";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;

            context.java_handles.set_dispatch_table(class_name, vtable);

            let mut exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };

            if message != 0 {
                let message_bytes = read_null_terminated_string_bytes(core, message)?;
                let message: String = message_bytes.iter().map(|&byte| char::from(byte)).collect();
                let java_message = match JavaLangString::from_rust_string(&context.jvm, &message).await {
                    Ok(message) => message,
                    Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
                };

                if let Err(error) = context
                    .jvm
                    .put_field(
                        &mut exception,
                        "detailMessage",
                        "Ljava/lang/String;",
                        java_message,
                    )
                    .await
                {
                    return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
                }
            }

            let exception = context.java_handles.address_of(exception)?;

            tracing::debug!(
                "vm_throw_class_cast_exception({message:#x}) -> longjmp({exception:#x})"
            );
            context.save_points.throw(core, exception)
        }
        // Legacy table64 0x26 is the negative-array-size failure helper.
        // Cross-game static analysis finds 50 strict callers and every caller
        // supplies r0 == NULL, so preserve the observed legacy ABI without
        // attempting to decode a revision-specific message argument.
        InitSvcId::LegacyVmThrowNegativeArraySizeException => {
            throw_vm_exception(core, context, "java/lang/NegativeArraySizeException").await
        }
        // Older LGT-generated code reaches table64 0x06 only after a resolved
        // dispatch slot has a null implementation pointer. Across multiple
        // binaries its incoming r0 is a live VM/object value, not a C-string
        // message. Preserve the abstract-method failure semantics without
        // interpreting that revision-specific argument as text.
        InitSvcId::LegacyVmThrowAbstractMethodError => {
            let class_name = "java/lang/VirtualMachineError";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;

            context.java_handles.set_dispatch_table(class_name, vtable);

            let exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };

            let exception = context.java_handles.address_of(exception)?;

            context.save_points.throw(core, exception)
        }
        // In this native build both vm_throw_abstract_method_error (0x38)
        // and vm_throw_no_such_method_error (0x40) are aliases of
        // vm_throw_virtual_machine_error(message). That routine loads
        // class_shared_java_lang_VirtualMachineError from its GOT slot and
        // forwards the incoming r0 as the optional message.
        InitSvcId::VmThrowAbstractMethodError | InitSvcId::VmThrowNoSuchMethodError => {
            let message = core.read_param(0)?;
            let class_name = "java/lang/VirtualMachineError";
            let vtable = synthetic_platform_vtable(core, context, class_name)?;

            context.java_handles.set_dispatch_table(class_name, vtable);

            let mut exception = match context.jvm.instantiate_class(class_name).await {
                Ok(exception) => exception,
                Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
            };

            if message != 0 {
                let message_bytes = read_null_terminated_string_bytes(core, message)?;
                let message: String = message_bytes.iter().map(|&byte| char::from(byte)).collect();
                let java_message = match JavaLangString::from_rust_string(&context.jvm, &message).await {
                    Ok(message) => message,
                    Err(error) => return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await),
                };

                if let Err(error) = context
                    .jvm
                    .put_field(
                        &mut exception,
                        "detailMessage",
                        "Ljava/lang/String;",
                        java_message,
                    )
                    .await
                {
                    return Err(wie_jvm_support::JvmSupport::to_wie_err(&context.jvm, error).await);
                }
            }

            let exception = context.java_handles.address_of(exception)?;

            tracing::debug!(
                "vm_throw_virtual_machine_error(import={:#x}, message={message:#x}) -> longjmp({exception:#x})",
                id.0
            );
            context.save_points.throw(core, exception)
        }
        // Native vm_find_interface(object, requested_class_shared) walks the
        // receiver class's linked interface list and returns the matching
        // interface dispatch-table pointer, or zero when it is not implemented.
        InitSvcId::VmFindInterface => {
            let object = core.read_param(0)?;
            let requested_root = core.read_param(1)?;

            let Some(instance) = context.java_handles.get(object) else {
                0u32.write(core, lr)?;
                return Ok(());
            };
            let receiver_name = instance.class_definition().name();

            let requested = {
                let imported = context.imported_classes.lock();
                imported.as_ref().and_then(|table| {
                    let index = table.class_of_root(requested_root)?;
                    let class = table.classes.get(index as usize)?;
                    let dispatch = table.interface_vtables.get(index as usize).copied().unwrap_or(0);
                    Some((class.name.clone(), dispatch))
                })
            };

            let Some((requested_name, dispatch)) = requested else {
                0u32.write(core, lr)?;
                return Ok(());
            };

            let implements = class_implements_interface(context, &receiver_name, &requested_name);

            if dispatch == 0 || !implements {
                tracing::trace!(
                    "vm_find_interface(object={object:#x}, receiver={receiver_name}, root={requested_root:#x}, interface={requested_name}) -> 0"
                );
                0u32.write(core, lr)?;
                return Ok(());
            }

            tracing::trace!(
                "vm_find_interface(object={object:#x}, receiver={receiver_name}, root={requested_root:#x}, interface={requested_name}) -> {dispatch:#x}"
            );
            dispatch.write(core, lr)
        }
        InitSvcId::VmGetStringClass => {
            let class = imported_class_token(context, "java/lang/String")
                .ok_or_else(|| WieError::FatalError("java/lang/String has no imported class token".into()))?;

            tracing::debug!("vm_get_string_class() -> {class:#x}");
            class.write(core, lr)
        }
        InitSvcId::VmGetStringArrayClass => {
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
/// The application fills in the slots it has code for and leaves inherited
/// slots zero. Native `vm_link_class_light` fills zero slots 1..N-1 from the
/// superclass's same slot; this runtime mirrors that before falling back to a
/// reporting stub. Slot zero remains runtime-owned.
///
/// Heavy-linked classes synthesize a variable-length table; light-linked
/// classes with no table continue to inherit their superclass/fallback table.
///
/// Native slot count of the immediate superclass used as the starting point
/// when `vm_link_class_heavy` appends newly declared virtual methods.
fn superclass_dispatch_slot_count(
    core: &ArmCore,
    context: &InitSvcContext,
    root: u32,
) -> Result<u32> {
    let classes = context.app_classes.lock();
    let superclass = classes
        .iter()
        .find(|class| class.root == root)
        .and_then(|class| class.superclass.clone());

    let Some(superclass) = superclass else {
        return Ok(0);
    };

    if let Some(class) = classes.iter().find(|class| class.name == superclass) {
        let metadata: u32 = read_generic(core, class.root + 8)?;
        let slots: u16 = read_generic(core, metadata + CLASS_DISPATCH_SLOTS)?;
        return Ok(u32::from(slots));
    }

    Ok(platform_class(&superclass)
        .map(|class| class.dispatch.len() as u32)
        .unwrap_or(0))
}

/// Ensures native heavy-link method slots have been assigned exactly once.
///
/// `vm_resolve_one` receives an already-linked class_shared root and immediately
/// reads its method rows. Some applications resolve virtual imports before the
/// class is activated, so slot linking cannot be deferred until vtable creation.
fn ensure_heavy_method_slots_linked(
    core: &mut ArmCore,
    context: &InitSvcContext,
    root: u32,
) -> Result<()> {
    if context.heavy_linked_classes.lock().contains(&root) {
        return Ok(());
    }

    let metadata: u32 = read_generic(core, root + 8)?;
    if metadata == 0 {
        return Ok(());
    }

    let linker_flags: u16 = read_generic(core, metadata + 0x24)?;
    let vtable: u32 = read_generic(core, metadata + CLASS_DISPATCH_TABLE)?;

    // Native vm_link_class_light reads its linker flags from metadata+0x24.
    // The proven heavy-link shape used by LoM f/n has bit 0x0100 set there
    // (f=0x0707, n=0x0703) and no prebuilt dispatch table.
    // Leave prebuilt/light classes untouched.
    if linker_flags & 0x0100 == 0 || vtable != 0 {
        return Ok(());
    }

    // The application's main Jlet can live outside the table passed to
    // vm_register_classes. Legend of Master's `Lm` is such a class: native
    // vm_resolve_one still receives its real class_shared root and heavy-links
    // it normally. Cache that parsed root on first use so the same linker path
    // applies to registered and main-only application classes.
    if !context.app_classes.lock().iter().any(|class| class.root == root) {
        let class = app_classes::parse_class_root(core, root)?;
        context.app_classes.lock().push(class);
    }

    let snapshot = context.app_classes.lock().clone();
    let index = snapshot
        .iter()
        .position(|class| class.root == root)
        .expect("heavy-linked application class was just cached");

    let mut class = snapshot[index].clone();
    let superclass_slots = superclass_dispatch_slot_count(core, context, root)?;
    let slots = assign_heavy_method_slots(core, &mut class, &snapshot, superclass_slots)?;
    let slots_u16 = u16::try_from(slots).map_err(|_| {
        WieError::FatalError(format!(
            "Heavy-linked LGT class {} requires {slots} dispatch slots",
            class.name
        ))
    })?;

    write_generic(core, metadata + CLASS_DISPATCH_SLOTS, slots_u16)?;

    if let Some(cached) = context
        .app_classes
        .lock()
        .iter_mut()
        .find(|cached| cached.root == root)
    {
        *cached = class;
    }

    context.heavy_linked_classes.lock().insert(root);
    Ok(())
}

/// Synthesizes the variable-length instance dispatch table produced by native
/// `vm_link_class_heavy`.
///
/// Heavy AOT classes such as LoM `f` and `n` ship a null vtable pointer and
/// positive method-slot link markers. Native linking rewrites those markers to
/// inherited/new final slots, updates metadata+0x26, allocates exactly that many
/// dispatch slots, copies the superclass dispatch, then installs every method
/// whose linked signed slot is non-negative.
fn activate_heavy_dispatch_table(
    core: &mut ArmCore,
    context: &InitSvcContext,
    root: u32,
) -> Result<u32> {
    ensure_heavy_method_slots_linked(core, context, root)?;

    let class = context
        .app_classes
        .lock()
        .iter()
        .find(|class| class.root == root)
        .cloned()
        .ok_or_else(|| {
            WieError::FatalError(format!(
                "Heavy-linked LGT class at {root:#x} was never registered"
            ))
        })?;

    let superclass_slots = superclass_dispatch_slot_count(core, context, root)?;
    let metadata: u32 = read_generic(core, root + 8)?;
    let slots: u16 = read_generic(core, metadata + CLASS_DISPATCH_SLOTS)?;
    let slots = u32::from(slots);

    let bytes = slots
        .checked_add(1)
        .and_then(|words| words.checked_mul(4))
        .ok_or_else(|| WieError::FatalError("Heavy LGT dispatch-table size overflow".into()))?;

    let installed = Allocator::alloc(core, bytes)?;
    core.write_bytes(installed, &vec![0; bytes as usize])?;
    write_generic(core, installed, root)?;

    let fallback = context.java_handles.fallback_dispatch_table();

    // Native copies superclass dispatch slots 1..N-1 only:
    // vm_link_class_heavy memcpy's from superclass_vtable+8 to new_vtable+8
    // for (superclass_slots - 1) words. Slot zero remains class/runtime-owned.
    for slot in 1..superclass_slots {
        let entry = inherited_dispatch_entry(core, context, root, slot, fallback)?;
        write_generic(core, installed + 4 + slot * 4, entry)?;
    }

    // Native's fill loop accepts every non-negative linked method slot.
    for member in class.methods() {
        let slot = member.slot() as u16 as i16;
        if slot < 0 {
            continue;
        }

        let slot = slot as u32;
        if slot >= slots {
            return Err(WieError::FatalError(format!(
                "Heavy-linked method {}.{}{} has slot {slot} outside {slots}",
                class.name,
                member.name(),
                member.descriptor()
            )));
        }

        let entry = member.entry().ok_or_else(|| {
            WieError::FatalError(format!(
                "Heavy-linked member {}.{}{} has no entry",
                class.name,
                member.name(),
                member.descriptor()
            ))
        })?;

        write_generic(core, installed + 4 + slot * 4, entry)?;
    }

    // Native publishes the newly allocated table through metadata+0x0c.
    write_generic(core, metadata + CLASS_DISPATCH_TABLE, installed)?;

    Ok(installed)
}

fn activate_dispatch_table(core: &mut ArmCore, context: &InitSvcContext, root: u32) -> Result<u32> {
    let fallback = context.java_handles.fallback_dispatch_table();

    let metadata: u32 = read_generic(core, root + 8)?;
    if metadata == 0 {
        return Ok(fallback);
    }

    let linker_flags: u16 = read_generic(core, metadata + 0x24)?;
    let vtable: u32 = read_generic(core, metadata + CLASS_DISPATCH_TABLE)?;
    let slots: u16 = read_generic(core, metadata + CLASS_DISPATCH_SLOTS)?;

    // Native linker flags live at metadata+0x24. vm_link_class_heavy handles
    // classes whose linker flag 0x0100 is
    // set. LoM's f/n are the proven case: they carry no prebuilt vtable and
    // require a newly synthesized variable-length table.
    if linker_flags & 0x0100 != 0 && vtable == 0 {
        return activate_heavy_dispatch_table(core, context, root);
    }

    // A light-linked class with no table of its own inherits the nearest
    // available superclass dispatch table wholesale.
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
        } else if slot != 0 && slot < slots {
            inherited_dispatch_entry(core, context, root, slot, fallback)?
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
fn class_implements_interface(context: &InitSvcContext, class_name: &str, interface_name: &str) -> bool {
    let mut current = Some(String::from(class_name));

    for _ in 0..MAX_SUPERCLASS_DEPTH {
        let Some(name) = current.take() else {
            return false;
        };

        if name == interface_name {
            return true;
        }

        if let Some(class) = context.app_classes.lock().iter().find(|class| class.name == name).cloned() {
            if class.interfaces.iter().any(|interface| interface == interface_name) {
                return true;
            }

            current = class.superclass;
            continue;
        }

        let Some(class) = platform_class(&name) else {
            return false;
        };

        if class.interfaces.iter().any(|interface| *interface == interface_name) {
            return true;
        }

        current = class.superclass.map(String::from);
    }

    false
}

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

/// Resolves one zero application dispatch slot the same way native
/// `vm_link_class_light` does: walk the superclass chain and inherit the
/// first implementation of the same slot. Application superclass tables may
/// themselves contain zero placeholders, so keep walking until an
/// implementation or a platform class is reached.
fn inherited_dispatch_entry(
    core: &ArmCore,
    context: &InitSvcContext,
    root: u32,
    slot: u32,
    fallback: u32,
) -> Result<u32> {
    let app_classes = context.app_classes.lock();
    let imported_classes = context.imported_classes.lock();

    inherited_dispatch_entry_from_tables(
        core,
        &app_classes,
        imported_classes.as_ref(),
        root,
        slot,
        fallback,
    )
}

fn inherited_dispatch_entry_from_tables(
    core: &ArmCore,
    app_classes: &[AppClass],
    imported_classes: Option<&ClassTable>,
    root: u32,
    slot: u32,
    fallback: u32,
) -> Result<u32> {
    let mut superclass = app_classes.iter().find(|x| x.root == root).and_then(|x| x.superclass.clone());

    for _ in 0..MAX_SUPERCLASS_DEPTH {
        let Some(name) = superclass else { break };

        if let Some(class) = app_classes.iter().find(|x| x.name == name) {
            let metadata: u32 = read_generic(core, class.root + 8)?;
            if metadata != 0 {
                let vtable: u32 = read_generic(core, metadata + CLASS_DISPATCH_TABLE)?;
                let slots: u16 = read_generic(core, metadata + CLASS_DISPATCH_SLOTS)?;

                if vtable != 0 && slot < u32::from(slots) {
                    let entry: u32 = read_generic(core, vtable + 4 + slot * 4)?;
                    if entry != 0 {
                        return Ok(entry);
                    }
                }
            }

            superclass = class.superclass.clone();
            continue;
        }

        let platform_vtable = imported_classes.and_then(|table| {
            let index = table.classes.iter().position(|x| x.name == name)?;
            table.vtables.get(index).copied()
        });

        if let Some(vtable) = platform_vtable {
            return read_generic(core, vtable + 4 + slot * 4);
        }

        break;
    }

    read_generic(core, fallback + 4 + slot * 4)
}

/// Finds the native virtual slot a superclass already owns for `name+descriptor`.
///
/// Native `vm_link_class_heavy` walks application superclasses first, comparing
/// both strings, then continues into the platform hierarchy. Only positive
/// signed 16-bit method slots participate in virtual dispatch.
fn superclass_virtual_slot(
    app_classes: &[AppClass],
    superclass: Option<&str>,
    name: &str,
    descriptor: &str,
) -> Option<u32> {
    let mut current = superclass.map(String::from);

    for _ in 0..MAX_SUPERCLASS_DEPTH {
        let class_name = current.take()?;

        if let Some(class) = app_classes.iter().find(|class| class.name == class_name) {
            if let Some(member) = class.members.iter().find(|member| {
                member.is_method()
                    && member.name() == name
                    && member.descriptor() == descriptor
                    && (member.slot() as u16 as i16) > 0
            }) {
                return Some(member.slot());
            }

            current = class.superclass.clone();
            continue;
        }

        return platform_class(&class_name)
            .and_then(|class| class.virtual_method(name, descriptor))
            .map(|method| method.slot);
    }

    None
}

/// Assigns final virtual slots to one native heavy-linked application class.
///
/// The incoming positive slot is a link marker, not the final slot. An override
/// reuses the superclass method's slot; otherwise the method is appended after
/// the superclass's final dispatch slot. This is the rule used by native
/// `vm_link_class_heavy`.
fn assign_heavy_method_slots(
    core: &mut ArmCore,
    class: &mut AppClass,
    app_classes: &[AppClass],
    superclass_slots: u32,
) -> Result<u32> {
    let superclass = class.superclass.clone();
    let mut next_slot = superclass_slots;

    for member in &mut class.members {
        if !member.is_method() || (member.slot() as u16 as i16) <= 0 {
            continue;
        }

        let linked_slot = superclass_virtual_slot(
            app_classes,
            superclass.as_deref(),
            member.name(),
            member.descriptor(),
        )
        .unwrap_or_else(|| {
            let slot = next_slot;
            next_slot += 1;
            slot
        });

        let row = member
            .method_row()
            .ok_or_else(|| WieError::FatalError("Heavy-linked method has no native row".into()))?;

        // Native vm_link_class_heavy rewrites the signed 16-bit slot in the
        // method row itself. vm_resolve_one subsequently reads that linked row.
        if row != 0 {
            write_generic(core, row + 0x10, linked_slot as u16)?;
        }

        assert!(member.set_method_slot(linked_slot));
    }

    Ok(next_slot)
}

fn primitive_array_descriptor(atype: u32) -> Option<char> {
    Some(match atype {
        4 => 'Z',
        5 => 'C',
        6 => 'F',
        7 => 'D',
        8 => 'B',
        9 => 'S',
        10 => 'I',
        11 => 'J',
        _ => return None,
    })
}

fn synthetic_platform_vtable(core: &mut ArmCore, context: &InitSvcContext, class_name: &str) -> Result<u32> {
    if let Some((&root, _)) = context
        .synthetic_classes
        .lock()
        .iter()
        .find(|(_, name)| name.as_str() == class_name)
    {
        let vtable = context
            .java_handles
            .dispatch_table(class_name)
            .ok_or_else(|| WieError::FatalError(format!("Synthetic class {class_name} root {root:#x} has no dispatch table")))?;
        return Ok(vtable);
    }

    let base_vtable = {
        let classes = context.imported_classes.lock();
        let table = classes
            .as_ref()
            .ok_or_else(|| WieError::FatalError("Platform classes are not loaded".into()))?;
        let index = table
            .classes
            .iter()
            .position(|class| class.name == "java/lang/Exception")
            .ok_or_else(|| WieError::FatalError("java/lang/Exception is not imported".into()))?;

        *table
            .vtables
            .get(index)
            .ok_or_else(|| WieError::FatalError("java/lang/Exception has no dispatch table".into()))?
    };

    // Preserve java/lang/Exception's callable slots, changing only vtable word
    // zero to the VM-created subclass identity.
    let root = Allocator::alloc(core, 4)?;
    write_generic(core, root, 0u32)?;

    let size = 4 + DISPATCH_TABLE_SLOTS * 4;
    let vtable = Allocator::alloc(core, size)?;
    for offset in (0..size).step_by(4) {
        let value: u32 = read_generic(core, base_vtable + offset)?;
        write_generic(core, vtable + offset, value)?;
    }
    write_generic(core, vtable, root)?;

    context.synthetic_classes.lock().insert(root, class_name.into());
    context.java_handles.set_dispatch_table(class_name, vtable);

    Ok(vtable)
}

fn class_identity_name(context: &InitSvcContext, root: u32) -> Option<String> {
    if root == 0 {
        return None;
    }

    if let Some(name) = context
        .app_classes
        .lock()
        .iter()
        .find(|class| class.root == root)
        .map(|class| class.name.clone())
    {
        return Some(name);
    }

    if let Some(name) = context.imported_classes.lock().as_ref().and_then(|table| {
        let index = table.class_of_root(root)?;
        table.classes.get(index as usize).map(|class| class.name.clone())
    }) {
        return Some(name);
    }

    if let Some(name) = context.synthetic_classes.lock().get(&root).cloned() {
        return Some(name);
    }

    let array = context.array_classes.lock().get(&root).copied()?;

    let prefix = "[".repeat(array.dimensions as usize);

    if array.element_class != 0 {
        let element = class_identity_name(context, array.element_class)?;

        if element.starts_with('[') {
            Some(format!("{prefix}{element}"))
        } else {
            Some(format!("{prefix}L{element};"))
        }
    } else {
        primitive_array_descriptor(array.atype).map(|descriptor| format!("{prefix}{descriptor}"))
    }
}

/// The actual class whose hierarchy RustJava must know before an assignability
/// test. Array identities use JVM descriptors, so peel every `[` and the
/// object-descriptor `L...;` wrapper. Primitive array components need no class
/// registration.
fn class_identity_bridge_name(name: &str) -> Option<&str> {
    let mut name = name;

    while let Some(component) = name.strip_prefix('[') {
        name = component;
    }

    if let Some(class) = name.strip_prefix('L').and_then(|name| name.strip_suffix(';')) {
        return Some(class);
    }

    match name {
        "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D" => None,
        _ => Some(name),
    }
}

fn object_class_root(core: &ArmCore, object: u32) -> Result<u32> {
    let vtable: u32 = read_generic(core, object)?;
    if vtable == 0 {
        return Err(WieError::FatalError(format!(
            "guest object {object:#x} has no dispatch table"
        )));
    }

    read_generic(core, vtable)
}

/// Returns the native component class used by `vm_aastore_impl`.
///
/// A one-dimensional reference array stores instances of `element_class`.
/// A multidimensional array stores arrays of one dimension less.
fn array_component_class(array_classes: &ArrayClasses, array_class: u32) -> Result<u32> {
    let info = array_classes
        .lock()
        .get(&array_class)
        .copied()
        .ok_or_else(|| {
            WieError::FatalError(format!(
                "reference array class {array_class:#x} has no array metadata"
            ))
        })?;

    if info.dimensions <= 1 {
        if info.element_class == 0 {
            return Err(WieError::FatalError(format!(
                "aastore used with primitive array class {array_class:#x}"
            )));
        }
        return Ok(info.element_class);
    }

    array_classes
        .lock()
        .iter()
        .find(|(_, candidate)| {
            candidate.dimensions == info.dimensions - 1
                && candidate.element_class == info.element_class
                && candidate.atype == info.atype
        })
        .map(|(class, _)| *class)
        .ok_or_else(|| {
            WieError::FatalError(format!(
                "array class {array_class:#x} has no {}-dimension component class",
                info.dimensions - 1
            ))
        })
}

async fn throw_vm_exception(
    core: &mut ArmCore,
    context: &mut InitSvcContext,
    class_name: &str,
) -> Result<()> {
    let vtable = synthetic_platform_vtable(core, context, class_name)?;
    context.java_handles.set_dispatch_table(class_name, vtable);

    let exception = match context.jvm.instantiate_class(class_name).await {
        Ok(exception) => exception,
        Err(error) => {
            return Err(wie_jvm_support::JvmSupport::to_wie_err(
                &context.jvm,
                error,
            )
            .await);
        }
    };
    let exception = context.java_handles.address_of(exception)?;

    context.save_points.throw(core, exception)
}

async fn class_is_assignable_to(core: &ArmCore, context: &mut InitSvcContext, source_root: u32, target_root: u32) -> Result<bool> {
    if source_root == target_root {
        return Ok(true);
    }

    let source = class_identity_name(context, source_root).ok_or_else(|| {
        WieError::FatalError(format!(
            "vm_class_is_assignable_to source {source_root:#x} names no application, platform, or array class"
        ))
    })?;
    let target = class_identity_name(context, target_root).ok_or_else(|| {
        WieError::FatalError(format!(
            "vm_class_is_assignable_to target {target_root:#x} names no application, platform, or array class"
        ))
    })?;

    // Application classes only exist in the AOT image until they are bridged
    // into RustJava. For an array, bridge its reference component rather than
    // the array descriptor itself: RustJava's array assignability recurses into
    // that component class.
    for identity in [&source, &target] {
        let Some(class_name) = class_identity_bridge_name(identity) else {
            continue;
        };

        bridge_class_chain(
            &context.jvm,
            core,
            &context.java_handles,
            &context.app_classes,
            &context.image_ranges,
            class_name,
        )
        .await;
    }

    Ok(context.jvm.is_type_assignable(
        &JavaType::from_class_name(&source),
        &JavaType::from_class_name(&target),
    ))
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

    // The token itself is the synthetic array class_shared identity.
    let class = Allocator::alloc(core, 8)?;
    write_generic(core, class, element_size)?;
    write_generic(core, class + 4, dimensions)?;

    // Arrays still need the fallback method entries, but their dispatch-table
    // word zero must identify their own array class for instanceof/aastore and
    // other native class tests.
    let vtable_size = (DISPATCH_TABLE_SLOTS + 1) * 4;
    let fallback = context.java_handles.fallback_dispatch_table();
    let vtable = Allocator::alloc(core, vtable_size)?;
    let mut dispatch = vec![0u8; vtable_size as usize];
    core.read_bytes(fallback, &mut dispatch)?;
    core.write_bytes(vtable, &dispatch)?;
    write_generic(core, vtable, class)?;

    context.array_classes.lock().insert(
        class,
        ArrayClassInfo {
            dimensions,
            element_class,
            atype,
            element_size,
            vtable,
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

    let class = context
        .app_classes
        .lock()
        .iter()
        .find(|x| x.root == root)
        .map(|x| (x.name.clone(), x.instance_words));

    let Some((class, instance_words)) = class else {
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

    let object = context
        .java_handles
        .allocate_instance_with_fields(vtable, instance_words)?;

    context.java_handles.bind(object, instance);

    tracing::debug!(
        "LGT new {class} at {object:#x}, dispatch table {vtable:#x}, {instance_words} instance words"
    );

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

        let instance_words = platform_class(&class.name)?.instance_words;
        Some((class.name.clone(), *table.vtables.get(index as usize)?, instance_words))
    });

    let Some((name, vtable, instance_words)) = layout else {
        tracing::warn!("LGT vm_instantiate({class_object:#x}) does not name a class");
        return Ok(class_object);
    };

    drop(table);

    let object = context
        .java_handles
        .allocate_instance_with_fields(vtable, instance_words)?;

    match context.jvm.instantiate_class(&name).await {
        Ok(instance) => {
            context.java_handles.bind(object, instance);
            tracing::debug!(
                "LGT new {name} instance at {object:#x}, dispatch table {vtable:#x}, {instance_words} instance words, JVM instance bound"
            );
        }
        Err(error) => {
            tracing::warn!("LGT could not create uninitialized JVM instance for {name} at {object:#x}: {error:?}");
        }
    }

    Ok(object)
}

/// Implements the two class-accessor rows at the head of an imported class's
/// static method block.
///
/// Native `vm_resolve_one` installs `get_class` followed by `get_raw_class`.
/// Both return the class token; `get_class` additionally guarantees that the
/// JVM class has completed initialization.
async fn call_reserved_slot(core: &mut ArmCore, context: &mut InitSvcContext, index: u32) -> Result<u32> {
    let a0 = core.read_param(0)?;

    let resolved = {
        let imported_classes = context.imported_classes.lock();
        let Some(table) = imported_classes.as_ref() else {
            return Ok(a0);
        };
        let Some((class, slot)) = table.static_method_owner(index) else {
            tracing::warn!("LGT reserved static row {index} belongs to no class");
            return Ok(a0);
        };

        let class_object = table
            .classes
            .iter()
            .position(|x| core::ptr::eq(x, class))
            .and_then(|index| table.class_objects.get(index).copied())
            .unwrap_or(a0);

        (class.name.clone(), slot, class_object)
    };

    let (class_name, slot, class_object) = resolved;

    // Native vm_resolve_one fills the leading blank static-method pair with
    // get_class/get_raw_class. Both return the same activated class token;
    // get_class additionally guarantees Java class initialization.
    if slot <= 1 {
        if slot == 0 {
            let class = context.jvm.resolve_class(&class_name).await.map_err(|error| {
                wie_util::WieError::FatalError(alloc::format!(
                    "LGT get_class could not resolve {class_name}: {error:?}"
                ))
            })?;
            context.jvm.ensure_initialized(&class).await.map_err(|error| {
                wie_util::WieError::FatalError(alloc::format!(
                    "LGT get_class could not initialize {class_name}: {error:?}"
                ))
            })?;
        }

        return Ok(class_object);
    }

    tracing::warn!("LGT reserved slot {slot} of {class_name} called with a0={a0:#x}");
    Ok(a0)
}

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

    // Resolve fixed native dispatch slots from the receiver's actual platform
    // class even when the application never imported that class or method.
    if let Some(receiver_class) = context.java_handles.get(this).map(|instance| instance.class_definition().name())
        && let Some((method_name, method_descriptor)) =
            platform_class(&receiver_class).and_then(|class| class.dispatch_method(slot))
    {
        let member = ResolvedMember {
            class_name: receiver_class.clone(),
            name: method_name.into(),
            descriptor: method_descriptor.into(),
        };

        let handles = context.java_handles.clone();
        let jvm = context.jvm.clone();

        return match method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await {
            Ok(result) => Ok(result),
            Err(error) => {
                tracing::warn!(
                    "LGT {}.{}{} at slot {slot} failed: {error}",
                    receiver_class,
                    method_name,
                    method_descriptor
                );
                Ok(0)
            }
        };
    }

    // Native dt_* entries take precedence over java/lang/Object slots when the
    // concrete receiver overrides one. Keep the Object layout only as a
    // fallback for objects whose JVM-side class cannot identify a native dt_*.
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

    if let Some((method_name, method_descriptor)) =
        platform_class(&class).and_then(|platform| platform.dispatch_method(slot))
    {
        let member = ResolvedMember {
            class_name: class.clone(),
            name: method_name.into(),
            descriptor: method_descriptor.into(),
        };

        let handles = context.java_handles.clone();
        let jvm = context.jvm.clone();

        return method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await;
    }

    tracing::warn!("LGT undeclared dispatch slot {slot} of {class} called on {this:#x}");

    Ok(0)
}

/// Handles a call the compiled code made through a class's dispatch table.
async fn invoke_imported_interface(core: &mut ArmCore, context: &mut InitSvcContext, index: u32) -> Result<u32> {
    let this = core.read_param(0)?;

    let member = context
        .imported_classes
        .lock()
        .as_ref()
        .and_then(|table| ResolvedMember::interface_method(table, index));

    let Some(member) = member else {
        return Err(WieError::FatalError(format!("Imported interface method {index} has no descriptor")));
    };

    let handles = context.java_handles.clone();
    let jvm = context.jvm.clone();

    method_bridge::invoke(core, &jvm, &handles, &member, Some(this)).await
}

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

pub async fn load_native(
    core: &mut ArmCore,
    system: &mut System,
    jvm: &Jvm,
    data: &[u8],
    jar_filename: &str,
    _main_class_name: Option<&str>,
) -> Result<()> {
    let (entrypoint, image_ranges) = load_executable(core, data)?;
    let save_points = SavePointState::default();

    // Native dlet_main derives property 200 by removing the 11-byte
    // `:binary.mod` suffix from the loaded module path. WIE already has the
    // exact JAR filename, so publish the equivalent guest C-string directly.
    let jar_filename_ptr = Allocator::alloc(core, jar_filename.len() as u32 + 1)?;
    write_null_terminated_string_bytes(core, jar_filename_ptr, jar_filename.as_bytes())?;

    let dlet_properties: DletProperties = Default::default();
    dlet_properties.lock().insert(200, (jar_filename_ptr, 0));

    register_wipic_svc_handler(core, system, jvm)?;
    register_stdlib_svc_handler(core, system, &save_points)?;
    register_init_svc_handler(
        core,
        system,
        jvm,
        Arc::new(image_ranges),
        &save_points,
        dlet_properties,
    )?;

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
            (0x1f8, 0x16) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::DletSetProperty)?,
            (0x1f8, 0x17) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::DletGetProperty)?,
            (0x1fc, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::WipiJavaModuleActivate)?,
            (0x1ff, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::LgteModuleActivate)?,
            (0x201, 0x03) => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::BankonModuleActivate)?,
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

#[cfg(test)]
mod vm_thread_reschedule_tests {
    use alloc::collections::BTreeMap;

    use spin::Mutex;

    use super::{
        VM_RESCHEDULE_COUNT_THRESHOLD, vm_ensure_reschedule_deadline,
        vm_thread_reschedule_due,
    };

    #[test]
    fn native_reschedule_countdown_checks_after_one_hundred_fast_calls() {
        let count = Mutex::new(VM_RESCHEDULE_COUNT_THRESHOLD);

        for _ in 0..VM_RESCHEDULE_COUNT_THRESHOLD {
            assert!(!vm_thread_reschedule_due(&count));
        }
        assert_eq!(*count.lock(), 0);

        assert!(vm_thread_reschedule_due(&count));
        assert_eq!(*count.lock(), VM_RESCHEDULE_COUNT_THRESHOLD);

        for _ in 0..VM_RESCHEDULE_COUNT_THRESHOLD {
            assert!(!vm_thread_reschedule_due(&count));
        }
        assert_eq!(*count.lock(), 0);

        assert!(vm_thread_reschedule_due(&count));
    }

    #[test]
    fn native_exec_env_deadlines_are_thread_local() {
        let deadlines = Mutex::new(BTreeMap::new());

        assert_eq!(vm_ensure_reschedule_deadline(&deadlines, 1, 1000), 1020);
        assert_eq!(vm_ensure_reschedule_deadline(&deadlines, 2, 2000), 2020);

        // Re-reading an existing exec-env keeps its original deadline.
        assert_eq!(vm_ensure_reschedule_deadline(&deadlines, 1, 9000), 1020);

        let deadlines = deadlines.lock();
        assert_eq!(deadlines.get(&1), Some(&1020));
        assert_eq!(deadlines.get(&2), Some(&2020));
    }
}

#[cfg(test)]
mod dlet_property_tests {
    use wie_core_arm::{Allocator, ArmCore};
    use wie_util::{read_generic, write_generic};

    use super::{DletProperties, dlet_get_process_local_property, dlet_set_process_local_property};

    fn core() -> ArmCore {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        core
    }

    #[test]
    fn scalar_property_round_trip_matches_native_size_zero_contract() {
        let mut core = core();
        let properties: DletProperties = Default::default();
        let output = Allocator::alloc(&mut core, 4).unwrap();

        write_generic(&mut core, output, 0xdead_beefu32).unwrap();

        assert_eq!(
            dlet_set_process_local_property(&properties, 0, 200, 0x1234_5678, 0).unwrap(),
            0
        );
        assert_eq!(
            dlet_get_process_local_property(&mut core, &properties, 0, 200, output).unwrap(),
            0
        );

        let value: u32 = read_generic(&core, output).unwrap();
        assert_eq!(value, 0x1234_5678);
    }

    #[test]
    fn missing_property_returns_native_minus_2002_without_touching_output() {
        let mut core = core();
        let properties: DletProperties = Default::default();
        let output = Allocator::alloc(&mut core, 4).unwrap();

        write_generic(&mut core, output, 0xfeed_faceu32).unwrap();

        assert_eq!(
            dlet_get_process_local_property(&mut core, &properties, 0, 999, output).unwrap(),
            (-2002i32) as u32
        );

        let value: u32 = read_generic(&core, output).unwrap();
        assert_eq!(value, 0xfeed_face);
    }

    #[test]
    fn invalid_process_local_arguments_match_native_failure_shape() {
        let mut core = core();
        let properties: DletProperties = Default::default();
        let output = Allocator::alloc(&mut core, 4).unwrap();

        assert_eq!(
            dlet_get_process_local_property(&mut core, &properties, 1, 200, output).unwrap(),
            u32::MAX
        );
        assert_eq!(
            dlet_get_process_local_property(&mut core, &properties, 0, 200, 0).unwrap(),
            u32::MAX
        );

        assert!(dlet_set_process_local_property(&properties, 1, 200, 1, 0).is_err());
        assert!(dlet_set_process_local_property(&properties, 0, 200, 1, 4).is_err());
    }
}

#[cfg(test)]
mod application_dispatch_tests {
    use alloc::{string::String, vec, vec::Vec};

    use crate::runtime::java::app_classes::AppMember;

    use wie_core_arm::{Allocator, ArmCore};
    use wie_util::{ByteWrite, write_generic};

    use super::{
        AppClass, ArrayClassInfo, ArrayClasses, REFERENCE_SIZE, VmMonitors,
        assign_heavy_method_slots, inherited_dispatch_entry_from_tables,
        vm_monitor_exit_owned, vm_monitor_try_enter,
    };

    fn core() -> ArmCore {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        core
    }

    fn app_class(root: u32, name: &str, superclass: Option<&str>) -> AppClass {
        AppClass {
            root,
            get_class: 0,
            get_raw_class: 0,
            name: String::from(name),
            superclass: superclass.map(String::from),
            interfaces: Vec::new(),
            members: Vec::new(),
            instance_words: 0,
        }
    }

    fn class_image(core: &mut ArmCore, slots: u16, entries: &[(u32, u32)]) -> (u32, u32) {
        let metadata = Allocator::alloc(core, 0x4c).unwrap();
        let root = Allocator::alloc(core, 0x14).unwrap();
        let vtable = Allocator::alloc(core, 4 + u32::from(slots) * 4).unwrap();

        core.write_bytes(metadata, &vec![0; 0x4c]).unwrap();
        core.write_bytes(root, &vec![0; 0x14]).unwrap();
        core.write_bytes(vtable, &vec![0; (4 + u32::from(slots) * 4) as usize]).unwrap();

        write_generic(core, root + 8, metadata).unwrap();
        write_generic(core, metadata + 0x0c, vtable).unwrap();
        write_generic(core, metadata + 0x26, slots).unwrap();
        write_generic(core, vtable, root).unwrap();

        for &(slot, entry) in entries {
            write_generic(core, vtable + 4 + slot * 4, entry).unwrap();
        }

        (root, vtable)
    }

    fn virtual_method(name: &str, descriptor: &str) -> AppMember {
        AppMember::Method {
            name: String::from(name),
            descriptor: String::from(descriptor),
            flags: 0,
            row: 0,
            slot: 1,
            entry: 0x1000,
            argument_words: 1,
        }
    }

    #[test]
    fn object_class_root_follows_native_vtable_word_zero() {
        let mut core = core();
        let class = 0x1234_5678;
        let vtable = Allocator::alloc(&mut core, 8).unwrap();
        let object = Allocator::alloc(&mut core, 12).unwrap();

        write_generic(&mut core, vtable, class).unwrap();
        write_generic(&mut core, object, vtable).unwrap();

        assert_eq!(super::object_class_root(&core, object).unwrap(), class);
    }

    #[test]
    fn aastore_component_class_uses_element_for_one_dimension() {
        let arrays: ArrayClasses = Default::default();
        let element = 0x1111;
        let array = 0x2222;

        arrays.lock().insert(
            array,
            ArrayClassInfo {
                dimensions: 1,
                element_class: element,
                atype: 1,
                element_size: REFERENCE_SIZE,
                vtable: 0,
            },
        );

        assert_eq!(super::array_component_class(&arrays, array).unwrap(), element);
    }

    #[test]
    fn aastore_component_class_uses_lower_array_dimension() {
        let arrays: ArrayClasses = Default::default();
        let element = 0x1111;
        let one_dim = 0x2222;
        let two_dim = 0x3333;

        {
            let mut arrays = arrays.lock();
            arrays.insert(
                one_dim,
                ArrayClassInfo {
                    dimensions: 1,
                    element_class: element,
                    atype: 1,
                    element_size: REFERENCE_SIZE,
                    vtable: 0,
                },
            );
            arrays.insert(
                two_dim,
                ArrayClassInfo {
                    dimensions: 2,
                    element_class: element,
                    atype: 1,
                    element_size: REFERENCE_SIZE,
                    vtable: 0,
                },
            );
        }

        assert_eq!(super::array_component_class(&arrays, two_dim).unwrap(), one_dim);
    }

    #[test]
    fn vm_monitor_is_reentrant_and_releases_at_zero_depth() {
        let monitors: VmMonitors = Default::default();
        let object = 0x1234;
        let owner = 7;

        assert!(vm_monitor_try_enter(&monitors, object, owner));
        assert!(vm_monitor_try_enter(&monitors, object, owner));
        assert_eq!(monitors.lock().get(&object).copied(), Some((owner, 2)));

        assert!(vm_monitor_exit_owned(&monitors, object, owner));
        assert_eq!(monitors.lock().get(&object).copied(), Some((owner, 1)));

        assert!(vm_monitor_exit_owned(&monitors, object, owner));
        assert!(!monitors.lock().contains_key(&object));
    }

    #[test]
    fn vm_monitor_rejects_other_owner_until_release() {
        let monitors: VmMonitors = Default::default();
        let object = 0x5678;

        assert!(vm_monitor_try_enter(&monitors, object, 11));
        assert!(!vm_monitor_try_enter(&monitors, object, 12));
        assert!(!vm_monitor_exit_owned(&monitors, object, 12));
        assert_eq!(monitors.lock().get(&object).copied(), Some((11, 1)));

        assert!(vm_monitor_exit_owned(&monitors, object, 11));
        assert!(vm_monitor_try_enter(&monitors, object, 12));
        assert_eq!(monitors.lock().get(&object).copied(), Some((12, 1)));
    }

    #[test]
    fn heavy_slots_reuse_platform_override_and_append_new_method() {
        let mut class = app_class(0x1000, "f", Some("org/kwis/msp/lcdui/Card"));
        class.members = vec![
            virtual_method("keyNotify", "(II)Z"),
            virtual_method("ownMethod", "()V"),
        ];

        let mut core = core();
        let slots = assign_heavy_method_slots(&mut core, &mut class, &[], 25).unwrap();

        assert_eq!(slots, 26);
        assert_eq!(class.members[0].slot(), 17);
        assert_eq!(class.members[1].slot(), 25);
    }

    #[test]
    fn lom_f_heavy_slots_match_native_linker_layout() {
        let mut class = app_class(0x1000, "f", Some("org/kwis/msp/lcdui/Card"));

        // LoM f has 371 positive-slot methods after its constructor. Native
        // reuses Card.paint/keyNotify and appends every other method in row
        // order starting at slot 25.
        class.members = (1..=371)
            .map(|row| match row {
                56 => virtual_method("a", "(II)V"),
                224 => virtual_method("paint", "(Lorg/kwis/msp/lcdui/Graphics;)V"),
                227 => virtual_method("keyNotify", "(II)Z"),
                231 => virtual_method("a", "()V"),
                232 => virtual_method("b", "(II)V"),
                370 => virtual_method("b", "()V"),
                371 => virtual_method("c", "()V"),
                _ => virtual_method(&alloc::format!("m{row}"), "()V"),
            })
            .collect();

        let mut core = core();
        let slots = assign_heavy_method_slots(&mut core, &mut class, &[], 25).unwrap();

        assert_eq!(slots, 394);

        let slot = |name: &str, descriptor: &str| {
            class
                .members
                .iter()
                .find(|member| member.name() == name && member.descriptor() == descriptor)
                .unwrap()
                .slot()
        };

        assert_eq!(slot("paint", "(Lorg/kwis/msp/lcdui/Graphics;)V"), 19);
        assert_eq!(slot("keyNotify", "(II)Z"), 17);

        // Exact LoM java_resolve_one virtual rows 38..43:
        assert_eq!(slot("c", "()V"), 393);
        assert_eq!(slot("b", "()V"), 392);
        assert_eq!(slot("keyNotify", "(II)Z"), 17);
        assert_eq!(slot("b", "(II)V"), 254);
        assert_eq!(slot("a", "()V"), 253);
        assert_eq!(slot("a", "(II)V"), 80);
    }

    #[test]
    fn lom_n_heavy_slots_match_native_linker_layout() {
        let mut class = app_class(
            0x2000,
            "n",
            Some("org/kwis/msp/lwc/TextBoxComponent"),
        );

        class.members = vec![
            virtual_method("keyNotify", "(II)Z"),
            virtual_method("a", "(Ljava/lang/String;)I"),
        ];

        let mut core = core();
        let slots = assign_heavy_method_slots(&mut core, &mut class, &[], 65).unwrap();

        assert_eq!(slots, 66);
        assert_eq!(class.members[0].slot(), 32);
        assert_eq!(class.members[1].slot(), 65);

        // Exact LoM java_resolve_one virtual row 46.
        assert_eq!(
            class
                .members
                .iter()
                .find(|member| {
                    member.name() == "keyNotify" && member.descriptor() == "(II)Z"
                })
                .unwrap()
                .slot(),
            32
        );
    }

    #[test]
    fn lom_lm_heavy_slots_match_native_linker_layout() {
        let mut class = app_class(0x140004c, "Lm", Some("org/kwis/msp/lcdui/Jlet"));
        class.members = vec![
            virtual_method("startApp", "([Ljava/lang/String;)V"),
            virtual_method("pauseApp", "()V"),
            virtual_method("resumeApp", "()V"),
            virtual_method("destroyApp", "(Z)V"),
            virtual_method("a", "()V"),
        ];

        let mut core = core();
        let slots = assign_heavy_method_slots(&mut core, &mut class, &[], 22).unwrap();

        assert_eq!(slots, 23);

        let slot = |name: &str, descriptor: &str| {
            class
                .members
                .iter()
                .find(|member| member.name() == name && member.descriptor() == descriptor)
                .unwrap()
                .slot()
        };

        assert_eq!(slot("startApp", "([Ljava/lang/String;)V"), 11);
        assert_eq!(slot("pauseApp", "()V"), 12);
        assert_eq!(slot("resumeApp", "()V"), 13);
        assert_eq!(slot("destroyApp", "(Z)V"), 14);

        // Exact LoM java_resolve_one virtual rows 44..45.
        assert_eq!(slot("a", "()V"), 22);
        assert_eq!(slot("destroyApp", "(Z)V"), 14);
    }

    #[test]
    fn heavy_slots_are_written_back_to_native_method_rows() {
        let mut core = core();
        let row = Allocator::alloc(&mut core, 0x1c).unwrap();
        core.write_bytes(row, &[0; 0x1c]).unwrap();
        write_generic(&mut core, row + 0x10, 1u16).unwrap();

        let mut class = app_class(0x3000, "f", Some("org/kwis/msp/lcdui/Card"));
        class.members = vec![AppMember::Method {
            name: String::from("keyNotify"),
            descriptor: String::from("(II)Z"),
            flags: 0,
            row,
            slot: 1,
            entry: 0x1234,
            argument_words: 3,
        }];

        let slots = assign_heavy_method_slots(&mut core, &mut class, &[], 25).unwrap();

        assert_eq!(slots, 25);
        assert_eq!(class.members[0].slot(), 17);
        let native_slot: u16 = wie_util::read_generic(&core, row + 0x10).unwrap();
        assert_eq!(native_slot, 17);
    }

    #[test]
    fn heavy_slots_use_text_box_component_native_slot() {
        let mut class = app_class(0x2000, "n", Some("org/kwis/msp/lwc/TextBoxComponent"));
        class.members = vec![
            virtual_method("keyNotify", "(II)Z"),
            virtual_method("a", "(Ljava/lang/String;)I"),
        ];

        let mut core = core();
        let slots = assign_heavy_method_slots(&mut core, &mut class, &[], 65).unwrap();

        assert_eq!(slots, 66);
        assert_eq!(class.members[0].slot(), 32);
        assert_eq!(class.members[1].slot(), 65);
    }

    #[test]
    fn zero_dispatch_slot_walks_application_superclass_chain() {
        let mut core = core();

        let (j_root, _) = class_image(&mut core, 12, &[(10, 0x000e_4234)]);
        let (c_root, _) = class_image(&mut core, 12, &[(11, 0x0000_b18c)]);
        let (child_root, _) = class_image(&mut core, 12, &[]);

        let fallback = Allocator::alloc(&mut core, 4 + 12 * 4).unwrap();
        core.write_bytes(fallback, &vec![0; 4 + 12 * 4]).unwrap();
        write_generic(&mut core, fallback + 4 + 10 * 4, 0xdead_beefu32).unwrap();

        let classes = vec![
            app_class(j_root, "j", Some("java/lang/Object")),
            app_class(c_root, "c", Some("j")),
            app_class(child_root, "child", Some("c")),
        ];

        assert_eq!(
            inherited_dispatch_entry_from_tables(&core, &classes, None, child_root, 10, fallback).unwrap(),
            0x000e_4234
        );
        assert_eq!(
            inherited_dispatch_entry_from_tables(&core, &classes, None, child_root, 11, fallback).unwrap(),
            0x0000_b18c
        );
    }
}

#[cfg(test)]
mod slot12_class_identity_tests {
    use super::class_identity_bridge_name;

    #[test]
    fn bridge_name_keeps_plain_classes() {
        assert_eq!(class_identity_bridge_name("f"), Some("f"));
        assert_eq!(
            class_identity_bridge_name("java/lang/String"),
            Some("java/lang/String")
        );
    }

    #[test]
    fn bridge_name_extracts_reference_array_component() {
        assert_eq!(
            class_identity_bridge_name("[Ljava/lang/String;"),
            Some("java/lang/String")
        );
        assert_eq!(class_identity_bridge_name("[[Lf;"), Some("f"));
    }

    #[test]
    fn bridge_name_skips_primitive_array_component() {
        assert_eq!(class_identity_bridge_name("[I"), None);
        assert_eq!(class_identity_bridge_name("[[B"), None);
    }
}
