//! The editor: changes to the world with undo and redo, and Play / Stop
//! (run the game, then put the world back as it was).
//!
//! Kept apart from the egui panels (`crate::inspector`) so every edit can
//! be tested without a window, and so the control endpoint's `editor.*`
//! methods make the same edits: the history lives in [`Ctx`], shared by a
//! person in the inspector and an agent over MCP. Edits work on reflected
//! components only: they're what the editor can read, write and put back.
//! Nothing here depends on egui; it costs nothing until something edits.

use std::collections::{HashMap, HashSet};

use ponko_ecs::{Component, Entity};
use serde_json::{Value, json};

use crate::{ctx::Ctx, reflect::WorldReflect};
use glam::Vec2;

#[cfg(feature = "editor")]
#[path = "editor_author.rs"]
pub(crate) mod author;
#[cfg(feature = "editor")]
#[path = "editor_cook.rs"]
pub(crate) mod cook;
#[path = "editor_document.rs"]
mod document;
#[cfg(all(feature = "editor", feature = "3d"))]
#[path = "editor_gltf.rs"]
pub(crate) mod gltf;
#[cfg(feature = "editor")]
#[path = "editor_play.rs"]
pub(crate) mod play;
#[cfg(feature = "editor")]
#[path = "editor_prefab.rs"]
pub(crate) mod prefab;
// The web build keeps the software thumbnails: it has no blocking GPU
// readback to capture a frame with, and no cache folder to keep it in.
#[cfg(all(feature = "editor", feature = "3d", not(target_arch = "wasm32")))]
#[path = "editor_thumb_studio.rs"]
pub(crate) mod thumb_studio;
#[cfg(feature = "editor")]
#[path = "editor_thumbs.rs"]
pub(crate) mod thumbs;
#[cfg(feature = "editor")]
#[path = "editor_web.rs"]
pub(crate) mod web;
#[cfg(feature = "editor")]
pub(crate) use document::DiskChange;

/// How often the document's file is looked at for changes (for tests).
#[cfg(all(test, feature = "editor"))]
pub(crate) fn document_check_every() -> web_time::Duration {
    document::CHECK_EVERY
}
pub(crate) use document::Kind as DocKind;
#[cfg(feature = "inspector")]
pub(crate) use document::Unloaded;

/// Undo steps kept; older ones are dropped.
const MAX_UNDO: usize = 200;

/// One change, as what puts it back.
#[derive(Debug)]
enum Undo {
    /// Component `name` of `e` was `before` (`None`: it didn't have it).
    Component {
        e: Entity,
        name: String,
        before: Option<Value>,
    },
    /// These entities were spawned: despawn them, with their trees (prefab
    /// parts and what is attached to them).
    Spawned(Vec<Entity>),
    /// These entities were despawned with these (reflected) components:
    /// spawn them again. Handles never come back, so they're new entities;
    /// [`Editor::remap`] points the other steps at them, and references
    /// between them (attachments, prefab parts) are kept.
    Despawned(Vec<(Entity, Value)>),
    /// Resource `name` was `before`.
    Resource { name: String, before: Value },
    /// The record of `root`'s glTF scene held this link of node `node` to
    /// a parent in another scene (`None`: none).
    #[cfg(all(feature = "editor", feature = "3d"))]
    GltfLink {
        root: Entity,
        node: String,
        link: Option<crate::three::WaitingLink>,
    },
    /// Asset document `key` (an authoring panel's file) was `before`.
    #[cfg(feature = "editor")]
    Asset { key: String, before: Value },
    /// Several changes made as one (a drag of several entities).
    Group(Vec<Undo>),
}

/// A recorded step: what puts it back, and the state of the document the
/// world was in on the other side of it (see [`Editor::dirty`]).
#[derive(Debug)]
struct Step {
    undo: Undo,
    state: u64,
}

/// Edits with undo and redo, and the world saved by Play for Stop.
#[derive(Default)]
pub(crate) struct Editor {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The state the world is in: each change makes a new one, undo and
    /// redo go back to the one they left. The document is clean while it's
    /// the state it was saved (or opened) in.
    state: u64,
    /// States handed out so far.
    states: u64,
    /// The last step is still taking changes: the component a drag in the
    /// panels keeps setting (see [`set_merged`](Self::set_merged)).
    open: Option<Merge>,
    /// The file being edited and what is known about it on disk.
    pub(crate) doc: document::Document,
    /// Entities that were despawned and brought back by an undo or redo,
    /// to the entity that stands for them now. Older steps keep naming the
    /// first ids; they are looked up here, and component values that name
    /// them are read through it.
    remap: HashMap<Entity, Entity>,
    /// The world as it was when Play was pressed, how many undo steps there
    /// were then (steps taken while playing are dropped by Stop), the
    /// remap and the state of then.
    played_from: Option<Played>,
    /// `Ctx::scene_loads` the history belongs to: a load since then
    /// replaced the entities its steps name.
    loads: u64,
    /// Files the authoring panels edit (tilemaps, graphs, materials,
    /// terrain), with what is saved of each.
    #[cfg(feature = "editor")]
    pub(crate) docs: author::Docs,
}

/// What Play saved for Stop.
struct Played {
    save: Value,
    steps: usize,
    remap: HashMap<Entity, Entity>,
    state: u64,
    /// The entities there were at Play.
    #[cfg(feature = "editor")]
    alive: std::collections::HashSet<Entity>,
    /// The glTF scenes' entities at Play: saves leave them out, so Stop
    /// spawns the scenes again and points the history at the new ones.
    #[cfg(all(feature = "editor", feature = "3d"))]
    gltf: crate::three::GltfEntities,
}

/// What a merging step changes: one component of one entity, or one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Merge {
    Component(Entity, String),
    Resource(String),
    #[cfg(feature = "editor")]
    Asset(String),
}

/// Marks an entity whose components a restore hasn't set yet.
#[derive(Component)]
struct Restoring;

impl Ctx {
    /// Runs `f` with the shared editor (see [`Ctx::editor`]). The history is
    /// dropped first if a scene was loaded since the last edit: its steps
    /// named the old world's entities.
    pub(crate) fn edit<R>(&mut self, f: impl FnOnce(&mut Editor, &mut Ctx) -> R) -> R {
        let mut editor = std::mem::take(&mut self.editor);
        // glTF scenes keep what their files made from now on, so how they
        // are edited is kept as their overrides.
        #[cfg(feature = "3d")]
        self.track_gltf_overrides();
        if editor.loads != self.scene_loads {
            // Something else loaded a scene: that file is the document now.
            editor.forget_history();
            editor.loads = self.scene_loads;
            let path = self.scene_file.clone();
            editor.doc.opened(path, document::Kind::Scene, editor.state);
        }
        let out = f(&mut editor, self);
        self.editor = editor;
        out
    }
}

