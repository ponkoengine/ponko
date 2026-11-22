//! Area lights (rectangles and tubes), measured against the engine's point
//! lights: what each would cost a pixel of the clustered path, how close
//! the cheap methods come to the light's true shading, and whether they
//! run with WebGL2's limits.
//!
//! The shaders are in `area_lights_eval.wgsl`, outside the renderer: one
//! full-screen pass shades a procedural surface (bumpy normals, roughness
//! 0.25 to 0.9) with every light in a records texture laid out like the
//! engine's (four RGBA32Float texels a light, read with `textureLoad`), as a
//! pixel of the clustered path shades every light of its froxel. Methods:
//!
//! - **point**: the engine's point light (`punctual_light` in `mesh.wgsl`);
//! - **rectangle, LTC**: linearly transformed cosines (Heitz, Dupuy, Hill
//!   and Neubelt 2016; the horizon as Hill and Heitz 2017), two lookups in
//!   64x64 half-float tables, diffuse and specular from four edge
//!   integrals each. The tables here hold the identity, not a fitted GGX,
//!   so the specular term's *cost* is real but its look isn't: shipping it
//!   needs the fitted tables;
//! - **rectangle, representative point** (Drobot 2014): diffuse from the
//!   exact form factor, specular from the rectangle's point nearest the
//!   reflected ray;
//! - **tube, representative point** (Karis 2013);
//! - **reference**: the light's surface sampled on a jittered grid, each
//!   sample shaded as a point light with the engine's GGX.
//!
//! The accuracy and WebGL2 tests are part of the suite. The cost is an
//! ignored test, run explicitly:
//! `cargo test -p ponko --lib --features 3d area_light_gpu_cost_benchmark -- --ignored --nocapture`.
//! The numbers and the decision are in the guide (`book/src/3d.md`, "Area
//! lights: measured, not shipped"). Offscreen only, no window.
use super::render_test_support::{self as support, TestGpu};
use glam::{Vec3, vec3};
use wgpu::util::DeviceExt;

const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const EYE: Vec3 = vec3(0.0, 6.0, 8.0);
/// The tables' width and height, as the published LTC fits.
const LTC_SIZE: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Method {
    Point,
    RectLtc,
    RectPoint,
    Tube,
    Reference,
}

impl Method {
    fn entry(self) -> &'static str {
        match self {
            Method::Point => "fs_point",
            Method::RectLtc => "fs_rect_ltc",
            Method::RectPoint => "fs_rect_point",
            Method::Tube => "fs_tube",
            Method::Reference => "fs_reference",
        }
    }
}

/// A light as the records texture holds it.
#[derive(Clone, Copy)]
struct Light {
    center: Vec3,
    range: f32,
    radiance: Vec3,
    /// Rectangle: half axis A; tube: half the segment.
    a: Vec3,
    /// Tube radius (0: a rectangle or a point).
    radius: f32,
    /// Rectangle: half axis B. A point light: `x` is -2 (no cone).
    b: Vec3,
}

impl Light {
    fn point(center: Vec3, intensity: f32) -> Self {
        Self {
            center,
            range: 100.0,
            radiance: Vec3::splat(intensity),
            a: Vec3::ZERO,
            radius: -2.0,
            b: Vec3::ZERO,
        }
    }
    /// A rectangle `w` by `d` facing down (it emits along A x B = -Y).
    fn rect(center: Vec3, w: f32, d: f32, radiance: f32) -> Self {
        Self {
            center,
            range: 100.0,
            radiance: Vec3::splat(radiance),
            a: vec3(w * 0.5, 0.0, 0.0),
            radius: 0.0,
            b: vec3(0.0, 0.0, d * 0.5),
        }
    }
    fn tube(center: Vec3, half: Vec3, radius: f32, radiance: f32) -> Self {
        Self {
            center,
            range: 100.0,
            radiance: Vec3::splat(radiance),
            a: half,
            radius,
            b: Vec3::ZERO,
        }
    }
    fn texels(&self) -> [f32; 16] {
        let (c, r, a, b) = (self.center, self.radiance, self.a, self.b);
        [
            c.x,
            c.y,
            c.z,
            self.range,
            r.x,
            r.y,
            r.z,
            0.0,
            a.x,
            a.y,
            a.z,
            self.radius,
            b.x,
            b.y,
            b.z,
            0.0,
        ]
    }
}

struct Rig {
    gpu: TestGpu,
    size: (u32, u32),
    layout: wgpu::BindGroupLayout,
    module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    tables: [wgpu::TextureView; 2],
    sampler: wgpu::Sampler,
    target: wgpu::Texture,
}

