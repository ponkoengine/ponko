//! Order-independent transparency, measured against sorting.
//!
//! Three see-through scenes (intersecting glass panes, particles through a
//! tilted pane, layered crossed foliage cards) drawn offscreen four ways:
//!
//! - **sorted**, as the engine draws see-through things: models far to near
//!   by their centre's distance (a model's own triangles in index order),
//!   then the particles far to near among themselves, premultiplied
//!   "over" blending;
//! - **sorted together**: the same, but particles and models in one list,
//!   which shows how much of sorting's error is the separate particle list;
//! - **weighted blended OIT** (McGuire and Bavoil 2013, weight function 7):
//!   one accumulation pass into a 16-bit float colour target and a 16-bit
//!   float weight target, then one full-screen resolve;
//! - **depth peeling** with a fixed number of layers: exact up to that many
//!   layers per pixel, one geometry pass per layer.
//!
//! The reference is depth peeling run until a layer comes back empty: every
//! fragment of every pixel, composited in exact per-pixel depth order (the
//! scenes have no coplanar overlaps, so the order has no ties).
//!
//! The quality test is part of the suite. The cost is an ignored test, run
//! explicitly, which times each method with the engine's GPU pass timer
//! (`timer.rs`):
//! `cargo test --lib --features 3d oit_gpu_cost_benchmark -- --ignored --nocapture`.
//! `PONKO_OIT_RESULTS=<dir>` saves every image the quality test compares
//! (and a difference image per method). The numbers and the decision are in
//! the guide's rendering notes (`book/src/3d.md`, "Order-independent
//! transparency"). Offscreen only, no window.
use super::render_test_support::{self as support, Image, TestGpu};
use crate::random::Rng;
use glam::{Mat4, Quat, Vec3, vec3};
use std::f32::consts::FRAC_PI_2;
use wgpu::util::DeviceExt;

const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const WEIGHT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const OUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Layers the reference peels at most before giving up.
const MAX_LAYERS: u32 = 64;
const EYE: Vec3 = vec3(0.0, 0.0, 4.0);

/// The blend state of both weighted targets. One state for both: WebGL2
/// (wgpu's GL backend without `GL_EXT_draw_buffers_indexed`, so without
/// `DownlevelFlags::INDEPENDENT_BLEND`) can't blend each target its own way.
/// Colour adds (the weighted sums), alpha multiplies by `1 - alpha` (the
/// revealage), so the colour target keeps `sum(c * a * w)` in RGB and the
/// revealage in A, and the weight target `sum(a * w)` in R.
const WEIGHTED_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Front-to-back "under" compositing of a peeled layer into the layers
/// nearer than it.
const UNDER: wgpu::BlendComponent = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
    dst_factor: wgpu::BlendFactor::One,
    operation: wgpu::BlendOperation::Add,
};

const SHADER: &str = r#"
struct Frame { view_proj: mat4x4<f32>, view: mat4x4<f32> };
@group(0) @binding(0) var<uniform> frame: Frame;

struct In {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) shape: u32,
};
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) shape: u32,
    @location(3) view_z: f32,
};

@vertex fn vs(v: In) -> Out {
    var o: Out;
    o.clip = frame.view_proj * vec4<f32>(v.pos, 1.0);
    o.view_z = -(frame.view * vec4<f32>(v.pos, 1.0)).z;
    o.uv = v.uv;
    o.color = v.color;
    o.shape = v.shape;
    return o;
}

// 0: a flat pane, 1: a soft round puff, 2: a five-lobed leaf with a soft rim.
fn coverage(uv: vec2<f32>, shape: u32) -> f32 {
    let p = uv * 2.0 - 1.0;
    let r = length(p);
    if shape == 1u {
        return exp(-3.0 * r * r) * (1.0 - smoothstep(0.85, 1.0, r));
    }
    if shape == 2u {
        let edge = 0.72 + 0.2 * cos(5.0 * atan2(p.y, p.x));
        return 1.0 - smoothstep(edge - 0.1, edge, r);
    }
    return 1.0;
}

fn premultiplied(o: Out) -> vec4<f32> {
    let a = o.color.a * coverage(o.uv, o.shape);
    return vec4<f32>(o.color.rgb * a, a);
}

@fragment fn fs_blend(o: Out) -> @location(0) vec4<f32> {
    return premultiplied(o);
}

struct Weighted { @location(0) sum: vec4<f32>, @location(1) weight: vec4<f32> };

