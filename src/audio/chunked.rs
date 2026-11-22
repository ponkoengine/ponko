//! Streamed sound on the web: decoded one packet at a time as it plays.
//!
//! Kira streams on a decoder thread of its own, which a browser page
//! doesn't have, so on the web a streamed sound keeps its encoded bytes and
//! [`ChunkedSound`] decodes the next packet whenever the mixer has used up
//! the last one, on the thread that mixes (the page's). Only one packet's
//! samples are held decoded at a time instead of the whole track.
//!
//! It is compiled for the web, and natively for its tests.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

use kira::info::{Info, WhenToStart};
use kira::sound::{PlaybackState, Sound, SoundData};
use kira::{Frame, Panning, StartTime, Tween, interpolate_frame};
use symphonia::core::codecs::{CodecParameters, audio::AudioDecoder};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::units::{Time, TimeBase};

/// How long a stop fades out: Kira's default tween, which the other voices use.
const STOP_FADE_SECONDS: f64 = 0.01;

/// How quickly volume and panning follow a change, in seconds (about one
/// mixer block), so a change doesn't click.
const SMOOTHING_SECONDS: f64 = 0.01;

/// How far before its target a seek in the file lands: a decoder that was
/// reset gives nothing for the first packet after it (a Vorbis packet
/// only completes the one before it), so decoding starts a packet or two
/// early and the samples before the target are dropped.
const PREROLL_SECONDS: f64 = 0.1;

/// A sound's header: its sample rate and, when the file says, its length.
pub(crate) struct Header {
    pub sample_rate: u32,
    pub frames: Option<u64>,
}

impl Header {
    /// Length in seconds; 0 when the file doesn't say.
    pub fn seconds(&self) -> f32 {
        self.frames
            .map_or(0.0, |frames| (frames as f64 / f64::from(self.sample_rate)) as f32)
    }
}

/// One decoder over the encoded bytes: the container reader and the codec.
struct Decoding {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    /// The unit of the track's packet timestamps (a Matroska file's are
    /// milliseconds, not samples).
    time_base: Option<TimeBase>,
}

fn open(bytes: &Arc<[u8]>) -> Result<(Decoding, Header), String> {
    super::sound_file::guarded(|| open_unguarded(bytes))
}

fn open_unguarded(bytes: &Arc<[u8]>) -> Result<(Decoding, Header), String> {
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.clone())), Default::default());
    let reader = symphonia::default::get_probe()
        .probe(&Hint::default(), source, Default::default(), Default::default())
        .map_err(|e| e.to_string())?;
    let track = reader.default_track(TrackType::Audio).ok_or("it has no audio track")?;
    let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
        return Err("it has no audio track".into());
    };
    let sample_rate = params
        .sample_rate
        .filter(|&rate| rate > 0)
        .ok_or("its sample rate is unknown")?;
    super::sound_file::check_params(params)?;
    let header = Header {
        sample_rate,
        frames: track.num_frames,
    };
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &Default::default())
        .map_err(|e| e.to_string())?;
    let time_base = track.time_base;
    let track = track.id;
    Ok((
        Decoding {
            reader,
            decoder,
            track,
            time_base,
        },
        header,
    ))
}

/// Reads the header of encoded `bytes`, decoding no samples.
pub(crate) fn header(bytes: &Arc<[u8]>) -> Result<Header, String> {
    open(bytes).map(|(_, header)| header)
}

/// What a voice's handle and its sound share.
struct Shared {
    /// Linear gain, as `f32` bits.
    volume: AtomicU32,
    /// -1 (left) to 1 (right), as `f32` bits.
    panning: AtomicU32,
    /// Playback speed, as `f64` bits.
    rate: AtomicU64,
    stop: AtomicBool,
    /// 0 playing, 1 stopping (fading out), 2 stopped.
    state: AtomicU8,
    /// Seconds into the sound the mixer has played up to, as `f64` bits:
    /// the clock a video's picture follows.
    heard: AtomicU64,
    /// Volume changes and stops that wait for a start time or take longer
    /// than the smoothing. Only ever contended in native tests: on the web
    /// one thread both plays and mixes.
    changes: Mutex<Vec<Change>>,
}

