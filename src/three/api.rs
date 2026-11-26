//! The 3D methods on [`Ctx`]: making meshes and materials, loading models.

use crate::handle::INDEX_MASK;
use std::ops::Range;

use glam::Vec3;

use super::{
    AnimationClip, Blend, Material, MaterialDesc, Mesh, MeshData, MeshMorph, MeshSkin, Shader, SharedImage, Skeleton,
    gltf, linear,
};
use crate::gfx::Graphics;
use crate::{assets, ctx::Ctx};
use ponko_ecs::World;

/// A material as the GPU reads it (one uniform block).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaterialUniform {
    /// Linear RGBA.
    pub color: [f32; 4],
    /// Linear RGB, then unused.
    pub emissive: [f32; 4],
    /// roughness, metallic, normal map strength, unused.
    pub params: [f32; 4],
    /// Which maps it has (1/0): normal, ORM, emissive; then whether the
    /// model's tint is masked by the vertex colour's alpha (1/0). A
    /// two-channel normal map is 2 (X and Y in red and green) or 3 (in red
    /// and alpha): the shader rebuilds Z.
    pub maps: [u32; 4],
    /// For custom shaders.
    pub custom: [[f32; 4]; 4],
}

/// A material's textures, in binding order: base color, normal map, ORM
/// map, emissive map and two for custom shaders. `None`: the default.
pub(crate) type MaterialTextures = [Option<u32>; 6];

/// Everything the GPU needs to make or change a material.
pub(crate) struct GpuMaterialDesc {
    pub uniform: MaterialUniform,
    pub textures: MaterialTextures,
    /// Texture arrays for custom shaders, by id (`None`: a white one).
    pub arrays: [Option<u32>; 3],
    /// Index into the renderer's shaders (0: the built-in one).
    pub shader: u32,
    pub transparent: bool,
    pub cutout: bool,
    pub double_sided: bool,
    pub stencil: Option<super::Stencil>,
}

impl GpuMaterialDesc {
    pub(crate) fn new(desc: &MaterialDesc) -> Self {
        let [r, g, b, _] = linear(desc.emissive);
        let id = |t: Option<crate::texture::Texture>| t.map(|t| t.id);
        Self {
            uniform: MaterialUniform {
                color: linear(desc.color),
                emissive: [r, g, b, 0.0],
                params: [desc.roughness.max(0.03), desc.metallic, desc.normal_strength, 0.0],
                maps: [
                    u32::from(desc.normal_map.is_some()),
                    u32::from(desc.orm_map.is_some()),
                    u32::from(desc.emissive_map.is_some()),
                    u32::from(desc.masked_tint),
                ],
                custom: desc.custom.map(|v| v.to_array()),
            },
            textures: [
                id(desc.texture),
                id(desc.normal_map),
                id(desc.orm_map),
                id(desc.emissive_map),
                id(desc.custom_textures[0]),
                id(desc.custom_textures[1]),
            ],
            arrays: desc.custom_arrays.map(|a| a.map(|a| a.id)),
            shader: desc.shader.map_or(0, |s| (s.id & INDEX_MASK) + 1),
            transparent: desc.blend == Blend::Transparent,
            cutout: desc.blend == Blend::Cutout,
            double_sided: desc.double_sided,
            stencil: desc.stencil,
        }
    }
}

/// A glTF file's nodes, the mesh made for each node that has one (one
/// for all the nodes of one glTF mesh), and
/// where the file is on disk, when it is a file
/// ([`Graphics::load_node_meshes`]).
pub(crate) type NodeMeshes = (
    std::sync::Arc<gltf::ReadNodes>,
    Vec<Option<Mesh>>,
    Option<std::path::PathBuf>,
);

impl Graphics {
    /// Puts a material on the GPU: a color (and texture), how rough and how
    /// metallic, and light it gives off.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let bricks = ctx.gfx.load_texture("bricks.png");
    /// let wall = ctx.gfx.create_material(ponko::render::MaterialDesc::default().with_texture(bricks).with_roughness(0.9));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If roughness or metallic is outside 0..=1.
    pub fn create_material(&mut self, desc: MaterialDesc) -> Material {
        self.try_create_material(desc).unwrap_or_else(|e| panic!("{e}"))
    }

    /// The material of image `path` alone (white, otherwise the default
    /// material), made once and kept: what a scene's `"ModelMaterial": {
    /// "texture": path }` draws with, and what the editor puts on a model
    /// an image is dropped on.
    pub(crate) fn texture_material(&mut self, path: &str) -> Result<Material, crate::error::Error> {
        if let Some(material) = self.material_names.textured(path)
            && self.assets3d.materials_live.contains(material.id)
        {
            return Ok(material);
        }
        let texture = self.try_load_texture(path)?;
        let material = self.try_create_material(MaterialDesc::default().with_texture(texture))?;
        self.material_names.add_texture(path, material);
        Ok(material)
    }

