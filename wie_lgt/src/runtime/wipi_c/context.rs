use alloc::{boxed::Box, format, vec, vec::Vec};

use jvm::{
    Jvm,
    runtime::{JavaIoInputStream, JavaLangClassLoader},
};
use wipi_types::wipic::{WIPICIndirectPtr, WIPICWord};

use wie_backend::{AsyncCallable, Instant, System};
use wie_core_arm::{Allocator, ArmCore};
use wie_util::{ByteRead, ByteWrite, Result, WieError, read_generic, write_generic};
use wie_wipi_c::{
    WIPICContext, WIPICMethodBody,
    api::{net::SharedNetworkState, serial::SharedSerialState},
};

// mostly same as ktf's one, can we merge those?
#[derive(Clone)]
pub struct LgtWIPICContext {
    core: ArmCore,
    system: System,
    jvm: Jvm,
    network_state: SharedNetworkState,
    serial_state: SharedSerialState,
}

impl LgtWIPICContext {
    pub fn new(
        core: ArmCore,
        system: System,
        jvm: Jvm,
        network_state: SharedNetworkState,
        serial_state: SharedSerialState,
    ) -> Self {
        Self {
            core,
            system,
            jvm,
            network_state,
            serial_state,
        }
    }
}

#[async_trait::async_trait]
impl WIPICContext for LgtWIPICContext {
    fn alloc_raw(&mut self, size: WIPICWord) -> Result<WIPICWord> {
        Allocator::alloc(&mut self.core, size)
    }

    fn alloc(&mut self, size: WIPICWord) -> Result<WIPICIndirectPtr> {
        let address = Allocator::alloc(&mut self.core, size + size_of::<WIPICWord>() as WIPICWord)?;
        write_generic(&mut self.core, address, size)?;

        Ok(WIPICIndirectPtr(address + size_of::<WIPICWord>() as WIPICWord))
    }

    fn free(&mut self, memory: WIPICIndirectPtr) -> Result<()> {
        let base_address = memory.0 - size_of::<WIPICWord>() as WIPICWord;

        let size: WIPICWord = read_generic(&self.core, base_address)?;
        Allocator::free(&mut self.core, base_address, size + size_of::<WIPICWord>() as WIPICWord)
    }

    fn free_raw(&mut self, address: WIPICWord, size: WIPICWord) -> Result<()> {
        Allocator::free(&mut self.core, address, size)?;

        Ok(())
    }

    fn free_raw_unsized(&mut self, address: WIPICWord) -> Result<()> {
        Allocator::free_unsized(&mut self.core, address)
    }

    fn raw_alloc_size(&self, address: WIPICWord) -> Result<WIPICWord> {
        Allocator::allocation_size(&self.core, address)
    }

    fn data_ptr(&self, memory: WIPICIndirectPtr) -> Result<WIPICWord> {
        Ok(memory.0)
    }

    fn system(&mut self) -> &mut System {
        &mut self.system
    }

    fn network_state(&self) -> SharedNetworkState {
        self.network_state.clone()
    }

    fn serial_state(&self) -> SharedSerialState {
        self.serial_state.clone()
    }

    async fn call_function(&mut self, address: WIPICWord, args: &[WIPICWord]) -> Result<WIPICWord> {
        self.core.run_function(address, args).await
    }

    fn spawn(&mut self, callback: WIPICMethodBody) -> Result<()> {
        struct SpawnProxy {
            context: LgtWIPICContext,
            callback: WIPICMethodBody,
        }

        impl AsyncCallable<Result<()>> for SpawnProxy {
            async fn call(mut self) -> Result<()> {
                self.context.jvm.attach_thread(None).await.unwrap();
                self.callback.call(&mut self.context, Box::new([])).await?;
                self.context.jvm.detach_thread().unwrap();

                Ok(())
            }
        }

        self.system.spawn(SpawnProxy {
            context: self.clone(),
            callback,
        });

        Ok(())
    }

    async fn get_resource_size(&self, name: &str) -> Result<Option<usize>> {
        let class_loader = self.jvm.current_class_loader().await.unwrap();
        let stream = JavaLangClassLoader::get_resource_as_stream(&self.jvm, &class_loader, name).await.unwrap();

        if let Some(stream) = stream {
            let available: i32 = self.jvm.invoke_virtual(&stream, "available", "()I", ()).await.unwrap();
            return Ok(Some(available as _));
        }

        Ok(self.system.filesystem().size(name).await)
    }

    async fn read_resource(&self, name: &str) -> Result<Vec<u8>> {
        let class_loader = self.jvm.current_class_loader().await.unwrap();
        let stream = JavaLangClassLoader::get_resource_as_stream(&self.jvm, &class_loader, name).await.unwrap();

        if let Some(stream) = stream {
            return Ok(JavaIoInputStream::read_until_end(&self.jvm, &stream).await.unwrap());
        }

        let Some(size) = self.system.filesystem().size(name).await else {
            return Err(WieError::FatalError(format!("Missing resource: {name}")));
        };
        let mut data = vec![0; size];
        let read = self.system.filesystem().read(name, 0, size, &mut data).await.unwrap_or(0);
        data.truncate(read);

        Ok(data)
    }

    fn set_timer(&mut self, id: WIPICWord, due: Instant, callback: WIPICMethodBody) {
        let context = self.clone();

        self.system().event_queue().push_timer(id, due, move || {
            let mut context = context.clone();

            async move {
                callback.call(&mut context, Box::new([])).await?;
                Ok(())
            }
        })
    }

    fn unset_timer(&mut self, id: WIPICWord) {
        self.system().event_queue().cancel_timer(id);
    }
}

impl ByteRead for LgtWIPICContext {
    fn read_bytes(&self, address: WIPICWord, result: &mut [u8]) -> wie_util::Result<usize> {
        self.core.read_bytes(address, result)
    }
}

impl ByteWrite for LgtWIPICContext {
    fn write_bytes(&mut self, address: WIPICWord, data: &[u8]) -> wie_util::Result<()> {
        self.core.write_bytes(address, data)
    }
}
