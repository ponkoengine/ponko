//! The 2D follow camera and pixel-perfect 2D, public as
//! [`d2::FollowCamera`](crate::d2::FollowCamera) and
//! [`d2::PixelPerfect`](crate::d2::PixelPerfect).

use glam::Vec2;

use super::damp::smooth_damp_moving;
use crate::camera::Rect;
use crate::camera2d::Camera2D;

/// A 2D camera that follows a target: with a dead zone the target moves
/// in freely, a look-ahead that shows more of where it is heading, and
/// bounds the camera never shows past (the level's edges).
///
/// Keep one as a resource and put a
/// [`CameraTarget`](crate::camera::CameraTarget) on the entity it should
/// follow: the engine updates it every frame, after everything moved. Or
/// call [`update`](Self::update) yourself every frame with the target's
/// position. The first update (and [`snap`](Self::snap), for a respawn or
/// a new level) puts the camera on the target at once; later ones ease it
/// there.
///
/// The settings are plain fields; the rest is the rig's state, which
/// `update` keeps (read it, don't write it).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d2::FollowCamera;
///
/// Game::new("Platformer", 640, 360).run_headless(60, |ctx| {
///     let mut follow = FollowCamera::default()
///         .with_dead_zone(vec2(32.0, 48.0))
///         .with_look_ahead(vec2(48.0, 0.0))
///         .with_bounds(Rect::new(0.0, 0.0, 4000.0, 360.0));
///     let hero = vec2(100.0, 300.0);
///     follow.update(&mut ctx.camera, hero, ctx.time.frame_dt());
///     // Near the level's left edge, the camera stops at it.
///     assert_eq!(ctx.camera.visible_rect().min.x, 0.0);
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FollowCamera2D {
    /// The box in the middle of the view the target moves in without
    /// moving the camera, in world units (width and height). Default zero:
    /// the camera always follows.
    pub dead_zone: Vec2,
    /// How far ahead of the target the camera looks, in world units, on
    /// each axis: it shifts by this toward the way the target last moved.
    /// Default zero.
    pub look_ahead: Vec2,
    /// How fast the target must move on an axis (world units per second)
    /// to turn the look-ahead that way. Below it, the look-ahead holds, so
    /// standing still doesn't swing the view back. Default 20.
    pub look_ahead_speed: f32,
    /// Always added to where the camera looks (a little above the hero:
    /// `vec2(0.0, -16.0)`). Default zero.
    pub offset: Vec2,
    /// Roughly how long the camera takes to catch up, in seconds (see
    /// [`smooth_damp`](crate::camera::smooth_damp)). 0 sticks to the
    /// target. Default 0.15.
    pub smooth_time: f32,
    /// The part of the world the camera may show, if any: it stops at the
    /// edges, and centers on bounds smaller than the view. Default none.
    pub bounds: Option<Rect>,
    /// Pixel art: [`snap`](Self::snap) and [`update`](Self::update) apply
    /// it to the camera before placing it (the bounds depend on its zoom)
    /// and after (so the camera rests on the art's pixel grid). Default
    /// none.
    pub pixel_perfect: Option<PixelPerfect2D>,
    /// State: the world point the camera centers on.
    pub center: Vec2,
    /// State: how fast `center` moves (for the smoothing).
    pub velocity: Vec2,
    /// State: the point the dead zone is centered on.
    pub focus: Vec2,
    /// State: the look-ahead direction, -1, 0 or 1 on each axis. `update`
    /// turns it the way the target moves; a game that knows which way its
    /// hero faces may set it before `update`.
    pub facing: Vec2,
    /// State: the point the camera was heading for at the last update
    /// (the target with the dead zone, look-ahead, offset and bounds).
    pub goal: Vec2,
    /// State: the target's position at the last update.
    pub last_target: Vec2,
    /// State: whether the camera has been put on a target yet.
    pub started: bool,
}

impl Default for FollowCamera2D {
    fn default() -> Self {
        Self {
            dead_zone: Vec2::ZERO,
            look_ahead: Vec2::ZERO,
            look_ahead_speed: 20.0,
            offset: Vec2::ZERO,
            smooth_time: 0.15,
            bounds: None,
            pixel_perfect: None,
            center: Vec2::ZERO,
            velocity: Vec2::ZERO,
            focus: Vec2::ZERO,
            facing: Vec2::ZERO,
            goal: Vec2::ZERO,
            last_target: Vec2::ZERO,
            started: false,
        }
    }
}

