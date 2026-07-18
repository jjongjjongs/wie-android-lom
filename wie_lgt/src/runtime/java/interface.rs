use alloc::string::String;

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

    Ok(())
}

pub async fn java_unk9(_core: &mut ArmCore, _: &mut (), a0: u32) -> Result<()> {
    tracing::warn!("java_unk9({a0:#x})");

    Ok(())
}

pub async fn java_unk11(core: &mut ArmCore, _: &mut (), a0: u32, a1: u32, a2: u32, a3: u32) -> Result<()> {
    tracing::warn!("java_unk11({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");
    tracing::warn!("java_unk11 class_ptr={a0:#x}, argc={a2}, argv={a3:#x}");

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
    Ok(())
}

pub async fn java_unk12(_core: &mut ArmCore, _: &mut (), a0: u32) -> Result<()> {
    tracing::warn!("java_unk12({a0:#x})");

    Ok(())
}
