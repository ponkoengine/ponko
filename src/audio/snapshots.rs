//! Mixer snapshots (named sets of bus volumes and effect settings, blended
//! in and out over time) and the mixer state a game saves with its
//! settings.

use super::mixer::{ActiveSnapshot, finite};
use super::{Audio, AudioBus, BusEffect};
use serde::{Deserialize, Serialize};

/// A named mix to blend to: underwater, paused, a cave. It overrides some
/// buses' volumes and effect settings; everything it leaves out keeps its
/// own value. Add it with [`Audio::add_snapshot`], then start and stop it
/// with fades. Snapshots started later blend over earlier ones.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::audio::{BusEffect, BusSnapshot, EffectOverride, FilterEffect, MixerSnapshot};
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let world = ctx.audio.bus("world");
///     ctx.audio.set_bus_effects(world, &[BusEffect::Filter(FilterEffect::default())]);
///     let muffled = BusEffect::Filter(FilterEffect { cutoff_hz: 600.0, ..Default::default() });
///     ctx.audio.add_snapshot("underwater", MixerSnapshot {
///         buses: vec![BusSnapshot {
///             bus: world,
///             volume: Some(0.7),
///             effects: vec![EffectOverride { index: 0, effect: muffled }],
///         }],
///     });
///     ctx.audio.start_snapshot("underwater", 0.0);
///     assert_eq!(ctx.audio.bus_gain(world), 0.7);
/// });
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MixerSnapshot {
    /// What it changes, bus by bus.
    pub buses: Vec<BusSnapshot>,
}

/// What a [`MixerSnapshot`] changes on one bus.
///
/// ```
/// # use ponko::prelude::*;
/// let quiet = ponko::audio::BusSnapshot { bus: AudioBus::default(), volume: Some(0.2), effects: vec![] };
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BusSnapshot {
    /// The bus.
    pub bus: AudioBus,
    /// Its volume while the snapshot is on, or `None` to keep its own.
    pub volume: Option<f32>,
    /// New settings for effects in its chain.
    pub effects: Vec<EffectOverride>,
}

/// New settings for the effect at `index` in a bus's chain. It must be the
/// same kind as the effect there (a snapshot changes settings, never the
/// chain); otherwise it is ignored.
///
/// ```
/// use ponko::audio::{BusEffect, EffectOverride, FilterEffect};
/// let o: EffectOverride = serde_json::from_str(r#"{"index": 0, "effect": {"filter": {"cutoff_hz": 500}}}"#).unwrap();
/// assert_eq!(o.effect, BusEffect::Filter(FilterEffect { cutoff_hz: 500.0, ..Default::default() }));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectOverride {
    /// Position in the bus's chain, from 0.
    pub index: usize,
    /// The settings to blend to.
    pub effect: BusEffect,
}

/// The mixer's state for a settings file: each bus's volume and mute, by
/// name, and the snapshots that are on. Get it with [`Audio::mixer_state`],
/// save it with the rest of the settings (it is serde, so
/// `ctx.storage.write_json` takes it), and give it back to
/// [`Audio::restore_mixer_state`] at the next start. The mix itself (bus
/// tree, effects, ducking, snapshot definitions) comes from the game's
/// mixer file, not from here.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::audio::MixerState;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let music = ctx.audio.bus("music");
///     ctx.audio.set_bus_volume(music, 0.4);
///     let json = serde_json::to_string(&ctx.audio.mixer_state()).unwrap();
///     ctx.audio.set_bus_volume(music, 1.0);
///     ctx.audio.restore_mixer_state(&serde_json::from_str::<MixerState>(&json).unwrap());
///     assert_eq!(ctx.audio.bus_volume(music), 0.4);
/// });
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerState {
    /// Every bus, in the order they were made.
    pub buses: Vec<SavedBus>,
    /// The snapshots that are on (or fading in), in the order they started.
    pub snapshots: Vec<SavedSnapshot>,
}

/// One bus in a [`MixerState`].
///
/// ```
/// let sfx = ponko::audio::SavedBus { name: "sfx".into(), volume: 0.8, muted: false };
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedBus {
    /// The bus's name.
    pub name: String,
    /// Its volume, as [`Audio::bus_volume`] reads it.
    pub volume: f32,
    /// Whether it is muted.
    #[serde(default)]
    pub muted: bool,
}

