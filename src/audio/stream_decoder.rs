//! Streamed sound on the desktop: the decoder Kira's decoder thread runs.
//!
//! Kira decodes a streamed sound on a thread of its own, through a
//! [`Decoder`]. Its built-in one calls the format readers and codecs
//! unguarded, so a panic inside one (a crafted file, a reader bug that a
//! build with overflow checks trips) would end that thread and leave the
//! sound silent forever. [`StreamDecoder`] is the engine's own: the same
//! readers and codecs, with the header checked against the limits of
//! [`sound_file`](super::sound_file) and every call guarded. A panic is an
//! error, which Kira answers by stopping the sound; the decoder then stays
//! closed, and the engine logs the error when the voice ends.
//!
//! It never opens an audio device, so the fuzz target `sound` runs it over
//! the fuzzer's bytes directly (`fuzz_stream`).

use std::io::Cursor;
use std::sync::Arc;

use kira::Frame;
use kira::sound::streaming::Decoder;
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::{CodecParameters, audio::AudioDecoder};
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::units::Timestamp;

/// The error of every call after a decoder panicked.
pub(crate) const CRASHED: &str = "the file crashed the sound decoder (it is damaged or hostile)";

/// A streamed sound's decoder: a container reader and a codec over a file
/// or encoded bytes in memory.
pub(crate) struct StreamDecoder {
    /// `None` once a call panicked: the reader and codec may be half way
    /// through changing their state, so they are never used again.
    open: Option<Open>,
    sample_rate: u32,
    frames: usize,
    /// Tests: the next `decode` panics inside the guard, as a reader bug would.
    #[cfg(test)]
    pub(super) panic_next_decode: bool,
}

struct Open {
    reader: Box<dyn FormatReader>,
    codec: Box<dyn AudioDecoder>,
    track: u32,
}

impl StreamDecoder {
    /// Opens `source`: reads its header and checks it against the limits,
    /// decoding no samples. Errors (and panics in the reader) are values.
    pub(crate) fn open(source: impl MediaSource + 'static) -> Result<Self, String> {
        super::sound_file::guarded(|| Self::open_unguarded(source))
    }

    /// [`open`](Self::open) over encoded bytes held in memory.
    pub(crate) fn from_bytes(bytes: &Arc<[u8]>) -> Result<Self, String> {
        Self::open(Cursor::new(bytes.clone()))
    }

    /// [`open`](Self::open) over a file on disk.
    pub(crate) fn from_file(path: &std::path::Path) -> Result<Self, String> {
        Self::open(std::fs::File::open(path).map_err(|e| e.to_string())?)
    }

    fn open_unguarded(source: impl MediaSource + 'static) -> Result<Self, String> {
        let stream = MediaSourceStream::new(Box::new(source), Default::default());
        let reader = symphonia::default::get_probe()
            .probe(&Hint::default(), stream, Default::default(), Default::default())
            .map_err(|e| e.to_string())?;
        let track = reader.default_track(TrackType::Audio).ok_or("it has no audio track")?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
            return Err("it has no audio track".into());
        };
        super::sound_file::check_params(params)?;
        let sample_rate = params
            .sample_rate
            .filter(|&rate| rate > 0)
            .ok_or("its sample rate is unknown")?;
        // Kira plays a stream to its length, so a stream needs one (a
        // sound whose file doesn't say is loaded whole instead).
        let frames = track
            .num_frames
            .ok_or("its length is unknown (load it whole instead of streaming it)")?;
        let frames = usize::try_from(frames).map_err(|_| format!("its length of {frames} frames is too long"))?;
        let codec = symphonia::default::get_codecs()
            .make_audio_decoder(params, &Default::default())
            .map_err(|e| e.to_string())?;
        let track = track.id;
        Ok(Self {
            open: Some(Open { reader, codec, track }),
            sample_rate,
            frames,
            #[cfg(test)]
            panic_next_decode: false,
        })
    }

    /// Runs `f` over the open reader and codec; a panic inside it closes
    /// the decoder for good and is an error, as is every call after it.
    fn guard<T>(&mut self, f: impl FnOnce(&mut Open) -> Result<T, String>) -> Result<T, String> {
        let Some(open) = self.open.as_mut() else {
            // Kira's decoder thread calls again until the mixer has seen
            // the error and stopped the sound (its next block): don't spin.
            std::thread::sleep(std::time::Duration::from_millis(1));
            return Err(CRASHED.into());
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(open))) {
            Ok(result) => result,
            Err(_) => {
                self.open = None;
                Err(CRASHED.into())
            }
        }
    }
}

