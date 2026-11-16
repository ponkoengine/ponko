//! CPU-only RGBA8 comparison for tools and render tests.
//! Preserves the renderer's symmetric edge allowance. SSIM uses global Rec.709
//! luminance with standard constants (K1=.01, K2=.03, L=255): inexpensive,
//! interpretable structural evidence, never a replacement for pixel thresholds.
use std::path::Path;

/// The class of failure; match with a wildcard for future kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Image dimensions, buffers or tolerance are invalid.
    Invalid,
    /// A file could not be read or a new output created.
    Io,
    /// Image bytes could not be decoded or encoded.
    Decode,
}

/// A comparison failure, with an optional file and underlying cause.
#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    path: Option<std::path::PathBuf>,
    message: String,
    source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>>,
}

impl Error {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Invalid,
            path: None,
            message: message.into(),
            source: None,
        }
    }

    /// A failed file operation: what went wrong, then `remedy`.
    fn file(
        kind: ErrorKind,
        path: &Path,
        source: impl std::error::Error + Send + Sync + 'static,
        remedy: &str,
    ) -> Self {
        Self {
            kind,
            path: Some(path.to_path_buf()),
            message: format!("{source}; {remedy}"),
            source: Some(std::sync::Arc::new(source)),
        }
    }

    /// An `image` crate failure: reading or writing the file, or its bytes.
    fn image(path: &Path, source: image::ImageError, remedy: &str) -> Self {
        let kind = if matches!(source, image::ImageError::IoError(_)) {
            ErrorKind::Io
        } else {
            ErrorKind::Decode
        };
        Self::file(kind, path, source, remedy)
    }

    /// What failed, without parsing the human-readable message.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// File involved, when this is a file operation.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Explanation and suggested remedy.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ponko: ")?;
        if let Some(path) = &self.path {
            write!(f, "{}: ", path.display())?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|e| e as _)
    }
}

/// Allows string-returning tool frontends to propagate comparison errors with `?`.
impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}

/// Borrowed top-to-bottom RGBA8 pixels (including alpha).
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Exactly width * height * 4 bytes.
    pub rgba: &'a [u8],
}

/// Bounding box of a four-connected component of pixels beyond tolerance.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Region {
    /// Leftmost pixel.
    pub x: u32,
    /// Topmost pixel.
    pub y: u32,
    /// Bounding width.
    pub width: u32,
    /// Bounding height.
    pub height: u32,
    /// Number of differing pixels in this component.
    pub pixels: usize,
}

/// How far an image may stray from what it's compared with: each channel by
/// up to `channel`, and beyond that on at most `outliers` (0..=1) of the
/// pixels.
///
/// `edge` is for comparing frames of two adapters. Rasterisers put an edge
/// (a silhouette, a shadow's border) up to a pixel apart and resolve its
/// samples differently, so along edges the same picture differs by the
/// whole contrast of the edge. With `edge` 1 a pixel is compared with the
/// other image's pixels up to one pixel away in each direction: it is fine
/// when every channel lies within `channel` of the range those pixels
/// span, and the same is asked of the other image's pixel against this
/// image's neighbours (so a thin line missing on either side still
/// counts). Flat areas are compared as with `edge` 0; an edge that moved
/// further than `edge` pixels, or a shade that changed, is beyond the
/// tolerance. `outliers` then only has to cover what isn't an edge.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tolerance {
    /// Absolute per-channel slack, 0..=255.
    pub channel: u8,
    /// Allowed fraction of pixels beyond slack, 0..=1.
    pub outliers: f32,
    /// How many pixels an edge may sit apart (0: pixels are compared in
    /// place).
    #[serde(default)]
    pub edge: u32,
}

/// How two images of the same size differ.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Diff {
    /// The largest difference of any channel.
    pub max_channel: u8,
    /// The mean absolute difference over every channel, 0..=255.
    pub mean: f32,
    /// Pixels with a channel differing by more than the tolerance's (after
    /// the tolerance's allowance for edges).
    pub outliers: usize,
    /// Total number of compared pixels.
    pub pixels: usize,
    /// The largest difference left after the allowance for edges: what
    /// `channel` would have to be for no pixel to be beyond it.
    pub beyond: u8,
    /// Pixels of the second image that lie on an edge: a neighbour within
    /// the tolerance's `edge` differs from them by more than `channel`.
    /// (0 without an allowance for edges.)
    pub edges: usize,
    /// Percentage of pixels beyond the edge-aware channel tolerance (0..=100).
    pub percent_over: f64,
    /// Global luminance SSIM, -1..=1; 1 means identical luminance structure.
    pub ssim: f64,
    /// Up to eight largest four-connected differing regions, largest first.
    pub regions: Vec<Region>,
}

