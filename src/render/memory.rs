//! Counting the GPU memory the renderer holds, by class, for
//! [`GpuMemory`]: every part of the renderer hands its textures and
//! buffers to a [`MemoryTally`], which counts each one once.
//!
//! A texture or buffer can be held in several places (a shadow map's view
//! in the frame bind group's sources, a render target in the game's
//! texture table and in what a target camera keeps), so the tally keeps the
//! ones it has seen and the first class to hand one in owns it: the parts
//! that own a resource are asked before the ones that only borrow it.
//!
//! Every texture and buffer is handed in with a label naming what it is
//! ("scene color", "sun shadow map"). A tally made with
//! [`MemoryTally::listing`] also keeps one [`GpuAllocation`] per resource
//! (label, size, format, samples, usage, bytes) for `perf.get`'s
//! `gpu_allocations`; the default tally keeps only the sums.
//!
//! Built only when asked for (the control endpoint, a report while
//! profiling, about twice a second), never per frame.

use std::collections::HashSet;

use serde::Serialize;

use crate::perf::GpuMemory;

/// A class of [`GpuMemory`], named after its field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) enum MemoryClass {
    Textures,
    Meshes,
    GpuBuffers,
    FrameTargets,
    Shadows,
    Atlases,
    Skinning,
    Probes,
    InstanceBatches,
    Other,
}

/// What has been counted so far (see the module docs).
#[derive(Default)]
pub(crate) struct MemoryTally {
    textures: HashSet<wgpu::Texture>,
    buffers: HashSet<wgpu::Buffer>,
    memory: GpuMemory,
    /// Every allocation counted, when made with [`listing`](Self::listing).
    list: Option<Vec<GpuAllocation>>,
}

#[cfg_attr(not(feature = "3d"), allow(dead_code))]
impl MemoryTally {
    /// A tally that also lists every allocation it counts (see
    /// [`allocations`](Self::allocations)).
    pub fn listing() -> Self {
        Self {
            list: Some(Vec::new()),
            ..Self::default()
        }
    }

    /// Counts `texture`, named `label`, under `class`, unless it was
    /// counted already.
    pub fn texture(&mut self, class: MemoryClass, label: &'static str, texture: &wgpu::Texture) {
        if self.textures.insert(texture.clone()) {
            let bytes = super::gpu::texture_bytes(texture);
            self.add(class, bytes);
            if let Some(list) = &mut self.list {
                list.push(GpuAllocation {
                    class: class.name(),
                    label,
                    kind: AllocationKind::Texture,
                    bytes,
                    size: Some([texture.width(), texture.height(), texture.depth_or_array_layers()]),
                    mips: Some(texture.mip_level_count()),
                    samples: Some(texture.sample_count()),
                    format: Some(format!("{:?}", texture.format())),
                    usage: None,
                });
            }
        }
    }

    /// Counts the texture `view` shows (all of it, not only the view's
    /// layers or levels), named `label`, under `class`, unless it was
    /// counted already.
    pub fn view(&mut self, class: MemoryClass, label: &'static str, view: &wgpu::TextureView) {
        self.texture(class, label, view.texture());
    }

    /// Counts `buffer`, named `label`, under `class`, unless it was counted
    /// already.
    pub fn buffer(&mut self, class: MemoryClass, label: &'static str, buffer: &wgpu::Buffer) {
        if self.buffers.insert(buffer.clone()) {
            let bytes = buffer.size();
            self.add(class, bytes);
            if let Some(list) = &mut self.list {
                list.push(GpuAllocation {
                    class: class.name(),
                    label,
                    kind: AllocationKind::Buffer,
                    bytes,
                    size: None,
                    mips: None,
                    samples: None,
                    format: None,
                    usage: Some(BufferUsage::of(buffer.usage())),
                });
            }
        }
    }

    /// Adds `bytes`, named `label`, to `class`: memory counted some other
    /// way (a shared arena that knows its own size, a query set).
    pub fn bytes(&mut self, class: MemoryClass, label: &'static str, bytes: u64) {
        self.add(class, bytes);
        if let Some(list) = &mut self.list {
            list.push(GpuAllocation {
                class: class.name(),
                label,
                kind: AllocationKind::Other,
                bytes,
                size: None,
                mips: None,
                samples: None,
                format: None,
                usage: None,
            });
        }
    }

    fn add(&mut self, class: MemoryClass, bytes: u64) {
        let m = &mut self.memory;
        let field = match class {
            MemoryClass::Textures => &mut m.textures,
            MemoryClass::Meshes => &mut m.meshes,
            MemoryClass::GpuBuffers => &mut m.gpu_buffers,
            MemoryClass::FrameTargets => &mut m.frame_targets,
            MemoryClass::Shadows => &mut m.shadows,
            MemoryClass::Atlases => &mut m.atlases,
            MemoryClass::Skinning => &mut m.skinning,
            MemoryClass::Probes => &mut m.probes,
            MemoryClass::InstanceBatches => &mut m.instance_batches,
            MemoryClass::Other => &mut m.other,
        };
        *field += bytes;
    }

