use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};

use spin::Mutex;

use jvm::{Jvm, Result as JvmResult, runtime::JavaLangString};
use wie_core_arm::{Allocator, ArmCore};
use wie_jvm_support::JvmSupport;
use wie_util::{ByteRead, ByteWrite, Result, read_generic, read_null_terminated_string_bytes, write_generic};

use crate::runtime::{
    SVC_CATEGORY_INIT,
    java::{
        app_classes::{self, AppClass},
        class_table::{ClassTable, OutputArrays},
        compiled_class::{self, CompiledContext},
        handles::JavaHandles,
        platform_metadata::platform_class,
    },
    svc_ids::InitSvcId,
};

/// Guard on the argument vector the application passes to `Main.main`.
const MAX_MAIN_ARGUMENTS: u32 = 16;

/// Guard on how far an application class hierarchy is followed.
const MAX_CLASS_DEPTH: usize = 32;

/// Diagnostic SVC range used for unresolved LGT Java-interface imports.
/// The low 12 bits preserve the original function index.
pub const JAVA_DIAG_SVC_BASE: u32 = 0x1000;

/// One SVC id per row of the application's static method table. The low bits
/// carry the row index, which is how the handler knows what was called.
pub const JAVA_STATIC_METHOD_SVC_BASE: u32 = 0x2000;

/// One SVC id per row of the virtual method table. The stub sits in a class's
/// dispatch table, so the receiver arrives in the first word.
pub const JAVA_VIRTUAL_METHOD_SVC_BASE: u32 = 0x6000;

/// Maximum native platform dispatch-table slot count extracted from
/// `liblgt_system.so`. The largest `dt_*` contains slots 0 through 102.
pub const DISPATCH_TABLE_SLOTS: u32 = 103;

/// Classes that can have their own reserved block of unknown-slot ids. One
/// past the last is the fallback table's index.
pub const MAX_DISPATCH_CLASSES: u32 = 63;

/// One SVC id per (class, slot) pair a dispatch table does not account for.
pub const JAVA_UNKNOWN_SLOT_SVC_BASE: u32 = 0x8000;

/// One SVC id per row of the application's interface-method table. Interface
/// dispatch uses a table returned by vm_find_interface rather than the
/// receiver's ordinary virtual table, so these rows need their own namespace.
pub const JAVA_INTERFACE_METHOD_SVC_BASE: u32 = 0xa000;

/// One SVC id per row of the static method table that carries no descriptor.
/// The compiled code calls these too, so they cannot be left null.
pub const JAVA_RESERVED_SLOT_SVC_BASE: u32 = 0x4000;

/// Upper bound on rows, so a table that fails to parse cannot run off the end
/// of its SVC range.
pub const JAVA_METHOD_SVC_LIMIT: u32 = 0x2000;

pub fn get_java_interface_method(core: &mut ArmCore, function_index: u32) -> Result<u32> {
    // Table-0x64 is the CLDC module. The index is the module's own export order,
    // recovered from the reference firmware's `cldc` export table in
    // liblgt_system.so (each entry is [index, function, name]). This is the
    // authoritative mapping; earlier per-slot guesses had many wrong.
    let id = match function_index {
        0x03 => InitSvcId::CldcModuleActivate,
        0x04 => InitSvcId::VmRegisterClasses,
        0x06 => InitSvcId::VmUnregisterClasses,
        // vm_register_classes_on_process registers the same class tables as
        // vm_register_classes; the process scoping is not modelled here.
        0x07 => InitSvcId::VmRegisterClasses,
        0x09 => InitSvcId::VmGetConstantString,
        0x0b => InitSvcId::VmInitializeClassShared,
        0x0c => InitSvcId::VmActivateClass,
        0x0d => InitSvcId::VmInitializeClass,
        0x0e => InitSvcId::VmGetArrayClass,
        0x0f => InitSvcId::VmInstantiate,
        0x10 => InitSvcId::VmInstantiateArray,
        0x11 => InitSvcId::VmInstantiateMultiArray,
        0x12 => InitSvcId::VmClassIsAssignableTo,
        0x13 => InitSvcId::JavaResolveOne,
        // vm_resolve_lists publishes the application's class tables.
        0x14 => InitSvcId::JavaLoadClasses,
        0x1f => InitSvcId::VmAllocSavePoint,
        0x20 => InitSvcId::VmFreeSavePoint,
        0x21 => InitSvcId::VmThrowException,
        0x22 => InitSvcId::VmThrowNullPointerException,
        0x23 => InitSvcId::VmThrowArrayIndexOutOfBoundsException,
        0x25 => InitSvcId::VmThrowArithmeticException,
        0x26 => InitSvcId::VmThrowClassCastException,
        0x27 => InitSvcId::LegacyVmThrowNegativeArraySizeException,
        0x38 => InitSvcId::VmThrowAbstractMethodError,
        0x40 => InitSvcId::VmThrowNoSuchMethodError,
        0x54 => InitSvcId::VmCheckStackOverflow,
        0x55 => InitSvcId::VmThreadReschedule,
        0x56 => InitSvcId::VmMonitorEnter,
        0x57 => InitSvcId::VmMonitorExit,
        0x61 => InitSvcId::VmAastoreImpl,
        0x64 => InitSvcId::VmFindInterface,
        0x82 => InitSvcId::VmAddClasspath,
        0x83 => InitSvcId::VmRunMainClass,
        0xe1 => InitSvcId::VmGetStringClass,
        0xe2 => InitSvcId::VmGetStringArrayClass,
        0xfa => InitSvcId::VmAastoreImplFast,
        _ => {
            tracing::warn!("Unimplemented LGT CLDC import {function_index:#x}; installing diagnostic zero-return stub");
            return core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + function_index);
        }
    };

    core.make_svc_stub(SVC_CATEGORY_INIT, id)
}

