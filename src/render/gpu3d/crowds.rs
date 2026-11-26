//! Crowds of skinned characters (`Graphics::create_crowd`, see
//! `three/crowd.rs`): the renderer's side.
//!
//! Two paths, picked by the device:
//!
//! - **GPU** (compute culling: compute shaders and four storage buffers a
//!   stage). The records live in a storage buffer (two, so the motion
//!   vectors have the last frame's), the baked animation in another. Each
//!   frame one compute dispatch tests every character against every view
//!   and appends it to the visible list of each (view, level) that draws
//!   it, and a second writes the counts into the indirect draws
//!   (`crowd_cull.wgsl`). Each (view, level, mesh part) is then one indirect
//!   draw, whose vertex shader skins every corner from the bake, between
//!   the clip's two nearest frames and blended with a second clip
//!   (`crowd_vertex.wgsl`), before the material's own `vertex` runs. A
//!   material's crowd pipelines are built the first time a crowd draws it.
//! - **CPU** (WebGL2: no compute shaders, no storage buffers, and every
//!   texture slot of the mesh shaders already taken, so no vertex shader can
//!   read the bake). Each level's mesh is posed on the CPU once per baked
//!   frame when the crowd is made, into one vertex buffer; each frame the
//!   characters are culled on the CPU, grouped by (view, level, the baked
//!   frame nearest their time) and drawn with the material's plain
//!   pipelines, the vertex buffer bound at their frame. No blending between
//!   frames or clips (a character shows the clip it shows more of), and no
//!   motion vectors (WebGL2 has no temporal antialiasing).
//!
//! Both paths cull against every view the frame draws: the camera, the sun's
//! cascades, light shadow views, target cameras, global illumination's
//! views (its sun shadow map and its six face directions, keeping only the
//! characters at least a pixel wide from a probe updated this frame), and a
//! window view's camera when that view is drawn. The camera's view also
//! leaves out what the occlusion grid hides, when occlusion culling is on.
//!
//! Time: each crowd keeps when its records were first drawn (its epoch) in
//! 64-bit seconds, and the shaders get the 32-bit seconds since then. So
//! that never grows large, every [`REBASE_SECONDS`] the records' clip times
//! are moved on to the frame's time and the epoch with them (a compute
//! dispatch, `crowd_advance.wgsl`, on the GPU path).
//!
//! A game without crowds makes none of this: no buffers, pipelines or passes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use glam::{Mat4, Vec3, Vec4};

use super::instances::intersects;
use super::pipeline_cache::TargetKey;
use super::record::Kind;
use super::shaders::{Pipelines, ShaderPipelines, compose_material};
use super::skinning::GpuSkin;
use super::{GpuLimits, Instance3D, Instancing, MAX_CASCADES, Renderer3D, ShaderVariant, Sides, Vertex};
use crate::render::traffic::{self, Counted, NewBuffer, Upload};
use crate::three::crowd::{CrowdData, CrowdRecord, CullBox, MAX_CROWD_LEVELS, top_influences};

/// Joints (`Uint16x4`) and weights (`Float32x4`) of each corner, from the
/// skin's influence buffer.
const CROWD_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![10 => Uint16x4, 11 => Float32x4];

/// The third vertex buffer of a crowd's pipelines (see [`GpuSkin::influences`]).
pub(super) const INFLUENCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: 24,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &CROWD_ATTRIBUTES,
};

const CULL: &str = include_str!("crowd_cull.wgsl");
const ADVANCE: &str = include_str!("crowd_advance.wgsl");
const VERTEX: &str = include_str!("crowd_vertex.wgsl");

/// Bytes of one indirect indexed draw.
const ARGS: u64 = 20;
/// Bytes of the cull pass's uniform (see `Params` in `crowd_cull.wgsl`):
/// 16 words, each level's band, each level's kinds.
const PARAMS: u64 = 64 + 16 * MAX_CROWD_LEVELS as u64 + 4 * MAX_CROWD_LEVELS as u64;
/// Steps of a crossfade's share of pixels kept: the 9 bits a visible
/// list's entry has for it above the character's index (22 bits, see
/// [`MAX_CROWD_INSTANCES`](crate::three::crowd::MAX_CROWD_INSTANCES)), with a 10th for its sign.
const FADE_STEPS: f32 = 511.0;
/// Where the advance pass's uniform (`Advance` in `crowd_advance.wgsl`)
/// starts in the params buffer: past the cull's, at the offset alignment
/// every device allows.
const ADVANCE_AT: u64 = 256;
/// Seconds of game time between two moves of a crowd's clock (see the
/// module's docs): the 32-bit seconds the shaders add stay below this
/// (plus a frame), precise to about 2 microseconds.
pub(super) const REBASE_SECONDS: f64 = 16.0;
/// Room for the probes global illumination updates in a frame, after the
/// views (see `Params::probes` in `crowd_cull.wgsl`).
const PROBES: u64 = crate::three::gi::MAX_GI_PROBES_PER_FRAME as u64;
/// The most bytes the depth grid's uniform takes (a uniform binding's most
/// on most devices).
const GRID_BYTES: u64 = 64 << 10;

/// The cull shader for a depth grid uniform of `bytes` bytes.
fn cull_source(bytes: u64) -> String {
    CULL.replace("GRID_VEC4S", &(bytes / 16 - 5).to_string())
}

/// How `level` draws a character whose origin is `distance` from a view's
/// eye: `None` not at all, `Some(0.0)` whole, else dithered with that
/// share of its pixels (negative: the level fading out, keeping the pixels
/// the one fading in leaves; see `Instance3D::with_fade`), in steps of
/// 1/[`FADE_STEPS`]. A level fades out over the last `fade_out` of its
/// band, and in over the `fade_in` before it (the fade of the level that
/// ends where it starts). The cull pass does the same (`crowd_cull.wgsl`).
fn level_fade(level: &Level, distance: f32) -> Option<f32> {
    let keep = if distance >= level.min && distance < level.max {
        let start = level.max - level.fade_out;
        if level.fade_out > 0.0 && distance >= start {
            -(1.0 - (distance - start) / level.fade_out)
        } else {
            return Some(0.0);
        }
    } else if level.fade_in > 0.0 && distance < level.min && distance >= level.min - level.fade_in {
        (distance - (level.min - level.fade_in)) / level.fade_in
    } else {
        return None;
    };
    let steps = (keep.abs() * FADE_STEPS + 0.5).floor();
    if steps <= 0.0 {
        None
    } else if steps >= FADE_STEPS {
        Some(0.0)
    } else {
        Some(steps.copysign(keep) / FADE_STEPS)
    }
}
/// What the CPU path's posed frames may take per crowd: more frames than
/// fit are thinned out (every second one, every third...).
const POSED_BYTES: u64 = 64 << 20;

/// The crowd's material shader: `compose_material`'s, with each corner's
/// joints and weights in `VertexIn` and the crowd's `static_instance` in
/// place of the batches' (so `world_vertex` sees the posed corner).
pub(super) fn compose_crowd(user: &str, decals: bool, gi: bool) -> String {
    const FIELDS: &str = "    @location(9) uv1: vec2<f32>,\n};";
    const BINDINGS: &str = "@group(3) @binding(1) var<storage, read> static_indices: array<u32>;\n";
    const FADE: &str = "    out.fade = in.m1.w;\n";
    let source = compose_material(user, decals, gi);
    let helper = include_str!("instance_vertex.wgsl");
    // Invariants: the mesh shader's own text (checked by
    // `crowd_shaders_compose_on_the_mesh_shader`).
    debug_assert!(
        source.contains(FIELDS)
            && source.contains(BINDINGS)
            && source.contains(helper)
            && source.matches(FADE).count() == 2
    );
    source
        .replacen(
            FIELDS,
            "    @location(9) uv1: vec2<f32>,\n    @location(10) crowd_joints: vec4<u32>,\n    @location(11) crowd_weights: vec4<f32>,\n};",
            1,
        )
        .replacen(BINDINGS, &format!("{BINDINGS}{VERTEX}"), 1)
        .replacen(helper, "", 1)
        // The main and shadow vertex entry points take the dither from the
        // character's list entry (see `crowd_fade`).
        .replace(FADE, "    out.fade = crowd_fade(in);\n")
}

/// One level of a crowd, as the renderer draws it.
#[derive(Clone, Copy)]
struct Level {
    mesh: u32,
    min: f32,
    max: f32,
    camera: bool,
    shadows: bool,
    influences: u32,
    section: u32,
    slots: u32,
    /// Distance over which it fades out before `max`, and in before `min`
    /// (see [`level_fade`]).
    fade_out: f32,
    fade_in: f32,
}

/// What a frame's upload and culling read of a crowd besides its levels.
#[derive(Clone, Copy)]
struct Frame {
    count: u32,
    clips: u32,
    /// Seconds its clips played on since its records were first drawn,
    /// and as of the last frame for the last frame's records.
    elapsed: f32,
    past_elapsed: f32,
}

/// Which of the frame's view slots are drawn: the camera, the cascades and
/// light views drawn this frame, the target cameras, global
/// illumination's views.
#[derive(Clone, Copy)]
struct Drawn {
    cascades: usize,
    light_views: usize,
    /// The first target camera's slot.
    targets: usize,
    /// Global illumination's sun shadow slot, then its six face
    /// directions' (`None`: no probe updates this frame).
    gi: Option<usize>,
}

impl Drawn {
    fn slot(self, s: usize) -> bool {
        match s {
            0 => true,
            s if s >= self.targets => true,
            s if s <= MAX_CASCADES => s - 1 < self.cascades,
            s => s - 1 - MAX_CASCADES < self.light_views,
        }
    }

    fn shadow(self, s: usize) -> bool {
        (1..self.targets).contains(&s) || self.gi == Some(s)
    }

    /// Global illumination's face direction slots (empty without updates).
    fn gi_faces(self) -> std::ops::Range<usize> {
        self.gi.map_or(0..0, |gi| gi + 1..gi + crate::three::gi::GI_BATCH_FACES)
    }
}

/// What the camera's view culls against besides its frustum, and global
/// illumination's face views besides theirs.
#[derive(Clone, Copy, Default)]
struct Extra<'a> {
    /// The depth grid (occlusion culling on) and where the camera is.
    grid: Option<(&'a crate::three::occlusion::OcclusionGrid, Vec3)>,
    /// The probes global illumination updates this frame.
    probes: &'a [Vec3],
}

