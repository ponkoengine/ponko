//! The texture mip limit ([`Graphics::set_texture_mip_limit`]): 3D textures
//! loaded while it is set drop their largest mip levels before upload, so
//! each level dropped quarters their video memory.
//!
//! [`Graphics::set_texture_mip_limit`]: crate::gfx::Graphics::set_texture_mip_limit

/// The smallest side a texture is cut down to: smaller textures (masks,
/// gradients, lookup strips) keep their size.
pub(crate) const MIN_SIDE: u32 = 32;

/// The highest limit: a texture never loses more than this many levels.
pub(crate) const MAX_LIMIT: u32 = 4;

/// How many of its largest levels a `width` x `height` texture drops under
/// `limit`: up to `limit`, while both sides stay at least [`MIN_SIDE`].
pub(crate) fn levels_to_drop(width: u32, height: u32, limit: u32) -> u32 {
    let mut dropped = 0;
    let (mut w, mut h) = (width, height);
    while dropped < limit.min(MAX_LIMIT) && w / 2 >= MIN_SIDE && h / 2 >= MIN_SIDE {
        w /= 2;
        h /= 2;
        dropped += 1;
    }
    dropped
}

/// `rgba` (`width` x `height`, RGBA8) without its `levels` largest mip
/// levels: halved that many times, each output pixel the average of the
/// 2x2 block above it (a box filter, as the mip chain itself would be).
/// An odd last row or column is folded into its neighbour.
pub(crate) fn reduce(width: u32, height: u32, rgba: &[u8], levels: u32) -> (u32, u32, Vec<u8>) {
    debug_assert_eq!(rgba.len(), width as usize * height as usize * 4);
    let mut out = (width, height, rgba.to_vec());
    for _ in 0..levels {
        out = halve(out.0, out.1, &out.2);
    }
    out
}

fn halve(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let (sw, sh) = (width as usize, height as usize);
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h as usize {
        let y0 = (2 * y).min(sh - 1);
        let y1 = (2 * y + 1).min(sh - 1);
        for x in 0..w as usize {
            let x0 = (2 * x).min(sw - 1);
            let x1 = (2 * x + 1).min(sw - 1);
            let at = |px: usize, py: usize| (py * sw + px) * 4;
            let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
            let o = (y * w as usize + x) * 4;
            for ch in 0..4 {
                let sum = rgba[a + ch] as u32 + rgba[b + ch] as u32 + rgba[c + ch] as u32 + rgba[d + ch] as u32;
                out[o + ch] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_halves_both_sides() {
        assert_eq!(levels_to_drop(2048, 1024, 1), 1);
        assert_eq!(levels_to_drop(2048, 1024, 2), 2);
        let (w, h, px) = reduce(256, 128, &vec![0; 256 * 128 * 4], 2);
        assert_eq!((w, h, px.len()), (64, 32, 64 * 32 * 4));
    }

    #[test]
    fn small_textures_keep_their_size() {
        assert_eq!(levels_to_drop(32, 32, 3), 0);
        assert_eq!(levels_to_drop(64, 64, 3), 1, "stops at the smallest side");
        assert_eq!(levels_to_drop(4096, 48, 2), 0, "the shorter side decides");
        assert_eq!(levels_to_drop(1024, 1024, 0), 0, "no limit, no change");
    }

    #[test]
    fn the_limit_is_capped() {
        assert_eq!(levels_to_drop(1 << 14, 1 << 14, 99), MAX_LIMIT);
    }

    #[test]
    fn halving_averages_each_block() {
        // 2x2 → 1x1: the mean of the four pixels, per channel.
        let px = [0, 0, 0, 255, 255, 255, 255, 255, 0, 255, 0, 255, 255, 0, 255, 255];
        let (w, h, out) = reduce(2, 2, &px, 1);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![128, 128, 128, 255]);
    }

    #[test]
    fn odd_sizes_fold_the_last_row_and_column() {
        // A 3x1 red-red-blue strip halves to one pixel of the first pair.
        let px = [255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255];
        let (w, h, out) = reduce(3, 1, &px, 1);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![255, 0, 0, 255]);
    }
}
