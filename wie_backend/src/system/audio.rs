use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use smaf_player::{SmafEvent, parse_smaf};

use crate::{System, audio_sink::AudioSink};

pub type AudioHandle = u32;
#[derive(Debug)]
pub enum AudioError {
    InvalidHandle,
    InvalidAudio,
}

enum AudioFile {
    Smaf(Vec<u8>),
}

pub struct Audio {
    sink: Arc<Box<dyn AudioSink>>,
    files: BTreeMap<AudioHandle, AudioFile>,
    volumes: BTreeMap<AudioHandle, Arc<AtomicU8>>,
    playing: BTreeMap<AudioHandle, Arc<AtomicBool>>,
    last_audio_handle: AudioHandle,
    default_clip_handle: Option<AudioHandle>,
    /// The clip currently rendering through the sink's pre-rendered path, kept
    /// so a title that tears the player down and rebuilds it every frame with
    /// byte-identical looping data (시드 restarts its BGM in `paint`) does not
    /// restart the audio from zero each time - the identical re-play continues
    /// the existing playback instead. See [`Self::play_with_completion`].
    active: Option<ActiveSmaf>,
    /// Whether the deferred-stop reaper task has been spawned (once per Audio).
    reaper_started: bool,
}

/// A pre-rendered SMAF clip playing through the sink, tracked so an immediate
/// identical re-play is seamless and a real stop is honored after a short grace.
struct ActiveSmaf {
    /// Hash of the SMAF bytes, to recognize a byte-identical re-play.
    hash: u64,
    repeat: bool,
    /// The handle the title currently knows the clip by (remapped on each
    /// seamless re-play, since it allocates a fresh handle every time). Used to
    /// match the title's `stop`, which names this latest handle.
    handle: AudioHandle,
    /// The handle the sink actually plays the stream under - fixed at the first
    /// `play_smaf` and never remapped, because a seamless re-play keeps that
    /// original stream. Stopping the clip must target THIS handle, not the
    /// latest one the title allocated (which the sink never played).
    sink_handle: AudioHandle,
    stop_flag: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    /// Reaper polls remaining before a deferred stop actually stops the sink;
    /// `None` while playing. A re-play clears it, so continuous re-play never
    /// stops; a genuine stop with no re-play flushes after the grace.
    pending_stop_polls: Option<u32>,
}

/// Reaper poll interval and the grace (in polls) before a deferred stop takes
/// effect - long enough to bridge a per-frame tear-down/rebuild, short enough
/// that a real stop is barely audible as a tail.
const REAPER_POLL_MS: u64 = 100;
const PENDING_STOP_GRACE_POLLS: u32 = 3;

