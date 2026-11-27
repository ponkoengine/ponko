//! The renderer's side of reading GPU buffers and render targets back for
//! the game (`Graphics::read_gpu_buffer`, `Graphics::read_render_target`).
//!
//! A read is queued with the resource it copies from; after the frame's
//! passes, [`Readbacks::submit`] copies every queued read into a staging
//! buffer in one submission and maps those buffers. [`Readbacks::collect`]
//! polls the device without waiting and hands over each read whose
//! mapping finished, as `Vec4`s. Staging buffers are kept for later reads
//! (the smallest that fits is taken), so a game reading the same few
//! values each frame makes no new buffers and no CPU allocations of its
//! own once running. The same code runs on every backend: on the web,
//! mappings finish from the browser's event loop between frames.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use glam::Vec4;

use super::traffic::NewBuffer;
use crate::error::ErrorKind;

/// A staging buffer's mapping states.
const WAITING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Staging buffers kept for later reads, at most.
const SPARE: usize = 16;

/// Why a buffer read fails on a GPU without storage buffers.
const NO_STORAGE: &str = "this GPU has no storage buffers (WebGL2): a GPU buffer has no GPU copy and no compute pass \
                          writes it, so there is nothing to read back; read a render target instead";

/// A buffer that the GPU copies into and the CPU maps, with its mapping
/// state (kept with it, so reusing it allocates nothing).
struct Staging {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

impl Staging {
    fn size(&self) -> u64 {
        self.buffer.size()
    }
}

/// How a read's bytes become values.
#[derive(Clone, Copy)]
enum Layout {
    /// `len` elements of four `f32`s.
    Elements { len: u32 },
    /// Rows of RGBA half floats, `row_bytes` apart (padded to wgpu's copy
    /// alignment).
    Texels { width: u32, height: u32, row_bytes: u32 },
}

impl Layout {
    fn bytes(self) -> u64 {
        match self {
            Self::Elements { len } => u64::from(len) * 16,
            Self::Texels { height, row_bytes, .. } => u64::from(height) * u64::from(row_bytes),
        }
    }
}

/// What a queued read copies from.
enum Source {
    Buffer {
        buffer: wgpu::Buffer,
        offset: u64,
    },
    /// The rectangle at `at` of `size` texels, rows `row_bytes` apart.
    Texture {
        texture: wgpu::Texture,
        at: [u32; 2],
        size: [u32; 2],
        row_bytes: u32,
    },
}

/// A read asked for this frame, copied at its end.
struct Queued {
    read: u32,
    source: Source,
    layout: Layout,
}

/// A read whose copy was submitted, waiting for its mapping.
struct InFlight {
    read: u32,
    staging: Staging,
    layout: Layout,
}

/// The renderer's reads (see the module).
#[derive(Default)]
pub(crate) struct Readbacks {
    queued: Vec<Queued>,
    /// Copied and being mapped; `mapping` of them still need `map_async`.
    in_flight: Vec<InFlight>,
    /// Reads that can't be answered, handed over at the next collect.
    failed: Vec<(u32, &'static str)>,
    spare: Vec<Staging>,
    /// The values of the read being handed over.
    values: Vec<Vec4>,
}

impl Readbacks {
    /// Queues a read of `len` elements of `buffer` from `offset`; `None`
    /// for a buffer without a GPU copy (no storage buffers).
    pub fn read_buffer(&mut self, read: u32, buffer: Option<&wgpu::Buffer>, offset: u32, len: u32) {
        match buffer {
            Some(buffer) => self.queued.push(Queued {
                read,
                source: Source::Buffer {
                    buffer: buffer.clone(),
                    offset: u64::from(offset) * 16,
                },
                layout: Layout::Elements { len },
            }),
            None => self.failed.push((read, NO_STORAGE)),
        }
    }

    /// Queues a read of the `size` texels of render target `texture` at
    /// `at` (an RGBA half float texture).
    pub fn read_texture(&mut self, read: u32, texture: &wgpu::Texture, at: [u32; 2], size: [u32; 2]) {
        debug_assert_eq!(texture.format(), wgpu::TextureFormat::Rgba16Float);
        let row_bytes = (size[0] * 8).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        self.queued.push(Queued {
            read,
            source: Source::Texture {
                texture: texture.clone(),
                at,
                size,
                row_bytes,
            },
            layout: Layout::Texels {
                width: size[0],
                height: size[1],
                row_bytes,
            },
        });
    }

    /// A staging buffer of at least `bytes`: the smallest spare one that
    /// fits, or a new one (rounded up to a power of two, so later reads of
    /// about the same size reuse it).
    fn staging(&mut self, device: &wgpu::Device, bytes: u64) -> Staging {
        let best = (self.spare.iter().enumerate())
            .filter(|(_, s)| s.size() >= bytes)
            .min_by_key(|(_, s)| s.size())
            .map(|(i, _)| i);
        if let Some(i) = best {
            let staging = self.spare.swap_remove(i);
            staging.state.store(WAITING, Ordering::Release);
            return staging;
        }
        Staging {
            buffer: device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("readback staging"),
                size: bytes.max(256).next_power_of_two(),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(WAITING)),
        }
    }

