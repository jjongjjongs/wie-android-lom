use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::time::Duration;

use spin::Mutex;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use java_runtime::{
    File, FileDescriptorId, FileSize, FileStat, FileType, IOError, IOResult, RT_RUSTJAR, Runtime, RuntimeClassProto, RuntimeContext, SpawnCallback,
    get_runtime_class_proto,
};
use jvm::{Array, ClassDefinition, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_backend::{AsyncCallable, System};
use wie_util::WieError;

use crate::{JvmImplementation, JvmSupport, WIE_RUSTJAR, WieJavaClassProto, WieJvmContext};

mod file;

use file::FileImpl;

const STDOUT_FD: u32 = 1;
const STDERR_FD: u32 = 2;

struct FileTableInner {
    files: BTreeMap<u32, Box<dyn File>>,
    next_id: u32,
}

impl FileTableInner {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            next_id: 3, // 0=stdin, 1=stdout, 2=stderr
        }
    }

    fn add(&mut self, file: Box<dyn File>) -> FileDescriptorId {
        let id = self.next_id;
        self.next_id += 1;
        self.files.insert(id, file);
        FileDescriptorId::new(id)
    }
}

#[derive(Clone)]
struct StdoutFile {
    system: System,
}

#[async_trait::async_trait]
impl File for StdoutFile {
    async fn read(&mut self, _buf: &mut [u8]) -> IOResult<usize> {
        Err(IOError::Unsupported)
    }

    async fn write(&mut self, buf: &[u8]) -> IOResult<usize> {
        self.system.platform().write_stdout(buf);

        Ok(buf.len())
    }

    async fn seek(&mut self, _pos: FileSize) -> IOResult<()> {
        Err(IOError::Unsupported)
    }

    async fn tell(&self) -> IOResult<FileSize> {
        Err(IOError::Unsupported)
    }

    async fn set_len(&mut self, _len: FileSize) -> IOResult<()> {
        Err(IOError::Unsupported)
    }

    async fn metadata(&self) -> IOResult<FileStat> {
        Err(IOError::Unsupported)
    }
}

#[derive(Clone)]
struct StderrFile {
    system: System,
}

#[async_trait::async_trait]
impl File for StderrFile {
    async fn read(&mut self, _buf: &mut [u8]) -> IOResult<usize> {
        Err(IOError::Unsupported)
    }

    async fn write(&mut self, buf: &[u8]) -> IOResult<usize> {
        self.system.platform().write_stderr(buf);

        Ok(buf.len())
    }

    async fn seek(&mut self, _pos: FileSize) -> IOResult<()> {
        Err(IOError::Unsupported)
    }

    async fn tell(&self) -> IOResult<FileSize> {
        Err(IOError::Unsupported)
    }

    async fn set_len(&mut self, _len: FileSize) -> IOResult<()> {
        Err(IOError::Unsupported)
    }

    async fn metadata(&self) -> IOResult<FileStat> {
        Err(IOError::Unsupported)
    }
}

#[derive(Clone)]
pub struct JvmRuntime<T>
where
    T: JvmImplementation + Sync + Send + 'static,
{
    system: System,
    implementation: T,
    protos: Arc<Mutex<Vec<WieJavaClassProto>>>,
    file_table: Arc<Mutex<FileTableInner>>,
}

impl<T> JvmRuntime<T>
where
    T: JvmImplementation + Sync + Send + 'static,
{
    pub fn new(system: System, implementation: T, protos: Box<[Box<[WieJavaClassProto]>]>) -> Self {
        let mut file_table = FileTableInner::new();
        file_table.files.insert(STDOUT_FD, Box::new(StdoutFile { system: system.clone() }));
        file_table.files.insert(STDERR_FD, Box::new(StderrFile { system: system.clone() }));

        Self {
            system,
            implementation,
            protos: Arc::new(Mutex::new(protos.into_vec().into_iter().flat_map(|x| x.into_vec()).collect())),
            file_table: Arc::new(Mutex::new(file_table)),
        }
    }
}

