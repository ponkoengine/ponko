//! GPU block-compressed textures (feature `compressed-textures`).
//!
//! Reading KTX2 and DDS files whose pixels are already in a GPU format
//! (BC1-BC7, ETC2/EAC, ASTC 4x4), so they upload as they are, with their
//! mipmaps, and stay compressed in video memory. Basis Universal UASTC
//! files are transcoded to what the GPU samples (feature
//! `basis-universal`, native builds only). A GPU without the file's format
//! gets level 0 decoded to RGBA8 on the CPU and goes the usual way.
//!
//! The decode is CPU work with no GPU (any thread), like
//! [`decode_image`](crate::assets::decode_image); the renderer turns a
//! [`CompressedImage`] into a texture.

use crate::assets::Image;
use crate::error::{Error, ErrorKind};

/// A block-compressed GPU format. Every one has 4 x 4 pixel blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BlockFormat {
    /// RGB with 1-bit alpha, 8 bytes a block.
    Bc1,
    /// RGBA with explicit 4-bit alpha.
    Bc2,
    /// RGBA with interpolated alpha.
    Bc3,
    /// One channel (masks), 8 bytes a block.
    Bc4,
    /// Two channels (normal maps).
    Bc5,
    /// High-quality RGBA.
    Bc7,
    /// ETC2 RGB, 8 bytes a block.
    Etc2Rgb8,
    /// ETC2 RGBA.
    Etc2Rgba8,
    /// EAC one channel, 8 bytes a block.
    EacR11,
    /// EAC two channels.
    EacRg11,
    /// ASTC with 4 x 4 blocks.
    Astc4x4,
}

/// The pixel size of every [`BlockFormat`]'s block.
pub(crate) const BLOCK: u32 = 4;

impl BlockFormat {
    /// Bytes of one 4 x 4 block.
    pub(crate) fn block_bytes(self) -> u32 {
        match self {
            BlockFormat::Bc1 | BlockFormat::Bc4 | BlockFormat::Etc2Rgb8 | BlockFormat::EacR11 => 8,
            _ => 16,
        }
    }

    /// Whether a GPU with `gpu` samples it.
    pub(crate) fn sampled_by(self, gpu: GpuFormats) -> bool {
        match self {
            BlockFormat::Bc1
            | BlockFormat::Bc2
            | BlockFormat::Bc3
            | BlockFormat::Bc4
            | BlockFormat::Bc5
            | BlockFormat::Bc7 => gpu.bc,
            BlockFormat::Etc2Rgb8 | BlockFormat::Etc2Rgba8 | BlockFormat::EacR11 | BlockFormat::EacRg11 => gpu.etc2,
            BlockFormat::Astc4x4 => gpu.astc,
        }
    }

    /// Whether it holds only red and green (a normal map's X and Y).
    pub(crate) fn two_channel(self) -> bool {
        matches!(self, BlockFormat::Bc5 | BlockFormat::EacRg11)
    }

    /// Whether it has an sRGB variant (the one- and two-channel ones don't).
    pub(crate) fn has_srgb(self) -> bool {
        !matches!(
            self,
            BlockFormat::Bc4 | BlockFormat::Bc5 | BlockFormat::EacR11 | BlockFormat::EacRg11
        )
    }

    /// Its name in messages, such as `"BC7"`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            BlockFormat::Bc1 => "BC1",
            BlockFormat::Bc2 => "BC2",
            BlockFormat::Bc3 => "BC3",
            BlockFormat::Bc4 => "BC4",
            BlockFormat::Bc5 => "BC5",
            BlockFormat::Bc7 => "BC7",
            BlockFormat::Etc2Rgb8 => "ETC2 RGB",
            BlockFormat::Etc2Rgba8 => "ETC2 RGBA",
            BlockFormat::EacR11 => "EAC R11",
            BlockFormat::EacRg11 => "EAC RG11",
            BlockFormat::Astc4x4 => "ASTC 4x4",
        }
    }

    /// Bytes of a `width` x `height` image (blocks at the edges count whole).
    pub(crate) fn level_bytes(self, width: u32, height: u32) -> u64 {
        u64::from(width.div_ceil(BLOCK)) * u64::from(height.div_ceil(BLOCK)) * u64::from(self.block_bytes())
    }
}

/// Which block-compressed format families the GPU samples (its wgpu
/// `TEXTURE_COMPRESSION_*` features).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GpuFormats {
    /// BC1-BC7 (desktop GPUs, Apple silicon).
    pub bc: bool,
    /// ETC2 and EAC (mobile GPUs, most WebGL2 on phones).
    pub etc2: bool,
    /// ASTC (mobile GPUs, Apple silicon).
    pub astc: bool,
}

impl GpuFormats {
    /// Every family: headless runs, where nothing is decoded for a GPU.
    pub(crate) const ALL: GpuFormats = GpuFormats {
        bc: true,
        etc2: true,
        astc: true,
    };
}

/// Where a normal map keeps its X and Y. A two-channel normal map has no Z:
/// the material shader rebuilds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum NormalChannels {
    /// X, Y and Z in red, green and blue (any RGB normal map).
    #[default]
    Rgb,
    /// X in red, Y in green (BC5, EAC RG11).
    Rg,
    /// X in red, Y in alpha (a two-channel UASTC file transcoded to RGBA).
    #[cfg_attr(
        not(all(feature = "basis-universal", not(target_arch = "wasm32"))),
        allow(dead_code, reason = "only UASTC transcoding makes it")
    )]
    Ra,
}

/// A texture in a GPU block format, as the file had it (or as UASTC was
/// transcoded to).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompressedImage {
    pub width: u32,
    pub height: u32,
    pub format: BlockFormat,
    /// Colors (sampled through sRGB decoding) or data (as stored).
    pub srgb: bool,
    /// Mip levels, largest first: each one's blocks, row by row.
    pub levels: Vec<Vec<u8>>,
    /// Where a normal map's channels are.
    pub normal: NormalChannels,
}

impl CompressedImage {
    /// The size of mip level `level`.
    pub(crate) fn level_size(&self, level: usize) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// Bytes on the GPU: every level.
    pub(crate) fn bytes(&self) -> u64 {
        self.levels.iter().map(|l| l.len() as u64).sum()
    }

    /// The same texture from mip level `first` down: a smaller resolution
    /// for streaming, with its own mipmaps.
    pub(crate) fn starting_at_level(&self, first: usize) -> CompressedImage {
        let (width, height) = self.level_size(first);
        CompressedImage {
            width,
            height,
            format: self.format,
            srgb: self.srgb,
            levels: self.levels[first..].to_vec(),
            normal: self.normal,
        }
    }
}

/// Whether a GPU texture can start at a `width` x `height` level of a block
/// format: whole blocks only.
pub(crate) fn block_aligned(width: u32, height: u32) -> bool {
    width.is_multiple_of(BLOCK) && height.is_multiple_of(BLOCK)
}

/// A decoded texture file: compressed blocks to upload as they are, or
/// RGBA8 (an image file, or a compressed one this GPU can't sample).
pub(crate) enum Decoded {
    Rgba(Image),
    Blocks(CompressedImage),
}

const KTX2_MAGIC: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];

/// Whether `bytes` start like a KTX2 file.
pub(crate) fn is_ktx2(bytes: &[u8]) -> bool {
    bytes.starts_with(&KTX2_MAGIC)
}

