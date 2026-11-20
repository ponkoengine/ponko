//! Prefab instances in the editor: where an instance differs from its
//! prefab (its overrides), putting those back (revert), and writing an
//! instance into its prefab file and every other instance (apply).
//!
//! An instance is an entity with a [`PrefabInstance`] (the editor adds it to
//! the prefabs it places) and the [`PrefabParts`] spawning gave it. To
//! compare it, the prefab is spawned again where the instance stands, by
//! the same code a game uses, so defaults, included prefabs and entity
//! references come out as they would; the copy's references are pointed at
//! the instance's entities and each component of each entity is compared
//! (numbers within a small tolerance), the root by number, the parts by
//! their place in `PrefabParts`. Then the copy is despawned. Where the
//! instance stands (its root's transform) is its own, never an override.
//!
//! glTF scenes placed as prefabs aren't instances in this sense: their root
//! is a `GltfRoot` and their file isn't written by the editor.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ponko_ecs::Entity;
use serde_json::{Value, json};

use super::{Editor, Undo, snapshot};
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};
use crate::hierarchy::PrefabInstance;
use crate::reflect::WorldReflect;
use crate::scene::{Placement, PrefabFile, PrefabParts};

/// Components that are links spawning makes (or the engine keeps), not
/// content: never compared.
const LINKS: [&str; 3] = ["PrefabParts", "PrefabInstance", "Children"];
/// Links never written into a prefab file (spawning makes them again).
const NOT_WRITTEN: [&str; 2] = ["PrefabParts", "PrefabInstance"];
/// The root's own placement.
const PLACEMENT: [&str; 2] = ["Transform2D", "Transform3D"];

/// How an instance's component differs from its prefab's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverrideKind {
    /// Both have it, with different values.
    Changed,
    /// Only the instance has it.
    Added,
    /// Only the prefab has it.
    Removed,
}

/// One component of one entity of an instance that isn't as the prefab has
/// it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Override {
    /// Which entity of the instance: 0 the root, then its parts in order.
    pub member: usize,
    pub entity: Entity,
    pub component: String,
    /// The prefab's value (naming the instance's entities); `None`: the
    /// prefab doesn't have it.
    pub prefab: Option<Value>,
    /// The instance's value; `None`: it doesn't have it.
    pub instance: Option<Value>,
}

impl Override {
    pub fn kind(&self) -> OverrideKind {
        match (&self.prefab, &self.instance) {
            (Some(_), Some(_)) => OverrideKind::Changed,
            (None, _) => OverrideKind::Added,
            (_, None) => OverrideKind::Removed,
        }
    }

    /// The fields that differ, for a value with fields; else empty.
    pub fn fields(&self) -> Vec<String> {
        match (&self.prefab, &self.instance) {
            (Some(Value::Object(p)), Some(Value::Object(i))) => {
                let mut keys: Vec<&String> = p.keys().chain(i.keys()).collect();
                keys.sort();
                keys.dedup();
                keys.into_iter()
                    .filter(|k| !matches!((p.get(*k), i.get(*k)), (Some(a), Some(b)) if close(a, b)))
                    .cloned()
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

/// An instance against its prefab file.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Comparison {
    /// The prefab file.
    pub path: String,
    pub overrides: Vec<Override>,
    /// Entities hanging off the instance that the prefab doesn't have:
    /// parts beyond the prefab's, and entities attached to it in the scene.
    pub added: Vec<Entity>,
    /// Entities of the prefab the instance doesn't have (deleted from it).
    pub missing: usize,
    /// Parts the instance has beyond the prefab's (they're in `added`).
    pub extra_parts: usize,
}

impl Comparison {
    /// Whether the instance is exactly its prefab.
    pub fn is_clean(&self) -> bool {
        self.overrides.is_empty() && self.added.is_empty() && self.missing == 0
    }

    /// Whether its entities line up with the prefab's one for one.
    fn same_parts(&self) -> bool {
        self.missing == 0 && self.extra_parts == 0
    }
}

/// Whether two JSON values are the same, numbers within a tolerance (the
/// parts of a moved instance are placed again, with rounding).
fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-4 * x.abs().max(y.abs()).max(1.0),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| close(a, b)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, a)| y.get(k).is_some_and(|b| close(a, b)))
        }
        _ => a == b,
    }
}

/// The prefab file `e` is an instance of, if it's an instance's root.
pub(crate) fn instance_path(world: &ponko_ecs::World, e: Entity) -> Option<String> {
    world.get::<PrefabInstance>(e).map(|p| p.path.clone())
}

