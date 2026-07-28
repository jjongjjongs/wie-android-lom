use alloc::{boxed::Box, string::String, sync::Arc, vec, vec::Vec};
use core::sync::atomic::AtomicU32;

use jvm::{Jvm, Result as JvmResult, runtime::JavaLangString};
use jvm_rust::ClassDefinitionImpl;
use wie_core_arm::ArmCore;
use wie_jvm_support::JvmSupport;
use wie_util::{ByteRead, Result, write_generic};

use crate::runtime::{
    SVC_CATEGORY_INIT,
    java::{
        class_table::{ClassTable, OutputArrays},
        classes::lm::{Lm, LmContext},
        handles::JavaHandles,
    },
    svc_ids::InitSvcId,
};

/// Diagnostic SVC range used for unresolved LGT Java-interface imports.
/// The low 12 bits preserve the original function index.
pub const JAVA_DIAG_SVC_BASE: u32 = 0x1000;

/// One SVC id per row of the application's static method table. The low bits
/// carry the row index, which is how the handler knows what was called.
pub const JAVA_STATIC_METHOD_SVC_BASE: u32 = 0x2000;

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
        0x14 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaLoadClasses)?,
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

    let table = ClassTable::parse(
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

    install_dispatch(core, &table)?;

    Ok(table)
}

/// Publishes the table into the arrays the compiled code reads.
///
/// Rows the application left blank are skipped: it reserves two at the head of
/// every class's static method block, and what belongs there is not yet known.
fn install_dispatch(core: &mut ArmCore, table: &ClassTable) -> Result<()> {
    if table.static_methods.len() as u32 > JAVA_METHOD_SVC_LIMIT {
        return Err(wie_util::WieError::FatalError(alloc::format!(
            "LGT static method table has {} rows, more than the {JAVA_METHOD_SVC_LIMIT} reserved",
            table.static_methods.len()
        )));
    }

    for (index, member) in table.static_methods.iter().enumerate() {
        let Some(member) = member else { continue };

        let stub = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_STATIC_METHOD_SVC_BASE + index as u32)?;
        write_generic(core, table.outputs.static_method_offsets + index as u32 * 4, stub)?;

        tracing::trace!("LGT static method[{index}] {} -> {stub:#x}", table.describe(member));
    }

    // Virtual methods are dispatched through the receiver, so the array holds
    // the slot to index its vtable with rather than an address. The entries are
    // halfwords: the array is only large enough for one per row at that width.
    for (index, member) in table.virtual_methods.iter().enumerate() {
        let Some(member) = member else { continue };
        let Some(slot) = table.virtual_slot(index as u32) else { continue };

        write_generic(core, table.outputs.virtual_method_offsets + index as u32 * 2, slot as u16)?;

        tracing::trace!("LGT virtual method[{index}] {} -> slot {slot}", table.describe(member));
    }

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

