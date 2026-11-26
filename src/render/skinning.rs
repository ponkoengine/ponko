//! Skinning on the GPU: a compute shader poses each skinned model drawn this
//! frame into a shared vertex buffer (a page, kept between frames) before
//! any pass draws. Models of one mesh lie next to each other in a page and
//! are posed by one dispatch (up to [`CHUNK`] at a time), so a crowd of a
//! thousand figures costs a handful of dispatches, not a thousand. Every
//! pass then draws a model like a plain mesh, from its page at its first
//! corner (`base_vertex`), so material shaders need nothing extra and the
//! shadow cascades reuse the one posed result. Morph targets (blend shapes)
//! are added in the same pass, before skinning; meshes with morph targets
//! but no joints go through it with one identity matrix. Where the GPU
//! can't run compute shaders (WebGL2), the CPU poses each model into a
//! buffer of its own instead.

use std::collections::HashMap;

use crate::render::traffic::{self, Counted, MAX_WRITES, NewBuffer, Synced, Upload};

use super::Vertex;
use crate::three::SkinFrame;

/// Corners per workgroup (`@workgroup_size` in skin.wgsl).
const GROUP: u32 = 64;
/// The most models one dispatch poses: the `array<Params, 512>` of
/// skin.wgsl, 16 KiB of uniforms (what every device binds).
const CHUNK: u32 = 512;
const SHADER: &str = include_str!("skin.wgsl");

/// What a skinned mesh keeps on the GPU besides its rest-pose vertices.
pub(super) struct GpuSkin {
    /// Per corner: 4 joints as u16 pairs, then 4 weights.
    influences: wgpu::Buffer,
    /// Its morph targets' offsets, target after target: per corner the
    /// position's (3 floats), then the normal's (3). A dummy without any.
    deltas: wgpu::Buffer,
    count: u32,
    /// How many morph targets it has.
    targets: u32,
    /// Without compute shaders: the rest pose, influences and offsets, on
    /// the CPU.
    cpu: Option<(Vec<Vertex>, Vec<Influence>, Vec<Delta>)>,
    /// How many skin matrices its corners use (one past the highest joint
    /// any corner weighs): the CPU path compares only those between frames.
    joints: u32,
    /// Tells it apart from every other skin made (also one made again under
    /// the same mesh id): the CPU path's comparison, and the key of the
    /// compute path's bind groups.
    id: u64,
}

/// The next `GpuSkin::id`.
static NEXT_SKIN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One corner's offset in one morph target (6 words in skin.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Delta {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Influence {
    pub(super) joints: [u16; 4],
    pub(super) weights: [f32; 4],
}

/// CPU path: where one skinned model's posed corners go.
struct Slot {
    buffer: wgpu::Buffer,
    /// Corners it holds.
    capacity: u32,
    /// What `buffer` was last posed from (the skin's id, the skin matrices
    /// its corners use, its morph weights). Posed from the same again, it
    /// already holds the corners: nothing is uploaded, so a held pose costs
    /// nothing.
    posed: Option<(u64, Vec<glam::Mat4>, Vec<f32>)>,
    /// With motion vectors on, while its pose moves: the same model posed
    /// as it was in the last frame (from `SkinFrame::previous`), which the
    /// motion-vector pass reads for where each corner was.
    previous: Option<Box<Slot>>,
}

/// Compute path: a vertex buffer many models are posed into, one after
/// another, and its twin for their last frame's poses (made while some
/// pose moves with motion vectors on), laid out the same.
struct Page {
    buffer: wgpu::Buffer,
    previous: Option<wgpu::Buffer>,
    /// Corners each buffer holds.
    capacity: u32,
    /// Changes whenever either buffer is made anew (bind groups name them).
    id: u64,
}

/// Compute path: one dispatch, posing `models` models of one mesh into one
/// page, their params from entry `param` on.
struct Chunk {
    /// The mesh's skin (`GpuSkin::id`) and the mesh.
    skin: u64,
    mesh: u32,
    page: u32,
    /// Its first `Params` entry, a multiple of the dynamic offset alignment.
    param: u32,
    models: u32,
    /// Corners per model.
    count: u32,
    /// Its bind groups for this frame's poses and the last frame's.
    groups: [Option<wgpu::BindGroup>; 2],
}

/// A compute-path bind group, kept while a frame uses it.
struct Group {
    /// The joints/weights/params generation and page it was made for.
    made_for: (u64, u64),
    /// The last frame that used it.
    used: u64,
    group: wgpu::BindGroup,
}

/// `Skinning::placed` of a model posed nowhere this frame.
const NOT_POSED: (u32, u32) = (u32::MAX, 0);

/// One model's entry in a dispatch (`Params` in skin.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    first: u32,
    count: u32,
    morph_first: u32,
    morph_count: u32,
    /// Where its posed corners start in its page, in corners.
    out_first: u32,
    /// 1 when its last frame's pose is posed too.
    moving: u32,
    pad: [u32; 2],
}

