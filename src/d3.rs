//! Three-dimensional game types. Pair with `ponko::prelude::*`.
//!
//! **Maturity: tested**, from the guide chapter `3d.md`. Game: Shard Grove, played headless by
//! `tests/shard_grove.rs`; its package is played headless in the list of checks. Tests:
//! `tests/three.rs`, `tests/lights.rs`, `tests/morph3d.rs` and the 3D render tests. Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics:
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found. Missing: Shard Grove's frame-time baseline per GPU
//! tier, and its web build and window played on a real GPU of each OS (#454).
//!
//! `use ponko::d3::*;` brings in the types of a 3D world that have a 2D
//! twin or would otherwise carry a `3D` in their name: the transform, the
//! camera, lights, decals, text, lines, trails, particle emitters,
//! reflection probes, positional audio, navigation agents, world-space UI
//! panels and the physics (feature `physics3d`). This module is their one
//! path, under these names, so it is the name rustdoc and the guide use.
//!
//! The rest of 3D lives in its topic module: [`render`](crate::render)
//! (meshes, materials, models, shaders, level of detail, render settings),
//! [`anim`](crate::anim) (skeletons, clips, animators, IK),
//! [`terrain`](crate::terrain), [`particles`](crate::particles) (curves and
#![cfg_attr(
    any(feature = "physics", feature = "physics3d"),
    doc = " gradients shared with 2D), [`collision`](crate::collision) (body kinds"
)]
#![cfg_attr(
    not(any(feature = "physics", feature = "physics3d")),
    doc = " gradients shared with 2D), `collision` (body kinds"
)]
//! and collision events, shared with 2D), [`attach`](crate::attach)
//! (3D attachments, `Attached3D`, with the 2D ones) and [`nav`](crate::nav)
//! (navigation mesh baking).
//!
//! ```
//! use ponko::prelude::*;
//! use ponko::d3::*;
//! use ponko::render::MaterialDesc;
//!
//! let desc = MaterialDesc::color(Color::WHITE);
//! let light = PointLight { intensity: 20.0, ..Default::default() };
//! # let _ = (desc, light, Transform::default());
//! ```
//!
//! ## Physics
//!
//! 3D physics with rapier (feature `physics3d`): rigid bodies, colliders
//! (heightfield terrain and triangle meshes too), a character controller
//! and raycast-wheel vehicles as components, stepped inside the simulation
//! clock. [`Transform`] stays the source of truth: set it and the body
//! teleports; moving bodies write their new pose back after every step,
//! and between steps (in windowed games) a pose blended between the last
//! two steps, so motion looks smooth at any frame rate. Turn it on with
#![cfg_attr(feature = "physics3d", doc = " [`Game::physics3d`](crate::game::Game::physics3d).")]
#![cfg_attr(not(feature = "physics3d"), doc = " `Game::physics3d`.")]
#![deny(missing_docs)]

// Types whose home is this module.
#[doc(inline)]
pub use crate::camera::rigs3d::{FirstPersonCamera, OrbitCamera, ThirdPersonCamera};
/// Another 3D camera in the window: split screen, a minimap (see [`SceneSettings::views`](crate::render::SceneSettings::views)).
pub use crate::camera::views3d::View3D as View;
#[doc(inline)]
pub use crate::three::Camera3D as Camera;
#[doc(inline)]
pub use crate::three::Decal3D as Decal;
#[doc(inline)]
pub use crate::three::Line3D as Line;
#[doc(inline)]
pub use crate::three::ParticleBlend3D as ParticleBlend;
#[doc(inline)]
pub use crate::three::ParticleEmitter3D as ParticleEmitter;
#[doc(inline)]
pub use crate::three::ParticleSettings3D as ParticleSettings;
#[doc(inline)]
pub use crate::three::ParticleShape3D as ParticleShape;
#[doc(inline)]
pub use crate::three::PointLight3D as PointLight;
#[doc(inline)]
pub use crate::three::ProbeRefresh;
#[doc(inline)]
pub use crate::three::ReflectionProbe3D as ReflectionProbe;
#[doc(inline)]
pub use crate::three::ReflectionRefresh3D as ReflectionRefresh;
#[doc(inline)]
pub use crate::three::SpotLight3D as SpotLight;
#[doc(inline)]
pub use crate::three::Text3D as Text;
#[doc(inline)]
pub use crate::three::TextAlign3D as TextAlign;
#[doc(inline)]
pub use crate::three::Trail3D as Trail;
#[doc(inline)]
pub use crate::three::Transform3D as Transform;
#[doc(inline)]
pub use crate::three::{
    GltfNode, GltfOptions, GltfRoot, GltfScene, GltfSceneNode, NodeBody, NodeCollider, NodeProperties, SpawnPoint,
    gltf_properties_schema,
};
#[doc(inline)]
pub use crate::three::{HeldItem, VIEW_MODEL_LAYER, ViewModel};

