use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::io::InputStream;
use jvm::{ClassInstanceRef, Jvm, Result, runtime::JavaIoInputStream};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class net.wie.SmafPlayer
pub struct SmafPlayer;

impl SmafPlayer {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "net/wie/SmafPlayer",
            parent_class: Some("java/lang/Object"),
            interfaces: vec!["javax/microedition/media/Player"],
            methods: vec![
                JavaMethodProto::new("<init>", "(Ljava/io/InputStream;)V", Self::init, Default::default()),
                JavaMethodProto::new("start", "()V", Self::start, Default::default()),
                JavaMethodProto::new("start", "(Z)V", Self::start_with_repeat, Default::default()),
                JavaMethodProto::new("stop", "()V", Self::stop, Default::default()),
                JavaMethodProto::new("close", "()V", Self::close, Default::default()),
                JavaMethodProto::new("setVolume", "(I)I", Self::set_volume, Default::default()),
                JavaMethodProto::new("getVolume", "()I", Self::get_volume, Default::default()),
            ],
            fields: vec![JavaFieldProto::new("audioHandle", "I", Default::default())],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, context: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, stream: ClassInstanceRef<InputStream>) -> Result<()> {
        tracing::debug!("net.wie.SmafPlayer::<init>({this:?}, {stream:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        let data = JavaIoInputStream::read_until_end(jvm, &stream).await?;
        let audio_handle = context.system().audio().load_smaf(&data).unwrap();

        jvm.put_field(&mut this, "audioHandle", "I", audio_handle as i32).await?;

        Ok(())
    }

    async fn start(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> Result<()> {
        Self::start_with_repeat(jvm, context, this, false).await
    }

    async fn start_with_repeat(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, repeat: bool) -> Result<()> {
        tracing::debug!("net.wie.SmafPlayer::start({this:?}, {repeat})");

        let audio_handle: i32 = jvm.get_field(&this, "audioHandle", "I").await?;

        let system = context.system();

        system.audio().play(system, audio_handle as u32, repeat).unwrap();

        Ok(())
    }

    async fn stop(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> Result<()> {
        tracing::debug!("net.wie.SmafPlayer::stop({this:?})");

        let audio_handle: i32 = jvm.get_field(&this, "audioHandle", "I").await?;

        let system = context.system();

        system.audio().stop(audio_handle as u32);

        Ok(())
    }

    async fn close(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> Result<()> {
        tracing::debug!("net.wie.SmafPlayer::close({this:?})");

        let audio_handle: i32 = jvm.get_field(&this, "audioHandle", "I").await?;

        let system = context.system();

        system.audio().close(audio_handle as u32).unwrap();

        Ok(())
    }

    async fn set_volume(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, volume: i32) -> Result<i32> {
        tracing::debug!("net.wie.SmafPlayer::setVolume({this:?}, {volume})");

        let audio_handle: i32 = jvm.get_field(&this, "audioHandle", "I").await?;
        let system = context.system();

        match system.audio().set_volume(audio_handle as u32, volume.clamp(0, 100) as u8) {
            Ok(()) => Ok(0),
            Err(_) => Ok(-9),
        }
    }

    async fn get_volume(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> Result<i32> {
        tracing::debug!("net.wie.SmafPlayer::getVolume({this:?})");

        let audio_handle: i32 = jvm.get_field(&this, "audioHandle", "I").await?;
        let system = context.system();

        match system.audio().get_volume(audio_handle as u32) {
            Ok(volume) => Ok(i32::from(volume)),
            Err(_) => Ok(-9),
        }
    }
}