/// Decodes a KTX2 file, or a DDS file holding BCn blocks, for a GPU that
/// samples `gpu`. `None`: not one of those (an image file, or a DDS file
/// of plain pixels for the image decoder). `path` and `what` are for
/// messages, as in [`decode_image`](crate::assets::decode_image).
pub(crate) fn decode(bytes: &[u8], path: &str, what: &str, gpu: GpuFormats) -> Option<Result<Decoded, Error>> {
    let mut slices = match DecodeSlices::new(bytes, path, what, gpu)? {
        Ok(slices) => slices.with_slice_bytes(usize::MAX),
        Err(e) => return Some(Err(e)),
    };
    Some(loop {
        match slices.step(bytes) {
            Ok(Some(decoded)) => break Ok(decoded),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
    })
}

/// [`decode`] a slice at a time, for a streamed texture where there are no
/// worker threads: a KTX2 file's levels unpacked from their
/// supercompression, UASTC transcoded and blocks the GPU can't sample
/// decoded to RGBA8, each a bounded number of output bytes a slice (a band
/// of whole block rows, at least one). `decode` runs the same slices
/// without the bound, so both make the same texture.
pub(crate) struct DecodeSlices {
    path: String,
    what: String,
    gpu: GpuFormats,
    /// Output bytes a slice makes at most (at least one block row).
    slice: usize,
    phase: SlicePhase,
}

enum SlicePhase {
    /// A KTX2 file's levels being unpacked.
    Unpack(Box<Unpack>),
    /// Every level there: what the GPU gets is chosen next.
    Read(Box<File>),
    /// What the GPU gets being made.
    Gpu(Box<GpuWork>),
    Taken,
}

/// A KTX2 file whose levels are being unpacked into `file`.
struct Unpack {
    file: File,
    /// Each level's bytes in the file, and its unpacked length.
    ranges: Vec<(std::ops::Range<usize>, usize)>,
    scheme: Option<ktx2::SupercompressionScheme>,
    /// The level being unpacked.
    current: Option<Inflate>,
}

/// A supercompressed level part way through.
enum Inflate {
    Zstd {
        decoder: Box<ruzstd::decoding::FrameDecoder>,
        /// Bytes of the level read so far.
        read: usize,
        out: Vec<u8>,
    },
    Zlib {
        decoder: Box<flate2::Decompress>,
        out: Vec<u8>,
    },
}

/// What the GPU gets, being made from the read file.
enum GpuWork {
    /// Level 0 of blocks the GPU can't sample, decoded to RGBA8 a band of
    /// block rows at a time.
    Cpu {
        file: File,
        format: BlockFormat,
        rgba: Vec<u8>,
        /// The next block row.
        row: u32,
    },
    /// UASTC transcoded a band of block rows at a time.
    #[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
    Uastc(Box<Transcode>),
}

impl DecodeSlices {
    /// Starts decoding a KTX2 or DDS file `bytes` (reading its header, and
    /// a DDS file's levels); `None` for other files, as [`decode`].
    pub(crate) fn new(bytes: &[u8], path: &str, what: &str, gpu: GpuFormats) -> Option<Result<Self, Error>> {
        let phase = if is_ktx2(bytes) {
            read_ktx2_header(bytes, path, what).map(|unpack| SlicePhase::Unpack(Box::new(unpack)))
        } else if bytes.starts_with(b"DDS ") {
            read_dds(bytes, path, what)?.map(|file| SlicePhase::Read(Box::new(file)))
        } else {
            return None;
        };
        Some(phase.map(|phase| Self {
            path: path.to_string(),
            what: what.to_string(),
            gpu,
            slice: crate::assets::group::sliced::SLICE_BYTES,
            phase,
        }))
    }

    /// Makes at most `bytes` of output a slice (at least one block row).
    pub(crate) fn with_slice_bytes(mut self, bytes: usize) -> Self {
        self.slice = bytes.max(1);
        self
    }

    /// The next slice of the decode of `bytes` (the file's, as given to
    /// [`new`](Self::new)): the texture once it is done.
    pub(crate) fn step(&mut self, bytes: &[u8]) -> Result<Option<Decoded>, Error> {
        let (path, what) = (self.path.as_str(), self.what.as_str());
        match std::mem::replace(&mut self.phase, SlicePhase::Taken) {
            SlicePhase::Unpack(mut unpack) => {
                self.phase = if unpack.step(bytes, self.slice, path, what)? {
                    check_levels(&unpack.file, path, what)?;
                    SlicePhase::Read(Box::new(unpack.file))
                } else {
                    SlicePhase::Unpack(unpack)
                };
                Ok(None)
            }
            SlicePhase::Read(file) => match choose_for_gpu(*file, path, what, self.gpu)? {
                Ok(decoded) => Ok(Some(decoded)),
                Err(work) => {
                    self.phase = SlicePhase::Gpu(Box::new(work));
                    Ok(None)
                }
            },
            SlicePhase::Gpu(mut work) => {
                if let Some(decoded) = work.step(self.slice, path, what)? {
                    return Ok(Some(decoded));
                }
                self.phase = SlicePhase::Gpu(work);
                Ok(None)
            }
            SlicePhase::Taken => Err(Error::new(ErrorKind::Invalid, format!("{what} was decoded already"))),
        }
    }
}

/// Block rows of `format` at `width` that make about `bytes` of output,
/// at `out_block` bytes an output block (at least one row).
fn band_rows(width: u32, out_block: usize, bytes: usize) -> u32 {
    let row = (width.div_ceil(BLOCK) as usize).saturating_mul(out_block).max(1);
    u32::try_from(bytes / row).unwrap_or(u32::MAX).max(1)
}

impl Unpack {
    /// Unpacks the next piece of the levels, `slice` bytes of output at
    /// most: `true` once every level is in `file`.
    fn step(&mut self, bytes: &[u8], slice: usize, path: &str, what: &str) -> Result<bool, Error> {
        let i = self.file.levels.len();
        let Some((range, unpacked)) = self.ranges.get(i).cloned() else {
            return Ok(true);
        };
        let data = bytes.get(range).ok_or_else(|| {
            invalid(
                path,
                what,
                format_args!("KTX2: mip level {i} is past the end of the file"),
            )
        })?;
        let too_long = |i: usize, unpacked: usize| {
            invalid(
                path,
                what,
                format_args!("KTX2: mip level {i} unpacks to more than its {unpacked} bytes"),
            )
        };
        let level = match self.scheme {
            None => Some(data.to_vec()),
            Some(ktx2::SupercompressionScheme::Zstandard) => {
                let zstd = |e: &dyn std::fmt::Display| invalid(path, what, format_args!("Zstandard: {e}"));
                let current = match self.current.take() {
                    Some(current) => current,
                    None => {
                        let mut decoder = Box::new(ruzstd::decoding::FrameDecoder::new());
                        let mut source = data;
                        decoder.init(&mut source).map_err(|e| zstd(&e))?;
                        Inflate::Zstd {
                            decoder,
                            read: data.len() - source.len(),
                            out: Vec::with_capacity(unpacked),
                        }
                    }
                };
                let Inflate::Zstd {
                    mut decoder,
                    mut read,
                    mut out,
                } = current
                else {
                    unreachable!("a Zstandard file unpacks with Zstandard")
                };
                let mut source = &data[read..];
                // Never much past the level's length: a few bytes of
                // Zstandard can unpack to gigabytes.
                let room = unpacked.saturating_sub(out.len()).saturating_add(1);
                let done = decoder
                    .decode_blocks(
                        &mut source,
                        ruzstd::decoding::BlockDecodingStrategy::UptoBytes(slice.min(room)),
                    )
                    .map_err(|e| zstd(&e))?;
                read = data.len() - source.len();
                if let Some(piece) = decoder.collect() {
                    out.extend_from_slice(&piece);
                }
                if out.len() > unpacked {
                    return Err(too_long(i, unpacked));
                }
                if done {
                    Some(out)
                } else {
                    self.current = Some(Inflate::Zstd { decoder, read, out });
                    None
                }
            }
            Some(ktx2::SupercompressionScheme::ZLIB) => {
                let current = self.current.take().unwrap_or_else(|| Inflate::Zlib {
                    decoder: Box::new(flate2::Decompress::new(true)),
                    out: Vec::with_capacity(unpacked),
                });
                let Inflate::Zlib { mut decoder, mut out } = current else {
                    unreachable!("a zlib file unpacks with zlib")
                };
                let goal = out.len().saturating_add(slice);
                let status = loop {
                    if out.len() == out.capacity() {
                        out.reserve(slice.clamp(1, 64 << 10));
                    }
                    let input = &data[decoder.total_in() as usize..];
                    let before = (decoder.total_in(), decoder.total_out());
                    let status = decoder
                        .decompress_vec(input, &mut out, flate2::FlushDecompress::None)
                        .map_err(|e| invalid(path, what, format_args!("zlib: {e}")))?;
                    let stuck = (decoder.total_in(), decoder.total_out()) == before;
                    if out.len() > unpacked {
                        return Err(too_long(i, unpacked));
                    }
                    if status == flate2::Status::StreamEnd || out.len() >= goal || stuck {
                        break (status, stuck);
                    }
                };
                match status {
                    (flate2::Status::StreamEnd, _) => Some(out),
                    // Cut short: what it has, as a read to the end makes.
                    (_, true) => Some(out),
                    _ => {
                        self.current = Some(Inflate::Zlib { decoder, out });
                        None
                    }
                }
            }
            Some(scheme) => {
                return Err(unsupported(
                    path,
                    format!("{what} uses {scheme:?} supercompression, which ponko doesn't unpack"),
                ));
            }
        };
        if let Some(level) = level {
            self.file.levels.push(level);
        }
        Ok(self.file.levels.len() == self.ranges.len())
    }
}

impl GpuWork {
    /// The next band: the texture once it is done.
    fn step(&mut self, slice: usize, path: &str, what: &str) -> Result<Option<Decoded>, Error> {
        match self {
            GpuWork::Cpu {
                file,
                format,
                rgba,
                row,
            } => {
                let (width, height) = (file.width, file.height);
                let rows = band_rows(width, 4 * 4 * 4, slice);
                let (y0, y1) = (*row * BLOCK, row.saturating_add(rows).saturating_mul(BLOCK).min(height));
                let start = format.level_bytes(width, y0) as usize;
                let end = format.level_bytes(width, y1) as usize;
                let band = decode_blocks(*format, width, y1 - y0, &file.levels[0][start..end])
                    .map_err(|e| invalid(path, what, e))?;
                rgba.extend_from_slice(&band.rgba);
                *row = y1.div_ceil(BLOCK);
                if y1 < height {
                    return Ok(None);
                }
                Ok(Some(Decoded::Rgba(Image {
                    width,
                    height,
                    rgba: std::mem::take(rgba),
                })))
            }
            #[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
            GpuWork::Uastc(transcode) => transcode.step(slice, path, what),
        }
    }
}

/// [`decode`] to RGBA8 on the CPU, for loaders that need pixels rather
/// than a texture.
pub(crate) fn decode_rgba(bytes: &[u8], path: &str, what: &str) -> Option<Result<Image, Error>> {
    // No block format is sampled, so every file comes back as RGBA8.
    Some(
        decode(bytes, path, what, GpuFormats::default())?.map(|decoded| match decoded {
            Decoded::Rgba(image) => image,
            Decoded::Blocks(image) => {
                unreachable!("a {} image chosen for a GPU without block formats", image.format.name())
            }
        }),
    )
}

/// What a file holds, before choosing for a GPU.
enum Pixels {
    Blocks(BlockFormat),
    Rgba8,
    /// Basis Universal UASTC, and the channels it holds.
    Uastc(UastcChannels),
}

/// A UASTC file's channels (its data format descriptor says).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UastcChannels {
    Rgb,
    Rgba,
    /// One channel copied into red, green and blue.
    Rrr,
    /// X in red, green and blue, Y in alpha: a two-channel normal map.
    Rrrg,
    /// X in red, Y in green.
    Rg,
}