fn smaf_hash(data: &[u8]) -> u64 {
    // FNV-1a, enough to tell one clip's bytes from another.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl Audio {
    pub fn new(sink: Box<dyn AudioSink>) -> Self {
        Self {
            sink: Arc::new(sink),
            files: BTreeMap::new(),
            volumes: BTreeMap::new(),
            playing: BTreeMap::new(),
            last_audio_handle: 0,
            default_clip_handle: None,
            active: None,
            reaper_started: false,
        }
    }

    /// The handle bound to the implicit "clip 0" default player.
    ///
    /// Some titles never allocate a clip object and drive the whole MC_mda*
    /// sequence with `clip == 0` - a single reusable player (나는마왕이다2, for
    /// one, loads and plays every effect and BGM this way). The clip's audio
    /// still has to live under a real handle for the sink to play it, so the one
    /// most recently loaded under clip 0 is remembered here and read back by the
    /// clip-0 play/volume/stop paths.
    pub fn set_default_clip(&mut self, handle: AudioHandle) {
        self.default_clip_handle = Some(handle);
    }

    pub fn default_clip(&self) -> Option<AudioHandle> {
        self.default_clip_handle
    }

    pub fn load_smaf(&mut self, data: &[u8]) -> Result<AudioHandle, AudioError> {
        let audio_handle = self.last_audio_handle;

        self.last_audio_handle += 1;
        self.files.insert(audio_handle, AudioFile::Smaf(data.to_vec()));
        self.volumes.insert(audio_handle, Arc::new(AtomicU8::new(100)));

        Ok(audio_handle)
    }

    pub fn play(&mut self, system: &System, audio_handle: AudioHandle, repeat: bool) -> Result<(), AudioError> {
        self.play_with_completion(system, audio_handle, repeat)?;

        Ok(())
    }

    pub fn play_with_completion(
        &mut self,
        system: &System,
        audio_handle: AudioHandle,
        repeat: bool,
    ) -> Result<(Arc<AtomicBool>, Arc<AtomicBool>), AudioError> {
        let data = match self.files.get(&audio_handle) {
            Some(AudioFile::Smaf(data)) => data.clone(),
            None => return Err(AudioError::InvalidHandle),
        };
        let volume = self.volumes.get(&audio_handle).ok_or(AudioError::InvalidHandle)?.clone();
        let hash = smaf_hash(&data);

        // Seamless re-play: a looping clip torn down and rebuilt with identical
        // bytes (시드 restarts its BGM every paint) keeps the existing sink
        // playback instead of restarting from zero. Adopt the fresh handle and
        // cancel any deferred stop.
        if repeat
            && let Some(active) = self.active.as_mut()
            && active.repeat
            && active.hash == hash
        {
            active.pending_stop_polls = None;
            active.handle = audio_handle;
            let (completed, stop_flag) = (active.completed.clone(), active.stop_flag.clone());
            self.sink.set_master_volume(volume.load(Ordering::Relaxed));
            self.playing.insert(audio_handle, stop_flag.clone());
            return Ok((completed, stop_flag));
        }

        // A different looping clip replaces the active one: stop it for real
        // first. A one-shot (SFX) never becomes active and never disturbs a
        // looping BGM playing alongside it.
        if repeat {
            self.flush_active();
        }

        self.stop(audio_handle);
        self.sink.set_master_volume(volume.load(Ordering::Relaxed));

        let stop_flag = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        self.playing.insert(audio_handle, stop_flag.clone());

        // Offer the file to the sink's own renderer first. When it takes it, the
        // sink plays the pre-rendered stream and this task only watches for the
        // stop flag (looping) or the clip's length (one-shot).
        if let Some(duration_ms) = self.sink.play_smaf(audio_handle, &data, repeat) {
            if repeat {
                // Track the looping clip so an identical re-play coalesces and a
                // real stop is honored by the reaper after a short grace.
                self.active = Some(ActiveSmaf {
                    hash,
                    repeat,
                    handle: audio_handle,
                    sink_handle: audio_handle,
                    stop_flag: stop_flag.clone(),
                    completed: completed.clone(),
                    pending_stop_polls: None,
                });
                self.ensure_reaper(system);

                let system_clone = system.clone();
                let stop_flag_clone = stop_flag.clone();
                system.spawn(async move || {
                    while !stop_flag_clone.load(Ordering::Relaxed) {
                        system_clone.sleep(50).await;
                    }
                    Ok(())
                });
            } else {
                let system_clone = system.clone();
                let sink_clone = self.sink.clone();
                let stop_flag_clone = stop_flag.clone();
                let completed_clone = completed.clone();
                system.spawn(async move || {
                    let mut elapsed = 0u64;
                    loop {
                        if stop_flag_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        if elapsed >= u64::from(duration_ms) {
                            completed_clone.store(true, Ordering::Release);
                            break;
                        }
                        system_clone.sleep(50).await;
                        elapsed += 50;
                    }
                    sink_clone.stop_smaf(audio_handle);
                    Ok(())
                });
            }
            return Ok((completed, stop_flag));
        }

        // Otherwise stream the sequence live as MIDI events.
        let player = SmafPlayer::new(&data);
        let mut system_clone = system.clone();
        let sink_clone = self.sink.clone();
        let stop_flag_clone = stop_flag.clone();
        let completed_clone = completed.clone();

        // TODO use dedicated audio player task
        system.spawn(async move || {
            player.play(&mut system_clone, &**sink_clone, &stop_flag_clone, repeat).await;

            if !stop_flag_clone.load(Ordering::Relaxed) {
                completed_clone.store(true, Ordering::Release);
            }

            Ok(())
        });

        Ok((completed, stop_flag))
    }

    pub fn is_playing(&self, audio_handle: AudioHandle) -> bool {
        self.playing.contains_key(&audio_handle)
    }

    pub fn set_volume(&mut self, audio_handle: AudioHandle, volume: u8) -> Result<(), AudioError> {
        let state = self.volumes.get(&audio_handle).ok_or(AudioError::InvalidHandle)?;
        state.store(volume.min(100), Ordering::Relaxed);

        if self.playing.contains_key(&audio_handle) {
            self.sink.set_master_volume(volume.min(100));
        }

        Ok(())
    }

    pub fn get_volume(&self, audio_handle: AudioHandle) -> Result<u8, AudioError> {
        self.volumes
            .get(&audio_handle)
            .map(|state| state.load(Ordering::Relaxed))
            .ok_or(AudioError::InvalidHandle)
    }

    pub fn stop(&mut self, audio_handle: AudioHandle) {
        // Defer stopping the active looping clip: the title tears it down and
        // rebuilds it every frame, so an immediate stop would restart the audio
        // from zero. The reaper stops it for real once no identical re-play has
        // arrived within the grace window.
        if let Some(active) = self.active.as_mut()
            && active.handle == audio_handle
        {
            if active.pending_stop_polls.is_none() {
                active.pending_stop_polls = Some(0);
            }
            self.playing.remove(&audio_handle);
            return;
        }

        if let Some(stop_flag) = self.playing.remove(&audio_handle) {
            stop_flag.store(true, Ordering::Relaxed);
        }
    }

    /// Stops the active looping clip's sink playback immediately and forgets it.
    fn flush_active(&mut self) {
        if let Some(active) = self.active.take() {
            // Stop the stream the sink actually plays, not the latest handle the
            // title allocated - they differ once a looping clip has re-played.
            self.sink.stop_smaf(active.sink_handle);
            active.stop_flag.store(true, Ordering::Relaxed);
            self.playing.remove(&active.handle);
        }
    }

    /// Spawns the one deferred-stop reaper for this `Audio`. It polls the active
    /// looping clip and, once a deferred stop has stood for the grace window
    /// with no identical re-play cancelling it, stops the sink for real.
    fn ensure_reaper(&mut self, system: &System) {
        if self.reaper_started {
            return;
        }
        self.reaper_started = true;

        let system_clone = system.clone();
        system.spawn(async move || {
            loop {
                system_clone.sleep(REAPER_POLL_MS).await;

                let mut audio = system_clone.audio();
                let flush = match audio.active.as_mut() {
                    Some(active) => match active.pending_stop_polls {
                        Some(polls) if polls + 1 >= PENDING_STOP_GRACE_POLLS => true,
                        Some(polls) => {
                            active.pending_stop_polls = Some(polls + 1);
                            false
                        }
                        None => false,
                    },
                    None => false,
                };
                if flush {
                    audio.flush_active();
                }
            }
        });
    }

    pub fn close(&mut self, audio_handle: AudioHandle) -> Result<(), AudioError> {
        self.stop(audio_handle);

        if self.files.remove(&audio_handle).is_none() {
            return Err(AudioError::InvalidHandle);
        }
        self.volumes.remove(&audio_handle);

        Ok(())
    }
}