/// A change from the handle, made when `at` comes.
#[derive(Clone, Copy, Debug)]
struct Change {
    at: StartTime,
    what: What,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum What {
    /// To this linear gain, over these seconds.
    Volume(f32, f64),
    /// Fade out over these seconds, then stop.
    Stop(f64),
}

/// Seconds a tween lasts, at least one smoothing step so nothing clicks.
fn tween_seconds(tween: &Tween) -> f64 {
    tween.duration.as_secs_f64().max(SMOOTHING_SECONDS)
}

const PLAYING: u8 = 0;
const STOPPING: u8 = 1;
const STOPPED: u8 = 2;

/// A streamed sound to play: the encoded bytes and how to play them.
pub(crate) struct ChunkedData {
    bytes: Arc<[u8]>,
    volume: f32,
    rate: f64,
    looping: bool,
    panning: f32,
    start_time: StartTime,
    /// Seconds into the sound to start from.
    from: f64,
    /// Where a loop goes back to, and where it turns (seconds).
    loop_start: f64,
    loop_end: Option<f64>,
    fade_in: Option<f64>,
}

impl ChunkedData {
    /// Plays `bytes` at linear gain `volume` and speed `rate`, looping or not.
    pub fn new(bytes: Arc<[u8]>, volume: f32, rate: f64, looping: bool) -> Self {
        Self {
            bytes,
            volume: gain(volume),
            rate: speed(rate),
            looping,
            panning: 0.0,
            start_time: StartTime::Immediate,
            from: 0.0,
            loop_start: 0.0,
            loop_end: None,
            fade_in: None,
        }
    }
    /// -1 (left) to 1 (right).
    #[must_use]
    pub fn panning(mut self, panning: f32) -> Self {
        self.panning = pan(panning);
        self
    }
    /// Silent until `at` (a delay or a clock time), then plays.
    #[must_use]
    pub fn start_time(mut self, at: StartTime) -> Self {
        self.start_time = at;
        self
    }
    /// Starts `seconds` into the sound.
    #[must_use]
    pub fn start_position(mut self, seconds: f64) -> Self {
        self.from = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        self
    }
    /// A looping playback turns at `end` (`None`: the end of the sound)
    /// and goes back to `start` (seconds).
    #[must_use]
    pub fn loop_points(mut self, start: f64, end: Option<f64>) -> Self {
        self.loop_start = if start.is_finite() { start.max(0.0) } else { 0.0 };
        self.loop_end = end.filter(|e| e.is_finite() && *e > self.loop_start);
        self
    }
    /// Fades in from silence over the tween once it starts.
    #[must_use]
    pub fn fade_in(mut self, tween: Option<Tween>) -> Self {
        self.fade_in = tween.as_ref().map(tween_seconds);
        self
    }
}

fn pan(panning: f32) -> f32 {
    if panning.is_finite() {
        panning.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn gain(volume: f32) -> f32 {
    if volume.is_finite() { volume.max(0.0) } else { 0.0 }
}

fn speed(rate: f64) -> f64 {
    if rate.is_finite() { rate.max(0.01) } else { 1.0 }
}

impl SoundData for ChunkedData {
    type Error = String;
    type Handle = ChunkedHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, ChunkedHandle), String> {
        let (sound, handle) = ChunkedSound::new(self)?;
        Ok((Box::new(sound), handle))
    }
}

/// Controls a playing [`ChunkedSound`], like Kira's own sound handles.
pub(crate) struct ChunkedHandle {
    shared: Arc<Shared>,
}

impl ChunkedHandle {
    pub fn state(&self) -> PlaybackState {
        match self.shared.state.load(Relaxed) {
            PLAYING => PlaybackState::Playing,
            STOPPING => PlaybackState::Stopping,
            _ => PlaybackState::Stopped,
        }
    }

    /// Fades out over 10 ms, then stops.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Relaxed);
    }

    /// Fades out over the tween from its start time, then stops.
    pub fn stop_with(&mut self, tween: Tween) {
        if tween.start_time == StartTime::Immediate && tween.duration.as_secs_f64() <= STOP_FADE_SECONDS {
            self.stop();
        } else {
            self.push(tween.start_time, What::Stop(tween_seconds(&tween)));
        }
    }

    /// Linear gain, over the tween from its start time.
    pub fn set_volume_with(&mut self, volume: f32, tween: Tween) {
        if tween.start_time == StartTime::Immediate && tween.duration.as_secs_f64() <= SMOOTHING_SECONDS {
            self.set_volume(volume);
        } else {
            self.push(tween.start_time, What::Volume(gain(volume), tween_seconds(&tween)));
        }
    }

    fn push(&mut self, at: StartTime, what: What) {
        let mut changes = self
            .shared
            .changes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        changes.push(Change { at, what });
    }

    /// Linear gain, smoothed.
    pub fn set_volume(&mut self, volume: f32) {
        self.shared.volume.store(gain(volume).to_bits(), Relaxed);
    }

    /// -1 (left) to 1 (right), smoothed.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub fn set_panning(&mut self, panning: f32) {
        self.shared.panning.store(pan(panning).to_bits(), Relaxed);
    }

    /// Playback speed (which also changes the pitch).
    pub fn set_playback_rate(&mut self, rate: f64) {
        self.shared.rate.store(speed(rate).to_bits(), Relaxed);
    }

