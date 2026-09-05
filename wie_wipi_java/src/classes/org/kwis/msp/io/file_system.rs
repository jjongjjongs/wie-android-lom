use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use java_runtime::classes::java::{io::File as JavaFile, lang::String, util::Vector};
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

/// What every space question answers with. The backend does not report the
/// host's free space, and a title only ever checks that there is room to save.
const REPORTED_SPACE: i32 = 0x100_0000;

// class org.kwis.msp.io.FileSystem
pub struct FileSystem;

impl FileSystem {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/io/FileSystem",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("isFile", "(Ljava/lang/String;)Z", Self::is_file, MethodAccessFlags::STATIC),
                JavaMethodProto::new("isDirectory", "(Ljava/lang/String;I)Z", Self::is_directory, MethodAccessFlags::STATIC),
                JavaMethodProto::new("exists", "(Ljava/lang/String;)Z", Self::exists, MethodAccessFlags::STATIC),
                JavaMethodProto::new("exists", "(Ljava/lang/String;I)Z", Self::exists_with_flag, MethodAccessFlags::STATIC),
                JavaMethodProto::new("mkdir", "(Ljava/lang/String;I)V", Self::mkdir, MethodAccessFlags::STATIC),
                JavaMethodProto::new("available", "()I", Self::available, MethodAccessFlags::STATIC),
                JavaMethodProto::new("available", "(Ljava/lang/String;I)I", Self::available_on, MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "availableLsize",
                    "(Ljava/lang/String;I)D",
                    Self::available_lsize,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("totalSpace", "()I", Self::total_space, MethodAccessFlags::STATIC),
                JavaMethodProto::new("totalSpace", "(Ljava/lang/String;)I", Self::total_space_on, MethodAccessFlags::STATIC),
                JavaMethodProto::new("getCounts", "(Ljava/lang/String;)I", Self::get_counts, MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "getCounts",
                    "(Ljava/lang/String;I)I",
                    Self::get_counts_with_flag,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getMountedNames",
                    "()[Ljava/lang/String;",
                    Self::get_mounted_names,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("setMode", "(Ljava/lang/String;I)Z", Self::set_mode, MethodAccessFlags::STATIC),
                JavaMethodProto::new("setMode", "(Ljava/lang/String;II)Z", Self::set_mode_with_flag, MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "addFileSystemListener",
                    "(Lorg/kwis/msp/io/FileSystemListener;)Z",
                    Self::add_file_system_listener,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "removeFileSystemListener",
                    "(Lorg/kwis/msp/io/FileSystemListener;)Z",
                    Self::remove_file_system_listener,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("handleEvent", "(III)Z", Self::handle_event, MethodAccessFlags::STATIC),
                JavaMethodProto::new("getMaxFilenameLength", "()I", Self::get_max_filename_length, MethodAccessFlags::STATIC),
                JavaMethodProto::new("list", "(Ljava/lang/String;)Ljava/util/Vector;", Self::list, MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "list",
                    "(Ljava/lang/String;I)Ljava/util/Vector;",
                    Self::list_with_flag,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("remove", "(Ljava/lang/String;)V", Self::remove, MethodAccessFlags::STATIC),
                JavaMethodProto::new("remove", "(Ljava/lang/String;I)V", Self::remove_with_flag, MethodAccessFlags::STATIC),
                JavaMethodProto::new("mkdir", "(Ljava/lang/String;)V", Self::mkdir_without_flag, MethodAccessFlags::STATIC),
                JavaMethodProto::new("rmdir", "(Ljava/lang/String;)V", Self::rmdir, MethodAccessFlags::STATIC),
                JavaMethodProto::new("rmdir", "(Ljava/lang/String;I)V", Self::rmdir_with_flag, MethodAccessFlags::STATIC),
                JavaMethodProto::new("toCString", "(Ljava/lang/String;)[B", Self::to_c_string, MethodAccessFlags::STATIC),
                JavaMethodProto::new("isFile", "(Ljava/lang/String;I)Z", Self::is_file_with_flag, MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "isDirectory",
                    "(Ljava/lang/String;)Z",
                    Self::is_directory_without_flag,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getCreationTime",
                    "(Ljava/lang/String;)I",
                    Self::get_creation_time,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getCreationTime",
                    "(Ljava/lang/String;I)I",
                    Self::get_creation_time_with_flag,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "rename",
                    "(Ljava/lang/String;Ljava/lang/String;)V",
                    Self::rename,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "rename",
                    "(Ljava/lang/String;Ljava/lang/String;I)V",
                    Self::rename_with_flag,
                    MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.io.FileSystem::<init>({this:?})");

        jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await
    }

    async fn is_file(jvm: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.io.FileSystem::is_file({name:?})");

        // A null path is not a file. The reference returns false rather than
        // dereferencing it; `new File(null)` would otherwise reach a proxy that
        // panics on the null argument. Iljimae (일지매) probes isFile(null).
        if name.is_null() {
            return Ok(false);
        }

        let file = jvm.new_class("java/io/File", "(Ljava/lang/String;)V", (name,)).await?;
        let is_file = jvm.invoke_virtual(&file, "isFile", "()Z", ()).await?;

        Ok(is_file)
    }

    async fn is_directory(jvm: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.io.FileSystem::isDirectory({name:?}, {flag:?})");

        let file = jvm.new_class("java/io/File", "(Ljava/lang/String;)V", (name,)).await?;
        let is_directory = jvm.invoke_virtual(&file, "isDirectory", "()Z", ()).await?;

        Ok(is_directory)
    }

    async fn exists(jvm: &Jvm, _context: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.io.FileSystem::exists({name:?})");

        jvm.invoke_static("org/kwis/msp/io/FileSystem", "exists", "(Ljava/lang/String;I)Z", (name, 0))
            .await
    }

    async fn exists_with_flag(jvm: &Jvm, _context: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.io.FileSystem::exists({name:?}, {flag:?})");

        let file = jvm.new_class("java/io/File", "(Ljava/lang/String;)V", (name,)).await?;
        let exists = jvm.invoke_virtual(&file, "exists", "()Z", ()).await?;

        Ok(exists)
    }

    async fn mkdir(_jvm: &Jvm, _context: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::mkdir({name:?}, {flag:?})");

        Ok(())
    }

    async fn available(_: &Jvm, _: &mut WieJvmContext) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::available()");

        Ok(REPORTED_SPACE)
    }

    /// Free and total space. The one storage a title has is the directory the
    /// platform gives it, and the backend does not report how much of the host
    /// is left, so every volume answers with the same figure `available` has
    /// always reported - room to write.
    async fn available_on(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::available({name:?}, {flag})");

        Ok(REPORTED_SPACE)
    }

    async fn available_lsize(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<f64> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::availableLsize({name:?}, {flag})");

        Ok(REPORTED_SPACE as f64)
    }

    async fn total_space(_: &Jvm, _: &mut WieJvmContext) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::totalSpace()");

        Ok(REPORTED_SPACE)
    }

    async fn total_space_on(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::totalSpace({name:?})");

        Ok(REPORTED_SPACE)
    }

    /// How many entries a directory holds. `list` does not enumerate one yet,
    /// so neither does this; both say empty rather than disagree.
    async fn get_counts(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::getCounts({dirname:?})");

        Ok(0)
    }

    async fn get_counts_with_flag(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>, flag: i32) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::getCounts({dirname:?}, {flag})");

        Ok(0)
    }

    /// The volumes a title can write to. There is one and it is the title's own
    /// directory, which it reaches without naming a volume, so the list is
    /// empty.
    async fn get_mounted_names(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<ClassInstanceRef<Array<String>>> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::getMountedNames()");

        Ok(jvm.instantiate_array("Ljava/lang/String;", 0).await?.into())
    }

    /// File attributes - read-only, hidden and so on. The backend keeps none,
    /// so a title setting them is told it could not rather than believing they
    /// took.
    async fn set_mode(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, mode: i32) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::setMode({name:?}, {mode})");

        Ok(false)
    }

