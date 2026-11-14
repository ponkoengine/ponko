//! Split toning and depth of field, drawn offscreen: split toning tints the
//! dark and the bright parts of the frame apart (and a tint for one leaves
//! the other alone), and depth of field blurs what is out of focus while
//! what is in focus keeps its edges (and with no blur, or switched off, the
//! frame is the plain one). Semantic checks, each against the same scene
//! without the effect; no reference images.
//!
//! Skips without a GPU adapter.

use super::*;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::render_test_support::{self as support, Image};
use crate::render::static_batch_render_test::{BatchRig, RigSetup};
use crate::render::{AntiAliasing, MaterialDesc, Model};

/// A material that glows `level` grey whatever the light.
fn glow(level: f32) -> MaterialDesc {
    MaterialDesc {
        color: Color::BLACK,
        emissive: Color::rgb(level, level, level),
        ..Default::default()
    }
}

/// A rig whose camera stands at the origin looking down -Z, with nothing
/// else in the post chain (no bloom, no antialiasing).
fn rig() -> Option<BatchRig> {
    let mut rig = BatchRig::new(RigSetup::default())?;
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.rotation = Quat::IDENTITY;
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    Some(rig)
}

fn cube(rig: &mut BatchRig, size: f32, level: f32, at: Vec3) {
    let mesh = rig.mesh(&MeshData::cube(size), glow(level));
    rig.ctx.world.spawn((Transform3D::at(at), Model::new(mesh)));
}

