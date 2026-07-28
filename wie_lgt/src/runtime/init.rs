use alloc::{collections::BTreeMap, format, sync::Arc, vec::Vec};
use core::mem::size_of;

use elf::{ElfBytes, endian::AnyEndian};

use jvm::Jvm;
use spin::Mutex;
use wipi_types::lgt::{InitParam1, InitParam2, InitStruct};

use wie_backend::System;
use wie_core_arm::{Allocator, ArmCore, EmulatedFunction, ResultWriter, SvcId};
use wie_util::{ByteRead, Result, WieError, read_generic, write_generic};

use super::{
    SVC_CATEGORY_INIT, SVC_CATEGORY_STDLIB, SVC_CATEGORY_WIPIC,
    java::{
        app_classes::{self, AppClass},
        class_table::ClassTable,
        get_java_interface_method,
        handles::JavaHandles,
        interface::{
            JAVA_DIAG_SVC_BASE, JAVA_METHOD_SVC_LIMIT, JAVA_STATIC_METHOD_SVC_BASE, java_import_09, java_import_10, java_import_11, java_import_23,
            java_load_classes, java_unk0, java_unk9, java_unk11, java_unk12,
        },
        method_bridge,
    },
    stdlib::register_stdlib_svc_handler,
    svc_ids::InitSvcId,
    wipi_c::register_wipic_svc_handler,
};

type JavaClassTables = Arc<Mutex<BTreeMap<u32, (u32, u32)>>>;
/// Compiled classes the application registered, published by import `0x07`.
type AppClasses = Arc<Mutex<Vec<AppClass>>>;
type JavaActivatedClasses = Arc<Mutex<BTreeMap<u32, u32>>>;
/// Platform classes the application imports, published by import `0x14`.
type ImportedClasses = Arc<Mutex<Option<ClassTable>>>;

/// Main class of the one application the ahead-of-time compiled Java runtime
/// has been reverse engineered against so far.
///
/// The handlers guarded by this are not a compatibility implementation: they
/// poke absolute addresses (class roots, the runtime dispatch table at
/// `0x015009e4`, the object layout at `0x01500e40`) that were read out of
/// *this* `binary.mod`. Running them against any other LGT application writes
/// over unrelated memory, so they stay off unless the descriptor names `Lm`.
const LM_EXPERIMENT_MAIN_CLASS: &str = "Lm";

#[derive(Clone)]
struct InitSvcContext {
    wipic_category: u32,
    stdlib_category: u32,
    jvm: Jvm,
    java_handles: JavaHandles,
    imported_classes: ImportedClasses,
    app_classes: AppClasses,
    image_ranges: ImageRanges,
    java_class_tables: JavaClassTables,
    java_activated_classes: JavaActivatedClasses,
    lm_experiment: bool,
}

fn register_init_svc_handler(core: &mut ArmCore, jvm: &Jvm, lm_experiment: bool, image_ranges: ImageRanges) -> Result<()> {
    let java_handles = JavaHandles::new(core.clone());

    core.register_svc_handler(
        SVC_CATEGORY_INIT,
        handle_init_svc,
        &InitSvcContext {
            wipic_category: SVC_CATEGORY_WIPIC,
            stdlib_category: SVC_CATEGORY_STDLIB,
            jvm: jvm.clone(),
            java_handles,
            imported_classes: Default::default(),
            app_classes: Default::default(),
            image_ranges,
            java_class_tables: Default::default(),
            java_activated_classes: Default::default(),
            lm_experiment,
        },
    )
}