pub struct SmafPlayer {
    events: Vec<(usize, SmafEvent)>,
}

impl SmafPlayer {
    pub fn new(data: &[u8]) -> Self {
        Self { events: parse_smaf(data) }
    }

    pub async fn play(&self, system: &mut System, sink: &dyn AudioSink, stop_flag: &AtomicBool, repeat: bool) {
        // An isolated voice for this clip, so its sequence does not collide with
        // other clips playing at the same time (a looping track under short
        // effects) on shared MIDI channels.
        let voice = sink.open_midi_voice();
        tracing::info!("[audio] SMAF clip on isolated voice {voice}");

        loop {
            let mut active_notes: Vec<(u8, u8)> = Vec::new();
            let mut used_channels: BTreeSet<u8> = BTreeSet::new();

            let start_time = system.platform().now();
            for (time, event) in &self.events {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                let now = system.platform().now();
                if (*time as u64) > now - start_time {
                    system.sleep(((*time as u64) - (now - start_time)) as _).await;

                    if stop_flag.load(Ordering::Relaxed) {
                        break;
                    }
                }

                match event {
                    SmafEvent::Wave {
                        channel,
                        sampling_rate,
                        data,
                    } => {
                        sink.play_wave(*channel, *sampling_rate, data);
                    }
                    SmafEvent::MidiNoteOn { channel, note, velocity } => {
                        sink.midi_note_on(voice, *channel, *note, *velocity);
                        active_notes.push((*channel, *note));
                        used_channels.insert(*channel);
                    }
                    SmafEvent::MidiNoteOff { channel, note, velocity } => {
                        sink.midi_note_off(voice, *channel, *note, *velocity);
                        active_notes.retain(|(c, n)| !(*c == *channel && *n == *note));
                    }
                    SmafEvent::MidiProgramChange { channel, program } => {
                        sink.midi_program_change(voice, *channel, *program);
                        used_channels.insert(*channel);
                    }
                    SmafEvent::MidiControlChange { channel, control, value } => {
                        sink.midi_control_change(voice, *channel, *control, *value);
                        used_channels.insert(*channel);
                    }
                    SmafEvent::MidiPitchBend { channel, value } => {
                        sink.midi_pitch_bend(voice, *channel, *value);
                        used_channels.insert(*channel);
                    }
                    SmafEvent::MidiSysEx(data) => {
                        sink.midi_sysex(voice, data);
                    }
                    SmafEvent::End => {}
                }
            }

            for (channel, note) in &active_notes {
                sink.midi_note_off(voice, *channel, *note, 0);
            }

            for channel in &used_channels {
                sink.midi_control_change(voice, *channel, 64, 0);
                sink.midi_control_change(voice, *channel, 120, 0);
                sink.midi_control_change(voice, *channel, 123, 0);
            }

            if !repeat || stop_flag.load(Ordering::Relaxed) {
                break;
            }
        }

        // The clip is done feeding events; let its voice ring out and be
        // reclaimed once silent.
        sink.close_midi_voice(voice);
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, sync::Arc, vec};
    use alloc::{string::String, vec::Vec};
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use smaf_player::SmafEvent;