pub(super) struct Skinning {
    /// `None` without compute shaders.
    pipeline: Option<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    /// With `pipeline`: the one posing positions only, for the last
    /// frame's poses (the motion-vector pass reads nothing else).
    positions: Option<wgpu::ComputePipeline>,
    /// This frame's skin matrices, written a changed matrix at a time (a
    /// skeleton that holds its pose costs no upload).
    joints: Synced,
    /// This frame's morph weights, written a changed vec4 at a time.
    weights: Synced,
    /// The last frame's skin matrices and morph weights as this frame lays
    /// them out (`SkinFrame::previous`), for the poses motion vectors start
    /// from. Written only while some posed model moves with motion on.
    previous_joints: Synced,
    previous_weights: Synced,
    /// Bumped whenever `joints`, `weights`, their previous twins or
    /// `params` is made anew (bind groups name them).
    generation: u64,
    /// The dispatches' `Params`, each chunk's from an aligned entry on;
    /// only changed ones are written.
    params: Synced,
    /// The dynamic offset alignment, in `Params` entries.
    align: u32,
    /// Compute path: the pages, the most corners one holds (the device's
    /// storage binding and buffer limits), and the next `Page::id`.
    pages: Vec<Page>,
    page_limit: u32,
    page_ids: u64,
    /// Compute path, this frame: the dispatches, and per model its chunk
    /// and first corner in the chunk's page (`NOT_POSED`: none).
    chunks: Vec<Chunk>,
    placed: Vec<(u32, u32)>,
    /// Compute path: bind groups by (skin, page, last frame's pose), and
    /// the frame count that tells which were used.
    groups: HashMap<(u64, u32, bool), Group>,
    frame: u64,
    /// Compute path scratch: (skin, model) in the order they are posed,
    /// and the corners each page holds this frame.
    order: Vec<(u64, u32)>,
    needs: Vec<u32>,
    /// CPU path: one slot per model, collection indices kept stable even
    /// when earlier models are culled. Empty slots have no GPU allocation
    /// until a pass first draws them.
    slots: Vec<Option<Slot>>,
    /// This frame, per model: the skinned mesh, and whether any pass draws it.
    pub draws: Vec<(u32, bool)>,
    /// This frame, per model: whether its last frame's pose is posed too
    /// (into the page's or slot's twin), for motion vectors.
    moving: Vec<bool>,
    scratch: Vec<Vertex>,
    params_scratch: Vec<u8>,
    matrix_scratch: Vec<[f32; 16]>,
}

impl Skinning {
    /// Whether meshes are posed by the compute shader (else on the CPU).
    #[cfg(test)]
    pub fn uses_compute(&self) -> bool {
        self.pipeline.is_some()
    }