@fragment fn fs_weighted(o: Out) -> Weighted {
    let c = premultiplied(o);
    let z = abs(o.view_z);
    let w = c.a * clamp(10.0 / (1e-5 + pow(z / 5.0, 2.0) + pow(z / 200.0, 6.0)), 1e-2, 3e3);
    var out: Weighted;
    out.sum = vec4<f32>(c.rgb * w, c.a);
    out.weight = vec4<f32>(c.a * w, 0.0, 0.0, c.a);
    return out;
}

@group(1) @binding(0) var nearer: texture_depth_2d;

// The depth is written from the shader: the value the next layer compares
// against must be exactly the one this fragment saw (the fixed-function
// depth can differ from `position.z` in the last bit, and a layer would then
// peel the same fragment forever).
struct Peeled { @location(0) color: vec4<f32>, @builtin(frag_depth) depth: f32 };

@fragment fn fs_peel(o: Out) -> Peeled {
    let peeled = textureLoad(nearer, vec2<i32>(o.clip.xy), 0);
    let c = premultiplied(o);
    if o.clip.z <= peeled || c.a < 1e-4 {
        discard;
    }
    return Peeled(c, o.clip.z);
}

@vertex fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}

@fragment fn fs_background(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let odd = ((u32(p.x) / 16u + u32(p.y) / 16u) % 2u) == 1u;
    let v = select(0.18, 0.32, odd);
    return vec4<f32>(v, v, v, 1.0);
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var weights: texture_2d<f32>;

@fragment fn fs_copy(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(p.xy), 0);
}

@fragment fn fs_resolve(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(p.xy);
    let sum = textureLoad(source, at, 0);
    let weight = textureLoad(weights, at, 0).r;
    let covered = 1.0 - sum.a;
    if covered <= 0.0 {
        discard;
    }
    return vec4<f32>(sum.rgb / max(weight, 1e-5) * covered, covered);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    shape: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Pane = 0,
    Puff = 1,
    Leaf = 2,
}

/// Which of the engine's lists a see-through thing is drawn from.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Model,
    Particle,
}

struct Object {
    first: u32,
    count: u32,
    center: Vec3,
    kind: Kind,
}

pub(super) struct Scene {
    name: &'static str,
    vertices: Vec<Vertex>,
    objects: Vec<Object>,
}

/// A quad's half extents, turned by `yaw` about Y after `pitch` about X.
fn pane(yaw: f32, pitch: f32, half: f32) -> (Vec3, Vec3) {
    let r = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
    (r * Vec3::X * half, r * Vec3::Y * half)
}

impl Scene {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            vertices: Vec::new(),
            objects: Vec::new(),
        }
    }

    /// One object: `quads` (half extents) centred on `center`, all `color`.
    fn push(&mut self, kind: Kind, center: Vec3, quads: &[(Vec3, Vec3)], color: [f32; 4], shape: Shape) {
        let quads: Vec<_> = quads.iter().map(|&(right, up)| (right, up, color)).collect();
        self.push_shaded(kind, center, &quads, shape);
    }

    /// One object of quads with a colour each.
    fn push_shaded(&mut self, kind: Kind, center: Vec3, quads: &[(Vec3, Vec3, [f32; 4])], shape: Shape) {
        let first = self.vertices.len() as u32;
        for &(right, up, color) in quads {
            let corner = |s: f32, t: f32| Vertex {
                pos: (center + right * (s * 2.0 - 1.0) + up * (t * 2.0 - 1.0)).to_array(),
                uv: [s, t],
                color,
                shape: shape as u32,
            };
            let quad = [
                corner(0., 0.),
                corner(1., 0.),
                corner(1., 1.),
                corner(0., 0.),
                corner(1., 1.),
                corner(0., 1.),
            ];
            self.vertices.extend(quad);
        }
        let count = self.vertices.len() as u32 - first;
        self.objects.push(Object {
            first,
            count,
            center,
            kind,
        });
    }

    /// Indices in the engine's order (far to near by centre; particles after
    /// the models unless `together`), each object's triangles as built.
    fn sorted_indices(&self, together: bool) -> Vec<u32> {
        let mut order: Vec<usize> = (0..self.objects.len()).collect();
        let rank = |o: &Object| u8::from(!together && o.kind == Kind::Particle);
        order.sort_by(|&a, &b| {
            let (a, b) = (&self.objects[a], &self.objects[b]);
            rank(a)
                .cmp(&rank(b))
                .then((b.center - EYE).length().total_cmp(&(a.center - EYE).length()))
        });
        order
            .into_iter()
            .flat_map(|i| {
                let o = &self.objects[i];
                o.first..o.first + o.count
            })
            .collect()
    }
}

