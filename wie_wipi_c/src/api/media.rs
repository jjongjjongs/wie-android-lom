use alloc::vec;

use bytemuck::{Pod, Zeroable};

use wipi_types::wipic::WIPICWord;

use wie_util::{Result, read_generic, write_generic};

use crate::context::WIPICContext;

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

    let clip = context.alloc_raw(size_of::<MdaClip>() as u32)?;

    Ok(clip)
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

pub async fn clip_get_volume(context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipGetVolume({clip:#x})");

    if clip == 0 {
        return Ok(0);
    }

    let clip_data: MdaClip = read_generic(context, clip)?;
    Ok(clip_data.original_volume.clamp(0, 100) as WIPICWord)
}

pub async fn clip_set_volume(context: &mut dyn WIPICContext, clip: WIPICWord, volume: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipSetVolume({clip:#x}, {volume:#x})");

    if clip == 0 {
        return Ok(0);
    }

    let mut clip_data: MdaClip = read_generic(context, clip)?;
    clip_data.original_volume = (volume as i32).clamp(0, 100);
    write_generic(context, clip, clip_data)?;

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

    let system = context.system();

    let result = system.audio().play(system, clip.handle, repeat != 0);

    if let Err(x) = result {
        tracing::error!("Failed to load audio: {x:?}");
    }

    Ok(0)
}

pub async fn clip_alloc_player(context: &mut dyn WIPICContext, clip: WIPICWord, param: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipAllocPlayer({clip:#x}, {param:#x})");

    if clip == 0 {
        return Ok(0);
    }

    // Callers treat the return as an opaque player handle and later call it
    // back with that value; a stubbed zero is read as "no player" and the game
    // stalls at load. The clip already owns the backend audio handle, so the
    // guest clip pointer itself is a stable non-null handle to hand back.
    let mut clip_data: MdaClip = read_generic(context, clip)?;
    clip_data.in_use = 1;
    if clip_data.original_volume == 0 {
        clip_data.original_volume = 80;
    }
    write_generic(context, clip, clip_data)?;

    // With a null second argument, the vendor `MC_mdaClipAllocPlayer` does not
    // just allocate - it starts the clip, routing through the very function
    // `MC_mdaPlay` ends on. Gamevil titles (ZENONIA 1-3, HYBRID 1/2, ADVENA)
    // rely on that: they set a clip up once and never resolve `MC_mdaPlay`, so
    // allocating the player is the only thing that would ever make a sound.
    // Allocating without starting left them silent. A non-zero argument selects
    // a different, named allocation mode, so only the default path plays.
    if param == 0 {
        let system = context.system();
        if let Err(x) = system.audio().play(system, clip_data.handle, false) {
            tracing::error!("Failed to start audio on player alloc: {x:?}");
        }
    }

    Ok(clip)
}

pub async fn clip_free_player(context: &mut dyn WIPICContext, clip: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_mdaClipFreePlayer({clip:#x})");

    if clip != 0 {
        let mut clip_data: MdaClip = read_generic(context, clip)?;
        clip_data.in_use = 0;
        write_generic(context, clip, clip_data)?;
    }

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

    let system = context.system();

    system.audio().stop(clip.handle);

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
