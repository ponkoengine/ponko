//! The GPU's own time for the uploads of the asset group and streaming
//! steps (textures and mesh buffers), and of textures made outside them
//! (`create_texture`), with timestamp queries.
//!
//! `Queue::write_texture` and `write_buffer` hand their bytes to `wgpu`,
//! which copies them on the GPU at the start of the next submission, in a
//! command buffer of its own that no timestamp can bracket (and
//! `create_buffer_init` fills a buffer mapped at creation, with no copy to
//! time at all). While a step's uploads are timed ([`activate`] to
//! [`deactivate`]), each texture write and mesh buffer write or creation on
//! this thread is made instead as `wgpu` makes it, through a staging
//! buffer, and the copy is recorded between two timestamps in a command
//! buffer submitted at once (so it still lands before anything submitted
//! later that reads it). The step's GPU time is the sum of those copies'
//! times: the GPU's work alone, without the idle time between them.
//!
//! Textures made outside the steps ([`UploadStep::Created`]) gather into
//! one span a frame: it stays open across their calls and is read back at
//! the start of the next frame.
//!
//! Results come back through a small ring of readback buffers, a frame or
//! a few later; if every buffer is still in flight, that step isn't timed.
//! GPUs without timestamp queries (most browsers today, some adapters) have
//! no timer: the stats say so.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::traffic::NewBuffer;

/// Which step the uploads were for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadStep {
    /// The asset group step.
    Groups,
    /// The streaming upload step.
    Streaming,
    /// Textures made outside the steps (`create_texture`), a frame's worth.
    Created,
}

const SLOTS: usize = 6;
/// The most texture writes timed per step; later ones go untimed.
const PAIRS: u32 = 64;
const QUERIES_PER_SLOT: u32 = PAIRS * 2;
const SLOT_BYTES: u64 = QUERIES_PER_SLOT as u64 * 8;
const FREE: u8 = 0;
const OPEN: u8 = 1;
const MAPPING: u8 = 2;
const READY: u8 = 3;

struct Slot {
    /// Its own queries, written from 0 (resolving a range that doesn't
    /// start at 0 read the wrong queries on Metal).
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Timestamp pairs written: one per timed copy.
    pairs: u32,
    step: UploadStep,
}

/// Times the copies of texture uploads on the GPU.
pub(crate) struct UploadTimer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// An empty compute shader: each timestamp's pass runs it once, since
    /// some GPUs (Metal) write no timestamps for an empty pass.
    marker: wgpu::ComputePipeline,
    slots: [Slot; SLOTS],
    /// Nanoseconds per timestamp tick.
    period: f64,
    /// The slot the step being timed writes to.
    current: Option<usize>,
    /// The slot this frame's [`UploadStep::Created`] copies gather in,
    /// open until [`collect`](Self::collect).
    created: Option<usize>,
    next: usize,
}

thread_local! {
    /// The timer of the step being timed on this thread.
    static ACTIVE: RefCell<Option<UploadTimer>> = const { RefCell::new(None) };
}

