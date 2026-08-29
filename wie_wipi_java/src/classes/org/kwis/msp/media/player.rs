use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::media::{BaseClip, Clip};

// class org.kwis.msp.media.Player
pub struct Player;

impl Player {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/media/Player",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("pause", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::pause, MethodAccessFlags::STATIC),
                JavaMethodProto::new("stop", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::stop, MethodAccessFlags::STATIC),
                JavaMethodProto::new("resume", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::resume, MethodAccessFlags::STATIC),
                JavaMethodProto::new("play", "(Lorg/kwis/msp/media/BaseClip;Z)Z", Self::play, MethodAccessFlags::STATIC),
                JavaMethodProto::new("record", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::record, MethodAccessFlags::STATIC),
                JavaMethodProto::new("play", "(Lorg/kwis/msp/media/Clip;Z)Z", Self::play_clip, MethodAccessFlags::STATIC),
                JavaMethodProto::new("stop", "(Lorg/kwis/msp/media/Clip;)Z", Self::stop_clip, MethodAccessFlags::STATIC),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn pause(_: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.media.Player::pause({clip:?})");

        Ok(false)
    }

    async fn stop(_: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.media.Player::stop({clip:?})");

        Ok(false)
    }

    async fn resume(_: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.media.Player::resume({clip:?})");

        Ok(false)
    }

    async fn play(_: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>, repeat: bool) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.media.Player::play({clip:?}, {repeat})");

        Ok(false)
    }

    async fn record(_: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.media.Player::record({clip:?})");

        Ok(false)
    }

    async fn play_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>, repeat: bool) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.media.Player::play({clip:?}, {repeat})");

        let alloc_result: i32 = jvm.invoke_virtual(&clip, "allocPlayer", "()I", ()).await?;
        if alloc_result != 0 {
            return Err(jvm.exception("org/kwis/msp/media/MediaUnavailableException", "").await);
        }

        let play_result: i32 = jvm.invoke_virtual(&clip, "mediaPlay", "(Z)I", (repeat,)).await?;

        match play_result {
            0 => Ok(true),
            -16 | -9 | -7 | -6 | -1 => Err(jvm.exception("org/kwis/msp/media/MediaUnavailableException", "").await),
            _ => Ok(false),
        }
    }

    async fn stop_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.media.Player::stop({clip:?})");

        let result: i32 = jvm.invoke_virtual(&clip, "mediaStop", "()I", ()).await?;

        Ok(result >= 0)
    }
}

#[cfg(test)]
mod test {
    use alloc::boxed::Box;

    use java_runtime::classes::java::lang::String;
    use jvm::{ClassInstanceRef, JavaError, runtime::JavaLangString};
    use test_utils::run_jvm_test;
    use wie_util::Result;

    use crate::{
        classes::org::kwis::msp::media::{BaseClip, Clip},
        get_protos,
    };

    #[test]
    fn test_base_clip_overloads_return_false() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let clip: ClassInstanceRef<BaseClip> = jvm.new_class("org/kwis/msp/media/BaseClip", "()V", ()).await?.into();

            let paused: bool = jvm
                .invoke_static("org/kwis/msp/media/Player", "pause", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip.clone(),))
                .await?;
            let stopped: bool = jvm
                .invoke_static("org/kwis/msp/media/Player", "stop", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip.clone(),))
                .await?;
            let resumed: bool = jvm
                .invoke_static("org/kwis/msp/media/Player", "resume", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip.clone(),))
                .await?;
            let played: bool = jvm
                .invoke_static(
                    "org/kwis/msp/media/Player",
                    "play",
                    "(Lorg/kwis/msp/media/BaseClip;Z)Z",
                    (clip.clone(), true),
                )
                .await?;
            let recorded: bool = jvm
                .invoke_static("org/kwis/msp/media/Player", "record", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip,))
                .await?;

            assert!(!paused);
            assert!(!stopped);
            assert!(!resumed);
            assert!(!played);
            assert!(!recorded);

            Ok(())
        })
    }

    #[test]
    fn test_clip_compatibility_overloads_remain() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let r#type: ClassInstanceRef<String> = JavaLangString::from_rust_string(&jvm, "audio/test").await?.into();
            let clip: ClassInstanceRef<Clip> = jvm.new_class("org/kwis/msp/media/Clip", "(Ljava/lang/String;)V", (r#type,)).await?.into();

            let alloc_result: i32 = jvm.invoke_virtual(&clip, "allocPlayer", "()I", ()).await?;
            let media_stop_result: i32 = jvm.invoke_virtual(&clip, "mediaStop", "()I", ()).await?;

            assert_eq!(alloc_result, -9);
            assert_eq!(media_stop_result, -9);

            let play_result: core::result::Result<bool, JavaError> = jvm
                .invoke_static(
                    "org/kwis/msp/media/Player",
                    "play",
                    "(Lorg/kwis/msp/media/Clip;Z)Z",
                    (clip.clone(), false),
                )
                .await;

            let JavaError::JavaException(exception) = play_result.expect_err("unbuffered Clip play must throw");
            assert!(jvm.is_instance(&*exception, "org/kwis/msp/media/MediaUnavailableException"));
            assert!(jvm.is_instance(&*exception, "java/lang/RuntimeException"));

            let stopped: bool = jvm
                .invoke_static("org/kwis/msp/media/Player", "stop", "(Lorg/kwis/msp/media/Clip;)Z", (clip,))
                .await?;
            assert!(!stopped);

            Ok(())
        })
    }
}
