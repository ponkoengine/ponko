use super::{Audio, BusEffect, Ducking, MixerSnapshot, Playing, Sound, SoundParams};
use crate::error::{Error, ErrorKind};

/// Named mixer bus, created once with [`Audio::bus`]. Handles belong to
/// their creating context. The default handle selects the `sfx` bus.
/// ```
/// use ponko::prelude::AudioBus;
/// let bus = AudioBus::default();
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)] // the C API mirrors it as a `size_t`
pub struct AudioBus(pub(super) usize);

/// Per-voice Kira low-pass and shared reverb send settings. Invalid values
/// become defaults; cutoff is clamped to 20–20,000 Hz and send to 0–1.
/// ```
/// use ponko::audio::AudioEffects;
/// let underwater = ponko::audio::AudioEffects { low_pass_hz: 800.0, reverb: 0.3 };
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioEffects {
    /// Low-pass cutoff in Hz. Default 20,000.
    /// ```
    /// # use ponko::audio::AudioEffects;
    /// assert_eq!(ponko::audio::AudioEffects::default().low_pass_hz, 20_000.0);
    /// ```
    pub low_pass_hz: f32,
    /// Linear send gain to a shared, fully wet Kira reverb. Default zero.
    /// ```
    /// # use ponko::audio::AudioEffects;
    /// assert_eq!(ponko::audio::AudioEffects::default().reverb, 0.0);
    /// ```
    pub reverb: f32,
}
impl Default for AudioEffects {
    fn default() -> Self {
        Self {
            low_pass_hz: 20_000.0,
            reverb: 0.0,
        }
    }
}
impl AudioEffects {
    pub(super) fn validated(self) -> Self {
        Self {
            low_pass_hz: finite(self.low_pass_hz, 20_000.0).clamp(20.0, 20_000.0),
            reverb: finite(self.reverb, 0.0).clamp(0.0, 1.0),
        }
    }
}
pub(super) fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}
#[cfg(feature = "audio")]
pub(super) fn volume_db(value: f32) -> kira::Decibels {
    let value = finite(value, 0.0).max(0.0);
    if value <= 0.001 {
        kira::Decibels::SILENCE
    } else {
        kira::Decibels(20.0 * value.log10())
    }
}
pub(super) struct BusState {
    pub(super) name: String,
    /// The volume set, before snapshots.
    pub(super) volume: f32,
    pub(super) muted: bool,
    /// `None`: straight to the output.
    pub(super) parent: Option<AudioBus>,
    /// The chain set, validated, before snapshots.
    pub(super) effects: Vec<BusEffect>,
    pub(super) ducking: Vec<Ducking>,
    /// What is heard now: `volume` with the active snapshots blended in.
    pub(super) heard_volume: f32,
    /// `effects` with the active snapshots blended in (same kinds, same order).
    pub(super) heard_effects: Vec<BusEffect>,
    /// Changes when the parent or the effects' kinds change, which needs
    /// the bus's tracks built again.
    pub(super) shape: u32,
}
impl BusState {
    /// Its own gain as heard: zero while muted.
    pub(super) fn gain(&self) -> f32 {
        if self.muted { 0.0 } else { self.heard_volume }
    }
}
#[derive(Default)]
pub(super) struct BusRegistry {
    pub(super) entries: Vec<BusState>,
    /// Snapshots by name, in the order they were added.
    pub(super) snapshots: Vec<(String, MixerSnapshot)>,
    /// Started snapshots, in the order they were started (later ones win).
    pub(super) active: Vec<ActiveSnapshot>,
}
impl BusRegistry {
    pub(super) fn ensure_default(&mut self) {
        if self.entries.is_empty() {
            self.insert("sfx");
        }
    }
    fn insert(&mut self, name: &str) -> AudioBus {
        let id = AudioBus(self.entries.len());
        self.entries.push(BusState {
            name: name.into(),
            volume: 1.0,
            muted: false,
            parent: None,
            effects: Vec::new(),
            ducking: Vec::new(),
            heard_volume: 1.0,
            heard_effects: Vec::new(),
            shape: 0,
        });
        id
    }
    pub(super) fn named(&mut self, name: &str) -> AudioBus {
        self.ensure_default();
        if let Some(bus) = self.find(name) {
            bus
        } else {
            self.insert(name)
        }
    }
    pub(super) fn find(&self, name: &str) -> Option<AudioBus> {
        self.entries.iter().position(|bus| bus.name == name).map(AudioBus)
    }
    pub(super) fn get(&self, bus: AudioBus) -> Option<&BusState> {
        self.entries.get(bus.0)
    }
    /// Whether `ancestor` is `bus` or one of its parents.
    pub(super) fn descends(&self, bus: AudioBus, ancestor: AudioBus) -> bool {
        let mut at = Some(bus);
        // A tree never repeats a bus, so this ends within `len` steps.
        for _ in 0..=self.entries.len() {
            match at {
                Some(b) if b == ancestor => return true,
                Some(b) => at = self.entries.get(b.0).and_then(|s| s.parent),
                None => return false,
            }
        }
        false
    }
    /// The gain `bus` is heard at: its own times every parent's.
    pub(super) fn chain_gain(&self, bus: AudioBus) -> f32 {
        let mut gain = 1.0;
        let mut at = Some(bus);
        for _ in 0..=self.entries.len() {
            let Some(state) = at.and_then(|b| self.entries.get(b.0)) else {
                break;
            };
            gain *= state.gain();
            at = state.parent;
        }
        gain
    }
    pub(super) fn set_parent(&mut self, bus: AudioBus, parent: Option<AudioBus>) -> Result<(), Error> {
        self.ensure_default();
        if self.get(bus).is_none() || parent.is_some_and(|p| self.get(p).is_none()) {
            return Err(Error::new(ErrorKind::BadHandle, "no such audio bus"));
        }
        if let Some(p) = parent
            && self.descends(p, bus)
        {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "bus \"{}\" can't go under \"{}\": that would make a loop",
                    self.entries[bus.0].name, self.entries[p.0].name
                ),
            ));
        }
        let state = &mut self.entries[bus.0];
        if state.parent != parent {
            state.parent = parent;
            state.shape = state.shape.wrapping_add(1);
        }
        Ok(())
    }
    pub(super) fn set_effects(&mut self, bus: AudioBus, effects: &[BusEffect]) {
        self.ensure_default();
        let Some(state) = self.entries.get_mut(bus.0) else {
            return;
        };
        let shape_changed = state.effects.len() != effects.len()
            || state
                .effects
                .iter()
                .zip(effects)
                .any(|(a, b)| a.shape() != b.validated().shape());
        state.effects.clear();
        state.effects.extend(effects.iter().map(|e| e.validated()));
        if shape_changed {
            state.shape = state.shape.wrapping_add(1);
        }
        self.blend();
    }
    pub(super) fn set_ducking(&mut self, bus: AudioBus, ducking: &[Ducking]) -> Result<(), Error> {
        self.ensure_default();
        if self.get(bus).is_none() || ducking.iter().any(|d| self.get(d.key).is_none()) {
            return Err(Error::new(ErrorKind::BadHandle, "no such audio bus"));
        }
        if ducking.len() > Ducking::MAX_KEYS {
            return Err(Error::new(
                ErrorKind::Limit,
                format!("a bus can be ducked by at most {} buses", Ducking::MAX_KEYS),
            ));
        }
        if ducking.iter().any(|d| d.key == bus) {
            return Err(Error::new(ErrorKind::Invalid, "a bus can't duck itself"));
        }
        let state = &mut self.entries[bus.0];
        state.ducking.clear();
        state.ducking.extend(ducking.iter().map(|d| d.validated()));
        Ok(())
    }

    /// Sets what is heard: each bus's own values, with the started
    /// snapshots blended over them in the order they started.
    pub(super) fn blend(&mut self) {
        for state in &mut self.entries {
            state.heard_volume = state.volume;
            state.heard_effects.clone_from(&state.effects);
        }
        for active in &self.active {
            let snapshot = &self.snapshots[active.index].1;
            let w = active.weight;
            for over in &snapshot.buses {
                let Some(state) = self.entries.get_mut(over.bus.0) else {
                    continue;
                };
                if let Some(volume) = over.volume {
                    let volume = finite(volume, state.heard_volume).max(0.0);
                    state.heard_volume = super::bus_effects::lerp(state.heard_volume, volume, w);
                }
                for e in &over.effects {
                    if let Some(heard) = state.heard_effects.get_mut(e.index)
                        && heard.same_kind(&e.effect)
                    {
                        *heard = heard.lerp(&e.effect.validated(), w);
                    }
                }
            }
        }
    }

    /// Moves started and stopped snapshots `dt` seconds along their fades.
    /// Whether anything heard changed.
    pub(super) fn advance(&mut self, dt: f32) -> bool {
        let dt = finite(dt, 0.0).max(0.0);
        let mut moved = false;
        for active in &mut self.active {
            if active.weight != active.target {
                let step = if active.rate.is_finite() { active.rate * dt } else { 1.0 };
                active.weight = if active.weight < active.target {
                    (active.weight + step).min(active.target)
                } else {
                    (active.weight - step).max(active.target)
                };
                moved = true;
            }
        }
        if moved {
            self.active.retain(|a| a.target > 0.0 || a.weight > 0.0);
            self.blend();
        }
        moved
    }
}