    /// Seconds into the sound the mixer has played up to (where it
    /// started until the mixer first plays it).
    #[cfg_attr(not(feature = "video"), allow(dead_code))]
    pub fn position(&self) -> f64 {
        f64::from_bits(self.shared.heard.load(Relaxed))
    }
}

/// A playing streamed sound: decodes a packet whenever it has played the
/// last one, and resamples to the output rate.
pub(crate) struct ChunkedSound {
    bytes: Arc<[u8]>,
    /// `None` once the end is reached (and it doesn't loop) or decoding failed.
    decoding: Option<Decoding>,
    sample_rate: u32,
    looping: bool,
    /// The current packet's samples, interleaved, reused for every packet.
    samples: Vec<f32>,
    channels: usize,
    /// The next frame of `samples` to play.
    next: usize,
    /// The frames the resampler interpolates between: previous, current
    /// and the next two.
    window: [Frame; 4],
    /// How far playback is from `window[1]` towards `window[2]`.
    fraction: f64,
    /// Frames of silence pushed into the window after the end; once the
    /// current frame is one of them (3), the sound is over.
    tail: u8,
    /// The source frame `next_frame` gives next.
    position: u64,
    /// Frames still to drop after a seek landed before its target.
    skip: u64,
    /// After a seek in the file: the source frame to start at, found in
    /// the decoded packets by their timestamps.
    align: Option<u64>,
    /// Loop points in source frames.
    loop_start: u64,
    loop_end: Option<u64>,
    /// Waiting for this to start; silent meanwhile.
    waiting: Option<StartTime>,
    volume: f32,
    /// Where the volume moves, and by how much a second.
    target: f32,
    volume_rate: f32,
    /// The last volume the handle set without a tween.
    asked: f32,
    panning: f32,
    /// 0 to 1: the fade in, then the fade out of a stop.
    fade: f64,
    /// Change of `fade` per second: positive fading in, negative stopping.
    fade_rate: f64,
    stopping: bool,
    /// Changes from the handle not yet due, oldest first.
    pending: Vec<Change>,
    shared: Arc<Shared>,
}

impl ChunkedSound {
    fn new(data: ChunkedData) -> Result<(Self, ChunkedHandle), String> {
        let (decoding, header) = open(&data.bytes)?;
        let shared = Arc::new(Shared {
            volume: AtomicU32::new(data.volume.to_bits()),
            panning: AtomicU32::new(data.panning.to_bits()),
            rate: AtomicU64::new(data.rate.to_bits()),
            stop: AtomicBool::new(false),
            state: AtomicU8::new(PLAYING),
            heard: AtomicU64::new(data.from.max(0.0).to_bits()),
            changes: Mutex::new(Vec::with_capacity(4)),
        });
        let rate = f64::from(header.sample_rate);
        let frames = |seconds: f64| (seconds * rate).round() as u64;
        let mut sound = Self {
            bytes: data.bytes,
            decoding: Some(decoding),
            sample_rate: header.sample_rate,
            looping: data.looping,
            samples: Vec::new(),
            channels: 1,
            next: 0,
            window: [Frame::ZERO; 4],
            fraction: 0.0,
            tail: 0,
            position: 0,
            skip: 0,
            align: None,
            loop_start: frames(data.loop_start),
            loop_end: data.loop_end.map(frames),
            waiting: (data.start_time != StartTime::Immediate).then_some(data.start_time),
            volume: data.volume,
            target: data.volume,
            volume_rate: 0.0,
            asked: data.volume,
            panning: data.panning,
            fade: if data.fade_in.is_some() { 0.0 } else { 1.0 },
            fade_rate: data.fade_in.map_or(0.0, |s| 1.0 / s),
            stopping: false,
            pending: Vec::with_capacity(4),
            shared: shared.clone(),
        };
        let from = frames(data.from);
        if from > 0 {
            sound.seek(from);
        }
        for _ in 1..4 {
            sound.advance();
        }
        Ok((sound, ChunkedHandle { shared }))
    }

    /// Decoded samples held right now, in bytes: one packet's worth.
    #[cfg(test)]
    fn decoded_bytes(&self) -> usize {
        self.samples.capacity() * std::mem::size_of::<f32>()
    }

    /// Seconds into the sound being played: the window's current frame
    /// (three behind the next one read), plus how far towards the next.
    fn heard(&self) -> f64 {
        (self.position.saturating_sub(3) as f64 + self.fraction) / f64::from(self.sample_rate)
    }

    fn done(&self) -> bool {
        self.tail >= 3 || (self.stopping && self.fade <= 0.0)
    }

