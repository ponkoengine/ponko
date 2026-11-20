//! The music player on the audio thread: one Kira clock ticking the beats
//! of the song playing, and the song's layers as voices that are never
//! virtual. Changes on the beat are scheduled at clock times, so they land
//! on the same sample in every layer.

use super::{Audio, AudioBus, AudioEffects, SoundParams, StartTime, Tween, Voice};
use crate::audio::music::{Quantize, boundary};
use kira::clock::{ClockHandle, ClockSpeed, ClockTime};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct MusicOut {
    clock: Option<ClockHandle>,
    /// The song's beat is the clock's ticks plus this.
    offset: f64,
    bpm: f32,
    beats_per_bar: u32,
    /// 0: not set (1).
    speed: f32,
    volume: f32,
    bus: AudioBus,
    /// The playing song's layers, by voice id.
    layers: Vec<u64>,
    clock_paused: bool,
}

impl MusicOut {
    fn speed(&self) -> f32 {
        if self.speed > 0.0 { self.speed } else { 1.0 }
    }
    fn ticks_per_minute(&self) -> ClockSpeed {
        ClockSpeed::TicksPerMinute(f64::from(self.bpm * self.speed()))
    }
    /// When `at` comes on the song's grid: a clock time, or now.
    fn when(&self, at: Quantize) -> StartTime {
        let Some(clock) = &self.clock else {
            return StartTime::Immediate;
        };
        if at == Quantize::Now || self.bpm <= 0.0 {
            return StartTime::Immediate;
        }
        let t = clock.time();
        let beat = t.ticks as f64 + t.fraction + self.offset;
        let target = boundary(beat, at, self.beats_per_bar, self.bpm) - self.offset;
        StartTime::ClockTime(ClockTime::from_ticks_f64(clock.id(), target.max(0.0)))
    }
}

fn tween(when: StartTime, seconds: f32) -> Tween {
    Tween {
        start_time: when,
        duration: Duration::from_secs_f32(seconds.max(0.0)),
        ..Default::default()
    }
}

impl Audio {
    /// Starts a song's layers (sound id and volume each) at `at`, fading
    /// out the old song's over `fade`. Returns the layers' voice ids.
    #[allow(clippy::too_many_arguments)] // the song's fields, passed apart
    pub fn music_play(
        &mut self,
        layers: &[(u32, f32)],
        bpm: f32,
        beats_per_bar: u32,
        volume: f32,
        bus: AudioBus,
        fade: f32,
        at: Quantize,
    ) -> Vec<u64> {
        let when = self.music.when(at);
        // The old grid's beats per second, to time the tempo change.
        let old_rate = f64::from(self.music.bpm * self.music.speed()) / 60.0;
        self.music_out(when, fade);
        // The new song's grid starts where it does.
        self.music.bpm = bpm;
        self.music.beats_per_bar = beats_per_bar.max(1);
        self.music.volume = volume;
        self.music.bus = bus;
        let mut start = when;
        let ticks = self.music.ticks_per_minute();
        if bpm > 0.0 {
            if self.music.clock.is_none() {
                self.new_music_clock(0.0);
                start = self.music.clock.as_ref().map_or(StartTime::Immediate, |c| {
                    StartTime::ClockTime(ClockTime::from_ticks_u64(c, 0))
                });
            } else if let Some(clock) = &mut self.music.clock {
                let now = clock.time();
                let now = now.ticks as f64 + now.fraction;
                let at_tick = match when {
                    StartTime::ClockTime(t) => t.ticks as f64 + t.fraction,
                    _ => now,
                };
                // A clock can't wait for its own ticks (Kira gives it only
                // the others' times), so the change waits the same time on
                // the audio thread's sample count instead.
                let wait = if old_rate > 0.0 {
                    (at_tick - now).max(0.0) / old_rate
                } else {
                    0.0
                };
                let change = Tween {
                    start_time: StartTime::Delayed(Duration::from_secs_f64(wait)),
                    duration: Duration::ZERO,
                    ..Default::default()
                };
                clock.set_speed(ticks, change);
                self.music.offset = -at_tick;
            }
        }
        let fade_in = (fade > 0.0).then(|| tween(start, fade));
        let speed = self.music.speed();
        let mut ids = Vec::with_capacity(layers.len());
        for &(sound, layer_volume) in layers {
            self.next += 1;
            let id = self.next;
            let mut voice = Voice::new(
                sound,
                SoundParams {
                    volume: layer_volume * volume,
                    speed,
                    looping: true,
                },
                bus,
                AudioEffects::default(),
            );
            voice.exempt = true;
            self.playing.insert(id, voice);
            self.realize(id, start, fade_in);
            ids.push(id);
        }
        self.music.layers.clone_from(&ids);
        ids
    }

