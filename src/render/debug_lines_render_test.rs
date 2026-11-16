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

/// The GPU side of 100,000 debug lines at 1280x720 with 4 samples: the
/// median wall time of a frame (prepare, record, submit and wait for the
/// GPU) with and without them, over a floor and a few boxes. Ignored by
/// default; `ponko_bench -- debug/` measures the CPU side.
#[test]
#[ignore = "a timing: run on purpose, on a quiet machine"]
fn hundred_thousand_lines_gpu_time() {
    let Some(mut stage) = Stage::with(Setup {
        size: (1280, 720),
        ..Default::default()
    }) else {
        return;
    };
    stage.floor();
    let grey = stage.cube(Color::rgb(0.5, 0.5, 0.5));
    for i in 0..5 {
        let x = i as f32 * 1.5 - 3.0;
        stage.block(grey, vec3(x - 0.4, 0.0, -0.4), vec3(x + 0.4, 1.5, 0.4));
    }
    stage.ctx.debug.set_enabled(true);
    let median = |stage: &mut Stage, lines: bool| {
        let mut times = Vec::new();
        for frame in 0..60 {
            stage.ctx.debug.clear();
            if lines {
                for i in 0..100_000 {
                    let x = (i % 1000) as f32 * 0.008 - 4.0;
                    let z = (i / 1000) as f32 * 0.04 - 2.0;
                    let shape = stage.ctx.debug.line(vec3(x, 0.0, z), vec3(x, 1.0, z), Color::GREEN);
                    if i % 2 == 0 {
                        shape.on_top();
                    }
                }
            }
            stage.ctx.gfx.frame3d.debug_lines.clear();
            stage.ctx.hand_over_debug_draw();
            let start = std::time::Instant::now();
            stage.render();
            stage.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if frame >= 10 {
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(f64::total_cmp);
        (times[times.len() / 2], times[0])
    };
    let without = median(&mut stage, false);
    let with = median(&mut stage, true);
    let again = median(&mut stage, false);
    println!(
        "frame without lines: median {:.3} ms (min {:.3}); with 100k lines: median {:.3} ms (min {:.3}); without again: median {:.3} ms",
        without.0, without.1, with.0, with.1, again.0
    );
    stage.finish();
}