    use super::SmafPlayer;
    use crate::{AudioSink, Database, DatabaseRepository, DefaultTaskRunner, Filesystem, Instant, Platform, Screen, System, canvas::Image};

    struct NullDatabase;

    #[async_trait::async_trait]
    impl Database for NullDatabase {
        async fn next_id(&self) -> u32 {
            1
        }

        async fn add(&mut self, _data: &[u8]) -> u32 {
            1
        }

        async fn get(&self, _id: u32) -> Option<alloc::vec::Vec<u8>> {
            None
        }

        async fn set(&mut self, _id: u32, _data: &[u8]) -> bool {
            true
        }

        async fn delete(&mut self, _id: u32) -> bool {
            true
        }

        async fn get_record_ids(&self) -> alloc::vec::Vec<u32> {
            vec![]
        }
    }

    struct NullDatabaseRepository;

    #[async_trait::async_trait]
    impl DatabaseRepository for NullDatabaseRepository {
        async fn open(&self, _name: &str, _app_id: &str) -> Box<dyn Database> {
            Box::new(NullDatabase)
        }

        async fn exists(&self, _name: &str, _app_id: &str) -> bool {
            false
        }

        async fn delete(&self, _name: &str, _app_id: &str) -> bool {
            false
        }

        async fn list(&self, _app_id: &str) -> Vec<String> {
            vec![]
        }
    }

    struct NullFilesystem;