    /// Goes to source frame `frame`: a seek in the file where it can
    /// (decoding from a little before it and dropping the samples before
    /// it, found by the packets' timestamps), else decoding from the start.
    fn seek(&mut self, frame: u64) {
        self.samples.clear();
        self.next = 0;
        self.position = frame;
        self.skip = 0;
        self.align = None;
        let early = (frame as f64 / f64::from(self.sample_rate) - PREROLL_SECONDS).max(0.0);
        if frame > 0
            && let Some(d) = self.decoding.as_mut()
            && d.time_base.is_some()
            && let Some(time) = Time::try_from_secs_f64(early)
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                d.reader
                    .seek(
                        SeekMode::Accurate,
                        SeekTo::Time {
                            time,
                            track_id: Some(d.track),
                        },
                    )
                    .is_ok()
            }))
            .unwrap_or(false)
        {
            d.decoder.reset();
            self.align = Some(frame);
            return;
        }
        self.decoding = open(&self.bytes).ok().map(|(decoding, _)| decoding);
        self.skip = frame;
    }

    /// Moves the window on by one source frame.
    fn advance(&mut self) {
        let next = match self.next_frame() {
            Some(frame) => frame,
            None => {
                self.tail = self.tail.saturating_add(1);
                Frame::ZERO
            }
        };
        self.window = [self.window[1], self.window[2], self.window[3], next];
    }

    /// The next source frame, decoding a packet when the last one is used
    /// up; `None` at the end.
    fn next_frame(&mut self) -> Option<Frame> {
        let mut reopened = false;
        if self.looping && self.loop_end.is_some_and(|end| self.position >= end) {
            self.seek(self.loop_start);
        }
        while self.next * self.channels >= self.samples.len() || self.skip > 0 {
            if self.skip > 0 && self.next * self.channels < self.samples.len() {
                let left = self.samples.len() / self.channels - self.next;
                let dropped = left.min(usize::try_from(self.skip).unwrap_or(usize::MAX));
                self.next += dropped;
                self.skip -= dropped as u64;
                continue;
            }
            // A panic inside a decoder ends the sound instead of the game.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.read_packet(&mut reopened))) {
                Ok(Some(())) => {}
                Ok(None) => return None,
                Err(_) => {
                    self.decoding = None;
                    return None;
                }
            }
        }
        let at = self.next * self.channels;
        self.next += 1;
        self.position += 1;
        let left = self.samples[at];
        Some(if self.channels == 1 {
            Frame::from_mono(left)
        } else {
            Frame::new(left, self.samples[at + 1])
        })
    }

    /// Reads and decodes the next packet into `samples`; `None` when there
    /// is no decoder any more. `reopened` says whether the frame being
    /// looked for already went back to the loop start: it is never cleared
    /// here, since a packet that decodes to nothing, or to frames before a
    /// seek's target, is no progress (clearing it then let such a file loop
    /// back to its start forever, rebuilding the decoder each time).
    fn read_packet(&mut self, reopened: &mut bool) -> Option<()> {
        let decoding = self.decoding.as_mut()?;
        match decoding.reader.next_packet() {
            Ok(Some(packet)) if packet.track_id == decoding.track => match decoding.decoder.decode(&packet) {
                Ok(buffer) => {
                    self.channels = buffer.num_planes().max(1);
                    buffer.copy_to_vec_interleaved(&mut self.samples);
                    self.next = 0;
                    if let Some(target) = self.align {
                        let start = decoding.time_base.and_then(|tb| tb.calc_time(packet.pts));
                        let start = start.map(|t| (t.as_secs_f64() * f64::from(self.sample_rate)).round() as i64);
                        self.next = aligned(start, self.samples.len() / self.channels, target);
                        if self.next < self.samples.len() / self.channels {
                            self.align = None;
                        }
                    }
                }
                // A damaged packet: skip it, as players do.
                Err(DecodeError::DecodeError(_)) => {}
                Err(_) => self.end(reopened),
            },
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => self.end(reopened),
        }
        Some(())
    }

    /// The decoder reached the end: back to the loop start when looping
    /// (once per frame asked for, so a sound without samples can't loop
    /// forever), else stop decoding.
    fn end(&mut self, reopened: &mut bool) {
        if self.looping && !*reopened {
            *reopened = true;
            self.seek(self.loop_start);
        } else {
            self.decoding = None;
        }
    }

    /// Whether `at` has come, counting a block of `block` seconds: a delay
    /// is due within the block when `within` (the start, which is cut to
    /// the sample), else from the block after it ends (changes, as Kira
    /// times its own). `None`: it never will.
    fn due(at: &mut StartTime, block: f64, within: bool, info: &Info) -> Option<bool> {
        match at {
            StartTime::Immediate => Some(true),
            StartTime::Delayed(left) => {
                let due = if within {
                    left.as_secs_f64() <= block
                } else {
                    left.is_zero()
                };
                *left = left.saturating_sub(std::time::Duration::from_secs_f64(block));
                Some(due)
            }
            StartTime::ClockTime(time) => match info.when_to_start(*time) {
                WhenToStart::Now => Some(true),
                WhenToStart::Later => Some(false),
                WhenToStart::Never => None,
            },
        }
    }

    /// Applies the handle's changes that are due this block.
    fn take_changes(&mut self, block: f64, info: &Info) {
        let mut i = 0;
        while i < self.pending.len() {
            match Self::due(&mut self.pending[i].at, block, false, info) {
                Some(false) => i += 1,
                // A clock that went away: the change never comes.
                None => {
                    self.pending.remove(i);
                }
                Some(true) => match self.pending.remove(i).what {
                    What::Volume(volume, seconds) => {
                        self.target = volume;
                        self.volume_rate = (volume - self.volume).abs() / seconds as f32;
                    }
                    What::Stop(seconds) => self.begin_stop(seconds),
                },
            }
        }
    }

    fn begin_stop(&mut self, seconds: f64) {
        if !self.stopping {
            self.stopping = true;
            self.fade_rate = -self.fade.max(1e-3) / seconds;
            self.shared.state.store(STOPPING, Relaxed);
        }
    }
}

