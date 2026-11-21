//! Lightmap pages block-compressed: BC6H (unsigned half floats, 16 bytes
//! per 4 x 4 texels, 1 byte a texel), which desktop GPUs sample directly,
//! and the shared-exponent RGB9E5 format (4 bytes a texel) for the others.
//!
//! Every block is BC6H's mode 11: one line through the colors of the
//! block from endpoint A to endpoint B (10 bits a channel), and a 4-bit
//! index per texel picking one of 16 points along it. Interpolation is in
//! the half floats' bit patterns, which grow about as the logarithm of the
//! value, so the error is about the same share of any brightness.

use rayon::prelude::*;

/// Bytes in a BC6H block.
pub(crate) const BLOCK_BYTES: usize = 16;

/// The weights (of 64) of BC6H's 16 points between two endpoints.
const WEIGHTS: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// The largest finite half float (65504).
const MAX_HALF: u16 = 0x7bff;

/// An endpoint's 10 bits as the 16 the interpolation works in.
fn unquantize(e: u32) -> u32 {
    match e {
        0 => 0,
        1023 => 0xffff,
        e => ((e << 16) + 0x8000) >> 10,
    }
}

/// The half float point `index` of the line from `a` to `b` decodes to.
fn point(a: u32, b: u32, index: usize) -> u16 {
    let w = WEIGHTS[index];
    let v = (unquantize(a) * (64 - w) + unquantize(b) * w + 32) >> 6;
    ((v * 31) >> 6) as u16
}

/// The endpoint whose value is nearest half float `h`.
fn endpoint(h: u16) -> u32 {
    // Decoded, endpoint e is (unquantize(e) * 31) >> 6: about 31 e.
    let target = u32::from(h);
    let guess = (target * 64 / 31).saturating_sub(32) / 64;
    (guess.saturating_sub(1)..=(guess + 2).min(1023))
        .min_by_key(|&e| ((unquantize(e) * 31) >> 6).abs_diff(target))
        .unwrap_or(0)
}

/// Encodes 16 texels (row by row; RGB half floats, non-negative) as one
/// block. Only the texels whose bit is set in `used` count (the others
/// are never read): an unused texel takes whatever is nearest.
pub(crate) fn encode_block(texels: &[[u16; 3]; 16], used: u16) -> [u8; BLOCK_BYTES] {
    if used == 0 {
        return pack([0; 3], [0; 3], [0; 16]);
    }
    let clamp = |h: u16| if h & 0x8000 != 0 { 0 } else { h.min(MAX_HALF) };
    let mut lo = [u16::MAX; 3];
    let mut hi = [0u16; 3];
    for (i, t) in texels.iter().enumerate() {
        if used >> i & 1 != 0 {
            for c in 0..3 {
                lo[c] = lo[c].min(clamp(t[c]));
                hi[c] = hi[c].max(clamp(t[c]));
            }
        }
    }
    let (lo, hi) = (lo.map(endpoint), hi.map(endpoint));
    let texels = texels.map(|t| t.map(clamp));
    // The line along one of the box's four diagonals: green and blue each
    // rising or falling with red. The one that fits best, its endpoints
    // then fitted to the texels by least squares.
    let mut best = (u64::MAX, [0u32; 3], [0u32; 3], [0u8; 16]);
    for flip in 0..4 {
        let mut a = lo;
        let mut b = hi;
        for (c, bit) in [(1, 1), (2, 2)] {
            if flip & bit != 0 {
                (a[c], b[c]) = (hi[c], lo[c]);
            }
        }
        let (mut error, mut indices) = assign(&texels, used, a, b);
        for _ in 0..2 {
            let (fa, fb) = fit(&texels, used, &indices, a, b);
            let (e, k) = assign(&texels, used, fa, fb);
            if e >= error {
                break;
            }
            (error, indices, a, b) = (e, k, fa, fb);
        }
        if error < best.0 {
            best = (error, a, b, indices);
        }
    }
    let (_, mut a, mut b, mut indices) = best;
    // The first texel's index has 3 bits: its top bit must be 0. The
    // weights are symmetric, so swapping the endpoints and counting the
    // indices from the other end decodes the same.
    if indices[0] >= 8 {
        std::mem::swap(&mut a, &mut b);
        for i in &mut indices {
            *i = 15 - *i;
        }
    }
    pack(a, b, indices)
}

