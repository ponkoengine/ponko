//! Counts what the renderer sends to the GPU: bytes written into buffers
//! and textures, by kind, and buffers created. Every upload and buffer
//! creation in the renderer goes through these functions; the renderer
//! takes the counts once per drawn frame into `RenderStats::uploads`.
//!
//! The counts live in a thread-local: a renderer prepares, uploads and
//! takes them on one thread, and renderers on other threads (tests run
//! side by side) don't mix into each other's counts.

use std::cell::Cell;

use wgpu::util::DeviceExt;

use super::GpuUploads;

/// What an upload is for: the field of [`GpuUploads`] it counts into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) enum Upload {
    /// Per-frame 3D model records (transforms and colors).
    Instances,
    /// 2D sprite records.
    Sprites,
    /// Persistent instance batches and their culling data.
    StaticBatches,
    /// Small per-frame uniforms: cameras, post-processing, effect settings.
    Uniforms,
    /// Light and decal grids.
    Lights,
    /// Material parameters.
    Materials,
    /// Skinning: joint matrices, weights, posed vertices.
    Skins,
    /// Mesh vertices and indices, and game-made GPU buffers.
    Meshes,
    /// Texture pixels (images, probes).
    Textures,
    /// Everything else (particles, 3D text, occlusion grids, ...).
    Other,
}

thread_local! {
    static COUNTS: Cell<GpuUploads> = const { Cell::new(GpuUploads::ZERO) };
}

#[cfg(test)]
thread_local! {
    /// Upload calls: `write_buffer`, `write_texture`, `create_buffer_init`.
    /// In `wgpu` each write makes and maps a staging buffer of its own.
    static WRITES: Cell<[u32; 10]> = const { Cell::new([0; 10]) };
}

/// The upload calls since the last call on this thread, by [`Upload`] kind
/// (indexed `kind as usize`), which are then reset.
#[cfg(all(test, feature = "3d"))]
pub(crate) fn take_writes() -> [u32; 10] {
    WRITES.with(|w| w.replace([0; 10]))
}

fn count(f: impl FnOnce(&mut GpuUploads)) {
    COUNTS.with(|c| {
        let mut counts = c.get();
        f(&mut counts);
        c.set(counts);
    });
}

fn add(kind: Upload, bytes: u64) {
    #[cfg(test)]
    WRITES.with(|w| {
        let mut writes = w.get();
        writes[kind as usize] += 1;
        w.set(writes);
    });
    count(|c| {
        let field = match kind {
            Upload::Instances => &mut c.instances,
            Upload::Sprites => &mut c.sprites,
            Upload::StaticBatches => &mut c.static_batches,
            Upload::Uniforms => &mut c.uniforms,
            Upload::Lights => &mut c.lights,
            Upload::Materials => &mut c.materials,
            Upload::Skins => &mut c.skins,
            Upload::Meshes => &mut c.meshes,
            Upload::Textures => &mut c.textures,
            Upload::Other => &mut c.other,
        };
        *field += bytes;
    });
}

fn created(size: u64) {
    count(|c| {
        c.buffers_created += 1;
        c.buffer_bytes_created += size;
    });
}

/// Whether a buffer holding `capacity` items must be replaced to hold
/// `needed`; if so, sets `capacity` to the size to make it (`needed`
/// rounded up to a power of two, so a buffer that keeps growing is
/// replaced only a logarithmic number of times, but never past `most`,
/// the most the device's buffers hold) and counts a reallocation.
/// Capacities never shrink.
///
/// The caller then makes the new buffer, and everything that refers to the
/// old one (bind groups, a [`Mirror`]) anew: the old one is dropped. When
/// `needed` is more than `most`, `capacity` ends below it: the caller
/// draws only what fits, and says so (see [`too_big`]).
pub(crate) fn outgrown(capacity: &mut usize, needed: usize, most: usize) -> bool {
    if needed <= *capacity {
        return false;
    }
    let grown = needed.next_power_of_two().min(most);
    if grown <= *capacity {
        return false;
    }
    *capacity = grown;
    reallocated();
    true
}

/// The most bytes a buffer of `usage` can hold on `device`, a multiple of
/// 4: storage buffers are bound whole, so also no more than one binding.
pub(crate) fn most_bytes(device: &wgpu::Device, usage: wgpu::BufferUsages) -> usize {
    let limits = device.limits();
    let mut most = limits.max_buffer_size;
    if usage.contains(wgpu::BufferUsages::STORAGE) {
        most = most.min(limits.max_storage_buffer_binding_size);
    }
    usize::try_from(most).unwrap_or(usize::MAX) / 4 * 4
}

/// Says `message` where a developer sees it on every platform: stderr,
/// or the browser's console (stderr goes nowhere there).
pub(crate) fn warn(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(message));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{message}");
}

/// Reports (see [`warn`]) that `what` needed `needed` bytes, more than the
/// `most` this device's buffers hold, and what is drawn instead (`then`).
/// The caller reports once until it fits again, not every frame.
pub(crate) fn too_big(what: &str, needed: usize, most: usize, then: &str) {
    #[cfg(test)]
    TOO_BIG.with(|c| c.set(c.get() + 1));
    warn(&format!(
        "ponko: {what} need {needed} bytes, more than this GPU's buffers hold ({most}); {then}"
    ));
}

