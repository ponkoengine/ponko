//! Several cameras in one window, drawn offscreen: a two-player split
//! screen, a minimap on its own render layers, the sun's shadow maps shared
//! by cameras far apart, and each view's own temporal history.
//!
//! The frames go through the code a game's frame does: the main frame and
//! every view's collected as `Ctx::prepare_3d` does, then drawn and copied
//! into the window by the renderer's `WindowViews`.

use super::regression_stage::*;
use super::render_test_support::*;
use super::views::WindowViews;
use crate::camera::{RenderLayers, Viewport};
use crate::prelude::*;
use crate::render::AntiAliasing;
use crate::three::{Camera3D, Scene3D, SkyLight, linear};

/// Draws one frame of the stage's main camera and `ctx.scene3d.views` into
/// the stage's output and reads it back. `shared_shadows: false` leaves the
/// views out of the sun's cascades (what the views would get without
/// shared shadows: a negative control).
pub(super) fn frame(stage: &mut Stage, windows: &mut WindowViews, shared_shadows: bool) -> Image {
    let ctx = &mut stage.ctx;
    ctx.place_cameras();
    let post = &ctx.scene3d.post;
    ctx.gfx.frame3d.motion.enabled = post.anti_aliasing == AntiAliasing::Taa;
    ctx.gfx.frame3d.lod = ctx.scene3d.lod;
    let mut skins = std::mem::take(&mut ctx.gfx.frame3d.skins);
    skins.motion = ctx.gfx.frame3d.motion.enabled;
    skins.collect(&mut ctx.world, &ctx.gfx.assets3d);
    ctx.gfx.frame3d.shadow_views.clear();
    if shared_shadows && ctx.scene3d.lighting.shadows {
        let cameras = ctx.scene3d.views.iter().map(|v| v.camera);
        ctx.gfx.frame3d.shadow_views.extend(cameras);
    }
    ctx.gfx.frame3d.collect(
        &mut ctx.world,
        &ctx.gfx.assets3d,
        &skins,
        &ctx.scene3d.camera,
        &ctx.scene3d.lighting,
    );
    crate::three::api::collect_window_views(
        &mut ctx.world,
        &ctx.gfx.assets3d,
        &mut ctx.gfx.frame3d,
        &ctx.scene3d,
        &skins,
    );
    ctx.gfx.frame3d.skins = skins;
    let rgb = |c| {
        let [r, g, b, _] = linear(c);
        Vec3::new(r, g, b)
    };
    let main = Scene3D {
        frame: &ctx.gfx.frame3d,
        lighting: &ctx.scene3d.lighting,
        sky: &ctx.scene3d.sky,
        light: SkyLight {
            sun: rgb(ctx.scene3d.lighting.sun_color) * ctx.scene3d.lighting.sun_intensity,
            sky: rgb(ctx.scene3d.sky.sky_color),
            ground: rgb(ctx.scene3d.sky.ground_color),
        },
        camera: ctx.scene3d.camera.position,
        post: &ctx.scene3d.post,
        time: (0.0, 1.0 / 60.0),
        occlusion: false,
    };
    let views = ctx
        .scene3d
        .views
        .iter()
        .zip(&ctx.gfx.frame3d.window_views)
        .map(|(view, frame)| {
            let scene = Scene3D {
                frame,
                camera: view.camera.position,
                post: view.post.as_ref().unwrap_or(&ctx.scene3d.post),
                ..main
            };
            (scene, view.camera.rect, view.clear.unwrap_or(Color::BLACK))
        });
    let size = [stage.size.0, stage.size.1];
    windows.draw_3d(
        &stage.device,
        &stage.queue,
        &mut stage.renderer,
        stage.output.format(),
        size,
        (main, ctx.scene3d.camera.rect, Color::BLACK),
        views,
        &|_| None,
        &stage.textures,
        None,
    );
    let mut encoder = stage.device.create_command_encoder(&Default::default());
    let view = stage.output.create_view(&Default::default());
    assert!(windows.composite(&mut encoder, &view, size, Color::BLACK, None));
    stage.queue.submit([encoder.finish()]);
    for frame in &mut ctx.gfx.frame3d.window_views {
        frame.motion.reset = false;
    }
    ctx.gfx.frame3d.motion.reset = false;
    let image = Image::read(&stage.device, &stage.queue, &stage.output);
    keep_image(&image);
    image
}