/// The nearest of the 16 points from `a` to `b` for each used texel, and
/// the squared error (in half float steps) they make.
fn assign(texels: &[[u16; 3]; 16], used: u16, a: [u32; 3], b: [u32; 3]) -> (u64, [u8; 16]) {
    let palette: [[u16; 3]; 16] = std::array::from_fn(|i| std::array::from_fn(|c| point(a[c], b[c], i)));
    let mut error = 0u64;
    let mut indices = [0u8; 16];
    for (i, t) in texels.iter().enumerate() {
        if used >> i & 1 == 0 {
            continue;
        }
        let (k, e) = palette
            .iter()
            .enumerate()
            .map(|(k, p)| (k, (0..3).map(|c| u64::from(p[c].abs_diff(t[c])).pow(2)).sum::<u64>()))
            .min_by_key(|&(_, e)| e)
            .unwrap_or((0, 0));
        indices[i] = k as u8;
        error += e;
    }
    (error, indices)
}

/// The endpoints that fit the used texels best with `indices`, channel by
/// channel: the least-squares line through them, rounded to the nearest
/// endpoints and then the best of their neighbours. `a` and `b` where the
/// texels all take one point.
fn fit(texels: &[[u16; 3]; 16], used: u16, indices: &[u8; 16], a: [u32; 3], b: [u32; 3]) -> ([u32; 3], [u32; 3]) {
    let mut fa = a;
    let mut fb = b;
    for c in 0..3 {
        // In the 16 bits the points are mixed in: a half float h comes
        // from about (h + 0.5) * 64 / 31.
        let (mut s11, mut s12, mut s22, mut t1, mut t2) = (0.0f64, 0.0, 0.0, 0.0, 0.0);
        for (i, t) in texels.iter().enumerate() {
            if used >> i & 1 == 0 {
                continue;
            }
            let w = f64::from(WEIGHTS[indices[i] as usize]) / 64.0;
            let v = (f64::from(t[c]) + 0.5) * 64.0 / 31.0;
            s11 += (1.0 - w) * (1.0 - w);
            s12 += (1.0 - w) * w;
            s22 += w * w;
            t1 += (1.0 - w) * v;
            t2 += w * v;
        }
        let det = s11 * s22 - s12 * s12;
        if det.abs() < 1e-9 {
            continue;
        }
        let ua = (t1 * s22 - t2 * s12) / det;
        let ub = (t2 * s11 - t1 * s12) / det;
        let near = |u: f64| ((u - 32.0) / 64.0).round().clamp(0.0, 1023.0) as u32;
        let (ea, eb) = (near(ua), near(ub));
        let around = |e: u32| e.saturating_sub(1)..=(e + 1).min(1023);
        let mut best = (u64::MAX, a[c], b[c]);
        for x in around(ea) {
            for y in around(eb) {
                let error: u64 = texels
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| used >> i & 1 != 0)
                    .map(|(i, t)| u64::from(point(x, y, indices[i] as usize).abs_diff(t[c])).pow(2))
                    .sum();
                if error < best.0 {
                    best = (error, x, y);
                }
            }
        }
        (fa[c], fb[c]) = (best.1, best.2);
    }
    (fa, fb)
}