/// Like [`too_big`], for a count that isn't bytes: `what` are `needed`,
/// more than the `most` the device takes at once.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn too_many(what: &str, needed: u64, most: u64, then: &str) {
    #[cfg(test)]
    TOO_BIG.with(|c| c.set(c.get() + 1));
    warn(&format!(
        "ponko: {what}: {needed}, more than this GPU takes at once ({most}); {then}"
    ));
}

#[cfg(test)]
thread_local! {
    static TOO_BIG: Cell<u32> = const { Cell::new(0) };
}

/// The [`too_big`] reports since the last call on this thread.
#[cfg(all(test, feature = "3d"))]
pub(crate) fn take_too_big() -> u32 {
    TOO_BIG.with(|c| c.replace(0))
}

/// Counts a buffer or texture replaced by a bigger one (see [`outgrown`]).
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn reallocated() {
    count(|c| c.reallocations += 1);
}

/// Counted uploads and buffer creations: `device.new_buffer(&desc)` for
/// `create_buffer`, `device.uploading(kind).create_buffer_init(&desc)`,
/// and `queue.uploading(kind).write_buffer(..)` / `.write_texture(..)`.
pub(crate) trait Counted {
    /// This device or queue, counting what it uploads as `kind`.
    fn uploading(&self, kind: Upload) -> Uploading<'_, Self> {
        Uploading { to: self, kind }
    }
}
impl Counted for wgpu::Device {}
impl Counted for wgpu::Queue {}

/// A device or queue whose uploads count as one kind.
pub(crate) struct Uploading<'a, T: ?Sized> {
    to: &'a T,
    kind: Upload,
}

impl Uploading<'_, wgpu::Queue> {
    /// `Queue::write_buffer`, counted. While a [`WriteBatch`] gathers this
    /// thread's writes (see [`begin_batch`]), a small one joins it instead.
    pub fn write_buffer(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        add(self.kind, data.len() as u64);
        if batched(self.kind, buffer, offset, data) {
            return;
        }
        // wgpu's own allocations (a staging buffer a write) aren't the
        // caller's: tests that count a module's allocations leave them out.
        #[cfg(all(test, feature = "3d"))]
        let _wgpu = crate::three::lights::tests::unmarked();
        // Lands at the next submission: what is held must go first.
        submit_held();
        self.to.write_buffer(buffer, offset, data);
    }
    /// `Queue::write_texture`, counted. While a [`WriteBatch`] gathers this
    /// thread's writes, a small one of an uncompressed format joins it.
    pub fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        add(self.kind, data.len() as u64);
        if batched_texture(&texture, data, layout, size) {
            return;
        }
        submit_held();
        self.to.write_texture(texture, data, layout, size);
    }
}

/// The largest write a [`WriteBatch`] takes. A larger one is written on
/// its own (one staging buffer for it costs little next to its bytes, and
/// they aren't copied twice), unless its buffer already has batched writes
/// this frame: it then joins them, to land after them.
pub(crate) const BATCHED_WRITE_BYTES: usize = 64 * 1024;

/// A frame's small buffer writes, gathered into one upload.
///
/// `wgpu` makes, maps and frees a staging buffer for every
/// `Queue::write_buffer` (`wgpu-core`'s `Queue::write_buffer` makes a
/// `StagingBuffer` each call), with a few heap allocations each. While a
/// batch is active on a thread (see [`begin_batch`]), the writes made
/// there go into one byte list instead, and [`upload`](Self::upload)
/// writes it into one staging buffer kept from frame to frame;
/// [`record`](Self::record) then records a copy from it into each
/// destination, at the start of the frame's first command buffer (before
/// anything that reads those buffers). Nothing changes
/// on the GPU: the same bytes land in the same places, in the order they
/// were written, before the frame's passes run.
///
/// Persistent instance batches (`Upload::StaticBatches`) are written on
/// their own as before. A write too big for the batch
/// ([`BATCHED_WRITE_BYTES`]), unaligned, or made on another thread goes
/// straight to `wgpu` too: it lands ahead of the batch's copies, which is
/// the order it was made in unless its buffer already had a batched write,
/// in which case it joins the batch.
#[derive(Default)]
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) struct WriteBatch {
    bytes: Vec<u8>,
    copies: Vec<BatchedCopy>,
    staging: Option<wgpu::Buffer>,
    /// While command buffers are held (see [`hold`]): the hold this batch's
    /// staging buffer is being filled for, and the bytes of it the held
    /// buffers' copies still read. Each upload goes after them.
    used: (u64, u64),
}

/// One batched write: `size` bytes from `from` in the staging buffer to
/// `to`.
struct BatchedCopy {
    to: Destination,
    from: u64,
    size: u64,
}

/// Where a batched write goes.
enum Destination {
    /// At `offset` in a buffer.
    Buffer(wgpu::Buffer, u64),
    /// Into part of a texture, from rows of `bytes_per_row` (a multiple of
    /// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`) in the staging buffer.
    Texture {
        texture: wgpu::Texture,
        mip_level: u32,
        origin: wgpu::Origin3d,
        aspect: wgpu::TextureAspect,
        extent: wgpu::Extent3d,
        bytes_per_row: u32,
    },
}

