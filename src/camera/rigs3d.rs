//! 3D camera rigs, public as [`d3::OrbitCamera`](crate::d3::OrbitCamera),
//! [`d3::FirstPersonCamera`](crate::d3::FirstPersonCamera) and
//! [`d3::ThirdPersonCamera`](crate::d3::ThirdPersonCamera).

use glam::{EulerRot, Quat, Vec2, Vec3};

use super::damp::damp_moving;
use crate::three::Camera3D;

/// The default lowest and highest pitch: a little short of straight down
/// and up, where yaw would lose its meaning.
const PITCH_LIMIT: f32 = 1.5;

/// The camera's rotation for `yaw` (about +Y; 0 looks along -Z, positive
/// turns left) and `pitch` (positive looks up).
fn look_rotation(yaw: f32, pitch: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)
}

/// Level forward and right for `yaw`, for moving relative to the view.
fn flat_axes(yaw: f32) -> (Vec3, Vec3) {
    let r = Quat::from_rotation_y(yaw);
    (r * Vec3::NEG_Z, r * Vec3::X)
}

/// `lo..=hi`, tolerating them swapped or NaN.
fn clamp_range(v: f32, lo: f32, hi: f32) -> f32 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if v.is_nan() {
        lo.max(0.0).min(hi)
    } else {
        v.clamp(lo, hi)
    }
}

/// A camera circling a point: drag to turn around it, scroll to come
/// closer, pan to move the point. For model viewers, strategy and
/// building games, editors and title screens.
///
/// [`yaw`](Self::yaw), [`pitch`](Self::pitch), [`distance`](Self::distance)
/// and [`target`](Self::target) are where it should be; the camera eases
/// there with [`smoothing`](Self::smoothing). Change them directly, or with
/// [`rotate`](Self::rotate), [`zoom`](Self::zoom), [`pan`](Self::pan) or
/// [`mouse_controls`](Self::mouse_controls), then call
/// [`update`](Self::update) once a frame.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::OrbitCamera;
///
/// Game::new("Viewer", 800, 600).run_headless(1, |ctx| {
///     let mut orbit = OrbitCamera::new(Vec3::ZERO, 10.0);
///     orbit.update(&mut ctx.scene3d.camera, ctx.time.frame_dt());
///     let cam = ctx.scene3d.camera;
///     assert!((cam.position.length() - 10.0).abs() < 1e-4);
///     assert!(cam.forward().dot(-cam.position.normalize()) > 0.9999); // looks at the target
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct OrbitCamera {
    /// The point it circles and looks at.
    pub target: Vec3,
    /// Turn about +Y, in radians: 0 looks along -Z (from the target's +Z
    /// side); positive circles toward +X. Default 0.
    pub yaw: f32,
    /// Up and down, in radians: negative looks down on the target.
    /// Default -0.5 (about 29° down).
    pub pitch: f32,
    /// How far from the target, in world units. Default 10.
    pub distance: f32,
    /// The lowest pitch. Default -1.5 (about straight down).
    pub min_pitch: f32,
    /// The highest pitch. Default 1.5.
    pub max_pitch: f32,
    /// The closest it comes. Default 1.
    pub min_distance: f32,
    /// The farthest it goes. Default 500.
    pub max_distance: f32,
    /// Radians per pixel dragged, for [`mouse_controls`](Self::mouse_controls).
    /// Default 0.005.
    pub sensitivity: f32,
    /// Half-life of the easing, in seconds (see
    /// [`damp`](crate::camera::damp)). 0 moves at once. Default 0.05.
    pub smoothing: f32,
    /// State: the target the camera is at now.
    pub current_target: Vec3,
    /// State: the yaw, pitch and distance the camera is at now.
    pub current: Vec3,
    /// State: the target at the last update.
    pub last_target: Vec3,
    /// State: the yaw, pitch and distance wanted at the last update.
    pub last_goal: Vec3,
    /// State: whether the camera has been placed yet.
    pub started: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: -0.5,
            distance: 10.0,
            min_pitch: -PITCH_LIMIT,
            max_pitch: PITCH_LIMIT,
            min_distance: 1.0,
            max_distance: 500.0,
            sensitivity: 0.005,
            smoothing: 0.05,
            current_target: Vec3::ZERO,
            current: Vec3::ZERO,
            last_target: Vec3::ZERO,
            last_goal: Vec3::ZERO,
            started: false,
        }
    }
}