impl UploadTimer {
    /// `None` if the GPU can't do timestamp queries.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let slot = || Slot {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("upload timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: QUERIES_PER_SLOT,
            }),
            resolve: device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("upload timestamp resolve"),
                size: SLOT_BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("upload timestamp readback"),
                size: SLOT_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            pairs: 0,
            step: UploadStep::Groups,
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("upload timestamp marker"),
            source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
        });
        let marker = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("upload timestamp marker"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Some(Self {
            device: device.clone(),
            queue: queue.clone(),
            marker,
            slots: std::array::from_fn(|_| slot()),
            period: f64::from(queue.get_timestamp_period()),
            current: None,
            created: None,
            next: 0,
        })
    }

    /// Starts timing `step`'s uploads, if a readback slot is free (made
    /// textures carry on in this frame's slot for them, if there is one).
    fn begin(&mut self, step: UploadStep) {
        self.current = None;
        if step == UploadStep::Created
            && let Some(i) = self.created
        {
            self.current = Some(i);
            return;
        }
        for k in 0..SLOTS {
            let i = (self.next + k) % SLOTS;
            if self.slots[i].state.load(Ordering::Acquire) == FREE {
                let slot = &mut self.slots[i];
                slot.state.store(OPEN, Ordering::Release);
                slot.pairs = 0;
                slot.step = step;
                self.current = Some(i);
                self.next = (i + 1) % SLOTS;
                if step == UploadStep::Created {
                    self.created = Some(i);
                }
                return;
            }
        }
    }

    /// Ends the step: reads its timestamps back, if it timed any copies.
    /// Made textures' slot stays open for the rest of the frame, until it
    /// is full.
    fn end(&mut self) {
        let Some(i) = self.current.take() else {
            return;
        };
        if self.created == Some(i) {
            if self.slots[i].pairs < PAIRS {
                return;
            }
            self.created = None;
        }
        self.finish(i);
    }

    /// Reads slot `i`'s timestamps back, if it timed any copies.
    fn finish(&mut self, i: usize) {
        let slot = &mut self.slots[i];
        if slot.pairs == 0 {
            slot.state.store(FREE, Ordering::Release);
            return;
        }
        let used = slot.pairs * 2;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("upload timestamps"),
        });
        encoder.resolve_query_set(&slot.queries, 0..used, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.readback, 0, u64::from(used) * 8);
        self.queue.submit([encoder.finish()]);
        slot.state.store(MAPPING, Ordering::Release);
        let state = Arc::clone(&slot.state);
        slot.readback.map_async(wgpu::MapMode::Read, .., move |result| {
            state.store(if result.is_ok() { READY } else { FREE }, Ordering::Release);
        });
    }

    /// Writes `data` into `texture` as `Queue::write_texture` would, timing
    /// the copy: `false` (nothing done) when it can't be timed, so the
    /// caller writes it as usual.
    fn write_texture(
        &mut self,
        texture: &wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) -> bool {
        let Some(i) = self.current else {
            return false;
        };
        if self.slots[i].pairs >= PAIRS {
            return false;
        }
        let Some(rows) = Rows::of(texture, &layout, size, data.len()) else {
            return false;
        };
        let staging = self.device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("timed texture upload"),
            size: rows.padded * u64::from(rows.count) * u64::from(size.depth_or_array_layers),
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        {
            let Ok(mut mapped) = staging.get_mapped_range_mut(..) else {
                return false;
            };
            let mut at = 0usize;
            for layer in 0..u64::from(size.depth_or_array_layers) {
                for row in 0..u64::from(rows.count) {
                    let from = (layout.offset + (layer * u64::from(rows.per_image) + row) * rows.stride) as usize;
                    let to = at + rows.bytes as usize;
                    mapped
                        .slice(at..to)
                        .copy_from_slice(&data[from..from + rows.bytes as usize]);
                    at += rows.padded as usize;
                }
            }
        }
        staging.unmap();
        let texture = wgpu::TexelCopyTextureInfo {
            texture: texture.texture,
            mip_level: texture.mip_level,
            origin: texture.origin,
            aspect: texture.aspect,
        };
        self.timed_copy(i, "timed texture upload", |encoder| {
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(rows.padded as u32),
                        rows_per_image: Some(rows.count),
                    },
                },
                texture,
                size,
            );
        });
        #[cfg(test)]
        TIMED.with(|t| t.set([t.get()[0] + 1, t.get()[1]]));
        true
    }

    /// Writes `data` into `buffer` at `offset` as `Queue::write_buffer`
    /// would, timing the copy: `false` (nothing done) when it can't be
    /// timed (or isn't 4-byte aligned, as a copy must be), so the caller
    /// writes it as usual.
    fn write_buffer(&mut self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) -> bool {
        let Some(i) = self.current else {
            return false;
        };
        let align = wgpu::COPY_BUFFER_ALIGNMENT;
        let size = data.len() as u64;
        if self.slots[i].pairs >= PAIRS || size == 0 || !size.is_multiple_of(align) || !offset.is_multiple_of(align) {
            return false;
        }
        let staging = self.device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("timed buffer upload"),
            size,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        {
            let Ok(mut mapped) = staging.get_mapped_range_mut(..) else {
                return false;
            };
            mapped.slice(0..data.len()).copy_from_slice(data);
        }
        staging.unmap();
        self.timed_copy(i, "timed buffer upload", |encoder| {
            encoder.copy_buffer_to_buffer(&staging, 0, buffer, offset, size);
        });
        #[cfg(test)]
        TIMED.with(|t| t.set([t.get()[0], t.get()[1] + 1]));
        true
    }

    /// Records what `copy` records between two timestamps of slot `i`, and
    /// submits it at once.
    fn timed_copy(&mut self, i: usize, label: &str, copy: impl FnOnce(&mut wgpu::CommandEncoder)) {
        let at = self.slots[i].pairs * 2;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        let mut copy = Some(copy);
        // Passes that only write a timestamp: one before the copy, one
        // after it.
        for (begin, end) in [(Some(at), None), (None, Some(at + 1))] {
            if begin.is_none()
                && let Some(copy) = copy.take()
            {
                copy(&mut encoder);
            }
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("upload timestamp"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &self.slots[i].queries,
                    beginning_of_pass_write_index: begin,
                    end_of_pass_write_index: end,
                }),
            });
            pass.set_pipeline(&self.marker);
            pass.dispatch_workgroups(1, 1, 1);
        }
        self.queue.submit([encoder.finish()]);
        self.slots[i].pairs += 1;
    }

    /// Calls `done` with each step whose timestamps came back, and its GPU
    /// time in milliseconds. Never waits for the GPU.
    /// This frame's made textures' copies are read back from now on.
    pub fn collect(&mut self, mut done: impl FnMut(UploadStep, f32)) {
        if let Some(i) = self.created.take() {
            self.finish(i);
        }
        let _ = self.device.poll(wgpu::PollType::Poll);
        for slot in &mut self.slots {
            if slot.state.load(Ordering::Acquire) != READY {
                continue;
            }
            let ms = match slot.readback.get_mapped_range(..) {
                Ok(view) => {
                    let t: &[u64] = bytemuck::cast_slice(&view);
                    copy_time(&t[..slot.pairs as usize * 2], self.period)
                }
                Err(_) => None,
            };
            slot.readback.unmap();
            slot.state.store(FREE, Ordering::Release);
            if let Some(ms) = ms.filter(|ms| ms.is_finite() && *ms < 10_000.0) {
                done(slot.step, ms);
            }
        }
    }
}

