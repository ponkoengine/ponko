//! Target cameras' own culling box, background and screen effects, and
//! mipmapped render targets, through a game's own frame
//! (`Ctx::render_frame` with the offscreen renderer). Each test has its
//! negative control: the same scene without the setting. Skips without a
//! GPU.
use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::prelude::*;
use crate::render::{MaterialDesc, MeshData, Model, RenderTarget, ScreenEffect, TargetCamera};

/// The window and the render target.
const SIZE: (u32, u32) = (64, 64);

/// A game with a black background and no sky, a red box left of the
/// origin and a green one right of it, and a target camera looking at
/// both from the front (as the main camera does).
struct Scene {
    ctx: crate::ctx::Ctx,
    target: RenderTarget,
}

impl Scene {
    fn new(mipmapped: bool) -> Option<Self> {
        let mut ctx = offscreen_ctx(SIZE)?;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = false;
        ctx.gfx.clear_color = Color::BLACK;
        let glowing = |ctx: &mut crate::ctx::Ctx, color| {
            let material = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::BLACK).with_emissive(color));
            ctx.gfx.create_mesh(&MeshData::cube(1.0), material)
        };
        let red = glowing(&mut ctx, Color::rgb(1.0, 0.0, 0.0));
        let green = glowing(&mut ctx, Color::rgb(0.0, 1.0, 0.0));
        ctx.world
            .spawn((crate::d3::Transform::at(vec3(-1.0, 0.0, 0.0)), Model::new(red)));
        ctx.world
            .spawn((crate::d3::Transform::at(vec3(1.0, 0.0, 0.0)), Model::new(green)));
        let target = if mipmapped {
            ctx.gfx.create_mipmapped_render_target(SIZE.0, SIZE.1)
        } else {
            ctx.gfx.create_render_target(SIZE.0, SIZE.1)
        };
        let mut camera = ctx.scene3d.camera;
        camera.position = vec3(0.0, 0.0, 5.0);
        camera.look_at(Vec3::ZERO);
        ctx.scene3d
            .passes
            .target_cameras
            .push(TargetCamera::new(camera, target));
        ctx.scene3d.camera = camera;
        let mut scene = Self { ctx, target };
        // The meshes reach the GPU with the first frame drawn.
        scene.ctx.render_frame();
        Some(scene)
    }

    fn camera(&mut self) -> &mut TargetCamera {
        &mut self.ctx.scene3d.passes.target_cameras[0]
    }

    /// Draws a frame; returns the window as RGBA8.
    fn frame(&mut self) -> Image {
        let path = std::env::temp_dir().join(format!(
            "ponko-target-camera-{}-{:?}.png",
            std::process::id(),
            std::thread::current().id()
        ));
        self.ctx.gfx.queue_screenshot(path.clone(), 0, None);
        assert_eq!(self.ctx.render_frame(), crate::render::FrameStatus::Offscreen);
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

    /// Mip level `level` of the render target: linear RGBA per texel, with
    /// its width.
    fn level(&self, level: u32) -> (Vec<[f32; 4]>, u32) {
        let renderer = self.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
        let (bytes, width, height) = renderer
            .read_texture_level(self.target.texture().id, level)
            .expect("the render target has that level");
        let texels: Vec<[f32; 4]> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
            .collect();
        let encode = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        keep_image(&Image {
            width,
            height,
            rgba: texels
                .iter()
                .flat_map(|c| [encode(c[0]), encode(c[1]), encode(c[2]), 255])
                .collect(),
        });
        (texels, width)
    }

    /// The render target's first level.
    fn target(&self) -> Vec<[f32; 4]> {
        self.level(0).0
    }
}

/// Texels whose light is mostly `channel` (0 red, 1 green, 2 blue).
fn count(texels: &[[f32; 4]], channel: usize) -> usize {
    texels
        .iter()
        .filter(|c| c[channel] > 0.5 && (0..3).all(|i| i == channel || c[i] < 0.2))
        .count()
}

/// Pixels of an RGBA8 image that are clearly `channel`.
fn count8(image: &Image, channel: usize) -> usize {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[channel] > 150 && (0..3).all(|i| i == channel || p[i] < 60))
        .count()
}