async fn handle_init_svc(core: &mut ArmCore, context: &mut InitSvcContext, id: SvcId) -> Result<()> {
    let wipic_category = &context.wipic_category;
    let stdlib_category = &context.stdlib_category;
    let jvm = &mut context.jvm;
    let (_, lr) = core.read_pc_lr()?;

    if id.0 >= JAVA_STATIC_METHOD_SVC_BASE && id.0 < JAVA_STATIC_METHOD_SVC_BASE + JAVA_METHOD_SVC_LIMIT {
        let index = id.0 - JAVA_STATIC_METHOD_SVC_BASE;
        let result = invoke_imported_static(core, context, index).await?;

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
        if function_index == 0x0d && context.lm_experiment {
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

        if function_index == 0x104 && context.lm_experiment {
            let mut original = [0u8; 16];
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
        if function_index == 0xfc && context.lm_experiment {
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
            let app_classes = app_classes.lock();

            let image_ranges = context.image_ranges.clone();

            java_unk11(core, jvm, &app_classes, &image_ranges, argc, argv).await?.write(core, lr)
        }
        InitSvcId::JavaImport09 => {
            let a0 = core.read_param(0)?;
            let a1 = core.read_param(1)?;
            let a2 = core.read_param(2)?;
            let a3 = core.read_param(3)?;

            let result = java_import_09(core, &context.java_handles, jvm, a0, a1, a2, a3).await?;

            result.write(core, lr)
        }
        InitSvcId::JavaImport0e => {
            let a0 = core.read_param(0)?;
            let a1 = core.read_param(1)?;
            let a2 = core.read_param(2)?;
            let a3 = core.read_param(3)?;

            // Resolves one of the application's own classes by index into the
            // table registered by import 0x07, returning its root.
            let classes = context.java_class_tables.lock().values().next().map(|(classes, _)| *classes).unwrap_or(0);

            let class = if classes == 0 {
                tracing::warn!("java_import_0e({a2}) before any class table was registered");
                0
            } else {
                let count: u32 = read_generic(core, classes)?;
                if a2 < count {
                    read_generic(core, classes + 8 + a2 * 4)?
                } else {
                    tracing::warn!("java_import_0e({a2}) is out of range for {count} classes");
                    0
                }
            };

            tracing::debug!("java_import_0e(a0={a0:#x}, a1={a1:#x}, index={a2}, a3={a3:#x}) -> root {class:#x}");
            class.write(core, lr)
        }
        InitSvcId::JavaImport10 => EmulatedFunction::call(&java_import_10, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaImport11 => EmulatedFunction::call(&java_import_11, core, &mut ()).await?.write(core, lr),
        InitSvcId::JavaImport23 => EmulatedFunction::call(&java_import_23, core, &mut ()).await?.write(core, lr),
    }
}
/// Handles a call the compiled code made through `static_method_offsets`.
///
/// `index` is the row of the static method table, which is all the stub
/// carries; everything else comes from the table published at load time.
async fn invoke_imported_static(core: &mut ArmCore, context: &mut InitSvcContext, index: u32) -> Result<u32> {
    let imported_classes = context.imported_classes.clone();
    let table = imported_classes.lock();

    let Some(table) = table.as_ref() else {
        return Err(WieError::FatalError(format!(
            "Imported static method {index} called before the class table was loaded"
        )));
    };

    let Some(Some(member)) = table.static_methods.get(index as usize) else {
        return Err(WieError::FatalError(format!("Imported static method {index} has no descriptor")));
    };

    let handles = context.java_handles.clone();
    let jvm = context.jvm.clone();

    method_bridge::invoke(core, &jvm, &handles, table, member, None).await
}

pub async fn load_native(core: &mut ArmCore, system: &mut System, jvm: &Jvm, data: &[u8], main_class_name: Option<&str>) -> Result<()> {
    let lm_experiment = main_class_name == Some(LM_EXPERIMENT_MAIN_CLASS);
    if lm_experiment {
        tracing::warn!("Enabling experimental {LM_EXPERIMENT_MAIN_CLASS} runtime patches; these are specific to that binary.mod");
    }

    let (entrypoint, image_ranges) = load_executable(core, data)?;
    register_wipic_svc_handler(core, system, jvm)?;
    register_stdlib_svc_handler(core, system)?;
    register_init_svc_handler(core, jvm, lm_experiment, Arc::new(image_ranges))?;

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

    let mut ranges = Vec::new();

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
            ranges.push((shdr.sh_addr as u32, shdr.sh_size as u32));
        }
    }

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
