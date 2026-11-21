//! An isolated ray-query reflection prototype, outside the engine's
//! renderer: its own acceleration structures, its own compute shader, its
//! own offscreen image. It checks what the decision note
//! (`crates/ponko_bench/RAY_TRACING.md`) says a ray-traced technique would
//! fix, on the scenes where screen-space reflections are measured to fail
//! (`ssr_render_test.rs`), and measures what it costs.
//!
//! It needs `Features::EXPERIMENTAL_RAY_QUERY` (Vulkan, Metal on macOS 15+,
//! DX12; see `ray_tracing_probe_render_test.rs`) and skips everywhere else,
//! and under a forced `PONKO_GPU_TIER` (no tier has ray queries).
//!
//! The shader shoots one ray per pixel from the camera; where it meets the
//! floor (a perfect mirror), a second ray along the reflection. Shading is a
//! constant per object (the glowing box red, all else black): what is
//! compared is what each ray finds, not materials.
use crate::three::Camera3D;
use glam::{Mat4, Vec2, Vec3, vec3};

const SIZE: u32 = 128;
/// Objects' ids in the acceleration structure (`instance_custom_data`).
const FLOOR: u32 = 0;
const GLOW: u32 = 1;
const DARK: u32 = 2;

const SHADER: &str = r"
enable wgpu_ray_query;

struct Params {
    inverse: mat4x4<f32>, // clip space to the world
    eye: vec4<f32>,
    size: vec4<f32>,      // width, height, 1: reflect off the floor
};
@group(0) @binding(0) var scene: acceleration_structure;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var image: texture_storage_2d<rgba8unorm, write>;

fn trace(origin: vec3<f32>, dir: vec3<f32>) -> RayIntersection {
    var rq: ray_query;
    rayQueryInitialize(&rq, scene, RayDesc(RAY_FLAG_FORCE_OPAQUE, 0xFFu, 0.0001, 1000.0, origin, dir));
    rayQueryProceed(&rq);
    return rayQueryGetCommittedIntersection(&rq);
}

fn glows(hit: RayIntersection) -> bool {
    return hit.kind != RAY_QUERY_INTERSECTION_NONE && hit.instance_custom_data == 1u;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(params.size.xy);
    if any(id.xy >= size) { return; }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size);
    let far = params.inverse * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - params.eye.xyz);
    let hit = trace(params.eye.xyz, dir);
    var red = 0.0;
    if glows(hit) {
        red = 1.0;
    } else if hit.kind != RAY_QUERY_INTERSECTION_NONE && hit.instance_custom_data == 0u && params.size.z > 0.5 {
        // The floor faces up; off it a little, so the ray doesn't find it.
        let at = params.eye.xyz + dir * hit.t + vec3<f32>(0.0, 0.0005, 0.0);
        if glows(trace(at, reflect(dir, vec3<f32>(0.0, 1.0, 0.0)))) {
            red = 1.0;
        }
    }
    textureStore(image, id.xy, vec4<f32>(red, 0.0, 0.0, 1.0));
}
";

/// A triangle mesh: positions and indices.
struct Geometry {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Geometry {
    /// A unit cube around the origin.
    fn cube() -> Self {
        let positions = (0..8)
            .map(|i| {
                let c = |bit| if i & bit == 0 { -0.5 } else { 0.5 };
                [c(1), c(2), c(4)]
            })
            .collect();
        // Two triangles per face (winding doesn't matter: rays are opaque
        // and cull nothing).
        let faces = [
            [0, 2, 6, 4],
            [1, 3, 7, 5],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [0, 1, 3, 2],
            [4, 5, 7, 6],
        ];
        let indices = faces.iter().flat_map(|[a, b, c, d]| [*a, *b, *c, *a, *c, *d]).collect();
        Self { positions, indices }
    }

    /// A flat grid at y = 0, `cells` by `cells` squares across `extent`.
    fn floor(extent: f32, cells: u32) -> Self {
        let n = cells + 1;
        let positions = (0..n * n)
            .map(|i| {
                let (x, z) = ((i % n) as f32 / cells as f32, (i / n) as f32 / cells as f32);
                [(x - 0.5) * extent, 0.0, (z - 0.5) * extent]
            })
            .collect();
        let indices = (0..cells * cells)
            .flat_map(|c| {
                let i = (c / cells) * n + c % cells;
                [i, i + 1, i + n + 1, i, i + n + 1, i + n]
            })
            .collect();
        Self { positions, indices }
    }

