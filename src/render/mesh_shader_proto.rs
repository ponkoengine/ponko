//! A prototype of a geometry-heavy workload drawn two ways, outside the
//! renderer (issue #164): the way the engine draws instance batches (a
//! compute pass culls instances and picks distance levels, indexed indirect
//! draws draw what is left), and through a mesh-shader pipeline (a task
//! shader culls instances and meshlets, a mesh shader emits each meshlet's
//! triangles). Both draw the same spheres into the same offscreen target
//! with the same shading, so their GPU time, CPU time and memory compare.
//!
//! Only tests and the benchmark build it; it's no part of the renderer. See
//! `crates/ponko_bench/MESH_SHADERS.md` for what it measures and what it
//! leaves out (full transforms, materials, shadows, MSAA, skinning).
use super::render_test_support as support;
use crate::render::MeshData;
use glam::{Mat4, Vec3};
use std::time::Instant;
use wgpu::util::DeviceExt;

/// Vertices a meshlet holds at most (the size NVIDIA and AMD recommend; the
/// adapters that report a smaller output limit can't run it).
pub(super) const MESHLET_VERTICES: u32 = 64;
/// Triangles a meshlet holds at most.
pub(super) const MESHLET_TRIANGLES: u32 = 124;
/// Meshlet entries a task workgroup can hand on (8 bytes each): the payload's capacity.
const PAYLOAD_ENTRIES: u32 = 1024;
/// Distance levels the prototype draws (the engine's batches take any number).
const LEVELS: usize = 3;
/// Radius of the unit sphere every level approximates, in model space.
const SPHERE_RADIUS: f32 = 0.5;
/// Most instances a dispatch row holds before it wraps to the next one.
const ROW: u32 = 32768;

/// One meshlet: a cluster of up to [`MESHLET_VERTICES`] vertices and
/// [`MESHLET_TRIANGLES`] triangles, with the bounds the task shader culls by.
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(super) struct Meshlet {
    /// Where its vertex list starts in the meshlet vertex array.
    pub vertex_offset: u32,
    pub vertex_count: u32,
    /// Where its triangles start in the triangle array.
    pub triangle_offset: u32,
    pub triangle_count: u32,
    /// Bounding sphere (center, radius) in model space.
    pub sphere: [f32; 4],
    /// Normal cone: axis and the cosine-like cutoff (1: never culls).
    pub cone: [f32; 4],
}

/// A mesh cut into meshlets.
#[derive(Default)]
pub(super) struct Meshlets {
    pub table: Vec<Meshlet>,
    /// Every meshlet's vertices, as indices into the mesh's vertex array.
    pub vertices: Vec<u32>,
    /// Every meshlet's triangles: three 8-bit indices into its own vertex
    /// list, packed in one `u32` (`a | b << 8 | c << 16`).
    pub triangles: Vec<u32>,
}

impl Meshlets {
    pub fn triangle_total(&self) -> usize {
        self.table.iter().map(|m| m.triangle_count as usize).sum()
    }

    /// Triangles per meshlet on average: how full the greedy build packs
    /// them, against [`MESHLET_TRIANGLES`].
    pub fn average_triangles(&self) -> f64 {
        self.triangle_total() as f64 / self.table.len().max(1) as f64
    }

    /// The bits of every triangle's three corner positions, rebuilt from the
    /// meshlets alone, for checking they hold the mesh.
    pub fn corner_positions(&self, data: &MeshData) -> Vec<[u32; 9]> {
        let mut out = Vec::new();
        for m in &self.table {
            for t in 0..m.triangle_count {
                let packed = self.triangles[(m.triangle_offset + t) as usize];
                let mut c = [0u32; 9];
                for k in 0..3 {
                    let local = (packed >> (8 * k)) & 0xff;
                    let vertex = self.vertices[(m.vertex_offset + local) as usize];
                    for (a, v) in data.positions[vertex as usize].to_array().iter().enumerate() {
                        c[k as usize * 3 + a] = v.to_bits();
                    }
                }
                out.push(c);
            }
        }
        out
    }
}

/// Cuts `data` into meshlets by walking its triangles in index order: a
/// meshlet closes when the next triangle would pass a limit. That is as good
/// as the mesh's own triangle order (a generated sphere comes ring by ring);
/// a production importer would reorder for locality first, as
/// meshoptimizer's meshlet builder does.
pub(super) fn build_meshlets(data: &MeshData) -> Meshlets {
    let mut out = Meshlets::default();
    let mut local: Vec<u32> = Vec::new();
    let mut triangles: Vec<[u8; 3]> = Vec::new();
    let flush = |out: &mut Meshlets, local: &mut Vec<u32>, triangles: &mut Vec<[u8; 3]>| {
        if triangles.is_empty() {
            return;
        }
        let points: Vec<Vec3> = local.iter().map(|&i| data.positions[i as usize]).collect();
        let (lo, hi) = points
            .iter()
            .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
                (lo.min(*p), hi.max(*p))
            });
        let center = (lo + hi) / 2.0;
        let radius = points.iter().map(|p| p.distance(center)).fold(0.0, f32::max);
        // The normal cone from the outward normals of the corners.
        let face_normals: Vec<Vec3> = triangles
            .iter()
            .map(|t| {
                t.iter()
                    .map(|&k| data.normals[local[k as usize] as usize])
                    .sum::<Vec3>()
                    .normalize_or_zero()
            })
            .collect();
        let axis = face_normals.iter().copied().sum::<Vec3>().normalize_or_zero();
        let mindp = face_normals.iter().map(|n| n.dot(axis)).fold(1.0, f32::min);
        let cutoff = if axis == Vec3::ZERO || mindp < 0.1 {
            1.0
        } else {
            (1.0 - mindp * mindp).sqrt()
        };
        out.table.push(Meshlet {
            vertex_offset: out.vertices.len() as u32,
            vertex_count: local.len() as u32,
            triangle_offset: out.triangles.len() as u32,
            triangle_count: triangles.len() as u32,
            sphere: [center.x, center.y, center.z, radius],
            cone: [axis.x, axis.y, axis.z, cutoff],
        });
        out.vertices.extend(local.iter());
        out.triangles.extend(
            triangles
                .iter()
                .map(|t| u32::from(t[0]) | u32::from(t[1]) << 8 | u32::from(t[2]) << 16),
        );
        local.clear();
        triangles.clear();
    };
    for tri in data.indices.as_chunks::<3>().0 {
        let fresh = tri.iter().filter(|i| !local.contains(i)).count()
            - usize::from(tri[0] == tri[1] && !local.contains(&tri[0]))
            - usize::from(tri[1] == tri[2] && !local.contains(&tri[1]))
            - usize::from(tri[0] == tri[2] && !local.contains(&tri[0]));
        if local.len() + fresh > MESHLET_VERTICES as usize || triangles.len() + 1 > MESHLET_TRIANGLES as usize {
            flush(&mut out, &mut local, &mut triangles);
        }
        let mut corner = [0u8; 3];
        for (k, &i) in tri.iter().enumerate() {
            let at = local.iter().position(|&v| v == i).unwrap_or_else(|| {
                local.push(i);
                local.len() - 1
            });
            corner[k] = at as u8;
        }
        triangles.push(corner);
    }
    flush(&mut out, &mut local, &mut triangles);
    out
}