/// A started snapshot and where its fade is.
#[derive(Clone, Copy, Debug)]
pub(super) struct ActiveSnapshot {
    pub(super) index: usize,
    /// How much of it is heard, 0–1.
    pub(super) weight: f32,
    /// Where `weight` is going: 1 started, 0 stopping.
    pub(super) target: f32,
    /// Weight per second; infinite for an instant change.
    pub(super) rate: f32,
}
impl Audio {
    /// Gets or creates a named bus. Lookup allocates only for a new name;
    /// save the returned handle. This never opens an audio device.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.audio.bus("music"), ctx.audio.bus("music"));
    /// });
    /// ```
    pub fn bus(&mut self, name: &str) -> AudioBus {
        self.backend.buses.named(name)
    }

    /// Changes a bus's linear gain, preserving its mute state. Negative or
    /// nonfinite values become silence. Finished reverb tails decay naturally.
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let music = ctx.audio.bus("music");
    /// ctx.audio.set_bus_volume(music, 0.5);
    /// # }
    /// ```
    pub fn set_bus_volume(&mut self, bus: AudioBus, volume: f32) {
        self.backend.buses.ensure_default();
        if let Some(state) = self.backend.buses.entries.get_mut(bus.0) {
            state.volume = finite(volume, 0.0).max(0.0);
        }
        self.backend.buses.blend();
        self.backend.sync_buses();
    }
    /// Mutes or unmutes a bus without losing its configured volume. Existing
    /// reverb tails decay naturally; new input to reverb is muted immediately.
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let music = ctx.audio.bus("music");
    /// ctx.audio.mute_bus(music, true);
    /// # }
    /// ```
    pub fn mute_bus(&mut self, bus: AudioBus, muted: bool) {
        self.backend.buses.ensure_default();
        if let Some(state) = self.backend.buses.entries.get_mut(bus.0) {
            state.muted = muted;
        }
        self.backend.sync_buses();
    }