impl Editor {
    /// Whether Play was pressed and Stop wasn't yet.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn playing(&self) -> bool {
        self.played_from.is_some()
    }

    /// The state the world is in (see `state`): changes with every edit,
    /// undo and redo, so a view of the world can be kept until it does.
    #[cfg(feature = "editor")]
    pub(crate) fn revision(&self) -> u64 {
        self.state
    }

    fn forget_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.remap.clear();
        self.open = None;
        self.state = self.new_state();
        if let Some(played) = &mut self.played_from {
            played.steps = 0;
            played.remap.clear();
            played.state = self.state;
        }
    }

    fn new_state(&mut self) -> u64 {
        self.states += 1;
        self.states
    }

    /// Whether the world differs from the document's file: something was
    /// changed since it was opened or saved, and not undone.
    pub fn dirty(&self) -> bool {
        // An authoring panel's file differing from the disk counts too: it
        // can outlive its steps (Stop drops the steps made while playing).
        #[cfg(feature = "editor")]
        if self.docs.any_unsaved() {
            return true;
        }
        self.state != self.doc.saved
    }

    /// Runs the game, first saving the world for [`stop`](Self::stop).
    /// Pressed again while playing (after a pause), it only resumes.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn play(&mut self, ctx: &mut Ctx) {
        if self.played_from.is_none() {
            // How the glTF scenes were edited goes in their records, so
            // Stop spawns them again as they are now.
            #[cfg(feature = "3d")]
            ctx.record_gltf_overrides(None);
            self.played_from = Some(Played {
                save: ctx.world.save(),
                steps: self.undo.len(),
                remap: self.remap.clone(),
                state: self.state,
                #[cfg(feature = "editor")]
                alive: ctx
                    .world
                    .raw_ref()
                    .archetypes()
                    .iter()
                    .flat_map(|a| a.entities().iter().copied())
                    .collect(),
                #[cfg(all(feature = "editor", feature = "3d"))]
                gltf: ctx.gltf_entities(),
            });
        }
        self.open = None;
        ctx.time.resume();
    }

    /// Pauses and puts the world back as it was when Play was pressed.
    /// Components that aren't reflected can't be saved, so aren't put back.
    /// glTF scenes are spawned again from their files, with how they were
    /// edited before Play; the history's steps name their new entities.
    /// If the world can't be put back (a lockstep session is running:
    /// [`ErrorKind::Unsupported`](crate::error::ErrorKind::Unsupported)), the Play
    /// snapshot and the undo history are kept and the simulation keeps
    /// running, so Stop can be retried.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn stop(&mut self, ctx: &mut Ctx) -> Result<(), crate::error::Error> {
        let Some(played) = &self.played_from else {
            ctx.time.pause();
            return Ok(());
        };
        // Pause only once the world is back: a refused Stop changes nothing.
        ctx.replace_world(&played.save)?;
        ctx.time.pause();
        let Some(played) = self.played_from.take() else {
            return Ok(());
        };
        self.undo.truncate(played.steps);
        self.redo.clear();
        self.remap = played.remap;
        #[cfg(all(feature = "editor", feature = "3d"))]
        {
            let (moved, failed) = ctx.respawn_gltf_entities(&played.gltf);
            for (old, new) in moved {
                self.moved(old, new);
            }
            // The scenes that don't spawn again join the ones that didn't
            // load when the document was opened: the same panel, the same
            // Retry. A scene that spawned now leaves it.
            self.doc.unloaded = failed
                .into_iter()
                .map(|(file, e)| Unloaded {
                    file,
                    why: e.message().to_string(),
                })
                .collect();
        }
        self.state = played.state;
        self.open = None;
        // The editor's own load: the steps from before Play still hold.
        self.loads = ctx.scene_loads;
        Ok(())
    }

    /// Keeps the world as it is now: Stop won't go back.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn keep(&mut self) {
        if self.played_from.take().is_some() {
            // The world is what playing made of it: not the file any more.
            self.state = self.new_state();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// A new change: it can be undone, and what was undone can't be redone.
    fn record(&mut self, step: Undo) {
        if self.undo.len() == MAX_UNDO {
            self.undo.remove(0);
            if let Some(played) = &mut self.played_from {
                played.steps = played.steps.saturating_sub(1);
            }
        }
        let state = self.state;
        self.undo.push(Step { undo: step, state });
        self.state = self.new_state();
        self.redo.clear();
        self.open = None;
    }

    /// Ends the step that [`set_merged`](Self::set_merged) and
    /// [`set_resource_merged`](Self::set_resource_merged) keep adding to:
    /// the next change is a step of its own. The panels call it when the
    /// mouse button goes up.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn end_merge(&mut self) {
        self.open = None;
    }

    /// Whether the last step takes `key`'s next change.
    fn merges(&self, key: &Merge) -> bool {
        self.open.as_ref() == Some(key) && self.redo.is_empty() && !self.undo.is_empty()
    }

    /// The entity that stands for `e` now (`e` itself unless it was
    /// despawned and brought back).
    pub fn resolve(&self, e: Entity) -> Entity {
        self.remap.get(&e).copied().unwrap_or(e)
    }

    /// `old` was despawned and `new` stands for it now: steps naming
    /// either name `new`.
    fn moved(&mut self, old: Entity, new: Entity) {
        for to in self.remap.values_mut() {
            if *to == old {
                *to = new;
            }
        }
        self.remap.insert(old, new);
    }

    /// Writes component `name` of `e` (adding it if missing), with undo.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn set(&mut self, ctx: &mut Ctx, e: Entity, name: &str, value: Value) -> Result<(), crate::error::Error> {
        let before = ctx.world.get_json(e, name).ok();
        ctx.load_scene_assets(&json!({ name: &value }))?;
        ctx.world.set_json(e, name, value)?;
        self.record(Undo::Component {
            e,
            name: name.to_string(),
            before,
        });
        settle(&mut ctx.world);
        Ok(())
    }

    /// [`set`](Self::set) for a change that goes on (a number dragged in
    /// the panels): changes to the same component, one after another, are
    /// one undo step until [`end_merge`](Self::end_merge) or another change.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn set_merged(
        &mut self,
        ctx: &mut Ctx,
        e: Entity,
        name: &str,
        value: Value,
    ) -> Result<(), crate::error::Error> {
        let key = Merge::Component(e, name.to_string());
        if !self.merges(&key) {
            self.set(ctx, e, name, value)?;
            self.open = Some(key);
            return Ok(());
        }
        ctx.load_scene_assets(&json!({ name: &value }))?;
        ctx.world.set_json(e, name, value)?;
        // The step keeps the value from before the drag; the world is in
        // a state it wasn't in before.
        self.state = self.new_state();
        settle(&mut ctx.world);
        Ok(())
    }

    /// Writes resource `name`, which must be inserted already, with undo.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn set_resource(&mut self, ctx: &mut Ctx, name: &str, value: Value) -> Result<(), crate::error::Error> {
        let before = ctx.world.resource_json(name)?;
        ctx.world.set_resource_json(name, value)?;
        self.record(Undo::Resource {
            name: name.to_string(),
            before,
        });
        Ok(())
    }

    /// [`set_resource`](Self::set_resource) for a change that goes on, merged
    /// like [`set_merged`](Self::set_merged).
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn set_resource_merged(&mut self, ctx: &mut Ctx, name: &str, value: Value) -> Result<(), crate::error::Error> {
        let key = Merge::Resource(name.to_string());
        if !self.merges(&key) {
            self.set_resource(ctx, name, value)?;
            self.open = Some(key);
            return Ok(());
        }
        ctx.world.set_resource_json(name, value)?;
        self.state = self.new_state();
        Ok(())
    }

    /// Names `e` (its [`EntityName`](crate::hierarchy::EntityName), shown in the
    /// hierarchy and saved with the scene), with undo. An empty name takes
    /// the name away.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn rename(&mut self, ctx: &mut Ctx, e: Entity, name: &str) -> Result<(), crate::error::Error> {
        let name = name.trim();
        if name.is_empty() {
            if ctx.world.get::<crate::hierarchy::EntityName>(e).is_some() {
                return self.remove(ctx, e, "EntityName");
            }
            return Ok(());
        }
        if ctx.world.get::<crate::hierarchy::EntityName>(e).map(|n| n.0.as_str()) == Some(name) {
            return Ok(());
        }
        self.set(ctx, e, "EntityName", json!(name))
    }

    /// Adds component `name` to `e` with its default fields (types whose
    /// fields all have defaults; others say which field is missing).
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn add(&mut self, ctx: &mut Ctx, e: Entity, name: &str) -> Result<(), crate::error::Error> {
        self.set(ctx, e, name, json!({}))
    }

    /// Removes component `name` from `e`, with undo.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn remove(&mut self, ctx: &mut Ctx, e: Entity, name: &str) -> Result<(), crate::error::Error> {
        let before = ctx.world.get_json(e, name)?;
        ctx.world.remove_json(e, name)?;
        self.record(Undo::Component {
            e,
            name: name.to_string(),
            before: Some(before),
        });
        Ok(())
    }

    /// Records, as one step, that these components were as given before a
    /// change made directly (a gizmo drag of several entities). Components
    /// that didn't change are left out; nothing is recorded if none did.
    pub fn changed(&mut self, world: &ponko_ecs::World, before: Before) {
        let steps: Vec<Undo> = before
            .0
            .into_iter()
            .filter(|(e, name, value)| world.get_json(*e, name).ok() != *value)
            .map(|(e, name, before)| Undo::Component { e, name, before })
            .collect();
        if !steps.is_empty() {
            self.record(Undo::Group(steps));
        }
    }

    /// Spawns an entity with only a transform at `at`.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn spawn_empty(&mut self, ctx: &mut Ctx, at: Vec2) -> Entity {
        let e = ctx.world.spawn((crate::d2::Transform::at(at),));
        self.record(Undo::Spawned(vec![e]));
        e
    }

    /// Spawns an entity with only a 3D transform `at`.
    #[cfg(feature = "3d")]
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn spawn_empty3d(&mut self, ctx: &mut Ctx, at: crate::d3::Transform) -> Entity {
        let e = ctx.world.spawn((at,));
        self.record(Undo::Spawned(vec![e]));
        e
    }

    /// Spawns prefab `path` at `at`, with undo (which despawns its parts too).
    pub fn spawn_prefab(
        &mut self,
        ctx: &mut Ctx,
        path: &str,
        at: impl Into<crate::scene::Placement>,
    ) -> Result<Entity, crate::error::Error> {
        let e = ctx.spawn_prefab(path, at)?;
        // Remembers where it came from, for overrides and apply (not glTF
        // scenes: their root names its file already).
        let ext = std::path::Path::new(path).extension().and_then(|x| x.to_str());
        if !ext.is_some_and(|x| x.eq_ignore_ascii_case("gltf") || x.eq_ignore_ascii_case("glb")) {
            ctx.world.insert(e, crate::hierarchy::PrefabInstance::new(path));
        }
        self.record(Undo::Spawned(vec![e]));
        settle(&mut ctx.world);
        Ok(e)
    }

    /// Spawns a sprite of image `path` at world point `at`, with undo.
    #[cfg(feature = "editor")]
    pub fn spawn_sprite(&mut self, ctx: &mut Ctx, path: &str, at: Vec2) -> Result<Entity, crate::error::Error> {
        let components = json!({
            "Transform2D": serde_json::to_value(crate::d2::Transform::at(at)).map_err(crate::error::Error::from)?,
            "Sprite": { "texture": path },
        });
        ctx.load_scene_assets(&components).map_err(|e| e.with_path(path))?;
        let e = ctx.world.spawn_json(components)?;
        self.record(Undo::Spawned(vec![e]));
        settle(&mut ctx.world);
        Ok(e)
    }

    /// Spawns a model of the mesh file `path` (glTF) at `at`, with undo.
    #[cfg(feature = "3d")]
    pub fn spawn_model(
        &mut self,
        ctx: &mut Ctx,
        path: &str,
        at: crate::d3::Transform,
    ) -> Result<Entity, crate::error::Error> {
        let components = json!({
            "Transform3D": serde_json::to_value(at).map_err(crate::error::Error::from)?,
            "Model": { "mesh": path },
        });
        ctx.load_scene_assets(&components).map_err(|e| e.with_path(path))?;
        let e = ctx.world.spawn_json(components)?;
        self.record(Undo::Spawned(vec![e]));
        Ok(e)
    }

    /// A copy of `e`'s reflected components, `offset` away.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn duplicate(&mut self, ctx: &mut Ctx, e: Entity, offset: Vec2) -> Result<Entity, crate::error::Error> {
        let copies = self.duplicate_many(ctx, &[e], offset, Offset3::ZERO)?;
        Ok(copies[0])
    }

    /// Copies of `roots` with their trees (prefab parts and what is
    /// attached to them; references inside a tree name the copies), moved
    /// by `offset` (2D) or `offset3` (3D). Returns the copies of `roots`.
    pub fn duplicate_many(
        &mut self,
        ctx: &mut Ctx,
        roots: &[Entity],
        offset: Vec2,
        offset3: Offset3,
    ) -> Result<Vec<Entity>, crate::error::Error> {
        let members = tree(&ctx.world, roots);
        let snapshot = snapshot(&ctx.world, &members)?;
        let map = respawn(ctx, &snapshot, HashMap::new())?;
        let copies: Vec<Entity> = roots.iter().map(|r| map[r]).collect();
        for &copy in &copies {
            if let Some(t) = ctx.world.get_mut::<crate::d2::Transform>(copy) {
                t.position += offset;
            }
            #[cfg(feature = "3d")]
            if let Some(&t) = ctx.world.get::<crate::d3::Transform>(copy) {
                let moved = crate::d3::Transform {
                    position: t.position + offset3.0,
                    ..t
                };
                place_world3d(&mut ctx.world, copy, moved);
            }
        }
        let _ = offset3;
        settle(&mut ctx.world);
        self.record(Undo::Spawned(map.values().copied().collect()));
        restore_terrain(ctx)?;
        Ok(copies)
    }

    /// Despawns `e`, with undo (which brings back its reflected components
    /// as a new entity: references to `e` from other components stay dead).
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn delete(&mut self, ctx: &mut Ctx, e: Entity) -> Result<(), crate::error::Error> {
        self.delete_many(ctx, &[e])
    }

    /// Despawns `roots` with their trees (prefab parts and what is attached
    /// to them), as one step. Undo brings them all back as new entities,
    /// still attached to each other.
    pub fn delete_many(&mut self, ctx: &mut Ctx, roots: &[Entity]) -> Result<(), crate::error::Error> {
        for &e in roots {
            if !ctx.world.is_alive(e) {
                return Err(crate::error::Error::new(
                    crate::error::ErrorKind::NotFound,
                    format!("no entity {}", e.to_bits()),
                ));
            }
        }
        let members = tree(&ctx.world, roots);
        let snapshot = snapshot(&ctx.world, &members)?;
        for &e in &members {
            ctx.world.despawn(e);
        }
        if snapshot
            .iter()
            .any(|(_, c)| c.as_object().is_some_and(|c| !c.is_empty()))
        {
            self.record(Undo::Despawned(snapshot));
        }
        Ok(())
    }

    /// Makes `child` follow `parent` (`None`: follow nothing), without
    /// moving it: its attachment's offset is where it is now, in the
    /// parent's space. 3D entities get an `Attached3D`, 2D ones an
    /// [`Attached`](crate::attach::Attached). With undo.
    ///
    /// # Errors
    ///
    /// `Invalid` when the link would make a loop, the two entities
    /// aren't both 2D or both 3D, or `child` already follows a skeleton or
    /// a skin; `NotFound` when either is gone.
    pub fn set_parent(
        &mut self,
        ctx: &mut Ctx,
        child: Entity,
        parent: Option<Entity>,
    ) -> Result<(), crate::error::Error> {
        use crate::error::{Error, ErrorKind};
        let invalid = |m: String| Err(Error::new(ErrorKind::Invalid, m));
        for e in std::iter::once(child).chain(parent) {
            if !ctx.world.is_alive(e) {
                return Err(Error::new(ErrorKind::NotFound, format!("no entity {}", e.to_bits())));
            }
        }
        let world = &ctx.world;
        let is3d = has3d(world, child);
        let name = if is3d { "Attached3D" } else { "Attached" };
        let before = world.get_json(child, name).ok();
        let Some(parent) = parent else {
            if before.is_some() {
                ctx.world.remove_json(child, name)?;
                self.record(Undo::Component {
                    e: child,
                    name: name.into(),
                    before,
                });
                settle(&mut ctx.world);
            }
            return Ok(());
        };
        if parent == child || ancestors(world, parent).contains(&child) {
            return invalid(format!(
                "entity {} is above entity {} already: that link would make a loop",
                child.to_bits(),
                parent.to_bits()
            ));
        }
        if let Some((_, link)) = parent_of(world, child, &HashMap::new())
            && matches!(link, Link::Joint | Link::Skin)
        {
            return invalid(format!(
                "entity {} follows a {}: remove that first",
                child.to_bits(),
                link.name()
            ));
        }
        let value = if is3d {
            link3d(world, child, parent)
        } else {
            link2d(world, child, parent)
        };
        let Some(value) = value else {
            return invalid(format!(
                "entities {} and {} need the same kind of transform (both 2D or both 3D) to be linked",
                child.to_bits(),
                parent.to_bits()
            ));
        };
        ctx.world.set_json(child, name, value)?;
        self.record(Undo::Component {
            e: child,
            name: name.into(),
            before,
        });
        settle(&mut ctx.world);
        Ok(())
    }

    /// [`set_parent`](Self::set_parent) for several children, as one undo
    /// step. Nothing changes if any of them can't be linked.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn set_parents(
        &mut self,
        ctx: &mut Ctx,
        children: &[Entity],
        parent: Option<Entity>,
    ) -> Result<(), crate::error::Error> {
        let mark = self.undo.len();
        for &child in children {
            if let Err(e) = self.set_parent(ctx, child, parent) {
                while self.undo.len() > mark {
                    self.undo(ctx)?;
                }
                self.redo.clear();
                return Err(e);
            }
        }
        self.group_since(mark);
        Ok(())
    }

    /// Spawns an empty entity attached to `parent`, on top of it (2D or 3D
    /// as `parent` is), as one undo step.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn spawn_child(&mut self, ctx: &mut Ctx, parent: Entity) -> Result<Entity, crate::error::Error> {
        if !ctx.world.is_alive(parent) {
            return Err(crate::error::Error::new(
                crate::error::ErrorKind::NotFound,
                format!("no entity {}", parent.to_bits()),
            ));
        }
        let mark = self.undo.len();
        #[cfg(feature = "3d")]
        if let Some(&t) = ctx.world.get::<crate::d3::Transform>(parent) {
            let e = ctx.world.spawn((crate::d3::Transform::at(t.position),));
            self.record(Undo::Spawned(vec![e]));
            self.set_parent(ctx, e, Some(parent))?;
            self.group_since(mark);
            return Ok(e);
        }
        let at = ctx
            .world
            .get::<crate::d2::Transform>(parent)
            .map_or(Vec2::ZERO, |t| t.position);
        let e = self.spawn_empty(ctx, at);
        if ctx.world.get::<crate::d2::Transform>(parent).is_some() {
            self.set_parent(ctx, e, Some(parent))?;
        }
        self.group_since(mark);
        Ok(e)
    }

    /// Makes the steps recorded since the history had `mark` steps one step.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    fn group_since(&mut self, mark: usize) {
        if self.undo.len() <= mark + 1 {
            return;
        }
        let steps: Vec<Step> = self.undo.drain(mark..).collect();
        let state = steps[0].state;
        // Undone last first: `apply` of a group runs its parts in reverse.
        let group = Undo::Group(steps.into_iter().map(|s| s.undo).collect());
        self.undo.push(Step { undo: group, state });
    }

    /// Moves, turns or scales 2D entities to these world transforms, as
    /// one step; attached ones keep their parent and get a new offset, and
    /// what is attached to them follows.
    pub fn set_transforms2d(&mut self, ctx: &mut Ctx, to: &[(Entity, crate::d2::Transform)]) {
        let entities: Vec<Entity> = to.iter().map(|&(e, _)| e).collect();
        let before = Before::of2d(&ctx.world, &entities);
        for &(e, t) in to {
            place_world2d(&mut ctx.world, e, t);
        }
        settle(&mut ctx.world);
        self.changed(&ctx.world, before);
    }

    /// Moves, turns or scales entities to these world transforms, as one
    /// step; attached ones keep their parent and get a new offset, and
    /// what is attached to them follows.
    #[cfg(feature = "3d")]
    pub fn set_transforms3d(&mut self, ctx: &mut Ctx, to: &[(Entity, crate::d3::Transform)]) {
        let entities: Vec<Entity> = to.iter().map(|&(e, _)| e).collect();
        let before = Before::of3d(&ctx.world, &entities);
        for &(e, t) in to {
            place_world3d(&mut ctx.world, e, t);
        }
        settle(&mut ctx.world);
        self.changed(&ctx.world, before);
    }

    /// Puts back the last change. `Ok(false)` if there's nothing to undo.
    pub fn undo(&mut self, ctx: &mut Ctx) -> Result<bool, crate::error::Error> {
        let Some(step) = self.undo.pop() else {
            return Ok(false);
        };
        self.open = None;
        let back = self.apply(ctx, step.undo)?;
        self.redo.push(Step {
            undo: back,
            state: self.state,
        });
        self.state = step.state;
        settle(&mut ctx.world);
        restore_terrain(ctx)?;
        Ok(true)
    }

    /// Makes the last undone change again. `Ok(false)` if there's nothing
    /// to redo; any new change empties what can be redone.
    pub fn redo(&mut self, ctx: &mut Ctx) -> Result<bool, crate::error::Error> {
        let Some(step) = self.redo.pop() else {
            return Ok(false);
        };
        self.open = None;
        let back = self.apply(ctx, step.undo)?;
        self.undo.push(Step {
            undo: back,
            state: self.state,
        });
        self.state = step.state;
        settle(&mut ctx.world);
        restore_terrain(ctx)?;
        Ok(true)
    }

    /// Applies `step` and returns the step that puts that back.
    fn apply(&mut self, ctx: &mut Ctx, step: Undo) -> Result<Undo, crate::error::Error> {
        Ok(match step {
            Undo::Component { e, name, before } => {
                let e = self.resolve(e);
                let now = ctx.world.get_json(e, &name).ok();
                let world = &mut ctx.world;
                Entity::with_remap(&self.remap, || match before {
                    Some(value) => world.set_json(e, &name, value),
                    None => world.remove_json(e, &name).map(drop),
                })?;
                Undo::Component { e, name, before: now }
            }
            Undo::Spawned(entities) => {
                let entities: Vec<Entity> = entities.iter().map(|&e| self.resolve(e)).collect();
                let members = tree(&ctx.world, &entities);
                let snapshot = snapshot(&ctx.world, &members)?;
                for &e in &members {
                    ctx.world.despawn(e);
                }
                Undo::Despawned(snapshot)
            }
            Undo::Despawned(snapshot) => {
                // What each stood for before (a glTF node's entity after
                // Stop spawned its scene again).
                #[cfg(feature = "3d")]
                let was: Vec<Entity> = snapshot.iter().map(|(old, _)| self.resolve(*old)).collect();
                let map = respawn(ctx, &snapshot, self.remap.clone())?;
                let mut back = Vec::with_capacity(snapshot.len());
                for (old, _) in &snapshot {
                    let new = map[old];
                    self.moved(*old, new);
                    back.push(new);
                }
                // glTF scene nodes are their scene's nodes again.
                #[cfg(feature = "3d")]
                {
                    let mut nodes: Vec<(Entity, Entity)> = Vec::with_capacity(2 * back.len());
                    for (((old, _), was), &new) in snapshot.iter().zip(was).zip(&back) {
                        nodes.push((*old, new));
                        nodes.push((was, new));
                    }
                    ctx.gltf_nodes_back(&nodes);
                }
                Undo::Spawned(back)
            }
            Undo::Resource { name, before } => {
                let now = ctx.world.resource_json(&name)?;
                ctx.world.set_resource_json(&name, before)?;
                Undo::Resource { name, before: now }
            }
            #[cfg(all(feature = "editor", feature = "3d"))]
            Undo::GltfLink { root, node, link } => {
                let root = self.resolve(root);
                let link = ctx.set_waiting_link(root, &node, link)?;
                Undo::GltfLink { root, node, link }
            }
            #[cfg(feature = "editor")]
            Undo::Asset { key, before } => {
                let now = self.docs.swap(&key, before);
                Undo::Asset { key, before: now }
            }
            Undo::Group(steps) => {
                let mut back = Vec::with_capacity(steps.len());
                for step in steps.into_iter().rev() {
                    back.push(self.apply(ctx, step)?);
                }
                Undo::Group(back)
            }
        })
    }
}

