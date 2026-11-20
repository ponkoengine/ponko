//! Decoded video planes (Y'CbCr, 8 bits) to RGBA pixels on the CPU.
//!
//! Integer math in 16.16 fixed point, one row at a time; tall frames are
//! split over rayon's threads. Pure Rust, no `unsafe`.

use rayon::prelude::*;

/// How the color planes are subsampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Subsampling {
    /// 4:2:0: half width, half height.
    Both,
    /// 4:2:2: half width.
    Horizontal,
    /// 4:4:4.
    None,
    /// Luma only (monochrome).
    Gray,
}

/// The Y'CbCr to RGB matrix the video was encoded with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

/// One decoded frame's planes.
pub(crate) struct Planes<'a> {
    pub width: usize,
    pub height: usize,
    pub subsampling: Subsampling,
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
    pub luma_stride: usize,
    pub chroma_stride: usize,
    pub matrix: Matrix,
    /// 0-255 for every channel; otherwise "TV" range (16-235 luma).
    pub full_range: bool,
}

/// Fixed-point coefficients: `r = y + rv*v`, `g = y - gu*u - gv*v`,
/// `b = y + bu*u`, after scaling luma.
struct Coefficients {
    y_scale: i32,
    y_offset: i32,
    rv: i32,
    gu: i32,
    gv: i32,
    bu: i32,
}

fn coefficients(matrix: Matrix, full_range: bool) -> Coefficients {
    let (kr, kb) = match matrix {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
        Matrix::Bt2020 => (0.2627, 0.0593),
    };
    let kg = 1.0 - kr - kb;
    // Limited range: luma 16-235, chroma 16-240 around 128.
    let (y_scale, y_offset, c_scale) = if full_range {
        (1.0, 0, 1.0)
    } else {
        (255.0 / 219.0, 16, 255.0 / 224.0)
    };
    let fixed = |x: f64| (x * 65536.0).round() as i32;
    Coefficients {
        y_scale: fixed(y_scale),
        y_offset,
        rv: fixed(2.0 * (1.0 - kr) * c_scale),
        gu: fixed(2.0 * (1.0 - kb) * kb / kg * c_scale),
        gv: fixed(2.0 * (1.0 - kr) * kr / kg * c_scale),
        bu: fixed(2.0 * (1.0 - kb) * c_scale),
    }
}

/// Frames at least this many pixels tall are converted on several threads.
const PARALLEL_ROWS: usize = 256;
/// Rows per task: a 1080p frame is 17 tasks, not 1080.
const ROWS_PER_TASK: usize = 64;

/// Writes `planes` into `rgba` (`width * height * 4` bytes, top row first,
/// alpha 255).
pub(crate) fn to_rgba(planes: &Planes<'_>, rgba: &mut [u8]) {
    let row_bytes = planes.width * 4;
    debug_assert_eq!(rgba.len(), row_bytes * planes.height);
    let c = coefficients(planes.matrix, planes.full_range);
    let convert = |(row, out): (usize, &mut [u8])| convert_row(planes, &c, row, out);
    if planes.height >= PARALLEL_ROWS {
        rgba.par_chunks_mut(row_bytes)
            .enumerate()
            .with_min_len(ROWS_PER_TASK)
            .for_each(convert);
    } else {
        rgba.chunks_mut(row_bytes).enumerate().for_each(convert);
    }
}

fn convert_row(planes: &Planes<'_>, c: &Coefficients, row: usize, out: &mut [u8]) {
    let y_row = &planes.y[row * planes.luma_stride..][..planes.width];
    let clamp = |x: i32| ((x + 32768) >> 16).clamp(0, 255) as u8;
    if planes.subsampling == Subsampling::Gray {
        for (px, &y) in out.as_chunks_mut::<4>().0.iter_mut().zip(y_row) {
            let l = clamp((i32::from(y) - c.y_offset) * c.y_scale);
            *px = [l, l, l, 255];
        }
        return;
    }
    let chroma_row = if planes.subsampling == Subsampling::Both {
        row / 2
    } else {
        row
    };
    let u_row = &planes.u[chroma_row * planes.chroma_stride..];
    let v_row = &planes.v[chroma_row * planes.chroma_stride..];
    let shift = usize::from(planes.subsampling != Subsampling::None);
    for (x, (px, &y)) in out.as_chunks_mut::<4>().0.iter_mut().zip(y_row).enumerate() {
        let y = (i32::from(y) - c.y_offset) * c.y_scale;
        let u = i32::from(u_row[x >> shift]) - 128;
        let v = i32::from(v_row[x >> shift]) - 128;
        px[0] = clamp(y + c.rv * v);
        px[1] = clamp(y - c.gu * u - c.gv * v);
        px[2] = clamp(y + c.bu * u);
        px[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_pixel(y: u8, u: u8, v: u8, matrix: Matrix, full_range: bool) -> [u8; 4] {
        let planes = Planes {
            width: 1,
            height: 1,
            subsampling: Subsampling::None,
            y: &[y],
            u: &[u],
            v: &[v],
            luma_stride: 1,
            chroma_stride: 1,
            matrix,
            full_range,
        };
        let mut rgba = [0u8; 4];
        to_rgba(&planes, &mut rgba);
        rgba
    }

    #[test]
    fn limited_range_black_and_white_reach_the_ends() {
        assert_eq!(one_pixel(16, 128, 128, Matrix::Bt709, false), [0, 0, 0, 255]);
        assert_eq!(one_pixel(235, 128, 128, Matrix::Bt709, false), [255, 255, 255, 255]);
        assert_eq!(one_pixel(0, 128, 128, Matrix::Bt709, true), [0, 0, 0, 255]);
    }

    #[test]
    fn bt709_primaries_convert_back_to_their_colors() {
        // BT.709 limited range codes of pure red, green and blue.
        let close = |a: [u8; 4], b: [u8; 3]| a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 2);
        assert!(close(one_pixel(63, 102, 240, Matrix::Bt709, false), [255, 0, 0]));
        assert!(close(one_pixel(173, 42, 26, Matrix::Bt709, false), [0, 255, 0]));
        assert!(close(one_pixel(32, 240, 118, Matrix::Bt709, false), [0, 0, 255]));
    }

    #[test]
    fn bt601_red_differs_from_bt709_red() {
        // The same code read with the other matrix is a different red.
        let red601 = one_pixel(81, 90, 240, Matrix::Bt601, false);
        assert!(red601[0] > 250 && red601[1] < 5 && red601[2] < 5, "{red601:?}");
        assert_ne!(one_pixel(81, 90, 240, Matrix::Bt709, false), red601);
    }

    #[test]
    fn subsampled_chroma_covers_two_by_two_pixels_and_odd_sizes() {
        // 3 x 3 luma, 2 x 2 chroma: the last column and row use the last chroma sample.
        let planes = Planes {
            width: 3,
            height: 3,
            subsampling: Subsampling::Both,
            y: &[126; 9],
            u: &[128, 128, 128, 240],
            v: &[128, 128, 128, 118],
            luma_stride: 3,
            chroma_stride: 2,
            matrix: Matrix::Bt709,
            full_range: false,
        };
        let mut rgba = vec![0u8; 36];
        to_rgba(&planes, &mut rgba);
        let px = |x: usize, y: usize| &rgba[(y * 3 + x) * 4..][..4];
        assert_eq!(px(0, 0), px(1, 1), "gray block");
        assert!(px(2, 2)[2] > px(0, 0)[2] + 100, "bottom-right is blue: {:?}", px(2, 2));
        assert_eq!(px(2, 0), px(0, 0), "top-right reads the gray chroma sample");
    }
}