    /// What the counted textures and buffers take when an allocator places
    /// each one on its own range, in whole pages of `page` bytes (test
    /// hook, for comparing with such an allocator's figure).
    #[cfg(test)]
    pub fn placed_bytes(&self, page: u64) -> u64 {
        let textures = self.textures.iter().map(super::gpu::texture_bytes);
        let buffers = self.buffers.iter().map(wgpu::Buffer::size);
        textures.chain(buffers).map(|b| b.next_multiple_of(page)).sum()
    }

    /// The counts, with their total; the adapter's figure is left for the
    /// caller.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn finish(self) -> GpuMemory {
        self.finish_listed().0
    }

    /// The counts, as [`finish`](Self::finish), and every allocation counted,
    /// largest first (empty unless the tally was made with
    /// [`listing`](Self::listing)).
    pub fn finish_listed(self) -> (GpuMemory, Vec<GpuAllocation>) {
        let mut m = self.memory;
        m.total = m.classes().iter().map(|c| c.1).sum();
        let mut list = self.list.unwrap_or_default();
        list.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.label.cmp(b.label)));
        (m, list)
    }
}

impl MemoryClass {
    /// Its [`GpuMemory`] field's name, as reports print it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Textures => "textures",
            Self::Meshes => "meshes",
            Self::GpuBuffers => "gpu_buffers",
            Self::FrameTargets => "frame_targets",
            Self::Shadows => "shadows",
            Self::Atlases => "atlases",
            Self::Skinning => "skinning",
            Self::Probes => "probes",
            Self::InstanceBatches => "instance_batches",
            Self::Other => "other",
        }
    }
}

/// One texture or buffer the engine holds, from a listing tally: what
/// `perf.get` lists in `render_inspector.gpu_allocations`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct GpuAllocation {
    /// The [`GpuMemory`] class it is counted under (`"frame_targets"`).
    pub class: &'static str,
    /// What it is (`"scene color"`, `"sun shadow map"`), the same for every
    /// allocation of its kind.
    pub label: &'static str,
    pub kind: AllocationKind,
    /// Its bytes at the size asked for (mipmaps, layers and samples
    /// included), without the driver's padding.
    pub bytes: u64,
    /// A texture's width, height and layers (or depth).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 3]>,
    /// A texture's mip levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mips: Option<u32>,
    /// A texture's samples per pixel (more than 1: multisampled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<u32>,
    /// A texture's format, as wgpu names it (`"Rgba16Float"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// What a buffer is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<BufferUsage>,
}

/// What a [`GpuAllocation`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AllocationKind {
    Texture,
    Buffer,
    /// Memory counted by its size: a shared arena, a query set.
    Other,
}

/// What a buffer is mainly for, from its wgpu usage flags: the first that
/// applies of staging (mapped for reading or writing), index, vertex,
/// indirect (draw or dispatch arguments), storage, uniform, query (timestamp
/// resolves), and copy (only copied to and from).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BufferUsage {
    Staging,
    Index,
    Vertex,
    Indirect,
    Storage,
    Uniform,
    Query,
    Copy,
}

impl BufferUsage {
    /// Every usage, in the order reports print them.
    pub const ALL: [Self; 8] = [
        Self::Uniform,
        Self::Storage,
        Self::Vertex,
        Self::Index,
        Self::Indirect,
        Self::Staging,
        Self::Query,
        Self::Copy,
    ];

    pub fn of(usage: wgpu::BufferUsages) -> Self {
        use wgpu::BufferUsages as U;
        if usage.intersects(U::MAP_READ | U::MAP_WRITE) {
            Self::Staging
        } else if usage.contains(U::INDEX) {
            Self::Index
        } else if usage.contains(U::VERTEX) {
            Self::Vertex
        } else if usage.contains(U::INDIRECT) {
            Self::Indirect
        } else if usage.contains(U::STORAGE) {
            Self::Storage
        } else if usage.contains(U::UNIFORM) {
            Self::Uniform
        } else if usage.contains(U::QUERY_RESOLVE) {
            Self::Query
        } else {
            Self::Copy
        }
    }

    /// Its name, as reports print it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Staging => "staging",
            Self::Index => "index",
            Self::Vertex => "vertex",
            Self::Indirect => "indirect",
            Self::Storage => "storage",
            Self::Uniform => "uniform",
            Self::Query => "query",
            Self::Copy => "copy",
        }
    }
}