/// Five panes: three crossing each other near the middle, one behind and
/// one in front of them.
pub(super) fn intersecting_glass() -> Scene {
    let mut s = Scene::new("intersecting_glass");
    s.push(
        Kind::Model,
        vec3(0.0, 0.0, -1.3),
        &[pane(0.0, 0.0, 0.8)],
        [1.0, 0.75, 0.1, 0.4],
        Shape::Pane,
    );
    s.push(
        Kind::Model,
        vec3(-0.1, 0.05, 0.0),
        &[pane(0.7, 0.0, 1.1)],
        [0.9, 0.15, 0.1, 0.5],
        Shape::Pane,
    );
    s.push(
        Kind::Model,
        vec3(0.1, -0.05, 0.05),
        &[pane(-0.7, 0.0, 1.1)],
        [0.1, 0.8, 0.2, 0.5],
        Shape::Pane,
    );
    s.push(
        Kind::Model,
        vec3(0.0, 0.1, -0.05),
        &[pane(0.0, 0.8, 1.0)],
        [0.15, 0.3, 1.0, 0.45],
        Shape::Pane,
    );
    s.push(
        Kind::Model,
        vec3(0.5, -0.4, 1.2),
        &[pane(0.0, 0.0, 0.5)],
        [0.9, 0.9, 0.95, 0.3],
        Shape::Pane,
    );
    s
}

/// A tilted cyan pane with `count` soft fire puffs (camera-facing) in front
/// of it, behind it and cutting through it.
pub(super) fn particles_through_glass(count: usize) -> Scene {
    let mut s = Scene::new("particles_through_glass");
    s.push(
        Kind::Model,
        Vec3::ZERO,
        &[pane(0.45, 0.0, 1.3)],
        [0.2, 0.7, 0.9, 0.55],
        Shape::Pane,
    );
    let mut rng = Rng::new(55);
    for _ in 0..count {
        let at = vec3(rng.range(-1.5, 1.5), rng.range(-1.4, 1.4), rng.range(-1.4, 1.4));
        let half = rng.range(0.07, 0.18);
        let color = [1.0, rng.range(0.3, 0.7), 0.1, rng.range(0.5, 0.9)];
        s.push(
            Kind::Particle,
            at,
            &[(Vec3::X * half, Vec3::Y * half)],
            color,
            Shape::Puff,
        );
    }
    s
}

/// `count` bushes, each two leaf cards crossed at right angles (one model),
/// the second one shaded darker.
pub(super) fn layered_foliage(count: usize) -> Scene {
    let mut s = Scene::new("layered_foliage");
    let mut rng = Rng::new(56);
    for _ in 0..count {
        let at = vec3(rng.range(-1.6, 1.6), rng.range(-1.1, 1.1), rng.range(-1.5, 1.2));
        let (yaw, half) = (rng.angle(), rng.range(0.35, 0.55));
        let color = [
            rng.range(0.1, 0.35),
            rng.range(0.45, 0.85),
            rng.range(0.1, 0.25),
            rng.range(0.85, 0.97),
        ];
        // The second card faces another way, so the light shades it darker.
        let shaded = [color[0] * 0.6, color[1] * 0.6, color[2] * 0.6, color[3]];
        let ((r1, u1), (r2, u2)) = (pane(yaw, 0.0, half), pane(yaw + FRAC_PI_2, 0.0, half));
        s.push_shaded(Kind::Model, at, &[(r1, u1, color), (r2, u2, shaded)], Shape::Leaf);
    }
    s
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Method {
    Sorted,
    SortedTogether,
    Weighted,
    Peeled(u32),
}

impl Method {
    fn name(self) -> String {
        match self {
            Method::Sorted => "sorted".into(),
            Method::SortedTogether => "sorted_together".into(),
            Method::Weighted => "weighted".into(),
            Method::Peeled(n) => format!("peeled{n}"),
        }
    }

    /// Bytes per pixel (per sample) of the targets the method adds to the
    /// scene's colour and depth.
    pub(super) fn bytes_per_pixel(self) -> u32 {
        let size = |f: wgpu::TextureFormat| f.block_copy_size(None).unwrap_or(4);
        match self {
            Method::Sorted | Method::SortedTogether => 0,
            Method::Weighted => size(HDR) + size(WEIGHT),
            // The layer, the layers composited so far, and two depths
            // (this layer's and the one nearer).
            Method::Peeled(_) => size(HDR) * 2 + size(DEPTH) * 2,
        }
    }
}

/// The colour targets of the weighted pass: both with [`WEIGHTED_BLEND`].
fn weighted_targets() -> [Option<wgpu::ColorTargetState>; 2] {
    [HDR, WEIGHT].map(|format| {
        Some(wgpu::ColorTargetState {
            format,
            blend: Some(WEIGHTED_BLEND),
            write_mask: wgpu::ColorWrites::ALL,
        })
    })
}

struct Uploaded {
    vertices: wgpu::Buffer,
    vertex_count: u32,
    sorted: wgpu::Buffer,
    together: wgpu::Buffer,
}

pub(super) struct Rig {
    pub gpu: TestGpu,
    frame: wgpu::BindGroup,
    background: wgpu::RenderPipeline,
    sorted: wgpu::RenderPipeline,
    weighted: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    peel: wgpu::RenderPipeline,
    under: wgpu::RenderPipeline,
    over: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    hdr: wgpu::Texture,
    out: wgpu::Texture,
    sum: wgpu::Texture,
    weight: wgpu::Texture,
    layer: wgpu::Texture,
    accum: wgpu::Texture,
    depth: [wgpu::Texture; 3],
    /// Reads of the depths: `[0]`, `[1]` ping-pong, `[2]` is all zero (nothing peeled yet).
    nearer: [wgpu::BindGroup; 3],
    resolve_group: wgpu::BindGroup,
    layer_group: wgpu::BindGroup,
    accum_group: wgpu::BindGroup,
    hdr_group: wgpu::BindGroup,
}

fn texture(device: &wgpu::Device, size: (u32, u32), format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("oit"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&Default::default())
}

fn attach(view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    })
}

