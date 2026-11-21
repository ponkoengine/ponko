//! Offscreen GPU tests of the water kit's newer parts, on a whole game
//! context drawn by the production renderer into a texture (no window):
//! waves from a spectrum (the GPU's FFT works out the field the CPU does,
//! a GPU without compute shaders gets the CPU's field uploaded, the drawn
//! surface is where the CPU height query says, the sea has waves) and
//! contact ripples on a WebGL2-class GPU (the CPU's simulation leaves what
//! the compute pass does). `spectrum_waves_cost` (ignored, run by hand)
//! measures what the spectrum costs at each size. Skips without a GPU.
use super::gpu::Renderer;
use super::regression_stage::{REFERENCE, luma, mean_color};
use super::render_test_support::*;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{FrameStatus, GpuTier, MaterialDesc, Model, TargetCamera, Tonemap};
use crate::time::FrameTiming;
use crate::water::{
    SpectrumParts, Water, WaterDesc, WaterQuality, WaterReflection, WaveSpectrum, Waves, shaders, water_mesh,
};

/// A game context whose renderer draws offscreen, on a device with
/// `tier`'s limits (`None`: a desktop game's), with GPU timestamps if
/// `timestamps` and the adapter has them. `None` without a GPU.
pub(super) fn offscreen(size: (u32, u32), tier: Option<GpuTier>, timestamps: bool) -> Option<Box<Ctx>> {
    let gpu = gpu_with(|adapter| {
        let engine = wgpu::Limits {
            max_sampled_textures_per_shader_stage: adapter.limits().max_sampled_textures_per_shader_stage.clamp(16, 24),
            ..wgpu::Limits::default()
        };
        Ok(wgpu::DeviceDescriptor {
            required_limits: tier.map_or(engine.clone(), |t| t.device_limits(engine)),
            required_features: if timestamps {
                adapter.features() & wgpu::Features::TIMESTAMP_QUERY
            } else {
                wgpu::Features::empty()
            },
            ..Default::default()
        })
    })?;
    let mut ctx = Box::new(Ctx::new(1.0 / 60.0, Vec2::new(size.0 as f32, size.1 as f32)));
    ctx.audio.headless = true;
    ctx.gfx.renderer = Some(Renderer::offscreen(gpu, size));
    // Behind everything, the flat sky's colour (as the kit's other suite).
    ctx.gfx.clear(Color::rgb(0.55, 0.7, 0.95));
    let scene = &mut ctx.scene3d;
    scene.post.bloom = 0.0;
    scene.post.tonemap = Tonemap::None;
    scene.sky.atmosphere = false;
    scene.sky.sky_color = Color::rgb(0.55, 0.7, 0.95);
    scene.sky.ground_color = Color::rgb(0.35, 0.3, 0.25);
    let lighting = &mut scene.lighting;
    lighting.shadows = false;
    lighting.sun_direction = vec3(-0.45, -0.6, -0.65);
    lighting.sun_color = Color::WHITE;
    lighting.sun_intensity = 2.5;
    lighting.ambient_intensity = 1.0;
    Some(ctx)
}

/// One frame as a game runs it: the clock moves on a 60th of a second,
/// the water kit's update runs, the frame is drawn.
pub(super) fn frame(ctx: &mut Ctx) {
    ctx.begin_frame(FrameTiming {
        dt: 1.0 / 60.0,
        seconds: 1.0 / 60.0,
        fixed_steps: 0,
        alpha: 0.0,
    });
    crate::water::update_water(ctx);
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen, "the frame was drawn");
}

/// The frame as an image (a screenshot of the next frame).
pub(super) fn image(ctx: &mut Ctx, name: &str) -> Image {
    let path = std::env::temp_dir().join(format!("ponko-water-kit-{}-{name}.png", std::process::id()));
    ctx.gfx.save_screenshot(&path);
    frame(ctx);
    let png = ::image::open(&path).expect("the screenshot was saved").to_rgba8();
    let _ = std::fs::remove_file(&path);
    let image = Image {
        width: png.width(),
        height: png.height(),
        rgba: png.into_raw(),
    };
    keep_image(&image);
    image
}

