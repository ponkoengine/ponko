//! 3D: meshes with materials, drawn from entities with a [`d3::Transform`](crate::d3::Transform) and
//! a [`Model`], through a perspective [`d3::Camera`](crate::d3::Camera), lit by a sun and ambient
//! light ([`Lighting`]). Behind the `3d` feature.
//!
//! The 3D pass is drawn first, with a depth buffer; 2D sprites, text and UI
//! go on top of it. Games without 3D models never run it, and builds without
//! the feature don't have it at all.
//!
//! Coordinates are right-handed with Y up (as in glTF): a camera with no
//! rotation looks down -Z.

#![deny(missing_docs)]

/// `with_*` builder methods for `#[non_exhaustive]` descriptions, one per
/// field: `with_field(value)` returns the description with that field set.
/// An `Option<T>` field's method takes `impl Into<Option<T>>`, so both
/// `with_texture(image)` and `with_texture(None)` work.
macro_rules! with_fields {
    () => {};
    ($(#[$doc:meta])* $field:ident: Option<$ty:ty> => $name:ident; $($rest:tt)*) => {
        $(#[$doc])*
        #[must_use]
        pub fn $name(mut self, $field: impl Into<Option<$ty>>) -> Self {
            self.$field = $field.into();
            self
        }
        with_fields! { $($rest)* }
    };
    ($(#[$doc:meta])* $field:ident: $ty:ty => $name:ident; $($rest:tt)*) => {
        $(#[$doc])*
        #[must_use]
        pub fn $name(mut self, $field: $ty) -> Self {
            self.$field = $field;
            self
        }
        with_fields! { $($rest)* }
    };
}

mod animator;
pub(crate) mod api;
pub(crate) mod bake;
#[cfg(all(feature = "window", not(target_arch = "wasm32")))]
pub use bake::bake_lighting_gpu;
pub use bake::{
    BakeModel, BakeStats, BakeTexture, BakedLighting, GltfBake, LightBakeScene, LightBakeSettings, LightmapRect,
    Lightmapped, MAX_LIGHTMAP_PAGES, Unbaked, WorldBake, bake_lighting,
};
mod clip;
mod collect;
mod compute;
mod effect;
#[cfg(any(test, feature = "bench-internals"))]
mod extraction_bench;
#[cfg(any(test, feature = "bench-internals"))]
pub use extraction_bench::{ExtractionBench, ExtractionSample};
pub(crate) mod gi;
pub(crate) mod gltf;
mod gltf_ext;
mod gltf_props;
mod gltf_scene;
mod ik;
pub(crate) mod instances;
mod retarget;
pub use instances::{BatchCulling, InstanceBatch, InstanceBatchOptions, InstanceLevel};
pub(crate) mod crowd;
pub use crowd::{
    Crowd, CrowdAnimation, CrowdClip, CrowdInstance, CrowdLevel, CrowdOptions, MAX_CROWD_INSTANCES, MAX_CROWD_LEVELS,
};
pub(crate) mod instance_cells;
pub use instance_cells::{InstanceCell, InstanceCellData};
mod lod;
pub(crate) mod occlusion;
pub use gi::GlobalIllumination;
#[cfg(feature = "window")]
pub(crate) use gi::MAX_GI_PROBES_PER_FRAME;
pub use ik::{look_at_joint, two_bone_ik};
pub use lod::{LodGroup, LodHysteresis, LodLevel, LodPick, LodRenderer, LodSettings, LodShadow};
pub use retarget::{HumanBone, HumanoidMap, RetargetError, Retargeter};
pub(crate) mod lights;
#[cfg(feature = "editor")]
mod material_edit;
pub(crate) mod material_extras;
mod material_variant;
mod stencil;
pub use stencil::{Stencil, StencilCompare, StencilFace, StencilOp};
mod mesh;
pub use lights::{LIGHT_SHADOW_ATLAS_SIZE, LIGHT_SHADOW_SIZE, MAX_LIGHT_SHADOW_VIEWS, PointLight3D, SpotLight3D};
pub(crate) use lights::{MAX_LIGHT_SHADOW_RESOLUTION, MIN_LIGHT_SHADOW_RESOLUTION};
#[cfg(feature = "window")]
pub(crate) use material_variant::{MaterialInstancing, MaterialSides, MaterialVariantKey};
pub(crate) mod shadow_atlas;
pub(crate) use shadow_atlas::{MAX_LIGHT_SHADOW_ATLAS, MIN_LIGHT_SHADOW_ATLAS};
pub(crate) mod depth_sort;
pub(crate) mod particles;
pub use particles::{ParticleBlend3D, ParticleEmitter3D, ParticleSettings3D, ParticleShape3D};
mod decal;
mod post;
pub(crate) mod projected;
pub use projected::{Decal3D, ReceivesDecals};
pub(crate) mod probe;
pub use probe::{ProbeRefresh, ReflectionProbe3D, ReflectionRefresh3D};
pub(crate) mod autotune;
#[cfg(test)]
mod extraction_test;
#[cfg(test)]
mod gltf_scene_test;
mod quality;
mod resident;
mod retained;
mod sliced_mesh;
mod streamed;
pub(crate) use streamed::LoadedFile;
pub use streamed::StreamedLod;
mod skin;
pub(crate) mod strips;
pub use strips::{Line3D, Trail3D};
pub(crate) mod text3d;
pub use text3d::{Text3D, TextAlign3D};
mod far_terrain;
mod shader_status;
pub(crate) use crate::render::{ShaderDiagnostic, ShaderKind};
pub(crate) use shader_status::{COMPUTE_STAND_IN, EFFECT_STAND_IN};
mod settings;
mod sky;
mod sky_kit;
mod sky_shader;
mod target;
mod terrain;
mod terrain_file;
pub use far_terrain::FarTerrain;
mod texture_array;
pub(crate) mod view_model;
pub use view_model::{HeldItem, VIEW_MODEL_LAYER, ViewModel};

pub use animator::{
    AnimationGraph, AnimationLayer, AnimationState, Animator, JointMask, LayerBlend, LayerId, Motion, MotionChild,
    RootMotion,
};
pub(crate) use api::GpuMaterialDesc;
#[cfg(feature = "window")]
pub(crate) use api::{MaterialTextures, MaterialUniform};
pub use clip::{AnimationClip, AnimationTrack, Interpolation, JointAnimation, MorphAnimation};
pub(crate) mod motion;
pub use autotune::{QualityAutotune, QualityAutotuneReason, QualityAutotuneStatus, QualityChange};
#[cfg(feature = "window")]
pub(crate) use collect::{
    FADED, INSTANCE_BRIDGE, Instance3D, MAX_CASCADES, MAX_SHADOW_VIEWS, MaterialOverride, shadow_views,
};
pub(crate) use collect::{Frame3D, MAX_SHADOW_RESOLUTION, MIN_SHADOW_RESOLUTION, SHADOW_MAP_SIZE};
pub use compute::{Compute, ComputePass, GpuBuffer};
pub use effect::{Effect, ScreenEffect, TargetEffect};
pub use gltf_ext::{ModelCamera, ModelLight, ModelLightKind, ModelProjection};
pub use gltf_props::{NodeBody, NodeCollider, NodeProperties, gltf_properties_schema};
#[cfg(all(test, feature = "editor"))]
pub(crate) use gltf_scene::cube_gltf;
#[cfg(feature = "editor")]
pub(crate) use gltf_scene::{GltfEntities, GltfOverride, GltfOverrideKind, WaitingLink};
pub use gltf_scene::{GltfNode, GltfOptions, GltfRoot, GltfScene, GltfSceneNode, SpawnPoint};
pub use mesh::{MeshData, MorphFrame, MorphTarget};
pub use post::Upscaler;
pub use post::{AmbientOcclusion, AntiAliasing, DepthOfField, Post, PostVolume, SplitToning, Tonemap};
pub(crate) use post::{MAX_RENDER_SCALE, MIN_RENDER_SCALE};
#[cfg(feature = "window")]
pub(crate) use post::{PostUniform, scaled_size};
pub use quality::GraphicsQuality;
pub use settings::{RenderPasses, SceneSettings};
#[cfg(feature = "window")]
pub(crate) use skin::blend as blend_skin;
pub use skin::{AttachToJoint, Joint, MorphWeights, Skeleton, SkinOf};
pub(crate) use skin::{MeshMorph, MeshSkin, SkinFrame};
#[cfg(all(test, feature = "window"))]
pub(crate) use skin::{SkinDraw, normalized as normalized_weights};
pub(crate) use sky::SkyLight;
pub use sky::{Sky, SkyShader};
#[cfg(feature = "window")]
pub(crate) use sky_kit::CLOUD_SHADOW_TEXELS;
pub use sky_kit::{CloudQuality, Clouds, DayNight, Moon, SkyKit};
pub use target::{RenderTarget, TargetCamera};
pub use terrain::{Heightfield, TERRAIN_LODS, TerrainMaterialDesc, TerrainTile, TerrainTileData};
pub use terrain_file::TerrainFile;
#[cfg_attr(not(feature = "editor"), allow(unused_imports))]
pub(crate) use terrain_file::{
    Built as TerrainBuilt, LayerJson as TerrainLayerJson, TERRAIN_FORMAT, TerrainData, TerrainJson,
    build as build_terrain, encode_grey16, encode_rgba, load_layers as load_terrain_layers,
    note_tile as note_terrain_tile,
};
pub(crate) use terrain_file::{
    follow_files as follow_terrain_files, give_tiles as give_terrain_tiles, prepare_scene as prepare_scene_terrain,
    restore_tiles as restore_terrain_tiles,
};
pub use texture_array::{TextureArray, TextureKind};

use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

use crate::{color::Color, texture::Texture};
use ponko_ecs::Component;

/// Position, rotation and scale of a 3D entity.
///
/// ```
/// # use ponko::prelude::*;
/// let crate_box = ponko::d3::Transform::at(vec3(0.0, 0.5, -3.0)).rotated(Quat::from_rotation_y(0.3));
/// # let _ = crate_box;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Component)]
#[serde(default)]
#[repr(C, align(16))] // shared with C/C++ (`PonkoTransform3D`): 48 bytes everywhere
pub struct Transform3D {
    /// Which way it faces.
    pub rotation: Quat,
    /// Where it is, in world units.
    pub position: Vec3,
    /// Size along its own axes. Default `(1, 1, 1)`.
    pub scale: Vec3,
}

impl Default for Transform3D {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform3D {
    /// At `position`, not rotated, scale 1.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let t = ponko::d3::Transform::at(vec3(1.0, 2.0, 3.0));
    /// assert_eq!(t.scale, Vec3::ONE);
    /// ```
    #[must_use]
    pub fn at(position: Vec3) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }

    /// The same, rotated by `rotation`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let t = ponko::d3::Transform::default().rotated(Quat::from_rotation_y(1.0));
    /// # let _ = t;
    /// ```
    #[must_use]
    pub fn rotated(self, rotation: Quat) -> Self {
        Self { rotation, ..self }
    }

    /// The same, scaled by `scale` on every axis.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// assert_eq!(ponko::d3::Transform::default().scaled(2.0).scale, Vec3::splat(2.0));
    /// ```
    #[must_use]
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            scale: Vec3::splat(scale),
            ..self
        }
    }

    /// The same, turned so its -Z axis points at `target` (with +Y as up).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let t = ponko::d3::Transform::at(vec3(0.0, 0.0, 5.0)).looking_at(Vec3::ZERO);
    /// assert!((t.forward() - vec3(0.0, 0.0, -1.0)).length() < 1e-5);
    /// ```
    #[must_use]
    pub fn looking_at(self, target: Vec3) -> Self {
        Self {
            rotation: look_rotation(target - self.position),
            ..self
        }
    }

    /// The direction it faces: its -Z axis.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// assert_eq!(ponko::d3::Transform::default().forward(), vec3(0.0, 0.0, -1.0));
    /// ```
    #[must_use]
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// The matrix from its local space to the world.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let m = ponko::d3::Transform::at(vec3(1.0, 0.0, 0.0)).matrix();
    /// assert_eq!(m.transform_point3(Vec3::ZERO), vec3(1.0, 0.0, 0.0));
    /// ```
    #[must_use]
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}

/// The rotation that turns -Z towards `direction`, keeping +Y up (or +Z up
/// when looking straight up or down).
fn look_rotation(direction: Vec3) -> Quat {
    let forward = direction.try_normalize().unwrap_or(Vec3::NEG_Z);
    let up = if forward.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
    let right = forward.cross(up).normalize();
    let up = right.cross(forward);
    Quat::from_mat3(&Mat3::from_cols(right, up, -forward))
}

/// A mesh on the GPU: triangles, split into parts that each have a
/// [`Material`]. Made with [`Graphics::create_mesh`](crate::gfx::Graphics::create_mesh) or
/// loaded with [`Graphics::load_mesh`](crate::gfx::Graphics::load_mesh); a cheap handle.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let tree: ponko::render::Mesh = ctx.gfx.load_mesh("tree.glb");
/// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(tree)));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)] // shared with C/C++ (`PonkoMesh`)
pub struct Mesh {
    pub(crate) id: u32,
}

impl Mesh {
    /// No mesh: a [`Model`] showing it draws nothing and casts no shadow, at
    /// almost no cost. For models that are sometimes hidden, such as
    /// plants beyond their draw distance. No mesh is ever made with its
    /// id. The same value as [`Handle::NONE`](crate::handle::Handle::NONE).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let material = ctx.gfx.default_material();
    ///     let bush = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(1.0), material);
    ///     ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(ponko::render::Mesh::NONE)));
    ///     assert!(!ctx.gfx.is_live(ponko::render::Mesh::NONE));
    ///     assert_ne!(bush, ponko::render::Mesh::NONE);
    /// });
    /// ```
    pub const NONE: Mesh = Mesh { id: u32::MAX };

    /// Its slot in the engine's mesh tables and the renderer: the id
    /// without its generation (see [`SlotIds`](crate::handle::SlotIds)).
    #[inline]
    pub(crate) fn slot(self) -> u32 {
        self.id & SLOT_MASK
    }
}

/// The slot bits of a [`Mesh`] or [`GpuBuffer`] id (see
/// [`SlotIds`](crate::handle::SlotIds)).
pub(crate) const SLOT_MASK: u32 = crate::handle::INDEX_MASK;

/// What one mesh cost to draw in the last frame, from
/// [`Graphics::mesh_costs`](crate::gfx::Graphics::mesh_costs).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshCost {
    /// The mesh.
    pub mesh: Mesh,
    /// The file it was loaded from, if it came from one (glTF meshes are
    /// `file.glb#index`-style paths as they were loaded).
    pub name: Option<String>,
    /// Models the camera drew with it.
    pub models: usize,
    /// Triangles the camera's passes drew with it.
    pub triangles: usize,
    /// Models drawn with it into the shadow maps, all cascades together.
    pub shadow_models: usize,
    /// Triangles drawn with it into the shadow maps, all cascades together.
    pub shadow_triangles: usize,
}

/// How a surface looks: its color (and texture), how rough and how metallic
/// it is, and light it gives off. Made with
/// [`Graphics::create_material`](crate::gfx::Graphics::create_material); a cheap handle.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let gold = ctx.gfx.create_material(ponko::render::MaterialDesc::color(Color::hex(0xFFC83D)).with_metallic(1.0).with_roughness(0.3));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)] // shared with C/C++ (`PonkoMaterial`)
pub struct Material {
    pub(crate) id: u32,
}

/// A material shader written in WGSL: made with
/// [`Graphics::load_shader`](crate::gfx::Graphics::load_shader) or
/// [`Graphics::create_shader`](crate::gfx::Graphics::create_shader); a cheap handle. See
/// the 3D chapter of the guide for what a shader can do ("Custom shaders"
/// and "The shader contract").
///
/// # The contract, in short
///
/// - The game's WGSL may define `fn vertex(v: Vertex) -> Vertex` (world
///   space) and `fn surface(s: Surface) -> Surface`, with exactly these
///   signatures; a missing one passes its input through. It declares no
///   bindings of its own: groups 0 (`frame`), 1 (`material` and its
///   textures), 2 (the scene behind, for transparent materials) and 3
///   (`items`, a mesh's GPU buffer) are the engine's.
/// - The same code runs in every pass: `vertex` in the depth prepass, the
///   opaque or transparent pass, the shadow maps and the motion vectors;
///   `surface` in the lit passes, and in depth and shadow passes only for
///   [`Blend::Cutout`] materials. `drawing_shadows()` tells shadow maps apart.
/// - Each shader is built as six pipeline sets: one- or double-sided
///   ([`MaterialDesc::double_sided`]) times plain, crossfading (levels of
///   detail) or shared-instance (GPU buffer) draws. There are no defines;
///   skinned meshes use the same pipelines.
/// - Culling uses the mesh's bounds before `vertex` moves anything: grow
///   them with [`Graphics::set_mesh_gpu_bounds`](crate::gfx::Graphics::set_mesh_gpu_bounds)
///   for shaders that move vertices far.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let water = ctx.gfx.load_shader("shaders/water.wgsl");
/// let lake = ctx.gfx.create_material(ponko::render::MaterialDesc::default().with_shader(water));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)] // shared with C/C++ (`PonkoShader`)
pub struct Shader {
    pub(crate) id: u32,
}

/// How a material covers what's behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
#[non_exhaustive]
pub enum Blend {
    /// Solid, whatever its alpha. The fastest: hidden pixels and shadows
    /// skip the material's shader.
    #[default]
    Opaque = 0,
    /// See-through by its alpha (glass, water). Drawn after everything
    /// solid, casts no shadow. Transparent models of one mesh are drawn far
    /// to near (by their origins), so they blend in the right order;
    /// models of different meshes are not sorted against each other, so
    /// one may show over another that is in front of it.
    Transparent = 1,
    /// Solid, with pixels whose alpha is below 0.5 cut out (leaves,
    /// fences, grass cards). Their shadows have the same holes.
    Cutout = 2,
}

/// What a [`Material`] is made from.
///
/// ```
/// # use ponko::prelude::*;
/// let grass = ponko::render::MaterialDesc::color(Color::hex(0x4C8A3A)).with_roughness(0.9);
/// assert_eq!(grass.metallic, 0.0);
/// ```
///
/// New fields may be added in any release: build one with
/// [`MaterialDesc::color`] or [`Default`] and the `with_*` methods, or set
/// the public fields of a `mut` value.
///
/// # Defaults and cost
///
/// [`Default`] is an opaque, single-sided, white material with a roughness of
/// 0.6, no metal, no light of its own and no textures, shader or stencil.
/// The description holds handles, so textures are shared, never copied. What
/// costs is what you turn on: [`Blend::Transparent`] is drawn after all solid
/// geometry and casts no shadow, [`Blend::Cutout`] runs the material's shader
/// for hidden pixels and shadows too (opaque skips it), `double_sided`
/// rasterizes both faces, and each texture map is one more read per pixel.
/// A custom `shader` runs your code for every pixel the material covers.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MaterialDesc {
    /// Base color, multiplied with the texture. Default white.
    pub color: Color,
    /// An image on the surface (by the mesh's texture coordinates).
    pub texture: Option<Texture>,
    /// 0 is mirror-smooth, 1 is fully rough (matte). Default 0.6.
    pub roughness: f32,
    /// 0 for most things, 1 for bare metal. Default 0.
    pub metallic: f32,
    /// Light it gives off itself (not lighting anything else). Default black.
    pub emissive: Color,
    /// A normal map: surface detail (bumps, grooves) as tangent-space
    /// normals, OpenGL style (green up), as glTF has them.
    pub normal_map: Option<Texture>,
    /// How strong the normal map is. Default 1.
    pub normal_strength: f32,
    /// An occlusion-roughness-metallic map, as glTF has it: red is ambient
    /// occlusion, green multiplies `roughness`, blue multiplies `metallic`.
    pub orm_map: Option<Texture>,
    /// Multiplies `emissive` (which must not be black for it to show).
    pub emissive_map: Option<Texture>,
    /// How it covers what's behind it. Default [`Blend::Opaque`].
    pub blend: Blend,
    /// Draws the back of each triangle too (leaves, cloth, planes seen from
    /// below). Default false.
    pub double_sided: bool,
    /// A custom shader; `None` is the built-in one.
    pub shader: Option<Shader>,
    /// Values for a custom shader: `material.custom[0]` to `[3]` there.
    pub custom: [Vec4; 4],
    /// Textures for a custom shader: `custom_texture0` and
    /// `custom_texture1` there.
    pub custom_textures: [Option<Texture>; 2],
    /// Texture arrays for a custom shader: `custom_array0` to
    /// `custom_array2` there (`texture_2d_array<f32>`), each read as its
    /// [`TextureKind`] says.
    pub custom_arrays: [Option<TextureArray>; 3],
    /// Stencil test and writes; `None` (the default) is none. See
    /// [`Stencil`] for when they happen and the drawing order they keep.
    pub stencil: Option<Stencil>,
}

