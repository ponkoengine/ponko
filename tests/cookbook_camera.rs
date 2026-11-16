//! Cookbook, "Cameras": following the player, shaking the screen, zooming
//! toward the cursor, pixel art and a minimap. Every recipe between `ANCHOR`
//! lines is what `book/src/cookbook/camera.md` shows, and each test runs it
//! headless (with frames, so the frame systems run) and checks that it did
//! its job.
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::camera::CameraShake;
use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

#[derive(Component)]
struct Player;

/// What the camera looked at on each frame: its centre.
#[derive(Default)]
struct Centres(Vec<Vec2>);

fn record_camera(ctx: &mut Ctx) {
    let centre = ctx.camera.center();
    ctx.world.resource_mut::<Centres>().0.push(centre);
}

// ANCHOR: follow
fn setup_follow_camera(ctx: &mut Ctx) {
    ctx.world.insert_resource(
        FollowCamera::default()
            .with_dead_zone(vec2(32.0, 48.0)) // the player can move this much without scrolling
            .with_look_ahead(vec2(48.0, 0.0)) // show more of where they are heading
            .with_bounds(Rect::new(0.0, 0.0, 4000.0, 720.0)), // never show past the level
    );
}

/// A frame system, after the player moved.
fn follow_the_player(ctx: &mut Ctx) {
    let Some(hero) = ctx
        .world
        .query::<(&Player, &Transform)>()
        .iter()
        .next()
        .map(|(_, t)| t.position)
    else {
        return;
    };
    let dt = ctx.time.frame_dt();
    ctx.world
        .resource_mut::<FollowCamera>()
        .update(&mut ctx.camera, hero, dt);
}
// ANCHOR_END: follow

#[test]
fn the_camera_catches_up_with_the_player_and_stops_at_the_level_edge() {
    let run = |hero_at: Vec2| {
        Game::new("follow", 800, 600)
            .setup(move |ctx| {
                setup_follow_camera(ctx);
                ctx.world.insert_resource(Centres::default());
                ctx.world.spawn((Player, Transform::at(hero_at)));
            })
            .system(follow_the_player)
            .system(record_camera)
            .headless_frames(true)
            .run_headless(180)
    };
    let world = run(vec2(2000.0, 360.0));
    let last = *world.resource::<Centres>().0.last().unwrap();
    assert!(
        (last.x - 2000.0).abs() < 60.0,
        "centred on the player, give or take the look-ahead: {last}"
    );
    let world = run(vec2(10.0, 360.0));
    let last = world.resource::<Centres>().0.last().unwrap();
    assert!(
        (last.x - 400.0).abs() < 1.0,
        "the level starts at 0, so the camera stops half a screen in: {last}"
    );
}

// ANCHOR: shake
struct Hit; // an event: something the player felt

fn shake_on_hits(ctx: &mut Ctx) {
    let hits = ctx.events.read::<Hit>().len();
    let shake = ctx.world.resource_mut::<CameraShake>();
    if hits > 0 {
        shake.add_trauma(0.5 * hits as f32); // trauma runs from 0 to 1 and wears off by itself
    }
    shake.update(ctx.time.frame_dt());
    shake.apply_2d(&mut ctx.camera); // after the camera was placed for this frame
}
// ANCHOR_END: shake

#[test]
fn a_hit_shakes_the_camera_and_the_shaking_dies_down() {
    let world = Game::new("shake", 800, 600)
        .setup(|ctx| {
            setup_follow_camera(ctx);
            ctx.world.insert_resource(CameraShake::default());
            ctx.world.insert_resource(Centres::default());
            ctx.world.spawn((Player, Transform::at(vec2(400.0, 300.0))));
        })
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 20 {
                ctx.events.send(Hit);
            }
        })
        .system(follow_the_player)
        .system(shake_on_hits)
        .system(record_camera)
        .headless_frames(true)
        .run_headless(200);
    let centres = &world.resource::<Centres>().0;
    let rest = centres[10];
    assert_eq!(centres[19], rest, "still before the hit");
    let worst = centres[21..40].iter().map(|c| c.distance(rest)).fold(0.0, f32::max);
    assert!(worst > 0.3, "shook by up to {worst} pixels");
    assert!(
        centres[199].distance(rest) < 0.01,
        "calm again after a few seconds: {}",
        centres[199]
    );
}