const CLEAR: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);

impl Rig {
    pub fn new(gpu: TestGpu, size: (u32, u32)) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[entry(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            )],
        });
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[entry(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )],
        });
        let float = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let pair_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(0, wgpu::ShaderStages::FRAGMENT, float),
                entry(1, wgpu::ShaderStages::FRAGMENT, float),
            ],
        });
        let layout = |groups: &[&wgpu::BindGroupLayout]| {
            let groups: Vec<_> = groups.iter().map(|g| Some(*g)).collect();
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &groups,
                immediate_size: 0,
            })
        };
        let scene_layout = layout(&[&frame_layout]);
        let peel_layout = layout(&[&frame_layout, &depth_layout]);
        let full_layout = layout(&[&pair_layout]);
        let empty_layout = layout(&[]);
        let vertex_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Uint32],
        };
        let vertex_buffers = [Some(vertex_buffer)];
        let pipeline = |layout: &wgpu::PipelineLayout,
                        full: bool,
                        fragment: &str,
                        targets: &[Option<wgpu::ColorTargetState>],
                        depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(if full { "vs_full" } else { "vs" }),
                    compilation_options: Default::default(),
                    buffers: if full { &[] } else { &vertex_buffers },
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth.then(|| wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let target = |format, blend| {
            [Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let premultiplied = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let under = Some(wgpu::BlendState {
            color: UNDER,
            alpha: UNDER,
        });
        let background = pipeline(&empty_layout, true, "fs_background", &target(HDR, None), false);
        let sorted = pipeline(&scene_layout, false, "fs_blend", &target(HDR, premultiplied), false);
        let weighted = pipeline(&scene_layout, false, "fs_weighted", &weighted_targets(), false);
        let resolve = pipeline(&full_layout, true, "fs_resolve", &target(HDR, premultiplied), false);
        let peel = pipeline(&peel_layout, false, "fs_peel", &target(HDR, None), true);
        let under = pipeline(&full_layout, true, "fs_copy", &target(HDR, under), false);
        let over = pipeline(&full_layout, true, "fs_copy", &target(HDR, premultiplied), false);
        let blit = pipeline(&full_layout, true, "fs_copy", &target(OUT, None), false);

        let aspect = size.0 as f32 / size.1 as f32;
        // The camera on +Z looking at the origin; a plain perspective with
        // depth 0 at the near plane (the engine's own camera is reverse-Z,
        // which only flips the depth test).
        let view_m = Mat4::from_translation(-EYE);
        let (f, near, far) = (1.0 / 25f32.to_radians().tan(), 0.1, 50.0);
        let proj = Mat4::from_cols_array(&[
            f / aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            f,
            0.0,
            0.0,
            0.0,
            0.0,
            far / (near - far),
            -1.0,
            0.0,
            0.0,
            near * far / (near - far),
            0.0,
        ]);
        let uniform: Vec<f32> = [proj * view_m, view_m].iter().flat_map(|m| m.to_cols_array()).collect();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("oit frame"),
            contents: bytemuck::cast_slice(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let hdr = texture(device, size, HDR);
        let out = texture(device, size, OUT);
        let sum = texture(device, size, HDR);
        let weight = texture(device, size, WEIGHT);
        let layer = texture(device, size, HDR);
        let accum = texture(device, size, HDR);
        let depth = [(); 3].map(|()| texture(device, size, DEPTH));
        let nearer = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &depth_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view(&depth[i])),
                }],
            })
        });
        let pair = |a: &wgpu::Texture, b: &wgpu::Texture| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pair_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view(a)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view(b)),
                    },
                ],
            })
        };
        let resolve_group = pair(&sum, &weight);
        let layer_group = pair(&layer, &layer);
        let accum_group = pair(&accum, &accum);
        let hdr_group = pair(&hdr, &hdr);
        // Nothing is nearer than the first layer: a depth of 0 everywhere.
        let mut encoder = device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oit zero depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view(&depth[2]),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        gpu.queue.submit([encoder.finish()]);
        Self {
            frame,
            background,
            sorted,
            weighted,
            resolve,
            peel,
            under,
            over,
            blit,
            hdr,
            out,
            sum,
            weight,
            layer,
            accum,
            depth,
            nearer,
            resolve_group,
            layer_group,
            accum_group,
            hdr_group,
            gpu,
        }
    }

    fn upload(&self, scene: &Scene) -> Uploaded {
        let device = &self.gpu.device;
        let indices = |together| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oit sorted"),
                contents: bytemuck::cast_slice(&scene.sorted_indices(together)),
                usage: wgpu::BufferUsages::INDEX,
            })
        };
        Uploaded {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("oit vertices"),
                contents: bytemuck::cast_slice(&scene.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            vertex_count: scene.vertices.len() as u32,
            sorted: indices(false),
            together: indices(true),
        }
    }

    /// A full-screen pass into `target`, reading `group`.
    fn full(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        load: wgpu::LoadOp<wgpu::Color>,
        pipeline: &wgpu::RenderPipeline,
        group: Option<&wgpu::BindGroup>,
        time: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let target = view(target);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oit full screen"),
            color_attachments: &[attach(&target, load)],
            depth_stencil_attachment: None,
            timestamp_writes: time,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        if let Some(group) = group {
            pass.set_bind_group(0, group, &[]);
        }
        pass.draw(0..3, 0..1);
    }

    /// Peels layer `i` (0 = nearest) into `layer` and composites it under
    /// the layers before it in `accum`.
    fn peel_layer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Uploaded,
        i: u32,
        timer: Option<&super::timer::GpuTimer>,
    ) {
        let (layer, depth) = (view(&self.layer), view(&self.depth[i as usize % 2]));
        let nearer = if i == 0 { 2 } else { (i as usize + 1) % 2 };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("oit peel"),
                color_attachments: &[attach(&layer, CLEAR)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timer.and_then(|t| t.pass("peel")),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.peel);
            pass.set_bind_group(0, &self.frame, &[]);
            pass.set_bind_group(1, &self.nearer[nearer], &[]);
            pass.set_vertex_buffer(0, scene.vertices.slice(..));
            pass.draw(0..scene.vertex_count, 0..1);
        }
        let load = if i == 0 { CLEAR } else { wgpu::LoadOp::Load };
        self.full(
            encoder,
            &self.accum,
            load,
            &self.under,
            Some(&self.layer_group),
            timer.and_then(|t| t.pass("under")),
        );
    }

    /// Records the background, then `method`'s passes over it (timed with
    /// `timer`), then the copy into the 8-bit image.
    fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Uploaded,
        method: Method,
        timer: Option<&super::timer::GpuTimer>,
    ) {
        self.full(encoder, &self.hdr, CLEAR, &self.background, None, None);
        let hdr = view(&self.hdr);
        match method {
            Method::Sorted | Method::SortedTogether => {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("oit sorted"),
                    color_attachments: &[attach(&hdr, wgpu::LoadOp::Load)],
                    depth_stencil_attachment: None,
                    timestamp_writes: timer.and_then(|t| t.pass("sorted")),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.sorted);
                pass.set_bind_group(0, &self.frame, &[]);
                pass.set_vertex_buffer(0, scene.vertices.slice(..));
                let indices = if method == Method::Sorted {
                    &scene.sorted
                } else {
                    &scene.together
                };
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..scene.vertex_count, 0, 0..1);
            }
            Method::Weighted => {
                let (sum, weight) = (view(&self.sum), view(&self.weight));
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("oit weighted"),
                        color_attachments: &[
                            attach(
                                &sum,
                                wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 1.0,
                                }),
                            ),
                            attach(&weight, CLEAR),
                        ],
                        depth_stencil_attachment: None,
                        timestamp_writes: timer.and_then(|t| t.pass("weighted")),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&self.weighted);
                    pass.set_bind_group(0, &self.frame, &[]);
                    pass.set_vertex_buffer(0, scene.vertices.slice(..));
                    pass.draw(0..scene.vertex_count, 0..1);
                }
                self.full(
                    encoder,
                    &self.hdr,
                    wgpu::LoadOp::Load,
                    &self.resolve,
                    Some(&self.resolve_group),
                    timer.and_then(|t| t.pass("resolve")),
                );
            }
            Method::Peeled(layers) => {
                for i in 0..layers {
                    self.peel_layer(encoder, scene, i, timer);
                }
                self.over_accum(encoder, timer);
            }
        }
        self.full(encoder, &self.out, CLEAR, &self.blit, Some(&self.hdr_group), None);
    }

    fn over_accum(&self, encoder: &mut wgpu::CommandEncoder, timer: Option<&super::timer::GpuTimer>) {
        self.full(
            encoder,
            &self.hdr,
            wgpu::LoadOp::Load,
            &self.over,
            Some(&self.accum_group),
            timer.and_then(|t| t.pass("over")),
        );
    }

    fn draw(&self, scene: &Uploaded, method: Method) -> Image {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.record(&mut encoder, scene, method, None);
        self.gpu.queue.submit([encoder.finish()]);
        Image::read(&self.gpu.device, &self.gpu.queue, &self.out)
    }

    /// The exact picture: layers peeled until one comes back empty, and how
    /// many held something.
    fn reference(&self, scene: &Uploaded) -> (Image, u32) {
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let mut encoder = device.create_command_encoder(&Default::default());
        self.full(&mut encoder, &self.hdr, CLEAR, &self.background, None, None);
        queue.submit([encoder.finish()]);
        let mut layers = 0;
        loop {
            assert!(layers < MAX_LAYERS, "{MAX_LAYERS} layers peeled and still not empty");
            let mut encoder = device.create_command_encoder(&Default::default());
            self.peel_layer(&mut encoder, scene, layers, None);
            queue.submit([encoder.finish()]);
            let texels = support::read_texture(device, queue, &self.layer);
            if texels.iter().all(|&b| b == 0) {
                break;
            }
            layers += 1;
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        self.over_accum(&mut encoder, None);
        self.full(&mut encoder, &self.out, CLEAR, &self.blit, Some(&self.hdr_group), None);
        queue.submit([encoder.finish()]);
        (Image::read(device, queue, &self.out), layers)
    }
}