    pub fn new(device: &wgpu::Device, compute: bool) -> Self {
        let pipeline = compute.then(|| {
            let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("3d skinning layout"),
                entries: &[
                    storage(0, true),
                    storage(1, true),
                    storage(2, true),
                    storage(3, false),
                    storage(5, true),
                    storage(6, true),
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: wgpu::BufferSize::new(CHUNK_BYTES),
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("3d skinning pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("3d skinning"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
            let pipeline = |label, entry_point| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    cache: None,
                })
            };
            (
                (pipeline("3d skinning", "skin"), layout),
                pipeline("3d previous poses", "skin_positions"),
            )
        });
        let (pipeline, positions) = pipeline.unzip();
        let limits = device.limits();
        let entry = size_of::<Params>() as u32;
        let align = limits.min_uniform_buffer_offset_alignment.max(entry) / entry;
        // A page is bound whole as storage; `base_vertex` is an i32.
        let most = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
        let page_limit = (most / size_of::<Vertex>() as u64).min(i32::MAX as u64) as u32;
        let storage = wgpu::BufferUsages::STORAGE;
        Self {
            pipeline,
            positions,
            joints: Synced::new(
                device,
                "3d skin matrices",
                storage,
                64 * 64,
                Upload::Skins,
                (64, 4, MAX_WRITES),
            ),
            weights: Synced::new(
                device,
                "3d morph weights",
                storage,
                64 * 4,
                Upload::Skins,
                (16, 8, MAX_WRITES),
            ),
            previous_joints: Synced::new(
                device,
                "3d previous skin matrices",
                storage,
                64 * 64,
                Upload::Skins,
                (64, 4, MAX_WRITES),
            ),
            previous_weights: Synced::new(
                device,
                "3d previous morph weights",
                storage,
                64 * 4,
                Upload::Skins,
                (16, 8, MAX_WRITES),
            ),
            generation: 0,
            params: Synced::new(
                device,
                "3d skinning params",
                wgpu::BufferUsages::UNIFORM,
                CHUNK_BYTES as usize,
                Upload::Skins,
                (size_of::<Params>(), 0, MAX_WRITES),
            ),
            align,
            pages: Vec::new(),
            page_limit,
            page_ids: 0,
            chunks: Vec::new(),
            placed: Vec::new(),
            groups: HashMap::new(),
            frame: 0,
            order: Vec::new(),
            needs: Vec::new(),
            slots: Vec::new(),
            draws: Vec::new(),
            moving: Vec::new(),
            scratch: Vec::new(),
            params_scratch: Vec::new(),
            matrix_scratch: Vec::new(),
        }
    }

    /// Usage the rest-pose vertex buffer of a skinned mesh needs.
    pub fn rest_usage(&self) -> wgpu::BufferUsages {
        if self.pipeline.is_some() {
            wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::empty()
        }
    }

    /// The GPU side of a skinned mesh of `vertices`, or of one with morph
    /// targets: `deltas` holds `vertices.len()` offsets per target, target
    /// after target. Without `joints`, every corner follows matrix 0.
    pub fn create(
        &self,
        device: &wgpu::Device,
        vertices: &[Vertex],
        joints: &[[u16; 4]],
        weights: &[[f32; 4]],
        deltas: Vec<Delta>,
    ) -> GpuSkin {
        let influences: Vec<Influence> = if joints.is_empty() {
            let still = Influence {
                joints: [0; 4],
                weights: [1.0, 0.0, 0.0, 0.0],
            };
            vec![still; vertices.len()]
        } else {
            joints
                .iter()
                .zip(weights)
                .map(|(&joints, &weights)| Influence { joints, weights })
                .collect()
        };
        let buffer = device
            .uploading(Upload::Skins)
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3d skin influences"),
                contents: bytemuck::cast_slice(&influences),
                // Crowds read them as a vertex buffer (see `crowds.rs`).
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            });
        let targets = deltas.len().checked_div(vertices.len()).unwrap_or(0) as u32;
        // As `blend_skin` reads them: a joint counts where its weight is
        // above 0.
        let joints = influences
            .iter()
            .flat_map(|i| {
                (0..4)
                    .filter(|&k| i.weights[k] > 0.0)
                    .map(|k| u32::from(i.joints[k]) + 1)
            })
            .max()
            .unwrap_or(0);
        let dummy = [Delta::default()];
        let delta_buffer = device
            .uploading(Upload::Skins)
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("3d morph targets"),
                contents: bytemuck::cast_slice(if deltas.is_empty() { &dummy[..] } else { &deltas[..] }),
                usage: wgpu::BufferUsages::STORAGE,
            });
        GpuSkin {
            influences: buffer,
            deltas: delta_buffer,
            count: vertices.len() as u32,
            targets,
            joints,
            id: NEXT_SKIN.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            cpu: self.pipeline.is_none().then(|| (vertices.to_vec(), influences, deltas)),
        }
    }

    /// Gets this frame's skinned models ready: their skin matrices, pages
    /// and dispatches (or, on the CPU path, their posed corners). `mesh(id)`
    /// gives a skinned mesh's rest-pose vertices and skin; `used(k)` whether
    /// a pass draws model `k`. With `motion` (motion vectors drawn this
    /// frame), the models whose pose changed are posed a second time as
    /// they were in the last frame.
    pub fn prepare<'m>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        skins: &SkinFrame,
        mesh: impl Fn(u32) -> Option<(&'m wgpu::Buffer, &'m GpuSkin)>,
        used: impl Fn(usize) -> bool,
        motion: bool,
    ) {
        self.draws.clear();
        self.draws
            .extend(skins.draws.iter().enumerate().map(|(k, d)| (d.mesh, used(k))));
        self.chunks.clear();
        self.placed.clear();
        // The last frame's poses are laid out as this frame's.
        let history = motion
            && skins.previous.len() == skins.matrices.len()
            && skins.previous_weights.len() == skins.weights.len();
        self.moving.clear();
        self.moving.extend(
            skins
                .draws
                .iter()
                .zip(&self.draws)
                .map(|(d, &(_, used))| history && used && d.posed_motion),
        );
        if self.draws.iter().all(|d| !d.1) {
            return;
        }
        if self.pipeline.is_some() {
            self.prepare_compute(device, queue, skins, &mesh);
            return;
        }
        // The CPU path reads no params, but reserves a record per model as
        // it always has: its buffers are counted and reported as before.
        let record = u64::from(self.align) * size_of::<Params>() as u64;
        self.params.reserve(device, self.draws.len() * record as usize);
        if self.slots.len() < skins.draws.len() {
            self.slots.resize_with(skins.draws.len(), || None);
        }
        let now = (&skins.matrices[..], &skins.weights[..]);
        let before = (&skins.previous[..], &skins.previous_weights[..]);
        for (k, d) in skins.draws.iter().enumerate() {
            if !self.draws[k].1 {
                continue;
            }
            let Some((_, skin)) = mesh(d.mesh) else {
                continue;
            };
            let p = Params {
                first: d.first,
                count: skin.count,
                morph_first: d.morph_first,
                morph_count: d.morph_count.min(skin.targets),
                ..Params::default()
            };
            let slot = self.slots[k].get_or_insert_with(|| Slot::new(device, skin.count));
            slot.fit(device, skin.count);
            pose_on_cpu(queue, slot, &p, skin, now, &mut self.scratch);
            if self.moving[k] {
                let previous = slot
                    .previous
                    .get_or_insert_with(|| Box::new(Slot::new(device, skin.count)));
                previous.fit(device, skin.count);
                pose_on_cpu(queue, previous, &p, skin, before, &mut self.scratch);
            }
        }
    }

    /// The compute path of [`prepare`](Self::prepare): the skin matrices and
    /// morph weights uploaded, the drawn models laid out in pages (models
    /// of one mesh next to each other) and their dispatches listed.
    fn prepare_compute<'m>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        skins: &SkinFrame,
        mesh: &impl Fn(u32) -> Option<(&'m wgpu::Buffer, &'m GpuSkin)>,
    ) {
        let any_moving = self.moving.iter().any(|&m| m);
        // Bind groups name the joints and weights: new ones need new groups.
        let matrices = skins.matrices.len().max(1);
        for (buffer, len, live) in [
            (&mut self.joints, matrices * 64, true),
            (&mut self.weights, skins.weights.len() * 4, true),
            (&mut self.previous_joints, matrices * 64, any_moving),
            (&mut self.previous_weights, skins.weights.len() * 4, any_moving),
        ] {
            if live && buffer.reserve(device, len) {
                self.generation += 1;
            }
        }
        let pairs = [
            (
                &mut self.joints,
                &mut self.weights,
                &skins.matrices,
                &skins.weights,
                true,
            ),
            (
                &mut self.previous_joints,
                &mut self.previous_weights,
                &skins.previous,
                &skins.previous_weights,
                any_moving,
            ),
        ];
        for (joints, weights, m, w, live) in pairs {
            if live && !m.is_empty() {
                self.matrix_scratch.clear();
                self.matrix_scratch.extend(m.iter().map(glam::Mat4::to_cols_array));
                joints.write(queue, 0, bytemuck::cast_slice(&self.matrix_scratch));
            }
            if live && !w.is_empty() {
                weights.write(queue, 0, bytemuck::cast_slice(w));
            }
        }

        // Models of one mesh next to each other, so one dispatch poses many.
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        for (k, d) in skins.draws.iter().enumerate() {
            if let Some((_, skin)) = mesh(d.mesh).filter(|_| self.draws[k].1) {
                order.push((skin.id, k as u32));
            }
        }
        order.sort_unstable();
        self.placed.resize(skins.draws.len(), NOT_POSED);
        self.needs.clear();
        self.params_scratch.clear();
        let entry = size_of::<Params>();
        let (mut page, mut fill, mut param) = (0u32, 0u32, 0u32);
        for &(id, k) in &order {
            let d = &skins.draws[k as usize];
            let Some((_, skin)) = mesh(d.mesh).filter(|(_, s)| s.count > 0) else {
                continue;
            };
            let count = skin.count;
            if fill > 0 && u64::from(fill) + u64::from(count) > u64::from(self.page_limit) {
                page += 1;
                fill = 0;
            }
            let joins = self
                .chunks
                .last()
                .is_some_and(|c| c.skin == id && c.page == page && c.models < CHUNK);
            if !joins {
                param = param.next_multiple_of(self.align);
                self.chunks.push(Chunk {
                    skin: id,
                    mesh: d.mesh,
                    page,
                    param,
                    models: 0,
                    count,
                    groups: [None, None],
                });
            }
            let p = Params {
                first: d.first,
                count,
                morph_first: d.morph_first,
                morph_count: d.morph_count.min(skin.targets),
                out_first: fill,
                moving: u32::from(self.moving[k as usize]),
                pad: [0; 2],
            };
            let at = param as usize * entry;
            self.params_scratch.resize(at + entry, 0);
            self.params_scratch[at..].copy_from_slice(bytemuck::bytes_of(&p));
            let chunk = self.chunks.len() - 1;
            self.chunks[chunk].models += 1;
            param += 1;
            self.placed[k as usize] = (chunk as u32, fill);
            fill += count;
            if self.needs.len() <= page as usize {
                self.needs.resize(page as usize + 1, 0);
            }
            self.needs[page as usize] = fill;
        }
        self.order = order;

        // Each dispatch binds `CHUNK` entries from its first: the buffer
        // reaches that far past the last one.
        let bytes = self.chunks.last().map_or(0, |c| (c.param + CHUNK) as usize * entry);
        self.params_scratch.resize(bytes, 0);
        if self.params.reserve(device, bytes) {
            self.generation += 1;
        }
        // Past the most the device holds (reported), skins stay at rest.
        let fits = self.params.fits();
        while self
            .chunks
            .last()
            .is_some_and(|c| (c.param + CHUNK) as usize * entry > fits)
        {
            self.chunks.pop();
        }
        for (k, placed) in self.placed.iter_mut().enumerate() {
            if placed.0 != NOT_POSED.0 && placed.0 as usize >= self.chunks.len() {
                *placed = NOT_POSED;
            }
            if *placed == NOT_POSED {
                self.draws[k].1 = false;
                self.moving[k] = false;
            }
        }
        let any_moving = self.moving.iter().any(|&m| m);
        let written = bytes.min(fits);
        self.params.write(queue, 0, &self.params_scratch[..written]);

        // Pages big enough for what they hold, and their twins for motion.
        for (p, &need) in self.needs.iter().enumerate() {
            if self.pages.len() <= p {
                self.page_ids += 1;
                self.pages.push(Page::new(device, need, self.page_limit, self.page_ids));
            }
            let page = &mut self.pages[p];
            if page.capacity < need {
                traffic::reallocated();
                self.page_ids += 1;
                *page = Page::new(device, need, self.page_limit, self.page_ids);
            }
            if any_moving && page.previous.is_none() {
                page.previous = Some(page_buffer(device, page.capacity));
                self.page_ids += 1;
                page.id = self.page_ids;
            }
        }

        // Bind groups: one per mesh and page (and twin), kept while used.
        self.frame += 1;
        let Some((_, layout)) = &self.pipeline else {
            return;
        };
        for c in &mut self.chunks {
            let Some((rest, skin)) = mesh(c.mesh) else {
                continue;
            };
            let page = &self.pages[c.page as usize];
            for (i, twin) in [false, true].into_iter().enumerate() {
                let (target, joints, weights) = if twin {
                    let Some(previous) = page.previous.as_ref().filter(|_| any_moving) else {
                        continue;
                    };
                    (previous, &self.previous_joints, &self.previous_weights)
                } else {
                    (&page.buffer, &self.joints, &self.weights)
                };
                let made_for = (self.generation, page.id);
                let group = self
                    .groups
                    .entry((c.skin, c.page, twin))
                    .and_modify(|g| {
                        if g.made_for != made_for {
                            g.group = bind_group(device, layout, rest, skin, joints, weights, target, &self.params);
                            g.made_for = made_for;
                        }
                    })
                    .or_insert_with(|| Group {
                        made_for,
                        used: 0,
                        group: bind_group(device, layout, rest, skin, joints, weights, target, &self.params),
                    });
                group.used = self.frame;
                c.groups[i] = Some(group.group.clone());
            }
        }
        let frame = self.frame;
        self.groups.retain(|_, g| g.used == frame);
    }

    /// Poses this frame's skinned models (compute path only).
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let Some((pipeline, _)) = &self.pipeline else {
            return;
        };
        if self.chunks.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("3d skinning"),
            timestamp_writes: timestamps,
        });
        let entry = size_of::<Params>() as u32;
        pass.set_pipeline(pipeline);
        for c in &self.chunks {
            if let Some(group) = &c.groups[0] {
                pass.set_bind_group(0, group, &[c.param * entry]);
                pass.dispatch_workgroups(c.count.div_ceil(GROUP), c.models, 1);
            }
        }
        // The last frame's poses of the models whose pose changed, for
        // motion vectors: positions only (the shader skips the others).
        let Some(positions) = &self.positions else {
            return;
        };
        let mut set = false;
        for c in &self.chunks {
            if let Some(group) = &c.groups[1] {
                if !set {
                    pass.set_pipeline(positions);
                    set = true;
                }
                pass.set_bind_group(0, group, &[c.param * entry]);
                pass.dispatch_workgroups(c.count.div_ceil(GROUP), c.models, 1);
            }
        }
    }

    /// See `Renderer3D::check_mirrors`.
    #[cfg(test)]
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        for buffer in [
            &self.joints,
            &self.weights,
            &self.previous_joints,
            &self.previous_weights,
            &self.params,
        ] {
            buffer.check(device, queue);
        }
    }

    /// Whether this frame poses anything on the GPU.
    pub fn has_work(&self) -> bool {
        self.pipeline.is_some() && !self.chunks.is_empty()
    }

    /// Skinned model `k` posed as it was in the last frame, if it's drawn
    /// this frame and its pose changed since (with motion on): the buffer,
    /// and the model's first corner in it (laid out as [`vertices`](Self::vertices)).
    pub fn previous_vertices(&self, k: usize) -> Option<(&wgpu::Buffer, i32)> {
        if !self.moving.get(k).copied().unwrap_or(false) {
            return None;
        }
        if self.pipeline.is_none() {
            return Some((&self.slots.get(k)?.as_ref()?.previous.as_ref()?.buffer, 0));
        }
        let (page, base) = self.page_of(k)?;
        Some((page.previous.as_ref()?, base))
    }

    /// Skinned model `k`'s posed vertices, if it's drawn this frame: the
    /// buffer, and the model's first corner in it (the draw's `base_vertex`).
    pub fn vertices(&self, k: usize) -> Option<(&wgpu::Buffer, i32)> {
        let &(_, used) = self.draws.get(k)?;
        if !used {
            return None;
        }
        if self.pipeline.is_none() {
            return self.slots.get(k).and_then(Option::as_ref).map(|s| (&s.buffer, 0));
        }
        let (page, base) = self.page_of(k)?;
        Some((&page.buffer, base))
    }

    /// Compute path: the page model `k` is posed into this frame, and its
    /// first corner there.
    fn page_of(&self, k: usize) -> Option<(&Page, i32)> {
        let &(chunk, base) = self.placed.get(k)?;
        let c = self.chunks.get(chunk as usize)?;
        c.groups[0].as_ref()?;
        Some((self.pages.get(c.page as usize)?, i32::try_from(base).ok()?))
    }
}

