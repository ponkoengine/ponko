//! glTF files as game scenes: every node of the file an entity, with the
//! components, colliders, lights, spawn points, prefabs and levels of
//! detail its custom properties ask for (the convention is in
//! [`gltf_props`](super::gltf_props)), kept up to date while the file is
//! exported again.

#![deny(missing_docs)]

use glam::{Mat4, Vec3};
use ponko_ecs::{Component, Entity};
use serde_json::{Value, json};

use super::gltf::{LoadedNode, load_nodes};
#[cfg(feature = "physics3d")]
use super::gltf_props::NodeCollider;
use super::gltf_props::{NodeBody, NodeProperties};
use super::{Mesh, MeshData, ModelLight, ModelLightKind, Transform3D};
use crate::attach::{Attached3D, compose3d};
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};
use crate::hot_reload::Asset;
use crate::reflect::{JsonMap, NotSaved, WorldReflect};

/// How [`GltfRoot::spawn`] spawns a file.
///
/// ```
/// let options = ponko::d3::GltfOptions::default().without_saved_objects();
/// assert!(options.no_saved_objects);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct GltfOptions {
    /// Leave out the prefab nodes (`ponko:prefab`): after loading a world
    /// save, which holds them already, spawn the file again with this.
    pub no_saved_objects: bool,
}

impl GltfOptions {
    /// The same options, leaving out the prefab nodes (see
    /// [`no_saved_objects`](Self::no_saved_objects)).
    #[must_use]
    pub fn without_saved_objects(self) -> Self {
        Self { no_saved_objects: true }
    }
}

/// What [`GltfRoot::spawn`] spawned for one node.
#[derive(Debug, Clone)]
struct SpawnedNode {
    path: String,
    entity: Entity,
    /// The prefab it is an instance of.
    prefab: Option<String>,
    /// The components its properties named (`ponko:<Component>`).
    components: Vec<String>,
}

/// The root of a glTF file spawned with [`Ctx::spawn_prefab`] or
/// [`GltfRoot::spawn`]: its
/// [`Transform`](crate::d3::Transform) places the whole file, and it
/// knows the entity of every node, by path.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) -> Result<(), Error> {
/// let root = ctx.spawn_prefab("levels/yard.glb", vec3(0.0, 0.0, 0.0))?;
/// let door = ctx.world.get::<ponko::d3::GltfRoot>(root).and_then(|r| r.entity("House/Door"));
/// # let _ = door; Ok(()) }
/// ```
#[derive(Debug, Clone, Component)]
pub struct GltfRoot {
    /// The file, as it was spawned.
    pub path: String,
    /// The options it was spawned with.
    pub options: GltfOptions,
    nodes: Vec<SpawnedNode>,
    /// The meshes made for its nodes, freed with it.
    meshes: Vec<Mesh>,
    /// The collision shapes made for its nodes, freed with it.
    #[cfg(feature = "physics3d")]
    shapes: Vec<crate::d3::CollisionMesh>,
}

impl GltfRoot {
    /// The entity of the node at `path` (names from the top of the file
    /// down, joined with `/`, as in [`GltfNode::path`]), or of the first
    /// node named `path`.
    #[must_use]
    pub fn entity(&self, path: &str) -> Option<Entity> {
        let by_path = self.nodes.iter().find(|n| n.path == path);
        let by_name = || self.nodes.iter().find(|n| n.path.rsplit('/').next() == Some(path));
        by_path.or_else(by_name).map(|n| n.entity)
    }

    /// Every node's path and entity, parents before their children.
    pub fn entities(&self) -> impl Iterator<Item = (&str, Entity)> {
        self.nodes.iter().map(|n| (n.path.as_str(), n.entity))
    }
}

/// The node of a glTF file an entity was spawned from by
/// [`GltfRoot::spawn`]: its name, its path and every custom property it
/// has (the game's own included), for systems to find nodes and read
/// their data.
///
/// ```
/// # use ponko::prelude::*;
/// fn doors(ctx: &mut Ctx) {
///     for node in ctx.world.query::<&ponko::d3::GltfNode>().iter() {
///         if node.extras.get("door").is_some() {
///             // ...
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Component)]
pub struct GltfNode {
    /// The node's name (empty if it has none).
    pub name: String,
    /// The names of the nodes from the top of the file down to it, joined
    /// with `/`. A node without a name is `#<n>`, its place among its
    /// siblings; a second node of the same name gets `#2`, and so on.
    /// Hot reload keeps an entity while its path stays.
    pub path: String,
    /// The node's `extras`: every custom property, as the file has them.
    pub extras: JsonMap,
}