impl Diff {
    /// Whether outliers fit the original render-test fractional budget.
    pub fn within(&self, tolerance: Tolerance) -> bool {
        self.outliers as f32 <= tolerance.outliers * self.pixels as f32
    }
}

impl std::fmt::Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let share = |n: usize| n as f32 * 100.0 / self.pixels.max(1) as f32;
        write!(
            f,
            "max channel difference {} ({} off edges), mean {:.3}, {} of {} pixels ({:.2}%) beyond the tolerance, \
             {:.1}% of pixels on edges",
            self.max_channel,
            self.beyond,
            self.mean,
            self.outliers,
            self.pixels,
            share(self.outliers),
            share(self.edges)
        )?;
        write!(f, ", SSIM {:.6}, regions {:?}", self.ssim, self.regions)
    }
}

/// How far `pixel` is outside the range that `other`'s pixels up to `reach`
/// away from (x, y) span, as the worst channel; and that range's own width.
fn outside(pixel: &[u8], other: &Image<'_>, x: usize, y: usize, reach: usize) -> (u8, u8) {
    let (width, height) = (other.width as usize, other.height as usize);
    let (mut low, mut high) = ([255u8; 4], [0u8; 4]);
    for ny in y.saturating_sub(reach)..=y.saturating_add(reach).min(height - 1) {
        for nx in x.saturating_sub(reach)..=x.saturating_add(reach).min(width - 1) {
            let at = (ny * width + nx) * 4;
            for c in 0..4 {
                low[c] = low[c].min(other.rgba[at + c]);
                high[c] = high[c].max(other.rgba[at + c]);
            }
        }
    }
    let excess = (0..4)
        .map(|c| low[c].saturating_sub(pixel[c]).max(pixel[c].saturating_sub(high[c])))
        .max()
        .unwrap();
    (excess, (0..4).map(|c| high[c] - low[c]).max().unwrap())
}

/// What the pixel at (x, y) differs by after the allowance for edges (each
/// image's pixel against the other's neighbours, the worse of the two),
/// and whether `b` has an edge there wider than `channel`.
fn excess(a: &Image<'_>, b: &Image<'_>, x: usize, y: usize, tolerance: Tolerance) -> (u8, bool) {
    let (at, reach) = ((y * a.width as usize + x) * 4, tolerance.edge as usize);
    let (p, q) = (&a.rgba[at..at + 4], &b.rgba[at..at + 4]);
    let (ours, span) = outside(p, b, x, y, reach);
    (ours.max(outside(q, a, x, y, reach).0), span > tolerance.channel)
}

/// How `a` differs from `b` under `tolerance`: its `channel`, and its
/// allowance for edges (see [`Tolerance`]). Returns an error for size mismatch,
/// malformed/empty buffers or a non-finite/out-of-range outlier fraction.
pub fn compare(a: &Image<'_>, b: &Image<'_>, tolerance: Tolerance) -> Result<Diff, Error> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(Error::invalid(format!(
            "image size mismatch: {}x{} versus {}x{}; compare images rendered at the same size",
            a.width, a.height, b.width, b.height
        )));
    }
    let bytes = (a.width as usize)
        .checked_mul(a.height as usize)
        .and_then(|n| n.checked_mul(4));
    if a.width == 0 || a.height == 0 || bytes != Some(a.rgba.len()) || bytes != Some(b.rgba.len()) {
        return Err(Error::invalid(
            "images must have nonzero dimensions and exactly width * height * 4 RGBA bytes; provide a complete RGBA8 buffer",
        ));
    }
    if !tolerance.outliers.is_finite() || !(0.0..=1.0).contains(&tolerance.outliers) {
        return Err(Error::invalid(
            "outliers must be a finite fraction in 0..=1; set Tolerance::outliers to a fraction such as 0.01",
        ));
    }
    let (width, height) = (a.width as usize, a.height as usize);
    let (mut max_channel, mut sum, mut outliers, mut beyond, mut edges) = (0u8, 0u64, 0usize, 0u8, 0usize);
    let mut mask = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            let (p, q) = (&a.rgba[at..at + 4], &b.rgba[at..at + 4]);
            sum += p.iter().zip(q).map(|(x, y)| u64::from(x.abs_diff(*y))).sum::<u64>();
            max_channel = max_channel.max(p.iter().zip(q).map(|(x, y)| x.abs_diff(*y)).max().unwrap());
            let (left, edge) = excess(a, b, x, y, tolerance);
            beyond = beyond.max(left);
            mask[y * width + x] = left > tolerance.channel;
            outliers += usize::from(left > tolerance.channel);
            edges += usize::from(edge);
        }
    }
    let pixels = a.rgba.len() / 4;
    Ok(Diff {
        max_channel,
        mean: sum as f32 / (pixels * 4).max(1) as f32,
        outliers,
        pixels,
        beyond,
        edges,
        percent_over: outliers as f64 * 100.0 / pixels as f64,
        ssim: luminance_ssim(a.rgba, b.rgba),
        regions: regions(&mut mask, width, height),
    })
}

