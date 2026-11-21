//! WebM (Matroska) files: the video track's frames, in order, with their
//! times. The file is held in memory, so going back is starting over and
//! reading forward (the demuxer's own seek, through the file's cue index,
//! lands in the wrong cluster for files with more than one).

use std::io::Cursor;
use std::sync::Arc;

use matroska_demuxer::{Frame, MatroskaFile, TrackType};

type File = MatroskaFile<Cursor<Arc<[u8]>>>;

/// What a file holds, read once when it is opened.
#[derive(Clone, Debug)]
pub(crate) struct Info {
    pub width: u32,
    pub height: u32,
    /// The video track's codec, as Matroska names it (`V_AV1`).
    pub codec: String,
    /// The audio track's codec (`A_VORBIS`), if there is one.
    pub audio_codec: Option<String>,
    pub seconds: f64,
    /// The AV1 sequence header (its OBUs), to send again after the
    /// decoder is flushed: encoders write it in the first frame only.
    pub sequence_header: Vec<u8>,
    /// The `av1C` record's first 4 bytes (profile, level, tier, bit
    /// depth), when the file has one: the browser's decoder is asked for
    /// that codec.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub config: Option<[u8; 4]>,
    /// Every video frame's time in seconds, in file order, and whether it
    /// is a keyframe.
    pub frames: Vec<(f64, bool)>,
}

/// A video frame from [`Demuxer::next`]: its time in seconds, whether it is
/// a keyframe, and its data.
pub(crate) type Unit<'a> = (f64, bool, &'a [u8]);

/// Reads one file's video frames.
pub(crate) struct Demuxer {
    bytes: Arc<[u8]>,
    file: File,
    track: u64,
    /// Seconds per Matroska timestamp tick.
    tick: f64,
    frame: Frame,
}

fn open(bytes: &Arc<[u8]>) -> Result<File, String> {
    MatroskaFile::open(Cursor::new(bytes.clone())).map_err(|e| format!("it is not a WebM or Matroska file ({e})"))
}

impl Demuxer {
    /// Opens `bytes` and reads its frame list.
    pub fn new(bytes: Arc<[u8]>) -> Result<(Self, Info), String> {
        let file = open(&bytes)?;
        let video = file
            .tracks()
            .iter()
            .find(|t| t.track_type() == TrackType::Video)
            .ok_or("it has no video track")?;
        let size = video.video().ok_or("its video track has no picture size")?;
        let width = u32::try_from(size.pixel_width().get()).map_err(|_| "its picture is too wide")?;
        let height = u32::try_from(size.pixel_height().get()).map_err(|_| "its picture is too tall")?;
        // The player allocates a texture of this size before any frame.
        crate::assets::check_image_size(width.into(), height.into(), "", "its picture")
            .map_err(|e| e.message().to_string())?;
        let track = video.track_number().get();
        let codec = video.codec_id().to_owned();
        // The `av1C` record: 4 bytes, then the sequence header's OBUs.
        let private = video.codec_private().and_then(|p| p.get(4..)).map(<[u8]>::to_vec);
        let config = video
            .codec_private()
            .and_then(|p| p.get(..4))
            .and_then(|p| <[u8; 4]>::try_from(p).ok());
        let audio_codec = file
            .tracks()
            .iter()
            .find(|t| t.track_type() == TrackType::Audio)
            .map(|t| t.codec_id().to_owned());
        let tick = file.info().timestamp_scale().get() as f64 * 1e-9;
        let declared = file.info().duration().map(|d| d * tick);
        let mut demuxer = Self {
            bytes,
            file,
            track,
            tick,
            frame: Frame::default(),
        };
        let mut frames = Vec::new();
        let mut sequence_header = private.filter(|p| !p.is_empty()).unwrap_or_default();
        while let Some((at, keyframe, data)) = demuxer.next()? {
            if sequence_header.is_empty() {
                sequence_header = sequence_headers(data);
            }
            frames.push((at, keyframe));
        }
        if frames.is_empty() {
            return Err("its video track has no frames".into());
        }
        // A frame lasts until the next; the last one as long as the one before it.
        let last = frames.last().map_or(0.0, |f| f.0);
        let step = frames.len().checked_sub(2).map_or(0.0, |i| last - frames[i].0);
        // The picture's length: the container's duration also covers a
        // sound track that runs a little longer, which would leave the
        // last frame up for that long at every loop.
        let declared = declared.filter(|d| d.is_finite() && *d > 0.0);
        let seconds = if last + step > 0.0 {
            last + step
        } else {
            declared.unwrap_or(0.0)
        };
        demuxer.restart()?;
        let info = Info {
            width,
            height,
            codec,
            audio_codec,
            seconds,
            sequence_header,
            config,
            frames,
        };
        Ok((demuxer, info))
    }

