//! [`Audio`], the sound service at `ctx.audio`: sound effects and music,
//! played with `kira`.
//!
//! **Maturity: tested**, from the guide chapter `audio.md`. Game: Coin Dash, Ember Hop and Shard
//! Grove play sounds and music. Tests: `tests/audio.rs`, `tests/audio_mixer.rs`. Platforms: Linux,
//! macOS and Windows, headless (the workspace tests in the list of checks). Graphics: not involved.
//! Missing: sound out of a real device: the runs in the list of checks hear nothing.
//!
//! **What lives here:** loading and creating sounds ([`Audio::load`],
//! [`Audio::create`], [`Audio::load_stream`]), playing them
//! ([`Audio::play`], [`Audio::play_ex`], [`Audio::play_on`], and
//! `Audio::play_at` for a point in the 3D world, feature `3d`), the handles of what is
//! playing ([`Audio::stop`], [`Audio::set_effects`]), the mixer (buses
//! with [`Audio::bus`], [`Audio::set_bus_volume`], [`Audio::mute_bus`];
//! the bus tree, [`BusEffect`] chains and [`Ducking`];
//! [`MixerSnapshot`]s; [`Audio::load_mixer`] for a mixer file and
//! [`MixerState`] for the settings) and the 3D listener
//! (`Audio::set_listener`, feature `3d`).
//!
//! **When to call it:** load in setup and keep the [`Sound`] handles; play,
//! stop and mix from any system. It only borrows `ctx.audio`, so a sound can
//! be played while iterating `ctx.ui.events()` or a `ctx.world` query.
//!
//! **What it needs:** the `audio` feature and an output device to be heard.
//! The device opens the first time a sound is loaded or played. If there is
//! none (servers, CI), the feature is off, or the run is headless,
//! everything here still works and is simply silent, so game code never
//! needs a special case. Positional sound needs the `3d` feature.

#![deny(missing_docs)]

#[cfg(feature = "3d")]
pub(crate) mod spatial;
#[cfg(feature = "3d")]
pub use spatial::AudioRolloff;
#[cfg(feature = "3d")]
pub(crate) use spatial::*;

/// A loaded sound. Cheap to copy. Get one from
/// [`Ctx::load_sound`](crate::ctx::Ctx::load_sound) or
/// [`Audio::create`](crate::audio::Audio::create).
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Sound {
    pub(crate) id: u32,
    /// Length in seconds.
    seconds: f32,
}

// A sound's length is never NaN, so equality is an equivalence.
impl Eq for Sound {}

impl std::hash::Hash for Sound {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Equal sounds have equal ids; the length comes with the id.
        self.id.hash(state);
    }
}

impl Sound {
    pub(crate) const fn new(id: u32, seconds: f32) -> Self {
        Self { id, seconds }
    }

    /// Length in seconds.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(sound: Sound) {
    /// println!("{:.1} s", sound.seconds());
    /// # }
    /// ```
    #[must_use]
    pub fn seconds(&self) -> f32 {
        self.seconds
    }
}

/// A sound that was started, to stop it later with
/// [`Audio::stop`](crate::audio::Audio::stop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Playing(pub(crate) u64);

/// Optional settings for [`Audio::play_ex`](crate::audio::Audio::play_ex).
///
/// ```
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx, music: Sound) {
/// ctx.audio.play_ex(music, SoundParams { volume: 0.5, looping: true, ..Default::default() });
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundParams {
    /// 0 is silent, 1 (default) is as recorded.
    pub volume: f32,
    /// Playback speed, which also changes the pitch: 2 is twice as fast
    /// and an octave higher. Default 1.
    pub speed: f32,
    /// Start over at the end, for music and ambience. Default `false`.
    pub looping: bool,
}

impl Default for SoundParams {
    fn default() -> Self {
        Self {
            volume: 1.0,
            speed: 1.0,
            looping: false,
        }
    }
}

/// A sound that was started: what [`Audio::started`] lists, so a test (or
/// a tool) can see what a game played, headless runs included.
///
/// ```
/// use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let beep = ctx.audio.create(8000, &[0.0; 800]);
///     let playing = ctx.audio.play_ex(beep, SoundParams { volume: 0.5, ..Default::default() });
///     let started = ctx.audio.started()[0];
///     assert_eq!((started.playing, started.sound, started.volume), (playing, beep, 0.5));
///     assert_eq!(started.bus, ctx.audio.bus("sfx"));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SoundStarted {
    /// Its handle, as the play call returned it (headless runs hand out
    /// handles too, a different one for each sound).
    pub playing: Playing,
    /// The sound.
    pub sound: Sound,
    /// The bus it plays on: `sfx` for [`Audio::play`] and [`Audio::play_ex`].
    pub bus: AudioBus,
    /// Its volume, 0 to 1 as asked for; for a sound played at a point in the
    /// 3D world, with the distance's fall-off applied at the start.
    pub volume: f32,
    /// Its speed (and pitch), 1 as recorded.
    pub speed: f32,
    /// Whether it starts over at its end.
    pub looping: bool,
}

mod bus_effects;
mod device;
mod mixer;
mod mixer_file;
pub(crate) mod music;
mod pause;
mod session;
mod snapshots;
mod voices;
#[cfg(feature = "audio")]
use backend::MAX_VOICE_LIMIT;
pub use bus_effects::{
    BusEffect, CompressorEffect, DelayEffect, Ducking, FilterEffect, FilterMode, PitchShiftEffect, ReverbEffect,
};
pub use device::{AudioDevice, AudioDeviceEvent};
use mixer::BusRegistry;
pub use mixer::{AudioBus, AudioEffects};
pub use music::{Quantize, Song};
pub use session::AudioSessionCategory;
pub use snapshots::{
    BusSnapshot, EffectOverride, MixerSnapshot, MixerState, SavedBus, SavedSnapshot, SnapshotId, SnapshotRef,
};
pub use voices::{DEFAULT_PRIORITY, VoiceCount};
#[cfg(all(test, feature = "audio"))]
mod completeness_render_tests;
#[cfg(feature = "audio")]
mod dsp;
#[cfg(all(test, feature = "audio"))]
mod mixer_render_tests;
#[cfg(feature = "audio")]
mod tracks;
// Streams decoded a packet at a time, for the web (tested natively), and
// for a video's soundtrack (feature `video`).
#[cfg(all(feature = "audio", any(target_arch = "wasm32", test, feature = "video", fuzzing)))]
mod chunked;
// Streams on the desktop: the decoder Kira's decoder thread runs, guarded.
#[cfg(feature = "audio")]
pub(crate) mod sound_file;
#[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
mod stream_decoder;
#[cfg(feature = "audio")]
mod unlock;
#[cfg(all(feature = "audio", any(target_arch = "wasm32", test)))]
mod web_output;

#[cfg(feature = "audio")]
mod backend {
    // Voice limits and virtual voices, the music clock, and the output
    // device (choosing it, and opening it again when it goes away).
    mod limits;
    mod music_out;
    mod output;
    // A video's soundtrack (feature `video`).
    #[cfg(feature = "video")]
    mod soundtrack;

