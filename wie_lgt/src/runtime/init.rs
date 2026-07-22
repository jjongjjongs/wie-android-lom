use alloc::format;
use core::mem::size_of;

use elf::{ElfBytes, endian::AnyEndian};

use jvm::Jvm;
use wipi_types::lgt::{InitParam1, InitParam2, InitStruct};

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId};
use wie_util::{Result, WieError, read_generic, write_generic};

use super::{
    SVC_CATEGORY_INIT, SVC_CATEGORY_STDLIB, SVC_CATEGORY_WIPIC,
    java::{
        get_java_interface_method,
        interface::{JAVA_DIAG_SVC_BASE, java_load_classes, java_unk0, java_unk5, java_unk9, java_unk11, java_unk12},
    },
    stdlib::register_stdlib_svc_handler,
    svc_ids::InitSvcId,
    wipi_c::register_wipic_svc_handler,
};

fn register_init_svc_handler(core: &mut ArmCore, jvm: &Jvm) -> Result<()> {
    core.register_svc_handler(
        SVC_CATEGORY_INIT,
        handle_init_svc,
        &(SVC_CATEGORY_WIPIC, SVC_CATEGORY_STDLIB, jvm.clone()),
    )
}

async fn handle_init_svc(core: &mut ArmCore, (wipic_category, stdlib_category, jvm): &mut (u32, u32, Jvm), id: SvcId) -> Result<()> {
    let (_, lr) = core.read_pc_lr()?;

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
        if function_index == 0x54 {
            let address = Allocator::alloc(core, a0)?;
            tracing::warn!("lgt_java_alloc(size={a0:#x}) -> {address:#x}");
            address.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x104 {
            let vtable = Allocator::alloc(core, 8)?;
            let method_stub = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x105)?;

            write_generic(core, vtable, 0u32)?;
            write_generic(core, vtable + 4, method_stub)?;
            write_generic(core, a0, vtable)?;
            let object_word: u32 = read_generic(core, a0)?;
            let vtable_word0: u32 = read_generic(core, vtable)?;
            let vtable_word1: u32 = read_generic(core, vtable + 4)?;

            tracing::warn!(
                "Lm runtime object readback: object[0]={object_word:#x}, \
     vtable[0]={vtable_word0:#x}, vtable[1]={vtable_word1:#x}"
            );

            tracing::warn!(
                "Lm runtime object initialized: object={a0:#x}, \
         vtable={vtable:#x}, method={method_stub:#x}"
            );

            a0.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x105 {
            tracing::warn!("Lm virtual method stub(a0={a0:#x})");
            a0.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x0f
            || function_index == 0xf0
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
        InitSvcId::JavaInterfaceUnk5 => EmulatedFunction::call(&java_unk5, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaLoadClasses => EmulatedFunction::call(&java_load_classes, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk9 => EmulatedFunction::call(&java_unk9, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk11 => EmulatedFunction::call(&java_unk11, core, jvm).await?.write(core, lr),
    }
}
pub async fn load_native(core: &mut ArmCore, system: &mut System, jvm: &Jvm, data: &[u8]) -> Result<()> {
    let entrypoint = load_executable(core, data)?;
    register_wipic_svc_handler(core, system, jvm)?;
    register_stdlib_svc_handler(core, system)?;
    register_init_svc_handler(core, jvm)?;

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

    let lm_stub_84 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x84)?;
    let lm_stub_8c = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x8c)?;
    let lm_stub_90 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x90)?;
    let lm_stub_98 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x98)?;
    let lm_stub_f0 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0xf0)?;
    let lm_stub_f8 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0xf8)?;
    let lm_stub_fc = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0xfc)?;
    let lm_stub_104 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x104)?;
    let lm_stub_108 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x108)?;
    let lm_stub_110 = core.make_svc_stub(SVC_CATEGORY_INIT, JAVA_DIAG_SVC_BASE + 0x110)?;

    write_generic(core, 0x015009e4, lm_stub_84)?;
    write_generic(core, 0x015009ec, lm_stub_8c)?;
    write_generic(core, 0x015009f0, lm_stub_90)?;
    write_generic(core, 0x015009f8, lm_stub_98)?;
    write_generic(core, 0x01500a50, lm_stub_f0)?;
    write_generic(core, 0x01500a58, lm_stub_f8)?;
    write_generic(core, 0x01500a5c, lm_stub_fc)?;
    write_generic(core, 0x01500a64, lm_stub_104)?;
    write_generic(core, 0x01500a68, lm_stub_108)?;
    write_generic(core, 0x01500a70, lm_stub_110)?;

    tracing::warn!(
        "Installed Lm runtime stubs: [0x015009e4]={lm_stub_84:#x}, \
     [0x015009ec]={lm_stub_8c:#x}, \
     [0x015009f0]={lm_stub_90:#x}, \
     [0x015009f8]={lm_stub_98:#x}, \
     [0x01500a50]={lm_stub_f0:#x}, \
     [0x01500a58]={lm_stub_f8:#x}, \
     [0x01500a5c]={lm_stub_fc:#x}, \
     [0x01500a64]={lm_stub_104:#x}, \
     [0x01500a68]={lm_stub_108:#x}, \
     [0x01500a70]={lm_stub_110:#x}"
    );

    tracing::debug!("Calling initializer at {:#x}", init_struct.fn_init);
    let _: () = core.run_function(init_struct.fn_init, &[]).await?;
    for offset in (0..0x30).step_by(4) {
        let address = 0x01500e40 + offset;
        let value: u32 = read_generic(core, address)?;
        tracing::warn!("Lm runtime data [{address:#x}] = {value:#x}");
    }

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

fn load_executable(core: &mut ArmCore, data: &[u8]) -> Result<u32> {
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
        }
    }

    tracing::debug!("Entrypoint: {:#x}", elf.ehdr.e_entry);

    Ok(elf.ehdr.e_entry as u32)
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