/// A crowd on the renderer's side.
struct GpuCrowd {
    levels: Vec<Level>,
    casts: bool,
    /// Whether a level crossfades (the GPU path then draws each level's
    /// dithered list too).
    fades: bool,
    clips: u32,
    count: u32,
    min: Vec3,
    max: Vec3,
    /// The bake it was made from (a new bake makes the crowd anew).
    animation: Arc<[[f32; 4]]>,
    /// When its records were first drawn (`None`: not yet), and the ones
    /// the last frame drew, in game seconds; whether an update waits for
    /// the next frame.
    epoch: Option<f64>,
    past_epoch: f64,
    updated: bool,
    /// Seconds since the epoch at the main frame and at the last one (what
    /// window views draw too).
    elapsed: f32,
    past_elapsed: f32,
    /// Whether this frame draws records newer than the last frame's.
    moved: bool,
    path: Path,
}

enum Path {
    Gpu(Box<GpuPath>),
    Cpu(Box<CpuPath>),
}

/// What the GPU path keeps (see the module's docs).
struct GpuPath {
    /// The records, twice: the frame's (`current`) and the last ones.
    records: [wgpu::Buffer; 2],
    capacity: u32,
    current: usize,
    anim: wgpu::Buffer,
    params: wgpu::Buffer,
    /// Per level, at the uniform alignment: its `CrowdLevel` (see
    /// `crowd_vertex.wgsl`) for its whole characters, then every level's
    /// for its dithered ones.
    levels: wgpu::Buffer,
    /// Views, then the level of each indirect draw, room for the probes,
    /// then the clips' frame ranges' boxes (from `boxes_at`, in vec4s).
    views: wgpu::Buffer,
    /// The bake's cull table (`CrowdData::cull_table`): per clip its
    /// timing and first range, then per frame range its model-space box.
    clip_boxes: Vec<[f32; 4]>,
    boxes_at: u32,
    /// Per (view, level) a list: a leading 0, the whole characters from
    /// the front, the dithered ones from the back.
    visible: wgpu::Buffer,
    /// Per (view, level) two counts (whole, dithered), then the indirect
    /// draws: per view, every level's parts whole, then dithered.
    draws: wgpu::Buffer,
    /// Views the buffers have room for.
    view_capacity: u32,
    /// Entries of one (view, level) list.
    region: u32,
    /// Indirect draws per view, and the first of each level.
    parts: u32,
    part_base: Vec<u32>,
    /// The cull bind group, per current records buffer.
    cull: [wgpu::BindGroup; 2],
    /// The advance bind group, per current records buffer.
    advance: [wgpu::BindGroup; 2],
    /// A clock move this frame's cull pass makes, from the epoch it
    /// replaced (see the module's docs), and whether a pass recorded it: a
    /// frame prepared but never drawn moved nothing, so the next one puts
    /// the epoch back.
    pending: Option<f64>,
    advanced: AtomicBool,
    /// Seconds that move covers.
    seconds: f32,
    /// The draw bind group, per current buffer and whether the last frame's
    /// records are the other one.
    draw: [[wgpu::BindGroup; 2]; 2],
    /// Views culled this frame (0: none drawn).
    culled: u32,
    /// Per-frame upload scratch.
    scratch: Vec<u8>,
}

/// What the CPU path keeps (see the module's docs).
struct CpuPath {
    records: Vec<CrowdRecord>,
    /// This frame's culling boxes, per character: the box of the frame
    /// range of the frame it shows.
    boxes: Vec<CullBox>,
    /// Per clip where its frame ranges' boxes start in `range_bounds`, and
    /// those boxes (`CrowdData::range_bounds`).
    range_first: Vec<u32>,
    range_bounds: Vec<(Vec3, Vec3)>,
    /// The clip table as baked (first frame, frames, fps, looping).
    table: Vec<[f32; 4]>,
    /// Per clip: its first frame among the posed ones, how many were posed,
    /// and every how many baked frames one was.
    clips: Vec<(u32, u32, f32, bool, u32)>,
    step: u32,
    /// Per level: its posed frames, frame after frame (`None`: its mesh had
    /// no corners on the CPU), and corners a frame.
    posed: Vec<Option<(wgpu::Buffer, u32)>>,
    /// This frame's characters, grouped as `runs` says.
    instances: Option<wgpu::Buffer>,
    instance_capacity: u32,
    /// (view, level, posed frame, first instance, count, dithered), by view.
    runs: Vec<(u32, u32, u32, u32, u32, bool)>,
    /// Per view: its range of `runs`.
    view_runs: Vec<(u32, u32)>,
    /// (view, level, dithered, frame) as a key, the character, its dither.
    keys: Vec<(u64, u32, f32)>,
    frames: Vec<u32>,
    out: Vec<Instance3D>,
}

/// What every crowd on the GPU path shares, made with the first one.
struct Shared {
    cull_layout: wgpu::BindGroupLayout,
    cull: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    advance_layout: wgpu::BindGroupLayout,
    advance: wgpu::ComputePipeline,
    /// The depth grid the camera's views cull against (one for every
    /// crowd), what it holds, its size and scratch to fill it.
    grid: wgpu::Buffer,
    grid_mirror: traffic::Mirror,
    grid_bytes: u64,
    grid_scratch: Vec<f32>,
    /// Whether this frame's grid is on.
    grid_on: bool,
    draw_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    shadow_layout: wgpu::PipelineLayout,
    /// A zero-stride instance buffer for vertex slot 1 (the records come
    /// from storage).
    marker: wgpu::Buffer,
    /// Storage and uniform offset alignments, in bytes.
    storage_align: u32,
    uniform_align: u32,
    max_binding: u64,
    max_groups: u32,
}

/// One material shader's crowd pipelines.
struct CrowdSet {
    set: Weak<ShaderPipelines>,
    sides: Sides,
    /// The crossfade (dithering) variant.
    fade: bool,
    targets: TargetKey,
    /// `None`: the crowd module didn't build; its crowds draw nothing.
    pipelines: Option<Pipelines>,
}

/// The renderer's crowds (see the module's docs).
pub(super) struct Crowds {
    /// The shadow passes' frame layout (the renderer keeps no other copy).
    shadow_frame: wgpu::BindGroupLayout,
    crowds: Vec<Option<GpuCrowd>>,
    shared: Option<Shared>,
    sets: Vec<CrowdSet>,
    /// This frame: the shadow views a crowd draws into.
    shadow_mask: u32,
    /// The last prepared frame's time.
    last_now: Option<f64>,
    /// Limits already reported, so each is said once.
    reported: bool,
    /// This frame's probe updates (global illumination), for the cut.
    probes: Vec<Vec3>,
}

impl Crowds {
    pub(super) fn new(shadow_frame: wgpu::BindGroupLayout) -> Self {
        Self {
            shadow_frame,
            crowds: Vec::new(),
            shared: None,
            sets: Vec::new(),
            shadow_mask: 0,
            last_now: None,
            reported: false,
            probes: Vec::new(),
        }
    }

    /// Whether a frame draws any crowd.
    pub(super) fn has_draws(&self) -> bool {
        self.crowds.iter().flatten().any(|c| c.count > 0)
    }

    /// The meshes of the camera levels of every crowd with characters
    /// (culled or not: the refraction copy is decided before culling).
    pub(super) fn camera_meshes(&self) -> impl Iterator<Item = u32> + '_ {
        self.crowds
            .iter()
            .flatten()
            .filter(|c| c.count > 0)
            .flat_map(|c| c.levels.iter().filter(|l| l.camera).map(|l| l.mesh))
    }

    /// Whether the frame's cull pass runs (see `Renderer3D::cull_crowds`).
    pub(super) fn culls(&self) -> bool {
        self.shared.is_some()
            && self.crowds.iter().flatten().any(|c| match &c.path {
                Path::Gpu(g) => c.count > 0 && g.culled > 0,
                Path::Cpu(_) => false,
            })
    }

    /// Whether a crowd draws into shadow view `v` this frame (it moves:
    /// the view is drawn anew).
    pub(super) fn reaches_shadow_view(&self, v: usize) -> bool {
        self.shadow_mask & (1 << v) != 0
    }

    /// GPU objects the crowds made: buffers, bind groups and pipelines
    /// (none without a crowd).
    #[cfg(test)]
    pub(super) fn gpu_objects(&self) -> usize {
        let crowds: usize = self
            .crowds
            .iter()
            .flatten()
            .map(|c| match &c.path {
                Path::Gpu(_) => 16,
                Path::Cpu(p) => p.posed.iter().flatten().count() + usize::from(p.instances.is_some()),
            })
            .sum();
        crowds + if self.shared.is_some() { 11 } else { 0 } + self.sets.len()
    }

    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        for crowd in self.crowds.iter().flatten() {
            match &crowd.path {
                Path::Gpu(g) => {
                    for b in &g.records {
                        t.buffer(C::InstanceBatches, "crowd records", b);
                    }
                    t.buffer(C::Skinning, "crowd animation", &g.anim);
                    t.buffer(C::InstanceBatches, "crowd views", &g.views);
                    t.buffer(C::InstanceBatches, "crowd visible lists", &g.visible);
                    t.buffer(C::InstanceBatches, "crowd draws", &g.draws);
                    t.buffer(C::InstanceBatches, "crowd levels", &g.levels);
                    t.buffer(C::InstanceBatches, "crowd cull params", &g.params);
                }
                Path::Cpu(p) => {
                    for (b, _) in p.posed.iter().flatten() {
                        t.buffer(C::Skinning, "crowd posed frames", b);
                    }
                    if let Some(b) = &p.instances {
                        t.buffer(C::InstanceBatches, "crowd instances", b);
                    }
                }
            }
        }
        if let Some(shared) = &self.shared {
            t.buffer(C::InstanceBatches, "crowd marker", &shared.marker);
            t.buffer(C::InstanceBatches, "crowd depth grid", &shared.grid);
        }
    }

    /// The crowd pipelines of `set` drawn with `sides` (dithering for a
    /// crossfade with `fade`), if built.
    fn pipelines(&self, set: &Arc<ShaderPipelines>, sides: Sides, fade: bool) -> Option<&Pipelines> {
        self.set(set, sides, fade)?.pipelines.as_ref()
    }

    /// The entry of `set`'s crowd pipelines for `sides` and `fade`, built or not.
    fn set(&self, set: &Arc<ShaderPipelines>, sides: Sides, fade: bool) -> Option<&CrowdSet> {
        let at = Arc::as_ptr(set);
        self.sets
            .iter()
            .find(|s| s.sides == sides && s.fade == fade && std::ptr::eq(s.set.as_ptr(), at))
    }
}