    /// A unit sphere of `rings` by `segments` quads.
    fn sphere(rings: u32, segments: u32) -> Self {
        let mut positions = Vec::new();
        for r in 0..=rings {
            let theta = std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..=segments {
                let phi = std::f32::consts::TAU * s as f32 / segments as f32;
                positions.push([
                    theta.sin() * phi.cos() * 0.5,
                    theta.cos() * 0.5,
                    theta.sin() * phi.sin() * 0.5,
                ]);
            }
        }
        let n = segments + 1;
        let indices = (0..rings * segments)
            .flat_map(|q| {
                let i = (q / segments) * n + q % segments;
                [i, i + 1, i + n + 1, i, i + n + 1, i + n]
            })
            .collect();
        Self { positions, indices }
    }

    fn triangles(&self) -> u32 {
        self.indices.len() as u32 / 3
    }
}

/// One object: which geometry, where, and its id.
struct Instance {
    geometry: usize,
    transform: Mat4,
    id: u32,
}

/// A device with ray queries (and timestamps where the adapter has them),
/// or `None` (the test returns early, logged as a skip).
fn ray_query_gpu() -> Option<crate::render::render_test_support::TestGpu> {
    if let Some(tier) = crate::render::render_test_support::forced_tier() {
        crate::render::render_test_support::skip(format_args!("no {tier} tier has ray queries"));
        return None;
    }
    crate::render::render_test_support::gpu_with(|adapter| {
        let features = adapter.features();
        if !features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
            return Err("the adapter reports no EXPERIMENTAL_RAY_QUERY".to_owned());
        }
        Ok(wgpu::DeviceDescriptor {
            label: Some("ray query prototype"),
            required_features: wgpu::Features::EXPERIMENTAL_RAY_QUERY | (features & wgpu::Features::TIMESTAMP_QUERY),
            required_limits: wgpu::Limits::default().using_acceleration_structure_values(adapter.limits()),
            // SAFETY: wgpu marks its ray tracing API experimental (it may
            // still have bugs, including undefined behaviour). Only this
            // isolated test module opts in; the engine's device never does.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            ..Default::default()
        })
    })
}

/// A scene's acceleration structures and the buffers they were built from.
struct Scene {
    tlas: wgpu::Tlas,
    /// Each geometry's BLAS, vertex and index buffers, and counts.
    geometries: Vec<(wgpu::Blas, wgpu::Buffer, wgpu::Buffer, u32, u32)>,
}

