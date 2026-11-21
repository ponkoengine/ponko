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

/// A 4 x 8 `Rgba32Float` texture (64-byte rows) that writes and copies
/// can go into.
fn texture(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The texture's 8 rows of 64 bytes.
fn texture_bytes(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let readback = buffer(device, 256 * 8);
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let padded = traffic::read_back(device, queue, &readback, 0, 256 * 8);
    padded.chunks(256).flat_map(|row| row[..64].to_vec()).collect()
}

/// Writes `rows` rows of 64 bytes from row `first` of `texture`, the source
/// rows `stride` bytes apart from `offset` in `data`.
fn write_rows(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    first: u32,
    rows: u32,
    data: &[u8],
    offset: u64,
    stride: u32,
) {
    queue.uploading(Upload::Lights).write_texture(
        wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d { x: 0, y: first, z: 0 },
            ..texture.as_image_copy()
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset,
            bytes_per_row: Some(stride),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: 4,
            height: rows,
            depth_or_array_layers: 1,
        },
    );
}

#[test]
fn batched_texture_writes_land_as_writes_made_one_by_one() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (&gpu.device, &gpu.queue);
    // Rows of source data 80 bytes apart (not the copy's 256), from 16 in.
    let data: Vec<u8> = (0..16 + 80 * 8).map(|i| (i % 251) as u8).collect();
    let write = |batched: bool, overlap: u8| {
        let target = texture(device);
        if batched {
            assert!(traffic::begin_batch(WriteBatch::default()));
        }
        write_rows(queue, &target, 0, 8, &data, 16, 80);
        // Over rows 2 and 3: the later write wins.
        write_rows(queue, &target, 2, 2, &[overlap; 128], 0, 64);
        if batched {
            let mut batch = traffic::end_batch();
            assert!(batch.has_copies(), "the texture writes were gathered");
            batch.upload(device, queue);
            let mut encoder = device.create_command_encoder(&Default::default());
            batch.record(&mut encoder);
            queue.submit([encoder.finish()]);
        }
        traffic::take();
        texture_bytes(device, queue, &target)
    };
    let one_by_one = write(false, 7);
    assert_eq!(&one_by_one[..64], &data[16..80], "positive control: the first row");
    assert_eq!(&one_by_one[128..256], &[7; 128], "positive control: the overlap");
    assert_eq!(write(true, 7), one_by_one);
    // A negative control: another overlap shows.
    assert_ne!(write(true, 9), one_by_one);
}

/// How [`two_frames`] submits its frames.
#[derive(Clone, Copy, PartialEq)]
enum Frames {
    /// Each on its own, its writes batched.
    Apart,
    /// Held (`traffic::hold`) and submitted together, writes batched.
    Held,
    /// Together, in one `Queue::submit`, writes made straight to wgpu.
    Raw,
}

/// Two frames sharing one uniform: each writes it, then copies it out
/// (as its passes would read it). Gives what each frame read.
fn two_frames(how: Frames) -> Option<[Vec<u8>; 2]> {
    let gpu = gpu()?;
    let (device, queue) = (&gpu.device, &gpu.queue);
    let len = 2 * BATCHED_WRITE_BYTES as u64;
    let uniform = buffer(device, len);
    let read = [buffer(device, len), buffer(device, len)];
    if how == Frames::Held {
        traffic::hold(queue);
    }
    let mut writes = WriteBatch::default();
    let mut raw = Vec::new();
    for (frame, out) in read.iter().enumerate() {
        let value = frame as u8 + 1;
        let big = vec![value; BATCHED_WRITE_BYTES + 256];
        if how == Frames::Raw {
            queue.write_buffer(&uniform, 0, &[value; 64]);
            queue.write_buffer(&uniform, 256, &big);
        } else {
            assert!(traffic::begin_batch(std::mem::take(&mut writes)));
            queue
                .uploading(Upload::Uniforms)
                .write_buffer(&uniform, 0, &[value; 64]);
            // Too big for a batch on its own: while held, still the frame's.
            queue.uploading(Upload::StaticBatches).write_buffer(&uniform, 256, &big);
            writes = traffic::end_batch();
            writes.upload(device, queue);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        writes.record(&mut encoder);
        encoder.copy_buffer_to_buffer(&uniform, 0, out, 0, len);
        match how {
            Frames::Raw => raw.push(encoder.finish()),
            _ => traffic::submit(queue, [encoder.finish()]),
        }
    }
    match how {
        Frames::Held => assert_eq!(traffic::release(), (2, 1), "two command buffers, one submission"),
        Frames::Raw => {
            queue.submit(raw);
        }
        Frames::Apart => {}
    }
    traffic::take();
    Some(read.map(|b| traffic::read_back(device, queue, &b, 0, len)))
}

#[test]
fn held_frames_each_read_their_own_writes() {
    let Some(apart) = two_frames(Frames::Apart) else { return };
    for (frame, read) in apart.iter().enumerate() {
        let value = frame as u8 + 1;
        assert!(
            read[..64].iter().all(|&b| b == value),
            "frame {frame}'s uniform, submitted alone"
        );
        assert!(read[256..512].iter().all(|&b| b == value), "frame {frame}'s big write");
    }
    let Some(together) = two_frames(Frames::Held) else {
        return;
    };
    assert!(together == apart, "held and submitted together, the same");
    // Negative control: writes made straight to wgpu all land ahead of one
    // submission, so the first frame reads the second one's.
    let Some(raw) = two_frames(Frames::Raw) else { return };
    assert!(
        raw[0][..64].iter().all(|&b| b == 2),
        "in one submission without the batch, frame 0 reads frame 1's uniform"
    );
}

#[test]
fn a_write_that_cannot_wait_submits_what_is_held_first() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (&gpu.device, &gpu.queue);
    let uniform = buffer(device, 256);
    let out = buffer(device, 256);
    traffic::hold(queue);
    queue.uploading(Upload::Uniforms).write_buffer(&uniform, 0, &[1; 16]);
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&uniform, 0, &out, 0, 256);
    traffic::submit(queue, [encoder.finish()]);
    // Outside a batch: it would land ahead of the held copy.
    queue.uploading(Upload::Uniforms).write_buffer(&uniform, 0, &[2; 16]);
    assert_eq!(traffic::release(), (1, 1), "the held buffer went before the write");
    assert_eq!(traffic::read_back(device, queue, &out, 0, 16), vec![1; 16]);
    assert_eq!(traffic::read_back(device, queue, &uniform, 0, 16), vec![2; 16]);
    traffic::take();
}

#[test]
fn a_buffer_grown_while_writes_are_gathered_keeps_them() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (&gpu.device, &gpu.queue);
    let (old, grown) = (buffer(device, 256), buffer(device, 512));
    for held in [false, true] {
        if held {
            traffic::hold(queue);
        }
        assert!(traffic::begin_batch(WriteBatch::default()));
        queue
            .uploading(Upload::StaticBatches)
            .write_buffer(&old, 0, &[5 + u8::from(held); 64]);
        traffic::submit_copy(device, queue, None, |encoder| {
            encoder.copy_buffer_to_buffer(&old, 0, &grown, 0, 256);
        });
        let mut rest = traffic::end_batch();
        assert!(!rest.has_copies(), "the gathered writes went with the copy");
        rest.upload(device, queue);
        if held {
            assert_eq!(traffic::release(), (1, 1));
        }
        assert_eq!(
            traffic::read_back(device, queue, &grown, 0, 64),
            vec![5 + u8::from(held); 64],
            "held: {held}"
        );
    }
    traffic::take();
}