impl Destination {
    fn is_buffer(&self, buffer: &wgpu::Buffer) -> bool {
        matches!(self, Self::Buffer(b, _) if b == buffer)
    }
    fn is_texture(&self, texture: &wgpu::Texture) -> bool {
        matches!(self, Self::Texture { texture: t, .. } if t == texture)
    }
}

thread_local! {
    static BATCH: std::cell::RefCell<Option<WriteBatch>> = const { std::cell::RefCell::new(None) };
}

/// Gathers this thread's buffer writes into `batch` until [`end_batch`].
/// `false` (and `batch` dropped) when a batch is already gathering here:
/// the outer one takes the writes.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn begin_batch(batch: WriteBatch) -> bool {
    BATCH.with(|b| {
        let mut b = b.borrow_mut();
        if b.is_some() {
            return false;
        }
        *b = Some(batch);
        true
    })
}

/// Stops gathering this thread's writes and gives the batch back.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn end_batch() -> WriteBatch {
    BATCH.with(|b| b.borrow_mut().take()).unwrap_or_default()
}

/// The unchanged bytes a uniform's [`Mirror::upload`] writes along between
/// two changed runs rather than starting another write, while a
/// [`WriteBatch`] gathers this thread's writes: there a write costs a copy
/// command (and `wgpu` a heap allocation for its barriers), and a few
/// kilobytes more of a uniform cost next to nothing. Records written per
/// model (instances, motion) keep uploading only what changed.
pub(crate) const BATCHED_BRIDGE_BYTES: usize = 4096;

/// [`BATCHED_BRIDGE_BYTES`] in pages of `page` bytes while a batch gathers
/// this thread's writes, otherwise 0.
fn batched_bridge(page: usize) -> usize {
    let gathering = BATCH.with(|b| b.try_borrow().is_ok_and(|b| b.is_some()));
    if gathering {
        BATCHED_BRIDGE_BYTES / page.max(1)
    } else {
        0
    }
}

/// Puts a write into this thread's batch, if one is gathering and the
/// write belongs there (see [`WriteBatch`]).
fn batched(kind: Upload, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) -> bool {
    let aligned = data.len().is_multiple_of(4) && offset.is_multiple_of(4) && !data.is_empty();
    BATCH.with(|b| {
        let Ok(mut b) = b.try_borrow_mut() else {
            return false;
        };
        let Some(batch) = b.as_mut() else {
            return false;
        };
        let joins = || batch.copies.iter().any(|c| c.to.is_buffer(buffer));
        // While command buffers are held (see `hold`), a write made on its
        // own would land ahead of them: every one that can joins.
        let own = (kind == Upload::StaticBatches || data.len() > BATCHED_WRITE_BYTES) && held_generation().is_none();
        if !aligned || own && !joins() {
            return false;
        }
        batch.push(buffer, offset, data);
        true
    })
}

/// [`batched`] for a texture write: one of a format of single-texel
/// blocks, whose rows are laid out again (padded to
/// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`) to be copied from the staging
/// buffer.
fn batched_texture(
    texture: &wgpu::TexelCopyTextureInfo<'_>,
    data: &[u8],
    layout: wgpu::TexelCopyBufferLayout,
    extent: wgpu::Extent3d,
) -> bool {
    let format = texture.texture.format();
    let Some(block) = format.block_copy_size(Some(texture.aspect)) else {
        return false;
    };
    if format.block_dimensions() != (1, 1)
        || extent.width == 0
        || extent.height == 0
        || extent.depth_or_array_layers == 0
    {
        return false;
    }
    let row = u64::from(extent.width) * u64::from(block);
    let src_row = layout.bytes_per_row.map_or(row, u64::from);
    let rows = u64::from(layout.rows_per_image.unwrap_or(extent.height));
    let (height, layers) = (u64::from(extent.height), u64::from(extent.depth_or_array_layers));
    // What wgpu would refuse is left for it to say.
    let last = layout.offset + ((layers - 1) * rows + height - 1) * src_row + row;
    if src_row < row || rows < height || last > data.len() as u64 {
        return false;
    }
    let padded = row.next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
    BATCH.with(|b| {
        let Ok(mut b) = b.try_borrow_mut() else {
            return false;
        };
        let Some(batch) = b.as_mut() else {
            return false;
        };
        let joins = || batch.copies.iter().any(|c| c.to.is_texture(texture.texture));
        let own = padded * height * layers > BATCHED_WRITE_BYTES as u64 && held_generation().is_none();
        if own && !joins() {
            return false;
        }
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        batch.bytes.resize(batch.bytes.len().next_multiple_of(align), 0);
        let from = batch.bytes.len() as u64;
        for layer in 0..layers {
            for y in 0..height {
                let at = (layout.offset + (layer * rows + y) * src_row) as usize;
                batch.bytes.extend_from_slice(&data[at..at + row as usize]);
                batch.bytes.resize(batch.bytes.len() + (padded - row) as usize, 0);
            }
        }
        batch.copies.push(BatchedCopy {
            to: Destination::Texture {
                texture: texture.texture.clone(),
                mip_level: texture.mip_level,
                origin: texture.origin,
                aspect: texture.aspect,
                extent,
                bytes_per_row: padded as u32,
            },
            from,
            size: padded * height * layers,
        });
        true
    })
}

