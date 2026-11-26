//! The engine's built-in systems, as plain functions.
//!
//! **Maturity: proven**, from the guide chapter `systems.md`. Game: Coin Dash and Ember Hop,
//! packaged and played on Linux, macOS and Windows. Tests: `tests/system_registration.rs`,
//! `tests/phases.rs`, `tests/schedule_barriers.rs`. Platforms: Linux, macOS and Windows, headless
//! (the workspace tests in the list of checks). Graphics: not involved.
//!
//! What lives here: every step the engine runs by itself, or adds when you
//! turn a feature on (`Game::physics`, `Game::spatial_grid`), is an ordinary
//! system `fn(&mut Ctx)`. A game doesn't normally call these: [`Game`] adds
//! them in the right [`Phase`]. They are here for the cases where you place
//! one yourself: take it out with [`Game::remove`] and its
//! [`Systems`](crate::game::Systems) id, then add the function again where you
//! want it, or add one of the opt-in systems.
//!
//! | Function | Id in [`Systems`](crate::game::Systems) | Added by | Phase |
//! |---|---|---|---|
//! | [`rebuild_spatial_grid`] | `REBUILD_SPATIAL_GRID` | `Game::spatial_grid` | `Input` |
//! | `step_physics` (feature `physics`) | `PHYSICS` | `Game::physics` | `Physics` |
//! | `step_physics3d` (feature `physics3d`) | `PHYSICS3D` | `Game::physics3d` | `Physics` |
//! | `interpolate_physics3d` (feature `physics3d`) | `INTERPOLATE_PHYSICS3D` | `Game::physics3d` | `Frame` |
//! | `update_animators` | `UPDATE_ANIMATORS` | always (`3d`) | end of `Animate` |
//! | [`update_skeletons2d`] | `UPDATE_SKELETONS2D` | always | end of `Animate` |
//! | [`update_particles`] | `UPDATE_PARTICLES` | always | end of `Animate` |
//! | [`update_camera_rigs`] | `UPDATE_CAMERA_RIGS` | always | end of `Animate` |
//! | `update_audio3d` | `UPDATE_AUDIO3D` | always (`3d`) | end of `Draw` |
//! | [`animate_sprites`] | `ANIMATE_SPRITES` | always | `Draw` |
//! | [`draw_tilemaps`] | `DRAW_TILEMAPS` | always | `Draw` |
//! | [`draw_sprites`] | `DRAW_SPRITES` | always | `Draw` |
//! | [`draw_particles`] | `DRAW_PARTICLES` | always | `Draw` |
//! | [`follow_attachments`] | `FOLLOW_ATTACHMENTS` | you | your choice |
//! | `step_navmesh3d` (feature `navmesh`) | `STEP_NAVMESH3D` | you | your choice |
//! | `apply_net_poses` (feature `3d`) | | you | your choice |
//!
//! ```
//! use ponko::{systems, prelude::{Game, Phase, Systems}};
//!
//! // Draw sprites before the tilemaps instead of after them.
//! let game = Game::new("Order", 800, 600)
//!     .remove(Systems::DRAW_TILEMAPS)
//!     .add(Phase::Draw, systems::draw_tilemaps);
//! # let _ = game;
//! ```
//!
//! [`Game`]: crate::game::Game
//! [`Game::remove`]: crate::game::Game::remove
//! [`Phase`]: crate::game::Phase

use crate::ctx::Ctx;

pub use crate::ecs::draw_sprites;
#[cfg(feature = "navmesh")]
pub use crate::nav3d::{step_navmesh3d, step_navmesh3d_access};
#[cfg(feature = "3d")]
pub use crate::net::snapshots::apply_net_poses;
#[cfg(feature = "physics")]
pub use crate::physics::{step_physics, step_physics_access};
#[cfg(feature = "physics3d")]
pub use crate::physics3d_impl::{
    interpolate_physics3d, interpolate_physics3d_access, step_physics3d, step_physics3d_access,
};

/// Advances every [`Animator`](crate::anim::Animator) that isn't in manual mode
/// by this frame's time and poses its [`Skeleton`](crate::anim::Skeleton); with
/// `physics3d`, ragdolls pose theirs first. The game runs it at the end of
/// [`Phase::Animate`](crate::game::Phase::Animate)
/// ([`Systems::UPDATE_ANIMATORS`](crate::game::Systems::UPDATE_ANIMATORS)), so late
/// systems, draw systems and the renderer see this frame's pose.
#[cfg(feature = "3d")]
pub fn update_animators(ctx: &mut Ctx) {
    ctx.update_animators();
}

/// Advances every 2D [`Skeleton`](crate::skeleton2d::Skeleton) that isn't
/// manual by this frame's time (after applying hot reloads of skeleton
/// files) and poses it. The game runs it at the end of
/// [`Phase::Animate`](crate::game::Phase::Animate)
/// ([`Systems::UPDATE_SKELETONS2D`](crate::game::Systems::UPDATE_SKELETONS2D)),
/// after the 3D animators and before the particles; with no skeleton it
/// does nothing.
pub fn update_skeletons2d(ctx: &mut Ctx) {
    ctx.update_skeletons2d();
}