    #[async_trait::async_trait]
    impl Filesystem for NullFilesystem {
        async fn exists(&self, _aid: &str, _path: &str) -> bool {
            false
        }

        async fn size(&self, _aid: &str, _path: &str) -> Option<usize> {
            None
        }

        async fn read(&self, _aid: &str, _path: &str, _offset: usize, _count: usize, _buf: &mut [u8]) -> Option<usize> {
            None
        }

        async fn write(&self, _aid: &str, _path: &str, _offset: usize, data: &[u8]) -> usize {
            data.len()
        }

        async fn truncate(&self, _aid: &str, _path: &str, _len: usize) {}

        async fn remove(&self, _aid: &str, _path: &str) -> bool {
            false
        }

        async fn mkdir(&self, _aid: &str, _path: &str) -> core::result::Result<(), crate::platform::FilesystemMkdirError> {
            Err(crate::platform::FilesystemMkdirError::NotFound)
        }

        async fn rmdir(&self, _aid: &str, _path: &str) -> core::result::Result<(), crate::platform::FilesystemRmDirError> {
            Err(crate::platform::FilesystemRmDirError::NotFound)
        }

        async fn rename(&self, _aid: &str, _from: &str, _to: &str) -> core::result::Result<(), crate::platform::FilesystemRenameError> {
            Err(crate::platform::FilesystemRenameError::NotFound)
        }

        async fn set_mode(&self, _aid: &str, _path: &str, _mode: u32) -> core::result::Result<(), crate::platform::FilesystemSetModeError> {
            Err(crate::platform::FilesystemSetModeError::NotFound)
        }

        async fn total_space(&self, _aid: &str) -> Option<u64> {
            None
        }

        async fn available_space(&self, _aid: &str) -> Option<u64> {
            None
        }

        async fn list(&self, _aid: &str, _path: &str) -> Option<Vec<String>> {
            None
        }
    }

    struct NullScreen;

    impl Screen for NullScreen {
        fn request_redraw(&self) -> wie_util::Result<()> {
            Ok(())
        }

        fn paint(&self, _image: &dyn Image) {}

        fn width(&self) -> u32 {
            240
        }

        fn height(&self) -> u32 {
            320
        }
    }

    struct NullPlatform {
        screen: NullScreen,
        database_repository: NullDatabaseRepository,
        filesystem: NullFilesystem,
        now: AtomicUsize,
        smaf_play: Arc<AtomicUsize>,
        smaf_stop: Arc<AtomicUsize>,
        smaf_last_played: Arc<AtomicUsize>,
        smaf_last_stopped: Arc<AtomicUsize>,
    }

    impl NullPlatform {
        fn new() -> Self {
            Self {
                screen: NullScreen,
                database_repository: NullDatabaseRepository,
                filesystem: NullFilesystem,
                now: AtomicUsize::new(0),
                smaf_play: Arc::new(AtomicUsize::new(0)),
                smaf_stop: Arc::new(AtomicUsize::new(0)),
                smaf_last_played: Arc::new(AtomicUsize::new(usize::MAX)),
                smaf_last_stopped: Arc::new(AtomicUsize::new(usize::MAX)),
            }
        }
    }

    impl Platform for NullPlatform {
        fn screen(&self) -> &dyn Screen {
            &self.screen
        }

        fn now(&self) -> Instant {
            Instant::from_epoch_millis(self.now.fetch_add(8, Ordering::SeqCst) as u64)
        }

        fn database_repository(&self) -> &dyn DatabaseRepository {
            &self.database_repository
        }

        fn filesystem(&self) -> &dyn Filesystem {
            &self.filesystem
        }

        fn audio_sink(&self) -> Box<dyn AudioSink> {
            Box::new(SmafCountingSink {
                play: self.smaf_play.clone(),
                stop: self.smaf_stop.clone(),
                last_played: self.smaf_last_played.clone(),
                last_stopped: self.smaf_last_stopped.clone(),
            })
        }

        fn write_stdout(&self, _buf: &[u8]) {}

        fn write_stderr(&self, _buf: &[u8]) {}

