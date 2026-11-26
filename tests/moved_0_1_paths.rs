//! Code written for 0.1 still builds against 0.2 where an item only moved:
//! 0.1's paths stay as deprecated aliases until 0.3.0 (changelog.d/605-soften-0.1-breaks.md
//! lists them). This file is 0.1 code with the warnings allowed; the doc
//! tests on `ponko::physics3d`, `ponko::d3::ShadowMesh` and
//! `ponko::terrain::CellStreamer` check that the same imports do warn.
#![cfg(feature = "3d")]
#![allow(deprecated)]

/// Compiles only when `A` and `B` are the same type: an alias, not a copy.
fn same<A, B>()
where
    A: Is<B>,
{
}
trait Is<T> {}
impl<T> Is<T> for T {}

/// `use ponko::d3::*;` as 0.1 wrote it: the render, animation, particle and
/// terrain types it brought are still there, and are the topic modules' own.
mod d3_glob {
    use super::same;
    use ponko::d3::*;
    use ponko::prelude::*;

    #[test]
    fn the_d3_glob_still_names_the_types_that_moved_out() {
        same::<AmbientOcclusion, ponko::render::AmbientOcclusion>();
        same::<AntiAliasing, ponko::render::AntiAliasing>();
        same::<BatchCulling, ponko::render::BatchCulling>();
        same::<Blend, ponko::render::Blend>();
        same::<Compute, ponko::render::Compute>();
        same::<ComputePass, ponko::render::ComputePass>();
        same::<Effect, ponko::render::Effect>();
        same::<GlobalIllumination, ponko::render::GlobalIllumination>();
        same::<GpuBuffer, ponko::render::GpuBuffer>();
        same::<GraphicsQuality, ponko::render::GraphicsQuality>();
        same::<InstanceBatch, ponko::render::InstanceBatch>();
        same::<InstanceBatchOptions, ponko::render::InstanceBatchOptions>();
        same::<InstanceLevel, ponko::render::InstanceLevel>();
        same::<Lighting, ponko::render::Lighting>();
        same::<LodGroup, ponko::render::LodGroup>();
        same::<LodHysteresis, ponko::render::LodHysteresis>();
        same::<LodLevel, ponko::render::LodLevel>();
        same::<LodPick, ponko::render::LodPick>();
        same::<LodRenderer, ponko::render::LodRenderer>();
        same::<LodSettings, ponko::render::LodSettings>();
        same::<LodShadow, ponko::render::LodShadow>();
        same::<Material, ponko::render::Material>();
        same::<MaterialDesc, ponko::render::MaterialDesc>();
        same::<Mesh, ponko::render::Mesh>();
        same::<MeshCost, ponko::render::MeshCost>();
        same::<MeshData, ponko::render::MeshData>();
        same::<ModelMaterial, ponko::render::ModelMaterial>();
        same::<ModelMaterials, ponko::render::ModelMaterials>();
        same::<Post, ponko::render::Post>();
        same::<ReceivesDecals, ponko::render::ReceivesDecals>();
        same::<RenderPasses, ponko::render::RenderPasses>();
        same::<SceneSettings, ponko::render::SceneSettings>();
        same::<ScreenEffect, ponko::render::ScreenEffect>();
        same::<Shader, ponko::render::Shader>();
        same::<ShaderDiagnostic, ponko::render::ShaderDiagnostic>();
        same::<ShaderKind, ponko::render::ShaderKind>();
        same::<ShaderOutcome, ponko::render::ShaderOutcome>();
        same::<ShaderStatus, ponko::render::ShaderStatus>();
        same::<ShadowFilter, ponko::render::ShadowFilter>();
        same::<ShadowMesh, ponko::render::ShadowMesh>();
        same::<Sky, ponko::render::Sky>();
        same::<StreamedLod, ponko::render::StreamedLod>();
        same::<TargetCamera, ponko::render::TargetCamera>();
        same::<TargetEffect, ponko::render::TargetEffect>();
        same::<Tonemap, ponko::render::Tonemap>();
        same::<AnimationClip, ponko::anim::AnimationClip>();
        same::<AnimationCondition, ponko::anim::AnimationCondition>();
        same::<AnimationEvent, ponko::anim::AnimationEvent>();
        same::<AnimationGraph, ponko::anim::AnimationGraph>();
        same::<AnimationGraphError, ponko::anim::AnimationGraphError>();
        same::<AnimationLayer, ponko::anim::AnimationLayer>();
        same::<AnimationParameter, ponko::anim::AnimationParameter>();
        same::<AnimationState, ponko::anim::AnimationState>();
        same::<AnimationTrack, ponko::anim::AnimationTrack>();
        same::<AnimationTransition, ponko::anim::AnimationTransition>();
        same::<Animator, ponko::anim::Animator>();
        same::<AttachToJoint, ponko::anim::AttachToJoint>();
        same::<Interpolation, ponko::anim::Interpolation>();
        same::<JointAnimation, ponko::anim::JointAnimation>();
        same::<JointMask, ponko::anim::JointMask>();
        same::<LayerBlend, ponko::anim::LayerBlend>();
        same::<LayerId, ponko::anim::LayerId>();
        same::<MorphAnimation, ponko::anim::MorphAnimation>();
        same::<MorphFrame, ponko::anim::MorphFrame>();
        same::<MorphTarget, ponko::anim::MorphTarget>();
        same::<MorphWeights, ponko::anim::MorphWeights>();
        same::<Motion, ponko::anim::Motion>();
        same::<MotionChild, ponko::anim::MotionChild>();
        same::<ParamId, ponko::anim::ParamId>();
        same::<RootMotion, ponko::anim::RootMotion>();
        same::<Skeleton, ponko::anim::Skeleton>();
        same::<SkinOf, ponko::anim::SkinOf>();
        same::<StateId, ponko::anim::StateId>();
        same::<ParticleCurve, ponko::particles::ParticleCurve>();
        same::<ParticleGradient, ponko::particles::ParticleGradient>();
        same::<ParticleSpace, ponko::particles::ParticleSpace>();
        same::<FarTerrain, ponko::terrain::FarTerrain>();
        same::<Heightfield, ponko::terrain::Heightfield>();
        same::<TerrainMaterialDesc, ponko::terrain::TerrainMaterialDesc>();
        same::<TerrainTile, ponko::terrain::TerrainTile>();
        same::<TerrainTileData, ponko::terrain::TerrainTileData>();
        same::<TextureArray, ponko::terrain::TextureArray>();
        same::<TextureKind, ponko::terrain::TextureKind>();
    }