/// What the asset browser lists: files in `assets/` that can be dragged
/// into the scene, and the materials of the mesh files loaded so far.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AssetList {
    /// glTF files (`.glb`, `.gltf`).
    pub meshes: Vec<String>,
    /// Prefab files (`"format": "ponko-prefab"`).
    pub prefabs: Vec<String>,
    /// Scene files (`"format": "ponko-world"`).
    pub scenes: Vec<String>,
    /// (mesh file, number in it, name) of each material of a loaded mesh file.
    pub materials: Vec<(String, usize, String)>,
    /// Images (`.png`, `.jpg`, `.jpeg`).
    pub textures: Vec<String>,
    /// Sounds (`.wav`, `.ogg`, `.flac`, `.mp3`).
    pub sounds: Vec<String>,
    /// Shaders (`.wgsl`).
    pub shaders: Vec<String>,
}

/// Folders below `assets/` the browser looks into, at most.
#[cfg(not(target_arch = "wasm32"))]
const BROWSE_DEPTH: usize = 8;
/// Files the browser lists, at most.
const BROWSE_FILES: usize = 4096;

/// Lists the assets the editor can place (see [`AssetList`]), from every
/// `assets/` folder found. Web builds have no folders: they list the web
/// build's file manifest instead (see [`browse_names`]).
pub(crate) fn browse(ctx: &Ctx) -> AssetList {
    #[cfg(not(target_arch = "wasm32"))]
    let mut out = browse_folders();
    #[cfg(target_arch = "wasm32")]
    let mut out = {
        let names = crate::assets::web_manifest();
        browse_names(names.iter().map(String::as_str), crate::assets::read_web_asset)
    };
    #[cfg(feature = "3d")]
    if let Some(names) = ctx.world.get_resource::<crate::reflect::MaterialNames>() {
        out.materials = names.list();
    }
    let _ = ctx;
    out.sort();
    out
}

