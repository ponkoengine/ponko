//! Offscreen GPU tests of when a game times its GPU passes: one frame in
//! [`GPU_TIMING_EVERY`] by default, every frame while something reads every
//! frame (the HUD, profiling, a trace, automatic quality), on a whole game
//! context drawn by the production renderer into a texture (no window).
//! Skips without a GPU or without timestamp queries.
use super::render_test_support::*;
use crate::d3::Transform as Transform3D;
use crate::perf::{GPU_TIMING_EVERY, GpuTiming};
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
///
/// Every frame is timed; a frame's sample still can't arrive when the GPU
/// left none of its timestamps usable (`timer.rs`, `read_frame`), which a
/// heavily loaded machine does now and then. So a lone frame may bring no
/// new sample, never two in a row, and at most two of the 32; sampling one
/// frame in two (or in sixteen, as without automatic quality) still fails.
/// "Fresh" is also a wall-clock age (250 ms): it is asserted where the two
/// frames a sample spans took less than that.
#[test]
fn automatic_quality_still_gets_a_fresh_gpu_sample_every_frame() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    ctx.scene3d
        .set_quality_autotune(Some(crate::render::QualityAutotune::default()));
    assert_eq!(ctx.gpu_timing_every(), 1, "automatic quality reads every frame");
    let mut last = 0;
    let (mut new, mut missed, mut missed_last) = (0, 0, false);
    let mut took = std::time::Duration::ZERO;
    for i in 0..40 {
        let start = std::time::Instant::now();
        frame(&mut ctx);
        let (before, now) = (took, start.elapsed());
        took = now;
        let renderer = ctx.gfx.renderer.as_ref().unwrap();
        let Some(sample) = renderer.quality_sample() else {
            assert!(i < 4, "no GPU sample after {i} frames");
            continue;
        };
        if i >= 8 {
            if sample.sequence > last {
                new += 1;
                missed_last = false;
                if before + now < std::time::Duration::from_millis(200) {
                    assert!(sample.fresh, "frame {i}'s sample is stale");
                }
            } else {
                eprintln!("frame {i} brought no new sample (its timestamps were unusable)");
                assert!(!missed_last, "frames {} and {i} both brought no new sample", i - 1);
                missed += 1;
                missed_last = true;
            }
        }
        last = sample.sequence;
    }
    assert!(
        missed <= 2 && new + missed == 32,
        "{new} of 32 frames brought a new sample"
    );
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

/// Frames timed over `n` frames after the setting settled: the first frame
/// after a change was begun before it, so it isn't counted.
fn timed_with(ctx: &mut Ctx, timing: GpuTiming, n: u64) -> u64 {
    ctx.perf.set_gpu_timing(timing);
    frame(ctx);
    // Timestamps from before the change may still come back; wait them out.
    for _ in 0..4 {
        frame(ctx);
    }
    timed_over(ctx, n)
}

/// The setting alone, with profiling off, picks which frames are timed.
#[test]
fn the_gpu_timing_setting_alone_changes_which_frames_are_timed() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    assert!(!ctx.perf.profiling());
    assert_eq!(ctx.perf.gpu_timing(), GpuTiming::Auto);
    let auto = timed_with(&mut ctx, GpuTiming::Auto, 64);
    assert!((3..=5).contains(&auto), "{auto} of 64 frames timed by default");
    let every = timed_with(&mut ctx, GpuTiming::Every(1), 64);
    assert!(every >= 62, "{every} of 64 frames timed with Every(1)");
    let fourth = timed_with(&mut ctx, GpuTiming::Every(4), 64);
    assert!((15..=17).contains(&fourth), "{fourth} of 64 frames timed with Every(4)");
    let slow = timed_with(&mut ctx, GpuTiming::Every(40), 80);
    assert!((1..=3).contains(&slow), "{slow} of 80 frames timed with Every(40)");
    assert!(!ctx.perf.profiling(), "none of this turned profiling on");

    // Off: nothing is timed, and nothing stale is shown.
    let off = timed_with(&mut ctx, GpuTiming::Off, 64);
    assert_eq!(off, 0, "{off} of 64 frames timed with Off");
    assert!(ctx.perf.gpu_passes().is_empty(), "no passes are listed while off");
    assert_eq!(ctx.gfx.renderer.as_ref().unwrap().stats().gpu_ms, None);

    // Back to the default: sampling resumes and the numbers return.
    let again = timed_with(&mut ctx, GpuTiming::Auto, 64);
    assert!((3..=5).contains(&again), "{again} of 64 frames timed after Off");
    assert!(!ctx.perf.gpu_passes().is_empty());
}