/// How far an image is from the reference, over the pixels the scene covers
/// (where the reference differs from the bare background).
#[derive(Clone, Copy, Debug)]
pub(super) struct Error {
    /// Mean absolute difference, 8-bit sRGB levels, averaged over RGB.
    pub mean: f64,
    /// Fraction of covered pixels off by more than 8 levels in a channel:
    /// visibly wrong.
    pub wrong: f64,
    /// Fraction off by more than 24 levels: badly wrong (a surface in the
    /// wrong order, not a slightly different tint).
    pub bad: f64,
    /// The largest difference in any channel.
    pub max: u8,
}

fn error(image: &Image, reference: &Image, background: &Image) -> Error {
    let (mut sum, mut wrong, mut bad, mut max, mut covered) = (0u64, 0usize, 0usize, 0u8, 0usize);
    for ((p, r), b) in image
        .rgba
        .chunks_exact(4)
        .zip(reference.rgba.chunks_exact(4))
        .zip(background.rgba.chunks_exact(4))
    {
        if (0..3).all(|c| r[c].abs_diff(b[c]) <= 2) {
            continue;
        }
        covered += 1;
        let d: [u8; 3] = std::array::from_fn(|c| p[c].abs_diff(r[c]));
        sum += d.iter().map(|&d| u64::from(d)).sum::<u64>();
        let worst = *d.iter().max().unwrap();
        wrong += usize::from(worst > 8);
        bad += usize::from(worst > 24);
        max = max.max(worst);
    }
    assert!(covered > 0, "the scene covers no pixel");
    Error {
        mean: sum as f64 / (covered * 3) as f64,
        wrong: wrong as f64 / covered as f64,
        bad: bad as f64 / covered as f64,
        max,
    }
}