/// Every entity of the world.
fn everyone(world: &ponko_ecs::World) -> Vec<Entity> {
    let mut all: Vec<Entity> = world
        .raw_ref()
        .archetypes()
        .iter()
        .flat_map(|a| a.entities().iter().copied())
        .collect();
    all.sort_unstable();
    all
}

/// The root of the instance `e` belongs to: `e` itself if it is one, else
/// the instance whose parts list it.
pub(crate) fn instance_root(world: &ponko_ecs::World, e: Entity) -> Option<Entity> {
    if world.get::<PrefabInstance>(e).is_some() {
        return Some(e);
    }
    everyone(world).into_iter().find(|&r| {
        world.get::<PrefabInstance>(r).is_some() && world.get::<PrefabParts>(r).is_some_and(|p| p.0.contains(&e))
    })
}

/// The roots of the instances of prefab `path`, in id order.
pub(crate) fn instances(world: &ponko_ecs::World, path: &str) -> Vec<Entity> {
    everyone(world)
        .into_iter()
        .filter(|&e| world.get::<PrefabInstance>(e).is_some_and(|p| p.path == path))
        .collect()
}

/// The root and its parts, in order; a part that was deleted is `None`.
fn members(world: &ponko_ecs::World, root: Entity) -> Vec<Option<Entity>> {
    let mut out = vec![Some(root)];
    if let Some(parts) = world.get::<PrefabParts>(root) {
        out.extend(parts.0.iter().map(|&p| world.is_alive(p).then_some(p)));
    }
    out
}

/// Where the instance stands: its root's whole transform.
fn placement(world: &ponko_ecs::World, root: Entity) -> Placement {
    #[cfg(feature = "3d")]
    if let Some(&t) = world.get::<crate::d3::Transform>(root) {
        return Placement::D3(t);
    }
    match world.get::<crate::d2::Transform>(root) {
        Some(&t) => Placement::D2(t),
        None => Placement::Position2(glam::Vec2::ZERO),
    }
}

/// Where the copy to compare with is spawned: at the instance's position,
/// with the prefab's own rotation and scale. Its attached parts are then
/// placed off the instance's root, the way the instance's own are (spawning
/// at a scaled placement scales the parts too, scaling afterwards doesn't).
fn position(world: &ponko_ecs::World, root: Entity) -> Placement {
    #[cfg(feature = "3d")]
    if let Some(t) = world.get::<crate::d3::Transform>(root) {
        return Placement::Position3(t.position);
    }
    Placement::Position2(
        world
            .get::<crate::d2::Transform>(root)
            .map_or(glam::Vec2::ZERO, |t| t.position),
    )
}

/// Runs `f` with prefab `path` read as `file` instead of what the file
/// holds now (to compare with the prefab as it was before it changed).
fn with_prefab<R>(ctx: &mut Ctx, path: &str, file: Option<Rc<PrefabFile>>, f: impl FnOnce(&mut Ctx) -> R) -> R {
    let Some(file) = file else { return f(ctx) };
    let kept = ctx.assets.prefabs.insert(path.to_string(), file);
    let out = f(ctx);
    match kept {
        Some(kept) => ctx.assets.prefabs.insert(path.to_string(), kept),
        None => ctx.assets.prefabs.remove(path),
    };
    out
}

/// A prefab file's content, read as a prefab (and its assets loaded), for
/// [`with_prefab`].
fn parse_prefab(ctx: &mut Ctx, path: &str, json: &Value) -> Result<Rc<PrefabFile>, Error> {
    let file = PrefabFile::parse(json).map_err(|e| Error::new(ErrorKind::Json, e).with_path(path))?;
    let entities = json.get("entities").and_then(Value::as_array);
    for entity in entities.into_iter().flatten() {
        if let Some(c) = entity.get("components") {
            ctx.load_scene_assets(c)?;
        }
    }
    if let Some(c) = json.get("components") {
        ctx.load_scene_assets(c)?;
    }
    Ok(Rc::new(file))
}

fn not_an_instance(e: Entity) -> Error {
    Error::new(
        ErrorKind::Invalid,
        format!(
            "entity {} isn't a prefab instance (placed from a prefab file in the editor)",
            e.to_bits()
        ),
    )
}

/// Compares instance `root` with its prefab file (see the module docs).
pub(crate) fn compare(ctx: &mut Ctx, root: Entity) -> Result<Comparison, Error> {
    let path = instance_path(&ctx.world, root).ok_or_else(|| not_an_instance(root))?;
    let inst = members(&ctx.world, root);
    // Before the copy exists: it hangs off the instance while compared.
    let attached = crate::scene::prefab_members(&ctx.world, root);
    let fresh_root = ctx.spawn_prefab(&path, position(&ctx.world, root))?;
    // Captured before the copy's links are pointed at the instance.
    let fresh: Vec<Entity> = members(&ctx.world, fresh_root).into_iter().flatten().collect();
    let out = compare_copy(ctx, &path, &inst, &fresh, attached);
    for e in fresh {
        ctx.world.despawn(e);
    }
    out
}