        fn exit(&self) {}

        fn vibrate(&self, _duration_ms: u64, _intensity: u8) {}

        fn set_backlight_mode(&self, _mode: u8) {}
    }

    struct NoopAudioSink;

    impl AudioSink for NoopAudioSink {
        fn play_wave(&self, _channel: u8, _sampling_rate: u32, _wave_data: &[i16]) {}

        fn midi_note_on(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}

        fn midi_note_off(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}

        fn midi_program_change(&self, _voice: u32, _channel_id: u8, _program: u8) {}

        fn midi_control_change(&self, _voice: u32, _channel_id: u8, _control: u8, _value: u8) {}

        fn midi_pitch_bend(&self, _voice: u32, _channel_id: u8, _value: u16) {}

        fn midi_sysex(&self, _voice: u32, _data: &[u8]) {}
    }

    /// A sink with a pre-rendered SMAF path that counts how many times a clip is
    /// started and stopped, to observe the coalescing of identical re-plays.
    struct SmafCountingSink {
        play: Arc<AtomicUsize>,
        stop: Arc<AtomicUsize>,
        /// The handle passed to the most recent `play_smaf` / `stop_smaf`, so a
        /// test can assert a stop targets the stream the sink actually played.
        last_played: Arc<AtomicUsize>,
        last_stopped: Arc<AtomicUsize>,
    }

    impl AudioSink for SmafCountingSink {
        fn play_wave(&self, _channel: u8, _sampling_rate: u32, _wave_data: &[i16]) {}
        fn midi_note_on(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}
        fn midi_note_off(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}
        fn midi_program_change(&self, _voice: u32, _channel_id: u8, _program: u8) {}
        fn midi_control_change(&self, _voice: u32, _channel_id: u8, _control: u8, _value: u8) {}
        fn midi_pitch_bend(&self, _voice: u32, _channel_id: u8, _value: u16) {}
        fn midi_sysex(&self, _voice: u32, _data: &[u8]) {}

        fn play_smaf(&self, id: u32, _data: &[u8], _repeat: bool) -> Option<u32> {
            self.play.fetch_add(1, Ordering::SeqCst);
            self.last_played.store(id as usize, Ordering::SeqCst);
            Some(16_000)
        }

        fn stop_smaf(&self, id: u32) {
            self.stop.fetch_add(1, Ordering::SeqCst);
            self.last_stopped.store(id as usize, Ordering::SeqCst);
        }
    }

    struct CountingSink {
        program_change_count: Arc<AtomicUsize>,
        stop_after: usize,
        stop_flag: Arc<AtomicBool>,
    }

    impl AudioSink for CountingSink {
        fn play_wave(&self, _channel: u8, _sampling_rate: u32, _wave_data: &[i16]) {}

        fn midi_note_on(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}

        fn midi_note_off(&self, _voice: u32, _channel_id: u8, _note: u8, _velocity: u8) {}

        fn midi_program_change(&self, _voice: u32, _channel_id: u8, _program: u8) {
            let count = self.program_change_count.fetch_add(1, Ordering::SeqCst) + 1;
            if count >= self.stop_after {
                self.stop_flag.store(true, Ordering::SeqCst);
            }
        }

        fn midi_control_change(&self, _voice: u32, _channel_id: u8, _control: u8, _value: u8) {}

        fn midi_pitch_bend(&self, _voice: u32, _channel_id: u8, _value: u16) {}

        fn midi_sysex(&self, _voice: u32, _data: &[u8]) {}
    }

