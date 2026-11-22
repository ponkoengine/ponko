//! What a sound file may claim, and the one place its header is checked.
//!
//! A sound file's header says its sample rate, its channels and often how
//! long it is, and a file from a download or a mod can lie about all three.
//! Every loader that reads a sound (a whole sound, a stream, a worker's
//! slice-by-slice decode, a video's soundtrack) checks the header here
//! before it decodes, and the whole-sound decode also stops at
//! [`MAX_FRAMES`] however short the header said the file was: a few
//! kilobytes of compressed silence can unpack to gigabytes. Every refusal
//! is an error naming the limit.

use std::io::Cursor;
use std::sync::Arc;
use symphonia::core::codecs::{CodecParameters, audio::AudioCodecParameters};
use symphonia::core::formats::{TrackType, probe::Hint};
use symphonia::core::io::{MediaSource, MediaSourceStream};

/// The highest sample rate a sound may have (Hz): the highest rate audio
/// interfaces and formats are made for.
pub(crate) const MAX_SAMPLE_RATE: u32 = 768_000;
/// The most channels a sound may have: the mixer plays mono and stereo.
pub(crate) const MAX_CHANNELS: usize = 2;
/// The most frames a sound decoded whole may have: 512 MiB of samples
/// (8 bytes a frame), about 25 minutes at 44.1 kHz. A longer sound is
/// streamed, which holds no decoded samples.
pub(crate) const MAX_FRAMES: u64 = 64 * 1024 * 1024;

/// Checks the sample rate and channel count a file's header claims.
pub(crate) fn check_params(params: &AudioCodecParameters) -> Result<(), String> {
    if let Some(rate) = params.sample_rate {
        if rate == 0 {
            return Err("its sample rate is zero".into());
        }
        if rate > MAX_SAMPLE_RATE {
            return Err(format!(
                "its sample rate of {rate} Hz is over the limit of {MAX_SAMPLE_RATE} Hz"
            ));
        }
    }
    if let Some(channels) = params.channels.as_ref() {
        let count = channels.count();
        if count == 0 || count > MAX_CHANNELS {
            return Err(format!(
                "it has {count} channels and only 1 or {MAX_CHANNELS} are supported (limit {MAX_CHANNELS})"
            ));
        }
    }
    Ok(())
}

/// [`MAX_FRAMES`], except in a test that lowers it for its thread (a sound
/// of the real limit's size is half a gigabyte to decode).
fn max_frames() -> u64 {
    #[cfg(test)]
    if let Some(limit) = tests::LIMIT.with(std::cell::Cell::get) {
        return limit;
    }
    MAX_FRAMES
}

/// Checks the length (in frames) a sound that is decoded whole has, or its
/// file's header claims, against the limit.
pub(crate) fn check_length(frames: Option<u64>) -> Result<(), String> {
    match frames {
        Some(frames) if frames > max_frames() => Err(format!(
            "it is {frames} frames long and the limit is {} frames (stream it instead)",
            max_frames()
        )),
        _ => Ok(()),
    }
}

/// Runs `f`, which calls a decoder, and turns a panic inside the decoder
/// into an error: a crafted file must not be able to take the game down
/// through a bug in a format's reader (on a build with overflow checks,
/// `debug-assertions` or the fuzzer, an overflowing size in a header is
/// one). The panic message still goes to the log. A build that aborts on
/// panic can't catch it.
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err("the file crashed the sound decoder (it is damaged or hostile)".into()))
}

/// Reads the header of `source` and checks its sample rate and channels
/// (not its length: a stream holds no decoded samples), decoding no samples.
fn probe(source: impl MediaSource + 'static) -> Result<(), String> {
    guarded(|| probe_unguarded(source))
}

fn probe_unguarded(source: impl MediaSource + 'static) -> Result<(), String> {
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let reader = symphonia::default::get_probe()
        .probe(&Hint::default(), stream, Default::default(), Default::default())
        .map_err(|e| e.to_string())?;
    let track = reader.default_track(TrackType::Audio).ok_or("it has no audio track")?;
    match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(params)) => check_params(params),
        _ => Err("it has no audio track".into()),
    }
}

/// [`probe`] for encoded bytes held in memory.
pub(crate) fn check_bytes(bytes: &Arc<[u8]>) -> Result<(), String> {
    probe(Cursor::new(bytes.clone()))
}

