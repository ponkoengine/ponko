//! `ctx.debug`'s 3D lines on the GPU: depth-tested lines are hidden behind
//! nearer surfaces, lines on top are not, a line passing the camera keeps
//! its visible part, and nothing is drawn while debug drawing is off (the
//! negative control). With 4 samples and with 1 (as on WebGL2). Labels
//! and lines show in every 3D view; labels on top are on top in a render
//! target as on screen. Skips without a GPU.
use super::regression_stage::*;
use super::render_test_support::{Image, Tolerance, keep_image};
use crate::prelude::*;

/// Green when the green channel clearly leads.
fn green(c: [f32; 3]) -> bool {
    c[1] > 120.0 && c[0] < 90.0 && c[2] < 90.0
}

fn magenta(c: [f32; 3]) -> bool {
    c[0] > 120.0 && c[2] > 120.0 && c[1] < 90.0
}

fn blue(c: [f32; 3]) -> bool {
    c[2] > 120.0 && c[0] < 90.0 && c[1] < 90.0
}

/// A dark box in the middle; behind it a green depth-tested line and a
/// magenta one on top; a blue line from behind the camera to the right.
fn scene(stage: &mut Stage) {
    let dark = stage.cube(Color::rgb(0.15, 0.15, 0.15));
    stage.block(dark, vec3(-0.6, 0.0, -0.6), vec3(0.6, 1.4, 0.6));
    let debug = &mut stage.ctx.debug;
    debug.set_enabled(true);
    debug
        .line(vec3(-2.5, 0.5, -2.0), vec3(2.5, 0.5, -2.0), Color::GREEN)
        .width(4.0);
    debug
        .line(vec3(-2.5, 0.9, -2.0), vec3(2.5, 0.9, -2.0), Color::rgb(1.0, 0.0, 1.0))
        .width(4.0)
        .on_top();
    debug
        .line(vec3(0.0, 0.2, 10.0), vec3(1.5, 0.2, 0.0), Color::BLUE)
        .width(4.0);
}

fn image(stage: &mut Stage) -> Image {
    stage.ctx.gfx.frame3d.debug_lines.clear();
    stage.ctx.hand_over_debug_draw();
    stage.image()
}

/// A faint transparent pane in a corner that reads the scene behind it:
/// the transparent surfaces then get a pass of their own, and the lines
/// are drawn in that one.
fn scene_reading_pane(stage: &mut Stage) {
    let (shader, _) = stage.shader(
        "pane",
        "fn surface(s: Surface) -> Surface { var o = s; o.alpha = clamp(scene_distance(s.screen) * 0.01, 0.0, 0.2); return o; }",
    );
    let pane = stage.mesh(
        &crate::render::MeshData::cube(1.0),
        crate::render::MaterialDesc {
            color: Color::BLACK,
            blend: crate::render::Blend::Transparent,
            shader: Some(shader),
            ..Default::default()
        },
    );
    stage.block(pane, vec3(-3.0, 2.0, -1.0), vec3(-2.5, 2.5, -0.5));
}

#[test]
fn depth_tested_lines_hide_behind_surfaces_and_lines_on_top_do_not() {
    for (samples, split) in [(4, false), (1, false), (4, true)] {
        let Some(mut stage) = Stage::with(Setup {
            samples,
            ..Default::default()
        }) else {
            return;
        };
        scene(&mut stage);
        if split {
            scene_reading_pane(&mut stage);
        }
        let drawn = image(&mut stage);
        let features = super::gpu3d::last_features().expect("the frame was checked");
        assert_eq!(features.contains(super::gpu3d::FrameFeatures::SPLIT), split);
        let samples = format!("{samples}x{}", if split { ", own transparent pass" } else { "" });
        let at = |p: Vec3| stage.color_at(&drawn, p);
        // Beside the box, both lines show.
        assert!(
            green(at(vec3(-2.0, 0.5, -2.0))),
            "{samples}x: {:?}",
            at(vec3(-2.0, 0.5, -2.0))
        );
        assert!(magenta(at(vec3(2.0, 0.9, -2.0))), "{samples}x");
        // Behind the box: the depth-tested line is hidden, the other isn't.
        let hidden = at(vec3(0.0, 0.5, -2.0));
        assert!(
            !green(hidden) && luma(hidden) < 90.0,
            "{samples}x: the box covers it: {hidden:?}"
        );
        assert!(magenta(at(vec3(0.0, 0.9, -2.0))), "{samples}x: on top of the box");
        // The line passing the camera keeps the part in front of it.
        assert!(
            blue(at(vec3(1.4, 0.2, 2.0 / 3.0))),
            "{samples}x: {:?}",
            at(vec3(1.4, 0.2, 2.0 / 3.0))
        );
        // One draw call each for the tested lines and those on top.
        let with_lines = stage.renderer.draw_calls();

        // Negative control: drawing off, the same frame has no lines.
        stage.ctx.debug.set_enabled(false);
        let off = image(&mut stage);
        let at = |p: Vec3| stage.color_at(&off, p);
        assert!(!green(at(vec3(-2.0, 0.5, -2.0))), "{samples}x: off");
        assert!(!magenta(at(vec3(0.0, 0.9, -2.0))), "{samples}x: off");
        assert!(!blue(at(vec3(1.4, 0.2, 2.0 / 3.0))), "{samples}x: off");
        assert_differs(
            &drawn,
            &off,
            Tolerance {
                channel: 8,
                outliers: 0.0,
                edge: 0,
            },
            "debug lines on and off",
        );
        assert_eq!(with_lines - stage.renderer.draw_calls(), 2, "{samples}x");
        stage.finish();
    }
}