/// A named place to spawn things, from a node with `ponko:spawn`. Its
/// [`Transform`](crate::d3::Transform) is the node's.
///
/// ```
/// # use ponko::prelude::*;
/// fn player_start(world: &mut World) -> Option<ponko::d3::Transform> {
///     world
///         .query::<(&ponko::d3::Transform, &ponko::d3::SpawnPoint)>()
///         .iter()
///         .find(|(_, p)| p.name == "player")
///         .map(|(t, _)| *t)
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Component)]
pub struct SpawnPoint {
    /// Its name: the value of `ponko:spawn`, or the node's name.
    pub name: String,
}

/// One node of a [`GltfScene`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GltfSceneNode {
    /// Its name (empty if it has none).
    pub name: String,
    /// Its path, as [`GltfNode::path`].
    pub path: String,
    /// Its parent, an index into [`GltfScene::nodes`].
    pub parent: Option<usize>,
    /// Where it is in its parent's space.
    pub transform: Transform3D,
    /// Where it is in the file's space.
    pub world: Transform3D,
    /// What its custom properties ask for.
    pub properties: NodeProperties,
    /// Every custom property, as the file has them.
    pub extras: JsonMap,
    /// Its own mesh, in its own space.
    pub mesh: Option<MeshData>,
    /// Its light, relative to the node.
    pub light: Option<ModelLight>,
}

/// A glTF file read node by node and checked, without spawning anything:
/// for tools, for navigation meshes, or to place things your own way. See
/// [`GltfScene::load`].
#[derive(Debug, Clone)]
pub struct GltfScene {
    /// Every node of the file's scene, parents before their children.
    pub nodes: Vec<GltfSceneNode>,
}

impl GltfScene {
    /// The node at `path`, or the first one named `path`.
    #[must_use]
    pub fn node(&self, path: &str) -> Option<&GltfSceneNode> {
        let by_path = self.nodes.iter().find(|n| n.path == path);
        by_path.or_else(|| self.nodes.iter().find(|n| n.name == path))
    }

    /// The spawn points (`ponko:spawn`), by name, where they are in the
    /// file's space.
    pub fn spawn_points(&self) -> impl Iterator<Item = (&str, Transform3D)> {
        self.nodes
            .iter()
            .filter_map(|n| n.properties.spawn.as_ref().map(|_| (spawn_name(n), n.world)))
    }

    /// Whether `node`'s mesh is ground for navigation meshes: as its
    /// `ponko:navmesh` says, else when it is drawn as level geometry (not
    /// a spawn point, prefab stand-in, moving body or sensor). Of a level
    /// of detail group, only the first level counts.
    #[cfg(feature = "navmesh")]
    fn walkable(&self, i: usize) -> bool {
        let n = &self.nodes[i];
        if let Some(parent) = n.parent {
            let group = &self.nodes[parent];
            if !group.properties.lod.is_empty() {
                return lod_levels(
                    self.nodes.iter().map(|n| (n.parent, n.name.as_str(), n.mesh.is_some())),
                    parent,
                )
                .first()
                    == Some(&i)
                    && self.walkable(parent);
            }
        }
        let p = &n.properties;
        p.navmesh.unwrap_or_else(|| {
            p.spawn.is_none() && p.prefab.is_none() && !p.sensor && matches!(p.body, None | Some(NodeBody::Fixed))
        })
    }

    /// The triangles of every walkable mesh (see `ponko:navmesh`), in the
    /// file's space, to bake a navigation mesh from.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) -> Result<(), Box<dyn std::error::Error>> {
    /// let scene = ponko::d3::GltfScene::load(ctx, "levels/yard.glb")?;
    /// let nav = ponko::d3::NavMesh::bake(&[scene.nav_source()], ponko::nav::NavBakeSettings::default())?;
    /// # let _ = nav; Ok(()) }
    /// ```
    #[cfg(feature = "navmesh")]
    #[must_use]
    pub fn nav_source(&self) -> crate::nav::NavSource {
        let mut out = crate::nav::NavSource::default();
        for (i, n) in self.nodes.iter().enumerate() {
            let Some(mesh) = &n.mesh else { continue };
            if self.walkable(i) {
                out.triangles
                    .extend(crate::nav::NavSource::from_mesh(mesh, n.world).triangles);
            }
        }
        out
    }
}

/// A spawn point's name: its `ponko:spawn`, or the node's name.
fn spawn_name(n: &GltfSceneNode) -> &str {
    match n.properties.spawn.as_deref() {
        Some("") | None => &n.name,
        Some(name) => name,
    }
}