/// Import `0x14`. Reads the tables describing every platform class the
/// application imports, then fills the output arrays so the compiled code can
/// reach them.
#[allow(clippy::too_many_arguments)]
fn read_resolve_member(core: &ArmCore, table: u32, index: u32) -> Result<Option<(String, String)>> {
    let row = table + index * 8;
    let name: u32 = read_generic(core, row)?;
    let descriptor: u32 = read_generic(core, row + 4)?;

    if name == 0 || descriptor == 0 {
        return Ok(None);
    }

    let name = read_null_terminated_string_bytes(core, name)?;
    let descriptor = read_null_terminated_string_bytes(core, descriptor)?;

    Ok(Some((String::from_utf8_lossy(&name).into(), String::from_utf8_lossy(&descriptor).into())))
}

fn write_continuation_slot(core: &mut ArmCore, output: u32, index: u32) -> Result<()> {
    let previous = output
        .checked_add(index * 2)
        .and_then(|address| address.checked_sub(2))
        .ok_or_else(|| wie_util::WieError::FatalError(alloc::format!("Invalid LGT continuation slot {index} at {output:#x}")))?;
    let slot: u16 = read_generic(core, previous)?;
    write_generic(core, output + index * 2, slot.wrapping_add(1))
}

fn resolve_field_group(core: &mut ArmCore, class: &AppClass, table: u32, output: u32, start: u32, count: u32, want_static: bool) -> Result<()> {
    for index in start..start + count {
        let Some((name, descriptor)) = read_resolve_member(core, table, index)? else {
            write_continuation_slot(core, output, index)?;
            continue;
        };

        let member = class.members.iter().find(|member| {
            member.is_field() && member.name() == name && member.descriptor() == descriptor && ((member.flags() & 0x8) != 0) == want_static
        });

        let Some(member) = member else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_one could not resolve {} field {}.{}:{}",
                if want_static { "static" } else { "instance" },
                class.name,
                name,
                descriptor
            )));
        };

        write_generic(core, output + index * 2, member.slot() as u16)?;
    }

    Ok(())
}

fn resolve_virtual_member(
    core: &ArmCore,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    first: &AppClass,
    name: &str,
    descriptor: &str,
) -> Option<u32> {
    let mut current = first.clone();

    for _ in 0..MAX_CLASS_DEPTH {
        if let Some(member) = current
            .members
            .iter()
            .find(|member| member.is_method() && member.name() == name && member.descriptor() == descriptor && (member.slot() as u16 as i16) > 0)
        {
            return Some(member.slot());
        }

        let superclass = current.superclass.as_deref()?;

        if let Some(class) = app_classes.lock().iter().find(|class| class.name == superclass).cloned() {
            current = class;
            continue;
        }

        current = app_classes::find_class(core, image_ranges, superclass)?;
    }

    None
}