#[cfg_attr(not(feature = "3d"), allow(dead_code))]
impl WriteBatch {
    fn push(&mut self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        let from = self.bytes.len() as u64;
        let size = data.len() as u64;
        self.bytes.extend_from_slice(data);
        // A write right after the last one, into the same buffer: one copy.
        if let Some(last) = self.copies.last_mut()
            && let Destination::Buffer(to, at) = &last.to
            && to == buffer
            && at + last.size == offset
            && last.from + last.size == from
        {
            last.size += size;
            return;
        }
        self.copies.push(BatchedCopy {
            to: Destination::Buffer(buffer.clone(), offset),
            from,
            size,
        });
    }

    /// Uploads the gathered bytes in one write into the staging buffer
    /// (made, or made bigger, when they don't fit). Their copies into place
    /// wait for [`record`](Self::record), which must come before anything
    /// reads those buffers and before the next `upload`, unless command
    /// buffers are held (see [`hold`]): then each upload of the hold goes
    /// after the last one in the staging buffer, as copies of earlier ones
    /// wait in held buffers.
    pub(crate) fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.copies.is_empty() {
            self.bytes.clear();
            return;
        }
        let hold = held_generation();
        // Texture copies read rows from a multiple of the row alignment.
        let mut at = match hold {
            Some(hold) if self.used.0 == hold => self
                .used
                .1
                .next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)),
            _ => 0,
        };
        let len = self.bytes.len() as u64;
        if self.staging.as_ref().is_none_or(|s| s.size() < at + len) {
            // Held copies keep reading the old one; this upload starts the
            // new one, which fits a whole hold's uploads next time.
            self.staging = Some(device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("frame writes"),
                size: (at + len).next_power_of_two().max(4096),
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            at = 0;
        }
        if let Some(hold) = hold {
            self.used = (hold, at + len);
        }
        if let Some(staging) = &self.staging {
            // Counted when each write was gathered.
            queue.write_buffer(staging, at, &self.bytes);
        }
        if at > 0 {
            for copy in &mut self.copies {
                copy.from += at;
            }
        }
        self.bytes.clear();
    }

    /// Whether uploaded writes wait for [`record`](Self::record).
    pub(crate) fn has_copies(&self) -> bool {
        !self.copies.is_empty()
    }

    /// Records the uploaded writes' copies into place, first in `encoder`.
    pub(crate) fn record(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(staging) = &self.staging {
            for copy in &self.copies {
                match &copy.to {
                    Destination::Buffer(to, offset) => {
                        encoder.copy_buffer_to_buffer(staging, copy.from, to, *offset, copy.size);
                    }
                    Destination::Texture {
                        texture,
                        mip_level,
                        origin,
                        aspect,
                        extent,
                        bytes_per_row,
                    } => encoder.copy_buffer_to_texture(
                        wgpu::TexelCopyBufferInfo {
                            buffer: staging,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: copy.from,
                                bytes_per_row: Some(*bytes_per_row),
                                rows_per_image: Some(extent.height),
                            },
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture,
                            mip_level: *mip_level,
                            origin: *origin,
                            aspect: *aspect,
                        },
                        *extent,
                    ),
                }
            }
        }
        self.copies.clear();
    }
}

/// Command buffers held back to be submitted together (see [`hold`]).
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
struct Held {
    queue: wgpu::Queue,
    buffers: Vec<wgpu::CommandBuffer>,
    /// Command buffers held since [`hold`], submitted or not.
    held: usize,
    /// Which hold this is (never 0), for [`WriteBatch::upload`].
    generation: u64,
    /// Submissions made while holding: one at the end, and one each time
    /// a write that cannot wait in a batch made them go early.
    submits: u32,
}

thread_local! {
    static HELD: std::cell::RefCell<Option<Held>> = const { std::cell::RefCell::new(None) };
    static HOLDS: Cell<u64> = const { Cell::new(0) };
}

/// Holds the command buffers this thread submits through [`submit`] back,
/// for [`release`] to submit together: several frames (a window's views)
/// recorded one after another, in one submission instead of one each.
///
/// Nothing changes on the GPU: the buffers run in the order they were
/// made, and every write lands where it would have. A frame's buffer
/// writes are made at the next submission, ahead of every buffer in it, so
/// while buffers are held each write that can is gathered into the
/// preparing frame's [`WriteBatch`] (whose copies its own first buffer
/// records); one that cannot (unaligned, a texture's, made outside a
/// batch) submits what is held first.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn hold(queue: &wgpu::Queue) {
    let generation = HOLDS.with(|h| {
        h.set(h.get() + 1);
        h.get()
    });
    HELD.with(|held| {
        *held.borrow_mut() = Some(Held {
            queue: queue.clone(),
            buffers: Vec::new(),
            held: 0,
            generation,
            submits: 0,
        });
    });
}

/// The hold this thread is in, if it holds command buffers.
fn held_generation() -> Option<u64> {
    HELD.with(|h| h.try_borrow().ok()?.as_ref().map(|h| h.generation))
}

/// Submits `buffers`, or holds them after the ones held (see [`hold`]).
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn submit(queue: &wgpu::Queue, buffers: impl IntoIterator<Item = wgpu::CommandBuffer>) {
    let buffers = HELD.with(|h| match h.borrow_mut().as_mut() {
        Some(held) => {
            let before = held.buffers.len();
            held.buffers.extend(buffers);
            held.held += held.buffers.len() - before;
            None
        }
        None => Some(buffers),
    });
    if let Some(buffers) = buffers {
        queue.submit(buffers);
    }
}