/// The levels of the level-of-detail group `group`: its children with
/// meshes, by name. `nodes` gives each node's parent, name and whether it
/// has a mesh.
fn lod_levels<'a>(nodes: impl Iterator<Item = (Option<usize>, &'a str, bool)>, group: usize) -> Vec<usize> {
    let mut levels: Vec<(usize, &str)> = nodes
        .enumerate()
        .filter(|(_, (parent, _, mesh))| *parent == Some(group) && *mesh)
        .map(|(i, (_, name, _))| (i, name))
        .collect();
    levels.sort_by(|a, b| a.1.cmp(b.1));
    levels.into_iter().map(|(i, _)| i).collect()
}

/// A file's nodes, read and checked: paths, transforms in the file's
/// space, properties and which nodes are levels of a group.
struct Plan {
    nodes: Vec<Planned>,
}

struct Planned {
    name: String,
    path: String,
    parent: Option<usize>,
    local: Transform3D,
    /// In the file's space.
    world: Transform3D,
    props: NodeProperties,
    extras: JsonMap,
    /// The levels of its level-of-detail group (indices of its children).
    levels: Vec<usize>,
    /// Whether it is a level of its parent's group (no entity of its own).
    is_level: bool,
}

/// An error about node `node` of file `path`.
fn node_error(kind: ErrorKind, path: &str, node: &str, message: impl std::fmt::Display) -> Error {
    Error::new(kind, format!("{path}: node \"{node}\": {message}")).with_path(path)
}

impl Plan {
    fn of(path: &str, nodes: &[LoadedNode], components: &[&str]) -> Result<Self, Error> {
        let mut planned: Vec<Planned> = Vec::with_capacity(nodes.len());
        for (i, node) in nodes.iter().enumerate() {
            let name = node.name.clone().unwrap_or_default();
            let siblings = nodes[..i].iter().filter(|n| n.parent == node.parent);
            let own = if name.is_empty() {
                format!("#{}", siblings.count())
            } else {
                match siblings.filter(|n| n.name.as_deref() == Some(&name)).count() {
                    0 => name.clone(),
                    k => format!("{name}#{}", k + 1),
                }
            };
            let (node_path, world) = match node.parent {
                Some(p) => (
                    format!("{}/{own}", planned[p].path),
                    compose3d(&planned[p].world, &node.local),
                ),
                None => (own, node.local),
            };
            let extras = match &node.extras {
                Some(Value::Object(map)) => map.clone(),
                _ => JsonMap::new(),
            };
            let props = NodeProperties::read(&Value::Object(extras.clone()), components)
                .map_err(|e| node_error(ErrorKind::Decode, path, &node_path, e))?;
            planned.push(Planned {
                name,
                path: node_path,
                parent: node.parent,
                local: node.local,
                world,
                props,
                extras,
                levels: Vec::new(),
                is_level: false,
            });
        }
        for i in 0..planned.len() {
            if planned[i].props.lod.is_empty() {
                continue;
            }
            let levels = lod_levels(
                planned
                    .iter()
                    .zip(nodes)
                    .map(|(p, n)| (p.parent, p.name.as_str(), n.mesh.is_some())),
                i,
            );
            let wanted = planned[i].props.lod.len();
            if levels.len() != wanted {
                let message = format!(
                    "\"ponko:lod\" gives {wanted} screen heights, but the node has {} children with meshes (its levels)",
                    levels.len()
                );
                return Err(node_error(ErrorKind::Decode, path, &planned[i].path, message));
            }
            for &l in &levels {
                if planned[l].props != NodeProperties::default() {
                    let message = "a level of detail takes no ponko: properties; put them on its group (the parent)";
                    return Err(node_error(ErrorKind::Decode, path, &planned[l].path, message));
                }
                planned[l].is_level = true;
            }
            planned[i].levels = levels;
        }
        Ok(Self { nodes: planned })
    }

    /// The mesh a node's collider and size come from: its own, or its
    /// group's first level.
    #[cfg(feature = "physics3d")]
    fn shape_source(&self, i: usize) -> usize {
        match self.nodes[i].levels.first() {
            Some(&first) => first,
            None => i,
        }
    }
}

/// The box around `positions`: its center and size.
fn bounds(positions: &[Vec3]) -> (Vec3, Vec3) {
    let (min, max) = positions
        .iter()
        .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });
    ((min + max) * 0.5, max - min)
}

/// What a collider of node `i` will be, made before anything changes.
#[cfg(feature = "physics3d")]
struct ColliderPlan {
    node: usize,
    collider: crate::d3::Collider,
    body: crate::d3::RigidBody,
}

