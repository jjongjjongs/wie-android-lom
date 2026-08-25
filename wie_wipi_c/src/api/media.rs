use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};

use bytemuck::{Pod, Zeroable};
use core::sync::atomic::{AtomicBool, Ordering};

use wipi_types::wipic::WIPICWord;

use wie_util::{Result, WieError, read_generic, write_generic};

use crate::{WIPICResult, context::WIPICContext, method::MethodBody};

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

    if ptr_clip == 0 {
        return Ok(-1);
    }

    let mut data = vec![0; buf_size as _];
    context.read_bytes(buf, &mut data)?;

    let handle = context.system().audio().load_smaf(&data);
    if let Err(x) = handle {
        tracing::error!("Failed to load audio: {x:?}");
        return Ok(0);
    }

    let handle = handle.unwrap();

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

pub async fn clip_get_volume(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaClipGetVolume({clip:#x})");

    Ok(0)
}

pub async fn clip_set_volume(_context: &mut dyn WIPICContext, clip: WIPICWord, volume: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaClipSetVolume({clip:#x}, {volume:#x})");

    Ok(0)
}

pub async fn get_volume(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaGetVolume");

    Ok(0)
}

pub async fn play(context: &mut dyn WIPICContext, ptr_clip: WIPICWord, repeat: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_mdaPlay({ptr_clip:#x}, {repeat})");

    if ptr_clip == 0 {
        return Ok(0);
    }

    let clip: MdaClip = read_generic(context, ptr_clip)?;
    let callback = clip.h_proc as WIPICWord;

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
                    context.system().sleep(1).await;
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
pub async fn clip_control(context: &mut dyn WIPICContext, clip: WIPICWord, cmd: WIPICWord, arg1: WIPICWord, arg2: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipControl({clip:#x}, {cmd:#x}, {arg1:#x}, {arg2:#x})");

    match cmd {
        // Play (0x30 = once, 0x31 = looping). Drive the same load_smaf-backed
        // playback path as MC_mdaPlay.
        0x30 | 0x31 => {
            let repeat = WIPICWord::from(cmd == 0x31);
            play(context, clip, repeat).await?;
            Ok(0)
        }
        _ => {
            tracing::warn!("MC_mdaClipControl: unhandled command {cmd:#x} on clip {clip:#x}");
            Ok(0)
        }
    }
}

pub async fn clip_alloc_player(_context: &mut dyn WIPICContext, clip: WIPICWord, param: WIPICWord) -> Result<WIPICWord> {
    // Returning a non-null handle here makes titles read the player as "already
    // set up" and skip the following MC_mdaPlay, leaving the logo jingle and
    // looping BGM silent. Keep the validated stub so MC_mdaPlay drives playback.
    tracing::warn!("stub MC_mdaClipAllocPlayer({clip:#x}, {param:#x})");

    Ok(0)
}

pub async fn clip_free_player(_context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::warn!("stub MC_mdaClipFreePlayer({clip:#x})");

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