    /// The volume set on a bus with [`set_bus_volume`](Self::set_bus_volume)
    /// (or a mixer file, or restored settings), before snapshots, parents
    /// and mutes. 1 for an unknown bus.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     let music = ctx.audio.bus("music");
    ///     ctx.audio.set_bus_volume(music, 0.5);
    ///     assert_eq!(ctx.audio.bus_volume(music), 0.5);
    /// });
    /// ```
    #[must_use]
    pub fn bus_volume(&self, bus: AudioBus) -> f32 {
        self.backend.buses.get(bus).map_or(1.0, |s| s.volume)
    }

    /// Whether a bus is muted with [`mute_bus`](Self::mute_bus).
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     let music = ctx.audio.bus("music");
    ///     ctx.audio.mute_bus(music, true);
    ///     assert!(ctx.audio.bus_muted(music));
    /// });
    /// ```
    #[must_use]
    pub fn bus_muted(&self, bus: AudioBus) -> bool {
        self.backend.buses.get(bus).is_some_and(|s| s.muted)
    }

    /// The gain a bus is heard at now: its volume with the started
    /// snapshots blended in, times each parent's, and zero if it or a
    /// parent is muted. Ducking, which follows the sound on the audio
    /// thread, is not included. Headless runs keep it up to date too.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     let (music, master) = (ctx.audio.bus("music"), ctx.audio.bus("master"));
    ///     ctx.audio.set_bus_parent(music, Some(master)).unwrap();
    ///     ctx.audio.set_bus_volume(master, 0.5);
    ///     ctx.audio.set_bus_volume(music, 0.5);
    ///     assert_eq!(ctx.audio.bus_gain(music), 0.25);
    /// });
    /// ```
    #[must_use]
    pub fn bus_gain(&self, bus: AudioBus) -> f32 {
        if self.backend.buses.get(bus).is_none() {
            return 1.0;
        }
        self.backend.buses.chain_gain(bus)
    }