impl GltfRoot {
    /// Spawns the glTF file at `path` (`.glb`, or `.gltf` with its files)
    /// as a scene placed at `at`: a root entity with a [`GltfRoot`], and an
    /// entity for every node of the file's scene, attached to its parent
    /// (the root for top-level nodes, see [the hierarchy](crate::hierarchy)),
    /// with a [`Transform`](crate::d3::Transform), a [`GltfNode`] and:
    ///
    /// - a [`Model`](crate::render::Model) of the node's own mesh, with the
    ///   file's materials;
    /// - its light (`KHR_lights_punctual`): a point or spot light on the
    ///   entity, or the scene's sun;
    /// - what its custom properties ask for: reflected components by name
    ///   with JSON values, colliders made from its mesh (with the
    ///   `physics3d` feature), spawn points, prefabs, level-of-detail
    ///   groups and render layers. The guide's "From Blender to the game"
    ///   lists them.
    ///
    /// Everything but the prefab instances carries
    /// [`NotSaved`]: world saves hold the prefab instances (the game's
    /// state), the file rebuilds the rest. To load such a save, load it,
    /// then spawn the file again with
    /// [`GltfOptions::without_saved_objects`].
    ///
    /// With [hot reload](crate::game::Game::hot_reload) on, exporting the
    /// file again updates the scene in place: nodes whose path is still in
    /// the file keep their entity (and what the game added to it), with
    /// their transform, mesh, materials and properties from the new file;
    /// new nodes are spawned and removed ones despawned. A file that
    /// doesn't load keeps the old scene and prints why.
    /// [`Assets::reloaded_files`](crate::assets::Assets::reloaded_files)
    /// lists `path` that frame.
    ///
    /// [`Ctx::spawn_prefab`] spawns a `.glb` or `.gltf` file this way with
    /// `GltfOptions::default()`, and [`Ctx::despawn_prefab`] despawns the
    /// scene; this call takes options.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// let options = ponko::d3::GltfOptions::default().without_saved_objects();
    /// ponko::d3::GltfRoot::spawn(ctx, "levels/yard.glb", ponko::d3::Transform::default(), options).expect("level");
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] if the file (or a prefab it names) is
    /// missing, [`Decode`](ErrorKind::Decode) for a file that isn't a glTF
    /// model this engine reads and for custom properties that break the
    /// convention (an unknown name, with the closest known ones), and
    /// [`Json`](ErrorKind::Json) for a component value that doesn't fit its
    /// type. The error's path is `path` and its message names the node.
    /// Nothing is spawned then.
    pub fn spawn(ctx: &mut Ctx, path: &str, at: Transform3D, options: GltfOptions) -> Result<Entity, Error> {
        ctx.spawn_gltf(path, at, options)
    }
}

impl GltfScene {
    /// Reads the glTF file at `path` node by node and checks its custom
    /// properties against this game's reflected components, without
    /// spawning or drawing anything (images aren't decoded): for tools,
    /// navigation meshes (`GltfScene::nav_source`, with `navmesh`) or your own spawning.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) -> Result<(), Error> {
    /// let scene = ponko::d3::GltfScene::load(ctx, "levels/yard.glb")?;
    /// for (name, at) in scene.spawn_points() {
    ///     println!("{name} at {}", at.position);
    /// }
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// As [`GltfRoot::spawn`], except for component
    /// values and prefabs, which are only checked when spawned.
    pub fn load(ctx: &Ctx, path: &str) -> Result<GltfScene, Error> {
        let found = ctx.assets.asset_sources.find(path)?;
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let sources = &ctx.assets.asset_sources;
        let loaded =
            load_nodes(path, &bytes, &mut |name| sources.read(name), &|_| true).map_err(|e| Error::decode(path, e))?;
        let components = ctx.world.reflected_components();
        let plan = Plan::of(path, &loaded.nodes, &components)?;
        let nodes = plan
            .nodes
            .into_iter()
            .zip(loaded.nodes)
            .map(|(p, n)| GltfSceneNode {
                name: p.name,
                path: p.path,
                parent: p.parent,
                transform: p.local,
                world: p.world,
                properties: p.props,
                extras: p.extras,
                mesh: n.mesh.map(|m| m.data),
                light: n.light,
            })
            .collect();
        Ok(GltfScene { nodes })
    }
}

