//! Shared storage for immutable mesh geometry. Indices are rebased on upload,
//! so every tier (including WebGL2 without base-vertex draws) binds the whole
//! arena and draws with base vertex zero. Skinning keeps dedicated rest data.
//!
//! Uploads are staged on the CPU only until the next flush (asset creation
//! has a device but no queue), then the staging memory is freed: the GPU
//! buffer is the only full copy. Growth copies the old buffer into the larger
//! one on the GPU. Where the GPU can't copy a buffer (index buffers on
//! WebGL2, which can't be copy sources), that arena keeps a CPU mirror and
//! growth uploads it instead.
use super::resources::Vertex;
use crate::render::traffic::{self, Counted, NewBuffer, Upload};
use std::ops::Range;

#[derive(Clone)]
pub(super) struct Allocation {
    pub vertices: Range<u32>,
    pub indices: Range<u32>,
}

#[derive(Default)]
struct Arena<T> {
    /// Elements handed out so far, freed ones included: the high-water mark.
    len: u32,
    free: Vec<Range<u32>>,
    /// Writes since the last flush, in order: the arena range, and where its
    /// data starts in `staged`.
    dirty: Vec<(Range<u32>, usize)>,
    /// Their data; freed once flushed.
    staged: Vec<T>,
    /// The whole arena, kept only where the GPU can't copy the buffer.
    mirror: Option<Vec<T>>,
    buffer: Option<wgpu::Buffer>,
    /// Negative control for tests: growth drops the old contents.
    #[cfg(test)]
    lose_on_grow: bool,
}

impl<T: bytemuck::Pod + bytemuck::Zeroable> Arena<T> {
    fn allocate(&mut self, count: u32, limit: u64) -> Option<Range<u32>> {
        if let Some(i) = self.free.iter().position(|r| r.end - r.start >= count) {
            let start = self.free[i].start;
            self.free[i].start += count;
            if self.free[i].is_empty() {
                self.free.remove(i);
            }
            return Some(start..start + count);
        }
        let start = self.len;
        let end = start.checked_add(count)?;
        if u64::from(end) * size_of::<T>() as u64 > limit {
            return None;
        }
        self.len = end;
        if let Some(mirror) = &mut self.mirror {
            mirror.resize(end as usize, T::zeroed());
        }
        Some(start..end)
    }