fn luminance_ssim(a: &[u8], b: &[u8]) -> f64 {
    let luma = |p: &[u8; 4]| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
    let n = (a.len() / 4) as f64;
    let ma = a.as_chunks::<4>().0.iter().map(luma).sum::<f64>() / n;
    let mb = b.as_chunks::<4>().0.iter().map(luma).sum::<f64>() / n;
    let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
    for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()) {
        let (x, y) = (luma(p) - ma, luma(q) - mb);
        va += x * x;
        vb += y * y;
        cov += x * y;
    }
    let (c1, c2) = (2.55_f64.powi(2), 7.65_f64.powi(2));
    (((2.0 * ma * mb + c1) * (2.0 * cov / n + c2)) / ((ma * ma + mb * mb + c1) * ((va + vb) / n + c2))).clamp(-1.0, 1.0)
}

fn regions(mask: &mut [bool], width: usize, height: usize) -> Vec<Region> {
    let mut result = Vec::new();
    let mut stack = Vec::new();
    for at in 0..mask.len() {
        if !mask[at] {
            continue;
        }
        mask[at] = false;
        stack.push(at);
        let (mut x0, mut x1, mut y0, mut y1, mut pixels) = (at % width, at % width, at / width, at / width, 0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % width, i / width);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            pixels += 1;
            let neighbours = [
                x.checked_sub(1).map(|x| y * width + x),
                (x + 1 < width).then_some(i + 1),
                y.checked_sub(1).map(|y| y * width + x),
                (y + 1 < height).then_some(i + width),
            ];
            for j in neighbours.into_iter().flatten() {
                if mask[j] {
                    mask[j] = false;
                    stack.push(j);
                }
            }
        }
        result.push(Region {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0 + 1) as u32,
            height: (y1 - y0 + 1) as u32,
            pixels,
        });
        result.sort_by_key(|r| std::cmp::Reverse(r.pixels));
        result.truncate(8);
    }
    result
}

/// Difference heatmap: outliers red, matching pixels dim grey.
/// Validates the same inputs as [`compare`].
pub fn heatmap(a: &Image<'_>, b: &Image<'_>, tolerance: Tolerance) -> Result<Vec<u8>, Error> {
    compare(a, b, tolerance)?;
    let mut rgba = a.rgba.to_vec();
    for y in 0..a.height as usize {
        for x in 0..a.width as usize {
            let p = &mut rgba[(y * a.width as usize + x) * 4..][..4];
            let grey = ((u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 12) as u8;
            p.copy_from_slice(&if excess(a, b, x, y, tolerance).0 > tolerance.channel {
                [255, 0, 0, 255]
            } else {
                [grey, grey, grey, 255]
            });
        }
    }
    Ok(rgba)
}

/// Compare PNG files, optionally writing a heatmap to a **new** PNG file.
/// Existing outputs are never overwritten; decode, size and IO errors are explicit.
/// Paths are local to the calling process, independent of a running game.
pub fn compare_files(a: &Path, b: &Path, tolerance: Tolerance, output: Option<&Path>) -> Result<Diff, Error> {
    let load = |path: &Path| {
        image::open(path)
            .map(|im| im.to_rgba8())
            .map_err(|e| Error::image(path, e, "check that the path names a readable PNG image"))
    };
    let (a, b) = (load(a)?, load(b)?);
    let view = |im: &image::RgbaImage| (im.width(), im.height());
    let (w, h) = view(&a);
    let a = Image {
        width: w,
        height: h,
        rgba: a.as_raw(),
    };
    let b = Image {
        width: b.width(),
        height: b.height(),
        rgba: b.as_raw(),
    };
    let summary = compare(&a, &b, tolerance)?;
    if let Some(path) = output {
        use image::ImageEncoder;
        let rgba = heatmap(&a, &b, tolerance)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
                let remedy = if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "heatmaps never overwrite a file; choose a new output path"
                } else {
                    "choose a heatmap path in a writable folder"
                };
                Error::file(ErrorKind::Io, path, e, remedy)
            })?;
        image::codecs::png::PngEncoder::new(file)
            .write_image(&rgba, w, h, image::ExtendedColorType::Rgba8)
            .map_err(|e| Error::image(path, e, "check free disk space and that the heatmap path is writable"))?;
    }
    Ok(summary)
}
