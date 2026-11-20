//! Music: a [`Song`] of one or more layers that play in sync, changed on
//! the beat. [`Audio::play_music`] starts one or crossfades to it,
//! [`Audio::set_music_layer`] brings layers in and out (adaptive music:
//! drums when a fight starts), [`Audio::play_stinger`] plays a one-shot
//! on the beat, and [`Audio::set_music_speed`] changes its tempo.
//!
//! A song's beat grid starts at its first sample, so give it a tempo
//! ([`Song::tempo`]) and cut its intro and loop ([`Audio::set_loop`]) to
//! whole bars. Changes asked for at [`Quantize::Beat`] or [`Quantize::Bar`]
//! happen on the next beat or bar line, sample-accurately on the audio
//! thread; the game side follows the same grid by the frame clock, which
//! is what [`Audio::music_beat`] reads (and all a headless run has).

use super::{Audio, AudioBus, Playing, Sound, SoundStarted};

/// When a music change happens.
///
/// ```
/// use ponko::audio::Quantize;
/// assert_eq!(Quantize::default(), Quantize::Now);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Quantize {
    /// At once.
    #[default]
    Now,
    /// On the next beat.
    Beat,
    /// On the next bar line.
    Bar,
}

/// A piece of music: layers (stems) of the same length that start
/// together and stay in sync, each at its own volume, with an optional
/// tempo for changes on the beat. Songs loop; set the loop points of each
/// layer's sound with [`Audio::set_loop`] (the same on every layer) to
/// play an intro once and loop the rest.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::audio::Song;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let (base, drums) = (ctx.audio.create(8000, &[0.0; 8000]), ctx.audio.create(8000, &[0.0; 8000]));
///     let song = Song::new(base).layer(drums, 0.0).tempo(120.0, 4);
///     assert_eq!(song.layers(), &[(base, 1.0), (drums, 0.0)]);
///     assert_eq!(song.bpm(), 120.0);
/// });
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    layers: Vec<(Sound, f32)>,
    bpm: f32,
    beats_per_bar: u32,
    volume: f32,
}

impl Song {
    /// A song of one layer, `sound`, at full volume, without a tempo (so
    /// every change happens at once).
    #[must_use]
    pub fn new(sound: Sound) -> Self {
        Self {
            layers: vec![(sound, 1.0)],
            bpm: 0.0,
            beats_per_bar: 4,
            volume: 1.0,
        }
    }
    /// A song of the layers given, each with the volume it starts at, for
    /// songs described in data; `None` without a layer.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::audio::Song;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let (pads, drums) = (ctx.audio.create(8000, &[0.0; 8000]), ctx.audio.create(8000, &[0.0; 8000]));
    ///     let song = Song::from_layers(&[(pads, 0.5), (drums, 0.0)]).unwrap();
    ///     assert_eq!(song.layers(), &[(pads, 0.5), (drums, 0.0)]);
    ///     assert!(Song::from_layers(&[]).is_none());
    /// });
    /// ```
    #[must_use]
    pub fn from_layers(layers: &[(Sound, f32)]) -> Option<Self> {
        let (&(first, volume), rest) = layers.split_first()?;
        let mut song = Self::new(first);
        song.layers[0].1 = finite(volume, 0.0).max(0.0);
        Some(
            rest.iter()
                .fold(song, |song, &(sound, volume)| song.layer(sound, volume)),
        )
    }
    /// Adds a layer, `sound`, starting at `volume` (0 to bring it in later).
    #[must_use]
    pub fn layer(mut self, sound: Sound, volume: f32) -> Self {
        self.layers.push((sound, finite(volume, 0.0).max(0.0)));
        self
    }
    /// The tempo: `bpm` beats per minute, `beats_per_bar` beats to a bar.
    /// 0 bpm (the default) means no grid: quantized changes happen at once.
    #[must_use]
    pub fn tempo(mut self, bpm: f32, beats_per_bar: u32) -> Self {
        self.bpm = finite(bpm, 0.0).max(0.0);
        self.beats_per_bar = beats_per_bar.max(1);
        self
    }
    /// The whole song's volume, over its layers' own. Default 1.
    #[must_use]
    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = finite(volume, 1.0).max(0.0);
        self
    }
    /// The layers and the volume each starts at.
    #[must_use]
    pub fn layers(&self) -> &[(Sound, f32)] {
        &self.layers
    }
    /// Beats per minute; 0 without a tempo.
    #[must_use]
    pub fn bpm(&self) -> f32 {
        self.bpm
    }
    /// Beats to a bar.
    #[must_use]
    pub fn beats_per_bar(&self) -> u32 {
        self.beats_per_bar
    }
    /// The song's own volume.
    #[must_use]
    pub fn song_volume(&self) -> f32 {
        self.volume
    }
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

