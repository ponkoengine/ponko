//! Overrides of a glTF scene: how its nodes differ from what the file
//! makes of them, kept in the scene's saved record ([`SavedScene`]) and put
//! back on every spawn and hot reload of the file, as a prefab instance
//! keeps its overrides when its prefab changes.
//!
//! A scene keeps its nodes' components as the file made them (its
//! baseline) while the world is edited ([`GltfEditing`]), or when its
//! record already has overrides. Recording compares each node with its
//! baseline: a changed component keeps the fields that changed, an added
//! one its value, a removed one its name. Links that every spawn makes
//! again aren't compared (the node's parent entity, its `GltfNode`), and
//! neither is an attached node's world transform, which follows its
//! offset. A prefab node keeps only where it stands: its instance is saved
//! with the world already.
//!
//! A node deleted while editing is recorded as deleted (spawns and reloads
//! leave it out, its children hang off its parent), and a node attached to
//! another parent records that parent: another node of the scene by path,
//! the scene's root, or an entity saved with the world.

use std::collections::BTreeMap;
use std::sync::Arc;

use ponko_ecs::Entity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{GltfRoot, GltfSceneSaves, Transform3D};
use crate::ctx::Ctx;
use crate::reflect::{JsonMap, WorldReflect};

/// Present while the world is edited (the editor's first change turns it
/// on): every glTF scene keeps its baseline, so its changes can be
/// recorded as overrides.
pub(crate) struct GltfEditing;

/// How one node differs from the file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct NodeOverrides {
    /// Changed components (the changed fields of an object) and added
    /// ones (the whole value), by name.
    #[serde(default, skip_serializing_if = "JsonMap::is_empty")]
    components: JsonMap,
    /// Components the file gives the node and it doesn't have.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    remove: Vec<String>,
    /// The node was deleted: spawns and reloads leave it out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) deleted: bool,
    /// The node hangs off another parent than the file's (its offset is
    /// then the whole `Attached3D` in `components`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<NodeParent>,
}

impl NodeOverrides {
    fn is_empty(&self) -> bool {
        self.components.is_empty() && self.remove.is_empty() && !self.deleted && self.parent.is_none()
    }

    fn deleted() -> Self {
        Self {
            deleted: true,
            ..Self::default()
        }
    }
}

/// The parent a node was attached to instead of the file's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NodeParent {
    /// The scene's root.
    Root,
    /// Another node of the scene, by path.
    Node(String),
    /// An entity saved with the world (its id is kept by world saves).
    Entity(Entity),
}

/// One way a node of a glTF scene differs from its file, as the editor
/// lists it.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GltfOverride {
    /// The node's path.
    pub node: String,
    /// Its entity (dead for a deleted node).
    pub entity: Entity,
    pub kind: GltfOverrideKind,
}

/// What a [`GltfOverride`] changes.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GltfOverrideKind {
    /// Component `name` differs from the file's in these fields (empty: a
    /// value that isn't an object).
    Changed { name: String, fields: Vec<String> },
    /// Component `name` was added.
    Added(String),
    /// Component `name` of the file was taken off.
    Removed(String),
    /// It hangs off another parent: the entity it is attached to now.
    Parent(Entity),
    /// It was deleted.
    Deleted,
}

impl GltfOverrideKind {
    /// The component it is about (`Attached3D` for a parent).
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn component(&self) -> Option<&str> {
        match self {
            Self::Changed { name, .. } | Self::Added(name) | Self::Removed(name) => Some(name),
            Self::Parent(_) => Some("Attached3D"),
            Self::Deleted => None,
        }
    }
}

/// Components every spawn makes again, never compared.
const NOT_COMPARED: [&str; 5] = ["NotSaved", "GltfNode", "Children", "PrefabParts", "PrefabInstance"];