/// The files of every `assets/` folder found, by kind.
#[cfg(not(target_arch = "wasm32"))]
fn browse_folders() -> AssetList {
    let mut out = AssetList::default();
    for root in crate::assets::asset_roots() {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    if depth < BROWSE_DEPTH {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                let Ok(rel) = path.strip_prefix(&root) else { continue };
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.add(rel, || json_format(&path));
                if out.len() >= BROWSE_FILES {
                    break;
                }
            }
        }
    }
    out
}

/// Lists the assets named in a web build's file manifest, `names`: the
/// files `assets/index.json` lists (as `web/build.sh` and `ponko package
/// --web` write it), the page's asset pack, and the files the editor saved
/// in the browser. `read` gives a file's bytes, for the `"format"` of a
/// JSON file. Names that are folders' own (hidden files, `index.json`
/// itself) are left out.
// Natively only the web editor's tests use it.
#[cfg_attr(not(any(all(test, feature = "editor"), target_arch = "wasm32")), allow(dead_code))]
pub(crate) fn browse_names<'a>(
    names: impl IntoIterator<Item = &'a str>,
    read: impl Fn(&str) -> Option<Vec<u8>>,
) -> AssetList {
    let mut out = AssetList::default();
    for name in names {
        let hidden = name.split('/').any(|part| part.starts_with('.'));
        if hidden || name.is_empty() || name == "index.json" {
            continue;
        }
        out.add(name.to_string(), || read(name).and_then(|bytes| json_format_of(&bytes)));
        if out.len() >= BROWSE_FILES {
            break;
        }
    }
    out.sort();
    out
}

impl AssetList {
    /// Lists asset `rel` under its kind (by its extension), if the editor
    /// places it; `json_format` reads a JSON file's `"format"`.
    fn add(&mut self, rel: String, json_format: impl FnOnce() -> Option<String>) {
        let file = rel.rsplit('/').next().unwrap_or(&rel);
        let ext = file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("glb" | "gltf") => self.meshes.push(rel),
            Some("png" | "jpg" | "jpeg") => self.textures.push(rel),
            Some("wav" | "ogg" | "flac" | "mp3") => self.sounds.push(rel),
            Some("wgsl") => self.shaders.push(rel),
            Some("json") => match json_format().as_deref() {
                Some(crate::scene::PREFAB_FORMAT) => self.prefabs.push(rel),
                Some("ponko-world") => self.scenes.push(rel),
                _ => {}
            },
            _ => {}
        }
    }

    /// Files listed (materials aren't files).
    fn len(&self) -> usize {
        let files = [
            &self.meshes,
            &self.prefabs,
            &self.scenes,
            &self.textures,
            &self.sounds,
            &self.shaders,
        ];
        files.iter().map(|list| list.len()).sum()
    }

    /// Each kind's files sorted, each once.
    fn sort(&mut self) {
        for list in [
            &mut self.meshes,
            &mut self.prefabs,
            &mut self.scenes,
            &mut self.textures,
            &mut self.sounds,
            &mut self.shaders,
        ] {
            list.sort();
            list.dedup();
        }
    }
}

/// The `"format"` of a JSON file, read whole when it's small, else
/// looked for in its first bytes.
#[cfg(not(target_arch = "wasm32"))]
fn json_format(path: &std::path::Path) -> Option<String> {
    const WHOLE: u64 = 4 << 20;
    let len = std::fs::metadata(path).ok()?.len();
    if len <= WHOLE {
        return json_format_of(&std::fs::read(path).ok()?);
    }
    let mut head = vec![0u8; 64 << 10];
    let n = std::io::Read::read(&mut std::fs::File::open(path).ok()?, &mut head).ok()?;
    let head = String::from_utf8_lossy(&head[..n]);
    [crate::scene::PREFAB_FORMAT, "ponko-world"]
        .into_iter()
        .find(|f| head.contains(&format!("\"{f}\"")))
        .map(str::to_string)
}

/// The `"format"` of the JSON file `bytes`.
fn json_format_of(bytes: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    v["format"].as_str().map(str::to_string)
}

/// A 3D offset for [`Editor::duplicate_many`] (zero in builds without 3D).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Offset3(#[cfg(feature = "3d")] pub glam::Vec3);

impl Offset3 {
    #[cfg(feature = "3d")]
    pub const ZERO: Self = Self(glam::Vec3::ZERO);
    #[cfg(not(feature = "3d"))]
    pub const ZERO: Self = Self();
}

/// Components as they were before a direct change, for
/// [`Editor::changed`]: (entity, component, value or `None` if absent).
#[derive(Default, Debug)]
pub(crate) struct Before(Vec<(Entity, String, Option<Value>)>);