/// `frames` frames, the last one read back.
fn settled(stage: &mut Stage, windows: &mut WindowViews, frames: usize) -> Image {
    for _ in 1..frames {
        frame(stage, windows, true);
    }
    frame(stage, windows, true)
}

/// Where `camera` (placed in the stage's window) sees `p`, in pixels.
fn seen(camera: &Camera3D, p: Vec3) -> (u32, u32) {
    let at = camera.world_to_screen(p).expect("in front of the camera");
    (at.x as u32, at.y as u32)
}

fn rgb(p: [u8; 4]) -> [u16; 3] {
    [p[0], p[1], p[2]].map(u16::from)
}

fn red(p: [u8; 4]) -> bool {
    let [r, g, b] = rgb(p);
    r > 60 && r > g * 3 && r > b * 3
}

fn green(p: [u8; 4]) -> bool {
    let [r, g, b] = rgb(p);
    g > 60 && g > r * 3 && g > b * 3
}

fn yellow(p: [u8; 4]) -> bool {
    let [r, g, b] = rgb(p);
    r > 80 && g > 80 && r > b * 3 && g > b * 3
}

/// A camera 2.4 up and 5 back from `target`, looking at it.
fn camera_at(stage: &Stage, target: Vec3, rect: Viewport) -> Camera3D {
    let mut camera = stage.ctx.scene3d.camera;
    camera.position = target + vec3(0.0, 2.4, 5.0);
    camera.look_at(target);
    camera.rect = rect;
    camera
}

/// A red box at x = -6 and a green one at x = +6 on a grey floor.
fn two_boxes(stage: &mut Stage) -> (Vec3, Vec3) {
    stage.floor();
    let red = stage.cube(Color::rgb(0.9, 0.1, 0.1));
    let green = stage.cube(Color::rgb(0.1, 0.9, 0.1));
    stage.block(red, vec3(-6.6, 0.0, -0.6), vec3(-5.4, 1.2, 0.6));
    stage.block(green, vec3(5.4, 0.0, -0.6), vec3(6.6, 1.2, 0.6));
    (vec3(-6.0, 0.6, 0.6), vec3(6.0, 0.6, 0.6))
}

