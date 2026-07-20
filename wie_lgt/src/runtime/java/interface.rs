use alloc::{string::String, vec::Vec};

use jvm::Jvm;

use wie_core_arm::ArmCore;
use wie_util::{ByteRead, Result};

use crate::runtime::{SVC_CATEGORY_INIT, svc_ids::InitSvcId};

/// Diagnostic SVC range used for unresolved LGT Java-interface imports.
/// The low 12 bits preserve the original function index.
pub const JAVA_DIAG_SVC_BASE: u32 = 0x1000;

pub fn get_java_interface_method(core: &mut ArmCore, function_index: u32) -> Result<u32> {
    Ok(match function_index {
        0x03 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk0)?,
        0x06 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk12)?,
        0x07 => core.make_svc_stub(SVC_CATEGORY_INIT, InitSvcId::JavaInterfaceUnk5)?,
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

pub async fn java_unk5(_core: &mut ArmCore, _: &mut (), a0: u32, a1: u32) -> Result<()> {
    tracing::warn!("java_unk5({a0:#x}, {a1:#x})");

    // a0: class list

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn java_load_classes(
    _core: &mut ArmCore,
    _: &mut (),
    classes: u32,
    fields: u32,
    static_fields: u32,
    virtual_methods: u32,
    a4: u32,
    static_methods: u32,
    field_offsets: u32,
    static_field_offsets: u32,
    virtual_method_offsets: u32,
    a9: u32,
    static_method_offsets: u32,
) -> Result<()> {
    tracing::debug!(
        "java_load_classes({classes:#x}, {fields:#x}, {static_fields:#x}, {virtual_methods:#x}, {a4:#x}, {static_methods:#x}, {field_offsets:#x}, {static_field_offsets:#x}, {virtual_method_offsets:#x}, {a9:#x}, {static_method_offsets:#x})"
    );
    for (name, address) in [
        ("classes", classes),
        ("fields", fields),
        ("static_fields", static_fields),
        ("virtual_methods", virtual_methods),
        ("a4", a4),
        ("static_methods", static_methods),
        ("field_offsets", field_offsets),
        ("static_field_offsets", static_field_offsets),
        ("virtual_method_offsets", virtual_method_offsets),
        ("a9", a9),
        ("static_method_offsets", static_method_offsets),
    ] {
        let mut bytes = [0u8; 64];

        match _core.read_bytes(address, &mut bytes) {
            Ok(read) => {
                tracing::warn!("java_load_classes {name} @{address:#x}, read={read:#x}: {:02x?}", &bytes[..read]);
            }
            Err(error) => {
                tracing::warn!("java_load_classes {name} @{address:#x}: read failed: {error}");
            }
        }
    }

    let mut class_count_bytes = [0u8; 4];
    _core.read_bytes(classes, &mut class_count_bytes)?;
    let class_count = u32::from_le_bytes(class_count_bytes).min(64);

    tracing::warn!("java_load_classes class_count={class_count}");

    for index in 0..class_count {
        // classes + 4 뒤부터 클래스당 6개의 u32, 즉 24바이트
        let entry_address = classes.wrapping_add(4 + index * 24);

        let mut entry_bytes = [0u8; 24];

        if let Err(error) = _core.read_bytes(entry_address, &mut entry_bytes) {
            tracing::warn!(
                "java_load_classes class[{index}] entry read failed \
             @{entry_address:#x}: {error}"
            );
            continue;
        }

        let name_pointer = u32::from_le_bytes([entry_bytes[0], entry_bytes[1], entry_bytes[2], entry_bytes[3]]);

        let mut name_bytes = [0u8; 128];

        match _core.read_bytes(name_pointer, &mut name_bytes) {
            Ok(read) => {
                let end = name_bytes[..read].iter().position(|&value| value == 0).unwrap_or(read);

                tracing::warn!(
                    "java_load_classes class[{index}] entry={entry_address:#x}, \
                 name_ptr={name_pointer:#x}, name={}, raw={:02x?}",
                    String::from_utf8_lossy(&name_bytes[..end]),
                    entry_bytes
                );
            }
            Err(error) => {
                tracing::warn!(
                    "java_load_classes class[{index}] entry={entry_address:#x}, \
                 name_ptr={name_pointer:#x}: name read failed: {error}, \
                 raw={:02x?}",
                    entry_bytes
                );
            }
        }
    }

    Ok(())
}

pub async fn java_unk9(_core: &mut ArmCore, _: &mut (), a0: u32) -> Result<()> {
    tracing::warn!("java_unk9({a0:#x})");

    Ok(())
}

pub async fn java_unk11(core: &mut ArmCore, _jvm: &mut Jvm, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("java_unk11({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");
    tracing::warn!("java_unk11 class_ptr={a0:#x}, argc={a2}, argv={a3:#x}");

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
                            .chunks_exact(4)
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

    Ok(0)
}

pub async fn java_unk12(_core: &mut ArmCore, _: &mut (), a0: u32) -> Result<()> {
    tracing::warn!("java_unk12({a0:#x})");

    Ok(())
}