    /// Puts a bus under another (`None`: straight to the output). A bus is
    /// heard at its own volume times its parents', a muted parent silences
    /// it, and a parent's effects and ducking act on its children's sound
    /// too. Sounds already playing on the bus (or under it) move along and
    /// carry on from where they were.
    ///
    /// # Errors
    /// [`BadHandle`](ErrorKind::BadHandle) for an unknown bus,
    /// [`Invalid`](ErrorKind::Invalid) if `parent` is the bus or under it.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     let (world, steps) = (ctx.audio.bus("world"), ctx.audio.bus("footsteps"));
    ///     ctx.audio.set_bus_parent(steps, Some(world)).unwrap();
    ///     assert_eq!(ctx.audio.bus_parent(steps), Some(world));
    ///     assert!(ctx.audio.set_bus_parent(world, Some(steps)).is_err());
    /// });
    /// ```
    pub fn set_bus_parent(&mut self, bus: AudioBus, parent: Option<AudioBus>) -> Result<(), Error> {
        self.backend.buses.set_parent(bus, parent)?;
        self.backend.sync_buses();
        Ok(())
    }

    /// The bus a bus is under, if any.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.audio.bus_parent(AudioBus::default()), None);
    /// });
    /// ```
    #[must_use]
    pub fn bus_parent(&self, bus: AudioBus) -> Option<AudioBus> {
        self.backend.buses.get(bus).and_then(|s| s.parent)
    }

    /// The bus's name, if it exists.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     let music = ctx.audio.bus("music");
    ///     assert_eq!(ctx.audio.bus_name(music), Some("music"));
    /// });
    /// ```
    #[must_use]
    pub fn bus_name(&self, bus: AudioBus) -> Option<&str> {
        self.backend.buses.get(bus).map(|s| s.name.as_str())
    }

    /// Replaces a bus's effect chain. The chain runs in order on everything
    /// the bus plays (its sounds and child buses), before its volume.
    /// Changing only parameters is smooth; adding, removing or reordering
    /// effects (or changing a delay's time) rebuilds the bus, and its
    /// playing sounds carry on from where they were. Values out of range
    /// are clamped.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::audio::{BusEffect, ReverbEffect};
    /// Game::new("audio", 1, 1).run_headless(0, |ctx| {
    ///     let world = ctx.audio.bus("world");
    ///     ctx.audio.set_bus_effects(world, &[BusEffect::Reverb(ReverbEffect { mix: 0.2, ..Default::default() })]);
    ///     assert_eq!(ctx.audio.bus_effects(world).len(), 1);
    /// });
    /// ```
    pub fn set_bus_effects(&mut self, bus: AudioBus, effects: &[BusEffect]) {
        self.backend.buses.set_effects(bus, effects);
        self.backend.sync_buses();
    }