    fn release(&mut self, range: Range<u32>) {
        self.free.push(range);
        self.free.sort_unstable_by_key(|r| r.start);
        let mut i = 1;
        while i < self.free.len() {
            if self.free[i - 1].end == self.free[i].start {
                self.free[i - 1].end = self.free[i].end;
                self.free.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn write(&mut self, range: Range<u32>, data: impl IntoIterator<Item = T>) {
        let offset = self.staged.len();
        self.staged.extend(data);
        debug_assert_eq!(self.staged.len() - offset, (range.end - range.start) as usize);
        if let Some(mirror) = &mut self.mirror {
            mirror[range.start as usize..range.end as usize].copy_from_slice(&self.staged[offset..]);
        }
        self.dirty.push((range, offset));
    }

    /// Bytes held on the CPU: staged writes, the mirror and the range lists.
    fn cpu_bytes(&self) -> u64 {
        let elements = self.staged.capacity() + self.mirror.as_ref().map_or(0, Vec::capacity);
        (elements * size_of::<T>()
            + self.free.capacity() * size_of::<Range<u32>>()
            + self.dirty.capacity() * size_of::<(Range<u32>, usize)>()) as u64
    }

    fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, usage: wgpu::BufferUsages) {
        if self.dirty.is_empty() {
            return;
        }
        let needed = u64::from(self.len) * size_of::<T>() as u64;
        if self.buffer.as_ref().is_none_or(|b| b.size() < needed) {
            // Geometric growth capped by the device limit. Four-byte alignment
            // holds for both Vertex and u32, including queue writes and copies.
            let size = needed.next_power_of_two().min(device.limits().max_buffer_size);
            let copies = self.mirror.is_none() || cfg!(test);
            let buffer = device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("shared mesh arena"),
                size,
                usage: usage
                    | wgpu::BufferUsages::COPY_DST
                    | if copies {
                        wgpu::BufferUsages::COPY_SRC
                    } else {
                        wgpu::BufferUsages::empty()
                    },
                mapped_at_creation: false,
            });
            match (&self.mirror, self.buffer.take()) {
                (Some(mirror), _) => {
                    queue
                        .uploading(Upload::Meshes)
                        .write_buffer(&buffer, 0, bytemuck::cast_slice(mirror));
                    self.dirty.clear();
                }
                #[cfg(test)]
                (None, Some(_)) if self.lose_on_grow => {}
                (None, Some(old)) => {
                    // Submitted now, so the queue writes below (applied at the
                    // next submission) land after the copy, not under it.
                    traffic::submit_copy(device, queue, Some("grow shared mesh arena"), |encoder| {
                        encoder.copy_buffer_to_buffer(&old, 0, &buffer, 0, old.size());
                    });
                }
                (None, None) => {}
            }
            self.buffer = Some(buffer);
        }
        let buffer = self.buffer.as_ref().unwrap();
        let upload = |range: &Range<u32>, offset: usize| {
            let len = (range.end - range.start) as usize;
            queue.uploading(Upload::Meshes).write_buffer(
                buffer,
                u64::from(range.start) * size_of::<T>() as u64,
                bytemuck::cast_slice(&self.staged[offset..offset + len]),
            );
        };
        // In order, so a range freed and reused before this flush ends with
        // its newest data. Coalesce writes that follow each other in both the
        // arena and the staging (asset bursts).
        let mut writes = self.dirty.iter();
        if let Some((first, offset)) = writes.next() {
            let (mut range, mut offset) = (first.clone(), *offset);
            for (next, next_offset) in writes {
                if next.start == range.end && *next_offset == offset + (range.end - range.start) as usize {
                    range.end = next.end;
                } else {
                    upload(&range, offset);
                    (range, offset) = (next.clone(), *next_offset);
                }
            }
            upload(&range, offset);
        }
        // Free the staging: the buffer holds the geometry now.
        self.staged = Vec::new();
        self.dirty = Vec::new();
    }
}

impl Default for Vertex {
    fn default() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

/// Whether `device` can copy index buffers (`DownlevelFlags::UNRESTRICTED_INDEX_BUFFER`):
/// everywhere but the browser's WebGL2.
fn copies_index_buffers(device: &wgpu::Device) -> bool {
    !(cfg!(target_arch = "wasm32") && device.adapter_info().backend == wgpu::Backend::Gl)
}

#[derive(Default)]
pub(super) struct MeshArena {
    vertices: Arena<Vertex>,
    indices: Arena<u32>,
    live: usize,
    /// Keep a CPU mirror of the indices (decided at the first upload, or
    /// forced by tests).
    index_mirror: Option<bool>,
}

impl MeshArena {
    pub fn upload(&mut self, device: &wgpu::Device, vertices: &[Vertex], indices: &[u32]) -> Option<Allocation> {
        if vertices.is_empty() || indices.is_empty() {
            return None;
        }
        if *self.index_mirror.get_or_insert_with(|| !copies_index_buffers(device)) && self.indices.mirror.is_none() {
            self.indices.mirror = Some(vec![0; self.indices.len as usize]);
        }
        let limit = device.limits().max_buffer_size;
        let vertex_count = u32::try_from(vertices.len()).ok()?;
        let index_count = u32::try_from(indices.len()).ok()?;
        let v = self.vertices.allocate(vertex_count, limit)?;
        let Some(i) = self.indices.allocate(index_count, limit) else {
            self.vertices.release(v);
            return None;
        };
        self.vertices.write(v.clone(), vertices.iter().copied());
        self.indices
            .write(i.clone(), indices.iter().map(|index| index + v.start));
        self.live += 1;
        Some(Allocation {
            vertices: v,
            indices: i,
        })
    }
    pub fn release(&mut self, allocation: Allocation) {
        self.vertices.release(allocation.vertices);
        self.indices.release(allocation.indices);
        self.live -= 1;
        if self.live == 0 {
            *self = Self {
                index_mirror: self.index_mirror,
                ..Self::default()
            };
        }
    }
    pub fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.vertices.flush(device, queue, wgpu::BufferUsages::VERTEX);
        self.indices.flush(device, queue, wgpu::BufferUsages::INDEX);
    }
    pub fn buffers(&self) -> Option<(&wgpu::Buffer, &wgpu::Buffer)> {
        Some((self.vertices.buffer.as_ref()?, self.indices.buffer.as_ref()?))
    }
    pub fn bytes(&self) -> u64 {
        self.vertices.buffer.as_ref().map_or(0, wgpu::Buffer::size)
            + self.indices.buffer.as_ref().map_or(0, wgpu::Buffer::size)
    }
    /// Bytes the arena holds on the CPU: writes not flushed yet, and the
    /// index mirror where the GPU can't copy index buffers.
    pub fn cpu_bytes(&self) -> u64 {
        self.vertices.cpu_bytes() + self.indices.cpu_bytes()
    }
}