impl GpuCrowd {
    /// The main frame's clock at `now` (the last frame's at `last_now`):
    /// the epoch of an update first drawn, the clock moved on every
    /// [`REBASE_SECONDS`], and the seconds since the epoch the shaders get.
    fn tick(&mut self, now: f64, last_now: f64) {
        // A move the last frame planned but no pass made: undo it.
        if let Path::Gpu(g) = &mut self.path
            && let Some(old) = g.pending.take()
            && !g.advanced.swap(false, Ordering::Relaxed)
        {
            self.epoch = Some(old);
        }
        self.moved = std::mem::take(&mut self.updated);
        // A new crowd's last frame is its first records, played back.
        self.past_epoch = self.epoch.unwrap_or(now);
        if self.moved {
            self.epoch = Some(now);
        }
        if let Some(epoch) = self.epoch
            && !self.moved
            && self.count > 0
            && now - epoch >= REBASE_SECONDS
        {
            // The records move on to `now`; the last frame's are the same
            // records (nothing was updated), so both count from `now`.
            let seconds = now - epoch;
            match &mut self.path {
                Path::Gpu(g) => {
                    g.pending = Some(epoch);
                    g.seconds = seconds as f32;
                }
                Path::Cpu(p) => p.advance(seconds),
            }
            self.epoch = Some(now);
            self.past_epoch = now;
        }
        self.elapsed = (now - self.epoch.unwrap_or(now)) as f32;
        self.past_elapsed = (last_now - self.past_epoch) as f32;
    }
}

/// The time on clip `clip` (its row of the clip table: first frame, frames,
/// fps, looping) that shows, from now on, what `time` shows `seconds` later
/// at `speed`: wrapped into a looping clip, held at the end a held clip
/// plays towards (as `crowd_advance.wgsl` does on the GPU).
pub(super) fn advance_time(time: f32, speed: f32, seconds: f64, clip: [f32; 4]) -> f32 {
    if speed == 0.0 {
        return time;
    }
    let (frames, fps, looping) = (f64::from(clip[1]), f64::from(clip[2]), clip[3] > 0.5);
    let last = (frames - 1.0).max(0.0);
    let mut p = (f64::from(time) + f64::from(speed) * seconds) * fps;
    if looping && last > 0.0 {
        p = p.rem_euclid(last);
    } else if speed > 0.0 {
        p = p.min(last);
    } else {
        p = p.max(0.0);
    }
    (p / fps) as f32
}

impl Shared {
    /// Fills the depth grid's uniform from `grid` (the finest level that
    /// fits), with the camera now at `camera`; false if none fits.
    fn fill_grid(&mut self, grid: &crate::three::occlusion::OcclusionGrid, camera: Vec3) -> bool {
        let room = (self.grid_bytes / 4) as usize - 20;
        let Some((w, h, depths)) = grid.levels().iter().find(|(_, _, d)| d.len() <= room) else {
            return false;
        };
        let s = &mut self.grid_scratch;
        s.clear();
        s.extend_from_slice(&[*w as f32, *h as f32, grid.camera.distance(camera), 1.0]);
        s.extend_from_slice(&grid.view_proj.to_cols_array());
        s.extend_from_slice(depths);
        true
    }
}

/// A call into wgpu from the crowd code: in test builds what the call
/// allocates is wgpu's (a test asserts the crowd code itself allocates
/// nothing in a steady frame).
macro_rules! gpu {
    ($call:expr) => {{
        #[cfg(test)]
        let _wgpu = crate::three::lights::tests::unmarked();
        $call
    }};
}

/// `x` rounded up to a multiple of `to`.
fn align(x: u64, to: u64) -> u64 {
    x.div_ceil(to) * to
}

impl Renderer3D {
    /// Hands crowd `id` over (`None`: it was freed or withdrawn). The same
    /// bake and levels again only replaces the characters.
    pub fn set_crowd(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: u32, crowd: Option<&CrowdData>) {
        let id = id as usize;
        while self.crowds.crowds.len() <= id {
            self.crowds.crowds.push(None);
        }
        let Some(data) = crowd else {
            self.crowds.crowds[id] = None;
            return;
        };
        let same = self.crowds.crowds[id].as_ref().is_some_and(|c| {
            Arc::ptr_eq(&c.animation, &data.texels)
                && c.levels.len() == data.levels.len()
                && c.levels.iter().zip(&data.levels).all(|(l, d)| l.mesh == d.mesh.slot())
        });
        if !same {
            self.crowds.crowds[id] = self.make_crowd(device, data);
        }
        let shared = self.crowds.shared.as_ref();
        let Some(crowd) = self.crowds.crowds[id].as_mut() else {
            return;
        };
        let before = crowd.count;
        crowd.count = data.records.len() as u32;
        crowd.min = data.min;
        crowd.max = data.max;
        crowd.updated = true;
        match &mut crowd.path {
            Path::Gpu(g) => {
                let shared =
                    shared.expect("crowds on the GPU path have their shared state; invariant: made with the first");
                let grown = crowd.count > g.capacity;
                if grown {
                    g.grow(device, shared, crowd.count);
                }
                // The last frame's records stay in the other buffer (a new
                // or grown crowd has none: both hold the new ones).
                let fresh = !same || grown;
                let next = 1 - g.current;
                queue.uploading(Upload::StaticBatches).write_buffer(
                    &g.records[next],
                    0,
                    bytemuck::cast_slice(&data.records),
                );
                if fresh {
                    queue.uploading(Upload::StaticBatches).write_buffer(
                        &g.records[1 - next],
                        0,
                        bytemuck::cast_slice(&data.records),
                    );
                } else if crowd.count > before {
                    // Characters appended have no last record: they start
                    // from their new one, not from what last stood there.
                    let tail = &data.records[before as usize..];
                    queue.uploading(Upload::StaticBatches).write_buffer(
                        &g.records[1 - next],
                        u64::from(before) * size_of::<CrowdRecord>() as u64,
                        bytemuck::cast_slice(tail),
                    );
                }
            }
            Path::Cpu(p) => {
                p.records.clear();
                p.records.extend_from_slice(&data.records);
            }
        }
    }

    /// A new crowd on the path this device takes, or `None` if it can't be
    /// drawn here (said once).
    fn make_crowd(&mut self, device: &wgpu::Device, data: &CrowdData) -> Option<GpuCrowd> {
        let levels: Vec<Level> = data
            .levels
            .iter()
            .zip(&data.sections)
            .enumerate()
            .map(|(k, (l, s))| Level {
                mesh: l.mesh.slot(),
                min: l.min_distance,
                max: l.max_distance,
                camera: l.camera,
                shadows: l.shadows && data.options.cast_shadows,
                influences: u32::from(l.influences),
                section: s.start,
                slots: s.slots,
                fade_out: if l.max_distance.is_finite() { l.fade } else { 0.0 },
                // The fade of the level that ends where this one starts.
                fade_in: data
                    .levels
                    .iter()
                    .enumerate()
                    .find(|&(j, o)| j != k && o.max_distance == l.min_distance && o.fade > 0.0)
                    .map_or(0.0, |(_, o)| o.fade),
            })
            .collect();
        let gpu = self.resources.storage_read.is_some() && GpuLimits::of(device).compute_culling();
        let path = if gpu {
            if self.crowds.shared.is_none() {
                self.crowds.shared = Some(self.make_crowd_shared(device));
            }
            let shared = self.crowds.shared.as_ref()?;
            let anim = device
                .uploading(Upload::Skins)
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("crowd animation"),
                    contents: bytemuck::cast_slice(&data.texels),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let mut part_base = Vec::with_capacity(levels.len());
            let mut parts = 0;
            for level in &levels {
                part_base.push(parts);
                parts += self
                    .resources
                    .meshes
                    .get(level.mesh as usize)
                    .map_or(0, |m| m.parts.len() as u32);
            }
            let clip_boxes = data.cull_table();
            Path::Gpu(Box::new(GpuPath::new(
                device,
                shared,
                anim,
                (data.records.len() as u32).max(1),
                levels.len(),
                (parts, part_base),
                clip_boxes,
            )))
        } else {
            Path::Cpu(Box::new(self.pose_on_cpu(device, data, &levels)))
        };
        Some(GpuCrowd {
            casts: levels.iter().any(|l| l.shadows),
            fades: levels.iter().any(|l| l.fade_out > 0.0),
            levels,
            clips: data.clips,
            count: 0,
            min: data.min,
            max: data.max,
            animation: data.texels.clone(),
            epoch: None,
            past_epoch: 0.0,
            updated: true,
            elapsed: 0.0,
            past_elapsed: 0.0,
            moved: false,
            path,
        })
    }