/// The workload: a field of spheres in rows, seen by a camera that flies
/// over it, each sphere in one of [`LEVELS`] distance levels.
#[derive(Clone, Debug)]
pub(super) struct Scene {
    pub size: (u32, u32),
    pub columns: u32,
    pub rows: u32,
    /// Metres between neighbouring spheres.
    pub spacing: f32,
    /// Segments of the nearest level's sphere; each further level has half
    /// the one before.
    pub lod0_segments: u32,
    /// Distances at which levels 1 and 2 begin; infinite for one level.
    pub bands: (f32, f32),
    /// Nothing is drawn past this distance.
    pub far: f32,
    /// Metres the camera moves each frame.
    pub speed: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            size: (1280, 720),
            columns: 256,
            rows: 256,
            spacing: 1.5,
            lod0_segments: 48,
            bands: (30.0, 70.0),
            far: 150.0,
            speed: 0.1,
        }
    }
}

impl Scene {
    pub fn instances(&self) -> u32 {
        self.columns * self.rows
    }

    /// Instance `i`: position and uniform scale.
    pub fn instance(&self, i: u32) -> [f32; 4] {
        let (column, row) = (i % self.columns, i / self.columns);
        let h = hash(i);
        let scale = 0.7 + (h & 0xff) as f32 / 255.0 * 0.6;
        [
            (column as f32 - self.columns as f32 / 2.0) * self.spacing,
            SPHERE_RADIUS * scale,
            -(row as f32) * self.spacing,
            scale,
        ]
    }

    /// The view-projection matrix and eye of `frame`.
    pub fn camera(&self, frame: u32) -> (Mat4, Vec3) {
        let eye = Vec3::new(0.0, 7.0, 12.0 - frame as f32 * self.speed);
        let forward = Vec3::new(0.0, -0.2, -1.0).normalize();
        let view = glam::camera::rh::view::look_to_mat4(eye, forward, Vec3::Y);
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        (
            glam::camera::rh::proj::directx::perspective(1.05, aspect, 0.3, self.far) * view,
            eye,
        )
    }

    /// The eye of `frame` and the direction it looks.
    pub fn eye_and_forward(&self, frame: u32) -> (Vec3, Vec3) {
        (self.camera(frame).1, Vec3::new(0.0, -0.2, -1.0).normalize())
    }

    /// Instances each level draws in `frame`, the way the culling shaders
    /// decide it (a bounding sphere against the six planes and the bands).
    pub fn expected_visible(&self, frame: u32) -> [u32; LEVELS] {
        let (view_proj, eye) = self.camera(frame);
        let planes = frustum_planes(view_proj);
        let mut counts = [0u32; LEVELS];
        for i in 0..self.instances() {
            let t = self.instance(i);
            let center = Vec3::new(t[0], t[1], t[2]);
            if let Some(level) = self.level_of(&planes, eye, center, SPHERE_RADIUS * t[3]) {
                counts[level] += 1;
            }
        }
        counts
    }

    fn level_of(&self, planes: &[[f32; 4]; 6], eye: Vec3, center: Vec3, radius: f32) -> Option<usize> {
        if planes
            .iter()
            .any(|p| p[0] * center.x + p[1] * center.y + p[2] * center.z + p[3] < -radius)
        {
            return None;
        }
        let distance = center.distance(eye);
        Some(if distance < self.bands.0 {
            0
        } else if distance < self.bands.1 {
            1
        } else {
            2
        })
    }

    /// The meshes of the levels, nearest first.
    fn meshes(&self) -> [MeshData; LEVELS] {
        let segments = |level: u32| (self.lod0_segments >> level).max(6);
        [0, 1, 2].map(|l| MeshData::sphere(SPHERE_RADIUS, segments(l)))
    }
}

fn hash(i: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9e37_79b1);
    h ^= h >> 15;
    h.wrapping_mul(0x85eb_ca6b) ^ (h >> 13)
}

/// The six planes of a frustum (inward normals, depth 0 to 1), normalized.
fn frustum_planes(m: Mat4) -> [[f32; 4]; 6] {
    let r = [m.row(0), m.row(1), m.row(2), m.row(3)];
    [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[2], r[3] - r[2]].map(|p| {
        let n = p.truncate().length();
        (p / n).to_array()
    })
}

/// How a frame is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Path {
    /// The engine's way: a compute pass culls instances and picks levels,
    /// one indexed indirect draw per level.
    Indirect,
    /// One instanced indexed draw of the nearest level, no culling.
    IndirectPlain,
    /// The engine's culling pass, then indirect mesh draws: a mesh shader
    /// per meshlet of every kept instance, with no task shader (it culls
    /// its own meshlet by frustum and normal cone before it emits).
    MeshIndirect,
    /// A task shader culls instances (frustum, level) and meshlets
    /// (frustum); a mesh shader emits the survivors.
    Mesh,
    /// [`Mesh`](Self::Mesh), and the task shader also drops meshlets that
    /// face away.
    MeshCone,
    /// Mesh shaders only: every meshlet of every instance, nearest level,
    /// no culling.
    MeshPlain,
}

impl Path {
    pub const ALL: [Path; 6] = [
        Path::IndirectPlain,
        Path::MeshPlain,
        Path::Indirect,
        Path::MeshIndirect,
        Path::Mesh,
        Path::MeshCone,
    ];

    /// Whether the path draws through a mesh pipeline.
    pub fn is_mesh(self) -> bool {
        matches!(self, Path::MeshIndirect | Path::Mesh | Path::MeshCone | Path::MeshPlain)
    }

    /// Whether the culling compute pass runs first.
    pub fn culls_on_compute(self) -> bool {
        matches!(self, Path::Indirect | Path::MeshIndirect)
    }

    pub fn name(self) -> &'static str {
        match self {
            Path::Indirect => "indirect (cull + levels)",
            Path::IndirectPlain => "indirect plain (one draw)",
            Path::MeshIndirect => "indirect mesh (cull + levels)",
            Path::Mesh => "mesh (task cull + levels)",
            Path::MeshCone => "mesh + cones",
            Path::MeshPlain => "mesh plain (no cull)",
        }
    }
}

/// What a batch of frames cost, per frame.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FrameTime {
    /// Recording and submitting, on the CPU.
    pub cpu_ms: f64,
    /// Submitting until the GPU is done, over the frames of the batch (the
    /// GPU's time per frame, with the submit latency shared out).
    pub gpu_ms: f64,
    /// The cull compute pass on the GPU, timed on the batch's first frame.
    pub cull_ms: Option<f64>,
}

/// Frames one batch draws at most (each has its own parameters).
pub(super) const BATCH_MAX: u32 = 16;

/// Bytes of the buffers each path keeps, the same geometry in each.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Memory {
    /// Vertices shared by both paths (position and normal).
    pub vertices: u64,
    pub instances: u64,
    /// Indexed path only: indices, visible lists, draw arguments.
    pub indexed_extra: u64,
    /// Mesh path only: meshlet table, vertex lists, packed triangles.
    pub mesh_extra: u64,
}