fn compare_copy(
    ctx: &mut Ctx,
    path: &str,
    inst: &[Option<Entity>],
    fresh: &[Entity],
    attached: Vec<Entity>,
) -> Result<Comparison, Error> {
    let world = &mut ctx.world;
    // The copy's references name the instance's entities from here on.
    let map: HashMap<Entity, Entity> = fresh
        .iter()
        .zip(inst)
        .filter_map(|(&f, &i)| i.map(|i| (f, i)))
        .collect();
    for &f in fresh {
        let Value::Object(components) = world.entity_json(f)? else {
            continue;
        };
        Entity::with_remap(&map, || {
            for (name, value) in components {
                if !LINKS.contains(&name.as_str()) {
                    world.set_json(f, &name, value)?;
                }
            }
            Ok::<_, crate::reflect::ReflectError>(())
        })?;
    }
    // The copy's attached parts follow the instance's root now.
    crate::hierarchy::settle(world);
    let mut out = Comparison {
        path: path.to_string(),
        ..Comparison::default()
    };
    for (member, (&f, &i)) in fresh.iter().zip(inst).enumerate() {
        let Some(i) = i else {
            out.missing += 1;
            continue;
        };
        let (Value::Object(want), Value::Object(have)) = (world.entity_json(f)?, world.entity_json(i)?) else {
            continue;
        };
        let skip = |name: &str| LINKS.contains(&name) || (member == 0 && PLACEMENT.contains(&name));
        let mut names: Vec<&String> = want.keys().chain(have.keys()).filter(|n| !skip(n)).collect();
        names.sort();
        names.dedup();
        for name in names {
            let (p, h) = (want.get(name), have.get(name));
            if matches!((p, h), (Some(a), Some(b)) if close(a, b)) {
                continue;
            }
            out.overrides.push(Override {
                member,
                entity: i,
                component: name.clone(),
                prefab: p.cloned(),
                instance: h.cloned(),
            });
        }
    }
    out.missing += fresh.len().saturating_sub(inst.len());
    // Parts beyond the prefab's, then what the scene attached.
    let own: HashSet<Entity> = inst.iter().flatten().copied().collect();
    out.extra_parts = inst.len().saturating_sub(fresh.len());
    out.added.extend(inst.iter().skip(fresh.len()).flatten().copied());
    out.added.extend(attached.into_iter().filter(|e| !own.contains(e)));
    Ok(out)
}

impl Editor {
    /// Puts the chosen overrides of instance `root` back as the prefab has
    /// them, as one undo step. `which` names them by (member, component).
    pub fn revert_overrides(&mut self, ctx: &mut Ctx, root: Entity, which: &[(usize, String)]) -> Result<(), Error> {
        let cmp = compare(ctx, root)?;
        let mark = self.undo.len();
        let chosen = cmp
            .overrides
            .iter()
            .filter(|o| which.iter().any(|(m, c)| *m == o.member && *c == o.component));
        for o in chosen {
            self.revert_one(ctx, o)?;
        }
        self.group_since(mark);
        Ok(())
    }

    fn revert_one(&mut self, ctx: &mut Ctx, o: &Override) -> Result<(), Error> {
        match &o.prefab {
            Some(value) => self.set(ctx, o.entity, &o.component, value.clone()),
            None => self.remove(ctx, o.entity, &o.component),
        }
    }

    /// Makes instance `root` exactly its prefab again, as one undo step:
    /// every override back, and what was added to it deleted. If parts were
    /// deleted from it, it is placed again (a new root, which is returned).
    pub fn revert_instance(&mut self, ctx: &mut Ctx, root: Entity) -> Result<Entity, Error> {
        let cmp = compare(ctx, root)?;
        let mark = self.undo.len();
        let root = if cmp.missing > 0 {
            self.replace_instance(ctx, root, &[])?
        } else {
            for o in &cmp.overrides {
                self.revert_one(ctx, o)?;
            }
            root
        };
        if !cmp.added.is_empty() {
            let added: Vec<Entity> = cmp.added.iter().copied().filter(|&e| ctx.world.is_alive(e)).collect();
            self.delete_many(ctx, &added)?;
        }
        self.group_since(mark);
        Ok(root)
    }