#[test]
fn two_players_each_see_their_own_half_of_the_split_screen() {
    let Some(mut stage) = Stage::new() else { return };
    let (red_box, green_box) = two_boxes(&mut stage);
    stage.ctx.scene3d.camera = camera_at(&stage, red_box - Vec3::Y * 0.6, Viewport::split(2, 0));
    let p2 = camera_at(&stage, green_box - Vec3::Y * 0.6, Viewport::split(2, 1));
    stage.ctx.scene3d.views.push(d3::View::new(p2));
    let mut windows = WindowViews::default();
    let image = frame(&mut stage, &mut windows, true);

    // Each player's picture is drawn at its half's size, with its aspect.
    assert_eq!(stage.renderer.view_scene_size(0), Some((SIZE / 2, SIZE)));
    let (one, two) = (stage.ctx.scene3d.camera, stage.ctx.scene3d.views[0].camera);
    assert_eq!(one.screen_rect().size(), vec2(64.0, 128.0));
    assert_eq!(one.viewport(), vec2(64.0, 128.0));
    // The screen/world conversions follow each camera's half.
    let (rx, ry) = seen(&one, red_box);
    let (gx, gy) = seen(&two, green_box);
    assert!(rx < 64 && gx >= 64, "red at {rx}, green at {gx}");
    assert!(
        red(image.pixel(rx, ry)),
        "player one sees the red box at ({rx}, {ry}): {:?}",
        image.pixel(rx, ry)
    );
    assert!(
        green(image.pixel(gx, gy)),
        "player two sees the green box at ({gx}, {gy}): {:?}",
        image.pixel(gx, gy)
    );
    // Neither half shows the other player's box.
    assert_region(&image, 0..64, 0..SIZE, 1.0, "no green on the left", |p| !green(p));
    assert_region(&image, 64..SIZE, 0..SIZE, 1.0, "no red on the right", |p| !red(p));
    let ray = two.screen_ray(vec2(gx as f32 + 0.5, gy as f32 + 0.5));
    let hit = ray.0 + ray.1 * (ray.1.dot(green_box - ray.0));
    assert!(
        hit.distance(green_box) < 0.2,
        "the click on player two's half hits the box: {hit}"
    );

    // Negative control: player two turned to the red box sees red.
    stage.ctx.scene3d.views[0].camera = camera_at(&stage, red_box - Vec3::Y * 0.6, Viewport::split(2, 1));
    let turned = frame(&mut stage, &mut windows, true);
    assert!(
        turned.fraction(64..SIZE, 0..SIZE, red) > 0.02,
        "player two turned sees red"
    );
    assert_differs(&image, &turned, REFERENCE, "player two turned away");
    stage.ctx.scene3d.views[0].camera = p2;

    check_reference(
        "views_split_screen",
        &frame(&mut stage, &mut windows, true),
        REFERENCE,
        "Two-player split screen: a red box at x = -6 and a green box at x = +6 on a grey floor, the left half's \
         camera 2.4 up and 5 back from the red box, the right half's from the green one; white sun from the upper \
         left front, no shadows; 128x128 (two 64x128 pictures), 4x multisampling, no tonemapping, no bloom.",
        &stage.info,
    );
    reference_rejects(
        "views_split_screen",
        &turned,
        REFERENCE,
        "player two turned to the red box",
    );
    stage.finish();
}

#[test]
fn a_minimap_shows_icons_on_its_layer_the_main_camera_leaves_out() {
    let Some(mut stage) = Stage::new() else { return };
    let (red_box, _) = two_boxes(&mut stage);
    // The player's icon: a flat yellow marker over the red box, on layer 1.
    let marker = stage.cube(Color::rgb(0.95, 0.85, 0.1));
    let icon = stage.block(marker, vec3(-7.5, 1.5, -1.5), vec3(-4.5, 1.6, 1.5));
    stage.ctx.world.insert(icon, RenderLayers::layer(1));
    stage.ctx.scene3d.camera = camera_at(&stage, red_box - Vec3::Y * 0.6, Viewport::FULL);
    stage.ctx.scene3d.camera.layers = RenderLayers::ALL.without(1);
    // The map: straight down from high up, in the top-right corner.
    let mut map = stage.ctx.scene3d.camera;
    map.rect = Viewport::new(0.625, 0.0, 0.375, 0.375);
    map.position = vec3(0.0, 40.0, 0.0);
    // Straight down, north (-z) up.
    map.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    map.fov_y = 0.5;
    map.layers = RenderLayers::ALL;
    stage.ctx.scene3d.views.push(d3::View::new(map));
    let mut windows = WindowViews::default();
    let image = frame(&mut stage, &mut windows, true);

    let map = stage.ctx.scene3d.views[0].camera;
    let corner = map.screen_rect();
    assert_eq!((corner.min, corner.size()), (vec2(80.0, 0.0), vec2(48.0, 48.0)));
    let (ix, iy) = seen(&map, vec3(-6.0, 1.6, 0.0));
    assert!(
        corner.contains(vec2(ix as f32, iy as f32)),
        "the icon is in the corner: ({ix}, {iy})"
    );
    assert!(
        yellow(image.pixel(ix, iy)),
        "the map shows the icon: {:?}",
        image.pixel(ix, iy)
    );
    // The main camera leaves the icon out: no yellow outside the corner.
    assert_region(&image, 0..80, 0..SIZE, 1.0, "no icon left of the map", |p| !yellow(p));
    assert_region(&image, 80..SIZE, 48..SIZE, 1.0, "no icon below the map", |p| !yellow(p));
    // ...but sees the red box under it.
    let (rx, ry) = seen(&stage.ctx.scene3d.camera, red_box);
    assert!(
        red(image.pixel(rx, ry)),
        "the main camera sees the red box: {:?}",
        image.pixel(rx, ry)
    );

    // Negative control: the main camera on every layer sees the icon.
    stage.ctx.scene3d.camera.layers = RenderLayers::ALL;
    let all = frame(&mut stage, &mut windows, true);
    assert!(
        all.fraction(0..80, 0..SIZE, yellow) > 0.01,
        "the main camera on layer 1 sees the icon"
    );
    stage.ctx.scene3d.camera.layers = RenderLayers::ALL.without(1);

    check_reference(
        "views_minimap",
        &frame(&mut stage, &mut windows, true),
        REFERENCE,
        "A minimap: the red and green boxes on the grey floor seen from 2.4 up and 5 back of the red box, and in \
         the top-right corner (48x48 from x = 80) a camera 40 up looking straight down with -z up (fov 0.5) that also draws \
         a yellow marker above the red box on render layer 1, which the main camera leaves out; white sun, no \
         shadows; 128x128, 4x multisampling, no tonemapping, no bloom.",
        &stage.info,
    );
    reference_rejects("views_minimap", &all, REFERENCE, "the main camera on every layer");
    stage.finish();
}