/// See-through lines (alpha 0.5) show half of the scene behind them and
/// opaque lines cover it, depth-tested and on top: with 4 samples a pixel
/// (alpha to coverage: half the samples) and with 1 (blended).
#[test]
fn see_through_lines_show_the_scene_through_them_and_opaque_lines_cover_it() {
    for samples in [4, 1] {
        let Some(mut stage) = Stage::with(Setup {
            samples,
            ..Default::default()
        }) else {
            return;
        };
        let grey = stage.cube(Color::rgb(0.5, 0.5, 0.5));
        stage.block(grey, vec3(-2.0, 0.0, -1.0), vec3(2.0, 1.4, -0.6));
        let debug = &mut stage.ctx.debug;
        debug.set_enabled(true);
        let (opaque, half) = (vec3(-1.0, 0.4, 0.0), vec3(1.0, 0.4, 0.0));
        debug
            .line(opaque - Vec3::Y * 0.3, opaque + Vec3::Y * 0.3, Color::GREEN)
            .width(8.0);
        debug
            .line(half - Vec3::Y * 0.3, half + Vec3::Y * 0.3, Color::GREEN.with_alpha(0.5))
            .width(8.0);
        // On top: one of each kind again, beside the others.
        debug
            .line(opaque + Vec3::X * 0.5, opaque + vec3(0.5, 0.6, 0.0), Color::BLUE)
            .width(8.0)
            .on_top();
        debug
            .line(
                half - Vec3::X * 0.5,
                half + vec3(-0.5, 0.6, 0.0),
                Color::BLUE.with_alpha(0.5),
            )
            .width(8.0)
            .on_top();
        let drawn = image(&mut stage);
        let at = |p: Vec3| stage.color_at(&drawn, p);
        let (full, mixed) = (at(opaque), at(half));
        assert!(green(full), "{samples}x: opaque: {full:?}");
        // Half green over the grey box: greener than grey, less than full
        // green, and the grey shows through in red and blue.
        assert!(
            mixed[1] > mixed[0] + 20.0 && mixed[1] < full[1] - 20.0 && mixed[0] > full[0] + 20.0,
            "{samples}x: see-through: {mixed:?} vs {full:?}"
        );
        let (top, top_mixed) = (at(opaque + vec3(0.5, 0.3, 0.0)), at(half + vec3(-0.5, 0.3, 0.0)));
        assert!(blue(top), "{samples}x: opaque on top: {top:?}");
        assert!(
            top_mixed[2] < top[2] - 20.0 && top_mixed[0] > top[0] + 20.0,
            "{samples}x: {top_mixed:?} vs {top:?}"
        );
        // Negative control: the same lines blended (as before) look alike.
        stage.renderer.debug_see_through = Some(super::debug_lines::SeeThrough::Blend);
        let blended = image(&mut stage);
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 40.0);
        let blended_mixed = stage.color_at(&blended, half);
        assert!(
            close(mixed, blended_mixed),
            "{samples}x: like blending: {mixed:?} vs {blended_mixed:?}"
        );
        stage.renderer.debug_see_through = None;
        // A draw call for each of the four kinds.
        let _ = image(&mut stage);
        let with_lines = stage.renderer.draw_calls();
        stage.ctx.debug.set_enabled(false);
        let _ = image(&mut stage);
        assert_eq!(with_lines - stage.renderer.draw_calls(), 4, "{samples}x");
        stage.finish();
    }
}