    /// End of a frame, after its passes were submitted: copies every
    /// queued read into a staging buffer, in one submission, and starts
    /// mapping them.
    pub fn submit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.queued.is_empty() {
            return;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback copies"),
        });
        let first = self.in_flight.len();
        let mut queued = std::mem::take(&mut self.queued);
        for q in queued.drain(..) {
            let bytes = q.layout.bytes();
            let staging = self.staging(device, bytes);
            match &q.source {
                Source::Buffer { buffer, offset } => {
                    encoder.copy_buffer_to_buffer(buffer, *offset, &staging.buffer, 0, bytes);
                }
                &Source::Texture {
                    ref texture,
                    at,
                    size: [width, height],
                    row_bytes,
                } => {
                    encoder.copy_texture_to_buffer(
                        wgpu::TexelCopyTextureInfo {
                            texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: at[0],
                                y: at[1],
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyBufferInfo {
                            buffer: &staging.buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(row_bytes),
                                rows_per_image: Some(height),
                            },
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
            self.in_flight.push(InFlight {
                read: q.read,
                staging,
                layout: q.layout,
            });
        }
        // Kept, so the next frame's queue doesn't allocate.
        self.queued = queued;
        queue.submit([encoder.finish()]);
        for f in &self.in_flight[first..] {
            let state = f.staging.state.clone();
            f.staging
                .buffer
                .slice(..f.layout.bytes())
                .map_async(wgpu::MapMode::Read, move |result| {
                    state.store(if result.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
                });
        }
    }

    /// Hands every finished read to `done` (its id and its values, or why
    /// it failed), without waiting for the GPU.
    pub fn collect(
        &mut self,
        device: &wgpu::Device,
        mut done: impl FnMut(u32, Result<&[Vec4], (ErrorKind, &'static str)>),
    ) {
        for (read, why) in self.failed.drain(..) {
            done(read, Err((ErrorKind::Unsupported, why)));
        }
        if self.in_flight.is_empty() {
            return;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        let mut i = 0;
        while i < self.in_flight.len() {
            let state = self.in_flight[i].staging.state.load(Ordering::Acquire);
            if state == WAITING {
                i += 1;
                continue;
            }
            let f = self.in_flight.swap_remove(i);
            if state == FAILED {
                done(
                    f.read,
                    Err((
                        ErrorKind::Unsupported,
                        "the GPU couldn't map the read back (the device was lost?)",
                    )),
                );
                continue;
            }
            let slice = f.staging.buffer.slice(..f.layout.bytes());
            match slice.get_mapped_range() {
                Ok(view) => {
                    decode(&view, f.layout, &mut self.values);
                    drop(view);
                    done(f.read, Ok(&self.values));
                }
                Err(_) => done(
                    f.read,
                    Err((ErrorKind::Unsupported, "the GPU couldn't map the read back")),
                ),
            }
            f.staging.buffer.unmap();
            if self.spare.len() < SPARE {
                self.spare.push(f.staging);
            }
        }
    }

    /// Counts the staging buffers as other memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut super::memory::MemoryTally) {
        use super::memory::MemoryClass as C;
        for s in self.spare.iter().chain(self.in_flight.iter().map(|f| &f.staging)) {
            t.buffer(C::Other, "readback staging", &s.buffer);
        }
    }

    /// How many staging buffers there are (spare and in flight).
    #[cfg(test)]
    pub fn staging_buffers(&self) -> usize {
        self.spare.len() + self.in_flight.len()
    }
}

/// `bytes` as values: four `f32`s an element, or RGBA half floats a texel
/// (rows `row_bytes` apart).
fn decode(bytes: &[u8], layout: Layout, out: &mut Vec<Vec4>) {
    out.clear();
    match layout {
        Layout::Elements { len } => {
            let f = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            out.extend(
                bytes[..len as usize * 16]
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(|e| Vec4::new(f(&e[0..4]), f(&e[4..8]), f(&e[8..12]), f(&e[12..16]))),
            );
        }
        Layout::Texels {
            width,
            height,
            row_bytes,
        } => {
            let h = |b: &[u8]| half::f16::from_le_bytes([b[0], b[1]]).to_f32();
            for y in 0..height as usize {
                let row = &bytes[y * row_bytes as usize..][..width as usize * 8];
                out.extend(
                    row.as_chunks::<8>()
                        .0
                        .iter()
                        .map(|t| Vec4::new(h(&t[0..2]), h(&t[2..4]), h(&t[4..6]), h(&t[6..8]))),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_and_padded_texel_rows_decode() {
        let mut out = Vec::new();
        let bytes: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0, -1.0, 0.5, 0.0, 9.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        decode(&bytes, Layout::Elements { len: 2 }, &mut out);
        assert_eq!(out, [Vec4::new(1.0, 2.0, 3.0, 4.0), Vec4::new(-1.0, 0.5, 0.0, 9.0)]);
        // Two rows of one texel, 16 bytes apart (8 of padding each).
        let texel = |v: [f32; 4]| v.map(|c| half::f16::from_f32(c).to_le_bytes()).concat();
        let mut rows = texel([0.25, 0.5, 1.0, 2.0]);
        rows.extend([0xAA; 8]);
        rows.extend(texel([3.0, 0.0, -2.0, 1.0]));
        rows.extend([0xAA; 8]);
        decode(
            &rows,
            Layout::Texels {
                width: 1,
                height: 2,
                row_bytes: 16,
            },
            &mut out,
        );
        assert_eq!(out, [Vec4::new(0.25, 0.5, 1.0, 2.0), Vec4::new(3.0, 0.0, -2.0, 1.0)]);
    }
}