impl Rig {
    fn new(gpu: TestGpu, size: (u32, u32)) -> Self {
        let device = &gpu.device;
        let texture_entry = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("area lights"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, false),
                texture_entry(2, true),
                texture_entry(3, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("area lights"),
            source: wgpu::ShaderSource::Wgsl(include_str!("area_lights_eval.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // The identity matrix and a plain Schlick split, varied slightly
        // texel to texel so the lookups filter real data.
        let table = |f: &dyn Fn(u32, u32) -> [f32; 4]| {
            let mut bits = Vec::with_capacity((LTC_SIZE * LTC_SIZE * 4) as usize);
            for y in 0..LTC_SIZE {
                for x in 0..LTC_SIZE {
                    bits.extend(f(x, y).map(super::batch::f16_bits));
                }
            }
            device
                .create_texture_with_data(
                    &gpu.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("ltc table"),
                        size: wgpu::Extent3d {
                            width: LTC_SIZE,
                            height: LTC_SIZE,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba16Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    bytemuck::cast_slice(&bits),
                )
                .create_view(&Default::default())
        };
        let jitter = |x: u32, y: u32| ((x * 7 + y * 13) % 17) as f32 * 1e-3;
        let tables = [
            table(&|x, y| [1.0 + jitter(x, y), 0.0, 0.0, 1.0 - jitter(y, x)]),
            table(&|x, y| [1.0 - jitter(x, y), jitter(y, x), 0.0, 0.0]),
        ];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("area light target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self {
            gpu,
            size,
            layout,
            module,
            pipeline_layout,
            tables,
            sampler,
            target,
        }
    }

    fn pipeline(&self, method: Method) -> wgpu::RenderPipeline {
        self.gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(method.entry()),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: Some(method.entry()),
                compilation_options: Default::default(),
                targets: &[Some(HDR.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The bind group for `lights`, shaded diffuse only or in full, the
    /// reference with `samples` x `samples` points a light, at `roughness`
    /// (0: varies over the surface).
    fn bind(&self, lights: &[Light], diffuse_only: bool, samples: u32, roughness: f32) -> wgpu::BindGroup {
        let device = &self.gpu.device;
        let mut params = [0u32; 12];
        params[..4].copy_from_slice(&EYE.extend(1.0).to_array().map(f32::to_bits));
        params[4] = lights.len() as u32;
        params[5] = u32::from(diffuse_only);
        params[6] = samples;
        params[8] = (self.size.0 as f32).to_bits();
        params[9] = (self.size.1 as f32).to_bits();
        params[10] = roughness.to_bits();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let texels: Vec<f32> = lights.iter().flat_map(Light::texels).collect();
        let rows = (lights.len() as u32).max(1);
        let mut data = texels;
        data.resize((rows * 16) as usize, 0.0);
        let records = device
            .create_texture_with_data(
                &self.gpu.queue,
                &wgpu::TextureDescriptor {
                    label: Some("area light records"),
                    size: wgpu::Extent3d {
                        width: 4,
                        height: rows,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba32Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                bytemuck::cast_slice(&data),
            )
            .create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&records),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.tables[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.tables[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
        timer: Option<&super::timer::GpuTimer>,
    ) {
        let view = self.target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("area lights"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timer.and_then(|t| t.pass("area lights")),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The linear RGB of every pixel shaded by `method`.
    fn shade(
        &self,
        method: Method,
        lights: &[Light],
        diffuse_only: bool,
        samples: u32,
        roughness: f32,
    ) -> Vec<[f32; 3]> {
        let pipeline = self.pipeline(method);
        let bind = self.bind(lights, diffuse_only, samples, roughness);
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.record(&mut encoder, &pipeline, &bind, None);
        self.gpu.queue.submit([encoder.finish()]);
        let bytes = support::read_texture(&self.gpu.device, &self.gpu.queue, &self.target);
        bytemuck::cast_slice::<u8, u16>(&bytes)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [0, 1, 2].map(|c| super::batch::f16_value(p[c])))
            .collect()
    }
}

/// How far `got` is from `want`: the mean absolute error of the pixels'
/// luminance relative to the reference's mean luminance, and the share of
/// pixels more than 10 % off their own reference value.
fn error(got: &[[f32; 3]], want: &[[f32; 3]]) -> (f32, f32) {
    let luma = |p: &[f32; 3]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let mean = want.iter().map(luma).sum::<f32>() / want.len() as f32;
    let mut sum = 0.0;
    let mut off = 0;
    for (g, w) in got.iter().zip(want) {
        let (g, w) = (luma(g), luma(w));
        sum += (g - w).abs();
        if (g - w).abs() > 0.1 * w.max(1e-3) {
            off += 1;
        }
    }
    (sum / got.len() as f32 / mean, off as f32 / got.len() as f32)
}

/// A window-sized panel and a strip light over the surface.
fn panel() -> Vec<Light> {
    vec![Light::rect(vec3(-1.0, 2.5, 0.0), 3.0, 1.5, 4.0)]
}
fn strip() -> Vec<Light> {
    vec![Light::tube(vec3(1.0, 2.0, 0.5), vec3(1.5, 0.0, 0.3), 0.05, 40.0)]
}

const SMALL: (u32, u32) = (160, 90);

/// Diffuse light from a rectangle is exact in closed form (Lambert's edge
/// integral): the representative-point method, which uses it as is,
/// matches the sampled reference; LTC, which handles the horizon with an
/// analytic sphere fit instead of clipping, comes out a few percent bright.
/// The one-sided rectangle lights nothing behind it (negative control).
#[test]
fn rectangle_light_diffuse_matches_the_sampled_reference() {
    let Some(gpu) = support::gpu() else { return };
    let rig = Rig::new(gpu, SMALL);
    let lights = panel();
    let reference = rig.shade(Method::Reference, &lights, true, 48, 0.0);
    for (method, within) in [(Method::RectPoint, 0.02), (Method::RectLtc, 0.04)] {
        let got = rig.shade(method, &lights, true, 0, 0.0);
        let (mean, off) = error(&got, &reference);
        eprintln!(
            "{method:?} diffuse: mean error {:.1} %, {:.1} % of pixels off by 10 %",
            mean * 100.0,
            off * 100.0
        );
        assert!(
            mean < within && off < 0.02,
            "{method:?}: mean error {mean}, {off} of pixels off by 10 %"
        );
    }
    // The same panel turned to face up lights nothing below it.
    let mut up = lights[0];
    up.b = -up.b;
    for method in [Method::RectLtc, Method::RectPoint, Method::Reference] {
        let dark = rig.shade(method, &[up], true, 8, 0.0);
        let brightest = dark.iter().map(|p| p[1]).fold(0.0, f32::max);
        assert!(brightest < 1e-3, "{method:?} lights the back side: {brightest}");
    }
    // And a point light at the panel's centre is a different picture.
    let point = rig.shade(Method::Point, &[Light::point(lights[0].center, 18.0)], true, 0, 0.0);
    assert!(
        error(&point, &reference).0 > 0.05,
        "the comparison tells a point light apart"
    );
}

/// A tube's representative point (Karis 2013) is close to the sampled
/// tube beside its middle, but treats it as a line whose width doesn't
/// shrink when seen along its axis, so it is too bright off its ends.
#[test]
fn tube_light_diffuse_is_close_beside_the_tube_and_bright_off_its_ends() {
    let Some(gpu) = support::gpu() else { return };
    let rig = Rig::new(gpu, SMALL);
    let lights = strip();
    let reference = rig.shade(Method::Reference, &lights, true, 48, 0.0);
    let got = rig.shade(Method::Tube, &lights, true, 0, 0.0);
    let (mean, _) = error(&got, &reference);
    assert!(mean < 0.35, "tube: mean error {mean}");
    // Pixel (x, y) of the 16 x 9 unit surface at world (x, 0, z).
    let at = |x: f32, z: f32| (((z + 4.5) / 9.0 * 90.0) as usize) * 160 + ((x + 8.0) / 16.0 * 160.0) as usize;
    let (under, off_end) = (at(1.0, 0.5), at(-3.5, 0.0));
    let ratio = |i: usize| got[i][1] / reference[i][1];
    eprintln!(
        "tube diffuse: mean error {:.1} %, beside the middle x{:.3}, off the end x{:.2}",
        mean * 100.0,
        ratio(under),
        ratio(off_end)
    );
    assert!((ratio(under) - 1.0).abs() < 0.1, "beside the middle: {}", ratio(under));
    assert!(
        ratio(off_end) > 1.5,
        "off the end, seen along the axis: {}",
        ratio(off_end)
    );
    assert!(got.iter().all(|p| p[1] > 0.0), "every pixel faces the tube");
}

/// Every method compiles and draws the same picture on a device opened
/// with WebGL2's limits (no storage buffers, 16 textures, uniform buffers):
/// lights in a float texture read with `textureLoad`, the tables in
/// filterable half-float textures, as the engine's punctual records.
#[test]
fn area_light_shaders_draw_the_same_with_webgl2_limits() {
    let Some(full) = support::gpu() else { return };
    let webgl2 = crate::render::GpuTier::WebGl2.device_limits(wgpu::Limits::default());
    let Some(limited) = support::gpu_with(|_| {
        Ok(wgpu::DeviceDescriptor {
            required_limits: webgl2.clone(),
            ..Default::default()
        })
    }) else {
        return;
    };
    assert_eq!(limited.device.limits().max_storage_buffers_per_shader_stage, 0);
    let (full, limited) = (Rig::new(full, SMALL), Rig::new(limited, SMALL));
    let mut lights = panel();
    lights.extend(strip());
    let mut pictures = Vec::new();
    for (method, lights) in [
        (Method::RectLtc, &lights[..1]),
        (Method::RectPoint, &lights[..1]),
        (Method::Tube, &lights[1..]),
    ] {
        let a = full.shade(method, lights, false, 0, 0.0);
        let b = limited.shade(method, lights, false, 0, 0.0);
        let (mean, _) = error(&b, &a);
        assert!(mean < 1e-3, "{method:?} differs with WebGL2's limits: {mean}");
        assert!(a.iter().any(|p| p[1] > 0.01), "{method:?} lights the surface");
        pictures.push(a);
    }
    assert!(
        error(&pictures[0], &pictures[2]).0 > 0.05,
        "the comparison tells a rectangle from a tube"
    );
}

/// GPU time of a full-screen pass at 1920x1080 shading every pixel with
/// 0, 8 and 32 lights of each kind (a froxel's list), with the engine's GPU
/// pass timer: the median of the frames timed after 8 warm-up frames, and
/// the cost of one more light (from 8 to 32). Also the error of each method against the
/// sampled reference, in full (diffuse and specular), at three roughnesses.
/// Ignored: run explicitly. It measures GPU timestamps only, so a debug
/// build gives the same numbers as a release one.
#[test]
#[ignore = "benchmark: run explicitly"]
fn area_light_gpu_cost_benchmark() {
    const BIG: (u32, u32) = (1920, 1080);
    let Some(gpu) = support::gpu_with(|adapter| {
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return Err("needs timestamp queries".into());
        }
        Ok(wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })
    }) else {
        return;
    };
    let rig = Rig::new(gpu, BIG);
    let (device, queue) = (&rig.gpu.device, &rig.gpu.queue);
    let mut timer = super::timer::GpuTimer::new(device, queue).expect("a GPU timer");
    // Lights spread over the surface, all within reach of every pixel.
    let make = |method: Method, n: usize| -> Vec<Light> {
        (0..n)
            .map(|i| {
                let t = i as f32 / n.max(1) as f32;
                let at = vec3(t * 14.0 - 7.0, 2.0 + (i % 3) as f32, (i % 5) as f32 * 1.6 - 4.0);
                match method {
                    Method::Point => Light::point(at, 2.0),
                    Method::Tube => Light::tube(at, vec3(0.8, 0.0, 0.2), 0.05, 4.0),
                    _ => Light::rect(at, 1.5, 0.8, 0.2),
                }
            })
            .collect()
    };
    let mut time = |method: Method, n: usize| {
        let pipeline = rig.pipeline(method);
        let bind = rig.bind(&make(method, n), false, 0, 0.0);
        let mut times = Vec::new();
        let mut read = timer.frames_read;
        for frame in 0..48 + 6 {
            timer.collect(device);
            if timer.frames_read != read {
                read = timer.frames_read;
                if frame > 8 {
                    times.push(timer.latest_frame().expect("a timed frame").0);
                }
            }
            timer.begin_frame();
            let mut encoder = device.create_command_encoder(&Default::default());
            rig.record(&mut encoder, &pipeline, &bind, Some(&timer));
            timer.resolve(&mut encoder);
            queue.submit([encoder.finish()]);
            timer.submitted();
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
        times.sort_by(f32::total_cmp);
        assert!(times.len() >= 30, "{} frames timed", times.len());
        times[times.len() / 2]
    };
    // The GPU ramps its clocks up over the first busy frames: warm it up
    // so the first method timed isn't slowed by it.
    time(Method::RectLtc, 32);
    for method in [Method::Point, Method::RectLtc, Method::RectPoint, Method::Tube] {
        let [none, eight, many] = [0, 8, 32].map(|n| time(method, n));
        eprintln!(
            "area lights gpu {:<10} 0: {none:6.3} ms, 8: {eight:6.3} ms, 32: {many:6.3} ms; one light {:6.4} ms",
            format!("{method:?}"),
            (many - eight) / 24.0
        );
    }
    drop(time);
    let small = Rig::new(support::gpu().expect("a device, as above"), (BIG.0 / 4, BIG.1 / 4));
    for roughness in [0.3, 0.6, 0.9] {
        for (lights, methods) in [
            (panel(), &[Method::RectLtc, Method::RectPoint][..]),
            (strip(), &[Method::Tube][..]),
        ] {
            let reference = small.shade(Method::Reference, &lights, false, 64, roughness);
            for &method in methods {
                let (mean, off) = error(&small.shade(method, &lights, false, 0, roughness), &reference);
                eprintln!(
                    "area lights error roughness {roughness}: {:<10} mean {:5.1} %, {:5.1} % of pixels off by 10 %",
                    format!("{method:?}"),
                    mean * 100.0,
                    off * 100.0
                );
            }
        }
    }
}
