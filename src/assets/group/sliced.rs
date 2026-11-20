//! Decodes done a slice at a time, for asset groups where there are no
//! worker threads (a browser build without threads, or a page that doesn't
//! allow them): the group step decodes a few rows of a PNG, or a few
//! packets of a sound, then checks its frame budget, so one large file is
//! spread over several frames instead of taking one whole frame.
//!
//! Each produces exactly what the one-piece decode does (`decode_image`,
//! `decode_sound`), so a texture or sound looks and sounds the same however
//! it was decoded.

use std::io::Cursor;
use std::sync::Arc;

use crate::assets::Image;

/// Output bytes one slice decodes, about: small enough that a slice costs a
/// fraction of a millisecond, large enough that the budget checks between
/// slices cost nothing.
const SLICE_BYTES: usize = 64 * 1024;

/// A file's bytes, shared with the caller (which decodes them in one piece
/// if they can't be split).
struct Bytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// A PNG decoded a few rows at a time, into RGBA8 as `decode_image` makes it.
pub(super) struct PngSlices {
    reader: png::Reader<Cursor<Bytes>>,
    width: u32,
    height: u32,
    /// Rows decoded so far.
    row: u32,
    rows_per_slice: u32,
    /// Channels per pixel of the decoder's rows: 1 to 4.
    channels: usize,
    /// Whether samples are 16-bit (big-endian in the rows).
    wide: bool,
    rgba: Vec<u8>,
}

impl PngSlices {
    /// Starts decoding `bytes`; `None` if they aren't a PNG this can split
    /// (not a PNG, an interlaced one, whose rows come in seven passes, or
    /// one whose header is broken): decode those in one piece, which also
    /// gives the usual message for a broken file.
    pub fn new(bytes: &Arc<Vec<u8>>) -> Option<Self> {
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return None;
        }
        // As the `image` crate sets it up: no allocation limit, palettes
        // and small samples expanded, 16-bit samples kept.
        let source = Cursor::new(Bytes(Arc::clone(bytes)));
        let mut decoder = png::Decoder::new_with_limits(source, png::Limits { bytes: usize::MAX });
        decoder.set_transformations(png::Transformations::EXPAND);
        let reader = decoder.read_info().ok()?;
        let info = reader.info();
        if info.interlaced {
            return None;
        }
        let (width, height) = (info.width, info.height);
        let (color, depth) = reader.output_color_type();
        let channels = match color {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            // EXPAND turns palettes into RGB(A); anything else, in one piece.
            png::ColorType::Indexed => return None,
        };
        let wide = match depth {
            png::BitDepth::Eight => false,
            png::BitDepth::Sixteen => true,
            _ => return None,
        };
        let row_bytes = (width as usize * 4).max(1);
        let rows_per_slice = u32::try_from((SLICE_BYTES / row_bytes).max(1)).unwrap_or(u32::MAX);
        Some(Self {
            reader,
            width,
            height,
            row: 0,
            rows_per_slice,
            channels,
            wide,
            rgba: Vec::with_capacity(width as usize * height as usize * 4),
        })
    }

    /// Decodes the next slice of rows; `Ok(true)` once every row is in.
    pub fn step(&mut self) -> Result<bool, String> {
        let end = self.row.saturating_add(self.rows_per_slice).min(self.height);
        while self.row < end {
            let row = self
                .reader
                .next_row()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("the image data ended after {} of {} rows", self.row, self.height))?;
            push_rgba(&mut self.rgba, row.data(), self.width, self.channels, self.wide);
            self.row += 1;
        }
        Ok(self.row == self.height)
    }

    /// The decoded image, once [`step`](Self::step) said it is done.
    pub fn finish(self) -> Image {
        Image {
            width: self.width,
            height: self.height,
            rgba: self.rgba,
        }
    }
}

/// Appends one decoded row as RGBA8, converting as `image`'s `into_rgba8`
/// does: grey to all three colours, opaque without alpha, 16-bit samples
/// rounded to 8 bits.
fn push_rgba(out: &mut Vec<u8>, row: &[u8], width: u32, channels: usize, wide: bool) {
    let width = width as usize;
    // The usual layouts, without a per-sample branch.
    match (channels, wide) {
        (4, false) => return out.extend_from_slice(&row[..width * 4]),
        (3, false) => {
            for p in row[..width * 3].chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
            return;
        }
        _ => {}
    }
    let sample = |i: usize| -> u8 {
        if wide {
            let wide = u16::from_be_bytes([row[2 * i], row[2 * i + 1]]);
            ((u32::from(wide) + 128) / 257) as u8
        } else {
            row[i]
        }
    };
    for px in 0..width {
        let at = px * channels;
        let pixel = match channels {
            1 => {
                let g = sample(at);
                [g, g, g, 255]
            }
            2 => {
                let g = sample(at);
                [g, g, g, sample(at + 1)]
            }
            3 => [sample(at), sample(at + 1), sample(at + 2), 255],
            _ => [sample(at), sample(at + 1), sample(at + 2), sample(at + 3)],
        };
        out.extend_from_slice(&pixel);
    }
}