    /// Places prefab `root`'s file again where `root` stands, in its place:
    /// `root` and its parts are deleted (not what the scene attached to
    /// them, which hangs off the new ones), and `keep`'s instance values are
    /// written onto the new entities. Returns the new root.
    fn replace_instance(&mut self, ctx: &mut Ctx, root: Entity, keep: &[Override]) -> Result<Entity, Error> {
        let path = instance_path(&ctx.world, root).ok_or_else(|| not_an_instance(root))?;
        let at = placement(&ctx.world, root);
        let old = members(&ctx.world, root);
        let own: Vec<Entity> = old.iter().flatten().copied().collect();
        let extras: Vec<Entity> = crate::scene::prefab_members(&ctx.world, root)
            .into_iter()
            .filter(|e| !own.contains(e))
            .collect();
        let saved = snapshot(&ctx.world, &own)?;
        for &e in &own {
            ctx.world.despawn(e);
        }
        self.record(Undo::Despawned(saved));
        let new_root = self.spawn_prefab(ctx, &path, at)?;
        let new = members(&ctx.world, new_root);
        let map: HashMap<Entity, Entity> = old
            .iter()
            .zip(&new)
            .filter_map(|(o, n)| Some((o.as_ref().copied()?, n.as_ref().copied()?)))
            .collect();
        for o in keep {
            let Some(Some(e)) = new.get(o.member) else { continue };
            match &o.instance {
                Some(value) => Entity::with_remap(&map, || self.set(ctx, *e, &o.component, value.clone()))?,
                None if ctx.world.get_json(*e, &o.component).is_ok() => self.remove(ctx, *e, &o.component)?,
                None => {}
            }
        }
        // What the scene attached to the old entities hangs off the new ones.
        for x in extras {
            for name in ["Attached", "Attached3D"] {
                let Ok(mut v) = ctx.world.get_json(x, name) else {
                    continue;
                };
                let to = v["to"].as_u64().map(Entity::from_bits);
                if let Some(to) = to.and_then(|t| map.get(&t)) {
                    v["to"] = json!(to.to_bits());
                    self.set(ctx, x, name, v)?;
                }
            }
        }
        Ok(new_root)
    }

    /// Brings instance `inst` up to date with its prefab file, which
    /// changed: what differs from the new file and wasn't an override
    /// against the old one (`before`) takes the file's value. If the
    /// prefab's entities changed, the instance is placed again with its
    /// overrides; the new root is returned.
    fn sync_instance(&mut self, ctx: &mut Ctx, inst: Entity, before: &Comparison) -> Result<Entity, Error> {
        let now = compare(ctx, inst)?;
        if !now.same_parts() || !before.same_parts() {
            return self.replace_instance(ctx, inst, &before.overrides);
        }
        let kept: HashSet<(usize, &str)> = before
            .overrides
            .iter()
            .map(|o| (o.member, o.component.as_str()))
            .collect();
        for o in &now.overrides {
            if !kept.contains(&(o.member, o.component.as_str())) {
                self.revert_one(ctx, o)?;
            }
        }
        Ok(inst)
    }

