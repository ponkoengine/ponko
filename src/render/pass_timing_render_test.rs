//! Offscreen GPU tests of when a game times its GPU passes: one frame in
//! [`GPU_TIMING_EVERY`] by default, every frame while something reads every
//! frame (the HUD, profiling, a trace, automatic quality), on a whole game
//! context drawn by the production renderer into a texture (no window).
//! Skips without a GPU or without timestamp queries.
use super::render_test_support::*;
use crate::d3::Transform as Transform3D;
use crate::perf::GPU_TIMING_EVERY;
use crate::prelude::*;
use crate::render::{FrameStatus, MaterialDesc, MeshData, Model};
use crate::time::FrameTiming;

/// A game context drawing offscreen on a device with timestamp queries,
/// timing frames as a windowed game does (the offscreen test renderer
/// otherwise times every frame). `None` (logged) without them.
fn timed_ctx() -> Option<Box<Ctx>> {
    let gpu = gpu_with(|adapter| {
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return Err("needs timestamp queries".into());
        }
        Ok(wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })
    })?;
    if !gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        skip("needs timestamp queries, which this device lacks");
        return None;
    }
    let mut ctx = Box::new(Ctx::new(1.0 / 60.0, Vec2::new(64.0, 64.0)));
    ctx.audio.headless = true;
    let mut renderer = super::gpu::Renderer::offscreen(gpu, (64, 64));
    renderer.gpu_timing_pinned = false;
    ctx.gfx.renderer = Some(renderer);
    ctx.gfx.clear(Color::rgb(0.2, 0.3, 0.4));
    let material = ctx.gfx.create_material(MaterialDesc::default());
    let cube = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
    ctx.world.spawn((Transform3D::default(), Model::new(cube)));
    ctx.scene3d.camera.position = vec3(0.0, 1.5, 4.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    Some(ctx)
}

/// One drawn frame, waiting for the GPU after it: the next frame then reads
/// this one's timestamps back, so a timed frame's times are in by the
/// frame after it. Gives the frames timed so far.
fn frame(ctx: &mut Ctx) -> u64 {
    ctx.begin_frame(FrameTiming {
        dt: 1.0 / 60.0,
        seconds: 1.0 / 60.0,
        fixed_steps: 0,
        alpha: 0.0,
    });
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen, "the frame was drawn");
    let renderer = ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    let _ = renderer.texture_for_test(0).0.poll(wgpu::PollType::wait_indefinitely());
    renderer.detail().gpu_frames_timed
}

/// Frames timed over `n` more frames.
fn timed_over(ctx: &mut Ctx, n: u64) -> u64 {
    let before = frame(ctx);
    let mut after = before;
    for _ in 1..n {
        after = frame(ctx);
    }
    after - before
}

#[test]
fn a_game_times_one_frame_in_sixteen_unless_something_reads_every_frame() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    assert_eq!(ctx.gpu_timing_every(), GPU_TIMING_EVERY);
    // Settle: the first timed frame's times are in.
    for _ in 0..2 * GPU_TIMING_EVERY {
        frame(&mut ctx);
    }
    let n = 8 * u64::from(GPU_TIMING_EVERY);
    let sampled = timed_over(&mut ctx, n);
    // Every 16th frame, the window's ends counted either way.
    assert!(
        (n / u64::from(GPU_TIMING_EVERY) - 1..=n / u64::from(GPU_TIMING_EVERY) + 1).contains(&sampled),
        "{sampled} of {n} frames timed outside profiling"
    );
    // The numbers shown stay there between samples: smoothed per-pass and
    // frame times, not zero or missing on the untimed frames.
    let stats = ctx.gfx.renderer.as_ref().unwrap().stats();
    let gpu_ms = stats.gpu_ms.expect("a smoothed GPU time between samples");
    assert!(gpu_ms.is_finite() && gpu_ms > 0.0, "{gpu_ms}");
    assert!(
        !ctx.perf.gpu_passes().is_empty(),
        "the passes stay listed between samples"
    );
    assert!(ctx.perf.gpu_passes().iter().all(|p| p.ms.is_finite() && p.ms >= 0.0));

    // The HUD reads every frame: every frame is timed while it shows
    // (the first one after switching was begun before it).
    ctx.perf.set_hud(true);
    assert_eq!(ctx.gpu_timing_every(), 1);
    frame(&mut ctx);
    let hud = timed_over(&mut ctx, 32);
    assert!(hud >= 30, "{hud} of 32 frames timed with the HUD on");
    ctx.perf.set_hud(false);
    assert_eq!(ctx.gpu_timing_every(), GPU_TIMING_EVERY);
    frame(&mut ctx);
    let off = timed_over(&mut ctx, 32);
    assert!(off <= 3, "{off} of 32 frames timed after the HUD went off");
}

/// Automatic quality counts consecutive samples over or under its budget,
/// so it sees what it saw before sampling: a fresh sample every frame.
#[test]
fn automatic_quality_still_gets_a_fresh_gpu_sample_every_frame() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    ctx.scene3d
        .set_quality_autotune(Some(crate::render::QualityAutotune::default()));
    assert_eq!(ctx.gpu_timing_every(), 1, "automatic quality reads every frame");
    let mut last = 0;
    let mut fresh = 0;
    for i in 0..40 {
        frame(&mut ctx);
        let renderer = ctx.gfx.renderer.as_ref().unwrap();
        let Some(sample) = renderer.quality_sample() else {
            assert!(i < 4, "no GPU sample after {i} frames");
            continue;
        };
        if i >= 8 {
            assert!(sample.sequence > last, "frame {i} brought no new sample");
            assert!(sample.fresh, "frame {i}'s sample is stale");
            fresh += 1;
        }
        last = sample.sequence;
    }
    assert_eq!(fresh, 32);
    let status = ctx.quality_autotune_status().expect("the policy is on");
    assert_ne!(status.reason, crate::render::QualityAutotuneReason::GpuUnavailable);
    // The policy off, the game goes back to sampling.
    ctx.scene3d.set_quality_autotune(None);
    assert_eq!(ctx.gpu_timing_every(), GPU_TIMING_EVERY);
}

/// A trace shows every frame's GPU passes, so it times every frame while
/// it records; so do profiling and a benchmark run.
#[test]
fn traces_and_profiling_time_every_frame() {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(64.0, 64.0));
    assert_eq!(ctx.gpu_timing_every(), GPU_TIMING_EVERY);
    ctx.perf.set_profiling(true);
    assert_eq!(ctx.gpu_timing_every(), 1);
    ctx.perf.set_profiling(false);
    assert_eq!(ctx.gpu_timing_every(), GPU_TIMING_EVERY);
    let path = std::env::temp_dir().join(format!("ponko-pass-timing-{}.json", std::process::id()));
    ctx.perf.record_trace(&path, 4);
    assert_eq!(ctx.gpu_timing_every(), 1, "a trace is recording");
}