impl Before {
    /// The 2D transforms and attachments of `entities`: what a 2D gizmo drag changes.
    pub fn of2d(world: &ponko_ecs::World, entities: &[Entity]) -> Self {
        let mut out = Vec::with_capacity(entities.len() * 2);
        for &e in entities {
            for name in ["Transform2D", "Attached"] {
                out.push((e, name.to_string(), world.get_json(e, name).ok()));
            }
        }
        Self(out)
    }

    /// The 3D transforms and attachments of `entities`: what a gizmo drag changes.
    #[cfg(feature = "3d")]
    pub fn of3d(world: &ponko_ecs::World, entities: &[Entity]) -> Self {
        let mut out = Vec::with_capacity(entities.len() * 2);
        for &e in entities {
            for name in ["Transform3D", "Attached3D"] {
                out.push((e, name.to_string(), world.get_json(e, name).ok()));
            }
        }
        Self(out)
    }
}

/// Gives entities brought back without their terrain tile (a deleted one
/// undone, a copy) the tile of their terrain file.
fn restore_terrain(ctx: &mut Ctx) -> Result<(), crate::error::Error> {
    #[cfg(feature = "3d")]
    crate::three::restore_terrain_tiles(ctx, Default::default())?;
    let _ = ctx;
    Ok(())
}

/// Places every attachment now (the simulation may be paused while
/// editing), 2D and 3D, and brings the children lists up to date; broken
/// links are left for `follow_attachments` to despawn.
pub(crate) fn settle(world: &mut ponko_ecs::World) {
    crate::hierarchy::settle(world);
}

/// Gives `e` the world transform `t`. If it's attached, its offset changes
/// instead, so the attachment puts it there.
#[cfg(feature = "3d")]
pub(crate) fn place_world3d(world: &mut ponko_ecs::World, e: Entity, t: crate::d3::Transform) {
    use crate::attach::Attached3D;
    if let Some(&a) = world.get::<Attached3D>(e)
        && let Some(parent) = crate::attach::place3d(world, Attached3D::to(a.to))
    {
        let offset = crate::attach::relative3d(&parent, &t);
        if let Some(link) = world.get_mut::<Attached3D>(e) {
            link.offset = offset;
        }
    }
    if let Some(at) = world.get_mut::<crate::d3::Transform>(e) {
        *at = t;
    }
}

fn has3d(world: &ponko_ecs::World, e: Entity) -> bool {
    #[cfg(feature = "3d")]
    if world.get::<crate::d3::Transform>(e).is_some() {
        return true;
    }
    let _ = (world, e);
    false
}

/// `child`'s `Attached3D` to `parent`, where it is now.
#[allow(unused_variables)]
fn link3d(world: &ponko_ecs::World, child: Entity, parent: Entity) -> Option<Value> {
    #[cfg(feature = "3d")]
    {
        let c = *world.get::<crate::d3::Transform>(child)?;
        // The parent's own place, as its attachments put it.
        let p = match world.get::<crate::attach::Attached3D>(parent) {
            Some(&a) => crate::attach::place3d(world, a)?,
            None => *world.get::<crate::d3::Transform>(parent)?,
        };
        let link = crate::attach::Attached3D::to(parent).offset(crate::attach::relative3d(&p, &c));
        return serde_json::to_value(link).ok();
    }
    #[allow(unreachable_code)]
    None
}

/// `child`'s 2D [`Attached`](crate::attach::Attached) to `parent`, where it is now.
fn link2d(world: &ponko_ecs::World, child: Entity, parent: Entity) -> Option<Value> {
    serde_json::to_value(attached2d(world, child, parent)?).ok()
}

/// The 2D attachment to `parent` that puts `child` where it is now.
fn attached2d(world: &ponko_ecs::World, child: Entity, parent: Entity) -> Option<crate::attach::Attached> {
    use crate::d2::Transform as T2;
    let c = world.get::<T2>(child)?;
    let p = world.get::<T2>(parent)?;
    let divide = |a: f32, b: f32| if b == 0.0 { a } else { a / b };
    let local = Vec2::from_angle(-p.rotation).rotate(c.position - p.position);
    let offset = Vec2::new(divide(local.x, p.scale.x), divide(local.y, p.scale.y));
    Some(
        crate::attach::Attached::to(parent)
            .offset(offset)
            .rotated(c.rotation - p.rotation),
    )
}

/// Gives `e` the 2D world transform `t`. If it's attached, its offset
/// changes too, so the attachment keeps it there.
pub(crate) fn place_world2d(world: &mut ponko_ecs::World, e: Entity, t: crate::d2::Transform) {
    use crate::attach::Attached;
    if let Some(at) = world.get_mut::<crate::d2::Transform>(e) {
        *at = t;
    }
    if let Some(&a) = world.get::<Attached>(e)
        && let Some(link) = attached2d(world, e, a.to)
        && let Some(at) = world.get_mut::<Attached>(e)
    {
        at.offset = link.offset;
        at.rotation = link.rotation;
    }
}

/// How an entity hangs off its parent in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Link {
    /// [`Attached`](crate::attach::Attached).
    Attached,
    /// `Attached3D`.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    Attached3D,
    /// `AttachToJoint`: on a joint of the parent's skeleton.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    Joint,
    /// `SkinOf`: posed by the parent's skeleton.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    Skin,
    /// One of the parent's `PrefabParts`, not attached to anything.
    PrefabPart,
}

impl Link {
    pub fn name(self) -> &'static str {
        match self {
            Link::Attached => "attached",
            Link::Attached3D => "attached3d",
            Link::Joint => "joint",
            Link::Skin => "skin",
            Link::PrefabPart => "prefab_part",
        }
    }
}

/// The entity `e` hangs off, and how. `owners` maps prefab parts to their
/// prefab's root (see [`part_owners`]).
fn parent_of(world: &ponko_ecs::World, e: Entity, owners: &HashMap<Entity, Entity>) -> Option<(Entity, Link)> {
    if let Some(a) = world.get::<crate::attach::Attached>(e) {
        return Some((a.to, Link::Attached));
    }
    #[cfg(feature = "3d")]
    {
        if let Some(a) = world.get::<crate::attach::Attached3D>(e) {
            return Some((a.to, Link::Attached3D));
        }
        if let Some(a) = world.get::<crate::anim::AttachToJoint>(e) {
            return Some((a.skeleton, Link::Joint));
        }
        if let Some(s) = world.get::<crate::anim::SkinOf>(e) {
            return Some((s.0, Link::Skin));
        }
    }
    owners.get(&e).map(|&root| (root, Link::PrefabPart))
}

/// Each prefab part, to the root whose `PrefabParts` lists it.
fn part_owners(world: &ponko_ecs::World) -> HashMap<Entity, Entity> {
    let mut owners = HashMap::new();
    for a in world.raw_ref().archetypes() {
        for &root in a.entities() {
            if let Some(parts) = world.get::<crate::scene::PrefabParts>(root) {
                for &part in &parts.0 {
                    owners.entry(part).or_insert(root);
                }
            }
        }
    }
    owners
}

/// The entities above `e`, nearest first (stops at a loop).
pub(crate) fn ancestors(world: &ponko_ecs::World, e: Entity) -> Vec<Entity> {
    let owners = part_owners(world);
    let mut out = Vec::new();
    let mut at = e;
    while let Some((up, _)) = parent_of(world, at, &owners) {
        if up == e || out.contains(&up) || !world.is_alive(up) {
            break;
        }
        out.push(up);
        at = up;
    }
    out
}

/// One entity of the scene's hierarchy.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub entity: Entity,
    /// The entity it hangs off and how; `None` for a root.
    pub parent: Option<(Entity, Link)>,
    /// It's a prefab instance's root (it has `PrefabParts`).
    pub prefab: bool,
    /// The entities that hang off it: attached ones in the parent's
    /// children order, then the others in id order.
    pub children: Vec<Entity>,
}

/// Every entity as a tree: roots (in id order) each followed by what hangs
/// off it, depth first. A link to a dead entity, or a loop, makes a root.
pub(crate) fn hierarchy(world: &ponko_ecs::World) -> Vec<Node> {
    let owners = part_owners(world);
    let mut all: Vec<Entity> = world
        .raw_ref()
        .archetypes()
        .iter()
        .flat_map(|a| a.entities().iter().copied())
        .collect();
    all.sort_unstable();
    let mut nodes: HashMap<Entity, Node> = HashMap::with_capacity(all.len());
    for &e in &all {
        let parent = parent_of(world, e, &owners).filter(|(p, _)| *p != e && world.is_alive(*p));
        nodes.insert(
            e,
            Node {
                entity: e,
                parent,
                prefab: world.get::<crate::scene::PrefabParts>(e).is_some(),
                children: Vec::new(),
            },
        );
    }
    for &e in &all {
        if let Some((p, _)) = nodes[&e].parent
            && let Some(parent) = nodes.get_mut(&p)
        {
            parent.children.push(e);
        }
    }
    // Attached children in their parent's order (its `Children`), the
    // rest after them in id order.
    for node in nodes.values_mut() {
        if node.children.len() > 1 && world.get::<crate::hierarchy::Children>(node.entity).is_some() {
            use crate::hierarchy::WorldHierarchy;
            let order: HashMap<Entity, usize> = world.children(node.entity).enumerate().map(|(i, c)| (c, i)).collect();
            node.children
                .sort_by_key(|c| order.get(c).copied().unwrap_or(usize::MAX));
        }
    }
    // Depth first from the roots; entities in a loop have no root above
    // them, so they're added as roots after.
    let mut out = Vec::with_capacity(all.len());
    let mut seen = HashSet::with_capacity(all.len());
    let visit = |root: Entity, nodes: &HashMap<Entity, Node>, out: &mut Vec<Node>, seen: &mut HashSet<Entity>| {
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if !seen.insert(e) {
                continue;
            }
            let node = nodes[&e].clone();
            stack.extend(node.children.iter().rev().copied());
            out.push(node);
        }
    };
    for &e in &all {
        if nodes[&e].parent.is_none() {
            visit(e, &nodes, &mut out, &mut seen);
        }
    }
    for &e in &all {
        if !seen.contains(&e) {
            if let Some(n) = nodes.get_mut(&e) {
                n.parent = None;
            }
            visit(e, &nodes, &mut out, &mut seen);
        }
    }
    out
}

