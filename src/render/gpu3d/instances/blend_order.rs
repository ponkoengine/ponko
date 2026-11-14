//! See-through instance batches drawn far to near.
//!
//! A CPU-path batch with a see-through material keeps a copy of its
//! instances. Each frame the main camera moved, or its visible lists
//! changed, the visible instances of all its levels are sorted far to near
//! by their origins and uploaded in that order to a buffer of their own.
//! The transparent pass then draws them run by run: one draw per stretch
//! of instances of one level, so instances of overlapping levels
//! interleave correctly too.
//!
//! GPU-driven batches keep the batch's own order (see
//! [`BatchCulling::PreferGpu`](crate::render::BatchCulling::PreferGpu)).
//! Batches are drawn after the models: a batch is sorted within itself,
//! not against models or other batches.
use super::*;

/// What a see-through CPU-path batch keeps to draw its visible instances
/// far to near in the main view. Its vectors keep their capacity, so a
/// frame allocates nothing once the most visible instances were seen.
pub(crate) struct BlendOrder {
    /// The batch's instances, as uploaded.
    instances: Vec<Instance3D>,
    /// (level, instance) of each visible instance, level by level.
    items: Vec<(u32, u32)>,
    /// One key per item: the inverted bits of its squared distance (far
    /// first), then the item's index (ties keep the batch's order).
    keys: Vec<u64>,
    /// The sort's second buffer (see `depth_sort`).
    scratch: Vec<u64>,
    /// The visible instances, far to near.
    sorted: Vec<Instance3D>,
    /// Stretches of `sorted` of one level each: (level, start, count).
    pub(super) runs: Vec<(u32, u32, u32)>,
    /// `sorted` on the GPU.
    pub(super) buffer: Option<wgpu::Buffer>,
    /// The eye the order was made for; `None` when it must be made again.
    eye: Option<Vec3>,
}

impl BlendOrder {
    pub(super) fn new(instances: &[Instance3D]) -> Self {
        Self {
            instances: instances.to_vec(),
            items: Vec::new(),
            keys: Vec::new(),
            scratch: Vec::new(),
            sorted: Vec::new(),
            runs: Vec::new(),
            buffer: None,
            eye: None,
        }
    }

    /// The main view's visible lists changed: sort again this frame.
    pub(super) fn invalidate(&mut self) {
        self.eye = None;
    }

    /// Sorts what `visible` (the main view's lists, one per level) holds
    /// far to near from `eye`, unless that is what was sorted last, and
    /// uploads it. Past what one vertex buffer holds, nothing is sorted
    /// and the batch draws in its own order.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        visible: &[Vec<(u32, u32)>],
        eye: Vec3,
    ) {
        if self.eye == Some(eye) {
            return;
        }
        self.eye = Some(eye);
        self.sort(visible, eye);
        if self.sorted.is_empty() {
            return;
        }
        let bytes = (self.sorted.len() * size_of::<Instance3D>()) as u64;
        if bytes as usize > traffic::most_bytes(device, wgpu::BufferUsages::VERTEX) {
            self.runs.clear();
            return;
        }
        if self.buffer.as_ref().is_none_or(|b| b.size() < bytes) {
            // Room for every instance of one level at once, or more when
            // overlapping levels show some twice.
            let room = bytes.max((self.instances.len() * size_of::<Instance3D>()) as u64);
            self.buffer = Some(device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("static instance vertices, far to near"),
                size: room,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        queue.uploading(Upload::StaticBatches).write_buffer(
            self.buffer.as_ref().unwrap(),
            0,
            bytemuck::cast_slice(&self.sorted),
        );
    }

    /// Fills `sorted` and `runs` from `visible`, far to near from `eye`.
    fn sort(&mut self, visible: &[Vec<(u32, u32)>], eye: Vec3) {
        self.items.clear();
        self.keys.clear();
        self.sorted.clear();
        self.runs.clear();
        for (level, runs) in visible.iter().enumerate() {
            for &(start, count) in runs {
                for i in start..start + count {
                    let [x, y, z, _] = self.instances[i as usize].model[3];
                    let far = Vec3::new(x, y, z).distance_squared(eye);
                    // Distances are never negative, so their bits order like
                    // them; inverted, the farthest comes first.
                    let key = u64::from(!far.to_bits()) << 32 | self.items.len() as u64;
                    self.keys.push(key);
                    self.items.push((level as u32, i));
                }
            }
        }
        crate::three::depth_sort::sort_keys(&mut self.keys, &mut self.scratch);
        for (at, &key) in self.keys.iter().enumerate() {
            let (level, _) = self.items[(key & u64::from(u32::MAX)) as usize];
            match self.runs.last_mut() {
                Some((last, _, count)) if *last == level => *count += 1,
                _ => self.runs.push((level, at as u32, 1)),
            }
        }
        // Copying the instances in their new order reads them all over
        // memory: for a big batch, most of the work.
        let (items, instances) = (&self.items, &self.instances);
        let copy = |&key: &u64| instances[items[(key & u64::from(u32::MAX)) as usize].1 as usize];
        self.sorted.extend(self.keys.iter().map(copy));
    }

    /// The order last made: each instance's (level, index in the batch).
    #[cfg(test)]
    pub(crate) fn order(&self) -> Vec<(u32, u32)> {
        self.keys
            .iter()
            .map(|&key| self.items[(key & u64::from(u32::MAX)) as usize])
            .collect()
    }

    /// Counts the far-to-near copy as instance batches (see `memory.rs`).
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        if let Some(buffer) = &self.buffer {
            t.buffer(
                crate::render::memory::MemoryClass::InstanceBatches,
                "static batch blend order",
                buffer,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, z: f32) -> Instance3D {
        let mut model = glam::Mat4::from_translation(Vec3::new(x, 0.0, z)).to_cols_array_2d();
        model[0][0] = 1.0;
        Instance3D { model, color: [1.0; 4] }
    }

    #[test]
    fn visible_instances_sort_far_to_near_across_levels() {
        // Cell order: near, far, middle; level 1 shows one between them.
        let instances = [at(0.0, -1.0), at(0.0, -9.0), at(0.0, -5.0), at(0.0, -7.0)];
        let mut order = BlendOrder::new(&instances);
        order.sort(&[vec![(0, 3)], vec![(3, 1)]], Vec3::ZERO);
        assert_eq!(order.order(), [(0, 1), (1, 3), (0, 2), (0, 0)]);
        assert_eq!(order.runs, [(0, 0, 1), (1, 1, 1), (0, 2, 2)]);
        assert_eq!(order.sorted[0].model[3][2], -9.0);
        // From the other side, the other way round.
        order.sort(&[vec![(0, 3)], vec![(3, 1)]], Vec3::new(0.0, 0.0, -20.0));
        assert_eq!(order.order(), [(0, 0), (0, 2), (1, 3), (0, 1)]);
    }

    #[test]
    fn equally_far_instances_keep_the_batch_order() {
        let instances = [at(1.0, 0.0), at(-1.0, 0.0), at(0.0, 1.0)];
        let mut order = BlendOrder::new(&instances);
        order.sort(&[vec![(0, 3)]], Vec3::ZERO);
        assert_eq!(order.order(), [(0, 0), (0, 1), (0, 2)]);
        assert_eq!(order.runs, [(0, 0, 3)]);
    }
}
