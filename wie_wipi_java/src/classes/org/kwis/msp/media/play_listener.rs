use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

use crate::classes::org::kwis::msp::media::{BaseClip, Clip};

/// What a title extends to hear back from a clip it is playing.
///
/// The reference builds it on [`Player`](super::Player) - it carries the same
/// static play/stop/pause/resume/record - and adds `playerUpdate`, which the
/// player calls as a clip starts, ends or fails. A title overrides that one;
/// the rest it inherits, and each here forwards to `Player` so a title calling
/// them through this name gets the same answer.
// class org.kwis.msp.media.PlayListener
pub struct PlayListener;

impl PlayListener {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/media/PlayListener",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "playerUpdate",
                    "(Lorg/kwis/msp/media/BaseClip;II)V",
                    Self::player_update,
                    Default::default(),
                ),
                JavaMethodProto::new("play", "(Lorg/kwis/msp/media/BaseClip;Z)Z", Self::play, MethodAccessFlags::STATIC),
                JavaMethodProto::new("play", "(Lorg/kwis/msp/media/Clip;Z)Z", Self::play_clip, MethodAccessFlags::STATIC),
                JavaMethodProto::new("stop", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::stop, MethodAccessFlags::STATIC),
                JavaMethodProto::new("stop", "(Lorg/kwis/msp/media/Clip;)Z", Self::stop_clip, MethodAccessFlags::STATIC),
                JavaMethodProto::new("pause", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::pause, MethodAccessFlags::STATIC),
                JavaMethodProto::new("pause", "(Lorg/kwis/msp/media/Clip;)Z", Self::pause_clip, MethodAccessFlags::STATIC),
                JavaMethodProto::new("resume", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::resume, MethodAccessFlags::STATIC),
                JavaMethodProto::new("resume", "(Lorg/kwis/msp/media/Clip;)Z", Self::resume_clip, MethodAccessFlags::STATIC),
                JavaMethodProto::new("record", "(Lorg/kwis/msp/media/BaseClip;)Z", Self::record, MethodAccessFlags::STATIC),
                JavaMethodProto::new("record", "(Lorg/kwis/msp/media/Clip;)Z", Self::record_clip, MethodAccessFlags::STATIC),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.media.PlayListener::<init>({this:?})");

        Ok(())
    }

    /// The callback a title overrides. Nothing calls it while the audio backend
    /// reports no clip events, so the one here only stands in for a title that
    /// leaves it out.
    async fn player_update(
        _: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        clip: ClassInstanceRef<BaseClip>,
        event: i32,
        param: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.media.PlayListener::playerUpdate({this:?}, {clip:?}, {event}, {param})");

        Ok(())
    }

    async fn play(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>, repeat: bool) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "play", "(Lorg/kwis/msp/media/BaseClip;Z)Z", (clip, repeat))
            .await
    }

    async fn play_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>, repeat: bool) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "play", "(Lorg/kwis/msp/media/Clip;Z)Z", (clip, repeat))
            .await
    }

    async fn stop(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "stop", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip,))
            .await
    }

    async fn stop_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "stop", "(Lorg/kwis/msp/media/Clip;)Z", (clip,))
            .await
    }

    async fn pause(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "pause", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip,))
            .await
    }

    async fn pause_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "pause", "(Lorg/kwis/msp/media/Clip;)Z", (clip,))
            .await
    }

    async fn resume(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "resume", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip,))
            .await
    }

    async fn resume_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "resume", "(Lorg/kwis/msp/media/Clip;)Z", (clip,))
            .await
    }

    async fn record(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<BaseClip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "record", "(Lorg/kwis/msp/media/BaseClip;)Z", (clip,))
            .await
    }

    async fn record_clip(jvm: &Jvm, _: &mut WieJvmContext, clip: ClassInstanceRef<Clip>) -> JvmResult<bool> {
        jvm.invoke_static("org/kwis/msp/media/Player", "record", "(Lorg/kwis/msp/media/Clip;)Z", (clip,))
            .await
    }
}