impl OrbitCamera {
    /// Circling `target` at `distance`, looking down a little.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let orbit = ponko::d3::OrbitCamera::new(vec3(0.0, 1.0, 0.0), 6.0);
    /// # let _ = orbit;
    /// ```
    #[must_use]
    pub fn new(target: Vec3, distance: f32) -> Self {
        Self {
            target,
            distance,
            ..Self::default()
        }
    }

    /// Turns around the target by `yaw` and `pitch` radians (pitch stays
    /// within its limits).
    ///
    /// ```
    /// let mut orbit = ponko::d3::OrbitCamera::default();
    /// orbit.rotate(0.1, 10.0);
    /// assert_eq!(orbit.pitch, orbit.max_pitch);
    /// ```
    pub fn rotate(&mut self, yaw: f32, pitch: f32) {
        self.yaw += yaw;
        self.pitch = clamp_range(self.pitch + pitch, self.min_pitch, self.max_pitch);
    }

    /// Multiplies the distance by `factor` (below 1 comes closer), within
    /// its limits.
    ///
    /// ```
    /// let mut orbit = ponko::d3::OrbitCamera::new(ponko::prelude::Vec3::ZERO, 10.0);
    /// orbit.zoom(0.5);
    /// assert_eq!(orbit.distance, 5.0);
    /// ```
    pub fn zoom(&mut self, factor: f32) {
        if factor > 0.0 && factor.is_finite() {
            self.distance = clamp_range(self.distance * factor, self.min_distance, self.max_distance);
        }
    }

    /// Moves the target by `right` and `up` world units along the view's
    /// right and up axes.
    ///
    /// ```
    /// let mut orbit = ponko::d3::OrbitCamera::default();
    /// orbit.pan(1.0, 0.0);
    /// assert!((orbit.target.x - 1.0).abs() < 1e-6); // yaw 0: right is +X
    /// ```
    pub fn pan(&mut self, right: f32, up: f32) {
        let r = look_rotation(self.yaw, self.pitch);
        self.target += r * Vec3::X * right + r * Vec3::Y * up;
    }

    /// The usual mouse controls: drag with the left button to turn, with
    /// the right or middle button to pan, scroll to zoom. Call it before
    /// [`update`](Self::update).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, orbit: &mut ponko::d3::OrbitCamera) {
    /// if !ctx.ui.wants_pointer() {
    ///     orbit.mouse_controls(&ctx.input);
    /// }
    /// orbit.update(&mut ctx.scene3d.camera, ctx.time.frame_dt());
    /// # }
    /// ```
    pub fn mouse_controls(&mut self, input: &crate::input::Input) {
        use crate::input::MouseButton;
        let delta = input.mouse_delta();
        if input.mouse_down(MouseButton::Left) {
            self.rotate(-delta.x * self.sensitivity, -delta.y * self.sensitivity);
        }
        if input.mouse_down(MouseButton::Right) || input.mouse_down(MouseButton::Middle) {
            // One pixel moves the target about one pixel's width at its distance.
            let per_pixel = self.distance * 0.0015;
            self.pan(-delta.x * per_pixel, delta.y * per_pixel);
        }
        let wheel = input.mouse_wheel().y;
        if wheel != 0.0 {
            self.zoom(0.9f32.powf(wheel));
        }
    }