    #[test]
    fn values_build_through_the_aliases() {
        // Associated functions, enum variants and struct literals go through
        // an alias; a tuple-struct constructor needs the new path.
        let desc = MaterialDesc::color(Color::WHITE);
        let cube = MeshData::cube(1.0);
        let shadow: ShadowMesh = ponko::render::ShadowMesh(None);
        let space = ParticleSpace::World;
        let curve = ParticleCurve::new(vec![(0.0, 1.0), (1.0, 0.0)]);
        assert_eq!(space, ponko::particles::ParticleSpace::World);
        assert_eq!(curve.sample(0.0), 1.0);
        assert!(shadow.0.is_none());
        assert!(!cube.positions.is_empty());
        let _ = desc;
    }

    #[test]
    fn the_prelude_and_the_d3_glob_stay_unambiguous_together() {
        // `Model` and the collision types come from both globs as the same
        // items, so they have no alias in `d3`.
        same::<Model, ponko::render::Model>();
        #[cfg(feature = "physics3d")]
        same::<BodyKind, ponko::collision::BodyKind>();
        // The names `d3` keeps resolve as before.
        same::<Transform, ponko::d3::Transform>();
    }
}

#[test]
fn terrain_cell_streamer_is_the_streaming_one() {
    use ponko::terrain::CellStreamer;
    same::<CellStreamer, ponko::streaming::CellStreamer>();
}