#[test]
fn a_cull_box_keeps_only_the_models_that_reach_into_it() {
    let Some(mut scene) = Scene::new(false) else { return };
    // Negative control: without a box the camera draws both boxes.
    scene.frame();
    let all = scene.target();
    assert!(count(&all, 0) > 50 && count(&all, 1) > 50, "both boxes drawn");
    // A box around the red one only (the green one is a metre away).
    let camera = scene.camera().clone();
    *scene.camera() = camera.with_cull_box(vec3(-2.0, -1.0, -1.0), vec3(-0.6, 1.0, 1.0));
    let window = scene.frame();
    let boxed = scene.target();
    assert!(count(&boxed, 0) > 50, "the red box, inside it, is drawn");
    assert_eq!(count(&boxed, 1), 0, "the green box, outside it, is culled");
    // The main view has no box: both still on screen.
    assert!(
        count8(&window, 0) > 50 && count8(&window, 1) > 50,
        "the main view draws both"
    );
    // A model that only reaches into the box (its bounds overlap it) is kept.
    scene.camera().cull_box = Some((vec3(0.9, -1.0, -1.0), vec3(1.0, 1.0, 1.0)));
    scene.frame();
    let edge = scene.target();
    assert!(
        count(&edge, 1) > 50 && count(&edge, 0) == 0,
        "only the green box touches it"
    );
}

#[test]
fn a_target_camera_clears_to_its_own_colour_instead_of_the_frames_and_the_sky() {
    let Some(mut scene) = Scene::new(false) else { return };
    // Negative control: the frame's black clear colour behind the boxes.
    scene.frame();
    let corner = scene.target()[0];
    assert!(corner[..3].iter().all(|&c| c < 0.05), "black behind: {corner:?}");
    scene.camera().clear = Some(Color::rgb(0.0, 0.0, 1.0));
    let window = scene.frame();
    let own = scene.target();
    assert!(own[0][2] > 0.9 && own[0][0] < 0.05, "its own blue: {:?}", own[0]);
    assert!(count(&own, 0) > 50, "the boxes are drawn over it");
    assert_eq!(count8(&window, 2), 0, "the main view keeps its black");
    // The sky: drawn behind the boxes without its own colour, replaced
    // with one.
    scene.ctx.scene3d.sky.atmosphere = true;
    scene.camera().clear = None;
    scene.frame();
    let sky = scene.target()[0];
    assert!(sky[..3].iter().any(|&c| c > 0.05), "the sky behind: {sky:?}");
    // Transparent, for an icon over a UI: alpha 0 where nothing is drawn.
    scene.camera().clear = Some(Color::TRANSPARENT);
    scene.frame();
    let icon = scene.target();
    assert!(icon[0].iter().all(|&c| c.abs() < 1e-3), "see-through: {:?}", icon[0]);
    let center = icon[(SIZE.1 / 2 * SIZE.0 + SIZE.0 / 2 - 12) as usize];
    assert!(center[3] > 0.99 && center[0] > 0.5, "the red box is opaque: {center:?}");
}

/// Red becomes green, green blue, blue red.
const ROTATE: &str = "fn effect(p: Pixel) -> vec3<f32> { return p.color.brg; }";
/// Where nothing solid is (read from the camera's depth): yellow.
const SKY_YELLOW: &str =
    "fn effect(p: Pixel) -> vec3<f32> { return select(p.color, vec3<f32>(1.0, 1.0, 0.0), p.sky); }";

#[test]
fn a_target_cameras_effects_run_over_its_picture_and_not_the_main_views() {
    let Some(mut scene) = Scene::new(false) else { return };
    let rotate = scene.ctx.gfx.create_effect("rotate", ROTATE);
    let sky = scene.ctx.gfx.create_effect("sky yellow", SKY_YELLOW);
    // The main view's effect doesn't reach the target camera.
    scene.ctx.scene3d.passes.effects.push(ScreenEffect::new(rotate));
    let window = scene.frame();
    let plain = scene.target();
    assert!(count(&plain, 0) > 50 && count(&plain, 2) == 0, "the target is as drawn");
    assert!(
        count8(&window, 1) > 50 && count8(&window, 0) == 0,
        "the window's red became green"
    );
    scene.ctx.scene3d.passes.effects.clear();
    // One effect (into the scratch, copied back): red becomes green.
    scene.camera().effects.push(ScreenEffect::new(rotate));
    let window = scene.frame();
    let once = scene.target();
    assert!(
        count(&once, 1) > 100 && count(&once, 0) == 0,
        "red and green became green and blue"
    );
    assert!(count(&once, 2) > 50, "green became blue");
    assert!(count8(&window, 0) > 50, "the main view keeps its red");
    // Two (back into the target): red becomes blue.
    scene.camera().effects.push(ScreenEffect::new(rotate));
    scene.frame();
    let twice = scene.target();
    assert!(
        count(&twice, 2) > 50 && count(&twice, 0) > 50 && count(&twice, 1) == 0,
        "red blue, green red"
    );
    // Three: the last reads the camera's depth, painting where nothing is.
    scene.camera().effects.push(ScreenEffect::new(sky));
    scene.frame();
    let thrice = scene.target();
    let yellow = thrice.iter().filter(|c| c[0] > 0.9 && c[1] > 0.9 && c[2] < 0.1).count();
    assert!(
        yellow > SIZE.0 as usize * SIZE.1 as usize / 2,
        "the background is yellow: {yellow}"
    );
    assert!(count(&thrice, 2) > 50, "the boxes keep their colours");
    // Taken away again: the picture is as drawn.
    scene.camera().effects.clear();
    scene.frame();
    assert_eq!(count(&scene.target(), 2), 0, "no effects, no blue");
}