/// With `PONKO_OIT_RESULTS=<dir>`: `<dir>/<scene>.<method>.png`, and for
/// a method its difference from the reference, four times brighter.
fn save(scene: &str, method: &str, image: &Image, reference: Option<&Image>) {
    let Some(dir) = std::env::var_os("PONKO_OIT_RESULTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: String, rgba: &[u8]| {
        image::save_buffer(dir.join(name), rgba, image.width, image.height, image::ColorType::Rgba8).unwrap();
    };
    write(format!("{scene}.{method}.png"), &image.rgba);
    if let Some(reference) = reference {
        let diff: Vec<u8> = image
            .rgba
            .chunks_exact(4)
            .zip(reference.rgba.chunks_exact(4))
            .flat_map(|(p, r)| {
                let d: [u8; 3] = std::array::from_fn(|c| p[c].abs_diff(r[c]).saturating_mul(4));
                [d[0], d[1], d[2], 255]
            })
            .collect();
        write(format!("{scene}.{method}.diff.png"), &diff);
    }
}

const SIZE: (u32, u32) = (192, 192);
const METHODS: [Method; 5] = [
    Method::Sorted,
    Method::SortedTogether,
    Method::Weighted,
    Method::Peeled(1),
    Method::Peeled(4),
];

fn scenes() -> [Scene; 3] {
    [intersecting_glass(), particles_through_glass(300), layered_foliage(30)]
}