fn average(image: &Image, x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for py in y {
        for px in x.clone() {
            let p = image.pixel(px, py);
            for c in 0..3 {
                sum[c] += f32::from(p[c]);
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

#[test]
fn split_toning_tints_shadows_and_highlights_apart() {
    let Some(mut rig) = rig() else { return };
    // A dim cube on the left, a bright one on the right.
    cube(&mut rig, 1.0, 0.12, vec3(-1.0, 0.0, -4.0));
    cube(&mut rig, 1.0, 1.0, vec3(1.0, 0.0, -4.0));
    let (dark, bright) = ((34..44, 60..68), (84..94, 60..68));
    let look = |image: &Image| {
        (
            average(image, dark.0.clone(), dark.1.clone()),
            average(image, bright.0.clone(), bright.1.clone()),
        )
    };
    let plain = rig.image();
    let (plain_dark, plain_bright) = look(&plain);
    assert!(
        plain_dark[0] > 10.0 && plain_bright[0] > plain_dark[0] + 100.0,
        "the scene: a dim cube {plain_dark:?} and a bright one {plain_bright:?}"
    );
    let blue = Color::rgb(0.3, 0.4, 0.8);
    let orange = Color::rgb(0.8, 0.5, 0.3);

    // Both tints: the dim cube turns blue, the bright one orange.
    rig.ctx.scene3d.post.split_toning.shadows = blue;
    rig.ctx.scene3d.post.split_toning.highlights = orange;
    let (dark_toned, bright_toned) = look(&rig.image());
    let blueness = |c: [f32; 3]| c[2] - c[0];
    assert!(
        blueness(dark_toned) > blueness(plain_dark) + 8.0,
        "shadows take the blue tint: {plain_dark:?} -> {dark_toned:?}"
    );
    assert!(
        -blueness(bright_toned) > -blueness(plain_bright) + 8.0,
        "highlights take the orange tint: {plain_bright:?} -> {bright_toned:?}"
    );

    // Only a shadow tint: it lands mostly on the dim cube. Only a
    // highlight tint: mostly on the bright one.
    let change = |a: [f32; 3], b: [f32; 3]| (0..3).map(|c| (a[c] - b[c]).abs()).sum::<f32>();
    rig.ctx.scene3d.post.split_toning.highlights = Color::GRAY;
    let (dark_only, bright_kept) = look(&rig.image());
    assert!(blueness(dark_only) > blueness(plain_dark) + 8.0, "{dark_only:?}");
    assert!(
        change(dark_only, plain_dark) > 2.0 * change(bright_kept, plain_bright),
        "a shadow tint is mostly in the shadows: {plain_dark:?} -> {dark_only:?}, {plain_bright:?} -> {bright_kept:?}"
    );
    rig.ctx.scene3d.post.split_toning.shadows = Color::GRAY;
    rig.ctx.scene3d.post.split_toning.highlights = orange;
    let (dark_kept, bright_only) = look(&rig.image());
    assert!(
        -blueness(bright_only) > -blueness(plain_bright) + 8.0,
        "{bright_only:?}"
    );
    assert!(
        change(bright_only, plain_bright) > 2.0 * change(dark_kept, plain_dark),
        "a highlight tint is mostly in the highlights: {plain_bright:?} -> {bright_only:?}, {plain_dark:?} -> {dark_kept:?}"
    );

    // Both grey again: exactly the plain frame.
    rig.ctx.scene3d.post.split_toning.highlights = Color::GRAY;
    assert_eq!(
        support::diff(&plain, &rig.image(), 0).max_channel,
        0,
        "neutral tints change nothing"
    );
}

/// Pixels in the rectangle that are neither background nor the glowing
/// cube's full color: the soft edge. Blur widens it.
fn soft(image: &Image, x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> usize {
    let mut count = 0;
    for py in y {
        for px in x.clone() {
            count += usize::from((25..=190).contains(&level(image, px, py)));
        }
    }
    count
}

fn level(image: &Image, x: u32, y: u32) -> u32 {
    let [r, g, b, _] = image.pixel(x, y);
    (u32::from(r) + u32::from(g) + u32::from(b)) / 3
}

/// Of the pixels in the rectangle that show the cube in full in `plain`,
/// the share that still do (within 4 levels) in `image`: 1 for a sharp
/// cube, less as blur eats into its edges.
fn kept(plain: &Image, image: &Image, x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> f32 {
    let (mut cube, mut still) = (0, 0);
    for py in y {
        for px in x.clone() {
            if level(plain, px, py) >= 200 {
                cube += 1;
                still += usize::from(level(image, px, py).abs_diff(level(plain, px, py)) <= 4);
            }
        }
    }
    assert!(cube > 50, "the cube is in the rectangle: {cube} pixels");
    still as f32 / cube as f32
}

#[test]
fn depth_of_field_blurs_what_is_out_of_focus_and_keeps_the_rest() {
    let Some(mut rig) = rig() else { return };
    // A small cube close by on the left, a big one far away on the right:
    // about the same size on screen.
    cube(&mut rig, 0.5, 1.0, vec3(-0.6, 0.0, -3.0));
    cube(&mut rig, 4.0, 1.0, vec3(8.0, 0.0, -30.0));
    let (near_x, far_x, y) = (18..62, 74..120, 42..86);
    let plain = rig.image();
    let (near_soft, far_soft) = (
        soft(&plain, near_x.clone(), y.clone()),
        soft(&plain, far_x.clone(), y.clone()),
    );
    let plain_calls = rig.renderer.draw_calls();

    // Focused on the near cube: the far one blurs (its edge spreads and
    // eats into it), the near one stays whole.
    let dof = &mut rig.ctx.scene3d.post.depth_of_field;
    dof.enabled = true;
    dof.focus_distance = 3.0;
    dof.focus_range = 0.5;
    dof.blur = 0.05;
    let image = rig.image();
    assert_eq!(rig.renderer.draw_calls(), plain_calls + 1, "one more full-screen pass");
    let features = last_features().expect("the frame was checked");
    assert!(features.contains(FrameFeatures::DOF));
    let far_blurred = soft(&image, far_x.clone(), y.clone());
    assert!(
        far_blurred > far_soft * 2 + 20,
        "the far cube's edge spreads: {far_soft} -> {far_blurred} soft pixels"
    );
    let (near_kept, far_kept) = (
        kept(&plain, &image, near_x.clone(), y.clone()),
        kept(&plain, &image, far_x.clone(), y.clone()),
    );
    assert!(near_kept >= 0.97, "the near cube stays whole: {near_kept}");
    assert!(far_kept < 0.85, "blur eats into the far cube: {far_kept}");

    // Focused on the far cube: the other way round.
    rig.ctx.scene3d.post.depth_of_field.focus_distance = 28.0;
    rig.ctx.scene3d.post.depth_of_field.focus_range = 4.0;
    let image = rig.image();
    let near_blurred = soft(&image, near_x.clone(), y.clone());
    assert!(
        near_blurred > near_soft * 2 + 20,
        "the near cube's edge spreads: {near_soft} -> {near_blurred} soft pixels"
    );
    let (near_kept, far_kept) = (
        kept(&plain, &image, near_x.clone(), y.clone()),
        kept(&plain, &image, far_x.clone(), y.clone()),
    );
    assert!(far_kept >= 0.97, "the far cube stays whole: {far_kept}");
    assert!(near_kept < 0.85, "blur eats into the near cube: {near_kept}");

    // No blur, or switched off: the plain frame, and no pass.
    rig.ctx.scene3d.post.depth_of_field.blur = 0.0;
    assert_eq!(
        support::diff(&plain, &rig.image(), 0).max_channel,
        0,
        "no blur changes nothing"
    );
    assert_eq!(rig.renderer.draw_calls(), plain_calls);
    assert!(!last_features().unwrap().contains(FrameFeatures::DOF));
    rig.ctx.scene3d.post.depth_of_field.blur = 0.05;
    rig.ctx.scene3d.post.depth_of_field.enabled = false;
    assert_eq!(
        support::diff(&plain, &rig.image(), 0).max_channel,
        0,
        "off changes nothing"
    );
}

#[test]
fn depth_of_field_reads_the_scene_depth_as_a_float_texture_where_it_must() {
    // The GL backend's path (see `shaders::reads_depth_as_float`), on this
    // machine's GPU: the same blur.
    FORCE_FLOAT_DEPTH.set(true);
    let rig = rig();
    FORCE_FLOAT_DEPTH.set(false);
    let Some(mut rig) = rig else { return };
    cube(&mut rig, 4.0, 1.0, vec3(8.0, 0.0, -30.0));
    let far = |image: &Image| soft(image, 74..120, 42..86);
    let plain = far(&rig.image());
    let dof = &mut rig.ctx.scene3d.post.depth_of_field;
    dof.enabled = true;
    dof.focus_distance = 3.0;
    dof.blur = 0.05;
    let blurred = far(&rig.image());
    assert!(blurred > plain * 2 + 20, "{plain} -> {blurred} soft pixels");
}

/// The GPU time of the depth of field pass at 1920x1080: the cost the
/// `DepthOfField` docs state. (Split toning adds no pass: a few
/// instructions per pixel of the tone mapping pass.) A pass's time doesn't
/// depend on how the Rust was compiled, so this runs in the test profile:
/// `cargo test --lib --features 3d post_effects_cost -- --ignored --nocapture`
#[test]
#[ignore = "requires a GPU with timestamps; run explicitly for numbers"]
fn post_effects_cost() {
    let Some(mut rig) = BatchRig::new(RigSetup {
        size: (1920, 1080),
        timestamps: true,
        ..Default::default()
    }) else {
        return;
    };
    if rig.timer.is_none() {
        support::skip("no timestamp queries");
        return;
    }
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.rotation = Quat::IDENTITY;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    // Bloom's many passes would use up the timer's queries.
    rig.ctx.scene3d.post.bloom = 0.0;
    for i in 0..60 {
        let at = vec3((i % 10) as f32 - 4.5, (i / 10) as f32 - 2.5, -3.0 - i as f32 * 0.5);
        cube(&mut rig, 0.6, 0.2 + (i % 5) as f32 * 0.2, at);
    }
    println!("adapter: {}", support::describe_adapter(&rig.gpu.info));
    let measure = |rig: &mut BatchRig, pass: &str| {
        let mut times = Vec::new();
        let mut read = rig.timer.as_ref().unwrap().frames_read;
        for frame in 0..150 {
            rig.frame();
            let timer = rig.timer.as_ref().unwrap();
            if frame >= 30 && timer.frames_read != read {
                read = timer.frames_read;
                let ms: f32 = timer.last_frame.iter().filter(|p| p.name == pass).map(|p| p.ms).sum();
                times.push(ms);
            }
        }
        times.sort_by(f32::total_cmp);
        times[times.len() / 2]
    };
    // Other work on a shared machine only adds time: the best of a few
    // runs' medians.
    let measure = |rig: &mut BatchRig, pass: &str| (0..4).map(|_| measure(rig, pass)).fold(f32::MAX, f32::min);
    for blur in [0.01, 0.05] {
        let dof = &mut rig.ctx.scene3d.post.depth_of_field;
        dof.enabled = true;
        dof.focus_distance = 6.0;
        dof.blur = blur;
        let ms = measure(&mut rig, "depth of field");
        println!("depth of field pass, blur {blur}: {ms:.3} ms");
    }
}