    /// [`create_material`](Self::create_material) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) if roughness or
    /// metallic is outside 0..=1,
    /// [`BadHandle`](crate::error::ErrorKind::BadHandle) if its shader or
    /// texture arrays weren't made by this game.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_create_material(ponko::render::MaterialDesc::default().with_roughness(2.0)).unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::Invalid);
    /// });
    /// ```
    pub fn try_create_material(&mut self, desc: MaterialDesc) -> Result<Material, crate::error::Error> {
        self.check_material(&desc)?;
        let id = self.assets3d.material_ids.make("material");
        self.assets3d.materials_live.insert(id);
        self.asset_ledger.made(crate::inventory::AssetClass::Material, id);
        let gpu = self.gpu_material(&desc);
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.create_material(id, &gpu);
        }
        if let Some(sources) = self.assets3d.bake_sources.as_mut() {
            sources.material_set(id, &desc);
        }
        #[cfg(feature = "editor")]
        self.note_material_desc(Material { id }, &desc);
        Ok(Material { id })
    }

    /// Changes a material: every model drawn with it changes too. Handy for
    /// animating a custom shader's values, or a glow.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, lamp: ponko::render::Material) {
    /// let on = ponko::render::MaterialDesc::default().with_emissive(Color::hex(0xFFD080));
    /// ctx.gfx.update_material(lamp, on);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// As [`Graphics::create_material`](crate::gfx::Graphics::create_material), or if the material comes from another
    /// game. Updating a freed material does nothing: it stays freed.
    pub fn update_material(&mut self, material: Material, desc: MaterialDesc) {
        assert!(
            self.assets3d.materials_live.was_made(material.id),
            "ponko: update_material: that material wasn't made by this game; update a material this game created"
        );
        self.check_material(&desc).unwrap_or_else(|e| panic!("{e}"));
        if !self.assets3d.materials_live.contains(material.id) {
            return;
        }
        if let Some(sources) = self.assets3d.bake_sources.as_mut() {
            sources.material_set(material.id, &desc);
        }
        #[cfg(feature = "editor")]
        self.note_material_desc(material, &desc);
        let gpu = self.gpu_material(&desc);
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.update_material(material.id, &gpu, &self.assets3d.instance_batches);
        }
    }

    /// What the GPU makes of `desc`: freed shaders, textures and arrays
    /// are left out, so it looks as it would without them.
    pub(crate) fn gpu_material(&self, desc: &MaterialDesc) -> GpuMaterialDesc {
        let mut gpu = GpuMaterialDesc::new(desc);
        if desc.shader.is_some_and(|s| !self.assets3d.shaders_live.contains(s.id)) {
            gpu.shader = 0;
        }
        let freed = |id: u32| self.textures_live.was_made(id) && !self.textures_live.contains(id);
        for (slot, texture) in gpu.textures.iter_mut().enumerate() {
            if texture.is_some_and(freed) {
                *texture = None;
                // Slots 1 to 3 (normal, ORM, emissive) have flags.
                if (1..=3).contains(&slot) {
                    gpu.uniform.maps[slot - 1] = 0;
                }
            }
        }
        for array in &mut gpu.arrays {
            *array = array
                .filter(|&id| self.assets3d.arrays_live.contains(id))
                .map(|id| id & INDEX_MASK);
        }
        #[cfg(feature = "compressed-textures")]
        if let Some(normal) = gpu.textures[1] {
            gpu.uniform.maps[0] = match self.compressed.get(&normal) {
                Some(crate::compressed::NormalChannels::Rg) => 2,
                Some(crate::compressed::NormalChannels::Ra) => 3,
                _ => 1,
            };
        }
        gpu
    }

    /// Adds the live 3D handles to `counts` (see [`Ctx::live_counts`]).
    pub(crate) fn count_live_3d(&self, counts: &mut crate::handle::LiveCounts) {
        let a = &self.assets3d;
        counts.render_targets = a
            .render_targets
            .iter()
            .filter(|t| self.textures_live.contains(t.id))
            .count();
        counts.meshes = a.bounds.iter().filter(|&&(_, r)| r >= 0.0).count();
        counts.materials = a.materials_live.len();
        counts.shaders = a.shaders_live.len();
        counts.computes = a.computes_live.len();
        counts.effects = a.effects_live.len();
        counts.gpu_buffers = a.gpu_buffers.iter().filter(|&&len| len > 0).count();
        counts.instance_batches = a
            .instance_batches
            .iter()
            .flatten()
            .filter(|b| b.parent.is_none())
            .count();
        counts.instance_cells = a.instance_cells.live();
        counts.crowds = a.crowds.iter().flatten().count();
        counts.texture_arrays = a.arrays_live.len();
        counts.sky_shaders = a.sky_shaders_live.len();
    }

    /// Frees material `id` (see [`Ctx::free`]): mesh parts made with it
    /// draw with the default material's look from now on.
    pub(crate) fn free_material_id(&mut self, id: u32) {
        if !self.assets3d.materials_live.remove(id) {
            return;
        }
        self.assets3d.material_ids.release(id);
        self.asset_ledger.freed(crate::inventory::AssetClass::Material, id);
        #[cfg(feature = "editor")]
        self.assets3d.material_descs.remove(&id);
        if self.assets3d.default_material == Some(Material { id }) {
            self.assets3d.default_material = None;
        }
        self.material_names.remove(id);
        for materials in self.assets3d.loaded_materials.values_mut() {
            materials.retain(|(m, _)| m.id != id);
        }
        self.assets3d.loaded_materials.retain(|_, m| !m.is_empty());
        // A file whose material was freed makes only that one again when
        // reloaded (`file_materials`).
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.free_material(id);
        }
    }

    /// Frees material shader `id` (see [`Ctx::free`]): its materials draw
    /// with the built-in shader from now on.
    pub(crate) fn free_shader_id(&mut self, id: u32) {
        if !self.assets3d.shaders_live.remove(id) {
            return;
        }
        self.assets3d.shader_ids.release(id);
        self.forget_shader_name(super::ShaderKind::Material, id);
        self.asset_ledger.freed(crate::inventory::AssetClass::Shader, id);
        self.assets3d.loaded_shaders.retain(|_, s| s.id != id);
        if self.assets3d.terrain_shader == Some(Shader { id }) {
            self.assets3d.terrain_shader = None;
        }
        self.watcher
            .forget(|a| matches!(a, crate::hot_reload::Asset::Shader(s) if *s == id));
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.free_shader((id & INDEX_MASK) + 1);
        }
    }

    /// Builds, now, the render pipelines the renderer would otherwise build
    /// the first time a frame draws them: the variants of every material
    /// shader for crossfading [`LodGroup`](crate::render::LodGroup) levels and for
    /// buffer-driven instances. Each takes several milliseconds, a visible
    /// hitch if it happens mid-game (the first time a crossfade shows). Call
    /// it at the end of loading; shaders made afterwards then build theirs
    /// when they're made. Startup takes longer, so games that use neither
    /// can skip it. [`RenderStats::pipelines_built`](crate::render::RenderStats)
    /// counts the ones still built while drawing. Does nothing in headless runs.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// // ... load meshes, materials and shaders ...
    /// ctx.gfx.warm_render_pipelines();
    /// # }
    /// ```
    pub fn warm_render_pipelines(&mut self) {
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.warm_pipelines();
        }
    }

    fn check_material(&self, desc: &MaterialDesc) -> Result<(), crate::error::Error> {
        use crate::error::{Error, ErrorKind};
        if !((0.0..=1.0).contains(&desc.roughness) && (0.0..=1.0).contains(&desc.metallic)) {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "material roughness and metallic go from 0 to 1 (got {} and {})",
                    desc.roughness, desc.metallic
                ),
            ));
        }
        // Freed handles are allowed: they draw as the default.
        if let Some(shader) = desc.shader
            && !self.assets3d.shaders_live.was_made(shader.id)
        {
            return Err(Error::new(
                ErrorKind::BadHandle,
                "that shader wasn't made by this game; create or load it in this game",
            ));
        }
        for array in desc.custom_arrays.into_iter().flatten() {
            if !self.assets3d.knows_array(array) {
                return Err(Error::new(
                    ErrorKind::BadHandle,
                    "that texture array wasn't made by this game; create or load it in this game",
                ));
            }
        }
        Ok(())
    }

    /// Loads a material shader (WGSL) from an [added asset
    /// source](crate::assets::Assets::add_source) or `assets/`. It can define
    /// `fn vertex(v: Vertex) -> Vertex`, to move vertices (wind, waves), and
    /// `fn surface(s: Surface) -> Surface`, to change how each pixel looks;
    /// the engine lights it, shadows it and does the rest. Hooks are ordinary
    /// functions with exactly those types (parameter names may differ), not
    /// stage entry points. All `@group`/`@binding` declarations are engine-owned;
    /// use the material's custom values/textures or `items` instead. Contract
    /// errors name the expected rule and fix in [`Graphics::shader_status`].
    /// Set `s.unlit = true` in `surface` for albedo plus emissive without
    /// lighting; alpha, face mode, skinning and instancing still work as usual.
    /// With hot reload
    /// on, saving the file in `assets/` updates the game (an error keeps the old one).
    /// Loading the same file again returns the same shader.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let wind = ctx.gfx.load_shader("shaders/wind.wgsl");
    /// let leaves = ctx.gfx.create_material(ponko::render::MaterialDesc::default().with_shader(wind).with_double_sided(true));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the file can't be read or doesn't compile (the message says where).
    /// With hot reload on, a file that doesn't compile doesn't panic: it
    /// gets a stand-in and stays watched until a saved fix compiles
    /// ([`ShaderOutcome::Failed`](crate::render::ShaderOutcome::Failed), [`Graphics::shader_status`](crate::gfx::Graphics::shader_status)).
    pub fn load_shader(&mut self, path: &str) -> Shader {
        self.try_load_shader(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_shader`](Self::load_shader) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// is missing, [`Io`](crate::error::ErrorKind::Io) if it can't be read,
    /// [`Decode`](crate::error::ErrorKind::Decode) if it isn't UTF-8 text and
    /// [`Shader`](crate::error::ErrorKind::Shader) if it doesn't compile (the
    /// message has the compiler's log). With hot reload on, a file that doesn't
    /// compile still returns a stand-in, as `load_shader` does.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_shader("shaders/missing.wgsl").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_shader(&mut self, path: &str) -> Result<Shader, crate::error::Error> {
        let found = match self
            .asset_sources
            .lookup(path, |origin| self.assets3d.loaded_shaders.get(origin).copied())?
        {
            crate::assets::Lookup::Cached(_, shader) => return Ok(shader),
            crate::assets::Lookup::Found(found) => found,
        };
        let origin = found.origin.clone();
        let source = found.into_text(path)?;
        let shader = match self.try_create_shader(path, &source) {
            Ok(shader) => shader,
            // With hot reload on, it draws with the built-in look until a
            // saved fix compiles; its status says what's wrong.
            Err(e) if self.keeps_failed_load(&origin) => {
                let e = e.with_path(path);
                Self::report_stand_in(path, &e);
                let id = self.assets3d.shader_ids.next("shader");
                self.compile_shader(path, "", id, false).map_err(|_| e)?;
                let id = self.assets3d.shader_ids.make("shader");
                self.keep_shader_name(super::ShaderKind::Material, id, path);
                self.assets3d.shaders_live.insert(id);
                self.asset_ledger.made(crate::inventory::AssetClass::Shader, id);
                Shader { id }
            }
            Err(e) => return Err(e.with_path(path)),
        };
        self.assets3d.loaded_shaders.insert(origin.clone(), shader);
        self.asset_ledger
            .own(crate::inventory::AssetClass::Shader, shader.id, path);
        if let crate::assets::Origin::File(file) = origin {
            self.watcher
                .watch(path, file, crate::hot_reload::Asset::Shader(shader.id));
        }
        Ok(shader)
    }

    /// Makes a material shader from WGSL source; `name` is for error
    /// messages. See [`Graphics::load_shader`](crate::gfx::Graphics::load_shader) for what it can define.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let stripes = ctx.gfx.create_shader("stripes", "
    ///     fn surface(s: Surface) -> Surface {
    ///         var out = s;
    ///         out.albedo *= step(0.5, fract(s.uv.x * 8.0));
    ///         return out;
    ///     }");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If it doesn't compile (the message says where).
    pub fn create_shader(&mut self, name: &str, source: &str) -> Shader {
        self.try_create_shader(name, source).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_shader`](Self::create_shader) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Shader`](crate::error::ErrorKind::Shader) if it doesn't
    /// compile, with the compiler's log in the message.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_create_shader("broken", "fn surface(s: Surface) -> Surface { return 1; }").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::Shader);
    /// });
    /// ```
    pub fn try_create_shader(&mut self, name: &str, source: &str) -> Result<Shader, crate::error::Error> {
        let id = self.assets3d.shader_ids.next("shader");
        let compiled = self.compile_shader(name, source, id, false);
        self.record_shader(name, super::ShaderKind::Material, false, compiled)?;
        let id = self.assets3d.shader_ids.make("shader");
        self.keep_shader_name(super::ShaderKind::Material, id, name);
        self.assets3d.shaders_live.insert(id);
        self.asset_ledger.made(crate::inventory::AssetClass::Shader, id);
        Ok(Shader { id })
    }

    /// Recompiles a shader whose file changed (hot reload). On an error the
    /// last good version stays in use.
    pub(crate) fn reload_shader(&mut self, id: u32, name: &str, source: &str) -> Result<(), String> {
        if !self.assets3d.shaders_live.contains(id) {
            return Ok(());
        }
        let compiled = self.compile_shader(name, source, id, true);
        Ok(self.record_shader(name, super::ShaderKind::Material, true, compiled)?)
    }

    /// Checks material shader `source` and builds its pipelines, as shader
    /// `replace` or as the next one.
    fn compile_shader(
        &mut self,
        name: &str,
        source: &str,
        id: u32,
        replace: bool,
    ) -> Result<(), Box<super::ShaderDiagnostic>> {
        #[cfg(feature = "window")]
        crate::render::diagnose(
            name,
            super::ShaderKind::Material,
            crate::render::FULL_MATERIAL_VARIANT,
            &crate::render::compose_shader(source),
        )?;
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            let slot = (id & INDEX_MASK) + 1;
            if replace {
                renderer.replace_shader(slot, name, source)?;
            } else {
                renderer.create_shader(slot, name, source)?;
            }
        }
        let _ = (name, source, id, replace);
        Ok(())
    }

    /// A plain white material, made once and shared.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let white = ctx.gfx.default_material();
    /// let ball = ctx.gfx.create_mesh(&ponko::render::MeshData::sphere(0.5, 32), white);
    /// # }
    /// ```
    pub fn default_material(&mut self) -> Material {
        if let Some(m) = self.assets3d.default_material {
            return m;
        }
        let m = self.create_material(MaterialDesc::default());
        self.assets3d.default_material = Some(m);
        m
    }

    /// Puts triangles on the GPU as a [`Mesh`] of one material. Draw it by
    /// spawning entities with a [`Model`](crate::render::Model) of it.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let stone = ctx.gfx.create_material(ponko::render::MaterialDesc::color(Color::hex(0x8A8A8A)));
    /// let block = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(1.0), stone);
    /// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(block)));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the data doesn't fit together (see [`MeshData::check`]), or the
    /// material comes from another game.
    pub fn create_mesh(&mut self, data: &MeshData, material: Material) -> Mesh {
        self.try_create_mesh(data, material).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_mesh`](Self::create_mesh) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) if the data
    /// doesn't fit together, [`BadHandle`](crate::error::ErrorKind::BadHandle)
    /// if the material comes from another game.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let stone = ctx.gfx.create_material(ponko::render::MaterialDesc::default());
    ///     let mut broken = ponko::render::MeshData::cube(1.0);
    ///     broken.indices.pop();
    ///     assert_eq!(ctx.gfx.try_create_mesh(&broken, stone).unwrap_err().kind(), ErrorKind::Invalid);
    /// });
    /// ```
    pub fn try_create_mesh(&mut self, data: &MeshData, material: Material) -> Result<Mesh, crate::error::Error> {
        use crate::error::{Error, ErrorKind};
        data.problem()
            .map_err(|e| Error::new(ErrorKind::Invalid, format!("create_mesh: {e}")))?;
        if !self.assets3d.materials_live.was_made(material.id) {
            return Err(Error::new(
                ErrorKind::BadHandle,
                "create_mesh: that material wasn't made by this game; create it with create_material in this game",
            ));
        }
        Ok(self.mesh_from_parts(data, &[(0..data.indices.len() as u32, material)], None, Vec::new()))
    }

    /// Puts triangles on the GPU as a [`Mesh`] with no material of its own:
    /// [`create_mesh`](Self::create_mesh) with the
    /// [default material](Self::default_material). For meshes whose look
    /// comes from each model, with a [`ModelMaterial`](crate::render::ModelMaterial):
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let ball = ctx.gfx.create_plain_mesh(&ponko::render::MeshData::sphere(0.5, 32));
    /// let gold = ctx.gfx.create_material(ponko::render::MaterialDesc::color(Color::hex(0xFFC83D)).with_metallic(1.0));
    /// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(ball)));                        // plain white
    /// ctx.world.spawn((ponko::d3::Transform::at(Vec3::X), ponko::render::Model::new(ball), ponko::render::ModelMaterial(gold))); // gold
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the data doesn't fit together (see [`MeshData::check`]).
    pub fn create_plain_mesh(&mut self, data: &MeshData) -> Mesh {
        self.try_create_plain_mesh(data).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_plain_mesh`](Self::create_plain_mesh) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) if the data
    /// doesn't fit together.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let cube = ctx.gfx.try_create_plain_mesh(&ponko::render::MeshData::cube(1.0)).unwrap();
    ///     assert!(ctx.gfx.is_live(cube));
    ///     let mut broken = ponko::render::MeshData::cube(1.0);
    ///     broken.indices.pop();
    ///     assert_eq!(ctx.gfx.try_create_plain_mesh(&broken).unwrap_err().kind(), ErrorKind::Invalid);
    /// });
    /// ```
    pub fn try_create_plain_mesh(&mut self, data: &MeshData) -> Result<Mesh, crate::error::Error> {
        // Checked first, so bad data doesn't make the default material.
        data.problem().map_err(|e| {
            crate::error::Error::new(crate::error::ErrorKind::Invalid, format!("create_plain_mesh: {e}"))
        })?;
        let material = self.default_material();
        self.try_create_mesh(data, material)
    }

    /// Makes a mesh that bends with `skeleton`: each corner of `data` has
    /// up to four [`joints`](MeshData::joints) (indices into the
    /// skeleton's joints) and how much each moves it. `data` is modelled
    /// with the skeleton at rest. Draw it with a [`Model`](crate::render::Model)
    /// and a [`Skeleton`] (this one, or any made from the same joints, or
    /// with joints of the same names) on the entity.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let skeleton = ponko::anim::Skeleton::new(vec![
    ///         ponko::anim::Joint::new("base", None),
    ///         ponko::anim::Joint::new("top", 0).with_rest(ponko::d3::Transform::at(vec3(0.0, 1.0, 0.0))),
    ///     ]);
    ///     let mut post = ponko::render::MeshData::cube(0.2);
    ///     // Corners above the middle follow the top joint.
    ///     post.joints = post.positions.iter().map(|p| [if p.y > 0.0 { 1 } else { 0 }, 0, 0, 0]).collect();
    ///     post.weights = vec![[1.0, 0.0, 0.0, 0.0]; post.positions.len()];
    ///     let material = ctx.gfx.default_material();
    ///     let mesh = ctx.gfx.create_skinned_mesh(&post, material, &skeleton);
    ///     ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(mesh), skeleton));
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If the data doesn't fit together (see [`MeshData::check`]), has no
    /// joints, names a joint the skeleton doesn't have, or the material
    /// comes from another game.
    pub fn create_skinned_mesh(&mut self, data: &MeshData, material: Material, skeleton: &Skeleton) -> Mesh {
        if let Err(e) = data.problem() {
            panic!("ponko: create_skinned_mesh: {e}");
        }
        assert!(
            !data.joints.is_empty(),
            "ponko: create_skinned_mesh: the mesh data has no joints (use create_mesh)"
        );
        let n = skeleton.joints().len();
        if let Some(bad) = data.joints.iter().flatten().find(|&&j| usize::from(j) >= n) {
            panic!("ponko: create_skinned_mesh: joint {bad} is past the skeleton's {n} joints");
        }
        assert!(
            self.assets3d.materials_live.was_made(material.id),
            "ponko: create_skinned_mesh: that material wasn't made by this game; create it with create_material in this game"
        );
        let rest = Skeleton::from_shared(skeleton.shared_joints().clone());
        let inverse_binds = (0..n).map(|j| rest.joint_matrix(j).inverse()).collect();
        let mut data = data.clone();
        for w in &mut data.weights {
            *w = super::skin::normalized(*w);
        }
        let skin = MeshSkin::new(
            skeleton.shared_joints().clone(),
            (0..n as u16).collect(),
            inverse_binds,
            &data,
        );
        self.mesh_from_parts(
            &data,
            &[(0..data.indices.len() as u32, material)],
            Some(skin),
            Vec::new(),
        )
    }

    /// The [`Skeleton`] of a glTF file with skins (loading its mesh with
    /// [`load_mesh`](Self::load_mesh) if that hasn't happened), at rest:
    /// every node of the file's scene is a joint, named as in the file.
    /// Put it on the entity with the file's [`Model`](crate::render::Model) and
    /// change its `pose` to bend the model.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let fox = ctx.gfx.load_mesh("models/fox.glb");
    /// let skeleton = ctx.gfx.load_skeleton("models/fox.glb");
    /// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(fox), skeleton));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// With a clear message if the file is missing, can't be read or has
    /// no skins.
    pub fn load_skeleton(&mut self, path: &str) -> Skeleton {
        self.try_load_skeleton(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_skeleton`](Self::load_skeleton) with errors as values.
    ///
    /// # Errors
    ///
    /// Those of [`try_load_mesh`](Self::try_load_mesh), or
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// has no skins.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_skeleton("models/missing.glb").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_skeleton(&mut self, path: &str) -> Result<Skeleton, crate::error::Error> {
        let (_, origin) = self.load_mesh_from(path)?;
        self.assets3d.loaded_skeletons.get(&origin).cloned().ok_or_else(|| {
            crate::error::Error::new(
                crate::error::ErrorKind::NotFound,
                format!("{path} has no skins or animations, so no skeleton"),
            )
            .with_path(path)
        })
    }

    /// Loads all animation clips from a glTF file, cached per file.
    ///
    /// # Panics
    ///
    /// With the loading error if the file cannot be loaded; see
    /// [`try_load_clips`](Self::try_load_clips).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let clips = ctx.gfx.load_clips("models/character.glb");
    /// # }
    /// ```
    pub fn load_clips(&mut self, path: &str) -> Vec<AnimationClip> {
        self.try_load_clips(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_clips`](Self::load_clips) with errors as values.
    ///
    /// # Errors
    ///
    /// Those of [`try_load_mesh`](Self::try_load_mesh), and
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) for a file
    /// without skins or animations.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_clips("models/missing.glb").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_clips(&mut self, path: &str) -> Result<Vec<AnimationClip>, crate::error::Error> {
        let (_, origin) = self.load_mesh_from(path)?;
        self.assets3d
            .loaded_skeletons
            .contains_key(&origin)
            .then(|| self.assets3d.loaded_clips.get(&origin).cloned().unwrap_or_default())
            .ok_or_else(|| {
                crate::error::Error::new(
                    crate::error::ErrorKind::NotFound,
                    format!("{path} has no skins or animations, so no skeleton"),
                )
                .with_path(path)
            })
    }

    /// Loads a named clip.
    ///
    /// # Panics
    ///
    /// If the file cannot be loaded or has no clip called `name`; the message
    /// lists the clips it has. See [`try_load_clip`](Self::try_load_clip).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let walk = ctx.gfx.load_clip("models/character.glb", "walk");
    /// # }
    /// ```
    pub fn load_clip(&mut self, path: &str, name: &str) -> AnimationClip {
        self.try_load_clip(path, name).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_clip`](Self::load_clip) with errors as values.
    ///
    /// # Errors
    ///
    /// Those of [`try_load_clips`](Self::try_load_clips), and
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// has no clip called `name` (the message lists the ones it has).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let walk = ctx.gfx.try_load_clip("models/character.glb", "walk");
    /// # }
    /// ```
    pub fn try_load_clip(&mut self, path: &str, name: &str) -> Result<AnimationClip, crate::error::Error> {
        let clips = self.try_load_clips(path)?;
        clips.iter().find(|c| c.name() == name).cloned().ok_or_else(|| {
            let names: Vec<_> = clips.iter().map(AnimationClip::name).collect();
            let message = format!("{path}: no animation {name:?}; available animations: {names:?}");
            crate::error::Error::new(crate::error::ErrorKind::NotFound, message).with_path(path)
        })
    }

    /// The lights a glTF file places (`KHR_lights_punctual`), relative to
    /// the model, loading the file like [`load_mesh`](Self::load_mesh) if it
    /// isn't yet: none for a file without lights. Spawn point and spot
    /// lights with [`ModelLight::spawn`](crate::render::ModelLight::spawn)
    /// where the model is; a sun goes on [`Lighting`](crate::render::Lighting).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let at = ponko::d3::Transform::at(vec3(0.0, 0.0, -10.0));
    /// let house = ctx.gfx.load_mesh("models/house.glb");
    /// ctx.world.spawn((at, ponko::render::Model::new(house)));
    /// for light in ctx.gfx.load_lights("models/house.glb") {
    ///     light.spawn(&mut ctx.world, &at);
    /// }
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// As [`load_mesh`](Self::load_mesh) does.
    pub fn load_lights(&mut self, path: &str) -> Vec<crate::render::ModelLight> {
        self.try_load_lights(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_lights`](Self::load_lights) with errors as values.
    ///
    /// # Errors
    ///
    /// Those of [`try_load_mesh`](Self::try_load_mesh).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_lights("models/missing.glb").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_lights(&mut self, path: &str) -> Result<Vec<crate::render::ModelLight>, crate::error::Error> {
        let (_, origin) = self.load_mesh_from(path)?;
        Ok(self
            .assets3d
            .loaded_placed
            .get(&origin)
            .map(|p| p.0.clone())
            .unwrap_or_default())
    }

    /// The cameras a glTF file places, relative to the model, loading the
    /// file like [`load_mesh`](Self::load_mesh) if it isn't yet: none for a
    /// file without cameras. Look through one with
    /// [`ModelCamera::apply`](crate::render::ModelCamera::apply).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let cameras = ctx.gfx.load_cameras("levels/intro.glb");
    /// if let Some(shot) = cameras.iter().find(|c| c.name == "opening") {
    ///     shot.apply(&mut ctx.scene3d.camera, &ponko::d3::Transform::default());
    /// }
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// As [`load_mesh`](Self::load_mesh) does.
    pub fn load_cameras(&mut self, path: &str) -> Vec<crate::render::ModelCamera> {
        self.try_load_cameras(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_cameras`](Self::load_cameras) with errors as values.
    ///
    /// # Errors
    ///
    /// Those of [`try_load_mesh`](Self::try_load_mesh).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_cameras("models/missing.glb").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_cameras(&mut self, path: &str) -> Result<Vec<crate::render::ModelCamera>, crate::error::Error> {
        let (_, origin) = self.load_mesh_from(path)?;
        Ok(self
            .assets3d
            .loaded_placed
            .get(&origin)
            .map(|p| p.1.clone())
            .unwrap_or_default())
    }

    /// Loads a glTF model (`.glb`, or `.gltf` with its files next to it)
    /// from an [added asset source](crate::assets::Assets::add_source) or `assets/` as
    /// one [`Mesh`]: every mesh in the file's scene, where the file puts it, with its materials and their base color, normal,
    /// occlusion-roughness-metalness and emissive maps.
    /// Loading the same file again returns the same mesh. After freeing its
    /// geometry, loading it again reuses its materials and textures, keeping
    /// edits made with [`update_material`](Self::update_material).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// Game::new("Forest", 1280, 720).run(|ctx| {
    ///     let tree = ctx.gfx.load_mesh("models/tree.glb");
    ///     for i in 0..100 {
    ///         let at = vec3((i % 10) as f32 * 4.0, 0.0, (i / 10) as f32 * -4.0);
    ///         ctx.world.spawn((ponko::d3::Transform::at(at), ponko::render::Model::new(tree)));
    ///     }
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// With a clear message if the file is missing or can't be read.
    pub fn load_mesh(&mut self, path: &str) -> Mesh {
        self.try_load_mesh(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_mesh`](Self::load_mesh) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// is missing, [`Io`](crate::error::ErrorKind::Io) if it can't be read,
    /// [`Decode`](crate::error::ErrorKind::Decode) if it isn't a glTF model
    /// this engine can read (including files it names that are missing).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let hat = ctx.gfx.try_load_mesh("models/hat.glb").unwrap_or(ponko::render::Mesh::NONE);
    ///     assert_eq!(hat, ponko::render::Mesh::NONE);
    /// });
    /// ```
    pub fn try_load_mesh(&mut self, path: &str) -> Result<Mesh, crate::error::Error> {
        self.load_mesh_from(path).map(|(mesh, _)| mesh)
    }

    /// Loads (or returns the cached) mesh `path`, and says where it was
    /// found: the key of its skeleton, clips and materials.
    pub(crate) fn load_mesh_from(&mut self, path: &str) -> Result<(Mesh, assets::Origin), crate::error::Error> {
        let found = match self
            .asset_sources
            .lookup(path, |origin| self.assets3d.loaded.get(origin).copied())?
        {
            assets::Lookup::Cached(origin, mesh) => return Ok((mesh, origin)),
            assets::Lookup::Found(found) => found,
        };
        let file = found.origin.clone();
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let (loaded, image_keys) = self.decode_gltf(path, &bytes)?;
        Ok((self.mesh_from_loaded(path, file.clone(), loaded, image_keys), file))
    }

    /// Decodes the glTF file `path` from `bytes` on the game thread, without
    /// the pixels of images another model already shares, and works out
    /// each image's key (see [`image_keys`](Self::image_keys)).
    pub(crate) fn decode_gltf(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<(gltf::Loaded, Vec<Option<SharedImage>>), crate::error::Error> {
        let shared = &self.assets3d.shared_images;
        let sources = &self.asset_sources;
        // Each image file's key: where it is found, so the same name in two
        // sources is two images. Worked out once per file.
        let keys = std::cell::RefCell::new(std::collections::HashMap::<String, Option<SharedImage>>::new());
        let key_of = |source: &str| {
            keys.borrow_mut()
                .entry(source.to_string())
                .or_insert_with(|| SharedImage::of(sources, source))
                .clone()
        };
        let gpu = self.model_formats();
        // The model's pictures follow the image limits of the source it came from.
        let in_source = |f: &mut dyn FnMut() -> Result<gltf::Loaded, String>| match sources.locate(path) {
            Some(origin) => sources.image_limits().scope(&origin, f),
            None => f(),
        };
        let loaded = in_source(&mut || {
            gltf::load_for(
                path,
                bytes,
                &mut |name| sources.read(name),
                &|source| key_of(source).is_some_and(|key| shared.contains_key(&key)),
                gpu,
            )
        })
        .map_err(|e| crate::error::Error::decode(path, e))?;
        let image_keys = loaded
            .image_sources
            .iter()
            .map(|source| source.as_deref().and_then(key_of))
            .collect();
        Ok((loaded, image_keys))
    }

    /// Reads glTF file `path` node by node (`gltf::load_nodes`) and makes a
    /// mesh of each node that has triangles, in the node's own space, with
    /// the file's materials (shared with [`load_mesh`](Self::load_mesh)).
    /// Each call makes new meshes; the caller frees them, unless `reuse`
    /// holds meshes made for the same file before (another spawn's), all
    /// live and one for each node that has triangles: then those are
    /// returned again, and the file's images aren't decoded; if `read` (what
    /// an earlier call returned for the file) holds the file as it still is,
    /// the file isn't parsed either. `refresh` (the file changed) never
    /// reuses, reads its images again and updates its materials in place.
    /// Returns the nodes as read, to keep for the next call, and where the
    /// file is on disk, when it is a file.
    pub(crate) fn load_node_meshes(
        &mut self,
        path: &str,
        refresh: bool,
        reuse: Option<&[Option<Mesh>]>,
        read: Option<&std::sync::Arc<gltf::ReadNodes>>,
    ) -> Result<NodeMeshes, crate::error::Error> {
        let found = self.asset_sources.find(path)?;
        let file = found.origin.clone();
        let on_disk = match &file {
            assets::Origin::File(at) => Some(at.clone()),
            assets::Origin::Source { .. } => None,
        };
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let reuse = reuse.filter(|meshes| !refresh && meshes.iter().flatten().all(|&m| self.is_live(m)));
        let shared = &self.assets3d.shared_images;
        let sources = &self.asset_sources;
        // Every mesh is there and the file is as it was read: nothing to parse.
        let fits = |meshes: &[Option<Mesh>], nodes: &[gltf::LoadedNode]| {
            meshes.len() == nodes.len() && meshes.iter().zip(nodes).all(|(m, n)| m.is_some() == n.mesh.is_some())
        };
        if let (Some(meshes), Some(read)) = (reuse, read)
            && fits(meshes, &read.nodes)
            && read.unchanged(&bytes, &mut |name| sources.read(name))
        {
            return Ok((std::sync::Arc::clone(read), meshes.to_vec(), on_disk));
        }
        let gpu = self.model_formats();
        self.assets3d.gltf_scene_parses += 1;
        let loaded = sources
            .image_limits()
            .scope(&file, || {
                gltf::load_nodes(
                    path,
                    &bytes,
                    &mut |name| sources.read(name),
                    &|source| {
                        reuse.is_some()
                            || (!refresh
                                && SharedImage::of(sources, source).is_some_and(|key| shared.contains_key(&key)))
                    },
                    gpu,
                )
            })
            .map_err(|e| crate::error::Error::decode(path, e))?;
        if let Some(meshes) = reuse
            && fits(meshes, &loaded.nodes)
        {
            let read = gltf::ReadNodes::keep(loaded, &bytes);
            return Ok((std::sync::Arc::new(read), meshes.to_vec(), on_disk));
        }
        if reuse.is_some() {
            // The file no longer matches the meshes (changed on disk, with
            // hot reload off): read it whole.
            return self.load_node_meshes(path, refresh, None, None);
        }
        let image_keys = loaded
            .image_sources
            .iter()
            .map(|source| source.as_deref().and_then(|s| SharedImage::of(sources, s)))
            .collect();
        let images = (&loaded.images[..], &loaded.blocks[..]);
        let materials = self.file_materials(path, &file, &loaded.materials, images, image_keys, refresh);
        // Nodes of one glTF mesh share one uploaded mesh.
        let mut meshes: Vec<Option<Mesh>> = Vec::with_capacity(loaded.nodes.len());
        for node in &loaded.nodes {
            let mesh = match node.same_mesh_as {
                Some(first) => meshes[first],
                None => node.mesh.as_ref().map(|m| {
                    let parts: Vec<(Range<u32>, Material)> = m
                        .parts
                        .iter()
                        .map(|(range, i)| (range.clone(), materials[*i]))
                        .collect();
                    let mesh = self.mesh_from_parts(&m.data, &parts, None, m.morph_defaults.clone());
                    self.asset_ledger.own(crate::inventory::AssetClass::Mesh, mesh.id, path);
                    mesh
                }),
            };
            meshes.push(mesh);
        }
        let read = gltf::ReadNodes::keep(loaded, &bytes);
        Ok((std::sync::Arc::new(read), meshes, on_disk))
    }

    /// The block formats a model's KTX2 images stay in (see
    /// [`gltf::GpuFormats`]): the GPU's.
    pub(crate) fn model_formats(&self) -> gltf::GpuFormats {
        #[cfg(feature = "compressed-textures")]
        return self.compressed_formats();
        #[cfg(not(feature = "compressed-textures"))]
        gltf::GpuFormats {}
    }

    /// A texture of a model's image: its blocks uploaded as they are, else
    /// its RGBA8.
    fn model_texture(&mut self, image: &gltf::Image, blocks: Option<&gltf::Blocks>) -> crate::texture::Texture {
        #[cfg(feature = "compressed-textures")]
        if let Some(blocks) = blocks {
            let texture = self.make_compressed_texture(blocks);
            self.textures_live.insert(texture.id);
            return texture;
        }
        #[cfg(not(feature = "compressed-textures"))]
        if let Some(never) = blocks {
            match *never {}
        }
        self.create_texture(image.0, image.1, &image.2)
    }

    /// Frees the materials and textures glTF file `path` made, as
    /// [`unload_mesh`](Self::unload_mesh) does, unless
    /// [`load_mesh`](Self::load_mesh) holds a mesh of the same file (which
    /// draws with them): the last scene spawned from the file is gone.
    /// The custom shader material `m` of glTF file `path` names in its
    /// extras, loaded; `None` (with a warning) if it doesn't load.
    pub(super) fn extras_shader(&mut self, path: &str, m: &gltf::LoadedMaterial) -> Option<Shader> {
        let name = m.shader.as_deref()?;
        match self.try_load_shader(name) {
            Ok(shader) => Some(shader),
            Err(e) => {
                super::gltf_ext::warn(&format!(
                    "{path}: material \"{}\": its shader didn't load, it is drawn with the built-in one: {e}",
                    m.name
                ));
                None
            }
        }
    }

    pub(crate) fn release_file_materials(&mut self, path: &str) {
        let held = self
            .asset_sources
            .locate(path)
            .is_some_and(|file| self.assets3d.loaded.contains_key(&file));
        if !held {
            self.unload_mesh(path);
        }
    }

    /// The shared-image key of each image of a decoded glTF file (a file
    /// decoded on a worker, which can't look at the asset sources).
    pub(crate) fn image_keys(&self, loaded: &gltf::Loaded) -> Vec<Option<SharedImage>> {
        loaded
            .image_sources
            .iter()
            .map(|source| {
                source
                    .as_deref()
                    .and_then(|source| SharedImage::of(&self.asset_sources, source))
            })
            .collect()
    }

    /// Makes the mesh, materials and textures of a decoded glTF file
    /// `path` found at `file`, and caches it (the game-thread half of
    /// `load_mesh_from`; streaming decodes on a worker first). `image_keys`
    /// has the shared-image key of each of the file's images.
    pub(crate) fn mesh_from_loaded(
        &mut self,
        path: &str,
        file: assets::Origin,
        loaded: gltf::Loaded,
        image_keys: Vec<Option<SharedImage>>,
    ) -> Mesh {
        let images = (&loaded.images[..], &loaded.blocks[..]);
        let materials = self.file_materials(path, &file, &loaded.materials, images, image_keys, false);
        let parts: Vec<(Range<u32>, Material)> = loaded
            .parts
            .iter()
            .map(|(range, m)| (range.clone(), materials[*m]))
            .collect();
        self.assets3d.loaded_clips.insert(file.clone(), loaded.clips);
        self.assets3d
            .loaded_placed
            .insert(file.clone(), (loaded.lights, loaded.cameras));
        let skin = loaded.skin.map(|skin| {
            let skeleton = Skeleton::from_shared(skin.joints);
            let mesh_skin = MeshSkin::new(
                skeleton.shared_joints().clone(),
                skin.slots,
                skin.inverse_binds,
                &loaded.data,
            );
            self.assets3d.loaded_skeletons.insert(file.clone(), skeleton);
            mesh_skin
        });
        let mesh = self.mesh_from_parts(&loaded.data, &parts, skin, loaded.morph_defaults);
        self.assets3d.loaded.insert(file.clone(), mesh);
        self.asset_ledger.own(crate::inventory::AssetClass::Mesh, mesh.id, path);
        self.mesh_names.set(mesh.id, path, mesh);
        let named = materials
            .iter()
            .zip(&loaded.materials)
            .map(|(m, l)| (*m, l.name.clone()));
        self.material_names.add(path, named);
        mesh
    }

    /// The materials of glTF file `file` (loaded as `path`): the ones made
    /// when it was first loaded, which survive freeing its mesh and keep
    /// edits made with `update_material`. Only those the game freed since
    /// are made again, from the file's textures that are still live, so a
    /// file loaded, partly freed and loaded again never leaves unreachable
    /// materials or textures behind.
    ///
    /// `refresh` (a file that changed on disk) makes its textures again from
    /// `images` and updates its live materials in place, so what shows them
    /// shows the new file; textures no other file uses are freed. An image
    /// with blocks (a KTX2 image the GPU samples) uploads as it is.
    pub(super) fn file_materials(
        &mut self,
        path: &str,
        file: &assets::Origin,
        loaded_materials: &[gltf::LoadedMaterial],
        (images, blocks): (&[gltf::Image], &[Option<gltf::Blocks>]),
        image_keys: Vec<Option<SharedImage>>,
        refresh: bool,
    ) -> Vec<Material> {
        let live = |gfx: &Self, m: &Material| gfx.assets3d.materials_live.contains(m.id);
        let kept = self.assets3d.file_materials.get(file).cloned().unwrap_or_default();
        if !refresh && kept.len() == loaded_materials.len() && kept.iter().all(|m| live(self, m)) {
            return kept;
        }
        let old_textures = self.assets3d.file_textures.get(file).cloned().unwrap_or_default();
        let textures: Vec<crate::texture::Texture> = images
            .iter()
            .zip(image_keys)
            .enumerate()
            .map(|(i, (image, key))| {
                let blocks = blocks.get(i).and_then(Option::as_ref);
                if let Some(&t) = old_textures
                    .get(i)
                    .filter(|t| !refresh && self.textures_live.contains(t.id))
                {
                    return t;
                }
                match key {
                    Some(key) => match self.assets3d.shared_images.get(&key).filter(|_| !refresh) {
                        Some(&t) => t,
                        None => {
                            let t = self.model_texture(image, blocks);
                            self.assets3d.shared_images.insert(key, t);
                            t
                        }
                    },
                    None => self.model_texture(image, blocks),
                }
            })
            .collect();
        for t in &textures {
            self.asset_ledger.own(crate::inventory::AssetClass::Texture, t.id, path);
        }
        self.assets3d.file_textures.insert(file.clone(), textures.clone());
        if refresh {
            for t in old_textures {
                let used = textures.contains(&t) || self.assets3d.file_textures.values().any(|ts| ts.contains(&t));
                if !used && self.textures_live.contains(t.id) {
                    self.free(t);
                }
            }
        }
        let materials: Vec<Material> = loaded_materials
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let old = kept.get(i).copied().filter(|old| live(self, old));
                if let Some(old) = old
                    && !refresh
                {
                    return old;
                }
                let texture = |i: Option<usize>| i.map(|i| textures[i]);
                let desc = MaterialDesc {
                    color: m.color,
                    texture: texture(m.image),
                    roughness: m.roughness.clamp(0.0, 1.0),
                    metallic: m.metallic.clamp(0.0, 1.0),
                    emissive: m.emissive,
                    normal_map: texture(m.normal_map),
                    normal_strength: m.normal_strength,
                    orm_map: texture(m.orm_map),
                    emissive_map: texture(m.emissive_map),
                    blend: m.blend,
                    double_sided: m.double_sided,
                    custom: m.custom.unwrap_or_default(),
                    shader: self.extras_shader(path, m),
                    ..Default::default()
                };
                if let Some(old) = old {
                    self.update_material(old, desc);
                    return old;
                }
                let material = self.create_material(desc);
                self.asset_ledger
                    .own(crate::inventory::AssetClass::Material, material.id, path);
                if !m.name.is_empty() {
                    self.assets3d
                        .loaded_materials
                        .entry(m.name.clone())
                        .or_default()
                        .push((material, desc));
                }
                material
            })
            .collect();
        self.assets3d.file_materials.insert(file.clone(), materials.clone());
        materials
    }

    /// Frees everything loading glTF file `path` made: its mesh, its
    /// materials and its textures, except images another loaded file
    /// shares. For a long-running game that loads many different models:
    /// freeing only the mesh keeps the materials and textures for the next
    /// load of the same file. Whatever still shows them draws with the
    /// default, as [`free`](Self::free) describes. False if `path` was not
    /// loaded (or was unloaded already).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let before = ctx.live_counts();
    ///     let house = ctx.gfx.load_mesh("models/house.glb");
    ///     ctx.gfx.free(house);
    ///     assert_ne!(ctx.live_counts(), before, "its materials and textures stay");
    ///     assert!(ctx.gfx.unload_mesh("models/house.glb"));
    ///     assert_eq!(ctx.live_counts(), before);
    ///     assert!(!ctx.gfx.unload_mesh("models/house.glb"));
    /// });
    /// ```
    pub fn unload_mesh(&mut self, path: &str) -> bool {
        let Some(file) = self.asset_sources.locate(path) else {
            return false;
        };
        let mesh = self.assets3d.loaded.get(&file).copied();
        let materials = self.assets3d.file_materials.remove(&file);
        let textures = self.assets3d.file_textures.remove(&file);
        if mesh.is_none() && materials.is_none() && textures.is_none() {
            return false;
        }
        if let Some(mesh) = mesh {
            self.free(mesh);
        }
        for material in materials.unwrap_or_default() {
            self.free(material);
        }
        for texture in textures.unwrap_or_default() {
            let shared = self.assets3d.file_textures.values().any(|ts| ts.contains(&texture));
            if !shared {
                self.free(texture);
            }
        }
        self.material_names.forget_file(path);
        true
    }

    /// A glTF file's mesh on the CPU (all its parts in one, placed as in
    /// the file), without making anything on the GPU: to cut
    /// [decals](MeshData::decal) from, build collision shapes, or read its
    /// corners. Loads the file each call; keep the result.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let wall = ctx.gfx.load_mesh_data("models/wall.glb");
    /// let hole = wall.decal(Mat4::from_scale(Vec3::splat(0.1)), 0.002);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the file is missing or isn't a glTF file with triangles; use
    /// [`try_load_mesh_data`](Self::try_load_mesh_data) to handle that.
    pub fn load_mesh_data(&self, path: &str) -> MeshData {
        self.try_load_mesh_data(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_mesh_data`](Self::load_mesh_data) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// is missing, [`Io`](crate::error::ErrorKind::Io) if it can't be read,
    /// [`Decode`](crate::error::ErrorKind::Decode) if it isn't a glTF file with
    /// triangles (including files it names that are missing).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn wall(ctx: &Ctx) -> Result<ponko::render::MeshData, Box<dyn std::error::Error>> {
    ///     Ok(ctx.gfx.try_load_mesh_data("models/wall.glb")?)
    /// }
    /// Game::new("t", 1, 1).run_headless(0, |ctx| assert!(wall(ctx).is_err()));
    /// ```
    pub fn try_load_mesh_data(&self, path: &str) -> Result<MeshData, crate::error::Error> {
        let found = self.asset_sources.find(path)?;
        let origin = found.origin.clone();
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let sources = &self.asset_sources;
        let loaded = sources
            .image_limits()
            .scope(&origin, || {
                gltf::load(path, &bytes, &mut |name| sources.read(name), &|_| true)
            })
            .map_err(|e| crate::error::Error::decode(path, e))?;
        Ok(loaded.data)
    }

    /// A mesh whose index ranges each have their own material.
    /// With `skin`, it bends with a skeleton. Its morph targets, if any,
    /// show `morph_defaults` on models without [`MorphWeights`](crate::anim::MorphWeights).
    fn mesh_from_parts(
        &mut self,
        data: &MeshData,
        parts: &[(Range<u32>, Material)],
        skin: Option<MeshSkin>,
        morph_defaults: Vec<f32>,
    ) -> Mesh {
        // A freed mesh's id first, so streaming meshes in and out doesn't
        // grow the tables (and the per-frame sort, which spans every id).
        // A resident mesh coming back after eviction takes its old id, so
        // handles the game kept stay good.
        // A freed slot comes back under a new id (its generation counted
        // up), so handles to the freed mesh stay dead.
        let id = match self.assets3d.reuse_mesh_id.take() {
            Some(id) => {
                self.assets3d.mesh_ids.remake(id);
                id
            }
            None => self.assets3d.mesh_ids.make(self.assets3d.bounds.len(), "mesh"),
        };
        let slot = id & super::SLOT_MASK;
        if slot as usize == self.assets3d.bounds.len() {
            self.assets3d.bounds.push(data.bounds());
        } else {
            self.assets3d.gpu_mesh_bounds.remove(&slot);
            self.assets3d.bounds[slot as usize] = data.bounds();
        }
        self.asset_ledger.made(crate::inventory::AssetClass::Mesh, id);
        if let Some(sources) = self.assets3d.bake_sources.as_mut() {
            sources.mesh_made(id, data, parts);
        }
        let mesh = Mesh { id };
        let slot = mesh.slot() as usize;
        if self.assets3d.mesh_bytes.len() <= slot {
            self.assets3d.mesh_bytes.resize(slot + 1, 0);
        }
        self.assets3d.mesh_bytes[slot] = super::mesh_gpu_bytes(data);
        let posed = skin.is_some() || !data.morph_targets.is_empty();
        if let Some(skin) = skin {
            if self.assets3d.skins.len() <= slot {
                self.assets3d.skins.resize(slot + 1, None);
            }
            self.assets3d.skins[slot] = Some(std::sync::Arc::new(skin));
        }
        if let Some(morph) = MeshMorph::new(data, morph_defaults) {
            if self.assets3d.morphs.len() <= slot {
                self.assets3d.morphs.resize_with(slot + 1, || None);
            }
            self.assets3d.morphs[slot] = Some(std::sync::Arc::new(morph));
        }
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.create_mesh(mesh.slot(), data, parts, posed);
        }
        mesh
    }

    /// The index of `mesh`'s morph target called `name`, for
    /// [`MorphWeights`](crate::anim::MorphWeights); `None` if it has no such
    /// target (or was freed).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mut quad = ponko::render::MeshData::plane(1.0);
    ///     let up = vec![vec3(0.0, 0.1, 0.0); quad.positions.len()];
    ///     quad.morph_targets.push(ponko::anim::MorphTarget::new("lift", up));
    ///     let material = ctx.gfx.default_material();
    ///     let mesh = ctx.gfx.create_mesh(&quad, material);
    ///     assert_eq!(ctx.gfx.morph_target(mesh, "lift"), Some(0));
    ///     assert_eq!(ctx.gfx.morph_target(mesh, "sag"), None);
    /// });
    /// ```
    #[must_use]
    pub fn morph_target(&self, mesh: Mesh, name: &str) -> Option<usize> {
        self.assets3d.live_bounds(mesh.id)?;
        self.assets3d.morph(mesh.id)?.names.iter().position(|n| n == name)
    }

    /// Every material loaded from glTF files so far that the files call
    /// `name`, with what it was made from: to give materials of imported
    /// models a shader of your own, keep their textures and change the
    /// rest with [`Graphics::update_material`](crate::gfx::Graphics::update_material). Models loaded later aren't in it.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.load_scene("town/Town.json").unwrap();
    /// let brick = ctx.gfx.load_shader("shaders/brick.wgsl");
    /// for (material, desc) in ctx.gfx.loaded_materials("Brick") {
    ///     ctx.gfx.update_material(material, desc.with_shader(brick));
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn loaded_materials(&self, name: &str) -> Vec<(Material, MaterialDesc)> {
        let mut out = Vec::new();
        self.loaded_materials_into(name, &mut out);
        out
    }

    /// Replaces `out` with what [`loaded_materials`](Self::loaded_materials)
    /// returns for `name`: the old contents are dropped (empty when no glTF
    /// material has that name) and its capacity is reused, so a loop that
    /// keeps one vector doesn't allocate once it has grown.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mut found = Vec::new();
    ///     ctx.gfx.loaded_materials_into("Brick", &mut found);
    ///     assert!(found.is_empty(), "nothing is loaded yet");
    /// });
    /// ```
    pub fn loaded_materials_into(&self, name: &str, out: &mut Vec<(Material, MaterialDesc)>) {
        out.clear();
        if let Some(materials) = self.assets3d.loaded_materials.get(name) {
            out.extend_from_slice(materials);
        }
    }

    /// Frees mesh `id`: the engine side of [`Graphics::free`] with a mesh.
    pub(crate) fn free_mesh_id(&mut self, id: u32) {
        let mesh = Mesh { id };
        // A stale handle (its slot given to a newer mesh) frees nothing.
        let Some(slot) = self.assets3d.mesh_slot(id) else {
            return;
        };
        let radius = self.assets3d.bounds[slot].1;
        // Already freed: maybe an evicted resident mesh's kept id, which
        // freeing its handle releases.
        if radius < 0.0 {
            self.release_resident_mesh_id(mesh.id, false);
            return;
        }
        self.release_resident_mesh_id(mesh.id, true);
        self.asset_ledger.freed(crate::inventory::AssetClass::Mesh, mesh.id);
        if let Some(sources) = self.assets3d.bake_sources.as_mut() {
            sources.mesh_freed(mesh.id);
        }
        self.assets3d.bounds[slot] = (Vec3::ZERO, -1.0);
        if let Some(bytes) = self.assets3d.mesh_bytes.get_mut(slot) {
            *bytes = 0;
        }
        self.assets3d.gpu_mesh_bounds.remove(&mesh.slot());
        self.assets3d.mesh_ids.release(mesh.id);
        if let Some(shadowless) = self.assets3d.shadowless.get_mut(slot) {
            *shadowless = false;
        }
        if let Some(skin) = self.assets3d.skins.get_mut(slot) {
            *skin = None;
        }
        if let Some(morph) = self.assets3d.morphs.get_mut(slot) {
            *morph = None;
        }
        let freed: Vec<_> = self
            .assets3d
            .loaded
            .iter()
            .filter(|(_, m)| **m == mesh)
            .map(|(f, _)| f.clone())
            .collect();
        for file in freed {
            self.assets3d.loaded_skeletons.remove(&file);
            self.assets3d.loaded_clips.remove(&file);
            self.assets3d.loaded_placed.remove(&file);
        }
        self.assets3d.loaded.retain(|_, m| *m != mesh);
        self.mesh_names.remove(mesh.id);
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.free_mesh(mesh.slot());
        }
    }

    /// Whether models showing `mesh` cast shadows: on for every mesh at
    /// first. Turn it off for small things that crowd the ground, such as
    /// grass and pebbles: their shadows are too small to see but drawing
    /// them into the shadow map costs as much as drawing them.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let material = ctx.gfx.default_material();
    ///     let tuft = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(0.2), material);
    ///     ctx.gfx.set_mesh_shadows(tuft, false);
    ///     assert!(!ctx.gfx.mesh_shadows(tuft));
    /// });
    /// ```
    pub fn set_mesh_shadows(&mut self, mesh: Mesh, casts: bool) {
        let Some(id) = self.assets3d.live_bounds(mesh.id).and(self.assets3d.mesh_slot(mesh.id)) else {
            return;
        };
        let shadowless = &mut self.assets3d.shadowless;
        if shadowless.len() <= id {
            if casts {
                return;
            }
            shadowless.resize(id + 1, false);
        }
        shadowless[id] = !casts;
    }

    /// Whether models showing `mesh` cast shadows (see
    /// [`set_mesh_shadows`](Self::set_mesh_shadows)).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let material = ctx.gfx.default_material();
    ///     let house = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(4.0), material);
    ///     assert!(ctx.gfx.mesh_shadows(house));
    /// });
    /// ```
    #[must_use]
    pub fn mesh_shadows(&self, mesh: Mesh) -> bool {
        // A stale handle reads the default, not the setting of the mesh
        // made after it in its slot.
        self.assets3d.mesh_slot(mesh.id).is_none() || self.assets3d.casts_shadows(mesh.id)
    }

    /// How many 3D models were drawn last frame: those the camera could
    /// see, and those out of its view whose shadow can fall into it. Each
    /// model counts once. A model in a shadow cascade whose map is kept
    /// this frame ([`crate::render::Lighting::shadow_refresh`]) counts too.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// println!("{} models on screen", ctx.gfx.models_drawn());
    /// # }
    /// ```
    #[must_use]
    pub fn models_drawn(&self) -> usize {
        self.frame3d.models
    }

    /// Makes TAA, temporal ambient occlusion and screen-space reflections
    /// forget the frames before this one: call it when the camera cuts to
    /// somewhere else (a teleport, a new scene), so the old view does not
    /// blend into the new one. Call this after teleporting objects too:
    /// on this frame no model has motion (the old position is not its past).
    /// Auto exposure ([`Post::auto_exposure`](crate::render::Post::auto_exposure))
    /// starts over at the exposure the new view needs, instead of adapting
    /// to it.
    ///
    /// The engine resets by itself on a window resize or render scale
    /// change, when temporal antialiasing or another screen effect is
    /// turned on or off, after MSAA changes, shader reloads, presentation
    /// interruptions or surface recreation, and when the camera jumps far
    /// or turns sharply in one frame. A reset asked for on a frame that draws nothing applies
    /// to the next frame that does.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     ctx.scene3d.post.anti_aliasing = ponko::render::AntiAliasing::Taa;
    ///     ctx.scene3d.camera.position = vec3(500.0, 2.0, 0.0);
    ///     ctx.gfx.reset_temporal_history();
    /// });
    /// ```
    pub fn reset_temporal_history(&mut self) {
        self.frame3d.motion.reset = true;
    }

    /// Gives `entity`'s model no motion on the next drawn frame, as
    /// [`reset_temporal_history`](Self::reset_temporal_history) does for
    /// every model, but leaving the rest of the picture's history alone.
    /// Call it after teleporting one object, or when its animation snaps to
    /// another pose (a cut between clips with no blend), so TAA and
    /// temporal AO don't smear it along a path it never took. The frame
    /// after, its motion is measured from where it was drawn on this one.
    /// For a level-of-detail group, pass the group's entity; for a model
    /// posed through [`SkinOf`](crate::anim::SkinOf), the model's.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(1, |ctx| {
    ///     let hero = ctx.world.spawn((ponko::d3::Transform::default(),));
    ///     ctx.world.get_mut::<ponko::d3::Transform>(hero).unwrap().position = vec3(100.0, 0.0, 0.0);
    ///     ctx.gfx.reset_motion(hero);
    /// });
    /// ```
    pub fn reset_motion(&mut self, entity: ponko_ecs::Entity) {
        let motion = &mut self.frame3d.motion;
        // Kept until a frame draws. Past this many (a crowd, or frames that
        // draw nothing), the whole history is reset instead.
        if motion.forget.len() >= super::motion::MAX_FORGET {
            motion.forget.clear();
            motion.reset = true;
        } else if !motion.reset {
            motion.forget.push(entity.to_bits());
        }
    }

    /// What each mesh cost to draw in the last frame: the models and
    /// triangles the camera drew with it, and the same for the shadow maps,
    /// most triangles first. For finding what makes a frame slow; it
    /// allocates, so call it now and then, not every frame. Empty headless.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn heaviest(ctx: &mut Ctx) {
    ///     for c in ctx.gfx.mesh_costs().iter().take(5) {
    ///         let name = c.name.as_deref().unwrap_or("(made in code)");
    ///         println!("{name}: {} models, {} + {} shadow triangles", c.models, c.triangles, c.shadow_triangles);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(heaviest).run_headless(1, |_| {});
    /// ```
    pub fn mesh_costs(&self) -> Vec<super::MeshCost> {
        let mut out = Vec::new();
        self.mesh_costs_into(&mut out);
        out
    }

    /// Replaces `out` with [`mesh_costs`](Self::mesh_costs): largest triangle
    /// total first, the mesh id breaking ties. The old contents are dropped,
    /// and the vector and the records' name strings are reused, so only new
    /// records or longer names allocate. `out` is empty headless or before any
    /// draw.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut costs = Vec::new();
    /// let mut heaviest = move |ctx: &mut Ctx| {
    ///     ctx.gfx.mesh_costs_into(&mut costs);
    ///     for c in costs.iter().take(5) {
    ///         println!("{} triangles", c.triangles + c.shadow_triangles);
    ///     }
    /// };
    /// Game::new("t", 1, 1).system(move |ctx: &mut Ctx| heaviest(ctx)).run_headless(1, |_| {});
    /// ```
    pub fn mesh_costs_into(&self, out: &mut Vec<super::MeshCost>) {
        let Some(renderer) = self.renderer.as_ref() else {
            out.clear();
            return;
        };
        let mut len = 0;
        renderer.visit_mesh_costs(|slot, models, triangles, shadow_models, shadow_triangles| {
            let mesh = self.assets3d.mesh_in_slot(slot);
            let at = out[..len].iter().position(|c| c.mesh == mesh).unwrap_or_else(|| {
                let at = len;
                // Keep a loaded mesh's name with its handle when cost sorting
                // changes the order relative to renderer runs.
                if let Some(found) = out[at..].iter().position(|c| c.mesh == mesh) {
                    out.swap(at, at + found);
                }
                if at == out.len() {
                    out.push(super::MeshCost {
                        mesh,
                        name: None,
                        models: 0,
                        triangles: 0,
                        shadow_models: 0,
                        shadow_triangles: 0,
                    });
                }
                let cost = &mut out[at];
                cost.mesh = mesh;
                cost.models = 0;
                cost.triangles = 0;
                cost.shadow_models = 0;
                cost.shadow_triangles = 0;
                if let Some((origin, _)) = self.assets3d.loaded.iter().find(|(_, m)| **m == mesh) {
                    origin.display_into(cost.name.get_or_insert_with(String::new));
                } else {
                    cost.name = None;
                }
                len += 1;
                at
            });
            let cost = &mut out[at];
            cost.models += models;
            cost.triangles += triangles;
            cost.shadow_models += shadow_models;
            cost.shadow_triangles += shadow_triangles;
        });
        out.truncate(len);
        out.sort_unstable_by_key(|c| (std::cmp::Reverse(c.triangles + c.shadow_triangles), c.mesh.id));
    }

    /// Writes sampled skeleton morph channels to the models of `world`,
    /// including SkinOf attachments. Automatic animators call this each
    /// frame; call it after manual sampling. Unanimated weights retain their
    /// current values.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("morph", 1, 1).run_headless(0, |ctx| ctx.gfx.write_animated_morphs(&mut ctx.world));
    /// ```
    pub fn write_animated_morphs(&mut self, world: &mut World) {
        let mut entities = std::mem::take(&mut self.assets3d.morph_entities);
        entities.clear();
        // Only models whose mesh has morph targets: this runs every frame,
        // and a large world has hundreds of thousands of models.
        let morphs = &self.assets3d.morphs;
        if morphs.iter().all(Option::is_none) {
            self.assets3d.morph_bases.clear();
            self.assets3d.morph_entities = entities;
            return;
        }
        world.query::<(ponko_ecs::Entity, &super::Model)>().for_each(|(e, m)| {
            if morphs.get(m.mesh.slot() as usize).is_some_and(Option::is_some) {
                entities.push(e);
            }
        });
        for e in entities.iter().copied() {
            let mesh = world.get::<super::Model>(e).unwrap().mesh;
            let Some(morph) = self.assets3d.morphs.get(mesh.slot() as usize).and_then(|m| m.clone()) else {
                continue;
            };
            let owner = world.get::<super::SkinOf>(e).map_or(e, |s| s.0);
            if world.get::<Skeleton>(owner).is_none_or(|s| s.morphs.is_empty()) {
                continue;
            }
            let mut weights = world
                .get_mut::<super::MorphWeights>(e)
                .map(|w| std::mem::take(&mut *w))
                .unwrap_or_else(|| super::MorphWeights(morph.defaults.clone()));
            let base = self
                .assets3d
                .morph_bases
                .entry(e)
                .or_insert_with(|| (mesh.id, weights.clone(), weights.clone()));
            if base.0 != mesh.id {
                base.0 = mesh.id;
                base.1.clone_from(&weights);
            }
            for i in 0..weights.0.len().max(base.2.0.len()) {
                if weights.get(i) != base.2.get(i) {
                    base.1.set(i, weights.get(i));
                }
            }
            world
                .get::<Skeleton>(owner)
                .unwrap()
                .morphs
                .write_base(&morph.names, &base.1, &mut weights);
            base.2.clone_from(&weights);
            if world.get::<super::MorphWeights>(e).is_some() {
                *world.get_mut::<super::MorphWeights>(e).unwrap() = weights;
            } else {
                world.insert(e, weights);
            }
        }
        self.assets3d.morph_bases.retain(|e, _| world.is_alive(*e));
        self.assets3d.morph_entities = entities;
    }
}

impl Ctx {
    /// Engine side, before drawing: collect the visible models and hand
    /// them, the camera and the light to the renderer.
    /// What each target camera into a real render target sees.
    pub(crate) fn collect_views(&mut self, skins: &super::SkinFrame, fallback: Option<Material>) {
        let mut capture_cameras = Vec::new();
        self.gfx.frame3d.probe_bakes.clear();
        self.gfx.frame3d.probe_uploads.clear();
        self.gfx.frame3d.probe_refresh.clear();
        self.gfx.assets3d.probes.take_requests();
        self.gfx
            .frame3d
            .probe_uploads
            .extend(self.gfx.assets3d.probes.in_flight_uploads.iter().cloned());
        let face_camera = |base: super::Camera3D, center: Vec3, face: usize| {
            let (direction, up) = match face {
                0 => (Vec3::X, Vec3::Y),
                1 => (-Vec3::X, Vec3::Y),
                2 => (Vec3::Y, -Vec3::Z),
                3 => (-Vec3::Y, Vec3::Z),
                4 => (Vec3::Z, Vec3::Y),
                _ => (-Vec3::Z, Vec3::Y),
            };
            let mut camera = base;
            camera.position = center;
            camera.rotation = glam::Quat::from_mat3(&glam::Mat3::from_cols(direction.cross(up), up, -direction));
            camera.fov_y = std::f32::consts::FRAC_PI_2;
            camera.near = 0.05;
            camera
        };
        for &(slot, center) in &self.gfx.assets3d.probes.in_flight {
            let faces = self.gfx.assets3d.probes.captures[slot as usize - 1];
            self.gfx.frame3d.probe_bakes.push((slot, faces.map(|t| t.id)));
            for (face, target) in faces.into_iter().enumerate() {
                // Probes keep the scene, not what passes through it.
                capture_cameras.push(
                    super::TargetCamera::new(
                        face_camera(self.scene3d.camera, center, face),
                        super::RenderTarget::from_texture(target),
                    )
                    .with_particles(false),
                );
            }
        }
        // Automatic refreshes: at most one face a frame.
        let lighting = super::probe::lighting_key(&self.scene3d.lighting, &self.scene3d.sky);
        let mut steps = std::mem::take(&mut self.gfx.frame3d.probe_refresh);
        self.gfx
            .schedule_probe_refresh(&mut self.world, self.time.frame_dt(), &lighting, &mut steps);
        for step in &steps {
            if let &super::probe::RefreshStep::Capture { slot, face, .. } = step {
                let state = &self.gfx.assets3d.probes.refresh.slots[slot as usize - 1];
                let target = self.gfx.assets3d.probes.captures[slot as usize - 1][face as usize];
                capture_cameras.push(
                    super::TargetCamera::new(
                        face_camera(self.scene3d.camera, state.center, face as usize),
                        super::RenderTarget::from_texture(target),
                    )
                    .with_particles(false),
                );
            }
        }
        self.gfx.frame3d.probe_refresh = steps;
        self.gfx.frame3d.probes.fill(Default::default());
        for (t, p) in self
            .world
            .query::<(&super::Transform3D, &super::ReflectionProbe3D)>()
            .iter()
        {
            if (1..=4).contains(&p.content) && p.weight(t.position, t.position) > 0.0 {
                self.gfx.frame3d.probes[p.content as usize - 1] = super::probe::ProbeUniform {
                    center: t.position.extend(p.content as f32).to_array(),
                    size: (p.size * 0.5).extend(p.blend_distance.max(0.0)).to_array(),
                    params: [p.intensity, f32::from(p.box_projection), 0.0, 0.0],
                };
            }
        }
        // Icons drawn in all their frames are done: their materials go.
        let done = |i: &mut super::PendingIcon| i.frames >= super::ICON_FRAMES;
        for icon in self.gfx.icons.extract_if(.., done).collect::<Vec<_>>() {
            self.gfx.free(icon.material);
        }
        // Tone mapped as the main view is now.
        for i in 0..self.gfx.icons.len() {
            let (material, exposure) = (self.gfx.icons[i].material, self.gfx.icons[i].exposure);
            let desc = super::icon::icon_material(exposure, self.scene3d.post.tonemap);
            self.gfx.update_material(material, desc);
        }
        let targets = &self.gfx.assets3d.render_targets;
        let live = |c: &super::TargetCamera| targets.iter().any(|t| t.id == c.target.texture.id);
        // An icon whose target was freed before its frame is dropped.
        let dropped: Vec<_> = self.gfx.icons.extract_if(.., |i| !live(&i.camera)).collect();
        let cameras = self
            .scene3d
            .passes
            .target_cameras
            .iter()
            .chain(capture_cameras.iter())
            .filter(|c| live(c));
        let count = cameras.clone().count();
        let views = &mut self.gfx.frame3d.views;
        views.resize_with(count + self.gfx.icons.len(), Default::default);
        let lighting = super::Lighting {
            shadows: false,
            gi: super::GlobalIllumination {
                enabled: false,
                ..self.scene3d.lighting.gi
            },
            ..self.scene3d.lighting
        };
        let fill = FillView {
            assets: &self.gfx.assets3d,
            skins,
            lighting: &lighting,
            lod: self.scene3d.lod,
            dt: self.time.frame_dt(),
            fallback,
        };
        let (cameras_views, icon_views) = views.split_at_mut(count);
        for (view, camera) in cameras_views.iter_mut().zip(cameras) {
            fill.view(view, camera, &mut self.world, false);
        }
        // Icons: each from its own world, with no skinned models.
        let no_skins = super::SkinFrame::default();
        let fill = FillView {
            skins: &no_skins,
            ..fill
        };
        for (view, icon) in icon_views.iter_mut().zip(&mut self.gfx.icons) {
            fill.view(view, &icon.camera, &mut icon.world, true);
            icon.sent = true;
        }
        for icon in dropped {
            self.gfx.free(icon.material);
        }
    }

    /// Advances visual particle pools after frame systems.
    pub(crate) fn update_particles(&mut self) {
        let dt = self.time.dt();
        let mut emitters = self
            .world
            .query::<(&mut super::ParticleEmitter3D, &super::Transform3D)>();
        let mut any = emitters.count() > 0;
        emitters.par_for_each(|(emitter, transform)| emitter.update(dt, transform));
        let mut trails = self.world.query::<(&mut super::Trail3D, &super::Transform3D)>();
        any |= trails.count() > 0;
        trails.for_each(|(trail, transform)| trail.update(dt, transform.position));
        any |= self.world.query::<&super::Line3D>().count() > 0;
        if any && !self.headless && self.gfx.renderer.is_some() {
            self.gfx.default_material(); // also initializes a particle-only 3D renderer
        }
    }

    /// Advances every animator not in manual mode by this frame's time. The
    /// game runs it after the frame systems, so draw systems (and the
    /// renderer) see this frame's pose.
    pub(crate) fn update_animators(&mut self) {
        let dt = self.time.dt();
        #[cfg(feature = "physics3d")]
        let mut finished: Vec<ponko_ecs::Entity> = Vec::new();
        #[cfg(feature = "physics3d")]
        {
            // Ragdolls pose their skeletons (after this frame's interpolation);
            // their animators wait, except while blending back.
            crate::physics3d_impl::pose_ragdolls(&mut self.world, &mut self.physics3d.ragdolls);
            self.world
                .query::<(
                    ponko_ecs::Entity,
                    &mut super::Animator,
                    &mut Skeleton,
                    &mut crate::d3::Ragdoll,
                )>()
                .for_each(|(e, animator, skeleton, ragdoll)| {
                    if ragdoll.is_blending() {
                        if !animator.manual {
                            animator.update(dt, skeleton);
                        }
                        if crate::physics3d_impl::blend_back(ragdoll, skeleton, dt) {
                            finished.push(e);
                        }
                    }
                });
            self.world
                .query::<(ponko_ecs::Entity, &mut Skeleton, &mut crate::d3::Ragdoll)>()
                .filter::<ponko_ecs::Without<super::Animator>>()
                .for_each(|(e, skeleton, ragdoll)| {
                    if ragdoll.is_blending() && crate::physics3d_impl::blend_back(ragdoll, skeleton, dt) {
                        finished.push(e);
                    }
                });
            for e in finished.drain(..) {
                self.world.remove::<crate::d3::Ragdoll>(e);
            }
        }
        #[allow(unused_mut)]
        let mut animated = self
            .world
            .query::<(&mut super::Animator, &mut Skeleton, Option<&mut super::Transform3D>)>();
        #[cfg(feature = "physics3d")]
        let mut animated = animated.filter::<ponko_ecs::Without<crate::d3::Ragdoll>>();
        animated.for_each(|(animator, skeleton, transform)| {
            if animator.manual {
                return;
            }
            animator.update(dt, skeleton);
            if let (true, Some(t)) = (animator.apply_root_motion, transform) {
                let motion = animator.root_motion();
                t.position += t.rotation * (motion.position * t.scale);
                t.rotation = (t.rotation * motion.rotation).normalize();
            }
        });
        self.gfx.write_animated_morphs(&mut self.world);
    }

    /// The screen effects, target effects and compute passes (of both
    /// stages) this frame runs (with their
    /// materials, `fallback` for none): freed ones, and passes into a freed
    /// target or buffer, are left out.
    pub(crate) fn collect_passes(&mut self, fallback: Option<Material>) {
        self.unlink_freed_gpu_buffers();
        self.gfx.frame3d.effects.clear();
        let effects_live = &self.gfx.assets3d.effects_live;
        self.gfx
            .frame3d
            .effects
            .extend(self.scene3d.passes.effects.iter().filter_map(|e| {
                // A freed effect is skipped.
                if !effects_live.contains(e.effect.id) {
                    return None;
                }
                let material = e.material.or(fallback)?;
                Some((e.effect.id, material.id))
            }));
        let targets = &self.gfx.assets3d.render_targets;
        self.gfx.frame3d.target_effects.clear();
        self.gfx
            .frame3d
            .target_effects
            .extend(self.scene3d.passes.target_effects.iter().filter_map(|e| {
                // A freed effect, or one into a freed target, is skipped.
                if !effects_live.contains(e.effect.id) {
                    return None;
                }
                let material = e.material.or(fallback)?;
                targets.iter().any(|t| t.id == e.target.texture.id).then_some((
                    e.effect.id,
                    e.target.texture.id,
                    material.id,
                ))
            }));
        let mut computes = std::mem::take(&mut self.gfx.frame3d.computes);
        self.list_computes(&mut computes, false, fallback);
        self.gfx.frame3d.computes = computes;
        let mut scene_computes = std::mem::take(&mut self.gfx.frame3d.scene_computes);
        self.list_computes(&mut scene_computes, true, fallback);
        self.gfx.frame3d.scene_computes = scene_computes;
    }

    /// This frame's entries of [`RenderPasses::compute`] (or, with `scene`,
    /// [`RenderPasses::scene_compute`]) into `list`: (compute, target texture
    /// or GPU buffer slot with the top bit set, material). Freed passes, and
    /// passes into a freed target or buffer, are left out.
    fn list_computes(&self, list: &mut Vec<(u32, u32, u32)>, scene: bool, fallback: Option<Material>) {
        list.clear();
        let passes = if scene {
            &self.scene3d.passes.scene_compute
        } else {
            &self.scene3d.passes.compute
        };
        let assets = &self.gfx.assets3d;
        list.extend(passes.iter().filter_map(|c| {
            // A freed pass is skipped.
            if !assets.computes_live.contains(c.compute.id) {
                return None;
            }
            let material = c.material.or(fallback)?;
            if let Some(b) = c.buffer {
                // A freed buffer's passes do nothing.
                assets
                    .gpu_buffer_slot(b.id)
                    .filter(|&slot| assets.gpu_buffers[slot] > 0)?;
                Some((c.compute.id, b.slot() | (1 << 31), material.id))
            } else {
                assets
                    .render_targets
                    .iter()
                    .any(|t| t.id == c.target.texture.id)
                    .then_some((c.compute.id, c.target.texture.id, material.id))
            }
        }));
    }

    pub(crate) fn prepare_3d(&mut self) {
        if self.gfx.renderer.is_none()
            || (self.gfx.assets3d.bounds.is_empty()
                && self.scene3d.passes.compute.is_empty()
                && self.scene3d.passes.scene_compute.is_empty()
                && self.gfx.frame3d.particles.runs.is_empty()
                && self.gfx.frame3d.particles.strips.runs.is_empty()
                && self.world.query::<&super::ParticleEmitter3D>().count() == 0
                && self.world.query::<&super::Trail3D>().count() == 0
                && self.world.query::<&super::Line3D>().count() == 0
                && self.gfx.frame3d.particles.texts.vertices.is_empty()
                && self.world.query::<&super::Text3D>().count() == 0
                && self.gfx.frame3d.debug_lines.is_empty())
        {
            return; // no drawable 3D scene
        }
        let time = (self.time.elapsed(), self.time.dt());
        self.gfx.assets3d.decals_supported = self.gfx.supports_decals();
        self.gfx.assets3d.gi_supported = self.gfx.renderer.as_ref().is_some_and(|r| r.supports_gi());
        if self.gfx.assets3d.baked_upload.is_some() {
            self.gfx.upload_baked_lighting();
        }
        let fallback = if self.scene3d.passes.effects.iter().any(|e| e.material.is_none())
            || self.scene3d.passes.target_effects.iter().any(|e| e.material.is_none())
            || self.scene3d.passes.compute.iter().any(|c| c.material.is_none())
            || self.scene3d.passes.scene_compute.iter().any(|c| c.material.is_none())
            || (self.scene3d.passes.target_cameras.iter())
                .flat_map(|c| &c.effects)
                .any(|e| e.material.is_none())
        {
            Some(self.gfx.default_material())
        } else {
            None
        };
        self.collect_passes(fallback);
        self.gfx.validate_instance_batches();
        self.gfx.validate_crowds();
        let start = web_time::Instant::now();
        let motion = self.scene3d.post.anti_aliasing == super::AntiAliasing::Taa
            || (self.scene3d.post.ao_temporal && self.scene3d.post.ambient_occlusion.enabled);
        // Held items in front of the camera as it is now, after every system.
        let view_model = self.scene3d.view_model;
        super::view_model::place_held_items(&mut self.world, &self.scene3d.camera, view_model.as_ref());
        let mut skins = std::mem::take(&mut self.gfx.frame3d.skins);
        // The last frame's poses are kept only while something reads motion.
        skins.motion = motion;
        skins.collect(&mut self.world, &self.gfx.assets3d);
        self.collect_views(&skins, fallback);
        let Some(renderer) = self.gfx.renderer.as_mut() else {
            return;
        };
        self.gfx.frame3d.size_limit = renderer.max_texture_size();
        self.gfx.frame3d.lod = self.scene3d.lod;
        self.gfx.frame3d.lod_dt = Some(self.time.frame_dt());
        self.gfx.frame3d.motion.enabled = motion;
        if !self.scene3d.camera.occlusion_culling {
            self.gfx.frame3d.occlusion = None;
        } else {
            renderer.replace_occlusion(&mut self.gfx.frame3d.occlusion);
        }
        // Several cameras: the sun's cascades cover every view.
        let views_on = !self.scene3d.views.is_empty() || !self.scene3d.camera.rect.is_full();
        self.gfx.frame3d.shadow_views.clear();
        if self.scene3d.lighting.shadows {
            let cameras = self.scene3d.views.iter().map(|v| v.camera);
            self.gfx.frame3d.shadow_views.extend(cameras);
        }
        self.gfx.frame3d.collect(
            &mut self.world,
            &self.gfx.assets3d,
            &skins,
            &super::view_model::without_view_model(&self.scene3d.camera, view_model.as_ref()),
            &self.scene3d.lighting,
        );
        super::view_model::collect_view_model(
            &mut self.world,
            &self.gfx.assets3d,
            &mut self.gfx.frame3d,
            &self.scene3d,
            &skins,
        );
        if views_on {
            collect_window_views(
                &mut self.world,
                &self.gfx.assets3d,
                &mut self.gfx.frame3d,
                &self.scene3d,
                &skins,
            );
        } else {
            self.gfx.frame3d.window_views.clear();
        }
        // Hidden from the window's camera, the lines are still drawn by
        // target cameras that show them.
        self.gfx.frame3d.debug_lines.hidden = !self.scene3d.camera.draws_debug(false);
        self.gfx.frame3d.skins = skins;
        // Every camera's 3D text, facing it, now that the views are known.
        self.collect_text3d();
        let Some(renderer) = self.gfx.renderer.as_mut() else {
            return;
        };
        // The atmosphere, and the sky kit (which reddens and sets the sun as it
        // does), light the scene through the air.
        let sky = &self.scene3d.sky;
        let light = if (sky.atmosphere || sky.kit.is_some()) && sky.shader.is_none() {
            match self.gfx.frame3d.sky_light {
                Some((lighting, sky, light)) if lighting == self.scene3d.lighting && sky == self.scene3d.sky => light,
                _ => {
                    let light = super::SkyLight::new(&self.scene3d.lighting, &self.scene3d.sky);
                    self.gfx.frame3d.sky_light = Some((self.scene3d.lighting, self.scene3d.sky, light));
                    light
                }
            }
        } else {
            super::SkyLight::authored(&self.scene3d.lighting, &self.scene3d.sky)
        };
        let collect_ms = start.elapsed().as_secs_f32() * 1000.0;
        let scene = super::Scene3D {
            frame: &self.gfx.frame3d,
            lighting: &self.scene3d.lighting,
            sky: &self.scene3d.sky,
            light,
            camera: self.scene3d.camera.position,
            post: &self.scene3d.post,
            time,
            occlusion: self.scene3d.camera.occlusion_culling,
        };
        if views_on {
            let clear = self.gfx.clear_color;
            let views = self
                .scene3d
                .views
                .iter()
                .zip(&self.gfx.frame3d.window_views)
                .map(|(view, frame)| {
                    let scene = super::Scene3D {
                        frame,
                        camera: view.camera.position,
                        post: view.post.as_ref().unwrap_or(&self.scene3d.post),
                        occlusion: false,
                        ..scene
                    };
                    (scene, view.camera.rect, view.clear.unwrap_or(clear))
                });
            renderer.prepare_3d_views((&scene, self.scene3d.camera.rect, clear), views, collect_ms);
            for frame in &mut self.gfx.frame3d.window_views {
                frame.motion.reset = false;
            }
        } else {
            renderer.drop_3d_views();
            renderer.prepare_3d(&scene, collect_ms);
        }
        self.gfx.frame3d.motion.reset = false;
    }
}

/// Collects what each window view (`SceneSettings::views`) draws, each
/// into a frame of its own: its camera's models (on its layers), lights
/// and particles. The shadow maps, poses, probes and screen effects are
/// the main frame's.
pub(crate) fn collect_window_views(
    world: &mut ponko_ecs::World,
    assets: &super::Assets3D,
    main: &mut super::Frame3D,
    settings: &super::SceneSettings,
    skins: &super::SkinFrame,
) {
    let mut frames = std::mem::take(&mut main.window_views);
    frames.resize_with(settings.views.len(), Default::default);
    let lighting = super::Lighting {
        shadows: false,
        gi: super::GlobalIllumination {
            enabled: false,
            ..settings.lighting.gi
        },
        ..settings.lighting
    };
    for (frame, view) in frames.iter_mut().zip(&settings.views) {
        let post = view.post.as_ref().unwrap_or(&settings.post);
        frame.size_limit = main.size_limit;
        frame.lod = settings.lod;
        frame.lod_dt = main.lod_dt;
        frame.motion.enabled =
            post.anti_aliasing == super::AntiAliasing::Taa || (post.ao_temporal && post.ambient_occlusion.enabled);
        frame.occlusion = None;
        // The main frame's local light shadows (planned for every camera).
        frame.lights.share_shadows(&main.lights);
        let camera = super::view_model::without_view_model(&view.camera, settings.view_model.as_ref());
        frame.collect(world, assets, skins, &camera, &lighting);
        frame.lights.share_with_view(&main.lights);
        frame.light_views = main.light_views;
        // The main frame's shadow maps, which cover this view too.
        frame.cascades = main.cascades;
        frame.sun_view_proj = main.sun_view_proj;
        frame.cascade_texel = main.cascade_texel;
        frame.shadow_size = main.shadow_size;
        frame.probes = main.probes;
        frame.effects.clone_from(&main.effects);
        let lines = &mut frame.debug_lines;
        lines.clear();
        if view.camera.draws_debug(false) {
            lines.extend_lines(&main.debug_lines);
        }
    }
    main.window_views = frames;
}

/// What every target camera's view is collected with (`Ctx::collect_views`).
#[derive(Clone, Copy)]
struct FillView<'a> {
    assets: &'a super::Assets3D,
    skins: &'a super::SkinFrame,
    lighting: &'a super::Lighting,
    lod: super::LodSettings,
    dt: f32,
    fallback: Option<Material>,
}

impl FillView<'_> {
    /// Collects `camera`'s view of `world` into `view`.
    fn view(
        &self,
        view: &mut super::collect::ViewFrame,
        camera: &super::TargetCamera,
        world: &mut World,
        isolated: bool,
    ) {
        let size = (camera.target.width(), camera.target.height());
        let mut eye = camera.camera;
        eye.set_viewport(glam::Vec2::new(size.0 as f32, size.1 as f32));
        view.frame.lod = self.lod;
        view.frame.lod_dt = Some(self.dt);
        view.frame.cull_box = camera.cull_box;
        view.frame.collect(world, self.assets, self.skins, &eye, self.lighting);
        view.target = camera.target.texture.id;
        view.size = size;
        view.camera = eye.position;
        view.particles = eye.particles;
        view.debug = eye.draws_debug(true);
        view.eye = Some(eye);
        view.cull_box = camera.cull_box;
        view.clear = camera.clear;
        view.isolated = isolated;
        let effects_live = &self.assets.effects_live;
        view.effects.clear();
        view.effects.extend(camera.effects.iter().filter_map(|e| {
            // A freed effect is skipped, as in the main view's list.
            if !effects_live.contains(e.effect.id) {
                return None;
            }
            let material = e.material.or(self.fallback)?;
            Some((e.effect.id, material.id))
        }));
        view.view_proj = match camera.clip_plane {
            Some(plane) => super::target::oblique(eye.view_proj(), eye.view(), plane),
            None => eye.view_proj(),
        };
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    fn cached_clips() -> (Ctx, String) {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        // A real existing path, with its contents bypassed by the asset cache.
        let path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        let file = assets::Origin::File(assets::resolve(&path).unwrap());
        let skeleton = Skeleton::new(vec![]);
        let clips = vec![
            AnimationClip::new("idle", &skeleton, vec![]),
            AnimationClip::new("walk", &skeleton, vec![]),
        ];
        ctx.gfx.assets3d.loaded.insert(file.clone(), Mesh { id: 0 });
        ctx.gfx.assets3d.loaded_skeletons.insert(file.clone(), skeleton);
        ctx.gfx.assets3d.loaded_clips.insert(file, clips);
        (ctx, path)
    }

    #[test]
    fn cached_clips_load_by_name_and_missing_names_list_choices() {
        let (mut ctx, path) = cached_clips();
        let clips = ctx.gfx.load_clips(&path);
        assert_eq!(clips.len(), 2);
        assert_eq!(ctx.gfx.load_clip(&path, "walk").name(), "walk");
        assert!(std::sync::Arc::ptr_eq(
            clips[0].source_joints(),
            ctx.gfx.load_clip(&path, "idle").source_joints()
        ));
        let error = ctx.gfx.try_load_clip(&path, "run").unwrap_err();
        assert_eq!(error.kind(), crate::error::ErrorKind::NotFound);
        let message = error.message();
        assert!(message.contains("idle") && message.contains("walk") && message.contains("run"));
    }

    #[test]
    fn clips_cached_from_a_source_are_found_by_the_source_and_not_the_folder() {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.assets
            .add_source(crate::assets::MemorySource::new("pack").with("hero.glb", b"unread".to_vec()));
        let origin = assets::Origin::Source {
            source: "pack".into(),
            name: "hero.glb".into(),
        };
        let skeleton = Skeleton::new(vec![]);
        let clips = vec![AnimationClip::new("idle", &skeleton, vec![])];
        ctx.gfx.assets3d.loaded.insert(origin.clone(), Mesh { id: 0 });
        ctx.gfx.assets3d.loaded_skeletons.insert(origin.clone(), skeleton);
        ctx.gfx.assets3d.loaded_clips.insert(origin, clips);
        // The source's bytes are never read: the cache is asked first.
        assert_eq!(ctx.gfx.load_clip("hero.glb", "idle").name(), "idle");
        assert_eq!(ctx.gfx.load_clips("hero.glb").len(), 1);
        let error = ctx.gfx.try_load_clip("hero.glb", "run").unwrap_err();
        assert!(
            error.message().contains("idle") && error.message().contains("run"),
            "{error}"
        );
    }

    #[test]
    #[should_panic(expected = "available animations: [\"idle\", \"walk\"]")]
    fn missing_named_clip_panics_with_available_names() {
        let (mut ctx, path) = cached_clips();
        ctx.gfx.load_clip(&path, "run");
    }
}