    fn make_crowd_shared(&self, device: &wgpu::Device) -> Shared {
        let limits = device.limits();
        let storage = |binding, read_only, visibility, dynamic| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: dynamic,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform = |binding, visibility, dynamic| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: None,
            },
            count: None,
        };
        let compute = wgpu::ShaderStages::COMPUTE;
        let cull_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crowd cull"),
            entries: &[
                uniform(0, compute, false),
                storage(1, true, compute, false),
                storage(2, true, compute, false),
                storage(3, false, compute, false),
                storage(4, false, compute, false),
                uniform(5, compute, false),
            ],
        });
        let grid_bytes = limits.max_uniform_buffer_binding_size.min(GRID_BYTES);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crowd cull"),
            source: wgpu::ShaderSource::Wgsl(cull_source(grid_bytes).into()),
        });
        let cull_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("crowd cull"),
            bind_group_layouts: &[Some(&cull_layout)],
            immediate_size: 0,
        });
        let entry = |name| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&cull_pipeline_layout),
                module: &module,
                entry_point: Some(name),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (cull, finish) = (entry("cull"), entry("finish"));
        let advance_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crowd advance"),
            entries: &[
                uniform(0, compute, false),
                storage(1, false, compute, false),
                storage(2, true, compute, false),
            ],
        });
        let advance_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crowd advance"),
            source: wgpu::ShaderSource::Wgsl(ADVANCE.into()),
        });
        let advance = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("crowd advance"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("crowd advance"),
                bind_group_layouts: &[Some(&advance_layout)],
                immediate_size: 0,
            })),
            module: &advance_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let grid = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd depth grid"),
            size: grid_bytes,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // `items` and `static_indices` as material shaders declare them
        // (their `surface` may read `items`), then the crowd's own.
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let vertex = wgpu::ShaderStages::VERTEX;
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crowd draw"),
            entries: &[
                storage(0, true, both, false),
                storage(1, true, both, true),
                storage(2, true, vertex, false),
                storage(3, true, vertex, false),
                uniform(4, vertex, true),
            ],
        });
        let pipeline_layout = |label, frame: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[
                    Some(frame),
                    Some(&self.resources.material_layout),
                    Some(&self.scene_layout),
                    Some(&draw_layout),
                ],
                immediate_size: 0,
            })
        };
        let layout = pipeline_layout("crowd pipeline layout", &self.frame_sources.layout);
        let shadow_layout = pipeline_layout("crowd shadow pipeline layout", &self.crowds.shadow_frame);
        let marker = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd instance marker"),
            size: size_of::<Instance3D>() as u64,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        Shared {
            cull_layout,
            cull,
            finish,
            advance_layout,
            advance,
            grid,
            grid_mirror: traffic::Mirror::default(),
            grid_bytes,
            grid_scratch: Vec::new(),
            grid_on: false,
            draw_layout,
            layout,
            shadow_layout,
            marker,
            storage_align: limits.min_storage_buffer_offset_alignment,
            uniform_align: limits.min_uniform_buffer_offset_alignment,
            max_binding: limits.max_storage_buffer_binding_size.min(limits.max_buffer_size),
            max_groups: limits.max_compute_workgroups_per_dimension,
        }
    }

    /// The CPU path's crowd: each level's mesh posed once per kept baked
    /// frame, into one vertex buffer per level.
    fn pose_on_cpu(&self, device: &wgpu::Device, data: &CrowdData, levels: &[Level]) -> CpuPath {
        let table: Vec<[f32; 4]> = data.texels[..data.clips as usize].to_vec();
        let corners: Vec<u64> = levels
            .iter()
            .map(|l| {
                self.resources
                    .meshes
                    .get(l.mesh as usize)
                    .and_then(|m| m.skin.as_ref())
                    .and_then(GpuSkin::rest_on_cpu)
                    .map_or(0, |(v, _)| v.len() as u64)
            })
            .collect();
        let kept = |step: u32| -> u64 { table.iter().map(|c| u64::from((c[1] as u32 - 1) / step + 1)).sum() };
        let bytes = |step: u32| kept(step) * corners.iter().sum::<u64>() * size_of::<Vertex>() as u64;
        let longest = table.iter().map(|c| c[1] as u32).max().unwrap_or(1).max(1);
        let mut step = 1;
        while step < longest && bytes(step) > POSED_BYTES {
            step += 1;
        }
        let mut clips = Vec::with_capacity(table.len());
        let mut first = 0;
        for c in &table {
            let count = (c[1] as u32 - 1) / step + 1;
            clips.push((first, count, c[2], c[3] > 0.5, c[0] as u32));
            first += count;
        }
        let mut posed = Vec::with_capacity(levels.len());
        let mut skin = Vec::new();
        let mut vertices: Vec<Vertex> = Vec::new();
        for level in levels {
            let rest = self
                .resources
                .meshes
                .get(level.mesh as usize)
                .and_then(|m| m.skin.as_ref())
                .and_then(GpuSkin::rest_on_cpu);
            let Some((rest, influences)) = rest.filter(|(v, _)| !v.is_empty()) else {
                posed.push(None);
                continue;
            };
            vertices.clear();
            for (c, &(_, count, _, _, baked_first)) in table.iter().zip(&clips) {
                let frames = c[1] as u32;
                for k in 0..count {
                    let g = baked_first + (k * step).min(frames - 1);
                    skin.clear();
                    for s in 0..level.slots {
                        let at = (level.section + (g * level.slots + s) * 3) as usize;
                        let [r0, r1, r2] = [0, 1, 2].map(|r| Vec4::from(data.texels[at + r]));
                        skin.push(Mat4::from_cols(
                            Vec4::new(r0.x, r1.x, r2.x, 0.0),
                            Vec4::new(r0.y, r1.y, r2.y, 0.0),
                            Vec4::new(r0.z, r1.z, r2.z, 0.0),
                            Vec4::new(r0.w, r1.w, r2.w, 1.0),
                        ));
                    }
                    vertices.extend(rest.iter().zip(influences).map(|(v, i)| {
                        let (j, w) = top_influences(i.joints, i.weights, level.influences as u8);
                        let m = if w.iter().sum::<f32>() > 0.0 {
                            let last = skin.len().saturating_sub(1) as u16;
                            crate::three::blend_skin(&skin, j.map(|j| j.min(last)), w)
                        } else {
                            Mat4::IDENTITY
                        };
                        let p = m.transform_point3(Vec3::from(v.position));
                        let n = m.transform_vector3(Vec3::from(v.normal)).normalize_or(Vec3::Y);
                        Vertex {
                            position: p.to_array(),
                            normal: n.to_array(),
                            ..*v
                        }
                    }));
                }
            }
            let buffer = device
                .uploading(Upload::Skins)
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("crowd posed frames"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            posed.push(Some((buffer, rest.len() as u32)));
        }
        if step > 1 {
            traffic::too_big(
                "a crowd's posed frames on the CPU path",
                bytes(1) as usize,
                POSED_BYTES as usize,
                &format!("every {step}th baked frame is kept; bake fewer frames or give far levels fewer corners"),
            );
        }
        CpuPath {
            records: Vec::new(),
            boxes: Vec::new(),
            range_first: data.range_first.clone(),
            range_bounds: data.range_bounds.clone(),
            table,
            clips,
            step,
            posed,
            instances: None,
            instance_capacity: 0,
            runs: Vec::new(),
            view_runs: Vec::new(),
            keys: Vec::new(),
            frames: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Readies this frame's crowds: their clips' time, the views they're
    /// culled against (on the GPU, or here on the CPU path), the shadow
    /// views they reach, and the crowd pipelines of the materials they
    /// draw. Runs after the batches' views are known (`prepare_static`) and
    /// before the shadow views are planned. Nothing without a crowd.
    ///
    /// In a window view (`self.extra`) only the view's camera is culled,
    /// at the main frame's time, and nothing the main frame decided for
    /// every view (the clock, the shadow views, updates) changes.
    pub(super) fn prepare_crowds(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &crate::three::Frame3D,
        (now, dt): (f64, f32),
        (occlusion, camera): (bool, Vec3),
    ) {
        let extra = self.extra;
        if !extra {
            self.crowds.shadow_mask = 0;
        }
        if self.crowds.crowds.iter().all(Option::is_none) {
            if !extra {
                self.crowds.last_now = None;
            }
            return;
        }
        // Tests count what the crowd code itself allocates in a frame.
        #[cfg(test)]
        let _crowd_code = crate::three::lights::tests::marked();
        let last_now = if extra {
            now
        } else {
            let last = self.crowds.last_now.unwrap_or(now - f64::from(dt));
            self.crowds.last_now = Some(now);
            last
        };
        // A window view culls for its camera alone; the main frame also for
        // its shadow views, target cameras and global illumination's views
        // (its sun shadow map and face directions; not each probe's own).
        let gi = self.lists.gi_batch_slot.filter(|_| !extra);
        let drawn = Drawn {
            cascades: self.lists.cascades,
            light_views: self.lists.light_views,
            targets: 1 + MAX_CASCADES + frame.light_views,
            gi,
        };
        let slots = &self.batching.slots;
        let views = if extra {
            1
        } else {
            gi.map_or(slots.len(), |gi| gi + crate::three::gi::GI_BATCH_FACES)
                .min(slots.len())
        };
        let crowds = &mut self.crowds;
        crowds.probes.clear();
        if gi.is_some()
            && let Some(g) = frame.gi.as_deref()
        {
            let probes = g.batch_views.iter().skip(crate::three::gi::GI_BATCH_FACES).step_by(6);
            crowds.probes.extend(probes.filter_map(|v| v.1));
        }
        let grid = frame
            .occlusion
            .as_ref()
            .filter(|g| occlusion && !extra && g.base().is_some())
            .map(|g| (g, camera));
        let gpu_crowds = crowds
            .crowds
            .iter()
            .flatten()
            .any(|c| matches!(c.path, Path::Gpu(_)) && c.count > 0);
        if let Some(shared) = crowds.shared.as_mut().filter(|_| gpu_crowds) {
            shared.grid_on = !extra && grid.is_some_and(|(g, camera)| shared.fill_grid(g, camera));
            if shared.grid_on {
                let bytes = bytemuck::cast_slice(&shared.grid_scratch);
                shared
                    .grid_mirror
                    .upload(queue, Upload::StaticBatches, &shared.grid, 0, bytes);
            }
        }
        let cull_extra = Extra {
            grid,
            probes: &crowds.probes,
        };
        for crowd in crowds.crowds.iter_mut().flatten() {
            if !extra {
                crowd.tick(now, last_now);
            }
            if !extra && crowd.casts && crowd.count > 0 {
                let casting = || crowd.levels.iter().filter(|l| l.shadows);
                let near = casting().map(|l| l.min - l.fade_in).fold(f32::INFINITY, f32::min);
                let far = casting().map(|l| l.max).fold(0.0, f32::max);
                for v in super::shadow_views(drawn.cascades, drawn.light_views) {
                    let slot = &slots[v + 1];
                    if intersects(crowd.min, crowd.max, &slot.planes, slot.eye, near, far) {
                        crowds.shadow_mask |= 1 << v;
                    }
                }
            }
            let info = Frame {
                count: crowd.count,
                clips: crowd.clips,
                elapsed: crowd.elapsed,
                past_elapsed: crowd.past_elapsed,
            };
            let GpuCrowd {
                levels, path, moved, ..
            } = crowd;
            match path {
                Path::Gpu(g) => {
                    let Some(shared) = &crowds.shared else { continue };
                    if views as u32 > g.view_capacity {
                        g.fit_views(
                            device,
                            queue,
                            shared,
                            views as u32,
                            levels,
                            &self.resources.meshes,
                            &mut crowds.reported,
                        );
                    }
                    g.culled = (views as u32).min(g.view_capacity);
                    if *moved && !extra {
                        g.current = 1 - g.current;
                    }
                    let occluded = shared.grid_on && !extra;
                    g.upload(queue, shared, levels, info, slots, drawn, (occluded, cull_extra.probes));
                }
                Path::Cpu(p) => {
                    p.cull(levels, info, slots, views, drawn, cull_extra);
                    p.upload(device, queue);
                }
            }
        }
        self.prepare_crowd_pipelines(device);
    }

    /// Builds the crowd pipelines of every material a GPU-path crowd draws
    /// and drops those of shaders gone.
    fn prepare_crowd_pipelines(&mut self, device: &wgpu::Device) {
        let targets = self.shaders.cache.targets;
        self.crowds
            .sets
            .retain(|s| s.targets == targets && s.set.strong_count() > 0);
        for crowd in self.crowds.crowds.iter().flatten() {
            if !matches!(crowd.path, Path::Gpu(_)) {
                continue;
            }
            for level in &crowd.levels {
                let mesh = level.mesh;
                let Some(m) = self.resources.meshes.get(mesh as usize) else {
                    continue;
                };
                for (part, &(_, own)) in m.parts.iter().enumerate() {
                    let material = self.part_material(mesh, part, own);
                    let Some(gm) = self.resources.materials.get(material as usize) else {
                        continue;
                    };
                    let set = self.shaders.sets[gm.shader as usize].clone();
                    let sides = Sides::of(gm.double_sided);
                    // The dithering variant only for crowds that crossfade.
                    for fade in [false, crowd.fades] {
                        if self.crowds.set(&set, sides, fade).is_some() {
                            continue;
                        }
                        let pipelines = self.build_crowd_pipelines(device, &set, sides, fade);
                        self.crowds.sets.push(CrowdSet {
                            set: Arc::downgrade(&set),
                            sides,
                            fade,
                            targets,
                            pipelines,
                        });
                    }
                }
            }
        }
    }

    /// A material shader's crowd pipelines for `sides` (dithering for a
    /// crossfade with `fade`), or `None` (said) if its code doesn't build
    /// with the crowd's parts.
    fn build_crowd_pipelines(
        &self,
        device: &wgpu::Device,
        set: &Arc<ShaderPipelines>,
        sides: Sides,
        fade: bool,
    ) -> Option<Pipelines> {
        let shared = self.crowds.shared.as_ref()?;
        let source = self.shaders.compose_like(set, compose_crowd)?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crowd material"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let builder = super::shaders::SideBuilder {
            module,
            layout: shared.layout.clone(),
            shadow_layout: shared.shadow_layout.clone(),
            crowd: true,
            ..set.builder.clone()
        };
        let instancing = if fade { Instancing::Crossfade } else { Instancing::Plain };
        let pipelines = builder.build(ShaderVariant::new(sides, instancing));
        match super::end_scope(scope, false) {
            Ok(None) => Some(pipelines),
            Ok(Some(error)) => {
                traffic::warn(&format!(
                    "a crowd's material shader doesn't build with the crowd's vertex code (its crowds draw nothing): {error}"
                ));
                None
            }
            Err(_) => Some(pipelines),
        }
    }

    /// The crowds' cull pass (GPU path), at the start of the frame (and of
    /// each window view's), after moving the clock of the crowds whose
    /// move this frame plans (the main frame's only).
    pub(super) fn cull_crowds<'t>(&self, encoder: &mut wgpu::CommandEncoder, time: &super::PassTimer<'_, 't>) {
        if !self.crowds.culls() {
            return;
        }
        let Some(shared) = &self.crowds.shared else { return };
        super::passes::mark("static cull");
        for crowd in self.crowds.crowds.iter().flatten() {
            if let Path::Gpu(g) = &crowd.path
                && crowd.count > 0
                && g.culled > 0
            {
                let counts = u64::from(g.view_capacity) * crowd.levels.len() as u64 * 8;
                encoder.clear_buffer(&g.draws, 0, Some(counts));
            }
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("crowd cull"),
            timestamp_writes: time("crowd cull").map(|t| wgpu::ComputePassTimestampWrites {
                query_set: t.query_set,
                beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                end_of_pass_write_index: t.end_of_pass_write_index,
            }),
        });
        for crowd in self.crowds.crowds.iter().flatten() {
            let Path::Gpu(g) = &crowd.path else { continue };
            if crowd.count == 0 || g.culled == 0 {
                continue;
            }
            let groups = crowd.count.div_ceil(64);
            let row = groups.min(shared.max_groups);
            if g.pending.is_some() && !self.extra {
                pass.set_bind_group(0, &g.advance[g.current], &[]);
                pass.set_pipeline(&shared.advance);
                pass.dispatch_workgroups(row, groups.div_ceil(row), 1);
                g.advanced.store(true, Ordering::Relaxed);
            }
            pass.set_bind_group(0, &g.cull[g.current], &[]);
            pass.set_pipeline(&shared.cull);
            pass.dispatch_workgroups(row, groups.div_ceil(row), 1);
            pass.set_pipeline(&shared.finish);
            pass.dispatch_workgroups((g.culled * g.parts * 2).div_ceil(64), 1, 1);
        }
    }

    /// Draws every crowd's levels that `view` sees, for one pass (`view`
    /// as the batches number them: 0 the camera, then the shadow views,
    /// then target cameras, then global illumination's sun shadow map and
    /// six face directions). In a window view, 0 is its camera.
    pub(super) fn draw_crowds(&self, pass: &mut wgpu::RenderPass<'_>, kind: Kind, view: usize) {
        let n = self.draw_crowds_counted(pass, kind, view);
        self.state_counters.add(&n);
    }

    /// [`Self::draw_crowds`], returning what it recorded uncounted.
    pub(super) fn draw_crowds_counted(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        kind: Kind,
        view: usize,
    ) -> super::draw_order::StateChanges {
        let mut n = super::draw_order::StateChanges::default();
        if kind == Kind::VelocityStill || self.crowds.crowds.is_empty() {
            return n;
        }
        // A target camera's culling box takes or leaves a crowd whole.
        let slot = self.batching.slots.get(view);
        for crowd in self.crowds.crowds.iter().flatten() {
            if crowd.count == 0 || slot.is_some_and(|s| !s.in_cull_box(crowd.min, crowd.max)) {
                continue;
            }
            match &crowd.path {
                Path::Gpu(g) => self.draw_gpu_crowd(pass, &mut n, crowd, g, kind, view),
                Path::Cpu(p) if kind != Kind::Velocity => self.draw_cpu_crowd(pass, &mut n, crowd, p, kind, view),
                Path::Cpu(_) => {}
            }
        }
        n
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_gpu_crowd(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        n: &mut super::draw_order::StateChanges,
        crowd: &GpuCrowd,
        g: &GpuPath,
        kind: Kind,
        view: usize,
    ) {
        let Some(shared) = &self.crowds.shared else { return };
        if view as u32 >= g.culled {
            return;
        }
        let shadow = matches!(kind, Kind::Shadow(_));
        let levels = crowd.levels.len();
        let mut bound_buffers = false;
        for (l, level) in crowd.levels.iter().enumerate() {
            if !(if shadow { level.shadows } else { level.camera }) {
                continue;
            }
            let Some(mesh) = self.resources.meshes.get(level.mesh as usize) else {
                continue;
            };
            let (Some((vertices, indices)), Some(skin)) = (mesh.buffers(&self.resources.mesh_arena), &mesh.skin) else {
                continue;
            };
            let list = (view * levels + l) as u64;
            let mut buffers_bound = false;
            // The level's whole characters, then (crossfading) its dithered ones.
            for fade in [false, true] {
                if fade && !crowd.fades {
                    break;
                }
                let offsets = [
                    (list * u64::from(g.region) * 4) as u32,
                    ((usize::from(fade) * levels + l) as u64 * u64::from(shared.uniform_align)) as u32,
                ];
                let mut group_bound = false;
                for (part, (_, own)) in mesh.parts.iter().enumerate() {
                    let material = self.part_material(level.mesh, part, *own);
                    let Some(m) = self.resources.materials.get(material as usize) else {
                        continue;
                    };
                    let Some(role) = self.draw_role(kind, material, m) else {
                        continue;
                    };
                    let shader = &self.shaders.sets[m.shader as usize];
                    let Some(pipeline) = self
                        .crowds
                        .pipelines(shader, Sides::of(m.double_sided), fade)
                        .and_then(|p| p.for_kind(kind, role))
                    else {
                        continue;
                    };
                    pass.set_pipeline(pipeline);
                    n.pipelines += 1;
                    if !bound_buffers {
                        pass.set_vertex_buffer(1, shared.marker.slice(..));
                        n.vertex_buffers += 1;
                        bound_buffers = true;
                    }
                    if !buffers_bound {
                        pass.set_vertex_buffer(0, vertices.slice(..));
                        pass.set_vertex_buffer(2, skin.influences().slice(..));
                        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                        n.vertex_buffers += 2;
                        n.index_buffers += 1;
                        buffers_bound = true;
                    }
                    if !group_bound {
                        pass.set_bind_group(3, &g.draw[g.current][usize::from(crowd.moved)], &offsets);
                        n.bind_groups += 1;
                        group_bound = true;
                    }
                    pass.set_bind_group(1, &m.bind_group, &[]);
                    n.bind_groups += 1;
                    let draw = u64::from(g.parts) * (2 * view as u64 + u64::from(fade))
                        + u64::from(g.part_base[l])
                        + part as u64;
                    let at = u64::from(g.view_capacity) * levels as u64 * 8 + draw * ARGS;
                    pass.draw_indexed_indirect(&g.draws, at);
                    n.draws += 1;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cpu_crowd(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        n: &mut super::draw_order::StateChanges,
        crowd: &GpuCrowd,
        p: &CpuPath,
        kind: Kind,
        view: usize,
    ) {
        let (Some(&(start, end)), Some(instances)) = (p.view_runs.get(view), &p.instances) else {
            return;
        };
        let mut dummy = false;
        for &(_, l, frame, first, count, fade) in &p.runs[start as usize..end as usize] {
            let level = &crowd.levels[l as usize];
            let Some((posed, corners)) = &p.posed[l as usize] else {
                continue;
            };
            let Some(mesh) = self.resources.meshes.get(level.mesh as usize) else {
                continue;
            };
            let Some((_, indices)) = mesh.buffers(&self.resources.mesh_arena) else {
                continue;
            };
            let frame_bytes = u64::from(*corners) * size_of::<Vertex>() as u64;
            let mut bound = false;
            for (part, (range, own)) in mesh.parts.iter().enumerate() {
                let material = self.part_material(level.mesh, part, *own);
                let Some(m) = self.resources.materials.get(material as usize) else {
                    continue;
                };
                let Some(role) = self.draw_role(kind, material, m) else {
                    continue;
                };
                let shader = &self.shaders.sets[m.shader as usize];
                let instancing = if fade { Instancing::Crossfade } else { Instancing::Plain };
                let variant = ShaderVariant::new(Sides::of(m.double_sided), instancing);
                let Some(pipeline) = self.draw_pipeline(shader, material, variant, kind, role) else {
                    continue;
                };
                pass.set_pipeline(pipeline);
                n.pipelines += 1;
                if let Some(d) = self.resources.storage_dummy.as_ref().filter(|_| !dummy) {
                    pass.set_bind_group(3, d, &[]);
                    n.bind_groups += 1;
                    dummy = true;
                }
                if let Some(reference) = self.stencil.reference(material) {
                    pass.set_stencil_reference(reference);
                }
                if !bound {
                    let at = u64::from(frame) * frame_bytes;
                    pass.set_vertex_buffer(0, posed.slice(at..at + frame_bytes));
                    let at = u64::from(first) * size_of::<Instance3D>() as u64;
                    pass.set_vertex_buffer(
                        1,
                        instances.slice(at..at + u64::from(count) * size_of::<Instance3D>() as u64),
                    );
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    n.vertex_buffers += 2;
                    n.index_buffers += 1;
                    bound = true;
                }
                pass.set_bind_group(1, &m.bind_group, &[]);
                n.bind_groups += 1;
                pass.draw_indexed(range.clone(), 0, 0..count);
                n.draws += 1;
            }
        }
    }
}

impl GpuPath {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        shared: &Shared,
        anim: wgpu::Buffer,
        capacity: u32,
        levels: usize,
        (parts, part_base): (u32, Vec<u32>),
        clip_boxes: Vec<[f32; 4]>,
    ) -> Self {
        let records = Self::records(device, capacity);
        let params = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd cull params"),
            size: ADVANCE_AT + 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let level_buffer = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd levels"),
            size: 2 * levels as u64 * u64::from(shared.uniform_align),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (views, visible, draws) = Self::view_buffers(device, 1, 1, 1, (1, clip_boxes.len() as u32));
        let cull = Self::cull_groups(device, shared, &records, &params, &views, &visible, &draws);
        let draw = Self::draw_groups(device, shared, &records, &visible, &anim, &level_buffer, 4);
        let advance = Self::advance_groups(device, shared, &records, &params, &anim);
        Self {
            records,
            capacity,
            current: 0,
            anim,
            params,
            levels: level_buffer,
            views,
            clip_boxes,
            boxes_at: 0,
            visible,
            draws,
            view_capacity: 0,
            region: 1,
            parts,
            part_base,
            cull,
            advance,
            pending: None,
            advanced: AtomicBool::new(false),
            seconds: 0.0,
            draw,
            culled: 0,
            scratch: Vec::new(),
        }
    }

    fn records(device: &wgpu::Device, capacity: u32) -> [wgpu::Buffer; 2] {
        [0, 1].map(|_| {
            device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("crowd records"),
                size: u64::from(capacity) * size_of::<CrowdRecord>() as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
    }

    /// Where the clips' boxes start in the views buffer, in vec4s, for
    /// `views` views and `parts` mesh parts: past the most a frame writes.
    fn boxes_at(views: u32, parts: u32) -> u64 {
        u64::from(views) * 7 + u64::from(parts.div_ceil(4)).max(1) + PROBES
    }

    /// The views, visible lists and draws buffers for `views` views of
    /// `levels` levels, `region` entries a list, `parts` mesh parts a view
    /// (each drawn whole and dithered) and a cull table of `table` vec4s.
    fn view_buffers(
        device: &wgpu::Device,
        views: u32,
        levels: u32,
        region: u32,
        (parts, table): (u32, u32),
    ) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
        let views_buffer = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd views"),
            size: (Self::boxes_at(views, parts) + u64::from(table)) * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let visible = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd visible lists"),
            size: u64::from(views) * u64::from(levels) * u64::from(region) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let draws = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("crowd draws"),
            size: (u64::from(views) * u64::from(levels) * 8 + u64::from(views) * 2 * u64::from(parts).max(1) * ARGS)
                .max(16),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST
                | traffic::MIRRORED,
            mapped_at_creation: false,
        });
        (views_buffer, visible, draws)
    }

    fn cull_groups(
        device: &wgpu::Device,
        shared: &Shared,
        records: &[wgpu::Buffer; 2],
        params: &wgpu::Buffer,
        views: &wgpu::Buffer,
        visible: &wgpu::Buffer,
        draws: &wgpu::Buffer,
    ) -> [wgpu::BindGroup; 2] {
        records.each_ref().map(|r| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("crowd cull"),
                layout: &shared.cull_layout,
                entries: &[params, r, views, visible, draws, &shared.grid]
                    .iter()
                    .enumerate()
                    .map(|(binding, b)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: if binding == 0 {
                            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: b,
                                offset: 0,
                                size: wgpu::BufferSize::new(PARAMS),
                            })
                        } else {
                            b.as_entire_binding()
                        },
                    })
                    .collect::<Vec<_>>(),
            })
        })
    }

    /// The advance groups: per current records buffer, the advance
    /// uniform (in `params`), the records to move on and the clip table.
    fn advance_groups(
        device: &wgpu::Device,
        shared: &Shared,
        records: &[wgpu::Buffer; 2],
        params: &wgpu::Buffer,
        anim: &wgpu::Buffer,
    ) -> [wgpu::BindGroup; 2] {
        records.each_ref().map(|r| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("crowd advance"),
                layout: &shared.advance_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: params,
                            offset: ADVANCE_AT,
                            size: wgpu::BufferSize::new(16),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: r.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: anim.as_entire_binding(),
                    },
                ],
            })
        })
    }

    /// The draw groups: per current buffer, the last frame's records the
    /// same buffer (`[c][0]`) or the other (`[c][1]`); `static_indices` a
    /// list of `list_bytes`, the level uniform one level's.
    fn draw_groups(
        device: &wgpu::Device,
        shared: &Shared,
        records: &[wgpu::Buffer; 2],
        visible: &wgpu::Buffer,
        anim: &wgpu::Buffer,
        levels: &wgpu::Buffer,
        list_bytes: u64,
    ) -> [[wgpu::BindGroup; 2]; 2] {
        fn sized(buffer: &wgpu::Buffer, size: u64) -> wgpu::BindingResource<'_> {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(size),
            })
        }
        let group = |current: usize, past: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("crowd draw"),
                layout: &shared.draw_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: records[current].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: sized(visible, list_bytes),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: records[past].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: anim.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: sized(levels, 32),
                    },
                ],
            })
        };
        [[group(0, 0), group(0, 1)], [group(1, 1), group(1, 0)]]
    }

    /// Room for `capacity` records (the last ones are lost: the next frame
    /// draws no motion from them).
    fn grow(&mut self, device: &wgpu::Device, shared: &Shared, capacity: u32) {
        self.capacity = capacity;
        self.records = Self::records(device, capacity);
        // The lists grow with the records: made again with the views.
        self.view_capacity = 0;
        self.rebind(device, shared);
    }

    fn rebind(&mut self, device: &wgpu::Device, shared: &Shared) {
        self.cull = Self::cull_groups(
            device,
            shared,
            &self.records,
            &self.params,
            &self.views,
            &self.visible,
            &self.draws,
        );
        self.advance = Self::advance_groups(device, shared, &self.records, &self.params, &self.anim);
        let list_bytes = u64::from(self.region) * 4;
        self.draw = Self::draw_groups(
            device,
            shared,
            &self.records,
            &self.visible,
            &self.anim,
            &self.levels,
            list_bytes,
        );
    }

    /// Room for `views` views' lists and draws (as many as the device's
    /// storage binding holds), with the draws' fixed words written.
    #[allow(clippy::too_many_arguments)]
    fn fit_views(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shared: &Shared,
        views: u32,
        levels: &[Level],
        meshes: &[super::GpuMesh],
        reported: &mut bool,
    ) {
        let l = levels.len() as u64;
        let per_entry = u64::from(shared.storage_align / 4).max(1);
        let region = align(1 + u64::from(self.capacity), per_entry);
        let list_bytes = region * 4;
        let most = (shared.max_binding / (l * list_bytes).max(1)) as u32;
        let capacity = views.min(most).max(1);
        if capacity < views && !*reported {
            *reported = true;
            traffic::too_big(
                "a crowd's visible lists",
                (u64::from(views) * l * list_bytes) as usize,
                shared.max_binding as usize,
                "the crowd isn't drawn in the last views (global illumination's, then target cameras)",
            );
        }
        self.region = region as u32;
        self.view_capacity = capacity;
        let table = self.clip_boxes.len() as u32;
        (self.views, self.visible, self.draws) =
            Self::view_buffers(device, capacity, l as u32, self.region, (self.parts, table));
        // The draws' fixed words: index count and first index of each
        // part, whole then dithered.
        let mut args: Vec<u32> = Vec::with_capacity((capacity * self.parts * 10) as usize);
        for _ in 0..capacity * 2 {
            for level in levels {
                if let Some(mesh) = meshes.get(level.mesh as usize) {
                    for (range, _) in &mesh.parts {
                        args.extend([range.end - range.start, 0, range.start, 0, 0]);
                    }
                }
            }
        }
        if !args.is_empty() {
            let at = u64::from(capacity) * l * 8;
            queue
                .uploading(Upload::StaticBatches)
                .write_buffer(&self.draws, at, bytemuck::cast_slice(&args));
        }
        // The cull table, which only a new crowd changes.
        let boxes_at = Self::boxes_at(capacity, self.parts);
        self.boxes_at = boxes_at as u32;
        if !self.clip_boxes.is_empty() {
            queue.uploading(Upload::StaticBatches).write_buffer(
                &self.views,
                boxes_at * 16,
                bytemuck::cast_slice(&self.clip_boxes),
            );
        }
        self.rebind(device, shared);
    }

    /// This frame's cull and advance uniforms, views and level uniforms.
    /// `occluded`: the camera's view culls against the depth grid;
    /// `probes`: global illumination's probe updates this frame.
    #[allow(clippy::too_many_arguments)]
    fn upload(
        &mut self,
        queue: &wgpu::Queue,
        shared: &Shared,
        levels: &[Level],
        crowd: Frame,
        slots: &[super::instances::SlotView],
        drawn: Drawn,
        (occluded, probes): (bool, &[Vec3]),
    ) {
        // The views (one not drawn this frame culls everything), then each
        // draw's level, then the probes.
        let scratch = &mut self.scratch;
        scratch.clear();
        for (s, slot) in slots.iter().enumerate().take(self.culled as usize) {
            for plane in slot.planes {
                let plane = if drawn.slot(s) {
                    plane.to_array()
                } else {
                    [0.0, 0.0, 0.0, -1e30]
                };
                scratch.extend_from_slice(bytemuck::cast_slice(&plane));
            }
            let eye = [slot.eye.x, slot.eye.y, slot.eye.z, f32::from(u8::from(drawn.shadow(s)))];
            scratch.extend_from_slice(bytemuck::cast_slice(&eye));
        }
        for (l, &base) in self.part_base.iter().enumerate() {
            let end = self.part_base.get(l + 1).copied().unwrap_or(self.parts);
            for _ in base..end {
                scratch.extend_from_slice(&(l as u32).to_ne_bytes());
            }
        }
        scratch.resize(
            align(scratch.len() as u64, 16) as usize + 16 * usize::from(self.parts == 0),
            0,
        );
        let probes_at = (scratch.len() / 16) as u32;
        let probes = &probes[..probes.len().min(PROBES as usize)];
        for probe in probes {
            scratch.extend_from_slice(bytemuck::cast_slice(&probe.extend(0.0).to_array()));
        }
        // The cull uniform, and the advance pass's after it.
        let mut params = [0u32; (ADVANCE_AT / 4 + 4) as usize];
        let groups = crowd.count.div_ceil(64);
        let row = groups.min(shared.max_groups).max(1);
        let gi = drawn.gi_faces();
        params[..8].copy_from_slice(&[
            crowd.count,
            self.culled,
            levels.len() as u32,
            self.parts,
            self.region,
            // The indirect draws start after two counts per (view, level).
            self.view_capacity * levels.len() as u32 * 2,
            row * 64,
            crowd.clips,
        ]);
        params[8..16].copy_from_slice(&[
            self.boxes_at,
            u32::from(occluded),
            gi.start as u32,
            gi.end as u32,
            probes.len() as u32,
            probes_at,
            crate::three::gi::GI_SMALLEST.to_bits(),
            // The characters' times are this far on (their frame ranges).
            crowd.elapsed.to_bits(),
        ]);
        let kinds = 16 + 4 * MAX_CROWD_LEVELS;
        for (l, level) in levels.iter().enumerate() {
            let band = [level.min, level.max, level.fade_out, level.fade_in];
            params[16 + l * 4..20 + l * 4].copy_from_slice(&band.map(f32::to_bits));
            params[kinds + l] = u32::from(level.camera) | u32::from(level.shadows) << 1;
        }
        let advance = (ADVANCE_AT / 4) as usize;
        params[advance..advance + 4].copy_from_slice(&[crowd.count, row * 64, crowd.clips, self.seconds.to_bits()]);
        gpu!(
            queue
                .uploading(Upload::StaticBatches)
                .write_buffer(&self.params, 0, bytemuck::cast_slice(&params))
        );
        gpu!(
            queue
                .uploading(Upload::StaticBatches)
                .write_buffer(&self.views, 0, scratch)
        );
        // The levels' uniforms: for the whole characters (read from a
        // list's front, after its leading 0), then for the dithered ones
        // (from its back).
        let stride = shared.uniform_align as usize;
        scratch.clear();
        scratch.resize(2 * levels.len() * stride, 0);
        for fade in [false, true] {
            let (first, step) = if fade { (self.region - 1, -1_i32) } else { (1, 1) };
            for (l, level) in levels.iter().enumerate() {
                let words = [
                    level.section,
                    level.slots,
                    level.influences,
                    crowd.clips,
                    crowd.elapsed.to_bits(),
                    crowd.past_elapsed.to_bits(),
                    first,
                    step as u32,
                ];
                let at = (usize::from(fade) * levels.len() + l) * stride;
                scratch[at..at + 32].copy_from_slice(bytemuck::cast_slice(&words));
            }
        }
        gpu!(
            queue
                .uploading(Upload::StaticBatches)
                .write_buffer(&self.levels, 0, scratch)
        );
    }
}