    /// Eases toward the goal by `dt` seconds and places `camera`. The first
    /// call (or one after [`snap`](Self::snap)) places it at once.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, orbit: &mut ponko::d3::OrbitCamera) {
    /// orbit.update(&mut ctx.scene3d.camera, ctx.time.frame_dt());
    /// # }
    /// ```
    pub fn update(&mut self, camera: &mut Camera3D, dt: f32) {
        self.pitch = clamp_range(self.pitch, self.min_pitch, self.max_pitch);
        self.distance = clamp_range(self.distance, self.min_distance, self.max_distance);
        let goal = Vec3::new(self.yaw, self.pitch, self.distance);
        if !self.started || !self.current.is_finite() || !self.current_target.is_finite() {
            self.started = true;
            self.current = goal;
            self.current_target = self.target;
        } else {
            // The goals moved in a straight line during the step (input
            // comes in over the frame): smooth toward that, exactly.
            self.current = damp_moving(self.current, self.last_goal, goal, self.smoothing, dt);
            self.current_target = damp_moving(self.current_target, self.last_target, self.target, self.smoothing, dt);
        }
        self.last_goal = goal;
        self.last_target = self.target;
        let rotation = look_rotation(self.current.x, self.current.y);
        camera.rotation = rotation;
        camera.position = self.current_target + rotation * Vec3::new(0.0, 0.0, self.current.z);
    }

    /// Jumps to the goal on the next [`update`](Self::update), without
    /// easing.
    ///
    /// ```
    /// let mut orbit = ponko::d3::OrbitCamera::default();
    /// orbit.snap();
    /// # let _ = orbit;
    /// ```
    pub fn snap(&mut self) {
        self.started = false;
    }
}

/// A first-person camera: mouse look with limits on looking up and down,
/// at an eye position you give it every frame (the player's head).
///
/// Turn it with [`look`](Self::look) (mouse motion), place it with
/// [`update`](Self::update), and move the player along
/// [`flat_forward`](Self::flat_forward) and
/// [`flat_right`](Self::flat_right).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::FirstPersonCamera;
///
/// Game::new("Walk", 800, 600).run_headless(1, |ctx| {
///     let mut eyes = FirstPersonCamera::default();
///     eyes.look(vec2(0.0, -100.0)); // the mouse moved up: look up
///     eyes.update(&mut ctx.scene3d.camera, vec3(0.0, 1.7, 0.0), ctx.time.frame_dt());
///     assert!(ctx.scene3d.camera.forward().y > 0.0);
///     assert_eq!(ctx.scene3d.camera.position, vec3(0.0, 1.7, 0.0));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FirstPersonCamera {
    /// Turn about +Y, in radians: 0 looks along -Z, positive turns left.
    pub yaw: f32,
    /// Up and down, in radians: positive looks up. Default 0.
    pub pitch: f32,
    /// The lowest pitch. Default -1.5 (about straight down).
    pub min_pitch: f32,
    /// The highest pitch. Default 1.5.
    pub max_pitch: f32,
    /// Radians per unit of mouse motion, for [`look`](Self::look).
    /// Default 0.0025.
    pub sensitivity: f32,
    /// Half-life of the turning, in seconds (see
    /// [`damp`](crate::camera::damp)). Default 0: the view turns exactly
    /// with the mouse, as most players expect.
    pub smoothing: f32,
    /// State: the yaw and pitch the camera is at now.
    pub current: Vec2,
    /// State: the yaw and pitch wanted at the last update.
    pub last_goal: Vec2,
    /// State: whether the camera has been placed yet.
    pub started: bool,
}

impl Default for FirstPersonCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            min_pitch: -PITCH_LIMIT,
            max_pitch: PITCH_LIMIT,
            sensitivity: 0.0025,
            smoothing: 0.0,
            current: Vec2::ZERO,
            last_goal: Vec2::ZERO,
            started: false,
        }
    }
}