/// The first beat at or after `beat` that `at` waits for (`beat` itself for
/// [`Quantize::Now`] or without a tempo).
pub(crate) fn boundary(beat: f64, at: Quantize, beats_per_bar: u32, bpm: f32) -> f64 {
    if bpm <= 0.0 {
        return beat;
    }
    // A hair of slack, so a change asked for right on a beat takes it.
    let eps = 1e-6;
    match at {
        Quantize::Now => beat,
        Quantize::Beat => (beat - eps).ceil().max(0.0),
        Quantize::Bar => {
            let bar = f64::from(beats_per_bar.max(1));
            ((beat - eps) / bar).ceil().max(0.0) * bar
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Change {
    Play(Song),
    Stop,
    Layer(usize, f32),
}

/// The music as the game side follows it: the song, its layers' volumes
/// and where in it the beat is, with changes waiting for their beat.
#[derive(Default)]
pub(crate) struct MusicModel {
    pub(crate) bus: Option<AudioBus>,
    song: Option<Song>,
    layer_volumes: Vec<f32>,
    beat: f64,
    speed: f32,
    /// Changes waiting for their beat (of the song playing when asked), in order.
    pending: Vec<(f64, Change)>,
}

impl MusicModel {
    fn speed(&self) -> f32 {
        if self.speed > 0.0 { self.speed } else { 1.0 }
    }
    fn bpm(&self) -> f32 {
        self.song.as_ref().map_or(0.0, Song::bpm)
    }
    fn at(&self, at: Quantize) -> f64 {
        let (bpb, bpm) = self.song.as_ref().map_or((4, 0.0), |s| (s.beats_per_bar, s.bpm));
        boundary(self.beat, at, bpb, bpm)
    }
    fn ask(&mut self, at: Quantize, change: Change) {
        let beat = self.at(at);
        self.pending.push((beat, change));
        self.apply_due();
    }
    fn apply_due(&mut self) {
        while let Some(i) = self.pending.iter().position(|(beat, _)| *beat <= self.beat + 1e-9) {
            let (beat, change) = self.pending.remove(i);
            match change {
                Change::Play(song) => {
                    self.beat = (self.beat - beat).max(0.0);
                    self.layer_volumes = song.layers.iter().map(|l| l.1).collect();
                    self.song = Some(song);
                    // Waiting changes were on the old song's grid.
                    for p in &mut self.pending {
                        p.0 = (p.0 - beat).max(0.0);
                    }
                }
                Change::Stop => {
                    self.song = None;
                    self.layer_volumes.clear();
                    self.beat = 0.0;
                }
                Change::Layer(i, volume) => {
                    if let Some(v) = self.layer_volumes.get_mut(i) {
                        *v = volume;
                    }
                }
            }
        }
    }
    /// One frame along, unless its bus is paused.
    pub(crate) fn advance(&mut self, dt: f32, paused: bool) {
        if paused {
            return;
        }
        if self.song.is_some() {
            self.beat += f64::from(dt.max(0.0) * self.bpm() / 60.0 * self.speed());
        } else {
            // Nothing plays: what waits happens now.
            self.beat = self.pending.iter().map(|p| p.0).fold(self.beat, f64::max);
        }
        self.apply_due();
    }
}

impl Audio {
    /// The bus music plays on: `music` unless set with
    /// [`set_music_bus`](Self::set_music_bus).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let music = ctx.audio.music_bus();
    ///     assert_eq!(music, ctx.audio.bus("music"));
    /// });
    /// ```
    pub fn music_bus(&mut self) -> AudioBus {
        match self.music.bus {
            Some(bus) => bus,
            None => {
                let bus = self.bus("music");
                self.music.bus = Some(bus);
                bus
            }
        }
    }

    /// Plays music on `bus` from now on (songs already playing stay where they are).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let score = ctx.audio.bus("score");
    ///     ctx.audio.set_music_bus(score);
    ///     assert_eq!(ctx.audio.music_bus(), score);
    /// });
    /// ```
    pub fn set_music_bus(&mut self, bus: AudioBus) {
        self.music.bus = Some(bus);
    }

    /// Plays `song`, crossfading from the one playing (if any) over `fade`
    /// seconds, starting at `at` on the playing song's grid
    /// ([`Quantize::Bar`]: the next bar line). All its layers start
    /// together and loop.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::audio::{Quantize, Song};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let (calm, fight) = (ctx.audio.create(8000, &[0.0; 8000]), ctx.audio.create(8000, &[0.0; 8000]));
    ///     ctx.audio.play_music(Song::new(calm).tempo(120.0, 4), 0.0, Quantize::Now);
    ///     assert_eq!(ctx.audio.music().unwrap().layers()[0].0, calm);
    ///     // The song's first beat is on a bar line: the change comes at once.
    ///     // Later on, it waits for the next bar line.
    ///     ctx.audio.play_music(Song::new(fight).tempo(120.0, 4), 1.0, Quantize::Bar);
    ///     assert_eq!(ctx.audio.music().unwrap().layers()[0].0, fight);
    /// });
    /// ```
    pub fn play_music(&mut self, song: Song, fade: f32, at: Quantize) {
        let bus = self.music_bus();
        let fade = finite(fade, 0.0).max(0.0);
        if !self.silent_output() {
            let layers: Vec<(u32, f32)> = song.layers.iter().map(|(s, v)| (s.id, *v)).collect();
            let ids = self
                .backend
                .music_play(&layers, song.bpm, song.beats_per_bar, song.volume, bus, fade, at);
            for ((sound, volume), id) in song.layers.iter().zip(ids) {
                self.note_started(Playing(id), *sound, bus, *volume * song.volume, 1.0, true);
            }
        } else {
            for (sound, volume) in &song.layers {
                self.silent += 1;
                let playing = Playing(self.silent);
                self.note_started(playing, *sound, bus, *volume * song.volume, 1.0, true);
            }
        }
        self.music.ask(at, Change::Play(song));
    }

    /// Stops the music, fading out over `fade` seconds from `at`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::audio::{Quantize, Song};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let theme = ctx.audio.create(8000, &[0.0; 8000]);
    ///     ctx.audio.play_music(Song::new(theme), 0.0, Quantize::Now);
    ///     ctx.audio.stop_music(2.0, Quantize::Now);
    ///     assert!(ctx.audio.music().is_none());
    /// });
    /// ```
    pub fn stop_music(&mut self, fade: f32, at: Quantize) {
        if !self.silent_output() {
            self.backend.music_stop(finite(fade, 0.0).max(0.0), at);
        }
        self.music.ask(at, Change::Stop);
    }

    /// Fades layer `layer` (0 is the first) of the playing song to `volume`
    /// over `fade` seconds, from `at`: adaptive music, such as drums that
    /// come in on the next bar when a fight starts. Nothing happens for a
    /// layer the song doesn't have.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::audio::{Quantize, Song};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let (base, drums) = (ctx.audio.create(8000, &[0.0; 8000]), ctx.audio.create(8000, &[0.0; 8000]));
    ///     ctx.audio.play_music(Song::new(base).layer(drums, 0.0), 0.0, Quantize::Now);
    ///     ctx.audio.set_music_layer(1, 1.0, 0.5, Quantize::Now);
    ///     assert_eq!(ctx.audio.music_layer_volume(1), 1.0);
    /// });
    /// ```
    pub fn set_music_layer(&mut self, layer: usize, volume: f32, fade: f32, at: Quantize) {
        let volume = finite(volume, 0.0).max(0.0);
        if !self.silent_output() {
            self.backend.music_layer(layer, volume, finite(fade, 0.0).max(0.0), at);
        }
        self.music.ask(at, Change::Layer(layer, volume));
    }

    /// Plays `sound` once on the music bus at `at` on the song's grid: a
    /// stinger, such as a fanfare on the next bar. It isn't counted
    /// against the voice limits.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::audio::Quantize;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let fanfare = ctx.audio.create(8000, &[0.0; 800]);
    ///     let playing = ctx.audio.play_stinger(fanfare, Quantize::Bar);
    ///     assert_eq!(ctx.audio.started()[0].playing, playing);
    /// });
    /// ```
    pub fn play_stinger(&mut self, sound: Sound, at: Quantize) -> Playing {
        let bus = self.music_bus();
        let playing = if self.silent_output() {
            self.silent += 1;
            Playing(self.silent)
        } else {
            Playing(self.backend.music_stinger(sound.id, bus, at))
        };
        self.note_started(playing, sound, bus, 1.0, 1.0, false);
        playing
    }

    /// Changes the music's tempo over `fade` seconds: 1.1 is 10 % faster
    /// (and higher; for the same pitch, put a
    /// [`PitchShiftEffect::undo_speed`](super::PitchShiftEffect::undo_speed)
    /// on the music bus). The beat grid follows.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| ctx.audio.set_music_speed(1.1, 2.0));
    /// ```
    pub fn set_music_speed(&mut self, speed: f32, fade: f32) {
        let speed = finite(speed, 1.0).max(0.01);
        self.music.speed = speed;
        if !self.silent_output() {
            self.backend.music_speed(speed, finite(fade, 0.0).max(0.0));
        }
    }

    /// The song playing, if any.
    #[must_use]
    pub fn music(&self) -> Option<&Song> {
        self.music.song.as_ref()
    }

    /// Beats since the playing song started (0 without one), by the frame
    /// clock; paused while its bus is.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::audio::{Quantize, Song};
    /// let mut beat = 0.0;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let theme = ctx.audio.create(8000, &[0.0; 8000]);
    ///     ctx.audio.play_music(Song::new(theme).tempo(120.0, 4), 0.0, Quantize::Now);
    ///     beat = ctx.audio.music_beat();
    /// });
    /// assert_eq!(beat, 0.0);
    /// ```
    #[must_use]
    pub fn music_beat(&self) -> f64 {
        self.music.beat
    }

    /// The volume layer `layer` of the playing song is set to (0 for a
    /// layer it doesn't have).
    #[must_use]
    pub fn music_layer_volume(&self, layer: usize) -> f32 {
        self.music.layer_volumes.get(layer).copied().unwrap_or(0.0)
    }

    /// Moves the music model one frame along.
    pub(crate) fn advance_music(&mut self, dt: f32) {
        let paused = self.music.bus.is_some_and(|b| self.backend.buses.paused(b));
        self.music.advance(dt, paused);
    }

    fn note_started(&mut self, playing: Playing, sound: Sound, bus: AudioBus, volume: f32, speed: f32, looping: bool) {
        self.started.push(SoundStarted {
            playing,
            sound,
            bus,
            volume,
            speed,
            looping,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_land_on_the_next_beat_or_bar() {
        assert_eq!(boundary(5.3, Quantize::Now, 4, 120.0), 5.3);
        assert_eq!(boundary(5.3, Quantize::Beat, 4, 120.0), 6.0);
        assert_eq!(boundary(5.3, Quantize::Bar, 4, 120.0), 8.0);
        assert_eq!(boundary(8.0, Quantize::Bar, 4, 120.0), 8.0, "on the line: now");
        assert_eq!(boundary(0.0, Quantize::Bar, 3, 90.0), 0.0);
        assert_eq!(boundary(5.3, Quantize::Bar, 4, 0.0), 5.3, "no tempo: now");
    }

    fn song(id: u32) -> Song {
        Song::new(Sound::new(id, 4.0)).tempo(120.0, 4)
    }

    #[test]
    fn a_song_change_waits_for_the_bar_line_and_starts_its_own_grid() {
        let mut m = MusicModel::default();
        m.ask(Quantize::Now, Change::Play(song(1)));
        m.advance(1.0, false); // two beats at 120 bpm
        assert!((m.beat - 2.0).abs() < 1e-9);
        m.ask(Quantize::Bar, Change::Play(song(2)));
        assert_eq!(m.song.as_ref().unwrap().layers[0].0.id, 1);
        m.advance(0.75, false); // 3.5 beats
        assert_eq!(m.song.as_ref().unwrap().layers[0].0.id, 1);
        m.advance(0.5, false); // 4.5: past the bar line
        assert_eq!(m.song.as_ref().unwrap().layers[0].0.id, 2);
        assert!((m.beat - 0.5).abs() < 1e-9, "beat {} of the new song", m.beat);
    }

    #[test]
    fn layer_changes_wait_for_their_beat_and_pause_with_the_bus() {
        let mut m = MusicModel::default();
        m.ask(Quantize::Now, Change::Play(song(1).layer(Sound::new(2, 4.0), 0.0)));
        m.advance(0.25, false); // half a beat
        m.ask(Quantize::Beat, Change::Layer(1, 1.0));
        assert_eq!(m.layer_volumes, [1.0, 0.0]);
        m.advance(10.0, true);
        assert_eq!(m.layer_volumes, [1.0, 0.0], "paused: the beat doesn't come");
        m.advance(0.3, false);
        assert_eq!(m.layer_volumes, [1.0, 1.0]);
    }

    #[test]
    fn faster_music_reaches_the_bar_sooner() {
        let mut m = MusicModel::default();
        m.ask(Quantize::Now, Change::Play(song(1)));
        m.speed = 2.0;
        m.advance(1.0, false);
        assert!((m.beat - 4.0).abs() < 1e-9);
    }
}