/// `CHUNK` params, the size of a dispatch's uniform binding.
const CHUNK_BYTES: u64 = CHUNK as u64 * size_of::<Params>() as u64;

impl Slot {
    fn new(device: &wgpu::Device, count: u32) -> Self {
        Self {
            buffer: slot_buffer(device, count.max(1)),
            capacity: count.max(1),
            posed: None,
            previous: None,
        }
    }

    /// Makes room for `count` corners.
    fn fit(&mut self, device: &wgpu::Device, count: u32) {
        if self.capacity < count {
            self.capacity = count.next_power_of_two();
            traffic::reallocated();
            self.buffer = slot_buffer(device, self.capacity);
            self.posed = None;
        }
    }
}

impl Page {
    /// A page for at least `need` corners: the next power of two, up to
    /// `limit` (a single model past it gets just its own size).
    fn new(device: &wgpu::Device, need: u32, limit: u32, id: u64) -> Self {
        let capacity = need.max(64).next_power_of_two().min(limit).max(need);
        Self {
            buffer: page_buffer(device, capacity),
            previous: None,
            capacity,
            id,
        }
    }
}

/// The compute shader's bind group for one mesh posed into `target`.
#[allow(clippy::too_many_arguments)]
fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    rest: &wgpu::Buffer,
    skin: &GpuSkin,
    joints: &Synced,
    weights: &Synced,
    target: &wgpu::Buffer,
    params: &Synced,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("3d skinning"),
        layout,
        entries: &[
            entry(0, rest),
            entry(1, &skin.influences),
            entry(2, joints.buffer()),
            entry(3, target),
            entry(5, weights.buffer()),
            entry(6, &skin.deltas),
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: params.buffer(),
                    offset: 0,
                    size: wgpu::BufferSize::new(CHUNK_BYTES),
                }),
            },
        ],
    })
}

