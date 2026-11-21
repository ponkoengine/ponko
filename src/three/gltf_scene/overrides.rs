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
}

impl NodeOverrides {
    fn is_empty(&self) -> bool {
        self.components.is_empty() && self.remove.is_empty()
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
            let wanted: &[&str] = if node.prefab.is_some() { &["Transform3D"] } else { &names };
            let mut map = JsonMap::new();
            for &name in wanted {
                if NOT_COMPARED.contains(&name) {
                    continue;
                }
                if let Ok(v) = self.world.get_json(node.entity, name) {
                    map.insert(name.to_string(), v);
                }
            }
            out.insert(node.path.clone(), map);
        }
        out
    }

    /// Writes how the scenes of file `path` (of every file for `None`)
    /// differ from their baseline into their saved records, with where
    /// their roots stand now. Scenes without a baseline keep their record.
    pub(crate) fn record_gltf_overrides(&mut self, path: Option<&str>) {
        let roots: Vec<(Entity, Arc<BTreeMap<String, JsonMap>>)> = self
            .world
            .query::<(Entity, &GltfRoot)>()
            .iter()
            .filter(|(_, r)| path.is_none_or(|p| r.path == p))
            .filter_map(|(e, r)| Some((e, r.baseline.clone()?)))
            .collect();
        for (root, baseline) in roots {
            let now = self.gltf_nodes_now(root);
            let overrides: BTreeMap<String, NodeOverrides> = baseline
                .iter()
                .filter_map(|(node, file)| Some((node.clone(), node_overrides(file, now.get(node)?))))
                .filter(|(_, o)| !o.is_empty())
                .collect();
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
            data.push(if i <= chunk.len() { TABLE[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
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
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(&name);
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
        assert_eq!(ctx.world.get::<Hp>(crate_e), Some(&Hp(8.0)), "the changed component stays changed");
        let wall_at = ctx.world.get::<T3>(wall).unwrap().position;
        assert!(
            (wall_at - vec3(1.0, 2.0, -4.0)).length() < 1e-4,
            "where it was put in its parent, which the file still places: {wall_at}"
        );
        assert!(ctx.world.get::<Shiny>(wall).is_none(), "the removed component stays removed");

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
        assert_eq!(save["resources"]["GltfSceneSaves"]["scenes"][0]["overrides"]["Crate"]["components"]["Hp"], json!(8.0));

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
        assert!(node_overrides(&file, &file).is_empty(), "a node as the file made it has none");
    }
}