#[doc(inline)]
pub use crate::audio::spatial::{
    AudioListener3D as AudioListener, AudioSettings3D as AudioSettings, AudioSource3D as AudioSource,
};
#[cfg(feature = "navmesh")]
#[doc(inline)]
pub use crate::nav3d::{
    NavAgent3D as NavAgent, NavAgentState, NavConvexPolygon, NavMesh3D as NavMesh, NavObstacle3D as NavObstacle,
    NavObstacleShape,
};
#[cfg(feature = "physics3d")]
#[doc(inline)]
pub use crate::physics3d_impl::{
    Character3D as Character, Collider3D as Collider, ColliderFocus, CollisionGroups, CollisionMesh, CollisionMeshData,
    CollisionMeshOptions, ContactModifier3D as ContactModifier, ContactPair3D as ContactPair, FrictionCurve,
    Hit3D as Hit, Joint3D as Joint, JointAxis3D as JointAxis, JointDrive3D as JointDrive, JointMotion3D as JointMotion,
    Penetration3D as Penetration, Physics3D as Physics, Physics3DState as PhysicsState, PointHit3D as PointHit,
    QueryFilter3D as QueryFilter, Ragdoll3D as Ragdoll, RagdollBone, RigidBody3D as RigidBody, Shape3D as Shape,
    SquareColliderFocus, TerrainCollider, Vehicle3D as Vehicle, Velocity3D as Velocity, Wheel3D as Wheel,
};
#[doc(inline)]
pub use crate::ui::panel3d::UiPanel3D as UiPanel;