impl Ctx {
    /// Turns on [`GltfEditing`]: the scenes spawned before keep their
    /// nodes as they are now as their baseline.
    pub(crate) fn track_gltf_overrides(&mut self) {
        if self.world.get_resource::<GltfEditing>().is_some() {
            return;
        }
        self.world.insert_resource(GltfEditing);
        let roots: Vec<Entity> = self
            .world
            .query::<(Entity, &GltfRoot)>()
            .iter()
            .filter(|(_, r)| r.baseline.is_none())
            .map(|(e, _)| e)
            .collect();
        for root in roots {
            let baseline = Arc::new(self.gltf_nodes_now(root));
            if let Some(r) = self.world.get_mut::<GltfRoot>(root) {
                r.baseline = Some(baseline);
            }
        }
    }

    /// The compared components of each live node of `root`'s scene, by
    /// node path.
    fn gltf_nodes_now(&self, root: Entity) -> BTreeMap<String, JsonMap> {
        let Some(scene) = self.world.get::<GltfRoot>(root) else {
            return BTreeMap::new();
        };
        let names = self.world.reflected_components();
        let mut out = BTreeMap::new();
        for node in &scene.nodes {
            if !self.world.is_alive(node.entity) {
                continue;
            }
            let map = self.node_components(node.entity, node.prefab.is_some(), &names);
            out.insert(node.path.clone(), map);
        }
        out
    }

    /// The compared components of node entity `e` (of a prefab node, only
    /// where it stands), from the reflected `names`.
    fn node_components(&self, e: Entity, prefab: bool, names: &[&str]) -> JsonMap {
        let wanted: &[&str] = if prefab { &["Transform3D"] } else { names };
        let mut map = JsonMap::new();
        for &name in wanted {
            if NOT_COMPARED.contains(&name) {
                continue;
            }
            if let Ok(v) = self.world.get_json(e, name) {
                map.insert(name.to_string(), v);
            }
        }
        map
    }

    /// Writes how the scenes of file `path` (of every file for `None`)
    /// differ from their baseline into their saved records, with where
    /// their roots stand now. Scenes without a baseline keep their record.
    pub(crate) fn record_gltf_overrides(&mut self, path: Option<&str>) {
        let roots: Vec<Entity> = self
            .world
            .query::<(Entity, &GltfRoot)>()
            .iter()
            .filter(|(_, r)| path.is_none_or(|p| r.path == p))
            .map(|(e, _)| e)
            .collect();
        for root in roots {
            self.record_scene_overrides(root);
        }
    }

    /// [`record_gltf_overrides`](Self::record_gltf_overrides) for the
    /// scene of `root`.
    pub(super) fn record_scene_overrides(&mut self, root: Entity) {
        let Some(overrides) = self.scene_overrides(root) else {
            return;
        };
        let at = self.world.get::<Transform3D>(root).copied();
        if let Some(saves) = self.world.get_resource_mut::<GltfSceneSaves>()
            && let Some(scene) = saves.scenes.iter_mut().find(|s| s.root == Some(root))
        {
            scene.overrides = overrides;
            if let Some(at) = at {
                scene.at = at;
            }
        }
    }

    /// How the nodes of `root`'s scene differ from its baseline, by node
    /// path; `None` without a baseline.
    fn scene_overrides(&self, root: Entity) -> Option<BTreeMap<String, NodeOverrides>> {
        let scene = self.world.get::<GltfRoot>(root)?;
        let baseline = scene.baseline.clone()?;
        let now = self.gltf_nodes_now(root);
        let mut out = BTreeMap::new();
        for node in &scene.nodes {
            let o = match now.get(&node.path) {
                None => NodeOverrides::deleted(),
                Some(now) => {
                    let Some(file) = baseline.get(&node.path) else {
                        continue;
                    };
                    let mut o = node_overrides(file, now);
                    o.parent = self.parent_override(root, scene, &node.path, file, now);
                    if o.parent.is_some()
                        && let Some(Value::Object(link)) = now.get("Attached3D")
                    {
                        // Its offset is in the new parent's space: all of it.
                        let mut link = link.clone();
                        link.remove("to");
                        o.components.insert("Attached3D".into(), Value::Object(link));
                    }
                    o
                }
            };
            if !o.is_empty() {
                out.insert(node.path.clone(), o);
            }
        }
        Some(out)
    }