impl FollowCamera2D {
    /// The same, with this [`dead_zone`](Self::dead_zone).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let follow = ponko::d2::FollowCamera::default().with_dead_zone(vec2(40.0, 60.0));
    /// # let _ = follow;
    /// ```
    #[must_use]
    pub fn with_dead_zone(self, size: Vec2) -> Self {
        Self {
            dead_zone: size.max(Vec2::ZERO),
            ..self
        }
    }

    /// The same, with this [`look_ahead`](Self::look_ahead).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let follow = ponko::d2::FollowCamera::default().with_look_ahead(vec2(64.0, 0.0));
    /// # let _ = follow;
    /// ```
    #[must_use]
    pub fn with_look_ahead(self, distance: Vec2) -> Self {
        Self {
            look_ahead: distance,
            ..self
        }
    }

    /// The same, with this [`offset`](Self::offset).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let follow = ponko::d2::FollowCamera::default().with_offset(vec2(0.0, -16.0));
    /// # let _ = follow;
    /// ```
    #[must_use]
    pub fn with_offset(self, offset: Vec2) -> Self {
        Self { offset, ..self }
    }

    /// The same, with this [`smooth_time`](Self::smooth_time).
    ///
    /// ```
    /// let follow = ponko::d2::FollowCamera::default().with_smooth_time(0.3);
    /// # let _ = follow;
    /// ```
    #[must_use]
    pub fn with_smooth_time(self, seconds: f32) -> Self {
        Self {
            smooth_time: seconds,
            ..self
        }
    }

    /// The same, never showing outside `bounds`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let follow = ponko::d2::FollowCamera::default().with_bounds(Rect::new(0.0, 0.0, 3200.0, 720.0));
    /// # let _ = follow;
    /// ```
    #[must_use]
    pub fn with_bounds(self, bounds: Rect) -> Self {
        Self {
            bounds: Some(bounds),
            ..self
        }
    }

    /// The same, keeping the camera on `pixels`' grid (see
    /// [`pixel_perfect`](Self::pixel_perfect)).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// use ponko::d2::{FollowCamera, PixelPerfect};
    /// Game::new("Retro", 1280, 720).run_headless(1, |ctx| {
    ///     let mut follow = FollowCamera::default().with_pixel_perfect(PixelPerfect::new(320.0, 180.0));
    ///     follow.update(&mut ctx.camera, vec2(100.3, 50.8), ctx.time.frame_dt());
    ///     assert_eq!(ctx.camera.zoom, 4.0);
    ///     assert_eq!(ctx.camera.position, ctx.camera.position.round());
    /// });
    /// ```
    #[must_use]
    pub fn with_pixel_perfect(self, pixels: PixelPerfect2D) -> Self {
        Self {
            pixel_perfect: Some(pixels),
            ..self
        }
    }

    /// Puts the camera on `target` at once, without easing: for the first
    /// frame, a respawn or a new level. Forgets the dead zone and the
    /// smoothing's speed; keeps the look-ahead direction.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn respawn(ctx: &mut Ctx, follow: &mut ponko::d2::FollowCamera, spawn: Vec2) {
    /// follow.snap(&mut ctx.camera, spawn);
    /// # }
    /// ```
    pub fn snap(&mut self, camera: &mut Camera2D, target: Vec2) {
        if let Some(pixels) = self.pixel_perfect {
            pixels.apply(camera); // the zoom first: the bounds depend on it
        }
        self.focus = target;
        self.last_target = target;
        self.velocity = Vec2::ZERO;
        self.started = true;
        self.center = self.clamp(camera, target + self.facing * self.look_ahead + self.offset);
        self.goal = self.center;
        camera.look_at(self.center);
        if let Some(pixels) = self.pixel_perfect {
            pixels.apply(camera);
        }
    }