// 0.1 re-exported the types of topic modules here. They have one path now,
// in their module; these aliases keep 0.1 code building, with a warning that
// names the path, until 0.3.0 removes them. Not aliased: `Model` and the
// collision types, which the prelude brings (an alias here would make
// `use ponko::prelude::*; use ponko::d3::*;` ambiguous for them).
/// Moved: [`ponko::render::AmbientOcclusion`](crate::render::AmbientOcclusion).
#[deprecated(since = "0.2.0", note = "use `ponko::render::AmbientOcclusion`")]
pub type AmbientOcclusion = crate::render::AmbientOcclusion;
/// Moved: [`ponko::render::AntiAliasing`](crate::render::AntiAliasing).
#[deprecated(since = "0.2.0", note = "use `ponko::render::AntiAliasing`")]
pub type AntiAliasing = crate::render::AntiAliasing;
/// Moved: [`ponko::render::BatchCulling`](crate::render::BatchCulling).
#[deprecated(since = "0.2.0", note = "use `ponko::render::BatchCulling`")]
pub type BatchCulling = crate::render::BatchCulling;
/// Moved: [`ponko::render::Blend`](crate::render::Blend).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Blend`")]
pub type Blend = crate::render::Blend;
/// Moved: [`ponko::render::Compute`](crate::render::Compute).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Compute`")]
pub type Compute = crate::render::Compute;
/// Moved: [`ponko::render::ComputePass`](crate::render::ComputePass).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ComputePass`")]
pub type ComputePass = crate::render::ComputePass;
/// Moved: [`ponko::render::Effect`](crate::render::Effect).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Effect`")]
pub type Effect = crate::render::Effect;
/// Moved: [`ponko::render::GlobalIllumination`](crate::render::GlobalIllumination).
#[deprecated(since = "0.2.0", note = "use `ponko::render::GlobalIllumination`")]
pub type GlobalIllumination = crate::render::GlobalIllumination;
/// Moved: [`ponko::render::GpuBuffer`](crate::render::GpuBuffer).
#[deprecated(since = "0.2.0", note = "use `ponko::render::GpuBuffer`")]
pub type GpuBuffer = crate::render::GpuBuffer;
/// Moved: [`ponko::render::GraphicsQuality`](crate::render::GraphicsQuality).
#[deprecated(since = "0.2.0", note = "use `ponko::render::GraphicsQuality`")]
pub type GraphicsQuality = crate::render::GraphicsQuality;
/// Moved: [`ponko::render::InstanceBatch`](crate::render::InstanceBatch).
#[deprecated(since = "0.2.0", note = "use `ponko::render::InstanceBatch`")]
pub type InstanceBatch = crate::render::InstanceBatch;
/// Moved: [`ponko::render::InstanceBatchOptions`](crate::render::InstanceBatchOptions).
#[deprecated(since = "0.2.0", note = "use `ponko::render::InstanceBatchOptions`")]
pub type InstanceBatchOptions = crate::render::InstanceBatchOptions;
/// Moved: [`ponko::render::InstanceLevel`](crate::render::InstanceLevel).
#[deprecated(since = "0.2.0", note = "use `ponko::render::InstanceLevel`")]
pub type InstanceLevel = crate::render::InstanceLevel;
/// Moved: [`ponko::render::Lighting`](crate::render::Lighting).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Lighting`")]
pub type Lighting = crate::render::Lighting;
/// Moved: [`ponko::render::LodGroup`](crate::render::LodGroup).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodGroup`")]
pub type LodGroup = crate::render::LodGroup;
/// Moved: [`ponko::render::LodHysteresis`](crate::render::LodHysteresis).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodHysteresis`")]
pub type LodHysteresis = crate::render::LodHysteresis;
/// Moved: [`ponko::render::LodLevel`](crate::render::LodLevel).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodLevel`")]
pub type LodLevel = crate::render::LodLevel;
/// Moved: [`ponko::render::LodPick`](crate::render::LodPick).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodPick`")]
pub type LodPick = crate::render::LodPick;
/// Moved: [`ponko::render::LodRenderer`](crate::render::LodRenderer).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodRenderer`")]
pub type LodRenderer = crate::render::LodRenderer;
/// Moved: [`ponko::render::LodSettings`](crate::render::LodSettings).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodSettings`")]
pub type LodSettings = crate::render::LodSettings;
/// Moved: [`ponko::render::LodShadow`](crate::render::LodShadow).
#[deprecated(since = "0.2.0", note = "use `ponko::render::LodShadow`")]
pub type LodShadow = crate::render::LodShadow;
/// Moved: [`ponko::render::Material`](crate::render::Material).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Material`")]
pub type Material = crate::render::Material;
/// Moved: [`ponko::render::MaterialDesc`](crate::render::MaterialDesc).
#[deprecated(since = "0.2.0", note = "use `ponko::render::MaterialDesc`")]
pub type MaterialDesc = crate::render::MaterialDesc;
/// Moved: [`ponko::render::Mesh`](crate::render::Mesh).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Mesh`")]
pub type Mesh = crate::render::Mesh;
/// Moved: [`ponko::render::MeshCost`](crate::render::MeshCost).
#[deprecated(since = "0.2.0", note = "use `ponko::render::MeshCost`")]
pub type MeshCost = crate::render::MeshCost;
/// Moved: [`ponko::render::MeshData`](crate::render::MeshData).
#[deprecated(since = "0.2.0", note = "use `ponko::render::MeshData`")]
pub type MeshData = crate::render::MeshData;
/// Moved: [`ponko::render::ModelMaterial`](crate::render::ModelMaterial).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ModelMaterial`")]
pub type ModelMaterial = crate::render::ModelMaterial;
/// Moved: [`ponko::render::ModelMaterials`](crate::render::ModelMaterials).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ModelMaterials`")]
pub type ModelMaterials = crate::render::ModelMaterials;
/// Moved: [`ponko::render::Post`](crate::render::Post).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Post`")]
pub type Post = crate::render::Post;
/// Moved: [`ponko::render::ReceivesDecals`](crate::render::ReceivesDecals).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ReceivesDecals`")]
pub type ReceivesDecals = crate::render::ReceivesDecals;
/// Moved: [`ponko::render::RenderPasses`](crate::render::RenderPasses).
#[deprecated(since = "0.2.0", note = "use `ponko::render::RenderPasses`")]
pub type RenderPasses = crate::render::RenderPasses;
/// Moved: [`ponko::render::SceneSettings`](crate::render::SceneSettings).
#[deprecated(since = "0.2.0", note = "use `ponko::render::SceneSettings`")]
pub type SceneSettings = crate::render::SceneSettings;
/// Moved: [`ponko::render::ScreenEffect`](crate::render::ScreenEffect).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ScreenEffect`")]
pub type ScreenEffect = crate::render::ScreenEffect;
/// Moved: [`ponko::render::Shader`](crate::render::Shader).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Shader`")]
pub type Shader = crate::render::Shader;
/// Moved: [`ponko::render::ShaderDiagnostic`](crate::render::ShaderDiagnostic).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ShaderDiagnostic`")]
pub type ShaderDiagnostic = crate::render::ShaderDiagnostic;
/// Moved: [`ponko::render::ShaderKind`](crate::render::ShaderKind).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ShaderKind`")]
pub type ShaderKind = crate::render::ShaderKind;
/// Moved: [`ponko::render::ShaderOutcome`](crate::render::ShaderOutcome).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ShaderOutcome`")]
pub type ShaderOutcome = crate::render::ShaderOutcome;
/// Moved: [`ponko::render::ShaderStatus`](crate::render::ShaderStatus).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ShaderStatus`")]
pub type ShaderStatus = crate::render::ShaderStatus;
/// Moved: [`ponko::render::ShadowFilter`](crate::render::ShadowFilter).
#[deprecated(since = "0.2.0", note = "use `ponko::render::ShadowFilter`")]
pub type ShadowFilter = crate::render::ShadowFilter;
/// Moved: [`ponko::render::ShadowMesh`](crate::render::ShadowMesh).
///
/// The aliases warn, as every deprecated item does (here: fail, with the
/// warning denied); with `#![allow(deprecated)]` the same code builds:
///
/// ```compile_fail
/// #![deny(deprecated)]
/// use ponko::d3::*;
/// let shadow: ShadowMesh = ponko::render::ShadowMesh(None);
/// let desc = MaterialDesc::default();
/// ```
///
/// ```
/// #![allow(deprecated)]
/// use ponko::d3::*;
/// let shadow: ShadowMesh = ponko::render::ShadowMesh(None);
/// let desc = MaterialDesc::default();
/// ```
#[deprecated(since = "0.2.0", note = "use `ponko::render::ShadowMesh`")]
pub type ShadowMesh = crate::render::ShadowMesh;
/// Moved: [`ponko::render::Sky`](crate::render::Sky).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Sky`")]
pub type Sky = crate::render::Sky;
/// Moved: [`ponko::render::StreamedLod`](crate::render::StreamedLod).
#[deprecated(since = "0.2.0", note = "use `ponko::render::StreamedLod`")]
pub type StreamedLod = crate::render::StreamedLod;
/// Moved: [`ponko::render::TargetCamera`](crate::render::TargetCamera).
#[deprecated(since = "0.2.0", note = "use `ponko::render::TargetCamera`")]
pub type TargetCamera = crate::render::TargetCamera;
/// Moved: [`ponko::render::TargetEffect`](crate::render::TargetEffect).
#[deprecated(since = "0.2.0", note = "use `ponko::render::TargetEffect`")]
pub type TargetEffect = crate::render::TargetEffect;
/// Moved: [`ponko::render::Tonemap`](crate::render::Tonemap).
#[deprecated(since = "0.2.0", note = "use `ponko::render::Tonemap`")]
pub type Tonemap = crate::render::Tonemap;
/// Moved: [`ponko::anim::AnimationClip`](crate::anim::AnimationClip).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationClip`")]
pub type AnimationClip = crate::anim::AnimationClip;
/// Moved: [`ponko::anim::AnimationCondition`](crate::anim::AnimationCondition).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationCondition`")]
pub type AnimationCondition = crate::anim::AnimationCondition;
/// Moved: [`ponko::anim::AnimationEvent`](crate::anim::AnimationEvent).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationEvent`")]
pub type AnimationEvent = crate::anim::AnimationEvent;
/// Moved: [`ponko::anim::AnimationGraph`](crate::anim::AnimationGraph).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationGraph`")]
pub type AnimationGraph = crate::anim::AnimationGraph;
/// Moved: [`ponko::anim::AnimationGraphError`](crate::anim::AnimationGraphError).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationGraphError`")]
pub type AnimationGraphError = crate::anim::AnimationGraphError;
/// Moved: [`ponko::anim::AnimationLayer`](crate::anim::AnimationLayer).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationLayer`")]
pub type AnimationLayer = crate::anim::AnimationLayer;
/// Moved: [`ponko::anim::AnimationParameter`](crate::anim::AnimationParameter).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationParameter`")]
pub type AnimationParameter = crate::anim::AnimationParameter;
/// Moved: [`ponko::anim::AnimationState`](crate::anim::AnimationState).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationState`")]
pub type AnimationState = crate::anim::AnimationState;
/// Moved: [`ponko::anim::AnimationTrack`](crate::anim::AnimationTrack).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationTrack`")]
pub type AnimationTrack = crate::anim::AnimationTrack;
/// Moved: [`ponko::anim::AnimationTransition`](crate::anim::AnimationTransition).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AnimationTransition`")]
pub type AnimationTransition = crate::anim::AnimationTransition;
/// Moved: [`ponko::anim::Animator`](crate::anim::Animator).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::Animator`")]
pub type Animator = crate::anim::Animator;
/// Moved: [`ponko::anim::AttachToJoint`](crate::anim::AttachToJoint).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::AttachToJoint`")]
pub type AttachToJoint = crate::anim::AttachToJoint;
/// Moved: [`ponko::anim::Interpolation`](crate::anim::Interpolation).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::Interpolation`")]
pub type Interpolation = crate::anim::Interpolation;
/// Moved: [`ponko::anim::JointAnimation`](crate::anim::JointAnimation).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::JointAnimation`")]
pub type JointAnimation = crate::anim::JointAnimation;
/// Moved: [`ponko::anim::JointMask`](crate::anim::JointMask).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::JointMask`")]
pub type JointMask = crate::anim::JointMask;
/// Moved: [`ponko::anim::LayerBlend`](crate::anim::LayerBlend).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::LayerBlend`")]
pub type LayerBlend = crate::anim::LayerBlend;
/// Moved: [`ponko::anim::LayerId`](crate::anim::LayerId).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::LayerId`")]
pub type LayerId = crate::anim::LayerId;
/// Moved: [`ponko::anim::MorphAnimation`](crate::anim::MorphAnimation).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::MorphAnimation`")]
pub type MorphAnimation = crate::anim::MorphAnimation;
/// Moved: [`ponko::anim::MorphFrame`](crate::anim::MorphFrame).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::MorphFrame`")]
pub type MorphFrame = crate::anim::MorphFrame;
/// Moved: [`ponko::anim::MorphTarget`](crate::anim::MorphTarget).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::MorphTarget`")]
pub type MorphTarget = crate::anim::MorphTarget;
/// Moved: [`ponko::anim::MorphWeights`](crate::anim::MorphWeights).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::MorphWeights`")]
pub type MorphWeights = crate::anim::MorphWeights;
/// Moved: [`ponko::anim::Motion`](crate::anim::Motion).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::Motion`")]
pub type Motion = crate::anim::Motion;
/// Moved: [`ponko::anim::MotionChild`](crate::anim::MotionChild).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::MotionChild`")]
pub type MotionChild = crate::anim::MotionChild;
/// Moved: [`ponko::anim::ParamId`](crate::anim::ParamId).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::ParamId`")]
pub type ParamId = crate::anim::ParamId;
/// Moved: [`ponko::anim::RootMotion`](crate::anim::RootMotion).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::RootMotion`")]
pub type RootMotion = crate::anim::RootMotion;
/// Moved: [`ponko::anim::Skeleton`](crate::anim::Skeleton).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::Skeleton`")]
pub type Skeleton = crate::anim::Skeleton;
/// Moved: [`ponko::anim::SkinOf`](crate::anim::SkinOf).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::SkinOf`")]
pub type SkinOf = crate::anim::SkinOf;
/// Moved: [`ponko::anim::StateId`](crate::anim::StateId).
#[deprecated(since = "0.2.0", note = "use `ponko::anim::StateId`")]
pub type StateId = crate::anim::StateId;
/// Moved: [`ponko::particles::ParticleCurve`](crate::particles::ParticleCurve).
#[deprecated(since = "0.2.0", note = "use `ponko::particles::ParticleCurve`")]
pub type ParticleCurve = crate::particles::ParticleCurve;
/// Moved: [`ponko::particles::ParticleGradient`](crate::particles::ParticleGradient).
#[deprecated(since = "0.2.0", note = "use `ponko::particles::ParticleGradient`")]
pub type ParticleGradient = crate::particles::ParticleGradient;
/// Moved: [`ponko::particles::ParticleSpace`](crate::particles::ParticleSpace).
#[deprecated(since = "0.2.0", note = "use `ponko::particles::ParticleSpace`")]
pub type ParticleSpace = crate::particles::ParticleSpace;
/// Moved: [`ponko::terrain::FarTerrain`](crate::terrain::FarTerrain).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::FarTerrain`")]
pub type FarTerrain = crate::terrain::FarTerrain;
/// Moved: [`ponko::terrain::Heightfield`](crate::terrain::Heightfield).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::Heightfield`")]
pub type Heightfield = crate::terrain::Heightfield;
/// Moved: [`ponko::terrain::TerrainMaterialDesc`](crate::terrain::TerrainMaterialDesc).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::TerrainMaterialDesc`")]
pub type TerrainMaterialDesc = crate::terrain::TerrainMaterialDesc;
/// Moved: [`ponko::terrain::TerrainTile`](crate::terrain::TerrainTile).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::TerrainTile`")]
pub type TerrainTile = crate::terrain::TerrainTile;
/// Moved: [`ponko::terrain::TerrainTileData`](crate::terrain::TerrainTileData).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::TerrainTileData`")]
pub type TerrainTileData = crate::terrain::TerrainTileData;
/// Moved: [`ponko::terrain::TextureArray`](crate::terrain::TextureArray).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::TextureArray`")]
pub type TextureArray = crate::terrain::TextureArray;
/// Moved: [`ponko::terrain::TextureKind`](crate::terrain::TextureKind).
#[deprecated(since = "0.2.0", note = "use `ponko::terrain::TextureKind`")]
pub type TextureKind = crate::terrain::TextureKind;