/// Submits a copy that `record` records into a new encoder (`label`), after every
/// buffer write this thread made so far: a buffer about to be replaced by
/// a bigger one, copied over. Writes a batch gathered (see [`WriteBatch`])
/// are recorded first in it while buffers are held (otherwise they would
/// land after it, in the frame's first buffer); without a hold, writes made
/// on their own already land first, at its submission.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn submit_copy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: Option<&str>,
    record: impl FnOnce(&mut wgpu::CommandEncoder),
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label });
    if held_generation().is_some() {
        BATCH.with(|b| {
            if let Ok(mut b) = b.try_borrow_mut()
                && let Some(batch) = b.as_mut()
            {
                batch.upload(device, queue);
                batch.record(&mut encoder);
            }
        });
    }
    record(&mut encoder);
    submit(queue, [encoder.finish()]);
}

/// Submits what is held now, and keeps holding.
fn submit_held() {
    HELD.with(|h| {
        let Ok(mut h) = h.try_borrow_mut() else { return };
        if let Some(held) = h.as_mut().filter(|h| !h.buffers.is_empty()) {
            held.submits += 1;
            held.queue.submit(held.buffers.drain(..));
        }
    });
}

/// Ends the hold: submits what is held, in one submission. Gives the
/// command buffers and submissions made since [`hold`].
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn release() -> (usize, u32) {
    let Some(mut held) = HELD.with(|h| h.borrow_mut().take()) else {
        return (0, 0);
    };
    if !held.buffers.is_empty() {
        held.submits += 1;
        held.queue.submit(held.buffers);
    }
    (held.held, held.submits)
}

#[cfg_attr(not(feature = "3d"), allow(dead_code))]
impl Uploading<'_, wgpu::Device> {
    /// `DeviceExt::create_buffer_init`, counted: a buffer created, and its
    /// contents uploaded.
    pub fn create_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor<'_>) -> wgpu::Buffer {
        let buffer = self.to.create_buffer_init(desc);
        created(buffer.size());
        add(self.kind, desc.contents.len() as u64);
        buffer
    }
}

/// `Device::create_buffer`, counted.
pub(crate) trait NewBuffer {
    fn new_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> wgpu::Buffer;
}
impl NewBuffer for wgpu::Device {
    fn new_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> wgpu::Buffer {
        created(desc.size);
        self.create_buffer(desc)
    }
}

/// The most writes a frame makes into one buffer whose changes can be
/// scattered (see [`Mirror::sync_capped`]).
///
/// `wgpu` makes and maps a staging buffer for every `write_buffer`. On an
/// Apple M4 Max (Metal) a write costs the CPU about 2 µs, as much as
/// copying about 30 KB more (`upload_bench`: 64 writes of 64 bytes take
/// 150 µs, one write of 256 KB 37 µs), so closing gaps narrower than that
/// saves time. In the benchmark's scattered scene (every 17th of 1,024
/// models and every other of 200 emitters changing) preparing a frame
/// took 0.56 ms uncapped (241 writes), 0.37 ms with 64 writes a list,
/// 0.27 ms with 32 and 0.17 ms with 16 (37 writes), for 98, 208, 264 and
/// 292 KB uploaded. A few changes still upload only themselves.
pub(crate) const MAX_WRITES: usize = 16;

/// The usage a buffer written through a [`Mirror`] adds: in tests, it can
/// be read back to check that it holds what the mirror says
/// (`Renderer3D::check_mirrors`); otherwise none.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) const MIRRORED: wgpu::BufferUsages = if cfg!(test) {
    wgpu::BufferUsages::COPY_SRC
} else {
    wgpu::BufferUsages::empty()
};

/// What a GPU buffer (or texture) holds, kept on the CPU, so a frame
/// writes only the parts that changed since the last.
///
/// Forget it whenever the GPU side is replaced (a buffer outgrown and made
/// anew), or the next writes would be skipped against the old contents.
#[derive(Default)]
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) struct Mirror {
    bytes: Vec<u8>,
    /// Scratch for `sync_capped`: the runs of changed pages, and the gaps
    /// between them.
    runs: Vec<(usize, usize)>,
    gaps: Vec<(usize, usize)>,
}

#[cfg_attr(not(feature = "3d"), allow(dead_code))]
impl Mirror {
    /// Brings `buffer`'s bytes `at..at + data.len()` (whose contents this
    /// mirrors) up to `data`, counted as `kind`: a uniform, compared a vec4
    /// at a time, so a frame uploads only the values that changed (with
    /// up to eight unchanged vec4s between two changed ones along, or up to
    /// [`BATCHED_BRIDGE_BYTES`] while a [`WriteBatch`] gathers the writes).
    pub fn upload(&mut self, queue: &wgpu::Queue, kind: Upload, buffer: &wgpu::Buffer, at: u64, data: &[u8]) {
        let uploads = queue.uploading(kind);
        // While writes are batched, a write costs a copy command: a few
        // kilobytes of a uniform along cost less than another one.
        let bridge = 8.max(batched_bridge(16));
        self.sync_bridged(0, data, 16, bridge, |offset, bytes| {
            uploads.write_buffer(buffer, at + offset as u64, bytes);
        });
    }