impl Default for MaterialDesc {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            texture: None,
            roughness: 0.6,
            metallic: 0.0,
            emissive: Color::rgb(0.0, 0.0, 0.0),
            normal_map: None,
            normal_strength: 1.0,
            orm_map: None,
            emissive_map: None,
            blend: Blend::Opaque,
            double_sided: false,
            shader: None,
            custom: [Vec4::ZERO; 4],
            custom_textures: [None; 2],
            custom_arrays: [None; 3],
            stencil: None,
        }
    }
}

impl MaterialDesc {
    /// A plain material of one color.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let red = ponko::render::MaterialDesc::color(Color::rgb(0.8, 0.1, 0.1));
    /// # let _ = red;
    /// ```
    #[must_use]
    pub fn color(color: Color) -> Self {
        Self {
            color,
            ..Self::default()
        }
    }

    with_fields! {
        /// Sets [`color`](Self::color), the base color.
        color: Color => with_color;
        /// Sets [`texture`](Self::texture), the surface image.
        texture: Option<Texture> => with_texture;
        /// Sets [`roughness`](Self::roughness): 0 mirror-smooth, 1 matte.
        roughness: f32 => with_roughness;
        /// Sets [`metallic`](Self::metallic): 0 for most things, 1 for bare metal.
        metallic: f32 => with_metallic;
        /// Sets [`emissive`](Self::emissive), the light it gives off.
        emissive: Color => with_emissive;
        /// Sets [`normal_map`](Self::normal_map).
        normal_map: Option<Texture> => with_normal_map;
        /// Sets [`normal_strength`](Self::normal_strength).
        normal_strength: f32 => with_normal_strength;
        /// Sets [`orm_map`](Self::orm_map), the occlusion-roughness-metallic map.
        orm_map: Option<Texture> => with_orm_map;
        /// Sets [`emissive_map`](Self::emissive_map).
        emissive_map: Option<Texture> => with_emissive_map;
        /// Sets [`blend`](Self::blend), how it covers what's behind it.
        blend: Blend => with_blend;
        /// Sets [`double_sided`](Self::double_sided).
        double_sided: bool => with_double_sided;
        /// Sets [`shader`](Self::shader), a custom shader (`None` is the built-in one).
        shader: Option<Shader> => with_shader;
        /// Sets [`custom`](Self::custom), the values for a custom shader.
        custom: [Vec4; 4] => with_custom;
        /// Sets [`custom_textures`](Self::custom_textures), the textures for a custom shader.
        custom_textures: [Option<Texture>; 2] => with_custom_textures;
        /// Sets [`custom_arrays`](Self::custom_arrays), the texture arrays for a custom shader.
        custom_arrays: [Option<TextureArray>; 3] => with_custom_arrays;
        /// Sets [`stencil`](Self::stencil), the stencil test and writes.
        stencil: Option<Stencil> => with_stencil;
    }
}