impl FirstPersonCamera {
    /// Turns by mouse motion `delta` (as
    /// [`Input::mouse_motion`](crate::input::Input::mouse_motion): right
    /// and down positive) times the [`sensitivity`](Self::sensitivity).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, eyes: &mut ponko::d3::FirstPersonCamera) {
    /// eyes.look(ctx.input.mouse_motion());
    /// # }
    /// ```
    pub fn look(&mut self, delta: Vec2) {
        if delta.is_finite() {
            self.yaw -= delta.x * self.sensitivity;
            self.pitch = clamp_range(self.pitch - delta.y * self.sensitivity, self.min_pitch, self.max_pitch);
        }
    }

    /// Turns straight to the view on the next update, without
    /// [`smoothing`](Self::smoothing).
    ///
    /// ```
    /// let mut eyes = ponko::d3::FirstPersonCamera::default();
    /// eyes.snap();
    /// # let _ = eyes;
    /// ```
    pub fn snap(&mut self) {
        self.started = false;
    }

    /// Level forward (the way the view faces, along the ground), for
    /// walking.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let eyes = ponko::d3::FirstPersonCamera::default();
    /// assert!(eyes.flat_forward().abs_diff_eq(Vec3::NEG_Z, 1e-6));
    /// ```
    #[must_use]
    pub fn flat_forward(&self) -> Vec3 {
        flat_axes(self.yaw).0
    }

    /// Level right, for strafing.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let eyes = ponko::d3::FirstPersonCamera::default();
    /// assert!(eyes.flat_right().abs_diff_eq(Vec3::X, 1e-6));
    /// ```
    #[must_use]
    pub fn flat_right(&self) -> Vec3 {
        flat_axes(self.yaw).1
    }

    /// Places `camera` at `eye`, turned toward the current view, easing
    /// the turn by `dt` seconds of [`smoothing`](Self::smoothing).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, eyes: &mut ponko::d3::FirstPersonCamera, head: Vec3) {
    /// eyes.update(&mut ctx.scene3d.camera, head, ctx.time.frame_dt());
    /// # }
    /// ```
    pub fn update(&mut self, camera: &mut Camera3D, eye: Vec3, dt: f32) {
        self.pitch = clamp_range(self.pitch, self.min_pitch, self.max_pitch);
        let goal = Vec2::new(self.yaw, self.pitch);
        if !self.started || !self.current.is_finite() {
            self.started = true;
            self.current = goal;
        } else {
            self.current = damp_moving(self.current, self.last_goal, goal, self.smoothing, dt);
        }
        self.last_goal = goal;
        camera.position = eye;
        camera.rotation = look_rotation(self.current.x, self.current.y);
    }
}