/// Where to start in a packet of `frames` decoded frames that begins at
/// source frame `start`, to begin at source frame `target`: `frames` (all
/// of it dropped) when it ends before the target, and 0 when it begins
/// after it or its time is unknown.
fn aligned(start: Option<i64>, frames: usize, target: u64) -> usize {
    let Some(start) = start else {
        return 0;
    };
    let into = i64::try_from(target).unwrap_or(i64::MAX).saturating_sub(start);
    usize::try_from(into).map_or(0, |into| into.min(frames))
}

impl Sound for ChunkedSound {
    fn on_start_processing(&mut self) {
        if let Ok(mut changes) = self.shared.changes.try_lock() {
            self.pending.append(&mut changes);
        }
    }

    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        let block = dt * out.len() as f64;
        if self.shared.stop.load(Relaxed) {
            self.begin_stop(STOP_FADE_SECONDS);
        }
        let mut out = out;
        if let Some(at) = &mut self.waiting {
            if self.stopping {
                // Stopped before it started.
                self.tail = 3;
            }
            let wait = match at {
                StartTime::Delayed(left) => left.as_secs_f64(),
                _ => 0.0,
            };
            match Self::due(at, block, true, info) {
                Some(true) => {
                    // A delay ends inside the block: silence up to it.
                    let silent = ((wait / dt) as usize).min(out.len());
                    out[..silent].fill(Frame::ZERO);
                    out = &mut out[silent..];
                    self.waiting = None;
                }
                Some(false) if !self.done() => {
                    out.fill(Frame::ZERO);
                    return;
                }
                _ => self.tail = 3,
            }
        }
        self.take_changes(block, info);
        let asked = f32::from_bits(self.shared.volume.load(Relaxed));
        if asked != self.asked {
            self.asked = asked;
            self.target = asked;
            self.volume_rate = (asked - self.volume).abs() / SMOOTHING_SECONDS as f32;
        }
        let panning = f32::from_bits(self.shared.panning.load(Relaxed));
        let step = f64::from_bits(self.shared.rate.load(Relaxed)) * f64::from(self.sample_rate) * dt;
        let smoothing = (dt / SMOOTHING_SECONDS).min(1.0) as f32;
        let volume_step = self.volume_rate * dt as f32;
        for frame in out {
            if self.done() {
                *frame = Frame::ZERO;
                continue;
            }
            self.volume = if self.volume < self.target {
                (self.volume + volume_step).min(self.target)
            } else {
                (self.volume - volume_step).max(self.target)
            };
            self.panning += (panning - self.panning) * smoothing;
            self.fade = (self.fade + self.fade_rate * dt).min(1.0);
            let [previous, current, next_1, next_2] = self.window;
            let sample = interpolate_frame(previous, current, next_1, next_2, self.fraction as f32);
            *frame = (sample * self.volume * self.fade.max(0.0) as f32).panned(Panning(self.panning));
            self.fraction += step;
            while self.fraction >= 1.0 && !self.done() {
                self.fraction -= 1.0;
                self.advance();
            }
        }
        self.shared.heard.store(self.heard().to_bits(), Relaxed);
        if self.done() {
            self.decoding = None;
            self.shared.state.store(STOPPED, Relaxed);
        }
    }

    fn finished(&self) -> bool {
        self.done()
    }
}

