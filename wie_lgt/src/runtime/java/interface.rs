use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};

use spin::Mutex;

use jvm::{Jvm, Result as JvmResult, runtime::JavaLangString};
use wie_core_arm::{Allocator, ArmCore};
use wie_jvm_support::JvmSupport;
use wie_util::{ByteRead, Result, read_generic, read_null_terminated_string_bytes, write_generic};

use crate::runtime::{
    SVC_CATEGORY_INIT,
    java::{
        app_classes::{self, AppClass},
        class_table::{ClassTable, OutputArrays},
        compiled_class::{self, CompiledContext},
        handles::JavaHandles,
        platform_slots::platform_slot,
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

/// Slots every dispatch table has room for, declared or not.
pub const DISPATCH_TABLE_SLOTS: u32 = 96;

/// Where a class's own virtual methods start in its dispatch table. Slot 0 is
/// the class's `<init>` and slots 1 to 9 are `java/lang/Object`'s, in every
/// `dt_` in `liblgt_system.so`.
pub const FIRST_CLASS_SLOT: u32 = 10;

/// Classes that can have their own reserved block of unknown-slot ids. One
/// past the last is the fallback table's index.
pub const MAX_DISPATCH_CLASSES: u32 = 63;

/// One SVC id per (class, slot) pair a dispatch table does not account for.
pub const JAVA_UNKNOWN_SLOT_SVC_BASE: u32 = 0x8000;

/// One SVC id per row of the static method table that carries no descriptor.
/// The compiled code calls these too, so they cannot be left null.
pub const JAVA_RESERVED_SLOT_SVC_BASE: u32 = 0x4000;

/// Upper bound on rows, so a table that fails to parse cannot run off the end
/// of its SVC range.
pub const JAVA_METHOD_SVC_LIMIT: u32 = 0x2000;

pub fn get_java_interface_method(core: &mut ArmCore, function_index: u32) -> Result<u32> {
    let method = match function_index {
        0x03 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk0)?,
        0x06 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk12)?,
        0x07 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk5)?,
        0x09 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport09)?,
        0x0e => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport0e)?,
        0x10 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport10)?,
        0x11 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport11)?,
        0x23 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport23)?,
        0x13 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaResolveOne)?,
        0x14 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaLoadClasses)?,
        0x82 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk9)?,
        0x83 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk11)?,
        0xe1 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImportE1)?,
        0xe2 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImportE2)?,
        _ => {
            tracing::warn!("Unimplemented LGT Java import {function_index:#x};                  installing diagnostic zero-return stub");
            core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + function_index)?
        }
    };

    Ok(method)
}

pub async fn java_unk0(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32) -> Result<()> {
    tracing::warn!("java_unk0({a0:#x}, {a1:#x}, {a2:#x})");
    Ok(())
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

    Ok(Some((
        String::from_utf8_lossy(&name).into(),
        String::from_utf8_lossy(&descriptor).into(),
    )))
}

fn write_continuation_slot(core: &mut ArmCore, output: u32, index: u32) -> Result<()> {
    let previous = output
        .checked_add(index * 2)
        .and_then(|address| address.checked_sub(2))
        .ok_or_else(|| wie_util::WieError::FatalError(alloc::format!(
            "Invalid LGT continuation slot {index} at {output:#x}"
        )))?;
    let slot: u16 = read_generic(core, previous)?;
    write_generic(core, output + index * 2, slot.wrapping_add(1))
}

