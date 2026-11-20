//! Controlling what plays: fades and speed changes on a playing sound,
//! loop points on a sound, and the voice limits with their priorities and
//! virtual voices.
//!
//! # Voice limits
//!
//! At most [`Audio::voice_limit`] sounds (64 unless set) are heard at once,
//! and a bus can have a limit of its own ([`Audio::set_bus_voice_limit`]:
//! eight footsteps at most). Past them, sounds are ranked by priority
//! ([`Audio::set_priority`], 0–255, [`DEFAULT_PRIORITY`] unless set), then
//! by how loud they are heard (their volume, distance and buses), then by
//! age, older first. The ones ranked out are **virtual**: not mixed, but
//! still followed, so when a slot frees up (a louder sound ends, the
//! player walks closer) they are heard again from where they would be
//! now. A sound nobody can hear (beyond its maximum distance, on a muted
//! bus, at volume 0) is always virtual, so a thousand looping sources
//! around a level cost little more than the few near the player.
//!
//! A new sound is heard at once if there is room or it outranks the
//! weakest sound heard; otherwise it starts virtual and the next frame's
//! ranking decides. A real sound keeps its place until a virtual one is
//! clearly louder (by a quarter), so sounds of about the same loudness
//! don't take turns. Music ([`Audio::play_music`]) and stingers are never
//! virtual and don't count.

use super::{Audio, AudioBus, Playing, Sound};

/// The priority a sound plays at unless [`Audio::set_priority`] says
/// otherwise: the middle of 0–255.
pub const DEFAULT_PRIORITY: u8 = 128;