impl CpuPath {
    /// Moves every character's clip times `seconds` on (see
    /// [`advance_time`]).
    fn advance(&mut self, seconds: f64) {
        let clip = |c: u32| {
            self.table
                .get(c as usize)
                .or(self.table.last())
                .copied()
                .unwrap_or_default()
        };
        for r in &mut self.records {
            let [clips, ta, tb, packed] = r.anim;
            let speed = f32::from((packed >> 16) as u16 as i16) / 256.0;
            let a = advance_time(f32::from_bits(ta), speed, seconds, clip(clips & 0xffff));
            let b = advance_time(f32::from_bits(tb), speed, seconds, clip(clips >> 16));
            r.anim[1] = a.to_bits();
            r.anim[2] = b.to_bits();
        }
    }

    /// The clip a character shows (its blend clip from half way) and,
    /// among the clip's posed frames, the one nearest its time.
    fn shown(&self, record: &CrowdRecord, elapsed: f32) -> (usize, u32) {
        let [clips, ta, tb, packed] = record.anim;
        let blend = (packed & 0xffff) as f32 / 65535.0;
        let speed = f32::from((packed >> 16) as u16 as i16) / 256.0;
        let (clip, time) = if blend >= 0.5 {
            (clips >> 16, f32::from_bits(tb))
        } else {
            (clips & 0xffff, f32::from_bits(ta))
        };
        let clip = (clip as usize).min(self.clips.len().saturating_sub(1));
        let Some(&(_, count, fps, looping, _)) = self.clips.get(clip) else {
            return (0, 0);
        };
        let step = self.step as f32;
        // In baked frames, then among the posed ones (every `step`th).
        let last = ((count - 1) * self.step) as f32;
        let mut p = (time + speed * elapsed) * fps;
        p = if looping && last > 0.0 {
            p.rem_euclid(last)
        } else {
            p.clamp(0.0, last)
        };
        (clip, ((p / step).round() as u32).min(count - 1))
    }