fn resolve_field_group(
    core: &mut ArmCore,
    class: &AppClass,
    table: u32,
    output: u32,
    start: u32,
    count: u32,
    want_static: bool,
) -> Result<()> {
    for index in start..start + count {
        let Some((name, descriptor)) = read_resolve_member(core, table, index)? else {
            write_continuation_slot(core, output, index)?;
            continue;
        };

        let member = class.members.iter().find(|member| {
            member.is_field()
                && member.name() == name
                && member.descriptor() == descriptor
                && ((member.flags() & 0x8) != 0) == want_static
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
        if let Some(member) = current.members.iter().find(|member| {
            member.is_method()
                && member.name() == name
                && member.descriptor() == descriptor
                && (member.slot() as u16 as i16) > 0
        }) {
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

    resolve_field_group(
        core,
        &class,
        fields,
        field_offsets,
        field_start.into(),
        field_count.into(),
        false,
    )?;
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

        let Some(member) = class.members.iter().find(|member| {
            member.is_method() && member.name() == name && member.descriptor() == descriptor
        }) else {
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

        let Some(member) = class.members.iter().find(|member| {
            member.is_method() && member.name() == name && member.descriptor() == descriptor
        }) else {
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

    for index in 0..table.classes.len() as u32 {
        let (start, count) = {
            let class = &table.classes[index as usize];
            (class.virtual_method_start, class.virtual_method_count)
        };

        let vtable = build_dispatch_table(core, index, start, count, slots)?;

        // Eight bytes is the smallest allocation that reads back distinctly;
        // nothing inspects the contents, the address is the identity.
        let class_object = Allocator::alloc(core, 8)?;
        write_generic(core, class_object, 0u32)?;
        write_generic(core, class_object + 4, index)?;

        handles.set_dispatch_table(&table.classes[index as usize].name, vtable);

        tracing::trace!(
            "LGT class {} -> object {class_object:#x}, dispatch table {vtable:#x} ({count} declared slots)",
            table.classes[index as usize].name
        );

        table.vtables.push(vtable);
        table.class_objects.push(class_object);
    }

    // Objects of a class the application never declared still get called on,
    // so they need a table too.
    let fallback = build_dispatch_table(core, MAX_DISPATCH_CLASSES, 0, 0, slots)?;
    handles.set_fallback_dispatch_table(fallback);

    tracing::debug!("LGT fallback dispatch table at {fallback:#x}");

    Ok(())
}

/// Builds one dispatch table.
///
/// A class's own methods start at [`FIRST_CLASS_SLOT`], because the ten slots
/// before them belong to `<init>` and `java/lang/Object` in every table the
/// platform builds. Getting that wrong is not caught by anything the runtime
/// can see on its own - the slots it hands out and the table it builds stay
/// consistent with each other - but the compiled code also emits fixed slot
/// numbers for methods the platform is expected to provide, and those are
/// numbered from the real layout.
///
/// Every table is the same size whatever the class declares, because a class
/// gets called at slots it never mentions: Battle Monster branches through
/// slot 13 of a `java/lang/Runtime` that declares no virtual methods at all. A
/// short table leaves that slot zero and the branch goes to address zero, so
/// the slots a class does not declare are filled with stubs that report what
/// was called instead.
fn build_dispatch_table(core: &mut ArmCore, class_index: u32, start: u32, count: u32, slots: &[Option<u32>]) -> Result<u32> {
    let vtable = Allocator::alloc(core, (DISPATCH_TABLE_SLOTS + 1) * 4)?;
    write_generic(core, vtable, 0u32)?;

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
fn assign_virtual_slots(table: &ClassTable) -> Vec<Option<u32>> {
    (0..table.virtual_methods.len() as u32)
        .map(|row| {
            let member = table.virtual_methods[row as usize].as_ref()?;
            let class = table.class_name(member.class_index);
            let index = table.virtual_slot(row)?;

            Some(platform_slot(class, &member.name, &member.descriptor).unwrap_or(FIRST_CLASS_SLOT + index))
        })
        .collect()
}

/// Publishes the table into the arrays the compiled code reads.
///
/// Rows the application left blank are skipped: it reserves two at the head of
/// every class's static method block, and what belongs there is not yet known.
fn install_dispatch(core: &mut ArmCore, handles: &JavaHandles, table: &mut ClassTable) -> Result<()> {
    if table.static_methods.len() as u32 > JAVA_METHOD_SVC_LIMIT {
        return Err(wie_util::WieError::FatalError(alloc::format!(
            "LGT static method table has {} rows, more than the {JAVA_METHOD_SVC_LIMIT} reserved",
            table.static_methods.len()
        )));
    }

    for index in 0..table.static_methods.len() as u32 {
        let (svc, description) = match &table.static_methods[index as usize] {
            Some(member) => (JAVA_STATIC_METHOD_SVC_BASE + index, table.describe(member)),
            // Every class reserves two rows at the head of its static method
            // block, and the compiled code branches through them: an LGT
            // constructor calls its class's first reserved row before the
            // superclass constructor. Leaving them null turns that into a
            // branch to address zero, so they get a stub that reports what it
            // was called with. What they are meant to do is still unknown.
            None => {
                let (class, slot) = table
                    .static_method_owner(index)
                    .map(|(class, slot)| (class.name.as_str(), slot))
                    .unwrap_or(("<unowned>", 0));

                (JAVA_RESERVED_SLOT_SVC_BASE + index, alloc::format!("{class} reserved slot {slot}"))
            }
        };

        let stub = core.make_svc_stub(SVC_CATEGORY_INIT, svc)?;
        write_generic(core, table.outputs.static_method_offsets + index * 4, stub)?;

        tracing::trace!("LGT static method[{index}] {description} -> {stub:#x}");
    }

    let slots = assign_virtual_slots(table);

    // Virtual methods are dispatched through the receiver, so the array holds
    // the slot to index its vtable with rather than an address. The compiled
    // code reads it with `ldrsh`, so the entries are signed halfwords.
    for index in 0..table.virtual_methods.len() as u32 {
        let Some(member) = table.virtual_methods[index as usize].as_ref() else {
            continue;
        };
        let Some(slot) = slots[index as usize] else { continue };

        write_generic(core, table.outputs.virtual_method_offsets + index * 2, slot as u16)?;

        tracing::trace!("LGT virtual method[{index}] {} -> slot {slot}", table.describe(member));
    }

    build_dispatch_tables(core, handles, table, &slots)?;

    // Imported platform fields use the native VM's word-slot ABI. Rows beyond
    // the imported field table are also used by the application's AOT object
    // model, so keep those compatibility rows distinct from every native slot.
    //
    // The highest native slot currently established is TextComponent.iMode at
    // slot 19. Compatibility rows therefore begin at slot 20.
    let capacity = table.field_offset_capacity();
    let compatibility_base = table
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, member)| member.as_ref().and_then(|_| table.native_field_slot(index as u32)))
        .max()
        .map(|slot| slot + 1)
        .unwrap_or(0);

    let mut field_bindings = Vec::new();

    for row in 0..capacity {
        let slot = match table.native_field_slot(row) {
            Some(slot) => {
                let member = table.fields[row as usize].as_ref().expect("resolved field row");

                field_bindings.push(super::handles::JavaFieldBinding {
                    class_name: table.class_name(member.class_index).into(),
                    name: member.name.clone(),
                    descriptor: member.descriptor.clone(),
                    slot,
                });

                slot
            }
            None => compatibility_base + row,
        };

        write_generic(core, table.outputs.field_offsets + row * 2, slot as u16)?;
    }

    handles.set_field_bindings(field_bindings);
    handles.set_field_slots(compatibility_base + capacity);

    tracing::debug!(
        "LGT field slots: {capacity} rows, native base {compatibility_base}"
    );

    Ok(())
}

pub async fn java_unk9(_core: &mut ArmCore, _: &mut (), a0: u32) -> Result<()> {
    tracing::warn!("java_unk9({a0:#x})");

    Ok(())
}

/// Import `0x83`. The application asks the platform to run a Java main class,
/// passing the Jlet's own class name as the first argument - the same shape
/// KTF uses. The named class is one of the application's own compiled classes,
/// so a bridge is registered for it before `Main` is entered.
#[allow(clippy::too_many_arguments)]
pub async fn java_unk11(
    core: &mut ArmCore,
    jvm: &mut Jvm,
    handles: &JavaHandles,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    argc: u32,
    argv: u32,
) -> Result<u32> {
    let argc = argc.min(MAX_MAIN_ARGUMENTS);

    let mut arguments = Vec::with_capacity(argc as usize);
    for index in 0..argc {
        let pointer: u32 = read_generic(core, argv + index * 4)?;
        let bytes = read_null_terminated_string_bytes(core, pointer)?;

        arguments.push(String::from_utf8_lossy(&bytes).into_owned());
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

pub async fn java_unk12(core: &mut ArmCore, _: &mut (), a0: u32) -> Result<()> {
    tracing::warn!("java_unk12({a0:#x})");

    let mut bytes = [0u8; 128];

    match core.read_bytes(a0, &mut bytes) {
        Ok(read) => {
            tracing::warn!("java_unk12 classes @{a0:#x}, read={read:#x}: {:02x?}", &bytes[..read]);
        }
        Err(error) => {
            tracing::warn!("java_unk12 classes @{a0:#x}: read failed: {error}");
        }
    }

    let a1 = 0x01400ac4u32;
    let mut metadata = [0u8; 512];

    match core.read_bytes(a1, &mut metadata) {
        Ok(read) => {
            tracing::warn!("java_unk12 metadata @{a1:#x}, read={read:#x}: {:02x?}", &metadata[..read]);
        }
        Err(error) => {
            tracing::warn!("java_unk12 metadata @{a1:#x}: read failed: {error}");
        }
    }

    let lm_address = 0x01400000u32;
    let mut lm_metadata = [0u8; 2048];

    match core.read_bytes(lm_address, &mut lm_metadata) {
        Ok(read) => {
            tracing::warn!("java_unk12 Lm metadata @{lm_address:#x}, read={read:#x}: {:02x?}", &lm_metadata[..read]);
        }
        Err(error) => {
            tracing::warn!("java_unk12 Lm metadata @{lm_address:#x}: read failed: {error}");
        }
    }

    Ok(())
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
    let element_size = match array_class.lock().get(&class).copied() {
        Some(info) => info.element_size,
        None => {
            tracing::warn!("vm_instantiate_array({class:#x}, {length}) names no array class; assuming references");

            REFERENCE_SIZE
        }
    };

    let array = handles.allocate_array(handles.fallback_dispatch_table(), length, element_size)?;

    tracing::debug!("vm_instantiate_array({class:#x}, {length}) -> {array:#x}, {element_size} bytes an element");

    Ok(array)
}

pub async fn java_import_11(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("java_import_11(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");
    Ok(0)
}

pub async fn java_import_23(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("java_import_23(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");
    Ok(0)
}