/// A texture file read into its levels.
struct File {
    width: u32,
    height: u32,
    pixels: Pixels,
    srgb: bool,
    /// Every mip level, largest first, unpacked from any supercompression.
    levels: Vec<Vec<u8>>,
}

fn unsupported(path: &str, message: String) -> Error {
    Error::new(ErrorKind::Unsupported, message).with_path(path)
}

fn invalid(path: &str, what: &str, why: impl std::fmt::Display) -> Error {
    Error::decode(path, format!("{what} is not a valid texture file: {why}"))
}

/// Opens a KTX2 file, refusing what the `ktx2` crate would panic on: a
/// basic data format descriptor block with a texel block dimension byte of
/// 255 (it adds one to it in a `u8`).
fn ktx2_reader(bytes: &[u8]) -> Result<ktx2::Reader<&[u8]>, String> {
    let word = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)) };
    // The descriptor's offset and length sit at bytes 48 and 52 of the
    // header; `Reader::new` checks they fit the file.
    if let (Some(offset), Some(length)) = (word(48), word(52)) {
        let (start, end) = (offset as usize, offset as usize + length as usize);
        // Skip the descriptor's own 4-byte total size; blocks follow.
        let mut at = start.saturating_add(4);
        while at + 16 <= end.min(bytes.len()) {
            let header = word(at).unwrap_or(0);
            let size = u16::from_le_bytes([bytes[at + 6], bytes[at + 7]]) as usize;
            // Vendor 0 (Khronos), type 0 (basic): texel block dimensions
            // at bytes 12..16.
            if header == 0 && bytes[at + 12..at + 16].contains(&255) {
                return Err("a texel block dimension of 256".into());
            }
            if size == 0 {
                break;
            }
            at += size;
        }
    }
    ktx2::Reader::new(bytes).map_err(|e| e.to_string())
}

/// Reads a KTX2 file's header: its levels are unpacked next ([`Unpack`]).
fn read_ktx2_header(bytes: &[u8], path: &str, what: &str) -> Result<Unpack, Error> {
    let reader = ktx2_reader(bytes).map_err(|e| invalid(path, what, format_args!("KTX2: {e}")))?;
    let header = reader.header();
    if header.face_count != 1 {
        return Err(unsupported(
            path,
            format!("{what} is a KTX2 cube map; ponko loads 2D KTX2 textures only"),
        ));
    }
    if header.pixel_depth > 1 || header.pixel_height == 0 {
        return Err(unsupported(
            path,
            format!("{what} is a 1D or 3D KTX2 texture; ponko loads 2D KTX2 textures only"),
        ));
    }
    if header.layer_count > 1 {
        return Err(unsupported(
            path,
            format!(
                "{what} is a KTX2 array of {} layers; load each layer from its own file with load_texture_array",
                header.layer_count
            ),
        ));
    }
    let srgb = reader.transfer_function() == Some(ktx2::TransferFunction::SRGB);
    let pixels = match header.format {
        Some(format) => match block_format(format) {
            Some(block) => Pixels::Blocks(block),
            None if format == ktx2::Format::R8G8B8A8_UNORM || format == ktx2::Format::R8G8B8A8_SRGB => Pixels::Rgba8,
            None => {
                return Err(unsupported(
                    path,
                    format!(
                        "{what} is a KTX2 texture in {format:?}, which ponko doesn't load: use BC1-BC7, \
                         ETC2/EAC, ASTC 4x4, RGBA8 or Basis Universal UASTC (`ponko textures` makes them)"
                    ),
                ));
            }
        },
        None if reader.color_model() == Some(ktx2::ColorModel::UASTC) => {
            // KHR_DF_CHANNEL_UASTC_*: which channels the blocks hold.
            let channel = reader
                .basic_dfd()
                .and_then(|dfd| dfd.sample_information.first())
                .map_or(0, |s| s.channel_type);
            Pixels::Uastc(match channel {
                3 => UastcChannels::Rgba,
                4 => UastcChannels::Rrr,
                5 => UastcChannels::Rrrg,
                6 => UastcChannels::Rg,
                _ => UastcChannels::Rgb,
            })
        }
        None if reader.color_model() == Some(ktx2::ColorModel::ETC1S) => {
            return Err(unsupported(
                path,
                format!(
                    "{what} is Basis Universal ETC1S (BasisLZ), which ponko doesn't transcode: \
                     encode it as UASTC (`ponko textures` with \"basis\": true) or as BC7"
                ),
            ));
        }
        None => {
            return Err(unsupported(
                path,
                format!("{what} is a KTX2 texture of an unknown format, which ponko doesn't load"),
            ));
        }
    };
    let scheme = header.supercompression_scheme;
    if let Some(scheme) = scheme
        && !matches!(
            scheme,
            ktx2::SupercompressionScheme::Zstandard | ktx2::SupercompressionScheme::ZLIB
        )
    {
        return Err(unsupported(
            path,
            format!("{what} uses {scheme:?} supercompression, which ponko doesn't unpack"),
        ));
    }
    let (width, height) = (header.pixel_width, header.pixel_height);
    crate::assets::check_image_size(width.into(), height.into(), path, what)?;
    // Where each level is in the file (the reader checked they are in it),
    // and how long it is unpacked: what its size needs, never what the
    // header claims, which a few bytes could make terabytes.
    let base = bytes.as_ptr() as usize;
    let mut ranges = Vec::new();
    for (i, level) in reader.levels().enumerate().take(33) {
        let at = level.data.as_ptr() as usize - base;
        let (w, h) = ((width >> i).max(1), (height >> i).max(1));
        let needed = level_len(&pixels, w, h);
        if scheme.is_some() && level.uncompressed_byte_length != needed {
            return Err(invalid(
                path,
                what,
                format_args!(
                    "KTX2: mip level {i} ({w} x {h}) unpacks to {} bytes, not {needed}",
                    level.uncompressed_byte_length
                ),
            ));
        }
        ranges.push((at..at + level.data.len(), needed as usize));
    }
    Ok(Unpack {
        file: File {
            width: header.pixel_width,
            height: header.pixel_height,
            pixels,
            srgb,
            levels: Vec::new(),
        },
        ranges,
        scheme,
        current: None,
    })
}

/// The KTX2 key `ponko cook` writes into an image file of a glTF model
/// that already holds the ORM image the engine would merge from the
/// model's metallic-roughness and occlusion images: its value names those
/// two files, relative to the image's folder, `metal-rough|occlusion`
/// (either empty when the material has none). The engine then uploads the
/// file as it is instead of decoding both and merging them to RGBA8.
/// Only glTF models (the `3d` feature) read it.
#[cfg(feature = "3d")]
pub(crate) const ORM_KEY: &str = "ponkoOrm";

/// The value of [`ORM_KEY`] in a KTX2 file, if it has one.
#[cfg(feature = "3d")]
pub(crate) fn orm_note(bytes: &[u8]) -> Option<String> {
    if !is_ktx2(bytes) {
        return None;
    }
    let reader = ktx2_reader(bytes).ok()?;
    let (_, value) = reader.key_value_data().find(|(key, _)| *key == ORM_KEY)?;
    let value = value.strip_suffix(&[0]).unwrap_or(value);
    String::from_utf8(value.to_vec()).ok()
}

/// The block format of a Vulkan format, if it is one ponko reads.
fn block_format(format: ktx2::Format) -> Option<BlockFormat> {
    use ktx2::Format as F;
    Some(match format {
        F::BC1_RGB_UNORM_BLOCK | F::BC1_RGB_SRGB_BLOCK | F::BC1_RGBA_UNORM_BLOCK | F::BC1_RGBA_SRGB_BLOCK => {
            BlockFormat::Bc1
        }
        F::BC2_UNORM_BLOCK | F::BC2_SRGB_BLOCK => BlockFormat::Bc2,
        F::BC3_UNORM_BLOCK | F::BC3_SRGB_BLOCK => BlockFormat::Bc3,
        F::BC4_UNORM_BLOCK => BlockFormat::Bc4,
        F::BC5_UNORM_BLOCK => BlockFormat::Bc5,
        F::BC7_UNORM_BLOCK | F::BC7_SRGB_BLOCK => BlockFormat::Bc7,
        F::ETC2_R8G8B8_UNORM_BLOCK | F::ETC2_R8G8B8_SRGB_BLOCK => BlockFormat::Etc2Rgb8,
        F::ETC2_R8G8B8A8_UNORM_BLOCK | F::ETC2_R8G8B8A8_SRGB_BLOCK => BlockFormat::Etc2Rgba8,
        F::EAC_R11_UNORM_BLOCK => BlockFormat::EacR11,
        F::EAC_R11G11_UNORM_BLOCK => BlockFormat::EacRg11,
        F::ASTC_4x4_UNORM_BLOCK | F::ASTC_4x4_SRGB_BLOCK => BlockFormat::Astc4x4,
        _ => return None,
    })
}