/// Import `0x13`, native `vm_resolve_one`.
///
/// Unlike import `0x14`, its first argument is one 24-byte class-entry and its
/// second argument is the application's already-linked `class_shared` root.
#[allow(clippy::too_many_arguments)]
pub async fn java_resolve_one(
    core: &mut ArmCore,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    class_entry: u32,
    class_shared: u32,
    fields: u32,
    static_fields: u32,
    virtual_methods: u32,
    interface_methods: u32,
    static_methods: u32,
    field_offsets: u32,
    static_field_offsets: u32,
    virtual_method_offsets: u32,
    interface_method_offsets: u32,
    static_method_offsets: u32,
) -> Result<()> {
    let class = app_classes::parse_class_root(core, class_shared)?;

    let field_start: u16 = read_generic(core, class_entry + 4)?;
    let field_count: u16 = read_generic(core, class_entry + 6)?;
    let static_field_start: u16 = read_generic(core, class_entry + 8)?;
    let static_field_count: u16 = read_generic(core, class_entry + 10)?;
    let virtual_method_start: u16 = read_generic(core, class_entry + 12)?;
    let virtual_method_count: u16 = read_generic(core, class_entry + 14)?;
    let interface_method_start: u16 = read_generic(core, class_entry + 16)?;
    let interface_method_count: u16 = read_generic(core, class_entry + 18)?;
    let static_method_start: u16 = read_generic(core, class_entry + 20)?;
    let static_method_count: u16 = read_generic(core, class_entry + 22)?;

    resolve_field_group(core, &class, fields, field_offsets, field_start.into(), field_count.into(), false)?;
    resolve_field_group(
        core,
        &class,
        static_fields,
        static_field_offsets,
        static_field_start.into(),
        static_field_count.into(),
        true,
    )?;

    let virtual_start = u32::from(virtual_method_start);
    for index in virtual_start..virtual_start + u32::from(virtual_method_count) {
        let Some((name, descriptor)) = read_resolve_member(core, virtual_methods, index)? else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "Blank LGT virtual-method import row {index}"
            )));
        };

        let Some(slot) = resolve_virtual_member(core, app_classes, image_ranges, &class, &name, &descriptor) else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_one could not resolve virtual method {}.{}{}",
                class.name,
                name,
                descriptor
            )));
        };

        write_generic(core, virtual_method_offsets + index * 2, slot as u16)?;
    }

    let interface_start = u32::from(interface_method_start);
    for index in interface_start..interface_start + u32::from(interface_method_count) {
        let Some((name, descriptor)) = read_resolve_member(core, interface_methods, index)? else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "Blank LGT interface-method import row {index}"
            )));
        };

        let Some(member) = class
            .members
            .iter()
            .find(|member| member.is_method() && member.name() == name && member.descriptor() == descriptor)
        else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_one could not resolve interface method {}.{}{}",
                class.name,
                name,
                descriptor
            )));
        };

        write_generic(core, interface_method_offsets + index * 2, member.slot() as u16)?;
    }

    let static_start = u32::from(static_method_start);
    let static_end = static_start + u32::from(static_method_count);
    let mut index = static_start;
    while index < static_end {
        let Some((name, descriptor)) = read_resolve_member(core, static_methods, index)? else {
            // Native vm_resolve_one treats a blank static-method row as the
            // class metadata's get_class/get_raw_class pair and consumes two
            // consecutive output words.
            write_generic(core, static_method_offsets + index * 4, class.get_class)?;
            if index + 1 < static_end {
                write_generic(core, static_method_offsets + (index + 1) * 4, class.get_raw_class)?;
            }
            index += 2;
            continue;
        };

        let Some(member) = class
            .members
            .iter()
            .find(|member| member.is_method() && member.name() == name && member.descriptor() == descriptor)
        else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_one could not resolve static method {}.{}{}",
                class.name,
                name,
                descriptor
            )));
        };

        let Some(entry) = member.entry() else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_one selected non-method {}.{}{}",
                class.name,
                name,
                descriptor
            )));
        };

        write_generic(core, static_method_offsets + index * 4, entry)?;
        index += 1;
    }

    Ok(())
}

pub async fn java_load_classes(
    core: &mut ArmCore,
    handles: &JavaHandles,
    classes: u32,
    fields: u32,
    static_fields: u32,
    virtual_methods: u32,
    interface_methods: u32,
    static_methods: u32,
    field_offsets: u32,
    static_field_offsets: u32,
    virtual_method_offsets: u32,
    interface_method_offsets: u32,
    static_method_offsets: u32,
) -> Result<ClassTable> {
    let mut table = ClassTable::parse(
        core,
        classes,
        fields,
        static_fields,
        virtual_methods,
        interface_methods,
        static_methods,
        OutputArrays {
            field_offsets,
            static_field_offsets,
            virtual_method_offsets,
            interface_method_offsets,
            static_method_offsets,
        },
    )?;

    tracing::debug!(
        "java_load_classes: {} classes, {} static methods, {} virtual methods; inputs at \
         classes={classes:#x}, fields={fields:#x}, static_fields={static_fields:#x}, \
         virtual_methods={virtual_methods:#x}, interface_methods={interface_methods:#x}, \
         static_methods={static_methods:#x}; outputs at \
         static_method_offsets={:#x}, virtual_method_offsets={:#x}, field_offsets={:#x}, \
         static_field_offsets={:#x}, interface_method_offsets={:#x}",
        table.classes.len(),
        table.static_methods.len(),
        table.virtual_methods.len(),
        table.outputs.static_method_offsets,
        table.outputs.virtual_method_offsets,
        table.outputs.field_offsets,
        table.outputs.static_field_offsets,
        table.outputs.interface_method_offsets,
    );

    install_dispatch(core, handles, &mut table)?;

    Ok(table)
}