#[test]
fn a_mipmapped_render_target_gets_its_levels_made_after_each_draw() {
    let Some(mut scene) = Scene::new(true) else { return };
    scene.frame();
    let (top, width) = scene.level(0);
    assert_eq!(width, SIZE.0);
    let average = |texels: &[[f32; 4]]| {
        let n = texels.len() as f32;
        [0, 1, 2].map(|i| texels.iter().map(|c| c[i]).sum::<f32>() / n)
    };
    let whole = average(&top);
    assert!(whole[0] > 0.02 && whole[1] > 0.02, "both boxes drawn: {whole:?}");
    // Each level halves the one above; the last is one texel: the average.
    let levels = crate::render::mips::levels(SIZE.0, SIZE.1);
    assert_eq!(levels, 7);
    let (middle, middle_width) = scene.level(3);
    assert_eq!(middle_width, SIZE.0 / 8);
    let (last, last_width) = scene.level(levels - 1);
    assert_eq!((last_width, last.len()), (1, 1));
    for (name, level) in [
        ("level 3", average(&middle)),
        ("the last level", last[0][..3].try_into().unwrap()),
    ] {
        for i in 0..3 {
            assert!(
                (level[i] - whole[i]).abs() < 0.02 + whole[i] * 0.1,
                "{name} is the first level averaged: {level:?} vs {whole:?}"
            );
        }
    }
    // Drawn again with the red box out of sight: the levels follow
    // instead of keeping the last frame's.
    let camera = scene.camera().clone();
    *scene.camera() = camera.with_cull_box(vec3(0.5, -1.0, -1.0), vec3(2.0, 1.0, 1.0));
    scene.frame();
    let (last, _) = scene.level(levels - 1);
    assert!(last[0][0] < 0.005, "no red left in the last level: {:?}", last[0]);
    assert!(last[0][1] > 0.02, "the green is still there: {:?}", last[0]);
    // Its screen effects come first: the levels are made of their result.
    let rotate = scene.ctx.gfx.create_effect("rotate", ROTATE);
    scene.camera().effects.push(ScreenEffect::new(rotate));
    scene.frame();
    let (last, _) = scene.level(levels - 1);
    assert!(
        last[0][1] < 0.005 && last[0][2] > 0.02,
        "green rotated to blue: {:?}",
        last[0]
    );
}

#[test]
fn a_plain_render_target_has_one_level() {
    let Some(scene) = Scene::new(false) else { return };
    let renderer = scene.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    assert!(renderer.read_texture_level(scene.target.texture().id, 0).is_some());
    assert!(
        renderer.read_texture_level(scene.target.texture().id, 1).is_none(),
        "no levels below the first"
    );
}

#[test]
fn compute_passes_and_target_effects_get_a_mipmapped_targets_levels_made_too() {
    let Some(mut scene) = Scene::new(false) else { return };
    scene.ctx.scene3d.passes.target_cameras.clear();
    let field = scene.ctx.gfx.create_mipmapped_render_target(32, 32);
    let last = |scene: &Scene| {
        let renderer = scene.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
        let (bytes, w, _) = renderer.read_texture_level(field.texture().id, 5).expect("a 1x1 level");
        assert_eq!(w, 1);
        let c = |i: usize| half::f16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]).to_f32();
        [c(0), c(1), c(2)]
    };
    // A compute pass fills it; the levels follow in the same frame.
    let fill = scene.ctx.gfx.create_compute(
        "fill",
        "fn compute(c: Cell) -> vec4<f32> { return vec4<f32>(select(0.0, 1.0, c.uv.x < 0.5), 0.25, 0.0, 1.0); }",
    );
    scene
        .ctx
        .scene3d
        .passes
        .compute
        .push(crate::render::ComputePass::new(fill, field));
    scene.frame();
    let average = last(&scene);
    assert!(
        (average[0] - 0.5).abs() < 0.02 && (average[1] - 0.25).abs() < 0.01,
        "{average:?}"
    );
    // A target effect writes it instead: the levels follow by the next frame.
    scene.ctx.scene3d.passes.compute.clear();
    let blue = scene.ctx.gfx.create_effect(
        "blue",
        "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(0.0, 0.0, 0.75); }",
    );
    scene
        .ctx
        .scene3d
        .passes
        .target_effects
        .push(crate::render::TargetEffect::new(blue, field));
    scene.frame();
    scene.frame();
    let average = last(&scene);
    assert!(
        average[0] < 0.01 && (average[2] - 0.75).abs() < 0.02,
        "the effect's blue: {average:?}"
    );
}