/// The texels of render target `target` as the GPU holds them (RGBA half
/// floats, row by row).
fn read_target(ctx: &Ctx, target: crate::render::RenderTarget) -> Vec<[f32; 4]> {
    let renderer = ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    let (device, queue, texture) = renderer.texture_for_test(target.texture().id);
    let bytes = read_texture(device, queue, texture.expect("the target is on the GPU"));
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|p| std::array::from_fn(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
        .collect()
}

/// A sea from a spectrum, on a small square (64 texels) so the CPU's
/// FFT is quick in a debug build.
fn spectrum_sea() -> WaterDesc {
    WaterDesc {
        ripples: false,
        underwater: false,
        ..WaterDesc::sea(0.0, vec2(0.8, -0.6), 1.5)
            .with_waves(Waves::spectrum(
                WaveSpectrum::new(vec2(8.0, -6.0), 1.5).with_tile(96.0, 64),
            ))
            .with_quality(WaterQuality::High)
            .with_reflection(WaterReflection::Sky)
    }
}

/// The largest difference between the GPU's field and the CPU's at `t`,
/// per channel (height, sway x, sway z).
fn field_error(gpu: &[[f32; 4]], water: &Water, t: f32) -> [f32; 3] {
    let cpu = water.desc().waves.spectrum_field().unwrap().field(t, false);
    let mut worst = [0.0f32; 3];
    for (g, c) in gpu.iter().zip(&cpu.texels) {
        for i in 0..3 {
            worst[i] = worst[i].max((g[i] - c[i]).abs());
        }
    }
    worst
}

/// The field target of the water on `entity`, and whether the GPU's
/// passes make it.
fn field_of(ctx: &Ctx, entity: Entity) -> (crate::render::RenderTarget, bool) {
    let water = ctx.world.get::<Water>(entity).unwrap();
    let SpectrumParts { field, gpu } = water.parts.spectrum.expect("waves from a spectrum have a field");
    (field, gpu.is_some())
}

/// Spawns `desc` and draws `frames` frames; returns the GPU's field and
/// how far it is from the CPU's now and a second later (the negative
/// control), and whether the GPU's passes made it.
fn spectrum_field(tier: Option<GpuTier>, frames: usize) -> Option<([f32; 3], [f32; 3], bool, f32)> {
    let mut ctx = offscreen((64, 64), tier, false)?;
    ctx.scene3d.camera.position = vec3(0.0, 6.0, 10.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let e = Water::spawn(&mut ctx, spectrum_sea());
    for _ in 0..frames {
        frame(&mut ctx);
    }
    let t = ctx.time.elapsed() as f32;
    let (target, on_gpu) = field_of(&ctx, e);
    let gpu = read_target(&ctx, target);
    let water = ctx.world.get::<Water>(e).unwrap();
    let highest = gpu.iter().map(|g| g[0].abs()).fold(0.0, f32::max);
    Some((
        field_error(&gpu, water, t),
        field_error(&gpu, water, t + 1.0),
        on_gpu,
        highest,
    ))
}

#[test]
fn the_gpu_fft_works_out_the_field_the_cpu_does() {
    let Some(gpu) = gpu() else { return };
    if !needs(&gpu.device, crate::render::GpuFeature::Compute) {
        return;
    }
    drop(gpu);
    let Some((now, later, on_gpu, highest)) = spectrum_field(None, 5) else {
        return;
    };
    eprintln!("GPU FFT vs CPU FFT: worst {now:?} m (a second off: {later:?} m), highest {highest} m");
    assert!(on_gpu, "a GPU with compute shaders runs the FFT passes");
    assert!(highest > 0.6, "the field has waves: up to {highest} m");
    // Half floats in every one of the 13 passes: millimetres.
    assert!(
        now.iter().all(|e| *e < 0.02),
        "the GPU's field is the CPU's to 2 cm: {now:?}"
    );
    assert!(later[0] > 0.2, "a second later the waves have moved on: {later:?}");
}

#[test]
fn without_compute_shaders_the_cpu_uploads_the_same_field() {
    let Some((now, later, on_gpu, highest)) = spectrum_field(Some(GpuTier::WebGl2), 3) else {
        return;
    };
    eprintln!("uploaded vs CPU field: worst {now:?} m (a second off: {later:?} m), highest {highest} m");
    assert!(!on_gpu, "WebGL2 has no compute passes: the CPU works the field out");
    assert!(highest > 0.6, "the uploaded field has waves: up to {highest} m");
    assert!(now.iter().all(|e| *e < 0.005), "only half floats' rounding: {now:?}");
    assert!(later[0] > 0.2, "a second later the waves have moved on: {later:?}");
}

/// The probe's surface: each drawn point writes where it is, unlit and
/// opaque, scaled into the range where half floats are finest (x and z
/// over 32 m, height over 8 m, plus one so the water is never zero).
const PROBE_SURFACE: &str = "
fn surface(s: Surface) -> Surface {
    var out = s;
    out.unlit = true;
    out.albedo = vec3<f32>(0.0);
    out.emissive = vec3<f32>(s.world.x / 32.0 + 0.5, s.world.y / 8.0 + 1.0, s.world.z / 32.0 + 0.5);
    return out;
}";

#[test]
fn the_drawn_spectrum_waves_are_where_the_cpu_height_query_says() {
    for tier in [None, Some(GpuTier::WebGl2)] {
        let Some(mut ctx) = offscreen((64, 64), tier, false) else {
            return;
        };
        ctx.scene3d.sky.sky_color = Color::BLACK;
        ctx.scene3d.sky.ground_color = Color::BLACK;
        ctx.gfx.clear(Color::BLACK);
        // The sea's own surface is hidden; a patch 28 m across, drawn by
        // the kit's vertex hook over the same field, is seen from straight
        // above by a target camera that keeps HDR values.
        let e = Water::spawn(&mut ctx, spectrum_sea());
        ctx.world.remove::<Model>(e);
        let (field, _) = field_of(&ctx, e);
        let water = ctx.world.get::<Water>(e).unwrap();
        let desc = WaterDesc {
            follow_camera: false,
            center: Vec2::ZERO,
            size: vec2(28.0, 28.0),
            ..water.desc().clone()
        };
        let source = format!(
            "{}{}{PROBE_SURFACE}",
            shaders::wave_library(&desc.waves),
            shaders::VERTEX
        );
        let shader = ctx.gfx.create_shader("water probe", &source);
        let probe = ctx.gfx.create_material(MaterialDesc {
            shader: Some(shader),
            double_sided: true,
            texture: Some(field.texture()),
            ..Default::default()
        });
        let mesh = ctx.gfx.create_mesh(&water_mesh(&desc), probe);
        ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
        let target = ctx.gfx.create_render_target(96, 96);
        let mut camera = ctx.scene3d.camera;
        camera.position = vec3(0.0, 60.0, 0.0);
        camera.look_at(vec3(0.0, 0.0, -0.001));
        camera.fov_y = 0.3;
        ctx.scene3d
            .passes
            .target_cameras
            .push(TargetCamera::new(camera, target));
        for _ in 0..4 {
            frame(&mut ctx);
        }
        let t = ctx.time.elapsed() as f32;
        let drawn: Vec<(Vec2, f32)> = read_target(&ctx, target)
            .iter()
            .filter(|p| p[1] > 0.5)
            .map(|p| (vec2((p[0] - 0.5) * 32.0, (p[2] - 0.5) * 32.0), (p[1] - 1.0) * 8.0))
            .collect();
        assert!(drawn.len() > 5000, "the probe saw the water: {} texels", drawn.len());
        let water = ctx.world.get::<Water>(e).unwrap();
        let off = |at: f32| {
            drawn
                .iter()
                .map(|&(p, h)| (h - water.desc().waves.height_at(p, at)).abs())
                .fold(0.0f32, f32::max)
        };
        let (lo, hi) = drawn
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &(_, h)| (lo.min(h), hi.max(h)));
        let (worst, later) = (off(t), off(t + 1.0));
        eprintln!(
            "{tier:?}: drawn vs CPU height: {} texels from {lo} to {hi} m, worst {worst} m (a second off: {later} m)",
            drawn.len()
        );
        // The tolerance: the mesh's flat triangles (vertices 18 cm apart
        // over texels 1.5 m apart), the GPU's half-float field (a few mm)
        // and half floats' rounding of the values read back (under 8 mm of
        // height and 16 mm along the ground, times the slope).
        assert!(
            worst < 0.04,
            "{tier:?}: the drawn surface is within 4 cm of the CPU height: off by up to {worst} m"
        );
        assert!(hi - lo > 0.5, "{tier:?}: and it has waves: {lo} to {hi}");
        assert!(later > 0.2, "{tier:?}: a different time's waves are off: {later} m");
    }
}

/// How much the water's brightness changes at the scale of waves (as the
/// kit's other suite measures it): the lower half, blurred, each spot's
/// distance from its row's mean.
fn swell(image: &Image, r: u32) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in (image.height / 2..image.height - r).step_by(2) {
        let row: Vec<f32> = (r..image.width - r)
            .step_by(2)
            .map(|x| luma(mean_color(image, x, y, r)))
            .collect();
        let mean = row.iter().sum::<f32>() / row.len() as f32;
        sum += row.iter().map(|v| (v - mean).abs()).sum::<f32>();
        n += row.len() as f32;
    }
    sum / n
}

