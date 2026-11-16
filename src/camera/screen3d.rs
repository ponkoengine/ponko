//! Screen and world conversion for the 3D camera, next to the 2D camera's
//! [`screen_to_world`](crate::d2::Camera::screen_to_world) and
//! [`world_to_screen`](crate::d2::Camera::world_to_screen).

use glam::{Vec2, Vec3, Vec4};

use crate::three::Camera3D;

impl Camera3D {
    /// The window point (logical pixels, as
    /// [`Input::mouse_pos`](crate::input::Input::mouse_pos)) where world
    /// point `world` appears; `None` when it is behind the camera. Points
    /// outside the view give points outside the window.
    ///
    /// ```
    /// use ponko::prelude::*;
    ///
    /// Game::new("Labels", 800, 600).run_headless(1, |ctx| {
    ///     let cam = &mut ctx.scene3d.camera;
    ///     cam.position = vec3(0.0, 0.0, 10.0);
    ///     cam.look_at(Vec3::ZERO);
    ///     assert!(cam.world_to_screen(Vec3::ZERO).unwrap().abs_diff_eq(vec2(400.0, 300.0), 1e-3));
    ///     assert_eq!(cam.world_to_screen(vec3(0.0, 0.0, 20.0)), None); // behind
    /// });
    /// ```
    #[must_use]
    pub fn world_to_screen(&self, world: Vec3) -> Option<Vec2> {
        let clip = self.view_proj() * Vec4::new(world.x, world.y, world.z, 1.0);
        if clip.w <= 1e-6 || !clip.is_finite() {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let size = self.viewport();
        let corner = self.screen_rect().min;
        Some(corner + Vec2::new((ndc.x + 1.0) * 0.5 * size.x, (1.0 - ndc.y) * 0.5 * size.y))
    }

    /// The world point under window point `screen`, `depth` world units in
    /// front of the camera (along its view direction): the inverse of
    /// [`world_to_screen`](Self::world_to_screen) for a point at that depth.
    /// For placing things under the mouse at a chosen distance.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let cam = ctx.scene3d.camera;
    /// let cursor = cam.screen_to_world(ctx.input.mouse_pos(), 4.0);
    /// assert!(cam.world_to_screen(cursor).unwrap().abs_diff_eq(ctx.input.mouse_pos(), 1e-2));
    /// # }
    /// ```
    #[must_use]
    pub fn screen_to_world(&self, screen: Vec2, depth: f32) -> Vec3 {
        let (from, dir) = self.screen_ray(screen);
        let along = dir.dot(self.forward()).max(1e-6);
        from + dir * (depth / along)
    }

    /// Where the ray under window point `screen` meets the level plane
    /// `y = height` (the ground, a table top); `None` when it points away
    /// from it or along it. The usual "where on the ground did they click".
    ///
    /// ```
    /// use ponko::prelude::*;
    ///
    /// Game::new("Strategy", 800, 600).run_headless(1, |ctx| {
    ///     let cam = &mut ctx.scene3d.camera;
    ///     cam.position = vec3(0.0, 10.0, 0.0);
    ///     cam.look_at(vec3(0.0, 0.0, -0.001)); // straight down
    ///     let ground = cam.screen_to_ground(vec2(400.0, 300.0), 0.0).unwrap();
    ///     assert!(ground.abs_diff_eq(Vec3::ZERO, 1e-2));
    /// });
    /// ```
    #[must_use]
    pub fn screen_to_ground(&self, screen: Vec2, height: f32) -> Option<Vec3> {
        let (from, dir) = self.screen_ray(screen);
        if dir.y.abs() <= 1e-6 {
            return None;
        }
        let t = (height - from.y) / dir.y;
        (t >= 0.0 && t.is_finite()).then(|| from + dir * t)
    }

    /// The camera's right direction (its +X axis).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let strafe = ctx.scene3d.camera.right() * 2.0;
    /// # }
    /// ```
    #[must_use]
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// The camera's up direction (its +Y axis).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let above = ctx.scene3d.camera.position + ctx.scene3d.camera.up();
    /// # }
    /// ```
    #[must_use]
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera3D {
        let mut c = Camera3D::new(Vec2::new(1280.0, 720.0));
        c.position = Vec3::new(3.0, 5.0, 9.0);
        c.look_at(Vec3::new(-1.0, 0.0, 0.0));
        c
    }

    #[test]
    fn world_to_screen_and_screen_to_world_round_trip() {
        let c = cam();
        for screen in [Vec2::new(0.0, 0.0), Vec2::new(640.0, 360.0), Vec2::new(1100.0, 50.0)] {
            for depth in [0.5, 10.0, 300.0] {
                let w = c.screen_to_world(screen, depth);
                let depth_back = (w - c.position).dot(c.forward());
                assert!((depth_back - depth).abs() < depth * 1e-4, "{depth_back} vs {depth}");
                let back = c.world_to_screen(w).unwrap();
                assert!(back.abs_diff_eq(screen, 0.05), "{back} vs {screen}");
            }
        }
    }

    #[test]
    fn a_camera_in_part_of_the_window_converts_from_that_part() {
        let mut c = cam();
        c.rect = crate::camera::Viewport::new(0.5, 0.5, 0.5, 0.5);
        c.place(Vec2::new(1280.0, 720.0));
        assert_eq!(c.viewport(), Vec2::new(640.0, 360.0));
        // The middle of its quarter looks straight ahead.
        let (_, dir) = c.screen_ray(Vec2::new(960.0, 540.0));
        assert!(dir.abs_diff_eq(c.forward(), 1e-4), "{dir}");
        let ahead = c.position + c.forward() * 10.0;
        assert!(
            c.world_to_screen(ahead)
                .unwrap()
                .abs_diff_eq(Vec2::new(960.0, 540.0), 0.05)
        );
        for screen in [Vec2::new(700.0, 400.0), Vec2::new(1200.0, 700.0)] {
            let w = c.screen_to_world(screen, 7.0);
            assert!(c.world_to_screen(w).unwrap().abs_diff_eq(screen, 0.05));
        }
        // Its own aspect ratio: a quarter of a 16:9 window is 16:9 too, a
        // half is not.
        c.rect = crate::camera::Viewport::split(2, 0);
        c.place(Vec2::new(1280.0, 720.0));
        assert_eq!(c.viewport(), Vec2::new(640.0, 720.0));
        assert!((c.projection().x_axis.x / c.projection().y_axis.y - 720.0 / 640.0).abs() < 1e-4);
    }

    #[test]
    fn world_to_screen_agrees_with_the_screen_ray() {
        let c = cam();
        let (from, dir) = c.screen_ray(Vec2::new(200.0, 500.0));
        let p = c.world_to_screen(from + dir * 25.0).unwrap();
        assert!(p.abs_diff_eq(Vec2::new(200.0, 500.0), 0.05), "{p}");
        assert_eq!(c.world_to_screen(c.position - c.forward()), None);
    }

    #[test]
    fn screen_to_ground_hits_the_plane_under_the_cursor() {
        let c = cam();
        let p = c.screen_to_ground(Vec2::new(640.0, 360.0), 0.0).unwrap();
        assert!(p.y.abs() < 1e-4);
        assert!(p.abs_diff_eq(Vec3::new(-1.0, 0.0, 0.0), 1e-3), "{p}");
        // A plane above a ray that points down: never met.
        assert_eq!(c.screen_to_ground(Vec2::new(640.0, 360.0), 100.0), None);
    }
}