/// Draws a [`Mesh`] where the entity's [`d3::Transform`](crate::d3::Transform) is.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let white = ctx.gfx.default_material();
/// let cube = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(1.0), white);
/// for i in 0..10 {
///     let tint = Color::rgb(i as f32 / 10.0, 0.5, 1.0);
///     ctx.world.spawn((ponko::d3::Transform::at(vec3(i as f32 * 2.0, 0.0, 0.0)), ponko::render::Model::new(cube).with_color(tint)));
/// }
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[repr(C)] // shared with C/C++ (`PonkoModel`)
pub struct Model {
    /// What to draw.
    pub mesh: Mesh,
    /// Multiplied with the materials' colors, so one mesh can come in many
    /// colors without more materials. Default white (no change).
    pub color: Color,
}

/// Draws a different mesh into the shadow maps than the entity's [`Model`]
/// shows: a cheap stand-in for a detailed model's shadow (a simplified copy,
/// a billboard), or `None` for no shadow from this entity. Without it, the
/// model's own mesh casts the shadow (unless
/// [`Graphics::set_mesh_shadows`](crate::gfx::Graphics::set_mesh_shadows) turned that
/// mesh's shadows off).
///
/// Far from the camera, a shadow is a few texels of the shadow map, so a
/// mesh with a tenth of the triangles casts the same shadow for a tenth of
/// the cost. Swap it by distance, as you would the model's own detail.
///
/// An entity with a `ShadowMesh` and no [`Model`] casts only the shadow: a
/// stand-in for the shadow of several models around it that cast none. On
/// a [`TerrainTile`], `ShadowMesh(None)` turns the ground's shadow off.
///
/// In scene files it's `"ShadowMesh": { "mesh": "tree_shadow.glb" }`, or
/// `{ "mesh": null }` for no shadow.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let tree = ctx.gfx.load_mesh("tree.glb");
/// let blob = ctx.gfx.load_mesh("tree_shadow.glb"); // a few dozen triangles
/// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(tree), ponko::render::ShadowMesh(Some(blob))));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct ShadowMesh(pub Option<Mesh>);

/// Draws the entity's [`Model`] only into the shadow maps: no camera sees
/// it (the main one or a [`TargetCamera`]), but its shadow falls as usual.
/// For a first-person character whose body would fill the view from inside
/// its head but whose shadow should still show, or for a caster that only
/// stands in for others. Works on plain models and on skinned ones (posed by
/// a [`Skeleton`] or [`SkinOf`]); [`LodGroup`]s, terrain and instance
/// batches don't read it. Remove it to show the model again. Editor picking
/// still selects a shadow-only model, so it can be found and edited.
///
/// In scene files it's `"ShadowOnly": null`.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx, body: Entity) {
/// // First person: the camera sits in the head; keep the body's shadow.
/// ctx.world.insert(body, ponko::render::ShadowOnly);
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, Component)]
pub struct ShadowOnly;

/// Draws the entity's [`Model`] with this material instead of the ones its
/// mesh was made with (every part of the mesh), so one uploaded mesh can be
/// a wet and a dry rock, or a door in three paints, without a second copy
/// of its vertices. Without it, the mesh's own materials are drawn.
///
/// Models that share a mesh and a material are still drawn together, in
/// one draw per mesh part. The [`Model::color`] tint applies on top. A
/// transparent material casts no shadow, as on a mesh made with it.
///
/// It applies to everything that draws the entity's own meshes: a plain
/// model, a skinned model posed by a [`Skeleton`] (on the entity that has
/// the [`Model`], also one that borrows a skeleton with [`SkinOf`]), a mesh
/// with morph targets, every renderer of a [`LodGroup`] and every level of
/// a [`TerrainTile`]. A separate [`ShadowMesh`] keeps its own materials.
/// For some parts only, see [`ModelMaterials`].
///
/// In scene files it's `"ModelMaterial": { "mesh": "rocks.glb", "material":
/// "Wet" }`: a material of a loaded file, by its name in the file (or its
/// number there, from 0). A material made in code has no file to name, so
/// the override isn't saved, like a model whose mesh was made in code.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let rock = ctx.gfx.load_mesh("rock.glb");
/// let wet = ctx.gfx.create_material(ponko::render::MaterialDesc::color(Color::hex(0x404850)).with_roughness(0.1));
/// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(rock)));                    // as the file has it
/// ctx.world.spawn((ponko::d3::Transform::at(vec3(3.0, 0.0, 0.0)), ponko::render::Model::new(rock), ponko::render::ModelMaterial(wet))); // wet
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
#[repr(C)]
pub struct ModelMaterial(pub Material);

/// Draws some parts of the entity's [`Model`] with other materials: the
/// glass of one door, the paint of one car. A mesh's parts are numbered
/// from 0 in the order it was made with: a loaded file's materials in the
/// order they first appear, one part for
/// [`Graphics::create_mesh`](crate::gfx::Graphics::create_mesh).
///
/// Parts that aren't listed keep the [`ModelMaterial`] if the entity has
/// one, else the mesh's own material. A part number the mesh doesn't have
/// is ignored. Like `ModelMaterial` it applies to plain, skinned and
/// morphing models, [`LodGroup`] renderers (each renderer's own parts) and
/// [`TerrainTile`]s, and models with the same mesh and the same list are
/// drawn together.
///
/// In scene files it's `"ModelMaterials": { "parts": [{ "part": 1, "mesh":
/// "door.glb", "material": "Glass" }] }`, with the materials named as for
/// `ModelMaterial`.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let door = ctx.gfx.load_mesh("door.glb"); // part 0: wood, part 1: window
/// let frosted = ctx.gfx.create_material(ponko::render::MaterialDesc::default().with_roughness(0.6));
/// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(door), ponko::render::ModelMaterials::new(&[(1, frosted)])));
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Component)]
pub struct ModelMaterials(Box<[(u32, Material)]>);