#[test]
fn lines_alone_draw_without_any_model() {
    let Some(mut stage) = Stage::new() else {
        return;
    };
    stage.ctx.debug.set_enabled(true);
    stage.ctx.debug.grid(Vec3::ZERO, 0.5, 8, Color::WHITE);
    let drawn = image(&mut stage);
    let bright = drawn.fraction(0..drawn.width, 0..drawn.height, |p| p[0] > 150 && p[1] > 150);
    assert!(bright > 0.01, "the grid shows: {bright}");
    stage.finish();
}

/// What one case of [`hundred_thousand_lines_gpu_time`] draws.
#[derive(Clone, Copy, PartialEq)]
enum Lines {
    /// Nothing: the frame without debug lines.
    None,
    /// 100,000 upright lines a meter tall in a block in front of the
    /// camera, half of them on top (the benchmark's lines).
    Upright,
    /// The same lines behind the camera: every one dropped by the vertex
    /// shader, so only uploading and vertex work is left.
    Behind,
    /// 100,000 lines a millimeter long where the upright ones stand: about
    /// a pixel each, rasterizing without filling.
    Dots,
    /// The upright lines, all depth-tested.
    Tested,
    /// The upright lines, all on top.
    Top,
    /// The upright lines (half on top), see-through (alpha 0.5).
    SeeThrough,
}

fn draw_lines(debug: &mut crate::debug::DebugDraw, lines: Lines) {
    if lines == Lines::None {
        return;
    }
    let (behind, tall) = match lines {
        Lines::Behind => (20.0, 1.0),
        Lines::Dots => (0.0, 0.001),
        _ => (0.0, 1.0),
    };
    for i in 0..100_000 {
        let x = (i % 1000) as f32 * 0.008 - 4.0;
        let z = (i / 1000) as f32 * 0.04 - 2.0 + behind;
        let color = if lines == Lines::SeeThrough {
            Color::GREEN.with_alpha(0.5)
        } else {
            Color::GREEN
        };
        let shape = debug.line(vec3(x, 0.0, z), vec3(x, tall, z), color);
        let top = match lines {
            Lines::Tested => false,
            Lines::Top => true,
            _ => i % 2 == 0,
        };
        if top {
            shape.on_top();
        }
    }
}

/// The GPU side of 100,000 debug lines at 1280x720, over a floor and a few
/// boxes, measured with GPU timestamps: the opaque pass (where the lines
/// are drawn) and the whole frame, for each kind of [`Lines`], with 4
/// samples a pixel and with 1. See-through lines are timed both ways:
/// blended (what every sample count did before) and as drawn now (alpha to
/// coverage with several samples a pixel). The cases take turns, 20 frames at a time
/// for 8 rounds, so a busy GPU slows them alike; the median and the spread
/// (min, p90) of the timed frames are printed. Ignored by default (a
/// timing); `ponko_bench -- debug/` measures the CPU side. Run it with
/// `cargo test -p ponko --lib --features 3d hundred_thousand_lines_gpu_time -- --ignored --nocapture`.
#[test]
#[ignore = "a timing: run on purpose, on a quiet machine"]
fn hundred_thousand_lines_gpu_time() {
    for samples in [4, 1] {
        time_lines(samples);
    }
}