/// `roots` and everything that hangs off them (prefab parts, attachments,
/// skins, joint attachments, all the way down), roots first.
pub(crate) fn tree(world: &ponko_ecs::World, roots: &[Entity]) -> Vec<Entity> {
    let owners = part_owners(world);
    let mut children: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for a in world.raw_ref().archetypes() {
        for &e in a.entities() {
            if let Some((p, _)) = parent_of(world, e, &owners) {
                children.entry(p).or_default().push(e);
            }
        }
    }
    let mut out: Vec<Entity> = Vec::new();
    let mut seen = HashSet::new();
    for &r in roots {
        if world.is_alive(r) && seen.insert(r) {
            out.push(r);
        }
    }
    let mut at = 0;
    while at < out.len() {
        if let Some(kids) = children.get(&out[at]) {
            for &k in kids {
                if seen.insert(k) {
                    out.push(k);
                }
            }
        }
        at += 1;
    }
    out
}

/// The reflected components of `entities`.
fn snapshot(world: &ponko_ecs::World, entities: &[Entity]) -> Result<Vec<(Entity, Value)>, crate::error::Error> {
    entities.iter().map(|&e| Ok((e, world.entity_json(e)?))).collect()
}

/// Spawns `snapshot`'s entities as new ones and returns old id to new.
/// Components that name an entity of the snapshot name its new one, and
/// those in `remap` (entities brought back before) their new one too.
fn respawn(
    ctx: &mut Ctx,
    snapshot: &[(Entity, Value)],
    mut remap: HashMap<Entity, Entity>,
) -> Result<HashMap<Entity, Entity>, crate::error::Error> {
    for (_, components) in snapshot {
        ctx.load_scene_assets(components)?;
    }
    let mut fresh = Vec::with_capacity(snapshot.len());
    for (old, _) in snapshot {
        let e = ctx.world.spawn((Restoring,));
        remap.insert(*old, e);
        fresh.push(e);
    }
    let world = &mut ctx.world;
    let set = Entity::with_remap(&remap, || {
        for ((_, components), &e) in snapshot.iter().zip(&fresh) {
            if let Value::Object(components) = components {
                for (name, value) in components {
                    world.set_json(e, name, value.clone())?;
                }
            }
            world.remove::<Restoring>(e);
        }
        Ok::<_, crate::reflect::ReflectError>(())
    });
    if let Err(e) = set {
        for e in fresh {
            ctx.world.despawn(e);
        }
        return Err(e.into());
    }
    Ok(snapshot.iter().map(|(old, _)| (*old, remap[old])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::Transform as Transform2D;
    use glam::vec2;
    use ponko_ecs::Component;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    #[serde(default)]
    struct Hp(f32);

    fn ctx() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, vec2(800.0, 600.0));
        ctx.world.reflect::<Hp>();
        ctx
    }
    fn at(ctx: &Ctx, e: Entity) -> Vec2 {
        ctx.world.get::<Transform2D>(e).unwrap().position
    }

    #[test]
    fn every_edit_is_undone_in_reverse_order() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let a = ed.spawn_empty(&mut ctx, vec2(1.0, 1.0));
        ed.set(&mut ctx, a, "Hp", json!(5.0)).unwrap();
        let b = ed.duplicate(&mut ctx, a, vec2(10.0, 0.0)).unwrap();
        assert_eq!((at(&ctx, b), ctx.world.get::<Hp>(b)), (vec2(11.0, 1.0), Some(&Hp(5.0))));
        ed.remove(&mut ctx, a, "Hp").unwrap();
        ed.delete(&mut ctx, b).unwrap();
        assert_eq!(ctx.world.len(), 1);

        assert!(ed.undo(&mut ctx).unwrap()); // delete: back as a new entity
        assert!(!ctx.world.is_alive(b), "old handles stay dead");
        let back = ctx
            .world
            .query::<(Entity, &Hp)>()
            .iter()
            .map(|(e, _)| e)
            .next()
            .unwrap();
        assert_eq!(at(&ctx, back), vec2(11.0, 1.0));
        assert!(ed.undo(&mut ctx).unwrap()); // remove
        assert_eq!(ctx.world.get::<Hp>(a), Some(&Hp(5.0)));
        assert!(ed.undo(&mut ctx).unwrap()); // duplicate: despawns the one brought back
        assert!(!ctx.world.is_alive(back));
        assert!(ed.undo(&mut ctx).unwrap()); // set (added)
        assert_eq!(ctx.world.get::<Hp>(a), None);
        assert!(ed.undo(&mut ctx).unwrap()); // spawn
        assert_eq!(ctx.world.len(), 0);
        assert!(!ed.undo(&mut ctx).unwrap());
    }

    #[test]
    fn undoing_a_placed_prefab_removes_its_parts() {
        let path = format!("ponko-test-editor-tree-{}.json", std::process::id());
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(&path);
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let root = ctx.world.spawn((Transform2D::default(), Hp(1.0)));
        let part = ctx
            .world
            .spawn((Transform2D::default(), crate::attach::Attached::to(root)));
        let saved = ctx.assets.save_prefab(&ctx.world, root, &path);
        ctx.world.despawn(root);
        ctx.world.despawn(part);
        let placed = saved.and_then(|()| ed.spawn_prefab(&mut ctx, &path, vec2(5.0, 5.0)));
        let _ = std::fs::remove_file(&file);
        placed.unwrap();
        assert_eq!(ctx.world.len(), 2);
        assert!(ed.undo(&mut ctx).unwrap());
        assert_eq!(ctx.world.len(), 0, "the part went with the root");
    }

    #[test]
    fn a_drag_undoes_in_one_step() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let e = ctx.world.spawn((Transform2D::at(vec2(1.0, 2.0)),));
        let before = Before::of2d(&ctx.world, &[e]);
        for x in 0..10 {
            ctx.world.get_mut::<Transform2D>(e).unwrap().position.x = x as f32; // the drag
        }
        ed.changed(&ctx.world, before);
        ed.undo(&mut ctx).unwrap();
        assert_eq!(at(&ctx, e), vec2(1.0, 2.0));
    }

    #[test]
    fn stop_puts_the_world_back_as_it_was_at_play() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let e = ed.spawn_empty(&mut ctx, vec2(1.0, 2.0));
        ed.play(&mut ctx);
        assert!(ed.playing() && !ctx.time.is_paused());
        // The game runs: things move, spawn and die; the editor edits too.
        ctx.world.get_mut::<Transform2D>(e).unwrap().position = vec2(50.0, 50.0);
        ctx.world.spawn((Transform2D::default(),));
        ed.set(&mut ctx, e, "Hp", json!(1.0)).unwrap();
        ctx.time.pause();
        ed.play(&mut ctx); // resume: still from the first Play
        ed.stop(&mut ctx).unwrap();
        assert!(ctx.time.is_paused() && !ed.playing());
        assert_eq!(ctx.world.len(), 1);
        assert_eq!((at(&ctx, e), ctx.world.get::<Hp>(e)), (vec2(1.0, 2.0), None));
        // Undo steps from before Play still apply; those from playing are gone.
        assert!(ed.undo(&mut ctx).unwrap());
        assert_eq!(ctx.world.len(), 0);
        assert!(!ed.can_undo());
    }

    #[test]
    fn keep_makes_stop_do_nothing() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        ed.play(&mut ctx);
        ctx.world.spawn((Transform2D::default(),));
        ed.keep();
        ed.stop(&mut ctx).unwrap();
        assert_eq!(ctx.world.len(), 1);
    }

    #[test]
    fn a_failed_edit_changes_nothing_and_is_not_recorded() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let e = ctx.world.spawn((Transform2D::default(),));
        assert!(ed.set(&mut ctx, e, "Hp", json!("not a number")).is_err());
        assert!(ed.add(&mut ctx, e, "NoSuchThing").is_err());
        assert!(!ed.can_undo());
    }

    #[test]
    fn a_refused_stop_keeps_the_play_snapshot_and_undo_for_a_retry() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let a = ed.spawn_empty(&mut ctx, vec2(1.0, 1.0));
        ed.set(&mut ctx, a, "Hp", json!(5.0)).unwrap();
        ed.play(&mut ctx);
        ed.set(&mut ctx, a, "Hp", json!(9.0)).unwrap(); // while playing
        ctx.world.spawn((Hp(1.0),));
        // A lockstep session: Stop would put the world back on this peer only.
        let session = std::mem::replace(&mut ctx.net.lockstep, crate::net::Net::loopback(2).remove(0));
        drop(session);
        let err = ed.stop(&mut ctx).unwrap_err();
        assert!(err.message().contains("lockstep"), "{err}");
        assert!(ed.playing(), "the Play snapshot is kept");
        assert!(!ctx.time.is_paused(), "a refused Stop doesn't pause the simulation");
        assert_eq!(ed.undo.len(), 3, "and the undo history");
        assert_eq!(ctx.world.len(), 2, "the world is unchanged");
        assert_eq!(ctx.world.get::<Hp>(a), Some(&Hp(9.0)));
        // The session ended: Stop works now.
        ctx.net.lockstep = crate::net::Net::default();
        ed.stop(&mut ctx).unwrap();
        assert!(!ed.playing() && ctx.time.is_paused());
        assert_eq!(ed.undo.len(), 2, "edits made while playing are dropped");
        assert_eq!((ctx.world.len(), ctx.world.get::<Hp>(a)), (1, Some(&Hp(5.0))));
    }

    #[test]
    fn redo_makes_undone_edits_again_and_a_new_edit_drops_it() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let a = ed.spawn_empty(&mut ctx, vec2(1.0, 1.0));
        ed.set(&mut ctx, a, "Hp", json!(5.0)).unwrap();
        ed.delete(&mut ctx, a).unwrap();
        assert_eq!(ctx.world.len(), 0);
        assert!(ed.undo(&mut ctx).unwrap() && ed.undo(&mut ctx).unwrap());
        let back = ed.resolve(a);
        assert_eq!((ctx.world.get::<Hp>(back), ed.can_redo()), (None, true));
        assert!(ed.redo(&mut ctx).unwrap()); // set
        assert_eq!(ctx.world.get::<Hp>(back), Some(&Hp(5.0)));
        assert!(ed.redo(&mut ctx).unwrap()); // delete
        assert_eq!(ctx.world.len(), 0);
        assert!(!ed.redo(&mut ctx).unwrap());
        assert!(ed.undo(&mut ctx).unwrap() && ed.undo(&mut ctx).unwrap());
        ed.spawn_empty(&mut ctx, Vec2::ZERO);
        assert!(!ed.can_redo(), "a new edit drops what was undone");
    }

    #[test]
    fn undo_and_redo_of_a_spawn_and_delete_cycle_keep_finding_the_entity() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let a = ed.spawn_empty(&mut ctx, vec2(3.0, 4.0));
        ed.set(&mut ctx, a, "Hp", json!(2.0)).unwrap();
        for _ in 0..3 {
            ed.undo(&mut ctx).unwrap(); // Hp
            ed.undo(&mut ctx).unwrap(); // spawn: despawned
            assert_eq!(ctx.world.len(), 0);
            ed.redo(&mut ctx).unwrap(); // spawned again, a new entity
            ed.redo(&mut ctx).unwrap(); // Hp on the new one
            let now = ed.resolve(a);
            assert!(ctx.world.is_alive(now));
            assert_eq!(
                (at(&ctx, now), ctx.world.get::<Hp>(now)),
                (vec2(3.0, 4.0), Some(&Hp(2.0)))
            );
        }
    }

    #[test]
    fn deleting_a_parent_takes_its_children_and_undo_brings_them_back_attached() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D::at(vec2(10.0, 0.0)),));
        let child = ctx
            .world
            .spawn((Transform2D::default(), crate::attach::Attached::to(parent), Hp(1.0)));
        let other = ctx.world.spawn((Transform2D::default(),));
        ed.delete(&mut ctx, parent).unwrap();
        assert!(!ctx.world.is_alive(child), "the tree goes as one");
        assert!(ctx.world.is_alive(other));
        ed.undo(&mut ctx).unwrap();
        let (p, c) = (ed.resolve(parent), ed.resolve(child));
        assert!(ctx.world.is_alive(p) && ctx.world.is_alive(c) && p != parent);
        assert_eq!(
            ctx.world.get::<crate::attach::Attached>(c).unwrap().to,
            p,
            "still attached"
        );
        assert_eq!(ctx.world.get::<Hp>(c), Some(&Hp(1.0)));
    }

    #[test]
    fn parenting_keeps_the_child_where_it_is_and_refuses_loops() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D {
            position: vec2(10.0, 0.0),
            rotation: std::f32::consts::FRAC_PI_2,
            scale: vec2(2.0, 2.0),
        },));
        let child = ctx.world.spawn((Transform2D::at(vec2(10.0, 4.0)),));
        ed.set_parent(&mut ctx, child, Some(parent)).unwrap();
        let link = *ctx.world.get::<crate::attach::Attached>(child).unwrap();
        let (p, r) = crate::attach::tests_place(&ctx.world, link).unwrap();
        assert!((p - vec2(10.0, 4.0)).length() < 1e-4, "{p}: it didn't move");
        assert!((r - 0.0).abs() < 1e-5);
        let err = ed.set_parent(&mut ctx, parent, Some(child)).unwrap_err();
        assert!(err.message().contains("loop"), "{err}");
        ed.set_parent(&mut ctx, child, None).unwrap();
        assert!(ctx.world.get::<crate::attach::Attached>(child).is_none());
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ctx.world.get::<crate::attach::Attached>(child), Some(&link));
        ed.undo(&mut ctx).unwrap();
        assert!(ctx.world.get::<crate::attach::Attached>(child).is_none());
    }

    #[test]
    fn the_hierarchy_lists_roots_then_what_hangs_off_them() {
        let mut ctx = ctx();
        let root = ctx.world.spawn((Transform2D::default(),));
        let part = ctx.world.spawn((Transform2D::default(),));
        ctx.world.insert(root, crate::scene::PrefabParts(vec![part]));
        let attached = ctx
            .world
            .spawn((Transform2D::default(), crate::attach::Attached::to(part)));
        let loose = ctx.world.spawn((Transform2D::default(),));
        let nodes = hierarchy(&ctx.world);
        let order: Vec<Entity> = nodes.iter().map(|n| n.entity).collect();
        assert_eq!(order, vec![root, part, attached, loose]);
        assert!(nodes[0].prefab && nodes[0].parent.is_none());
        assert_eq!(nodes[1].parent, Some((root, Link::PrefabPart)));
        assert_eq!(nodes[2].parent, Some((part, Link::Attached)));
        assert_eq!(nodes[0].children, vec![part]);
        assert_eq!(tree(&ctx.world, &[root]), vec![root, part, attached]);
    }

    #[test]
    fn the_hierarchy_shows_children_in_the_parents_order() {
        use crate::hierarchy::WorldHierarchy;
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D::default(),));
        let a = ctx.world.spawn((Transform2D::default(),));
        let b = ctx.world.spawn((Transform2D::default(),));
        ed.set_parent(&mut ctx, a, Some(parent)).unwrap();
        ed.set_parent(&mut ctx, b, Some(parent)).unwrap();
        assert_eq!(ctx.world.children(parent).collect::<Vec<_>>(), [a, b], "listed at once");
        ctx.world.set_child_index(b, 0);
        let nodes = hierarchy(&ctx.world);
        assert_eq!(nodes[0].children, vec![b, a]);
        let order: Vec<Entity> = nodes.iter().map(|n| n.entity).collect();
        assert_eq!(order, vec![parent, b, a]);
    }

    #[test]
    fn duplicating_a_tree_copies_its_links_to_the_copies() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D::default(),));
        let child = ctx
            .world
            .spawn((Transform2D::default(), crate::attach::Attached::to(parent)));
        let copy = ed.duplicate(&mut ctx, parent, vec2(5.0, 0.0)).unwrap();
        assert_eq!(ctx.world.len(), 4);
        let copied_child = hierarchy(&ctx.world)
            .into_iter()
            .find(|n| n.parent.map(|p| p.0) == Some(copy))
            .unwrap()
            .entity;
        assert_ne!(copied_child, child);
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ctx.world.len(), 2, "undo removes the copied tree");
    }

    #[test]
    fn a_scene_load_drops_the_history_but_stop_keeps_it() {
        let mut ctx = ctx();
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, Vec2::ZERO));
        ctx.edit(|ed, ctx| ed.play(ctx));
        ctx.edit(|ed, ctx| ed.stop(ctx)).unwrap();
        assert!(ctx.editor.can_undo(), "Stop is the editor's own load");
        let save = ctx.world.save();
        ctx.replace_world(&save).unwrap();
        assert!(
            !ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap(),
            "the steps named the old world"
        );
    }

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug)]
    #[serde(default)]
    struct Rules {
        speed: f32,
    }

    #[test]
    fn a_dragged_number_is_one_undo_step_until_the_button_goes_up() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let e = ctx.world.spawn((Hp(1.0),));
        for v in [2.0, 3.0, 4.0] {
            ed.set_merged(&mut ctx, e, "Hp", json!(v)).unwrap();
        }
        ed.end_merge();
        ed.set_merged(&mut ctx, e, "Hp", json!(9.0)).unwrap();
        assert_eq!(ctx.world.get::<Hp>(e), Some(&Hp(9.0)));
        ed.undo(&mut ctx).unwrap();
        assert_eq!(
            ctx.world.get::<Hp>(e),
            Some(&Hp(4.0)),
            "the second drag is its own step"
        );
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ctx.world.get::<Hp>(e), Some(&Hp(1.0)), "the first drag undoes whole");
        assert!(!ed.can_undo());
        ed.redo(&mut ctx).unwrap();
        assert_eq!(ctx.world.get::<Hp>(e), Some(&Hp(4.0)), "and redoes whole");
    }

    #[test]
    fn another_change_ends_a_merge() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let a = ctx.world.spawn((Hp(1.0),));
        let b = ctx.world.spawn((Hp(1.0),));
        ed.set_merged(&mut ctx, a, "Hp", json!(2.0)).unwrap();
        ed.set_merged(&mut ctx, b, "Hp", json!(5.0)).unwrap();
        ed.set_merged(&mut ctx, a, "Hp", json!(3.0)).unwrap();
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ctx.world.get::<Hp>(a), Some(&Hp(2.0)));
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ctx.world.get::<Hp>(b), Some(&Hp(1.0)));
    }

    #[test]
    fn the_document_is_dirty_after_a_change_and_clean_when_it_is_undone() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        assert!(!ed.dirty(), "a new editor edits the world as it is");
        let e = ed.spawn_empty(&mut ctx, Vec2::ZERO);
        assert!(ed.dirty());
        ed.undo(&mut ctx).unwrap();
        assert!(!ed.dirty(), "back where the document was");
        ed.redo(&mut ctx).unwrap();
        assert!(ed.dirty());
        let _ = e;
        // A merged drag dirties too, even past its first change.
        ed.undo(&mut ctx).unwrap();
        let hp = ctx.world.spawn((Hp(1.0),));
        ed.set_merged(&mut ctx, hp, "Hp", json!(2.0)).unwrap();
        ed.set_merged(&mut ctx, hp, "Hp", json!(1.0)).unwrap();
        assert!(ed.dirty(), "the world went through a change");
        ed.undo(&mut ctx).unwrap();
        assert!(!ed.dirty());
    }

    #[test]
    fn stop_returns_to_the_documents_state_and_keep_dirties_it() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        ed.play(&mut ctx);
        ctx.world.spawn((Transform2D::default(),)); // the game plays
        ed.stop(&mut ctx).unwrap();
        assert!(!ed.dirty(), "Stop put the saved world back");
        ed.play(&mut ctx);
        ctx.world.spawn((Transform2D::default(),));
        ed.keep();
        assert!(ed.dirty(), "the world is what playing made of it");
    }

    #[test]
    fn resource_edits_undo_and_redo() {
        let mut ctx = ctx();
        ctx.world.reflect_resource::<Rules>();
        ctx.world.insert_resource(Rules { speed: 1.0 });
        let mut ed = Editor::default();
        ed.set_resource_merged(&mut ctx, "Rules", json!({ "speed": 2.0 }))
            .unwrap();
        ed.set_resource_merged(&mut ctx, "Rules", json!({ "speed": 3.0 }))
            .unwrap();
        assert!(ed.dirty());
        ed.undo(&mut ctx).unwrap();
        assert_eq!(ctx.world.resource::<Rules>().speed, 1.0);
        ed.redo(&mut ctx).unwrap();
        assert_eq!(ctx.world.resource::<Rules>().speed, 3.0);
        assert!(ed.set_resource(&mut ctx, "Missing", json!(1)).is_err());
    }

    #[test]
    fn renaming_names_an_entity_and_undo_takes_the_name_back() {
        use crate::hierarchy::EntityName;
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let e = ctx.world.spawn((Transform2D::default(),));
        ed.rename(&mut ctx, e, "  Gate  ").unwrap();
        assert_eq!(ctx.world.get::<EntityName>(e), Some(&EntityName::new("Gate")));
        ed.rename(&mut ctx, e, "Gate").unwrap();
        assert_eq!(ed.undo.len(), 1, "the same name again changes nothing");
        ed.rename(&mut ctx, e, "").unwrap();
        assert!(ctx.world.get::<EntityName>(e).is_none(), "an empty name takes it away");
        ed.undo(&mut ctx).unwrap();
        ed.undo(&mut ctx).unwrap();
        assert!(ctx.world.get::<EntityName>(e).is_none());
        ed.rename(&mut ctx, e, "Gate").unwrap();
        let save = ctx.world.save();
        assert!(
            save.to_string().contains("EntityName"),
            "names are saved with the scene"
        );
    }

    #[test]
    fn reparenting_several_is_one_step_and_a_refused_one_changes_nothing() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D::default(),));
        let a = ctx.world.spawn((Transform2D::at(vec2(5.0, 0.0)),));
        let b = ctx.world.spawn((Transform2D::at(vec2(0.0, 5.0)),));
        ed.set_parents(&mut ctx, &[a, b], Some(parent)).unwrap();
        assert_eq!(ancestors(&ctx.world, a), [parent]);
        assert_eq!(ancestors(&ctx.world, b), [parent]);
        assert_eq!(ed.undo.len(), 1);
        ed.undo(&mut ctx).unwrap();
        assert!(ancestors(&ctx.world, a).is_empty() && ancestors(&ctx.world, b).is_empty());
        // `parent` under `a` and then `a` under `parent` would loop.
        let refused = ed.set_parents(&mut ctx, &[parent, a], Some(a));
        assert!(refused.is_err());
        assert!(
            ancestors(&ctx.world, parent).is_empty(),
            "the first link was taken back"
        );
        assert!(!ed.can_redo());
        assert_eq!(ed.undo.len(), 0);
    }

    #[test]
    fn a_new_child_hangs_off_its_parent_and_undoes_in_one_step() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D::at(vec2(7.0, 3.0)),));
        let child = ed.spawn_child(&mut ctx, parent).unwrap();
        assert_eq!(ancestors(&ctx.world, child), [parent]);
        assert_eq!(at(&ctx, child), vec2(7.0, 3.0));
        ed.undo(&mut ctx).unwrap();
        assert!(!ctx.world.is_alive(child));
        assert_eq!(ctx.world.len(), 1);
        ed.redo(&mut ctx).unwrap();
        assert_eq!(ctx.world.len(), 2);
        let to = ctx
            .world
            .query::<&crate::attach::Attached>()
            .iter()
            .map(|a| a.to)
            .next();
        assert_eq!(to, Some(parent), "redone, still hanging off its parent");
    }

    #[test]
    fn moving_an_attached_2d_child_changes_its_offset_so_it_stays_there() {
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((Transform2D::at(vec2(10.0, 0.0)),));
        let child = ctx.world.spawn((Transform2D::at(vec2(12.0, 0.0)),));
        ed.set_parent(&mut ctx, child, Some(parent)).unwrap();
        let moved = Transform2D {
            rotation: 0.5,
            ..Transform2D::at(vec2(20.0, 5.0))
        };
        ed.set_transforms2d(&mut ctx, &[(child, moved)]);
        settle(&mut ctx.world);
        let t = *ctx.world.get::<Transform2D>(child).unwrap();
        assert!(t.position.distance(vec2(20.0, 5.0)) < 1e-4, "{:?}", t.position);
        assert!((t.rotation - 0.5).abs() < 1e-5);
        // The parent moves: the child keeps its new offset from it.
        ed.set_transforms2d(&mut ctx, &[(parent, Transform2D::at(vec2(0.0, 0.0)))]);
        assert!(at(&ctx, child).distance(vec2(10.0, 5.0)) < 1e-4);
        ed.undo(&mut ctx).unwrap();
        ed.undo(&mut ctx).unwrap();
        assert!(at(&ctx, child).distance(vec2(12.0, 0.0)) < 1e-4);
    }

    #[cfg(feature = "3d")]
    #[test]
    fn moving_a_3d_parent_carries_its_children_and_moving_a_child_changes_its_offset() {
        use crate::attach::Attached3D;
        use crate::d3::Transform as T3;
        use glam::vec3;
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let parent = ctx.world.spawn((T3::at(vec3(1.0, 0.0, 0.0)),));
        let child = ctx.world.spawn((T3::at(vec3(1.0, 2.0, 0.0)),));
        ed.set_parent(&mut ctx, child, Some(parent)).unwrap();
        let offset = ctx.world.get::<Attached3D>(child).unwrap().offset;
        assert!((offset.position - vec3(0.0, 2.0, 0.0)).length() < 1e-5);
        ed.set_transforms3d(&mut ctx, &[(parent, T3::at(vec3(5.0, 0.0, 0.0)))]);
        let c = ctx.world.get::<T3>(child).unwrap().position;
        assert!(
            (c - vec3(5.0, 2.0, 0.0)).length() < 1e-5,
            "{c}: the child follows at once"
        );
        ed.set_transforms3d(&mut ctx, &[(child, T3::at(vec3(5.0, 3.0, 1.0)))]);
        let offset = ctx.world.get::<Attached3D>(child).unwrap().offset;
        assert!(
            (offset.position - vec3(0.0, 3.0, 1.0)).length() < 1e-5,
            "the offset moved, not the link"
        );
        ed.undo(&mut ctx).unwrap();
        ed.undo(&mut ctx).unwrap();
        let c = ctx.world.get::<T3>(child).unwrap().position;
        assert!((c - vec3(1.0, 2.0, 0.0)).length() < 1e-5, "{c}: both moves undone");
        ed.redo(&mut ctx).unwrap();
        let c = ctx.world.get::<T3>(child).unwrap().position;
        assert!((c - vec3(5.0, 2.0, 0.0)).length() < 1e-5, "{c}: redone with the parent");
    }

    #[cfg(feature = "3d")]
    #[test]
    fn a_multi_entity_move_is_one_undo_step() {
        use crate::d3::Transform as T3;
        use glam::vec3;
        let mut ctx = ctx();
        let mut ed = Editor::default();
        let a = ctx.world.spawn((T3::default(),));
        let b = ctx.world.spawn((T3::at(vec3(2.0, 0.0, 0.0)),));
        ed.set_transforms3d(
            &mut ctx,
            &[(a, T3::at(vec3(0.0, 1.0, 0.0))), (b, T3::at(vec3(2.0, 1.0, 0.0)))],
        );
        assert!(ed.undo(&mut ctx).unwrap());
        assert_eq!(ctx.world.get::<T3>(a).unwrap().position, glam::Vec3::ZERO);
        assert_eq!(ctx.world.get::<T3>(b).unwrap().position, vec3(2.0, 0.0, 0.0));
        assert!(!ed.can_undo());
    }
}
