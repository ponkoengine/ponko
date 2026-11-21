//! Offscreen GPU tests of the GPU memory report ([`GpuMemory`]): the
//! quality presets change the classes they should by what they should (as
//! far as the device's texture size limit lets them), and the total agrees
//! with the platform's own figure where it reports one. Skips without a GPU.
use std::process::Command;

use super::memory::{AllocationKind, BufferUsage, GpuAllocation, MemoryClass as C, MemoryTally, adapter_bytes};
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
    tally(f).finish()
}

/// The tally behind [`memory`], before it is summed up.
fn tally(f: &Fixture) -> MemoryTally {
    let mut t = MemoryTally::default();
    f.renderer.tally_memory(&mut t);
    for texture in &f.textures {
        t.texture(C::Textures, "texture", &texture.texture);
    }
    t.texture(C::FrameTargets, "window", &f.output);
    if let Some(timer) = &f.timer {
        timer.tally_memory(&mut t);
    }
    t
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
        // What the preset asks for, capped at the largest texture the
        // device makes (2048 on the WebGL 2 and baseline tiers, where High
        // and Ultra get the same maps).
        let limit = f.device.limits().max_texture_dimension_2d;
        let lighting = &f.ctx.scene3d.lighting;
        assert_eq!(
            (size, layers),
            (lighting.shadow_size(limit), lighting.shadow_cascades),
            "{quality:?}: the preset's shadow map, within the device's limit of {limit}"
        );
        let wanted_atlas = (lighting.light_shadow_budget() > 0).then(|| lighting.light_shadow_atlas_size(limit));
        assert_eq!(
            f.renderer.light_shadow_map(),
            wanted_atlas,
            "{quality:?}: the preset's atlas"
        );
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
    // Each step up makes the maps and atlases bigger where the device
    // allows bigger ones, never smaller; and the total grows with them. On
    // a device capped at 2048² textures, High and Ultra hold the same
    // (asserted above against the presets' sizes).
    let limit = f.device.limits().max_texture_dimension_2d;
    for (a, b) in [(low, medium), (medium, high), (high, ultra)] {
        assert!(a.shadows <= b.shadows && a.atlases <= b.atlases, "{a}\n{b}");
        assert!(a.total <= b.total, "{a}\n{b}");
        if (a.shadows, a.atlases) != (b.shadows, b.atlases) {
            assert!(a.total < b.total, "{a}\n{b}");
        }
    }
    // Low has no light atlas; Medium (2048²) has one at any limit.
    assert!(low.atlases < medium.atlases && medium.atlases <= high.atlases);
    // More cascades from Low to Medium to High, whatever the limit.
    assert!(low.shadows < medium.shadows && medium.shadows < high.shadows);
    if limit >= 8192 {
        // Ultra's 4096² cascades and 8192² atlas fit.
        assert!(high.shadows < ultra.shadows && high.atlases < ultra.atlases);
    }
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

/// Direct3D 12 places every buffer and texture on its own range of whole
/// 64 KiB pages (`D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT`; wgpu asks
/// for no smaller alignment), so its allocator counts a 256-byte uniform
/// buffer as 64 KiB.
const D3D12_PAGE: u64 = 64 << 10;

/// The allocations wgpu makes for itself in its allocator's report: its
/// internal buffers (labelled `(wgpu internal) ...`: the zero buffer,
/// indirect draw validation) and, on Direct3D 12, a small buffer of
/// sampler indices for every bind group that holds samplers, each a whole
/// 64 KiB page. They are not the engine's textures and buffers, so they are
/// left out of the comparison; their bytes and count are printed.
fn wgpu_own(report: &wgpu::AllocatorReport) -> (u64, usize) {
    let own: Vec<u64> = report
        .allocations
        .iter()
        .filter(|a| a.name.contains("(wgpu internal)") || a.name.contains("Internal Sampler Index Buffer"))
        .map(|a| a.size)
        .collect();
    (own.iter().sum(), own.len())
}

/// How far the count may be from the figure where the allocator's report
/// gives each allocation (Direct3D 12, Vulkan): less than a 720p RGBA8
/// target (3.5 MiB), so forgetting one fails.
const REPORTED_SLACK: u64 = 3 << 20;

/// The label of the textures [`placed_by_driver`] makes.
const PROBE: &str = "gpu memory probe";

/// What the allocator gives a texture like `texture`: a probe made the same
/// way, looked up in the allocator's report by its label, then destroyed
/// (and freed by a poll, so the next probe is the only one). `None` where
/// there is no report.
fn placed_by_driver(device: &wgpu::Device, texture: &wgpu::Texture) -> Option<u64> {
    let probe = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(PROBE),
        size: texture.size(),
        mip_level_count: texture.mip_level_count(),
        sample_count: texture.sample_count(),
        dimension: texture.dimension(),
        format: texture.format(),
        usage: texture.usage(),
        view_formats: &[],
    });
    let report = device.generate_allocator_report();
    probe.destroy();
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let sizes: Vec<u64> = report?
        .allocations
        .iter()
        .filter(|a| a.name == PROBE)
        .map(|a| a.size)
        .collect();
    assert_eq!(sizes.len(), 1, "one probe in the report: {sizes:?}");
    Some(sizes[0])
}