/// A frame with no usable GPU time says why: not ready before the first
/// timestamps come back, off while the game turned timing off, a lost
/// device once the device is lost; `None` while frames are timed.
#[test]
fn a_frame_without_gpu_time_says_why() {
    use crate::perf::GpuUntimed;
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    ctx.perf.set_gpu_timing(GpuTiming::Every(1));
    frame(&mut ctx);
    assert_eq!(
        ctx.perf.gpu_untimed(),
        Some(GpuUntimed::NotReady),
        "the first frame's times aren't back"
    );
    let timed = timed_over(&mut ctx, 8);
    assert!(timed > 0, "{timed} frames timed");
    assert_eq!(ctx.perf.gpu_untimed(), None, "timed frames have no reason");
    timed_with(&mut ctx, GpuTiming::Off, 4);
    assert_eq!(ctx.perf.gpu_untimed(), Some(GpuUntimed::Off));
    timed_with(&mut ctx, GpuTiming::Every(1), 8);
    assert_eq!(ctx.perf.gpu_untimed(), None);
    // As the device-lost callback does when the driver resets.
    let renderer = ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    renderer
        .device_lost_flag()
        .store(true, std::sync::atomic::Ordering::Release);
    frame(&mut ctx);
    assert_eq!(ctx.perf.gpu_untimed(), Some(GpuUntimed::DeviceLost));
}

/// The overlay, a trace and automatic quality read every frame, so they get
/// every frame whatever the game chose, and the choice comes back after.
#[test]
fn the_overlay_and_automatic_quality_still_get_every_frame_whatever_the_setting() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    ctx.perf.set_gpu_timing(GpuTiming::Off);
    assert_eq!(ctx.gpu_timing_every(), 0);
    ctx.perf.set_hud(true);
    assert_eq!(ctx.gpu_timing_every(), 1, "the overlay reads every frame");
    frame(&mut ctx);
    let hud = timed_over(&mut ctx, 32);
    assert!(hud >= 30, "{hud} of 32 frames timed under the overlay with Off");
    assert!(!ctx.perf.gpu_passes().is_empty(), "the overlay has passes to show");
    ctx.perf.set_hud(false);
    assert_eq!(ctx.gpu_timing_every(), 0, "Off again once nothing reads");
    ctx.scene3d
        .set_quality_autotune(Some(crate::render::QualityAutotune::default()));
    ctx.perf.set_gpu_timing(GpuTiming::Every(8));
    assert_eq!(ctx.gpu_timing_every(), 1, "automatic quality reads every frame");
    ctx.scene3d.set_quality_autotune(None);
    assert_eq!(ctx.gpu_timing_every(), 8);
}

/// Profiling and the setting are separate: an explicit choice wins over
/// profiling, and `Auto` keeps what profiling did before the setting.
#[test]
fn profiling_and_the_gpu_timing_setting_are_separate() {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(64.0, 64.0));
    assert_eq!(ctx.gpu_timing_every(), GPU_TIMING_EVERY);
    // The setting without profiling.
    ctx.perf.set_gpu_timing(GpuTiming::Every(1));
    assert!(!ctx.perf.profiling());
    assert_eq!(ctx.gpu_timing_every(), 1);
    ctx.perf.set_gpu_timing(GpuTiming::Every(0));
    assert_eq!(ctx.gpu_timing_every(), 1, "0 counts as 1");
    ctx.perf.set_gpu_timing(GpuTiming::Off);
    assert_eq!(ctx.gpu_timing_every(), 0);
    // Profiling without the setting.
    ctx.perf.set_gpu_timing(GpuTiming::Auto);
    ctx.perf.set_profiling(true);
    assert_eq!(ctx.gpu_timing_every(), 1);
    // Profiling does not override a choice.
    ctx.perf.set_gpu_timing(GpuTiming::Off);
    assert_eq!(ctx.gpu_timing_every(), 0);
    ctx.perf.set_gpu_timing(GpuTiming::Every(4));
    assert_eq!(ctx.gpu_timing_every(), 4);
    // A trace still overrides it.
    let path = std::env::temp_dir().join(format!("ponko-gpu-setting-{}.json", std::process::id()));
    ctx.perf.record_trace(&path, 4);
    assert_eq!(ctx.gpu_timing_every(), 1, "a trace is recording");
}

/// What each choice costs the game thread in heap allocations per frame,
/// the numbers the guide gives. Only the order is asserted: the counts
/// depend on the graphics library and the adapter.
#[test]
fn allocations_per_frame_fall_as_gpu_timing_gets_rarer() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    const FRAMES: u64 = 64;
    let per_frame = |ctx: &mut Ctx, timing: GpuTiming| -> f64 {
        ctx.perf.set_gpu_timing(timing);
        for _ in 0..8 {
            frame(ctx);
        }
        let ((), allocations) = crate::three::lights::tests::count_allocations(|| {
            for _ in 0..FRAMES {
                frame(ctx);
            }
        });
        allocations as f64 / FRAMES as f64
    };
    let every = per_frame(&mut ctx, GpuTiming::Every(1));
    let sixteenth = per_frame(&mut ctx, GpuTiming::Auto);
    let fourth = per_frame(&mut ctx, GpuTiming::Every(4));
    let off = per_frame(&mut ctx, GpuTiming::Off);
    eprintln!(
        "game-thread allocations per frame: every frame {every:.1}, every 4th {fourth:.1}, \
         default (every 16th) {sixteenth:.1}, off {off:.1}"
    );
    assert!(
        every > fourth,
        "every frame ({every}) costs more than every 4th ({fourth})"
    );
    assert!(
        fourth > sixteenth,
        "every 4th ({fourth}) costs more than every 16th ({sixteenth})"
    );
    assert!(sixteenth > off, "every 16th ({sixteenth}) costs more than off ({off})");
}
