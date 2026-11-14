//! The depth that occlusion culling tests against: the camera's depth shrunk
//! to a small grid of each tile's farthest depth by a compute pass, read back
//! through a small ring of buffers so the CPU never waits for the GPU (the
//! grid arrives one to three frames late, like the GPU timer's results).

use crate::render::traffic::{self, Counted, NewBuffer, Upload};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use glam::{Mat4, Vec3};

use crate::three::occlusion::OcclusionGrid;

const SLOTS: usize = 3;
const FREE: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;
/// The grid is about this many tiles wide; tiles are at least `MIN_TILE`
/// pixels on a side.
const GRID_WIDTH: u32 = 160;
const MIN_TILE: u32 = 8;

const SHADER: &str = "
struct Params { size: vec2<u32>, grid: vec2<u32>, tile: u32, samples: u32, pad: vec2<u32> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var depth: DEPTH_TYPE;
@group(0) @binding(2) var<storage, read_write> farthest: array<f32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.grid.x || id.y >= params.grid.y { return; }
    let start = id.xy * params.tile;
    let end = min(start + vec2<u32>(params.tile), params.size);
    var far = 0.0;
    for (var y = start.y; y < end.y; y++) {
        for (var x = start.x; x < end.x; x++) {
            for (var s = 0u; s < params.samples; s++) {
                far = max(far, textureLoad(depth, vec2<i32>(i32(x), i32(y)), i32(s)));
            }
        }
    }
    farthest[id.y * params.grid.x + id.x] = far;
}
";

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    size: [u32; 2],
    grid: [u32; 2],
    tile: u32,
    samples: u32,
    pad: [u32; 2],
}

struct Slot {
    buffer: Option<wgpu::Buffer>,
    state: Arc<AtomicU8>,
    view_proj: Mat4,
    camera: Vec3,
    grid: (u32, u32),
    /// Which frame filled it, so the newest one wins.
    frame: u64,
}

/// The tile size and grid size for a `size` screen.
pub(crate) fn grid_for(size: (u32, u32)) -> (u32, (u32, u32)) {
    let tile = size.0.div_ceil(GRID_WIDTH).max(MIN_TILE);
    (tile, (size.0.div_ceil(tile), size.1.div_ceil(tile)))
}

pub(crate) struct Occlusion {
    supported: bool,
    /// The pipeline for a sample count, and its layout.
    pipeline: Option<(u32, wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    params: wgpu::Buffer,
    farthest: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    slots: [Slot; SLOTS],
    /// The slot this frame fills, if one was free and culling is on.
    current: Option<usize>,
    next: usize,
    frame: u64,
    /// The newest grid read back, until it is taken.
    pub latest: Option<OcclusionGrid>,
}

impl Occlusion {
    /// Whether occlusion culling can run on this device.
    #[cfg(test)]
    pub fn supported(&self) -> bool {
        self.supported
    }