const COMMON: &str = r"
struct Params {
    view_proj: mat4x4<f32>,
    planes: array<vec4<f32>, 6>,
    camera: vec4<f32>,
    bands: vec4<f32>,
    counts: vec4<u32>,
    lods: array<vec4<u32>, 3>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> instances: array<vec4<f32>>;

const RADIUS = 0.5;

fn sphere_visible(center: vec3<f32>, radius: f32) -> bool {
    for (var p = 0u; p < 6u; p++) {
        let pl = params.planes[p];
        if dot(pl.xyz, center) + pl.w < -radius { return false; }
    }
    return true;
}

fn level_for(distance: f32) -> u32 {
    if distance < params.bands.x { return 0u; }
    if distance < params.bands.y { return 1u; }
    return 2u;
}

fn hash_color(i: u32) -> vec3<f32> {
    var h = i * 2654435761u;
    h = h ^ (h >> 15u);
    h = h * 2246822507u;
    return vec3<f32>(f32(h & 255u), f32((h >> 8u) & 255u), f32((h >> 16u) & 255u)) / 255.0 * 0.7 + 0.3;
}

fn shade(n: vec3<f32>, color: vec3<f32>) -> vec4<f32> {
    let l = max(dot(normalize(n), normalize(vec3<f32>(0.4, 0.8, 0.3))), 0.0);
    return vec4<f32>(color * (0.25 + 0.75 * l), 1.0);
}
";

const CULL: &str = r"
@group(0) @binding(2) var<storage, read_write> visible: array<u32>;
@group(0) @binding(3) var<storage, read_write> args: array<atomic<u32>>;

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.counts.x { return; }
    let t = instances[i];
    let radius = RADIUS * t.w;
    if !sphere_visible(t.xyz, radius) { return; }
    let level = level_for(distance(t.xyz, params.camera.xyz));
    let slot = atomicAdd(&args[level * 5u + 1u], 1u);
    visible[level * params.counts.x + slot] = i;
}
";

const INDEXED: &str = r"
@group(0) @binding(2) var<storage, read> visible: array<u32>;

struct VIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @builtin(instance_index) instance: u32,
}
struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
}

@vertex
fn vs(v: VIn) -> VOut {
    let i = visible[v.instance];
    let t = instances[i];
    var o: VOut;
    o.position = params.view_proj * vec4<f32>(t.xyz + v.position * t.w, 1.0);
    o.normal = v.normal;
    o.color = hash_color(i);
    return o;
}

@fragment
fn fs(v: VOut) -> @location(0) vec4<f32> {
    return shade(v.normal, v.color);
}
";

/// The mesh pipelines' shaders. `STATS` counts what the mesh stage emits.
fn mesh_source(stats: bool) -> String {
    format!("enable wgpu_mesh_shader;\nconst STATS = {stats};\n{COMMON}{MESH}",)
}

const MESH: &str = r"
struct Meshlet {
    vertex_offset: u32,
    vertex_count: u32,
    triangle_offset: u32,
    triangle_count: u32,
    sphere: vec4<f32>,
    cone: vec4<f32>,
}
@group(0) @binding(2) var<storage, read> meshlets: array<Meshlet>;
@group(0) @binding(3) var<storage, read> meshlet_vertices: array<u32>;
@group(0) @binding(4) var<storage, read> meshlet_triangles: array<u32>;
@group(0) @binding(5) var<storage, read> vertices: array<f32>;
@group(0) @binding(6) var<storage, read_write> stats: array<atomic<u32>>;
// One draw's slice of the instances: where it starts, how many it covers and
// how wide a row of the dispatch is.
struct Chunk { first: u32, count: u32, row: u32, pad: u32 }
@group(0) @binding(7) var<uniform> chunk: Chunk;

// What a task workgroup hands to its mesh workgroups: one entry per meshlet
// that survived, as (instance, meshlet).
struct Payload {
    entries: array<vec2<u32>, 1024>,
}
var<task_payload> payload: Payload;
var<workgroup> survivors: atomic<u32>;
var<workgroup> shown: array<u32, 32>;
@group(0) @binding(8) var<storage, read> visible: array<u32>;

fn meshlet_kept(t: vec4<f32>, ml: Meshlet) -> bool {
    let center = t.xyz + ml.sphere.xyz * t.w;
    let radius = ml.sphere.w * t.w;
    if !sphere_visible(center, radius) { return false; }
    if (params.counts.y & 1u) != 0u && ml.cone.w < 1.0 {
        let to = center - params.camera.xyz;
        let d = length(to);
        if dot(to / d, ml.cone.xyz) >= ml.cone.w + radius / d { return false; }
    }
    return true;
}

// One workgroup culls `counts.z` instances (a lane each), then all its lanes
// walk the meshlets of the instances that survived.
@task @payload(payload) @workgroup_size(32)
fn task_main(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) -> @builtin(mesh_task_size) vec3<u32> {
    let per = params.counts.z;
    let unit = group.x + group.y * chunk.row;
    let base = chunk.first + unit * per;
    var tag = 0u;
    if lane < per && unit * per + lane < chunk.count {
        let t = instances[base + lane];
        if sphere_visible(t.xyz, RADIUS * t.w) {
            tag = level_for(distance(t.xyz, params.camera.xyz)) + 1u;
        }
    }
    shown[lane] = tag;
    if lane == 0u { atomicStore(&survivors, 0u); }
    workgroupBarrier();
    for (var j = 0u; j < per; j++) {
        let level_tag = shown[j];
        if level_tag == 0u { continue; }
        let level = level_tag - 1u;
        let t = instances[base + j];
        let first = params.lods[level].x;
        let count = params.lods[level].y;
        for (var m = lane; m < count; m += 32u) {
            if meshlet_kept(t, meshlets[first + m]) {
                let slot = atomicAdd(&survivors, 1u);
                payload.entries[slot] = vec2<u32>(base + j, first + m);
            }
        }
    }
    workgroupBarrier();
    return vec3<u32>(atomicLoad(&survivors), 1u, 1u);
}

struct MeshVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
}
struct MeshPrimitive {
    @builtin(triangle_indices) indices: vec3<u32>,
}
struct MeshOutput {
    @builtin(vertices) vertices: array<MeshVertex, 64>,
    @builtin(primitives) primitives: array<MeshPrimitive, 124>,
    @builtin(vertex_count) vertex_count: u32,
    @builtin(primitive_count) primitive_count: u32,
}
var<workgroup> mesh_out: MeshOutput;