    /// The parent node `path` (with components `now`, `file` in the file)
    /// hangs off instead of the file's, if it does and that parent can be
    /// named in a save.
    fn parent_override(
        &self,
        root: Entity,
        scene: &GltfRoot,
        path: &str,
        file: &JsonMap,
        now: &JsonMap,
    ) -> Option<NodeParent> {
        let to = |c: &JsonMap| c.get("Attached3D").and_then(|a| a["to"].as_u64());
        let now_to = to(now)?;
        if to(file) == Some(now_to) {
            return None;
        }
        let parent = Entity::from_bits(now_to);
        if parent == root {
            return Some(NodeParent::Root);
        }
        if let Some(node) = scene.nodes.iter().find(|n| n.entity == parent) {
            return Some(NodeParent::Node(node.path.clone()));
        }
        if self.world.get::<crate::reflect::NotSaved>(parent).is_some() {
            eprintln!(
                "ponko: {}: node \"{path}\" hangs off entity {now_to}, which saves leave out: that parent isn't kept",
                scene.path
            );
            return None;
        }
        Some(NodeParent::Entity(parent))
    }

    /// The overrides of `root`'s scene as the editor lists them, nodes in
    /// path order: deleted nodes, other parents, then components. Empty
    /// without a baseline (nothing was edited).
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn gltf_overrides(&self, root: Entity) -> Vec<GltfOverride> {
        let Some(scene) = self.world.get::<GltfRoot>(root) else {
            return Vec::new();
        };
        let overrides = self.scene_overrides(root).unwrap_or_default();
        let mut out = Vec::new();
        for (node, o) in overrides {
            let Some(entity) = scene.entity_at(&node) else {
                continue;
            };
            let mut push = |kind| {
                out.push(GltfOverride {
                    node: node.clone(),
                    entity,
                    kind,
                });
            };
            if o.deleted {
                push(GltfOverrideKind::Deleted);
                continue;
            }
            if o.parent.is_some()
                && let Some(link) = self.world.get::<crate::attach::Attached3D>(entity)
            {
                push(GltfOverrideKind::Parent(link.to));
            }
            let file = scene.baseline.as_ref().and_then(|b| b.get(&node));
            for (name, value) in &o.components {
                if name == "Attached3D" && o.parent.is_some() {
                    continue;
                }
                let kind = if file.is_some_and(|f| f.contains_key(name)) {
                    let fields = match value {
                        Value::Object(fields) => fields.keys().cloned().collect(),
                        _ => Vec::new(),
                    };
                    GltfOverrideKind::Changed {
                        name: name.clone(),
                        fields,
                    }
                } else {
                    GltfOverrideKind::Added(name.clone())
                };
                push(kind);
            }
            for name in &o.remove {
                push(GltfOverrideKind::Removed(name.clone()));
            }
        }
        out
    }

    /// Component `name` of node `node` of `root`'s scene as the file made
    /// it (`None`: the file doesn't give it one, or nothing was edited).
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn gltf_file_value(&self, root: Entity, node: &str, name: &str) -> Option<Value> {
        let scene = self.world.get::<GltfRoot>(root)?;
        scene.baseline.as_ref()?.get(node)?.get(name).cloned()
    }