/// The milliseconds of timestamp pairs (begin, end): their sum, leaving out
/// pairs the GPU didn't write sensibly (Metal now and then leaves one
/// unwritten); `None` if it wrote none.
fn copy_time(t: &[u64], period: f64) -> Option<f32> {
    let valid: Vec<u64> = t
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|[begin, end]| *begin != 0 && end >= begin)
        .map(|[begin, end]| end - begin)
        .collect();
    if valid.is_empty() {
        return None;
    }
    Some((valid.iter().sum::<u64>() as f64 * period / 1e6) as f32)
}

/// The rows of a texture write, as a staging buffer copy needs them.
struct Rows {
    /// Rows (of texels, or of blocks) per image.
    count: u32,
    /// Rows per image in the source data.
    per_image: u32,
    /// Bytes of each row's texels.
    bytes: u64,
    /// Bytes from one row to the next in the source data.
    stride: u64,
    /// Bytes per row in the staging buffer: `bytes` rounded up for the copy.
    padded: u64,
}

impl Rows {
    /// `None` for a write this can't lay out (then it is written untimed).
    fn of(
        texture: &wgpu::TexelCopyTextureInfo<'_>,
        layout: &wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
        len: usize,
    ) -> Option<Self> {
        let format = texture.texture.format();
        let (bw, bh) = format.block_dimensions();
        let block = u64::from(format.block_copy_size(Some(texture.aspect))?);
        let count = size.height.div_ceil(bh);
        let bytes = u64::from(size.width.div_ceil(bw)) * block;
        let stride = match layout.bytes_per_row {
            Some(stride) => u64::from(stride),
            None if count <= 1 && size.depth_or_array_layers <= 1 => bytes,
            None => return None,
        };
        let per_image = layout.rows_per_image.unwrap_or(count);
        let align = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let rows = Self {
            count,
            per_image,
            bytes,
            stride,
            padded: bytes.div_ceil(align) * align,
        };
        // Every row the copy reads is in `data`.
        let last = layout.offset
            + (u64::from(size.depth_or_array_layers.max(1) - 1) * u64::from(per_image) + u64::from(count.max(1) - 1))
                * stride
            + bytes;
        (count > 0 && bytes > 0 && stride >= bytes && last <= len as u64).then_some(rows)
    }
}