fn emit(instance: u32, meshlet: u32, lane: u32) {
    let ml = meshlets[meshlet];
    let t = instances[instance];
    let color = hash_color(instance);
    if lane == 0u {
        mesh_out.vertex_count = ml.vertex_count;
        mesh_out.primitive_count = ml.triangle_count;
        if STATS {
            atomicAdd(&stats[0], 1u);
            atomicAdd(&stats[1], ml.triangle_count);
        }
    }
    for (var v = lane; v < ml.vertex_count; v += 64u) {
        let g = meshlet_vertices[ml.vertex_offset + v] * 6u;
        let p = vec3<f32>(vertices[g], vertices[g + 1u], vertices[g + 2u]);
        mesh_out.vertices[v].position = params.view_proj * vec4<f32>(t.xyz + p * t.w, 1.0);
        mesh_out.vertices[v].normal = vec3<f32>(vertices[g + 3u], vertices[g + 4u], vertices[g + 5u]);
        mesh_out.vertices[v].color = color;
    }
    for (var k = lane; k < ml.triangle_count; k += 64u) {
        let packed = meshlet_triangles[ml.triangle_offset + k];
        mesh_out.primitives[k].indices = vec3<u32>(packed & 255u, (packed >> 8u) & 255u, (packed >> 16u) & 255u);
    }
}

@mesh(mesh_out) @payload(payload) @workgroup_size(64)
fn mesh_main(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let e = payload.entries[group.x];
    emit(e.x, e.y, lane);
}

@mesh(mesh_out) @workgroup_size(64)
fn mesh_plain(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let relative = group.y + group.z * chunk.row;
    if relative >= chunk.count {
        if lane == 0u {
            mesh_out.vertex_count = 0u;
            mesh_out.primitive_count = 0u;
        }
        return;
    }
    emit(chunk.first + relative, params.lods[0].x + group.x, lane);
}

// Without a task shader: the meshlets of the instances a compute pass kept
// (`chunk.first` is the level; the dispatch is meshlets by kept instances).
@mesh(mesh_out) @workgroup_size(64)
fn mesh_visible(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let level = chunk.first;
    let instance = visible[level * params.counts.x + group.y];
    let meshlet = params.lods[level].x + group.x;
    if !meshlet_kept(instances[instance], meshlets[meshlet]) {
        if lane == 0u {
            mesh_out.vertex_count = 0u;
            mesh_out.primitive_count = 0u;
        }
        return;
    }
    emit(instance, meshlet, lane);
}

struct FragIn {
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
}

@fragment
fn fs(v: FragIn) -> @location(0) vec4<f32> {
    return shade(v.normal, v.color);
}
";

const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The pipelines of one build: the indexed path's and the mesh path's.
struct Pipes {
    cull: wgpu::ComputePipeline,
    indexed: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    mesh_plain: wgpu::RenderPipeline,
    mesh_visible: wgpu::RenderPipeline,
    /// Milliseconds the shaders and pipelines took to build: the culling
    /// and indexed ones, and the three mesh ones.
    build_ms: (f64, f64),
}

/// What the mesh path needs of the device, as the adapter reports it.
#[derive(Clone, Copy, Debug)]
struct MeshLimits {
    task_per_dimension: u32,
    task_total: u32,
    mesh_per_dimension: u32,
    mesh_total: u32,
}

/// Everything one scene needs: the device, the geometry in both layouts, the
/// pipelines, a color and depth target.
pub(super) struct Rig {
    gpu: support::TestGpu,
    pub scene: Scene,
    limits: MeshLimits,
    /// Whether the mesh shaders are compiled with the checks that zero
    /// workgroup memory and bound every access (wgpu's default).
    checked: bool,
    pipes: Pipes,
    stats_pipes: Option<Pipes>,
    layouts: Layouts,
    color: wgpu::Texture,
    depth: wgpu::TextureView,
    params: wgpu::Buffer,
    stats: wgpu::Buffer,
    stats_read: wgpu::Buffer,
    instances: wgpu::Buffer,
    visible: wgpu::Buffer,
    /// Instance `i` as slot `i`: what the plain indexed draw reads (the
    /// plain path needs no visible list; it is here so both share a shader).
    identity: wgpu::Buffer,
    args: wgpu::Buffer,
    args_read: wgpu::Buffer,
    chunks: wgpu::Buffer,
    /// One slot of parameters per frame of a batch, and the draw arguments
    /// every frame starts from.
    params_all: wgpu::Buffer,
    args_template: wgpu::Buffer,
    /// Instances one task workgroup culls.
    per_task: u32,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    meshlets: wgpu::Buffer,
    meshlet_vertices: wgpu::Buffer,
    meshlet_triangles: wgpu::Buffer,
    /// Per level: index count, first index, base vertex.
    level_draw: [[u32; 3]; LEVELS],
    /// Per level: meshlet count and first meshlet.
    level_meshlets: [[u32; 2]; LEVELS],
    meshlet_count: u32,
    /// The set, its resolve and read buffers, and the right to hold the
    /// set (Metal allows 32 per process: see `query_sets.rs`).
    queries: Option<(wgpu::QuerySet, wgpu::Buffer, wgpu::Buffer, super::query_sets::Lease)>,
    memory: Memory,
}

struct Layouts {
    cull: wgpu::BindGroupLayout,
    indexed: wgpu::BindGroupLayout,
    mesh: wgpu::BindGroupLayout,
}

fn storage(binding: u32, visibility: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform(binding: u32, visibility: wgpu::ShaderStages, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

fn buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    })
}

fn pack<T: bytemuck::Pod>(data: &[T]) -> &[u8] {
    bytemuck::cast_slice(data)
}