    use super::mixer::{finite, volume_db};
    use super::tracks::{BusTracks, Manager};
    use super::unlock::{Gate, Waiting};
    use super::{AudioBus, AudioEffects, BusRegistry, SoundParams};
    use kira::effect::{
        filter::{FilterBuilder, FilterHandle},
        reverb::ReverbBuilder,
    };
    use kira::sound::{
        PlaybackState,
        static_sound::{StaticSoundData, StaticSoundHandle},
    };
    // Kira streams on a decoder thread, which the web doesn't have: there
    // a stream is decoded a packet at a time as it plays (`chunked`), as is
    // a video's soundtrack everywhere.
    #[cfg(any(target_arch = "wasm32", feature = "video"))]
    use super::chunked::{ChunkedData, ChunkedHandle};
    #[cfg(not(target_arch = "wasm32"))]
    use super::stream_decoder::StreamDecoder;
    #[cfg(not(target_arch = "wasm32"))]
    use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
    use kira::track::{SendTrackBuilder, SendTrackHandle, TrackBuilder, TrackHandle};
    use kira::{Frame, StartTime, Tween};
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    pub(crate) use limits::{Fade, MAX_VOICE_LIMIT};
    #[cfg(test)]
    pub(in crate::audio) use output::fake;
    #[cfg(test)]
    pub(in crate::audio) use output::{DeviceIssue, Outputs};