    /// `supported`: whether the device runs compute shaders.
    pub fn new(device: &wgpu::Device, supported: bool) -> Self {
        let slot = || Slot {
            buffer: None,
            state: Arc::new(AtomicU8::new(FREE)),
            view_proj: Mat4::IDENTITY,
            camera: Vec3::ZERO,
            grid: (0, 0),
            frame: 0,
        };
        Self {
            supported,
            pipeline: None,
            params: device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("3d occlusion params"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            farthest: None,
            bind_group: None,
            slots: [slot(), slot(), slot()],
            current: None,
            next: 0,
            frame: 0,
            latest: None,
        }
    }

    /// Picks up finished readbacks into `latest`.
    fn collect(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        let newest = (0..SLOTS)
            .filter(|&i| self.slots[i].state.load(Ordering::Acquire) == READY)
            .max_by_key(|&i| self.slots[i].frame);
        for i in 0..SLOTS {
            let slot = &self.slots[i];
            if slot.state.load(Ordering::Acquire) != READY {
                continue;
            }
            let Some(buffer) = &slot.buffer else { continue };
            if Some(i) == newest {
                let (w, h) = slot.grid;
                let bytes = (w * h) as usize * 4;
                if let Ok(view) = buffer.get_mapped_range(..) {
                    let depth: Vec<f32> = bytemuck::cast_slice(&view[..bytes]).to_vec();
                    drop(view);
                    self.latest = Some(OcclusionGrid::new(slot.view_proj, slot.camera, w, h, depth));
                }
            }
            buffer.unmap();
            slot.state.store(FREE, Ordering::Release);
        }
    }

    /// Sets up this frame's reduction of `depth` (`size`, `samples` per
    /// pixel), drawn with `view_proj` from `camera`. `on`: whether culling
    /// is on; when it's off nothing runs and old grids are dropped.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        depth: &wgpu::TextureView,
        size: (u32, u32),
        samples: u32,
        view_proj: Mat4,
        camera: Vec3,
        on: bool,
    ) {
        self.current = None;
        if !self.supported {
            return;
        }
        self.collect(device);
        if !on {
            self.latest = None;
            self.bind_group = None;
            return;
        }
        let i = self.next;
        if self.slots[i].state.load(Ordering::Acquire) != FREE {
            return;
        }
        let (tile, grid) = grid_for(size);
        let bytes = u64::from(grid.0 * grid.1) * 4;
        let fits = |b: &Option<wgpu::Buffer>| b.as_ref().is_some_and(|b| b.size() >= bytes);
        if !fits(&self.farthest) {
            if self.farthest.is_some() {
                traffic::reallocated();
            }
            self.farthest = Some(device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("3d occlusion depth"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
        }
        if !fits(&self.slots[i].buffer) {
            if self.slots[i].buffer.is_some() {
                traffic::reallocated();
            }
            self.slots[i].buffer = Some(device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("3d occlusion readback"),
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.pipeline.as_ref().is_none_or(|p| p.0 != samples) {
            self.pipeline = Some(build(device, samples));
        }
        let Some((_, _, layout)) = &self.pipeline else { return };
        let Some(farthest) = &self.farthest else { return };
        queue.uploading(Upload::Other).write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                size: [size.0, size.1],
                grid: [grid.0, grid.1],
                tile,
                samples,
                pad: [0; 2],
            }),
        );
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3d occlusion"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: farthest.as_entire_binding(),
                },
            ],
        }));
        self.frame += 1;
        let slot = &mut self.slots[i];
        slot.view_proj = view_proj;
        slot.camera = camera;
        slot.grid = grid;
        slot.frame = self.frame;
        self.current = Some(i);
        self.next = (i + 1) % SLOTS;
    }

    /// Whether [`record`](Self::record) has a grid to reduce this frame.
    pub fn recording(&self) -> bool {
        self.pipeline.is_some()
            && self.bind_group.is_some()
            && self.farthest.is_some()
            && self.current.is_some_and(|i| self.slots[i].buffer.is_some())
    }

    /// Records the reduction and the copy out, once the depth is complete.
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let (Some(i), Some((_, pipeline, _)), Some(bind_group), Some(farthest)) =
            (self.current, &self.pipeline, &self.bind_group, &self.farthest)
        else {
            return;
        };
        let slot = &self.slots[i];
        let Some(buffer) = &slot.buffer else { return };
        let (w, h) = slot.grid;
        super::gpu3d::mark_step("occlusion depth");
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("3d occlusion depth"),
                timestamp_writes: timestamps,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(farthest, 0, buffer, 0, u64::from(w * h) * 4);
    }

    /// Starts reading this frame's grid back. Call after submitting.
    pub fn submitted(&mut self) {
        if let Some(i) = self.current.take()
            && let Some(buffer) = &self.slots[i].buffer
        {
            let state = self.slots[i].state.clone();
            state.store(MAPPING, Ordering::Release);
            buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                state.store(if result.is_ok() { READY } else { FREE }, Ordering::Release);
            });
        }
    }
}

fn build(device: &wgpu::Device, samples: u32) -> (u32, wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    let multisampled = samples > 1;
    let source = SHADER.replace(
        "DEPTH_TYPE",
        if multisampled {
            "texture_depth_multisampled_2d"
        } else {
            "texture_depth_2d"
        },
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("3d occlusion depth"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("3d occlusion"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("3d occlusion"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("3d occlusion depth"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    (samples, pipeline, layout)
}

impl Occlusion {
    /// Counts the depth grid's buffers as other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Other, &self.params);
        for buffer in self
            .farthest
            .iter()
            .chain(self.slots.iter().filter_map(|s| s.buffer.as_ref()))
        {
            t.buffer(C::Other, buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_about_160_tiles_wide_with_tiles_of_at_least_8_pixels() {
        assert_eq!(grid_for((1920, 1080)), (12, (160, 90)));
        assert_eq!(grid_for((3840, 2160)), (24, (160, 90)));
        assert_eq!(grid_for((640, 480)), (8, (80, 60)));
        assert_eq!(grid_for((1, 1)), (8, (1, 1)));
    }

    #[test]
    fn both_shaders_are_valid() {
        for ty in ["texture_depth_multisampled_2d", "texture_depth_2d"] {
            let source = SHADER.replace("DEPTH_TYPE", ty);
            let module = wgpu::naga::front::wgsl::parse_str(&source).expect("parses");
            wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::all())
                .validate(&module)
                .expect("valid");
        }
    }
}