/// The sea from `waves`, seen from 4 m up, after three frames.
fn sea_image(waves: Waves, name: &str) -> Option<(Image, wgpu::AdapterInfo)> {
    let mut ctx = offscreen((160, 160), None, false)?;
    ctx.scene3d.camera.position = vec3(0.0, 4.0, 8.0);
    ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -12.0));
    Water::spawn(&mut ctx, spectrum_sea().with_waves(waves));
    for _ in 0..2 {
        frame(&mut ctx);
    }
    let image = image(&mut ctx, name);
    let info = ctx.gfx.renderer.as_ref().unwrap().texture_for_test(0).0.adapter_info();
    Some((image, info))
}

#[test]
fn a_sea_from_a_spectrum_has_waves() {
    let rough_sea = Waves::spectrum(WaveSpectrum::new(vec2(8.0, -6.0), 2.5).with_tile(128.0, 128));
    let Some((image, info)) = sea_image(rough_sea, "spectrum") else {
        return;
    };
    let Some((calm, _)) = sea_image(Waves::none(), "flat") else {
        return;
    };
    let (rough, still) = (swell(&image, 8), swell(&calm, 8));
    eprintln!("spectrum sea: swell {rough} with waves, {still} flat");
    assert!(rough > 3.5, "waves light and shade the sea: {rough}");
    assert!(
        still < rough * 0.6,
        "flat water has only fine ripples: {still} vs {rough}"
    );
    check_reference(
        "water_spectrum_sea",
        &image,
        REFERENCE,
        "Water kit sea (High, sky reflection, crest foam) with Waves::spectrum: wind (8, -6) m/s, 2.5 m \
         significant height, a 128 m square of 128 texels, choppiness 1, seed 1; mesh following the camera; \
         sky-coloured background, flat sky light (0.55, 0.7, 0.95), sun (-0.45, -0.6, -0.65) at 2.5, ambient 1, \
         no tonemapping; camera at (0, 4, 8) looking at (0, 0, -12); 160x160, the offscreen game renderer \
         (4x multisampling); frame 3 at 60 fps.",
        &info,
    );
    reference_rejects("water_spectrum_sea", &calm, REFERENCE, "the waves off");
}