#[async_trait::async_trait]
impl<T> Runtime for JvmRuntime<T>
where
    T: JvmImplementation + Sync + Send + 'static,
{
    async fn sleep(&self, duration: Duration) {
        self.system.sleep(duration.as_millis() as _).await;
    }

    async fn r#yield(&self) {
        self.system.yield_now().await;
    }

    fn spawn(&self, jvm: &Jvm, callback: Box<dyn SpawnCallback>) {
        struct SpawnProxy {
            jvm: Jvm,
            callback: Box<dyn SpawnCallback>,
        }

        impl AsyncCallable<Result<(), WieError>> for SpawnProxy {
            async fn call(self) -> Result<(), WieError> {
                let result = self.callback.call().await;
                if let Err(err) = result {
                    return Err(JvmSupport::to_wie_err(&self.jvm, err).await);
                }

                Ok(())
            }
        }

        self.system.spawn(SpawnProxy { jvm: jvm.clone(), callback });
    }

    fn exit(&self, _status: i32) {
        self.system.platform().exit();
    }

    fn now(&self) -> u64 {
        self.system.platform().now().raw()
    }

    fn current_task_id(&self) -> u64 {
        self.system.current_task_id()
    }

    fn stdin(&self) -> IOResult<FileDescriptorId> {
        Err(IOError::Unsupported)
    }

    fn stdout(&self) -> IOResult<FileDescriptorId> {
        Ok(FileDescriptorId::new(STDOUT_FD))
    }

    fn stderr(&self) -> IOResult<FileDescriptorId> {
        Ok(FileDescriptorId::new(STDERR_FD))
    }

    async fn open(&self, path: &str, write: bool) -> IOResult<FileDescriptorId> {
        tracing::debug!("open({path:?}, {write:?})");

        let file = FileImpl::new(self.system.clone(), path, write).await?;
        Ok(self.file_table.lock().add(Box::new(file)))
    }

    fn get_file(&self, fd: FileDescriptorId) -> IOResult<Box<dyn File>> {
        self.file_table.lock().files.get(&fd.id()).cloned().ok_or(IOError::NotFound)
    }

    fn close_file(&self, fd: FileDescriptorId) {
        self.file_table.lock().files.remove(&fd.id());
    }

    async fn unlink(&self, path: &str) -> IOResult<()> {
        if self.system.filesystem().remove(path).await {
            Ok(())
        } else {
            Err(IOError::NotFound)
        }
    }

    async fn metadata(&self, path: &str) -> IOResult<FileStat> {
        if path.is_empty() || path.ends_with("/") {
            return Ok(FileStat {
                size: 0,
                r#type: FileType::Directory,
            });
        }

        let size = self.system.filesystem().size(path).await.ok_or(IOError::NotFound)?;

        Ok(FileStat {
            size: size as _,
            r#type: FileType::File,
        })
    }

    async fn find_rustjar_class(&self, jvm: &Jvm, classpath: &str, class: &str) -> JvmResult<Option<Box<dyn ClassDefinition>>> {
        if classpath == RT_RUSTJAR {
            let proto = get_runtime_class_proto(class).map(refuse_a_null_array);
            if let Some(proto) = proto {
                return Ok(Some(
                    self.implementation
                        .define_class_rust(jvm, proto, Box::new(self.clone()) as Box<_>)
                        .await?,
                ));
            }
        } else if classpath == WIE_RUSTJAR {
            let proto_index = self.protos.lock().iter().position(|x| x.name == class);
            if let Some(proto_index) = proto_index {
                let proto = self.protos.lock().remove(proto_index);
                let context = Box::new(WieJvmContext::new(&self.system));

                return Ok(Some(self.implementation.define_class_rust(jvm, proto, context as Box<_>).await?));
            }
        }

        Ok(None)
    }

    async fn define_class(&self, jvm: &Jvm, data: &[u8]) -> JvmResult<Box<dyn ClassDefinition>> {
        self.implementation.define_class_java(jvm, data).await
    }

    async fn define_array_class(&self, _jvm: &Jvm, element_type_name: &str) -> JvmResult<Box<dyn ClassDefinition>> {
        self.implementation.define_array_class(_jvm, element_type_name).await
    }
}

/// A stand-in for the class whose constructors are replaced below, so the
/// bodies can name their receiver the way every other proto does.
struct ByteArrayInputStream;

