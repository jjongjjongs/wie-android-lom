use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};

use bytemuck::{Pod, Zeroable};
use core::sync::atomic::{AtomicBool, Ordering};

use wipi_types::wipic::WIPICWord;

use wie_util::{Result, WieError, read_generic, write_generic};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};

/// Top of the `MC_mdaClipSetVolume` range, and what a clip with no level of its
/// own reads back as.
const FULL_VOLUME: u8 = 100;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MdaClip {
    clip_id: i32,
    h_proc: i32,
    r#type: u8,
    in_use: u8, // bool
    _padding1: [u8; 2],
    dev_id: i32,

    x: i32,
    y: i32,
    w: i32,
    h: i32,
    mute: u8, // bool
    _padding2: [u8; 3],
    watermark: i32,
    position: i32,
    quality: i32,
    mode: i32,
    state: i32,
    penpot: i32,
    num_slave: i32,

    clip_save: WIPICWord, // MC_MdaClip**

    audio_tone_saved_len: i32,
    audio_tone_len: i32,
    audio_tone: WIPICWord,          // MC_MdaToneType*
    audio_tone_duration: WIPICWord, // M_Int32 *

    audio_freq_saved_len: i32,
    audio_freq_len: i32,
    audio_hi_freq: WIPICWord,       // M_Int32 *
    audio_low_freq: WIPICWord,      // M_Int32 *
    audio_freq_duration: WIPICWord, // M_Int32 *

    sound_data_saved_len: i32,
    sound_data_len: i32,
    sound_data: WIPICWord, // M_Byte *

    original_volume: i32,

    pos: i8,
    _padding3: [u8; 3],
    codec_config_data_size: i32,
    codec_config_data: WIPICWord, // M_Byte *
    tick_duration: i32,

    b_control: u8, // bool
    _padding4: [u8; 3],

    movie_record_size_width: i32,
    movie_record_size_height: i32,
    max_record_length: i32,

    temp_record_space: WIPICWord, // M_Byte *
    temp_record_space_size: i32,
    temp_record_size: i32,

    next_ptr: WIPICWord, // MC_MdaClip*

    mda_id: i32,
    device_info: i32,

    // not in sdk, for internal usage
    handle: u32,
}