/// A third-person camera behind a character: it orbits a pivot above the
/// character's position (turned by [`look`](Self::look)), follows it
/// smoothly, and comes in closer when a wall is between the camera and the
/// character, so the character stays in view.
///
/// Every frame, after the character moved, call
/// [`update`](Self::update) (no collision),
/// [`update_with`](Self::update_with) (your own obstacle test), or
/// `update_with_physics` (feature `physics3d`: a sphere cast against
/// the 3D physics colliders).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::ThirdPersonCamera;
///
/// Game::new("Adventure", 800, 600).run_headless(1, |ctx| {
///     let mut rig = ThirdPersonCamera::default();
///     let hero = vec3(0.0, 0.0, 0.0);
///     // A wall 2 units behind the pivot: the camera comes in to it.
///     rig.update_with(&mut ctx.scene3d.camera, hero, ctx.time.frame_dt(), |_from, _dir, max| (max > 2.0).then_some(2.0));
///     assert!((ctx.scene3d.camera.position.distance(rig.pivot()) - 2.0).abs() < 1e-4);
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ThirdPersonCamera {
    /// Turn about +Y, in radians: 0 looks along -Z (the camera behind a
    /// character facing -Z), positive turns left.
    pub yaw: f32,
    /// Up and down, in radians: negative looks down on the character.
    /// Default -0.3.
    pub pitch: f32,
    /// How far behind the pivot the camera wants to be. Default 5.
    pub distance: f32,
    /// How high above the target's position the pivot is (the head, or
    /// the shoulders). Default 1.5.
    pub height: f32,
    /// How far right of the pivot the camera sits (over the shoulder;
    /// negative is the left shoulder). Default 0.
    pub shoulder: f32,
    /// The lowest pitch. Default -1.2.
    pub min_pitch: f32,
    /// The highest pitch. Default 0.8.
    pub max_pitch: f32,
    /// Radians per unit of mouse motion, for [`look`](Self::look).
    /// Default 0.0025.
    pub sensitivity: f32,
    /// Half-life of following the target, in seconds (see
    /// [`damp`](crate::camera::damp)). 0 sticks to it. Default 0.05.
    pub follow_smoothing: f32,
    /// The camera's size against walls, in world units: it stays this far
    /// from them. Default 0.2.
    pub collision_radius: f32,
    /// Half-life of moving back out once a wall is gone, in seconds (it
    /// comes in at once). Default 0.2.
    pub recover_smoothing: f32,
    /// State: the pivot the camera turns around now.
    pub current_pivot: Vec3,
    /// State: how far from the pivot the camera is now (less than
    /// `distance` near walls).
    pub current_distance: f32,
    /// State: the pivot wanted at the last update.
    pub last_pivot: Vec3,
    /// State: whether the camera has been placed yet.
    pub started: bool,
}

impl Default for ThirdPersonCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -0.3,
            distance: 5.0,
            height: 1.5,
            shoulder: 0.0,
            min_pitch: -1.2,
            max_pitch: 0.8,
            sensitivity: 0.0025,
            follow_smoothing: 0.05,
            collision_radius: 0.2,
            recover_smoothing: 0.2,
            current_pivot: Vec3::ZERO,
            current_distance: 0.0,
            last_pivot: Vec3::ZERO,
            started: false,
        }
    }
}

impl ThirdPersonCamera {
    /// Turns by mouse motion `delta` (right and down positive) times the
    /// [`sensitivity`](Self::sensitivity).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, rig: &mut ponko::d3::ThirdPersonCamera) {
    /// rig.look(ctx.input.mouse_motion());
    /// # }
    /// ```
    pub fn look(&mut self, delta: Vec2) {
        if delta.is_finite() {
            self.yaw -= delta.x * self.sensitivity;
            self.pitch = clamp_range(self.pitch - delta.y * self.sensitivity, self.min_pitch, self.max_pitch);
        }
    }

    /// Jumps behind the target on the next update, without following
    /// smoothly (a respawn, a cut).
    ///
    /// ```
    /// let mut rig = ponko::d3::ThirdPersonCamera::default();
    /// rig.snap();
    /// # let _ = rig;
    /// ```
    pub fn snap(&mut self) {
        self.started = false;
    }

    /// Level forward (the way the camera faces, along the ground): move
    /// the character this way for "forward".
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let rig = ponko::d3::ThirdPersonCamera::default();
    /// assert!(rig.flat_forward().abs_diff_eq(Vec3::NEG_Z, 1e-6));
    /// ```
    #[must_use]
    pub fn flat_forward(&self) -> Vec3 {
        flat_axes(self.yaw).0
    }

    /// Level right, for strafing.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let rig = ponko::d3::ThirdPersonCamera::default();
    /// assert!(rig.flat_right().abs_diff_eq(Vec3::X, 1e-6));
    /// ```
    #[must_use]
    pub fn flat_right(&self) -> Vec3 {
        flat_axes(self.yaw).1
    }