impl ModelMaterials {
    /// `(part, material)` pairs. The order doesn't matter; if a part is
    /// listed twice, the last one counts.
    #[must_use]
    pub fn new(parts: &[(usize, Material)]) -> Self {
        let mut list: Vec<(u32, Material)> = Vec::with_capacity(parts.len());
        for &(part, material) in parts {
            let part = u32::try_from(part).unwrap_or(u32::MAX);
            match list.iter_mut().find(|(p, _)| *p == part) {
                Some(entry) => entry.1 = material,
                None => list.push((part, material)),
            }
        }
        list.sort_unstable_by_key(|&(part, _)| part);
        Self(list.into_boxed_slice())
    }

    /// The pairs, by part number.
    pub fn parts(&self) -> impl ExactSizeIterator<Item = (usize, Material)> + '_ {
        self.0.iter().map(|&(part, material)| (part as usize, material))
    }

    /// The material for `part`, if it's listed.
    #[must_use]
    pub fn get(&self, part: usize) -> Option<Material> {
        let part = u32::try_from(part).ok()?;
        let at = self.0.binary_search_by_key(&part, |&(p, _)| p).ok()?;
        Some(self.0[at].1)
    }

    /// The sorted pairs, for the renderer.
    pub(crate) fn raw(&self) -> &[(u32, Material)] {
        &self.0
    }
}

impl Model {
    /// Draws `mesh` as it is.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let rock = ctx.gfx.load_mesh("rock.glb");
    /// ctx.world.spawn((ponko::d3::Transform::default(), ponko::render::Model::new(rock)));
    /// # }
    /// ```
    #[must_use]
    pub fn new(mesh: Mesh) -> Self {
        Self {
            mesh,
            color: Color::WHITE,
        }
    }

    /// The same, tinted with `color`.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, cube: ponko::render::Mesh) {
    /// let red_cube = ponko::render::Model::new(cube).with_color(Color::rgb(1.0, 0.2, 0.2));
    /// # }
    /// ```
    #[must_use]
    pub fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }
}

/// The 3D camera: where it is, which way it looks, and how wide it sees.
/// One per game, at [`SceneSettings::camera`] (`ctx.scene3d.camera`).
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// ctx.scene3d.camera.position = vec3(0.0, 4.0, 10.0);
/// ctx.scene3d.camera.look_at(Vec3::ZERO);
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera3D {
    /// Where the camera is.
    pub position: Vec3,
    /// Which way it looks: it sees along its -Z axis.
    pub rotation: Quat,
    /// How tall the view is, as an angle in radians. Default 60°.
    pub fov_y: f32,
    /// Nothing closer than this is drawn. Default 0.1.
    ///
    /// The scene depth is reverse-Z (the near plane at depth 1, the far
    /// plane at 0, in a 32-bit float), so its precision hardly depends on
    /// the near plane: a first-person camera can use 0.01 for a held item
    /// without the distant world losing depth precision.
    pub near: f32,
    /// Nothing farther than this is drawn (it is also where culling, light
    /// clusters and the sky start). Default 1000.
    pub far: f32,
    /// Skip models hidden behind what was drawn a frame or two before.
    /// Default off.
    ///
    /// The renderer keeps the farthest depth of small screen tiles and reads
    /// it back without waiting for the GPU; a model whose bounds lie behind
    /// it everywhere they cover isn't drawn (it still casts shadows). A model
    /// that comes out from behind something can show up a frame or two late,
    /// mostly when the camera moves fast. Needs compute shaders: on WebGL2
    /// nothing is culled.
    pub occlusion_culling: bool,
    /// Draw particles, trails and lines. Default on. Turn it off for a
    /// [`TargetCamera`](crate::render::TargetCamera) that should show the
    /// scene without them (a security monitor, a map view): see
    /// [`TargetCamera::with_particles`](crate::render::TargetCamera::with_particles).
    pub particles: bool,
    /// Whether it draws `ctx.debug`'s 3D lines and labels: `Some(true)`
    /// always, `Some(false)` never. `None`, the default, keeps what cameras
    /// drew before this flag: a camera in the window draws them, a
    /// [`TargetCamera`](crate::render::TargetCamera) only while it draws
    /// [`particles`](Self::particles). They are on the default layer, so a
    /// camera whose [`layers`](Self::layers) leave it out never draws them.
    /// See [`TargetCamera::with_debug_draw`](crate::render::TargetCamera::with_debug_draw).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("Debug map", 640, 360).run_headless(0, |ctx| {
    ///     // A top-down map that shows the debug drawing without the smoke.
    ///     let map = ctx.gfx.create_render_target(256, 256);
    ///     let mut camera = ctx.scene3d.camera;
    ///     camera.particles = false;
    ///     camera.debug_draw = Some(true);
    ///     ctx.scene3d.passes.target_cameras.push(ponko::render::TargetCamera::new(camera, map));
    ///     // The window's camera keeps its view clean.
    ///     ctx.scene3d.camera.debug_draw = Some(false);
    /// });
    /// ```
    pub debug_draw: Option<bool>,
    /// The part of the window it draws into: the whole window by default.
    /// Split screen, minimaps and picture in picture give each camera its
    /// own (see `d3::View`); its aspect ratio, [`screen_ray`](Self::screen_ray),
    /// [`world_to_screen`](Self::world_to_screen) and the other screen
    /// conversions follow it at once. A
    /// [`TargetCamera`](crate::render::TargetCamera) ignores it: it fills
    /// its target.
    pub rect: crate::camera::Viewport,
    /// Which [`RenderLayers`](crate::camera::RenderLayers) it draws: all by
    /// default. Models on other layers aren't drawn by it (their shadows
    /// still fall in its view). Target cameras read it too.
    pub layers: crate::camera::RenderLayers,
    /// The window's size in logical pixels, kept up to date by the engine;
    /// its part of it comes from `rect`.
    window: Vec2,
}

impl Camera3D {
    pub(crate) fn new(viewport: Vec2) -> Self {
        Self {
            position: Vec3::new(0.0, 2.0, 8.0),
            rotation: look_rotation(Vec3::new(0.0, -2.0, -8.0)),
            fov_y: 60f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            occlusion_culling: false,
            particles: true,
            debug_draw: None,
            rect: crate::camera::Viewport::FULL,
            layers: crate::camera::RenderLayers::ALL,
            window: viewport,
        }
    }

    /// Whether it draws `ctx.debug`'s 3D lines and labels (see
    /// [`debug_draw`](Self::debug_draw)); `target`: it is a target camera.
    pub(crate) fn draws_debug(&self, target: bool) -> bool {
        self.layers.intersects(crate::camera::RenderLayers::DEFAULT)
            && self.debug_draw.unwrap_or(!target || self.particles)
    }

    pub(crate) fn set_viewport(&mut self, viewport: Vec2) {
        self.window = viewport;
    }

    /// Learns the window's size (its part of it comes from its
    /// [`rect`](Self::rect)).
    pub(crate) fn place(&mut self, window: Vec2) {
        self.window = window;
    }

    /// Its part of the window's top-left corner, in logical pixels.
    pub(crate) fn origin(&self) -> Vec2 {
        self.rect.pixels(self.window).min
    }

    /// The part of the window it draws into, in logical pixels (the whole
    /// window unless its [`rect`](Self::rect) says otherwise).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("Race", 800, 600).run_headless(1, |ctx| {
    ///     assert_eq!(ctx.scene3d.camera.screen_rect(), Rect::new(0.0, 0.0, 800.0, 600.0));
    /// });
    /// ```
    #[must_use]
    pub fn screen_rect(&self) -> crate::camera::Rect {
        self.rect.pixels(self.window)
    }

    /// The size the camera projects to, in logical pixels: the window's, or
    /// that of the part of it its [`rect`](Self::rect) gives it. Kept up to
    /// date by the engine (as [`d2::Camera::viewport`](crate::d2::Camera::viewport)).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// let aspect = ctx.scene3d.camera.viewport().x / ctx.scene3d.camera.viewport().y;
    /// # }
    /// ```
    #[must_use]
    pub fn viewport(&self) -> Vec2 {
        self.rect.pixels(self.window).size()
    }

    /// Turns the camera to look at `target`, keeping +Y up.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, player: Vec3) {
    /// ctx.scene3d.camera.position = player + vec3(0.0, 6.0, 8.0);
    /// ctx.scene3d.camera.look_at(player);
    /// # }
    /// ```
    pub fn look_at(&mut self, target: Vec3) {
        self.rotation = look_rotation(target - self.position);
    }

    /// The direction the camera looks.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let ahead = ctx.scene3d.camera.position + ctx.scene3d.camera.forward() * 5.0;
    /// # }
    /// ```
    #[must_use]
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// World to clip space: projection times view. The depth is
    /// reverse-Z: see [`Camera3D::projection`].
    #[must_use]
    pub(crate) fn view_proj(&self) -> Mat4 {
        self.projection() * self.view()
    }

    /// The camera's own space to clip space: a reverse-Z perspective, with
    /// the near plane at depth 1 and the far plane at depth 0 (see
    /// [`reverse_z_perspective`]).
    pub(crate) fn projection(&self) -> Mat4 {
        let aspect = (self.viewport().x / self.viewport().y.max(1.0)).max(1e-3);
        reverse_z_perspective(self.fov_y, aspect, self.near, self.far)
    }

    /// World to the camera's own space.
    pub(crate) fn view(&self) -> Mat4 {
        Mat4::from_rotation_translation(self.rotation, self.position).inverse()
    }

    /// The ray from the camera through window point `screen` (logical
    /// pixels, as [`Ctx::mouse_pos`](crate::ctx::Ctx::mouse_pos)): its start and
    /// unit direction. For picking things in the world with the mouse.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let (from, dir) = ctx.scene3d.camera.screen_ray(ctx.input.mouse_pos());
    /// // Where the mouse points on the ground (y = 0):
    /// if dir.y < 0.0 {
    ///     let ground = from + dir * (-from.y / dir.y);
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn screen_ray(&self, screen: Vec2) -> (Vec3, Vec3) {
        let screen = screen - self.origin();
        let ndc = Vec2::new(
            screen.x / self.viewport().x.max(1.0) * 2.0 - 1.0,
            1.0 - screen.y / self.viewport().y.max(1.0) * 2.0,
        );
        let inverse = self.view_proj().inverse();
        // Any point along the ray will do: halfway in depth (reverse-Z,
        // so 1 would be the near plane and 0 the far one).
        let along = inverse * Vec4::new(ndc.x, ndc.y, 0.5, 1.0);
        let along = along.truncate() / along.w;
        (self.position, (along - self.position).normalize_or(self.forward()))
    }
}