impl Open {
    /// The next packet of the track, decoded; no frames at the end.
    fn decode(&mut self) -> Result<Vec<Frame>, String> {
        let packet = loop {
            match self.reader.next_packet().map_err(|e| e.to_string())? {
                Some(packet) if packet.track_id == self.track => break packet,
                Some(_) => {}
                None => return Ok(Vec::new()),
            }
        };
        let buffer = self.codec.decode(&packet).map_err(|e| e.to_string())?;
        frames(&buffer)
    }
}

/// A decoded packet as Kira's frames (mono played on both sides).
fn frames(buffer: &GenericAudioBufferRef) -> Result<Vec<Frame>, String> {
    let planes = buffer.num_planes();
    let mut samples = Vec::new();
    buffer.copy_to_vec_interleaved(&mut samples);
    match planes {
        1 => Ok(samples.into_iter().map(Frame::from_mono).collect()),
        2 => Ok(samples.as_chunks::<2>().0.iter().map(|&[l, r]| Frame::new(l, r)).collect()),
        n => Err(format!(
            "a packet has {n} channels and only 1 or {} are supported",
            super::sound_file::MAX_CHANNELS
        )),
    }
}

impl Decoder for StreamDecoder {
    type Error = String;

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn num_frames(&self) -> usize {
        self.frames
    }

    fn decode(&mut self) -> Result<Vec<Frame>, String> {
        #[cfg(test)]
        let panic_now = std::mem::take(&mut self.panic_next_decode);
        self.guard(|open| {
            #[cfg(test)]
            assert!(!panic_now, "a decoder bug, as a test asked for");
            open.decode()
        })
    }

    fn seek(&mut self, index: usize) -> Result<usize, String> {
        let frames = self.frames;
        self.guard(|open| {
            let seeked = open
                .reader
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Timestamp {
                        ts: Timestamp::new(i64::try_from(index).unwrap_or(i64::MAX)),
                        track_id: open.track,
                    },
                )
                .map_err(|e| e.to_string())?;
            open.codec.reset();
            Ok(usize::try_from(seeked.actual_ts.get()).unwrap_or(0).min(frames))
        })
    }
}

