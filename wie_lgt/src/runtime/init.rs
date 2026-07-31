use alloc::{collections::BTreeMap, format, sync::Arc, vec};
use core::mem::size_of;

use elf::{ElfBytes, endian::AnyEndian};

use jvm::Jvm;
use spin::Mutex;
use wipi_types::lgt::{InitParam1, InitParam2, InitStruct};

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId};
use wie_util::{ByteRead, Result, WieError, read_generic, write_generic};

use crate::relocation::{
    R_ARM_ABS32, R_ARM_CALL, R_ARM_JUMP24, R_ARM_NONE, R_ARM_PC24, R_ARM_RABS32, R_ARM_RBASE, R_ARM_REL32, R_ARM_RPC24, R_ARM_RREL32, R_ARM_THM_CALL,
    R_ARM_THM_JUMP24, arm_abs32, arm_pc24, arm_rel32, raptor_rabs32, raptor_rpc24, raptor_rrel32, thumb_pc22,
};

use super::{
    SVC_CATEGORY_INIT, SVC_CATEGORY_STDLIB, SVC_CATEGORY_WIPIC,
    java::{
        get_java_interface_method,
        interface::{
            JAVA_DIAG_SVC_BASE, JavaHandleTable, java_import_09, java_import_10, java_import_11, java_import_23, java_load_classes, java_unk0,
            java_unk9, java_unk11, java_unk12,
        },
    },
    stdlib::register_stdlib_svc_handler,
    svc_ids::InitSvcId,
    wipi_c::register_wipic_svc_handler,
};

type JavaClassTables = Arc<Mutex<BTreeMap<u32, (u32, u32)>>>;
type JavaActivatedClasses = Arc<Mutex<BTreeMap<u32, u32>>>;
type ImportFunctionCache = Arc<Mutex<BTreeMap<(u32, u32), u32>>>;
type UnresolvedImportCallCounts = Arc<Mutex<BTreeMap<(u32, u32), u64>>>;

const UNRESOLVED_IMPORT_SVC_BASE: u32 = 0x1000_0000;
const UNRESOLVED_IMPORT_FIELD_MASK: u32 = 0x0fff;

#[derive(Clone)]
struct InitSvcContext {
    wipic_category: u32,
    stdlib_category: u32,
    jvm: Jvm,
    java_handles: JavaHandleTable,
    java_class_tables: JavaClassTables,
    java_activated_classes: JavaActivatedClasses,
    import_function_cache: ImportFunctionCache,
    unresolved_import_call_counts: UnresolvedImportCallCounts,
}