    enum Clip {
        Static(Box<StaticSoundData>),
        Stream(StreamFrom),
        /// A video's soundtrack: encoded bytes decoded a packet at a time
        /// as it plays, on the mixing thread. Kira's own streams need the
        /// track's length in samples, which a WebM file doesn't give.
        #[cfg(feature = "video")]
        Chunked(Arc<[u8]>),
        /// Freed: plays nothing, and its id is never reused.
        Freed,
    }
    /// What a sound is besides its samples: its length and loop points.
    #[derive(Clone, Copy, Default)]
    struct Meta {
        seconds: f64,
        /// Where a looping playback goes back to, and where it turns
        /// around (`None`: the end).
        loop_points: (f64, Option<f64>),
    }
    enum Handle {
        Static(StaticSoundHandle),
        #[cfg(not(target_arch = "wasm32"))]
        Stream(StreamingSoundHandle<String>),
        #[cfg(any(target_arch = "wasm32", feature = "video"))]
        Chunked(ChunkedHandle),
    }
    /// A voice that is heard: its Kira sound and the track it plays on.
    struct Real {
        handle: Handle,
        track: TrackHandle,
        filter: FilterHandle,
    }
    struct Voice {
        /// The sound it plays.
        sound: u32,
        /// `None` while virtual: not heard, its place kept in `position`.
        real: Option<Real>,
        bus: AudioBus,
        effects: AudioEffects,
        params: SoundParams,
        /// The last positional volume, pan and speed.
        #[cfg(feature = "3d")]
        spatial: Option<[f32; 3]>,
        /// The reverb send's gain as last set.
        send_gain: f32,
        /// `set_volume` and `set_speed` (1: as played), moved each frame.
        gain: Fade,
        rate: Fade,
        priority: u8,
        /// Music: never virtual and not counted against the limits.
        exempt: bool,
        /// Seconds into the sound, as followed on the game side.
        position: f64,
        /// Stopping with a fade: it ends when Kira's fade does.
        stopping: bool,
    }
    impl Voice {
        fn new(sound: u32, params: SoundParams, bus: AudioBus, effects: AudioEffects) -> Self {
            Self {
                sound,
                real: None,
                bus,
                effects,
                params,
                #[cfg(feature = "3d")]
                spatial: None,
                send_gain: 1.0,
                gain: Fade::new(1.0),
                rate: Fade::new(1.0),
                priority: super::DEFAULT_PRIORITY,
                exempt: false,
                position: 0.0,
                stopping: false,
            }
        }
        /// The volume, pan and speed to play at now.
        fn heard(&self) -> (f32, f32, f32) {
            #[cfg(feature = "3d")]
            if let Some([volume, pan, speed]) = self.spatial {
                return (volume * self.gain.value, pan, speed * self.rate.value);
            }
            (
                self.params.volume * self.gain.value,
                0.0,
                finite(self.params.speed, 1.0) * self.rate.value,
            )
        }
    }
    impl Handle {
        /// Seconds into the sound; `None` for a sound decoded a packet at
        /// a time (on the web, and a video's soundtrack), which can't tell.
        fn position(&self) -> Option<f64> {
            match self {
                Self::Static(h) => Some(h.position()),
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => Some(h.position()),
                #[cfg(any(target_arch = "wasm32", feature = "video"))]
                Self::Chunked(_) => None,
            }
        }
        fn state(&self) -> PlaybackState {
            match self {
                Self::Static(h) => h.state(),
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => h.state(),
                #[cfg(any(target_arch = "wasm32", feature = "video"))]
                Self::Chunked(h) => h.state(),
            }
        }
        fn stop(&mut self, tween: Tween) {
            match self {
                Self::Static(h) => h.stop(tween),
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => h.stop(tween),
                #[cfg(any(target_arch = "wasm32", feature = "video"))]
                Self::Chunked(h) => h.stop_with(tween),
            }
        }
        fn set_volume(&mut self, volume: f32, tween: Tween) {
            match self {
                Self::Static(h) => h.set_volume(volume_db(volume), tween),
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => h.set_volume(volume_db(volume), tween),
                #[cfg(any(target_arch = "wasm32", feature = "video"))]
                Self::Chunked(h) => h.set_volume_with(volume, tween),
            }
        }
        fn set_speed(&mut self, speed: f32, tween: Tween) {
            let rate = f64::from(finite(speed, 1.0).max(0.01));
            match self {
                Self::Static(h) => h.set_playback_rate(rate, tween),
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => h.set_playback_rate(rate, tween),
                #[cfg(any(target_arch = "wasm32", feature = "video"))]
                Self::Chunked(h) => h.set_playback_rate(rate),
            }
        }
        /// The error that ended a stream (its decoder failed or crashed).
        fn take_error(&mut self) -> Option<String> {
            match self {
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => h.pop_error(),
                _ => None,
            }
        }
        /// Volume, panning and speed, smoothed over Kira's default 10 ms.
        fn apply(&mut self, volume: f32, pan: f32, speed: f32) {
            self.set_volume(volume, Tween::default());
            self.set_speed(speed, Tween::default());
            match self {
                Self::Static(h) => h.set_panning(pan, Tween::default()),
                #[cfg(not(target_arch = "wasm32"))]
                Self::Stream(h) => h.set_panning(pan, Tween::default()),
                #[cfg(any(target_arch = "wasm32", feature = "video"))]
                Self::Chunked(h) => h.set_panning(pan),
            }
        }
    }
    /// Where a streamed sound's encoded bytes are: each playback opens its
    /// own decoder over them.
    enum StreamFrom {
        /// A file, opened for each playback.
        #[cfg(not(target_arch = "wasm32"))]
        File(PathBuf),
        /// Encoded bytes an asset source handed over (on the web, every
        /// streamed file's fetched copy), kept (not decoded) in memory and
        /// shared by every playback.
        Memory(Arc<[u8]>),
        /// Tests: encoded bytes whose decoder panics at its first packet,
        /// as a reader bug would.
        #[cfg(test)]
        Crashing(Arc<[u8]>),
    }
    #[cfg(not(target_arch = "wasm32"))]
    impl StreamFrom {
        /// A decoder over the sound, its header checked again (a file may
        /// have changed since it was first opened). Every call into it is
        /// guarded: a decoder panic while it plays ends the sound with an
        /// error instead of Kira's decoder thread (`stream_decoder`).
        fn open(&self) -> Result<StreamingSoundData<String>, String> {
            let decoder = match self {
                Self::File(path) => StreamDecoder::from_file(path)?,
                Self::Memory(bytes) => StreamDecoder::from_bytes(bytes)?,
                #[cfg(test)]
                Self::Crashing(bytes) => {
                    let mut decoder = StreamDecoder::from_bytes(bytes)?;
                    decoder.panic_next_decode = true;
                    decoder
                }
            };
            Ok(StreamingSoundData::from_decoder(decoder))
        }
    }
    #[derive(Default)]
    pub struct Audio {
        manager: Option<Option<Manager>>,
        /// Sounds by slot; a freed sound's slot goes to the next one made,
        /// under a new id (`ids`).
        sounds: Vec<Clip>,
        /// Each slot's length and loop points.
        meta: Vec<Meta>,
        ids: crate::handle::SlotIds,
        playing: HashMap<u64, Voice>,
        next: u64,
        pub(super) buses: BusRegistry,
        /// One Kira track per bus that has played something.
        tracks: BusTracks,
        reverb: Option<SendTrackHandle>,
        /// On the web: closed until the player's first gesture, holding
        /// the looping voices asked for before it.
        gate: Gate,
        /// How many voices are heard at once, and the frame's ranking.
        limits: limits::Limits,
        /// The music player's clock and layers.
        music: music_out::MusicOut,
        /// The output device: which one, what happened to it.
        pub(super) output: output::OutputState,
        #[cfg(feature = "3d")]
        pub(crate) spatial: super::spatial::SpatialState,
    }
    impl Audio {
        pub fn unlocked(&self) -> bool {
            let unlocked = !self.gate.is_locked();
            #[cfg(target_arch = "wasm32")]
            let unlocked = unlocked
                && self
                    .manager
                    .as_ref()
                    .and_then(Option::as_ref)
                    .is_some_and(Manager::ready);
            unlocked
        }
        /// A click, tap or key press: on the web the first one opens the
        /// output (browsers allow it now) and starts the voices that waited.
        pub fn user_gesture(&mut self) {
            if !self.gate.is_locked() {
                return;
            }
            let waiting = self.gate.open();
            // Opened here, inside the gesture, so the browser lets it play.
            self.manager();
            for voice in waiting {
                #[cfg_attr(not(feature = "3d"), allow(unused_mut))]
                let mut v = Voice::new(voice.sound, voice.params, voice.bus, voice.effects);
                #[cfg(feature = "3d")]
                {
                    v.spatial = voice.spatial;
                }
                self.playing.insert(voice.id, v);
                self.admit(voice.id);
            }
        }
        /// For tools: whether the output device was opened (it opens on first use).
        pub(crate) fn device_state(&self) -> &'static str {
            match &self.manager {
                None => "not_opened",
                Some(None) => "failed",
                Some(Some(_)) => "open",
            }
        }
        fn manager(&mut self) -> Option<&mut Manager> {
            if self.manager.is_none() {
                let opened = self.output.open_chosen();
                self.manager = Some(opened);
            }
            self.manager.as_mut()?.as_mut()
        }
        /// For tests: the backend playing into a buffer the test renders.
        #[cfg(test)]
        pub(crate) fn with_buffer_output(sample_rate: u32) -> (Self, super::dsp::buffer::Output) {
            let (manager, output) = Manager::buffer(sample_rate).expect("the buffer backend opens");
            (
                Self {
                    manager: Some(Some(manager)),
                    ..Default::default()
                },
                output,
            )
        }
        pub fn load(&mut self, bytes: Vec<u8>) -> Result<(u32, f32), String> {
            Ok(self.add(decode(bytes)?))
        }
        /// Sounds made and not freed, and the bytes they hold: decoded
        /// samples, or for a sound streamed from memory its encoded bytes
        /// (one streamed from a file is read as it plays: 0).
        pub fn inventory(&self) -> (usize, u64) {
            let live = self.sounds.iter().filter(|clip| !matches!(clip, Clip::Freed));
            (live.clone().count(), live.map(Self::clip_bytes).sum())
        }
        /// What sound `id` holds, as `inventory` counts it.
        pub fn sound_bytes(&self, id: u32) -> u64 {
            self.clip(id).map_or(0, Self::clip_bytes)
        }
        fn clip_bytes(clip: &Clip) -> u64 {
            match clip {
                Clip::Static(data) => (data.frames.len() * std::mem::size_of::<Frame>()) as u64,
                #[cfg(not(target_arch = "wasm32"))]
                Clip::Stream(StreamFrom::File(_)) => 0,
                Clip::Stream(StreamFrom::Memory(bytes)) => bytes.len() as u64,
                #[cfg(test)]
                Clip::Stream(StreamFrom::Crashing(bytes)) => bytes.len() as u64,
                #[cfg(feature = "video")]
                Clip::Chunked(bytes) => bytes.len() as u64,
                Clip::Freed => 0,
            }
        }
        pub fn replace(&mut self, id: u32, bytes: Vec<u8>) -> Result<(), String> {
            let data = decode(bytes)?;
            let seconds = data.duration().as_secs_f64();
            let slot = self.ids.slot(id);
            if let Some(clip) = self.clip_mut(id)
                && !matches!(clip, Clip::Freed)
            {
                *clip = Clip::Static(Box::new(data));
                if let Some(meta) = slot.and_then(|s| self.meta.get_mut(s)) {
                    meta.seconds = seconds;
                }
            }
            Ok(())
        }
        /// Drops sound `id`'s data and stops its voices.
        pub fn free(&mut self, id: u32) {
            if let Some(clip) = self.clip_mut(id)
                && !matches!(clip, Clip::Freed)
            {
                *clip = Clip::Freed;
                self.ids.release(id);
            }
            for voice in self.playing.values_mut().filter(|v| v.sound == id) {
                if let Some(real) = &mut voice.real {
                    real.handle.stop(Tween::default());
                }
            }
            self.playing.retain(|_, v| v.sound != id);
            self.gate.forget_sound(id);
        }
        #[cfg(not(target_arch = "wasm32"))]
        pub fn stream(&mut self, path: PathBuf) -> Result<(u32, f32), String> {
            if let Some(index) = self
                .sounds
                .iter()
                .position(|clip| matches!(clip, Clip::Stream(StreamFrom::File(p)) if *p == path))
            {
                let seconds = StreamFrom::File(path).open()?.duration().as_secs_f32();
                return Ok((self.ids.id_in(index), seconds));
            }
            let from = StreamFrom::File(path);
            let seconds = from.open()?.duration().as_secs_f32();
            Ok((self.push(Clip::Stream(from), seconds), seconds))
        }
        /// A sound streamed from encoded `bytes` held in memory (an asset
        /// source's): only the header is decoded now, the samples as it plays.
        #[cfg(not(target_arch = "wasm32"))]
        pub fn stream_bytes(&mut self, bytes: Vec<u8>) -> Result<(u32, f32), String> {
            let from = StreamFrom::Memory(bytes.into());
            let seconds = from
                .open()
                .map_err(|e| format!("could not decode the sound: {e}"))?
                .duration()
                .as_secs_f32();
            Ok((self.push(Clip::Stream(from), seconds), seconds))
        }
        /// The web has no files to stream from: stream the fetched copy.
        #[cfg(target_arch = "wasm32")]
        pub fn stream(&mut self, path: PathBuf) -> Result<(u32, f32), String> {
            self.stream_bytes(crate::assets::read(&path).map_err(|e| e.to_string())?)
        }
        /// On the web: only the header is decoded now, the samples a packet
        /// at a time as it plays (`chunked`).
        #[cfg(target_arch = "wasm32")]
        pub fn stream_bytes(&mut self, bytes: Vec<u8>) -> Result<(u32, f32), String> {
            let bytes: Arc<[u8]> = bytes.into();
            let seconds = super::chunked::header(&bytes)
                .map_err(|e| format!("could not decode the sound: {e}"))?
                .seconds();
            Ok((self.push(Clip::Stream(StreamFrom::Memory(bytes)), seconds), seconds))
        }
        pub fn create(&mut self, sample_rate: u32, samples: &[f32]) -> (u32, f32) {
            let frames: Arc<[Frame]> = samples.iter().map(|&s| Frame::from_mono(s)).collect();
            self.add(StaticSoundData {
                sample_rate,
                frames,
                settings: Default::default(),
                slice: None,
            })
        }
        fn add(&mut self, data: StaticSoundData) -> (u32, f32) {
            let seconds = data.duration().as_secs_f32();
            (self.push(Clip::Static(Box::new(data)), seconds), seconds)
        }
        /// A sound [decoded](decode_detached) on another thread, added.
        pub fn load_decoded(&mut self, data: Decoded) -> (u32, f32) {
            self.add(*data)
        }
        /// Puts `clip` in a freed slot or a new one; returns its id.
        fn push(&mut self, clip: Clip, seconds: f32) -> u32 {
            let id = self.ids.make(self.sounds.len(), "sound");
            let slot = (id & crate::handle::INDEX_MASK) as usize;
            let meta = Meta {
                seconds: f64::from(seconds),
                loop_points: (0.0, None),
            };
            match self.sounds.get_mut(slot) {
                Some(s) => {
                    *s = clip;
                    self.meta[slot] = meta;
                }
                None => {
                    self.sounds.push(clip);
                    self.meta.push(meta);
                }
            }
            id
        }
        /// Slots in the sound table, live or freed.
        #[cfg(test)]
        pub fn slots(&self) -> usize {
            self.sounds.len()
        }
        /// Sound `id`'s clip; `None` for an id whose slot went to a newer sound.
        fn clip(&self, id: u32) -> Option<&Clip> {
            self.sounds.get(self.ids.slot(id)?)
        }
        fn clip_mut(&mut self, id: u32) -> Option<&mut Clip> {
            self.sounds.get_mut(self.ids.slot(id)?)
        }
        fn meta(&self, id: u32) -> Meta {
            self.ids
                .slot(id)
                .and_then(|s| self.meta.get(s))
                .copied()
                .unwrap_or_default()
        }
        /// Where looping playbacks of sound `id` go back to and turn around.
        pub fn set_loop(&mut self, id: u32, start: f64, end: Option<f64>) {
            if let Some(meta) = self.ids.slot(id).and_then(|s| self.meta.get_mut(s)) {
                meta.loop_points = (start, end);
            }
        }
        fn prepare(&mut self, bus: AudioBus) -> Option<()> {
            self.buses.ensure_default();
            self.buses.entries.get(bus.0)?;
            if self.reverb.is_none() {
                self.reverb = Some(
                    self.manager()?
                        .add_send_track(SendTrackBuilder::new().with_effect(ReverbBuilder::new().mix(1.0)))
                        .ok()?,
                );
            }
            let manager = self.manager.as_mut()?.as_mut()?;
            self.tracks.ensure(manager, &self.buses, bus)
        }
        #[cfg(test)]
        pub fn play(&mut self, sound: u32, p: SoundParams) -> u64 {
            self.play_on(sound, p, AudioBus::default(), AudioEffects::default())
        }
        pub fn play_on(&mut self, sound: u32, p: SoundParams, bus: AudioBus, effects: AudioEffects) -> u64 {
            self.playing.retain(|_, voice| {
                voice
                    .real
                    .as_ref()
                    .is_none_or(|r| r.handle.state() != PlaybackState::Stopped)
            });
            self.next += 1;
            let id = self.next;
            if self.gate.is_locked() {
                self.gate.hold(Waiting::new(id, sound, p, bus, effects));
            } else {
                self.playing.insert(id, Voice::new(sound, p, bus, effects));
                self.admit(id);
            }
            id
        }
        /// Makes voice `id` heard now, from where it is, starting at
        /// `when`. Whether it is real afterwards.
        fn realize(&mut self, id: u64, when: StartTime, fade_in: Option<Tween>) -> bool {
            let Some(v) = self.playing.get(&id) else {
                return false;
            };
            if v.real.is_some() {
                return true;
            }
            let (sound, bus, effects, looping) = (v.sound, v.bus, v.effects, v.params.looping);
            let (volume, pan, speed) = v.heard();
            let meta = self.meta(sound);
            let from = wrap(v.position, meta, looping);
            if self.prepare(bus).is_none() {
                return false;
            }
            let send = self.reverb.as_ref().map(|r| r.id());
            let gain = self.buses.chain_gain(bus);
            let mut builder = TrackBuilder::new().sound_capacity(2).sub_track_capacity(1);
            if let Some(send) = send {
                builder = builder.with_send(send, volume_db(effects.reverb * gain));
            }
            let filter = builder.add_effect(FilterBuilder::new().cutoff(f64::from(effects.low_pass_hz)));
            let Some(Ok(mut track)) = self.tracks.track(bus).map(|t| t.add_sub_track(builder)) else {
                return false;
            };
            let volume_db = volume_db(volume);
            let rate = f64::from(finite(speed, 1.0).max(0.01));
            let (start, end) = meta.loop_points;
            let Some(clip) = self.clip(sound) else {
                return false;
            };
            let handle = match clip {
                Clip::Freed => return false,
                Clip::Static(data) => {
                    let mut data = (**data)
                        .clone()
                        .volume(volume_db)
                        .playback_rate(rate)
                        .panning(pan)
                        .start_time(when)
                        .fade_in_tween(fade_in);
                    if from > 0.0 {
                        data = data.start_position(from);
                    }
                    if looping {
                        data = match end {
                            Some(end) => data.loop_region(start..end),
                            None => data.loop_region(start..),
                        };
                    }
                    track.play(data).ok().map(Handle::Static)
                }
                #[cfg(not(target_arch = "wasm32"))]
                Clip::Stream(stream) => {
                    let Ok(data) = stream.open() else {
                        return false;
                    };
                    let mut data = data
                        .volume(volume_db)
                        .playback_rate(rate)
                        .panning(pan)
                        .start_time(when)
                        .fade_in_tween(fade_in);
                    if from > 0.0 {
                        data = data.start_position(from);
                    }
                    if looping {
                        data = match end {
                            Some(end) => data.loop_region(start..end),
                            None => data.loop_region(start..),
                        };
                    }
                    track.play(data).ok().map(Handle::Stream)
                }
                #[cfg(target_arch = "wasm32")]
                Clip::Stream(StreamFrom::Memory(bytes)) => track
                    .play(
                        ChunkedData::new(bytes.clone(), volume, rate, looping)
                            .panning(pan)
                            .start_time(when)
                            .start_position(from)
                            .loop_points(start, end)
                            .fade_in(fade_in),
                    )
                    .ok()
                    .map(Handle::Chunked),
                #[cfg(feature = "video")]
                Clip::Chunked(bytes) => track
                    .play(
                        ChunkedData::new(bytes.clone(), volume, rate, looping)
                            .panning(pan)
                            .start_time(when)
                            .start_position(from)
                            .loop_points(start, end)
                            .fade_in(fade_in),
                    )
                    .ok()
                    .map(Handle::Chunked),
            };
            let Some(mut handle) = handle else {
                return false;
            };
            let Some(v) = self.playing.get_mut(&id) else {
                handle.stop(Tween::default());
                return false;
            };
            v.send_gain = gain;
            v.position = from;
            v.real = Some(Real { handle, track, filter });
            true
        }
        /// Stops voice `id` being heard and keeps its place: it goes on
        /// virtually and can be heard again from there.
        fn virtualize(&mut self, id: u64) {
            if let Some(v) = self.playing.get_mut(&id)
                && let Some(mut real) = v.real.take()
            {
                if let Some(at) = real.handle.position() {
                    v.position = at;
                }
                real.handle.stop(Tween::default());
            }
        }
        /// Brings the Kira tracks in line with the bus model: rebuilds
        /// buses whose parent or chain changed (their voices carry on from
        /// where they were), then sends volumes, effect settings, pauses,
        /// ducking and reverb sends that changed.
        pub fn sync_buses(&mut self) {
            if self.tracks.drop_stale(&self.buses) {
                let mut moved: Vec<u64> = self
                    .playing
                    .iter()
                    .filter(|(_, v)| v.real.is_some() && !self.tracks.has_track(v.bus))
                    .map(|(&id, _)| id)
                    .collect();
                moved.sort_unstable();
                for id in moved {
                    let ended = self
                        .playing
                        .get(&id)
                        .and_then(|v| v.real.as_ref())
                        .is_some_and(|r| r.handle.state() == PlaybackState::Stopped);
                    if ended {
                        self.playing.remove(&id);
                        continue;
                    }
                    self.virtualize(id);
                    self.realize(id, StartTime::Immediate, None);
                }
            }
            self.tracks.push(&self.buses);
            if let Some(send) = &self.reverb {
                for voice in self.playing.values_mut() {
                    let gain = self.buses.chain_gain(voice.bus);
                    if gain != voice.send_gain
                        && let Some(real) = &mut voice.real
                    {
                        voice.send_gain = gain;
                        let _ =
                            real.track
                                .set_send(send.id(), volume_db(voice.effects.reverb * gain), Tween::default());
                    }
                }
            }
        }
        /// For tests: seconds into voice `id`.
        #[cfg(test)]
        pub(crate) fn voice_position(&self, id: u64) -> Option<f64> {
            let v = self.playing.get(&id)?;
            Some(v.real.as_ref().and_then(|r| r.handle.position()).unwrap_or(v.position))
        }
        pub fn effects(&mut self, id: u64, effects: AudioEffects) {
            if self.gate.set_effects(id, effects) {
                return;
            }
            if let Some(voice) = self.playing.get_mut(&id) {
                let effects = effects.validated();
                voice.effects = effects;
                let Some(real) = &mut voice.real else {
                    return;
                };
                real.filter.set_cutoff(f64::from(effects.low_pass_hz), Tween::default());
                if let Some(send) = &self.reverb {
                    let gain = self.buses.chain_gain(voice.bus);
                    let _ = real
                        .track
                        .set_send(send.id(), volume_db(effects.reverb * gain), Tween::default());
                }
            }
        }
        #[cfg(feature = "3d")]
        pub fn spatialize(&mut self, id: u64, volume: f32, pan: f32, speed: f32) {
            if self.gate.spatialize(id, volume, pan, speed) {
                return;
            }
            if let Some(v) = self.playing.get_mut(&id) {
                v.spatial = Some([volume, pan, speed]);
                let (volume, pan, speed) = v.heard();
                if let Some(real) = &mut v.real {
                    real.handle.apply(volume, pan, speed);
                }
            }
        }
        pub fn stop(&mut self, id: u64) {
            if self.gate.cancel(id) {
                return;
            }
            if let Some(mut v) = self.playing.remove(&id)
                && let Some(real) = &mut v.real
            {
                real.handle.stop(Tween::default());
            }
        }
    }
    /// `position` brought inside the sound: a looping one back into its
    /// loop, a finished one-shot to its end.
    fn wrap(position: f64, meta: Meta, looping: bool) -> f64 {
        let position = position.max(0.0);
        if meta.seconds <= 0.0 {
            return 0.0;
        }
        if !looping {
            return position.min(meta.seconds);
        }
        let (start, end) = meta.loop_points;
        let end = end.unwrap_or(meta.seconds).min(meta.seconds);
        let start = start.clamp(0.0, end);
        if position < end || end - start <= 1e-9 {
            return position.min(end);
        }
        start + (position - start) % (end - start)
    }
    /// Decodes a whole sound, refusing a header that claims a sample rate,
    /// channels or length over the limits of [`sound_file`](super::sound_file)
    /// and stopping at the length limit however short the header said it was.
    fn decode(bytes: Vec<u8>) -> Result<StaticSoundData, String> {
        let mut slices = crate::assets::group::sliced::SoundSlices::new(bytes)?;
        while !slices.step()? {}
        Ok(*slices.finish())
    }
    /// A decoded sound, made on any thread (asset groups decode on workers).
    pub type Decoded = Box<StaticSoundData>;
    /// Decodes `bytes` as `Audio::load` does, on any thread.
    pub fn decode_detached(bytes: Vec<u8>) -> Result<Decoded, String> {
        decode(bytes).map(Box::new)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn audio_stream_loads_headers_reuses_handle_and_never_opens_device() {
            let path = std::env::temp_dir().join(format!(
                "ponko-audio-{}-{}.wav",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let mut wav = Vec::from(&b"RIFF"[..]);
            wav.extend_from_slice(&236u32.to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
            wav.extend_from_slice(&1u16.to_le_bytes()); // mono
            wav.extend_from_slice(&100u32.to_le_bytes());
            wav.extend_from_slice(&200u32.to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&200u32.to_le_bytes());
            wav.resize(244, 0);
            std::fs::write(&path, wav).unwrap();
            let mut audio = Audio::default();
            let first = audio.stream(path.clone());
            let second = audio.stream(path.clone());
            std::fs::remove_file(path).unwrap();
            assert_eq!(first.unwrap(), second.unwrap());
            assert_eq!(audio.sounds.len(), 1);
            assert!(audio.manager.is_none());
            assert!(matches!(&audio.sounds[0], Clip::Stream(_)));
        }

        #[test]
        fn a_decoder_panic_while_a_stream_plays_ends_that_sound_with_an_error_not_the_game() {
            use super::super::stream_decoder::tests::{hostile_wav, wav};
            let (mut audio, out) = Audio::with_buffer_output(8000);
            let tone: Vec<i16> = (0..8000).map(|i| if i % 20 < 10 { 8000 } else { -8000 }).collect();
            let bytes = wav(8000, &tone);
            let crashing = audio.push(Clip::Stream(StreamFrom::Crashing(bytes.clone())), 1.0);
            let fine = audio.push(Clip::Stream(StreamFrom::Memory(bytes)), 1.0);
            let looping = SoundParams {
                looping: true,
                ..Default::default()
            };
            let crashed = audio.play(crashing, looping);
            let playing = audio.play(fine, looping);
            // Kira decodes on its own thread: mix until it has caught up.
            let rms =
                |frames: &[[f32; 2]]| (frames.iter().map(|f| f[0] * f[0]).sum::<f32>() / frames.len() as f32).sqrt();
            let mut loudness = 0.0;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while (audio.playing.contains_key(&crashed) || loudness < 0.05) && std::time::Instant::now() < deadline {
                loudness = rms(&out.render(1.0 / 60.0));
                audio.update(1.0 / 60.0);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(!audio.playing.contains_key(&crashed), "the crashed stream ended");
            assert!(audio.playing.contains_key(&playing), "the other stream plays on");
            assert!(loudness > 0.05, "and is heard: {loudness}");
            // The known hostile header: refused when the stream opens.
            assert!(audio.stream_bytes(hostile_wav()).is_err());
        }

        /// The backend as on the web before the first click.
        fn locked() -> Audio {
            Audio {
                gate: Gate::locked(),
                ..Default::default()
            }
        }

        #[test]
        fn web_audio_before_the_first_gesture_never_opens_the_device() {
            let mut audio = locked();
            let (sound, _) = audio.create(100, &[0.1; 100]);
            let music = audio.play_on(
                sound,
                SoundParams {
                    looping: true,
                    ..Default::default()
                },
                AudioBus::default(),
                AudioEffects::default(),
            );
            let click = audio.play(sound, SoundParams::default());
            audio.effects(music, AudioEffects::default());
            assert!(!audio.unlocked());
            assert_ne!(music, click, "every voice keeps its own id");
            assert!(audio.manager.is_none());
            assert!(audio.playing.is_empty());
            // Only the music waits; its handle is what starts at the gesture.
            assert_eq!(audio.gate.open().iter().map(|v| v.id).collect::<Vec<_>>(), [music]);
        }

        #[test]
        fn stopping_or_freeing_before_the_first_gesture_cancels_the_wait() {
            let mut audio = locked();
            let (a, _) = audio.create(100, &[0.1; 100]);
            let (b, _) = audio.create(100, &[0.1; 100]);
            let looping = SoundParams {
                looping: true,
                ..Default::default()
            };
            let stopped = audio.play(a, looping);
            let freed = audio.play(b, looping);
            let kept = audio.play(a, looping);
            audio.stop(stopped);
            audio.free(b);
            assert_ne!(freed, kept);
            assert_eq!(audio.gate.open().iter().map(|v| v.id).collect::<Vec<_>>(), [kept]);
        }

        #[test]
        fn desktop_audio_is_unlocked_and_gestures_change_nothing() {
            let mut audio = Audio::default();
            assert!(audio.unlocked());
            audio.user_gesture();
            assert!(audio.manager.is_none(), "a gesture opens nothing on the desktop");
        }

        #[test]
        fn audio_headless_routing_never_initializes_manager() {
            crate::game::Game::new("audio", 1, 1).run_headless(0, |ctx| {
                let bus = ctx.audio.bus("music");
                let sound = ctx.audio.create(100, &[0.0; 100]);
                let voice = ctx
                    .audio
                    .play_on(sound, SoundParams::default(), bus, AudioEffects::default());
                ctx.audio.set_effects(voice, AudioEffects::default());
                ctx.audio.set_bus_volume(bus, 0.5);
                ctx.audio.mute_bus(bus, true);
                assert!(ctx.audio.backend.manager.is_none());
                assert!(ctx.audio.backend.playing.is_empty());
                assert!(ctx.audio.backend.reverb.is_none());
            });
        }
    }
}

#[cfg(not(feature = "audio"))]
mod backend {
    use super::{AudioBus, AudioEffects, BusRegistry, SoundParams};
    /// Nothing is decoded without the `audio` feature.
    /// A sound decoded on a worker: nothing without the `audio` feature.
    pub struct Decoded;
    /// Nothing to decode: the file only has to exist.
    pub fn decode_detached(_bytes: Vec<u8>) -> Result<Decoded, String> {
        Ok(Decoded)
    }
    #[derive(Default)]
    pub struct Audio {
        /// Sound ids: a freed sound's slot goes to the next one made.
        ids: crate::handle::SlotCounter,
        /// Sounds made and not freed (`Ctx::free` frees each id once).
        live: u32,
        next: u64,
        pub(super) buses: BusRegistry,
        voice_limit: Option<u32>,
        #[cfg(feature = "3d")]
        pub(crate) spatial: super::spatial::SpatialState,
    }
    impl Audio {
        /// For tools: this build has no audio output.
        pub(crate) fn device_state(&self) -> &'static str {
            "compiled_out"
        }
        /// Nothing to wait for: there is no sound.
        pub fn unlocked(&self) -> bool {
            true
        }
        pub fn user_gesture(&mut self) {}
        pub fn load(&mut self, _bytes: Vec<u8>) -> Result<(u32, f32), String> {
            Ok((self.make(), 0.0))
        }
        /// Nothing decoded: see `load`.
        pub fn load_decoded(&mut self, _data: Decoded) -> (u32, f32) {
            (self.make(), 0.0)
        }
        fn make(&mut self) -> u32 {
            self.live += 1;
            self.ids.make("sound")
        }
        /// Slots given out, live, freed or retired.
        #[cfg(test)]
        pub fn slots(&self) -> usize {
            self.ids.slots()
        }
        /// Sounds made and not freed; nothing is decoded without the
        /// `audio` feature.
        pub fn inventory(&self) -> (usize, u64) {
            (self.live as usize, 0)
        }
        pub fn sound_bytes(&self, _id: u32) -> u64 {
            0
        }
        pub fn stream(&mut self, _path: std::path::PathBuf) -> Result<(u32, f32), String> {
            self.load(Vec::new())
        }
        pub fn stream_bytes(&mut self, _bytes: Vec<u8>) -> Result<(u32, f32), String> {
            self.load(Vec::new())
        }
        pub fn replace(&mut self, _id: u32, _bytes: Vec<u8>) -> Result<(), String> {
            Ok(())
        }
        pub fn free(&mut self, id: u32) {
            self.live -= 1;
            self.ids.release(id);
        }
        pub fn create(&mut self, sample_rate: u32, samples: &[f32]) -> (u32, f32) {
            (self.make(), samples.len() as f32 / sample_rate.max(1) as f32)
        }
        pub fn play_on(&mut self, _sound: u32, _p: SoundParams, _bus: AudioBus, _effects: AudioEffects) -> u64 {
            self.next += 1;
            self.next
        }
        /// A video's soundtrack: kept as a silent sound.
        #[cfg(feature = "video")]
        pub fn soundtrack(&mut self, _bytes: std::sync::Arc<[u8]>, _seconds: f32) -> Result<u32, String> {
            Ok(self.make())
        }
        #[cfg(feature = "video")]
        pub fn play_soundtrack(&mut self, _sound: u32, _bus: AudioBus, _from: f64) -> u64 {
            self.next += 1;
            self.next
        }
        /// Nothing is heard without the `audio` feature.
        #[cfg(feature = "video")]
        pub fn soundtrack_position(&self, _id: u64) -> Option<f64> {
            None
        }
        pub fn sync_buses(&mut self) {}
        pub fn effects(&mut self, _id: u64, _effects: AudioEffects) {}
        #[cfg(feature = "3d")]
        pub fn spatialize(&mut self, _id: u64, _volume: f32, _pan: f32, _speed: f32) {}
        pub fn stop(&mut self, _id: u64) {}
        pub fn set_loop(&mut self, _id: u32, _start: f64, _end: Option<f64>) {}
        pub fn update(&mut self, _dt: f32) {}
        pub fn set_volume(&mut self, _id: u64, _volume: f32, _seconds: f32) {}
        pub fn set_speed(&mut self, _id: u64, _speed: f32, _seconds: f32) {}
        pub fn fade_out(&mut self, _id: u64, _seconds: f32) {}
        pub fn set_priority(&mut self, _id: u64, _priority: u8) {}
        pub fn is_virtual(&self, _id: u64) -> Option<bool> {
            None
        }
        pub fn voice_count(&self) -> (u32, u32) {
            (0, 0)
        }
        pub fn set_voice_limit(&mut self, limit: u32) {
            self.voice_limit = Some(limit.min(256));
        }
        pub fn voice_limit(&self) -> u32 {
            self.voice_limit.unwrap_or(64)
        }
        #[allow(clippy::too_many_arguments)] // mirrors the kira backend
        pub fn music_play(
            &mut self,
            layers: &[(u32, f32)],
            _bpm: f32,
            _beats_per_bar: u32,
            _volume: f32,
            _bus: AudioBus,
            _fade: f32,
            _at: super::Quantize,
        ) -> Vec<u64> {
            layers
                .iter()
                .map(|_| {
                    self.next += 1;
                    self.next
                })
                .collect()
        }
        pub fn music_stop(&mut self, _fade: f32, _at: super::Quantize) {}
        pub fn music_layer(&mut self, _layer: usize, _volume: f32, _fade: f32, _at: super::Quantize) {}
        pub fn music_stinger(&mut self, _sound: u32, _bus: AudioBus, _at: super::Quantize) -> u64 {
            self.next += 1;
            self.next
        }
        pub fn music_speed(&mut self, _speed: f32, _fade: f32) {}
        pub fn output_devices(&mut self) -> Vec<super::AudioDevice> {
            Vec::new()
        }
        pub fn set_output_device(&mut self, device: Option<&str>) -> Result<(), String> {
            match device {
                Some(wanted) => Err(format!(
                    "no audio output device \"{wanted}\" (built without the `audio` feature)"
                )),
                None => Ok(()),
            }
        }
        pub fn output_device(&self) -> Option<&super::AudioDevice> {
            None
        }
        pub fn device_events(&self) -> &[super::AudioDeviceEvent] {
            &[]
        }
        pub fn open_offline(&mut self, _sample_rate: u32) {}
        // Called only by the iOS and Android window loops.
        #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
        pub fn hold_output(&mut self, _held: bool) {}
        /// Silence: nothing is mixed without the `audio` feature.
        pub fn render_offline(&mut self, seconds: f32, sample_rate: u32) -> Vec<[f32; 2]> {
            vec![[0.0; 2]; (seconds.max(0.0) * sample_rate as f32).round() as usize]
        }
    }
}

pub(crate) use backend::Audio as Backend;

/// Sounds, playing handles, buses and the listener: `ctx.audio`.
///
/// Every call is spelled here: [`load`](Self::load) and
/// [`play`](Self::play) as `ctx.audio.load(..)` and `ctx.audio.play(..)`.
/// `Ctx::load_sound` and `Ctx::play_sound` are deprecated forwarders.
///
/// - **What lives here:** loading and creating sounds, playing them (also
///   at a point in the 3D world), the handles of what is playing, mixer
///   buses and the 3D listener.
/// - **When to call it:** load in setup and keep the [`Sound`] handles;
///   play, stop and mix from any system. It borrows only `ctx.audio`, so a
///   sound can play while `ctx.ui.events()` or a `ctx.world` query is
///   being read.
/// - **What it needs:** the `audio` feature and an output device to be
///   heard. Without them, and in headless runs, every call still works and
///   is silent. Positional sound needs the `3d` feature.
///
/// ```
/// use ponko::prelude::*;
///
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let beep = ctx.audio.create(8000, &[0.0; 800]);
///     let music = ctx.audio.bus("music");
///     ctx.audio.set_bus_volume(music, 0.5);
///     let playing = ctx.audio.play_on(beep, SoundParams::default(), music, ponko::audio::AudioEffects::default());
///     ctx.audio.stop(playing);
/// });
/// ```
#[derive(Default)]
pub struct Audio {
    /// The mixer: kira, or the silent stand-in without the `audio` feature.
    pub(crate) backend: Backend,
    /// The game's sounds, by id, live or freed.
    pub(crate) sounds_live: crate::handle::SlotLiveSet,
    /// Sounds and streams loaded from files, by origin: loading one again
    /// returns the same handle.
    pub(crate) loaded: crate::asset_cache::AssetCache<Sound>,
    /// Where named assets come from, shared with the other loading services.
    pub(crate) asset_sources: crate::assets::SharedSources,
    /// Hot reload: which files sounds came from.
    pub(crate) watcher: crate::hot_reload::SharedWatcher,
    /// No window: nothing is played.
    pub(crate) headless: bool,
    /// The last handle a headless run gave out: they count up from 1.
    pub(crate) silent: u64,
    /// The sounds started since the last tick ended or frame began ([`Audio::started`]).
    pub(crate) started: Vec<SoundStarted>,
    /// A rollback simulates a tick again: sounds started now were heard
    /// the first time, so they are dropped.
    pub(crate) replaying: bool,
    /// When each live sound was made and which file it came from.
    pub(crate) ledger: crate::inventory::Ledger,
    /// The music as the game side follows it.
    pub(crate) music: music::MusicModel,
    /// Mixing into memory ([`Audio::open_offline`]): played even when headless.
    pub(crate) offline: bool,
    pub(crate) offline_rate: u32,
    /// The device asked for ([`Audio::set_output_device`]).
    pub(crate) chosen_device: Option<String>,
    /// Loop points by sound id ([`Audio::set_loop`]).
    pub(crate) loops: std::collections::HashMap<u32, (f32, Option<f32>)>,
}

/// What the fuzz target `sound` does with the bytes of a sound file: every
/// way the engine reads one (decoded whole, in slices, streamed from memory
/// and as a video's soundtrack is) must refuse it with an error or accept
/// it, never crash, hang or allocate without bound. Returns the frames and
/// sample rate of the whole decode when it accepted the file; the target
/// checks them against the limits of `sound_file`.
#[cfg(all(fuzzing, feature = "audio"))]
#[doc(hidden)]
pub fn fuzz_sound(bytes: &[u8]) -> Option<(u64, u32)> {
    let decoded = backend::decode_detached(bytes.to_vec())
        .ok()
        .map(|sound| (sound.frames.len() as u64, sound.sample_rate));
    let _ = backend::Audio::default().stream_bytes(bytes.to_vec());
    stream_decoder::fuzz_stream(bytes);
    chunked::fuzz_stream(bytes);
    decoded
}

/// What the fuzz target `webm_audio` does with the bytes of a WebM file:
/// what a video reads before its soundtrack (the container's tracks), then
/// the soundtrack as a video plays it (its header read, then decoded a
/// packet at a time with seeks and a loop). The soundtrack runs even when
/// the track check would refuse the file, so the fuzzer reaches the audio
/// decoder without having to build a valid picture track first.
#[cfg(all(fuzzing, feature = "audio", feature = "video"))]
#[doc(hidden)]
pub fn fuzz_webm_audio(bytes: &[u8]) {
    let shared: std::sync::Arc<[u8]> = bytes.into();
    let seconds = crate::video::fuzz_tracks(&shared).unwrap_or(0.0);
    let _ = backend::Audio::default().soundtrack(shared, seconds as f32);
    chunked::fuzz_stream(bytes);
}

/// The error for a sound file that didn't decode: the decoder's words, what
/// the file looks like instead, and the formats that load.
pub(crate) fn sound_error(
    path: &str,
    origin: &crate::assets::Origin,
    sniff: &crate::assets::sniff::Sniff,
    detail: &str,
) -> crate::error::Error {
    let what = match origin {
        crate::assets::Origin::File(file) => format!("{:?}", file.display().to_string()),
        crate::assets::Origin::Source { .. } => origin.describe(path),
    };
    crate::error::Error::decode(path, crate::assets::sniff::not_a_sound(&what, sniff, detail))
}

/// A sound decoded on a worker thread, for [`Audio::adopt_decoded`].
pub(crate) type DecodedSound = backend::Decoded;

/// Decodes a sound file's bytes on any thread, as [`Audio::load`] does
/// (nothing without the `audio` feature).
pub(crate) fn decode_sound(bytes: Vec<u8>) -> Result<DecodedSound, String> {
    backend::decode_detached(bytes)
}

impl Audio {
    /// Loads a sound from an [added asset source](crate::assets::Assets::add_source) or the
    /// `assets/` folder: OGG Vorbis, MP3, FLAC, WAV (PCM, float or IMA
    /// ADPCM) or AIFF, told apart by their first bytes. Loading the same file
    /// again returns the same handle. Only a file from `assets/` is hot reloaded.
    ///
    /// Without the `audio` feature nothing is decoded: the file only has to
    /// exist, and the sound lasts zero seconds.
    ///
    /// # Panics
    /// With a clear message if the file is missing or, with the `audio`
    /// feature, can't be decoded.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// let hit = ctx.audio.load("hit.ogg");
    /// ctx.audio.play(hit);
    /// # }
    /// ```
    pub fn load(&mut self, path: &str) -> Sound {
        self.try_load(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load`](Self::load) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound),
    /// [`Io`](crate::error::ErrorKind::Io), or (with the `audio` feature)
    /// [`Decode`](crate::error::ErrorKind::Decode) if it can't be decoded.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     if let Ok(music) = ctx.audio.try_load("music/bonus.ogg") {
    ///         ctx.audio.play(music);
    ///     }
    /// });
    /// ```
    pub fn try_load(&mut self, path: &str) -> Result<Sound, crate::error::Error> {
        let found = match self
            .loaded
            .lookup(&self.asset_sources, path, crate::asset_cache::AssetKind::Sound)?
        {
            crate::assets::Lookup::Cached(_, sound) => return Ok(sound),
            crate::assets::Lookup::Found(found) => found,
        };
        let origin = found.origin.clone();
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let sniff = crate::assets::sniff::Sniff::of(&bytes);
        let (id, seconds) = self
            .backend
            .load(bytes)
            .map_err(|e| sound_error(path, &origin, &sniff, &e))?;
        let sound = Sound { id, seconds };
        self.sounds_live.insert(id);
        self.ledger.own(crate::inventory::AssetClass::Sound, id, path);
        self.loaded
            .insert(origin.clone(), crate::asset_cache::AssetKind::Sound, sound);
        if let crate::assets::Origin::File(file) = origin {
            self.watcher.watch(path, file, crate::hot_reload::Asset::Sound(id));
        }
        Ok(sound)
    }

    /// Adds a sound an asset group read and [decoded](decode_sound)
    /// on a worker, as [`try_load`](Self::try_load) would have: cached by
    /// where it came from (one loaded meanwhile is kept instead), and
    /// watched if it came from `assets/`.
    pub(crate) fn adopt_decoded(&mut self, path: &str, origin: crate::assets::Origin, data: DecodedSound) -> Sound {
        if let Some(sound) = self.loaded.get(&origin, crate::asset_cache::AssetKind::Sound) {
            return sound;
        }
        let (id, seconds) = self.backend.load_decoded(data);
        let sound = Sound { id, seconds };
        self.sounds_live.insert(id);
        self.ledger.own(crate::inventory::AssetClass::Sound, id, path);
        self.loaded
            .insert(origin.clone(), crate::asset_cache::AssetKind::Sound, sound);
        if let crate::assets::Origin::File(file) = origin {
            self.watcher.watch(path, file, crate::hot_reload::Asset::Sound(id));
        }
        sound
    }

    /// Makes a sound from mono samples in -1..1 at `sample_rate` Hz, for
    /// generated effects.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// // A short 880 Hz beep that fades out.
    /// let rate = 44_100;
    /// let beep: Vec<f32> = (0..rate / 10)
    ///     .map(|i| {
    ///         let t = i as f32 / rate as f32;
    ///         (t * 880.0 * std::f32::consts::TAU).sin() * (1.0 - t * 10.0) * 0.3
    ///     })
    ///     .collect();
    /// let beep = ctx.audio.create(rate, &beep);
    /// assert!((beep.seconds() - 0.1).abs() < 0.001);
    /// # }
    /// # Game::new("t", 1, 1).run_headless(0, setup);
    /// ```
    ///
    /// # Panics
    ///
    /// If `sample_rate` is 0.
    pub fn create(&mut self, sample_rate: u32, samples: &[f32]) -> Sound {
        assert!(sample_rate > 0, "ponko: the sample rate must not be zero");
        let (id, seconds) = self.backend.create(sample_rate, samples);
        self.sounds_live.insert(id);
        self.ledger.made(crate::inventory::AssetClass::Sound, id);
        Sound { id, seconds }
    }

    /// Plays `sound` once at full volume. Silent in headless runs.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, click: Sound) {
    /// if ctx.input.mouse_pressed(MouseButton::Left) {
    ///     ctx.audio.play(click);
    /// }
    /// # }
    /// ```
    pub fn play(&mut self, sound: Sound) -> Playing {
        self.play_ex(sound, SoundParams::default())
    }

    /// Plays `sound` with a volume, speed, or looping.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, step: Sound) {
    /// let speed = 0.9 + ctx.random().stream(0).f32() * 0.2; // vary the pitch a little
    /// ctx.audio.play_ex(step, SoundParams { volume: 0.4, speed, ..Default::default() });
    /// # }
    /// ```
    pub fn play_ex(&mut self, sound: Sound, params: SoundParams) -> Playing {
        self.play_on(sound, params, AudioBus::default(), AudioEffects::default())
    }

    /// The sounds started in this tick, oldest first: by the systems of the
    /// tick that is running, or, in a frame system, by the frame's systems
    /// so far. The list is emptied when a tick ends and when a frame
    /// begins. Every way of playing adds to it ([`play`](Self::play),
    /// [`play_ex`](Self::play_ex), [`play_on`](Self::play_on), and
    /// positional sound with the `3d` feature), and so do headless runs,
    /// which play nothing, so a test asserts what the game played:
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let world = Game::new("t", 1, 1)
    ///     .setup(|ctx| {
    ///         let beep = ctx.audio.create(8000, &[0.0; 800]);
    ///         ctx.world.insert_resource(beep);
    ///         ctx.world.insert_resource(0u32);
    ///     })
    ///     .sim_system(|ctx| {
    ///         if ctx.time.sim_tick() == 2 {
    ///             let beep = *ctx.world.resource::<Sound>();
    ///             ctx.audio.play(beep);
    ///         }
    ///     })
    ///     .sim_system(|ctx| {
    ///         let heard = ctx.audio.started().len() as u32;
    ///         *ctx.world.resource_mut::<u32>() += heard * (1 + ctx.time.sim_tick() as u32);
    ///     })
    ///     .run_headless(5);
    /// assert_eq!(*world.resource::<u32>(), 3, "one sound, seen on tick 2 only");
    /// ```
    #[must_use]
    pub fn started(&self) -> &[SoundStarted] {
        &self.started
    }

    /// The tick ended or a frame began: [`started`](Self::started) starts over.
    pub(crate) fn forget_started(&mut self) {
        self.started.clear();
    }

    /// Whether browser audio is ready. Browser output opens after the player
    /// first clicks, taps or presses a key. This stays `false` if the output
    /// fails to open. Before the gesture, looping sounds (music, ambience)
    /// wait and one-shots are dropped. Returns `true` on the desktop, in
    /// headless runs and when the `audio` feature is disabled.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// if !ctx.audio.unlocked() {
    ///     ctx.gfx.draw_text("Click to start", vec2(20.0, 20.0), 24.0, Color::WHITE);
    /// }
    /// # }
    /// Game::new("t", 1, 1).run_headless(0, |ctx| assert!(ctx.audio.unlocked()));
    /// ```
    pub fn unlocked(&self) -> bool {
        self.headless || self.backend.unlocked()
    }

    /// Whether nothing is mixed: a headless run without an offline output,
    /// or a tick a rollback simulates again.
    pub(crate) fn silent_output(&self) -> bool {
        (self.headless && !self.offline) || self.replaying
    }

    /// Frees sound `id` (see [`Ctx::free`]).
    pub(crate) fn free_id(&mut self, id: u32) {
        if !self.sounds_live.remove(id) {
            return;
        }
        self.loops.remove(&id);
        self.ledger.freed(crate::inventory::AssetClass::Sound, id);
        {
            use crate::asset_cache::AssetKind::{Sound, Stream};
            self.loaded.release(&[Sound, Stream], |s| s.id == id);
        }
        self.watcher
            .forget(|a| matches!(a, crate::hot_reload::Asset::Sound(s) if *s == id));
        self.backend.free(id);
    }

    /// Stops a sound started with [`play`](Self::play) (with a
    /// short fade). Does nothing if it already finished.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, music: Sound) {
    /// let song = ctx.audio.play_ex(music, SoundParams { looping: true, ..Default::default() });
    /// // ... later
    /// ctx.audio.stop(song);
    /// # }
    /// ```
    pub fn stop(&mut self, playing: Playing) {
        #[cfg(feature = "3d")]
        self.backend.spatial.remove(playing);
        self.backend.stop(playing.0);
    }
}
