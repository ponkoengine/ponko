//! The capability report against what the 3D renderer really does: devices
//! opened on this machine's adapter with forced lower limits (WebGL2-like,
//! WebGPU's defaults) and without timestamp queries draw frames with every
//! device-dependent effect asked for, without a GPU error, and the report
//! names exactly the fallbacks the renderer used.
//!
//! This forces *limits* on the real adapter; it doesn't run the WebGL2
//! backend itself (no GL downlevel flags). Skips without a GPU adapter, or
//! when the adapter can't open a device with a profile's limits.
//!
//! `PONKO_CAPABILITIES_PNG=dir` saves each profile's frame as a PNG there.

use std::sync::{Arc, Mutex};

use super::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::GpuTier;
use crate::render::capabilities::{self, GpuFeature};
#[allow(unused_imports)]
use crate::render::{
    AmbientOcclusion, AntiAliasing, Compute, GlobalIllumination, GpuCapabilities, GraphicsQuality, MaterialDesc,
    MeshData, Model,
};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight, linear};

const SIZE: u32 = 128;

/// One forced device setup.
struct Profile {
    name: &'static str,
    limits: fn(&wgpu::Adapter) -> wgpu::Limits,
    timestamps: bool,
}

const PROFILES: [Profile; 4] = [
    Profile {
        name: "webgl2-limits",
        limits: |_| wgpu::Limits::downlevel_webgl2_defaults(),
        timestamps: false,
    },
    Profile {
        name: "downlevel-limits",
        limits: |_| wgpu::Limits::downlevel_defaults(),
        timestamps: false,
    },
    Profile {
        name: "webgpu-default-limits",
        limits: |_| wgpu::Limits::default(),
        timestamps: false,
    },
    Profile {
        // As the engine asks on desktop.
        name: "engine-desktop",
        limits: |a| wgpu::Limits {
            max_sampled_textures_per_shader_stage: a.limits().max_sampled_textures_per_shader_stage.clamp(16, 24),
            ..wgpu::Limits::default()
        },
        timestamps: true,
    },
];

struct Outcome {
    report: GpuCapabilities,
    errors: Vec<String>,
    pixels: Vec<u8>,
    /// The built-in shader's cached set: its composed engine parts.
    features: super::pipeline_cache::Features,
}

/// The limits the engine asks for on desktop.
fn engine_limits(a: &wgpu::Adapter) -> wgpu::Limits {
    wgpu::Limits {
        max_sampled_textures_per_shader_stage: a.limits().max_sampled_textures_per_shader_stage.clamp(16, 24),
        ..wgpu::Limits::default()
    }
}

/// Each tier forced as `PONKO_GPU_TIER` forces it on the engine's request.
const TIERS: [(GpuTier, Profile); 3] = [
    (
        GpuTier::WebGl2,
        Profile {
            name: "forced-webgl2",
            limits: |a| GpuTier::WebGl2.device_limits(engine_limits(a)),
            timestamps: false,
        },
    ),
    (
        GpuTier::Baseline,
        Profile {
            name: "forced-baseline",
            limits: |a| GpuTier::Baseline.device_limits(engine_limits(a)),
            timestamps: false,
        },
    ),
    (
        GpuTier::Desktop,
        Profile {
            name: "forced-desktop",
            limits: |a| GpuTier::Desktop.device_limits(engine_limits(a)),
            timestamps: true,
        },
    ),
];

/// Opens a device for `profile` and draws three frames with TAA, GTAO,
/// contact shadows, screen-space reflections and global illumination on;
/// `None` when it can't be set up here (printed).
fn draw(profile: &Profile) -> Option<Outcome> {
    draw_with(profile, (SIZE, SIZE), |_| {})
}

