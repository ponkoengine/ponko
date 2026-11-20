//! The engine feeding the camera rigs: [`CameraTarget`] and the built-in
//! system [`systems::update_camera_rigs`](crate::systems::update_camera_rigs).

use ponko_ecs::{Component, Entity, With};

/// Marks the entity the camera rigs follow: the hero, the player's car.
///
/// Keep a rig as a resource ([`d2::FollowCamera`](crate::d2::FollowCamera),
/// `d3::ThirdPersonCamera`, `d3::FirstPersonCamera` or `d3::OrbitCamera`)
/// and put this on the entity it should follow. Every frame, after the
/// frame and animation systems placed everything (and before
/// [`Phase::Late`](crate::game::Phase::Late)), the engine reads the
/// entity's `Transform` and updates each rig resource with it: the 2D rig
/// moves `ctx.camera`, the 3D ones `ctx.scene3d.camera`. The game writes no
/// camera code; it only changes the rig's settings (its yaw, its bounds) when
/// it wants to.
///
/// - The first frame with a target, and whenever the target becomes
///   another entity (a respawn, a new level, another character), the
///   camera cuts to it instead of easing there.
/// - While no entity has it, the rigs are left alone: a title screen
///   can move the camera itself.
/// - With several, the first one found is followed.
/// - A rig that isn't a resource (a field of your own, a second camera)
///   is yours to update, as before.
///
/// It is saved with the world and kept by prefabs, like `Transform`.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::camera::CameraTarget;
/// use ponko::d2::FollowCamera;
///
/// let mut game = Game::new("Platformer", 640, 360).headless_frames(true);
/// let world = game.run_headless(2, |ctx| {
///     ctx.world.insert_resource(FollowCamera::default());
///     ctx.world.spawn((d2::Transform::at(vec2(500.0, 200.0)), CameraTarget));
/// });
/// # let _ = world;
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, Component)]
pub struct CameraTarget;

/// Which entity the rigs followed at the last update, so a new one is cut
/// to (the engine keeps it as a resource).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct FedTarget {
    entity2d: Option<Entity>,
    #[cfg(feature = "3d")]
    entity3d: Option<Entity>,
}

/// The system behind [`systems::update_camera_rigs`](crate::systems::update_camera_rigs).
pub(crate) fn update_camera_rigs(ctx: &mut crate::ctx::Ctx) {
    feed_rigs(ctx);
    // Held items in front of the camera where the rigs put it (and again
    // just before the frame is drawn, after later systems).
    #[cfg(feature = "3d")]
    crate::three::view_model::place_held_items(&mut ctx.world, &ctx.scene3d.camera, ctx.scene3d.view_model.as_ref());
}

/// The rigs' part of [`update_camera_rigs`].
fn feed_rigs(ctx: &mut crate::ctx::Ctx) {
    let rig2d = ctx.world.get_resource::<super::follow2d::FollowCamera2D>().is_some();
    #[cfg(feature = "3d")]
    let rig3d = ctx.world.get_resource::<super::rigs3d::ThirdPersonCamera>().is_some()
        || ctx.world.get_resource::<super::rigs3d::FirstPersonCamera>().is_some()
        || ctx.world.get_resource::<super::rigs3d::OrbitCamera>().is_some();
    #[cfg(not(feature = "3d"))]
    let rig3d = false;
    if !rig2d && !rig3d {
        return; // no rig resource: nothing to do, not even a query
    }
    if ctx.world.get_resource::<FedTarget>().is_none() {
        ctx.world.insert_resource(FedTarget::default());
    }
    let dt = ctx.time.frame_dt();
    if rig2d {
        let target = ctx
            .world
            .query::<(Entity, &crate::d2::Transform)>()
            .filter::<With<CameraTarget>>()
            .iter()
            .next()
            .map(|(e, t)| (e, t.position));
        let fed = ctx.world.resource_mut::<FedTarget>();
        let cut = target.map(|(e, _)| e) != fed.entity2d;
        fed.entity2d = target.map(|(e, _)| e);
        if let Some((_, at)) = target {
            let rig = ctx.world.resource_mut::<super::follow2d::FollowCamera2D>();
            if cut {
                rig.snap(&mut ctx.camera, at);
            } else {
                rig.update(&mut ctx.camera, at, dt);
            }
        }
    }
    #[cfg(feature = "3d")]
    if rig3d {
        feed_3d(ctx, dt);
    }
}

/// The 3D rigs' part of [`update_camera_rigs`].
#[cfg(feature = "3d")]
fn feed_3d(ctx: &mut crate::ctx::Ctx, dt: f32) {
    use super::rigs3d::{FirstPersonCamera, OrbitCamera, ThirdPersonCamera};
    let target = ctx
        .world
        .query::<(Entity, &crate::d3::Transform)>()
        .filter::<With<CameraTarget>>()
        .iter()
        .next()
        .map(|(e, t)| (e, t.position));
    let fed = ctx.world.resource_mut::<FedTarget>();
    let cut = target.map(|(e, _)| e) != fed.entity3d;
    fed.entity3d = target.map(|(e, _)| e);
    let Some((entity, at)) = target else {
        return;
    };
    let camera = &mut ctx.scene3d.camera;
    if let Some(rig) = ctx.world.get_resource_mut::<OrbitCamera>() {
        if cut {
            rig.snap();
        }
        rig.target = at;
        rig.update(camera, dt);
    }
    if let Some(rig) = ctx.world.get_resource_mut::<FirstPersonCamera>() {
        if cut {
            rig.snap();
        }
        rig.update(camera, at + glam::Vec3::Y * rig.height, dt);
    }
    if let Some(rig) = ctx.world.get_resource_mut::<ThirdPersonCamera>() {
        if cut {
            rig.snap();
        }
        // Out of the 3D physics' walls, past the target's own collider.
        #[cfg(feature = "physics3d")]
        {
            let filter = crate::d3::QueryFilter::default().exclude(entity);
            rig.update_with_physics(camera, at, &ctx.physics3d, filter, dt);
        }
        #[cfg(not(feature = "physics3d"))]
        {
            let _ = entity;
            rig.update(camera, at, dt);
        }
    }
}
