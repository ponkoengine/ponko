//! Offscreen GPU tests of the GPU memory report ([`GpuMemory`]): the
//! quality presets change the classes they should by what they should, and
//! the total is within a few percent of the platform's own figure where it
//! reports one. Skips without a GPU.
use std::process::Command;

use super::memory::{MemoryClass as C, MemoryTally, adapter_bytes};
use super::quality_render_test::Fixture;
use crate::perf::GpuMemory;
use crate::render::GraphicsQuality;

/// A 720p window: frame targets of a realistic size next to the shadow maps.
const W: u32 = 1280;
const H: u32 = 720;

/// Everything the fixture holds on the GPU: the 3D renderer's, and the
/// textures, window and timer the fixture made for it (what the engine's
/// renderer would add).
fn memory(f: &Fixture) -> GpuMemory {
    let mut t = MemoryTally::default();
    f.renderer.tally_memory(&mut t);
    for texture in &f.textures {
        t.texture(C::Textures, &texture.texture);
    }
    t.texture(C::FrameTargets, &f.output);
    if let Some(timer) = &f.timer {
        timer.tally_memory(&mut t);
    }
    t.finish()
}

/// A 1x1 depth stand-in's bytes.
const STAND_IN: u64 = 4;

#[test]
fn quality_presets_change_shadows_atlases_and_frame_targets_as_they_ask() {
    let Some(mut f) = Fixture::sized(W, H) else { return };
    let mut seen = Vec::new();
    let mut uniforms = None;
    for quality in [
        GraphicsQuality::Low,
        GraphicsQuality::Medium,
        GraphicsQuality::High,
        GraphicsQuality::Ultra,
        GraphicsQuality::Low,
    ] {
        f.ctx.scene3d.set_quality(quality);
        f.draw();
        f.draw();
        let m = memory(&f);
        let (size, layers) = f.renderer.shadow_map();
        // Depth32Float: 4 bytes a texel, one layer per cascade; and the
        // shadow passes' uniforms, the same whatever the preset.
        let map = u64::from(size) * u64::from(size) * 4 * u64::from(layers);
        assert!(
            m.shadows > map,
            "{quality:?}: the {size}² x {layers} shadow map is {map} bytes, counted {}",
            m.shadows
        );
        let rest = *uniforms.get_or_insert(m.shadows - map);
        assert_eq!(m.shadows - map, rest, "{quality:?}: only the map changes");
        assert!(rest < 1 << 20, "the uniforms are small: {rest}");
        let atlas = f
            .renderer
            .light_shadow_map()
            .map_or(0, |a| u64::from(a) * u64::from(a) * 4);
        assert_eq!(
            m.atlases,
            atlas + STAND_IN,
            "{quality:?}: the light shadow atlas, and the 1x1 stand-in frames bind without one"
        );
        assert_eq!(m.total, m.classes().iter().map(|c| c.1).sum::<u64>());
        // No skinned models: only the joint buffers made up front.
        assert!(m.skinning < 64 << 10, "{quality:?}: {}", m.skinning);
        assert_eq!(m.gpu_buffers, 0, "{quality:?}: the scene has none");
        println!("{quality:?}: {m}");
        seen.push(m);
    }
    let [low, medium, high, ultra, low_again] = &seen[..] else {
        unreachable!()
    };
    // Bigger maps and atlases with each step up.
    for (a, b) in [(low, medium), (medium, high), (high, ultra)] {
        assert!(a.shadows < b.shadows, "{a}\n{b}");
        assert!(a.total < b.total, "{a}\n{b}");
    }
    assert!(low.atlases < high.atlases && high.atlases < ultra.atlases);
    // Low draws at two thirds of the size without reflections or
    // antialiasing; High at full size with both.
    assert!(
        low.frame_targets * 2 < high.frame_targets,
        "low {} against high {}",
        low.frame_targets,
        high.frame_targets
    );
    // Back to Low: what Ultra made is freed again.
    assert_eq!(
        (low_again.shadows, low_again.atlases, low_again.frame_targets),
        (low.shadows, low.atlases, low.frame_targets),
        "{low}\n{low_again}"
    );
    // Meshes are the scene's, whatever the preset.
    assert!(low.meshes > 0 && seen.iter().all(|m| m.meshes == low.meshes));
}

/// Set in the child process that measures against the adapter's figure.
const CHILD: &str = "PONKO_GPU_MEMORY_CHILD";

/// The platform's figure counts everything this process allocated on the
/// GPU (on Metal the device is shared by every test running beside it), so
/// it is compared in a process of its own, running only
/// [`adapter_figure_child`].
#[test]
fn the_total_is_within_a_few_percent_of_the_adapters_own_figure() {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "render::gpu_memory_render_test::adapter_figure_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    println!("{stdout}");
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("1 passed"), "the child ran: {stdout}");
}

/// Draws each preset and compares the counted total with the adapter's
/// figure, where the platform has one. Does nothing unless run as the
/// child of the test above.
///
/// The engine counts the sizes it asks for; a driver lays textures out in
/// its own tiles and keeps memory of its own. On an Apple M4 Max (Metal)
/// a feature switched on adds 0.8% more to the device's figure than to the
/// count for power-of-two shadow maps and 2-11% for screen-sized targets
/// (16 KB tiles), and wgpu holds 1.1 MB before the engine makes anything.
/// So at a render scale of 1 the two agree within 5% (Medium 3.7%, High
/// 3.3%, Ultra 1.8% at 720p), and Low's two-thirds scale, whose odd sizes
/// tile worst, within 10% (9.2%).
#[test]
fn adapter_figure_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let Some(mut f) = Fixture::sized(W, H) else { return };
    if let (None, why) = adapter_bytes(&f.device) {
        println!("skipped: {why}");
        return;
    }
    let mut off = Vec::new();
    for quality in [
        GraphicsQuality::Low,
        GraphicsQuality::Medium,
        GraphicsQuality::High,
        GraphicsQuality::Ultra,
    ] {
        f.ctx.scene3d.set_quality(quality);
        f.draw();
        f.draw();
        // What the draws dropped (the readback buffer) is freed by a poll.
        f.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let m = memory(&f);
        let counted = m.total;
        let adapter = adapter_bytes(&f.device).0.expect("reported above");
        println!("{m}");
        let ratio = counted as f64 / adapter as f64;
        println!(
            "{quality:?}: counted {:.2} MB, adapter {:.2} MB, ratio {ratio:.4}",
            counted as f64 / 1048576.0,
            adapter as f64 / 1048576.0
        );
        let within = if f.ctx.scene3d.post.render_scale == 1.0 {
            0.05
        } else {
            0.10
        };
        if (ratio - 1.0).abs() >= within {
            off.push(format!(
                "{quality:?}: counted {counted} bytes, the adapter reports {adapter}"
            ));
        }
    }
    assert!(off.is_empty(), "{off:#?}");
}