/// `perf.get`'s `render_inspector.gpu_allocations` from a listing,
/// largest first: how many there are, the `limit` largest, the bytes of
/// the rest, and the buffers' bytes by [`BufferUsage`] (all of them, listed
/// or not).
pub(crate) fn allocations_json(list: &[GpuAllocation], limit: usize) -> serde_json::Value {
    let listed = limit.min(list.len());
    let by_usage: serde_json::Map<String, serde_json::Value> = BufferUsage::ALL
        .iter()
        .map(|&u| {
            let bytes: u64 = list.iter().filter(|a| a.usage == Some(u)).map(|a| a.bytes).sum();
            (u.name().into(), serde_json::json!(bytes))
        })
        .collect();
    serde_json::json!({
        "count": list.len(),
        "listed": listed,
        "unlisted_bytes": list[listed..].iter().map(|a| a.bytes).sum::<u64>(),
        "buffer_bytes_by_usage": by_usage,
        "entries": &list[..listed],
    })
}

/// The platform's own figure for the memory `device` holds, or why there
/// is none (see [`GpuMemory::adapter_bytes`]).
pub(crate) fn adapter_bytes(device: &wgpu::Device) -> (Option<u64>, &'static str) {
    if let Some(report) = device.generate_allocator_report() {
        return (
            Some(report.total_allocated_bytes),
            "the backend's allocator (wgpu's allocation report): the bytes of its live allocations",
        );
    }
    #[cfg(target_vendor = "apple")]
    if let Some(bytes) = metal_allocated_bytes(device) {
        return (
            Some(bytes),
            "Metal: the device's currentAllocatedSize, everything this process allocated on the GPU",
        );
    }
    (
        None,
        "this backend doesn't report its allocations (OpenGL, and WebGPU or WebGL in a browser)",
    )
}

/// Metal's `currentAllocatedSize` for `device`, on the Metal backend.
#[cfg(target_vendor = "apple")]
fn metal_allocated_bytes(device: &wgpu::Device) -> Option<u64> {
    use objc2_metal::MTLDevice as _;
    // SAFETY: only reads a property of the Metal device behind wgpu's; the
    // guard is dropped before wgpu is used again, and nothing of wgpu's
    // state is touched.
    unsafe {
        let hal = device.as_hal::<wgpu::hal::api::Metal>()?;
        Some(hal.raw_device().currentAllocatedSize() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_add_up_to_the_total() {
        let mut t = MemoryTally::default();
        t.bytes(MemoryClass::Shadows, "a", 100);
        t.bytes(MemoryClass::FrameTargets, "b", 20);
        t.bytes(MemoryClass::Shadows, "c", 1);
        let m = t.finish();
        assert_eq!((m.shadows, m.frame_targets, m.total), (101, 20, 121));
    }

    #[test]
    fn a_listing_tally_lists_each_allocation_largest_first_and_the_default_none() {
        let mut t = MemoryTally::listing();
        t.bytes(MemoryClass::Meshes, "mesh arena", 20);
        t.bytes(MemoryClass::Shadows, "sun shadow map", 100);
        let (m, list) = t.finish_listed();
        assert_eq!(m.total, 120);
        let labels: Vec<_> = list.iter().map(|a| (a.class, a.label, a.bytes)).collect();
        assert_eq!(
            labels,
            [("shadows", "sun shadow map", 100), ("meshes", "mesh arena", 20)]
        );
        let mut t = MemoryTally::default();
        t.bytes(MemoryClass::Shadows, "sun shadow map", 100);
        assert!(t.finish_listed().1.is_empty());
    }

    #[test]
    fn the_json_lists_the_largest_and_sums_the_rest() {
        let mut t = MemoryTally::listing();
        for (label, bytes) in [("a", 5), ("b", 30), ("c", 10)] {
            t.bytes(MemoryClass::Other, label, bytes);
        }
        let v = allocations_json(&t.finish_listed().1, 2);
        assert_eq!((v["count"].as_u64(), v["listed"].as_u64()), (Some(3), Some(2)));
        assert_eq!(v["unlisted_bytes"], 5);
        assert_eq!(v["entries"][0]["label"], "b");
        assert_eq!(v["entries"][0]["kind"], "other");
        assert_eq!(v["entries"][1]["bytes"], 10);
        assert_eq!(v["buffer_bytes_by_usage"]["uniform"], 0);
        assert!(v["entries"][0].get("format").is_none(), "no texture fields: {v}");
    }

    #[test]
    fn buffers_are_named_by_their_main_usage() {
        use wgpu::BufferUsages as U;
        for (usage, want) in [
            (U::UNIFORM | U::COPY_DST, BufferUsage::Uniform),
            (U::STORAGE | U::INDIRECT, BufferUsage::Indirect),
            (U::STORAGE | U::COPY_SRC, BufferUsage::Storage),
            (U::VERTEX | U::STORAGE, BufferUsage::Vertex),
            (U::INDEX | U::COPY_DST, BufferUsage::Index),
            (U::MAP_READ | U::COPY_DST, BufferUsage::Staging),
            (U::QUERY_RESOLVE | U::COPY_SRC, BufferUsage::Query),
            (U::COPY_SRC | U::COPY_DST, BufferUsage::Copy),
        ] {
            assert_eq!(BufferUsage::of(usage), want, "{usage:?}");
        }
    }
}