/// Each scene's error per method in [`METHODS`], and its reference's layers.
fn measure(rig: &Rig) -> Vec<(&'static str, u32, Vec<Error>)> {
    let empty = rig.upload(&{
        let mut s = Scene::new("empty");
        // One zero-alpha quad: draws nothing.
        s.push(Kind::Model, Vec3::ZERO, &[pane(0.0, 0.0, 0.1)], [0.0; 4], Shape::Pane);
        s
    });
    let background = rig.draw(&empty, Method::Sorted);
    let out = support::output_dir();
    scenes()
        .iter()
        .map(|scene| {
            let uploaded = rig.upload(scene);
            let (reference, layers) = rig.reference(&uploaded);
            save(scene.name, "reference", &reference, None);
            let errors: Vec<Error> = METHODS
                .iter()
                .map(|&method| {
                    let image = rig.draw(&uploaded, method);
                    save(scene.name, &method.name(), &image, Some(&reference));
                    let e = error(&image, &reference, &background);
                    support::record(
                        &out,
                        &format!(
                            "oit {:<24} {:<16} mean {:5.2} levels, {:5.1}% off > 8, {:5.1}% off > 24, max {:3} ({layers} layers)",
                            scene.name,
                            method.name(),
                            e.mean,
                            e.wrong * 100.0,
                            e.bad * 100.0,
                            e.max
                        ),
                    );
                    e
                })
                .collect();
            (scene.name, layers, errors)
        })
        .collect()
}

fn of(errors: &[Error], method: Method) -> Error {
    errors[METHODS.iter().position(|&m| m == method).unwrap()]
}

/// What the evaluation found, held on every adapter the suite runs on
/// (the numbers on an Apple M4 Max are in the guide): weighted blended OIT
/// doesn't fix sorting's errors, it trades them. Where sorting draws a
/// surface in the wrong order (crossing panes, crossed cards) weighted
/// blending has fewer badly wrong pixels, but on every scene more pixels
/// visibly off in tint, and on near-opaque foliage a larger error overall.
/// Most of sorting's error with particles behind glass comes from the
/// particles being a list of their own, drawn after the see-through models.
#[test]
fn weighted_oit_trades_sorting_errors_for_wider_tint_errors() {
    let Some(gpu) = support::gpu() else { return };
    let rig = Rig::new(gpu, SIZE);
    let results = measure(&rig);
    for (scene, layers, errors) in &results {
        // The reference is exact: peeling stopped at an empty layer, and a
        // scene this deep needs more than one layer (the negative control:
        // one layer is visibly wrong).
        assert!(*layers >= 3, "{scene}: only {layers} layers deep");
        assert!(
            of(errors, Method::Peeled(1)).wrong > 0.05,
            "{scene}: one layer passes for exact"
        );
        assert!(
            of(errors, Method::Peeled(4)).mean < 1.0,
            "{scene}: four layers are nearly exact"
        );
        let (sorted, weighted) = (of(errors, Method::Sorted), of(errors, Method::Weighted));
        assert!(
            weighted.wrong > sorted.wrong * 1.2,
            "{scene}: weighted blending tints more pixels visibly wrong ({weighted:?} against {sorted:?})"
        );
    }
    let scene = |name| &results.iter().find(|r| r.0 == name).unwrap().2;
    let glass = scene("intersecting_glass");
    assert!(
        of(glass, Method::Weighted).bad < of(glass, Method::Sorted).bad / 2.0,
        "crossing panes: weighted blending has fewer badly wrong pixels than sorting"
    );
    let foliage = scene("layered_foliage");
    assert!(
        of(foliage, Method::Weighted).mean > of(foliage, Method::Sorted).mean * 1.2,
        "near-opaque foliage: weighted blending is further off than sorting"
    );
    let particles = scene("particles_through_glass");
    assert!(
        of(particles, Method::SortedTogether).mean < of(particles, Method::Sorted).mean / 2.0,
        "particles sorted with the models are far closer than particles drawn after them"
    );
}

