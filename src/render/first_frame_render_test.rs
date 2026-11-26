//! The first frame of a game draws its 3D scene: in the window and in
//! every target camera, also one added later in the game, and also the
//! frame after the window's surface changed (shown, hidden, recreated).
//! Each test has its negative control. Skips without a GPU.
use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::prelude::*;
use crate::render::surface_lifecycle::SurfaceEvent;
use crate::render::{MaterialDesc, MeshData, Model, RenderTarget, TargetCamera};

/// The window.
const SIZE: (u32, u32) = (64, 64);
/// The target cameras' targets.
const TARGET: u32 = 32;

/// A game with a blue background and no sky, a green glowing box at the
/// origin and the camera looking at it from the front. No frame drawn yet.
fn game() -> Option<crate::ctx::Ctx> {
    let mut ctx = offscreen_ctx(SIZE)?;
    ctx.scene3d.sky.atmosphere = false;
    ctx.gfx.clear_color = Color::rgb(0.0, 0.0, 1.0);
    let material = ctx
        .gfx
        .create_material(MaterialDesc::color(Color::BLACK).with_emissive(Color::rgb(0.0, 1.0, 0.0)));
    let green = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
    ctx.world.spawn((crate::d3::Transform::default(), Model::new(green)));
    ctx.scene3d.camera.position = vec3(0.0, 0.0, 5.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    Some(ctx)
}

/// A target camera at the main camera's place, looking at `at`.
fn target_camera(ctx: &mut crate::ctx::Ctx, at: Vec3) -> RenderTarget {
    let target = ctx.gfx.create_render_target(TARGET, TARGET);
    let mut eye = ctx.scene3d.camera;
    eye.look_at(at);
    ctx.scene3d.passes.target_cameras.push(TargetCamera::new(eye, target));
    target
}

/// Draws a frame; returns the window as RGBA8.
fn frame(ctx: &mut crate::ctx::Ctx) -> Image {
    let path = std::env::temp_dir().join(format!(
        "ponko-first-frame-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    assert_eq!(ctx.render_frame(), crate::render::FrameStatus::Offscreen);
    let shot = image::open(&path).expect("the frame was captured").to_rgba8();
    let _ = std::fs::remove_file(&path);
    let window = Image {
        width: shot.width(),
        height: shot.height(),
        rgba: shot.into_raw(),
    };
    keep_image(&window);
    window
}

/// Texels of `target` whose light is mostly green.
fn green_texels(ctx: &crate::ctx::Ctx, target: RenderTarget) -> usize {
    let renderer = ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    let (bytes, _, _) = renderer
        .read_texture_level(target.texture().id, 0)
        .expect("the target is on the GPU");
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|p| std::array::from_fn::<f32, 4, _>(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
        .filter(|c| c[1] > 0.4 && c[0] < 0.2 && c[2] < 0.2)
        .count()
}

/// Pixels of the window that are clearly `channel` (0 red, 1 green, 2 blue).
fn count(image: &Image, channel: usize) -> usize {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[channel] > 150 && (0..3).all(|i| i == channel || p[i] < 60))
        .count()
}

/// The window shows the green box on the blue background.
fn assert_drawn(window: &Image, when: &str) {
    assert!(count(window, 1) > 50, "{when}: the box is drawn ({})", count(window, 1));
    // Before the fix the whole frame was left uncleared: (0, 0, 0, 0).
    assert_eq!(window.pixel(0, 0), [0, 0, 255, 255], "{when}: the background is drawn");
}

#[test]
fn the_first_frame_draws_the_3d_scene_in_the_window_and_target_cameras() {
    let Some(mut ctx) = game() else { return };
    let seeing = target_camera(&mut ctx, Vec3::ZERO);
    // Negative control: a target camera looking away from the box.
    let away = target_camera(&mut ctx, vec3(0.0, 0.0, 10.0));
    let window = frame(&mut ctx);
    assert_drawn(&window, "first frame");
    assert!(green_texels(&ctx, seeing) > 10, "the target camera draws the box");
    assert_eq!(green_texels(&ctx, away), 0, "the one looking away doesn't");
    // The second frame draws the same picture.
    assert_eq!(frame(&mut ctx).rgba, window.rgba, "the same as the second frame");
}

#[test]
fn a_target_camera_added_during_the_game_draws_in_its_first_frame() {
    let Some(mut ctx) = game() else { return };
    for _ in 0..3 {
        frame(&mut ctx);
    }
    let seeing = target_camera(&mut ctx, Vec3::ZERO);
    let away = target_camera(&mut ctx, vec3(0.0, 0.0, 10.0));
    assert_drawn(&frame(&mut ctx), "with new target cameras");
    assert!(green_texels(&ctx, seeing) > 10, "the new target camera draws the box");
    assert_eq!(green_texels(&ctx, away), 0, "the one looking away doesn't");
}

#[test]
fn the_frame_after_a_surface_change_draws_its_3d_scene() {
    let Some(mut ctx) = game() else { return };
    let seeing = target_camera(&mut ctx, Vec3::ZERO);
    assert_drawn(&frame(&mut ctx), "first frame");
    // The window shown (this frame is drawn offscreen again: hidden once
    // more, a change heard while drawing it), recreated, suspended and
    // resumed: each frame after the change draws, and the one after that.
    for event in [
        SurfaceEvent::Occluded(false),
        SurfaceEvent::Recreated,
        SurfaceEvent::Suspended(true),
        SurfaceEvent::Suspended(false),
    ] {
        let renderer = ctx.gfx.renderer.as_mut().expect("an offscreen renderer");
        renderer.surface_event(event);
        assert_drawn(&frame(&mut ctx), &format!("after {event:?}"));
        assert!(
            green_texels(&ctx, seeing) > 10,
            "after {event:?}: the target camera draws"
        );
        assert_drawn(&frame(&mut ctx), &format!("the frame after {event:?}'s"));
    }
}