    /// Writes instance `root` into its prefab file (its overrides, and what
    /// is attached to it, become the prefab), then brings every other
    /// instance in the world up to date, keeping their own overrides. The
    /// instances' changes are one undo step; the file isn't undone (like a
    /// save). Returns how many other instances there were.
    ///
    /// # Errors
    ///
    /// `Invalid` for an entity that isn't an instance, `Unsupported` for a
    /// prefab that includes other prefabs (open it and change it there),
    /// and the file's errors.
    pub fn apply_to_prefab(&mut self, ctx: &mut Ctx, root: Entity) -> Result<usize, Error> {
        let path = instance_path(&ctx.world, root).ok_or_else(|| not_an_instance(root))?;
        let (_, text) = ctx.assets.read_text(&path)?;
        let old: Value = serde_json::from_str(&text).map_err(|e| Error::from(e).with_path(&path))?;
        let nested = old["entities"]
            .as_array()
            .is_some_and(|list| list.iter().any(|e| e.get("prefab").is_some()));
        if nested {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("{path} includes other prefabs: open it and change it there"),
            )
            .with_path(&path));
        }
        let others: Vec<Entity> = instances(&ctx.world, &path)
            .into_iter()
            .filter(|&e| e != root)
            .collect();
        let mut befores = Vec::with_capacity(others.len());
        for &o in &others {
            befores.push(compare(ctx, o)?);
        }

        let mine = compare(ctx, root)?;
        let mut written: Vec<Entity> = members(&ctx.world, root).into_iter().flatten().collect();
        for &e in &mine.added {
            if !written.contains(&e) {
                written.push(e);
            }
        }
        let file = prefab_json(&ctx.world, root, &written)?;
        let bytes = serde_json::to_string_pretty(&file).map_err(Error::from)?;
        crate::assets::write_asset(&path, bytes.as_bytes())?;
        ctx.assets.prefabs.remove(&path);

        let mark = self.undo.len();
        // Its parts are the file's entities now, in the file's order.
        let parts: Vec<u64> = written[1..].iter().map(|e| e.to_bits()).collect();
        if !parts.is_empty() || ctx.world.get::<PrefabParts>(root).is_some() {
            self.set(ctx, root, "PrefabParts", json!(parts))?;
        }
        for (o, before) in others.iter().zip(&befores) {
            self.sync_instance(ctx, *o, before)?;
        }
        self.group_since(mark);
        Ok(others.len())
    }

    /// Prefab `path` was changed and saved; `old` is what its file held
    /// before. Brings every instance of it up to date, keeping their
    /// overrides: in the world, then in each scene file in `assets/` that
    /// has one (opened, updated, saved). The open document is opened again
    /// afterwards, so it must be saved first. Returns the scene files
    /// written.
    pub fn apply_prefab_to_scenes(&mut self, ctx: &mut Ctx, path: &str, old: &Value) -> Result<Vec<String>, Error> {
        if self.dirty() || self.doc.path.is_none() {
            return Err(Error::new(
                ErrorKind::Invalid,
                "save the open document first: the scenes are opened in turn and it is opened again after",
            ));
        }
        let old = parse_prefab(ctx, path, old)?;
        let doc = self.doc.path.clone().unwrap_or_default();
        let mut written = Vec::new();
        let scenes = super::browse(ctx).scenes;
        let result = (|| {
            let gone = |e: &Error| e.kind() == ErrorKind::NotFound;
            for scene in scenes {
                // A file deleted since the folder was listed is skipped.
                let text = match ctx.assets.read_text(&scene) {
                    Ok((_, text)) => text,
                    Err(e) if gone(&e) => continue,
                    Err(e) => return Err(e),
                };
                if !text.contains("PrefabInstance") || !text.contains(path) {
                    continue;
                }
                match self.open(ctx, &scene) {
                    Ok(_) => {}
                    Err(e) if gone(&e) => continue,
                    Err(e) => return Err(e),
                }
                let roots = instances(&ctx.world, path);
                let befores = with_prefab(ctx, path, Some(old.clone()), |ctx| {
                    roots.iter().map(|&r| compare(ctx, r)).collect::<Result<Vec<_>, _>>()
                })?;
                for (r, before) in roots.iter().zip(&befores) {
                    self.sync_instance(ctx, *r, before)?;
                }
                if self.dirty() {
                    self.save(ctx)?;
                    written.push(scene);
                }
            }
            Ok::<_, Error>(())
        })();
        self.open(ctx, &doc)?;
        result.map(|()| written)
    }
}