// ANCHOR: zoom
/// Zooms in (wheel up) or out around the pixel under the cursor, so that
/// pixel stays under the cursor.
fn zoom_at_cursor(ctx: &mut Ctx, wheel: f32) {
    if wheel != 0.0 {
        let cursor = ctx.input.mouse_pos();
        ctx.camera.zoom_at(cursor, 1.1f32.powf(wheel)); // 10% per notch
    }
}

fn zoom_with_the_wheel(ctx: &mut Ctx) {
    let wheel = ctx.input.mouse_wheel().y;
    zoom_at_cursor(ctx, wheel);
}
// ANCHOR_END: zoom

#[test]
fn zooming_keeps_the_world_point_under_the_cursor_where_it_was() {
    Game::new("zoom", 800, 600).run_headless(0, |ctx| {
        ctx.camera.position = vec2(120.0, 80.0);
        ctx.input.feed(InputEvent::MouseMove(vec2(600.0, 150.0))); // the test plays the mouse
        let cursor = ctx.input.mouse_pos();
        let under_before = ctx.camera.screen_to_world(cursor);
        zoom_at_cursor(ctx, 3.0);
        assert!(
            (ctx.camera.zoom - 1.1f32.powf(3.0)).abs() < 1e-4,
            "three notches in: {}",
            ctx.camera.zoom
        );
        let under_after = ctx.camera.screen_to_world(cursor);
        assert!(
            under_before.distance(under_after) < 1e-3,
            "{under_before} then {under_after}"
        );
        zoom_at_cursor(ctx, 0.0);
        assert!((ctx.camera.zoom - 1.1f32.powf(3.0)).abs() < 1e-4, "no wheel, no zoom");
    });
}

#[test]
fn two_notches_of_the_wheel_zoom_in_around_the_pointer() {
    let pointer = vec2(200.0, 450.0);
    let world = Game::new("zoom", 800, 600)
        .setup(|ctx| ctx.world.insert_resource(Centres(Vec::new())))
        .sim_system(zoom_with_the_wheel)
        .sim_system(|ctx: &mut Ctx| {
            let (zoom, under) = (ctx.camera.zoom, ctx.camera.screen_to_world(ctx.input.mouse_pos()));
            ctx.world.resource_mut::<Centres>().0.push(vec2(zoom, under.x));
        })
        .input_scenario(
            InputScenario::new()
                .at(0, InputEvent::MouseMove(pointer))
                .at(2, InputEvent::MouseWheel(vec2(0.0, 2.0))),
        )
        .run_headless(4);
    let seen = &world.resource::<Centres>().0;
    assert_eq!(seen[1], vec2(1.0, 200.0), "no wheel yet");
    assert!((seen[2].x - 1.21).abs() < 1e-4, "two notches of 10%: {}", seen[2].x);
    assert!(
        (seen[3].y - 200.0).abs() < 1e-3,
        "the same world point under the pointer"
    );
    assert_eq!(seen[3].x, seen[2].x, "the wheel turned in one frame only");
}

// ANCHOR: pixel_perfect
fn setup_pixel_art(ctx: &mut Ctx) {
    // Art drawn for 320 x 180: the camera zooms by whole numbers only.
    ctx.world.insert_resource(PixelPerfect::new(320.0, 180.0));
}

/// A frame system: after anything else that moves the camera.
fn keep_pixels_square(ctx: &mut Ctx) {
    ctx.world.resource::<PixelPerfect>().apply(&mut ctx.camera);
}
// ANCHOR_END: pixel_perfect