    /// The next video frame: its time in seconds, whether the file marks
    /// it as a keyframe, and its data. `None` at the end of the file.
    pub fn next(&mut self) -> Result<Option<Unit<'_>>, String> {
        loop {
            match self.file.next_frame(&mut self.frame) {
                Ok(true) if self.frame.track == self.track => {
                    let at = self.frame.timestamp as f64 * self.tick;
                    // Simple blocks say whether they are keyframes; other
                    // blocks don't, and then only the first frame counts.
                    let keyframe = self.frame.is_keyframe.unwrap_or(false);
                    return Ok(Some((at, keyframe, &self.frame.data)));
                }
                Ok(true) => {}
                Ok(false) => return Ok(None),
                Err(e) => return Err(format!("its data is damaged ({e})")),
            }
        }
    }

    /// Back to the first frame.
    pub fn restart(&mut self) -> Result<(), String> {
        self.file = open(&self.bytes)?;
        Ok(())
    }

    /// Reads past the next `count` video frames.
    pub fn skip(&mut self, count: usize) -> Result<(), String> {
        for _ in 0..count {
            if self.next()?.is_none() {
                break;
            }
        }
        Ok(())
    }
}

/// The sequence header OBUs of an AV1 temporal unit (in the low-overhead
/// format Matroska stores: every OBU with its size). Empty if there are
/// none or the data can't be walked.
pub(crate) fn sequence_headers(unit: &[u8]) -> Vec<u8> {
    const SEQUENCE_HEADER: u8 = 1;
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(&header) = unit.get(at) {
        let kind = (header >> 3) & 0xf;
        let extension = usize::from(header & 0b100 != 0);
        if header & 0b10 == 0 {
            // No size field: the OBU runs to the end; not used in Matroska.
            break;
        }
        let Some((size, size_bytes)) = leb128(unit.get(at + 1 + extension..).unwrap_or(&[])) else {
            break;
        };
        let Some(end) = (at + 1 + extension + size_bytes)
            .checked_add(size)
            .filter(|&end| end <= unit.len())
        else {
            break;
        };
        if kind == SEQUENCE_HEADER {
            found.extend_from_slice(&unit[at..end]);
        }
        at = end;
    }
    found
}

/// An unsigned LEB128 number (at most 8 bytes, as AV1 allows) and its length.
fn leb128(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut value = 0u64;
    for (i, &byte) in bytes.iter().take(8).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((usize::try_from(value).ok()?, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_headers_are_picked_out_of_a_temporal_unit() {
        // A temporal delimiter (type 2, size 0), a sequence header (type 1)
        // of 3 bytes, a frame (type 6) of 2 bytes.
        let unit = [0x12, 0x00, 0x0a, 0x03, 1, 2, 3, 0x32, 0x02, 9, 9];
        assert_eq!(sequence_headers(&unit), [0x0a, 0x03, 1, 2, 3]);
        assert!(sequence_headers(&[0x12, 0x00]).is_empty());
        // A size running past the end stops the walk.
        assert!(sequence_headers(&[0x0a, 0x7f, 1]).is_empty());
        assert_eq!(leb128(&[0x80, 0x01]), Some((128, 2)));
        assert_eq!(leb128(&[0x80; 9]), None);
    }
}