#[test]
fn shadows_fall_in_a_view_far_from_the_main_camera() {
    let Some(mut stage) = Stage::new() else { return };
    // A long floor, a post far to the right casting a shadow on it.
    let grey = stage.cube(Color::rgb(0.6, 0.6, 0.6));
    stage.block(grey, vec3(-60.0, -1.0, -10.0), vec3(60.0, 0.0, 10.0));
    let post = stage.cube(Color::rgb(0.6, 0.6, 0.6));
    stage.block(post, vec3(39.5, 0.0, -0.5), vec3(40.5, 3.0, 0.5));
    stage.ctx.scene3d.lighting.shadows = true;
    stage.ctx.scene3d.lighting.shadow_distance = 20.0;
    stage.ctx.scene3d.camera = camera_at(&stage, vec3(-40.0, 0.0, 0.0), Viewport::split(2, 0));
    let far = camera_at(&stage, vec3(40.0, 0.0, 0.0), Viewport::split(2, 1));
    stage.ctx.scene3d.views.push(d3::View::new(far));
    let mut windows = WindowViews::default();
    // The sun shines toward -x and -z: the shadow falls to the left of the
    // post and behind it.
    let shade = vec3(38.8, 0.0, -1.0);
    let sunlit = vec3(41.0, 0.0, 1.0);
    let shared = frame(&mut stage, &mut windows, true);
    let far = stage.ctx.scene3d.views[0].camera;
    let (sx, sy) = seen(&far, shade);
    let (lx, ly) = seen(&far, sunlit);
    let (dark, lit) = (mean_color(&shared, sx, sy, 1), mean_color(&shared, lx, ly, 1));
    assert!(sx >= 64 && lx >= 64, "both points in the right half: {sx}, {lx}");
    assert!(
        luma(dark) < luma(lit) * 0.6,
        "the post's shadow falls in the far view: {dark:?} vs {lit:?}"
    );

    // Negative control: cascades fitted to the main camera alone leave the
    // far view unshadowed.
    let alone = frame(&mut stage, &mut windows, false);
    let unshaded = mean_color(&alone, sx, sy, 1);
    assert!(
        luma(unshaded) > luma(lit) * 0.85,
        "without sharing, no shadow: {unshaded:?} vs {lit:?}"
    );
    stage.finish();
}