impl Rig {
    /// A rig on this machine's adapter, or `None` (logged as a skip) when
    /// there is no adapter, it lacks mesh shaders, or it can't take the
    /// prototype's shaders. A forced tier below `desktop` (`PONKO_GPU_TIER`)
    /// and a software adapter are capability skips too: those tiers have no
    /// mesh shaders, and software rasterisers (lavapipe, WARP) that advertise them
    /// are known not to run naga's output reliably. Like every capability
    /// skip, these stay skips under `PONKO_REQUIRE_GPU`.
    pub fn new(scene: &Scene) -> Option<Self> {
        let mut limits_out = None;
        let gpu = support::gpu_with(|adapter| {
            if let Some(tier) = support::forced_tier().filter(|t| *t != crate::render::GpuTier::Desktop) {
                return Err(format!(
                    "the forced {tier} tier has no mesh shaders (its limits allow no task or mesh workgroups)"
                ));
            }
            if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
                return Err("a software adapter: the mesh-shader prototype runs only on a hardware GPU".to_owned());
            }
            let features = adapter.features();
            if !features.contains(wgpu::Features::EXPERIMENTAL_MESH_SHADER) {
                return Err("the adapter reports no mesh shaders (EXPERIMENTAL_MESH_SHADER)".to_owned());
            }
            if !features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE) {
                return Err("the indexed path needs INDIRECT_FIRST_INSTANCE".to_owned());
            }
            let a = adapter.limits();
            if a.max_mesh_output_vertices < MESHLET_VERTICES
                || a.max_mesh_output_primitives < MESHLET_TRIANGLES
                || a.max_mesh_invocations_per_workgroup < 64
                || a.max_task_invocations_per_workgroup < 32
                || a.max_task_payload_size < PAYLOAD_ENTRIES * 8
            {
                return Err(format!(
                    "the mesh limits are below the prototype's meshlets ({} vertices, {} primitives, {} + {} threads, \
                     {} B payload)",
                    a.max_mesh_output_vertices,
                    a.max_mesh_output_primitives,
                    a.max_mesh_invocations_per_workgroup,
                    a.max_task_invocations_per_workgroup,
                    a.max_task_payload_size
                ));
            }
            limits_out = Some(MeshLimits {
                task_per_dimension: a.max_task_workgroups_per_dimension,
                task_total: a.max_task_workgroup_total_count,
                mesh_per_dimension: a.max_mesh_workgroups_per_dimension,
                mesh_total: a.max_mesh_workgroup_total_count,
            });
            let mut required = wgpu::Features::EXPERIMENTAL_MESH_SHADER | wgpu::Features::INDIRECT_FIRST_INSTANCE;
            required |= features & wgpu::Features::TIMESTAMP_QUERY;
            let required_limits = wgpu::Limits {
                max_task_workgroup_total_count: a.max_task_workgroup_total_count,
                max_task_workgroups_per_dimension: a.max_task_workgroups_per_dimension,
                max_mesh_workgroup_total_count: a.max_mesh_workgroup_total_count,
                max_mesh_workgroups_per_dimension: a.max_mesh_workgroups_per_dimension,
                max_task_invocations_per_workgroup: a.max_task_invocations_per_workgroup,
                max_task_invocations_per_dimension: a.max_task_invocations_per_dimension,
                max_mesh_invocations_per_workgroup: a.max_mesh_invocations_per_workgroup,
                max_mesh_invocations_per_dimension: a.max_mesh_invocations_per_dimension,
                max_task_payload_size: a.max_task_payload_size,
                max_mesh_output_vertices: a.max_mesh_output_vertices,
                max_mesh_output_primitives: a.max_mesh_output_primitives,
                max_mesh_output_layers: a.max_mesh_output_layers,
                max_mesh_multiview_view_count: a.max_mesh_multiview_view_count,
                ..wgpu::Limits::default()
            };
            Ok(wgpu::DeviceDescriptor {
                label: Some("mesh shader prototype"),
                required_features: required,
                required_limits,
                // SAFETY: the prototype is a test and benchmark; experimental
                // mesh shaders may hit driver bugs, which a run reports.
                experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
                ..Default::default()
            })
        })?;
        let limits = limits_out?;
        let device = &gpu.device;

        // The geometry, once for each layout.
        let meshes = scene.meshes();
        let (mut vertices, mut indices) = (Vec::<f32>::new(), Vec::<u32>::new());
        let mut table = Vec::<Meshlet>::new();
        let (mut meshlet_vertices, mut meshlet_triangles) = (Vec::<u32>::new(), Vec::<u32>::new());
        let mut level_draw = [[0u32; 3]; LEVELS];
        let mut level_meshlets = [[0u32; 2]; LEVELS];
        for (level, data) in meshes.iter().enumerate() {
            let base_vertex = (vertices.len() / 6) as u32;
            level_draw[level] = [data.indices.len() as u32, indices.len() as u32, base_vertex];
            for (p, n) in data.positions.iter().zip(&data.normals) {
                vertices.extend([p.x, p.y, p.z, n.x, n.y, n.z]);
            }
            indices.extend(&data.indices);
            let built = build_meshlets(data);
            level_meshlets[level] = [built.table.len() as u32, table.len() as u32];
            for m in &built.table {
                table.push(Meshlet {
                    vertex_offset: m.vertex_offset + meshlet_vertices.len() as u32,
                    triangle_offset: m.triangle_offset + meshlet_triangles.len() as u32,
                    ..*m
                });
            }
            meshlet_vertices.extend(built.vertices.iter().map(|v| v + base_vertex));
            meshlet_triangles.extend(&built.triangles);
        }
        let n = scene.instances();
        let transforms: Vec<[f32; 4]> = (0..n).map(|i| scene.instance(i)).collect();
        let data = wgpu::BufferUsages::STORAGE;
        let init = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytes,
                usage,
            })
        };
        let vertex_buffer = init("vertices", pack(&vertices), wgpu::BufferUsages::VERTEX | data);
        let index_buffer = init("indices", pack(&indices), wgpu::BufferUsages::INDEX);
        let meshlet_buffer = init("meshlets", pack(&table), data);
        let meshlet_vertex_buffer = init("meshlet vertices", pack(&meshlet_vertices), data);
        let meshlet_triangle_buffer = init("meshlet triangles", pack(&meshlet_triangles), data);
        let instance_buffer = init("instances", pack(&transforms), data);
        let visible = buffer(
            device,
            "visible instances",
            u64::from(n) * LEVELS as u64 * 4,
            data | wgpu::BufferUsages::COPY_DST,
        );
        let identity = init("identity", pack(&(0..n).collect::<Vec<u32>>()), data);
        let args = buffer(
            device,
            "draw arguments",
            (LEVELS * 20) as u64,
            data | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        );
        let args_read = buffer(
            device,
            "draw arguments read",
            (LEVELS * 20) as u64,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        // One dynamic-offset slot per draw of a chunk (see `chunk_draws`).
        let chunks = buffer(
            device,
            "chunk starts",
            256 * 4096 + 16,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let stats = buffer(
            device,
            "mesh stage counters",
            8,
            data | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        );
        let stats_read = buffer(
            device,
            "mesh stage counters read",
            8,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        let params = buffer(
            device,
            "parameters",
            256,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let params_all = buffer(
            device,
            "parameters of a batch",
            256 * u64::from(BATCH_MAX),
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let args_template = buffer(
            device,
            "draw argument template",
            (LEVELS * 20) as u64,
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let most_meshlets = level_meshlets.iter().map(|[count, _]| *count).max().unwrap_or(1).max(1);
        let per_task = (PAYLOAD_ENTRIES / most_meshlets).clamp(1, 32);

        use wgpu::ShaderStages as S;
        let layouts = Layouts {
            cull: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cull"),
                entries: &[
                    uniform(0, S::COMPUTE, false),
                    storage(1, S::COMPUTE, true),
                    storage(2, S::COMPUTE, false),
                    storage(3, S::COMPUTE, false),
                ],
            }),
            indexed: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("indexed"),
                entries: &[
                    uniform(0, S::VERTEX, false),
                    storage(1, S::VERTEX, true),
                    storage(2, S::VERTEX, true),
                ],
            }),
            mesh: device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("mesh"),
                entries: &[
                    uniform(0, S::TASK | S::MESH, false),
                    storage(1, S::TASK | S::MESH, true),
                    storage(2, S::TASK | S::MESH, true),
                    storage(3, S::MESH, true),
                    storage(4, S::MESH, true),
                    storage(5, S::MESH, true),
                    storage(6, S::MESH, false),
                    uniform(7, S::TASK | S::MESH, true),
                    storage(8, S::MESH, true),
                ],
            }),
        };
        let size = scene.size;
        let color = support::target(device, size, COLOR);
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let lease = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| super::query_sets::lease(device, &gpu.queue, "prototype timer"))
            .flatten();
        let queries = lease.map(|lease| {
            (
                device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("prototype timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
                buffer(
                    device,
                    "timestamp resolve",
                    16,
                    wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                ),
                buffer(
                    device,
                    "timestamp read",
                    16,
                    wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                ),
                lease,
            )
        });
        let memory = Memory {
            vertices: vertex_buffer.size(),
            instances: instance_buffer.size(),
            indexed_extra: index_buffer.size() + visible.size() + args.size(),
            mesh_extra: meshlet_buffer.size() + meshlet_vertex_buffer.size() + meshlet_triangle_buffer.size(),
        };
        assert!(
            level_meshlets.iter().all(|[count, _]| *count <= PAYLOAD_ENTRIES),
            "a level has more meshlets than the task payload holds: {level_meshlets:?}"
        );
        let pipes = Self::pipes(device, &layouts, true, false)?;
        let rig = Self {
            gpu,
            scene: scene.clone(),
            limits,
            checked: true,
            pipes,
            stats_pipes: None,
            layouts,
            color,
            depth,
            params,
            params_all,
            args_template,
            per_task,
            stats,
            stats_read,
            instances: instance_buffer,
            visible,
            identity,
            args,
            args_read,
            chunks,
            vertices: vertex_buffer,
            indices: index_buffer,
            meshlets: meshlet_buffer,
            meshlet_vertices: meshlet_vertex_buffer,
            meshlet_triangles: meshlet_triangle_buffer,
            level_draw,
            level_meshlets,
            meshlet_count: table.len() as u32,
            queries,
            memory,
        };
        Some(rig)
    }

    /// Whether the device timestamps passes (the culling pass's time).
    pub fn timestamps(&self) -> bool {
        self.queries.is_some()
    }

    pub fn adapter(&self) -> String {
        support::describe_adapter(&self.gpu.info)
    }

    /// Milliseconds the shaders and pipelines took to build: the culling and
    /// indexed ones (2 pipelines), and the mesh ones (3 pipelines).
    pub fn build_ms(&self) -> (f64, f64) {
        self.pipes.build_ms
    }

    pub fn memory(&self) -> Memory {
        self.memory
    }

    pub fn meshlets(&self) -> u32 {
        self.meshlet_count
    }

    /// Meshlets of the nearest level.
    pub fn nearest_meshlets(&self) -> u32 {
        self.level_meshlets[0][0]
    }

    /// Triangles per meshlet on average, per level (nearest first).
    pub fn meshlet_fill(&self) -> [f64; LEVELS] {
        std::array::from_fn(|level| {
            f64::from(self.level_draw[level][0] / 3) / f64::from(self.level_meshlets[level][0].max(1))
        })
    }

    /// Compiles the shaders and builds the pipelines. `checked` keeps wgpu's
    /// runtime checks on the mesh shaders; off, they are built through
    /// `create_shader_module_trusted` without them (the mesh-shader docs
    /// recommend it, as the workgroup memory zeroing is costly). `None`
    /// (logged) when a pipeline can't be built.
    fn pipes(device: &wgpu::Device, layouts: &Layouts, checked: bool, stats: bool) -> Option<Pipes> {
        let source = |text: String| wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(text.into()),
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let indexed_started = Instant::now();
        let cull_module = device.create_shader_module(source(format!("{COMMON}{CULL}")));
        let indexed_module = device.create_shader_module(source(format!("{COMMON}{INDEXED}")));
        let indexed_modules_ms = indexed_started.elapsed().as_secs_f64() * 1e3;
        let mesh_started = Instant::now();
        let mesh_text = mesh_source(stats);
        let mesh_module = if checked {
            device.create_shader_module(source(mesh_text))
        } else {
            // SAFETY: the shader indexes its arrays within their bounds
            // (the checked build runs the same code in the tests); without
            // the checks nothing zeroes workgroup memory first, and the
            // shader writes every output before it reads it.
            unsafe { device.create_shader_module_trusted(source(mesh_text), wgpu::ShaderRuntimeChecks::unchecked()) }
        };
        let layout = |label: &str, group: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(group)],
                immediate_size: 0,
            })
        };
        let cull_layout = layout("cull", &layouts.cull);
        let indexed_layout = layout("indexed", &layouts.indexed);
        let mesh_layout = layout("mesh", &layouts.mesh);
        let mesh_module_ms = mesh_started.elapsed().as_secs_f64() * 1e3;
        let indexed_started = Instant::now();
        let cull = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cull"),
            layout: Some(&cull_layout),
            module: &cull_module,
            entry_point: Some("cull"),
            compilation_options: Default::default(),
            cache: None,
        });
        let depth = Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        });
        let primitive = wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        };
        let targets = [Some(COLOR.into())];
        let fragment = |module| wgpu::FragmentState {
            module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &targets,
        };
        let vertex_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let indexed = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("indexed"),
            layout: Some(&indexed_layout),
            vertex: wgpu::VertexState {
                module: &indexed_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attributes,
                })],
            },
            primitive,
            depth_stencil: depth.clone(),
            multisample: Default::default(),
            fragment: Some(fragment(&indexed_module)),
            multiview_mask: None,
            cache: None,
        });
        let indexed_ms = indexed_modules_ms + indexed_started.elapsed().as_secs_f64() * 1e3;
        let mesh_started = Instant::now();
        let mesh_pipeline = |label: &str, task: bool, mesh_entry: &str| {
            device.create_mesh_pipeline(&wgpu::MeshPipelineDescriptor {
                label: Some(label),
                layout: Some(&mesh_layout),
                task: task.then(|| wgpu::TaskState {
                    module: &mesh_module,
                    entry_point: Some("task_main"),
                    compilation_options: Default::default(),
                }),
                mesh: wgpu::MeshState {
                    module: &mesh_module,
                    entry_point: Some(mesh_entry),
                    compilation_options: Default::default(),
                },
                primitive,
                depth_stencil: depth.clone(),
                multisample: Default::default(),
                fragment: Some(fragment(&mesh_module)),
                multiview: None,
                cache: None,
            })
        };
        let mesh = mesh_pipeline("task and mesh", true, "mesh_main");
        let mesh_plain = mesh_pipeline("mesh only", false, "mesh_plain");
        let mesh_visible = mesh_pipeline("mesh only, kept instances", false, "mesh_visible");
        let mesh_ms = mesh_module_ms + mesh_started.elapsed().as_secs_f64() * 1e3;
        if let Some(error) = pollster::block_on(scope.pop()) {
            support::skip(format_args!("the prototype's pipelines don't build here: {error}"));
            return None;
        }
        Some(Pipes {
            cull,
            indexed,
            mesh,
            mesh_plain,
            mesh_visible,
            build_ms: (indexed_ms, mesh_ms),
        })
    }

    /// Rebuilds the mesh shaders with or without wgpu's runtime checks.
    pub fn set_checked(&mut self, checked: bool) {
        if self.checked != checked {
            self.pipes = Self::pipes(&self.gpu.device, &self.layouts, checked, false).expect("pipelines rebuild");
            self.stats_pipes = None;
            self.checked = checked;
        }
    }

    /// Instances one task workgroup culls (each of its 32 lanes takes one;
    /// at most as many as the payload holds meshlets for).
    pub fn instances_per_task(&self) -> u32 {
        self.per_task
    }

    pub fn set_instances_per_task(&mut self, per_task: u32) {
        assert!((1..=32).contains(&per_task), "1 to 32 instances per task workgroup");
        let most = self.level_meshlets.iter().map(|[count, _]| *count).max().unwrap_or(1);
        assert!(
            per_task * most <= PAYLOAD_ENTRIES,
            "{per_task} instances of {most} meshlets overfill the payload"
        );
        self.per_task = per_task;
    }

    fn params_bytes(&self, frame: u32, cone: bool) -> [u8; 256] {
        let (view_proj, eye) = self.scene.camera(frame);
        let mut words = [0u32; 64];
        let mut put = |at: usize, values: &[f32]| {
            for (k, v) in values.iter().enumerate() {
                words[at + k] = v.to_bits();
            }
        };
        put(0, &view_proj.to_cols_array());
        for (p, plane) in frustum_planes(view_proj).iter().enumerate() {
            put(16 + p * 4, plane);
        }
        put(40, &[eye.x, eye.y, eye.z, self.scene.far]);
        put(44, &[self.scene.bands.0, self.scene.bands.1, 0.0, 0.0]);
        words[48] = self.scene.instances();
        words[49] = u32::from(cone);
        words[50] = self.per_task;
        for (level, [count, first]) in self.level_meshlets.iter().enumerate() {
            words[52 + level * 4] = *first;
            words[53 + level * 4] = *count;
        }
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(bytemuck::cast_slice(&words));
        bytes
    }

    /// The draws a mesh path makes over the instances: chunks small enough
    /// for the dispatch limits, as `(first instance, instances, row width in
    /// units)`. A unit is `unit` instances (one workgroup column) and takes
    /// `groups_per_unit` workgroups.
    fn chunk_draws(&self, unit: u32, groups_per_unit: u32, per_dimension: u32, total: u32) -> Vec<(u32, u32, u32)> {
        let n = self.scene.instances();
        let row = ROW.min(per_dimension);
        let by_total = (u64::from(total) / u64::from(groups_per_unit.max(1))).max(1);
        let mut units = by_total
            .min(u64::from(row) * u64::from(per_dimension))
            .min(u64::from(n.div_ceil(unit)))
            .max(1) as u32;
        // A chunk past one row fills whole rows, so no padding group passes the total.
        if units > row {
            units = units / row * row;
        }
        let chunk = units * unit;
        (0..n)
            .step_by(chunk as usize)
            .map(|first| (first, chunk.min(n - first), row))
            .collect()
    }

    /// Whether the indirect mesh path's dispatches stay within the adapter's
    /// limits over `frames` (it can't split a dispatch the GPU sizes).
    pub fn indirect_mesh_fits(&self, frames: impl IntoIterator<Item = u32>) -> Result<(), String> {
        for frame in frames {
            for (level, kept) in self.scene.expected_visible(frame).iter().enumerate() {
                let meshlets = self.level_meshlets[level][0];
                if !dispatch_fits(self.limits, meshlets, *kept) {
                    return Err(format!(
                        "frame {frame}, level {level}: {kept} kept instances of {meshlets} meshlets pass the mesh \
                         dispatch limits ({} a dimension, {} in all)",
                        self.limits.mesh_per_dimension, self.limits.mesh_total
                    ));
                }
            }
        }
        Ok(())
    }

    /// Draws `frames` back to back on `path` (each with its own camera),
    /// optionally counting what the mesh stage emits (one frame only), and
    /// times the batch.
    fn run(&mut self, path: Path, frames: &[u32], stats: bool) -> FrameTime {
        assert!(!frames.is_empty() && frames.len() <= BATCH_MAX as usize);
        assert!(!stats || frames.len() == 1, "counters sum over a batch");
        if stats && self.stats_pipes.is_none() && path.is_mesh() {
            self.stats_pipes = Self::pipes(&self.gpu.device, &self.layouts, self.checked, true);
        }
        let started = Instant::now();
        let device = self.gpu.device.clone();
        let queue = self.gpu.queue.clone();
        let cone = matches!(path, Path::MeshCone | Path::MeshIndirect);
        for (k, frame) in frames.iter().enumerate() {
            queue.write_buffer(&self.params_all, k as u64 * 256, &self.params_bytes(*frame, cone));
        }
        let n = self.scene.instances();
        let mut draws = Vec::new();
        match path {
            Path::Mesh | Path::MeshCone | Path::MeshPlain => {
                let (unit, groups, per_dimension, total) = if path == Path::MeshPlain {
                    (
                        1,
                        self.nearest_meshlets(),
                        self.limits.mesh_per_dimension,
                        self.limits.mesh_total,
                    )
                } else {
                    (self.per_task, 1, self.limits.task_per_dimension, self.limits.task_total)
                };
                draws = self.chunk_draws(unit, groups, per_dimension, total);
                assert!(draws.len() <= 4096, "{} draws", draws.len());
                for (k, (first, count, row)) in draws.iter().enumerate() {
                    queue.write_buffer(&self.chunks, k as u64 * 256, pack(&[*first, *count, *row, 0]));
                }
            }
            Path::MeshIndirect => {
                for level in 0..LEVELS {
                    queue.write_buffer(&self.chunks, level as u64 * 256, pack(&[level as u32, 0, 0, 0]));
                }
                let mut template = [0u32; LEVELS * 5];
                for (level, [meshlets, _]) in self.level_meshlets.iter().enumerate() {
                    template[level * 5..level * 5 + 3].copy_from_slice(&[*meshlets, 0, 1]);
                }
                queue.write_buffer(&self.args_template, 0, pack(&template));
            }
            Path::Indirect | Path::IndirectPlain => {
                // One draw per level (the plain path has only the nearest).
                let mut template = [0u32; LEVELS * 5];
                for (level, [index_count, first_index, base_vertex]) in self.level_draw.iter().enumerate() {
                    let plain = path == Path::IndirectPlain;
                    template[level * 5..level * 5 + 5].copy_from_slice(&[
                        *index_count,
                        if plain && level == 0 { n } else { 0 },
                        *first_index,
                        *base_vertex,
                        level as u32 * n,
                    ]);
                }
                queue.write_buffer(&self.args_template, 0, pack(&template));
            }
        }
        if stats {
            queue.write_buffer(&self.stats, 0, &[0u8; 8]);
        }
        let bind = |layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry<'_>]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries,
            })
        };
        let cull_group = bind(
            &self.layouts.cull,
            &[
                entry(0, &self.params),
                entry(1, &self.instances),
                entry(2, &self.visible),
                entry(3, &self.args),
            ],
        );
        let mesh_group = bind(
            &self.layouts.mesh,
            &[
                entry(0, &self.params),
                entry(1, &self.instances),
                entry(2, &self.meshlets),
                entry(3, &self.meshlet_vertices),
                entry(4, &self.meshlet_triangles),
                entry(5, &self.vertices),
                entry(6, &self.stats),
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.chunks,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                },
                entry(8, &self.visible),
            ],
        );
        let indexed_group = bind(
            &self.layouts.indexed,
            &[
                entry(0, &self.params),
                entry(1, &self.instances),
                entry(
                    2,
                    if path == Path::IndirectPlain {
                        &self.identity
                    } else {
                        &self.visible
                    },
                ),
            ],
        );
        let pipes = if stats {
            self.stats_pipes.as_ref().unwrap_or(&self.pipes)
        } else {
            &self.pipes
        };
        let color = self.color.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        for k in 0..frames.len() as u64 {
            encoder.copy_buffer_to_buffer(&self.params_all, k * 256, &self.params, 0, 256);
            if path.culls_on_compute() || matches!(path, Path::IndirectPlain) {
                encoder.copy_buffer_to_buffer(&self.args_template, 0, &self.args, 0, (LEVELS * 20) as u64);
            }
            if path.culls_on_compute() {
                let stamps = self.queries.as_ref().filter(|_| k == 0).map(|(set, ..)| set);
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("cull"),
                    timestamp_writes: stamps.map(|set| wgpu::ComputePassTimestampWrites {
                        query_set: set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                });
                pass.set_pipeline(&self.pipes.cull);
                pass.set_bind_group(0, &cull_group, &[]);
                pass.dispatch_workgroups(n.div_ceil(64), 1, 1);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            match path {
                Path::Mesh | Path::MeshCone => {
                    pass.set_pipeline(&pipes.mesh);
                    for (d, (_, count, row)) in draws.iter().enumerate() {
                        let units = count.div_ceil(self.per_task);
                        pass.set_bind_group(0, &mesh_group, &[d as u32 * 256]);
                        pass.draw_mesh_tasks(units.min(*row), units.div_ceil(*row), 1);
                    }
                }
                Path::MeshPlain => {
                    pass.set_pipeline(&pipes.mesh_plain);
                    for (d, (_, count, row)) in draws.iter().enumerate() {
                        pass.set_bind_group(0, &mesh_group, &[d as u32 * 256]);
                        pass.draw_mesh_tasks(self.nearest_meshlets(), (*count).min(*row), count.div_ceil(*row));
                    }
                }
                Path::MeshIndirect => {
                    pass.set_pipeline(&pipes.mesh_visible);
                    for level in 0..LEVELS {
                        pass.set_bind_group(0, &mesh_group, &[level as u32 * 256]);
                        pass.draw_mesh_tasks_indirect(&self.args, level as u64 * 20);
                    }
                }
                Path::Indirect | Path::IndirectPlain => {
                    pass.set_pipeline(&self.pipes.indexed);
                    pass.set_bind_group(0, &indexed_group, &[]);
                    pass.set_vertex_buffer(0, self.vertices.slice(..));
                    pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                    let levels = if path == Path::IndirectPlain { 1 } else { LEVELS };
                    for level in 0..levels {
                        pass.draw_indexed_indirect(&self.args, level as u64 * 20);
                    }
                }
            }
        }
        if let Some((set, resolve, read, _)) = &self.queries {
            // A trailing blit: Metal writes an encoder's end sample when the
            // next one starts, so the resolve sees it.
            encoder.clear_buffer(&self.args_read, 0, Some(16));
            encoder.resolve_query_set(set, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, read, 0, 16);
        }
        if stats {
            encoder.copy_buffer_to_buffer(&self.stats, 0, &self.stats_read, 0, 8);
        }
        queue.submit([encoder.finish()]);
        let cpu_ms = started.elapsed().as_secs_f64() * 1e3 / frames.len() as f64;
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let gpu_ms = started.elapsed().as_secs_f64() * 1e3 / frames.len() as f64;
        let mut cull_ms = None;
        if let (Some((_, _, read, _)), true) = (&self.queries, path.culls_on_compute()) {
            let ticks = read_u64(&device, read, 2);
            let period = f64::from(queue.get_timestamp_period());
            cull_ms = Some(ticks[1].saturating_sub(ticks[0]) as f64 * period / 1e6);
        }
        FrameTime {
            cpu_ms,
            gpu_ms,
            cull_ms,
        }
    }

    /// Draws `count` frames from `first` on `path` back to back and times
    /// them, per frame.
    pub fn batch(&mut self, path: Path, first: u32, count: u32) -> FrameTime {
        let frames: Vec<u32> = (first..first + count).collect();
        self.run(path, &frames, false)
    }

    /// The color target after drawing `frame` on `path`.
    pub fn image(&mut self, path: Path, frame: u32) -> support::Image {
        self.run(path, &[frame], false);
        support::Image::read(&self.gpu.device, &self.gpu.queue, &self.color)
    }

    /// Instances per level the culling pass kept in `frame`.
    pub fn indirect_instances(&mut self, frame: u32) -> [u32; LEVELS] {
        self.run(Path::Indirect, &[frame], false);
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.args, 0, &self.args_read, 0, (LEVELS * 20) as u64);
        self.gpu.queue.submit([encoder.finish()]);
        let words = read_u32(&self.gpu.device, &self.args_read, LEVELS * 5);
        [words[1], words[6], words[11]]
    }

    /// Triangles the mesh stage emitted in `frame` on a mesh path.
    pub fn mesh_triangles(&mut self, path: Path, frame: u32) -> u64 {
        self.run(path, &[frame], true);
        let words = read_u32(&self.gpu.device, &self.stats_read, 2);
        u64::from(words[1])
    }

    /// Triangles the indexed path drew in `frame`: the kept instances of
    /// each level times its triangles (plain: every instance, nearest level).
    pub fn indexed_triangles(&mut self, path: Path, frame: u32) -> u64 {
        if path == Path::IndirectPlain {
            return u64::from(self.scene.instances()) * u64::from(self.level_draw[0][0] / 3);
        }
        let kept = self.indirect_instances(frame);
        kept.iter()
            .zip(&self.level_draw)
            .map(|(n, d)| u64::from(*n) * u64::from(d[0] / 3))
            .sum()
    }
}

