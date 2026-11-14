//! What the renderer remembers about each CPU-uploaded texture's alpha, so
//! a pixel upload that leaves alpha alone keeps built-in cutout shadows.
//!
//! Every texture keeps only a summary: its size, whether it is fully
//! opaque, and a hash of its alpha (24 bytes). A full per-texel mask is kept
//! only for a texture that a cutout material reads as its base map, and only
//! once a region upload needs one: it is read back from the GPU once and
//! then updated from each upload. Full uploads are compared by summary.
//! Shadows never read these: the GPU samples the texture itself.
use std::collections::HashMap;

/// One texture's alpha. Size takes part because filtering depends on it.
#[derive(Debug, Clone, PartialEq)]
struct TextureAlpha {
    width: u32,
    height: u32,
    /// Every texel's alpha is 255.
    opaque: bool,
    /// A hash of every texel's alpha, row by row.
    hash: u64,
    /// Each texel's alpha: only for alpha-tested textures (see the module).
    mask: Option<Vec<u8>>,
}

impl TextureAlpha {
    fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let (opaque, hash) = summarize_rgba(rgba);
        Self {
            width,
            height,
            opaque,
            hash,
            mask: None,
        }
    }

    /// Same size and alpha as `other`.
    fn same_alpha(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.opaque == other.opaque
            && self.hash == other.hash
    }
}

/// Whether every alpha in `rgba` is 255, and a hash of them all.
fn summarize_rgba(rgba: &[u8]) -> (bool, u64) {
    let (blocks, rest) = rgba.as_chunks::<32>();
    // The alphas of 8 texels as one little-endian word.
    let words = blocks
        .iter()
        .map(|b| (0..8).fold(0u64, |word, k| word | (u64::from(b[4 * k + 3]) << (8 * k))));
    let tail: Vec<u8> = rest.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    summarize(words, &tail)
}

/// [`summarize_rgba`] for alpha bytes alone (a mask); the same result.
fn summarize_alpha(alpha: &[u8]) -> (bool, u64) {
    let (blocks, tail) = alpha.as_chunks::<8>();
    summarize(blocks.iter().map(|b| u64::from_le_bytes(*b)), tail)
}

/// Whether every alpha is 255, and a hash of them all: a 64-bit
/// multiply-rotate mix of 8 alphas at a time, then the fewer than 8 left.
fn summarize(words: impl Iterator<Item = u64>, tail: &[u8]) -> (bool, u64) {
    const K: u64 = 0x9e37_79b9_7f4a_7c15;
    let mix = |hash: u64, word: u64| (hash ^ word).wrapping_mul(K).rotate_left(29);
    let (mut hash, mut all) = (K, u64::MAX);
    for word in words {
        all &= word;
        hash = mix(hash, word);
    }
    let last = tail.iter().rev().fold(0u64, |word, &a| (word << 8) | u64::from(a));
    // The tail's length too, so trailing zeros still change the hash.
    hash = mix(mix(hash, last), tail.len() as u64);
    (all == u64::MAX && tail.iter().all(|&a| a == 255), hash)
}

/// The CPU memory alpha bookkeeping holds (see
/// [`AssetInventory::texture_alpha`](crate::inventory::AssetInventory::texture_alpha)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AlphaMemory {
    pub summaries: usize,
    pub masks: usize,
    pub mask_bytes: u64,
    pub bytes: u64,
}

/// Every CPU-uploaded texture's alpha, by texture id. Render targets and
/// block-compressed textures have none: any change to them counts.
#[derive(Default)]
pub(crate) struct TextureAlphas {
    textures: HashMap<u32, TextureAlpha>,
    /// Masks built so far (each read back once).
    masks_built: u64,
}

impl TextureAlphas {
    /// Texture `id` was made from `rgba`: only its summary is kept.
    pub fn created(&mut self, id: u32, width: u32, height: u32, rgba: &[u8]) {
        self.textures.insert(id, TextureAlpha::from_rgba(width, height, rgba));
    }

    /// Texture `id` has no CPU-known alpha from now on (freed, compressed).
    pub fn forget(&mut self, id: u32) {
        self.textures.remove(&id);
    }

    /// Every texel of `id` is replaced by `rgba`; whether its alpha (or
    /// size) changed. A kept mask is rebuilt from the new pixels.
    pub fn replaced(&mut self, id: u32, width: u32, height: u32, rgba: &[u8]) -> bool {
        let mut next = TextureAlpha::from_rgba(width, height, rgba);
        let old = self.textures.get(&id);
        let changed = old.is_none_or(|old| !old.same_alpha(&next));
        if old.is_some_and(|old| old.mask.is_some()) {
            next.mask = Some(rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect());
        }
        self.textures.insert(id, next);
        changed
    }