/// How many sounds are heard and how many are virtual (music not counted),
/// from [`Audio::voice_count`].
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let count = ctx.audio.voice_count();
///     assert_eq!((count.real, count.virtualized), (0, 0));
/// });
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct VoiceCount {
    /// Mixed and heard.
    pub real: u32,
    /// Followed but not mixed: over the limits, or inaudible.
    pub virtualized: u32,
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

impl Audio {
    /// Fades a playing sound to `volume` (1 is the volume it was played at)
    /// over `seconds` (0: at once). For a sound in the 3D world it scales
    /// what distance leaves of it. Nothing happens once it ended.
    ///
    /// A crossfade between two sounds is one fading out while the other
    /// fades in from 0:
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, old: Playing, next: Sound) {
    /// ctx.audio.fade_out(old, 2.0);
    /// let new = ctx.audio.play_ex(next, SoundParams { looping: true, ..Default::default() });
    /// ctx.audio.set_volume(new, 0.0, 0.0);
    /// ctx.audio.set_volume(new, 1.0, 2.0);
    /// # }
    /// ```
    pub fn set_volume(&mut self, playing: Playing, volume: f32, seconds: f32) {
        self.backend
            .set_volume(playing.0, finite(volume, 1.0).max(0.0), finite(seconds, 0.0));
    }

    /// Changes a playing sound's speed over `seconds`: 1 is the speed it
    /// was played at, 2 twice as fast and an octave higher (speed and
    /// pitch go together; for one without the other put a
    /// [`PitchShiftEffect`](super::PitchShiftEffect) on its bus).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, engine: Playing, throttle: f32) {
    /// ctx.audio.set_speed(engine, 0.8 + throttle * 0.6, 0.1);
    /// # }
    /// ```
    pub fn set_speed(&mut self, playing: Playing, speed: f32, seconds: f32) {
        self.backend
            .set_speed(playing.0, finite(speed, 1.0).max(0.01), finite(seconds, 0.0));
    }

    /// Fades a playing sound out over `seconds`, then stops it.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, song: Playing) {
    /// ctx.audio.fade_out(song, 1.5);
    /// # }
    /// ```
    pub fn fade_out(&mut self, playing: Playing, seconds: f32) {
        #[cfg(feature = "3d")]
        self.backend.spatial.remove(playing);
        self.backend.fade_out(playing.0, finite(seconds, 0.0).max(0.0));
    }

    /// How important a playing sound is when there are more than the voice
    /// limits allow: 0–255, higher first; [`DEFAULT_PRIORITY`] unless set.
    /// For a sound on an entity, use `AudioSource::with_priority` (feature `3d`).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, line: Sound) {
    /// let dialogue = ctx.audio.play(line);
    /// ctx.audio.set_priority(dialogue, 250); // never lost in a crowd
    /// # }
    /// ```
    pub fn set_priority(&mut self, playing: Playing, priority: u8) {
        self.backend.set_priority(playing.0, priority);
    }

    /// Whether a playing sound is virtual: followed but not heard, because
    /// of the voice limits or because it can't be heard. `false` once it
    /// ended, and always in headless runs.
    #[must_use]
    pub fn is_virtual(&self, playing: Playing) -> bool {
        self.backend.is_virtual(playing.0).unwrap_or(false)
    }

    /// How many sounds are heard and how many are virtual.
    #[must_use]
    pub fn voice_count(&self) -> VoiceCount {
        let (real, virtualized) = self.backend.voice_count();
        VoiceCount { real, virtualized }
    }

    /// How many sounds are heard at once at most, 1–256 (64 unless set).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.set_voice_limit(32);
    ///     assert_eq!(ctx.audio.voice_limit(), 32);
    ///     ctx.audio.set_voice_limit(10_000);
    ///     assert_eq!(ctx.audio.voice_limit(), 256);
    /// });
    /// ```
    pub fn set_voice_limit(&mut self, limit: u32) {
        self.backend.set_voice_limit(limit.max(1));
    }

    /// The limit [`set_voice_limit`](Self::set_voice_limit) set.
    #[must_use]
    pub fn voice_limit(&self) -> u32 {
        self.backend.voice_limit()
    }

    /// How many sounds playing on `bus` itself (not its child buses) are
    /// heard at once at most; `None`: only the overall limit.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let steps = ctx.audio.bus("footsteps");
    ///     ctx.audio.set_bus_voice_limit(steps, Some(8));
    ///     assert_eq!(ctx.audio.bus_voice_limit(steps), Some(8));
    /// });
    /// ```
    pub fn set_bus_voice_limit(&mut self, bus: AudioBus, limit: Option<u32>) {
        self.backend.buses.ensure_default();
        if let Some(state) = self.backend.buses.entries.get_mut(bus.0) {
            state.voice_limit = limit;
        }
    }

    /// The limit [`set_bus_voice_limit`](Self::set_bus_voice_limit) set.
    #[must_use]
    pub fn bus_voice_limit(&self, bus: AudioBus) -> Option<u32> {
        self.backend.buses.get(bus).and_then(|s| s.voice_limit)
    }

    /// Loop points for `sound`: a looping playback plays from the start to
    /// `end` (`None`: the end of the sound), then goes back to `start`
    /// and repeats, so an intro plays once and the rest loops seamlessly.
    /// Seconds into the sound; applies to playbacks started from now on.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let theme = ctx.audio.create(8000, &[0.0; 8000 * 10]);
    ///     ctx.audio.set_loop(theme, 2.0, None); // two seconds of intro
    ///     assert_eq!(ctx.audio.sound_loop(theme), (2.0, None));
    ///     ctx.audio.play_ex(theme, SoundParams { looping: true, ..Default::default() });
    /// });
    /// ```
    pub fn set_loop(&mut self, sound: Sound, start: f32, end: Option<f32>) {
        let start = finite(start, 0.0).max(0.0);
        let end = end.map(|e| finite(e, sound.seconds()).max(start));
        self.loops.insert(sound.id, (start, end));
        self.backend.set_loop(sound.id, f64::from(start), end.map(f64::from));
    }

    /// The loop points [`set_loop`](Self::set_loop) set: `(0, None)`, the
    /// whole sound, unless set.
    #[must_use]
    pub fn sound_loop(&self, sound: Sound) -> (f32, Option<f32>) {
        self.loops.get(&sound.id).copied().unwrap_or((0.0, None))
    }
}
