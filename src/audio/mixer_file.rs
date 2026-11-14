//! The mixer as a data file: buses, their parents, effects and ducking,
//! and the snapshots, in one JSON file that hot reloads.

use super::{Audio, BusEffect, BusSnapshot, Ducking, EffectOverride, MixerSnapshot};
use crate::error::{Error, ErrorKind};
use serde::Deserialize;

/// A mixer file. Every list may be left out.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MixerFile {
    #[serde(default)]
    buses: Vec<BusDef>,
    #[serde(default)]
    snapshots: Vec<SnapshotDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusDef {
    name: String,
    /// A bus defined in this file (above or below) or already made.
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    muted: Option<bool>,
    #[serde(default)]
    effects: Vec<BusEffect>,
    #[serde(default)]
    ducking: Vec<DuckDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuckDef {
    key: String,
    #[serde(default)]
    gain: Option<f32>,
    #[serde(default)]
    threshold: Option<f32>,
    #[serde(default)]
    attack: Option<f32>,
    #[serde(default)]
    release: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotDef {
    name: String,
    #[serde(default)]
    buses: Vec<BusSnapshotDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusSnapshotDef {
    bus: String,
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    effects: Vec<EffectOverride>,
}

impl Audio {
    /// Loads the mixer from a JSON file (from an [added asset
    /// source](crate::assets::Assets::add_source) or `assets/`): its buses
    /// with their parents, volumes, effect chains and ducking, and its
    /// snapshots. Buses are made by name if they don't exist, and keep
    /// their handles. With hot reload on, saving the file applies it again
    /// to the running game; a file with a mistake changes nothing and says
    /// why. See the guide's "Mixer" section for the format.
    ///
    /// # Panics
    /// If the file is missing or wrong; [`try_load_mixer`](Self::try_load_mixer)
    /// returns the error.
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// ctx.audio.load_mixer("mixer.json");
    /// let music = ctx.audio.bus("music");
    /// # }
    /// ```
    pub fn load_mixer(&mut self, path: &str) {
        self.try_load_mixer(path).unwrap_or_else(|e| panic!("{e}"));
    }

    /// [`load_mixer`](Self::load_mixer) with errors as values:
    /// [`NotFound`](ErrorKind::NotFound), [`Io`](ErrorKind::Io),
    /// [`Json`](ErrorKind::Json) for a file that doesn't parse, or
    /// [`Invalid`](ErrorKind::Invalid) for one that names a bus it doesn't
    /// define, loops its parents, or breaks a limit.
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.audio.try_load_mixer("missing.json").unwrap_err().kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_mixer(&mut self, path: &str) -> Result<(), Error> {
        let found = self.asset_sources.find(path)?;
        let origin = found.origin.clone();
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::new(ErrorKind::Json, "the mixer file is not UTF-8 text").with_path(path))?;
        self.apply_mixer_json(&text).map_err(|e| e.with_path(path))?;
        if let crate::assets::Origin::File(file) = origin {
            self.watcher.watch(path, file, crate::hot_reload::Asset::Mixer);
        }
        Ok(())
    }

    /// Applies a mixer definition held in a string, in the format of
    /// [`load_mixer`](Self::load_mixer)'s file. On an error nothing changes.
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.apply_mixer_json(r#"{
    ///         "buses": [
    ///             {"name": "master"},
    ///             {"name": "music", "parent": "master", "volume": 0.8,
    ///              "ducking": [{"key": "dialogue", "gain": 0.3}]},
    ///             {"name": "dialogue", "parent": "master"}
    ///         ]
    ///     }"#).unwrap();
    ///     let music = ctx.audio.bus("music");
    ///     assert_eq!(ctx.audio.bus_volume(music), 0.8);
    ///     assert_eq!(ctx.audio.bus_parent(music), Some(ctx.audio.bus("master")));
    /// });
    /// ```
    pub fn apply_mixer_json(&mut self, json: &str) -> Result<(), Error> {
        let file: MixerFile = serde_json::from_str(json)?;
        self.check_mixer_file(&file)?;
        // Every name exists from here on; parents are known not to loop.
        for def in &file.buses {
            self.backend.buses.named(&def.name);
        }
        let bus = |audio: &mut Self, name: &str| audio.backend.buses.named(name);
        for def in &file.buses {
            let b = bus(self, &def.name);
            let parent = def.parent.as_deref().map(|p| bus(self, p));
            // Checked above, but a parent named only later in the file may
            // still sit under this bus from before: lift it out first.
            if let Some(p) = parent
                && self.backend.buses.descends(p, b)
            {
                self.backend.buses.set_parent(p, None)?;
            }
            self.backend.buses.set_parent(b, parent)?;
        }
        for def in &file.buses {
            let b = bus(self, &def.name);
            let ducking: Vec<Ducking> = def
                .ducking
                .iter()
                .map(|d| {
                    let base = Ducking::new(bus(self, &d.key));
                    Ducking {
                        gain: d.gain.unwrap_or(base.gain),
                        threshold: d.threshold.unwrap_or(base.threshold),
                        attack: d.attack.unwrap_or(base.attack),
                        release: d.release.unwrap_or(base.release),
                        ..base
                    }
                })
                .collect();
            let buses = &mut self.backend.buses;
            buses.set_effects(b, &def.effects);
            buses.set_ducking(b, &ducking)?;
            let state = &mut buses.entries[b.0];
            if let Some(volume) = def.volume {
                state.volume = super::mixer::finite(volume, 1.0).max(0.0);
            }
            if let Some(muted) = def.muted {
                state.muted = muted;
            }
        }
        for def in &file.snapshots {
            let snapshot = MixerSnapshot {
                buses: def
                    .buses
                    .iter()
                    .map(|b| BusSnapshot {
                        bus: bus(self, &b.bus),
                        volume: b.volume,
                        effects: b.effects.clone(),
                    })
                    .collect(),
            };
            let buses = &mut self.backend.buses;
            if let Some(entry) = buses.snapshots.iter_mut().find(|(n, _)| *n == def.name) {
                entry.1 = snapshot;
            } else {
                buses.snapshots.push((def.name.clone(), snapshot));
            }
        }
        self.backend.buses.blend();
        self.backend.sync_buses();
        Ok(())
    }

    /// Everything that can be wrong with `file` before any of it applies.
    fn check_mixer_file(&self, file: &MixerFile) -> Result<(), Error> {
        let invalid = |message: String| Err(Error::new(ErrorKind::Invalid, message));
        let buses = &self.backend.buses;
        let known =
            |name: &str| buses.find(name).is_some() || name == "sfx" || file.buses.iter().any(|b| b.name == name);
        for (i, def) in file.buses.iter().enumerate() {
            if file.buses[..i].iter().any(|b| b.name == def.name) {
                return invalid(format!("bus \"{}\" is defined twice", def.name));
            }
            if let Some(parent) = &def.parent
                && !known(parent)
            {
                return invalid(format!("bus \"{}\": no bus \"{parent}\" to go under", def.name));
            }
            if def.ducking.len() > Ducking::MAX_KEYS {
                return invalid(format!(
                    "bus \"{}\": at most {} buses can duck one bus",
                    def.name,
                    Ducking::MAX_KEYS
                ));
            }
            for d in &def.ducking {
                if !known(&d.key) {
                    return invalid(format!("bus \"{}\": no bus \"{}\" to duck it", def.name, d.key));
                }
                if d.key == def.name {
                    return invalid(format!("bus \"{}\" can't duck itself", def.name));
                }
            }
        }
        // Parents after the file applies: the file's, else what is set now.
        let parent_of = |name: &str| -> Option<String> {
            match file.buses.iter().find(|b| b.name == name) {
                Some(def) => def.parent.clone(),
                None => buses
                    .find(name)
                    .and_then(|b| buses.get(b)?.parent)
                    .and_then(|p| buses.get(p))
                    .map(|s| s.name.clone()),
            }
        };
        for def in &file.buses {
            let mut at = def.parent.clone();
            for _ in 0..=file.buses.len() + buses.entries.len() {
                match at {
                    Some(name) if name == def.name => {
                        return invalid(format!("bus \"{}\" is under itself", def.name));
                    }
                    Some(name) => at = parent_of(&name),
                    None => break,
                }
            }
        }
        for snap in &file.snapshots {
            for over in &snap.buses {
                if !known(&over.bus) {
                    return invalid(format!("snapshot \"{}\": no bus \"{}\"", snap.name, over.bus));
                }
                let chain: &[BusEffect] = match file.buses.iter().find(|b| b.name == over.bus) {
                    Some(def) => &def.effects,
                    None => buses.find(&over.bus).map_or(&[], |b| &buses.entries[b.0].effects),
                };
                for e in &over.effects {
                    match chain.get(e.index) {
                        Some(have) if have.same_kind(&e.effect) => {}
                        Some(_) => {
                            return invalid(format!(
                                "snapshot \"{}\": effect {} of bus \"{}\" is another kind",
                                snap.name, e.index, over.bus
                            ));
                        }
                        None => {
                            return invalid(format!(
                                "snapshot \"{}\": bus \"{}\" has no effect {}",
                                snap.name, over.bus, e.index
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::error::ErrorKind;
    use crate::game::Game;

    const MIXER: &str = r#"{
        "buses": [
            {"name": "master", "volume": 0.9},
            {"name": "music", "parent": "master", "volume": 0.8,
             "ducking": [{"key": "dialogue", "gain": 0.25, "attack": 0.1}]},
            {"name": "dialogue", "parent": "master"},
            {"name": "world", "parent": "master",
             "effects": [{"filter": {}}, {"reverb": {"mix": 0.1}}]}
        ],
        "snapshots": [
            {"name": "underwater", "buses": [
                {"bus": "world", "volume": 0.6, "effects": [{"index": 0, "effect": {"filter": {"cutoff_hz": 500}}}]},
                {"bus": "music", "volume": 0.4}
            ]}
        ]
    }"#;

    #[test]
    fn mixer_json_builds_the_tree_effects_ducking_and_snapshots() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            ctx.audio.apply_mixer_json(MIXER).unwrap();
            let (master, music, dialogue, world) = (
                ctx.audio.bus("master"),
                ctx.audio.bus("music"),
                ctx.audio.bus("dialogue"),
                ctx.audio.bus("world"),
            );
            assert_eq!(ctx.audio.bus_parent(music), Some(master));
            assert!((ctx.audio.bus_gain(music) - 0.72).abs() < 1e-6);
            let duck = ctx.audio.bus_ducking(music)[0];
            assert_eq!(
                (duck.key, duck.gain, duck.attack, duck.release),
                (dialogue, 0.25, 0.1, 0.5)
            );
            assert_eq!(ctx.audio.bus_effects(world).len(), 2);
            assert!(ctx.audio.start_snapshot("underwater", 0.0));
            assert!((ctx.audio.bus_gain(world) - 0.54).abs() < 1e-6);
        });
    }

    #[test]
    fn mixer_json_mistakes_change_nothing() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            ctx.audio.apply_mixer_json(MIXER).unwrap();
            let before = ctx.audio.mixer_state();
            let parents = |ctx: &mut crate::ctx::Ctx| {
                let music = ctx.audio.bus("music");
                ctx.audio.bus_parent(music)
            };
            let was = parents(ctx);
            for (bad, kind) in [
                (r#"{"buses": [{"name": "a", "parent": "nowhere"}]}"#, ErrorKind::Invalid),
                (r#"{"buses": [{"name": "a", "parent": "b"}, {"name": "b", "parent": "a"}]}"#, ErrorKind::Invalid),
                (r#"{"buses": [{"name": "master", "parent": "music"}]}"#, ErrorKind::Invalid),
                (r#"{"buses": [{"name": "a", "ducking": [{"key": "a"}]}]}"#, ErrorKind::Invalid),
                (
                    r#"{"snapshots": [{"name": "s", "buses": [{"bus": "world", "effects": [{"index": 1, "effect": {"filter": {}}}]}]}]}"#,
                    ErrorKind::Invalid,
                ),
                (
                    r#"{"snapshots": [{"name": "s", "buses": [{"bus": "music", "effects": [{"index": 0, "effect": {"filter": {}}}]}]}]}"#,
                    ErrorKind::Invalid,
                ),
                (r#"{"buses": [{"name": "a", "colour": 1}]}"#, ErrorKind::Json),
                (r#"{"buses": [{"name": "a"}"#, ErrorKind::Json),
            ] {
                let err = ctx.audio.apply_mixer_json(bad).unwrap_err();
                assert_eq!(err.kind(), kind, "{bad}: {err}");
            }
            assert_eq!(ctx.audio.mixer_state(), before);
            assert_eq!(parents(ctx), was);
            assert_eq!(ctx.audio.backend.buses.find("a"), None, "no bus made by a bad file");
        });
    }

    #[test]
    fn mixer_json_applied_again_moves_buses_without_new_handles() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            ctx.audio.apply_mixer_json(MIXER).unwrap();
            let music = ctx.audio.bus("music");
            let dialogue = ctx.audio.bus("dialogue");
            // Swap the parent order: dialogue goes under music.
            ctx.audio
                .apply_mixer_json(r#"{"buses": [{"name": "dialogue", "parent": "music"}, {"name": "music"}]}"#)
                .unwrap();
            assert_eq!(ctx.audio.bus("music"), music);
            assert_eq!(ctx.audio.bus_parent(dialogue), Some(music));
            assert_eq!(ctx.audio.bus_parent(music), None);
        });
    }
}