    /// The GPU side's contents are unknown (a new buffer): write everything next.
    pub fn forget(&mut self) {
        self.bytes.clear();
    }

    /// Brings bytes `offset..offset + new.len()` up to `new`: calls
    /// `write(at, bytes)` for each run of changed `page`-sized pages (one
    /// call per run of neighbouring ones). Writes past what is known are
    /// always made. `offset` and `page` keep `wgpu`'s 4-byte copy alignment
    /// when they and `new.len()` are multiples of 4.
    pub fn sync(&mut self, offset: usize, new: &[u8], page: usize, write: impl FnMut(usize, &[u8])) {
        self.sync_bridged(offset, new, page, 0, write);
    }

    /// Like [`sync`](Self::sync), but changed pages with up to `bridge`
    /// unchanged ones between them go in one write (the unchanged along
    /// with them): small pages then cost no more writes than large ones,
    /// while a lone change still uploads one page.
    pub fn sync_bridged(
        &mut self,
        offset: usize,
        new: &[u8],
        page: usize,
        bridge: usize,
        mut write: impl FnMut(usize, &[u8]),
    ) {
        let page = page.max(1);
        if offset > self.bytes.len() {
            // A gap of unknown bytes: only what's contiguous is mirrored.
            write(offset, new);
            return;
        }
        let end = offset + new.len();
        let known = self.bytes.len().min(end) - offset;
        // The run of pages to write: where it starts, and where its last
        // changed page ends.
        let mut run: Option<(usize, usize)> = None;
        let mut at = 0;
        while at < new.len() {
            let to = (at + page).min(new.len());
            let same = to <= known && self.bytes[offset + at..offset + to] == new[at..to];
            if !same {
                run = match run {
                    Some((start, last)) if at - last <= bridge * page => Some((start, to)),
                    Some((start, last)) => {
                        write(offset + start, &new[start..last]);
                        Some((at, to))
                    }
                    None => Some((at, to)),
                };
            }
            at = to;
        }
        if let Some((start, last)) = run {
            write(offset + start, &new[start..last]);
        }
        self.remember(offset, new);
    }

    /// Like [`sync_bridged`](Self::sync_bridged), but in at most
    /// `max_writes` writes: when more runs of pages changed, the runs with
    /// the narrowest gaps of unchanged bytes between them go in one write
    /// (the gap along), so what is uploaded beyond the changes is as
    /// little as that many writes allow. A few changes cost little, and
    /// changes scattered all over cost no more writes than `max_writes`,
    /// nor more bytes than writing all.
    ///
    /// Every write costs a staging buffer (see [`MAX_WRITES`]), so buffers
    /// whose changes can be scattered (one record per model, particle,
    /// joint) are written through this.
    pub fn sync_capped(
        &mut self,
        offset: usize,
        new: &[u8],
        (page, bridge): (usize, usize),
        max_writes: usize,
        mut write: impl FnMut(usize, &[u8]),
    ) {
        let page = page.max(1);
        if offset > self.bytes.len() {
            write(offset, new);
            return;
        }
        let end = offset + new.len();
        let known = self.bytes.len().min(end) - offset;
        let mut runs = std::mem::take(&mut self.runs);
        runs.clear();
        let mut at = 0;
        while at < new.len() {
            let to = (at + page).min(new.len());
            let same = to <= known && self.bytes[offset + at..offset + to] == new[at..to];
            if !same {
                match runs.last_mut() {
                    Some(run) if at - run.1 <= bridge * page => run.1 = to,
                    _ => runs.push((at, to)),
                }
            }
            at = to;
        }
        let splits = max_writes.max(1) - 1;
        if runs.len() > splits + 1 {
            // Keep the widest gaps (by the run before them), close the rest.
            let mut gaps = std::mem::take(&mut self.gaps);
            gaps.clear();
            gaps.extend(runs.windows(2).enumerate().map(|(i, w)| (w[1].0 - w[0].1, i)));
            if splits > 0 {
                gaps.select_nth_unstable_by(splits - 1, |a, b| b.cmp(a));
            }
            let kept = &mut gaps[..splits];
            kept.sort_unstable_by_key(|g| g.1);
            let mut start = runs[0].0;
            for &(_, i) in kept.iter() {
                write(offset + start, &new[start..runs[i].1]);
                start = runs[i + 1].0;
            }
            let last = runs[runs.len() - 1].1;
            write(offset + start, &new[start..last]);
            self.gaps = gaps;
        } else {
            for &(start, last) in &runs {
                write(offset + start, &new[start..last]);
            }
        }
        self.runs = runs;
        self.remember(offset, new);
    }

    fn remember(&mut self, offset: usize, new: &[u8]) {
        let end = offset + new.len();
        if end > self.bytes.len() {
            self.bytes.resize(end, 0);
        }
        self.bytes[offset..end].copy_from_slice(new);
    }
}

/// A GPU buffer together with its [`Mirror`]: writes upload only the pages
/// that changed, and the buffer grows (see [`outgrown`]) with its mirror
/// forgotten in the same step, so no write is ever skipped against
/// contents a replaced buffer held.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) struct Synced {
    buffer: wgpu::Buffer,
    mirror: Mirror,
    /// How many bytes it holds.
    capacity: usize,
    label: &'static str,
    usage: wgpu::BufferUsages,
    kind: Upload,
    page: usize,
    bridge: usize,
    max_writes: usize,
    /// The most bytes the device lets it hold (see [`most_bytes`]).
    most: usize,
    /// More was asked for than `most`: reported, not yet fitting again.
    full: bool,
}