/// A sound decoded a few packets at a time, into what `decode_sound` makes.
#[cfg(feature = "audio")]
pub(super) struct SoundSlices {
    reader: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
    track: u32,
    sample_rate: u32,
    frames: Vec<kira::Frame>,
    /// One packet's samples, interleaved (reused).
    samples: Vec<f32>,
}

#[cfg(feature = "audio")]
impl SoundSlices {
    /// Opens `bytes` as Kira's loader does (the same probe and codecs),
    /// decoding no samples yet.
    pub fn new(bytes: Vec<u8>) -> Result<Self, String> {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::formats::TrackType;
        use symphonia::core::io::MediaSourceStream;
        let fail = |e: &dyn std::fmt::Display| format!("could not decode the sound: {e}");
        let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
        let reader = symphonia::default::get_probe()
            .probe(&Default::default(), stream, Default::default(), Default::default())
            .map_err(|e| fail(&e))?;
        let no_track = || fail(&"the file has no default audio track");
        let track = reader.default_track(TrackType::Audio).ok_or_else(no_track)?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
            return Err(no_track());
        };
        let sample_rate = params.sample_rate.ok_or_else(|| fail(&"the sample rate is unknown"))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &Default::default())
            .map_err(|e| fail(&e))?;
        let track = track.id;
        Ok(Self {
            reader,
            decoder,
            track,
            sample_rate,
            frames: Vec::new(),
            samples: Vec::new(),
        })
    }

    /// Decodes the next few packets; `Ok(true)` at the end of the sound.
    pub fn step(&mut self) -> Result<bool, String> {
        let fail = |e: &dyn std::fmt::Display| format!("could not decode the sound: {e}");
        let goal = self.frames.len() + SLICE_BYTES / std::mem::size_of::<kira::Frame>();
        while self.frames.len() < goal {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(true),
                Err(e) => return Err(fail(&e)),
            };
            if packet.track_id != self.track {
                continue;
            }
            let buffer = self.decoder.decode(&packet).map_err(|e| fail(&e))?;
            let channels = buffer.num_planes();
            buffer.copy_to_vec_interleaved(&mut self.samples);
            match channels {
                1 => self
                    .frames
                    .extend(self.samples.iter().map(|&s| kira::Frame::from_mono(s))),
                2 => self
                    .frames
                    .extend(self.samples.chunks_exact(2).map(|lr| kira::Frame::new(lr[0], lr[1]))),
                _ => return Err(fail(&"unsupported channel configuration")),
            }
        }
        Ok(false)
    }

    /// The decoded sound, once [`step`](Self::step) said it is done.
    pub fn finish(self) -> crate::audio::DecodedSound {
        Box::new(kira::sound::static_sound::StaticSoundData {
            sample_rate: self.sample_rate,
            frames: self.frames.into(),
            settings: Default::default(),
            slice: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG written by the `png` encoder: `width` x `height` of `color`
    /// at `depth`, its samples a pattern, with an optional palette and
    /// transparency chunk.
    fn png_file(
        (width, height): (u32, u32),
        color: png::ColorType,
        depth: png::BitDepth,
        palette: Option<(&[u8], &[u8])>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(depth);
            if let Some((palette, trns)) = palette {
                encoder.set_palette(palette.to_vec());
                encoder.set_trns(trns.to_vec());
            }
            let mut writer = encoder.write_header().unwrap();
            let bits = color.samples() * depth as usize;
            let row = (width as usize * bits).div_ceil(8);
            let mut data = vec![0u8; row * height as usize];
            let mut s = 0x9E37_79B9u32;
            for b in &mut data {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                *b = if palette.is_some() {
                    // Four 2-bit indices into a three-colour palette.
                    (0..4).fold(0, |byte, k| (byte << 2) | ((s >> (8 * k)) % 3) as u8)
                } else {
                    s as u8
                };
            }
            writer.write_image_data(&data).unwrap();
        }
        out
    }

    fn sliced(bytes: Vec<u8>) -> Image {
        let mut png = PngSlices::new(&Arc::new(bytes)).expect("a PNG it can split");
        let mut slices = 1;
        while !png.step().unwrap() {
            slices += 1;
        }
        assert!(slices >= 1);
        png.finish()
    }

    #[test]
    fn sliced_pngs_decode_to_exactly_what_the_one_piece_decode_makes() {
        use png::{BitDepth::*, ColorType::*};
        let size = (37, 300);
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("rgba8", png_file(size, Rgba, Eight, None)),
            ("rgb8", png_file(size, Rgb, Eight, None)),
            ("grey8", png_file(size, Grayscale, Eight, None)),
            ("grey-alpha8", png_file(size, GrayscaleAlpha, Eight, None)),
            ("grey2", png_file(size, Grayscale, Two, None)),
            ("rgba16", png_file(size, Rgba, Sixteen, None)),
            ("rgb16", png_file(size, Rgb, Sixteen, None)),
            ("grey16", png_file(size, Grayscale, Sixteen, None)),
            (
                "palette2-trns",
                png_file(
                    size,
                    Indexed,
                    Two,
                    Some((&[255, 0, 0, 0, 255, 0, 0, 0, 255], &[0, 128])),
                ),
            ),
        ];
        for (name, bytes) in cases {
            let whole = crate::assets::decode_image(&bytes, name, name).unwrap();
            let parts = sliced(bytes);
            assert_eq!((parts.width, parts.height), (whole.width, whole.height), "{name}");
            assert!(parts.rgba == whole.rgba, "{name}: the slices made other pixels");
        }
    }

    #[test]
    fn a_tall_png_takes_one_slice_per_few_rows() {
        let bytes = png_file((1024, 64), png::ColorType::Rgba, png::BitDepth::Eight, None);
        let mut png = PngSlices::new(&Arc::new(bytes)).unwrap();
        let mut slices = 1;
        while !png.step().unwrap() {
            slices += 1;
        }
        // 64 KiB of RGBA a slice: 16 rows of 1024 pixels.
        assert_eq!(slices, 4);
    }

    #[test]
    fn files_that_are_not_plain_pngs_are_left_to_the_one_piece_decode() {
        assert!(PngSlices::new(&Arc::new(b"GIF89a....".to_vec())).is_none());
        let mut broken = png_file((4, 4), png::ColorType::Rgba, png::BitDepth::Eight, None);
        broken.truncate(20); // the header is cut short
        assert!(PngSlices::new(&Arc::new(broken)).is_none());
    }

    #[test]
    fn a_png_cut_short_fails_in_a_slice_with_a_message() {
        let mut bytes = png_file((512, 512), png::ColorType::Rgba, png::BitDepth::Eight, None);
        bytes.truncate(bytes.len() / 2);
        let mut png = PngSlices::new(&Arc::new(bytes)).expect("its header is whole");
        let error = loop {
            match png.step() {
                Ok(false) => {}
                Ok(true) => panic!("half a file decoded"),
                Err(e) => break e,
            }
        };
        assert!(!error.is_empty());
    }

    /// A 16-bit WAV of `frames` frames at 44.1 kHz, `channels` 1 or 2.
    #[cfg(feature = "audio")]
    fn wav(frames: u32, channels: u16) -> Vec<u8> {
        let data = frames * 2 * u32::from(channels);
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&44_100u32.to_le_bytes());
        out.extend_from_slice(&(44_100 * 2 * u32::from(channels)).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for i in 0..frames * u32::from(channels) {
            out.extend_from_slice(&((i.wrapping_mul(2_654_435_761) >> 16) as i16).to_le_bytes());
        }
        out
    }

    #[cfg(feature = "audio")]
    #[test]
    fn sliced_sounds_decode_to_exactly_what_the_one_piece_decode_makes() {
        for channels in [1, 2] {
            let bytes = wav(100_000, channels);
            let whole = crate::audio::decode_sound(bytes.clone()).unwrap();
            let mut sound = SoundSlices::new(bytes).unwrap();
            let mut slices = 1;
            while !sound.step().unwrap() {
                slices += 1;
            }
            assert!(slices > 10, "{channels} channels: {slices} slices");
            let parts = sound.finish();
            assert_eq!(parts.sample_rate, whole.sample_rate);
            assert_eq!(parts.frames.len(), whole.frames.len(), "{channels} channels");
            assert!(parts.frames == whole.frames, "{channels} channels: other samples");
        }
    }

    #[cfg(feature = "audio")]
    #[test]
    fn a_file_that_is_not_a_sound_fails_when_opened() {
        let error = SoundSlices::new(b"not a sound at all".to_vec()).err().unwrap();
        assert!(error.starts_with("could not decode the sound"), "{error}");
    }
}
