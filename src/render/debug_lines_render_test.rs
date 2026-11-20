//! `ctx.debug`'s 3D lines on the GPU: depth-tested lines are hidden behind
//! nearer surfaces, lines on top are not, a line passing the camera keeps
//! its visible part, and nothing is drawn while debug drawing is off (the
//! negative control). With 4 samples and with 1 (as on WebGL2). Skips
//! without a GPU.
use super::regression_stage::*;
use super::render_test_support::{Image, Tolerance};
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

#[test]
fn see_through_lines_blend_over_the_scene_and_opaque_lines_cover_it() {
    let Some(mut stage) = Stage::new() else {
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
    assert!(green(full), "opaque: {full:?}");
    // Half green over the grey box: greener than grey, less than full green,
    // and the grey shows through in red and blue.
    assert!(
        mixed[1] > mixed[0] + 20.0 && mixed[1] < full[1] - 20.0 && mixed[0] > full[0] + 20.0,
        "see-through: {mixed:?} vs {full:?}"
    );
    let (top, top_mixed) = (at(opaque + vec3(0.5, 0.3, 0.0)), at(half + vec3(-0.5, 0.3, 0.0)));
    assert!(blue(top), "opaque on top: {top:?}");
    assert!(
        top_mixed[2] < top[2] - 20.0 && top_mixed[0] > top[0] + 20.0,
        "{top_mixed:?} vs {top:?}"
    );
    // A draw call for each of the four kinds.
    let with_lines = stage.renderer.draw_calls();
    stage.ctx.debug.set_enabled(false);
    let _ = image(&mut stage);
    assert_eq!(with_lines - stage.renderer.draw_calls(), 4);
    stage.finish();
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
        let shape = debug.line(vec3(x, 0.0, z), vec3(x, tall, z), Color::GREEN);
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
/// samples a pixel and with 1. The cases take turns, 20 frames at a time
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
    let cases = [
        ("none", Lines::None),
        ("upright", Lines::Upright),
        ("behind", Lines::Behind),
        ("dots", Lines::Dots),
        ("tested", Lines::Tested),
        ("top", Lines::Top),
    ];
    let mut times = vec![(Vec::new(), Vec::new()); cases.len()];
    for _round in 0..8 {
        for (case, &(_, lines)) in cases.iter().enumerate() {
            stage.ctx.debug.clear();
            draw_lines(&mut stage.ctx.debug, lines);
            stage.ctx.gfx.frame3d.debug_lines.clear();
            stage.ctx.hand_over_debug_draw();
            let mut read = timer.frames_read;
            for n in 0..25 {
                timer.collect(&stage.device);
                timer.begin_frame();
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
    for ((name, _), (opaque, frame)) in cases.iter().zip(&mut times) {
        assert!(opaque.len() >= 40, "{name}: only {} frames timed", opaque.len());
        let (o, f) = (spread(opaque), spread(frame));
        println!(
            "{name:>8}: opaque pass median {:.3} ms (min {:.3}, p90 {:.3}); frame median {:.3} ms (min {:.3}, p90 {:.3}); {} frames",
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