    /// Moves the camera toward `target` (the followed thing's world
    /// position) by `dt` seconds: call it once a frame with
    /// [`Time::frame_dt`](crate::time::Time::frame_dt), after the target
    /// moved. The first call [snaps](Self::snap).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn follow_hero(ctx: &mut Ctx, follow: &mut ponko::d2::FollowCamera, hero: Vec2) {
    /// follow.update(&mut ctx.camera, hero, ctx.time.frame_dt());
    /// # }
    /// ```
    pub fn update(&mut self, camera: &mut Camera2D, target: Vec2, dt: f32) {
        if !self.started || !self.center.is_finite() {
            self.snap(camera, target);
            return;
        }
        if let Some(pixels) = self.pixel_perfect {
            pixels.apply(camera); // the zoom first: the bounds depend on it
        }
        if dt > 0.0 {
            let speed = (target - self.last_target) / dt;
            let min = self.look_ahead_speed.max(0.0);
            for axis in 0..2 {
                if speed[axis].abs() > min {
                    self.facing[axis] = speed[axis].signum();
                }
            }
        }
        self.last_target = target;
        let half = self.dead_zone.max(Vec2::ZERO) / 2.0;
        self.focus = self.focus.clamp(target - half, target + half);
        let want = self.clamp(camera, self.focus + self.facing * self.look_ahead + self.offset);
        // The goal moved from where it was to `want` during this step.
        self.center = smooth_damp_moving(self.center, self.goal, want, &mut self.velocity, self.smooth_time, dt);
        self.goal = want;
        let clamped = self.clamp(camera, self.center);
        if clamped != self.center {
            self.center = clamped;
            self.velocity = Vec2::ZERO;
        }
        camera.look_at(self.center);
        if let Some(pixels) = self.pixel_perfect {
            pixels.apply(camera);
        }
    }

    /// `center` moved so the view stays within the bounds.
    fn clamp(&self, camera: &Camera2D, center: Vec2) -> Vec2 {
        let Some(b) = self.bounds else { return center };
        let half = camera.viewport() / (2.0 * camera.zoom.max(f32::MIN_POSITIVE));
        let fit = |v: f32, lo: f32, hi: f32, h: f32| {
            if hi - lo <= 2.0 * h {
                (lo + hi) / 2.0
            } else {
                v.clamp(lo + h, hi - h)
            }
        };
        Vec2::new(
            fit(center.x, b.min.x, b.max.x, half.x),
            fit(center.y, b.min.y, b.max.y, half.y),
        )
    }
}

/// Pixel-perfect 2D for pixel art: the window shows the art at a whole
/// number of window pixels per art pixel (never 2.5, which makes some art
/// pixels wider than others), and the camera moves in whole art pixels, so
/// pixels never shimmer as it scrolls.
///
/// Set [`resolution`](Self::resolution) to the size the game is drawn for
/// (320 x 180, say) and call [`apply`](Self::apply) once a frame, after
/// whatever placed the camera. The zoom becomes the largest whole scale at
/// which that resolution fits the window. A window that isn't an exact
/// multiple shows a little more of the world around it;
/// [`screen_rect`](Self::screen_rect) says where the exact resolution is,
/// to cover the rest with bars.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d2::PixelPerfect;
///
/// Game::new("Retro", 1280, 720).run_headless(1, |ctx| {
///     let pixels = PixelPerfect::new(320.0, 180.0);
///     ctx.camera.look_at(vec2(100.3, 50.8));
///     pixels.apply(&mut ctx.camera);
///     assert_eq!(ctx.camera.zoom, 4.0); // 1280 / 320
///     assert_eq!(ctx.camera.position, ctx.camera.position.round()); // whole art pixels
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PixelPerfect2D {
    /// The size, in art pixels, the game is drawn for. Default 320 x 180.
    pub resolution: Vec2,
    /// Art pixels per world unit. Default 1: a world unit is an art pixel.
    pub pixels_per_unit: f32,
    /// Move the camera in whole art pixels. Default on. Off, it moves in
    /// whole window pixels, smoother but with art pixels that can shift
    /// against each other.
    pub snap: bool,
}

impl Default for PixelPerfect2D {
    fn default() -> Self {
        Self::new(320.0, 180.0)
    }
}