    async fn set_mode_with_flag(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, mode: i32, flag: i32) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::setMode({name:?}, {mode}, {flag})");

        Ok(false)
    }

    /// Storage being inserted or removed, which cannot happen to the one
    /// directory a title has: a listener would never be called, so registering
    /// one reports that it was not taken.
    async fn add_file_system_listener(_: &Jvm, _: &mut WieJvmContext, listener: ClassInstanceRef<()>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::addFileSystemListener({listener:?})");

        Ok(false)
    }

    async fn remove_file_system_listener(_: &Jvm, _: &mut WieJvmContext, listener: ClassInstanceRef<()>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::removeFileSystemListener({listener:?})");

        Ok(false)
    }

    async fn handle_event(_: &Jvm, _: &mut WieJvmContext, event: i32, param1: i32, param2: i32) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::handleEvent({event}, {param1}, {param2})");

        Ok(false)
    }

    async fn get_max_filename_length(_: &Jvm, _: &mut WieJvmContext) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::getMaxFilenameLength()");

        Ok(0)
    }

    async fn list(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>) -> JvmResult<ClassInstanceRef<Vector>> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::list({dirname:?})");

        Ok(ClassInstanceRef::new(None))
    }

    async fn list_with_flag(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>, flag: i32) -> JvmResult<ClassInstanceRef<Vector>> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::list({dirname:?}, {flag})");

        Ok(ClassInstanceRef::new(None))
    }

    async fn remove(jvm: &Jvm, _: &mut WieJvmContext, filename: ClassInstanceRef<String>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.io.FileSystem::remove({filename:?})");

        jvm.invoke_static("org/kwis/msp/io/FileSystem", "remove", "(Ljava/lang/String;I)V", (filename, 1))
            .await
    }

    async fn remove_with_flag(jvm: &Jvm, _: &mut WieJvmContext, filename: ClassInstanceRef<String>, flag: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.io.FileSystem::remove({filename:?}, {flag})");

        let file = jvm.new_class("java/io/File", "(Ljava/lang/String;)V", (filename,)).await?;

        let removed: bool = jvm.invoke_virtual(&file, "delete", "()Z", ()).await?;

        if !removed {
            return Err(jvm.exception("java/io/IOException", "file isn't exist").await);
        }

        Ok(())
    }

    async fn mkdir_without_flag(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::mkdir({dirname:?})");

        Ok(())
    }

    async fn rmdir(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::rmdir({dirname:?})");

        Ok(())
    }

    async fn rmdir_with_flag(_: &Jvm, _: &mut WieJvmContext, dirname: ClassInstanceRef<String>, flag: i32) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::rmdir({dirname:?}, {flag})");

        Ok(())
    }

    async fn to_c_string(_: &Jvm, _: &mut WieJvmContext, value: ClassInstanceRef<String>) -> JvmResult<ClassInstanceRef<Array<i8>>> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::toCString({value:?})");

        Ok(ClassInstanceRef::new(None))
    }

    async fn is_file_with_flag(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::isFile({name:?}, {flag})");

        Ok(false)
    }

    async fn is_directory_without_flag(jvm: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.io.FileSystem::isDirectory({name:?})");

        let file: ClassInstanceRef<JavaFile> = jvm.new_class("java/io/File", "(Ljava/lang/String;)V", (name,)).await?.into();
        jvm.invoke_virtual(&file, "isDirectory", "()Z", ()).await
    }

    async fn get_creation_time(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::getCreationTime({name:?})");

        Ok(0)
    }

    async fn get_creation_time_with_flag(_: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>, flag: i32) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::getCreationTime({name:?}, {flag})");

        Ok(0)
    }

    async fn rename(_: &Jvm, _: &mut WieJvmContext, old_name: ClassInstanceRef<String>, new_name: ClassInstanceRef<String>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::rename({old_name:?}, {new_name:?})");

        Ok(())
    }

    async fn rename_with_flag(
        _: &Jvm,
        _: &mut WieJvmContext,
        old_name: ClassInstanceRef<String>,
        new_name: ClassInstanceRef<String>,
        flag: i32,
    ) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.io.FileSystem::rename({old_name:?}, {new_name:?}, {flag})");

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use alloc::boxed::Box;

    use java_runtime::classes::java::{lang::String, util::Vector};
    use jvm::{Array, ClassInstanceRef, JavaError, Result as JvmResult, runtime::JavaLangString};
    use test_utils::run_jvm_test;
    use wie_util::Result;

    use crate::get_protos;

    use super::FileSystem;

    #[test]
    fn test_filesystem_overloads_and_neutral_stubs() -> Result<()> {
        run_jvm_test(Box::new([get_protos().into()]), |jvm| async move {
            let _: ClassInstanceRef<FileSystem> = jvm.new_class("org/kwis/msp/io/FileSystem", "()V", ()).await?.into();
            let name: ClassInstanceRef<String> = JavaLangString::from_rust_string(&jvm, "missing").await?.into();
            let new_name: ClassInstanceRef<String> = JavaLangString::from_rust_string(&jvm, "renamed").await?.into();

            let max_length: i32 = jvm.invoke_static("org/kwis/msp/io/FileSystem", "getMaxFilenameLength", "()I", ()).await?;
            assert_eq!(max_length, 0);

            let listed: ClassInstanceRef<Vector> = jvm
                .invoke_static(
                    "org/kwis/msp/io/FileSystem",
                    "list",
                    "(Ljava/lang/String;)Ljava/util/Vector;",
                    (name.clone(),),
                )
                .await?;
            let listed_with_flag: ClassInstanceRef<Vector> = jvm
                .invoke_static(
                    "org/kwis/msp/io/FileSystem",
                    "list",
                    "(Ljava/lang/String;I)Ljava/util/Vector;",
                    (name.clone(), 1),
                )
                .await?;
            assert!(listed.is_null());
            assert!(listed_with_flag.is_null());

            let c_string: ClassInstanceRef<Array<i8>> = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "toCString", "(Ljava/lang/String;)[B", (name.clone(),))
                .await?;
            assert!(c_string.is_null());

            let is_file: bool = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "isFile", "(Ljava/lang/String;I)Z", (name.clone(), 1))
                .await?;
            let is_directory: bool = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "isDirectory", "(Ljava/lang/String;)Z", (name.clone(),))
                .await?;
            let creation_time: i32 = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "getCreationTime", "(Ljava/lang/String;)I", (name.clone(),))
                .await?;
            let creation_time_with_flag: i32 = jvm
                .invoke_static(
                    "org/kwis/msp/io/FileSystem",
                    "getCreationTime",
                    "(Ljava/lang/String;I)I",
                    (name.clone(), 1),
                )
                .await?;
            assert!(!is_file);
            assert!(!is_directory);
            assert_eq!(creation_time, 0);
            assert_eq!(creation_time_with_flag, 0);

            let _: () = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "mkdir", "(Ljava/lang/String;)V", (name.clone(),))
                .await?;
            let _: () = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "rmdir", "(Ljava/lang/String;)V", (name.clone(),))
                .await?;
            let _: () = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "rmdir", "(Ljava/lang/String;I)V", (name.clone(), 1))
                .await?;
            let _: () = jvm
                .invoke_static(
                    "org/kwis/msp/io/FileSystem",
                    "rename",
                    "(Ljava/lang/String;Ljava/lang/String;)V",
                    (name.clone(), new_name.clone()),
                )
                .await?;
            let _: () = jvm
                .invoke_static(
                    "org/kwis/msp/io/FileSystem",
                    "rename",
                    "(Ljava/lang/String;Ljava/lang/String;I)V",
                    (name, new_name, 1),
                )
                .await?;

            Ok(())
        })
    }

    #[test]
    fn test_filesystem_remove_deletes_file_and_reports_missing_file() -> Result<()> {
        run_jvm_test(Box::new([get_protos().into()]), |jvm| async move {
            let name: ClassInstanceRef<String> = JavaLangString::from_rust_string(&jvm, "remove-test.dat").await?.into();

            let file = jvm.new_class("java/io/File", "(Ljava/lang/String;)V", (name.clone(),)).await?;

            let output = jvm.new_class("java/io/FileOutputStream", "(Ljava/io/File;)V", (file.clone(),)).await?;

            let _: () = jvm.invoke_virtual(&output, "write", "(I)V", (0x41,)).await?;
            let _: () = jvm.invoke_virtual(&output, "close", "()V", ()).await?;

            let exists_before: bool = jvm.invoke_virtual(&file, "exists", "()Z", ()).await?;
            assert!(exists_before);

            let _: () = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "remove", "(Ljava/lang/String;)V", (name.clone(),))
                .await?;

            let exists_after: bool = jvm.invoke_virtual(&file, "exists", "()Z", ()).await?;
            assert!(!exists_after);

            let second: JvmResult<()> = jvm
                .invoke_static("org/kwis/msp/io/FileSystem", "remove", "(Ljava/lang/String;)V", (name,))
                .await;

            let Err(JavaError::JavaException(exception)) = second else {
                panic!("second remove unexpectedly succeeded");
            };

            assert!(jvm.is_instance(&*exception, "java/io/IOException"));

            Ok(())
        })
    }
}