/// Times `timer`'s texture writes on this thread as `step`'s, until
/// [`deactivate`] gives it back.
pub(crate) fn activate(mut timer: UploadTimer, step: UploadStep) {
    timer.begin(step);
    ACTIVE.with(|active| *active.borrow_mut() = Some(timer));
}

/// Whether a timer is active on this thread.
pub(crate) fn active() -> bool {
    ACTIVE.with(|active| active.try_borrow().map_or(true, |a| a.is_some()))
}

/// Stops timing this thread's texture writes and gives the timer back.
pub(crate) fn deactivate() -> Option<UploadTimer> {
    let mut timer = ACTIVE.with(|active| active.borrow_mut().take())?;
    timer.end();
    Some(timer)
}

/// Whether the active timer on this thread can time another copy.
pub(crate) fn can_time() -> bool {
    ACTIVE.with(|active| {
        active.try_borrow().is_ok_and(|active| {
            active
                .as_ref()
                .is_some_and(|timer| timer.current.is_some_and(|i| timer.slots[i].pairs < PAIRS))
        })
    })
}

/// Writes into a buffer as the active timer times it; `false` when no timer
/// is active on this thread or it can't time this write.
pub(crate) fn write_buffer(buffer: &wgpu::Buffer, offset: u64, data: &[u8]) -> bool {
    ACTIVE.with(|active| {
        let Ok(mut active) = active.try_borrow_mut() else {
            return false;
        };
        active
            .as_mut()
            .is_some_and(|timer| timer.write_buffer(buffer, offset, data))
    })
}

#[cfg(test)]
thread_local! {
    /// Copies timed on this thread: textures, buffers.
    static TIMED: std::cell::Cell<[u32; 2]> = const { std::cell::Cell::new([0; 2]) };
}

/// The texture and buffer copies timed on this thread since the last call.
#[cfg(all(test, feature = "3d"))]
pub(crate) fn take_timed() -> [u32; 2] {
    TIMED.with(|t| t.replace([0; 2]))
}

/// Writes a texture as the active timer times it; `false` when no timer
/// is active on this thread or it can't time this write.
pub(crate) fn write_texture(
    texture: &wgpu::TexelCopyTextureInfo<'_>,
    data: &[u8],
    layout: wgpu::TexelCopyBufferLayout,
    size: wgpu::Extent3d,
) -> bool {
    ACTIVE.with(|active| {
        let Ok(mut active) = active.try_borrow_mut() else {
            return false;
        };
        active
            .as_mut()
            .is_some_and(|timer| timer.write_texture(texture, data, layout, size))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_times_add_up_the_pairs_the_gpu_wrote() {
        // Two copies of 1000 and 500 ticks, and a pair never written.
        let t = [100, 1100, 2000, 2500, 0, 0];
        assert!((copy_time(&t, 1.0).unwrap() - 0.0015).abs() < 1e-7);
        // A pair that ends before it begins (or never ends) is left out,
        // and a step with no pair written reports nothing.
        assert_eq!(copy_time(&[500, 400, 600, 0], 1.0), None);
    }
}