/// [`draw`], with `setup` changing the scene's settings before the frames.
fn draw_with(profile: &Profile, size: (u32, u32), setup: impl Fn(&mut Ctx)) -> Option<Outcome> {
    crate::render::render_test_support::take_gpu_place();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        crate::render::render_test_support::no_gpu(format_args!("{}: no GPU adapter", profile.name));
        return None;
    };
    let required_features = if profile.timestamps {
        adapter.features() & wgpu::Features::TIMESTAMP_QUERY
    } else {
        wgpu::Features::empty()
    };
    let Ok((device, queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: (profile.limits)(&adapter),
        required_features,
        ..Default::default()
    })) else {
        crate::render::render_test_support::skip(format_args!(
            "{}: the adapter can't open a device with these limits",
            profile.name
        ));
        return None;
    };
    let info = adapter.get_info();
    eprintln!("{}: running on {} ({:?})", profile.name, info.name, info.backend);
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = errors.clone();
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| sink.lock().unwrap().push(e.to_string())));

    let report = capabilities::of_device(&info, &device, (4, 4));

    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    queue.write_texture(
        white.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        white.size(),
    );
    let sampler = device.create_sampler(&Default::default());
    let view = white.create_view(&Default::default());
    let textures = vec![super::super::gpu::GpuTexture {
        texture: white.clone(),
        nearest: false,
        mips: None,
        bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }),
    }];
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(size.0 as f32, size.1 as f32));
    // As `Ctx` does from the renderer's answers.
    ctx.gfx.assets3d.decals_supported = report.available(GpuFeature::Decals);
    ctx.gfx.assets3d.gi_supported = report.available(GpuFeature::GlobalIllumination);
    let matte = MaterialDesc {
        color: Color::rgb(0.8, 0.5, 0.2),
        roughness: 0.3,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(matte);
    let mut renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
    renderer.create_material(&device, &GpuMaterialDesc::new(&matte), &textures);
    let mut add = |ctx: &mut Ctx, data: &MeshData, at: Transform3D| {
        let mesh = ctx.gfx.create_mesh(data, material);
        renderer.create_mesh(&device, data, &[(0..data.indices.len() as u32, material)], true);
        ctx.world.spawn((Model::new(mesh), at));
    };
    add(&mut ctx, &MeshData::cube(1.0), Transform3D::default());
    add(
        &mut ctx,
        &MeshData::cube(1.0),
        Transform3D {
            scale: vec3(8.0, 0.2, 8.0),
            ..Transform3D::at(vec3(0.0, -0.7, 0.0))
        },
    );
    ctx.scene3d.lighting.sun_direction = vec3(-0.4, -1.0, -0.3);
    ctx.scene3d.lighting.gi = GlobalIllumination {
        enabled: true,
        extent: vec3(8.0, 4.0, 8.0),
        spacing: 1.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    ctx.scene3d.post.ambient_occlusion.enabled = true;
    ctx.scene3d.post.contact_shadows = 1.0;
    ctx.scene3d.post.ssr = 1.0;
    ctx.scene3d.camera.position = vec3(2.5, 2.0, 3.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    setup(&mut ctx);

    let mut pixels = Vec::new();
    for _ in 0..3 {
        let mut frame = Frame3D::default();
        // As the engine does: no shadow map larger than the device allows.
        frame.size_limit = device.limits().max_texture_dimension_2d;
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let rgb = |c| {
            let [r, g, b, _] = linear(c);
            vec3(r, g, b)
        };
        let scene = Scene3D {
            frame: &frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: rgb(ctx.scene3d.lighting.sun_color) * ctx.scene3d.lighting.sun_intensity,
                sky: rgb(ctx.scene3d.sky.sky_color),
                ground: rgb(ctx.scene3d.sky.ground_color),
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (0.0, 1.0 / 60.0),
            occlusion: true,
        };
        renderer.prepare(
            &device,
            &queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size,
            },
        );
        let target = output.create_view(&Default::default());
        let buffers = renderer.draw(&device, &target, wgpu::Color::BLACK, &textures, None);
        queue.submit(buffers);
        pixels = read(&device, &queue, &output);
    }
    // Every effect's settings were written only where they changed.
    renderer.check_mirrors(&device, &queue);

    // The renderer's own choices, against the report.
    let on = |f| report.available(f);
    assert_eq!(
        renderer.screen.taa_on,
        on(GpuFeature::TemporalAntialiasing) && ctx.scene3d.post.anti_aliasing == AntiAliasing::Taa,
        "{}: TAA",
        profile.name
    );
    // TAA where the device has it, FXAA instead; other modes as asked.
    let aa = match ctx.scene3d.post.anti_aliasing {
        AntiAliasing::Taa if !on(GpuFeature::TemporalAntialiasing) => AntiAliasing::Fxaa,
        asked => asked,
    };
    assert_eq!(
        renderer.post.antialiasing(),
        aa,
        "{}: the antialiasing used",
        profile.name
    );
    assert_eq!(
        renderer.screen.ao_enabled,
        on(GpuFeature::AmbientOcclusion)
            && (ctx.scene3d.post.ambient_occlusion.enabled || ctx.scene3d.post.contact_shadows > 0.0),
        "{}: GTAO",
        profile.name
    );
    assert_eq!(
        renderer.screen.ssr_on,
        on(GpuFeature::ScreenSpaceReflections) && ctx.scene3d.post.ssr > 0.0,
        "{}: SSR",
        profile.name
    );
    assert_eq!(
        renderer.gi.is_some(),
        on(GpuFeature::GlobalIllumination) && ctx.scene3d.lighting.gi.enabled,
        "{}: GI",
        profile.name
    );
    assert_eq!(
        renderer.resources.storage_read.is_some(),
        on(GpuFeature::StorageBuffers),
        "{}: storage buffers",
        profile.name
    );
    assert_eq!(
        renderer.skinning.uses_compute(),
        on(GpuFeature::GpuSkinning),
        "{}: GPU skinning",
        profile.name
    );
    assert_eq!(
        renderer.occlusion.supported(),
        on(GpuFeature::OcclusionCulling),
        "{}: occlusion culling",
        profile.name
    );
    assert_eq!(
        renderer.frame_sources.decals,
        on(GpuFeature::Decals),
        "{}: decals",
        profile.name
    );
    assert_eq!(
        super::super::timer::GpuTimer::new(&device, &queue).is_some(),
        on(GpuFeature::Timestamps),
        "{}: GPU timestamps",
        profile.name
    );

    if let Some(dir) = std::env::var_os("PONKO_CAPABILITIES_PNG") {
        let path = std::path::Path::new(&dir).join(format!("{}.png", profile.name));
        image::save_buffer(&path, &pixels, size.0, size.1, image::ColorType::Rgba8).unwrap();
    }
    let features = renderer
        .shaders
        .cache
        .built_keys()
        .into_iter()
        .find(|k| k.set.module.source.is_empty())
        .expect("the built-in shader's set is cached")
        .set
        .module
        .features;
    let errors = errors.lock().unwrap().clone();
    Some(Outcome {
        report,
        errors,
        pixels,
        features,
    })
}

fn read(device: &wgpu::Device, queue: &wgpu::Queue, output: &wgpu::Texture) -> Vec<u8> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(output.width() * output.height() * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                // A multiple of 256 at every size these tests use.
                bytes_per_row: Some(output.width() * 4),
                rows_per_image: None,
            },
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    readback.get_mapped_range(..).unwrap().to_vec()
}

