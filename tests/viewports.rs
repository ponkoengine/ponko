//! Several cameras in one window, in running headless games: each camera
//! is given its part of the window at the frame's start, and a point of the
//! window is picked through the camera that draws there.

use std::cell::RefCell;
use std::rc::Rc;

use ponko::camera::{RenderLayers, Viewport};
use ponko::prelude::*;

/// Runs one headless frame of an 800 x 400 window set up by `setup`, and
/// returns what `probe` saw in it.
fn in_a_frame<T: Default + 'static>(setup: impl FnOnce(&mut Ctx) + 'static, probe: impl Fn(&Ctx) -> T + 'static) -> T {
    let out = Rc::new(RefCell::new(T::default()));
    let seen = out.clone();
    Game::new("views", 800, 400)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| *seen.borrow_mut() = probe(ctx))
        .run_headless(1, setup);
    out.take()
}

#[test]
fn a_click_in_player_twos_half_goes_through_player_twos_2d_camera() {
    let (at, world, mouse_rect) = in_a_frame(
        |ctx| {
            ctx.camera.rect = Viewport::split(2, 0);
            let mut p2 = ctx.camera;
            p2.rect = Viewport::split(2, 1);
            p2.position = vec2(1000.0, 0.0);
            ctx.views2d.push(d2::View::new(p2));
        },
        |ctx| {
            let click = vec2(410.0, 20.0); // 10 into the right half
            let camera = d2::View::camera_at(&ctx.camera, &ctx.views2d, click);
            (camera.position, camera.screen_to_world(click), ctx.camera.screen_rect())
        },
    );
    assert_eq!(at, vec2(1000.0, 0.0), "the right half's camera");
    assert_eq!(world, vec2(1010.0, 20.0));
    assert_eq!(mouse_rect, Rect::new(0.0, 0.0, 400.0, 400.0), "player one's half");
}

#[test]
fn outside_every_view_the_main_2d_camera_answers() {
    let main = in_a_frame(
        |ctx| {
            let mut map = ctx.camera;
            map.rect = Viewport::new(0.75, 0.0, 0.25, 0.25);
            map.position = vec2(5000.0, 0.0);
            ctx.views2d.push(d2::View::new(map));
        },
        |ctx| {
            let in_map = d2::View::camera_at(&ctx.camera, &ctx.views2d, vec2(700.0, 50.0)).position;
            let below = d2::View::camera_at(&ctx.camera, &ctx.views2d, vec2(700.0, 300.0)).position;
            let outside = d2::View::camera_at(&ctx.camera, &ctx.views2d, vec2(-5.0, -5.0)).position;
            [in_map, below, outside]
        },
    );
    assert_eq!(main, [vec2(5000.0, 0.0), Vec2::ZERO, Vec2::ZERO]);
}

#[test]
fn render_layers_default_to_layer_zero_drawn_by_every_camera() {
    assert_eq!(RenderLayers::default(), RenderLayers::DEFAULT);
    let camera = in_a_frame(|_| {}, |ctx| ctx.camera.layers);
    assert_eq!(camera, RenderLayers::ALL);
    assert!(camera.intersects(RenderLayers::DEFAULT));
    assert!(!RenderLayers::ALL.without(1).intersects(RenderLayers::layer(1)));
}

#[cfg(feature = "3d")]
#[test]
fn a_click_in_a_3d_view_picks_the_ground_under_that_views_camera() {
    let (position, ground, main_ground) = in_a_frame(
        |ctx| {
            ctx.scene3d.camera.rect = Viewport::split(2, 0);
            ctx.scene3d.camera.position = vec3(0.0, 10.0, 10.0);
            ctx.scene3d.camera.look_at(Vec3::ZERO);
            let mut p2 = ctx.scene3d.camera;
            p2.rect = Viewport::split(2, 1);
            p2.position = vec3(50.0, 10.0, 10.0);
            p2.look_at(vec3(50.0, 0.0, 0.0));
            ctx.scene3d.views.push(d3::View::new(p2));
        },
        |ctx| {
            // The middle of each half looks at that camera's target.
            let right = vec2(600.0, 200.0);
            let camera = ctx.scene3d.camera_at(right);
            let left = vec2(200.0, 200.0);
            let main = ctx.scene3d.camera_at(left);
            (
                camera.position,
                camera.screen_to_ground(right, 0.0),
                main.screen_to_ground(left, 0.0),
            )
        },
    );
    assert_eq!(position, vec3(50.0, 10.0, 10.0));
    let ground = ground.expect("the click hits the ground");
    assert!(ground.distance(vec3(50.0, 0.0, 0.0)) < 1e-3, "{ground}");
    let main_ground = main_ground.expect("the click hits the ground");
    assert!(main_ground.distance(Vec3::ZERO) < 1e-3, "{main_ground}");
}

#[test]
fn render_layers_save_as_their_bits_and_load_back() {
    Game::new("layers", 1, 1).run_headless(0, |ctx| {
        let icon = ctx
            .world
            .spawn((d2::Transform::default(), RenderLayers::layer(1).with(3)));
        assert_eq!(ctx.world.get_json(icon, "RenderLayers").unwrap(), serde_json::json!(10));
        let save = ctx.world.save();
        ctx.world.clear();
        ctx.world.load(&save).unwrap();
        assert_eq!(ctx.world.get::<RenderLayers>(icon), Some(&RenderLayers(0b1010)));
    });
}

#[test]
fn a_changed_rect_converts_points_in_the_same_frame() {
    Game::new("Duel", 800, 400).run_headless(0, |ctx| {
        ctx.camera.rect = Viewport::split(2, 1);
        assert_eq!(ctx.camera.screen_rect(), Rect::new(400.0, 0.0, 400.0, 400.0));
        assert_eq!(ctx.camera.viewport(), vec2(400.0, 400.0));
        assert_eq!(ctx.camera.screen_to_world(vec2(410.0, 0.0)), vec2(10.0, 0.0));
        assert_eq!(ctx.camera.world_to_screen(vec2(10.0, 0.0)), vec2(410.0, 0.0));
        // A view made from the main camera knows the window too.
        let mut map = ctx.camera;
        map.rect = Viewport::new(0.0, 0.0, 0.25, 0.25);
        assert_eq!(map.screen_rect(), Rect::new(0.0, 0.0, 200.0, 100.0));
    });
}