#[test]
fn pixel_art_zooms_by_the_largest_whole_number_that_fits() {
    let zoom_in = |w: u32, h: u32| {
        let world = Game::new("pixels", w, h)
            .setup(setup_pixel_art)
            .system(keep_pixels_square)
            .system(|ctx: &mut Ctx| {
                let zoom = ctx.camera.zoom;
                ctx.world.insert_resource(Centres(vec![vec2(zoom, 0.0)]));
            })
            .headless_frames(true)
            .run_headless(2);
        world.resource::<Centres>().0[0].x
    };
    assert_eq!(zoom_in(1280, 720), 4.0, "exactly 4 window pixels per art pixel");
    assert_eq!(zoom_in(1000, 600), 3.0, "1000 / 320 = 3.1, 600 / 180 = 3.3");
    assert_eq!(zoom_in(640, 360), 2.0);
}

// ANCHOR: minimap
const MAP_SIZE: Vec2 = vec2(160.0, 90.0); // pixels on screen
const WORLD_SIZE: Vec2 = vec2(3200.0, 1800.0);

/// Where a world position lands on the minimap, in window pixels. The map sits
/// in the bottom right corner, 10 pixels from the edges.
fn on_minimap(window: Vec2, world_pos: Vec2) -> Vec2 {
    let corner = window - MAP_SIZE - Vec2::splat(10.0);
    corner + world_pos / WORLD_SIZE * MAP_SIZE
}

#[derive(Component)]
struct Marker(Color);

/// A frame system. `DrawSpace::Screen` puts a sprite in window pixels, ignoring the camera.
fn draw_minimap(ctx: &mut Ctx) {
    let dot = ctx.world.resource::<Dot>().0; // a 1 x 1 white texture, made once in setup
    let window = ctx.window_size();
    // The pivot is the point of the sprite at the draw position: the top left, not the middle.
    let screen = DrawParams::default()
        .with_space(DrawSpace::Screen)
        .with_pivot(Vec2::ZERO);
    let corner = window - MAP_SIZE - Vec2::splat(10.0);
    ctx.gfx.draw_ex(
        dot,
        corner,
        screen.with_scale(MAP_SIZE).with_color(Color::BLACK.with_alpha(0.5)),
    );
    ctx.world.query::<(&Transform, &Marker)>().for_each(|(t, marker)| {
        let at = on_minimap(window, t.position) - Vec2::splat(1.5);
        ctx.gfx
            .draw_ex(dot, at, screen.with_scale(Vec2::splat(3.0)).with_color(marker.0));
    });
}
// ANCHOR_END: minimap

struct Dot(Texture);

#[test]
fn units_land_on_the_minimap_in_proportion_and_the_map_draws_in_a_frame() {
    let window = vec2(1280.0, 720.0);
    let top_left = on_minimap(window, Vec2::ZERO);
    let bottom_right = on_minimap(window, WORLD_SIZE);
    assert_eq!(top_left, vec2(1110.0, 620.0));
    assert_eq!(bottom_right, vec2(1270.0, 710.0), "10 pixels from the corner");
    assert_eq!(
        on_minimap(window, WORLD_SIZE / 2.0),
        vec2(1190.0, 665.0),
        "the middle of the world, the middle of the map"
    );
    // And a frame runs the system over some units.
    Game::new("minimap", 1280, 720)
        .headless_frames(true)
        .setup(|ctx| {
            let dot = ctx.gfx.create_texture(1, 1, &[255, 255, 255, 255]);
            ctx.world.insert_resource(Dot(dot));
            ctx.world.spawn((Transform::at(vec2(100.0, 100.0)), Marker(Color::RED)));
            ctx.world
                .spawn((Transform::at(vec2(3000.0, 900.0)), Marker(Color::GREEN)));
        })
        .system(draw_minimap)
        .run_headless_scenario(InputScenario::new().duration(3))
        .1
        .assert_passed();
}