/// CPU path: poses one model into `slot` from `(matrices, weights)`,
/// uploading the corners only when what they are posed from changed.
fn pose_on_cpu(
    queue: &wgpu::Queue,
    slot: &mut Slot,
    p: &Params,
    skin: &GpuSkin,
    (matrices, weights): (&[glam::Mat4], &[f32]),
    scratch: &mut Vec<Vertex>,
) {
    let Some((vertices, influences, deltas)) = &skin.cpu else {
        return;
    };
    let skin_matrices = matrices.get(p.first as usize..).unwrap_or(&[]);
    let morph = weights
        .get(p.morph_first as usize..(p.morph_first + p.morph_count) as usize)
        .unwrap_or(&[]);
    // The posed corners depend only on these (joints past the slice's end
    // fall back to identity either way).
    let used = &skin_matrices[..(skin.joints as usize).min(skin_matrices.len())];
    if matches!(&slot.posed, Some((id, m, w)) if *id == skin.id && m[..] == *used && w[..] == *morph) {
        return;
    }
    let corners = vertices.len();
    scratch.clear();
    scratch.extend(vertices.iter().zip(influences).enumerate().map(|(c, (v, i))| {
        let (mut p, mut n) = (glam::Vec3::from(v.position), glam::Vec3::from(v.normal));
        for (t, &w) in morph.iter().enumerate() {
            if w != 0.0 {
                let delta = &deltas[t * corners + c];
                p += w * glam::Vec3::from(delta.position);
                n += w * glam::Vec3::from(delta.normal);
            }
        }
        let m = crate::three::blend_skin(skin_matrices, i.joints, i.weights);
        let p = m.transform_point3(p);
        let n = m.transform_vector3(n).normalize_or(glam::Vec3::Y);
        Vertex {
            position: p.to_array(),
            normal: n.to_array(),
            ..*v
        }
    }));
    queue
        .uploading(Upload::Skins)
        .write_buffer(&slot.buffer, 0, bytemuck::cast_slice(scratch));
    let (id, m, w) = slot.posed.get_or_insert_with(Default::default);
    *id = skin.id;
    m.clear();
    m.extend_from_slice(used);
    w.clear();
    w.extend_from_slice(morph);
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

/// A buffer of `corners` posed vertices: a page (with `storage`), or a
/// CPU-path slot.
fn posed_buffer(device: &wgpu::Device, corners: u32, storage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.new_buffer(&wgpu::BufferDescriptor {
        label: Some("3d posed vertices"),
        size: u64::from(corners) * size_of::<Vertex>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC | storage,
        mapped_at_creation: false,
    })
}

fn page_buffer(device: &wgpu::Device, corners: u32) -> wgpu::Buffer {
    posed_buffer(device, corners, wgpu::BufferUsages::STORAGE)
}

fn slot_buffer(device: &wgpu::Device, corners: u32) -> wgpu::Buffer {
    posed_buffer(device, corners, wgpu::BufferUsages::empty())
}

impl GpuSkin {
    /// Per corner its 4 joints (`Uint16x4`) and weights (`Float32x4`), 24
    /// bytes a corner: what crowds skin each corner from (see `crowds.rs`).
    pub fn influences(&self) -> &wgpu::Buffer {
        &self.influences
    }

    /// Without compute shaders: the rest-pose corners and their influences,
    /// on the CPU (crowds pose them there; see `crowds.rs`).
    pub fn rest_on_cpu(&self) -> Option<(&[Vertex], &[Influence])> {
        self.cpu.as_ref().map(|(v, i, _)| (&v[..], &i[..]))
    }

    /// Counts a mesh's influences and morph deltas as skinning (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Skinning, "skin influences", &self.influences);
        t.buffer(C::Skinning, "morph deltas", &self.deltas);
    }
}