/// A lake with ripples, on a device with `tier`'s limits; touched in the
/// middle on the second frame, then drawn for `frames` more. Its ripple
/// field and whether a compute pass ran it.
fn touched_lake(tier: Option<GpuTier>, touch: Vec2, frames: usize) -> Option<(Vec<[f32; 4]>, bool, usize)> {
    let mut ctx = offscreen((64, 64), tier, false)?;
    ctx.scene3d.camera.position = vec3(0.0, 10.0, 12.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let e = Water::spawn(
        &mut ctx,
        WaterDesc {
            underwater: false,
            ripple_area: 32.0,
            ..WaterDesc::lake(Vec2::ZERO, vec2(32.0, 32.0), 0.0)
                .with_reflection(WaterReflection::Sky)
                .with_quality(WaterQuality::Medium)
        },
    );
    frame(&mut ctx);
    ctx.world.get_mut::<Water>(e).unwrap().touch(touch, 1.0, 0.2);
    for _ in 0..frames {
        frame(&mut ctx);
    }
    let water = ctx.world.get::<Water>(e).unwrap();
    let ripples = water.parts.ripples.expect("the lake has ripples");
    let side = ripples.target.width() as usize;
    Some((read_target(&ctx, ripples.target), ripples.compute.is_some(), side))
}

/// The strongest ripple on a ring `r` texels from the middle.
fn ring(texels: &[[f32; 4]], side: usize, r: f32) -> f32 {
    (0..96)
        .map(|i| {
            let a = i as f32 / 96.0 * std::f32::consts::TAU;
            let (x, y) = (side as f32 / 2.0 + a.cos() * r, side as f32 / 2.0 + a.sin() * r);
            texels[y as usize * side + x as usize][0].abs()
        })
        .fold(0.0, f32::max)
}

#[test]
fn contact_ripples_spread_on_a_webgl2_class_gpu() {
    let Some((texels, compute, side)) = touched_lake(Some(GpuTier::WebGl2), Vec2::ZERO, 20) else {
        return;
    };
    assert!(!compute, "no compute pass on WebGL2: the CPU runs the ripples");
    let rings: Vec<f32> = (2..side / 2 - 2).map(|r| ring(&texels, side, r as f32)).collect();
    let (peak, strongest) = rings
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, v)| (i + 2, *v))
        .unwrap();
    let far = rings[rings.len() - 8..].iter().copied().fold(0.0, f32::max);
    eprintln!("WebGL2 ripples after 20 frames: strongest ring {peak} texels out ({strongest}), {far} at the edge");
    assert!(strongest > 0.005, "the ring carries the touch: {strongest}");
    assert!((8..=24).contains(&peak), "the ring is on its way out: {peak} texels");
    assert!(far < strongest * 0.05, "it hasn't reached the edge: {far}");
    // The negative control: a touch outside the field does nothing.
    let Some((quiet, _, _)) = touched_lake(Some(GpuTier::WebGl2), vec2(100.0, 0.0), 5) else {
        return;
    };
    assert!(quiet.iter().all(|t| t[0] == 0.0), "a touch outside makes no ripples");
}

