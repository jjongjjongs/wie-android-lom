use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::javax::microedition::media::Player;

// not in reference, but called by some apps..
// class org.kwis.msp.media.BaseClip
pub struct BaseClip;

impl BaseClip {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/media/BaseClip",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("allocPlayer", "()I", Self::alloc_player, Default::default()),
                JavaMethodProto::new("setBuffer", "([BI)Z", Self::set_buffer, Default::default()),
                JavaMethodProto::new("putData", "([BII)I", Self::put_data, Default::default()),
                JavaMethodProto::new("mediaPlay", "(Z)I", Self::media_play, Default::default()),
                JavaMethodProto::new("mediaStop", "()I", Self::media_stop, Default::default()),
                JavaMethodProto::new("clearData", "()V", Self::clear_data, Default::default()),
                JavaMethodProto::new("availableDataSize", "()I", Self::available_data_size, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("player", "Ljavax/microedition/media/Player;", Default::default()),
                JavaFieldProto::new("__wieBufferSize", "I", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.media.BaseClip::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;

        Ok(())
    }

    async fn alloc_player(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::allocPlayer({this:?})");

        let player: ClassInstanceRef<Player> =
            jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        // The native backend reports an invalid/unavailable clip as -9.
        // In WIE, putData/setBuffer creates the MIDP player eagerly, so a
        // non-null player represents an already allocated native player.
        if player.is_null() {
            Ok(-9)
        } else {
            Ok(0)
        }
    }

    async fn set_buffer(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        buffer: ClassInstanceRef<Array<i8>>,
        size: i32,
    ) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.media.BaseClip::setBuffer({this:?}, {buffer:?}, {size})");

        let current_size: i32 = jvm.get_field(&this, "__wieBufferSize", "I").await?;
        if current_size > 0 {
            return Ok(false);
        }

        // Native dereferences the byte array here, so a null buffer must
        // preserve the JVM's normal null-array failure.
        let array_length = jvm.array_length(&buffer).await? as i32;
        let data_size = core::cmp::min(array_length, size);

        let result: i32 = jvm.invoke_virtual(&this, "putData", "([BII)I", (buffer, 0, data_size)).await?;
        if result < 0 {
            return Ok(false);
        }

        // Native stores the original requested size, while mediaSetBuffer0
        // clamps only the byte count passed to the backend.
        jvm.put_field(&mut this, "__wieBufferSize", "I", size).await?;

        Ok(true)
    }

    async fn available_data_size(_jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::warn!("stub org.kwis.msp.media.BaseClip::availableDataSize({this:?})");

        Ok(10000000 as _)
    }

    async fn put_data(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        buffer: ClassInstanceRef<Array<i8>>,
        offset: i32,
        length: i32,
    ) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.Clip::putData({this:?}, {buffer:?}, {offset}, {length})");

        let input_stream = jvm.new_class("java/io/ByteArrayInputStream", "([BII)V", (buffer, offset, length)).await?;
        let r#type = JavaLangString::from_rust_string(jvm, "application/vnd.smaf").await?;

        let player: ClassInstanceRef<Player> = jvm
            .invoke_static(
                "javax/microedition/media/Manager",
                "createPlayer",
                "(Ljava/io/InputStream;Ljava/lang/String;)Ljavax/microedition/media/Player;",
                (input_stream, r#type),
            )
            .await?;

        jvm.put_field(&mut this, "player", "Ljavax/microedition/media/Player;", player).await?;

        Ok(length)
    }

    async fn media_play(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        repeat: bool,
    ) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::mediaPlay({this:?}, {repeat})");

        let player: ClassInstanceRef<Player> =
            jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        if player.is_null() {
            return Ok(-9);
        }

        let _: () = jvm.invoke_virtual(&player, "start", "(Z)V", (repeat,)).await?;

        Ok(0)
    }

    async fn media_stop(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::mediaStop({this:?})");

        let player: ClassInstanceRef<Player> =
            jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        if player.is_null() {
            return Ok(-9);
        }

        let _: () = jvm.invoke_virtual(&player, "stop", "()V", ()).await?;

        Ok(0)
    }

    async fn clear_data(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.media.BaseClip::clearData({this:?})");

        let player: ClassInstanceRef<Player> = jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;
        if player.is_null() {
            return Ok(());
        }

        let _: () = jvm.invoke_virtual(&player, "close", "()V", ()).await?;

        jvm.put_field(&mut this, "player", "Ljavax/microedition/media/Player;", None).await?;

        Ok(())
    }
}