#[cfg_attr(not(feature = "3d"), allow(dead_code))]
impl Synced {
    /// A buffer of `bytes` (at least 4) whose writes count as `kind`, compared in
    /// `page`-byte pages (a multiple of 4), joined across up to `bridge`
    /// unchanged pages, and at most `max_writes` a frame (see
    /// [`Mirror::sync_capped`]).
    pub fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        bytes: usize,
        kind: Upload,
        (page, bridge, max_writes): (usize, usize, usize),
    ) -> Self {
        let most = most_bytes(device, usage);
        let capacity = bytes.max(4).next_multiple_of(4).min(most);
        Self {
            buffer: Self::make(device, label, usage, capacity),
            mirror: Mirror::default(),
            capacity,
            label,
            usage,
            kind,
            page,
            bridge,
            max_writes,
            most,
            full: false,
        }
    }

    fn make(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages, bytes: usize) -> wgpu::Buffer {
        device.new_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST | MIRRORED,
            mapped_at_creation: false,
        })
    }

    /// The buffer, as it is until the next [`reserve`](Self::reserve) that
    /// returns true.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Makes room for `bytes`, or as many as the device allows (see
    /// [`fits`](Self::fits); more is reported once). True when that took a
    /// new buffer: whatever refers to the old one (bind groups) must be made
    /// again.
    pub fn reserve(&mut self, device: &wgpu::Device, bytes: usize) -> bool {
        let grew = outgrown(&mut self.capacity, bytes, self.most);
        if grew {
            self.buffer = Self::make(device, self.label, self.usage, self.capacity);
            self.mirror.forget();
        }
        let full = bytes > self.capacity;
        if full && !self.full {
            too_big(self.label, bytes, self.most, "drawing only what fits");
        }
        self.full = full;
        grew
    }

    /// How many bytes it holds: writes past them are dropped.
    pub fn fits(&self) -> usize {
        self.capacity
    }

    /// Brings bytes `offset..offset + data.len()` up to `data`, uploading
    /// only the pages that changed, in at most its `max_writes` writes. What
    /// doesn't fit (see [`reserve`](Self::reserve)) is dropped; `offset` and
    /// `data.len()` are multiples of 4.
    pub fn write(&mut self, queue: &wgpu::Queue, offset: usize, data: &[u8]) {
        let data = &data[..data.len().min(self.capacity.saturating_sub(offset))];
        let (buffer, uploads) = (&self.buffer, queue.uploading(self.kind));
        let paging = (self.page, self.bridge);
        self.mirror
            .sync_capped(offset, data, paging, self.max_writes, |at, bytes| {
                uploads.write_buffer(buffer, at as u64, bytes);
            });
    }
}

#[cfg(all(test, feature = "3d"))]
impl Mirror {
    /// What the buffer holds, as far as this mirror knows.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Panics unless `buffer` holds, from `at`, what this mirror says it
    /// does: no write was skipped against contents it doesn't have.
    pub fn check(&self, device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, at: u64, what: &str) {
        if self.bytes.is_empty() {
            return;
        }
        let held = read_back(device, queue, buffer, at, self.bytes.len() as u64);
        let differ = held.iter().zip(&self.bytes).filter(|(a, b)| a != b).count();
        assert_eq!(
            differ,
            0,
            "{what}: {differ} of {} bytes differ from what was written",
            held.len()
        );
    }
}

#[cfg(all(test, feature = "3d"))]
impl Synced {
    /// See [`Mirror::check`].
    pub fn check(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.mirror.check(device, queue, &self.buffer, 0, self.label);
    }
}

/// Reads `len` bytes of `buffer` from `at` back from the GPU.
#[cfg(all(test, feature = "3d"))]
pub(crate) fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    at: u64,
    len: u64,
) -> Vec<u8> {
    let size = len.next_multiple_of(4);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read back"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, at, &readback, 0, size);
    queue.submit([encoder.finish()]);
    readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = readback.get_mapped_range(..).unwrap().to_vec();
    bytes[..len as usize].to_vec()
}