/// Mode 11's bits: the mode (5 bits, 00011), endpoint A's red, green and
/// blue (10 bits each), then B's, then the indices (3 bits for the first
/// texel, 4 for each other), from the lowest bit up.
fn pack(a: [u32; 3], b: [u32; 3], indices: [u8; 16]) -> [u8; BLOCK_BYTES] {
    let mut bits = 0u128;
    let mut at = 0;
    let mut put = |v: u32, n: u32| {
        bits |= u128::from(v & ((1 << n) - 1)) << at;
        at += n;
    };
    put(0b00011, 5);
    for v in a.into_iter().chain(b) {
        put(v, 10);
    }
    for (i, &k) in indices.iter().enumerate() {
        put(u32::from(k), if i == 0 { 3 } else { 4 });
    }
    bits.to_le_bytes()
}

/// Decodes a block [`encode_block`] made: 16 RGB half floats, row by row.
/// `None` for a block of another mode.
pub(crate) fn decode_block(block: &[u8; BLOCK_BYTES]) -> Option<[[u16; 3]; 16]> {
    let bits = u128::from_le_bytes(*block);
    let mut at = 0;
    let mut take = |n: u32| {
        let v = (bits >> at) as u32 & ((1 << n) - 1);
        at += n;
        v
    };
    if take(5) != 0b00011 {
        return None;
    }
    let a = [take(10), take(10), take(10)];
    let b = [take(10), take(10), take(10)];
    let mut out = [[0u16; 3]; 16];
    for (i, t) in out.iter_mut().enumerate() {
        let k = take(if i == 0 { 3 } else { 4 }) as usize;
        *t = std::array::from_fn(|c| point(a[c], b[c], k));
    }
    Some(out)
}

/// Encodes a `side` square page of RGBA half floats (`side` a multiple of
/// 4), counting the texels whose alpha isn't 0: its blocks row by row.
pub(crate) fn encode_page(texels: &[[u16; 4]], side: u32) -> Vec<u8> {
    let blocks = (side / 4) as usize;
    let side = side as usize;
    let mut out = vec![0u8; blocks * blocks * BLOCK_BYTES];
    out.par_chunks_mut(blocks * BLOCK_BYTES)
        .enumerate()
        .for_each(|(by, row)| {
            for bx in 0..blocks {
                let mut block = [[0u16; 3]; 16];
                let mut used = 0u16;
                for (i, b) in block.iter_mut().enumerate() {
                    let t = texels[(by * 4 + i / 4) * side + bx * 4 + i % 4];
                    *b = [t[0], t[1], t[2]];
                    if t[3] != 0 {
                        used |= 1 << i;
                    }
                }
                row[bx * BLOCK_BYTES..(bx + 1) * BLOCK_BYTES].copy_from_slice(&encode_block(&block, used));
            }
        });
    out
}

/// Decodes [`encode_page`]'s blocks into the RGB of `texels` (a `side`
/// square page), keeping their alpha. `None` for a block of another mode
/// or blocks of the wrong length.
pub(crate) fn decode_page(blocks: &[u8], side: u32, texels: &mut [[u16; 4]]) -> Option<()> {
    let n = (side / 4) as usize;
    let side = side as usize;
    if blocks.len() != n * n * BLOCK_BYTES || texels.len() != side * side {
        return None;
    }
    for (b, block) in blocks.as_chunks::<BLOCK_BYTES>().0.iter().enumerate() {
        let (bx, by) = (b % n, b / n);
        for (i, rgb) in decode_block(block)?.into_iter().enumerate() {
            let t = &mut texels[(by * 4 + i / 4) * side + bx * 4 + i % 4];
            t[..3].copy_from_slice(&rgb);
        }
    }
    Some(())
}