    /// A `size` block of `id` at `at` becomes `rgba` (not uploaded yet);
    /// whether its alpha changed. Without a mask, an opaque block on an
    /// opaque texture is unchanged and anything else counts as changed;
    /// when `alpha_tested` (a cutout material reads it) a mask is built
    /// first from `read`, its alpha as it is now, once.
    pub fn region(
        &mut self,
        id: u32,
        at: [u32; 2],
        size: [u32; 2],
        rgba: &[u8],
        alpha_tested: bool,
        read: impl FnOnce(u32, u32) -> Option<Vec<u8>>,
    ) -> bool {
        let Some(alpha) = self.textures.get_mut(&id) else {
            return true;
        };
        if alpha.mask.is_none() && alpha_tested {
            alpha.mask =
                read(alpha.width, alpha.height).filter(|mask| mask.len() == (alpha.width * alpha.height) as usize);
            self.masks_built += u64::from(alpha.mask.is_some());
        }
        let Some(mask) = &mut alpha.mask else {
            let block_opaque = rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
            if alpha.opaque && block_opaque {
                return false;
            }
            // Its alpha is unknown now: the next upload counts as a change.
            self.textures.remove(&id);
            return true;
        };
        let mut changed = false;
        for (i, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
            let x = at[0] as usize + i % size[0] as usize;
            let y = at[1] as usize + i / size[0] as usize;
            let old = &mut mask[y * alpha.width as usize + x];
            changed |= *old != pixel[3];
            *old = pixel[3];
        }
        if changed {
            (alpha.opaque, alpha.hash) = summarize_alpha(mask);
        }
        changed
    }

    /// What this holds on the CPU.
    pub fn memory(&self) -> AlphaMemory {
        let masks: Vec<u64> = self
            .textures
            .values()
            .filter_map(|t| t.mask.as_ref().map(|m| m.capacity() as u64))
            .collect();
        let mask_bytes = masks.iter().sum();
        let summary = std::mem::size_of::<(u32, TextureAlpha)>() as u64;
        AlphaMemory {
            summaries: self.textures.len(),
            masks: masks.len(),
            mask_bytes,
            bytes: mask_bytes + self.textures.capacity() as u64 * summary,
        }
    }

    /// Masks built so far (tests: each texture's is built once).
    #[cfg(test)]
    pub fn masks_built(&self) -> u64 {
        self.masks_built
    }
}