/// A snapshot that is on, in a [`MixerState`].
///
/// ```
/// let on = ponko::audio::SavedSnapshot { name: "paused".into(), weight: 1.0 };
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSnapshot {
    /// Its name.
    pub name: String,
    /// How much of it is heard, 0–1.
    pub weight: f32,
}

impl Audio {
    /// Adds a snapshot under `name`, or replaces the one of that name (a
    /// started one stays on and blends to the new settings).
    pub fn add_snapshot(&mut self, name: &str, snapshot: MixerSnapshot) {
        let buses = &mut self.backend.buses;
        if let Some(entry) = buses.snapshots.iter_mut().find(|(n, _)| n == name) {
            entry.1 = snapshot;
        } else {
            buses.snapshots.push((name.into(), snapshot));
        }
        buses.blend();
        self.backend.sync_buses();
    }

    /// Starts blending to snapshot `name` over `fade` seconds (0: at once),
    /// from wherever it is. It stays on until stopped; starting it again
    /// moves it last, so it wins over the others. Returns `false` (and
    /// does nothing) for an unknown name. Fades run on the frame clock, so
    /// they carry on while the game is paused.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.add_snapshot("paused", Default::default());
    ///     assert!(ctx.audio.start_snapshot("paused", 0.5));
    ///     assert!(!ctx.audio.start_snapshot("missing", 0.5));
    /// });
    /// ```
    pub fn start_snapshot(&mut self, name: &str, fade: f32) -> bool {
        self.fade_snapshot(name, 1.0, fade)
    }

    /// Blends snapshot `name` back out over `fade` seconds (0: at once).
    /// Does nothing if it isn't on.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.add_snapshot("paused", Default::default());
    ///     ctx.audio.start_snapshot("paused", 0.0);
    ///     ctx.audio.stop_snapshot("paused", 0.0);
    ///     assert_eq!(ctx.audio.snapshot_weight("paused"), 0.0);
    /// });
    /// ```
    pub fn stop_snapshot(&mut self, name: &str, fade: f32) {
        if self.snapshot_weight(name) > 0.0 || self.snapshot_target(name) > 0.0 {
            self.fade_snapshot(name, 0.0, fade);
        }
    }

    /// How much of snapshot `name` is heard: 0 off, 1 fully on, between
    /// while it fades.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.add_snapshot("paused", Default::default());
    ///     ctx.audio.start_snapshot("paused", 0.0);
    ///     assert_eq!(ctx.audio.snapshot_weight("paused"), 1.0);
    /// });
    /// ```
    #[must_use]
    pub fn snapshot_weight(&self, name: &str) -> f32 {
        self.active(name).map_or(0.0, |a| a.weight)
    }

    fn snapshot_target(&self, name: &str) -> f32 {
        self.active(name).map_or(0.0, |a| a.target)
    }

    fn active(&self, name: &str) -> Option<&ActiveSnapshot> {
        let buses = &self.backend.buses;
        let index = buses.snapshots.iter().position(|(n, _)| n == name)?;
        buses.active.iter().find(|a| a.index == index)
    }

    fn fade_snapshot(&mut self, name: &str, target: f32, fade: f32) -> bool {
        let buses = &mut self.backend.buses;
        let Some(index) = buses.snapshots.iter().position(|(n, _)| n == name) else {
            return false;
        };
        let weight = buses
            .active
            .iter()
            .position(|a| a.index == index)
            .map_or(0.0, |at| buses.active.remove(at).weight);
        let fade = finite(fade, 0.0).max(0.0);
        let rate = if fade > 0.0 { fade.recip() } else { f32::INFINITY };
        buses.active.push(ActiveSnapshot {
            index,
            weight,
            target,
            rate,
        });
        if fade == 0.0 {
            buses.advance(0.0);
        }
        // The order changed even if no weight did.
        buses.blend();
        self.backend.sync_buses();
        true
    }

    /// Engine side, once a frame: moves snapshot fades along by `dt`
    /// seconds of frame time.
    pub(crate) fn advance_mixer(&mut self, dt: f32) {
        if self.backend.buses.advance(dt) {
            self.backend.sync_buses();
        }
    }