/// Whether a mesh dispatch of `meshlets` by `kept` workgroups is within the limits.
fn dispatch_fits(limits: MeshLimits, meshlets: u32, kept: u32) -> bool {
    kept <= limits.mesh_per_dimension
        && meshlets <= limits.mesh_per_dimension
        && u64::from(kept) * u64::from(meshlets) <= u64::from(limits.mesh_total)
}

/// [`dispatch_fits`] for the limits of Apple GPUs (the family this was written on).
#[cfg(test)]
pub(super) fn dispatch_fits_apple9(meshlets: u32, kept: u32) -> bool {
    dispatch_fits(
        MeshLimits {
            task_per_dimension: u32::MAX,
            task_total: u32::MAX,
            mesh_per_dimension: 1 << 20,
            mesh_total: 1 << 20,
        },
        meshlets,
        kept,
    )
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn read_u32(device: &wgpu::Device, buffer: &wgpu::Buffer, count: usize) -> Vec<u32> {
    buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let words = bytemuck::cast_slice::<u8, u32>(&buffer.get_mapped_range(..).unwrap()[..count * 4]).to_vec();
    buffer.unmap();
    words
}

fn read_u64(device: &wgpu::Device, buffer: &wgpu::Buffer, count: usize) -> Vec<u64> {
    buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let words = bytemuck::cast_slice::<u8, u64>(&buffer.get_mapped_range(..).unwrap()[..count * 8]).to_vec();
    buffer.unmap();
    words
}