    /// After `root`'s scene was spawned or brought to its file again:
    /// keeps its baseline (while editing, or if it has overrides) and puts
    /// its saved overrides back.
    pub(super) fn reapply_gltf_overrides(&mut self, root: Entity) {
        let overrides = self
            .world
            .get_resource::<GltfSceneSaves>()
            .and_then(|s| s.scenes.iter().find(|s| s.root == Some(root)))
            .map(|s| s.overrides.clone())
            .unwrap_or_default();
        let keep = !overrides.is_empty() || self.world.get_resource::<GltfEditing>().is_some();
        let baseline = keep.then(|| Arc::new(self.gltf_nodes_now(root)));
        let Some(scene) = self.world.get_mut::<GltfRoot>(root) else {
            return;
        };
        scene.baseline = baseline;
        if overrides.is_empty() {
            return;
        }
        let file = scene.path.clone();
        let nodes: Vec<(String, Entity)> = scene.nodes.iter().map(|n| (n.path.clone(), n.entity)).collect();
        for (path, o) in &overrides {
            let Some(&(_, e)) = nodes.iter().find(|(p, e)| p == path && self.world.is_alive(*e)) else {
                continue;
            };
            if let Some(parent) = &o.parent {
                let to = match parent {
                    NodeParent::Root => Some(root),
                    NodeParent::Node(p) => nodes.iter().find(|(n, _)| n == p).map(|&(_, e)| e),
                    NodeParent::Entity(e) => Some(*e),
                };
                match to.filter(|&to| self.world.is_alive(to) && to != e) {
                    Some(to) => match self.world.get_mut::<crate::attach::Attached3D>(e) {
                        Some(link) => link.to = to,
                        None => {
                            self.world.insert(e, crate::attach::Attached3D::to(to));
                        }
                    },
                    None => eprintln!("ponko: {file}: node \"{path}\": the parent it was attached to is gone"),
                }
            }
            for (name, value) in &o.components {
                let value = match (self.world.get_json(e, name), value) {
                    (Ok(Value::Object(mut now)), Value::Object(fields)) => {
                        now.extend(fields.iter().map(|(k, v)| (k.clone(), v.clone())));
                        Value::Object(now)
                    }
                    _ => value.clone(),
                };
                if let Err(err) = self.world.set_json(e, name, value) {
                    eprintln!("ponko: {file}: node \"{path}\": its override of {name} is left out: {err}");
                }
            }
            for name in &o.remove {
                let _ = self.world.remove_json(e, name);
            }
        }
        // Attached nodes go where their changed offsets put them.
        crate::hierarchy::settle(&mut self.world);
    }

    /// Spawns again every glTF scene whose saved record lost its root
    /// (a scene file or save was loaded), where it was saved; each adopts
    /// its record. Returns the errors, one per scene that didn't spawn.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn respawn_saved_gltf(&mut self) -> Vec<crate::error::Error> {
        let orphans: Vec<(String, Transform3D)> = self
            .world
            .get_resource::<GltfSceneSaves>()
            .map(|s| {
                s.scenes
                    .iter()
                    .filter(|s| s.root.is_none())
                    .map(|s| (s.path.clone(), s.at))
                    .collect()
            })
            .unwrap_or_default();
        let mut errors = Vec::new();
        for (path, at) in orphans {
            if let Err(e) = self.spawn_gltf(&path, at, super::GltfOptions::default()) {
                errors.push(e);
            }
        }
        errors
    }
}

/// The entities of every glTF scene when the editor's Play saved the
/// world, by the place of the scene's record in [`GltfSceneSaves`]: its
/// root and its nodes by path. Stop spawns the scenes again (they aren't
/// saved) and maps these to the new entities, so the undo history keeps
/// pointing at them.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct GltfEntities(Vec<Option<SceneEntities>>);

/// A scene's root and its nodes' entities by path.
type SceneEntities = (Entity, Vec<(String, Entity)>);

impl Ctx {
    /// The entities of every glTF scene now (see [`GltfEntities`]).
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn gltf_entities(&self) -> GltfEntities {
        let Some(saves) = self.world.get_resource::<GltfSceneSaves>() else {
            return GltfEntities::default();
        };
        GltfEntities(
            saves
                .scenes
                .iter()
                .map(|s| {
                    let root = s.root?;
                    let scene = self.world.get::<GltfRoot>(root)?;
                    Some((root, scene.nodes.iter().map(|n| (n.path.clone(), n.entity)).collect()))
                })
                .collect(),
        )
    }