fn register_init_svc_handler(core: &mut ArmCore, jvm: &Jvm) -> Result<()> {
    core.register_svc_handler(
        SVC_CATEGORY_INIT,
        handle_init_svc,
        &InitSvcContext {
            wipic_category: SVC_CATEGORY_WIPIC,
            stdlib_category: SVC_CATEGORY_STDLIB,
            jvm: jvm.clone(),
            java_handles: Default::default(),
            java_class_tables: Default::default(),
            java_activated_classes: Default::default(),
            import_function_cache: Default::default(),
            unresolved_import_call_counts: Default::default(),
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

            let activated = Allocator::alloc(core, 12)?;
            write_generic(core, activated, 0u32)?;
            write_generic(core, activated + 4, 0u32)?;
            write_generic(core, activated + 8, data)?;

            context.java_activated_classes.lock().insert(root, activated);

            tracing::warn!("LGT vm_activate_class(root={root:#x}, table={a1:#x}) -> handle={activated:#x}, data={data:#x}");

            activated.write(core, lr)?;
            return Ok(());
        }
        if function_index == 0x0d {
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

        if function_index == 0x104 {
            let mut original = [0u8; 0x80];
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
            let original_word0: u32 = read_generic(core, a0)?;
            let original_word4: u32 = read_generic(core, a0 + 4)?;
            let original_word8: u32 = read_generic(core, a0 + 8)?;
            let original_wordc: u32 = read_generic(core, a0 + 12)?;

            tracing::warn!(
                "LGT callback original words: object={a0:#x}, \
                 +0={original_word0:#x}, +4={original_word4:#x}, \
                 +8={original_word8:#x}, +c={original_wordc:#x}"
            );

            for (name, pointer) in [
                ("word0", original_word0),
                ("word4", original_word4),
                ("word8", original_word8),
                ("wordc", original_wordc),
            ] {
                if pointer >= 0x1000 {
                    let address = pointer & !1;
                    let mut bytes = [0u8; 0x100];

                    match core.read_bytes(address, &mut bytes) {
                        Ok(read) => tracing::warn!(
                            "LGT callback linked block {name}: \
                             pointer={pointer:#x}, address={address:#x}, \
                             read={read:#x}, bytes={:02x?}",
                            &bytes[..read]
                        ),
                        Err(error) => tracing::warn!(
                            "LGT callback linked block {name}: \
                             pointer={pointer:#x}, address={address:#x}, \
                             read failed: {error}"
                        ),
                    }
                }
            }

            if original_word8 >= 0x1000 {
                match read_generic::<u32, _>(core, original_word8 + 8) {
                    Ok(level2) => {
                        tracing::warn!(
                            "LGT callback pointer chain: \
                             object+8={original_word8:#x}, \
                             [object+8]+8={level2:#x}"
                        );

                        if level2 >= 0x1000 {
                            match read_generic::<u32, _>(core, level2 + 8) {
                                Ok(level3) => tracing::warn!(
                                    "LGT callback pointer chain: \
                                     level2+8={level3:#x}"
                                ),
                                Err(error) => tracing::warn!("LGT callback pointer chain level3 failed: {error}"),
                            }
                        }
                    }
                    Err(error) => tracing::warn!("LGT callback pointer chain level2 failed: {error}"),
                }
            }

            let method_ref_base = 0x01500e00u32;
            let mut method_ref_bytes = [0u8; 0x100];

            match core.read_bytes(method_ref_base, &mut method_ref_bytes) {
                Ok(read) => tracing::warn!(
                    "Lm method reference block before patch: \
                     address={method_ref_base:#x}, read={read:#x}, bytes={:02x?}",
                    &method_ref_bytes[..read]
                ),
                Err(error) => tracing::warn!(
                    "Lm method reference block read failed: \
                     address={method_ref_base:#x}, error={error}"
                ),
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
        if function_index == 0xfc {
            let lm_class_handle = 0x014015dcu32;
            tracing::warn!("Lm class getter(a0={a0:#x}) -> {lm_class_handle:#x}");
            lm_class_handle.write(core, lr)?;
            return Ok(());
        }

        if function_index == 0x0f {
            let instance = Allocator::alloc(core, 12)?;
            write_generic(core, instance, 0u32)?;
            write_generic(core, instance + 4, 0u32)?;
            write_generic(core, instance + 8, a0)?;

            tracing::warn!("Lm vm_instantiate(class={a0:#x}) -> instance={instance:#x}");

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
        InitSvcId::JavaInterfaceUnk5 => {
            let classes = core.read_param(0)?;
            let runtime_table = core.read_param(1)?;
            let mut tables = context.java_class_tables.lock();
            let index = (0u32..).find(|index| !tables.contains_key(index)).unwrap();
            tables.insert(index, (classes, runtime_table));
            tracing::warn!("java_register_classes(classes={classes:#x}, runtime_table={runtime_table:#x}) -> {index:#x}");
            index.write(core, lr)
        }
        InitSvcId::JavaLoadClasses => EmulatedFunction::call(&java_load_classes, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk9 => EmulatedFunction::call(&java_unk9, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaUnk11 => EmulatedFunction::call(&java_unk11, core, jvm).await?.write(core, lr),
        InitSvcId::JavaImport09 => {
            let a0 = core.read_param(0)?;
            let a1 = core.read_param(1)?;
            let a2 = core.read_param(2)?;
            let a3 = core.read_param(3)?;

            let result = java_import_09(core, &mut context.java_handles, jvm, a0, a1, a2, a3).await?;

            result.write(core, lr)
        }
        InitSvcId::JavaImport0e => {
            let a0 = core.read_param(0)?;
            let a1 = core.read_param(1)?;
            let a2 = core.read_param(2)?;
            let a3 = core.read_param(3)?;

            let classes = context.java_class_tables.lock().values().next().map(|(classes, _)| *classes).unwrap_or(0);

            let class = if classes == 0 {
                0
            } else {
                let count: u32 = read_generic(core, classes)?;
                if a2 < count { read_generic(core, classes + 8 + a2 * 4)? } else { 0 }
            };

            tracing::warn!("java_import_0e(a0={a0:#x}, a1={a1:#x}, a2={a2:#x}, a3={a3:#x}) -> {class:#x}");
            class.write(core, lr)
        }
        InitSvcId::JavaImport10 => EmulatedFunction::call(&java_import_10, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaImport11 => EmulatedFunction::call(&java_import_11, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaImport23 => EmulatedFunction::call(&java_import_23, core, &mut ()).await?.write(core, lr),
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

    let mut lm_runtime_probe = [0u8; 4];
    let has_lm_runtime = core.read_bytes(0x015009e4, &mut lm_runtime_probe).is_ok();

    if has_lm_runtime {
        for address in [
            0x015009e4u32,
            0x015009ec,
            0x015009f0,
            0x015009f8,
            0x01500a50,
            0x01500a58,
            0x01500a5c,
            0x01500a64,
            0x01500a68,
            0x01500a70,
        ] {
            let value: u32 = read_generic(core, address)?;
            tracing::warn!("Lm runtime slot before patch [{address:#x}] = {value:#x}");
        }

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
        tracing::warn!("Installed minimal Lm runtime stubs: +0xfc={lm_stub_fc:#x},          +0x108={lm_stub_108:#x}, +0x110={lm_stub_110:#x}");

        tracing::warn!(
            "Lm runtime stub installation temporarily disabled:          84={lm_stub_84:#x}, 8c={lm_stub_8c:#x}, 90={lm_stub_90:#x},          98={lm_stub_98:#x}, f0={lm_stub_f0:#x}, f8={lm_stub_f8:#x},          fc={lm_stub_fc:#x}, 104={lm_stub_104:#x},          108={lm_stub_108:#x}, 110={lm_stub_110:#x}"
        );

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
    }

    tracing::debug!("Calling initializer at {:#x}", init_struct.fn_init);
    let _: () = core.run_function(init_struct.fn_init, &[]).await?;

    if has_lm_runtime {
        for address in [
            0x015009e4u32,
            0x015009ec,
            0x015009f0,
            0x015009f8,
            0x01500a50,
            0x01500a58,
            0x01500a5c,
            0x01500a64,
            0x01500a68,
            0x01500a70,
        ] {
            let value: u32 = read_generic(core, address)?;
            tracing::warn!("Lm runtime slot after fn_init [{address:#x}] = {value:#x}");
        }

        for offset in (0..0x30).step_by(4) {
            let address = 0x01500e40 + offset;
            let value: u32 = read_generic(core, address)?;
            tracing::warn!("Lm runtime data [{address:#x}] = {value:#x}");
        }
    }

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
    let section_headers: alloc::vec::Vec<_> = shdrs.iter().collect();

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
        }
    }

    apply_relocations(core, data, &section_headers)?;

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