/// The adapter's figure once what was dropped has been given back: the
/// smallest of a few readings, each after waiting for the GPU, stopping
/// once two in a row agree. Memory a driver frees late (Metal releases a
/// dropped buffer or texture after its last command buffer completes,
/// which on a loaded machine can come after the poll that waited for it)
/// only ever leaves the figure, so the smallest reading is the one to
/// compare; a resource the engine forgets to count is in every reading
/// and still breaks the comparison.
fn settled_adapter_bytes(device: &wgpu::Device) -> u64 {
    let read = || {
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        adapter_bytes(device).0.expect("reported above")
    };
    let mut last = read();
    let mut least = last;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let now = read();
        least = least.min(now);
        if now == last {
            break;
        }
        last = now;
    }
    least
}

/// Draws each preset and compares the counted total with the adapter's
/// figure, where the platform has one. Does nothing unless run as the
/// child of the test above.
///
/// The engine counts the sizes it asks for; a driver lays textures out in
/// its own tiles and keeps memory of its own, and each backend differs:
///
/// - Metal (`currentAllocatedSize`, everything the process holds): on an
///   Apple M4 Max a feature switched on adds 0.8% more to the device's
///   figure than to the count for power-of-two shadow maps and 2-11% for
///   screen-sized targets (16 KB tiles), and wgpu holds 1.1 MB before the
///   engine makes anything. So at a render scale of 1 the two agree within
///   5% (Medium 3.7%, High 3.3%, Ultra 1.8% at 720p), and Low's two-thirds
///   scale, whose odd sizes tile worst, within 10% (9.2%).
/// - wgpu's allocator (Direct3D 12, Vulkan): the report names each
///   allocation, so what wgpu makes for itself ([`wgpu_own`]) is taken
///   out, and the engine's resources are counted as the allocator places
///   them: whole 64 KiB pages on Direct3D 12 ([`D3D12_PAGE`]), and a
///   multisampled texture at the size the driver gives the same texture
///   ([`placed_by_driver`]). The driver lays samples out in its own tiles:
///   on the Windows runner's software adapter (WARP) a 4x 720p colour
///   target takes 6 MiB more than its samples, and a 1x1 4x depth stand-in
///   7.6 MiB; together about 17-21 MiB at every preset. What is left is
///   small and fixed, so on a hardware adapter the two agree within
///   [`REPORTED_SLACK`] as well as the 5% and 10%: a forgotten
///   screen-sized target breaks it. A software adapter (WARP) places tiny
///   multisampled textures erratically, so only the 5% and 10% hold there.
///
/// A resource the engine forgets to count (a missing 16 MB light atlas,
/// a frame target) still breaks either comparison.
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
    let page = if f.backend == wgpu::Backend::Dx12 {
        D3D12_PAGE
    } else {
        1
    };
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
        let t = tally(&f);
        let placed = t.placed_bytes(page, |t| {
            (t.sample_count() > 1).then(|| placed_by_driver(&f.device, t)).flatten()
        });
        let m = t.finish();
        let raw = settled_adapter_bytes(&f.device);
        println!("{m}");
        // The engine's share of the figure, and what it is compared with.
        let (counted, adapter, slack) = match f.device.generate_allocator_report() {
            Some(report) => {
                let (own, n) = wgpu_own(&report);
                println!(
                    "{quality:?}: wgpu's own allocations: {n}, {:.2} MB; the engine's {} placed in {page}-byte pages: {:.2} MB",
                    own as f64 / 1048576.0,
                    m.total,
                    placed as f64 / 1048576.0
                );
                // A software adapter places tiny multisampled textures
                // erratically (a 1x1 4x depth stand-in takes 7.6 MiB on
                // WARP, which a probe made the same way doesn't always
                // reproduce): only the 5% and 10% bounds hold there.
                let slack = (!f.software).then_some(REPORTED_SLACK);
                (placed, raw - own, slack)
            }
            None => (m.total, raw, None),
        };
        let ratio = counted as f64 / adapter as f64;
        println!(
            "{quality:?}: counted {:.2} MB, adapter {:.2} MB (raw {:.2} MB), ratio {ratio:.4}",
            counted as f64 / 1048576.0,
            adapter as f64 / 1048576.0,
            raw as f64 / 1048576.0
        );
        let within = if f.ctx.scene3d.post.render_scale == 1.0 {
            0.05
        } else {
            0.10
        };
        let apart = counted.abs_diff(adapter);
        if (ratio - 1.0).abs() >= within || slack.is_some_and(|slack| apart >= slack) {
            off.push(format!(
                "{quality:?}: counted {counted} bytes, the adapter reports {adapter} (raw {raw})"
            ));
            // Where the difference is: this preset's largest allocations.
            if let Some(mut report) = f.device.generate_allocator_report() {
                report.allocations.sort_by_key(|a| std::cmp::Reverse(a.size));
                println!("{quality:?}: the allocator's largest allocations:");
                for a in report.allocations.iter().take(40) {
                    println!("{:>12} {}", a.size, a.name);
                }
            }
        }
    }
    assert!(off.is_empty(), "{off:#?}");
}