    /// A bus's effect chain as set (clamped), before snapshots.
    #[must_use]
    pub fn bus_effects(&self, bus: AudioBus) -> &[BusEffect] {
        self.backend.buses.get(bus).map_or(&[], |s| &s.effects)
    }

    /// Replaces the buses that duck a bus (at most
    /// [`Ducking::MAX_KEYS`]); an empty slice turns ducking off. See
    /// [`Ducking`].
    ///
    /// # Errors
    /// [`BadHandle`](ErrorKind::BadHandle) for an unknown bus,
    /// [`Limit`](ErrorKind::Limit) past the maximum, [`Invalid`](ErrorKind::Invalid)
    /// if a bus would duck itself. Nothing changes then.
    pub fn set_bus_ducking(&mut self, bus: AudioBus, ducking: &[Ducking]) -> Result<(), Error> {
        self.backend.buses.set_ducking(bus, ducking)?;
        self.backend.sync_buses();
        Ok(())
    }

    /// What ducks a bus, as set (clamped).
    #[must_use]
    pub fn bus_ducking(&self, bus: AudioBus) -> &[Ducking] {
        self.backend.buses.get(bus).map_or(&[], |s| &s.ducking)
    }
    /// Plays a decoded or streamed sound on a bus with per-voice effects.
    /// `ctx.audio.play` and `ctx.audio.play_ex` use `sfx` and default effects.
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, sound: Sound) {
    /// let music = ctx.audio.bus("music");
    /// ctx.audio.play_on(sound, SoundParams::default(), music, ponko::audio::AudioEffects::default());
    /// # }
    /// ```
    pub fn play_on(&mut self, sound: Sound, params: SoundParams, bus: AudioBus, effects: AudioEffects) -> Playing {
        let playing = if self.headless {
            self.silent += 1;
            Playing(self.silent)
        } else {
            Playing(self.backend.play_on(sound.id, params, bus, effects.validated()))
        };
        self.started.push(super::SoundStarted {
            playing,
            sound,
            bus,
            volume: params.volume,
            speed: params.speed,
            looping: params.looping,
        });
        playing
    }
    /// Changes a playing voice's low-pass cutoff and reverb send with Kira
    /// smoothing. Does nothing for stopped or headless voices.
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, voice: Playing) {
    /// ctx.audio.set_effects(voice, ponko::audio::AudioEffects { low_pass_hz: 700.0, reverb: 0.4 });
    /// # }
    /// ```
    pub fn set_effects(&mut self, playing: Playing, effects: AudioEffects) {
        self.backend.effects(playing.0, effects.validated());
    }

    /// Loads music to stream, from an [added asset
    /// source](crate::assets::Assets::add_source) or `assets/`, returning the same kind
    /// of handle as `load_sound`. Each playback opens its own decoder:
    /// samples are decoded on Kira's worker thread as they play rather than
    /// held decoded in memory. A file from `assets/` is read as it plays; a
    /// source's bytes are kept in memory, still encoded. Loading the same
    /// name again returns the same handle. Streams are never hot reloaded.
    /// In a browser the fetched (or source) bytes are kept encoded and
    /// decoded a packet at a time as they play, on the page's thread.
    ///
    /// # Panics
    /// If the asset is missing or its header cannot be decoded (with audio
    /// enabled); [`try_load_stream`](Self::try_load_stream) returns the error.
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let music = ctx.audio.load_stream("music.ogg");
    /// ctx.audio.play_ex(music, SoundParams { looping: true, ..Default::default() });
    /// # }
    /// ```
    pub fn load_stream(&mut self, path: &str) -> Sound {
        self.try_load_stream(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_stream`](Self::load_stream) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] if no source has
    /// it, [`Io`](crate::error::ErrorKind::Io) if it can't be read, or (with the
    /// `audio` feature) [`Decode`](crate::error::ErrorKind::Decode) if its header
    /// can't be decoded.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let music = ctx.audio.try_load_stream("music/theme.ogg");
    ///     assert_eq!(music.unwrap_err().kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_stream(&mut self, path: &str) -> Result<Sound, crate::error::Error> {
        use crate::assets::{Lookup, Origin};
        let kind = crate::asset_cache::AssetKind::Stream;
        let found = match self.loaded.lookup(&self.asset_sources, path, kind)? {
            Lookup::Cached(_, sound) => return Ok(sound),
            Lookup::Found(found) => found,
        };
        let origin = found.origin.clone();
        let streamed = match &origin {
            Origin::File(file) => self.backend.stream(file.clone()),
            Origin::Source { .. } => {
                let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
                self.backend.stream_bytes(bytes)
            }
        };
        let (id, seconds) = streamed.map_err(|e| match &origin {
            Origin::File(file) => crate::error::Error::decode(path, format!("{}: {e}", file.display())),
            Origin::Source { .. } => crate::error::Error::decode(path, format!("{}: {e}", origin.describe(path))),
        })?;
        let sound = Sound { id, seconds };
        self.sounds_live.insert(id);
        self.ledger.own(crate::inventory::AssetClass::Sound, id, path);
        self.loaded.insert(origin, kind, sound);
        Ok(sound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_effects_validate_nonfinite_and_out_of_range_parameters() {
        assert_eq!(
            AudioEffects {
                low_pass_hz: f32::NAN,
                reverb: f32::INFINITY
            }
            .validated(),
            AudioEffects::default()
        );
        assert_eq!(
            AudioEffects {
                low_pass_hz: -100.0,
                reverb: 2.0
            }
            .validated(),
            AudioEffects {
                low_pass_hz: 20.0,
                reverb: 1.0
            }
        );
    }
    #[test]
    fn audio_bus_lookup_reuses_names_and_reserves_default() {
        let mut buses = BusRegistry::default();
        let music = buses.named("music");
        assert_ne!(music, AudioBus::default());
        assert_eq!(music, buses.named("music"));
        assert_eq!(buses.named("sfx"), AudioBus::default());
        assert_eq!(buses.entries.len(), 2);
    }
    #[test]
    fn audio_headless_buses_and_effects_never_open_device() {
        crate::game::Game::new("audio", 1, 1).run_headless(0, |ctx| {
            let music = ctx.audio.bus("music");
            ctx.audio.set_bus_volume(music, 0.4);
            ctx.audio.mute_bus(music, true);
            ctx.audio.mute_bus(music, false);
            assert_eq!(ctx.audio.backend.buses.entries[music.0].volume, 0.4);
            assert!(!ctx.audio.backend.buses.entries[music.0].muted);
            let sound = ctx.audio.create(100, &[0.0; 100]);
            let voice = ctx
                .audio
                .play_on(sound, SoundParams::default(), music, AudioEffects::default());
            ctx.audio.set_effects(
                voice,
                AudioEffects {
                    low_pass_hz: 600.0,
                    reverb: 0.5,
                },
            );
            ctx.audio.stop(voice);
        });
    }
    #[cfg(feature = "audio")]
    #[test]
    fn audio_volume_mapping_is_linear_gain_to_decibels() {
        assert_eq!(volume_db(1.0), kira::Decibels(0.0));
        assert!((volume_db(0.5).0 + 6.0206).abs() < 0.001);
        assert_eq!(volume_db(f32::NAN), kira::Decibels::SILENCE);
    }
}