pub async fn java_unk11(core: &mut ArmCore, jvm: &mut Jvm, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("java_unk11({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");
    tracing::warn!("java_unk11 class_ptr={a0:#x}, argc={a2}, argv={a3:#x}");

    let lm_class = ClassDefinitionImpl::from_class_proto(
        Lm::as_proto(),
        Box::new(LmContext {
            core: core.clone(),
            native_this: Arc::new(AtomicU32::new(0)),
        }) as Box<_>,
    );

    match jvm.register_class(Box::new(lm_class), None).await {
        Ok(_) => {
            tracing::warn!("Registered Lm stub class");
        }
        Err(error) => {
            tracing::warn!("Lm stub class registration failed: {error:?}");
        }
    }

    let mut argv_raw = [0u8; 64];
    if core.read_bytes(a3, &mut argv_raw).is_ok() {
        tracing::warn!("java_unk11 argv raw @{a3:#x}: {argv_raw:02x?}");
    }

    let mut class_bytes = [0u8; 128];
    match core.read_bytes(a0, &mut class_bytes) {
        Ok(read) => {
            let end = class_bytes[..read].iter().position(|&value| value == 0).unwrap_or(read);

            tracing::warn!("java_unk11 class: {}", String::from_utf8_lossy(&class_bytes[..end]));
        }
        Err(error) => {
            tracing::warn!("java_unk11 class read failed: {error}");
        }
    }

    let argc = a2.min(16);

    for index in 0..argc {
        let pointer_address = a3.wrapping_add(index * 4);
        let mut pointer_bytes = [0u8; 4];

        if let Err(error) = core.read_bytes(pointer_address, &mut pointer_bytes) {
            tracing::warn!("java_unk11 argv[{index}] pointer read failed @{pointer_address:#x}: {error}");
            continue;
        }

        let pointer = u32::from_le_bytes(pointer_bytes);

        let mut argument_bytes = [0u8; 128];
        match core.read_bytes(pointer, &mut argument_bytes) {
            Ok(read) => {
                let end = argument_bytes[..read].iter().position(|&value| value == 0).unwrap_or(read);

                tracing::warn!(
                    "java_unk11 argv[{index}] ptr={pointer:#x}: {} | raw={:02x?}",
                    String::from_utf8_lossy(&argument_bytes[..end]),
                    &argument_bytes[..end]
                );
            }
            Err(error) => {
                tracing::warn!("java_unk11 argv[{index}] ptr={pointer:#x}: read failed: {error}");
            }
        }
    }

    // invoke static? used to be called with org/kwis/msp/lcdui/Main

    // Diagnostic mode: keep the ARM application alive so the next missing
    // interface call can be observed. A real JVM bridge will replace this.
    for address in [0x01500954u32, 0x01500958u32] {
        let mut value_bytes = [0u8; 4];

        match core.read_bytes(address, &mut value_bytes) {
            Ok(_) => {
                let value = u32::from_le_bytes(value_bytes);
                let mut object_bytes = [0u8; 32];

                match core.read_bytes(value, &mut object_bytes) {
                    Ok(_) => {
                        let words: Vec<u32> = object_bytes
                            .chunks(4)
                            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                            .collect();

                        tracing::warn!("java_unk11 slot target @{value:#x}: {words:#x?}");
                    }
                    Err(error) => {
                        tracing::warn!("java_unk11 slot target @{value:#x} read failed: {error}");
                    }
                }

                tracing::warn!("java_unk11 global slot @{address:#x} = {value:#x}, thumb={}", value & 1);
                for target in [0x71000001u32, 0x71000000u32, 0x71000011u32, 0x71000010u32] {
                    let mut bytes = [0u8; 16];

                    match core.read_bytes(target, &mut bytes) {
                        Ok(_) => {
                            tracing::warn!("java_unk11 candidate @{target:#x}: {bytes:02x?}");
                        }
                        Err(error) => {
                            tracing::warn!("java_unk11 candidate @{target:#x} read failed: {error}");
                        }
                    }
                }
            }
            Err(error) => {
                tracing::warn!("java_unk11 global slot @{address:#x} read failed: {error}");
            }
        }
    }
    let mut args_array = match jvm.instantiate_array("Ljava/lang/String;", argc as usize).await {
        Ok(array) => array,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    for index in 0..argc {
        let pointer_address = a3.wrapping_add(index * 4);
        let mut pointer_bytes = [0u8; 4];
        core.read_bytes(pointer_address, &mut pointer_bytes)?;

        let pointer = u32::from_le_bytes(pointer_bytes);
        let mut argument_bytes = [0u8; 128];
        let read = core.read_bytes(pointer, &mut argument_bytes)?;
        let end = argument_bytes[..read].iter().position(|&value| value == 0).unwrap_or(read);

        let argument = String::from_utf8_lossy(&argument_bytes[..end]);
        let java_argument = match JavaLangString::from_rust_string(jvm, argument.as_ref()).await {
            Ok(argument) => argument,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        if let Err(error) = jvm.store_array(&mut args_array, index as usize, vec![java_argument]).await {
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
