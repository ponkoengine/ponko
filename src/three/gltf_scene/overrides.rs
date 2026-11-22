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
//! the scene's root, the root or a node of another scene (by the number of
//! that scene's record), or an entity saved with the world. Another
//! scene's node may spawn after the node attached to it (scenes spawn
//! one by one after a load): each spawn links the nodes waiting for it.

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
///
/// Saved as [`SavedNodeOverrides`]: a parent in another scene goes in a
/// field of its own, which builds from before such parents existed skip.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedNodeOverrides", into = "SavedNodeOverrides")]
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

/// [`NodeOverrides`] as saves hold it. A parent in another scene, with its
/// offset (the node's `Attached3D` without `to`), is `scene_parent`, not
/// `parent` and `components`: an older build reads a variant of
/// [`NodeParent`] it doesn't know as an error, which would lose the whole
/// record, and would put that offset on the file's parent. It skips the
/// unknown field instead and leaves the node on the file's parent. Saves
/// that hold such a parent as `parent` still read.
#[derive(Serialize, Deserialize)]
struct SavedNodeOverrides {
    #[serde(default, skip_serializing_if = "JsonMap::is_empty")]
    components: JsonMap,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    remove: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<NodeParent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_parent: Option<SceneParent>,
}

/// A parent in another scene, as saves hold it.
#[derive(Serialize, Deserialize)]
struct SceneParent {
    /// The number of that scene's record.
    scene: u64,
    /// The node's path; none for the scene's root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node: Option<String>,
    /// The `Attached3D` fields but `to`: the offset in that parent's space.
    #[serde(default, skip_serializing_if = "JsonMap::is_empty")]
    link: JsonMap,
}

impl From<NodeOverrides> for SavedNodeOverrides {
    fn from(o: NodeOverrides) -> Self {
        let NodeOverrides {
            mut components,
            remove,
            deleted,
            parent,
        } = o;
        let (parent, scene_parent) = match parent {
            Some(NodeParent::SceneRoot(scene)) => (None, Some((scene, None))),
            Some(NodeParent::SceneNode { scene, node }) => (None, Some((scene, Some(node)))),
            other => (other, None),
        };
        let scene_parent = scene_parent.map(|(scene, node)| SceneParent {
            scene,
            node,
            link: match components.remove("Attached3D") {
                Some(Value::Object(link)) => link,
                _ => JsonMap::new(),
            },
        });
        Self {
            components,
            remove,
            deleted,
            parent,
            scene_parent,
        }
    }
}