/// A right-handed reverse-Z perspective projection: depth 1 at `near`,
/// 0 at `far` (0..1 clip depth, as wgpu's). Every view drawn into the
/// scene depth uses it (the camera, target cameras, global illumination's
/// captures); shadow maps keep standard depth. Reverse-Z with a float
/// depth buffer keeps nearly constant relative precision at every
/// distance, whatever `far / near` is. `far` may be infinite.
pub(crate) fn reverse_z_perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    if far.is_finite() {
        // Swapping the planes of the standard projection flips the depth.
        glam::camera::rh::proj::directx::perspective(fov_y, aspect, far, near)
    } else {
        glam::camera::rh::proj::directx::perspective_infinite_reverse(fov_y, aspect, near)
    }
}

/// The distance in front of the camera (along its view axis) of a
/// reverse-Z depth `depth` of [`reverse_z_perspective`]: the inverse of
/// that projection's depth (the shaders' depth of field and global
/// illumination do the same).
#[cfg(test)]
pub(crate) fn linear_depth(depth: f32, near: f32, far: f32) -> f32 {
    // depth = near * (far - d) / (d * (far - near)), solved for d.
    if far.is_finite() {
        near * far / (near + depth * (far - near))
    } else {
        near / depth
    }
}

/// The light in the 3D world: a sun far away (one direction everywhere),
/// how bright the ambient light from all around is, shadows and bounced
/// light. At [`SceneSettings::lighting`] (`ctx.scene3d.lighting`). The
/// ambient light's colors come from the sky: see [`Sky::atmosphere`].
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// // Evening: low, orange sun.
/// ctx.scene3d.lighting.sun_direction = vec3(-1.0, -0.3, -0.2);
/// ctx.scene3d.lighting.sun_color = Color::hex(0xFFB066);
/// # }
/// ```
///
/// # Defaults and cost
///
/// [`Default`] is a warm sun of intensity 3 shining down and a little
/// sideways, ambient light at the sky's own brightness, bounced light off,
/// and shadows on: the sun's cast out to 60 world units in 4 cascades of
/// 2048 x 2048 texels (`4 * resolution² * cascades` bytes: 64 MiB), the
/// far cascades refitted every 2 frames, and up to 12 point and spot light
/// shadow views in an atlas of 4096 x 4096 texels (`4 * atlas²` bytes: 64
/// MiB, allocated once a light casts shadows). A still scene draws no
/// shadow pass, since shadow maps are kept while their casters are
/// unchanged. Fewer cascades, a smaller resolution and `shadows: false` are
/// the ways to spend less.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Lighting {
    /// The way the sunlight travels (from the sun towards the ground).
    pub sun_direction: Vec3,
    /// The sun's color.
    pub sun_color: Color,
    /// How bright the sun is. Default 3.
    pub sun_intensity: f32,
    /// How bright the ambient light from the sky and the ground is: 1 is as
    /// bright as the sky makes it (or the [`Sky`]'s flat colors, without
    /// an atmosphere). Default 1.
    pub ambient_intensity: f32,
    /// Whether the sun casts shadows, and point and spot lights with
    /// `shadows` set too (off turns every shadow off). Default on.
    pub shadows: bool,
    /// How far from the camera shadows reach, in world units. Default 60.
    pub shadow_distance: f32,
    /// How many shadow maps (cascades) share that distance, 1 to 4: each
    /// covers a stretch further out than the one before, so shadows stay
    /// sharp near the camera however far they reach. Fewer are cheaper.
    /// Default 4.
    pub shadow_cascades: u32,
    /// Width and height of each of the sun's shadow maps, in texels:
    /// 256 to 4096 (and no more than the GPU allows). Doubling it quadruples
    /// the memory (`4 * resolution² * cascades` bytes) and sharpens shadows.
    /// Changing it at run time makes new shadow maps. Default 2048.
    pub shadow_resolution: u32,
    /// How many frames the far cascades (the farther half of them) keep
    /// the box they are drawn in, 1 to 8. At 1 every cascade is fitted to
    /// the camera every frame, so a moving camera draws them all every
    /// frame. Above 1 the far ones are drawn into a box a little bigger
    /// than they need, held for up to this many frames (they take turns
    /// fitting a new one), or until the camera leaves it or the sun moves
    /// (past [`shadow_sun_step`](Self::shadow_sun_step), if set): while the
    /// camera moves inside it their shadow maps can be kept. Default 2.
    ///
    /// Whatever this is, a shadow map (a cascade, or a point or spot
    /// light's view) is kept from frame to frame only while it holds what
    /// drawing it again would draw, so a still scene draws no shadow pass
    /// and shadows never lag behind their casters. A kept map is drawn
    /// again on the frame that:
    /// - places it differently: a new cascade box, a light that moved;
    /// - changes a caster it holds or one coming into it: a model moved,
    ///   spawned, despawned, given another mesh, tint or material; a
    ///   level-of-detail group switching level; a terrain tile or
    ///   shadow-only caster changing; an instance batch or instance cell
    ///   made, changed or freed where the map can see it (only the maps it
    ///   reaches); an instance batch level whose distance band from the
    ///   camera may have taken instances in or out;
    /// - changes what its casters are drawn with: a mesh's data or shadow
    ///   setting, a material or shader its casters use (only the maps that
    ///   use it), a texture a cut-out or custom shader reads, the shadow
    ///   maps' size.
    ///
    /// Maps holding casters drawn from data that can change unseen
    /// (skinned or morphing models, meshes drawn from GPU buffers, custom
    /// vertex functions reading the time or the camera) are drawn every
    /// frame.
    pub shadow_refresh: u32,
    /// How far the sun may turn, in radians, before its shadows follow it.
    /// 0 (the default): every frame's shadows use the exact sun direction.
    /// Above 0, the sun's shadow maps are drawn from a held direction that
    /// moves only once the true direction has turned more than this from
    /// it; then it jumps to the true direction and is held there. So a sun
    /// that turns a little every frame (a day and night cycle) keeps still,
    /// cached shadows, which change in one step every `shadow_sun_step`
    /// instead of shimmering every frame. Lighting and the sky still use
    /// the exact direction.
    ///
    /// The hold has hysteresis built in: after a jump the true direction
    /// must turn a whole step away again, so a sun wobbling around one
    /// angle never flips the shadows back and forth. It depends only on
    /// the directions drawn since the hold began, so the same sequence of
    /// frames gives the same shadows. The hold starts over (at the true
    /// direction) when this value changes or shadows are turned off; a new
    /// sun more than a step away (a new scene, a time skip) is followed at
    /// once. Around 0.01 to 0.03 (half a degree to two) suits a day cycle.
    pub shadow_sun_step: f32,
    /// How soft the sun's shadow edges are: how many lookups each lit pixel
    /// takes in the shadow map, or [`ShadowFilter::ContactHardening`] for
    /// edges sharp where a shadow touches its caster and softer further
    /// away. Default [`ShadowFilter::Softest`].
    pub shadow_filter: ShadowFilter,
    /// How large the sun is for its shadows' soft edges, in degrees across
    /// (0 to 10): with [`ShadowFilter::ContactHardening`], a shadow's edge
    /// is as wide as the sun seen from the shadow's caster covers, so it
    /// widens by about `2 × tan(shadow_sun_size / 2)` per unit of distance
    /// from the caster. The other filters ignore it. Default 0.53, the real
    /// sun: the shadow of a 10-unit pole under a sun straight above has an
    /// edge about 0.09 units wide at the tip.
    /// It is apart from [`Sky::sun_size`], which draws the disc in the sky
    /// (often larger than the real sun, so it reads on a screen).
    pub shadow_sun_size: f32,
    /// How many shadow views point and spot lights may share, 0 to
    /// [`PointLight::MAX_SHADOW_VIEWS`](crate::d3::PointLight::MAX_SHADOW_VIEWS): a shadowed spot light takes one, a
    /// shadowed point light six. 0 turns point and spot light shadows off.
    /// Default 12.
    pub light_shadow_views: u32,
    /// Width and height of the largest point or spot light shadow view, in
    /// texels: 128 to 2048, rounded down to a power of two. A light whose
    /// reach fills the view gets this; one twice as far away half of it,
    /// down to 64 (see [`PointLight3D::shadow_resolution`] to pick a
    /// light's own). Default [`PointLight::SHADOW_SIZE`](crate::d3::PointLight::SHADOW_SIZE).
    pub light_shadow_resolution: u32,
    /// Width and height of the depth atlas all point and spot light shadow
    /// views are packed into, in texels: 1024 to 8192, rounded down to a
    /// power of two (and no more than the GPU allows). It takes
    /// `4 * light_shadow_atlas²` bytes once a light casts shadows; when
    /// the views don't fit, the largest shrink. Changing it at run time
    /// makes a new atlas. Default [`PointLight::SHADOW_ATLAS_SIZE`](crate::d3::PointLight::SHADOW_ATLAS_SIZE).
    pub light_shadow_atlas: u32,
    /// Light bounced off the world's surfaces (global illumination). Off
    /// by default; see [`GlobalIllumination`].
    pub gi: GlobalIllumination,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            sun_direction: Vec3::new(-0.4, -1.0, -0.3),
            sun_color: Color::hex(0xFFF4E0),
            sun_intensity: 3.0,
            ambient_intensity: 1.0,
            shadows: true,
            shadow_distance: 60.0,
            shadow_cascades: 4,
            shadow_resolution: SHADOW_MAP_SIZE,
            shadow_refresh: 2,
            shadow_sun_step: 0.0,
            shadow_filter: ShadowFilter::Softest,
            shadow_sun_size: DEFAULT_SHADOW_SUN_SIZE,
            light_shadow_views: 12,
            light_shadow_resolution: LIGHT_SHADOW_SIZE,
            light_shadow_atlas: LIGHT_SHADOW_ATLAS_SIZE,
            gi: GlobalIllumination::default(),
        }
    }
}