/// Reads mip 0 of an RGBA8 `texture` back and returns its alpha, waiting
/// for the GPU: `None` for other formats and in the browser, which can't
/// wait. Uploads queued before this are included.
pub(crate) fn read_alpha(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Option<Vec<u8>> {
    if cfg!(target_arch = "wasm32")
        || !matches!(
            texture.format(),
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        )
        || !texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
    {
        return None;
    }
    let (width, height) = (texture.width(), texture.height());
    let row = (4 * width).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("texture alpha readback"),
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    rx.recv().ok()?.ok()?;
    let data = buffer.get_mapped_range(..).ok()?;
    let mut alpha = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        let start = y * row as usize;
        alpha.extend(
            data[start..start + 4 * width as usize]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[3]),
        );
    }
    Some(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(alpha: &[u8]) -> Vec<u8> {
        alpha.iter().flat_map(|&a| [10, 20, 30, a]).collect()
    }

    #[test]
    fn an_opaque_texture_keeps_no_cpu_mask() {
        let mut alphas = TextureAlphas::default();
        alphas.created(0, 64, 64, &[255; 64 * 64 * 4]);
        alphas.created(1, 64, 64, &rgba(&[0; 64 * 64]));
        let memory = alphas.memory();
        assert_eq!((memory.summaries, memory.masks, memory.mask_bytes), (2, 0, 0));
        assert!(memory.bytes < 256, "two summaries hold {} bytes", memory.bytes);
        // Region uploads on a texture no cutout material reads build none either.
        alphas.region(0, [0, 0], [2, 2], &rgba(&[255; 4]), false, |_, _| panic!("no readback"));
        alphas.region(1, [0, 0], [2, 2], &rgba(&[7; 4]), false, |_, _| panic!("no readback"));
        assert_eq!(alphas.memory().masks, 0);
        assert_eq!(alphas.masks_built(), 0);
    }

    #[test]
    fn full_uploads_compare_alpha_and_size_by_summary() {
        let mut alphas = TextureAlphas::default();
        alphas.created(0, 2, 2, &rgba(&[255, 0, 255, 0]));
        let other_rgb: Vec<u8> = [255, 0, 255, 0].iter().flat_map(|&a| [77, 1, 2, a]).collect();
        assert!(!alphas.replaced(0, 2, 2, &other_rgb), "RGB-only uploads keep alpha");
        assert!(alphas.replaced(0, 2, 2, &rgba(&[255, 0, 255, 1])));
        assert!(alphas.replaced(0, 1, 4, &rgba(&[255, 0, 255, 1])), "size changes count");
        assert!(!alphas.replaced(0, 1, 4, &rgba(&[255, 0, 255, 1])));
        assert!(
            alphas.replaced(7, 1, 1, &rgba(&[255])),
            "unknown textures count as changed"
        );
        assert_ne!(summarize_alpha(&[0; 8]), summarize_alpha(&[0; 9]));
        for len in [0, 1, 7, 8, 9, 31, 64] {
            let alpha: Vec<u8> = (0..len).map(|i| (i * 37) as u8).collect();
            assert_eq!(summarize_rgba(&rgba(&alpha)), summarize_alpha(&alpha), "{len} texels");
        }
    }

    #[test]
    fn region_uploads_without_a_mask_stay_conservative() {
        let mut alphas = TextureAlphas::default();
        alphas.created(0, 4, 4, &[255; 64]);
        assert!(!alphas.region(0, [1, 1], [2, 2], &rgba(&[255; 4]), false, |_, _| None));
        assert!(alphas.region(0, [1, 1], [1, 1], &rgba(&[3]), false, |_, _| None));
        // Unknown now: even an equal full upload counts once.
        assert!(alphas.replaced(0, 4, 4, &[255; 64]));
        assert!(!alphas.replaced(0, 4, 4, &[255; 64]));
        assert!(
            alphas.region(5, [0, 0], [1, 1], &rgba(&[255]), true, |_, _| None),
            "render targets"
        );
    }

    #[test]
    fn an_alpha_tested_texture_builds_its_mask_once_and_compares_exactly() {
        let mut alphas = TextureAlphas::default();
        let start = [255, 0, 255, 0];
        alphas.created(0, 2, 2, &rgba(&start));
        let reads = std::cell::Cell::new(0);
        let read = |w: u32, h: u32| {
            reads.set(reads.get() + 1);
            assert_eq!((w, h), (2, 2));
            Some(start.to_vec())
        };
        // RGB-only: kept, after one readback.
        assert!(!alphas.region(0, [0, 0], [2, 2], &rgba(&start), true, read));
        assert!(alphas.region(0, [1, 0], [1, 2], &rgba(&[0, 9]), true, read));
        assert!(!alphas.region(0, [1, 0], [1, 2], &rgba(&[0, 9]), true, read));
        assert!(alphas.region(0, [0, 0], [1, 1], &rgba(&[1]), true, read));
        assert_eq!(reads.get(), 1, "the mask is read back once");
        assert_eq!(alphas.masks_built(), 1);
        let memory = alphas.memory();
        assert_eq!((memory.masks, memory.mask_bytes), (1, 4));
        // The summary follows the mask, and full uploads keep it.
        assert!(!alphas.replaced(0, 2, 2, &rgba(&[1, 0, 255, 9])));
        assert!(alphas.replaced(0, 2, 2, &rgba(&[1, 0, 255, 8])));
        assert!(!alphas.region(0, [1, 1], [1, 1], &rgba(&[8]), true, read));
        assert_eq!(reads.get(), 1);
        alphas.forget(0);
        let memory = alphas.memory();
        assert_eq!((memory.summaries, memory.masks, memory.mask_bytes), (0, 0, 0));
    }

    /// CPU bytes and creation time for 300 textures (157 MB of RGBA8 alpha
    /// channels before), 12 of them cutout bases that take a region upload.
    #[test]
    #[ignore = "a measurement: run in release with --ignored --nocapture"]
    fn alpha_bookkeeping_for_three_hundred_textures() {
        let sizes: Vec<u32> = [(240, 512), (50, 1024), (10, 2048)]
            .iter()
            .flat_map(|&(n, size)| std::iter::repeat_n(size, n))
            .collect();
        let pixels: Vec<Vec<u8>> = [512u32, 1024, 2048]
            .iter()
            .map(|&s| {
                (0..s * s * 4)
                    .map(|i| if i % 4 == 3 { (i / 64) as u8 } else { 9 })
                    .collect()
            })
            .collect();
        let rgba = |size: u32| &pixels[size.trailing_zeros() as usize - 9];
        // Before: a full mask for every texture.
        let start = std::time::Instant::now();
        let before: Vec<Vec<u8>> = sizes
            .iter()
            .map(|&s| rgba(s).as_chunks::<4>().0.iter().map(|p| p[3]).collect())
            .collect();
        let before_time = start.elapsed();
        let before_bytes: u64 = before.iter().map(|m| m.capacity() as u64).sum();
        // After: summaries, and masks for the alpha-tested ones.
        let mut alphas = TextureAlphas::default();
        let start = std::time::Instant::now();
        for (id, &s) in sizes.iter().enumerate() {
            alphas.created(id as u32, s, s, rgba(s));
        }
        let after_time = start.elapsed();
        for id in 0..12 {
            let s = sizes[id as usize];
            alphas.region(id, [0, 0], [1, 1], &[1, 2, 3, 4], true, |_, _| {
                Some(rgba(s).as_chunks::<4>().0.iter().map(|p| p[3]).collect())
            });
        }
        let m = alphas.memory();
        println!(
            "300 textures: before {before_bytes} bytes of masks ({before_time:?} to build); \
             after {} bytes ({} summaries, {} masks of {} bytes; {after_time:?} to summarize)",
            m.bytes, m.summaries, m.masks, m.mask_bytes
        );
        assert_eq!(m.masks, 12);
    }
}