impl From<SavedNodeOverrides> for NodeOverrides {
    fn from(o: SavedNodeOverrides) -> Self {
        let SavedNodeOverrides {
            mut components,
            remove,
            deleted,
            mut parent,
            scene_parent,
        } = o;
        if let Some(SceneParent { scene, node, link }) = scene_parent {
            parent = Some(match node {
                Some(node) => NodeParent::SceneNode { scene, node },
                None => NodeParent::SceneRoot(scene),
            });
            components.insert("Attached3D".into(), Value::Object(link));
        }
        Self {
            components,
            remove,
            deleted,
            parent,
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
    /// The root of another scene, by its record's number. Saved as
    /// [`SceneParent`]; read here from saves that held it as `parent`.
    SceneRoot(u64),
    /// A node of another scene: its record's number and the node's path.
    /// Saved as [`SceneParent`], as [`SceneRoot`](Self::SceneRoot) is.
    SceneNode { scene: u64, node: String },
}

impl NodeParent {
    /// The other scene it names, if it names one.
    fn other_scene(&self) -> Option<u64> {
        match self {
            Self::SceneRoot(id) | Self::SceneNode { scene: id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// A node's link to a parent in another scene, as its scene's record
/// holds it: the parent and the `Attached3D` fields but `to` (see
/// [`Ctx::set_waiting_link`]).
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WaitingLink {
    parent: NodeParent,
    link: Value,
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
    /// It was attached to the root or a node of another scene that isn't
    /// there (its file didn't load): which one, as the editor names it
    /// (`node "Post" of posts.glb`). The node waits on the file's parent;
    /// the link stays in its record until it is reverted.
    MissingParent(String),
    /// It was deleted.
    Deleted,
}

impl GltfOverrideKind {
    /// The component it is about (`Attached3D` for a parent).
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn component(&self) -> Option<&str> {
        match self {
            Self::Changed { name, .. } | Self::Added(name) | Self::Removed(name) => Some(name),
            Self::Parent(_) | Self::MissingParent(_) => Some("Attached3D"),
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
                    } else if let Some(waiting) = self.waiting_link(root, &node.path) {
                        // Its parent's scene isn't spawned (yet): the link
                        // it was saved with stays.
                        o.parent = Some(waiting.0);
                        o.components.insert("Attached3D".into(), waiting.1);
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
        if let Some(other) = self.other_scene_parent(parent) {
            return Some(other);
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
            if let Some(missing) = o.parent.as_ref().and_then(|p| self.missing_parent(p)) {
                push(GltfOverrideKind::MissingParent(missing));
            } else if o.parent.is_some()
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
            // Nodes of other scenes may wait for this one.
            self.link_scene_parents();
            crate::hierarchy::settle(&mut self.world);
            return;
        }
        let file = scene.path.clone();
        let nodes: Vec<(String, Entity)> = scene.nodes.iter().map(|n| (n.path.clone(), n.entity)).collect();
        for (path, o) in &overrides {
            let Some(&(_, e)) = nodes.iter().find(|(p, e)| p == path && self.world.is_alive(*e)) else {
                continue;
            };
            if let Some(parent) = &o.parent {
                if parent.other_scene().is_some() {
                    // Linked below, with its offset, once that scene is
                    // spawned.
                    let rest = o.components.iter().filter(|(name, _)| *name != "Attached3D");
                    let rest: Vec<(String, Value)> = rest.map(|(k, v)| (k.clone(), v.clone())).collect();
                    self.apply_node_components(&file, path, e, &rest, &o.remove);
                    continue;
                }
                let to = match parent {
                    NodeParent::Root => Some(root),
                    NodeParent::Node(p) => nodes.iter().find(|(n, _)| n == p).map(|&(_, e)| e),
                    NodeParent::Entity(e) => Some(*e),
                    NodeParent::SceneRoot(_) | NodeParent::SceneNode { .. } => None,
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
            let components: Vec<(String, Value)> = o.components.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            self.apply_node_components(&file, path, e, &components, &o.remove);
        }
        self.link_scene_parents();
        // Attached nodes go where their changed offsets put them.
        crate::hierarchy::settle(&mut self.world);
    }

    /// Puts a node's overridden `components` (changed fields merged into
    /// what it has) on node entity `e` and takes `remove` off it.
    fn apply_node_components(
        &mut self,
        file: &str,
        path: &str,
        e: Entity,
        components: &[(String, Value)],
        remove: &[String],
    ) {
        for (name, value) in components {
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
        for name in remove {
            let _ = self.world.remove_json(e, name);
        }
    }

    /// The scene record numbers and roots of the spawned scenes.
    fn spawned_scenes(&self) -> Vec<(u64, Entity)> {
        let Some(saves) = self.world.get_resource::<GltfSceneSaves>() else {
            return Vec::new();
        };
        saves
            .scenes
            .iter()
            .filter_map(|s| s.root.filter(|&r| self.world.is_alive(r)).map(|r| (s.id, r)))
            .collect()
    }

    /// `parent` as the root or a node of another spawned scene.
    fn other_scene_parent(&self, parent: Entity) -> Option<NodeParent> {
        for (id, root) in self.spawned_scenes() {
            if root == parent {
                return Some(NodeParent::SceneRoot(id));
            }
            let Some(scene) = self.world.get::<GltfRoot>(root) else {
                continue;
            };
            if let Some(node) = scene.nodes.iter().find(|n| n.entity == parent) {
                return Some(NodeParent::SceneNode {
                    scene: id,
                    node: node.path.clone(),
                });
            }
        }
        None
    }

    /// The entity a parent in another scene is now: `None` while that
    /// scene isn't spawned or the node is deleted.
    fn other_scene_entity(&self, parent: &NodeParent) -> Option<Entity> {
        let id = parent.other_scene()?;
        let (_, root) = self.spawned_scenes().into_iter().find(|&(s, _)| s == id)?;
        let to = match parent {
            NodeParent::SceneNode { node, .. } => self.world.get::<GltfRoot>(root)?.entity_at(node)?,
            _ => root,
        };
        self.world.is_alive(to).then_some(to)
    }

    /// The saved link of node `path` of `root`'s scene to another scene's
    /// root or node, with its saved `Attached3D` fields, while that scene
    /// has a record and isn't spawned (yet). A link to a scene that is
    /// gone, or to a node deleted from it, isn't kept.
    fn waiting_link(&self, root: Entity, path: &str) -> Option<(NodeParent, Value)> {
        let saves = self.world.get_resource::<GltfSceneSaves>()?;
        let record = saves.scenes.iter().find(|s| s.root == Some(root))?;
        let o = record.overrides.get(path)?;
        let parent = o.parent.clone()?;
        let other = saves.scenes.iter().find(|s| Some(s.id) == parent.other_scene())?;
        if other.root.is_some_and(|r| self.world.is_alive(r)) {
            return None;
        }
        Some((parent, o.components.get("Attached3D")?.clone()))
    }

    /// `parent` named as the editor lists it, if it is in another scene
    /// that has a record and isn't spawned: the node waits for it.
    fn missing_parent(&self, parent: &NodeParent) -> Option<String> {
        let id = parent.other_scene()?;
        let saves = self.world.get_resource::<GltfSceneSaves>()?;
        let other = saves.scenes.iter().find(|s| s.id == id)?;
        if other.root.is_some_and(|r| self.world.is_alive(r)) {
            return None;
        }
        Some(match parent {
            NodeParent::SceneNode { node, .. } => format!("node \"{node}\" of {}", other.path),
            _ => format!("the root of {}", other.path),
        })
    }

    /// Sets the link of node `node` of `root`'s scene to a parent in
    /// another scene, as its record holds it, to `link` (`None` forgets
    /// it: the node stays on the file's parent), and returns the link the
    /// record held. A link to a scene that is spawned is made at once.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn set_waiting_link(
        &mut self,
        root: Entity,
        node: &str,
        link: Option<WaitingLink>,
    ) -> Result<Option<WaitingLink>, crate::error::Error> {
        let Some(scene) = self
            .world
            .get_resource_mut::<GltfSceneSaves>()
            .and_then(|s| s.scenes.iter_mut().find(|s| s.root == Some(root)))
        else {
            return Err(crate::error::Error::new(
                crate::error::ErrorKind::NotFound,
                "the glTF scene has no saved record",
            ));
        };
        let was = scene.overrides.get(node).and_then(|o| {
            let parent = o.parent.clone().filter(|p| p.other_scene().is_some())?;
            let link = o.components.get("Attached3D").cloned().unwrap_or(Value::Null);
            Some(WaitingLink { parent, link })
        });
        let linked = link.is_some();
        match link {
            Some(WaitingLink { parent, link }) => {
                let o = scene.overrides.entry(node.to_string()).or_default();
                o.parent = Some(parent);
                o.components.insert("Attached3D".into(), link);
            }
            None => {
                if let Some(o) = scene.overrides.get_mut(node)
                    && o.parent.as_ref().is_some_and(|p| p.other_scene().is_some())
                {
                    o.parent = None;
                    o.components.remove("Attached3D");
                }
                scene.overrides.retain(|_, o| !o.is_empty());
            }
        }
        if linked {
            self.link_scene_parents();
            crate::hierarchy::settle(&mut self.world);
        }
        Ok(was)
    }

    /// Hangs every spawned node whose saved parent is in another scene off
    /// that parent, where it is spawned and the node isn't on it yet, with
    /// its saved offset.
    fn link_scene_parents(&mut self) {
        let Some(saves) = self.world.get_resource::<GltfSceneSaves>() else {
            return;
        };
        let mut links: Vec<(String, String, Entity, NodeParent, Value)> = Vec::new();
        for record in &saves.scenes {
            let Some(scene) = record.root.and_then(|r| self.world.get::<GltfRoot>(r)) else {
                continue;
            };
            for (path, o) in &record.overrides {
                let Some(parent) = o.parent.clone().filter(|p| p.other_scene().is_some()) else {
                    continue;
                };
                let Some(e) = scene.entity_at(path).filter(|&e| self.world.is_alive(e)) else {
                    continue;
                };
                let fields = o.components.get("Attached3D").cloned().unwrap_or(Value::Null);
                links.push((record.path.clone(), path.clone(), e, parent, fields));
            }
        }
        for (file, path, e, parent, fields) in links {
            let Some(to) = self.other_scene_entity(&parent).filter(|&to| to != e) else {
                continue;
            };
            if self
                .world
                .get::<crate::attach::Attached3D>(e)
                .is_some_and(|l| l.to == to)
            {
                continue;
            }
            let mut link = match self.world.get_json(e, "Attached3D") {
                Ok(Value::Object(now)) => now,
                _ => JsonMap::new(),
            };
            if let Value::Object(fields) = fields {
                link.extend(fields);
            }
            link.insert("to".into(), serde_json::json!(to.to_bits()));
            if let Err(err) = self.world.set_json(e, "Attached3D", Value::Object(link)) {
                eprintln!("ponko: {file}: node \"{path}\": the link to its parent in another scene is left out: {err}");
            }
        }
    }

    /// Spawns again every glTF scene whose saved record lost its root
    /// (a scene file or save was loaded), where it was saved; each adopts
    /// its record. Returns the scenes that didn't spawn: each one's file,
    /// and its error.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn respawn_saved_gltf(&mut self) -> Vec<(String, crate::error::Error)> {
        self.respawn_saved_gltf_of(None)
    }

    /// [`respawn_saved_gltf`](Self::respawn_saved_gltf) for the records of
    /// file `only` alone (`None`: every file), as the editor's Retry does
    /// for a scene that didn't load. Each scene that spawns hangs the
    /// nodes of other scenes saved on its nodes off them again.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn respawn_saved_gltf_of(&mut self, only: Option<&str>) -> Vec<(String, crate::error::Error)> {
        let orphans: Vec<(String, Transform3D)> = self
            .world
            .get_resource::<GltfSceneSaves>()
            .map(|s| {
                s.scenes
                    .iter()
                    .filter(|s| s.root.is_none() && only.is_none_or(|f| f == s.path))
                    .map(|s| (s.path.clone(), s.at))
                    .collect()
            })
            .unwrap_or_default();
        let mut errors = Vec::new();
        for (path, at) in orphans {
            if let Err(e) = self.spawn_gltf(&path, at, super::GltfOptions::default()) {
                errors.push((path, e));
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

/// A glTF scene that didn't spawn: its file and its error.
type GltfFailure = (String, crate::error::Error);

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
    /// another one now, with that one. A scene whose file no longer loads
    /// stays out: its error is printed and returned with its file (the
    /// editor lists it in its panel of scenes that didn't load).
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn respawn_gltf_entities(&mut self, before: &GltfEntities) -> (Vec<(Entity, Entity)>, Vec<GltfFailure>) {
        let failed = self.respawn_saved_gltf();
        for (_, e) in &failed {
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
        (moved, failed)
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

    #[test]
    fn a_node_attached_to_another_scenes_node_comes_back_on_it_whichever_scene_spawns_first() {
        let yard = Temp::new("cross-yard", &yard(3.0, 1.0));
        let posts = Temp::new("cross-posts", &cube_gltf(&json!([{ "name": "Post", "mesh": 0 }])));
        let mut ctx = ctx();
        let a = ctx.spawn_prefab(&yard.name, T3::default()).unwrap();
        let b = ctx.spawn_prefab(&posts.name, T3::at(vec3(4.0, 0.0, 0.0))).unwrap();
        ctx.track_gltf_overrides();
        let (wall, post) = (node(&ctx, a, "Shed/Wall"), node(&ctx, b, "Post"));
        // Hung off the post where it stands, as the editor does.
        let at = *ctx.world.get::<T3>(wall).unwrap();
        ctx.world.insert(wall, crate::attach::Attached3D::to(post));
        crate::editor::place_world3d(&mut ctx.world, wall, at);
        ctx.record_gltf_overrides(None);
        let save = ctx.world.save();
        let records = &save["resources"]["GltfSceneSaves"]["scenes"];
        let wall_record = &records[0]["overrides"]["Shed/Wall"];
        assert_eq!(wall_record["scene_parent"]["scene"], records[1]["id"], "{records}");
        assert_eq!(wall_record["scene_parent"]["node"], "Post", "{records}");

        for posts_first in [false, true] {
            let mut game = self::ctx();
            game.replace_world(&save).unwrap();
            let spawn =
                |game: &mut Ctx, file: &Temp, x: f32| game.spawn_prefab(&file.name, T3::at(vec3(x, 0.0, 0.0))).unwrap();
            let (a, b) = if posts_first {
                let b = spawn(&mut game, &posts, 4.0);
                (spawn(&mut game, &yard, 0.0), b)
            } else {
                let a = spawn(&mut game, &yard, 0.0);
                // Saved again before the post's scene is there: the link waits.
                game.record_gltf_overrides(None);
                let again = game.world.save();
                assert_eq!(
                    again["resources"]["GltfSceneSaves"]["scenes"][0]["overrides"], records[0]["overrides"],
                    "kept while its parent's scene isn't spawned"
                );
                (a, spawn(&mut game, &posts, 4.0))
            };
            let (wall, post) = (node(&game, a, "Shed/Wall"), node(&game, b, "Post"));
            let link = game.world.get::<crate::attach::Attached3D>(wall).map(|l| l.to);
            assert_eq!(link, Some(post), "posts first: {posts_first}");
            let now = game.world.get::<T3>(wall).unwrap().position;
            assert!((now - at.position).length() < 1e-4, "posts first: {posts_first}: {now}");
        }
    }

    /// A save whose wall hangs off the post of another scene.
    fn cross_scene_save(yard_file: &Temp, posts: &Temp) -> Value {
        let mut ctx = ctx();
        let a = ctx.spawn_prefab(&yard_file.name, T3::default()).unwrap();
        let b = ctx.spawn_prefab(&posts.name, T3::at(vec3(4.0, 0.0, 0.0))).unwrap();
        ctx.track_gltf_overrides();
        let (wall, post) = (node(&ctx, a, "Shed/Wall"), node(&ctx, b, "Post"));
        let at = *ctx.world.get::<T3>(wall).unwrap();
        ctx.world.insert(wall, crate::attach::Attached3D::to(post));
        crate::editor::place_world3d(&mut ctx.world, wall, at);
        // Changed on top: an older build keeps that.
        ctx.world.insert(node(&ctx, a, "Crate"), Hp(9.0));
        ctx.record_gltf_overrides(None);
        ctx.world.save()
    }

    /// The records of glTF scenes as builds from before parents in other
    /// scenes read them (the types of then, field for field).
    mod before_scene_parents {
        use super::*;

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        pub enum NodeParent {
            Root,
            Node(String),
            Entity(Entity),
        }

        #[derive(Debug, Deserialize)]
        pub struct NodeOverrides {
            #[serde(default)]
            pub components: JsonMap,
            #[serde(default)]
            pub parent: Option<NodeParent>,
        }

        #[derive(Debug, Deserialize)]
        pub struct SavedScene {
            #[allow(dead_code)]
            pub path: String,
            #[serde(default)]
            pub overrides: BTreeMap<String, NodeOverrides>,
        }

        #[derive(Debug, Deserialize)]
        pub struct GltfSceneSaves {
            pub scenes: Vec<SavedScene>,
        }
    }

    #[test]
    fn a_save_with_a_parent_in_another_scene_stays_readable_by_builds_from_before_such_parents() {
        let yard_file = Temp::new("old-build-yard", &yard(3.0, 1.0));
        let posts = Temp::new("old-build-posts", &cube_gltf(&json!([{ "name": "Post", "mesh": 0 }])));
        let save = cross_scene_save(&yard_file, &posts);
        let records = save["resources"]["GltfSceneSaves"].clone();

        let old: before_scene_parents::GltfSceneSaves = serde_json::from_value(records.clone())
            .unwrap_or_else(|e| panic!("an older build reads the records: {e}\n{records}"));
        let wall = &old.scenes[0].overrides["Shed/Wall"];
        assert!(
            wall.parent.is_none(),
            "it leaves the wall on the file's parent: {wall:?}"
        );
        assert!(
            !wall.components.contains_key("Attached3D"),
            "and doesn't put the other parent's offset on the file's: {wall:?}"
        );
        assert_eq!(
            old.scenes[0].overrides["Crate"].components["Hp"],
            json!(9.0),
            "the rest it keeps"
        );

        // Negative control: the link held as `parent`, as saves of builds
        // in between wrote it, is what an older build can't read.
        let mut between = records.clone();
        let link = between["scenes"][0]["overrides"]["Shed/Wall"].as_object_mut().unwrap();
        let held = link.remove("scene_parent").unwrap();
        link.insert(
            "parent".into(),
            json!({ "scene_node": { "scene": held["scene"], "node": held["node"] } }),
        );
        link.insert("components".into(), json!({ "Attached3D": held["link"] }));
        let refused = serde_json::from_value::<before_scene_parents::GltfSceneSaves>(between.clone());
        assert!(refused.is_err(), "{refused:?}");

        // This build reads both forms the same.
        let now: GltfSceneSaves = serde_json::from_value(records).unwrap();
        let then: GltfSceneSaves = serde_json::from_value(between).unwrap();
        assert_eq!(now.scenes[0].overrides, then.scenes[0].overrides);
        assert!(matches!(
            now.scenes[0].overrides["Shed/Wall"].parent,
            Some(NodeParent::SceneNode { ref node, .. }) if node == "Post"
        ));
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