    /// Fades the playing song's layers out from `when` and lets them go.
    fn music_out(&mut self, when: StartTime, fade: f32) {
        for id in std::mem::take(&mut self.music.layers) {
            let Some(v) = self.playing.get_mut(&id) else {
                continue;
            };
            match &mut v.real {
                Some(real) => {
                    // A short fade at least, so the cut doesn't click.
                    real.handle.stop(tween(when, fade.max(0.01)));
                    v.stopping = true;
                }
                None => {
                    self.playing.remove(&id);
                }
            }
        }
    }

    pub fn music_stop(&mut self, fade: f32, at: Quantize) {
        let when = self.music.when(at);
        self.music_out(when, fade);
    }

    pub fn music_layer(&mut self, layer: usize, volume: f32, fade: f32, at: Quantize) {
        let when = self.music.when(at);
        let Some(&id) = self.music.layers.get(layer) else {
            return;
        };
        let song_volume = self.music.volume;
        if let Some(v) = self.playing.get_mut(&id) {
            v.params.volume = volume * song_volume;
            if let Some(real) = &mut v.real {
                real.handle.set_volume(volume * song_volume, tween(when, fade));
            }
        }
    }

    pub fn music_stinger(&mut self, sound: u32, bus: AudioBus, at: Quantize) -> u64 {
        let when = self.music.when(at);
        self.next += 1;
        let id = self.next;
        let mut voice = Voice::new(sound, SoundParams::default(), bus, AudioEffects::default());
        voice.exempt = true;
        self.playing.insert(id, voice);
        self.realize(id, when, None);
        id
    }

    pub fn music_speed(&mut self, speed: f32, fade: f32) {
        self.music.speed = speed;
        let t = tween(StartTime::Immediate, fade);
        for &id in &self.music.layers {
            if let Some(v) = self.playing.get_mut(&id) {
                v.params.speed = speed;
                if let Some(real) = &mut v.real {
                    real.handle.set_speed(speed, t);
                }
            }
        }
        if self.music.bpm > 0.0 {
            let ticks = self.music.ticks_per_minute();
            if let Some(clock) = &mut self.music.clock {
                clock.set_speed(ticks, t);
            }
        }
    }

    /// A clock for the music, started at `beat` of the song.
    fn new_music_clock(&mut self, beat: f64) {
        let speed = self.music.ticks_per_minute();
        let Some(manager) = self.manager() else {
            return;
        };
        if let Ok(mut clock) = manager.add_clock(speed) {
            clock.start();
            self.music.clock = Some(clock);
            self.music.offset = beat;
            self.music.clock_paused = false;
        }
    }

    /// The output is going away: its clock goes with it.
    pub(super) fn drop_music_clock(&mut self) {
        self.music.clock = None;
    }

    /// The output was opened again: a new clock, at the beat the first
    /// layer had reached.
    pub(super) fn restart_music_clock(&mut self, position: f64) {
        self.music.clock = None;
        if self.music.bpm > 0.0 && !self.music.layers.is_empty() {
            self.new_music_clock(position * f64::from(self.music.bpm) / 60.0);
        }
    }

    /// The first layer's place in its sound, for opening the output again.
    pub(super) fn music_position(&self) -> Option<f64> {
        let id = self.music.layers.first()?;
        self.playing.get(id).map(|v| v.position)
    }

    /// Holds the clock while the music's bus is paused, so the grid stays
    /// on the music.
    pub(super) fn update_music(&mut self) {
        let paused = self.buses.paused(self.music.bus);
        if let Some(clock) = &mut self.music.clock
            && paused != self.music.clock_paused
        {
            if paused {
                clock.pause();
            } else {
                clock.start();
            }
            self.music.clock_paused = paused;
        }
    }

    /// For tests: layer `i`'s voice id.
    #[cfg(test)]
    pub(crate) fn music_layer_id(&self, i: usize) -> Option<u64> {
        self.music.layers.get(i).copied()
    }

    /// For tests: the clock's song beat.
    #[cfg(test)]
    pub(crate) fn music_clock_beat(&self) -> Option<f64> {
        let t = self.music.clock.as_ref()?.time();
        Some(t.ticks as f64 + t.fraction + self.music.offset)
    }
}