impl Scene {
    /// Builds `geometries` and `instances` into acceleration structures on
    /// the GPU. An instance with `hidden` set to its id is left out of
    /// every ray (mask 0).
    fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        geometries: &[Geometry],
        instances: &[Instance],
        hidden: Option<u32>,
    ) -> Self {
        use wgpu::util::DeviceExt;
        let geometries: Vec<_> = geometries
            .iter()
            .map(|g| {
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ray query vertices"),
                    contents: bytemuck::cast_slice(&g.positions),
                    usage: wgpu::BufferUsages::BLAS_INPUT,
                });
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ray query indices"),
                    contents: bytemuck::cast_slice(&g.indices),
                    usage: wgpu::BufferUsages::BLAS_INPUT,
                });
                let blas = device.create_blas(
                    &wgpu::CreateBlasDescriptor {
                        label: Some("ray query blas"),
                        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
                    },
                    wgpu::BlasGeometrySizeDescriptors::Triangles {
                        descriptors: vec![size_of(g)],
                    },
                );
                (
                    blas,
                    vertices,
                    indices,
                    g.positions.len() as u32,
                    g.indices.len() as u32,
                )
            })
            .collect();
        let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
            label: Some("ray query tlas"),
            max_instances: instances.len() as u32,
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        });
        for (i, instance) in instances.iter().enumerate() {
            // 3x4, row major: the transposed matrix's first three columns.
            let rows: [f32; 12] = instance.transform.transpose().to_cols_array()[..12].try_into().unwrap();
            let mask = if hidden == Some(instance.id) { 0 } else { 0xff };
            tlas[i] = Some(wgpu::TlasInstance::new(
                &geometries[instance.geometry].0,
                rows,
                instance.id,
                mask,
            ));
        }
        let scene = Self { tlas, geometries };
        scene.rebuild(device, queue, true);
        scene
    }

    /// Records and runs a build: of every BLAS and the TLAS, or of the TLAS
    /// alone (what moving objects cost each frame).
    fn rebuild(&self, device: &wgpu::Device, queue: &wgpu::Queue, blases: bool) {
        let sizes: Vec<_> = self
            .geometries
            .iter()
            .map(
                |(_, _, _, vertices, indices)| wgpu::BlasTriangleGeometrySizeDescriptor {
                    vertex_format: wgpu::VertexFormat::Float32x3,
                    vertex_count: *vertices,
                    index_format: Some(wgpu::IndexFormat::Uint32),
                    index_count: Some(*indices),
                    flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
                },
            )
            .collect();
        let entries: Vec<_> = if blases {
            self.geometries
                .iter()
                .zip(&sizes)
                .map(|((blas, vertices, indices, _, _), size)| wgpu::BlasBuildEntry {
                    blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![wgpu::BlasTriangleGeometry {
                        size,
                        vertex_buffer: vertices,
                        first_vertex: 0,
                        vertex_stride: 12,
                        index_buffer: Some(indices),
                        first_index: Some(0),
                        transform_buffer: None,
                        transform_buffer_offset: None,
                    }]),
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.build_acceleration_structures(entries.iter(), [&self.tlas]);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }

    fn triangles(&self) -> u32 {
        self.geometries.iter().map(|g| g.4 / 3).sum()
    }
}

fn size_of(g: &Geometry) -> wgpu::BlasTriangleGeometrySizeDescriptor {
    wgpu::BlasTriangleGeometrySizeDescriptor {
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: g.positions.len() as u32,
        index_format: Some(wgpu::IndexFormat::Uint32),
        index_count: Some(g.indices.len() as u32),
        flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
    }
}

/// The trace pipeline and its output image.
struct Tracer {
    pipeline: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    output: wgpu::Texture,
    group: wgpu::BindGroup,
}

impl Tracer {
    fn new(device: &wgpu::Device, scene: &Scene, size: (u32, u32)) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ray query prototype"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ray query prototype"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ray query params"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ray query image"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray query prototype"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::AccelerationStructure(&scene.tlas),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        Self {
            pipeline,
            params,
            output,
            group,
        }
    }

    /// Sets the camera; `reflect` whether floor pixels trace a reflection.
    fn aim(&self, queue: &wgpu::Queue, camera: &Camera3D, reflect: bool) {
        let mut data = [0f32; 24];
        data[..16].copy_from_slice(&camera.view_proj().inverse().to_cols_array());
        data[16..19].copy_from_slice(&camera.position.to_array());
        data[20] = self.output.width() as f32;
        data[21] = self.output.height() as f32;
        data[22] = if reflect { 1.0 } else { 0.0 };
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&data));
    }

    fn dispatch(&self, encoder: &mut wgpu::CommandEncoder, timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ray query trace"),
            timestamp_writes: timestamps,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.dispatch_workgroups(self.output.width().div_ceil(8), self.output.height().div_ceil(8), 1);
    }

    fn draw(&self, gpu: &crate::render::render_test_support::TestGpu) -> crate::render::render_test_support::Image {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.dispatch(&mut encoder, None);
        gpu.queue.submit([encoder.finish()]);
        crate::render::render_test_support::Image::read(&gpu.device, &gpu.queue, &self.output)
    }
}

/// The mirror-floor scenes of `ssr_render_test.rs`: a 20x20 floor, the
/// glowing unit box at (0, 1, 0), and with `wall` the dark wall between it
/// and the camera.
fn mirror_scene(wall: bool) -> (Vec<Geometry>, Vec<Instance>) {
    let mut instances = vec![
        Instance {
            geometry: 0,
            transform: Mat4::IDENTITY,
            id: FLOOR,
        },
        Instance {
            geometry: 1,
            transform: Mat4::from_translation(vec3(0.0, 1.0, 0.0)),
            id: GLOW,
        },
    ];
    if wall {
        instances.push(Instance {
            geometry: 1,
            transform: Mat4::from_scale_rotation_translation(
                vec3(3.0, 1.8, 0.1),
                glam::Quat::IDENTITY,
                vec3(0.0, 1.5, 1.2),
            ),
            id: DARK,
        });
    }
    (vec![Geometry::floor(20.0, 1), Geometry::cube()], instances)
}