/// Builds, per class, the dispatch table an instance points at and the token
/// its first reserved static row hands back.
///
/// The compiled code dispatches a virtual call as
///
/// ```text
/// ldrsh r2, [r8, #row * 2]   ; slot, from virtual_method_offsets
/// ldr   r3, [r5]             ; the receiver's dispatch table, at its word 0
/// add   r3, r3, r2, lsl #2
/// ldr   ip, [r3, #4]         ; the entry, one word past the slot
/// bx    ip
/// ```
///
/// so the table needs a leading word before its entries, and every instance
/// needs to point at one.
fn build_dispatch_tables(core: &mut ArmCore, handles: &JavaHandles, table: &mut ClassTable, slots: &[Option<u32>]) -> Result<()> {
    if table.classes.len() as u32 > MAX_DISPATCH_CLASSES {
        return Err(wie_util::WieError::FatalError(alloc::format!(
            "LGT class table has {} classes, more than the {MAX_DISPATCH_CLASSES} with reserved dispatch slots",
            table.classes.len()
        )));
    }

    // Native class identity is carried in dispatch-table word zero. Platform
    // classes do not have their original liblgt_system.so class_shared objects
    // in guest memory, so give each imported class a stable synthetic identity.
    for index in 0..table.classes.len() as u32 {
        let root = Allocator::alloc(core, 4)?;
        write_generic(core, root, index)?;
        table.class_roots.push(root);
    }

    for index in 0..table.classes.len() as u32 {
        let (start, count) = {
            let class = &table.classes[index as usize];
            (class.virtual_method_start, class.virtual_method_count)
        };
        let root = table.class_roots[index as usize];

        let vtable = build_dispatch_table(core, index, root, start, count, slots)?;

        handles.set_dispatch_table(&table.classes[index as usize].name, vtable);
        table.vtables.push(vtable);
    }

    // Objects of a class the application never declared still get called on,
    // so they need a table too.
    let fallback = build_dispatch_table(core, MAX_DISPATCH_CLASSES, 0, 0, 0, slots)?;
    handles.set_fallback_dispatch_table(fallback);

    // Native get_class/get_raw_class return an activated java/lang/Class
    // object. Its +8 word points at a data block whose +8 word is the
    // represented class_shared:
    //
    //   class_object + 8 -> data
    //   data + 8         -> class_shared
    //
    // Preserve exactly the part of that ABI the compiled application reads.
    let class_dispatch = table
        .classes
        .iter()
        .position(|class| class.name == "java/lang/Class")
        .and_then(|index| table.vtables.get(index).copied())
        .unwrap_or(fallback);

    for index in 0..table.classes.len() as u32 {
        let root = table.class_roots[index as usize];
        let vtable = table.vtables[index as usize];

        let data = Allocator::alloc(core, 12)?;
        core.write_bytes(data, &[0; 12])?;
        write_generic(core, data + 8, root)?;

        let class_object = Allocator::alloc(core, 12)?;
        write_generic(core, class_object, class_dispatch)?;
        write_generic(core, class_object + 4, 0u32)?;
        write_generic(core, class_object + 8, data)?;

        tracing::trace!(
            "LGT class {} -> root {root:#x}, object {class_object:#x}, dispatch table {vtable:#x}",
            table.classes[index as usize].name
        );

        table.class_objects.push(class_object);
    }

    tracing::debug!("LGT fallback dispatch table at {fallback:#x}");

    Ok(())
}

/// Builds one dispatch table.
///
/// The imported methods are installed at the native slots resolved from the
/// platform metadata. The compiled code also emits fixed slot numbers for
/// methods the platform is expected to provide, so the generated dispatch
/// layout must preserve those native slot numbers exactly.
///
/// Every table is the same size whatever the class declares, because a class
/// gets called at slots it never mentions: Battle Monster branches through
/// slot 13 of a `java/lang/Runtime` that declares no virtual methods at all. A
/// short table leaves that slot zero and the branch goes to address zero, so
/// the slots a class does not declare are filled with stubs that report what
/// was called instead.
fn build_dispatch_table(core: &mut ArmCore, class_index: u32, class_root: u32, start: u32, count: u32, slots: &[Option<u32>]) -> Result<u32> {
    let vtable = Allocator::alloc(core, (DISPATCH_TABLE_SLOTS + 1) * 4)?;
    write_generic(core, vtable, class_root)?;

    for slot in 0..DISPATCH_TABLE_SLOTS {
        let row = (start..start + count).find(|row| slots.get(*row as usize).copied().flatten() == Some(slot));

        let svc = if let Some(row) = row {
            JAVA_VIRTUAL_METHOD_SVC_BASE + row
        } else {
            JAVA_UNKNOWN_SLOT_SVC_BASE + class_index * DISPATCH_TABLE_SLOTS + slot
        };

        let stub = core.make_svc_stub(SVC_CATEGORY_INIT, svc)?;
        write_generic(core, vtable + 4 + slot * 4, stub)?;
    }

    Ok(vtable)
}

/// Native dispatch slot for each imported virtual-method row.
///
/// The application's import order is only a subset of a platform class's full
/// method table, so use the slot layout extracted from `liblgt_system.so` when
/// available. The relative-order fallback remains only for classes not yet
/// covered by the extracted platform table.
fn assign_virtual_slots(table: &ClassTable) -> Result<Vec<Option<u32>>> {
    let mut slots = vec![None; table.virtual_methods.len()];

    for (index, member) in table.virtual_methods.iter().enumerate() {
        let Some(member) = member else {
            continue;
        };

        let class_name = table.class_name(member.class_index);
        let Some(class) = platform_class(class_name) else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_lists could not find platform class {class_name}"
            )));
        };

        let Some(method) = class.virtual_method(&member.name, &member.descriptor) else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_lists could not resolve virtual method {}",
                table.describe(member)
            )));
        };

        slots[index] = Some(method.slot);
    }

    Ok(slots)
}