    /// The mixer's state to save with the settings: see [`MixerState`].
    #[must_use]
    pub fn mixer_state(&self) -> MixerState {
        let buses = &self.backend.buses;
        let mut saved: Vec<SavedBus> = buses
            .entries
            .iter()
            .map(|b| SavedBus {
                name: b.name.clone(),
                volume: b.volume,
                muted: b.muted,
            })
            .collect();
        if saved.is_empty() {
            saved.push(SavedBus {
                name: "sfx".into(),
                volume: 1.0,
                muted: false,
            });
        }
        MixerState {
            buses: saved,
            snapshots: buses
                .active
                .iter()
                .filter(|a| a.target > 0.0)
                .map(|a| SavedSnapshot {
                    name: buses.snapshots[a.index].0.clone(),
                    weight: a.target,
                })
                .collect(),
        }
    }

    /// Restores a saved [`MixerState`]: volumes and mutes by bus name
    /// (making buses that don't exist yet), and exactly the snapshots it
    /// lists, at once. Load the mixer file first, so its snapshots are
    /// known; unknown snapshot names are skipped.
    pub fn restore_mixer_state(&mut self, state: &MixerState) {
        let buses = &mut self.backend.buses;
        for saved in &state.buses {
            let bus = buses.named(&saved.name);
            let entry = &mut buses.entries[bus.0];
            entry.volume = finite(saved.volume, 1.0).max(0.0);
            entry.muted = saved.muted;
        }
        buses.active.clear();
        for saved in &state.snapshots {
            if let Some(index) = buses.snapshots.iter().position(|(n, _)| *n == saved.name) {
                let weight = finite(saved.weight, 1.0).clamp(0.0, 1.0);
                buses.active.push(ActiveSnapshot {
                    index,
                    weight,
                    target: weight,
                    rate: f32::INFINITY,
                });
            }
        }
        buses.blend();
        self.backend.sync_buses();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BusEffect, FilterEffect};
    use super::*;
    use crate::game::Game;

    fn muffle(ctx: &mut crate::ctx::Ctx) -> AudioBus {
        let world = ctx.audio.bus("world");
        ctx.audio
            .set_bus_effects(world, &[BusEffect::Filter(FilterEffect::default())]);
        ctx.audio.add_snapshot(
            "underwater",
            MixerSnapshot {
                buses: vec![BusSnapshot {
                    bus: world,
                    volume: Some(0.5),
                    effects: vec![EffectOverride {
                        index: 0,
                        effect: BusEffect::Filter(FilterEffect {
                            cutoff_hz: 200.0,
                            ..Default::default()
                        }),
                    }],
                }],
            },
        );
        world
    }

    fn cutoff(ctx: &crate::ctx::Ctx, bus: AudioBus) -> f32 {
        match ctx.audio.backend.buses.get(bus).unwrap().heard_effects[0] {
            BusEffect::Filter(f) => f.cutoff_hz,
            _ => unreachable!(),
        }
    }