/// The SSR tests' camera: at (0, 2, 6), looking at the box, or turned down
/// 48 degrees so the box is above the screen.
fn mirror_camera(below_the_box: bool) -> Camera3D {
    let mut camera = Camera3D::new(Vec2::splat(SIZE as f32));
    camera.position = vec3(0.0, 2.0, 6.0);
    if below_the_box {
        let pitch = 48f32.to_radians();
        camera.look_at(camera.position + vec3(0.0, -pitch.sin(), -pitch.cos()));
    } else {
        camera.look_at(vec3(0.0, 0.5, 0.0));
    }
    camera
}

/// The red at the floor point that mirrors the box, (0, 0, 2).
fn red_at_reflection(image: &crate::render::render_test_support::Image, camera: &Camera3D) -> u8 {
    let clip = camera.view_proj() * Vec3::new(0.0, 0.0, 2.0).extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
    let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
    image.pixel(x, y)[0]
}

/// Draws the prototype's image of a mirror scene and reads the red where
/// the floor mirrors the box: (with the box, with it masked out of the
/// rays). `None` without ray queries.
fn traced_reflection(wall: bool, below_the_box: bool) -> Option<(u8, u8)> {
    let gpu = ray_query_gpu()?;
    let camera = mirror_camera(below_the_box);
    let (geometries, instances) = mirror_scene(wall);
    let mut red = [0; 2];
    for (hidden, red) in [None, Some(GLOW)].into_iter().zip(&mut red) {
        let scene = Scene::build(&gpu.device, &gpu.queue, &geometries, &instances, hidden);
        let tracer = Tracer::new(&gpu.device, &scene, (SIZE, SIZE));
        tracer.aim(&gpu.queue, &camera, true);
        *red = red_at_reflection(&tracer.draw(&gpu), &camera);
    }
    Some((red[0], red[1]))
}

#[test]
fn a_traced_reflection_keeps_the_box_above_the_screen() {
    let Some((with, masked)) = traced_reflection(false, true) else {
        return;
    };
    eprintln!("ray query reflection of the glowing box above the screen: {with}, masked out {masked} (red)");
    assert!(with > 120, "the box above the screen is reflected: {with}");
    assert!(masked < 40, "negative control: without the box nothing is: {masked}");
}

#[test]
fn a_traced_reflection_keeps_the_box_behind_a_wall() {
    let Some((with, masked)) = traced_reflection(true, false) else {
        return;
    };
    eprintln!("ray query reflection of the glowing box behind a wall: {with}, masked out {masked} (red)");
    assert!(with > 120, "the box behind the wall is reflected: {with}");
    assert!(masked < 40, "negative control: without the box nothing is: {masked}");
}

#[test]
fn the_wall_hides_the_box_from_the_camera() {
    // What makes the wall scene a test of hidden geometry: without
    // reflections, no pixel shows the box.
    let Some(gpu) = ray_query_gpu() else {
        return;
    };
    let camera = mirror_camera(false);
    for (wall, expect_visible) in [(false, true), (true, false)] {
        let (geometries, instances) = mirror_scene(wall);
        let scene = Scene::build(&gpu.device, &gpu.queue, &geometries, &instances, None);
        let tracer = Tracer::new(&gpu.device, &scene, (SIZE, SIZE));
        tracer.aim(&gpu.queue, &camera, false);
        let seen = tracer.draw(&gpu).fraction(0..SIZE, 0..SIZE, |p| p[0] > 120);
        assert_eq!(
            seen > 0.0,
            expect_visible,
            "wall {wall}: {seen} of the pixels show the box"
        );
    }
}

