use alloc::{boxed::Box, sync::Arc, vec};
use core::sync::atomic::{AtomicBool, Ordering};

use java_class_proto::{JavaFieldProto, JavaMethodProto, MethodBody};
use jvm::{Array, ClassInstanceRef, JavaError, JavaValue, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::javax::microedition::media::Player;

use crate::classes::org::kwis::msp::media::PlayListener;

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
                JavaMethodProto::new("mediaGetVolume", "()I", Self::media_get_volume, Default::default()),
                JavaMethodProto::new("mediaSetVolume", "(I)I", Self::media_set_volume, Default::default()),
                JavaMethodProto::new("clearData", "()V", Self::clear_data, Default::default()),
                JavaMethodProto::new("availableDataSize", "()I", Self::available_data_size, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("player", "Ljavax/microedition/media/Player;", Default::default()),
                JavaFieldProto::new("playListener", "Lorg/kwis/msp/media/PlayListener;", Default::default()),
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

        let player: ClassInstanceRef<Player> = jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        // The native backend reports an invalid/unavailable clip as -9.
        // In WIE, putData/setBuffer creates the MIDP player eagerly, so a
        // non-null player represents an already allocated native player.
        if player.is_null() { Ok(-9) } else { Ok(0) }
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

    async fn media_play(jvm: &Jvm, context: &mut WieJvmContext, this: ClassInstanceRef<Self>, repeat: bool) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::mediaPlay({this:?}, {repeat})");

        let player: ClassInstanceRef<Player> = jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        if player.is_null() {
            return Ok(-9);
        }

        if player.class_definition().name() == "net/wie/SmafPlayer" {
            let audio_handle: i32 = jvm.get_field(&player, "audioHandle", "I").await?;
            let system = context.system();
            let (completed, stopped) = system.audio().play_with_completion(system, audio_handle as u32, repeat).unwrap();

            // The completion callback only fires at end-of-media, which a looping
            // clip never reaches - so a repeating clip needs no watcher. Skipping
            // it also avoids piling up a watcher task per call for a title that
            // restarts its looping BGM every frame (시드): the identical re-plays
            // coalesce audio-side and share one never-set stop flag, so a watcher
            // spawned each frame would wait forever and exhaust the thread stacks.
            if !repeat {
                context.spawn(
                    jvm,
                    Box::new(ClipCompletionRunner {
                        clip: this,
                        completed,
                        stopped,
                    }),
                )?;
            }
        } else {
            let _: () = jvm.invoke_virtual(&player, "start", "(Z)V", (repeat,)).await?;
        }

        Ok(0)
    }

    async fn media_stop(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::mediaStop({this:?})");

        let player: ClassInstanceRef<Player> = jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        if player.is_null() {
            return Ok(-9);
        }

        let _: () = jvm.invoke_virtual(&player, "stop", "()V", ()).await?;

        Ok(0)
    }

    async fn media_get_volume(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::mediaGetVolume({this:?})");

        let player: ClassInstanceRef<Player> = jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        if player.is_null() {
            return Ok(-9);
        }

        jvm.invoke_virtual(&player, "getVolume", "()I", ()).await
    }

    async fn media_set_volume(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, volume: i32) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.media.BaseClip::mediaSetVolume({this:?}, {volume})");

        let player: ClassInstanceRef<Player> = jvm.get_field(&this, "player", "Ljavax/microedition/media/Player;").await?;

        if player.is_null() {
            return Ok(-9);
        }

        jvm.invoke_virtual(&player, "setVolume", "(I)I", (volume,)).await
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

struct ClipCompletionRunner {
    clip: ClassInstanceRef<BaseClip>,
    completed: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl MethodBody<JavaError, WieJvmContext> for ClipCompletionRunner {
    async fn call(&self, jvm: &Jvm, context: &mut WieJvmContext, _args: Box<[JavaValue]>) -> Result<JavaValue, JavaError> {
        jvm.attach_thread(None).await?;

        while !self.completed.load(Ordering::Acquire) {
            if self.stopped.load(Ordering::Relaxed) {
                return Ok(JavaValue::Void);
            }

            context.system().sleep(1).await;
        }

        if self.stopped.load(Ordering::Relaxed) {
            return Ok(JavaValue::Void);
        }

        let listener: ClassInstanceRef<PlayListener> = jvm.get_field(&self.clip, "playListener", "Lorg/kwis/msp/media/PlayListener;").await?;

        if !listener.is_null() {
            // The end-of-media callback method varies by the SDK a title was
            // built against, so invoke whichever form the listener actually
            // declares. Getting this wrong is fatal: 시드's listener implements
            // `playerUpdate(BaseClip, int, int)` and calling the absent
            // `playUpdate(Clip, int, int)` threw NoSuchMethodError and froze the
            // game the moment a clip finished (right after enabling sound).
            //
            //   1. `playerUpdate(BaseClip, int, int)` -> void  (KWIS PlayerListener; 시드)
            //   2. `playUpdate(int, int)` -> boolean           (reference PlayListener)
            //   3. `playUpdate(Clip, int, int)` -> void        (MIDP-style fallback)
            let definition = listener.class_definition();
            let clip = self.clip.clone();
            if definition.method("playerUpdate", "(Lorg/kwis/msp/media/BaseClip;II)V", false).is_some() {
                let _: () = jvm
                    .invoke_virtual(&listener, "playerUpdate", "(Lorg/kwis/msp/media/BaseClip;II)V", (clip, 1i32, 0i32))
                    .await?;
            } else if definition.method("playUpdate", "(II)Z", false).is_some() {
                let _: bool = jvm.invoke_virtual(&listener, "playUpdate", "(II)Z", (1i32, 0i32)).await?;
            } else {
                let _: () = jvm
                    .invoke_virtual(&listener, "playUpdate", "(Lorg/kwis/msp/media/Clip;II)V", (clip, 1i32, 0i32))
                    .await?;
            }
        }

        Ok(JavaValue::Void)
    }
}

#[cfg(test)]
mod test {
    use alloc::{boxed::Box, vec, vec::Vec};

    use java_class_proto::{JavaFieldProto, JavaMethodProto};
    use jvm::{ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};
    use test_utils::run_jvm_test;
    use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
    use wie_util::Result;

    use crate::{
        classes::org::kwis::msp::media::{Clip, PlayListener},
        get_protos,
    };

    struct CompletionListener;

    impl CompletionListener {
        fn as_proto() -> WieJavaClassProto {
            WieJavaClassProto {
                name: "test/CompletionListener",
                parent_class: Some("java/lang/Object"),
                interfaces: vec!["org/kwis/msp/media/PlayListener"],
                methods: vec![
                    JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                    JavaMethodProto::new("playUpdate", "(Lorg/kwis/msp/media/Clip;II)V", Self::play_update, Default::default()),
                ],
                fields: vec![
                    JavaFieldProto::new("count", "I", Default::default()),
                    JavaFieldProto::new("event", "I", Default::default()),
                    JavaFieldProto::new("param", "I", Default::default()),
                    JavaFieldProto::new("clip", "Lorg/kwis/msp/media/Clip;", Default::default()),
                ],
                access_flags: Default::default(),
            }
        }

        async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
            jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await
        }

        async fn play_update(
            jvm: &Jvm,
            _: &mut WieJvmContext,
            mut this: ClassInstanceRef<Self>,
            clip: ClassInstanceRef<Clip>,
            event: i32,
            param: i32,
        ) -> JvmResult<()> {
            let count: i32 = jvm.get_field(&this, "count", "I").await?;
            jvm.put_field(&mut this, "count", "I", count + 1).await?;
            jvm.put_field(&mut this, "event", "I", event).await?;
            jvm.put_field(&mut this, "param", "I", param).await?;
            jvm.put_field(&mut this, "clip", "Lorg/kwis/msp/media/Clip;", clip).await
        }
    }

    fn minimal_smaf() -> Vec<i8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"MMMD");
        data.extend_from_slice(&27u32.to_be_bytes());
        data.extend_from_slice(b"CNTI");
        data.extend_from_slice(&5u32.to_be_bytes());
        data.extend_from_slice(&[0, 0, 0, 0, 0]);
        data.extend_from_slice(b"XXXX");
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&0u16.to_be_bytes());
        data.into_iter().map(|x| x as i8).collect()
    }

    #[test]
    fn test_natural_completion_calls_legacy_play_listener_once() -> Result<()> {
        run_jvm_test(
            Box::new([
                wie_midp::get_protos().into(),
                get_protos().into(),
                [CompletionListener::as_proto()].into(),
            ]),
            |jvm| async move {
                let r#type = JavaLangString::from_rust_string(&jvm, "audio/test").await?;
                let bytes = minimal_smaf();
                let mut data = jvm.instantiate_array("B", bytes.len()).await?;
                jvm.store_array(&mut data, 0, bytes).await?;

                let clip: ClassInstanceRef<Clip> = jvm
                    .new_class("org/kwis/msp/media/Clip", "(Ljava/lang/String;[B)V", (r#type, data))
                    .await?
                    .into();

                let listener: ClassInstanceRef<PlayListener> = jvm.new_class("test/CompletionListener", "()V", ()).await?.into();

                let listener_state = listener.clone();

                let _: () = jvm
                    .invoke_virtual(&clip, "setListener", "(Lorg/kwis/msp/media/PlayListener;)V", (listener,))
                    .await?;

                let result: i32 = jvm.invoke_virtual(&clip, "mediaPlay", "(Z)I", (false,)).await?;
                assert_eq!(result, 0);

                for _ in 0..100 {
                    let count: i32 = jvm.get_field(&listener_state, "count", "I").await?;
                    if count != 0 {
                        break;
                    }
                    let _: () = jvm.invoke_static("java/lang/Thread", "yield", "()V", ()).await?;
                }

                let count: i32 = jvm.get_field(&listener_state, "count", "I").await?;
                let event: i32 = jvm.get_field(&listener_state, "event", "I").await?;
                let param: i32 = jvm.get_field(&listener_state, "param", "I").await?;
                let callback_clip: ClassInstanceRef<Clip> = jvm.get_field(&listener_state, "clip", "Lorg/kwis/msp/media/Clip;").await?;

                assert_eq!(count, 1);
                assert_eq!(event, 1);
                assert_eq!(param, 0);
                assert_eq!(callback_clip.identity(), clip.identity());

                Ok(())
            },
        )
    }

    /// A listener in the reference LGT/KWIS form: `playUpdate(int, int)` -> Z.
    struct KwisCompletionListener;

    impl KwisCompletionListener {
        fn as_proto() -> WieJavaClassProto {
            WieJavaClassProto {
                name: "test/KwisCompletionListener",
                parent_class: Some("java/lang/Object"),
                interfaces: vec!["org/kwis/msp/media/PlayListener"],
                methods: vec![
                    JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                    JavaMethodProto::new("playUpdate", "(II)Z", Self::play_update, Default::default()),
                ],
                fields: vec![
                    JavaFieldProto::new("count", "I", Default::default()),
                    JavaFieldProto::new("event", "I", Default::default()),
                    JavaFieldProto::new("param", "I", Default::default()),
                ],
                access_flags: Default::default(),
            }
        }

        async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
            jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await
        }

        async fn play_update(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, event: i32, param: i32) -> JvmResult<bool> {
            let count: i32 = jvm.get_field(&this, "count", "I").await?;
            jvm.put_field(&mut this, "count", "I", count + 1).await?;
            jvm.put_field(&mut this, "event", "I", event).await?;
            jvm.put_field(&mut this, "param", "I", param).await?;
            Ok(false)
        }
    }

    #[test]
    fn test_natural_completion_calls_kwis_play_listener_once() -> Result<()> {
        run_jvm_test(
            Box::new([
                wie_midp::get_protos().into(),
                get_protos().into(),
                [KwisCompletionListener::as_proto()].into(),
            ]),
            |jvm| async move {
                let r#type = JavaLangString::from_rust_string(&jvm, "audio/test").await?;
                let bytes = minimal_smaf();
                let mut data = jvm.instantiate_array("B", bytes.len()).await?;
                jvm.store_array(&mut data, 0, bytes).await?;

                let clip: ClassInstanceRef<Clip> = jvm
                    .new_class("org/kwis/msp/media/Clip", "(Ljava/lang/String;[B)V", (r#type, data))
                    .await?
                    .into();

                let listener: ClassInstanceRef<PlayListener> = jvm.new_class("test/KwisCompletionListener", "()V", ()).await?.into();
                let listener_state = listener.clone();

                let _: () = jvm
                    .invoke_virtual(&clip, "setListener", "(Lorg/kwis/msp/media/PlayListener;)V", (listener,))
                    .await?;

                let result: i32 = jvm.invoke_virtual(&clip, "mediaPlay", "(Z)I", (false,)).await?;
                assert_eq!(result, 0);

                for _ in 0..100 {
                    let count: i32 = jvm.get_field(&listener_state, "count", "I").await?;
                    if count != 0 {
                        break;
                    }
                    let _: () = jvm.invoke_static("java/lang/Thread", "yield", "()V", ()).await?;
                }

                // The clip finishing invokes the (II)Z form exactly once instead
                // of throwing NoSuchMethodError for the Clip-taking overload.
                let count: i32 = jvm.get_field(&listener_state, "count", "I").await?;
                let event: i32 = jvm.get_field(&listener_state, "event", "I").await?;
                let param: i32 = jvm.get_field(&listener_state, "param", "I").await?;

                assert_eq!(count, 1);
                assert_eq!(event, 1);
                assert_eq!(param, 0);

                Ok(())
            },
        )
    }

    /// A listener in the KWIS PlayerListener form 시드 uses:
    /// `playerUpdate(BaseClip, int, int)` -> void.
    struct PlayerUpdateListener;

    impl PlayerUpdateListener {
        fn as_proto() -> WieJavaClassProto {
            WieJavaClassProto {
                name: "test/PlayerUpdateListener",
                parent_class: Some("java/lang/Object"),
                interfaces: vec!["org/kwis/msp/media/PlayListener"],
                methods: vec![
                    JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                    JavaMethodProto::new(
                        "playerUpdate",
                        "(Lorg/kwis/msp/media/BaseClip;II)V",
                        Self::player_update,
                        Default::default(),
                    ),
                ],
                fields: vec![
                    JavaFieldProto::new("count", "I", Default::default()),
                    JavaFieldProto::new("event", "I", Default::default()),
                    JavaFieldProto::new("param", "I", Default::default()),
                ],
                access_flags: Default::default(),
            }
        }

        async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
            jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await
        }

        async fn player_update(
            jvm: &Jvm,
            _: &mut WieJvmContext,
            mut this: ClassInstanceRef<Self>,
            _clip: ClassInstanceRef<super::BaseClip>,
            event: i32,
            param: i32,
        ) -> JvmResult<()> {
            let count: i32 = jvm.get_field(&this, "count", "I").await?;
            jvm.put_field(&mut this, "count", "I", count + 1).await?;
            jvm.put_field(&mut this, "event", "I", event).await?;
            jvm.put_field(&mut this, "param", "I", param).await
        }
    }

    #[test]
    fn test_natural_completion_calls_player_update_listener_once() -> Result<()> {
        run_jvm_test(
            Box::new([
                wie_midp::get_protos().into(),
                get_protos().into(),
                [PlayerUpdateListener::as_proto()].into(),
            ]),
            |jvm| async move {
                let r#type = JavaLangString::from_rust_string(&jvm, "audio/test").await?;
                let bytes = minimal_smaf();
                let mut data = jvm.instantiate_array("B", bytes.len()).await?;
                jvm.store_array(&mut data, 0, bytes).await?;

                let clip: ClassInstanceRef<Clip> = jvm
                    .new_class("org/kwis/msp/media/Clip", "(Ljava/lang/String;[B)V", (r#type, data))
                    .await?
                    .into();

                let listener: ClassInstanceRef<PlayListener> = jvm.new_class("test/PlayerUpdateListener", "()V", ()).await?.into();
                let listener_state = listener.clone();

                let _: () = jvm
                    .invoke_virtual(&clip, "setListener", "(Lorg/kwis/msp/media/PlayListener;)V", (listener,))
                    .await?;

                let result: i32 = jvm.invoke_virtual(&clip, "mediaPlay", "(Z)I", (false,)).await?;
                assert_eq!(result, 0);

                for _ in 0..100 {
                    let count: i32 = jvm.get_field(&listener_state, "count", "I").await?;
                    if count != 0 {
                        break;
                    }
                    let _: () = jvm.invoke_static("java/lang/Thread", "yield", "()V", ()).await?;
                }

                // The completion invokes playerUpdate(BaseClip, int, int) instead
                // of throwing NoSuchMethodError for the playUpdate overloads.
                let count: i32 = jvm.get_field(&listener_state, "count", "I").await?;
                let event: i32 = jvm.get_field(&listener_state, "event", "I").await?;
                let param: i32 = jvm.get_field(&listener_state, "param", "I").await?;

                assert_eq!(count, 1);
                assert_eq!(event, 1);
                assert_eq!(param, 0);

                Ok(())
            },
        )
    }
}