/// Advances every 2D [`ParticleEmitter`](crate::d2::ParticleEmitter) and,
/// with `3d`, every 3D particle emitter and trail by this frame's time.
/// The game runs it at the end of [`Phase::Animate`](crate::game::Phase::Animate),
/// after the animators
/// ([`Systems::UPDATE_PARTICLES`](crate::game::Systems::UPDATE_PARTICLES)).
pub fn update_particles(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.update_particles2d(dt);
    #[cfg(feature = "3d")]
    ctx.update_particles();
}

/// Updates the camera rigs kept as resources
/// ([`d2::FollowCamera`](crate::d2::FollowCamera), and with `3d` the
/// third-person, first-person and orbit cameras) with the entity that has a
/// [`CameraTarget`](crate::camera::CameraTarget), and places the camera.
/// The game runs it last in [`Phase::Animate`](crate::game::Phase::Animate),
/// after the particles
/// ([`Systems::UPDATE_CAMERA_RIGS`](crate::game::Systems::UPDATE_CAMERA_RIGS)),
/// so the rigs see where everything ended up this frame and
/// [`Phase::Late`](crate::game::Phase::Late) systems see the placed camera.
/// Then, with `3d`, it puts every `d3::HeldItem` in front of the camera.
/// Without a rig resource or a held item it does nothing.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::camera::CameraTarget;
/// use ponko::d2::FollowCamera;
///
/// let mut game = Game::new("Follow", 640, 360).headless_frames(true);
/// game.run_headless(2, |ctx| {
///     ctx.world.insert_resource(FollowCamera::default().with_smooth_time(0.0));
///     ctx.world.spawn((d2::Transform::at(vec2(900.0, 40.0)), CameraTarget));
/// });
/// ```
pub fn update_camera_rigs(ctx: &mut Ctx) {
    crate::camera::feed::update_camera_rigs(ctx);
}

/// Draws every 2D [`ParticleEmitter`](crate::d2::ParticleEmitter)'s
/// particles as sprites, at the emitter's `z` with its blend mode and
/// material. [`Game`](crate::game::Game) runs it every frame after
/// [`draw_sprites`] ([`Systems::DRAW_PARTICLES`](crate::game::Systems::DRAW_PARTICLES)).
pub fn draw_particles(ctx: &mut Ctx) {
    ctx.draw_particles2d();
}

/// Moves every positional sound ([`AudioSource3D`](crate::d3::AudioSource))
/// to this frame's listener and emitters. The game runs it at the end of
/// [`Phase::Draw`](crate::game::Phase::Draw), after the drawing systems
/// ([`Systems::UPDATE_AUDIO3D`](crate::game::Systems::UPDATE_AUDIO3D)), so sound
/// follows where everything ended up this frame.
#[cfg(feature = "3d")]
pub fn update_audio3d(ctx: &mut Ctx) {
    crate::audio::update_audio3d(ctx);
}

/// Built-in system added by [`Game::spatial_grid`](crate::game::Game::spatial_grid):
/// re-indexes every [`d2::Transform`](crate::d2::Transform) entity into `ctx.spatial`. It runs first
/// in every simulation tick, and every [`SpatialGrid::SORT_EVERY`](crate::spatial::SpatialGrid::SORT_EVERY) ticks it also
/// sorts entities by cell so neighbor queries stay cache-friendly.
pub fn rebuild_spatial_grid(ctx: &mut Ctx) {
    crate::spatial::rebuild_spatial_grid(ctx);
}

/// Built-in system: advances every [`Animation`](crate::sheet::Animation) and shows its frame on the
/// entity's [`Sprite`](crate::d2::Sprite). [`Game`](crate::game::Game) runs it every frame, before
/// [`draw_sprites`].
///
/// ```no_run
/// # use ponko::prelude::*;
/// // Animations driven by your own clock instead:
/// Game::new("t", 800, 600).remove(Systems::ANIMATE_SPRITES).run(|_| {});
/// ```
pub fn animate_sprites(ctx: &mut Ctx) {
    crate::sheet::animate_sprites(ctx);
}

/// Built-in system: draws the visible tiles of every [`TileMap`](crate::tilemap::TileMap).
/// [`Game`](crate::game::Game) runs it every frame, before
/// [`draw_sprites`].
///
/// ```no_run
/// # use ponko::prelude::*;
/// Game::new("No maps here", 800, 600).remove(Systems::DRAW_TILEMAPS).run(|_| {});
/// ```
pub fn draw_tilemaps(ctx: &mut Ctx) {
    crate::tilemap::draw_tilemaps(ctx);
}

/// Moves every entity with [`Attached`](crate::attach::Attached) (and, with the `3d` feature,
/// `Attached3D`) to its place next to the entity it follows, on all cores,
/// and despawns those whose entity is gone. Add it as a simulation system
/// after the ones that move things.
pub fn follow_attachments(ctx: &mut Ctx) {
    crate::attach::follow_attachments(ctx);
}