impl super::Renderer3D {
    /// Copies the meshes made since the last flush into the shared mesh
    /// arena now (as the frame's draw does).
    pub fn flush_mesh_arena(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.resources.mesh_arena.flush(device, queue);
    }

    /// Bytes the shared mesh arena holds on the CPU (`GpuResidency::mesh_cpu_bytes`).
    pub fn mesh_arena_cpu_bytes(&self) -> u64 {
        self.resources.mesh_arena.cpu_bytes()
    }
}

#[cfg(test)]
impl super::Renderer3D {
    /// Keeps the index arena's CPU mirror, as where the GPU can't copy
    /// index buffers (WebGL2). Before the first mesh.
    pub(crate) fn force_index_mirror_for_test(&mut self) {
        self.resources.mesh_arena.index_mirror = Some(true);
    }
    /// Negative control: growing the arena forgets what it held.
    pub(crate) fn lose_mesh_arena_on_grow_for_test(&mut self) {
        let arena = &mut self.resources.mesh_arena;
        (arena.vertices.lose_on_grow, arena.indices.lose_on_grow) = (true, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freed_ranges_coalesce_and_limits_fall_back_without_overflow() {
        let mut arena = Arena::<u32>::default();
        let a = arena.allocate(3, 32).unwrap();
        let b = arena.allocate(5, 32).unwrap();
        assert!(arena.allocate(1, 32).is_none());
        arena.release(a);
        arena.release(b);
        assert_eq!(arena.allocate(8, 32), Some(0..8));
        assert!(arena.allocate(u32::MAX, 32).is_none());
    }
    #[test]
    fn gpu_arena_upload_reuse_and_growth_preserve_live_geometry() {
        upload_reuse_and_growth(false);
    }
    #[test]
    fn gpu_arena_with_an_index_mirror_preserves_live_geometry() {
        upload_reuse_and_growth(true);
    }
    fn upload_reuse_and_growth(index_mirror: bool) {
        let Some(gpu) = crate::render::render_test_support::gpu() else {
            return;
        };
        let (device, queue) = (&gpu.device, &gpu.queue);
        let mut arena = MeshArena {
            index_mirror: Some(index_mirror),
            ..Default::default()
        };
        let vertices = vec![
            Vertex {
                position: [1.0, 2.0, 3.0],
                ..Default::default()
            };
            3
        ];
        let a = arena.upload(device, &vertices, &[0, 1, 2]).unwrap();
        let b = arena.upload(device, &vertices, &[2, 1, 0]).unwrap();
        arena.flush(device, queue);
        let check = |arena: &MeshArena, allocation: &Allocation, indices: &[u32]| {
            let (v, i) = arena.buffers().unwrap();
            let got = crate::render::traffic::read_back(
                device,
                queue,
                v,
                allocation.vertices.start as u64 * size_of::<Vertex>() as u64,
                vertices.len() as u64 * size_of::<Vertex>() as u64,
            );
            assert_eq!(got, bytemuck::cast_slice::<Vertex, u8>(&vertices));
            let got = crate::render::traffic::read_back(
                device,
                queue,
                i,
                allocation.indices.start as u64 * 4,
                indices.len() as u64 * 4,
            );
            let expected: Vec<u32> = indices.iter().map(|i| i + allocation.vertices.start).collect();
            assert_eq!(got, bytemuck::cast_slice::<u32, u8>(&expected));
        };
        check(&arena, &a, &[0, 1, 2]);
        check(&arena, &b, &[2, 1, 0]);
        arena.release(a.clone());
        let reused = arena.upload(device, &vertices, &[1, 2, 0]).unwrap();
        assert_eq!(reused.vertices, a.vertices);
        assert_eq!(reused.indices, a.indices);
        arena.flush(device, queue);
        check(&arena, &reused, &[1, 2, 0]);
        let big = arena
            .upload(device, &vec![Vertex::default(); 4096], &vec![0; 8192])
            .unwrap();
        arena.flush(device, queue);
        check(&arena, &b, &[2, 1, 0]);
        check(&arena, &reused, &[1, 2, 0]);
        // A range freed and taken again in the frame the arena grows ends
        // with its new data, not the copy of the old buffer.
        arena.release(b);
        let b = arena.upload(device, &vertices, &[0, 0, 1]).unwrap();
        let bigger = arena
            .upload(device, &vec![Vertex::default(); 8192], &vec![0; 16384])
            .unwrap();
        arena.flush(device, queue);
        check(&arena, &b, &[0, 0, 1]);
        check(&arena, &reused, &[1, 2, 0]);
        arena.release(bigger);
        arena.release(big);
        arena.release(b);
        arena.release(reused);
        assert_eq!(arena.bytes(), 0);
        assert!(arena.buffers().is_none());
        let again = arena.upload(device, &vertices, &[0, 2, 1]).unwrap();
        arena.flush(device, queue);
        check(&arena, &again, &[0, 2, 1]);
    }

    #[test]
    fn uploads_keep_no_cpu_copy_once_flushed() {
        let Some(gpu) = crate::render::render_test_support::gpu() else {
            return;
        };
        let (device, queue) = (&gpu.device, &gpu.queue);
        for index_mirror in [false, true] {
            let mut arena = MeshArena {
                index_mirror: Some(index_mirror),
                ..Default::default()
            };
            let vertices = vec![Vertex::default(); 1000];
            let indices: Vec<u32> = (0..3000).map(|i| i % 1000).collect();
            let (vertex_bytes, index_bytes) = (1000 * size_of::<Vertex>() as u64, 3000 * 4);
            let mut live = Vec::new();
            for frame in 0..10 {
                for _ in 0..20 {
                    live.push(arena.upload(device, &vertices, &indices).unwrap());
                }
                let staged = arena.cpu_bytes();
                assert!(
                    staged >= 20 * (vertex_bytes + index_bytes),
                    "staged until the flush: {staged}"
                );
                arena.flush(device, queue);
                let meshes = live.len() as u64;
                let expected = if index_mirror { meshes * index_bytes } else { 0 };
                let held = arena.cpu_bytes();
                assert!(
                    // The mirror's capacity may run up to twice its length.
                    held >= expected && held < 2 * expected + meshes * vertex_bytes / 10,
                    "frame {frame}, index mirror {index_mirror}: {held} bytes on the CPU for {meshes} meshes"
                );
                assert!(arena.bytes() >= meshes * (vertex_bytes + index_bytes));
            }
            // Streaming out leaves a free list, not geometry.
            for allocation in live.drain(..100) {
                arena.release(allocation);
            }
            assert!(index_mirror || arena.cpu_bytes() < 1024, "{}", arena.cpu_bytes());
        }
    }
}