fn time_lines(samples: u32) {
    let Some(mut stage) = Stage::with(Setup {
        size: (1280, 720),
        samples,
        timestamps: true,
        ..Default::default()
    }) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&stage.device, &stage.queue) else {
        println!("no GPU timestamps on {}: nothing measured", stage.info.name);
        return;
    };
    stage.floor();
    let grey = stage.cube(Color::rgb(0.5, 0.5, 0.5));
    for i in 0..5 {
        let x = i as f32 * 1.5 - 3.0;
        stage.block(grey, vec3(x - 0.4, 0.0, -0.4), vec3(x + 0.4, 1.5, 0.4));
    }
    stage.ctx.debug.set_enabled(true);
    println!(
        "adapter: {} ({:?}), {samples} sample(s) a pixel",
        stage.info.name, stage.info.backend
    );
    use super::debug_lines::SeeThrough;
    let now = SeeThrough::for_samples(samples);
    let cases = [
        ("none", Lines::None, now),
        ("upright", Lines::Upright, now),
        ("behind", Lines::Behind, now),
        ("dots", Lines::Dots, now),
        ("tested", Lines::Tested, now),
        ("top", Lines::Top, now),
        ("see-through blended", Lines::SeeThrough, SeeThrough::Blend),
        ("see-through now", Lines::SeeThrough, now),
    ];
    let mut times = vec![(Vec::new(), Vec::new()); cases.len()];
    for _round in 0..8 {
        for (case, &(_, lines, see_through)) in cases.iter().enumerate() {
            stage.renderer.debug_see_through = Some(see_through);
            stage.ctx.debug.clear();
            draw_lines(&mut stage.ctx.debug, lines);
            stage.ctx.gfx.frame3d.debug_lines.clear();
            stage.ctx.hand_over_debug_draw();
            let mut read = timer.frames_read;
            for n in 0..25 {
                // `render_timed` collects and begins the timer's frame itself.
                stage.render_timed(Some(&mut timer));
                stage.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                timer.collect(&stage.device);
                // The first frames of a case warm up (and may be the last case's).
                if timer.frames_read != read && n >= 5 {
                    let spans = &timer.last_frame;
                    let opaque: f32 = spans.iter().filter(|p| p.name == "opaque").map(|p| p.ms).sum();
                    let frame = spans.iter().map(|p| p.start_ms + p.ms).fold(0.0, f32::max);
                    // Some adapters report a pass of zero now and then: not a time.
                    if opaque > 0.0 {
                        times[case].0.push(opaque);
                        times[case].1.push(frame);
                    }
                }
                read = timer.frames_read;
            }
        }
    }
    let spread = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        (v[v.len() / 2], v[0], v[v.len() * 9 / 10])
    };
    for ((name, _, _), (opaque, frame)) in cases.iter().zip(&mut times) {
        assert!(opaque.len() >= 40, "{name}: only {} frames timed", opaque.len());
        let (o, f) = (spread(opaque), spread(frame));
        println!(
            "{name:>20}: opaque pass median {:.3} ms (min {:.3}, p90 {:.3}); frame median {:.3} ms (min {:.3}, p90 {:.3}); {} frames",
            o.0,
            o.1,
            o.2,
            f.0,
            f.1,
            f.2,
            opaque.len()
        );
    }
    stage.finish();
}

/// Several 3D cameras drawn through a game's own frame (`Ctx::render_frame`
/// with the offscreen renderer): the main camera and a second player in
/// split screen, and a target camera into a render target.
struct Views {
    ctx: crate::ctx::Ctx,
    target: crate::render::RenderTarget,
    wall: Option<Entity>,
}

/// The window's size: two square halves.
const VIEWS: (u32, u32) = (256, 128);
/// A magenta label in front of the wall, and where it is.
const FRONT: Vec3 = vec3(-1.5, 2.6, 1.0);
/// A cyan label behind the wall, where the wall hides it from every camera.
const BEHIND: Vec3 = vec3(-1.0, 2.2, -2.0);
/// A green line in front of the wall, and behind it a yellow one.
const LINE: (Vec3, Vec3) = (vec3(-2.0, 0.8, 1.0), vec3(2.0, 0.8, 1.0));
const LINE_BEHIND: (Vec3, Vec3) = (vec3(-2.0, 1.4, -2.0), vec3(2.0, 1.4, -2.0));