    /// The point the camera turns around and looks past now: the followed
    /// pivot, moved to the shoulder.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let rig = ponko::d3::ThirdPersonCamera::default();
    /// let _ = rig.pivot();
    /// ```
    #[must_use]
    pub fn pivot(&self) -> Vec3 {
        self.current_pivot + look_rotation(self.yaw, self.pitch) * Vec3::X * self.shoulder
    }

    /// Follows `target` (the character's position) by `dt` seconds and
    /// places `camera`, without checking for walls.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, rig: &mut ponko::d3::ThirdPersonCamera, hero: Vec3) {
    /// rig.update(&mut ctx.scene3d.camera, hero, ctx.time.frame_dt());
    /// # }
    /// ```
    pub fn update(&mut self, camera: &mut Camera3D, target: Vec3, dt: f32) {
        self.update_with(camera, target, dt, |_, _, _| None);
    }

    /// [`update`](Self::update), asking `obstacle(from, direction,
    /// max_distance)` how far the camera can go from the pivot along
    /// `direction` (a unit vector) before it touches something: `None` if
    /// nothing is within `max_distance`. A sphere cast of
    /// [`collision_radius`](Self::collision_radius) answers it exactly; a
    /// ray cast's distance minus the radius is close.
    ///
    /// The camera comes in to a wall at once, and moves back out with
    /// [`recover_smoothing`](Self::recover_smoothing).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, rig: &mut ponko::d3::ThirdPersonCamera, hero: Vec3) {
    /// // A ceiling-free room 8 units deep: no farther than its back wall at z = 4.
    /// rig.update_with(&mut ctx.scene3d.camera, hero, ctx.time.frame_dt(), |from, dir, max| {
    ///     (dir.z > 0.0).then(|| (4.0 - from.z) / dir.z).filter(|&d| d < max)
    /// });
    /// # }
    /// ```
    pub fn update_with(
        &mut self,
        camera: &mut Camera3D,
        target: Vec3,
        dt: f32,
        mut obstacle: impl FnMut(Vec3, Vec3, f32) -> Option<f32>,
    ) {
        self.pitch = clamp_range(self.pitch, self.min_pitch, self.max_pitch);
        let want_pivot = target + Vec3::Y * self.height;
        let first = !self.started || !self.current_pivot.is_finite() || !self.current_distance.is_finite();
        if first {
            self.current_pivot = want_pivot;
        } else {
            self.current_pivot = damp_moving(
                self.current_pivot,
                self.last_pivot,
                want_pivot,
                self.follow_smoothing,
                dt,
            );
        }
        self.last_pivot = want_pivot;
        let rotation = look_rotation(self.yaw, self.pitch);
        let pivot = self.pivot();
        let back = rotation * Vec3::Z;
        let want = self.distance.max(0.0);
        let free = obstacle(pivot, back, want).map_or(want, |d| d.clamp(0.0, want));
        if first || free < self.current_distance {
            self.current_distance = free;
        } else {
            self.current_distance = super::damp::damp(self.current_distance, free, self.recover_smoothing, dt);
        }
        self.started = true;
        camera.rotation = rotation;
        camera.position = pivot + back * self.current_distance;
    }