pub async fn clip_create(context: &mut dyn WIPICContext, ptr_type: WIPICWord, buf_size: WIPICWord, callback: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipCreate({ptr_type:#x}, {buf_size:#x}, {callback:#x})");

    let clip_address = context.alloc_raw(size_of::<MdaClip>() as u32)?;
    let clip = MdaClip {
        h_proc: callback as i32,
        in_use: 1,
        ..MdaClip::zeroed()
    };
    write_generic(context, clip_address, clip)?;

    tracing::info!("[media] MC_mdaClipCreate(type={ptr_type:#x}, buf_size={buf_size:#x}, cb={callback:#x}) -> clip {clip_address:#x}");

    Ok(clip_address)
}

pub async fn clip_free(context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipFree({clip:#x})");

    // some app call clip free with null clip...
    if clip == 0 {
        return Ok(0);
    }

    context.free_raw(clip, size_of::<MdaClip>() as u32)?;

    Ok(0)
}

pub async fn clip_get_type(_context: &mut dyn WIPICContext, clip: WIPICWord, buf: WIPICWord, buf_size: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaClipGetType({clip:#x}, {buf:#x}, {buf_size:#x})");

    Ok(0)
}

pub async fn get_mute_state(_context: &mut dyn WIPICContext, source: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaGetMuteState({source:#x})");

    Ok(0)
}

pub async fn clip_get_info(
    _context: &mut dyn WIPICContext,
    clip: WIPICWord,
    command: WIPICWord,
    buf: WIPICWord,
    buf_size: WIPICWord,
) -> Result<WIPICWord> {
    tracing::warn!("stub OEMC_mdaClipGetInfo({clip:#x}, {command:#x}, {buf:#x}, {buf_size:#x})");

    Ok(0)
}

pub async fn clip_put_data(context: &mut dyn WIPICContext, ptr_clip: WIPICWord, buf: WIPICWord, buf_size: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_mdaClipPutData({ptr_clip:#x}, {buf:#x}, {buf_size:#x})");

    let mut data = vec![0; buf_size as _];
    context.read_bytes(buf, &mut data)?;

    // First bytes help identify the format (SMAF "MMMD", etc.) when a title's
    // clips fail to load and stay silent.
    let magic: Vec<u8> = data.iter().take(4).copied().collect();
    let handle = context.system().audio().load_smaf(&data);
    if let Err(x) = handle {
        tracing::error!("[media] MC_mdaClipPutData(clip={ptr_clip:#x}, size={buf_size:#x}, magic={magic:02x?}) load_smaf FAILED: {x:?}");
        return Ok(0);
    }

    let handle = handle.unwrap();
    tracing::info!("[media] MC_mdaClipPutData(clip={ptr_clip:#x}, size={buf_size:#x}, magic={magic:02x?}) load_smaf -> handle {handle:#x}");

    // Titles that never allocate a clip object load their audio under the
    // implicit clip 0; bind the loaded handle to the default player so the
    // clip-0 play/volume/stop paths can reach it (otherwise every such effect is
    // silent). Titles that use real clip objects store the handle in the object.
    if ptr_clip == 0 {
        context.system().audio().set_default_clip(handle);
        return Ok(buf_size as _);
    }

    let mut clip: MdaClip = read_generic(context, ptr_clip)?;
    clip.handle = handle;
    write_generic(context, ptr_clip, clip)?;

    Ok(buf_size as _)
}

pub async fn clip_get_data(_context: &mut dyn WIPICContext, clip: WIPICWord, buf: WIPICWord, buf_size: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaClipGetData({clip:#x}, {buf:#x}, {buf_size:#x})");

    Ok(0)
}

pub async fn clip_set_position(_context: &mut dyn WIPICContext, clip: WIPICWord, ms: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaClipSetPosition({clip:#x}, {ms:#x})");

    Ok(0)
}

/// The level a clip is at, which is the level [`clip_set_volume`] last routed
/// to its handle.
///
/// This answered 0 as a stub, and 0 is the one answer that silences a title:
/// they read the level to put it back. 영웅서기5 sets a clip to 20, reads it, and
/// sets what it read - so every effect it loaded played at zero, and the game
/// ran mute. A clip whose data has not been loaded yet has no level of its own,
/// so answer full scale rather than the silence a zero would restore.
pub async fn clip_get_volume(context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    let handle = if clip == 0 {
        // Default-player titles ask about the clip-0 handle, as they set it.
        context.system().audio().default_clip()
    } else {
        // A clip record is zeroed at creation and only gets its handle from
        // `MC_mdaClipPutData`, so a 0 here is "nothing loaded" - no handle is
        // ever 0.
        let mda_clip: MdaClip = read_generic(context, clip)?;
        (mda_clip.handle != 0).then_some(mda_clip.handle)
    };

    let level = handle
        .and_then(|handle| context.system().audio().get_volume(handle).ok())
        .unwrap_or(FULL_VOLUME);

    tracing::info!("[media] MC_mdaClipGetVolume(clip={clip:#x}) handle={handle:?} level={level}");

    Ok(level as WIPICWord)
}

pub async fn clip_set_volume(context: &mut dyn WIPICContext, clip: WIPICWord, volume: WIPICWord) -> Result<WIPICWord> {
    if clip == 0 {
        // Default-player titles set the volume of the clip-0 handle.
        let default = context.system().audio().default_clip();
        if let Some(handle) = default {
            let level = (volume & 0xFF).min(FULL_VOLUME as WIPICWord) as u8;
            let _ = context.system().audio().set_volume(handle, level);
        }
        return Ok(0);
    }

    // The clip carries the audio handle its data was loaded under; route the
    // requested level (0..=100) to that handle so the rendered stream plays at
    // the volume the title asked for. Titles set this well below full scale
    // (Zenonia uses 50) to leave headroom, and honouring it keeps a bright,
    // near-full-scale sequence from being driven into the output limiter and
    // sounding harsh.
    let mda_clip: MdaClip = read_generic(context, clip)?;
    let handle = mda_clip.handle;
    let level = (volume & 0xFF).min(FULL_VOLUME as WIPICWord) as u8;
    tracing::info!("[media] MC_mdaClipSetVolume(clip={clip:#x}, volume={volume:#x}) handle={handle:#x} level={level}");

    let _ = context.system().audio().set_volume(handle, level);

    Ok(0)
}

/// The handset's media volume, which nothing here models.
///
/// Answer full scale for the same reason [`clip_get_volume`] does: a title that
/// reads this reads it to restore it, or to decide whether there is any point
/// playing at all, and a zero tells it the handset is muted.
pub async fn get_volume(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaGetVolume -> {FULL_VOLUME}");

    Ok(FULL_VOLUME as WIPICWord)
}

pub async fn play(context: &mut dyn WIPICContext, ptr_clip: WIPICWord, repeat: WIPICWord) -> Result<i32> {
    if ptr_clip == 0 {
        // Default-player titles (clip 0) play the handle their MC_mdaClipPutData
        // bound to the default clip. No clip object means no completion
        // callback; those titles drive stop/replay themselves.
        let default = context.system().audio().default_clip();
        if let Some(handle) = default {
            tracing::info!("[media] MC_mdaPlay(clip=0, repeat={repeat}) default handle={handle:#x}");
            let system = context.system();
            if let Err(error) = system.audio().play_with_completion(system, handle, repeat != 0) {
                tracing::error!("Failed to play default clip: {error:?}");
            }
        }
        return Ok(0);
    }

    let clip: MdaClip = read_generic(context, ptr_clip)?;
    let callback = clip.h_proc as WIPICWord;
    tracing::info!("[media] MC_mdaPlay(clip={ptr_clip:#x}, repeat={repeat}) handle={:#x}", clip.handle);

    let completed = {
        let system = context.system();
        system.audio().play_with_completion(system, clip.handle, repeat != 0)
    };

    let (completed, stopped) = match completed {
        Ok(status) => status,
        Err(error) => {
            tracing::error!("Failed to play audio: {error:?}");
            return Ok(0);
        }
    };

    if callback != 0 && repeat == 0 {
        /// How often a clip's completion flag is read while it plays.
        const COMPLETION_POLL_PERIOD: u64 = 16;

        struct PlaybackCompletedCallback {
            completed: Arc<AtomicBool>,
            stopped: Arc<AtomicBool>,
            callback: WIPICWord,
            clip: WIPICWord,
        }

        #[async_trait::async_trait]
        impl MethodBody<WieError> for PlaybackCompletedCallback {
            async fn call(&self, context: &mut dyn WIPICContext, _: Box<[WIPICWord]>) -> Result<WIPICResult> {
                while !self.completed.load(Ordering::Acquire) && !self.stopped.load(Ordering::Acquire) {
                    // A frame, not a millisecond: the audio layer writes these
                    // flags from a watcher of its own that polls at 50ms, so a
                    // finer read cannot see anything sooner and only takes
                    // executor time away from the guest.
                    context.system().sleep(COMPLETION_POLL_PERIOD).await;
                }

                if self.stopped.load(Ordering::Acquire) {
                    tracing::debug!("MC_mdaPlay completion callback cancelled for stopped clip {:#x}", self.clip);

                    return Ok(WIPICResult { results: Vec::new() });
                }

                tracing::debug!("MC_mdaPlay completion callback({:#x}, event=3)", self.callback);
                context.call_function(self.callback, &[self.clip, 3]).await?;

                Ok(WIPICResult { results: Vec::new() })
            }
        }

        context.spawn(Box::new(PlaybackCompletedCallback {
            completed,
            stopped,
            callback,
            clip: ptr_clip,
        }))?;
    }

    Ok(0)
}

/// `MC_mdaClipControl` (WIPI-C index `0x4b6`) — the player-path play/control
/// call that titles like Zenonia use instead of `MC_mdaPlay`. The sequence is
/// `ClipAllocPlayer` → `ClipSetVolume` → `ClipControl(clip, cmd, …)` →
/// `ClipFreePlayer`; the clip's audio was already loaded by `ClipPutData`
/// (`load_smaf`). Without this the loaded clip is never played and those effects
/// are silent. `cmd` selects the play mode: `0x31` loops (BGM), `0x30` plays
/// once (SFX); other commands are logged and ignored for now.
/// `MC_mdaClipClearData` (service 0x4b6), which drops whatever a clip has
/// buffered.
///
/// A clip's data arrives whole through `MC_mdaClipPutData` and is decoded into
/// an audio handle there, so there is no partial buffer to drop and the handle
/// stays valid for the replay that follows. Accepted and logged: this is the
/// service 0x4b6 actually is - it was routed to `clip_control` and logged under
/// that name until the reference firmware's service table gave both their real
/// numbers (`MC_mdaClipControl` is 0x4ca).
pub async fn clip_clear_data(_context: &mut dyn WIPICContext, clip: WIPICWord, a1: WIPICWord, a2: WIPICWord, a3: WIPICWord) -> Result<WIPICWord> {
    tracing::info!("[media] MC_mdaClipClearData(clip={clip:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    Ok(0)
}

pub async fn clip_control(_context: &mut dyn WIPICContext, clip: WIPICWord, cmd: WIPICWord, arg1: WIPICWord, arg2: WIPICWord) -> Result<WIPICWord> {
    // Diagnostic (INFO): playing here blindly double-triggered clips the game
    // also drives another way and layered them, so this is a logging no-op for
    // now. The media-path INFO trace (clip_create/put_data/play) shows the real
    // protocol; the player path is wired for real once that is understood.
    tracing::info!("[media] MC_mdaClipControl(clip={clip:#x}, cmd={cmd:#x}, arg1={arg1:#x}, arg2={arg2:#x})");

    Ok(0)
}

pub async fn clip_alloc_player(_context: &mut dyn WIPICContext, clip: WIPICWord, param: WIPICWord) -> Result<WIPICWord> {
    // Returning a non-null handle here makes titles read the player as "already
    // set up" and skip the following MC_mdaPlay, leaving the logo jingle and
    // looping BGM silent. Keep the validated stub so MC_mdaPlay drives playback.
    tracing::warn!("stub MC_mdaClipAllocPlayer({clip:#x}, {param:#x})");

    Ok(0)
}

pub async fn clip_free_player(context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    // Titles free the player to stop the clip - the game's stop for a looping
    // track. Without this a `repeat=1` BGM plays forever and every new track
    // stacks another endless loop on top (garbled, ever-louder audio); the
    // player-path stop (ClipControl + this) is where the sequence is meant to
    // end. Stop the clip's playback by its loaded handle.
    if clip == 0 {
        return Ok(0);
    }

    let mda_clip: MdaClip = read_generic(context, clip)?;
    let handle = mda_clip.handle;
    tracing::info!("[media] MC_mdaClipFreePlayer(clip={clip:#x}) stop handle {handle:#x}");
    context.system().audio().stop(handle);

    Ok(0)
}

pub async fn vibrator(context: &mut dyn WIPICContext, level: i32, timeout: i32) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaVibrator({level}, {timeout})");

    let duration_ms = timeout.max(0) as u64;
    let intensity = (level.clamp(0, 10) * 10) as u8;
    context.system().platform().vibrate(duration_ms, intensity);

    Ok(0)
}

pub async fn set_mute_state(_context: &mut dyn WIPICContext, source: i32, b_mute: i32) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaSetMuteState({source:#x}, {b_mute})");

    Ok(0)
}

pub async fn pause(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaPause({clip:#x})");

    Ok(0)
}

pub async fn resume(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaResume({clip:#x})");

    Ok(0)
}

pub async fn stop(context: &mut dyn WIPICContext, ptr_clip: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaStop({ptr_clip:#x})");

    if ptr_clip == 0 {
        // Default-player titles stop the clip-0 handle before loading the next.
        let default = context.system().audio().default_clip();
        if let Some(handle) = default {
            context.system().audio().stop(handle);
        }
        return Ok(0);
    }

    let clip: MdaClip = read_generic(context, ptr_clip)?;
    let callback = clip.h_proc as WIPICWord;

    // Whether this clip was actually playing lives in the backend registry, not
    // in the guest ABI struct, so a stop only reports the interruption when it
    // really tears down a running playback.
    let was_playing = context.system().audio().is_playing(clip.handle);

    context.system().audio().stop(clip.handle);

    if was_playing && callback != 0 {
        tracing::debug!("MC_mdaStop callback({callback:#x}, clip={ptr_clip:#x}, event=-1)");
        context.call_function(callback, &[ptr_clip, (-1i32) as WIPICWord]).await?;
    }

    Ok(0)
}

pub async fn record(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaRecord({clip:#x})");

    Ok(0)
}

pub async fn unk7(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaUnk7({clip:#x})");

    Ok(0)
}

pub async fn unk17(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaUnk17({clip:#x})");

    Ok(0)
}

pub async fn unk18(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaUnk18({clip:#x})");

    Ok(0)
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;

    use test_utils::TestPlatform;
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::ByteWrite;

    use crate::context::test::TestContext;

    use super::{FULL_VOLUME, clip_create, clip_get_volume, clip_put_data, clip_set_volume};

    fn test_context() -> TestContext {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        TestContext::with_system(system)
    }

    /// 영웅서기5's own sequence: it sets an effect's level, reads it back, and
    /// sets what it read. While the read answered 0 every effect it loaded
    /// played at zero and the game ran mute.
    #[futures_test::test]
    async fn a_clip_reads_back_the_level_it_was_set_to() {
        let mut context = test_context();

        let clip = clip_create(&mut context, 0, 0x793, 0).await.unwrap();
        context.write_bytes(0x1000, b"MMMD\0\0\0\0").unwrap();
        assert_eq!(clip_put_data(&mut context, clip, 0x1000, 8).await.unwrap(), 8);

        assert_eq!(clip_set_volume(&mut context, clip, 20).await.unwrap(), 0);
        assert_eq!(clip_get_volume(&mut context, clip).await.unwrap(), 20);

        // And what it reads is what it can set back.
        let level = clip_get_volume(&mut context, clip).await.unwrap();
        assert_eq!(clip_set_volume(&mut context, clip, level).await.unwrap(), 0);
        assert_eq!(clip_get_volume(&mut context, clip).await.unwrap(), 20);
    }

    /// A clip with nothing loaded has no level of its own. Answer full scale:
    /// a title restoring what it read must not restore silence. It must also
    /// not read back some other clip's level - a clip record is zeroed at
    /// creation, so the handle it has not been given yet must not name one.
    #[futures_test::test]
    async fn a_clip_with_no_data_reads_back_full_volume() {
        let mut context = test_context();

        let loaded = clip_create(&mut context, 0, 0x793, 0).await.unwrap();
        context.write_bytes(0x1000, b"MMMD\0\0\0\0").unwrap();
        clip_put_data(&mut context, loaded, 0x1000, 8).await.unwrap();
        clip_set_volume(&mut context, loaded, 20).await.unwrap();

        let empty = clip_create(&mut context, 0, 0x793, 0).await.unwrap();

        assert_eq!(clip_get_volume(&mut context, empty).await.unwrap(), FULL_VOLUME as _);
    }
}