/// Publishes the five native `vm_resolve_one` output groups for import `0x14`,
/// then builds guest dispatch tables whose imported virtual slots point at the
/// existing row-indexed JVM bridge stubs.
///
/// The native runtime resolves fields and methods from the selected platform
/// class metadata. Virtual lookup follows the superclass chain and accepts only
/// positive signed 16-bit slots; interface/static lookup is current-class only.
fn install_dispatch(core: &mut ArmCore, handles: &JavaHandles, table: &mut ClassTable) -> Result<()> {
    if table.static_methods.len() as u32 > JAVA_METHOD_SVC_LIMIT {
        return Err(wie_util::WieError::FatalError(alloc::format!(
            "LGT static method table has {} rows, more than the {JAVA_METHOD_SVC_LIMIT} reserved",
            table.static_methods.len()
        )));
    }

    let mut field_bindings = Vec::new();
    let mut highest_native_field_slot = None;

    for class in &table.classes {
        let Some(platform) = platform_class(&class.name) else {
            return Err(wie_util::WieError::FatalError(alloc::format!(
                "LGT vm_resolve_lists could not find platform class {}",
                class.name
            )));
        };

        for index in class.field_start..class.field_start + class.field_count {
            let Some(member) = table.fields.get(index as usize).and_then(|member| member.as_ref()) else {
                write_continuation_slot(core, table.outputs.field_offsets, index)?;
                continue;
            };

            let Some(field) = platform.field(&member.name, &member.descriptor, false) else {
                return Err(wie_util::WieError::FatalError(alloc::format!(
                    "LGT vm_resolve_lists could not resolve instance field {}",
                    table.describe(member)
                )));
            };

            write_generic(core, table.outputs.field_offsets + index * 2, field.slot as u16)?;
            highest_native_field_slot = Some(highest_native_field_slot.map_or(field.slot, |slot: u32| slot.max(field.slot)));

            field_bindings.push(super::handles::JavaFieldBinding {
                class_name: class.name.clone(),
                name: member.name.clone(),
                descriptor: member.descriptor.clone(),
                slot: field.slot,
            });
        }

        for index in class.static_field_start..class.static_field_start + class.static_field_count {
            let Some(member) = table.static_fields.get(index as usize).and_then(|member| member.as_ref()) else {
                write_continuation_slot(core, table.outputs.static_field_offsets, index)?;
                continue;
            };

            let Some(field) = platform.field(&member.name, &member.descriptor, true) else {
                return Err(wie_util::WieError::FatalError(alloc::format!(
                    "LGT vm_resolve_lists could not resolve static field {}",
                    table.describe(member)
                )));
            };

            write_generic(core, table.outputs.static_field_offsets + index * 2, field.slot as u16)?;
        }
    }

    let slots = assign_virtual_slots(table)?;
    for (index, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else {
            continue;
        };

        write_generic(core, table.outputs.virtual_method_offsets + index as u32 * 2, *slot as u16)?;
    }

    for class in &table.classes {
        let platform = platform_class(&class.name).expect("platform class checked above");

        for index in class.interface_method_start..class.interface_method_start + class.interface_method_count {
            let Some(member) = table.interface_methods.get(index as usize).and_then(|member| member.as_ref()) else {
                return Err(wie_util::WieError::FatalError(alloc::format!(
                    "Blank LGT interface-method import row {index}"
                )));
            };

            let Some(method) = platform.method(&member.name, &member.descriptor) else {
                return Err(wie_util::WieError::FatalError(alloc::format!(
                    "LGT vm_resolve_lists could not resolve interface method {}",
                    table.describe(member)
                )));
            };

            write_generic(core, table.outputs.interface_method_offsets + index * 2, method.slot as u16)?;
        }

        let mut index = class.static_method_start;
        let end = index + class.static_method_count;
        while index < end {
            match table.static_methods.get(index as usize).and_then(|member| member.as_ref()) {
                Some(member) => {
                    if platform.method(&member.name, &member.descriptor).is_none() {
                        return Err(wie_util::WieError::FatalError(alloc::format!(
                            "LGT vm_resolve_lists could not resolve static method {}",
                            table.describe(member)
                        )));
                    }

                    let stub = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_STATIC_METHOD_SVC_BASE + index)?;
                    write_generic(core, table.outputs.static_method_offsets + index * 4, stub)?;
                    index += 1;
                }
                None => {
                    // Native vm_resolve_one writes get_class/get_raw_class for this
                    // two-row pair. WIE bridges those class accessors through the
                    // existing reserved-row SVCs; their exact initialization
                    // distinction is implemented separately.
                    let first = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_RESERVED_SLOT_SVC_BASE + index)?;
                    write_generic(core, table.outputs.static_method_offsets + index * 4, first)?;

                    if index + 1 < end {
                        let second = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_RESERVED_SLOT_SVC_BASE + index + 1)?;
                        write_generic(core, table.outputs.static_method_offsets + (index + 1) * 4, second)?;
                    }

                    index += 2;
                }
            }
        }
    }

    build_dispatch_tables(core, handles, table, &slots)?;

    if table.interface_methods.len() as u32 > JAVA_METHOD_SVC_LIMIT {
        return Err(wie_util::WieError::FatalError(alloc::format!(
            "LGT interface method table has {} rows, more than the {JAVA_METHOD_SVC_LIMIT} reserved",
            table.interface_methods.len()
        )));
    }

    // Native vm_find_interface returns a pointer to an interface-specific
    // dispatch table whose word zero is the requested class_shared identity.
    // The compiled application then indexes that table with the native slot
    // written to interface_method_offsets and loads the callable at +4.
    for class_index in 0..table.classes.len() {
        let class = &table.classes[class_index];
        if class.interface_method_count == 0 {
            table.interface_vtables.push(0);
            continue;
        }

        let platform = platform_class(&class.name).expect("platform class checked above");
        let mut highest_slot = 0u32;

        for index in class.interface_method_start..class.interface_method_start + class.interface_method_count {
            let member = table
                .interface_methods
                .get(index as usize)
                .and_then(|member| member.as_ref())
                .ok_or_else(|| wie_util::WieError::FatalError(alloc::format!("Blank LGT interface-method import row {index}")))?;
            let method = platform
                .method(&member.name, &member.descriptor)
                .ok_or_else(|| wie_util::WieError::FatalError(alloc::format!("LGT interface table could not resolve {}", table.describe(member))))?;
            highest_slot = highest_slot.max(method.slot);
        }

        let root = table.class_roots[class_index];
        let dispatch = Allocator::alloc(core, (highest_slot + 2) * 4)?;
        core.write_bytes(dispatch, &vec![0; ((highest_slot + 2) * 4) as usize])?;
        write_generic(core, dispatch, root)?;

        for index in class.interface_method_start..class.interface_method_start + class.interface_method_count {
            let member = table.interface_methods[index as usize].as_ref().expect("interface member checked above");
            let method = platform.method(&member.name, &member.descriptor).expect("interface method checked above");

            let stub = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_INTERFACE_METHOD_SVC_BASE + index)?;
            write_generic(core, dispatch + 4 + method.slot * 4, stub)?;
        }

        table.interface_vtables.push(dispatch);
    }

    // The imported native fields occupy their original word slots. AOT
    // application code also indexes this output array beyond the platform
    // import rows, so preserve that separate compatibility tail until the
    // application-field path is moved off this array.
    let capacity = table.field_offset_capacity();
    let compatibility_base = highest_native_field_slot.map(|slot| slot + 1).unwrap_or(0);

    for row in table.fields.len() as u32..capacity {
        write_generic(core, table.outputs.field_offsets + row * 2, (compatibility_base + row) as u16)?;
    }

    handles.set_field_bindings(field_bindings);
    handles.set_field_slots(compatibility_base + capacity);

    tracing::debug!(
        "LGT native platform resolver: {} classes, {} field rows, {} static-field rows, {} virtual rows, {} interface rows, {} static rows",
        table.classes.len(),
        table.fields.len(),
        table.static_fields.len(),
        table.virtual_methods.len(),
        table.interface_methods.len(),
        table.static_methods.len(),
    );

    Ok(())
}

