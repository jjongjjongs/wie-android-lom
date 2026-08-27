pub trait AudioSink: Sync + Send {
    fn set_master_volume(&self, _volume: u8) {}
    fn play_wave(&self, channel: u8, sampling_rate: u32, wave_data: &[i16]);

    /// Opens an isolated MIDI voice - its own synth - and returns its id. Every
    /// `midi_*` call tagged with that id renders into that voice, mixed with the
    /// others, so concurrently playing clips do not collide on shared channels.
    /// A sink without per-voice synths returns 0 (a single shared voice).
    fn open_midi_voice(&self) -> u32 {
        0
    }
    /// Marks a voice's clip as finished. The voice keeps sounding until its
    /// release tails decay, then the sink drops it.
    fn close_midi_voice(&self, _voice: u32) {}

    fn midi_note_on(&self, voice: u32, channel_id: u8, note: u8, velocity: u8);
    fn midi_note_off(&self, voice: u32, channel_id: u8, note: u8, velocity: u8);
    fn midi_program_change(&self, voice: u32, channel_id: u8, program: u8);
    fn midi_control_change(&self, voice: u32, channel_id: u8, control: u8, value: u8);
    fn midi_pitch_bend(&self, voice: u32, channel_id: u8, value: u16);
    fn midi_sysex(&self, voice: u32, data: &[u8]);

    /// Offers a whole SMAF/MMF file to the sink to render and play on its own,
    /// returning the clip length in milliseconds if it took ownership. A sink
    /// with a faithful offline renderer plays the file that way - identical to
    /// the reference - instead of the caller streaming MIDI events into
    /// `midi_*`. Returning `None` (the default) means the sink declined and the
    /// caller should fall back to the live event path.
    fn play_smaf(&self, _data: &[u8], _repeat: bool) -> Option<u32> {
        None
    }

    /// Stops a file started through [`Self::play_smaf`].
    fn stop_smaf(&self) {}
}