    /// After the world was put back from a save that holds the scene
    /// records of `before` (the editor's Stop): spawns every scene again,
    /// with its overrides, and returns each entity of `before` that is
    /// another one now, with that one. Errors are printed: a scene whose
    /// file no longer loads stays out.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn respawn_gltf_entities(&mut self, before: &GltfEntities) -> Vec<(Entity, Entity)> {
        for e in self.respawn_saved_gltf() {
            eprintln!("ponko: {e}");
        }
        let roots: Vec<Option<Entity>> = self
            .world
            .get_resource::<GltfSceneSaves>()
            .map(|s| s.scenes.iter().map(|s| s.root).collect())
            .unwrap_or_default();
        let mut moved = Vec::new();
        for (old, new) in before.0.iter().zip(roots) {
            let (Some((old_root, nodes)), Some(new_root)) = (old, new) else {
                continue;
            };
            let Some(scene) = self.world.get::<GltfRoot>(new_root) else {
                continue;
            };
            moved.push((*old_root, new_root));
            for (path, e) in nodes {
                if let Some(now) = scene.entity_at(path) {
                    moved.push((*e, now));
                }
            }
        }
        moved.retain(|(a, b)| a != b);
        moved
    }

    /// An undo brought back despawned entities, as `(was, now)`: `was` the
    /// entity it stood for. Those that were nodes of a glTF scene are its
    /// nodes again: each scene with such a node records how its nodes
    /// differ from the file (the ones brought back no longer deleted) and
    /// is brought to its file again, which puts back on them what saves
    /// leave out (meshes, colliders, lights) and their overrides on top.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn gltf_nodes_back(&mut self, back: &[(Entity, Entity)]) {
        let mut touched: Vec<Entity> = Vec::new();
        let mut instances: Vec<(Entity, String, Entity)> = Vec::new();
        for (root, scene) in self.world.query::<(Entity, &mut GltfRoot)>().iter() {
            for node in &mut scene.nodes {
                let Some(&(_, now)) = back.iter().find(|(was, _)| *was == node.entity) else {
                    continue;
                };
                node.entity = now;
                if node.prefab.is_some() {
                    instances.push((root, node.path.clone(), now));
                }
                if !touched.contains(&root) {
                    touched.push(root);
                }
            }
        }
        for &(root, ref path, now) in &instances {
            if let Some(saves) = self.world.get_resource_mut::<GltfSceneSaves>()
                && let Some(scene) = saves.scenes.iter_mut().find(|s| s.root == Some(root))
                && let Some(instance) = scene.instances.get_mut(path)
            {
                instance.entity = Some(now);
            }
        }
        // Each node brought back, as it was deleted.
        let names: Vec<String> = self
            .world
            .reflected_components()
            .iter()
            .map(|n| n.to_string())
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut kept: Vec<(Entity, bool, JsonMap)> = Vec::new();
        for &(_, now) in back {
            let prefab = touched.iter().find_map(|&root| {
                let scene = self.world.get::<GltfRoot>(root)?;
                scene.nodes.iter().find(|n| n.entity == now).map(|n| n.prefab.is_some())
            });
            let Some(prefab) = prefab else { continue };
            if kept.iter().any(|(e, ..)| *e == now) {
                continue;
            }
            if !prefab {
                self.world.insert(now, crate::reflect::NotSaved);
            }
            kept.push((now, prefab, self.node_components(now, prefab, &names)));
        }
        for root in touched {
            self.record_scene_overrides(root);
            if let Err(e) = self.refresh_gltf_scene(root) {
                eprintln!("ponko: {e}");
            }
        }
        // The file dressed them again: what they had on top of it comes back.
        for (e, prefab, was) in kept {
            let now = self.node_components(e, prefab, &names);
            for (name, value) in &was {
                if now.get(name).is_none_or(|v| !same(v, value)) {
                    let _ = self.world.set_json(e, name, value.clone());
                }
            }
            for name in now.keys().filter(|n| !was.contains_key(*n)) {
                let _ = self.world.remove_json(e, name);
            }
        }
        crate::hierarchy::settle(&mut self.world);
    }

    /// Brings node `node` of `root`'s scene back, with the nodes below it,
    /// if they were deleted in the editor, and returns their entities.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn restore_gltf_nodes(&mut self, root: Entity, node: &str) -> Result<Vec<Entity>, crate::error::Error> {
        self.record_scene_overrides(root);
        let below = format!("{node}/");
        let mut restored: Vec<String> = Vec::new();
        if let Some(saves) = self.world.get_resource_mut::<GltfSceneSaves>()
            && let Some(scene) = saves.scenes.iter_mut().find(|s| s.root == Some(root))
        {
            for (path, o) in &mut scene.overrides {
                if o.deleted && (path == node || path.starts_with(&below)) {
                    o.deleted = false;
                    restored.push(path.clone());
                }
            }
            scene.overrides.retain(|_, o| !o.is_empty());
        }
        if restored.is_empty() {
            return Ok(Vec::new());
        }
        self.refresh_gltf_scene(root)?;
        let scene = self.world.get::<GltfRoot>(root);
        Ok(restored
            .iter()
            .filter_map(|p| scene.and_then(|s| s.entity_at(p)))
            .filter(|&e| self.world.is_alive(e))
            .collect())
    }

    /// Brings `root`'s scene to its file again as it was last read (as a
    /// hot reload does, without reading it), with its recorded overrides.
    fn refresh_gltf_scene(&mut self, root: Entity) -> Result<(), crate::error::Error> {
        let path = self
            .world
            .get::<GltfRoot>(root)
            .map(|s| s.path.clone())
            .ok_or_else(|| crate::error::Error::new(crate::error::ErrorKind::NotFound, "the glTF scene is gone"))?;
        let share = self
            .world
            .get_resource::<super::GltfShared>()
            .and_then(|t| t.0.get(&path).cloned());
        let Some((mut share, read)) = share.and_then(|s| {
            let read = s.read.clone()?;
            Some((s, read))
        }) else {
            return self
                .reload_gltf(&path)
                .map_err(|e| crate::error::Error::new(crate::error::ErrorKind::Decode, e).with_path(&path));
        };
        self.apply_gltf(root, &path, &read.nodes, &mut share, false, false)?;
        self.gltf_shared().0.insert(path, share);
        Ok(())
    }
}