impl PixelPerfect2D {
    /// Pixel art drawn for `width` x `height` art pixels.
    ///
    /// ```
    /// let pixels = ponko::d2::PixelPerfect::new(384.0, 216.0);
    /// # let _ = pixels;
    /// ```
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self {
            resolution: Vec2::new(width, height),
            pixels_per_unit: 1.0,
            snap: true,
        }
    }

    /// The whole number of window pixels per art pixel for a window of
    /// `window` pixels: the largest at which the resolution fits, at least 1.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let pixels = ponko::d2::PixelPerfect::new(320.0, 180.0);
    /// assert_eq!(pixels.scale(vec2(1920.0, 1080.0)), 6.0);
    /// assert_eq!(pixels.scale(vec2(1000.0, 1000.0)), 3.0);
    /// assert_eq!(pixels.scale(vec2(200.0, 100.0)), 1.0);
    /// ```
    #[must_use]
    pub fn scale(&self, window: Vec2) -> f32 {
        let fit = window / self.resolution.max(Vec2::ONE);
        fit.x.min(fit.y).floor().max(1.0)
    }

    /// The camera zoom for a window of `window` pixels: the
    /// [`scale`](Self::scale) times [`pixels_per_unit`](Self::pixels_per_unit).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut pixels = ponko::d2::PixelPerfect::new(320.0, 180.0);
    /// pixels.pixels_per_unit = 16.0; // one world unit is a 16-pixel tile
    /// assert_eq!(pixels.zoom(vec2(1280.0, 720.0)), 64.0);
    /// ```
    #[must_use]
    pub fn zoom(&self, window: Vec2) -> f32 {
        self.scale(window) * self.pixels_per_unit.max(f32::MIN_POSITIVE)
    }

    /// `world` moved to the nearest art pixel corner (or window pixel at
    /// `zoom`, with [`snap`](Self::snap) off): for sprites that should sit
    /// on the pixel grid too.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let pixels = ponko::d2::PixelPerfect::new(320.0, 180.0);
    /// assert_eq!(pixels.snap_point(vec2(10.4, 3.6), 4.0), vec2(10.0, 4.0));
    /// ```
    #[must_use]
    pub fn snap_point(&self, world: Vec2, zoom: f32) -> Vec2 {
        let per_unit = if self.snap {
            self.pixels_per_unit.max(f32::MIN_POSITIVE)
        } else {
            zoom.max(f32::MIN_POSITIVE)
        };
        (world * per_unit).round() / per_unit
    }

    /// Sets the camera's zoom to the whole scale for its window, keeping
    /// the world point in the middle, and moves it onto the pixel grid.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn late(ctx: &mut Ctx, pixels: &ponko::d2::PixelPerfect) {
    /// pixels.apply(&mut ctx.camera);
    /// # }
    /// ```
    pub fn apply(&self, camera: &mut Camera2D) {
        let center = camera.center();
        camera.zoom = self.zoom(camera.viewport());
        camera.look_at(center);
        camera.position = self.snap_point(camera.position, camera.zoom);
    }

    /// Where in a window of `window` pixels exactly the
    /// [`resolution`](Self::resolution) shows, at the whole scale, centered:
    /// draw bars around it (in screen space) to show nothing more.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let pixels = ponko::d2::PixelPerfect::new(320.0, 180.0);
    /// let r = pixels.screen_rect(vec2(1000.0, 600.0)); // scale 3: 960 x 540
    /// assert_eq!(r, Rect::new(20.0, 30.0, 960.0, 540.0));
    /// ```
    #[must_use]
    pub fn screen_rect(&self, window: Vec2) -> Rect {
        let size = self.resolution * self.scale(window);
        let min = ((window - size) / 2.0).floor();
        Rect { min, max: min + size }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera2D {
        Camera2D::new(Vec2::new(320.0, 180.0))
    }

    /// Runs `follow` after a target moving at `speed` from `start` for
    /// `seconds` at `hz`; returns the camera center.
    fn chase(mut follow: FollowCamera2D, hz: f32, seconds: f32, start: Vec2, speed: Vec2) -> (Vec2, FollowCamera2D) {
        let mut c = cam();
        follow.update(&mut c, start, 0.0);
        let steps = (seconds * hz).round() as usize;
        for i in 1..=steps {
            let t = i as f32 / hz;
            follow.update(&mut c, start + speed * t, 1.0 / hz);
        }
        (c.center(), follow)
    }

    #[test]
    fn the_first_update_puts_the_camera_on_the_target() {
        let mut c = cam();
        let mut f = FollowCamera2D::default().with_offset(Vec2::new(0.0, -10.0));
        f.update(&mut c, Vec2::new(500.0, 200.0), 1.0 / 60.0);
        assert!(c.center().abs_diff_eq(Vec2::new(500.0, 190.0), 1e-4));
    }

    #[test]
    fn the_target_moves_freely_inside_the_dead_zone() {
        let mut c = cam();
        let mut f = FollowCamera2D::default().with_dead_zone(Vec2::new(40.0, 40.0));
        f.update(&mut c, Vec2::new(100.0, 100.0), 0.0);
        for x in 100..=120 {
            f.update(&mut c, Vec2::new(x as f32, 100.0), 1.0 / 60.0);
        }
        assert!(c.center().abs_diff_eq(Vec2::new(100.0, 100.0), 1e-4), "{}", c.center());
        // Past its edge, the camera follows the edge.
        for _ in 0..240 {
            f.update(&mut c, Vec2::new(150.0, 100.0), 1.0 / 60.0);
        }
        assert!(c.center().abs_diff_eq(Vec2::new(130.0, 100.0), 1e-2), "{}", c.center());
    }

    #[test]
    fn look_ahead_shows_where_the_target_heads_and_holds_when_it_stops() {
        let follow = FollowCamera2D::default().with_look_ahead(Vec2::new(30.0, 0.0));
        let (center, f) = chase(follow, 60.0, 2.0, Vec2::ZERO, Vec2::new(-100.0, 0.0));
        assert_eq!(f.facing, Vec2::new(-1.0, 0.0));
        // Ahead of the target, minus the smoothing's steady lag (speed x smooth time).
        assert!(
            (center.x - (-200.0 - 30.0 + 100.0 * 0.15)).abs() < 0.01,
            "ahead, to the left: {center}"
        );
        let mut c = cam();
        let mut f = f;
        for _ in 0..240 {
            f.update(&mut c, Vec2::new(-200.0, 0.0), 1.0 / 60.0);
        }
        assert!(c.center().abs_diff_eq(Vec2::new(-230.0, 0.0), 1e-2), "{}", c.center());
    }

    #[test]
    fn the_camera_never_shows_past_the_bounds() {
        let mut c = cam();
        c.zoom = 2.0; // shows 160 x 90
        let bounds = Rect::new(0.0, 0.0, 1000.0, 400.0);
        let mut f = FollowCamera2D::default().with_bounds(bounds);
        for target in [
            Vec2::new(-50.0, -50.0),
            Vec2::new(990.0, 399.0),
            Vec2::new(500.0, 200.0),
        ] {
            f.snap(&mut c, target);
            for _ in 0..30 {
                f.update(&mut c, target, 1.0 / 60.0);
                let v = c.visible_rect();
                assert!(v.min.x >= -1e-3 && v.min.y >= -1e-3, "{v:?}");
                assert!(v.max.x <= 1000.0 + 1e-3 && v.max.y <= 400.0 + 1e-3, "{v:?}");
            }
        }
        // Bounds narrower than the view: centered on them.
        let mut f = FollowCamera2D::default().with_bounds(Rect::new(0.0, 0.0, 100.0, 50.0));
        f.update(&mut c, Vec2::new(90.0, 0.0), 0.0);
        assert!(c.center().abs_diff_eq(Vec2::new(50.0, 25.0), 1e-4));
    }

    #[test]
    fn following_is_frame_rate_independent() {
        let follow = FollowCamera2D::default()
            .with_dead_zone(Vec2::new(20.0, 10.0))
            .with_look_ahead(Vec2::new(40.0, 0.0))
            .with_smooth_time(0.2);
        let ends: Vec<Vec2> = [30.0, 60.0, 144.0]
            .iter()
            .map(|&hz| chase(follow, hz, 1.5, Vec2::ZERO, Vec2::new(120.0, -40.0)).0)
            .collect();
        // A target moving at a steady speed: the same place whatever the
        // frame rate.
        for e in &ends[1..] {
            assert!(e.distance(ends[0]) < 0.01, "{ends:?}");
        }
    }

    #[test]
    fn pixel_perfect_uses_whole_scales_and_whole_pixels() {
        let mut c = Camera2D::new(Vec2::new(1366.0, 768.0));
        let p = PixelPerfect2D::new(320.0, 180.0);
        c.look_at(Vec2::new(77.77, 33.33));
        p.apply(&mut c);
        assert_eq!(c.zoom, 4.0);
        assert_eq!(c.position, c.position.round());
        assert!(c.center().distance(Vec2::new(77.77, 33.33)) <= 0.5f32.hypot(0.5) + 1e-4);
        // Window pixels of an art pixel's edge are whole numbers.
        let edge = c.world_to_screen(Vec2::new(12.0, 7.0));
        assert_eq!(edge, edge.round());
        let r = p.screen_rect(c.viewport());
        assert_eq!(r.size(), Vec2::new(1280.0, 720.0));
        assert_eq!(r.min, Vec2::new(43.0, 24.0));
    }

    #[test]
    fn pixel_perfect_without_snap_moves_in_window_pixels() {
        let mut c = Camera2D::new(Vec2::new(640.0, 360.0));
        let p = PixelPerfect2D {
            snap: false,
            ..PixelPerfect2D::new(320.0, 180.0)
        };
        c.zoom = 2.0;
        c.position = Vec2::new(10.3, 4.2);
        p.apply(&mut c);
        assert_eq!(c.zoom, 2.0);
        assert_eq!(c.position, Vec2::new(10.5, 4.0));
    }
}