/// The bytes a `w` x `h` level of `pixels` takes.
fn level_len(pixels: &Pixels, w: u32, h: u32) -> u64 {
    match pixels {
        Pixels::Blocks(format) => format.level_bytes(w, h),
        Pixels::Rgba8 => u64::from(w) * u64::from(h) * 4,
        // UASTC blocks are 16 bytes each.
        Pixels::Uastc(_) => BlockFormat::Bc7.level_bytes(w, h),
    }
}

/// Checks every level holds exactly the bytes its size needs, so nothing
/// short reaches the GPU or the decoders.
fn check_levels(file: &File, path: &str, what: &str) -> Result<(), Error> {
    if file.levels.is_empty() {
        return Err(invalid(path, what, "it has no mip levels"));
    }
    if file.levels.len() > 32 {
        return Err(invalid(path, what, "it has more than 32 mip levels"));
    }
    for (i, level) in file.levels.iter().enumerate() {
        let (w, h) = ((file.width >> i).max(1), (file.height >> i).max(1));
        let expected = level_len(&file.pixels, w, h);
        if level.len() as u64 != expected {
            return Err(invalid(
                path,
                what,
                format_args!("mip level {i} ({w} x {h}) has {} bytes, not {expected}", level.len()),
            ));
        }
    }
    Ok(())
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Reads a DDS file of BCn blocks; `None` if it holds plain pixels (for
/// the image decoder, feature `image-dds`).
fn read_dds(bytes: &[u8], path: &str, what: &str) -> Option<Result<File, Error>> {
    const HEADER: usize = 128;
    const DX10: usize = 20;
    const MIPMAP_COUNT: u32 = 0x2_0000;
    const FOURCC: u32 = 0x4;
    const CUBEMAP: u32 = 0x200;
    const VOLUME: u32 = 0x20_0000;
    let short = || Some(Err(invalid(path, what, "the DDS header is cut short")));
    let (Some(height), Some(width), Some(flags), Some(mips), Some(pf_flags), Some(caps2)) = (
        read_u32(bytes, 12),
        read_u32(bytes, 16),
        read_u32(bytes, 8),
        read_u32(bytes, 28),
        read_u32(bytes, 80),
        read_u32(bytes, 112),
    ) else {
        return short();
    };
    if pf_flags & FOURCC == 0 {
        return None;
    }
    let fourcc = bytes.get(84..88)?;
    let (format, srgb, data) = match fourcc {
        b"DXT1" => (BlockFormat::Bc1, true, HEADER),
        b"DXT2" | b"DXT3" => (BlockFormat::Bc2, true, HEADER),
        b"DXT4" | b"DXT5" => (BlockFormat::Bc3, true, HEADER),
        b"ATI1" | b"BC4U" => (BlockFormat::Bc4, false, HEADER),
        b"ATI2" | b"BC5U" => (BlockFormat::Bc5, false, HEADER),
        b"DX10" => {
            let (Some(dxgi), Some(array)) = (read_u32(bytes, HEADER), read_u32(bytes, HEADER + 12)) else {
                return short();
            };
            if array > 1 {
                return Some(Err(unsupported(
                    path,
                    format!(
                        "{what} is a DDS array of {array} layers; load each layer from its own file with load_texture_array"
                    ),
                )));
            }
            let (format, srgb) = match dxgi {
                71 => (BlockFormat::Bc1, false),
                72 => (BlockFormat::Bc1, true),
                74 => (BlockFormat::Bc2, false),
                75 => (BlockFormat::Bc2, true),
                77 => (BlockFormat::Bc3, false),
                78 => (BlockFormat::Bc3, true),
                80 => (BlockFormat::Bc4, false),
                83 => (BlockFormat::Bc5, false),
                98 => (BlockFormat::Bc7, false),
                99 => (BlockFormat::Bc7, true),
                // Plain pixels (or a format ponko doesn't read): the image decoder's.
                _ => return None,
            };
            (format, srgb, HEADER + DX10)
        }
        _ => return None,
    };
    if caps2 & (CUBEMAP | VOLUME) != 0 {
        return Some(Err(unsupported(
            path,
            format!("{what} is a DDS cube map or volume; ponko loads 2D DDS textures only"),
        )));
    }
    if width == 0 || height == 0 {
        return Some(Err(invalid(path, what, "its size is zero")));
    }
    if let Err(e) = crate::assets::check_image_size(width.into(), height.into(), path, what) {
        return Some(Err(e));
    }
    let count = if flags & MIPMAP_COUNT != 0 { mips.max(1) } else { 1 }.min(32);
    let mut levels = Vec::with_capacity(count as usize);
    let mut at = data;
    for i in 0..count {
        let (w, h) = ((width >> i).max(1), (height >> i).max(1));
        let len = format.level_bytes(w, h) as usize;
        let Some(level) = at.checked_add(len).and_then(|end| bytes.get(at..end)) else {
            return Some(Err(invalid(
                path,
                what,
                format_args!("mip level {i} ({w} x {h}) is cut short"),
            )));
        };
        levels.push(level.to_vec());
        at += len;
    }
    Some(Ok(File {
        width,
        height,
        pixels: Pixels::Blocks(format),
        srgb,
        levels,
    }))
}

/// Chooses what to upload for a GPU that samples `gpu`: the file's blocks
/// or RGBA8 as they are (`Ok`), or the work that makes it: UASTC
/// transcoded, or blocks decoded to RGBA8 on the CPU (`Err`).
fn choose_for_gpu(file: File, path: &str, what: &str, gpu: GpuFormats) -> Result<Result<Decoded, GpuWork>, Error> {
    match file.pixels {
        Pixels::Rgba8 => {
            let mut levels = file.levels;
            Ok(Ok(Decoded::Rgba(Image {
                width: file.width,
                height: file.height,
                rgba: levels.swap_remove(0),
            })))
        }
        Pixels::Blocks(format) => {
            if format.sampled_by(gpu) && block_aligned(file.width, file.height) {
                Ok(Ok(Decoded::Blocks(CompressedImage {
                    width: file.width,
                    height: file.height,
                    format,
                    srgb: file.srgb && format.has_srgb(),
                    normal: if format.two_channel() {
                        NormalChannels::Rg
                    } else {
                        NormalChannels::Rgb
                    },
                    levels: file.levels,
                })))
            } else {
                let rgba = Vec::with_capacity(file.width as usize * file.height as usize * 4);
                Ok(Err(GpuWork::Cpu {
                    file,
                    format,
                    rgba,
                    row: 0,
                }))
            }
        }
        Pixels::Uastc(channels) => transcode_uastc(file, channels, path, what, gpu),
    }
}

/// Decodes one level of `format` blocks to RGBA8 on the CPU. A two-channel
/// format gets Z rebuilt into blue, so it reads as an ordinary normal map.
pub(crate) fn decode_blocks(format: BlockFormat, width: u32, height: u32, blocks: &[u8]) -> Result<Image, String> {
    use texture2ddecoder as t;
    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![0u32; w * h];
    let decoded = match format {
        BlockFormat::Bc1 => t::decode_bc1(blocks, w, h, &mut pixels),
        BlockFormat::Bc2 => t::decode_bc2(blocks, w, h, &mut pixels),
        BlockFormat::Bc3 => t::decode_bc3(blocks, w, h, &mut pixels),
        BlockFormat::Bc4 => t::decode_bc4(blocks, w, h, &mut pixels),
        BlockFormat::Bc5 => t::decode_bc5(blocks, w, h, &mut pixels),
        BlockFormat::Bc7 => t::decode_bc7(blocks, w, h, &mut pixels),
        BlockFormat::Etc2Rgb8 => t::decode_etc2_rgb(blocks, w, h, &mut pixels),
        BlockFormat::Etc2Rgba8 => t::decode_etc2_rgba8(blocks, w, h, &mut pixels),
        BlockFormat::EacR11 => t::decode_eacr(blocks, w, h, &mut pixels),
        BlockFormat::EacRg11 => t::decode_eacrg(blocks, w, h, &mut pixels),
        BlockFormat::Astc4x4 => t::decode_astc_4_4(blocks, w, h, &mut pixels),
    };
    decoded.map_err(|e| format!("its {} blocks don't decode: {e}", format.name()))?;
    // The decoder writes each pixel as little-endian B, G, R, A.
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in pixels {
        let [b, g, r, a] = p.to_le_bytes();
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    if format.two_channel() {
        rebuild_normal_z(&mut rgba);
    }
    Ok(Image { width, height, rgba })
}

/// Fills blue with a two-channel normal map's Z (from X in red and Y in
/// green), and makes alpha opaque.
fn rebuild_normal_z(rgba: &mut [u8]) {
    for p in rgba.as_chunks_mut::<4>().0 {
        let x = f32::from(p[0]) / 127.5 - 1.0;
        let y = f32::from(p[1]) / 127.5 - 1.0;
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        p[2] = ((z * 0.5 + 0.5) * 255.0).round() as u8;
        p[3] = 255;
    }
}

/// UASTC being transcoded to what the GPU samples, a band of block rows
/// at a time: every level to `to` (stored as `format`), or, with no block
/// format to use, level 0 through `to` and decoded to RGBA8 on the CPU.
#[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
struct Transcode {
    file: File,
    has_alpha: bool,
    to: basis_universal::TranscoderBlockFormat,
    format: BlockFormat,
    normal: NormalChannels,
    /// RGBA8 of level 0, not blocks.
    cpu: bool,
    /// Levels done, the level being made, and its next block row.
    levels: Vec<Vec<u8>>,
    current: Vec<u8>,
    row: u32,
}

#[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
impl Transcode {
    fn step(&mut self, slice: usize, path: &str, what: &str) -> Result<Option<Decoded>, Error> {
        use basis_universal::{DecodeFlags, SliceParametersUastc};
        let i = self.levels.len();
        let (w, h) = ((self.file.width >> i).max(1), (self.file.height >> i).max(1));
        let out_block = if self.cpu {
            4 * 4 * 4
        } else {
            self.format.block_bytes() as usize
        };
        let rows = band_rows(w, out_block, slice);
        let (y0, y1) = (
            self.row * BLOCK,
            self.row.saturating_add(rows).saturating_mul(BLOCK).min(h),
        );
        // UASTC blocks are 16 bytes each, as BC7's.
        let start = BlockFormat::Bc7.level_bytes(w, y0) as usize;
        let end = BlockFormat::Bc7.level_bytes(w, y1) as usize;
        // Made for each band: it holds a C++ object, which may not move
        // between threads as a stream's decode may; making one is cheap.
        let band = basis_universal::LowLevelUastcTranscoder::new()
            .transcode_slice(
                &self.file.levels[i][start..end],
                SliceParametersUastc {
                    num_blocks_x: w.div_ceil(BLOCK),
                    num_blocks_y: (y1 - y0).div_ceil(BLOCK),
                    has_alpha: self.has_alpha,
                    original_width: w,
                    original_height: y1 - y0,
                },
                DecodeFlags::HIGH_QUALITY,
                self.to,
            )
            .map_err(|e| invalid(path, what, format_args!("its UASTC blocks don't transcode: {e:?}")))?;
        if self.cpu {
            let pixels = decode_blocks(self.format, w, y1 - y0, &band).map_err(|e| invalid(path, what, e))?;
            self.current.extend_from_slice(&pixels.rgba);
        } else {
            self.current.extend_from_slice(&band);
        }
        self.row = y1.div_ceil(BLOCK);
        if y1 < h {
            return Ok(None);
        }
        self.row = 0;
        let level = std::mem::take(&mut self.current);
        if self.cpu {
            return Ok(Some(Decoded::Rgba(Image {
                width: w,
                height: h,
                rgba: level,
            })));
        }
        self.levels.push(level);
        if self.levels.len() < self.file.levels.len() {
            return Ok(None);
        }
        Ok(Some(Decoded::Blocks(CompressedImage {
            width: self.file.width,
            height: self.file.height,
            format: self.format,
            srgb: self.file.srgb && self.format.has_srgb(),
            levels: std::mem::take(&mut self.levels),
            normal: self.normal,
        })))
    }
}

#[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
fn transcode_uastc(
    file: File,
    channels: UastcChannels,
    _path: &str,
    _what: &str,
    gpu: GpuFormats,
) -> Result<Result<Decoded, GpuWork>, Error> {
    use basis_universal::TranscoderBlockFormat as T;
    // Two-channel files: X and Y (red and alpha) into BC5 or EAC RG11's two
    // channels, else into an RGBA format that keeps them in red and alpha.
    let rrrg = channels == UastcChannels::Rrrg;
    let two = |rg| if rrrg { rg } else { NormalChannels::Rgb };
    let target = if gpu.bc {
        Some(if rrrg {
            (T::BC5, BlockFormat::Bc5, NormalChannels::Rg)
        } else {
            (T::BC7, BlockFormat::Bc7, two(NormalChannels::Ra))
        })
    } else if gpu.astc {
        Some((T::ASTC_4x4, BlockFormat::Astc4x4, two(NormalChannels::Ra)))
    } else if gpu.etc2 {
        Some(if rrrg {
            (T::ETC2_EAC_RG11, BlockFormat::EacRg11, NormalChannels::Rg)
        } else {
            (T::ETC2_RGBA, BlockFormat::Etc2Rgba8, NormalChannels::Rgb)
        })
    } else {
        None
    };
    let target = target.filter(|_| block_aligned(file.width, file.height));
    let has_alpha = matches!(channels, UastcChannels::Rgba | UastcChannels::Rrrg);
    // No block format to use: RGBA8 of the full size, through BC7 (BC5 for
    // two channels) decoded on the CPU. Not the transcoder's own RGBA32
    // output: the `basis-universal` crate sizes its buffer for that with a
    // row pitch a quarter of the width, which the C++ transcoder then
    // writes past.
    let (to, format, normal, cpu) = match target {
        Some((to, format, normal)) => (to, format, normal, false),
        None if rrrg => (T::BC5, BlockFormat::Bc5, NormalChannels::Rgb, true),
        None => (T::BC7, BlockFormat::Bc7, NormalChannels::Rgb, true),
    };
    Ok(Err(GpuWork::Uastc(Box::new(Transcode {
        file,
        has_alpha,
        to,
        format,
        normal,
        cpu,
        levels: Vec::new(),
        current: Vec::new(),
        row: 0,
    }))))
}

#[cfg(not(all(feature = "basis-universal", not(target_arch = "wasm32"))))]
fn transcode_uastc(
    _file: File,
    _channels: UastcChannels,
    path: &str,
    what: &str,
    _gpu: GpuFormats,
) -> Result<Result<Decoded, GpuWork>, Error> {
    let why = if cfg!(target_arch = "wasm32") {
        "web builds have no Basis Universal transcoder"
    } else {
        "this build of ponko has no Basis Universal transcoder: turn on its `basis-universal` cargo feature"
    };
    Err(unsupported(
        path,
        format!("{what} is Basis Universal UASTC, and {why}. BC7 files from `ponko textures` load everywhere"),
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal KTX2 file of `format` (Vulkan value, 0 for UASTC) and
    /// `levels`, for tests.
    pub(crate) fn ktx2_file(
        format: Option<ktx2::Format>,
        width: u32,
        height: u32,
        levels: &[Vec<u8>],
        zstd: bool,
    ) -> Vec<u8> {
        ktx2_file_with(format, width, height, levels, zstd, &[])
    }

    /// [`ktx2_file`] with key/value entries (sorted by key, as KTX2 wants).
    pub(crate) fn ktx2_file_with(
        format: Option<ktx2::Format>,
        width: u32,
        height: u32,
        levels: &[Vec<u8>],
        zstd: bool,
        key_values: &[(&str, &str)],
    ) -> Vec<u8> {
        let scheme = zstd.then_some(ktx2::SupercompressionScheme::Zstandard);
        ktx2_file_packed(format, width, height, levels, scheme, key_values)
    }

    /// [`ktx2_file_with`] with its levels packed with `scheme`.
    fn ktx2_file_packed(
        format: Option<ktx2::Format>,
        width: u32,
        height: u32,
        levels: &[Vec<u8>],
        scheme: Option<ktx2::SupercompressionScheme>,
        key_values: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut kvd = Vec::new();
        for (key, value) in key_values {
            let len = key.len() + 1 + value.len() + 1;
            kvd.extend_from_slice(&(len as u32).to_le_bytes());
            kvd.extend_from_slice(key.as_bytes());
            kvd.push(0);
            kvd.extend_from_slice(value.as_bytes());
            kvd.push(0);
            kvd.resize(kvd.len().next_multiple_of(4), 0);
        }
        let dfd = match format {
            Some(f) => ktx2::dfd::Block::Basic(ktx2::dfd::Basic::from_format(f).expect("a DFD").0),
            None => ktx2::dfd::Block::Basic(uastc_dfd()),
        }
        .to_vec();
        let dfd_len = 4 + dfd.len();
        let index_len = 24 * levels.len();
        let dfd_at = 80 + index_len;
        let packed: Vec<Vec<u8>> = levels
            .iter()
            .map(|l| match scheme {
                Some(ktx2::SupercompressionScheme::Zstandard) => {
                    ruzstd::encoding::compress_to_vec(l.as_slice(), ruzstd::encoding::CompressionLevel::Fastest)
                }
                Some(ktx2::SupercompressionScheme::ZLIB) => {
                    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
                    std::io::Write::write_all(&mut z, l).unwrap();
                    z.finish().unwrap()
                }
                _ => l.clone(),
            })
            .collect();
        let kvd_at = dfd_at + dfd_len;
        let mut data_at = (kvd_at + kvd.len()).next_multiple_of(16);
        let mut offsets = Vec::new();
        for l in &packed {
            offsets.push(data_at);
            data_at = (data_at + l.len()).next_multiple_of(16);
        }
        let header = ktx2::Header {
            format,
            type_size: 1,
            pixel_width: width,
            pixel_height: height,
            pixel_depth: 0,
            layer_count: 0,
            face_count: 1,
            level_count: levels.len() as u32,
            supercompression_scheme: scheme,
            index: ktx2::Index {
                dfd_byte_offset: dfd_at as u32,
                dfd_byte_length: dfd_len as u32,
                kvd_byte_offset: if kvd.is_empty() { 0 } else { kvd_at as u32 },
                kvd_byte_length: kvd.len() as u32,
                sgd_byte_offset: 0,
                sgd_byte_length: 0,
            },
        };
        let mut out = header.as_bytes().to_vec();
        for ((l, p), at) in levels.iter().zip(&packed).zip(&offsets) {
            out.extend_from_slice(
                &ktx2::LevelIndex {
                    byte_offset: *at as u64,
                    byte_length: p.len() as u64,
                    uncompressed_byte_length: l.len() as u64,
                }
                .as_bytes(),
            );
        }
        out.extend_from_slice(&(dfd_len as u32).to_le_bytes());
        out.extend_from_slice(&dfd);
        out.extend_from_slice(&kvd);
        for (p, at) in packed.iter().zip(&offsets) {
            out.resize(*at, 0);
            out.extend_from_slice(p);
        }
        out
    }

    fn uastc_dfd() -> ktx2::dfd::Basic {
        let mut dfd = ktx2::dfd::Basic::from_format(ktx2::Format::BC7_SRGB_BLOCK)
            .expect("a DFD")
            .0;
        dfd.color_model = Some(ktx2::ColorModel::UASTC);
        dfd
    }

    /// One BC1 block of a single color (565): both endpoints `color`,
    /// every index 0.
    fn bc1_block(color: u16) -> [u8; 8] {
        let [a, b] = color.to_le_bytes();
        [a, b, a, b, 0, 0, 0, 0]
    }

    fn solid_bc1(width: u32, height: u32, color: u16) -> Vec<u8> {
        let blocks = (width.div_ceil(4) * height.div_ceil(4)) as usize;
        (0..blocks).flat_map(|_| bc1_block(color)).collect()
    }

    const RED_565: u16 = 0xF800;

    #[test]
    fn ktx2_blocks_upload_as_they_are_when_the_gpu_samples_them() {
        let levels = vec![
            solid_bc1(8, 8, RED_565),
            solid_bc1(4, 4, RED_565),
            solid_bc1(2, 2, RED_565),
        ];
        let file = ktx2_file(Some(ktx2::Format::BC1_RGBA_SRGB_BLOCK), 8, 8, &levels, false);
        let Some(Ok(Decoded::Blocks(image))) = decode(&file, "a.ktx2", "\"a.ktx2\"", GpuFormats::ALL) else {
            panic!("decoded as blocks");
        };
        assert_eq!((image.width, image.height, image.format), (8, 8, BlockFormat::Bc1));
        assert!(image.srgb);
        assert_eq!(image.levels, levels);
        assert_eq!(image.bytes(), 32 + 8 + 8);
    }

    #[test]
    fn zstd_supercompressed_levels_unpack() {
        let levels = vec![solid_bc1(16, 16, RED_565), solid_bc1(8, 8, RED_565)];
        let file = ktx2_file(Some(ktx2::Format::BC1_RGBA_UNORM_BLOCK), 16, 16, &levels, true);
        let Some(Ok(Decoded::Blocks(image))) = decode(&file, "a.ktx2", "a", GpuFormats::ALL) else {
            panic!("decoded as blocks");
        };
        assert!(!image.srgb);
        assert_eq!(image.levels, levels);
    }

    #[test]
    fn a_supercompressed_level_unpacking_past_its_size_is_refused() {
        // 8 x 8 BC1 is 32 bytes; the level unpacks to 4 MiB of zeros.
        let bomb = vec![vec![0u8; 4 << 20]];
        for scheme in [
            ktx2::SupercompressionScheme::Zstandard,
            ktx2::SupercompressionScheme::ZLIB,
        ] {
            let format = Some(ktx2::Format::BC1_RGBA_UNORM_BLOCK);
            let mut file = ktx2_file_packed(format, 8, 8, &bomb, Some(scheme), &[]);
            // As its header says: then the header is checked against the size.
            let Some(Err(e)) = decode(&file, "a.ktx2", "a", GpuFormats::ALL) else {
                panic!("{scheme:?}: a level whose header length isn't its size loaded");
            };
            assert!(
                e.message().contains("unpacks to 4194304 bytes, not 32"),
                "{}",
                e.message()
            );
            // Its header lies (32 bytes): the unpacking stops soon after.
            file[96..104].copy_from_slice(&32u64.to_le_bytes());
            let Some(Err(e)) = decode(&file, "a.ktx2", "a", GpuFormats::ALL) else {
                panic!("{scheme:?}: a level that unpacks past its size loaded");
            };
            assert!(
                e.message().contains("unpacks to more than its 32 bytes"),
                "{}",
                e.message()
            );
        }
    }

    #[test]
    fn a_texture_larger_than_the_limit_is_refused() {
        let level = vec![solid_bc1(4, 4, RED_565)];
        let ktx = ktx2_file(Some(ktx2::Format::BC1_RGBA_UNORM_BLOCK), 1 << 20, 4, &level, true);
        let Some(Err(e)) = decode(&ktx, "a.ktx2", "a", GpuFormats::ALL) else {
            panic!("a KTX2 file 2^20 pixels wide loaded");
        };
        assert_eq!(e.kind(), ErrorKind::Limit, "{}", e.message());
        let dds = dds(b"DXT1", 4, 1 << 20, &level);
        let Some(Err(e)) = decode(&dds, "a.dds", "a", GpuFormats::ALL) else {
            panic!("a DDS file 2^20 pixels tall loaded");
        };
        assert_eq!(e.kind(), ErrorKind::Limit, "{}", e.message());
    }

    #[test]
    fn lowered_image_limits_refuse_ktx2_and_dds_naming_them() {
        use crate::assets::{ImageLimits, with_image_limits};
        let level = vec![solid_bc1(4, 4, RED_565)];
        let ktx = ktx2_file(Some(ktx2::Format::BC1_RGBA_UNORM_BLOCK), 12, 4, &level, true);
        let dds = dds(b"DXT1", 4, 12, &level);
        let tight = ImageLimits::new(8, 1 << 20);
        for (name, file) in [("a.ktx2", &ktx), ("a.dds", &dds)] {
            let Some(Err(e)) = with_image_limits(tight, || decode(file, name, "a", GpuFormats::ALL)) else {
                panic!("{name}: a picture 12 pixels on a side loaded under 8");
            };
            assert_eq!(e.kind(), ErrorKind::Limit, "{name}: {}", e.message());
            assert!(
                e.message().contains("8 pixels a side, 1 MiB"),
                "{name}: {}",
                e.message()
            );
            // The same file is past the limit only because of the limit.
            let Some(Err(e)) = decode(file, name, "a", GpuFormats::ALL) else {
                continue; // it loads: not a hostile file at all
            };
            assert_ne!(e.kind(), ErrorKind::Limit, "{name}: {}", e.message());
        }
        // Bytes: 12 x 4 RGBA8 is 192.
        let few = ImageLimits::new(64, 191);
        let Some(Err(e)) = with_image_limits(few, || decode(&ktx, "a.ktx2", "a", GpuFormats::ALL)) else {
            panic!("192 bytes of pixels loaded under 191");
        };
        assert_eq!(e.kind(), ErrorKind::Limit, "{}", e.message());
        assert!(e.message().contains("191 bytes"), "{}", e.message());
    }

    #[test]
    fn a_texel_block_dimension_the_ktx2_crate_would_overflow_is_refused() {
        // Found by the `cooked_texture` fuzz target: the `ktx2` crate adds
        // one to each texel block dimension byte in a `u8` and panics on 255.
        let mut file = ktx2_file(
            Some(ktx2::Format::BC1_RGBA_UNORM_BLOCK),
            4,
            4,
            &[solid_bc1(4, 4, RED_565)],
            false,
        );
        let dfd_at = u32::from_le_bytes(file[48..52].try_into().unwrap()) as usize;
        file[dfd_at + 4 + 8 + 4] = 255;
        let Some(Err(e)) = decode(&file, "a.ktx2", "a", GpuFormats::ALL) else {
            panic!("a texel block 256 texels wide loaded");
        };
        assert_eq!(e.kind(), ErrorKind::Decode, "{}", e.message());
    }

    #[test]
    fn a_gpu_without_the_format_gets_rgba_decoded_on_the_cpu() {
        let file = ktx2_file(
            Some(ktx2::Format::BC1_RGBA_SRGB_BLOCK),
            8,
            4,
            &[solid_bc1(8, 4, RED_565)],
            false,
        );
        let Some(Ok(Decoded::Rgba(image))) = decode(&file, "a.ktx2", "a", GpuFormats::default()) else {
            panic!("decoded to RGBA");
        };
        assert_eq!((image.width, image.height), (8, 4));
        assert!(
            image.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]),
            "{:?}",
            &image.rgba[..8]
        );
    }

    #[test]
    fn sizes_that_are_not_whole_blocks_decode_on_the_cpu() {
        // A GPU texture of a block format must be whole blocks.
        let file = ktx2_file(
            Some(ktx2::Format::BC1_RGBA_SRGB_BLOCK),
            6,
            5,
            &[solid_bc1(6, 5, RED_565)],
            false,
        );
        let Some(Ok(Decoded::Rgba(image))) = decode(&file, "a.ktx2", "a", GpuFormats::ALL) else {
            panic!("decoded to RGBA");
        };
        assert_eq!((image.width, image.height, image.rgba.len()), (6, 5, 6 * 5 * 4));
    }

    #[test]
    fn two_channel_normal_maps_are_marked_and_their_cpu_decode_gets_z() {
        // BC5: red 255 (X = 1), green 128 (Y = 0) in every pixel.
        let channel = |v: u8| [v, v, 0, 0, 0, 0, 0, 0];
        let block: Vec<u8> = [channel(128), channel(128)].concat();
        let file = ktx2_file(Some(ktx2::Format::BC5_UNORM_BLOCK), 4, 4, &[block], false);
        let Some(Ok(Decoded::Blocks(image))) = decode(&file, "n.ktx2", "n", GpuFormats::ALL) else {
            panic!("blocks");
        };
        assert_eq!(image.normal, NormalChannels::Rg);
        assert!(!image.srgb, "BC5 has no sRGB variant");
        let Some(Ok(Decoded::Rgba(flat))) = decode(&file, "n.ktx2", "n", GpuFormats::default()) else {
            panic!("rgba");
        };
        // X = Y = 0: Z is 1, so blue is full.
        assert!(
            flat.rgba.chunks(4).all(|p| p[2] >= 254 && p[3] == 255),
            "{:?}",
            &flat.rgba[..4]
        );
    }

    #[test]
    fn short_levels_are_decode_errors_not_gpu_crashes() {
        let file = ktx2_file(Some(ktx2::Format::BC7_SRGB_BLOCK), 8, 8, &[vec![0; 16 * 3]], false);
        let err = decode(&file, "a.ktx2", "\"a.ktx2\"", GpuFormats::ALL)
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(err.message().contains("mip level 0"), "{}", err.message());
    }

    #[test]
    fn cube_maps_and_unknown_formats_are_unsupported() {
        let mut file = ktx2_file(Some(ktx2::Format::R16_SFLOAT), 4, 4, &[vec![0; 32]], false);
        let err = decode(&file, "a.ktx2", "a", GpuFormats::ALL).unwrap().err().unwrap();
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        file = ktx2_file(Some(ktx2::Format::BC7_SRGB_BLOCK), 4, 4, &[vec![0; 16]], false);
        file[36..40].copy_from_slice(&6u32.to_le_bytes());
        let err = decode(&file, "a.ktx2", "a", GpuFormats::ALL).unwrap().err().unwrap();
        assert!(err.message().contains("cube map"), "{}", err.message());
    }

    #[test]
    fn uastc_without_a_transcoder_names_the_way_out() {
        let file = ktx2_file(None, 4, 4, &[vec![0; 16]], false);
        let result = decode(&file, "a.ktx2", "\"a.ktx2\"", GpuFormats::ALL).unwrap();
        if cfg!(all(feature = "basis-universal", not(target_arch = "wasm32"))) {
            // All-zero blocks are a valid (if dull) UASTC block mode.
            let _ = result;
        } else {
            let err = result.err().unwrap();
            assert_eq!(err.kind(), ErrorKind::Unsupported);
            assert!(err.message().contains("basis-universal"), "{}", err.message());
        }
    }

    fn dds(fourcc: &[u8; 4], width: u32, height: u32, levels: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0u8; 128];
        out[..4].copy_from_slice(b"DDS ");
        out[4..8].copy_from_slice(&124u32.to_le_bytes());
        out[8..12].copy_from_slice(&(0x1007u32 | 0x2_0000).to_le_bytes());
        out[12..16].copy_from_slice(&height.to_le_bytes());
        out[16..20].copy_from_slice(&width.to_le_bytes());
        out[28..32].copy_from_slice(&(levels.len() as u32).to_le_bytes());
        out[76..80].copy_from_slice(&32u32.to_le_bytes());
        out[80..84].copy_from_slice(&4u32.to_le_bytes());
        out[84..88].copy_from_slice(fourcc);
        for l in levels {
            out.extend_from_slice(l);
        }
        out
    }

    #[test]
    fn dds_bc_files_load_with_their_mipmaps() {
        let levels = vec![
            solid_bc1(8, 8, RED_565),
            solid_bc1(4, 4, RED_565),
            solid_bc1(2, 2, RED_565),
            solid_bc1(1, 1, RED_565),
        ];
        let file = dds(b"DXT1", 8, 8, &levels);
        let Some(Ok(Decoded::Blocks(image))) = decode(&file, "a.dds", "a", GpuFormats::ALL) else {
            panic!("blocks");
        };
        assert_eq!(image.format, BlockFormat::Bc1);
        assert_eq!(image.levels.len(), 4);
        let cut = &file[..file.len() - 4];
        let err = decode(cut, "a.dds", "a", GpuFormats::ALL).unwrap().err().unwrap();
        assert_eq!(err.kind(), ErrorKind::Decode);
    }

    #[test]
    fn dds_files_of_plain_pixels_are_left_to_the_image_decoder() {
        let mut file = dds(b"DXT1", 4, 4, &[]);
        file[80..84].copy_from_slice(&0x41u32.to_le_bytes()); // RGB | alpha, no FourCC
        assert!(decode(&file, "a.dds", "a", GpuFormats::ALL).is_none());
        assert!(decode(b"\x89PNG", "a.png", "a", GpuFormats::ALL).is_none());
    }

    /// The fixture card (`tests/fixtures/textures`), and its pixels.
    fn card_png() -> Vec<u8> {
        image::load_from_memory(include_bytes!("../tests/fixtures/textures/card.png"))
            .unwrap()
            .into_rgba8()
            .into_raw()
    }

    fn mean_error(a: &[u8], b: &[u8]) -> f32 {
        let sum: f32 = a
            .chunks(4)
            .zip(b.chunks(4))
            .map(|(p, q)| (0..3).map(|c| (f32::from(p[c]) - f32::from(q[c])).abs()).sum::<f32>())
            .sum();
        sum / (a.len() / 4 * 3) as f32
    }

    #[test]
    fn bc7_from_ponko_textures_decodes_close_to_its_source() {
        let file = include_bytes!("../tests/fixtures/textures/card.ktx2");
        let Some(Ok(Decoded::Rgba(image))) = decode(file, "card.ktx2", "card", GpuFormats::default()) else {
            panic!("rgba");
        };
        let source = card_png();
        let error = mean_error(&image.rgba, &source);
        assert!(error < 2.0, "mean error {error}");
        // Negative control: another image is far from it.
        let inverted: Vec<u8> = source.iter().map(|v| 255 - v).collect();
        assert!(mean_error(&image.rgba, &inverted) > 40.0);
    }

    #[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
    #[test]
    fn uastc_transcodes_to_what_each_gpu_samples() {
        let file = include_bytes!("../tests/fixtures/textures/card_uastc.ktx2");
        let source = card_png();
        let cases = [
            (
                GpuFormats {
                    bc: true,
                    etc2: true,
                    astc: true,
                },
                Some(BlockFormat::Bc7),
            ),
            (
                GpuFormats {
                    astc: true,
                    etc2: true,
                    ..Default::default()
                },
                Some(BlockFormat::Astc4x4),
            ),
            (
                GpuFormats {
                    etc2: true,
                    ..Default::default()
                },
                Some(BlockFormat::Etc2Rgba8),
            ),
            (GpuFormats::default(), None),
        ];
        for (gpu, expected) in cases {
            let decoded = decode(file, "card_uastc.ktx2", "card", gpu).unwrap().unwrap();
            let rgba = match (decoded, expected) {
                (Decoded::Blocks(image), Some(format)) => {
                    assert_eq!(image.format, format);
                    assert!(image.srgb);
                    assert_eq!(image.levels.len(), 7);
                    decode_blocks(image.format, 64, 64, &image.levels[0]).unwrap().rgba
                }
                (Decoded::Rgba(image), None) => image.rgba,
                _ => panic!("{gpu:?}: not {expected:?}"),
            };
            let error = mean_error(&rgba, &source);
            assert!(error < 4.0, "{gpu:?}: mean error {error}");
        }
    }

    #[cfg(all(feature = "basis-universal", not(target_arch = "wasm32")))]
    #[test]
    fn two_channel_uastc_goes_to_bc5_or_keeps_y_in_alpha() {
        // A normal map from `ponko textures` with "basis": true (RRRG).
        let file = include_bytes!("../tests/fixtures/textures/bumps_uastc.ktx2");
        let source = image::load_from_memory(include_bytes!("../tests/fixtures/textures/bumps.png"))
            .unwrap()
            .into_rgba8()
            .into_raw();
        let xy_error = |rgba: &[u8], y: usize| {
            let sum: f32 = rgba
                .chunks(4)
                .zip(source.chunks(4))
                .map(|(p, q)| (f32::from(p[0]) - f32::from(q[0])).abs() + (f32::from(p[y]) - f32::from(q[1])).abs())
                .sum();
            sum / (rgba.len() / 4 * 2) as f32
        };
        let bc = GpuFormats {
            bc: true,
            ..Default::default()
        };
        let Decoded::Blocks(bc5) = decode(file, "b", "b", bc).unwrap().unwrap() else {
            panic!("blocks")
        };
        assert_eq!(
            (bc5.format, bc5.normal, bc5.srgb),
            (BlockFormat::Bc5, NormalChannels::Rg, false)
        );
        let error = xy_error(&decode_blocks(bc5.format, 64, 64, &bc5.levels[0]).unwrap().rgba, 1);
        assert!(error < 3.0, "BC5: {error}");
        let astc = GpuFormats {
            astc: true,
            ..Default::default()
        };
        let Decoded::Blocks(rgba) = decode(file, "b", "b", astc).unwrap().unwrap() else {
            panic!("blocks")
        };
        assert_eq!((rgba.format, rgba.normal), (BlockFormat::Astc4x4, NormalChannels::Ra));
        let error = xy_error(&decode_blocks(rgba.format, 64, 64, &rgba.levels[0]).unwrap().rgba, 3);
        assert!(error < 3.0, "ASTC, Y in alpha: {error}");
        // No block format: RGBA8 with X, Y and the rebuilt Z, like the source.
        let Decoded::Rgba(flat) = decode(file, "b", "b", GpuFormats::default()).unwrap().unwrap() else {
            panic!("rgba")
        };
        assert!(xy_error(&flat.rgba, 1) < 3.0);
        let z: f32 = flat
            .rgba
            .chunks(4)
            .zip(source.chunks(4))
            .map(|(p, q)| (f32::from(p[2]) - f32::from(q[2])).abs())
            .sum();
        assert!(z / 4096.0 < 4.0, "Z: {}", z / 4096.0);
    }

    #[test]
    fn streaming_levels_start_further_down_the_chain() {
        let levels = vec![
            solid_bc1(16, 8, RED_565),
            solid_bc1(8, 4, RED_565),
            solid_bc1(4, 2, RED_565),
        ];
        let image = CompressedImage {
            width: 16,
            height: 8,
            format: BlockFormat::Bc1,
            srgb: true,
            levels: levels.clone(),
            normal: NormalChannels::Rgb,
        };
        let half = image.starting_at_level(1);
        assert_eq!((half.width, half.height), (8, 4));
        assert_eq!(half.levels, levels[1..]);
    }

    /// `blocks` blocks of `bytes` each of noise: any bytes are a valid
    /// BC1, BC5 or BC7 block (decoders read every bit pattern).
    pub(crate) fn noise_blocks(blocks: usize, bytes: usize, seed: u32) -> Vec<u8> {
        let mut s = seed.wrapping_mul(0x9E37_79B9) | 1;
        (0..blocks * bytes)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                s as u8
            })
            .collect()
    }

    /// Noise levels of `format` from `width` x `height` down to 1 x 1.
    pub(crate) fn noise_levels(format: BlockFormat, width: u32, height: u32) -> Vec<Vec<u8>> {
        let mut levels = Vec::new();
        let (mut w, mut h) = (width, height);
        loop {
            let blocks = (w.div_ceil(BLOCK) * h.div_ceil(BLOCK)) as usize;
            levels.push(noise_blocks(blocks, format.block_bytes() as usize, w ^ h << 8));
            if w == 1 && h == 1 {
                return levels;
            }
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
        }
    }

    /// Decodes `file` for `gpu` a slice of `bytes` at a time: what it
    /// makes, and the slices it took.
    fn sliced(file: &[u8], gpu: GpuFormats, bytes: usize) -> (Decoded, usize) {
        let mut slices = DecodeSlices::new(file, "a.ktx2", "a", gpu)
            .expect("a KTX2 or DDS file")
            .unwrap()
            .with_slice_bytes(bytes);
        for n in 1.. {
            if let Some(decoded) = slices.step(file).unwrap() {
                return (decoded, n);
            }
        }
        unreachable!()
    }

    fn same(a: &Decoded, b: &Decoded) -> bool {
        match (a, b) {
            (Decoded::Rgba(a), Decoded::Rgba(b)) => (a.width, a.height, &a.rgba) == (b.width, b.height, &b.rgba),
            (Decoded::Blocks(a), Decoded::Blocks(b)) => a == b,
            _ => false,
        }
    }

    #[test]
    fn sliced_ktx2_decodes_make_what_the_whole_decode_makes() {
        use ktx2::Format as F;
        use ktx2::SupercompressionScheme as S;
        let cases = [
            // Unpacked a piece at a time, uploaded as blocks.
            (
                F::BC7_SRGB_BLOCK,
                BlockFormat::Bc7,
                128,
                64,
                Some(S::Zstandard),
                GpuFormats::ALL,
            ),
            (
                F::BC7_SRGB_BLOCK,
                BlockFormat::Bc7,
                128,
                64,
                Some(S::ZLIB),
                GpuFormats::ALL,
            ),
            // Decoded to RGBA8 a band of block rows at a time.
            (
                F::BC1_RGBA_UNORM_BLOCK,
                BlockFormat::Bc1,
                72,
                40,
                None,
                GpuFormats::default(),
            ),
            (
                F::BC1_RGBA_UNORM_BLOCK,
                BlockFormat::Bc1,
                70,
                38,
                Some(S::Zstandard),
                GpuFormats::ALL,
            ),
            (
                F::BC5_UNORM_BLOCK,
                BlockFormat::Bc5,
                64,
                36,
                Some(S::ZLIB),
                GpuFormats::default(),
            ),
            (
                F::BC7_UNORM_BLOCK,
                BlockFormat::Bc7,
                52,
                64,
                None,
                GpuFormats::default(),
            ),
        ];
        for (format, block, w, h, scheme, gpu) in cases {
            let levels = noise_levels(block, w, h);
            let file = ktx2_file_packed(Some(format), w, h, &levels, scheme, &[]);
            let whole = decode(&file, "a.ktx2", "a", gpu).unwrap().unwrap();
            let (sliced, n) = sliced(&file, gpu, 1024);
            assert!(
                same(&sliced, &whole),
                "{format:?} {w} x {h} {scheme:?}: the slices made another texture"
            );
            // A slice per level at least, then the choice; a CPU decode's
            // bands come on top (Zstandard splits only between its blocks,
            // which these small levels fit in one of).
            let bands = if matches!(whole, Decoded::Rgba(_)) { 2 } else { 0 };
            assert!(n > levels.len() + bands, "{format:?} {w} x {h} {scheme:?}: {n} slices");
        }
    }

    #[test]
    fn a_sliced_cpu_decode_makes_a_band_of_block_rows_a_slice() {
        let levels = vec![noise_blocks(16 * 16, 8, 3)];
        let file = ktx2_file(Some(ktx2::Format::BC1_RGBA_UNORM_BLOCK), 64, 64, &levels, false);
        // One block row of RGBA8 is 16 blocks of 4 x 4 pixels of 4 bytes.
        let row = 16 * 4 * 4 * 4;
        for (bytes, bands) in [(row, 16), (3 * row, 6), (row / 2, 16), (usize::MAX, 1)] {
            let (decoded, n) = sliced(&file, GpuFormats::default(), bytes);
            assert!(matches!(decoded, Decoded::Rgba(_)));
            // The level unpacked, the choice, then the bands.
            assert_eq!(n, 1 + 1 + bands, "{bytes} bytes a slice");
        }
    }

    #[test]
    fn a_sliced_zstd_level_unpacks_a_bounded_piece_a_slice() {
        let levels = vec![noise_blocks(64 * 64, 16, 5)];
        let file = ktx2_file(Some(ktx2::Format::BC7_UNORM_BLOCK), 256, 256, &levels, true);
        let (decoded, n) = sliced(&file, GpuFormats::ALL, 16 << 10);
        let Decoded::Blocks(image) = decoded else {
            panic!("blocks")
        };
        assert_eq!(image.levels, levels);
        // 64 KiB of blocks, at least 16 KiB a slice (whole Zstandard
        // blocks, at most 128 KiB each): more than one slice, at most four
        // and the choice.
        assert!((2..=5).contains(&n), "{n} slices");
    }

    #[cfg(feature = "basis-universal")]
    #[test]
    fn sliced_uastc_transcodes_make_what_the_whole_transcode_makes() {
        let file = include_bytes!("../tests/fixtures/textures/card_uastc.ktx2");
        let two = include_bytes!("../tests/fixtures/textures/bumps_uastc.ktx2");
        let gpus = [
            GpuFormats::ALL,
            GpuFormats {
                astc: true,
                ..Default::default()
            },
            GpuFormats {
                etc2: true,
                ..Default::default()
            },
            GpuFormats::default(),
        ];
        for file in [&file[..], &two[..]] {
            for gpu in gpus {
                let whole = decode(file, "a.ktx2", "a", gpu).unwrap().unwrap();
                let (sliced, n) = sliced(file, gpu, 256);
                assert!(same(&sliced, &whole), "{gpu:?}: the slices made another texture");
                assert!(n > 4, "{gpu:?}: {n} slices");
            }
        }
    }
}