/// How `now` (a node's components) differs from `file`.
fn node_overrides(file: &JsonMap, now: &JsonMap) -> NodeOverrides {
    // An attached node's world transform follows its offset.
    let derived = |name: &str| name == "Transform3D" && now.contains_key("Attached3D");
    let mut out = NodeOverrides::default();
    for (name, value) in now {
        if derived(name) {
            continue;
        }
        match file.get(name) {
            // A new link names an entity of this run only.
            None if name == "Attached3D" => {}
            None => {
                out.components.insert(name.clone(), value.clone());
            }
            Some(was) => {
                if let Some(changed) = changed(name, was, value) {
                    out.components.insert(name.clone(), changed);
                }
            }
        }
    }
    out.remove = file
        .keys()
        .filter(|name| !now.contains_key(*name) && !derived(name))
        .cloned()
        .collect();
    out
}

/// What of component `name` changed from `was` to `now`: the changed
/// fields of an object (not its parent link), else the whole value.
fn changed(name: &str, was: &Value, now: &Value) -> Option<Value> {
    match (was, now) {
        (Value::Object(was), Value::Object(now)) => {
            let fields: JsonMap = now
                .iter()
                .filter(|(k, _)| !(name == "Attached3D" && *k == "to"))
                .filter(|(k, v)| was.get(*k).is_none_or(|w| !same(w, v)))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (!fields.is_empty()).then_some(Value::Object(fields))
        }
        _ => (!same(was, now)).then(|| now.clone()),
    }
}