impl Views {
    fn new() -> Option<Self> {
        let mut ctx = super::render_test_support::offscreen_ctx(VIEWS)?;
        ctx.scene3d.sky.atmosphere = false;
        ctx.gfx.clear_color = Color::BLACK;
        let grey = ctx
            .gfx
            .create_material(crate::render::MaterialDesc::color(Color::rgb(0.4, 0.4, 0.4)));
        let cube = ctx.gfx.create_mesh(&crate::render::MeshData::cube(1.0), grey);
        // A wall 8 wide and 5 high, its front at z = 0.5.
        let wall = ctx.world.spawn((
            crate::d3::Transform {
                position: vec3(0.0, 2.0, 0.0),
                scale: vec3(8.0, 5.0, 1.0),
                ..Default::default()
            },
            crate::render::Model::new(cube),
        ));
        let camera = |x: f32, rect| {
            let mut camera = ctx.scene3d.camera;
            camera.position = vec3(x, 2.0, 9.0);
            camera.look_at(vec3(0.0, 1.8, 0.0));
            camera.rect = rect;
            camera
        };
        let (main, second) = (
            camera(-0.5, crate::camera::Viewport::split(2, 0)),
            camera(0.5, crate::camera::Viewport::split(2, 1)),
        );
        let seen_by_target = camera(0.0, crate::camera::Viewport::FULL);
        ctx.scene3d.camera = main;
        ctx.scene3d.views.push(crate::d3::View::new(second));
        let target = ctx.gfx.create_render_target(128, 128);
        ctx.scene3d
            .passes
            .target_cameras
            .push(crate::render::TargetCamera::new(seen_by_target, target));
        Some(Self {
            ctx,
            target,
            wall: Some(wall),
        })
    }

    /// Draws the labels and lines (each label depth-tested), then a frame;
    /// returns the window and the render target, as RGBA8 (the target's
    /// linear HDR light clamped and encoded as sRGB).
    fn frame(&mut self) -> (Image, Image) {
        self.frame_with(|debug| {
            debug
                .text(FRONT, "MM", Color::rgb(1.0, 0.0, 1.0))
                .size(16.0)
                .depth_tested();
            debug
                .text(BEHIND, "MM", Color::rgb(0.0, 1.0, 1.0))
                .size(16.0)
                .depth_tested();
            debug.line(LINE.0, LINE.1, Color::GREEN).width(3.0);
            debug.line(LINE_BEHIND.0, LINE_BEHIND.1, Color::YELLOW).width(3.0);
        })
    }