/// For the fuzz target `sound`: opens `bytes` as the desktop streams them
/// and runs the decoder as Kira's decoder thread does (decoding, seeking
/// back to the start, into the middle and past the end), with no audio
/// device. Whatever the file says, every call must return.
#[cfg(fuzzing)]
pub(crate) fn fuzz_stream(bytes: &[u8]) {
    let bytes: Arc<[u8]> = bytes.into();
    let Ok(mut decoder) = StreamDecoder::from_bytes(&bytes) else {
        return;
    };
    let frames = decoder.num_frames();
    let mut decoded = 0usize;
    for target in [0, frames / 2, frames.saturating_add(1)] {
        if decoder.seek(target).is_err() {
            return;
        }
        // A few packets each: enough to reach the codec, bounded however
        // many the file holds.
        for _ in 0..16 {
            match decoder.decode() {
                Ok(chunk) if chunk.is_empty() => break,
                Ok(chunk) => decoded = decoded.saturating_add(chunk.len()),
                Err(_) => break,
            }
        }
    }
    std::hint::black_box(decoded);
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A mono 16-bit PCM WAV of `samples` at `rate` Hz.
    pub(in crate::audio) fn wav(rate: u32, samples: &[i16]) -> Arc<[u8]> {
        let data = (samples.len() * 2) as u32;
        let mut out = Vec::from(&b"RIFF"[..]);
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out.into()
    }

    /// The known hostile header: 60 000 channels of 16 bits overflow the
    /// block size the RIFF reader works out (a panic in builds with
    /// overflow checks).
    pub(in crate::audio) fn hostile_wav() -> Vec<u8> {
        let mut out = Vec::from(&b"RIFF"[..]);
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&60_000u16.to_le_bytes());
        out.extend_from_slice(&8000u32.to_le_bytes());
        out.extend_from_slice(&8000u32.wrapping_mul(120_000).to_le_bytes());
        out.extend_from_slice(&60_000u16.wrapping_mul(2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&240u32.to_le_bytes());
        out.extend_from_slice(&[0; 240]);
        out
    }

    fn ramp(n: usize) -> Vec<i16> {
        (0..n).map(|i| (i as i16).wrapping_mul(64)).collect()
    }

    /// Every frame the decoder gives from where it stands to the end.
    fn drain(decoder: &mut StreamDecoder) -> Vec<Frame> {
        let mut out = Vec::new();
        loop {
            let chunk = decoder.decode().unwrap();
            if chunk.is_empty() {
                return out;
            }
            out.extend(chunk);
        }
    }

    #[test]
    fn the_stream_decoder_gives_the_samples_a_whole_decode_gives() {
        let bytes = wav(8000, &ramp(3000));
        let whole = crate::audio::decode_sound(bytes.to_vec()).unwrap();
        let mut decoder = StreamDecoder::from_bytes(&bytes).unwrap();
        assert_eq!((decoder.sample_rate(), decoder.num_frames()), (8000, 3000));
        assert_eq!(decoder.seek(0).unwrap(), 0);
        let streamed = drain(&mut decoder);
        assert_eq!(streamed.len(), whole.frames.len());
        assert!(streamed.iter().zip(whole.frames.iter()).all(|(a, b)| a == b));
    }

    #[test]
    fn the_stream_decoder_seeks_back_and_forth() {
        let bytes = wav(8000, &ramp(3000));
        let whole = crate::audio::decode_sound(bytes.to_vec()).unwrap();
        let mut decoder = StreamDecoder::from_bytes(&bytes).unwrap();
        drain(&mut decoder);
        let at = decoder.seek(1500).unwrap();
        assert!(at <= 1500, "a seek may land early, never late: {at}");
        let rest = drain(&mut decoder);
        assert_eq!(rest.len(), 3000 - at);
        assert_eq!(rest[0], whole.frames[at]);
        assert_eq!(decoder.seek(0).unwrap(), 0, "and back to the start, as a loop does");
        assert_eq!(drain(&mut decoder).len(), 3000);
    }

    #[test]
    fn the_known_hostile_wav_header_is_an_error_when_a_stream_opens_not_a_panic() {
        let bytes: Arc<[u8]> = hostile_wav().into();
        assert!(StreamDecoder::from_bytes(&bytes).is_err());
    }

    #[test]
    fn a_decoder_panic_while_streaming_is_an_error_and_closes_the_decoder_for_good() {
        let mut decoder = StreamDecoder::from_bytes(&wav(8000, &ramp(3000))).unwrap();
        assert!(!decoder.decode().unwrap().is_empty());
        decoder.panic_next_decode = true;
        assert_eq!(decoder.decode().unwrap_err(), CRASHED);
        assert_eq!(decoder.decode().unwrap_err(), CRASHED, "never used again");
        assert_eq!(decoder.seek(0).unwrap_err(), CRASHED);
    }

    #[test]
    fn a_stream_over_the_channel_limit_is_refused_at_open() {
        // Six channels: over the channel limit, before a sample is read.
        let mut six = wav(8000, &ramp(600)).to_vec();
        six[22..24].copy_from_slice(&6u16.to_le_bytes());
        six[32..34].copy_from_slice(&12u16.to_le_bytes());
        let error = StreamDecoder::from_bytes(&six.into()).err().unwrap();
        assert!(error.contains("channels"), "{error}");
    }
}
