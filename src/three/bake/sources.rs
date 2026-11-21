//! Where a bake's models come from without listing them by hand: the
//! models, levels of detail and instance batches of a world (with the mesh
//! data [`Graphics::keep_bake_sources`] keeps), or the nodes of a glTF
//! scene.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use glam::Mat4;
use ponko_ecs::{Entity, World};

use super::{BakeModel, BakedLighting, LightBakeScene, PartMaterial};
use crate::color::Color;
use crate::error::{Error, ErrorKind};
use crate::gfx::Graphics;
use crate::render::Material;
use crate::three::{LodGroup, MaterialDesc, MeshData, Model, ModelMaterial, ModelMaterials, Transform3D};

/// Leaves a model out of [`LightBakeScene::add_world`]: something that
/// moves (a door, a vehicle, a character), which shouldn't block or bounce
/// the baked light nor get a lightmap. It is lit by the probe volume.
///
/// ```
/// # use ponko::prelude::*;
/// let mut world = World::new();
/// world.spawn((ponko::d3::Transform::default(), ponko::render::Unbaked));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ponko_ecs::Component)]
pub struct Unbaked;

/// The mesh data, materials and textures a world bake reads, kept while
/// [`Graphics::keep_bake_sources`] is on.
#[derive(Default)]
pub(crate) struct BakeSources {
    /// Each mesh's data and parts (index ranges and materials), by id.
    meshes: HashMap<u32, (Arc<MeshData>, Vec<(Range<u32>, Material)>)>,
    /// Each material's description, by id.
    materials: HashMap<u32, MaterialDesc>,
    /// Each texture made from pixels, shrunk for the bake, by id.
    textures: HashMap<u32, Arc<super::BakeTexture>>,
}

impl BakeSources {
    pub(crate) fn mesh_made(&mut self, id: u32, data: &MeshData, parts: &[(Range<u32>, Material)]) {
        self.meshes.insert(id, (Arc::new(data.clone()), parts.to_vec()));
    }

    pub(crate) fn mesh_freed(&mut self, id: u32) {
        self.meshes.remove(&id);
    }

    pub(crate) fn texture_made(&mut self, id: u32, width: u32, height: u32, rgba: &[u8]) {
        if let Ok(map) = super::BakeTexture::from_rgba(width, height, rgba) {
            self.textures.insert(id, Arc::new(map));
        }
    }

    pub(crate) fn material_set(&mut self, id: u32, desc: &MaterialDesc) {
        self.materials.insert(id, *desc);
    }

    /// The model drawing mesh `id` with `matrix`, its parts' materials
    /// as `own` and `parts` override them (as the renderer resolves them),
    /// tinted with `tint`. `None` for a mesh it didn't keep.
    fn model(
        &self,
        id: u32,
        matrix: Mat4,
        tint: Color,
        own: Option<Material>,
        parts: Option<&ModelMaterials>,
    ) -> Option<BakeModel> {
        let (data, mesh_parts) = self.meshes.get(&id)?;
        let mut model = BakeModel::new(data, matrix);
        model.lightmapped = has_lightmap_uvs(data);
        for (k, (range, material)) in mesh_parts.iter().enumerate() {
            let listed = parts.and_then(|p| p.get(k));
            let material = listed.or(own).unwrap_or(*material);
            let desc = self.materials.get(&material.id).copied().unwrap_or_default();
            let mut part = PartMaterial::of(&desc, tint);
            part.map = desc.texture.and_then(|t| self.textures.get(&t.id)).cloned();
            model = model.with_part(range.clone(), part);
        }
        Some(model)
    }
}

/// Whether `mesh` has lightmap coordinates: a second UV set that isn't
/// all one point (files without one get zeros).
fn has_lightmap_uvs(mesh: &MeshData) -> bool {
    mesh.uvs1.len() == mesh.positions.len() && mesh.uvs1.iter().any(|&uv| uv != mesh.uvs1[0])
}

impl Graphics {
    /// Keeps (`true`) or drops (`false`) a copy of every mesh's triangles,
    /// every material's description and every texture made from pixels
    /// (shrunk to 128 on a side; block-compressed KTX2 and DDS textures
    /// aren't kept) from now on, for
    /// [`LightBakeScene::add_world`] to bake a world's models from. Off by
    /// default: it costs as much CPU memory as the meshes. Turn it on
    /// before making or loading the level's meshes (files and glTF scenes
    /// included), bake, then turn it off.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.gfx.keep_bake_sources(true);
    ///     // ... load or make the level ...
    ///     ctx.gfx.keep_bake_sources(false);
    /// });
    /// ```
    pub fn keep_bake_sources(&mut self, keep: bool) {
        if !keep {
            self.assets3d.bake_sources = None;
        } else if self.assets3d.bake_sources.is_none() {
            self.assets3d.bake_sources = Some(BakeSources::default());
        }
    }
}