    /// A character's culling box this frame, showing posed frame `k` of
    /// clip `clip` ([`shown`](Self::shown)): the box of the frame range of
    /// that baked frame, under its transform.
    fn cull_box(&self, record: &CrowdRecord, (clip, k): (usize, u32)) -> CullBox {
        let frames = self.table.get(clip).map_or(1, |c| c[1] as u32);
        let frame = (k * self.step).min(frames.saturating_sub(1));
        let first = self.range_first.get(clip).copied().unwrap_or(0);
        let next = self
            .range_first
            .get(clip + 1)
            .copied()
            .unwrap_or(self.range_bounds.len() as u32);
        let range = (first + frame / crate::three::crowd::RANGE_FRAMES).min(next.saturating_sub(1));
        let bounds = self
            .range_bounds
            .get(range as usize)
            .copied()
            .unwrap_or((Vec3::ZERO, Vec3::ZERO));
        // The record's model matrix carries other words in its first two
        // columns' w.
        let m = &record.instance.model;
        let model = Mat4::from_cols(
            Vec4::new(m[0][0], m[0][1], m[0][2], 0.0),
            Vec4::new(m[1][0], m[1][1], m[1][2], 0.0),
            Vec4::new(m[2][0], m[2][1], m[2][2], 0.0),
            Vec4::new(m[3][0], m[3][1], m[3][2], 1.0),
        );
        CullBox::new(&model, bounds)
    }