#[test]
fn a_spot_lights_shadow_falls_in_a_view_far_from_the_main_camera() {
    let Some(mut stage) = Stage::new() else { return };
    // A long floor; far to the right a box under a spot light, seen by the
    // right half's camera only; near the main camera another shadowed spot
    // light, a little farther from it. One shadowed light fits the budget.
    let grey = stage.cube(Color::rgb(0.6, 0.6, 0.6));
    stage.block(grey, vec3(-60.0, -1.0, -20.0), vec3(60.0, 0.0, 20.0));
    let lid = stage.cube(Color::rgb(0.6, 0.6, 0.6));
    stage.block(lid, vec3(39.5, 2.5, -0.5), vec3(40.5, 3.5, 0.5));
    let spot = |at: Vec3| {
        (
            d3::Transform::at(at).looking_at(vec3(at.x, 0.0, at.z + 0.001)),
            d3::SpotLight {
                intensity: 400.0,
                range: 20.0,
                inner_angle: 0.5,
                outer_angle: 0.7,
                shadows: true,
                ..Default::default()
            },
        )
    };
    stage.ctx.world.spawn(spot(vec3(40.0, 6.0, 0.0)));
    stage.ctx.world.spawn(spot(vec3(-40.0, 6.0, -10.0)));
    let lighting = &mut stage.ctx.scene3d.lighting;
    lighting.shadows = true;
    lighting.sun_intensity = 0.0;
    lighting.light_shadow_views = 1;
    stage.ctx.scene3d.camera = camera_at(&stage, vec3(-40.0, 0.0, 0.0), Viewport::split(2, 0));
    let far = camera_at(&stage, vec3(40.0, 0.0, 0.0), Viewport::split(2, 1));
    stage.ctx.scene3d.views.push(d3::View::new(far));
    let mut windows = WindowViews::default();
    // Straight under the box, and 1.8 toward the camera (in the cone, past
    // the box's shadow).
    let (shade, lit) = (vec3(40.0, 0.0, 0.0), vec3(40.0, 0.0, 1.8));
    let shared = frame(&mut stage, &mut windows, true);
    let far = stage.ctx.scene3d.views[0].camera;
    let ((sx, sy), (lx, ly)) = (seen(&far, shade), seen(&far, lit));
    assert!(
        (64..SIZE).contains(&sx) && (64..SIZE).contains(&lx) && sy < SIZE && ly < SIZE,
        "both points in the right half: ({sx}, {sy}), ({lx}, {ly})"
    );
    let (dark, bright) = (mean_color(&shared, sx, sy, 1), mean_color(&shared, lx, ly, 1));
    assert!(luma(bright) > 0.2, "the spot light lights the far view: {bright:?}");
    assert!(
        luma(dark) < luma(bright) * 0.5,
        "the box's shadow falls in the far view: {dark:?} vs {bright:?}"
    );

    // Negative control: shadows planned for the main camera alone give the
    // one shadow to the light near it, and the far view's light casts none.
    let alone = frame(&mut stage, &mut windows, false);
    let (unshaded, bright) = (mean_color(&alone, sx, sy, 1), mean_color(&alone, lx, ly, 1));
    assert!(
        luma(unshaded) > luma(bright) * 0.8,
        "planned for the main camera only, no shadow: {unshaded:?} vs {bright:?}"
    );
    stage.finish();
}

#[test]
fn each_view_keeps_its_own_temporal_history() {
    let Some(mut stage) = Stage::new() else { return };
    let (red_box, green_box) = two_boxes(&mut stage);
    stage.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let (left, right) = (Viewport::split(2, 0), Viewport::split(2, 1));
    let greens = |stage: &Stage, rect| camera_at(stage, green_box - Vec3::Y * 0.6, rect);
    // The green box in the right half, the red one's camera in the left.
    stage.ctx.scene3d.camera = camera_at(&stage, red_box - Vec3::Y * 0.6, left);
    stage.ctx.scene3d.views.push(d3::View::new(greens(&stage, right)));
    let mut windows = WindowViews::default();
    let first = frame(&mut stage, &mut windows, true);
    let together = settled(&mut stage, &mut windows, 12);
    let half = |image: &Image, x0: u32| Image {
        width: 64,
        height: SIZE,
        rgba: (0..SIZE)
            .flat_map(|y| (x0..x0 + 64).flat_map(move |x| image.pixel(x, y)))
            .collect(),
    };

    // The same green camera drawn alone, as the main one (in the left half).
    let Some(mut alone) = Stage::new() else { return };
    two_boxes(&mut alone);
    alone.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    alone.ctx.scene3d.camera = greens(&alone, left);
    let mut windows_alone = WindowViews::default();
    let by_itself = settled(&mut alone, &mut windows_alone, 12);
    // A view's history is its own: settled beside another camera, it is the
    // picture it settles to alone (a history mixed with the other camera's
    // would show red ghosts or smear).
    assert_similar(
        &half(&together, 64),
        &half(&by_itself, 0),
        SAME_PICTURE,
        "the view settles as alone",
    );
    assert_region(&together, 64..SIZE, 0..SIZE, 1.0, "no red ghost in the view", |p| {
        !red(p)
    });
    // Negative control: the history is there to keep (the settled picture
    // differs from the first, unaccumulated one).
    assert_differs(
        &half(&together, 64),
        &half(&first, 64),
        Tolerance {
            channel: 2,
            outliers: 0.0,
            edge: 0,
        },
        "the view's history accumulates",
    );
    alone.finish();
    stage.finish();
}