/// Equal, numbers within a small tolerance (transforms go through float
/// math on the way).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-4 * (1.0 + x.abs().max(y.abs())),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// A `.gltf` file of `nodes` (glTF node objects, `"mesh": 0` a 1 m cube),
/// every node without a parent at the top, for tests.
#[cfg(test)]
pub(crate) fn cube_gltf(nodes: &Value) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bytes = Vec::new();
    for i in 0..8u32 {
        for axis in 0..3 {
            let v: f32 = if i >> axis & 1 == 1 { 0.5 } else { -0.5 };
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    let faces: [u16; 36] = [
        0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5,
    ];
    for i in faces {
        bytes.extend_from_slice(&i.to_le_bytes());
    }
    let mut data = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            data.push(if i <= chunk.len() {
                TABLE[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    let list = nodes.as_array().map_or(&[][..], Vec::as_slice);
    let children: Vec<u64> = list
        .iter()
        .flat_map(|n| n["children"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c.as_u64())
        .collect();
    let top: Vec<usize> = (0..list.len()).filter(|i| !children.contains(&(*i as u64))).collect();
    serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": top }],
        "nodes": nodes,
        "meshes": [{ "name": "cube", "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }] }],
        "buffers": [{ "byteLength": bytes.len(), "uri": format!("data:application/octet-stream;base64,{data}") }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 96 },
            { "buffer": 0, "byteOffset": 96, "byteLength": 72 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5] },
            { "bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR" }
        ]
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3::Transform as T3;
    use glam::{Vec2, vec3};
    use ponko_ecs::Component;
    use serde_json::json;

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    struct Hp(f32);

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    struct Shiny {
        glow: f32,
    }

    /// A `.gltf` asset written for one test.
    struct Temp {
        name: String,
        file: std::path::PathBuf,
    }

    impl Temp {
        fn new(what: &str, text: &str) -> Self {
            let name = format!("ponko-test-gltf-overrides-{what}-{}.gltf", std::process::id());
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&name);
            std::fs::write(&file, text).unwrap();
            Self { name, file }
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.file);
        }
    }

    fn yard(crate_hp: f32, wall_x: f32) -> String {
        cube_gltf(&json!([
            { "name": "Crate", "mesh": 0, "extras": { "ponko:Hp": crate_hp } },
            { "name": "Shed", "translation": [0.0, 0.0, -4.0], "children": [2] },
            { "name": "Wall", "mesh": 0, "translation": [wall_x, 0.0, 0.0], "extras": { "ponko:Shiny": { "glow": 1.0 } } },
        ]))
    }

    fn ctx() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(640.0, 360.0));
        ctx.set_headless(true);
        ctx.world.reflect::<Hp>();
        ctx.world.reflect::<Shiny>();
        ctx
    }

    fn node(ctx: &Ctx, root: Entity, path: &str) -> Entity {
        ctx.world.get::<GltfRoot>(root).unwrap().entity(path).unwrap()
    }

    /// Edits as the editor does: the crate's Hp, the wall moved along its
    /// parent (its offset), its `Shiny` taken off.
    fn edit(ctx: &mut Ctx, root: Entity) {
        ctx.track_gltf_overrides();
        let (crate_e, wall) = (node(ctx, root, "Crate"), node(ctx, root, "Wall"));
        ctx.world.set_json(crate_e, "Hp", json!(8.0)).unwrap();
        let mut at = *ctx.world.get::<T3>(wall).unwrap();
        at.position.y = 2.0;
        crate::editor::place_world3d(&mut ctx.world, wall, at);
        ctx.world.remove_json(wall, "Shiny").unwrap();
    }

    #[test]
    fn edited_nodes_keep_their_overrides_when_the_file_is_exported_again() {
        let file = Temp::new("reexport", &yard(3.0, 1.0));
        let mut ctx = ctx();
        let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
        edit(&mut ctx, root);
        let (crate_e, wall) = (node(&ctx, root, "Crate"), node(&ctx, root, "Wall"));

        // The file changes the crate's Hp and the wall's place.
        std::fs::write(&file.file, yard(4.0, 2.0)).unwrap();
        ctx.reload_gltf(&file.name).unwrap();
        assert_eq!(node(&ctx, root, "Crate"), crate_e, "the same entities");
        assert_eq!(
            ctx.world.get::<Hp>(crate_e),
            Some(&Hp(8.0)),
            "the changed component stays changed"
        );
        let wall_at = ctx.world.get::<T3>(wall).unwrap().position;
        assert!(
            (wall_at - vec3(1.0, 2.0, -4.0)).length() < 1e-4,
            "where it was put in its parent, which the file still places: {wall_at}"
        );
        assert!(
            ctx.world.get::<Shiny>(wall).is_none(),
            "the removed component stays removed"
        );

        // A node edited back to the file has no override left.
        ctx.world.set_json(crate_e, "Hp", json!(4.0)).unwrap();
        ctx.record_gltf_overrides(None);
        let saves = ctx.world.resource::<GltfSceneSaves>();
        let kept: Vec<&String> = saves.scenes[0].overrides.keys().collect();
        assert_eq!(kept, ["Shed/Wall"]);
    }

    #[test]
    fn overrides_go_with_a_save_and_come_back_when_a_game_spawns_the_file_again() {
        let file = Temp::new("save", &yard(3.0, 1.0));
        let mut ctx = ctx();
        let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
        edit(&mut ctx, root);
        ctx.record_gltf_overrides(None);
        let save = ctx.world.save();
        assert_eq!(
            save["resources"]["GltfSceneSaves"]["scenes"][0]["overrides"]["Crate"]["components"]["Hp"],
            json!(8.0)
        );

        // A game without the editor: loads the save, spawns the level.
        let mut game = self::ctx();
        game.replace_world(&save).unwrap();
        let root = game.spawn_prefab(&file.name, T3::default()).unwrap();
        let (crate_e, wall) = (node(&game, root, "Crate"), node(&game, root, "Wall"));
        assert_eq!(game.world.get::<Hp>(crate_e), Some(&Hp(8.0)));
        let wall_at = game.world.get::<T3>(wall).unwrap().position;
        assert!((wall_at - vec3(1.0, 2.0, -4.0)).length() < 1e-4, "{wall_at}");
        assert!(game.world.get::<Shiny>(wall).is_none());
        // Saved again unedited, it still holds them.
        game.record_gltf_overrides(None);
        let again = game.world.save();
        assert_eq!(
            again["resources"]["GltfSceneSaves"]["scenes"][0]["overrides"],
            save["resources"]["GltfSceneSaves"]["scenes"][0]["overrides"]
        );
    }

    fn map(v: Value) -> JsonMap {
        match v {
            Value::Object(m) => m,
            _ => unreachable!(),
        }
    }

    #[test]
    fn only_changed_fields_added_and_removed_components_are_overrides() {
        let file = map(json!({
            "Attached3D": { "to": 1, "offset": { "position": [0, 0, 0] }, "rotation": 0 },
            "Transform3D": { "position": [1, 0, 0] },
            "Health": { "hp": 3, "max": 3 },
            "Door": {},
        }));
        let now = map(json!({
            "Attached3D": { "to": 7, "offset": { "position": [2, 0, 0] }, "rotation": 0 },
            "Transform3D": { "position": [3, 0, 0] },
            "Health": { "hp": 3.000_01, "max": 5 },
            "Locked": true,
        }));
        let o = node_overrides(&file, &now);
        assert_eq!(
            Value::Object(o.components),
            json!({
                "Attached3D": { "offset": { "position": [2, 0, 0] } },
                "Health": { "max": 5 },
                "Locked": true,
            }),
            "the parent link, the derived world transform and float noise aren't overrides"
        );
        assert_eq!(o.remove, ["Door"]);
        assert!(
            node_overrides(&file, &file).is_empty(),
            "a node as the file made it has none"
        );
    }
}