/// Every allocation the fixture holds, largest first, with the sums.
fn listing(f: &Fixture) -> (GpuMemory, Vec<GpuAllocation>) {
    let mut t = MemoryTally::listing();
    f.renderer.tally_memory(&mut t);
    t.texture(C::FrameTargets, "window", &f.output);
    t.finish_listed()
}

/// The one entry named `label`.
fn entry<'a>(list: &'a [GpuAllocation], label: &str) -> &'a GpuAllocation {
    let mut found = list.iter().filter(|a| a.label == label);
    let one = found.next().unwrap_or_else(|| panic!("no `{label}` in {list:#?}"));
    assert!(found.next().is_none(), "one `{label}` only");
    one
}

#[test]
fn the_listing_names_the_render_targets_and_shadow_maps_the_renderer_made() {
    let Some(mut f) = Fixture::sized(W, H) else { return };
    let mut shadow_sizes = Vec::new();
    for quality in [GraphicsQuality::Low, GraphicsQuality::Ultra] {
        f.ctx.scene3d.set_quality(quality);
        f.draw();
        f.draw();
        let (m, list) = listing(&f);
        // The listing adds up to the sums, class by class.
        assert_eq!(list.iter().map(|a| a.bytes).sum::<u64>(), m.total, "{quality:?}");
        for (class, bytes) in m.classes() {
            let listed: u64 = list.iter().filter(|a| a.class == class).map(|a| a.bytes).sum();
            assert_eq!(listed, bytes, "{quality:?}: {class}");
        }
        assert!(list.windows(2).all(|w| w[0].bytes >= w[1].bytes), "largest first");

        // The sun's shadow map: one Depth32Float layer per cascade, as made.
        let (size, layers) = f.renderer.shadow_map();
        let sun = entry(&list, "3d shadow map");
        assert_eq!(sun.class, "shadows");
        assert_eq!(sun.size, Some([size, size, layers]), "{quality:?}");
        assert_eq!(sun.format.as_deref(), Some("Depth32Float"));
        assert_eq!(sun.bytes, u64::from(size) * u64::from(size) * 4 * u64::from(layers));
        shadow_sizes.push(sun.size);
        // The lights' atlas, once made.
        if let Some(width) = f.renderer.light_shadow_map() {
            let atlas = entry(&list, "3d light shadow atlas");
            assert_eq!((atlas.class, atlas.size), ("atlases", Some([width, width, 1])));
        }

        // The scene-sized targets: at the scene's size (the render scale's),
        // HDR color multisampled as the depth is, its resolve single-sampled.
        let (w, h) = f.renderer.scene_size().expect("drawn");
        let color = entry(&list, "3d color");
        let depth = entry(&list, "3d depth");
        let hdr = entry(&list, "3d hdr");
        for t in [color, depth, hdr, entry(&list, "3d scene copy")] {
            assert_eq!(
                (t.class, t.size),
                ("frame_targets", Some([w, h, 1])),
                "{quality:?}: {t:?}"
            );
        }
        assert_eq!(color.samples, depth.samples);
        assert_eq!(hdr.samples, Some(1));
        assert_eq!(color.format, hdr.format);
        assert_eq!(hdr.format.as_deref(), Some("Rgba16Float"));
        let window = entry(&list, "window");
        assert_eq!(window.size, Some([W, H, 1]));

        // Buffers carry their usage, textures their format.
        for a in &list {
            match a.kind {
                AllocationKind::Texture => assert!(a.format.is_some() && a.usage.is_none(), "{a:?}"),
                AllocationKind::Buffer => assert!(a.usage.is_some() && a.format.is_none(), "{a:?}"),
                AllocationKind::Other => assert!(a.size.is_none(), "{a:?}"),
            }
        }
        assert_eq!(entry(&list, "frame uniforms").usage, Some(BufferUsage::Uniform));
        assert!(
            list.iter()
                .any(|a| a.label == "mesh arena" && a.class == "meshes" && a.bytes > 0)
        );
    }
    // The preset change shows: Ultra's sun map is bigger than Low's.
    assert_ne!(shadow_sizes[0], shadow_sizes[1], "{shadow_sizes:?}");
    // A default tally lists nothing.
    let mut t = MemoryTally::default();
    f.renderer.tally_memory(&mut t);
    assert!(t.finish_listed().1.is_empty());
}