/// The acceleration structures' sizes as the backend reports them (what
/// wgpu allocates for them; wgpu itself doesn't say): (all BLASes, the
/// TLAS, the largest build scratch). Only on Metal for now; `None` elsewhere.
#[cfg(target_vendor = "apple")]
fn backend_sizes(device: &wgpu::Device, scene: &Scene, instances: u32) -> Option<(u64, u64, u64)> {
    use wgpu::hal::{self, Device as _};
    type A = hal::api::Metal;
    let flags = wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE;
    // SAFETY: a second backend device on the same Metal device, used only
    // to ask for sizes and to make and free one small buffer of its own
    // (wgpu's guards can't be held two at a time, and the size query wants
    // a buffer it never reads). Nothing of wgpu's is touched through it.
    unsafe {
        let sizes = hal::metal::Device::device_from_raw(
            device.as_hal::<A>()?.raw_device().clone(),
            device.features(),
            &device.limits(),
        );
        let stand_in = sizes
            .create_buffer(&hal::BufferDescriptor {
                label: Some("size query stand-in"),
                size: 64,
                usage: wgpu::wgt::BufferUses::BOTTOM_LEVEL_ACCELERATION_STRUCTURE_INPUT,
                memory_flags: hal::MemoryFlags::empty(),
            })
            .ok()?;
        let (mut blas, mut scratch) = (0, 0);
        for (_, _, _, vertex_count, index_count) in &scene.geometries {
            let entries = hal::AccelerationStructureEntries::Triangles(vec![hal::AccelerationStructureTriangles {
                vertex_buffer: Some(&stand_in),
                vertex_format: wgpu::VertexFormat::Float32x3,
                first_vertex: 0,
                vertex_count: *vertex_count,
                vertex_stride: 12,
                indices: Some(hal::AccelerationStructureTriangleIndices {
                    format: wgpu::IndexFormat::Uint32,
                    buffer: Some(&stand_in),
                    offset: 0,
                    count: *index_count,
                }),
                transform: None,
                flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
            }]);
            let s = sizes.get_acceleration_structure_build_sizes(&hal::GetAccelerationStructureBuildSizesDescriptor {
                entries: &entries,
                flags,
            });
            blas += s.acceleration_structure_size;
            scratch = scratch.max(s.build_scratch_size);
        }
        let entries = hal::AccelerationStructureEntries::Instances(hal::AccelerationStructureInstances {
            buffer: Some(&stand_in),
            offset: 0,
            count: instances,
        });
        let s = sizes.get_acceleration_structure_build_sizes(&hal::GetAccelerationStructureBuildSizesDescriptor {
            entries: &entries,
            flags,
        });
        sizes.destroy_buffer(stand_in);
        Some((blas, s.acceleration_structure_size, scratch.max(s.build_scratch_size)))
    }
}

#[cfg(not(target_vendor = "apple"))]
fn backend_sizes(_: &wgpu::Device, _: &Scene, _: u32) -> Option<(u64, u64, u64)> {
    None
}

#[cfg(target_vendor = "apple")]
#[test]
fn the_backend_reports_acceleration_structure_sizes() {
    let Some(gpu) = ray_query_gpu() else {
        return;
    };
    let (geometries, instances) = mirror_scene(true);
    let scene = Scene::build(&gpu.device, &gpu.queue, &geometries, &instances, None);
    let (blas, tlas, scratch) = backend_sizes(&gpu.device, &scene, instances.len() as u32).expect("a Metal device");
    assert!(blas > 0 && tlas > 0 && scratch > 0, "({blas}, {tlas}, {scratch})");
}

/// A town-sized scene for the cost run: a 200 m floor of 131,072
/// triangles, 1,024 box buildings and 1,024 spheres of 4,096 triangles,
/// on a 32 by 32 grid of streets, a few of the buildings glowing.
fn town() -> (Vec<Geometry>, Vec<Instance>) {
    let geometries = vec![Geometry::floor(200.0, 256), Geometry::cube(), Geometry::sphere(32, 64)];
    let mut instances = vec![Instance {
        geometry: 0,
        transform: Mat4::IDENTITY,
        id: FLOOR,
    }];
    for i in 0..1024u32 {
        let (x, z) = ((i % 32) as f32 * 6.0 - 93.0, (i / 32) as f32 * 6.0 - 93.0);
        let height = 2.0 + (i.wrapping_mul(2_654_435_761) >> 28) as f32;
        instances.push(Instance {
            geometry: 1,
            transform: Mat4::from_scale_rotation_translation(
                vec3(3.0, height, 3.0),
                glam::Quat::IDENTITY,
                vec3(x, height * 0.5, z),
            ),
            id: if i % 7 == 0 { GLOW } else { DARK },
        });
        instances.push(Instance {
            geometry: 2,
            transform: Mat4::from_translation(vec3(x + 3.0, 0.6, z + 3.0)),
            id: DARK,
        });
    }
    (geometries, instances)
}