    #[test]
    fn snapshot_fades_in_over_its_time_and_back_out() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let world = muffle(ctx);
            assert!(ctx.audio.start_snapshot("underwater", 1.0));
            ctx.audio.advance_mixer(0.5);
            assert_eq!(ctx.audio.snapshot_weight("underwater"), 0.5);
            assert!((ctx.audio.bus_gain(world) - 0.75).abs() < 1e-6);
            assert!((cutoff(ctx, world) - 2000.0).abs() < 1.0, "halfway in pitch");
            ctx.audio.advance_mixer(0.6);
            assert_eq!(ctx.audio.snapshot_weight("underwater"), 1.0);
            assert_eq!(ctx.audio.bus_gain(world), 0.5);
            assert!((cutoff(ctx, world) - 200.0).abs() < 0.01);
            ctx.audio.stop_snapshot("underwater", 2.0);
            ctx.audio.advance_mixer(1.0);
            assert_eq!(ctx.audio.snapshot_weight("underwater"), 0.5);
            ctx.audio.advance_mixer(1.0);
            assert_eq!(ctx.audio.snapshot_weight("underwater"), 0.0);
            assert_eq!(ctx.audio.bus_gain(world), 1.0);
            assert!(ctx.audio.backend.buses.active.is_empty(), "a stopped snapshot leaves");
        });
    }

    #[test]
    fn snapshot_fades_follow_frame_time_in_a_running_game() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEEN: AtomicU32 = AtomicU32::new(0);
        Game::new("t", 1, 1)
            .sim_system(|ctx: &mut crate::ctx::Ctx| {
                SEEN.store(ctx.audio.snapshot_weight("underwater").to_bits(), Ordering::Relaxed);
            })
            .run_headless(6, |ctx| {
                muffle(ctx);
                ctx.audio.start_snapshot("underwater", 0.25);
            });
        // Six frames of 1/60 s into a quarter-second fade.
        let seen = f32::from_bits(SEEN.load(Ordering::Relaxed));
        assert!((seen - 0.4).abs() < 1e-4, "{seen}");
    }

    #[test]
    fn later_snapshots_win_and_unlisted_values_stay() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let world = muffle(ctx);
            let music = ctx.audio.bus("music");
            ctx.audio.set_bus_volume(music, 0.8);
            ctx.audio.add_snapshot(
                "paused",
                MixerSnapshot {
                    buses: vec![BusSnapshot {
                        bus: world,
                        volume: Some(0.1),
                        effects: vec![],
                    }],
                },
            );
            ctx.audio.start_snapshot("underwater", 0.0);
            ctx.audio.start_snapshot("paused", 0.0);
            assert_eq!(ctx.audio.bus_gain(world), 0.1, "paused started last");
            assert!((cutoff(ctx, world) - 200.0).abs() < 0.01, "underwater's filter stays");
            assert_eq!(
                ctx.audio.bus_gain(music),
                0.8,
                "a bus no snapshot names keeps its volume"
            );
            ctx.audio.start_snapshot("underwater", 0.0);
            assert_eq!(ctx.audio.bus_gain(world), 0.5, "starting again moves it last");
        });
    }

    #[test]
    fn snapshot_override_of_another_effect_kind_is_ignored() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let world = ctx.audio.bus("world");
            ctx.audio
                .set_bus_effects(world, &[BusEffect::Reverb(Default::default())]);
            ctx.audio.add_snapshot(
                "odd",
                MixerSnapshot {
                    buses: vec![BusSnapshot {
                        bus: world,
                        volume: None,
                        effects: vec![EffectOverride {
                            index: 0,
                            effect: BusEffect::Filter(FilterEffect::default()),
                        }],
                    }],
                },
            );
            ctx.audio.start_snapshot("odd", 0.0);
            assert_eq!(
                ctx.audio.backend.buses.get(world).unwrap().heard_effects,
                [BusEffect::Reverb(Default::default())]
            );
        });
    }

    #[test]
    fn mixer_state_round_trips_volumes_mutes_and_snapshots() {
        let mut saved = None;
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            muffle(ctx);
            let music = ctx.audio.bus("music");
            ctx.audio.set_bus_volume(music, 0.3);
            ctx.audio.mute_bus(AudioBus::default(), true);
            ctx.audio.start_snapshot("underwater", 0.5);
            saved = Some(serde_json::to_string(&ctx.audio.mixer_state()).unwrap());
        });
        let saved: MixerState = serde_json::from_str(&saved.unwrap()).unwrap();
        assert_eq!(
            saved.snapshots,
            [SavedSnapshot {
                name: "underwater".into(),
                weight: 1.0
            }],
            "a fading-in snapshot saves where it is going"
        );
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let world = muffle(ctx);
            ctx.audio.restore_mixer_state(&saved);
            let music = ctx.audio.bus("music");
            assert_eq!(ctx.audio.bus_volume(music), 0.3);
            assert!(ctx.audio.bus_muted(AudioBus::default()));
            assert_eq!(ctx.audio.snapshot_weight("underwater"), 1.0);
            assert_eq!(ctx.audio.bus_gain(world), 0.5);
            assert_eq!(ctx.audio.mixer_state(), saved);
        });
    }

    #[test]
    fn restoring_skips_unknown_snapshots_and_clears_others() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            muffle(ctx);
            ctx.audio.start_snapshot("underwater", 0.0);
            ctx.audio.restore_mixer_state(&MixerState {
                buses: vec![],
                snapshots: vec![SavedSnapshot {
                    name: "gone".into(),
                    weight: 1.0,
                }],
            });
            assert_eq!(ctx.audio.snapshot_weight("underwater"), 0.0);
            assert!(ctx.audio.mixer_state().snapshots.is_empty());
        });
    }
}