impl Ctx {
    /// [`GltfRoot::spawn`].
    pub(crate) fn spawn_gltf(&mut self, path: &str, at: Transform3D, options: GltfOptions) -> Result<Entity, Error> {
        let (loaded, meshes, file) = self.gfx.load_node_meshes(path, false)?;
        let root = self.world.spawn((
            at,
            GltfRoot {
                path: path.to_string(),
                options,
                nodes: Vec::new(),
                meshes: Vec::new(),
                #[cfg(feature = "physics3d")]
                shapes: Vec::new(),
            },
            NotSaved,
        ));
        if let Err(e) = self.apply_gltf(root, path, &loaded.nodes, meshes) {
            self.world.despawn(root);
            return Err(e);
        }
        if let Some(file) = file
            && !self.assets.watcher.watching(&file, |a| matches!(a, Asset::GltfScene))
        {
            self.assets.watcher.watch(path, file, Asset::GltfScene);
        }
        Ok(root)
    }

    /// Despawns the scene whose root is `root` (see [`GltfRoot::spawn`]):
    /// the root, every node's entity (prefab instances with their parts)
    /// and the meshes and collision shapes made for it. Returns how many
    /// entities that was, `None` if `root` isn't a scene's root.
    pub(crate) fn despawn_gltf(&mut self, root: Entity) -> Option<usize> {
        let scene = self.world.get::<GltfRoot>(root).cloned()?;
        let mut n = 0;
        for node in scene.nodes.iter().rev() {
            if node.prefab.is_some() {
                n += self.despawn_prefab(node.entity);
            } else {
                n += usize::from(self.world.despawn(node.entity));
            }
        }
        n += usize::from(self.world.despawn(root));
        self.free_gltf_assets(&scene);
        Some(n)
    }