/// `written` (the root first) as a prefab file, the parts relative to the
/// root, the root at the origin.
fn prefab_json(world: &ponko_ecs::World, root: Entity, written: &[Entity]) -> Result<Value, Error> {
    let mut entities = Vec::with_capacity(written.len());
    for &m in written {
        let mut c = world.entity_json(m)?;
        if m == root {
            if c.get("Transform2D").is_some() {
                c["Transform2D"]["position"] = json!([0.0, 0.0]);
            }
            if c.get("Transform3D").is_some() {
                c["Transform3D"]["position"] = json!([0.0, 0.0, 0.0]);
            }
        } else {
            crate::scene::relative_to(world, m, root, &mut c);
        }
        if let Value::Object(map) = &mut c {
            for link in NOT_WRITTEN {
                map.remove(link);
            }
        }
        entities.push(json!({ "entity": m.to_bits(), "components": c }));
    }
    Ok(if let [one] = &entities[..] {
        json!({ "format": crate::scene::PREFAB_FORMAT, "version": crate::scene::PREFAB_VERSION, "components": one["components"] })
    } else {
        json!({ "format": crate::scene::PREFAB_FORMAT, "version": crate::scene::PREFAB_TREE_VERSION, "entities": entities })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::Transform as T2;
    use glam::{Vec2, vec2};
    use ponko_ecs::Component;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    #[serde(default)]
    struct Hp {
        now: f32,
        max: f32,
    }

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    #[serde(default)]
    struct Glow(f32);

    /// Asset files unique to this test run, removed when dropped.
    struct TempAssets(Vec<std::path::PathBuf>);

    impl TempAssets {
        fn name(what: &str) -> String {
            format!("ponko-test-prefab-{what}-{}.json", std::process::id())
        }

        fn write(&mut self, name: &str, json: &Value) {
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(name);
            std::fs::write(&file, serde_json::to_string_pretty(json).unwrap()).unwrap();
            self.0.push(file);
        }
    }

    impl Drop for TempAssets {
        fn drop(&mut self) {
            for f in &self.0 {
                let _ = std::fs::remove_file(f);
            }
        }
    }

    /// A cart (Hp 10/10) with a wheel attached 20 to its right (Glow 1).
    fn cart() -> Value {
        json!({
            "format": "ponko-prefab",
            "version": 2,
            "entities": [
                { "entity": 1, "components": {
                    "Transform2D": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                    "Hp": { "now": 10.0, "max": 10.0 } } },
                { "entity": 2, "components": {
                    "Transform2D": { "position": [20.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                    "Attached": { "to": 1, "offset": [20.0, 0.0], "rotation": 0.0 },
                    "Glow": 1.0 } }
            ]
        })
    }

    fn game() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
        ctx.world.reflect::<Hp>();
        ctx.world.reflect::<Glow>();
        ctx.time.pause();
        ctx.edit(|_, _| ());
        ctx
    }

    fn place(ctx: &mut Ctx, path: &str, at: Vec2) -> Entity {
        ctx.edit(|ed, ctx| ed.spawn_prefab(ctx, path, at)).unwrap()
    }

    fn wheel(ctx: &Ctx, root: Entity) -> Entity {
        ctx.world.get::<PrefabParts>(root).unwrap().0[0]
    }

    fn overrides(ctx: &mut Ctx, root: Entity) -> Vec<(usize, String, OverrideKind)> {
        compare(ctx, root)
            .unwrap()
            .overrides
            .iter()
            .map(|o| (o.member, o.component.clone(), o.kind()))
            .collect()
    }

    #[test]
    fn a_placed_prefab_is_an_instance_without_overrides_wherever_it_is_moved() {
        let mut files = TempAssets(Vec::new());
        let path = TempAssets::name("clean");
        files.write(&path, &cart());
        let mut ctx = game();
        let root = place(&mut ctx, &path, vec2(100.0, 50.0));
        assert_eq!(instance_path(&ctx.world, root), Some(path.clone()));
        let w = wheel(&ctx, root);
        assert_eq!(instance_root(&ctx.world, w), Some(root), "a part finds its instance");
        assert!(compare(&mut ctx, root).unwrap().is_clean());

        let moved = T2 {
            position: vec2(-300.0, 7.5),
            rotation: 0.7,
            scale: Vec2::splat(2.0),
        };
        ctx.edit(|ed, ctx| ed.set_transforms2d(ctx, &[(root, moved)]));
        let cmp = compare(&mut ctx, root).unwrap();
        assert!(cmp.is_clean(), "where it stands is its own: {cmp:#?}");
        let n = ctx.world.len();
        let _ = compare(&mut ctx, root).unwrap();
        assert_eq!(ctx.world.len(), n, "the copy compared with is gone again");
    }

    #[test]
    fn changed_added_and_removed_components_are_overrides_and_revert_puts_them_back_as_one_step() {
        let mut files = TempAssets(Vec::new());
        let path = TempAssets::name("overrides");
        files.write(&path, &cart());
        let mut ctx = game();
        let root = place(&mut ctx, &path, vec2(100.0, 50.0));
        let w = wheel(&ctx, root);
        ctx.edit(|ed, ctx| ed.set(ctx, root, "Hp", json!({ "now": 3.0, "max": 10.0 })))
            .unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, root, "Glow", json!(2.0))).unwrap();
        ctx.edit(|ed, ctx| ed.remove(ctx, w, "Glow")).unwrap();
        assert_eq!(
            overrides(&mut ctx, root),
            [
                (0, "Glow".into(), OverrideKind::Added),
                (0, "Hp".into(), OverrideKind::Changed),
                (1, "Glow".into(), OverrideKind::Removed),
            ]
        );
        let cmp = compare(&mut ctx, root).unwrap();
        let hp = cmp.overrides.iter().find(|o| o.component == "Hp").unwrap();
        assert_eq!(hp.fields(), ["now"], "the field that differs");

        ctx.edit(|ed, ctx| ed.revert_overrides(ctx, root, &[(0, "Hp".into()), (1, "Glow".into())]))
            .unwrap();
        assert_eq!(ctx.world.get::<Hp>(root), Some(&Hp { now: 10.0, max: 10.0 }));
        assert_eq!(ctx.world.get::<Glow>(w), Some(&Glow(1.0)));
        assert_eq!(overrides(&mut ctx, root), [(0, "Glow".into(), OverrideKind::Added)]);

        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        assert_eq!(overrides(&mut ctx, root).len(), 3, "one undo brings both back");
    }

    #[test]
    fn revert_instance_deletes_what_was_attached_and_places_a_part_again() {
        let mut files = TempAssets(Vec::new());
        let path = TempAssets::name("revert-all");
        files.write(&path, &cart());
        let mut ctx = game();
        let root = place(&mut ctx, &path, vec2(100.0, 50.0));
        ctx.edit(|ed, ctx| ed.set(ctx, root, "Hp", json!({ "now": 1.0, "max": 1.0 })))
            .unwrap();
        let lamp = ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(100.0, 80.0)));
        ctx.edit(|ed, ctx| ed.set_parent(ctx, lamp, Some(root))).unwrap();
        assert_eq!(compare(&mut ctx, root).unwrap().added, [lamp]);
        let n = ctx.world.len();
        let same = ctx.edit(|ed, ctx| ed.revert_instance(ctx, root)).unwrap();
        assert_eq!(same, root, "nothing was deleted from it: the same entities");
        assert!(compare(&mut ctx, root).unwrap().is_clean());
        assert!(!ctx.world.is_alive(lamp));
        assert_eq!(ctx.world.len(), n - 1);

        // A deleted part: the instance is placed again, where it was.
        let w = wheel(&ctx, root);
        ctx.world.despawn(w);
        assert_eq!(compare(&mut ctx, root).unwrap().missing, 1);
        let new = ctx.edit(|ed, ctx| ed.revert_instance(ctx, root)).unwrap();
        assert!(compare(&mut ctx, new).unwrap().is_clean());
        assert_eq!(ctx.world.get::<T2>(new).unwrap().position, vec2(100.0, 50.0));
        assert!(!ctx.world.is_alive(root));
    }

    #[test]
    fn apply_writes_the_instance_into_the_prefab_and_other_instances_keep_their_own_overrides() {
        let mut files = TempAssets(Vec::new());
        let path = TempAssets::name("apply");
        files.write(&path, &cart());
        let mut ctx = game();
        let a = place(&mut ctx, &path, vec2(100.0, 50.0));
        let b = place(&mut ctx, &path, vec2(400.0, 50.0));
        let c = place(&mut ctx, &path, vec2(700.0, 50.0));
        // b has its own Hp; a gets a brighter wheel and a new component.
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Hp", json!({ "now": 2.0, "max": 10.0 })))
            .unwrap();
        let wa = wheel(&ctx, a);
        ctx.edit(|ed, ctx| ed.set(ctx, wa, "Glow", json!(5.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, a, "Hp", json!({ "now": 20.0, "max": 20.0 })))
            .unwrap();

        let others = ctx.edit(|ed, ctx| ed.apply_to_prefab(ctx, a)).unwrap();
        assert_eq!(others, 2);
        assert!(compare(&mut ctx, a).unwrap().is_clean(), "a is the prefab now");
        let file: Value = serde_json::from_str(&ctx.assets.read_text(&path).unwrap().1).unwrap();
        assert_eq!(
            file["entities"][0]["components"]["Transform2D"]["position"],
            json!([0.0, 0.0])
        );
        assert!(file["entities"][0]["components"].get("PrefabInstance").is_none());

        assert!(compare(&mut ctx, c).unwrap().is_clean(), "c follows the prefab");
        assert_eq!(ctx.world.get::<Glow>(wheel(&ctx, c)), Some(&Glow(5.0)));
        assert_eq!(ctx.world.get::<Hp>(c), Some(&Hp { now: 20.0, max: 20.0 }));
        assert_eq!(
            ctx.world.get::<Hp>(b),
            Some(&Hp { now: 2.0, max: 10.0 }),
            "b's own Hp is kept"
        );
        assert_eq!(
            ctx.world.get::<Glow>(wheel(&ctx, b)),
            Some(&Glow(5.0)),
            "the rest follows"
        );
        assert_eq!(
            ctx.world.get::<T2>(c).unwrap().position,
            vec2(700.0, 50.0),
            "placement stays"
        );

        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        assert_eq!(
            ctx.world.get::<Hp>(c),
            Some(&Hp { now: 10.0, max: 10.0 }),
            "the instances' update is one undo step"
        );
    }

    #[test]
    fn a_part_attached_to_an_instance_and_applied_is_placed_on_the_others() {
        let mut files = TempAssets(Vec::new());
        let path = TempAssets::name("apply-part");
        files.write(&path, &cart());
        let mut ctx = game();
        let a = place(&mut ctx, &path, vec2(100.0, 50.0));
        let b = place(&mut ctx, &path, vec2(400.0, 50.0));
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Hp", json!({ "now": 2.0, "max": 10.0 })))
            .unwrap();
        let lamp = ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(100.0, 80.0)));
        ctx.edit(|ed, ctx| ed.set(ctx, lamp, "Glow", json!(9.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set_parent(ctx, lamp, Some(a))).unwrap();
        ctx.edit(|ed, ctx| ed.apply_to_prefab(ctx, a)).unwrap();

        assert_eq!(
            ctx.world.get::<PrefabParts>(a).unwrap().0.len(),
            2,
            "the lamp is a part now"
        );
        assert!(compare(&mut ctx, a).unwrap().is_clean());
        let b = instances(&ctx.world, &path).into_iter().find(|&e| e != a).unwrap();
        let cmp = compare(&mut ctx, b).unwrap();
        assert_eq!(cmp.missing, 0, "b was placed again with three entities");
        assert_eq!(
            cmp.overrides.iter().map(|o| o.component.as_str()).collect::<Vec<_>>(),
            ["Hp"],
            "with its own Hp"
        );
        let parts = &ctx.world.get::<PrefabParts>(b).unwrap().0;
        let lamp_b = parts[1];
        assert_eq!(ctx.world.get::<Glow>(lamp_b), Some(&Glow(9.0)));
        let at = ctx.world.get::<T2>(lamp_b).unwrap().position;
        assert!((at - vec2(400.0, 80.0)).length() < 1e-3, "{at}");
    }

    #[test]
    fn a_saved_prefab_brings_the_instances_in_scene_files_up_to_date() {
        let mut files = TempAssets(Vec::new());
        let path = TempAssets::name("scenes-prefab");
        let scene = TempAssets::name("scenes-level");
        files.write(&path, &cart());
        let mut ctx = game();
        let a = place(&mut ctx, &path, vec2(100.0, 50.0));
        let b = place(&mut ctx, &path, vec2(400.0, 50.0));
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Hp", json!({ "now": 2.0, "max": 10.0 })))
            .unwrap();
        ctx.edit(|ed, ctx| ed.save_as(ctx, &scene, crate::editor::DocKind::Scene))
            .unwrap();
        files.0.push(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&scene),
        );

        // The prefab changes on its own (opened alone and saved).
        let old = cart();
        let mut new = cart();
        new["entities"][0]["components"]["Hp"] = json!({ "now": 50.0, "max": 50.0 });
        new["entities"][1]["components"]["Glow"] = json!(3.0);
        files.write(&path, &new);
        ctx.assets.prefabs.remove(&path);

        let written = ctx.edit(|ed, ctx| ed.apply_prefab_to_scenes(ctx, &path, &old)).unwrap();
        assert_eq!(written, std::slice::from_ref(&scene));
        assert_eq!(
            ctx.editor.doc.path.as_deref(),
            Some(scene.as_str()),
            "the document is open again"
        );
        assert!(!ctx.editor.dirty());
        let roots = instances(&ctx.world, &path);
        assert_eq!(roots.len(), 2);
        let hp: Vec<Hp> = roots.iter().map(|&r| *ctx.world.get::<Hp>(r).unwrap()).collect();
        assert!(hp.contains(&Hp { now: 50.0, max: 50.0 }), "{hp:?}");
        assert!(hp.contains(&Hp { now: 2.0, max: 10.0 }), "the override is kept: {hp:?}");
        for r in roots {
            assert_eq!(ctx.world.get::<Glow>(wheel(&ctx, r)), Some(&Glow(3.0)));
        }
        let _ = a;
    }

    #[test]
    fn apply_refuses_a_prefab_that_includes_another() {
        let mut files = TempAssets(Vec::new());
        let inner = TempAssets::name("inner");
        let outer = TempAssets::name("outer");
        files.write(&inner, &cart());
        files.write(
            &outer,
            &json!({ "format": "ponko-prefab", "version": 2, "entities": [
                { "entity": 1, "components": { "Hp": { "now": 1.0, "max": 1.0 } } },
                { "entity": 2, "prefab": inner, "components": {} }
            ]}),
        );
        let mut ctx = game();
        let root = place(&mut ctx, &outer, vec2(0.0, 0.0));
        let cmp = compare(&mut ctx, root).unwrap();
        assert!(cmp.is_clean(), "nested prefabs compare: {cmp:#?}");
        let err = ctx.edit(|ed, ctx| ed.apply_to_prefab(ctx, root)).unwrap_err();
        assert!(err.to_string().contains("includes other prefabs"), "{err}");
    }

    #[test]
    fn an_entity_that_was_not_placed_from_a_prefab_is_not_an_instance() {
        let mut ctx = game();
        let e = ctx.world.spawn((T2::default(),));
        assert!(
            compare(&mut ctx, e)
                .unwrap_err()
                .to_string()
                .contains("isn't a prefab instance")
        );
    }
}