impl Lighting {
    /// The sun's shadow map size, clamped to what the GPU can make (`limit`
    /// 0: no limit known).
    pub(crate) fn shadow_size(&self, limit: u32) -> u32 {
        let limit = if limit == 0 { u32::MAX } else { limit };
        self.shadow_resolution
            .clamp(MIN_SHADOW_RESOLUTION, MAX_SHADOW_RESOLUTION)
            .min(limit)
    }
    /// The point and spot lights' shadow view size, clamped likewise.
    pub(crate) fn light_shadow_size(&self, limit: u32) -> u32 {
        let limit = if limit == 0 { u32::MAX } else { limit };
        self.light_shadow_resolution
            .clamp(MIN_LIGHT_SHADOW_RESOLUTION, MAX_LIGHT_SHADOW_RESOLUTION)
            .min(limit)
    }
    /// The lights' shadow atlas size, clamped likewise.
    pub(crate) fn light_shadow_atlas_size(&self, limit: u32) -> u32 {
        let limit = if limit == 0 { u32::MAX } else { limit };
        let size = self
            .light_shadow_atlas
            .clamp(MIN_LIGHT_SHADOW_ATLAS, MAX_LIGHT_SHADOW_ATLAS)
            .min(limit);
        shadow_atlas::floor_pow2(size)
    }
    /// How many light shadow views are allowed this frame.
    pub(crate) fn light_shadow_budget(&self) -> usize {
        (self.light_shadow_views as usize).min(MAX_LIGHT_SHADOW_VIEWS)
    }
    /// How much a contact-hardening shadow's edge widens per unit of
    /// distance from its caster: the width of the sun's disc, seen from the
    /// caster, at one unit (`2 × tan(size / 2)`), with
    /// [`shadow_sun_size`](Self::shadow_sun_size) clamped to 0 to
    /// [`MAX_SHADOW_SUN_SIZE`] (a NaN counts as the default). 0 for the
    /// other filters.
    pub(crate) fn penumbra_per_distance(&self) -> f32 {
        if self.shadow_filter != ShadowFilter::ContactHardening {
            return 0.0;
        }
        let size = if self.shadow_sun_size.is_nan() {
            DEFAULT_SHADOW_SUN_SIZE
        } else {
            self.shadow_sun_size.clamp(0.0, MAX_SHADOW_SUN_SIZE)
        };
        2.0 * (size.to_radians() * 0.5).tan()
    }
}

/// [`Lighting::shadow_sun_size`]'s default, in degrees: the real sun.
pub(crate) const DEFAULT_SHADOW_SUN_SIZE: f32 = 0.53;
/// The largest [`Lighting::shadow_sun_size`] drawn, in degrees.
pub(crate) const MAX_SHADOW_SUN_SIZE: f32 = 10.0;

/// How soft the sun's shadow edges are ([`Lighting::shadow_filter`]): how
/// many lookups each lit pixel takes in the shadow map. Each step costs about
/// as much as the lookups it adds, on every lit pixel.
/// ```
/// use ponko::render::{Lighting, ShadowFilter};
/// assert_eq!(Lighting::default().shadow_filter, ShadowFilter::Softest);
/// // Shadows sharp at their caster's foot and soft further away:
/// let mut lighting = Lighting::default();
/// lighting.shadow_filter = ShadowFilter::ContactHardening;
/// lighting.shadow_sun_size = 1.0; // degrees: a larger sun, softer edges
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
#[repr(u32)]
pub enum ShadowFilter {
    /// One smooth lookup (2x2 texels): the cheapest, firmest edges.
    Sharp = 1,
    /// Four lookups (3x3 texels).
    Soft = 2,
    /// Nine lookups (4x4 texels): the softest of the fixed-width edges.
    #[default]
    Softest = 3,
    /// Contact-hardening shadows (as "PCSS" elsewhere): sharp where a
    /// shadow meets its caster and softer the further it falls from it,
    /// as the sun's size makes them ([`Lighting::shadow_sun_size`]). Each
    /// lit pixel first looks for what casts its shadow (16 reads around
    /// it), then filters the edge with 16 lookups across the width that
    /// distance gives; pixels with nothing nearby that casts a shadow stop
    /// after the first 16. The width is in world units, so it stays the
    /// same where two cascades meet; it is never narrower than
    /// [`Softest`](Self::Softest)'s edge in that cascade, and grows up to
    /// what a caster a quarter of [`Lighting::shadow_distance`] away
    /// gives. With temporal antialiasing on, the lookup pattern turns
    /// every frame and the antialiasing smooths it; without, it stays
    /// still. On the GL backend (WebGL2) it draws as
    /// [`Softest`](Self::Softest): its shading language can't read the
    /// shadow map's depths.
    ContactHardening = 4,
}

/// This frame's 3D scene, as the renderer gets it.
#[derive(Clone, Copy)]
pub(crate) struct Scene3D<'a> {
    pub frame: &'a Frame3D,
    pub lighting: &'a Lighting,
    pub sky: &'a Sky,
    /// The sun and ambient light (from the sky, with an atmosphere).
    pub light: SkyLight,
    pub camera: Vec3,
    pub post: &'a Post,
    /// Seconds since the game started, and since the last frame.
    pub time: (f64, f32),
    /// Whether to make the depth grid occlusion culling reads.
    pub occlusion: bool,
}

/// Bytes of one mesh corner in the renderer's vertex buffer (position,
/// normal, uv, color, second uv).
pub(crate) const MESH_VERTEX_BYTES: u64 = 44;

/// Estimated GPU bytes of a mesh made from `data`: its vertex and index
/// buffers as the renderer lays them out.
pub(crate) fn mesh_gpu_bytes(data: &MeshData) -> u64 {
    data.positions.len() as u64 * MESH_VERTEX_BYTES + data.indices.len() as u64 * 4
}

/// What the engine keeps about each mesh and material on the CPU side: for
/// culling, and for headless runs, which have no GPU.
#[derive(Default)]
pub(crate) struct Assets3D {
    pub morph_entities: Vec<ponko_ecs::Entity>,
    pub morph_bases: std::collections::HashMap<ponko_ecs::Entity, (u32, MorphWeights, MorphWeights)>,
    /// Bounding sphere of each mesh, in its own space: center and radius.
    pub bounds: Vec<(Vec3, f32)>,
    /// The id each mesh slot was last given out as, and the freed ones
    /// the next meshes take (under a new generation).
    pub mesh_ids: crate::handle::SlotIds,
    /// The id the next mesh made takes instead (a resident mesh reloaded
    /// after eviction; see `three::resident`).
    pub reuse_mesh_id: Option<u32>,
    /// Each mesh's estimated GPU bytes (`asset_inventory`), by id; 0 once freed.
    pub mesh_bytes: Vec<u64>,
    /// How many times a glTF file was parsed for a scene
    /// (`Graphics::load_node_meshes`): spawns of an unchanged file don't.
    pub gltf_scene_parses: u64,
    pub probes: probe::ProbeAssets,
    /// How each skinned mesh binds to joints, by mesh id; shorter than
    /// `bounds` when the last ones aren't skinned.
    pub skins: Vec<Option<std::sync::Arc<MeshSkin>>>,
    /// Each mesh's morph targets, by mesh id; shorter than `bounds` when
    /// the last ones have none.
    pub morphs: Vec<Option<std::sync::Arc<MeshMorph>>>,
    /// Shared animation clips of skinned glTF files, by where they were found (file or asset source).
    pub loaded_clips: std::collections::HashMap<crate::assets::Origin, Vec<AnimationClip>>,
    /// The joints of skinned glTF files, by where they were found (file or asset source).
    pub loaded_skeletons: std::collections::HashMap<crate::assets::Origin, Skeleton>,
    /// The lights and cameras glTF files place, by where they were found.
    pub loaded_placed: std::collections::HashMap<crate::assets::Origin, (Vec<ModelLight>, Vec<ModelCamera>)>,
    /// Meshes set to cast no shadow (`Graphics::set_mesh_shadows`), by id;
    /// shorter than `bounds` when the last ones cast.
    pub shadowless: Vec<bool>,
    pub material_ids: crate::handle::SlotCounter,
    /// What each material was last made or updated with, for the editor's
    /// material windows.
    #[cfg(feature = "editor")]
    pub material_descs: std::collections::HashMap<u32, MaterialDesc>,
    pub shader_ids: crate::handle::SlotCounter,
    pub compute_ids: crate::handle::SlotCounter,
    pub effect_ids: crate::handle::SlotCounter,
    pub array_ids: crate::handle::SlotCounter,
    /// Names whose status stays visible while a shader, effect or compute is live.
    pub shader_names: std::collections::HashMap<(ShaderKind, u32), String>,
    /// Which materials, shaders, compute passes, effects and texture
    /// arrays are live (made and not freed), by id.
    pub materials_live: crate::handle::SlotLiveSet,
    pub shaders_live: crate::handle::SlotLiveSet,
    pub computes_live: crate::handle::SlotLiveSet,
    pub effects_live: crate::handle::SlotLiveSet,
    pub arrays_live: crate::handle::SlotLiveSet,
    /// Texture arrays made so far, by id.
    pub arrays: Vec<TextureArray>,
    /// Whether each was made as `TextureKind::Data`, by id.
    pub array_data: Vec<bool>,
    /// Terrain tiles made from terrain files, for scenes that load them
    /// again and entities that move them (see `terrain_file::follow_files`).
    pub(crate) terrain_files: terrain_file::FileTiles,
    /// The texture array decals take their images from.
    pub decal_array: Option<TextureArray>,
    /// Whether the renderer can draw projected decals (its GPU has a spare
    /// texture slot); set each frame.
    pub decals_supported: bool,
    /// Whether the renderer can draw global illumination (compute shaders
    /// and a texture slot for the probes); set each frame.
    pub gi_supported: bool,
    /// Baked lighting set with `Graphics::set_baked_lighting` that the
    /// renderer hasn't taken yet: `Some(None)` takes it away.
    pub(crate) baked_upload: Option<Option<bake::BakedLayers>>,
    /// Meshes' and materials' data kept for world bakes
    /// (`Graphics::keep_bake_sources`); `None` while off.
    pub(crate) bake_sources: Option<bake::BakeSources>,
    /// Render targets made so far.
    pub render_targets: Vec<crate::texture::Texture>,
    /// GPU buffers' lengths, by slot; 0 for a freed one.
    pub gpu_buffers: Vec<u32>,
    /// The id each GPU buffer slot was last given out as, and the freed
    /// ones the next buffers take.
    pub gpu_buffer_ids: crate::handle::SlotIds,
    pub instance_batches: Vec<Option<instances::Batch>>,
    pub instance_batch_revision: u64,
    /// Crowds (`Graphics::create_crowd`), by id; `None` once freed.
    pub crowds: Vec<Option<crowd::CrowdData>>,
    /// Instance cells: their handles, and which rows of `instance_batches` they have.
    pub instance_cells: instance_cells::InstanceCells,
    /// Freed GPU buffers' ids whose compute passes haven't been removed yet;
    /// they are released to `gpu_buffer_ids` then.
    pub unlinking_gpu_buffers: Vec<u32>,
    /// Buffer-driven meshes' own bounds (before `set_mesh_gpu_bounds`
    /// grew them), by mesh id.
    pub gpu_mesh_bounds: std::collections::HashMap<u32, (Vec3, f32)>,
    pub loaded_computes: std::collections::HashMap<crate::assets::Origin, Compute>,
    /// Sky shaders made so far (ids are never reused).
    pub sky_shaders: u32,
    /// The sky shaders not freed.
    pub sky_shaders_live: crate::handle::LiveSet,
    /// Sky shaders loaded from files, by where they were found.
    pub loaded_sky_shaders: std::collections::HashMap<crate::assets::Origin, SkyShader>,
    /// Effects loaded from files, by where they were found (file or asset source).
    pub loaded_effects: std::collections::HashMap<crate::assets::Origin, Effect>,
    /// The built-in terrain shader, once a terrain material needs it.
    pub terrain_shader: Option<Shader>,
    /// Shaders loaded from files, by where they were found (file or asset source).
    pub loaded_shaders: std::collections::HashMap<crate::assets::Origin, Shader>,
    /// The material handed out for meshes without one.
    pub default_material: Option<Material>,
    /// Meshes loaded from files, by where they were found (file or asset source).
    pub loaded: std::collections::HashMap<crate::assets::Origin, Mesh>,
    /// Material handles survive geometry eviction; reloading must preserve
    /// user shader edits and must not allocate another material/texture set.
    pub file_materials: std::collections::HashMap<crate::assets::Origin, Vec<Material>>,
    /// The textures each glTF file's materials use, by image index in the
    /// file (shared images included): kept with `file_materials`, freed by
    /// `Graphics::unload_mesh`.
    pub file_textures: std::collections::HashMap<crate::assets::Origin, Vec<crate::texture::Texture>>,
    /// Images that glTF files take from files of their own, by where those
    /// files were found (see `gltf::Loaded::image_sources`): many models
    /// share them, and the same name in two asset sources is two images.
    pub shared_images: std::collections::HashMap<SharedImage, crate::texture::Texture>,
    /// Materials loaded from glTF files, by their name in the file, with
    /// what they were made from (`Graphics::loaded_materials`).
    pub loaded_materials: std::collections::HashMap<String, Vec<(Material, MaterialDesc)>>,
}