fn blue(p: [u8; 4]) -> bool {
    let [r, g, b] = rgb(p);
    b > 60 && b > r * 3 && b > g * 3
}

fn magenta(p: [u8; 4]) -> bool {
    let [r, g, b] = rgb(p);
    r > 60 && b > 60 && r > g * 3 && b > g * 3
}

#[test]
fn debug_lines_show_in_every_view_on_their_layer() {
    let Some(mut stage) = Stage::new() else { return };
    let (red_box, green_box) = two_boxes(&mut stage);
    stage.ctx.scene3d.camera = camera_at(&stage, red_box - Vec3::Y * 0.6, Viewport::split(2, 0));
    let p2 = camera_at(&stage, green_box - Vec3::Y * 0.6, Viewport::split(2, 1));
    stage.ctx.scene3d.views.push(d3::View::new(p2));
    // In front of the green box (only player two sees it): a blue
    // depth-tested line, and a magenta one behind the box but on top.
    let debug = &mut stage.ctx.debug;
    debug.set_enabled(true);
    let (a, b) = (green_box + vec3(-0.9, 0.9, 0.4), green_box + vec3(0.9, 0.9, 0.4));
    debug.line(a, b, Color::BLUE).width(4.0);
    let (c, d) = (green_box + vec3(-0.9, -0.2, -1.5), green_box + vec3(0.9, -0.2, -1.5));
    debug.line(c, d, Color::rgb(1.0, 0.0, 1.0)).width(4.0).on_top();
    let mut windows = WindowViews::default();
    let mut drawn = |stage: &mut Stage| {
        stage.ctx.place_cameras();
        stage.ctx.gfx.frame3d.debug_lines.clear();
        stage.ctx.hand_over_debug_draw();
        frame(stage, &mut windows, true)
    };
    let image = drawn(&mut stage);
    let (bx, by) = seen(&p2, (a + b) * 0.5);
    let (mx, my) = seen(&p2, (c + d) * 0.5);
    assert!(bx >= 64 && mx >= 64, "the lines are in player two's half: {bx}, {mx}");
    let near = |image: &Image, (x, y): (u32, u32), is: fn([u8; 4]) -> bool| {
        image.fraction(
            x.saturating_sub(3)..(x + 4).min(SIZE),
            y.saturating_sub(3)..(y + 4).min(SIZE),
            is,
        ) > 0.1
    };
    assert!(near(&image, (bx, by), blue), "player two sees the depth-tested line");
    assert!(
        near(&image, (mx, my), magenta),
        "player two sees the line on top through the box"
    );
    assert_region(&image, 0..64, 0..SIZE, 1.0, "no line in player one's half", |p| {
        !blue(p) && !magenta(p)
    });

    // Negative control: player two's camera off the default layer draws
    // neither line.
    stage.ctx.scene3d.views[0].camera.layers = RenderLayers::ALL.without(0);
    let off = drawn(&mut stage);
    assert_region(&off, 64..SIZE, 0..SIZE, 1.0, "no line off its layer", |p| {
        !blue(p) && !magenta(p)
    });
    stage.finish();
}