    /// [`frame`](Self::frame), with what `draw` draws instead.
    fn frame_with(&mut self, draw: impl FnOnce(&mut crate::debug::DebugDraw)) -> (Image, Image) {
        let debug = &mut self.ctx.debug;
        debug.clear();
        debug.set_enabled(true);
        draw(debug);
        let path = std::env::temp_dir().join(format!(
            "ponko-debug-views-{}-{:?}.png",
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
        let renderer = self.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
        let bytes = renderer
            .read_texture(self.target.texture().id)
            .expect("the render target");
        let encode = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        let rgba = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .flat_map(|p| {
                let c = |i: usize| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32();
                [encode(c(0)), encode(c(1)), encode(c(2)), 255]
            })
            .collect();
        let target = Image {
            width: 128,
            height: 128,
            rgba,
        };
        keep_image(&target);
        (window, target)
    }
}

fn magenta8(p: [u8; 4]) -> bool {
    p[0] > 110 && p[2] > 110 && p[1] < 80
}

fn cyan8(p: [u8; 4]) -> bool {
    p[1] > 110 && p[2] > 110 && p[0] < 80
}

fn green8(p: [u8; 4]) -> bool {
    p[1] > 110 && p[0] < 80 && p[2] < 80
}

fn yellow8(p: [u8; 4]) -> bool {
    p[0] > 110 && p[1] > 110 && p[2] < 80
}

/// How many pixels of `image` in columns `x` are `is`.
fn count(image: &Image, x: std::ops::Range<u32>, is: fn([u8; 4]) -> bool) -> usize {
    let mut n = 0;
    for y in 0..image.height {
        for x in x.clone() {
            n += usize::from(is(image.pixel(x, y)));
        }
    }
    n
}

/// What each 3D view shows: (main half, second player's half, render target).
fn shown(views: &mut Views, is: fn([u8; 4]) -> bool) -> [usize; 3] {
    let (window, target) = views.frame();
    [
        count(&window, 0..128, is),
        count(&window, 128..256, is),
        count(&target, 0..128, is),
    ]
}

#[test]
fn depth_tested_labels_and_lines_show_in_every_3d_view_and_hide_behind_surfaces() {
    let Some(mut views) = Views::new() else {
        return;
    };
    // In front of the wall: the label and the line in each view.
    let labels = shown(&mut views, magenta8);
    assert!(
        labels.iter().all(|&n| n >= 8),
        "the front label in the main half, the second half and the render target: {labels:?}"
    );
    let lines = shown(&mut views, green8);
    assert!(
        lines.iter().all(|&n| n >= 20),
        "the front line in every view: {lines:?}"
    );
    // Behind the wall: hidden from every camera.
    let hidden = shown(&mut views, cyan8);
    assert_eq!(hidden, [0; 3], "the wall hides the label behind it");
    assert_eq!(shown(&mut views, yellow8), [0; 3], "the wall hides the line behind it");

    // Negative control: without the wall, every view shows the label and
    // the line behind it, so they were drawn and hidden by depth.
    views.ctx.world.despawn(views.wall.take().unwrap());
    let behind = shown(&mut views, cyan8);
    assert!(behind.iter().all(|&n| n >= 8), "no wall, the label shows: {behind:?}");
    let behind = shown(&mut views, yellow8);
    assert!(behind.iter().all(|&n| n >= 20), "no wall, the line shows: {behind:?}");

    // A camera off the default layer shows neither; the others still do.
    let without_default = crate::camera::RenderLayers::ALL.without(0);
    views.ctx.scene3d.views[0].camera.layers = without_default;
    views.ctx.scene3d.passes.target_cameras[0].camera.layers = without_default;
    let off = shown(&mut views, magenta8);
    assert!(
        off[0] >= 8 && off[1] == 0 && off[2] == 0,
        "off the default layer: {off:?}"
    );
    let off = shown(&mut views, green8);
    assert!(
        off[0] >= 20 && off[1] == 0 && off[2] == 0,
        "off the default layer: {off:?}"
    );
    // The main camera off it, the target camera on it: only the target
    // shows them.
    views.ctx.scene3d.camera.layers = without_default;
    views.ctx.scene3d.passes.target_cameras[0].camera.layers = crate::camera::RenderLayers::ALL;
    let target_only = shown(&mut views, magenta8);
    assert!(
        target_only[0] == 0 && target_only[2] >= 8,
        "only the target camera shows the label: {target_only:?}"
    );
    let target_only = shown(&mut views, green8);
    assert!(
        target_only[0] == 0 && target_only[2] >= 20,
        "only the target camera shows the line: {target_only:?}"
    );
    // A target camera without particles leaves debug drawing out too.
    views.ctx.scene3d.camera.layers = crate::camera::RenderLayers::ALL;
    views.ctx.scene3d.passes.target_cameras[0].camera.particles = false;
    assert_eq!(shown(&mut views, magenta8)[2], 0, "no particles, no label");
    assert_eq!(shown(&mut views, green8)[2], 0, "no particles, no line");
}

#[test]
fn debug_drawing_in_a_target_camera_follows_its_own_flag_whatever_its_particles() {
    let Some(mut views) = Views::new() else {
        return;
    };
    let target = |views: &mut Views, is| shown(views, is)[2];
    // Negative control: by default a target camera without particles
    // leaves the debug drawing out.
    views.ctx.scene3d.passes.target_cameras[0].camera.particles = false;
    assert_eq!(target(&mut views, magenta8), 0, "no particles, no label by default");
    assert_eq!(target(&mut views, green8), 0, "no particles, no line by default");
    // Its own flag on: the depth-tested label and line show, without
    // particles.
    let camera = &mut views.ctx.scene3d.passes.target_cameras[0];
    *camera = camera.with_debug_draw(true);
    let label = target(&mut views, magenta8);
    let line = target(&mut views, green8);
    assert!(
        label >= 8 && line >= 20,
        "debug drawing on, particles off: label {label}, line {line}"
    );
    assert_eq!(
        shown(&mut views, cyan8)[2],
        0,
        "the wall still hides the label behind it"
    );
    // A label on top shows too, over the wall.
    let (_, image) = views.frame_with(|debug| {
        debug.text(BEHIND, "MM", Color::rgb(0.0, 1.0, 1.0)).size(16.0);
    });
    assert!(
        count(&image, 0..128, cyan8) >= 8,
        "the label on top shows without particles"
    );
    // Its own flag off: nothing, though it draws particles.
    let camera = &mut views.ctx.scene3d.passes.target_cameras[0];
    camera.camera.particles = true;
    *camera = camera.with_debug_draw(false);
    assert_eq!(
        target(&mut views, magenta8),
        0,
        "debug drawing off, particles on: no label"
    );
    assert_eq!(
        target(&mut views, green8),
        0,
        "debug drawing off, particles on: no line"
    );
    // Back to the default, it follows the particles again.
    views.ctx.scene3d.passes.target_cameras[0].camera.debug_draw = None;
    assert!(
        target(&mut views, green8) >= 20,
        "default, particles on: the line shows"
    );
    // The window's camera with the flag off: its half shows nothing, the
    // second player's half and the target still show it all.
    views.ctx.scene3d.camera.debug_draw = Some(false);
    for is in [magenta8, green8] {
        let seen = shown(&mut views, is);
        assert!(
            seen[0] == 0 && seen[1] >= 8 && seen[2] >= 8,
            "only the window's camera leaves it out: {seen:?}"
        );
    }
    let (window, _) = views.frame_with(|debug| {
        debug.text(BEHIND, "MM", Color::rgb(0.0, 1.0, 1.0)).size(16.0);
    });
    assert_eq!(count(&window, 0..128, cyan8), 0, "nor the label on top, in its half");
    assert!(
        count(&window, 128..256, cyan8) >= 8,
        "the second half still shows the label on top"
    );
}

#[test]
fn labels_on_top_are_on_top_in_a_render_target_as_on_screen() {
    let Some(mut views) = Views::new() else {
        return;
    };
    // Behind the wall, not depth-tested: on screen it is drawn over
    // everything, in both halves, and so it is in the render target, over
    // the wall and over a debug line on top drawn across it.
    let on_top = |debug: &mut crate::debug::DebugDraw| {
        debug.text(BEHIND, "MM", Color::rgb(0.0, 1.0, 1.0)).size(16.0);
        debug
            .line(
                BEHIND + vec3(-0.5, -0.5, 0.0),
                BEHIND + vec3(2.0, -0.5, 0.0),
                Color::YELLOW,
            )
            .width(3.0)
            .on_top();
    };
    let (window, target) = views.frame_with(on_top);
    let shown = [
        count(&window, 0..128, cyan8),
        count(&window, 128..256, cyan8),
        count(&target, 0..128, cyan8),
    ];
    assert!(
        shown.iter().all(|&n| n >= 8),
        "the label on top shows in the main half, the second half and the render target: {shown:?}"
    );
    // Over the line on top through it: the label hides part of it, where
    // the line alone shows whole.
    let crossed = count(&target, 0..128, yellow8);
    let (_, alone) = views.frame_with(|debug| {
        debug
            .line(
                BEHIND + vec3(-0.5, -0.5, 0.0),
                BEHIND + vec3(2.0, -0.5, 0.0),
                Color::YELLOW,
            )
            .width(3.0)
            .on_top();
    });
    let alone = count(&alone, 0..128, yellow8);
    eprintln!("yellow line pixels: {crossed} under the label, {alone} alone");
    assert!(
        crossed + 8 <= alone,
        "the label is drawn over the line on top: {crossed} of the line's pixels show under it, {alone} alone"
    );
    // Negative control: depth-tested, the wall hides it in every view.
    let (window, target) = views.frame_with(|debug| {
        debug
            .text(BEHIND, "MM", Color::rgb(0.0, 1.0, 1.0))
            .size(16.0)
            .depth_tested();
    });
    let hidden = [
        count(&window, 0..128, cyan8),
        count(&window, 128..256, cyan8),
        count(&target, 0..128, cyan8),
    ];
    assert_eq!(hidden, [0; 3], "depth-tested, the wall hides it");
    // A target camera off the default layer shows no label, as no line.
    let without_default = crate::camera::RenderLayers::ALL.without(0);
    views.ctx.scene3d.passes.target_cameras[0].camera.layers = without_default;
    let (window, off) = views.frame_with(on_top);
    assert!(count(&window, 0..128, cyan8) >= 8, "the window still shows it");
    assert_eq!(count(&off, 0..128, cyan8), 0, "off the default layer, no label");
    views.ctx.scene3d.passes.target_cameras[0].camera.layers = crate::camera::RenderLayers::ALL;
    let renderer = views.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    let info = renderer.texture_for_test(0).0.adapter_info();
    let (_, target) = views.frame_with(on_top);
    super::render_test_support::check_reference(
        "debug_label_on_top_render_target",
        &target,
        Tolerance {
            channel: 12,
            outliers: 0.01,
            edge: 1,
        },
        "a render target seeing a grey wall from the front, black around it; a cyan 16 px debug label (not \
         depth-tested) behind the wall drawn over it, top left of the middle, over a yellow debug line on top \
         through the middle of its letters, both drawn through the wall",
        &info,
    );
}