impl Skinning {
    /// Counts the joint matrices, morph weights and posed vertices (pages
    /// or slots, this frame's and the last) as skinning (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        for (label, synced) in [
            ("joint matrices", &self.joints),
            ("morph weights", &self.weights),
            ("previous joint matrices", &self.previous_joints),
            ("previous morph weights", &self.previous_weights),
            ("skinning params", &self.params),
        ] {
            t.buffer(C::Skinning, label, synced.buffer());
        }
        for page in &self.pages {
            t.buffer(C::Skinning, "posed vertices", &page.buffer);
            if let Some(previous) = &page.previous {
                t.buffer(C::Skinning, "posed vertices", previous);
            }
        }
        for slot in self.slots.iter().flatten() {
            let mut slot = Some(slot);
            while let Some(s) = slot {
                t.buffer(C::Skinning, "posed vertices", &s.buffer);
                slot = s.previous.as_deref();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_skinning_shader_is_valid() {
        super::super::check_shader("skin.wgsl", super::SHADER).unwrap();
    }

    /// Shapes (two morph targets) and poses three corners on the GPU (when
    /// there is one), with the compute shader and with the CPU fallback:
    /// both must agree with the CPU's math.
    #[test]
    fn gpu_skinning_matches_the_cpu() {
        posed_corners_match(true);
        posed_corners_match(false);
    }

    fn posed_corners_match(compute: bool) {
        use glam::{Mat4, Quat, Vec3};
        use wgpu::util::DeviceExt;
        let Some(crate::render::render_test_support::TestGpu { device, queue, .. }) =
            crate::render::render_test_support::gpu()
        else {
            return;
        };
        // The compute pass binds six storage buffers (the renderer's own check).
        if compute && !crate::render::render_test_support::needs(&device, crate::render::GpuFeature::GpuSkinning) {
            return;
        }
        let skinning = &mut super::Skinning::new(&device, compute);
        let vertex = |p: [f32; 3]| super::Vertex {
            position: p,
            normal: [0.0, 0.0, 1.0],
            uv: [0.25, 0.5],
            color: [1, 2, 3, 4],
            uv1: [7.0, 8.0],
        };
        let rest = [
            vertex([0.0, 0.0, 0.0]),
            vertex([1.0, 0.0, 0.0]),
            vertex([0.0, 1.0, 0.0]),
        ];
        let joints = [[0u16, 0, 0, 0], [0, 1, 0, 0], [1, 0, 0, 0]];
        let weights = [[1.0f32, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];
        // Target 0 lifts corner 1, target 1 pushes corner 2 along x and
        // tilts its normal.
        let delta = |p: [f32; 3], n: [f32; 3]| super::Delta { position: p, normal: n };
        let none = delta([0.0; 3], [0.0; 3]);
        let deltas = vec![
            none,
            delta([0.0, 2.0, 0.0], [0.0; 3]),
            none,
            none,
            none,
            delta([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ];
        let skin = skinning.create(&device, &rest, &joints, &weights, deltas.clone());
        let rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&rest),
            usage: wgpu::BufferUsages::VERTEX | skinning.rest_usage(),
        });
        let mut frame = crate::three::SkinFrame::default();
        // Another model's matrix first, so `first` matters.
        frame.matrices = vec![
            Mat4::from_scale(Vec3::splat(9.0)),
            Mat4::from_translation(Vec3::new(0.0, 0.0, 2.0)),
            Mat4::from_rotation_translation(Quat::from_rotation_z(1.0), Vec3::X),
        ];
        frame.draws.push(crate::three::SkinDraw {
            mesh: 0,
            instance: Default::default(),
            entity: 0,
            matrix: Mat4::IDENTITY,
            scale: 1.0,
            bounds: (Vec3::ZERO, 1.0),
            first: 1,
            morph_first: 1,
            morph_count: 2,
            material: u32::MAX,
            parts: (0, 0),
            lod: u32::MAX,
            posed_motion: false,
        });
        // Another model's weight first, so `morph_first` matters.
        frame.weights = vec![9.0, 0.5, 0.25];
        skinning.prepare(
            &device,
            &queue,
            &frame,
            |_| Some((&rest_buffer, &skin)),
            |_| true,
            false,
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        skinning.record(&mut encoder, None);
        let size = (rest.len() * size_of::<super::Vertex>()) as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (posed, base) = skinning.vertices(0).unwrap();
        let at = base as u64 * size_of::<super::Vertex>() as u64;
        encoder.copy_buffer_to_buffer(posed, at, &readback, 0, size);
        queue.submit([encoder.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let posed: Vec<super::Vertex> = bytemuck::cast_slice(&readback.slice(..).get_mapped_range().unwrap()).to_vec();
        for i in 0..3 {
            let m = crate::three::blend_skin(&frame.matrices[1..], joints[i], weights[i]);
            let shaped = Vec3::from(rest[i].position)
                + 0.5 * Vec3::from(deltas[i].position)
                + 0.25 * Vec3::from(deltas[3 + i].position);
            let want = m.transform_point3(shaped);
            assert!(
                Vec3::from(posed[i].position).abs_diff_eq(want, 1e-5),
                "compute {compute}, corner {i}: {:?} vs {want}",
                posed[i].position
            );
            let n = Vec3::from(posed[i].normal);
            let shaped_n = Vec3::from(rest[i].normal) + 0.25 * Vec3::from(deltas[3 + i].normal);
            let want_n = m.transform_vector3(shaped_n).normalize();
            assert!(n.abs_diff_eq(want_n, 1e-5), "corner {i}'s normal: {n} vs {want_n}");
            assert_eq!(
                (posed[i].uv, posed[i].color, posed[i].uv1),
                (rest[i].uv, rest[i].color, rest[i].uv1)
            );
        }
    }

    /// A crowd: many models of two meshes, interleaved, posed by a few
    /// dispatches (one per mesh and [`super::CHUNK`] models) into shared
    /// pages; every model's corners come out where `vertices` says, as the
    /// CPU's math puts them, also when the pages are small enough that
    /// models spill into several, and with the last frame's poses posed
    /// for the models that moved.
    #[test]
    fn a_crowd_is_posed_by_few_dispatches_into_shared_pages() {
        use glam::{Mat4, Quat, Vec3};
        use wgpu::util::DeviceExt;
        let Some(crate::render::render_test_support::TestGpu { device, queue, .. }) =
            crate::render::render_test_support::gpu()
        else {
            return;
        };
        if !crate::render::render_test_support::needs(&device, crate::render::GpuFeature::GpuSkinning) {
            return;
        }
        let vertex = |p: [f32; 3]| super::Vertex {
            position: p,
            normal: [0.0, 0.0, 1.0],
            uv: [0.5, 0.25],
            color: [5, 6, 7, 8],
            uv1: [1.0, 2.0],
        };
        // Mesh 0: three corners on two joints; mesh 1: four on one.
        let rests = [
            vec![vertex([0.0; 3]), vertex([1.0, 0.0, 0.0]), vertex([0.0, 1.0, 0.0])],
            vec![
                vertex([0.0; 3]),
                vertex([0.0, 0.0, 1.0]),
                vertex([1.0, 1.0, 0.0]),
                vertex([0.5, 0.5, 0.5]),
            ],
        ];
        let joints = [
            vec![[0u16, 0, 0, 0], [0, 1, 0, 0], [1, 0, 0, 0]],
            vec![[0u16, 0, 0, 0]; 4],
        ];
        let weights = [
            vec![[1.0f32, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]],
            vec![[1.0f32, 0.0, 0.0, 0.0]; 4],
        ];
        // More models of each mesh than one dispatch takes.
        let models = 2 * super::CHUNK as usize + 300;
        let dispatches: usize = (0..2)
            .map(|m| {
                (0..models)
                    .filter(|k| k % 2 == m && k % 5 != 4)
                    .count()
                    .div_ceil(super::CHUNK as usize)
            })
            .sum();
        assert_eq!(dispatches, 4);
        for (page_limit, moving) in [(None, false), (Some(10), true)] {
            let mut skinning = super::Skinning::new(&device, true);
            if let Some(limit) = page_limit {
                skinning.page_limit = limit;
            }
            let skins: Vec<_> = (0..2)
                .map(|m| skinning.create(&device, &rests[m], &joints[m], &weights[m], Vec::new()))
                .collect();
            let rest_buffers: Vec<_> = rests
                .iter()
                .map(|r| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(r),
                        usage: wgpu::BufferUsages::VERTEX | skinning.rest_usage(),
                    })
                })
                .collect();
            let mut frame = crate::three::SkinFrame::default();
            let pose = |k: usize, t: f32| {
                [
                    Mat4::from_translation(Vec3::new(k as f32, t, 0.0)),
                    Mat4::from_rotation_translation(Quat::from_rotation_z(k as f32 * 0.01 + t), Vec3::Z),
                ]
            };
            for k in 0..models {
                frame.draws.push(crate::three::SkinDraw {
                    mesh: (k % 2) as u32,
                    instance: Default::default(),
                    entity: k as u64,
                    matrix: Mat4::IDENTITY,
                    scale: 1.0,
                    bounds: (Vec3::ZERO, 1.0),
                    first: frame.matrices.len() as u32,
                    morph_first: 0,
                    morph_count: 0,
                    material: u32::MAX,
                    parts: (0, 0),
                    lod: u32::MAX,
                    posed_motion: moving && k % 3 == 0,
                });
                frame.matrices.extend(pose(k, 0.0));
                frame.previous.extend(pose(k, 1.0));
            }
            // Every fifth model is drawn by no pass: it is posed nowhere.
            skinning.prepare(
                &device,
                &queue,
                &frame,
                |m| Some((&rest_buffers[m as usize], &skins[m as usize])),
                |k| k % 5 != 4,
                moving,
            );
            if page_limit.is_none() {
                // Per mesh: a full dispatch, and one for the rest.
                assert_eq!(skinning.chunks.len(), dispatches, "dispatches");
                assert_eq!(skinning.pages.len(), 1);
            } else {
                assert!(skinning.pages.len() > 100, "{} pages", skinning.pages.len());
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            skinning.record(&mut encoder, None);
            queue.submit([encoder.finish()]);
            let read = |buffer: &wgpu::Buffer| -> Vec<super::Vertex> {
                bytemuck::cast_slice(&crate::render::traffic::read_back(
                    &device,
                    &queue,
                    buffer,
                    0,
                    buffer.size(),
                ))
                .to_vec()
            };
            let pages: Vec<_> = skinning
                .pages
                .iter()
                .map(|p| (read(&p.buffer), p.previous.as_ref().map(read)))
                .collect();
            let page_of = |buffer: &wgpu::Buffer| {
                skinning
                    .pages
                    .iter()
                    .position(|p| &p.buffer == buffer || p.previous.as_ref() == Some(buffer))
                    .unwrap()
            };
            for k in 0..models {
                let m = k % 2;
                let Some((buffer, base)) = skinning.vertices(k) else {
                    assert_eq!(k % 5, 4, "model {k} is drawn but not posed");
                    continue;
                };
                assert_ne!(k % 5, 4, "model {k} is drawn by no pass");
                let p = page_of(buffer);
                let check = |posed: &[super::Vertex], matrices: &[Mat4; 2], what: &str| {
                    for (i, r) in rests[m].iter().enumerate() {
                        let got = posed[base as usize + i];
                        let want = crate::three::blend_skin(matrices, joints[m][i], weights[m][i])
                            .transform_point3(Vec3::from(r.position));
                        assert!(
                            Vec3::from(got.position).abs_diff_eq(want, 1e-4),
                            "{what} model {k} corner {i}: {:?} vs {want}",
                            got.position
                        );
                    }
                };
                check(&pages[p].0, &pose(k, 0.0), "posed");
                assert_eq!(pages[p].0[base as usize].uv, [0.5, 0.25]);
                match skinning.previous_vertices(k) {
                    Some((previous, previous_base)) => {
                        assert!(moving && k % 3 == 0, "model {k} has a last pose");
                        assert_eq!(previous_base, base, "laid out as this frame's");
                        assert_eq!(page_of(previous), p);
                        check(pages[p].1.as_ref().unwrap(), &pose(k, 1.0), "previous");
                    }
                    None => assert!(!(moving && k % 3 == 0), "model {k} moved"),
                }
            }
        }
    }

    /// Without compute shaders the CPU poses the corners: it uploads them
    /// again only when what they're posed from changed (the skin, the skin
    /// matrices its corners weigh, its morph weights), not every frame.
    #[test]
    fn the_cpu_fallback_uploads_a_pose_only_when_what_it_reads_changed() {
        use glam::{Mat4, Vec3};
        use wgpu::util::DeviceExt;
        let Some(crate::render::render_test_support::TestGpu { device, queue, .. }) =
            crate::render::render_test_support::gpu()
        else {
            return;
        };
        let skinning = &mut super::Skinning::new(&device, false);
        let vertex = |p: [f32; 3]| super::Vertex {
            position: p,
            normal: [0.0, 0.0, 1.0],
            uv: [0.0; 2],
            color: [0; 4],
            uv1: [0.0; 2],
        };
        let rest = [vertex([0.0; 3]), vertex([1.0, 0.0, 0.0]), vertex([0.0, 1.0, 0.0])];
        // Joints 0 and 1 weigh; joint 2 is named with weight 0.
        let joints = [[0u16, 2, 0, 0], [0, 1, 0, 0], [1, 0, 0, 0]];
        let weights = [[1.0f32, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];
        let one = |p: [f32; 3]| super::Delta {
            position: p,
            normal: [0.0; 3],
        };
        let deltas = vec![one([0.0; 3]), one([0.0, 2.0, 0.0]), one([0.0; 3])];
        let made = |s: &super::Skinning| s.create(&device, &rest, &joints, &weights, deltas.clone());
        let mut skin = made(skinning);
        assert_eq!(skin.joints, 2);
        let rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&rest),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut frame = crate::three::SkinFrame::default();
        // Another model's matrix first, then this one's two and a third
        // its corners don't weigh.
        frame.matrices = vec![
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::X),
            Mat4::IDENTITY,
        ];
        frame.draws.push(crate::three::SkinDraw {
            mesh: 0,
            instance: Default::default(),
            entity: 0,
            matrix: Mat4::IDENTITY,
            scale: 1.0,
            bounds: (Vec3::ZERO, 1.0),
            first: 1,
            morph_first: 1,
            morph_count: 1,
            material: u32::MAX,
            parts: (0, 0),
            lod: u32::MAX,
            posed_motion: false,
        });
        frame.weights = vec![9.0, 0.5];
        let corners = (rest.len() * size_of::<super::Vertex>()) as u64;
        let uploaded = |skinning: &mut super::Skinning, frame: &crate::three::SkinFrame, skin: &super::GpuSkin| {
            crate::render::traffic::take();
            skinning.prepare(&device, &queue, frame, |_| Some((&rest_buffer, skin)), |_| true, false);
            crate::render::traffic::take().skins
        };
        assert_eq!(uploaded(skinning, &frame, &skin), corners, "first posed");
        assert_eq!(uploaded(skinning, &frame, &skin), 0, "held");
        frame.matrices[0] = Mat4::from_scale(Vec3::splat(9.0));
        frame.matrices[3] = Mat4::from_scale(Vec3::splat(9.0));
        frame.weights[0] = 1.0;
        assert_eq!(
            uploaded(skinning, &frame, &skin),
            0,
            "only matrices and weights it doesn't read changed"
        );
        frame.matrices[2] = Mat4::from_translation(Vec3::Y);
        assert_eq!(uploaded(skinning, &frame, &skin), corners, "a joint it weighs moved");
        frame.weights[1] = 0.25;
        assert_eq!(uploaded(skinning, &frame, &skin), corners, "its morph weight changed");
        assert_eq!(uploaded(skinning, &frame, &skin), 0, "held again");
        skin = made(skinning);
        assert_eq!(
            uploaded(skinning, &frame, &skin),
            corners,
            "a skin made anew under the same mesh"
        );
    }

    #[test]
    fn influences_are_six_words() {
        assert_eq!(size_of::<super::Influence>(), 24, "skin.wgsl reads 6 words per corner");
        assert_eq!(
            size_of::<super::Delta>(),
            24,
            "skin.wgsl reads 6 words per morph offset"
        );
        assert_eq!(
            size_of::<super::super::Vertex>(),
            48,
            "skin.wgsl reads 12 words per vertex"
        );
    }
}