    fn new_system() -> System {
        System::new(Box::new(NullPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner)
    }

    /// A system whose sink pre-renders SMAF, plus the (play, stop) counters that
    /// sink increments.
    struct SmafCounters {
        play: Arc<AtomicUsize>,
        stop: Arc<AtomicUsize>,
        last_played: Arc<AtomicUsize>,
        last_stopped: Arc<AtomicUsize>,
    }

    fn new_system_with_smaf_counters() -> (System, SmafCounters) {
        let platform = NullPlatform::new();
        let counters = SmafCounters {
            play: platform.smaf_play.clone(),
            stop: platform.smaf_stop.clone(),
            last_played: platform.smaf_last_played.clone(),
            last_stopped: platform.smaf_last_stopped.clone(),
        };
        let system = System::new(Box::new(platform), "test-pid", "test-aid", DefaultTaskRunner);
        (system, counters)
    }

    #[test]
    fn identical_looping_replay_does_not_restart_the_sink() {
        let (system, counters) = new_system_with_smaf_counters();

        // First BGM start: the sink begins playing the looping clip under h1.
        let h1 = system.audio().load_smaf(b"BGM-DATA").unwrap();
        system.audio().play_with_completion(&system, h1, true).unwrap();
        assert_eq!(counters.play.load(Ordering::SeqCst), 1);
        assert_eq!(counters.last_played.load(Ordering::SeqCst), h1 as usize);

        // The title tears the player down and rebuilds it with byte-identical
        // data (stop, close, load, play) - exactly 시드's per-frame pattern. The
        // sink must NOT be restarted or stopped: the playback continues, and the
        // stream stays under h1 even as the title allocates fresh handles.
        let mut latest = h1;
        for _ in 0..5 {
            system.audio().stop(latest);
            let _ = system.audio().close(latest);
            latest = system.audio().load_smaf(b"BGM-DATA").unwrap();
            system.audio().play_with_completion(&system, latest, true).unwrap();
        }
        assert_eq!(counters.play.load(Ordering::SeqCst), 1, "identical re-plays should coalesce");
        assert_eq!(counters.stop.load(Ordering::SeqCst), 0, "no stop while re-playing identical data");
        assert_ne!(latest, h1, "the title allocated fresh handles");

        // A genuinely different looping clip must stop the ORIGINAL stream (h1,
        // what the sink actually plays) - not the latest handle the title used,
        // which the sink never played - then start the new one.
        let other = system.audio().load_smaf(b"OTHER-BGM").unwrap();
        system.audio().play_with_completion(&system, other, true).unwrap();
        assert_eq!(counters.play.load(Ordering::SeqCst), 2);
        assert_eq!(counters.stop.load(Ordering::SeqCst), 1);
        assert_eq!(
            counters.last_stopped.load(Ordering::SeqCst),
            h1 as usize,
            "the stop must target the stream the sink played (h1), not a later handle"
        );
    }

    #[futures_test::test]
    async fn plays_once_when_repeat_is_false() {
        let counter = Arc::new(AtomicUsize::new(0));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let sink = CountingSink {
            program_change_count: counter.clone(),
            stop_after: usize::MAX,
            stop_flag: stop_flag.clone(),
        };
        let player = SmafPlayer {
            events: vec![(0, SmafEvent::MidiProgramChange { channel: 0, program: 1 })],
        };
        let mut system = new_system();

        player.play(&mut system, &sink, &stop_flag, false).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[futures_test::test]
    async fn repeats_until_stop_flag_is_set() {
        let counter = Arc::new(AtomicUsize::new(0));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let sink = CountingSink {
            program_change_count: counter.clone(),
            stop_after: 2,
            stop_flag: stop_flag.clone(),
        };
        let player = SmafPlayer {
            events: vec![(0, SmafEvent::MidiProgramChange { channel: 0, program: 1 })],
        };
        let mut system = new_system();

        player.play(&mut system, &sink, &stop_flag, true).await;

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn default_clip_binds_the_last_clip_zero_load() {
        let mut audio = super::Audio::new(Box::new(NoopAudioSink));
        // Nothing bound until a clip-0 load happens.
        assert_eq!(audio.default_clip(), None);

        // A clip-0 title loads its data under a real handle and binds it as the
        // default player; the most recent load wins.
        let first = audio.load_smaf(b"first").unwrap();
        audio.set_default_clip(first);
        assert_eq!(audio.default_clip(), Some(first));

        let second = audio.load_smaf(b"second").unwrap();
        audio.set_default_clip(second);
        assert_eq!(audio.default_clip(), Some(second));
    }
}