/// `ponko::physics3d` as 0.1 wrote it.
#[cfg(feature = "physics3d")]
mod physics3d {
    use super::same;
    use ponko::d3;
    use ponko::physics3d::*;
    use ponko::prelude::*;

    #[test]
    fn every_0_1_name_is_its_d3_type() {
        same::<Character3D, d3::Character>();
        same::<Collider3D, d3::Collider>();
        same::<ColliderFocus, d3::ColliderFocus>();
        same::<CollisionGroups, d3::CollisionGroups>();
        same::<CollisionMesh, d3::CollisionMesh>();
        same::<CollisionMeshData, d3::CollisionMeshData>();
        same::<ContactModifier3D, d3::ContactModifier>();
        same::<FrictionCurve, d3::FrictionCurve>();
        same::<Hit3D, d3::Hit>();
        same::<Joint3D, d3::Joint>();
        same::<JointAxis3D, d3::JointAxis>();
        same::<JointDrive3D, d3::JointDrive>();
        same::<JointMotion3D, d3::JointMotion>();
        same::<Physics3D, d3::Physics>();
        same::<QueryFilter3D, d3::QueryFilter>();
        same::<Ragdoll3D, d3::Ragdoll>();
        same::<RagdollBone, d3::RagdollBone>();
        same::<RigidBody3D, d3::RigidBody>();
        same::<Shape3D, d3::Shape>();
        same::<TerrainCollider, d3::TerrainCollider>();
        same::<Vehicle3D, d3::Vehicle>();
        same::<Velocity3D, d3::Velocity>();
        same::<Wheel3D, d3::Wheel>();
        same::<ContactPair3D<'static, 'static>, d3::ContactPair<'static, 'static>>();
        assert_eq!(PHYSICS3D_HZ, d3::Physics::HZ);
        assert_eq!(step_physics3d_access(), ponko::systems::step_physics3d_access());
        assert_eq!(
            interpolate_physics3d_access(),
            ponko::systems::interpolate_physics3d_access()
        );
    }

    #[test]
    fn a_0_1_physics_scene_runs_headless() {
        let mut world = Game::new("0.1 physics", 1, 1)
            .physics3d(vec3(0.0, -9.81, 0.0))
            .run_headless(30, |ctx| {
                ctx.world.spawn((
                    d3::Transform::default(),
                    RigidBody3D::fixed(),
                    Collider3D::cuboid(vec3(20.0, 1.0, 20.0)),
                ));
                ctx.world.spawn((
                    d3::Transform::at(vec3(0.0, 5.0, 0.0)),
                    RigidBody3D::dynamic(),
                    Collider3D::sphere(0.5),
                    Velocity3D::default(),
                ));
            });
        let mut heights = Vec::new();
        world
            .query::<(&d3::Transform, &Velocity3D)>()
            .for_each(|(t, _)| heights.push(t.position.y));
        // The ground stays at 0; the ball fell from 5.
        assert!(
            heights.iter().any(|&y| y > 0.5 && y < 5.0),
            "the ball falls: {heights:?}"
        );
    }

    #[test]
    fn the_old_step_systems_are_the_built_in_ones() {
        // Added by hand under 0.1's paths, they are recognized as the
        // built-in systems, and listed under those paths as before.
        let game = Game::new("t", 1, 1)
            .add(Phase::Physics, step_physics3d)
            .add(Phase::Frame, interpolate_physics3d);
        assert_eq!(game.system_count(Systems::PHYSICS3D), 1);
        assert_eq!(game.system_count(Systems::INTERPOLATE_PHYSICS3D), 1);
        let game = Game::new("t", 1, 1).physics3d(Vec3::ZERO);
        let names: Vec<_> = game.access_report().systems.iter().map(|s| s.name).collect();
        assert!(names.contains(&"ponko::physics3d::step_physics3d"), "{names:?}");
        assert!(names.contains(&"ponko::physics3d::interpolate_physics3d"), "{names:?}");
    }
}