    /// Culls the characters against this frame's views on the CPU and
    /// groups the visible ones by (view, level, posed frame).
    fn cull(
        &mut self,
        levels: &[Level],
        crowd: Frame,
        slots: &[super::instances::SlotView],
        views: usize,
        drawn: Drawn,
        extra: Extra<'_>,
    ) {
        let gi_faces = drawn.gi_faces();
        self.frames.clear();
        self.boxes.clear();
        for i in 0..self.records.len() {
            let shown = self.shown(&self.records[i], crowd.elapsed);
            let f = self.clips.get(shown.0).map_or(0, |c| c.0 + shown.1);
            self.frames.push(f);
            let b = self.cull_box(&self.records[i], shown);
            self.boxes.push(b);
        }
        self.keys.clear();
        for (v, slot) in slots.iter().enumerate().take(views) {
            if !drawn.slot(v) {
                continue;
            }
            let shadow = drawn.shadow(v);
            for (i, b) in self.boxes.iter().enumerate() {
                if slot.planes.iter().any(|p| b.behind(*p)) {
                    continue;
                }
                let e = b.extent();
                if v == 0
                    && let Some((grid, camera)) = extra.grid
                    && grid.hides(b.center - e, b.center + e, camera)
                {
                    continue;
                }
                let cut = crate::three::gi::GI_SMALLEST;
                if gi_faces.contains(&v) && !extra.probes.iter().any(|p| b.radius() >= cut * b.center.distance(*p)) {
                    continue;
                }
                let m = &self.records[i].instance.model;
                let distance = Vec3::new(m[3][0], m[3][1], m[3][2]).distance(slot.eye);
                for (l, level) in levels.iter().enumerate() {
                    let kind = if shadow { level.shadows } else { level.camera };
                    if let Some(fade) = level_fade(level, distance).filter(|_| kind) {
                        let key = (v as u64) << 40
                            | (l as u64) << 32
                            | u64::from(fade != 0.0) << 31
                            | u64::from(self.frames[i]);
                        self.keys.push((key, i as u32, fade));
                    }
                }
            }
        }
        self.keys.sort_unstable_by_key(|&(key, i, _)| (key, i));
        self.out.clear();
        self.runs.clear();
        self.view_runs.clear();
        self.view_runs.resize(views, (0, 0));
        for &(key, i, fade) in &self.keys {
            let (v, l) = ((key >> 40) as u32, (key >> 32) as u32 & 0xff);
            let (dithered, f) = (key >> 31 & 1 != 0, key as u32 & 0x7fff_ffff);
            match self.runs.last_mut() {
                Some(run) if (run.0, run.1, run.2, run.5) == (v, l, f, dithered) => run.4 += 1,
                _ => self.runs.push((v, l, f, self.out.len() as u32, 1, dithered)),
            }
            self.out.push(self.records[i as usize].instance.with_fade(fade));
        }
        let mut start = 0;
        for (v, range) in self.view_runs.iter_mut().enumerate() {
            let end = start + self.runs[start..].iter().take_while(|r| r.0 == v as u32).count();
            *range = (start as u32, end as u32);
            start = end;
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let count = self.out.len() as u32;
        if count > self.instance_capacity || self.instances.is_none() {
            self.instance_capacity = count.max(64).next_power_of_two();
            self.instances = Some(device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("crowd instances"),
                size: u64::from(self.instance_capacity) * size_of::<Instance3D>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.instances
            && count > 0
        {
            gpu!(
                queue
                    .uploading(Upload::StaticBatches)
                    .write_buffer(buffer, 0, bytemuck::cast_slice(&self.out))
            );
        }
    }
}

#[cfg(test)]
impl Renderer3D {
    /// GPU objects made for crowds (see [`Crowds::gpu_objects`]).
    pub(crate) fn crowd_gpu_objects(&self) -> usize {
        self.crowds.gpu_objects()
    }

    /// Whether this frame runs the crowds' cull pass.
    pub(crate) fn crowds_cull(&self) -> bool {
        self.crowds.culls()
    }

    /// Whether crowd `id` takes the GPU path (`None`: no such crowd).
    pub(crate) fn crowd_on_gpu(&self, id: u32) -> Option<bool> {
        let crowd = self.crowds.crowds.get(id as usize)?.as_ref()?;
        Some(matches!(crowd.path, Path::Gpu(_)))
    }

    /// Global illumination's face direction views this frame (empty
    /// without probe updates), as `crowd_drawn` numbers views.
    pub(crate) fn crowd_gi_faces(&self) -> std::ops::Range<usize> {
        self.lists
            .gi_batch_slot
            .map_or(0..0, |gi| gi + 1..gi + crate::three::gi::GI_BATCH_FACES)
    }

    /// Crowd `id`'s clock as the last frame's shaders got it: seconds since
    /// its epoch, at that frame and at the one before.
    pub(crate) fn crowd_clock(&self, id: u32) -> Option<(f32, f32)> {
        let crowd = self.crowds.crowds.get(id as usize)?.as_ref()?;
        Some((crowd.elapsed, crowd.past_elapsed))
    }

    /// How many characters of crowd `id` the last frame drawn put in view
    /// `view` (summed over its levels; read back from the GPU on the GPU path).
    pub(crate) fn crowd_drawn(&self, device: &wgpu::Device, queue: &wgpu::Queue, id: u32, view: usize) -> Option<u32> {
        self.crowd_drawn_dithered(device, queue, id, view)
            .map(|(whole, dithered)| whole + dithered)
    }

    /// [`crowd_drawn`](Self::crowd_drawn) split into the characters drawn
    /// whole and those drawn dithered (crossfading between levels): a
    /// character crossfading counts once per level it is drawn in.
    pub(crate) fn crowd_drawn_dithered(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u32,
        view: usize,
    ) -> Option<(u32, u32)> {
        let crowd = self.crowds.crowds.get(id as usize)?.as_ref()?;
        let levels = crowd.levels.len();
        match &crowd.path {
            Path::Cpu(p) => {
                let &(start, end) = p.view_runs.get(view)?;
                let runs = &p.runs[start as usize..end as usize];
                let count = |dithered: bool| runs.iter().filter(|r| r.5 == dithered).map(|r| r.4).sum();
                Some((count(false), count(true)))
            }
            Path::Gpu(g) => {
                if view as u32 >= g.culled {
                    return Some((0, 0));
                }
                let bytes = (levels * 8) as u64;
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("crowd counts readback"),
                    size: bytes,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                encoder.copy_buffer_to_buffer(&g.draws, (view * levels * 8) as u64, &readback, 0, bytes);
                queue.submit([encoder.finish()]);
                readback.map_async(wgpu::MapMode::Read, .., |r| r.expect("crowd counts map"));
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("crowd counts readback");
                let counts: Vec<u32> = bytemuck::cast_slice(&readback.get_mapped_range(..).expect("mapped")).to_vec();
                // Two counts per level: whole, dithered.
                let whole = counts.iter().step_by(2).sum();
                let dithered = counts.iter().skip(1).step_by(2).sum();
                Some((whole, dithered))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(min: f32, max: f32, fade_out: f32, fade_in: f32) -> Level {
        Level {
            mesh: 0,
            min,
            max,
            camera: true,
            shadows: true,
            influences: 4,
            section: 0,
            slots: 0,
            fade_out,
            fade_in,
        }
    }

    #[test]
    fn neighbouring_levels_fade_with_complementary_shares() {
        // Near to 10 m fading over its last 2, far from 10 m fading in.
        let (near, far) = (band(0.0, 10.0, 2.0, 0.0), band(10.0, f32::INFINITY, 0.0, 2.0));
        assert_eq!(level_fade(&near, 5.0), Some(0.0), "whole before the fade");
        assert_eq!(level_fade(&far, 5.0), None, "not yet drawn");
        assert_eq!(level_fade(&near, 8.0), Some(0.0), "the fade's start is still whole");
        assert_eq!(level_fade(&far, 8.0), None, "and keeps none of the far level");
        for d in [8.3, 9.0, 9.5, 9.9] {
            let (a, b) = (level_fade(&near, d).unwrap(), level_fade(&far, d).unwrap());
            // Fading out keeps the pixels the fading-in level leaves.
            assert!(a < 0.0 && b > 0.0, "{d}: {a} {b}");
            assert!(
                (-a + b - 1.0).abs() < 1.5 / FADE_STEPS,
                "{d}: {a} + {b} cover each pixel once"
            );
            assert!((b - (d - 8.0) / 2.0).abs() <= 0.5 / FADE_STEPS, "{d}: {b} of the way");
        }
        assert_eq!(level_fade(&near, 10.0), None, "past its band");
        assert_eq!(level_fade(&far, 10.0), Some(0.0), "whole from its own band");
        // Without a fade, a plain switch.
        let plain = band(0.0, 10.0, 0.0, 0.0);
        assert_eq!(level_fade(&plain, 9.99), Some(0.0));
        assert_eq!(level_fade(&plain, 10.0), None);
    }

    #[test]
    fn a_last_level_with_a_fade_fades_out_to_nothing() {
        let last = band(0.0, 50.0, 10.0, 0.0);
        let shares: Vec<f32> = [40.0, 42.5, 45.0, 47.5, 49.999]
            .iter()
            .map(|&d| level_fade(&last, d).unwrap_or(f32::NAN))
            .collect();
        assert_eq!(shares[0], 0.0, "whole at the fade's start");
        assert!(
            shares[1..4].windows(2).all(|w| w[1] > w[0]),
            "{shares:?}: fewer pixels kept"
        );
        assert!(shares[4].is_nan(), "nothing kept at the end: {shares:?}");
    }

    #[test]
    fn crowd_shaders_compose_on_the_mesh_shader() {
        // A game shader that reads `items` and moves vertices with time.
        let game = "fn vertex(v: Vertex) -> Vertex {
            var out = v;
            out.position.y += sin(frame.time.x) * 0.01 + items[0].x * 0.0;
            return out;
        }";
        for user in ["", game] {
            for (decals, gi) in [(false, false), (true, false), (true, true)] {
                let drawn = super::super::vertex_motion::with_past(user);
                let source = compose_crowd(&drawn, decals, gi);
                assert!(source.contains("crowd_joints") && source.contains("fn static_instance"));
                assert_eq!(
                    source.matches("fn static_instance").count(),
                    1,
                    "the batches' helper is replaced"
                );
                assert_eq!(
                    (
                        source.matches("out.fade = crowd_fade(in);").count(),
                        source.matches("out.fade = in.m1.w;").count()
                    ),
                    (2, 0),
                    "the main and shadow vertex entry points take the crowd's dither"
                );
                for samples in [1, 4] {
                    super::super::shaders::check_shader(
                        "crowd material",
                        &super::super::shaders::for_samples(&source, samples, false),
                    )
                    .unwrap_or_else(|e| panic!("{e}"));
                }
            }
        }
        for bytes in [16 << 10, GRID_BYTES] {
            super::super::shaders::check_shader("crowd cull", &cull_source(bytes)).unwrap_or_else(|e| panic!("{e}"));
        }
        super::super::shaders::check_shader("crowd advance", ADVANCE).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn drawn_slots_are_the_camera_the_frames_shadow_views_and_targets() {
        let d = Drawn {
            cascades: 2,
            light_views: 1,
            targets: 1 + MAX_CASCADES + 1,
            gi: None,
        };
        let drawn: Vec<bool> = (0..d.targets + 2).map(|s| d.slot(s)).collect();
        let mut expected = vec![true, true, true];
        expected.extend(std::iter::repeat_n(false, MAX_CASCADES - 2));
        expected.extend([true, true, true]);
        assert_eq!(drawn, expected);
        assert!(!d.shadow(0) && d.shadow(1) && d.shadow(d.targets - 1) && !d.shadow(d.targets));
        assert!(d.gi_faces().is_empty());
        // Global illumination's sun shadow map is a shadow view, its face
        // directions are drawn and are not.
        let gi = d.targets + 2;
        let d = Drawn { gi: Some(gi), ..d };
        assert!(d.shadow(gi) && d.slot(gi));
        assert_eq!(d.gi_faces(), gi + 1..gi + 7);
        assert!(d.gi_faces().all(|s| d.slot(s) && !d.shadow(s)));
    }

    /// Where clip `clip` shows character `time` at `speed` after `t` more
    /// seconds, in baked frames (as `crowd_frames` in `crowd_vertex.wgsl`).
    fn shown(time: f32, speed: f32, t: f64, clip: [f32; 4]) -> f64 {
        let last = f64::from(clip[1] - 1.0);
        let p = (f64::from(time) + f64::from(speed) * t) * f64::from(clip[2]);
        if clip[3] > 0.5 {
            p.rem_euclid(last)
        } else {
            p.clamp(0.0, last)
        }
    }

    #[test]
    fn the_cull_shader_groups_frames_into_ranges_as_the_bake_does() {
        let line = format!("const RANGE_FRAMES: u32 = {}u;", crate::three::crowd::RANGE_FRAMES);
        assert!(CULL.contains(&line), "crowd_cull.wgsl needs `{line}`");
    }

    #[test]
    fn a_moved_clock_shows_the_same_poses_from_then_on() {
        // A looping clip of 31 frames at 30 a second (one second), and one
        // that holds.
        let looping = [0.0, 31.0, 30.0, 1.0];
        let held = [31.0, 31.0, 30.0, 0.0];
        for clip in [looping, held] {
            for (time, speed) in [(0.25, 1.0), (0.7, -0.5), (-3.0, 2.0), (5.0, -1.0), (0.4, 0.0)] {
                for seconds in [REBASE_SECONDS, 86_400.0] {
                    let moved = advance_time(time, speed, seconds, clip);
                    for later in [0.0, 0.01, 0.5, 3.0, 40.0] {
                        let (a, b) = (
                            shown(time, speed, seconds + later, clip),
                            shown(moved, speed, later, clip),
                        );
                        // On a loop the same pose is also one loop on.
                        let off = (a - b).abs();
                        let off = if clip[3] > 0.5 { off.min(30.0 - off) } else { off };
                        assert!(
                            off < 1e-3,
                            "clip {clip:?}, {time} s at {speed}x, moved {seconds} s: {a} against {b} frames after {later} s"
                        );
                    }
                    if clip[3] > 0.5 && speed != 0.0 {
                        assert!((0.0..=1.0).contains(&moved), "a loop's time stays in it: {moved}");
                    }
                }
            }
        }
    }
}