impl Assets3D {
    /// A known array retains its white fallback even after its slot is reused.
    fn knows_array(&self, array: TextureArray) -> bool {
        self.arrays_live.was_made(array.id)
            && self
                .arrays
                .get((array.id & crate::handle::INDEX_MASK) as usize)
                .is_some_and(|current| current.id != array.id || *current == array)
    }

    /// Calls `f` with the id and estimated bytes of every live handle of
    /// `class` (`Graphics::asset_handles`); bytes as `inventory` counts them.
    pub(crate) fn records(
        &self,
        class: crate::inventory::AssetClass,
        textures_live: &crate::handle::SlotLiveSet,
        mut f: impl FnMut(u32, u64),
    ) {
        use crate::inventory::{AssetClass, mip_chain_bytes};
        match class {
            AssetClass::RenderTarget => {
                for t in self.render_targets.iter().filter(|t| textures_live.contains(t.id)) {
                    f(t.id, u64::from(t.width()) * u64::from(t.height()) * 8);
                }
            }
            AssetClass::TextureArray => {
                for a in self.arrays.iter().filter(|a| self.arrays_live.contains(a.id)) {
                    f(a.id, mip_chain_bytes(a.width, a.height, 4) * u64::from(a.layers));
                }
            }
            AssetClass::Mesh => {
                for (slot, _) in self.bounds.iter().enumerate().filter(|(_, b)| b.1 >= 0.0) {
                    let id = self.mesh_ids.id_in(slot);
                    f(id, self.mesh_bytes.get(slot).copied().unwrap_or(0));
                }
            }
            AssetClass::GpuBuffer => {
                for (slot, &len) in self.gpu_buffers.iter().enumerate().filter(|(_, l)| **l > 0) {
                    let id = self.gpu_buffer_ids.id_in(slot);
                    f(id, u64::from(len) * 16);
                }
            }
            AssetClass::Material => self.materials_live.ids().for_each(|id| f(id, 0)),
            AssetClass::Shader => self.shaders_live.ids().for_each(|id| f(id, 0)),
            AssetClass::Effect => self.effects_live.ids().for_each(|id| f(id, 0)),
            AssetClass::Compute => self.computes_live.ids().for_each(|id| f(id, 0)),
            AssetClass::Texture | AssetClass::Sound => {}
        }
    }

    /// Live 3D handles by class, counted from the tables that decide
    /// whether each handle is live (`Ctx::asset_inventory`).
    /// Freed ones aren't counted; `textures_live` says which render
    /// targets are.
    pub(crate) fn inventory(&self, textures_live: &crate::handle::SlotLiveSet) -> [crate::inventory::AssetCount; 8] {
        use crate::inventory::{AssetClass, mip_chain_bytes};
        let count = |class, live: usize, bytes: u64| crate::inventory::AssetCount { class, live, bytes };
        // Render targets are linear HDR: 8 bytes a pixel.
        let live_targets = self.render_targets.iter().filter(|t| textures_live.contains(t.id));
        let targets: u64 = live_targets
            .clone()
            .map(|t| u64::from(t.width()) * u64::from(t.height()) * 8)
            .sum();
        let arrays: u64 = self
            .arrays
            .iter()
            .filter(|a| self.arrays_live.contains(a.id))
            .map(|a| mip_chain_bytes(a.width, a.height, 4) * u64::from(a.layers))
            .sum();
        let meshes = self.bounds.iter().filter(|&&(_, r)| r >= 0.0).count();
        let buffers = self.gpu_buffers.iter().filter(|&&len| len > 0).count();
        let buffer_bytes: u64 = self.gpu_buffers.iter().map(|&len| u64::from(len) * 16).sum();
        [
            count(AssetClass::RenderTarget, live_targets.count(), targets),
            count(AssetClass::TextureArray, self.arrays_live.len(), arrays),
            count(AssetClass::Material, self.materials_live.len(), 0),
            count(AssetClass::Shader, self.shaders_live.len(), 0),
            count(AssetClass::Mesh, meshes, self.mesh_bytes.iter().sum()),
            count(AssetClass::GpuBuffer, buffers, buffer_bytes),
            count(AssetClass::Effect, self.effects_live.len(), 0),
            count(AssetClass::Compute, self.computes_live.len(), 0),
        ]
    }

    /// Mesh `id`'s bounds, unless there's no such mesh or it was freed.
    pub(crate) fn live_bounds(&self, id: u32) -> Option<(Vec3, f32)> {
        self.bounds.get(self.mesh_slot(id)?).copied().filter(|&(_, r)| r >= 0.0)
    }

    /// The slot of mesh `id`, unless the id is stale (its slot was given
    /// to a newer mesh) or was never made. Freed meshes keep their slot
    /// until it is reused; [`live_bounds`](Self::live_bounds) tells those apart.
    pub(crate) fn mesh_slot(&self, id: u32) -> Option<usize> {
        self.mesh_ids.slot(id).filter(|&slot| slot < self.bounds.len())
    }

    /// The live mesh handle in `slot`, for what the renderer reports by slot.
    pub(crate) fn mesh_in_slot(&self, slot: u32) -> Mesh {
        Mesh {
            id: self.mesh_ids.id_in(slot as usize),
        }
    }

    /// The slot of GPU buffer `id`, unless the id is stale or was never made.
    pub(crate) fn gpu_buffer_slot(&self, id: u32) -> Option<usize> {
        self.gpu_buffer_ids
            .slot(id)
            .filter(|&slot| slot < self.gpu_buffers.len())
    }

    /// How the mesh in `id`'s slot binds to joints, if it's skinned. Takes
    /// a mesh id or a slot; check the mesh with `live_bounds` first.
    pub(crate) fn skin(&self, id: u32) -> Option<&MeshSkin> {
        self.skins.get((id & SLOT_MASK) as usize)?.as_deref()
    }

    /// The morph targets of the mesh in `id`'s slot, if it has any (as
    /// [`skin`](Self::skin)).
    pub(crate) fn morph(&self, id: u32) -> Option<&MeshMorph> {
        self.morphs.get((id & SLOT_MASK) as usize)?.as_deref()
    }

    /// Whether the mesh in `id`'s slot casts shadows (the default; as
    /// [`skin`](Self::skin)).
    pub(crate) fn casts_shadows(&self, id: u32) -> bool {
        !self.shadowless.get((id & SLOT_MASK) as usize).copied().unwrap_or(false)
    }
}