/// GPU time of the trace pass, median of `frames` timed dispatches, in ms.
fn trace_ms(gpu: &crate::render::render_test_support::TestGpu, tracer: &Tracer, frames: u32) -> f64 {
    // Metal allows 32 timestamp query sets per process (`query_sets.rs`).
    let _lease = super::query_sets::lease(&gpu.device, "ray query timer").expect("a timestamp query set");
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("ray query timestamps"),
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let period = f64::from(gpu.queue.get_timestamp_period());
    let mut samples = Vec::new();
    for _ in 0..frames {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        tracer.dispatch(
            &mut encoder,
            Some(wgpu::ComputePassTimestampWrites {
                query_set: &queries,
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: Some(1),
            }),
        );
        encoder.resolve_query_set(&queries, 0..2, &resolve, 0);
        encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 16);
        gpu.queue.submit([encoder.finish()]);
        read.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let ticks: [u64; 2] = bytemuck::pod_read_unaligned(&read.get_mapped_range(..).unwrap()[..16]);
        read.unmap();
        samples.push(ticks[1].saturating_sub(ticks[0]) as f64 * period / 1e6);
    }
    median(samples)
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

/// Wall time of `run` (submit and wait), median of `runs`, in ms.
fn wall_ms(runs: u32, mut run: impl FnMut()) -> f64 {
    median(
        (0..runs)
            .map(|_| {
                let start = std::time::Instant::now();
                run();
                start.elapsed().as_secs_f64() * 1e3
            })
            .collect(),
    )
}

/// The prototype's cost on the town scene: acceleration structure sizes
/// and build times, and the trace pass's GPU time at 1920x1080 and at half
/// that (the screen-space trace's size). Run in release:
/// `cargo test --release --lib --features 3d -- --ignored --nocapture
/// --test-threads=1 ray_query_reflection_cost` (one test at a time, so no
/// other test shares the GPU).
#[test]
#[ignore]
fn ray_query_reflection_cost() {
    let Some(gpu) = ray_query_gpu() else {
        return;
    };
    if !gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        crate::render::render_test_support::skip("timestamp queries unavailable");
        return;
    }
    let (geometries, instances) = town();
    let instanced: u64 = instances
        .iter()
        .map(|i| u64::from(geometries[i.geometry].triangles()))
        .sum();
    let scene = Scene::build(&gpu.device, &gpu.queue, &geometries, &instances, None);
    eprintln!(
        "town: {} unique triangles in {} meshes, {} instances, {instanced} instanced triangles",
        scene.triangles(),
        geometries.len(),
        instances.len()
    );
    match backend_sizes(&gpu.device, &scene, instances.len() as u32) {
        Some((blas, tlas, scratch)) => eprintln!(
            "acceleration structures: BLAS {:.2} MB, TLAS {:.2} MB, build scratch {:.2} MB",
            blas as f64 / 1e6,
            tlas as f64 / 1e6,
            scratch as f64 / 1e6
        ),
        None => eprintln!("acceleration structure sizes: not measured on this backend"),
    }
    let empty = wall_ms(20, || {
        gpu.queue.submit([]);
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    });
    let full = wall_ms(10, || scene.rebuild(&gpu.device, &gpu.queue, true));
    let tlas = wall_ms(20, || scene.rebuild(&gpu.device, &gpu.queue, false));
    eprintln!(
        "build (wall time, submit to done; an empty submit takes {empty:.3} ms): every BLAS and the TLAS {full:.2} ms, \
         the TLAS alone {tlas:.3} ms"
    );
    for (width, height) in [(1920, 1080), (960, 540)] {
        let tracer = Tracer::new(&gpu.device, &scene, (width, height));
        let mut camera = Camera3D::new(Vec2::new(width as f32, height as f32));
        // Street level, down a street with buildings on both sides.
        camera.position = vec3(-90.0, 1.7, -90.0);
        camera.look_at(vec3(0.0, 1.0, 0.0));
        for reflect in [false, true] {
            tracer.aim(&gpu.queue, &camera, reflect);
            let ms = trace_ms(&gpu, &tracer, 200);
            let ms2 = trace_ms(&gpu, &tracer, 200);
            eprintln!(
                "{width}x{height}: {} trace median {ms:.3} ms (repeat {ms2:.3} ms)",
                if reflect {
                    "camera + reflection rays"
                } else {
                    "camera rays only"
                }
            );
        }
        let lit = tracer.draw(&gpu).fraction(0..width, 0..height, |p| p[0] > 120);
        eprintln!(
            "{width}x{height}: {:.1}% of the pixels see or reflect a glowing building",
            lit * 100.0
        );
    }
}
