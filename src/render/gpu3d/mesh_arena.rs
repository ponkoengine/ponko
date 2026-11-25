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
//!
//! A buffer grows to a quarter more than it needs (see [`planned_size`]), not
//! to the next power of two, which could leave half of it empty: in a large
//! world that was hundreds of megabytes of unused GPU memory. Freeing the
//! meshes at the end of an arena lowers its high-water mark, and a buffer
//! left far larger than its contents shrinks again.
use super::resources::Vertex;
use crate::render::traffic::{self, Counted, NewBuffer, Upload};
use std::ops::Range;

/// The spare room a growing buffer gets, as a fraction of what it needs:
/// a quarter, so a stream of small uploads grows it a logarithmic number of
/// times (each growth copies the buffer), while at most a fifth of it
/// stands empty.
const SLACK_DIVISOR: u64 = 4;

/// `needed` bytes and a quarter more, aligned for copies, at most `limit`.
fn grown_size(needed: u64, limit: u64) -> u64 {
    let align = wgpu::COPY_BUFFER_ALIGNMENT;
    (needed + needed / SLACK_DIVISOR)
        .next_multiple_of(align)
        .min(limit / align * align)
        .max(needed)
}

/// The size an arena buffer of `current` bytes (0 without one) should have
/// to hold `needed`: grown with slack when too small; shrunk to that when
/// more than the slack twice over is empty (so streaming around one size
/// doesn't shrink and grow it again every few frames); else kept.
fn planned_size(current: u64, needed: u64, limit: u64) -> u64 {
    if needed == 0 {
        return current;
    }
    let grown = grown_size(needed, limit);
    if current < needed || current > grown_size(grown, limit) {
        grown
    } else {
        current
    }
}

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
        // Free room at the end lowers the high-water mark, so the next
        // flush can shrink the buffer.
        if let Some(last) = self.free.last()
            && last.end == self.len
        {
            self.len = last.start;
            self.free.pop();
            if let Some(mirror) = &mut self.mirror {
                mirror.truncate(self.len as usize);
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
        // Four-byte alignment holds for both Vertex and u32, including
        // queue writes and copies.
        let needed = u64::from(self.len) * size_of::<T>() as u64;
        let current = self.buffer.as_ref().map_or(0, wgpu::Buffer::size);
        let size = planned_size(current, needed, device.limits().max_buffer_size);
        if self.dirty.is_empty() && size == current {
            return;
        }
        if size != current {
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
                    // Growing copies the whole old buffer, shrinking what's
                    // still in use.
                    traffic::submit_copy(device, queue, Some("resize shared mesh arena"), |encoder| {
                        encoder.copy_buffer_to_buffer(&old, 0, &buffer, 0, old.size().min(needed));
                    });
                }
                (None, None) => {}
            }
            self.buffer = Some(buffer);
        }
        let (Some(buffer), high) = (self.buffer.as_ref(), self.len) else {
            // Nothing live to write (every write was freed again).
            self.staged = Vec::new();
            self.dirty = Vec::new();
            return;
        };
        let upload = |range: &Range<u32>, offset: usize| {
            // A write freed at the end of the arena since lies past the
            // high-water mark, and maybe past a shrunk buffer: skipped.
            let end = range.end.min(high);
            if range.start >= end {
                return;
            }
            let len = (end - range.start) as usize;
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
    fn freeing_the_end_lowers_the_high_water_mark() {
        let mut arena = Arena::<u32>::default();
        let a = arena.allocate(3, 64).unwrap();
        let b = arena.allocate(5, 64).unwrap();
        let c = arena.allocate(2, 64).unwrap();
        arena.release(b);
        assert_eq!(arena.len, 10, "a hole in the middle stays in the free list");
        arena.release(c);
        assert_eq!(arena.len, 3, "the end and the hole before it go");
        assert!(arena.free.is_empty());
        assert_eq!(
            arena.allocate(7, 64),
            Some(3..10),
            "new meshes continue from the lower mark"
        );
        arena.release(a);
        assert_eq!((arena.free.len(), arena.free.first()), (1, Some(&(0..3))));
    }

    #[test]
    fn arena_buffers_grow_with_a_quarter_spare_and_shrink_with_hysteresis() {
        const MIB: u64 = 1 << 20;
        let limit = 256 * MIB;
        // Growth: never too small, at most a quarter over (plus alignment),
        // never past the limit.
        for needed in [4, 1000, 44 * 1001, 129 * MIB + 4, 200 * MIB, limit - 44, limit] {
            let size = planned_size(0, needed, limit);
            assert!(size >= needed && size <= limit, "{needed} -> {size}");
            assert!(size <= needed + needed / 4 + 4, "{needed} -> {size}");
            assert_eq!(size % wgpu::COPY_BUFFER_ALIGNMENT, 0);
        }
        // A power of two would have been 256 MiB.
        assert_eq!(planned_size(0, 129 * MIB, limit), 129 * MIB + 129 * MIB / 4);
        // Kept while it fits and isn't far too large.
        let size = planned_size(0, 100 * MIB, limit);
        assert_eq!(planned_size(size, 100 * MIB + 1024, limit), size);
        assert_eq!(
            planned_size(size, 85 * MIB, limit),
            size,
            "a little less doesn't shrink it"
        );
        // Shrunk once much of it is empty, to the size it would grow to.
        assert_eq!(planned_size(size, 40 * MIB, limit), grown_size(40 * MIB, limit));
        // Nothing live: kept (the mesh arena drops its buffers then).
        assert_eq!(planned_size(size, 0, limit), size);
        // Growing one mesh at a time copies the buffer a logarithmic number
        // of times: about 40 growths from one mesh to 100 MiB.
        let (mut size, mut growths) = (0, 0);
        for meshes in 1..=100 * MIB / 44_000 {
            let next = planned_size(size, meshes * 44_000, limit);
            growths += u32::from(next != size);
            size = next;
        }
        assert!(growths < 50, "{growths} growths");
    }

    /// A mesh arena's buffer sizes without a GPU: what `flush` would make
    /// them, and what the power-of-two growth it replaced would have (from
    /// the peak high-water mark, as the old arena never lowered it).
    struct Simulated {
        vertices: Arena<Vertex>,
        indices: Arena<u32>,
        live: std::collections::VecDeque<Option<(Range<u32>, Range<u32>)>>,
        /// Vertex then index buffer bytes.
        sizes: [u64; 2],
        power_of_two: [u64; 2],
        peak: [u64; 2],
        /// Geometry that didn't fit under the limit (own buffers).
        dedicated: u64,
    }

    const LIMIT: u64 = 256 << 20;

    impl Simulated {
        fn new() -> Self {
            Self {
                vertices: Arena::default(),
                indices: Arena::default(),
                live: Default::default(),
                sizes: [0; 2],
                power_of_two: [0; 2],
                peak: [0; 2],
                dedicated: 0,
            }
        }
        fn used(&self) -> [u64; 2] {
            [
                u64::from(self.vertices.len) * size_of::<Vertex>() as u64,
                u64::from(self.indices.len) * 4,
            ]
        }
        fn flush(&mut self) {
            for (k, needed) in self.used().into_iter().enumerate() {
                self.sizes[k] = planned_size(self.sizes[k], needed, LIMIT);
                self.peak[k] = self.peak[k].max(needed);
                if self.power_of_two[k] < self.peak[k] {
                    self.power_of_two[k] = self.peak[k].next_power_of_two().min(LIMIT);
                }
            }
        }
        /// Loads `meshes` (corners, indices), flushing every 50 as a
        /// streamed world's bursts do.
        fn load(&mut self, meshes: &[(u32, u32)]) {
            for (n, &(v, i)) in meshes.iter().enumerate() {
                let placed = self.vertices.allocate(v, LIMIT).and_then(|vr| {
                    let ir = self.indices.allocate(i, LIMIT);
                    if ir.is_none() {
                        self.vertices.release(vr.clone());
                    }
                    Some((vr, ir?))
                });
                if placed.is_none() {
                    self.dedicated += u64::from(v) * size_of::<Vertex>() as u64 + u64::from(i) * 4;
                }
                self.live.push_back(placed);
                if n % 50 == 49 {
                    self.flush();
                }
            }
            self.flush();
        }
        fn unload_oldest(&mut self, count: usize) {
            for (v, i) in self.live.drain(..count).flatten() {
                self.vertices.release(v);
                self.indices.release(i);
            }
        }
    }

    /// Buildings and props of 64 to 6000 corners with a triangle per corner.
    fn props(count: usize, seed: &mut u32) -> Vec<(u32, u32)> {
        (0..count)
            .map(|_| {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 17;
                *seed ^= *seed << 5;
                let corners = 64 + *seed % 5936;
                (corners, corners * 3)
            })
            .collect()
    }

    /// The arena's GPU bytes for synthetic streamed worlds of many sizes:
    /// terrain tiles of 129 x 129 heights at four details and props loaded
    /// in bursts, then ten times the oldest tenth out and a new tenth in.
    /// One size can happen to fit a power of two well; over many the slack
    /// is smaller. Prints the totals (`--nocapture`).
    #[test]
    fn streamed_worlds_leave_less_of_the_arena_empty() {
        const MIB: f64 = 1048576.0;
        let (mut used, mut new, mut old, mut worlds) = (0.0, 0.0, 0.0, 0);
        for size in 1..=16u32 {
            let mut seed = 0x2545_f491 + size;
            let mut world = Simulated::new();
            let terrain: Vec<(u32, u32)> = (0..size)
                .flat_map(|_| [129u32, 65, 33, 17].map(|side| (side * side, (side - 1) * (side - 1) * 6)))
                .collect();
            world.load(&terrain);
            world.load(&props(100 * size as usize, &mut seed));
            for _ in 0..10 {
                let out = world.live.len() / 10;
                world.unload_oldest(out);
                world.load(&props(out, &mut seed));
            }
            let in_use = world.used().iter().sum::<u64>() as f64;
            let sized = world.sizes.iter().sum::<u64>() as f64;
            assert!(
                sized >= in_use && sized <= in_use * 1.57,
                "world {size}: {sized} bytes for {in_use}"
            );
            used += in_use;
            new += sized;
            old += world.power_of_two.iter().sum::<u64>() as f64;
            worlds += 1;
        }
        println!(
            "{worlds} streamed worlds, {:.1} MiB of geometry in their arenas: buffers of {:.1} MiB ({:.0}% spare), \
             with power-of-two growth {:.1} MiB ({:.0}% spare)",
            used / MIB,
            new / MIB,
            (new / used - 1.0) * 100.0,
            old / MIB,
            (old / used - 1.0) * 100.0,
        );
        assert!(new < old, "less spare room than with power-of-two growth");
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
        // Freeing the two big meshes at the end shrinks the buffers to what
        // the small ones need, keeping their geometry.
        let grown = arena.bytes();
        arena.release(bigger);
        arena.release(big);
        arena.flush(device, queue);
        assert!(
            arena.bytes() * 100 < grown,
            "shrunk: {grown} -> {} bytes",
            arena.bytes()
        );
        check(&arena, &b, &[0, 0, 1]);
        check(&arena, &reused, &[1, 2, 0]);
        // A mesh freed at the end before the flush that writes it is
        // skipped, not written past the buffer.
        let late = arena.upload(device, &vertices, &[2, 2, 2]).unwrap();
        let after_late = arena.upload(device, &vertices, &[1, 1, 1]).unwrap();
        arena.release(after_late);
        arena.flush(device, queue);
        check(&arena, &late, &[2, 2, 2]);
        arena.release(late);
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