/// sRGB color to linear RGBA, for the GPU.
pub(crate) fn linear(c: Color) -> [f32; 4] {
    // Most tints are white: skip the three `powf`s (this runs per instance).
    if c.r == 1.0 && c.g == 1.0 && c.b == 1.0 {
        return [1.0, 1.0, 1.0, c.a];
    }
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    [lin(c.r), lin(c.g), lin(c.b), c.a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_at_points_minus_z_at_the_target() {
        for target in [Vec3::X, Vec3::NEG_X, Vec3::new(1.0, 2.0, -3.0), Vec3::Y, Vec3::NEG_Y] {
            let t = Transform3D::default().looking_at(target);
            assert!((t.forward() - target.normalize()).length() < 1e-5, "{target}");
        }
    }

    #[test]
    fn the_screen_center_ray_goes_where_the_camera_looks() {
        let mut camera = Camera3D::new(Vec2::new(800.0, 600.0));
        camera.position = Vec3::new(3.0, 4.0, 5.0);
        camera.look_at(Vec3::ZERO);
        let (from, dir) = camera.screen_ray(Vec2::new(400.0, 300.0));
        assert_eq!(from, camera.position);
        assert!((dir - camera.forward()).length() < 1e-4);
        // Left of center points left of forward.
        let (_, left) = camera.screen_ray(Vec2::new(0.0, 300.0));
        let right_axis = camera.rotation * Vec3::X;
        assert!(left.dot(right_axis) < 0.0);
    }

    /// The reverse-Z depth of a point `distance` in front of a camera with
    /// `projection`.
    fn depth_at(projection: Mat4, distance: f32) -> f32 {
        let clip = projection * Vec4::new(0.0, 0.0, -distance, 1.0);
        clip.z / clip.w
    }

    #[test]
    fn reverse_z_puts_the_near_plane_at_one_and_the_far_plane_at_zero() {
        for (near, far) in [(0.1, 1000.0), (0.01, 16_000.0), (0.5, 50.0)] {
            let m = reverse_z_perspective(1.0, 1.5, near, far);
            assert!(
                (depth_at(m, near) - 1.0).abs() < 1e-6,
                "near {near}: {}",
                depth_at(m, near)
            );
            assert!(depth_at(m, far).abs() < 1e-6, "far {far}: {}", depth_at(m, far));
            // Nearer is greater all the way.
            let mut last = 2.0;
            for d in [near, near * 2.0, (near * far).sqrt(), far * 0.5, far] {
                let z = depth_at(m, d);
                assert!(z < last, "{d}: {z} after {last}");
                last = z;
            }
            // Past the far plane and before the near plane: clipped.
            assert!(depth_at(m, far * 1.01) < 0.0 && depth_at(m, near * 0.99) > 1.0);
        }
    }

    #[test]
    fn an_infinite_far_plane_reaches_zero_only_at_infinity() {
        let m = reverse_z_perspective(1.0, 1.0, 0.01, f32::INFINITY);
        assert!((depth_at(m, 0.01) - 1.0).abs() < 1e-6);
        for d in [10.0, 1e4, 1e7] {
            let z = depth_at(m, d);
            assert!(z > 0.0 && (z - 0.01 / d).abs() < 1e-6 * z.max(1e-9), "{d}: {z}");
        }
        // The same picture as a finite far plane (x and y don't depend on it).
        let finite = reverse_z_perspective(1.0, 1.0, 0.01, 1000.0);
        assert_eq!(m.x_axis, finite.x_axis);
        assert_eq!(m.y_axis, finite.y_axis);
    }

    #[test]
    fn linear_depth_undoes_the_projection() {
        for (near, far) in [(0.1, 1000.0), (0.01, 16_000.0), (0.01, f32::INFINITY)] {
            let m = reverse_z_perspective(1.0, 1.0, near, far);
            for d in [near, 1.0, 100.0, 400.0, 5000.0] {
                let back = linear_depth(depth_at(m, d), near, far);
                assert!((back - d).abs() <= d * 1e-4, "{near}..{far}: {d} came back as {back}");
            }
        }
    }

    /// How far apart two distances `distance` in front of the camera must
    /// be to get different 32-bit float depths: the depth buffer's step
    /// there. `depth` maps a distance to the stored depth.
    fn depth_step(distance: f64, depth: impl Fn(f64) -> f64) -> f64 {
        let stored = depth(distance) as f32;
        let next = f32::from_bits(stored.to_bits() + 1);
        // The depth's slope at that distance turns one float step into metres.
        let h = distance * 1e-6;
        let slope = (depth(distance + h) - depth(distance - h)) / (2.0 * h);
        f64::from(next - stored).abs() / slope.abs()
    }

    #[test]
    fn reverse_z_depth_steps_are_micrometres_where_standard_z_has_centimetres() {
        let (near, far) = (0.01f64, 16_000.0f64);
        let standard = |d: f64| far * (d - near) / (d * (far - near));
        let reverse = |d: f64| near * (far - d) / (d * (far - near));
        // The engine's own projection is the reverse one.
        let m = reverse_z_perspective(1.0, 1.0, near as f32, far as f32);
        assert!((f64::from(depth_at(m, 250.0)) - reverse(250.0)).abs() < 1e-9);
        for (d, standard_at_least) in [(100.0, 0.02), (400.0, 0.4)] {
            let coarse = depth_step(d, standard);
            let fine = depth_step(d, reverse);
            eprintln!("{d} m: standard-Z step {coarse:.4} m, reverse-Z step {fine:.2e} m");
            // Negative control: standard depth is centimetres to a metre
            // coarse there, as the issue measured.
            assert!(coarse >= standard_at_least, "standard-Z at {d} m: {coarse} m");
            // Reverse-Z is finer than a tenth of a millimetre.
            assert!(fine < 1e-4, "reverse-Z at {d} m: {fine} m");
            assert!(coarse / fine > 1000.0);
        }
    }

    #[test]
    fn the_view_projection_puts_the_target_in_the_middle() {
        let mut camera = Camera3D::new(Vec2::new(800.0, 600.0));
        camera.position = Vec3::new(0.0, 10.0, 10.0);
        camera.look_at(Vec3::new(1.0, 0.0, 0.0));
        let clip = camera.view_proj() * Vec4::new(1.0, 0.0, 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(
            ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4 && (0.0..1.0).contains(&ndc.z),
            "{ndc}"
        );
    }
}

/// Where a glTF image shared between models came from: the cache key of
/// `Assets3d::shared_images`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum SharedImage {
    /// An image file of its own.
    Image(crate::assets::Origin),
    /// An ORM image made from a metallic-roughness and an occlusion image
    /// (either can be missing).
    Orm(Option<crate::assets::Origin>, Option<crate::assets::Origin>),
}

impl SharedImage {
    /// The key of a glTF image source (`gltf::Loaded::image_sources`): an
    /// asset name, or `orm:<name>|<name>` with an empty name for a missing
    /// image. Each name is keyed by the source that has it, so a model from
    /// one source shares images only with models whose images came from
    /// the same place. `None` if a name is in no source.
    pub(crate) fn of(sources: &crate::assets::SharedSources, image_source: &str) -> Option<Self> {
        let locate = |name: &str| sources.locate(name);
        match image_source.strip_prefix("orm:").and_then(|rest| rest.split_once('|')) {
            Some((m, o)) => {
                let part = |name: &str| {
                    if name.is_empty() {
                        Some(None)
                    } else {
                        locate(name).map(Some)
                    }
                };
                Some(Self::Orm(part(m)?, part(o)?))
            }
            None => locate(image_source).map(Self::Image),
        }
    }
}

/// The C API (`crates/ponko_ffi`) copies these types field by field and
/// variant by variant, but outside this crate `#[non_exhaustive]` stops it
/// from matching or destructuring them exhaustively. These patterns stop the
/// build of this crate's tests instead when one grows: update the C struct
/// and its conversions (and `include/ponko.h`), then the pattern here.
#[cfg(test)]
mod c_api_mirror {
    use super::*;

    /// The C value of each blend mode (`PonkoMaterialDesc::blend`).
    fn c_blend(blend: Blend) -> u32 {
        match blend {
            Blend::Opaque => 0,
            Blend::Transparent => 1,
            Blend::Cutout => 2,
        }
    }

    #[test]
    fn the_c_api_mirrors_every_field_and_blend_mode() {
        for blend in [Blend::Opaque, Blend::Transparent, Blend::Cutout] {
            assert_eq!(c_blend(blend), blend as u32);
        }
        let MaterialDesc {
            color: _,
            texture: _,
            roughness: _,
            metallic: _,
            emissive: _,
            normal_map: _,
            normal_strength: _,
            orm_map: _,
            emissive_map: _,
            blend: _,
            double_sided: _,
            shader: _,
            custom: _,
            custom_textures: _,
            custom_arrays: _,
            // C: `ponko_ctx_create_material_with_stencil` and `ponko_ctx_update_material_with_stencil`.
            stencil: _,
        } = MaterialDesc::default();
        let ParticleSettings3D {
            rate: _,
            burst: _,
            duration: _,
            looping: _,
            lifetime: _,
            speed: _,
            shape: _,
            gravity: _,
            drag: _,
            size: _,
            rotation: _,
            size_over_life: _,
            color_over_life: _,
            texture: _,
            sheet: _,
            blend: _,
            max_particles: _,
            space: _,
            seed: _,
            // Not in PonkoParticleSettings3D (its layout is frozen): it is the
            // `softness` of PonkoParticleSettings3DEx.
            softness: _,
        } = ParticleSettings3D::default();
        let texture = TextureArray {
            id: 0,
            width: 1,
            height: 1,
            layers: 1,
        };
        let TerrainMaterialDesc {
            layers: _,
            normals: _,
            normal_strength: _,
            weights: _,
            weights_corner: _,
            weights_size: _,
            layer_size: _,
            roughness: _,
        } = TerrainMaterialDesc::new(texture, texture, glam::Vec2::ZERO, glam::Vec2::ONE);
    }
}
