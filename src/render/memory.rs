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
//! Built only when asked for (the control endpoint, a report while
//! profiling, about twice a second), never per frame.

use std::collections::HashSet;

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
}

#[cfg_attr(not(feature = "3d"), allow(dead_code))]
impl MemoryTally {
    /// Counts `texture` under `class`, unless it was counted already.
    pub fn texture(&mut self, class: MemoryClass, texture: &wgpu::Texture) {
        if self.textures.insert(texture.clone()) {
            self.bytes(class, super::gpu::texture_bytes(texture));
        }
    }

    /// Counts the texture `view` shows (all of it, not only the view's
    /// layers or levels) under `class`, unless it was counted already.
    pub fn view(&mut self, class: MemoryClass, view: &wgpu::TextureView) {
        self.texture(class, view.texture());
    }

    /// Counts `buffer` under `class`, unless it was counted already.
    pub fn buffer(&mut self, class: MemoryClass, buffer: &wgpu::Buffer) {
        if self.buffers.insert(buffer.clone()) {
            self.bytes(class, buffer.size());
        }
    }

    /// Adds `bytes` to `class`: memory counted some other way (a shared
    /// arena that knows its own size).
    pub fn bytes(&mut self, class: MemoryClass, bytes: u64) {
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

    /// The counts, with their total; the adapter's figure is left for the
    /// caller.
    pub fn finish(self) -> GpuMemory {
        let mut m = self.memory;
        m.total = m.classes().iter().map(|c| c.1).sum();
        m
    }
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
        t.bytes(MemoryClass::Shadows, 100);
        t.bytes(MemoryClass::FrameTargets, 20);
        t.bytes(MemoryClass::Shadows, 1);
        let m = t.finish();
        assert_eq!((m.shadows, m.frame_targets, m.total), (101, 20, 121));
    }
}