/// For the fuzz target `sound`: opens `bytes` as a stream and plays a
/// little of it, seeking a few times. Whatever the file says, it must be
/// an error or play without crashing.
#[cfg(fuzzing)]
pub(crate) fn fuzz_stream(bytes: &[u8]) {
    let bytes: Arc<[u8]> = bytes.into();
    let _ = header(&bytes);
    for (looping, from) in [(false, 0.0), (true, 0.5)] {
        let data = ChunkedData::new(bytes.clone(), 1.0, 1.0, looping).start_position(from);
        let Ok((mut sound, _handle)) = ChunkedSound::new(data) else {
            continue;
        };
        let info = kira::info::MockInfoBuilder::new().build();
        let mut block = [Frame::ZERO; 256];
        for _ in 0..64 {
            sound.on_start_processing();
            sound.process(&mut block, 1.0 / 44_100.0, &info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mono 16-bit WAV of `samples` at `rate` Hz.
    fn wav(rate: u32, samples: &[i16]) -> Arc<[u8]> {
        let data = samples.len() as u32 * 2;
        let mut out = Vec::from(&b"RIFF"[..]);
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&[1, 0, 1, 0]); // PCM, mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&[2, 0, 16, 0]);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out.into()
    }

    /// A ramp that never repeats within `n` samples, so frames are told apart.
    fn ramp(n: usize) -> Vec<i16> {
        (0..n).map(|i| ((i % 20_000) as i16) + 1).collect()
    }

    fn play(bytes: &Arc<[u8]>, looping: bool) -> (ChunkedSound, ChunkedHandle) {
        ChunkedSound::new(ChunkedData::new(bytes.clone(), 1.0, 1.0, looping)).unwrap()
    }

    fn run(sound: &mut ChunkedSound, frames: usize, dt: f64) -> Vec<Frame> {
        let info = kira::info::MockInfoBuilder::new().build();
        let mut out = vec![Frame::ZERO; frames];
        // In blocks, as the mixer asks.
        for block in out.chunks_mut(128) {
            sound.on_start_processing();
            sound.process(block, dt, &info);
        }
        out
    }

    #[test]
    fn chunked_stream_plays_the_samples_whole_decoding_gives() {
        let bytes = wav(8000, &ramp(5000));
        let whole = kira::sound::static_sound::StaticSoundData::from_cursor(Cursor::new(bytes.clone())).unwrap();
        let (mut sound, _handle) = play(&bytes, false);
        let out = run(&mut sound, 5000, 1.0 / 8000.0);
        assert_eq!(out.len(), whole.frames.len());
        for (i, (got, want)) in out.iter().zip(whole.frames.iter()).enumerate() {
            assert!((got.left - want.left).abs() < 1e-6, "frame {i}: {got:?} != {want:?}");
        }
    }

    #[test]
    fn chunked_stream_holds_one_packet_not_the_whole_track() {
        // A minute at 8 kHz: 480 000 frames, 3.84 MB decoded as Kira frames.
        let bytes = wav(8000, &ramp(480_000));
        let whole_bytes = 480_000 * std::mem::size_of::<Frame>();
        let (mut sound, _handle) = play(&bytes, false);
        run(&mut sound, 8000 * 10, 1.0 / 8000.0);
        assert!(!sound.finished());
        assert!(sound.decoded_bytes() > 0);
        assert!(
            sound.decoded_bytes() * 100 < whole_bytes,
            "{} bytes decoded at once",
            sound.decoded_bytes()
        );
    }

    #[test]
    fn chunked_stream_finishes_at_the_end_unless_it_loops() {
        let bytes = wav(8000, &ramp(100));
        let (mut once, handle) = play(&bytes, false);
        run(&mut once, 300, 1.0 / 8000.0);
        assert!(once.finished());
        assert_eq!(handle.state(), PlaybackState::Stopped);

        let (mut looping, handle) = play(&bytes, true);
        let out = run(&mut looping, 1000, 1.0 / 8000.0);
        assert!(!looping.finished());
        assert_eq!(handle.state(), PlaybackState::Playing);
        assert_eq!(out[0].left, out[100].left);
        assert_eq!(out[37].left, out[937].left);
    }

    #[test]
    fn a_looping_stream_whose_loop_start_is_past_its_end_ends_instead_of_looping_forever() {
        // Every packet after the loop start's seek is before its target:
        // no frame to play, so going back again is no progress.
        let bytes = wav(8000, &ramp(100));
        let data = ChunkedData::new(bytes, 1.0, 1.0, true).loop_points(1.0, None);
        let (mut sound, _) = ChunkedSound::new(data).unwrap();
        let out = run(&mut sound, 1000, 1.0 / 8000.0);
        assert!(out[..90].iter().any(|f| f.left != 0.0), "it plays once");
        assert!(sound.finished(), "then ends");
    }

    /// What the fuzz target `webm_audio` found in its first run: a video's
    /// soundtrack whose packets decode to no frames, played looping from
    /// half a second in, went back to the start forever, rebuilding the
    /// decoder each time.
    #[cfg(feature = "video")]
    #[test]
    fn a_looping_soundtrack_whose_packets_hold_no_frames_ends_instead_of_looping_forever() {
        let bytes: Arc<[u8]> = include_bytes!("../../tests/fixtures/video/no-frames-soundtrack.webm")
            .as_slice()
            .into();
        let started = std::time::Instant::now();
        let data = ChunkedData::new(bytes, 1.0, 1.0, true).start_position(0.5);
        let (mut sound, _) = ChunkedSound::new(data).expect("its header is fine");
        run(&mut sound, 1024, 1.0 / 44_100.0);
        assert!(sound.finished(), "it ends");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn chunked_stream_resamples_to_the_output_rate() {
        let bytes = wav(8000, &ramp(800));
        let (mut sound, _handle) = play(&bytes, false);
        // A 16 kHz output plays each source frame twice as long.
        let out = run(&mut sound, 1590, 1.0 / 16_000.0);
        assert!(!sound.finished());
        assert!((out[400].left - ramp(800)[200] as f32 / 32768.0).abs() < 1e-6);
        run(&mut sound, 20, 1.0 / 16_000.0);
        assert!(sound.finished());
    }

    #[test]
    fn stopping_a_chunked_stream_fades_out_then_ends() {
        let bytes = wav(8000, &ramp(8000));
        let (mut sound, mut handle) = play(&bytes, true);
        run(&mut sound, 128, 1.0 / 8000.0);
        handle.stop();
        // 5 ms into the 10 ms fade.
        let out = run(&mut sound, 40, 1.0 / 8000.0);
        assert_eq!(handle.state(), PlaybackState::Stopping);
        assert!(!sound.finished());
        assert!(out[39].left.abs() < out[0].left.abs());
        let out = run(&mut sound, 128, 1.0 / 8000.0);
        assert!(sound.finished());
        assert_eq!(handle.state(), PlaybackState::Stopped);
        assert_eq!(out[127], Frame::ZERO);
    }

    #[test]
    fn chunked_stream_follows_volume_changes_smoothly() {
        let bytes = wav(8000, &[16_384; 4000]);
        let (mut sound, mut handle) = play(&bytes, false);
        handle.set_volume(0.0);
        let out = run(&mut sound, 400, 1.0 / 8000.0);
        // No jump to silence on the first frame; under 1% within 50 ms.
        assert!(out[0].left > 0.4);
        assert!(out[399].left.abs() < out[0].left * 0.01);
    }

    #[test]
    fn chunked_stream_follows_panning_and_speed() {
        let bytes = wav(8000, &[16_384; 8000]);
        let (mut sound, mut handle) = play(&bytes, false);
        handle.set_panning(-1.0);
        let out = run(&mut sound, 400, 1.0 / 8000.0);
        assert!(out[399].right.abs() < out[399].left.abs() * 0.1, "{:?}", out[399]);
        // Twice as fast: the 7600 frames left take 3800.
        handle.set_playback_rate(2.0);
        run(&mut sound, 3700, 1.0 / 8000.0);
        assert!(!sound.finished());
        run(&mut sound, 200, 1.0 / 8000.0);
        assert!(sound.finished());
    }

    fn value(i: usize) -> f32 {
        ramp(i + 1)[i] as f32 / 32768.0
    }

    #[test]
    fn chunked_stream_waits_for_its_start_time() {
        let bytes = wav(8000, &ramp(4000));
        let data = ChunkedData::new(bytes, 1.0, 1.0, false)
            .start_time(StartTime::Delayed(std::time::Duration::from_millis(50)));
        let (mut sound, _handle) = ChunkedSound::new(data).unwrap();
        let out = run(&mut sound, 800, 1.0 / 8000.0);
        // 50 ms at 8 kHz: 400 frames of silence, then the sound from its start.
        assert!(out[..400].iter().all(|f| *f == Frame::ZERO));
        assert!((out[400].left - value(0)).abs() < 1e-6, "{:?}", out[400]);
        assert!((out[500].left - value(100)).abs() < 1e-6);
    }

    #[test]
    fn chunked_stream_starts_at_its_panning() {
        let bytes = wav(8000, &[16_384; 800]);
        let (mut sound, _handle) = ChunkedSound::new(ChunkedData::new(bytes, 1.0, 1.0, false).panning(1.0)).unwrap();
        let out = run(&mut sound, 1, 1.0 / 8000.0);
        assert!(out[0].left.abs() < 1e-3 && out[0].right > 0.4, "{:?}", out[0]);
    }

    #[test]
    fn chunked_stream_starts_where_asked_and_loops_between_its_points() {
        let bytes = wav(8000, &ramp(8000));
        // Starts 0.1 s in; the loop turns at 0.5 s back to 0.25 s.
        let data = ChunkedData::new(bytes, 1.0, 1.0, true)
            .start_position(0.1)
            .loop_points(0.25, Some(0.5));
        let (mut sound, _handle) = ChunkedSound::new(data).unwrap();
        let out = run(&mut sound, 6000, 1.0 / 8000.0);
        assert!((out[0].left - value(800)).abs() < 1e-6, "{:?}", out[0]);
        // Frame 3199 of the sound is the last before the turn at 4000...
        assert!((out[3199].left - value(3999)).abs() < 1e-6);
        // ...then 2000 again, and each 2000 frames after.
        assert!((out[3200].left - value(2000)).abs() < 1e-6, "{:?}", out[3200]);
        assert!((out[5200].left - value(2000)).abs() < 1e-6);
    }

    #[test]
    fn chunked_stream_reports_the_position_it_has_played_to() {
        let bytes = wav(8000, &ramp(8000));
        let data = ChunkedData::new(bytes, 1.0, 1.0, false).start_position(0.25);
        let (mut sound, handle) = ChunkedSound::new(data).unwrap();
        // Before the mixer plays it: where it starts.
        assert!((handle.position() - 0.25).abs() < 1e-9, "{}", handle.position());
        let out = run(&mut sound, 800, 1.0 / 8000.0);
        // The last frame played is sound frame 2000 + 799; the next is 2800.
        assert!((out[799].left - value(2799)).abs() < 1e-6, "{:?}", out[799]);
        assert!(
            (handle.position() - 2800.0 / 8000.0).abs() < 1e-9,
            "{}",
            handle.position()
        );
        // At another speed the position moves with the sound, not the mixer.
        let mut handle = handle;
        handle.set_playback_rate(2.0);
        run(&mut sound, 800, 1.0 / 8000.0);
        assert!(
            (handle.position() - 4400.0 / 8000.0).abs() < 1e-3,
            "{}",
            handle.position()
        );
    }

    #[test]
    fn chunked_stream_fades_in_and_tweens_its_volume_from_a_start_time() {
        let bytes = wav(8000, &[16_384; 8000]);
        let tween = |ms| Tween {
            duration: std::time::Duration::from_millis(ms),
            ..Default::default()
        };
        let data = ChunkedData::new(bytes, 1.0, 1.0, false).fade_in(Some(tween(100)));
        let (mut sound, mut handle) = ChunkedSound::new(data).unwrap();
        let out = run(&mut sound, 1200, 1.0 / 8000.0);
        // Halfway through the 100 ms fade in, then full.
        assert!((out[400].left - 0.25).abs() < 0.01, "{:?}", out[400]);
        assert!((out[1000].left - 0.5).abs() < 1e-3);
        // Down to a fifth over 100 ms, 50 ms from now.
        handle.set_volume_with(
            0.2,
            Tween {
                start_time: StartTime::Delayed(std::time::Duration::from_millis(48)),
                ..tween(100)
            },
        );
        let out = run(&mut sound, 1600, 1.0 / 8000.0);
        assert!((out[300].left - 0.5).abs() < 1e-3, "not yet: {:?}", out[300]);
        assert!((out[800].left - 0.3).abs() < 0.02, "halfway: {:?}", out[800]);
        assert!((out[1500].left - 0.1).abs() < 1e-3);
    }

    #[test]
    fn chunked_stream_stops_over_a_tween_when_its_time_comes() {
        let bytes = wav(8000, &[16_384; 8000]);
        let (mut sound, mut handle) = play(&bytes, true);
        handle.stop_with(Tween {
            start_time: StartTime::Delayed(std::time::Duration::from_millis(16)),
            duration: std::time::Duration::from_millis(100),
            ..Default::default()
        });
        let out = run(&mut sound, 512, 1.0 / 8000.0);
        assert!((out[100].left - 0.5).abs() < 1e-3, "not yet");
        assert!((out[511].left - 0.25).abs() < 0.02, "halfway: {:?}", out[511]);
        assert_eq!(handle.state(), PlaybackState::Stopping);
        run(&mut sound, 512, 1.0 / 8000.0);
        assert!(sound.finished());
        assert_eq!(handle.state(), PlaybackState::Stopped);
    }

    #[test]
    fn chunked_header_reads_the_length_and_rejects_garbage() {
        let header = header(&wav(8000, &ramp(4000))).unwrap();
        assert_eq!(header.sample_rate, 8000);
        assert!((header.seconds() - 0.5).abs() < 1e-6);
        assert!(super::header(&Arc::from(&b"not a sound"[..])).is_err());
        assert!(ChunkedSound::new(ChunkedData::new(Arc::from(&b""[..]), 1.0, 1.0, true)).is_err());
    }
}
