use alloc::{string::String, vec, vec::Vec};

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
    },
    svc_ids::InitSvcId,
};

/// Guard on the argument vector the application passes to `Main.main`.
const MAX_MAIN_ARGUMENTS: u32 = 16;

/// Diagnostic SVC range used for unresolved LGT Java-interface imports.
/// The low 12 bits preserve the original function index.
pub const JAVA_DIAG_SVC_BASE: u32 = 0x1000;

/// One SVC id per row of the application's static method table. The low bits
/// carry the row index, which is how the handler knows what was called.
pub const JAVA_STATIC_METHOD_SVC_BASE: u32 = 0x2000;

/// One SVC id per row of the virtual method table. The stub sits in a class's
/// dispatch table, so the receiver arrives in the first word.
pub const JAVA_VIRTUAL_METHOD_SVC_BASE: u32 = 0x4000;

/// One SVC id per row of the static method table that carries no descriptor.
/// The compiled code calls these too, so they cannot be left null.
pub const JAVA_RESERVED_SLOT_SVC_BASE: u32 = 0x3000;

/// Upper bound on rows, so a table that fails to parse cannot run off the end
/// of its SVC range.
pub const JAVA_METHOD_SVC_LIMIT: u32 = 0x1000;

/// Instance layout for platform objects the application allocates itself:
/// a vtable pointer, the class index, then the fields.
pub const INSTANCE_FIELD_BASE: u32 = 8;

pub fn get_java_interface_method(core: &mut ArmCore, function_index: u32) -> Result<u32> {
    Ok(match function_index {
        0x03 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk0)?,
        0x06 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk12)?,
        0x07 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk5)?,
        0x09 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport09)?,
        0x0e => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport0e)?,
        0x10 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport10)?,
        0x11 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport11)?,
        0x23 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaImport23)?,
        0x13 | 0x14 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaLoadClasses)?,
        0x82 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk9)?,
        0x83 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaUnk11)?,
        _ => {
            tracing::warn!("Unimplemented LGT Java import {function_index:#x}; installing diagnostic zero-return stub");
            core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + function_index)?
        }
    })
}

pub async fn java_unk0(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32) -> Result<()> {
    tracing::warn!("java_unk0({a0:#x}, {a1:#x}, {a2:#x})");
    Ok(())
}

/// Import `0x14`. Reads the tables describing every platform class the
/// application imports, then fills the output arrays so the compiled code can
/// reach them.
#[allow(clippy::too_many_arguments)]
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
    let _ = handles;

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
        "java_load_classes: {} classes, {} static methods, {} virtual methods; outputs at \
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

    install_dispatch(core, &mut table)?;

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
fn build_dispatch_tables(core: &mut ArmCore, table: &mut ClassTable) -> Result<()> {
    for index in 0..table.classes.len() as u32 {
        let (start, count) = {
            let class = &table.classes[index as usize];
            (class.virtual_method_start, class.virtual_method_count)
        };

        let vtable = if count == 0 {
            0
        } else {
            let vtable = Allocator::alloc(core, (count + 1) * 4)?;
            write_generic(core, vtable, 0u32)?;

            for slot in 0..count {
                let stub = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_VIRTUAL_METHOD_SVC_BASE + start + slot)?;
                write_generic(core, vtable + 4 + slot * 4, stub)?;
            }

            vtable
        };

        // Eight bytes is the smallest allocation that reads back distinctly;
        // nothing inspects the contents, the address is the identity.
        let class_object = Allocator::alloc(core, 8)?;
        write_generic(core, class_object, 0u32)?;
        write_generic(core, class_object + 4, index)?;

        tracing::trace!(
            "LGT class {} -> object {class_object:#x}, dispatch table {vtable:#x} ({count} slots)",
            table.classes[index as usize].name
        );

        table.vtables.push(vtable);
        table.class_objects.push(class_object);
    }

    Ok(())
}

/// Publishes the table into the arrays the compiled code reads.
///
/// Rows the application left blank are skipped: it reserves two at the head of
/// every class's static method block, and what belongs there is not yet known.
fn install_dispatch(core: &mut ArmCore, table: &mut ClassTable) -> Result<()> {
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

    // Virtual methods are dispatched through the receiver, so the array holds
    // the slot to index its vtable with rather than an address. The compiled
    // code reads it with `ldrsh`, so the entries are signed halfwords.
    for index in 0..table.virtual_methods.len() as u32 {
        let Some(member) = table.virtual_methods[index as usize].as_ref() else {
            continue;
        };
        let Some(slot) = table.virtual_slot(index) else { continue };

        write_generic(core, table.outputs.virtual_method_offsets + index * 2, slot as u16)?;

        tracing::trace!("LGT virtual method[{index}] {} -> slot {slot}", table.describe(member));
    }

    build_dispatch_tables(core, table)?;

    for (index, member) in table.fields.iter().enumerate() {
        let Some(member) = member else { continue };

        let class = &table.classes[member.class_index as usize];
        let offset = INSTANCE_FIELD_BASE + (index as u32 - class.field_start) * 4;

        write_generic(core, table.outputs.field_offsets + index as u32 * 2, offset as u16)?;

        tracing::trace!("LGT field[{index}] {} -> +{offset:#x}", table.describe(member));
    }

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
        register_app_class(jvm, core, handles, app_classes, image_ranges, main_class_name).await;
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

/// Registers a JVM stand-in for one of the application's compiled classes, so
/// the platform's `Jlet` machinery can construct and drive it.
async fn register_app_class(
    jvm: &Jvm,
    core: &ArmCore,
    handles: &JavaHandles,
    app_classes: &Mutex<Vec<AppClass>>,
    image_ranges: &[(u32, u32)],
    name: &str,
) {
    // The definition is built while the lock is held, and the lock is let go
    // before the JVM runs: registering re-enters the runtime.
    //
    // The main class is usually absent from the registered table, so fall back
    // to finding it by shape in the image.
    let proto = {
        let app_classes = app_classes.lock();

        app_classes.iter().find(|x| x.name == name).map(compiled_class::as_proto)
    };
    let proto = match proto {
        Some(proto) => Some(proto),
        None => app_classes::find_class(core, image_ranges, name).map(|class| compiled_class::as_proto(&class)),
    };

    let Some(proto) = proto else {
        tracing::error!("Application class {name} is nowhere in the image; cannot bridge it");
        return;
    };

    let context = CompiledContext {
        core: core.clone(),
        handles: handles.clone(),
    };

    compiled_class::register(jvm, &context, name, proto).await;
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

pub async fn java_import_09(core: &mut ArmCore, handles: &JavaHandles, jvm: &mut Jvm, _a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    let mut bytes = vec![0u8; (a2 as usize) * 2];
    core.read_bytes(a1, &mut bytes)?;

    let utf16 = bytes.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
    let rust_string = String::from_utf16_lossy(&utf16);

    let java_string = match JavaLangString::from_rust_string(jvm, &rust_string).await {
        Ok(value) => value,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    let handle = handles.insert(java_string)?;
    write_generic(core, a3, handle)?;

    tracing::debug!("java_import_09 created {rust_string:?}, handle={handle:#x}, output={a3:#x}");

    Ok(0)
}

pub async fn java_import_10(core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    let (pc, lr) = core.read_pc_lr()?;

    tracing::warn!("java_import_10(pc={pc:#x}, lr={lr:#x}, a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");

    Ok(0)
}

pub async fn java_import_11(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("java_import_11(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");
    Ok(0)
}

pub async fn java_import_23(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("java_import_23(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x})");
    Ok(0)
}
