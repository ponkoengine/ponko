//! Offscreen: a frame's buffer writes gathered into one upload
//! (`traffic::WriteBatch`) land as the same writes made one by one would,
//! in the order they were made, with the writes that stay outside the batch
//! (large ones, persistent instance batches') in their place too. Skips
//! without a GPU.
use super::render_test_support::gpu;
use super::traffic::{self, BATCHED_WRITE_BYTES, Counted, Upload, WriteBatch};

/// A buffer of `len` zero bytes that writes and copies can go into.
fn buffer(device: &wgpu::Device, len: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: len,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

/// Each write: (buffer index, kind, offset, bytes).
type Write = (usize, Upload, u64, Vec<u8>);

/// The buffers' contents after `writes`, gathered into a batch (`batched`)
/// or each made on its own.
fn written(writes: &[Write], batched: bool) -> Option<Vec<Vec<u8>>> {
    let gpu = gpu()?;
    let (device, queue) = (&gpu.device, &gpu.queue);
    let len = 4 * BATCHED_WRITE_BYTES as u64;
    let buffers: Vec<wgpu::Buffer> = (0..3).map(|_| buffer(device, len)).collect();
    if batched {
        assert!(traffic::begin_batch(WriteBatch::default()));
        // A second batch on the same thread leaves the writes to the first.
        assert!(!traffic::begin_batch(WriteBatch::default()));
    }
    for (i, kind, offset, bytes) in writes {
        queue.uploading(*kind).write_buffer(&buffers[*i], *offset, bytes);
    }
    if batched {
        let mut batch = traffic::end_batch();
        batch.upload(device, queue);
        assert!(batch.has_copies(), "the small writes were gathered");
        let mut encoder = device.create_command_encoder(&Default::default());
        batch.record(&mut encoder);
        assert!(!batch.has_copies());
        queue.submit([encoder.finish()]);
    }
    traffic::take();
    Some(
        buffers
            .iter()
            .map(|b| traffic::read_back(device, queue, b, 0, len))
            .collect(),
    )
}

#[test]
fn batched_writes_land_as_writes_made_one_by_one() {
    let big = |v: u8| vec![v; BATCHED_WRITE_BYTES + 16];
    let writes: Vec<Write> = vec![
        // Neighbours (one copy), then one over both of them: the later wins.
        (0, Upload::Uniforms, 0, vec![1; 16]),
        (0, Upload::Uniforms, 16, vec![2; 16]),
        (0, Upload::Instances, 8, vec![3; 16]),
        // Too big for the batch, into a buffer it has writes for: it joins
        // them, so it still lands after them (over this one).
        (0, Upload::Uniforms, 64, vec![10; 16]),
        (0, Upload::Instances, 64, big(4)),
        // Too big, into a buffer of its own: written on its own.
        (1, Upload::Meshes, 64, big(5)),
        // After that big write, into the same buffer: lands over it.
        (1, Upload::Lights, 64, vec![6; 32]),
        // Persistent instance batches are written on their own...
        (2, Upload::StaticBatches, 0, vec![7; 64]),
        // ...and small writes after them land over them.
        (2, Upload::Other, 32, vec![8; 8]),
    ];
    let (Some(one_by_one), Some(batched)) = (written(&writes, false), written(&writes, true)) else {
        return;
    };
    for (i, (a, b)) in one_by_one.iter().zip(&batched).enumerate() {
        let differ = a.iter().zip(b).filter(|(x, y)| x != y).count();
        assert_eq!(differ, 0, "buffer {i}: {differ} bytes differ");
    }
    // A negative control: the same writes in another order differ.
    let mut reordered = writes.clone();
    reordered.swap(0, 2);
    let Some(other) = written(&reordered, true) else { return };
    assert_ne!(other[0], batched[0], "the order of overlapping writes shows");
}

#[test]
fn writes_outside_a_batch_are_made_at_once() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (&gpu.device, &gpu.queue);
    let target = buffer(device, 64);
    queue.uploading(Upload::Uniforms).write_buffer(&target, 0, &[9; 16]);
    assert_eq!(traffic::read_back(device, queue, &target, 0, 16), vec![9; 16]);
    // Nothing waits in a batch that isn't gathering.
    let batch = traffic::end_batch();
    assert!(!batch.has_copies());
}