    /// Exports of a spawned file again (hot reload): updates every scene
    /// spawned from `path`. On an error the old scenes stay.
    pub(crate) fn reload_gltf(&mut self, path: &str) -> Result<(), String> {
        let roots: Vec<Entity> = self
            .world
            .query::<(Entity, &GltfRoot)>()
            .iter()
            .filter(|(_, r)| r.path == path)
            .map(|(e, _)| e)
            .collect();
        let mut refresh = true;
        for root in roots {
            let (loaded, meshes, _) = self.gfx.load_node_meshes(path, refresh).map_err(|e| e.to_string())?;
            refresh = false;
            self.apply_gltf(root, path, &loaded.nodes, meshes)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Frees the meshes and collision shapes made for `scene`.
    fn free_gltf_assets(&mut self, scene: &GltfRoot) {
        for &mesh in &scene.meshes {
            self.free(mesh);
        }
        #[cfg(feature = "physics3d")]
        for &shape in &scene.shapes {
            self.free(shape);
        }
    }

    /// Brings the scene of `root` to `nodes` (with a mesh made for each
    /// node that has one): checks everything that can fail first, then
    /// changes the world, keeping the entities of nodes whose path stays.
    /// On an error nothing changed and `meshes` are freed.
    fn apply_gltf(
        &mut self,
        root: Entity,
        path: &str,
        nodes: &[LoadedNode],
        meshes: Vec<Option<Mesh>>,
    ) -> Result<(), Error> {
        let made: Vec<Mesh> = meshes.iter().flatten().copied().collect();
        let checked = self.check_gltf(root, path, nodes);
        let (plan, old, at) = match checked {
            Ok(c) => c,
            Err(e) => {
                made.iter().for_each(|&m| self.free(m));
                return Err(e);
            }
        };
        for (node, planned) in nodes.iter().zip(&plan.nodes) {
            if node.skinned && node.mesh.is_some() {
                eprintln!(
                    "ponko: {path}: node \"{}\" is skinned: drawn still, in its bind pose (load animated models with load_mesh, or put them in a prefab)",
                    planned.path
                );
            }
        }
        #[cfg(feature = "physics3d")]
        let colliders = match self.gltf_colliders(path, &plan, nodes) {
            Ok(c) => c,
            Err(e) => {
                made.iter().for_each(|&m| self.free(m));
                return Err(e);
            }
        };
        #[cfg(not(feature = "physics3d"))]
        {
            let left_out = plan.nodes.iter().filter(|n| n.props.collider.is_some()).count();
            if left_out > 0 {
                eprintln!("ponko: {path}: {left_out} colliders left out: the physics3d feature is off");
            }
        }

        // Prefab instances: new ones first, as they can fail.
        let mut fresh_prefabs: Vec<(usize, Entity)> = Vec::new();
        for (i, node) in plan.nodes.iter().enumerate() {
            let Some(prefab) = &node.props.prefab else { continue };
            if old.options.no_saved_objects || node.is_level {
                continue;
            }
            let kept = old
                .nodes
                .iter()
                .any(|o| o.path == node.path && o.prefab.as_ref() == Some(prefab) && self.world.is_alive(o.entity));
            if kept {
                continue;
            }
            match self.spawn_prefab(prefab, compose3d(&at, &node.world)) {
                Ok(e) => fresh_prefabs.push((i, e)),
                Err(e) => {
                    for (_, e) in fresh_prefabs {
                        self.despawn_prefab(e);
                    }
                    made.iter().for_each(|&m| self.free(m));
                    #[cfg(feature = "physics3d")]
                    for c in &colliders {
                        if let crate::d3::Shape::Mesh(shape) = c.collider.shape {
                            self.free(shape);
                        }
                    }
                    return Err(node_error(e.kind(), path, &node.path, e.message()));
                }
            }
        }

        // From here on nothing fails.
        let mut spawned: Vec<SpawnedNode> = Vec::with_capacity(plan.nodes.len());
        // Each node's entity (or, for nodes without one, where its children attach).
        let mut placed: Vec<(Entity, Transform3D)> = Vec::with_capacity(plan.nodes.len());
        let keep: std::collections::HashSet<&str> = plan.nodes.iter().map(|n| n.path.as_str()).collect();
        for gone in old.nodes.iter().filter(|o| !keep.contains(o.path.as_str())) {
            if gone.prefab.is_some() {
                self.despawn_prefab(gone.entity);
            } else {
                self.world.despawn(gone.entity);
            }
        }
        for (i, node) in plan.nodes.iter().enumerate() {
            let (parent, offset) = match node.parent {
                Some(p) => (placed[p].0, compose3d(&placed[p].1, &node.local)),
                None => (root, node.local),
            };
            if node.is_level || (node.props.prefab.is_some() && old.options.no_saved_objects) {
                placed.push((parent, offset));
                continue;
            }
            let world_at = compose3d(&at, &node.world);
            let before = old
                .nodes
                .iter()
                .find(|o| o.path == node.path && self.world.is_alive(o.entity));
            if let Some(prefab) = &node.props.prefab {
                let e = match fresh_prefabs.iter().find(|(n, _)| *n == i) {
                    Some(&(_, e)) => {
                        if let Some(b) = before {
                            self.despawn_prefab(b.entity);
                        }
                        e
                    }
                    None => {
                        let e = before.map(|b| b.entity).expect("a kept prefab instance");
                        self.world.insert(e, world_at);
                        e
                    }
                };
                self.world.insert(e, self.gltf_node(node));
                spawned.push(SpawnedNode {
                    path: node.path.clone(),
                    entity: e,
                    prefab: Some(prefab.clone()),
                    components: Vec::new(),
                });
                placed.push((e, Transform3D::default()));
                continue;
            }
            let e = match before {
                Some(b) if b.prefab.is_none() => {
                    self.clear_gltf_node(b);
                    b.entity
                }
                Some(b) => {
                    self.despawn_prefab(b.entity);
                    self.world.spawn((NotSaved,))
                }
                None => self.world.spawn((NotSaved,)),
            };
            self.world.insert(e, world_at);
            self.world.insert(e, NotSaved);
            self.world.insert(e, self.gltf_node(node));
            let moving = matches!(node.props.body, Some(NodeBody::Dynamic | NodeBody::Kinematic));
            if !moving {
                self.world.insert(e, Attached3D::to(parent).offset(offset));
            }
            self.dress_gltf_node(e, &plan, i, nodes, &meshes);
            for (name, value) in &node.props.components {
                // Checked by `check_gltf`.
                let _ = self.world.set_json(e, name, value.clone());
            }
            spawned.push(SpawnedNode {
                path: node.path.clone(),
                entity: e,
                prefab: None,
                components: node.props.components.iter().map(|(n, _)| n.clone()).collect(),
            });
            placed.push((e, Transform3D::default()));
        }
        #[cfg(feature = "physics3d")]
        let mut shapes = Vec::new();
        #[cfg(feature = "physics3d")]
        for c in colliders {
            let Some(e) = spawned
                .iter()
                .find(|s| s.path == plan.nodes[c.node].path)
                .map(|s| s.entity)
            else {
                continue;
            };
            if let crate::d3::Shape::Mesh(shape) = c.collider.shape {
                shapes.push(shape);
            }
            self.world.insert(e, c.body);
            self.world.insert(e, c.collider);
        }
        self.free_gltf_assets(&old);
        if let Some(r) = self.world.get_mut::<GltfRoot>(root) {
            r.nodes = spawned;
            r.meshes = made;
            #[cfg(feature = "physics3d")]
            {
                r.shapes = shapes;
            }
        }
        Ok(())
    }

    /// The plan for `nodes`, the scene's state before and where its root
    /// is, after checking the properties and component values.
    fn check_gltf(
        &mut self,
        root: Entity,
        path: &str,
        nodes: &[LoadedNode],
    ) -> Result<(Plan, GltfRoot, Transform3D), Error> {
        let components = self.world.reflected_components();
        let components: Vec<String> = components.iter().map(|c| c.to_string()).collect();
        let names: Vec<&str> = components.iter().map(String::as_str).collect();
        let plan = Plan::of(path, nodes, &names)?;
        for node in &plan.nodes {
            for (name, value) in &node.props.components {
                let one = json!({ name: value });
                self.load_scene_assets(&one)
                    .map_err(|e| node_error(e.kind(), path, &node.path, e.message()))?;
                // A value that would be added fresh, tried on an entity of its own.
                let probe = self
                    .world
                    .spawn_json(one)
                    .map_err(|e| node_error(ErrorKind::Json, path, &node.path, format!("\"ponko:{name}\": {e}")))?;
                self.world.despawn(probe);
            }
            if node.props.prefab.is_some() && node.props.spawn.is_some() {
                let message = "\"ponko:prefab\" and \"ponko:spawn\" don't go together";
                return Err(node_error(ErrorKind::Decode, path, &node.path, message));
            }
        }
        let old = self.world.get::<GltfRoot>(root).cloned().ok_or_else(|| {
            Error::new(ErrorKind::NotFound, format!("{path}: its scene's root is gone")).with_path(path)
        })?;
        let at = self.world.get::<Transform3D>(root).copied().unwrap_or_default();
        Ok((plan, old, at))
    }

    /// Makes the colliders the plan asks for, each from its node's mesh
    /// scaled as the node is in the world.
    #[cfg(feature = "physics3d")]
    fn gltf_colliders(&mut self, path: &str, plan: &Plan, nodes: &[LoadedNode]) -> Result<Vec<ColliderPlan>, Error> {
        use crate::d3::{Collider, CollisionGroups, RigidBody};
        let mut out: Vec<ColliderPlan> = Vec::new();
        let fail = |ctx: &mut Ctx, out: &[ColliderPlan], node: &str, message: &str| {
            for c in out {
                if let crate::d3::Shape::Mesh(shape) = c.collider.shape {
                    ctx.free(shape);
                }
            }
            node_error(ErrorKind::Decode, path, node, message)
        };
        for (i, node) in plan.nodes.iter().enumerate() {
            let Some(kind) = node.props.collider else { continue };
            if node.is_level || node.props.prefab.is_some() {
                continue;
            }
            let source = plan.shape_source(i);
            let Some(mesh) = &nodes[source].mesh else {
                return Err(fail(self, &out, &node.path, "\"ponko:collider\" needs a mesh to fit"));
            };
            // Colliders don't take the transform's scale: put it in the shape.
            let level = if source == i {
                Transform3D::default()
            } else {
                plan.nodes[source].local
            };
            let scale = node.world.scale.abs();
            let to_node = Mat4::from_scale(scale) * level_matrix(&level);
            let corners: Vec<Vec3> = mesh
                .data
                .positions
                .iter()
                .map(|&p| to_node.transform_point3(p))
                .collect();
            let (center, size) = bounds(&corners);
            let collider = match kind {
                NodeCollider::Box => Collider::cuboid(size).offset(center),
                NodeCollider::Sphere => {
                    let radius = corners.iter().map(|p| p.distance(center)).fold(0.0, f32::max);
                    Collider::sphere(radius).offset(center)
                }
                NodeCollider::Capsule => {
                    let radius = (size.x.max(size.z) * 0.5).max(1e-3);
                    Collider::capsule((size.y - 2.0 * radius).max(0.0), radius).offset(center)
                }
                NodeCollider::Hull => match self.physics3d.convex_hull(&corners) {
                    Some(shape) => Collider::mesh(shape),
                    None => return Err(fail(self, &out, &node.path, "its mesh has no volume for a convex hull")),
                },
                NodeCollider::Trimesh => {
                    let triangles: Vec<[u32; 3]> = mesh.data.indices.as_chunks::<3>().0.to_vec();
                    match self.physics3d.trimesh(&corners, &triangles) {
                        Some(shape) => Collider::mesh(shape),
                        None => {
                            return Err(fail(
                                self,
                                &out,
                                &node.path,
                                "its mesh has no triangles to collide with",
                            ));
                        }
                    }
                }
            };
            let mut collider = collider;
            collider.sensor = node.props.sensor;
            collider.groups = CollisionGroups::new(
                node.props.collision_layers.unwrap_or(u32::MAX),
                node.props.collides_with.unwrap_or(u32::MAX),
            );
            let body = match node.props.body.unwrap_or_default() {
                NodeBody::Dynamic => RigidBody::dynamic(),
                NodeBody::Kinematic => RigidBody::kinematic(),
                _ => RigidBody::fixed(),
            };
            out.push(ColliderPlan {
                node: i,
                collider,
                body,
            });
        }
        Ok(out)
    }

    /// The `GltfNode` of a planned node.
    fn gltf_node(&self, node: &Planned) -> GltfNode {
        GltfNode {
            name: node.name.clone(),
            path: node.path.clone(),
            extras: node.extras.clone(),
        }
    }

    /// Takes off a kept node's entity what its old properties put there,
    /// so the new ones apply to a clean entity.
    fn clear_gltf_node(&mut self, old: &SpawnedNode) {
        let e = old.entity;
        self.world.remove::<crate::render::Model>(e);
        self.world.remove::<crate::render::LodGroup>(e);
        self.world.remove::<crate::camera::RenderLayers>(e);
        self.world.remove::<crate::d3::PointLight>(e);
        self.world.remove::<crate::d3::SpotLight>(e);
        self.world.remove::<SpawnPoint>(e);
        self.world.remove::<Attached3D>(e);
        #[cfg(feature = "physics3d")]
        {
            self.world.remove::<crate::d3::Collider>(e);
            self.world.remove::<crate::d3::RigidBody>(e);
        }
        for name in &old.components {
            let _ = self.world.remove_json(e, name);
        }
    }

    /// Puts on `e` what node `i` draws and lights: its model or level of
    /// detail group, layers, light and spawn point.
    fn dress_gltf_node(&mut self, e: Entity, plan: &Plan, i: usize, nodes: &[LoadedNode], meshes: &[Option<Mesh>]) {
        let node = &plan.nodes[i];
        let props = &node.props;
        if let Some(_name) = &props.spawn {
            let name = match props.spawn.as_deref() {
                Some("") | None => node.name.clone(),
                Some(n) => n.to_string(),
            };
            self.world.insert(e, SpawnPoint { name });
        } else if !node.levels.is_empty() {
            let levels: Vec<crate::render::LodLevel> = node
                .levels
                .iter()
                .zip(&props.lod)
                .filter_map(|(&l, &height)| {
                    let mesh = meshes[l]?;
                    let renderer = crate::render::LodRenderer::new(mesh).with_offset(plan.nodes[l].local);
                    Some(crate::render::LodLevel::new(height, vec![renderer]))
                })
                .collect();
            let first = node.levels[0];
            let corners: Vec<Vec3> = nodes[first].mesh.as_ref().map_or_else(Vec::new, |m| {
                let to_group = level_matrix(&plan.nodes[first].local);
                m.data.positions.iter().map(|&p| to_group.transform_point3(p)).collect()
            });
            let (center, size) = bounds(&corners);
            let group = crate::render::LodGroup::new(size.max_element().max(1e-3), levels).with_center(center);
            self.world.insert(e, group);
        } else if !props.hidden
            && let Some(mesh) = meshes[i]
        {
            self.world.insert(e, crate::render::Model::new(mesh));
        }
        if let Some(bits) = props.layers {
            self.world.insert(e, crate::camera::RenderLayers(bits));
        }
        if let Some(light) = &nodes[i].light {
            let shadows = props.shadows.unwrap_or(false);
            match light.kind {
                ModelLightKind::Point(mut l) => {
                    l.shadows = shadows;
                    self.world.insert(e, l);
                }
                ModelLightKind::Spot(mut l) => {
                    l.shadows = shadows;
                    self.world.insert(e, l);
                }
                ModelLightKind::Sun { color, intensity } => {
                    let world = self.world.get::<Transform3D>(e).copied().unwrap_or_default();
                    let lighting = &mut self.scene3d.lighting;
                    lighting.sun_direction = world.rotation * Vec3::NEG_Z;
                    lighting.sun_color = color;
                    lighting.sun_intensity = intensity;
                }
            }
        }
    }
}

/// `t` as a matrix.
fn level_matrix(t: &Transform3D) -> Mat4 {
    Mat4::from_scale_rotation_translation(t.scale, t.rotation, t.position)
}