/// The cube in the middle of the frame is lit (not the black clear color).
fn assert_drawn(name: &str, pixels: &[u8]) {
    assert_drawn_at(name, pixels, (SIZE, SIZE));
}

/// [`assert_drawn`] for a frame of `size`.
fn assert_drawn_at(name: &str, pixels: &[u8], (w, h): (u32, u32)) {
    let i = (((h / 2) * w + w / 2) * 4) as usize;
    let [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
    assert!(
        u32::from(r) + u32::from(g) + u32::from(b) > 60,
        "{name}: the cube wasn't drawn: {r} {g} {b}"
    );
}

#[test]
fn webgl2_limits_fall_back_without_gpu_errors() {
    let Some(out) = draw(&PROFILES[0]) else { return };
    assert_eq!(out.errors, Vec::<String>::new());
    assert_drawn(PROFILES[0].name, &out.pixels);
    for f in [
        GpuFeature::Timestamps,
        GpuFeature::Compute,
        GpuFeature::StorageBuffers,
        GpuFeature::ComputeCulling,
        GpuFeature::GpuSkinning,
        GpuFeature::OcclusionCulling,
        GpuFeature::TemporalAntialiasing,
        GpuFeature::AmbientOcclusion,
        GpuFeature::ContactShadows,
        GpuFeature::ScreenSpaceReflections,
        GpuFeature::GlobalIllumination,
        GpuFeature::Decals,
    ] {
        let s = out.report.feature(f).unwrap();
        assert!(!s.available && s.fallback.is_some() && s.reason.is_some(), "{s:?}");
    }
    assert!(out.report.available(GpuFeature::Multisampling));
    assert_eq!(out.report.limits.max_compute_workgroups_per_dimension, 0);
}

#[test]
fn downlevel_and_default_limits_keep_compute_without_timestamps() {
    for profile in &PROFILES[1..3] {
        let Some(out) = draw(profile) else { continue };
        assert_eq!(out.errors, Vec::<String>::new(), "{}", profile.name);
        assert_drawn(profile.name, &out.pixels);
        let r = &out.report;
        assert!(!r.available(GpuFeature::Timestamps), "{}", profile.name);
        for f in [
            GpuFeature::Compute,
            GpuFeature::StorageBuffers,
            GpuFeature::ComputeCulling,
            GpuFeature::TemporalAntialiasing,
            GpuFeature::ScreenSpaceReflections,
        ] {
            assert!(r.available(f), "{}: {f:?}", profile.name);
        }
        // Skinning binds six storage buffers: the downlevel limits have four.
        assert_eq!(
            r.available(GpuFeature::GpuSkinning),
            r.limits.max_storage_buffers_per_shader_stage >= 6,
            "{}",
            profile.name
        );
        // 16 texture slots: no room for decals or the GI probes.
        assert!(!r.available(GpuFeature::Decals), "{}", profile.name);
        assert!(!r.available(GpuFeature::GlobalIllumination), "{}", profile.name);
    }
}

#[test]
fn engine_desktop_device_reports_what_the_adapter_has() {
    let Some(out) = draw(&PROFILES[3]) else { return };
    assert_eq!(out.errors, Vec::<String>::new());
    assert_drawn(PROFILES[3].name, &out.pixels);
    let r = &out.report;
    assert!(r.available(GpuFeature::Compute));
    assert!(r.available(GpuFeature::TemporalAntialiasing));
    for f in &r.features {
        assert_eq!(f.available, f.fallback.is_none(), "{f:?}");
        assert_eq!(f.available, f.reason.is_none(), "{f:?}");
    }
    eprintln!("{}", serde_json::to_string_pretty(r).unwrap());
}

#[test]
fn forced_tiers_report_their_tier_and_cache_distinct_pipelines() {
    let mut seen = Vec::new();
    for (tier, profile) in &TIERS {
        let Some(out) = draw(profile) else { continue };
        assert_eq!(out.errors, Vec::<String>::new(), "{}", profile.name);
        assert_drawn(profile.name, &out.pixels);
        let r = &out.report;
        if *tier == GpuTier::Desktop && GpuTier::of(&r.limits) < GpuTier::Desktop {
            // An adapter with fewer than 18 texture slots can't be forced up.
            crate::render::render_test_support::skip(format_args!(
                "{}: this adapter reaches only {}",
                profile.name, r.tier
            ));
            continue;
        }
        assert_eq!(r.tier, *tier, "{}", profile.name);
        // The cached built-in set is composed from what the report says.
        let f = out.features;
        assert_eq!(
            (f.decals, f.gi, f.storage),
            (
                r.available(GpuFeature::Decals),
                r.available(GpuFeature::GlobalIllumination),
                r.available(GpuFeature::StorageBuffers)
            ),
            "{}",
            profile.name
        );
        // A tier guarantees what its floor has.
        for g in GpuFeature::ALL {
            assert!(
                !tier.report().available(g) || r.available(g),
                "{}: {g:?} is guaranteed at {tier}",
                profile.name
            );
        }
        seen.push((*tier, f));
    }
    for (i, (a, fa)) in seen.iter().enumerate() {
        for (b, fb) in &seen[i + 1..] {
            assert_ne!(fa, fb, "{a} and {b} share pipeline keys");
        }
    }
}

#[test]
fn every_quality_preset_draws_on_every_tier_without_gpu_errors() {
    // `None`: the tier's recommended preset, fitted to its floor.
    let presets = [
        ("low", Some(GraphicsQuality::Low)),
        ("medium", Some(GraphicsQuality::Medium)),
        ("high", Some(GraphicsQuality::High)),
        ("ultra", Some(GraphicsQuality::Ultra)),
        ("for_tier", None),
    ];
    for (tier, profile) in &TIERS {
        for (name, quality) in &presets {
            let Some(out) = draw_with(profile, (SIZE, SIZE), |ctx| match quality {
                Some(quality) => ctx.scene3d.set_quality(*quality),
                None => ctx.scene3d.set_recommended_quality(&tier.report()),
            }) else {
                return;
            };
            assert_eq!(
                out.errors,
                Vec::<String>::new(),
                "{} with the {name} preset",
                profile.name
            );
            assert_drawn(&format!("{} with the {name} preset", profile.name), &out.pixels);
            // The preset fitted to the device asks for nothing it lacks.
            let mut fit = crate::render::SceneSettings::default();
            fit.set_recommended_quality(&out.report);
            let recommended = (fit.lighting, fit.post);
            fit.fit_quality_to(&out.report);
            assert_eq!(recommended, (fit.lighting, fit.post));
        }
    }
}

/// What `PONKO_GPU_TIER` gives the engine's window device: the tier's
/// limits, but room for the window. A 1280x720 window on a 2x display is
/// drawn at 2560x1440, past the tiers' 2048 texture floor; the forced
/// device draws a frame that size without a GPU error.
#[test]
fn forced_window_devices_draw_a_high_dpi_window_sized_frame() {
    const WINDOW: (u32, u32) = (2560, 1440);
    let profiles = [
        Profile {
            name: "window-webgl2",
            limits: |a| GpuTier::WebGl2.window_limits(engine_limits(a)),
            timestamps: false,
        },
        Profile {
            name: "window-baseline",
            limits: |a| GpuTier::Baseline.window_limits(engine_limits(a)),
            timestamps: false,
        },
    ];
    for (tier, profile) in [GpuTier::WebGl2, GpuTier::Baseline].iter().zip(&profiles) {
        let Some(out) = draw_with(profile, WINDOW, |_| {}) else {
            continue;
        };
        assert_eq!(out.errors, Vec::<String>::new(), "{}", profile.name);
        assert_drawn_at(profile.name, &out.pixels, WINDOW);
        assert_eq!(out.report.tier, *tier, "{}: still that tier", profile.name);
        assert!(
            out.report.limits.max_texture_dimension_2d >= WINDOW.0,
            "{}",
            profile.name
        );
    }
}