/// Makes `java.io.ByteArrayInputStream`'s constructors throw on a null array
/// instead of taking the emulator down with them.
///
/// A J2ME title reads a save file by asking whether it is there and handing
/// what it got to `new ByteArrayInputStream(...)`, null and all, because a real
/// handset answers that with `NullPointerException` and the title catches it -
/// that is its "no save yet" path. 놈3 does exactly this on a first run, three
/// times over, for `/a`, `/start` and `/nom`.
///
/// The runtime's own constructor reaches straight for the array's length, and a
/// null reference there is a `None` unwrapped inside the JVM - a Rust panic, so
/// the process died where the title expected to catch an exception. The bodies
/// here are the runtime's, with the check the reference makes in front.
///
/// Anything that is not that class is handed back untouched.
fn refuse_a_null_array(mut proto: RuntimeClassProto) -> RuntimeClassProto {
    if proto.name != "java/io/ByteArrayInputStream" {
        return proto;
    }

    for method in proto.methods.iter_mut() {
        if method.name != "<init>" {
            continue;
        }

        *method = match method.descriptor.as_str() {
            "([B)V" => JavaMethodProto::new("<init>", "([B)V", byte_array_input_stream_init, MethodAccessFlags::empty()),
            "([BII)V" => JavaMethodProto::new(
                "<init>",
                "([BII)V",
                byte_array_input_stream_init_with_offset_length,
                MethodAccessFlags::empty(),
            ),
            _ => continue,
        };
    }

    proto
}

async fn byte_array_input_stream_init(
    jvm: &Jvm,
    _: &mut RuntimeContext,
    this: ClassInstanceRef<ByteArrayInputStream>,
    data: ClassInstanceRef<Array<i8>>,
) -> JvmResult<()> {
    if data.is_null() {
        return Err(jvm.exception("java/lang/NullPointerException", "buf is null").await);
    }

    let count = jvm.array_length(&data).await?;

    jvm.invoke_special(&this, "java/io/ByteArrayInputStream", "<init>", "([BII)V", (data, 0, count as i32))
        .await
}

async fn byte_array_input_stream_init_with_offset_length(
    jvm: &Jvm,
    _: &mut RuntimeContext,
    mut this: ClassInstanceRef<ByteArrayInputStream>,
    data: ClassInstanceRef<Array<i8>>,
    offset: i32,
    length: i32,
) -> JvmResult<()> {
    if data.is_null() {
        return Err(jvm.exception("java/lang/NullPointerException", "buf is null").await);
    }

    let data_length = jvm.array_length(&data).await? as i32;
    if offset < 0 || length < 0 || offset > data_length {
        return Err(jvm.exception("java/lang/IndexOutOfBoundsException", "Invalid offset or length").await);
    }

    let _: () = jvm.invoke_special(&this, "java/io/InputStream", "<init>", "()V", ()).await?;

    jvm.put_field(&mut this, "buf", "[B", data).await?;
    jvm.put_field(&mut this, "pos", "I", offset).await?;
    jvm.put_field(&mut this, "count", "I", (offset + length).min(data_length)).await?;
    jvm.put_field(&mut this, "mark", "I", offset).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use java_runtime::get_runtime_class_proto;

    use super::refuse_a_null_array;

    #[test]
    fn byte_array_input_streams_constructors_are_the_ones_replaced() {
        let stock = get_runtime_class_proto("java/io/ByteArrayInputStream").unwrap();
        let stock: Vec<_> = stock
            .methods
            .iter()
            .map(|method| (method.name.clone(), method.descriptor.clone()))
            .collect();

        let guarded = refuse_a_null_array(get_runtime_class_proto("java/io/ByteArrayInputStream").unwrap());
        let guarded: Vec<_> = guarded
            .methods
            .iter()
            .map(|method| (method.name.clone(), method.descriptor.clone()))
            .collect();

        // The class keeps every method it had, in the order it had them - only
        // the two constructors' bodies change, and a body is not comparable.
        assert_eq!(stock, guarded);
        assert!(guarded.contains(&("<init>".into(), "([B)V".into())));
        assert!(guarded.contains(&("<init>".into(), "([BII)V".into())));
    }

    #[test]
    fn every_other_runtime_class_is_handed_back_as_it_was() {
        for class in ["java/io/DataInputStream", "java/lang/String", "java/io/InputStream"] {
            let stock = get_runtime_class_proto(class).unwrap();
            let name = stock.name;
            let methods = stock.methods.len();

            let out = refuse_a_null_array(get_runtime_class_proto(class).unwrap());

            assert_eq!(out.name, name);
            assert_eq!(out.methods.len(), methods);
        }
    }
}