/// Weighted blended OIT on a device opened with WebGL2's limits (wgpu's
/// `downlevel_webgl2_defaults`: no storage buffers, at most 4 colour targets
/// and 32 bytes of them per sample): it needs only two float colour targets
/// and one blend state shared by both, so it runs there and draws what it
/// draws on the full device. (The browser's other conditions are in the
/// guide: `EXT_color_buffer_float` for blending into 16-bit float targets,
/// which the engine's HDR target already needs.)
#[test]
fn weighted_oit_needs_only_webgl2_limits_and_one_blend_state() {
    let [sum, weight] = weighted_targets().map(Option::unwrap);
    assert_eq!(sum.blend, weight.blend, "WebGL2 can't blend each target its own way");
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
    let limits = limited.device.limits();
    assert_eq!(
        limits.max_storage_buffers_per_shader_stage, 0,
        "a WebGL2 device: no storage buffers"
    );
    assert!(limits.max_color_attachments >= 2);
    assert!(Method::Weighted.bytes_per_pixel() <= limits.max_color_attachment_bytes_per_sample);
    let scene = intersecting_glass();
    let draw = |gpu| {
        let rig = Rig::new(gpu, SIZE);
        let uploaded = rig.upload(&scene);
        rig.draw(&uploaded, Method::Weighted)
    };
    let (on_full, on_webgl2) = (draw(full), draw(limited));
    let d = support::diff(&on_full, &on_webgl2, 1);
    assert_eq!(d.outliers, 0, "the WebGL2-limited device draws the same picture: {d:?}");
    // Negative control: the comparison sees a different method.
    let rig = Rig::new(support::gpu().expect("a device, as above"), SIZE);
    let sorted = rig.draw(&rig.upload(&scene), Method::Sorted);
    assert!(
        support::diff(&on_full, &sorted, 1).outliers > 0,
        "sorting draws a different picture"
    );
}

/// GPU time of each method on bigger scenes at 1920x1080, with the engine's
/// GPU pass timer: the median of the frames timed after 8 warm-up frames.
/// Also the memory each method adds. Ignored: run explicitly. It measures
/// GPU timestamps only, which don't depend on the build's optimisation, so a
/// debug build gives the same numbers as a release one.
#[test]
#[ignore = "benchmark: run explicitly in release"]
fn oit_gpu_cost_benchmark() {
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
    let pixels = f64::from(BIG.0 * BIG.1);
    let methods = [Method::Sorted, Method::Weighted, Method::Peeled(4), Method::Peeled(8)];
    for method in methods {
        let mb = |samples: u32| f64::from(method.bytes_per_pixel() * samples) * pixels / 1e6;
        eprintln!(
            "oit memory {:<9} {:>2} B/px: {:6.1} MB at 1920x1080, {:6.1} MB with 4x MSAA",
            method.name(),
            method.bytes_per_pixel(),
            mb(1),
            mb(4)
        );
    }
    for scene in [
        intersecting_glass(),
        particles_through_glass(2000),
        layered_foliage(200),
    ] {
        let uploaded = rig.upload(&scene);
        for method in methods {
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
                rig.record(&mut encoder, &uploaded, method, Some(&timer));
                timer.resolve(&mut encoder);
                queue.submit([encoder.finish()]);
                timer.submitted();
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
            times.sort_by(f32::total_cmp);
            assert!(times.len() >= 30, "{} frames timed", times.len());
            eprintln!(
                "oit gpu {:<24} {:<9} median {:6.3} ms (min {:6.3}, max {:6.3}, {} frames, {} vertices)",
                scene.name,
                method.name(),
                times[times.len() / 2],
                times[0],
                times[times.len() - 1],
                times.len(),
                scene.vertices.len()
            );
        }
    }
}