/// What [`LightBakeScene::add_world`] added: which entity is which model of
/// the bake.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldBake {
    entries: Vec<(Entity, usize)>,
    skipped: Vec<Entity>,
}

impl WorldBake {
    /// Each entity added, with its index in the bake: a model's, or a level
    /// of detail group's first renderer's (its first level's renderers
    /// follow it).
    #[must_use]
    pub fn entries(&self) -> &[(Entity, usize)] {
        &self.entries
    }

    /// Models whose mesh wasn't kept (made before
    /// [`Graphics::keep_bake_sources`] was on): left out of the bake.
    #[must_use]
    pub fn skipped(&self) -> &[Entity] {
        &self.skipped
    }

    /// Puts the [`Lightmapped`](super::Lightmapped) component of `baked` (a
    /// bake of the scene this was added to) on every entity that has a
    /// lightmap in it; returns how many.
    pub fn insert_lightmaps(&self, baked: &BakedLighting, world: &mut World) -> usize {
        let mut n = 0;
        for &(entity, index) in &self.entries {
            if let Some(lightmap) = baked.lightmap(index)
                && world.is_alive(entity)
            {
                world.insert(entity, lightmap);
                n += 1;
            }
        }
        n
    }
}

/// What [`LightBakeScene::add_gltf`] added: which node of the file is which
/// model of the bake.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GltfBake {
    nodes: Vec<(String, usize)>,
}

impl GltfBake {
    /// Each node added (by its path, see [`GltfNode::path`](crate::d3::GltfNode::path)),
    /// with its index in the bake. A level of detail group's entry is its
    /// first level's mesh.
    #[must_use]
    pub fn nodes(&self) -> &[(String, usize)] {
        &self.nodes
    }

    /// Puts the [`Lightmapped`](super::Lightmapped) component of `baked` on
    /// the entities of the scene spawned at `root` (with
    /// [`Ctx::spawn_prefab`](crate::ctx::Ctx::spawn_prefab) or
    /// [`GltfRoot::spawn`](crate::d3::GltfRoot::spawn), where it was baked)
    /// that have a lightmap in it; returns how many.
    pub fn insert_lightmaps(&self, baked: &BakedLighting, world: &mut World, root: Entity) -> usize {
        let Some(root) = world.get::<crate::d3::GltfRoot>(root).cloned() else {
            return 0;
        };
        let mut n = 0;
        for (path, index) in &self.nodes {
            if let (Some(lightmap), Some(entity)) = (baked.lightmap(*index), root.entity(path))
                && world.is_alive(entity)
            {
                world.insert(entity, lightmap);
                n += 1;
            }
        }
        n
    }
}