#[test]
fn the_cpu_ripples_leave_what_the_compute_pass_does() {
    let Some(gpu) = gpu() else { return };
    if !needs(&gpu.device, crate::render::GpuFeature::Compute) {
        return;
    }
    drop(gpu);
    let Some((gpu_field, compute, side)) = touched_lake(None, vec2(3.0, -2.0), 15) else {
        return;
    };
    let Some((cpu_field, cpu_compute, cpu_side)) = touched_lake(Some(GpuTier::WebGl2), vec2(3.0, -2.0), 15) else {
        return;
    };
    assert!(compute && !cpu_compute, "one of each");
    assert_eq!(side, cpu_side, "the same field");
    let worst = gpu_field
        .iter()
        .zip(&cpu_field)
        .map(|(g, c)| (g[0] - c[0]).abs().max((g[1] - c[1]).abs()))
        .fold(0.0f32, f32::max);
    let highest = gpu_field.iter().map(|g| g[0].abs()).fold(0.0f32, f32::max);
    eprintln!("ripples, compute pass vs CPU: worst {worst} of up to {highest}");
    assert!(highest > 0.01, "the touch rippled the water: {highest}");
    assert!(
        worst < highest * 0.02,
        "the CPU's ripples are the compute pass's: off by {worst}"
    );
}

/// What waves from a spectrum add to a frame at each size: the GPU's FFT
/// passes (the whole frame's GPU time against the same sea with Gerstner
/// waves, 1280 x 720) and the CPU's FFT (the height query's, and on
/// WebGL2 each frame's), interleaved run after run. Run by hand in release:
/// `cargo test -p ponko --lib --features 3d --release spectrum_waves_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn spectrum_waves_cost() {
    let size = (1280, 720);
    let Some(mut ctx) = offscreen(size, None, true) else {
        return;
    };
    let runs: usize = std::env::var("PONKO_SPECTRUM_RUNS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3);
    ctx.scene3d.camera.position = vec3(0.0, 6.0, 10.0);
    ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -30.0));
    let gerstner = WaterDesc::sea(0.0, vec2(0.8, -0.6), 1.5).with_reflection(WaterReflection::Sky);
    // The frame's GPU time, median of 40 after 20 to settle.
    let measure = |ctx: &mut Ctx, desc: WaterDesc| {
        let e = Water::spawn(ctx, desc);
        let mut totals = Vec::new();
        for i in 0..60 {
            frame(ctx);
            let renderer = ctx.gfx.renderer.as_ref().unwrap();
            if let Some((spans, _)) = renderer.gpu_frame()
                && i >= 20
                && !spans.is_empty()
            {
                totals.push(spans.iter().map(|p| p.ms).sum::<f32>());
            }
        }
        crate::water::despawn_water(ctx, e);
        totals.sort_by(f32::total_cmp);
        totals.get(totals.len() / 2).copied()
    };
    for r in 0..runs {
        let Some(base) = measure(&mut ctx, gerstner.clone()) else {
            skip("no GPU timestamps");
            return;
        };
        eprint!("run {r}: frame with Gerstner waves {base:.3} ms");
        for cells in [64, 128, 256] {
            let spectrum = WaveSpectrum::new(vec2(8.0, -6.0), 1.5).with_tile(256.0, cells);
            let total = measure(&mut ctx, gerstner.clone().with_waves(Waves::spectrum(spectrum))).unwrap_or(0.0);
            // The CPU's FFT at a new time each call (the cache keeps two).
            let field = crate::water::spectrum::SpectrumField::new(spectrum);
            let start = std::time::Instant::now();
            for i in 0..10 {
                std::hint::black_box(field.field(i as f32 * 0.1, false));
            }
            let cpu = start.elapsed().as_secs_f32() * 100.0;
            let start = std::time::Instant::now();
            for i in 0..10 {
                std::hint::black_box(field.field(i as f32 * 0.1 + 0.05, true));
            }
            let cpu_speed = start.elapsed().as_secs_f32() * 100.0;
            eprint!(
                "; {cells}: GPU +{:.3} ms, CPU field {cpu:.3} ms ({cpu_speed:.3} ms with velocities)",
                total - base
            );
        }
        eprintln!();
    }
}