    /// [`update`](Self::update), keeping the camera out of the 3D physics
    /// colliders: a sphere of [`collision_radius`](Self::collision_radius)
    /// cast from the pivot. Give the character's entity in `filter` (with
    /// [`QueryFilter::exclude`](crate::d3::QueryFilter::exclude)) so its
    /// own collider doesn't count.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::d3::{Collider, QueryFilter, RigidBody, ThirdPersonCamera, Transform};
    ///
    /// fn camera(ctx: &mut Ctx) {
    ///     let mut rig = ThirdPersonCamera::default();
    ///     rig.pitch = 0.0;
    ///     let hero = Vec3::ZERO;
    ///     rig.update_with_physics(&mut ctx.scene3d.camera, hero, &ctx.physics3d, QueryFilter::default(), 0.0);
    ///     // Short of the wall, not 5 units back inside it.
    ///     assert!(ctx.scene3d.camera.position.z <= 3.0 - rig.collision_radius + 1e-3);
    /// }
    /// Game::new("Cave", 800, 600).physics3d(Vec3::ZERO).sim_system(camera).run_headless(2, |ctx| {
    ///     // A wall whose near face is 3 units behind the hero.
    ///     let at = Transform::at(vec3(0.0, 0.0, 3.5));
    ///     ctx.world.spawn((at, RigidBody::fixed(), Collider::cuboid(vec3(20.0, 20.0, 1.0))));
    /// });
    /// ```
    #[cfg(feature = "physics3d")]
    pub fn update_with_physics(
        &mut self,
        camera: &mut Camera3D,
        target: Vec3,
        physics: &crate::d3::Physics,
        filter: crate::d3::QueryFilter,
        dt: f32,
    ) {
        let radius = self.collision_radius.max(0.0);
        self.update_with(camera, target, dt, |from, dir, max| {
            if radius > 0.0 {
                physics.cast_sphere(from, radius, dir, max, filter).map(|h| h.distance)
            } else {
                physics.cast_ray(from, dir, max, filter).map(|h| h.distance)
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn cam() -> Camera3D {
        Camera3D::new(Vec2::new(800.0, 600.0))
    }

    fn steps(hz: f32, seconds: f32) -> usize {
        (seconds * hz).round() as usize
    }

    #[test]
    fn orbit_looks_at_its_target_from_its_distance() {
        let mut c = cam();
        let mut o = OrbitCamera::new(Vec3::new(1.0, 2.0, 3.0), 7.0);
        o.yaw = 0.8;
        o.update(&mut c, 1.0 / 60.0);
        assert!((c.position.distance(o.target) - 7.0).abs() < 1e-4);
        assert!(c.forward().dot((o.target - c.position).normalize()) > 0.99999);
        assert!(c.position.y > o.target.y, "pitch -0.5 is above the target");
    }

    #[test]
    fn orbit_keeps_its_limits() {
        let mut o = OrbitCamera::default();
        o.zoom(1e6);
        assert_eq!(o.distance, o.max_distance);
        o.zoom(1e-9);
        assert_eq!(o.distance, o.min_distance);
        o.rotate(0.0, -10.0);
        assert_eq!(o.pitch, o.min_pitch);
        // Limits apply to values set directly too, on update.
        o.pitch = 5.0;
        o.distance = -3.0;
        o.update(&mut cam(), 0.016);
        assert_eq!((o.pitch, o.distance), (o.max_pitch, o.min_distance));
    }

    #[test]
    fn orbit_easing_is_frame_rate_independent() {
        let mut ends = Vec::new();
        for hz in [30.0, 60.0, 144.0] {
            let mut c = cam();
            let mut o = OrbitCamera::new(Vec3::ZERO, 10.0);
            o.smoothing = 0.2;
            o.update(&mut c, 0.0);
            // The same turn, approach and move, spread over the frames of
            // 0.5 s; looked at mid-way.
            let n = steps(hz, 0.5);
            for _ in 0..steps(hz, 1.0 / 3.0) {
                o.rotate(1.0 / n as f32, 0.3 / n as f32);
                o.distance -= 5.0 / n as f32;
                o.target += Vec3::new(2.0, 0.0, -1.0) / n as f32;
                o.update(&mut c, 1.0 / hz);
            }
            ends.push((c.position, c.rotation));
        }
        for (p, r) in &ends[1..] {
            assert!(p.abs_diff_eq(ends[0].0, 1e-3), "{p} vs {}", ends[0].0);
            assert!(r.abs_diff_eq(ends[0].1, 1e-4));
        }
    }

    #[test]
    fn first_person_looks_where_the_mouse_turns_it_and_stops_at_the_limits() {
        let mut c = cam();
        let mut f = FirstPersonCamera::default();
        f.look(Vec2::new(100.0, 0.0)); // right
        f.update(&mut c, Vec3::new(0.0, 1.7, 0.0), 0.016);
        assert!(c.forward().x > 0.2, "{}", c.forward());
        assert!(f.flat_right().z > 0.2, "right turned with the view");
        f.look(Vec2::new(0.0, -1e6)); // far up
        assert_eq!(f.pitch, f.max_pitch);
        f.update(&mut c, Vec3::ZERO, 0.016);
        assert!(c.forward().y < 1.0 && c.forward().y > 0.99);
        assert_eq!(f.flat_forward().y, 0.0);
    }

    #[test]
    fn first_person_turning_is_frame_rate_independent() {
        let mut ends = Vec::new();
        for hz in [30.0, 60.0, 144.0] {
            let mut c = cam();
            let mut f = FirstPersonCamera {
                smoothing: 0.05,
                ..FirstPersonCamera::default()
            };
            f.update(&mut c, Vec3::ZERO, 0.0);
            // The same mouse motion, spread over the frames of 0.5 s;
            // looked at mid-turn.
            let n = steps(hz, 0.5);
            for _ in 0..steps(hz, 1.0 / 3.0) {
                f.look(Vec2::new(400.0, 100.0) / n as f32);
                f.update(&mut c, Vec3::ZERO, 1.0 / hz);
            }
            ends.push(c.rotation);
        }
        for r in &ends[1..] {
            assert!(r.abs_diff_eq(ends[0], 1e-5), "{r} vs {}", ends[0]);
        }
    }

    #[test]
    fn third_person_stays_behind_and_above_the_target() {
        let mut c = cam();
        let mut t = ThirdPersonCamera::default();
        t.update(&mut c, Vec3::new(5.0, 0.0, 0.0), 0.016);
        assert!((c.position.distance(Vec3::new(5.0, 1.5, 0.0)) - 5.0).abs() < 1e-4);
        assert!(
            c.position.z > 0.0 && c.position.y > 1.5,
            "behind (+Z) and above: {}",
            c.position
        );
        assert!(c.forward().dot((Vec3::new(5.0, 1.5, 0.0) - c.position).normalize()) > 0.99999);
    }

    #[test]
    fn third_person_comes_in_at_once_at_a_wall_and_eases_back_out() {
        let mut c = cam();
        let mut t = ThirdPersonCamera::default();
        t.update(&mut c, Vec3::ZERO, 0.016);
        let wall = |_: Vec3, _: Vec3, max: f32| (max > 1.5).then_some(1.5);
        t.update_with(&mut c, Vec3::ZERO, 0.016, wall);
        assert!((t.current_distance - 1.5).abs() < 1e-6, "pulled in at once");
        assert!((c.position.distance(t.pivot()) - 1.5).abs() < 1e-4);
        t.update(&mut c, Vec3::ZERO, t.recover_smoothing);
        assert!(
            (t.current_distance - 3.25).abs() < 1e-3,
            "half way back out after one half-life: {}",
            t.current_distance
        );
    }

    #[test]
    fn third_person_follow_is_frame_rate_independent() {
        let mut ends = Vec::new();
        for hz in [30.0, 60.0, 144.0] {
            let mut c = cam();
            let mut t = ThirdPersonCamera {
                follow_smoothing: 0.15,
                shoulder: 0.5,
                ..ThirdPersonCamera::default()
            };
            t.update(&mut c, Vec3::ZERO, 0.0);
            // A character running at a steady 6 units/s, seen mid-run.
            let speed = Vec3::new(4.0, 0.0, -4.47);
            for i in 1..=steps(hz, 0.5) {
                t.update(&mut c, speed * (i as f32 / hz), 1.0 / hz);
            }
            ends.push(c.position);
        }
        for p in &ends[1..] {
            assert!(p.abs_diff_eq(ends[0], 1e-4), "{p} vs {}", ends[0]);
        }
    }
}