impl LightBakeScene {
    /// Adds what `world` draws and lights, as it stands: every
    /// [`Model`](crate::render::Model) (with its
    /// [`ModelMaterial`](crate::render::ModelMaterial),
    /// [`ModelMaterials`](crate::render::ModelMaterials) and color), the
    /// first level of every [`LodGroup`](crate::render::LodGroup), every
    /// instance of the instance batches, and every point and spot light
    /// ([`add_lights_from`](Self::add_lights_from)). Meshes with lightmap
    /// coordinates get lightmaps; the others, and the instance batches,
    /// only block and bounce light (the probes light them). Skinned models
    /// and those with [`Unbaked`] are left out.
    ///
    /// The triangles and materials come from what `gfx` kept with
    /// [`Graphics::keep_bake_sources`]. After the bake, put the lightmaps on
    /// the entities with [`WorldBake::insert_lightmaps`].
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.gfx.keep_bake_sources(true);
    ///     let mut floor = ponko::render::MeshData::plane(4.0);
    ///     floor.generate_lightmap_uvs(2.0);
    ///     let stone = ctx.gfx.create_material(ponko::render::MaterialDesc::default());
    ///     let floor = ctx.gfx.create_mesh(&floor, stone);
    ///     ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(floor)));
    ///     let mut scene = ponko::render::LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
    ///     let added = scene.add_world(&mut ctx.world, &ctx.gfx).unwrap();
    ///     let settings = ponko::render::LightBakeSettings::default().with_samples(8).with_page_size(64);
    ///     let baked = ponko::render::bake_lighting(&scene, &settings).unwrap();
    ///     assert_eq!(added.insert_lightmaps(&baked, &mut ctx.world), 1);
    /// });
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`] when `gfx` keeps no mesh data
    /// ([`Graphics::keep_bake_sources`] is off).
    pub fn add_world(&mut self, world: &mut World, gfx: &Graphics) -> Result<WorldBake, Error> {
        let Some(sources) = gfx.assets3d.bake_sources.as_ref() else {
            return Err(Error::new(
                ErrorKind::Invalid,
                "add_world: no mesh data was kept; call ctx.gfx.keep_bake_sources(true) before making or loading the level's meshes",
            ));
        };
        let assets = &gfx.assets3d;
        let mut out = WorldBake::default();
        let models: Vec<_> = world
            .query::<(
                Entity,
                &Transform3D,
                &Model,
                Option<&ModelMaterial>,
                Option<&ModelMaterials>,
                Option<&Unbaked>,
            )>()
            .iter()
            .filter(|(_, _, m, _, _, unbaked)| unbaked.is_none() && assets.skin(m.mesh.id).is_none())
            .map(|(e, t, m, own, parts, _)| (e, t.matrix(), *m, own.map(|o| o.0), parts.cloned()))
            .collect();
        for (entity, matrix, model, own, parts) in models {
            match sources.model(model.mesh.id, matrix, model.color, own, parts.as_ref()) {
                Some(m) => {
                    let lightmapped = m.lightmapped;
                    let index = self.add(m);
                    if lightmapped {
                        out.entries.push((entity, index));
                    }
                }
                None => out.skipped.push(entity),
            }
        }
        let groups: Vec<_> = world
            .query::<(
                Entity,
                &Transform3D,
                &LodGroup,
                Option<&ModelMaterial>,
                Option<&ModelMaterials>,
                Option<&Unbaked>,
            )>()
            .iter()
            .filter(|g| g.5.is_none())
            .map(|(e, t, g, own, parts, _)| (e, t.matrix(), g.clone(), own.map(|o| o.0), parts.cloned()))
            .collect();
        for (entity, matrix, group, own, parts) in groups {
            let Some(level) = group.levels.first() else { continue };
            let mut first = None;
            let mut any_lightmap = false;
            for r in &level.renderers {
                let at = matrix * r.offset.matrix();
                let model = if assets.skin(r.mesh.id).is_some() {
                    None
                } else {
                    sources.model(r.mesh.id, at, r.color, own, parts.as_ref())
                };
                // A renderer whose mesh wasn't kept keeps its place in the
                // entries (renderer k reads entry first + k) with nothing.
                let model = model.unwrap_or_else(|| {
                    out.skipped.push(entity);
                    BakeModel::new(&MeshData::default(), Mat4::IDENTITY).with_lightmapped(false)
                });
                any_lightmap |= model.lightmapped;
                let index = self.add(model);
                first.get_or_insert(index);
            }
            if let Some(first) = first
                && any_lightmap
            {
                out.entries.push((entity, first));
            }
        }
        for batch in assets.instance_batches.iter().flatten() {
            if batch.parent.is_some() {
                // An instance cell's row: its batch has its instances.
                continue;
            }
            let Some(level) = batch.levels.first() else { continue };
            for instance in &batch.instances {
                let mut matrix = Mat4::from_cols_array_2d(&instance.model);
                // The spare slots of its last row carry other things.
                matrix.x_axis.w = 0.0;
                matrix.y_axis.w = 0.0;
                matrix.z_axis.w = 0.0;
                matrix.w_axis.w = 1.0;
                let tint = batch.options.tint;
                if let Some(model) = sources.model(level.mesh.id, matrix, tint, None, None) {
                    self.add(model.with_lightmapped(false));
                }
            }
        }
        self.add_lights_from(world);
        Ok(out)
    }

    /// Adds the glTF file at `path` placed by `transform` (the scene's
    /// root transform, as spawned): every node with a mesh that stays put
    /// (not a spawn point, prefab, hidden node, moving body or sensor; of a
    /// level of detail group, its first level), with its materials, and the
    /// file's point and spot lights. Meshes with a second UV set
    /// (`TEXCOORD_1`) get lightmaps; the others only block and bounce
    /// light. After the bake, put the lightmaps on the spawned scene with
    /// [`GltfBake::insert_lightmaps`].
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) -> Result<(), Error> {
    /// let mut scene = ponko::render::LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
    /// let added = scene.add_gltf(ctx, "levels/yard.glb", Mat4::IDENTITY)?;
    /// let baked = ponko::render::bake_lighting(&scene, &ponko::render::LightBakeSettings::default())?;
    /// let root = ctx.spawn_prefab("levels/yard.glb", vec3(0.0, 0.0, 0.0))?;
    /// added.insert_lightmaps(&baked, &mut ctx.world, root);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// As [`GltfScene::load`](crate::d3::GltfScene::load).
    pub fn add_gltf(&mut self, ctx: &crate::ctx::Ctx, path: &str, transform: Mat4) -> Result<GltfBake, Error> {
        use crate::three::gltf;
        let scene = crate::d3::GltfScene::load(ctx, path)?;
        // Read again with the materials, which the scene doesn't keep.
        let bytes = ctx
            .assets
            .asset_sources
            .find(path)?
            .into_bytes()
            .map_err(|e| e.with_path(path))?;
        let sources = &ctx.assets.asset_sources;
        let loaded = gltf::load_nodes(
            path,
            &bytes,
            &mut |name| sources.read(name),
            &|_| false,
            gltf::GpuFormats::default(),
        )
        .map_err(|e| Error::decode(path, e))?;
        let nodes = &scene.nodes;
        let mut out = GltfBake::default();
        let add_node = |this: &mut Self, i: usize| -> Option<usize> {
            let node = &nodes[i];
            let mesh = node.mesh.as_ref()?;
            let found = loaded.nodes.get(i)?;
            if found.skinned {
                return None;
            }
            let mut model = BakeModel::new(mesh, transform * node.world.matrix());
            model.lightmapped = has_lightmap_uvs(mesh);
            for (range, material) in found.mesh.as_ref().map_or(&[][..], |m| &m.parts[..]) {
                let Some(m) = loaded.materials.get(*material) else {
                    continue;
                };
                let desc = MaterialDesc {
                    color: m.color,
                    emissive: m.emissive,
                    double_sided: m.double_sided,
                    ..Default::default()
                };
                let mut part = PartMaterial::of(&desc, Color::WHITE);
                part.map = m
                    .image
                    .and_then(|i| loaded.images.get(i))
                    .filter(|image| !image.2.is_empty())
                    .and_then(|(w, h, rgba)| super::BakeTexture::from_rgba(*w, *h, rgba).ok())
                    .map(Arc::new);
                model = model.with_part(range.clone(), part);
            }
            let lightmapped = model.lightmapped;
            let index = this.add(model);
            lightmapped.then_some(index)
        };
        let in_group = |i: usize| nodes[i].parent.is_some_and(|p| !nodes[p].properties.lod.is_empty());
        for i in 0..nodes.len() {
            let p = &nodes[i].properties;
            let fixed = matches!(p.body, None | Some(crate::three::gltf_props::NodeBody::Fixed));
            if p.spawn.is_some() || p.prefab.is_some() || p.sensor || !fixed {
                continue;
            }
            if !p.lod.is_empty() {
                // A level of detail group: its first level, as the group's.
                let levels = crate::three::gltf_scene::lod_levels(
                    nodes.iter().map(|n| (n.parent, n.name.as_str(), n.mesh.is_some())),
                    i,
                );
                if let Some(&first) = levels.first()
                    && let Some(index) = add_node(self, first)
                {
                    out.nodes.push((nodes[i].path.clone(), index));
                }
                continue;
            }
            if p.hidden || in_group(i) {
                continue;
            }
            if let Some(index) = add_node(self, i) {
                out.nodes.push((nodes[i].path.clone(), index));
            }
        }
        for node in nodes {
            let Some(light) = &node.light else { continue };
            let at = transform * node.world.matrix() * light.transform.matrix();
            let position = at.transform_point3(glam::Vec3::ZERO);
            match &light.kind {
                crate::three::ModelLightKind::Point(l) => self.add_point_light(position, l),
                crate::three::ModelLightKind::Spot(l) => {
                    let forward = at.transform_vector3(glam::Vec3::NEG_Z);
                    self.add_spot_light(position, forward, l);
                }
                // The sun is the scene's Lighting, which the bake was made with.
                crate::three::ModelLightKind::Sun { .. } => {}
            }
        }
        Ok(out)
    }
}