/// `rgb` (non-negative) in the shared-exponent format: 9 bits of mantissa
/// a channel and a 5-bit exponent for all three, rounded to nearest.
pub(crate) fn rgb9e5(rgb: [f32; 3]) -> u32 {
    const MAX: f32 = 65408.0;
    let c = rgb.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, MAX) });
    let max = c[0].max(c[1]).max(c[2]);
    if max == 0.0 {
        return 0;
    }
    // floor(log2(max)), exactly, from the float's exponent.
    let floor_log2 = ((max.to_bits() >> 23) & 0xff) as i32 - 127;
    let mut exponent = floor_log2.max(-16) + 16;
    let mut scale = 2f32.powi(exponent - 15 - 9);
    if (max / scale + 0.5).floor() >= 512.0 {
        scale *= 2.0;
        exponent += 1;
    }
    let m = c.map(|v| ((v / scale + 0.5).floor() as u32).min(511));
    m[0] | m[1] << 9 | m[2] << 18 | (exponent as u32) << 27
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::three::bake::{f16_to_f32, f32_to_f16};

    /// A block of light with a gradient across it: half again as bright
    /// at one end (a steep one for a lightmap, whose light is smooth).
    fn gradient() -> [[u16; 3]; 16] {
        std::array::from_fn(|i| {
            let t = i as f32 / 15.0;
            [0.4 + 0.2 * t, 0.2 + 0.1 * t, 0.1 + 0.05 * t].map(f32_to_f16)
        })
    }

    #[test]
    fn a_block_decodes_within_a_few_percent_of_its_texels() {
        let block = gradient();
        let decoded = decode_block(&encode_block(&block, u16::MAX)).unwrap();
        for (t, d) in block.iter().zip(&decoded) {
            for c in 0..3 {
                let (want, got) = (f16_to_f32(t[c]), f16_to_f32(d[c]));
                assert!((got - want).abs() <= want * 0.03, "{want} -> {got}");
            }
        }
        // One color: exact to within an endpoint's step.
        let flat = [[f32_to_f16(0.5); 3]; 16];
        for d in decode_block(&encode_block(&flat, u16::MAX)).unwrap() {
            assert!((f16_to_f32(d[0]) - 0.5).abs() < 0.5 * 0.02, "{}", f16_to_f32(d[0]));
        }
    }

    #[test]
    fn unused_texels_dont_pull_the_used_ones() {
        let mut block = [[f32_to_f16(0.4); 3]; 16];
        // Black where nothing was baked (texels 8 to 15).
        for t in &mut block[8..] {
            *t = [0; 3];
        }
        let decoded = decode_block(&encode_block(&block, 0x00ff)).unwrap();
        for d in &decoded[..8] {
            assert!((f16_to_f32(d[0]) - 0.4).abs() < 0.4 * 0.02, "{}", f16_to_f32(d[0]));
        }
        assert_eq!(decode_block(&encode_block(&block, 0)).unwrap(), [[0; 3]; 16]);
    }

    #[test]
    fn colors_that_fall_as_others_rise_fit_a_flipped_diagonal() {
        // Red rising while blue falls: the box's main diagonal can't hold it.
        // (Within one power of two: across one, the two channels' steps
        // differ and a line holds them less well.)
        let block: [[u16; 3]; 16] = std::array::from_fn(|i| {
            let t = i as f32 / 15.0;
            [0.3 + 0.15 * t, 0.35, 0.45 - 0.15 * t].map(f32_to_f16)
        });
        let decoded = decode_block(&encode_block(&block, u16::MAX)).unwrap();
        for (t, d) in block.iter().zip(&decoded) {
            for c in 0..3 {
                let (want, got) = (f16_to_f32(t[c]), f16_to_f32(d[c]));
                assert!((got - want).abs() <= want * 0.03, "{want} -> {got}");
            }
        }
        // Along the main diagonal it is far off.
        let a = [
            endpoint(f32_to_f16(0.3)),
            endpoint(f32_to_f16(0.35)),
            endpoint(f32_to_f16(0.3)),
        ];
        let b = [
            endpoint(f32_to_f16(0.45)),
            endpoint(f32_to_f16(0.35)),
            endpoint(f32_to_f16(0.45)),
        ];
        let worst = block
            .iter()
            .map(|t| {
                (0..16)
                    .map(|k| {
                        (0..3)
                            .map(|c| (f16_to_f32(point(a[c], b[c], k)) - f16_to_f32(t[c])).abs())
                            .fold(0.0, f32::max)
                    })
                    .fold(f32::MAX, f32::min)
            })
            .fold(0.0, f32::max);
        assert!(worst > 0.07, "{worst}");
    }

    #[test]
    fn blocks_of_another_mode_are_refused() {
        let mut block = encode_block(&gradient(), u16::MAX);
        block[0] ^= 1;
        assert_eq!(decode_block(&block), None);
    }

    /// Checks the blocks against an independent decoder (texture2ddecoder,
    /// which keeps 8 bits of the values from 0 to 1).
    #[cfg(feature = "compressed-textures")]
    #[test]
    fn blocks_decode_the_same_in_another_decoder() {
        let blocks = [
            gradient(),
            std::array::from_fn(|i| {
                [
                    f32_to_f16(i as f32 / 16.0),
                    f32_to_f16(0.5),
                    f32_to_f16(1.0 - i as f32 / 16.0),
                ]
            }),
        ];
        for block in blocks {
            let encoded = encode_block(&block, u16::MAX);
            let ours = decode_block(&encoded).unwrap();
            let mut theirs = [0u32; 16];
            texture2ddecoder::decode_bc6_block_unsigned(&encoded, &mut theirs);
            for (o, t) in ours.iter().zip(theirs) {
                // texture2ddecoder packs BGRA.
                let rgb = [(t >> 16) & 0xff, (t >> 8) & 0xff, t & 0xff];
                for c in 0..3 {
                    let ours = (f16_to_f32(o[c]).clamp(0.0, 1.0) * 255.0).round() as i32;
                    assert!((ours - rgb[c] as i32).abs() <= 1, "{ours} vs {}", rgb[c]);
                }
            }
        }
    }

    #[test]
    fn a_page_decodes_to_its_blocks_texels_keeping_alpha() {
        let side = 8;
        let page: Vec<[u16; 4]> = (0..side * side)
            .map(|i| {
                let v = f32_to_f16(0.1 + (i % side) as f32 * 0.05);
                [v, v, v, u16::from(i % 3 != 0) * f32_to_f16(1.0)]
            })
            .collect();
        let blocks = encode_page(&page, side);
        assert_eq!(blocks.len(), 4 * BLOCK_BYTES);
        let mut back = page.clone();
        decode_page(&blocks, side, &mut back).unwrap();
        for (a, b) in page.iter().zip(&back) {
            assert_eq!(a[3], b[3]);
            if a[3] != 0 {
                let (want, got) = (f16_to_f32(a[0]), f16_to_f32(b[0]));
                assert!((got - want).abs() <= want * 0.03, "{want} -> {got}");
            }
        }
        assert_eq!(decode_page(&blocks[1..], side, &mut back), None);
    }

    #[test]
    fn rgb9e5_keeps_nine_bits_of_the_brightest_channel() {
        let decode = |p: u32| {
            let e = (p >> 27) as i32 - 15 - 9;
            [p & 511, p >> 9 & 511, p >> 18 & 511].map(|m| m as f32 * 2f32.powi(e))
        };
        for rgb in [
            [0.5, 0.25, 0.125],
            [3.0, 0.01, 1.5],
            [1000.0, 2.0, 0.0],
            [0.001, 0.002, 0.003],
        ] {
            let back = decode(rgb9e5(rgb));
            let max = rgb.iter().copied().fold(0.0, f32::max);
            for c in 0..3 {
                assert!((back[c] - rgb[c]).abs() <= max / 512.0, "{rgb:?} -> {back:?}");
            }
        }
        assert_eq!(rgb9e5([0.0; 3]), 0);
        // 1.0: mantissa 256, exponent 15 + 1.
        assert_eq!(rgb9e5([1.0, 0.0, 0.0]), 256 | 16 << 27);
    }
}