/// Import `0x83`. The application asks the platform to run a Java main class,
/// passing the Jlet's own class name as the first argument - the same shape
/// KTF uses. The named class is one of the application's own compiled classes,
/// so a bridge is registered for it before `Main` is entered.
#[allow(clippy::too_many_arguments)]
pub async fn vm_run_main_class(
    core: &mut ArmCore,
    jvm: &mut Jvm,
    handles: &JavaHandles,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    argc: u32,
    argv: u32,
    fallback_main_class: Option<&str>,
) -> Result<u32> {
    let argc = argc.min(MAX_MAIN_ARGUMENTS);
    tracing::debug!("vm_run_main_class: argc={argc}, argv={argv:#x}");

    // Reading the argument vector must not end the run. Not every ABI passes
    // argc/argv in the registers the classic one does, so a title can reach
    // here with a bogus vector; an unreadable entry stops collection and the
    // main class still runs (with the arguments gathered so far), rather than
    // the whole title dying before it starts.
    let mut arguments = Vec::with_capacity(argc as usize);
    for index in 0..argc {
        let pointer: u32 = match read_generic(core, argv + index * 4) {
            Ok(pointer) => pointer,
            Err(error) => {
                tracing::warn!(
                    "vm_run_main_class: argv[{index}] at {:#x} unreadable ({error:?}); using {} argument(s)",
                    argv + index * 4,
                    arguments.len()
                );
                break;
            }
        };
        let bytes = match read_null_terminated_string_bytes(core, pointer) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(
                    "vm_run_main_class: argv[{index}] string at {pointer:#x} unreadable ({error:?}); using {} argument(s)",
                    arguments.len()
                );
                break;
            }
        };

        arguments.push(String::from_utf8_lossy(&bytes).into_owned());
    }

    // `org.kwis.msp.lcdui.Main::main` reads argv[0] as the application's main
    // class. When the ABI did not hand over a readable vector, fall back to the
    // main class the archive descriptor named (`app_info` `mclass`), which WIE
    // already knows, so the title still launches its Jlet.
    if arguments.is_empty()
        && let Some(main_class) = fallback_main_class
        && !main_class.is_empty()
    {
        tracing::debug!("vm_run_main_class: no argv read; falling back to descriptor main class {main_class:?}");
        arguments.push(main_class.to_owned());
    }

    tracing::debug!("java_run_main({arguments:?})");

    if let Some(main_class_name) = arguments.first() {
        bridge_class_chain(jvm, core, handles, app_classes, image_ranges, main_class_name).await;
    }

    let mut args_array = match jvm.instantiate_array("Ljava/lang/String;", arguments.len()).await {
        Ok(array) => array,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    for (index, argument) in arguments.iter().enumerate() {
        let java_argument = match JavaLangString::from_rust_string(jvm, argument).await {
            Ok(value) => value,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        if let Err(error) = jvm.store_array(&mut args_array, index, vec![java_argument]).await {
            return Err(JvmSupport::to_wie_err(jvm, error).await);
        }
    }

    let result: JvmResult<()> = jvm
        .invoke_static("org/kwis/msp/lcdui/Main", "main", "([Ljava/lang/String;)V", (args_array,))
        .await;

    if let Err(error) = result {
        return Err(JvmSupport::to_wie_err(jvm, error).await);
    }

    Ok(0)
}

/// Collects the application classes `name` depends on, each listed after the
/// classes it needs, so registering `order` front to back never defines a
/// class before its parent or an interface it implements.
///
/// `visited` is marked on entry, both to skip work already done and to keep a
/// cyclic metadata reference from recursing forever; `order` is filled in
/// post-order, so a class lands after its superclass and interfaces. A name
/// the image does not carry is a platform class the JVM already knows and ends
/// that branch.
fn collect_app_class_dependencies(
    core: &ArmCore,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    name: &str,
    visited: &mut Vec<String>,
    order: &mut Vec<String>,
    depth: usize,
) -> bool {
    if visited.iter().any(|x| x == name) {
        return true;
    }
    if depth >= MAX_CLASS_DEPTH {
        tracing::warn!("Application class {name} sits deeper than {MAX_CLASS_DEPTH} in the dependency graph; giving up");
        return false;
    }
    visited.push(name.to_owned());

    // The dependencies that must exist before this class can be defined: its
    // superclass and every interface it implements.
    let dependencies = {
        let app_classes = app_classes.lock();
        app_classes.iter().find(|x| x.name == name).map(|class| {
            let mut dependencies = class.interfaces.clone();
            if let Some(superclass) = &class.superclass {
                dependencies.push(superclass.clone());
            }
            dependencies
        })
    };
    let dependencies = match dependencies {
        Some(dependencies) => dependencies,
        None => match app_classes::find_class(core, image_ranges, name) {
            Some(class) => {
                let mut dependencies = class.interfaces;
                if let Some(superclass) = class.superclass {
                    dependencies.push(superclass);
                }
                dependencies
            }
            // A platform class the JVM already knows; nothing to register.
            None => return true,
        },
    };

    for dependency in dependencies {
        if !collect_app_class_dependencies(core, app_classes, image_ranges, &dependency, visited, order, depth + 1) {
            return false;
        }
    }

    order.push(name.to_owned());
    true
}

/// Registers a JVM stand-in for an application class and everything it
/// inherits from or implements.
///
/// An application class can extend another one - Battle Monster's `Game`
/// extends `a`, which extends `org/kwis/msp/lcdui/Jlet` - and it can implement
/// application interfaces too - Legend of Master's `f` implements `k` - so the
/// whole dependency graph is walked and registered from the leaves in. A class
/// cannot be registered before its parent or its interfaces.
pub async fn bridge_class_chain(
    jvm: &Jvm,
    core: &ArmCore,
    handles: &JavaHandles,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    name: &str,
) -> bool {
    // argv hands the main class in binary (dotted) form - `atdata.JimaeMD` - but
    // the image indexes classes in JVM internal (slash) form - `atdata/JimaeMD`.
    // Normalise so a packaged main class is found; a name already in slash form
    // (or a bare class with no package) is unchanged.
    let normalized = name.replace('.', "/");
    let name = normalized.as_str();

    let mut visited = Vec::new();
    let mut order = Vec::new();
    if !collect_app_class_dependencies(core, app_classes, image_ranges, name, &mut visited, &mut order, 0) {
        return false;
    }

    if order.is_empty() {
        tracing::error!("Application class {name} is nowhere in the image; cannot bridge it");
        return false;
    }

    let context = CompiledContext {
        core: core.clone(),
        handles: handles.clone(),
    };

    // The proto is built with the lock held and registered with it released:
    // registering re-enters the runtime. The main class is usually absent from
    // the registered table, so fall back to finding it by shape in the image.
    for class_name in order {
        let proto = {
            let app_classes_guard = app_classes.lock();
            let described = app_classes_guard.iter().find(|x| x.name == class_name).map(compiled_class::as_proto);
            drop(app_classes_guard);

            match described {
                Some(proto) => proto,
                None => match app_classes::find_class(core, image_ranges, &class_name) {
                    Some(class) => compiled_class::as_proto(&class),
                    None => continue,
                },
            }
        };

        if !compiled_class::register(jvm, &context, &class_name, proto).await {
            return false;
        }
    }

    true
}

/// `vm_get_constant_string(class, chars, length, cache)`.
///
/// The application keeps one word per string constant and passes its address
/// as `cache`. A non-zero word means the constant has been interned already
/// and is handed straight back; otherwise the string is built, stored there,
/// and **returned** - the compiled code takes the result from `r0` and only
/// reads the cache word on a later call. Returning zero here handed the
/// application a null every time a constant was first used, which is how
/// `new StringBuffer("/res/script/")` came to be constructed on a null.
pub async fn vm_get_constant_string(core: &mut ArmCore, handles: &JavaHandles, jvm: &mut Jvm, chars: u32, length: u32, cache: u32) -> Result<u32> {
    let interned: u32 = read_generic(core, cache)?;
    if interned != 0 {
        return Ok(interned);
    }

    let mut bytes = vec![0u8; (length as usize) * 2];
    core.read_bytes(chars, &mut bytes)?;

    let utf16 = bytes.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
    let rust_string = String::from_utf16_lossy(&utf16);

    let java_string = match JavaLangString::from_rust_string(jvm, &rust_string).await {
        Ok(value) => value,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    let handle = handles.insert(java_string)?;
    write_generic(core, cache, handle)?;

    tracing::debug!("vm_get_constant_string({rust_string:?}) -> {handle:#x}, cached at {cache:#x}");

    Ok(handle)
}

/// The shape represented by one array-class token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArrayClassInfo {
    pub dimensions: u32,
    pub element_class: u32,
    pub atype: u32,
    pub element_size: u32,
    /// Dispatch table whose word zero is this array class's identity.
    pub vtable: u32,
}

/// Array classes handed out by `vm_get_array_class`, mapped to their complete
/// shape. Different array types can have the same element width and must not
/// therefore share a token.
pub type ArrayClasses = Arc<Mutex<BTreeMap<u32, ArrayClassInfo>>>;

/// Bytes a reference takes, which is also what an element of an array of
/// arrays takes.
pub const REFERENCE_SIZE: u32 = 4;

/// Bytes one element of a primitive array takes.
///
/// The codes are the JVM's `newarray` atypes, which is what
/// `vm_get_array_class` indexes its descriptor letters with: 4 `boolean`,
/// 5 `char`, 6 `float`, 7 `double`, 8 `byte`, 9 `short`, 10 `int`, 11 `long`.
pub fn primitive_element_size(atype: u32) -> Option<u32> {
    Some(match atype {
        // Standard JVM primitive array tags.
        //
        // LGT tag 1 is not a byte primitive in Legend of Master: it is also
        // used for arrays whose elements are object references. Leaving it
        // unresolved makes `vm_get_array_class` use REFERENCE_SIZE.
        4 | 8 => 1,
        5 | 9 => 2,
        6 | 10 => 4,
        7 | 11 => 8,
        _ => return None,
    })
}

/// `vm_instantiate_array(array_class, length)`.
///
/// The compiled code asks `vm_get_array_class` for the class first, so the
/// element size is whatever that call recorded for it. An array has no class
/// of its own here, so it dispatches through the fallback table like anything
/// else the application never declared.
pub async fn vm_instantiate_array(handles: &JavaHandles, array_class: &ArrayClasses, class: u32, length: u32) -> Result<u32> {
    let (element_size, vtable) = match array_class.lock().get(&class).copied() {
        Some(info) => (info.element_size, info.vtable),
        None => {
            tracing::warn!("vm_instantiate_array({class:#x}, {length}) names no array class; assuming references");

            (REFERENCE_SIZE, handles.fallback_dispatch_table())
        }
    };

    let array = handles.allocate_array(vtable, length, element_size)?;

    tracing::debug!("vm_instantiate_array({class:#x}, {length}) -> {array:#x}, {element_size} bytes an element");

    Ok(array)
}

#[cfg(test)]
mod tests {
    use wie_core_arm::{Allocator, ArmCore};
    use wie_util::{ByteWrite, read_generic, write_generic};

    use super::write_continuation_slot;

    #[test]
    fn continuation_slot_advances_wide_field_second_word() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();

        let output = Allocator::alloc(&mut core, 4).unwrap();
        core.write_bytes(output, &[0; 4]).unwrap();

        // LoM field row 50 is ew:J at native slot 262, while row 51 is
        // a blank continuation row for the long's second 32-bit word.
        write_generic(&mut core, output, 262u16).unwrap();

        write_continuation_slot(&mut core, output, 1).unwrap();

        let first: u16 = read_generic(&core, output).unwrap();
        let continuation: u16 = read_generic(&core, output + 2).unwrap();

        assert_eq!(first, 262);
        assert_eq!(continuation, 263);
    }
}