/// What the new per-frame work costs, offscreen at 1920x1080 with 400
/// boxes in view: the GPU time of a mipmapped target's levels ("target
/// mipmaps") at 512 and 1024 texels, of one and two target camera
/// effects ("target camera effects") at 1024, and the target camera's
/// passes ("target cameras") and the frame's CPU time with and without a
/// culling box around a quarter of the boxes. For numbers, not a check:
/// `cargo test --release -p ponko --lib --features 3d target_camera_costs -- --ignored --nocapture`.
#[test]
#[ignore = "offscreen GPU timestamps at 1080p; run in release with --ignored --nocapture"]
fn target_camera_costs() {
    const FRAMES: usize = 240;
    let run = |side: u32, mipmapped: bool, effects: usize, cull: bool| -> Option<(f32, f32, f32, f64)> {
        let gpu = super::render_test_support::gpu_with(|adapter| {
            Ok(wgpu::DeviceDescriptor {
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            })
        })?;
        let size = (1920, 1080);
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, vec2(size.0 as f32, size.1 as f32));
        ctx.audio.headless = true;
        ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, size));
        ctx.scene3d.lighting.shadows = false;
        let mesh = {
            let material = ctx.gfx.create_material(MaterialDesc::color(Color::rgb(0.8, 0.5, 0.3)));
            ctx.gfx.create_mesh(&MeshData::cube(0.5), material)
        };
        for i in 0..400 {
            let at = vec3((i % 20) as f32 - 9.5, (i / 20) as f32 * 0.6 - 6.0, -12.0);
            ctx.world.spawn((crate::d3::Transform::at(at), Model::new(mesh)));
        }
        let target = if mipmapped {
            ctx.gfx.create_mipmapped_render_target(side, side)
        } else {
            ctx.gfx.create_render_target(side, side)
        };
        let mut camera = TargetCamera::new(ctx.scene3d.camera, target);
        if cull {
            camera = camera.with_cull_box(vec3(-10.0, -6.5, -13.0), vec3(0.0, 0.0, -11.0));
        }
        for i in 0..effects {
            let e = ctx.gfx.create_effect(
                if i == 0 { "warm" } else { "cool" },
                "fn effect(p: Pixel) -> vec3<f32> { return p.color.brg * 0.9; }",
            );
            camera = camera.with_effect(ScreenEffect::new(e));
        }
        ctx.scene3d.passes.target_cameras.push(camera);
        ctx.gfx.renderer.as_mut()?.set_gpu_timing_every(1);
        for _ in 0..30 {
            ctx.render_frame();
        }
        let started = std::time::Instant::now();
        for _ in 0..FRAMES {
            ctx.render_frame();
        }
        let cpu = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
        let renderer = ctx.gfx.renderer.as_ref()?;
        let pass = |n: &str| {
            renderer
                .gpu_passes()
                .iter()
                .filter(|p| p.name == n)
                .map(|p| p.ms)
                .sum::<f32>()
        };
        Some((
            pass("target mipmaps"),
            pass("target camera effects"),
            pass("target cameras"),
            cpu,
        ))
    };
    let Some(_) = run(64, false, 0, false) else {
        return;
    };
    for (name, side, mipmapped, effects, cull) in [
        ("512 plain", 512, false, 0, false),
        ("512 mipmapped", 512, true, 0, false),
        ("1024 plain", 1024, false, 0, false),
        ("1024 mipmapped", 1024, true, 0, false),
        ("1024, 1 effect", 1024, false, 1, false),
        ("1024, 2 effects", 1024, false, 2, false),
        ("1024, culling box", 1024, false, 0, true),
    ] {
        for round in 1..=3 {
            let (mips, fx, cameras, cpu) = run(side, mipmapped, effects, cull).expect("a GPU, as above");
            eprintln!(
                "{name} (run {round}): mipmaps {mips:.4} ms, effects {fx:.4} ms, target camera passes {cameras:.4} ms, frame on the CPU {cpu:.3} ms"
            );
        }
    }
}