/// The counts since the last call on this thread, which are then reset.
pub(crate) fn take() -> GpuUploads {
    COUNTS.with(|c| c.replace(GpuUploads::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_add_up_by_kind_and_reset_when_taken() {
        take();
        add(Upload::Instances, 64);
        add(Upload::Instances, 16);
        add(Upload::Lights, 8);
        created(256);
        let c = take();
        assert_eq!((c.instances, c.lights, c.bytes()), (80, 8, 88));
        assert_eq!((c.buffers_created, c.buffer_bytes_created), (1, 256));
        assert_eq!(take(), GpuUploads::default());
    }

    fn writes(mirror: &mut Mirror, offset: usize, new: &[u8]) -> Vec<(usize, Vec<u8>)> {
        let mut out = Vec::new();
        mirror.sync(offset, new, 4, |at, b| out.push((at, b.to_vec())));
        out
    }

    #[test]
    fn mirrors_write_only_changed_pages() {
        let mut m = Mirror::default();
        let first: Vec<u8> = (0..16).collect();
        assert_eq!(writes(&mut m, 0, &first), [(0, first.clone())], "unknown: all of it");
        assert!(writes(&mut m, 0, &first).is_empty(), "unchanged: nothing");
        let mut second = first.clone();
        second[5] = 99;
        second[6] = 98;
        second[13] = 97;
        assert_eq!(
            writes(&mut m, 0, &second),
            [(4, second[4..8].to_vec()), (12, second[12..16].to_vec())]
        );
        // Neighbouring changed pages are one write; a longer tail is new.
        let mut third = second.clone();
        third[0] = 1;
        third.extend_from_slice(&[7, 7, 7, 7]);
        assert_eq!(writes(&mut m, 0, &third), [(0, third[0..4].to_vec()), (16, vec![7; 4])]);
        assert_eq!(writes(&mut m, 8, &[9; 6]), [(8, vec![9; 6])]);
        let all = m.bytes.clone();
        assert!(writes(&mut m, 0, &all).is_empty());
    }

    #[test]
    fn bridged_writes_take_the_few_unchanged_pages_between_changes_along() {
        let mut m = Mirror::default();
        let first = vec![0u8; 40];
        m.sync(0, &first, 4, |_, _| {});
        // Pages 1, 3, 7 and 9 change: one unchanged page lies between the
        // first two and the last two, three between the pairs.
        let mut second = first.clone();
        for page in [1, 3, 7, 9] {
            second[page * 4] = 1;
        }
        let mut out = Vec::new();
        m.sync_bridged(0, &second, 4, 1, |at, b| out.push((at, b.len())));
        assert_eq!(out, [(4, 12), (28, 12)], "runs end with their last changed page");
        let mut third = second.clone();
        third[0] = 2;
        third[39] = 2;
        out.clear();
        m.sync_bridged(0, &third, 4, 8, |at, b| out.push((at, b.len())));
        assert_eq!(out, [(0, 40)], "a wide bridge joins them all");
        out.clear();
        m.sync_bridged(0, &third, 4, 8, |at, b| out.push((at, b.len())));
        assert!(out.is_empty(), "and what was bridged is known");
    }

    #[test]
    fn buffers_grow_by_powers_of_two_up_to_the_device_limit() {
        take();
        let mut capacity = 16;
        assert!(!outgrown(&mut capacity, 16, 1000), "it fits");
        assert!(outgrown(&mut capacity, 17, 1000));
        assert_eq!(capacity, 32);
        assert!(outgrown(&mut capacity, 600, 1000), "not rounded past the limit");
        assert_eq!(capacity, 1000);
        assert!(!outgrown(&mut capacity, 5000, 1000), "nothing bigger to make");
        assert_eq!(capacity, 1000);
        assert_eq!(take().reallocations, 2);
    }

    #[test]
    fn capped_writes_close_the_narrowest_gaps_first() {
        let mut m = Mirror::default();
        let first = vec![0u8; 64];
        m.sync(0, &first, 4, |_, _| {});
        // Pages 0, 2, 3, 10 and 15 change: gaps of 1, 6 and 4 pages.
        let mut second = first.clone();
        for page in [0, 2, 3, 10, 15] {
            second[page * 4] = 1;
        }
        let mut out = Vec::new();
        m.clone_from_bytes(&first);
        m.sync_capped(0, &second, (4, 0), 8, |at, b| out.push((at, b.len())));
        assert_eq!(out, [(0, 4), (8, 8), (40, 4), (60, 4)], "under the cap: each run alone");
        m.clone_from_bytes(&first);
        out.clear();
        m.sync_capped(0, &second, (4, 1), 8, |at, b| out.push((at, b.len())));
        assert_eq!(out, [(0, 16), (40, 4), (60, 4)], "bridged across one page");
        for (cap, expected) in [
            (3, vec![(0, 16), (40, 4), (60, 4)]),
            (2, vec![(0, 16), (40, 24)]),
            (1, vec![(0, 64)]),
        ] {
            m.clone_from_bytes(&first);
            out.clear();
            m.sync_capped(0, &second, (4, 0), cap, |at, b| out.push((at, b.len())));
            assert_eq!(out, expected, "{cap} writes");
        }
        out.clear();
        m.sync_capped(0, &second, (4, 0), 1, |at, b| out.push((at, b.len())));
        assert!(out.is_empty(), "what was written is known");
        // Past what is known, everything.
        out.clear();
        m.sync_capped(80, &[1; 8], (4, 0), 4, |at, b| out.push((at, b.len())));
        assert_eq!(out, [(80, 8)]);
    }

    impl Mirror {
        fn clone_from_bytes(&mut self, bytes: &[u8]) {
            self.bytes.clear();
            self.bytes.extend_from_slice(bytes);
        }
    }

    #[test]
    fn forgotten_and_far_mirrors_write_everything() {
        let mut m = Mirror::default();
        writes(&mut m, 0, &[1; 8]);
        m.forget();
        assert_eq!(writes(&mut m, 0, &[1; 8]), [(0, vec![1; 8])]);
        assert_eq!(writes(&mut m, 20, &[2; 4]), [(20, vec![2; 4])], "past a gap");
        assert_eq!(writes(&mut m, 20, &[2; 4]), [(20, vec![2; 4])], "still unknown");
    }
}