/// [`probe`] for a sound file on disk.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn check_file(path: &std::path::Path) -> Result<(), String> {
    probe(std::fs::File::open(path).map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::group::sliced::SoundSlices;
    use crate::audio::{backend, chunked, decode_sound};
    use std::cell::Cell;

    thread_local! {
        /// A lower `MAX_FRAMES` for the test of this thread.
        pub(super) static LIMIT: Cell<Option<u64>> = const { Cell::new(None) };
    }

    /// A RIFF/WAVE file: a `fmt ` chunk of format `tag`, `channels`, `rate`
    /// and `bits` (with `extra` bytes after the 16 of a PCM one), then a
    /// `data` chunk that says it is `declared` bytes long and holds `body`.
    fn wave(tag: u16, channels: u16, rate: u32, bits: u16, extra: &[u8], declared: u32, body: &[u8]) -> Vec<u8> {
        let align = if tag == 0x11 {
            256
        } else {
            channels.wrapping_mul(bits.div_ceil(8)).max(1)
        };
        let mut out = Vec::from(&b"RIFF"[..]);
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&(16 + extra.len() as u32).to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&rate.wrapping_mul(u32::from(align)).to_le_bytes());
        out.extend_from_slice(&align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(extra);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&declared.to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    /// An 80-bit extended float, as AIFF writes its sample rate.
    fn extended(rate: u32) -> [u8; 10] {
        let log = 31 - rate.leading_zeros();
        let mut out = [0u8; 10];
        out[..2].copy_from_slice(&((16383 + log) as u16).to_be_bytes());
        out[2..].copy_from_slice(&(u64::from(rate) << (63 - log)).to_be_bytes());
        out
    }

    /// An AIFF file: `channels`, `frames` (as the header says), 16-bit
    /// samples at `rate`, and `body` for the samples.
    fn aiff(channels: i16, frames: u32, rate: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::from(&b"FORM"[..]);
        out.extend_from_slice(&(4 + 26 + 16 + body.len() as u32).to_be_bytes());
        out.extend_from_slice(b"AIFFCOMM");
        out.extend_from_slice(&18u32.to_be_bytes());
        out.extend_from_slice(&channels.to_be_bytes());
        out.extend_from_slice(&frames.to_be_bytes());
        out.extend_from_slice(&16i16.to_be_bytes());
        out.extend_from_slice(&extended(rate));
        out.extend_from_slice(b"SSND");
        out.extend_from_slice(&(8 + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(body);
        out
    }

    /// A FLAC file of a STREAMINFO block saying `channels`, `rate` and
    /// `total` samples, and one frame header.
    fn flac(channels: u8, rate: u32, total: u64) -> Vec<u8> {
        let mut out = Vec::from(&b"fLaC"[..]);
        out.extend_from_slice(&[0x80, 0, 0, 34]);
        out.extend_from_slice(&4096u16.to_be_bytes());
        out.extend_from_slice(&4096u16.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let packed = (u64::from(rate) << 44) | (u64::from(channels - 1) << 41) | (15 << 36) | total;
        out.extend_from_slice(&packed.to_be_bytes());
        out.extend_from_slice(&[0; 16]);
        // A frame header (fixed block size of 16, rate and sample size from
        // the STREAMINFO) and a little data, so the reader finds its audio.
        let mut header = vec![0xFF, 0xF8, 0x60, (channels - 1) << 4, 0x00, 15];
        let mut crc = 0u8;
        for byte in &header {
            crc ^= byte;
            for _ in 0..8 {
                crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
            }
        }
        header.push(crc);
        out.extend_from_slice(&header);
        out.extend_from_slice(&[0; 32]);
        out
    }

    /// An MP3 file of `count` silent frames (MPEG-1 layer III, 128 kbit/s,
    /// 44.1 kHz stereo); with `claimed`, the first is a Xing header saying
    /// the file has that many frames.
    fn mp3(claimed: Option<u32>, count: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for n in 0..count {
            let mut frame = vec![0u8; 417];
            frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            if let (0, Some(claimed)) = (n, claimed) {
                frame[36..40].copy_from_slice(b"Xing");
                frame[40..44].copy_from_slice(&1u32.to_be_bytes());
                frame[44..48].copy_from_slice(&claimed.to_be_bytes());
            }
            out.extend_from_slice(&frame);
        }
        out
    }

    /// The Ogg Vorbis file in the examples with its first header's channels
    /// and sample rate replaced, and its last page saying it ends at `end`.
    fn vorbis(channels: u8, rate: u32, end: Option<u64>) -> Vec<u8> {
        /// Writes the checksum of the Ogg page starting at `at` into it.
        fn seal(file: &mut [u8], at: usize) {
            let segments = usize::from(file[at + 26]);
            let lacing = &file[at + 27..at + 27 + segments];
            let size = 27 + segments + lacing.iter().map(|&n| usize::from(n)).sum::<usize>();
            file[at + 22..at + 26].fill(0);
            let mut crc = 0u32;
            for &byte in &file[at..(at + size).min(file.len())] {
                crc ^= u32::from(byte) << 24;
                for _ in 0..8 {
                    crc = if crc & 0x8000_0000 != 0 {
                        (crc << 1) ^ 0x04C1_1DB7
                    } else {
                        crc << 1
                    };
                }
            }
            file[at + 22..at + 26].copy_from_slice(&crc.to_le_bytes());
        }
        let mut out = include_bytes!("../../assets/ember_hop/music/cave.ogg").to_vec();
        out[39] = channels;
        out[40..44].copy_from_slice(&rate.to_le_bytes());
        seal(&mut out, 0);
        if let Some(end) = end {
            let last = out.windows(4).rposition(|w| w == b"OggS").unwrap();
            out[last + 6..last + 14].copy_from_slice(&end.to_le_bytes());
            seal(&mut out, last);
        }
        out
    }

    /// What every way of reading `bytes` does with a header over a limit:
    /// each refuses it with an error naming `limit`, the whole and the
    /// slice-by-slice decoders at their first step (before decoding a
    /// sample), the streams from the header alone (a stream holds no
    /// samples, so its length is not limited).
    fn refused(bytes: &[u8], limit: &str, stream_too: bool) {
        let error = SoundSlices::new(bytes.to_vec())
            .err()
            .expect("the slice decoder refuses it at open");
        assert!(error.contains(limit), "{error}");
        let error = decode_sound(bytes.to_vec()).expect_err("the whole decode refuses it");
        assert!(error.contains(limit), "{error}");
        if stream_too {
            let shared: Arc<[u8]> = bytes.into();
            let error = check_bytes(&shared).expect_err("the stream check refuses it");
            assert!(error.contains(limit), "{error}");
            let error = backend::Audio::default()
                .stream_bytes(bytes.to_vec())
                .expect_err("a stream refuses it");
            assert!(error.contains(limit), "{error}");
            let error = chunked::header(&shared).err().expect("the chunked stream refuses it");
            assert!(error.contains(limit), "{error}");
        }
    }

    const RATE: &str = "sample rate";
    const CHANNELS: &str = "channels";
    const LENGTH: &str = "frames long";

    #[test]
    fn a_wav_with_an_absurd_sample_rate_channel_count_or_length_is_refused_before_decoding() {
        let ok = wave(1, 1, 8000, 16, &[], 200, &[0; 200]);
        assert_eq!(decode_sound(ok).unwrap().frames.len(), 100, "a plain file loads");
        refused(&wave(1, 1, 4_000_000_000, 16, &[], 200, &[0; 200]), RATE, true);
        refused(&wave(1, 6, 8000, 16, &[], 240, &[0; 240]), CHANNELS, true);
        // A data chunk that says it is 4 GB long, with 200 bytes behind it.
        refused(&wave(1, 1, 8000, 16, &[], u32::MAX - 100, &[0; 200]), LENGTH, false);
    }

    #[test]
    fn a_wav_whose_channel_count_overflows_the_readers_arithmetic_is_an_error_not_a_panic() {
        // 60 000 channels of 16 bits overflow the block size the reader works out.
        let bytes = wave(1, 60_000, 8000, 16, &[], 240, &[0; 240]);
        assert!(decode_sound(bytes.clone()).is_err());
        assert!(SoundSlices::new(bytes.clone()).is_err());
        assert!(check_bytes(&bytes.into()).is_err());
    }

    #[test]
    fn an_adpcm_wav_with_an_absurd_sample_rate_or_length_is_refused_before_decoding() {
        let adpcm = |rate, declared| wave(0x11, 1, rate, 4, &[2, 0, 0xF9, 1], declared, &[0; 256]);
        assert_eq!(
            decode_sound(adpcm(8000, 256)).unwrap().frames.len(),
            505,
            "a plain file loads"
        );
        refused(&adpcm(4_000_000_000, 256), RATE, true);
        refused(&adpcm(8000, u32::MAX - 1000), LENGTH, false);
    }

    #[test]
    fn an_aiff_with_an_absurd_sample_rate_or_channel_count_is_refused_and_a_false_length_costs_nothing() {
        assert_eq!(decode_sound(aiff(1, 100, 8000, &[0; 200])).unwrap().frames.len(), 100);
        refused(&aiff(1, 100, 4_000_000_000, &[0; 200]), RATE, true);
        refused(&aiff(6, 100, 8000, &[0; 200]), CHANNELS, true);
        // Four billion frames claimed, 100 there: only what is there is read.
        let lying = decode_sound(aiff(1, u32::MAX, 8000, &[0; 200])).unwrap();
        assert_eq!(lying.frames.len(), 100);
    }

    #[test]
    fn a_flac_with_an_absurd_sample_rate_channel_count_or_length_is_refused_before_decoding() {
        assert!(decode_sound(flac(1, 8000, 100)).is_ok(), "a plain file loads");
        // Past what the FLAC format can say, so its reader refuses it itself.
        assert!(decode_sound(flac(1, 1_048_575, 100)).is_err());
        refused(&flac(8, 8000, 100), CHANNELS, true);
        refused(&flac(1, 8000, (1 << 36) - 1), LENGTH, false);
    }

    #[test]
    fn an_mp3_that_claims_billions_of_frames_is_refused_before_decoding() {
        assert_eq!(
            decode_sound(mp3(Some(3), 4)).unwrap().frames.len(),
            3456,
            "a plain file loads"
        );
        refused(&mp3(Some(u32::MAX), 4), LENGTH, false);
    }

    #[test]
    fn an_ogg_vorbis_with_an_absurd_sample_rate_channel_count_or_length_is_refused_before_decoding() {
        assert_eq!(
            decode_sound(vorbis(2, 22050, None)).unwrap().sample_rate,
            22050,
            "a plain file loads"
        );
        refused(&vorbis(2, 4_000_000_000, None), RATE, true);
        refused(&vorbis(6, 22050, None), CHANNELS, true);
        refused(&vorbis(2, 22050, Some(1 << 60)), LENGTH, false);
    }

    #[test]
    fn a_sound_that_decodes_to_more_than_the_limit_is_refused_even_if_its_header_has_no_length() {
        LIMIT.with(|limit| limit.set(Some(2000)));
        // Four MP3 frames of 1152 samples and nothing in the header about it.
        let silent = mp3(None, 4);
        let error = decode_sound(silent.clone()).expect_err("refused while decoding");
        assert!(error.contains(LENGTH), "{error}");
        // Within the limit it loads, so the limit is what refused it.
        LIMIT.with(|limit| limit.set(Some(5000)));
        assert_eq!(decode_sound(silent).unwrap().frames.len(), 4608);
        LIMIT.with(|limit| limit.set(None));
    }

    #[test]
    fn the_length_limit_is_512_mib_of_decoded_samples() {
        assert!(check_length(Some(MAX_FRAMES)).is_ok());
        assert!(
            check_length(None).is_ok(),
            "a file that doesn't say is checked as it decodes"
        );
        assert!(check_length(Some(MAX_FRAMES + 1)).unwrap_err().contains("67108864"));
        assert_eq!(MAX_FRAMES * std::mem::size_of::<kira::Frame>() as u64, 512 << 20);
    }

    #[test]
    fn a_streamed_file_on_disk_is_checked_when_it_is_opened() {
        let path = std::env::temp_dir().join(format!("ponko-sound-limits-{}.wav", std::process::id()));
        std::fs::write(&path, wave(1, 1, 4_000_000_000, 16, &[], 200, &[0; 200])).unwrap();
        let error = check_file(&path).expect_err("refused");
        let streamed = backend::Audio::default().stream(path.clone());
        std::fs::remove_file(path).unwrap();
        assert!(error.contains(RATE), "{error}");
        assert!(streamed.expect_err("refused").contains(RATE));
    }
}
