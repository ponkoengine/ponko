//! [`Video`]: a video file played into a texture, its sound through the
//! mixer (feature `video`).
//!
//! **What lives here:** loading a WebM file with AV1 pictures and Vorbis
//! sound ([`Video::load`], [`Video::from_bytes`]), playing, pausing,
//! seeking and looping it, and the [`Texture`] it shows, to draw as a
//! sprite, a full-screen intro or the material of an in-world screen.
//!
//! **When to call it:** load once (in setup or a loading state), keep the
//! `Video` (a resource or a local), call [`Video::update`] once a frame,
//! draw [`Video::texture`]. [`Video::free`] when done.
//!
//! **What it needs:** the `video` feature (off by default). Pictures are
//! decoded on the CPU by rav1d, the Rust port of the dav1d AV1 decoder,
//! and copied into an ordinary texture; the sound plays on an audio bus
//! like any other sound (silent without the `audio` feature or in a
//! headless run without an offline output). Desktop only: the web build
//! compiles the API, and loading reports
//! [`ErrorKind::Unsupported`].
//! The guide's "Video" chapter has the encoding settings and the reasons.

#![deny(missing_docs)]

#[cfg(not(target_arch = "wasm32"))]
mod av1;
mod demux;
#[cfg(not(target_arch = "wasm32"))]
mod yuv;

use std::sync::Arc;

use crate::audio::{Audio, AudioBus, Playing, Sound, SoundStarted};
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};
use crate::texture::Texture;

/// The browser build has no decoder: the type exists so the code that
/// holds one compiles, and none is ever made.
#[cfg(target_arch = "wasm32")]
enum Av1 {}

#[cfg(not(target_arch = "wasm32"))]
use av1::Av1;

/// How far apart, in seconds, the picture and the sound may drift before
/// the one behind catches up: more than a mixer block or two, less than
/// the eye notices.
const SYNC_SECONDS: f64 = 0.1;

/// The Matroska codec id of AV1 pictures.
const AV1: &str = "V_AV1";
/// The Matroska codec id of Vorbis sound.
const VORBIS: &str = "A_VORBIS";

/// A video being played into a texture, with its sound on a mixer bus.
///
/// A WebM file with AV1 pictures (8-bit, any chroma layout) and, if it
/// has sound, Vorbis audio. It starts paused on its first frame; call
/// [`play`](Self::play), then [`update`](Self::update) every frame and draw
/// [`texture`](Self::texture).
///
/// ```no_run
/// use ponko::prelude::*;
/// use ponko::video::Video;
///
/// struct Intro(Video);
///
/// fn play_intro(ctx: &mut Ctx) {
///     let Some(mut intro) = ctx.world.remove_resource::<Intro>() else { return };
///     if intro.0.update(ctx).is_err() || intro.0.finished() {
///         intro.0.free(ctx); // on to the menu
///         return;
///     }
///     let center = intro.0.size().as_vec2() / 2.0;
///     ctx.gfx.draw_ex(intro.0.texture(), center, DrawParams::default().with_space(DrawSpace::Screen));
///     ctx.world.insert_resource(intro);
/// }
///
/// Game::new("Intro", 1280, 720)
///     .setup(|ctx| {
///         let mut intro = Video::load(ctx, "intro.webm").expect("the intro video");
///         intro.play(ctx);
///         ctx.world.insert_resource(Intro(intro));
///     })
///     .system(play_intro)
///     .run();
/// ```
pub struct Video {
    texture: Texture,
    /// The soundtrack, if the file has sound.
    sound: Option<Sound>,
    /// Its voice while it plays.
    voice: Option<Playing>,
    bus: AudioBus,
    volume: f32,
    info: demux::Info,
    demuxer: demux::Demuxer,
    decoder: Av1,
    /// The frame shown, as RGBA.
    rgba: Vec<u8>,
    /// Seconds into the video.
    time: f64,
    /// Index of the frame shown, if one is.
    shown: Option<usize>,
    /// Index of the frame the demuxer reads next.
    next: usize,
    playing: bool,
    looping: bool,
    finished: bool,
}

impl std::fmt::Debug for Video {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Video")
            .field("texture", &self.texture)
            .field("sound", &self.sound)
            .field("seconds", &self.info.seconds)
            .field("time", &self.time)
            .field("playing", &self.playing)
            .field("looping", &self.looping)
            .finish_non_exhaustive()
    }
}

impl Video {
    /// Loads a video from an [added asset source](crate::assets::Assets::add_source)
    /// or the `assets/` folder and shows its first frame, paused.
    ///
    /// The file is kept in memory, encoded; one frame is decoded at a time.
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`] or [`Io`](ErrorKind::Io) if it can't be
    /// read, and the errors of [`from_bytes`](Self::from_bytes).
    pub fn load(ctx: &mut Ctx, path: &str) -> Result<Self, Error> {
        let bytes = ctx.assets.load_with(path, |bytes| Ok(bytes.to_vec()))?;
        Self::from_bytes(ctx, bytes).map_err(|e| e.with_path(path))
    }

    /// A video from the bytes of a WebM file, showing its first frame,
    /// paused.
    ///
    /// # Errors
    /// [`ErrorKind::Decode`] if it is not a WebM (Matroska) file or its
    /// first frame doesn't decode; [`ErrorKind::Unsupported`] if its
    /// pictures are not AV1, are more than 8 bits deep, its sound is not
    /// Vorbis, or this is the web build (which has no decoder: see the
    /// guide's "Video" chapter).
    pub fn from_bytes(ctx: &mut Ctx, bytes: Vec<u8>) -> Result<Self, Error> {
        let bytes: Arc<[u8]> = bytes.into();
        let (demuxer, info) = demux::Demuxer::new(bytes.clone()).map_err(|e| Error::new(ErrorKind::Decode, e))?;
        if info.codec != AV1 {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!(
                    "its pictures are {}; ponko plays AV1 (V_AV1). Encode with ffmpeg -c:v libsvtav1",
                    info.codec
                ),
            ));
        }
        if let Some(codec) = info.audio_codec.as_deref().filter(|c| *c != VORBIS) {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!(
                    "its sound is {codec}; ponko plays Vorbis (A_VORBIS) in videos. \
                     Encode with ffmpeg -c:a libvorbis, or -an for none"
                ),
            ));
        }
        let decoder = new_decoder()?;
        let (width, height) = (info.width, info.height);
        let rgba = vec![0u8; width as usize * height as usize * 4];
        let sound = match info.audio_codec {
            Some(_) => Some(
                ctx.audio
                    .add_soundtrack(bytes, info.seconds as f32)
                    .map_err(|e| Error::new(ErrorKind::Decode, e))?,
            ),
            None => None,
        };
        let texture = ctx.gfx.create_texture(width, height, &rgba);
        let mut video = Self {
            texture,
            sound,
            voice: None,
            bus: AudioBus::default(),
            volume: 1.0,
            info,
            demuxer,
            decoder,
            rgba,
            time: 0.0,
            shown: None,
            next: 0,
            playing: false,
            looping: false,
            finished: false,
        };
        if let Err(e) = video.show(ctx, 0.0) {
            video.free(ctx);
            return Err(e);
        }
        Ok(video)
    }

    /// The texture the current frame is in: draw it like any texture, or
    /// use it as a material's texture for a screen in the world.
    #[must_use]
    pub fn texture(&self) -> Texture {
        self.texture
    }

    /// Width and height in pixels.
    #[must_use]
    pub fn size(&self) -> crate::math::UVec2 {
        crate::math::UVec2::new(self.info.width, self.info.height)
    }

    /// Length in seconds.
    #[must_use]
    pub fn seconds(&self) -> f32 {
        self.info.seconds as f32
    }

    /// Seconds into the video: where it is playing or paused.
    #[must_use]
    pub fn time(&self) -> f32 {
        self.time as f32
    }

    /// The time of the frame shown, in seconds: the frame whose time is the
    /// last at or before [`time`](Self::time).
    #[must_use]
    pub fn frame_time(&self) -> f32 {
        self.shown.map_or(0.0, |i| self.info.frames[i].0 as f32)
    }

    /// The frame shown, as RGBA pixels (4 bytes per pixel, row by row, top
    /// row first): the same pixels as the texture, for tests and tools.
    #[must_use]
    pub fn frame_rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The soundtrack, if the file has sound: a sound like any other, for
    /// [`Audio::started`] and the inventory. Played by the video itself.
    #[must_use]
    pub fn sound(&self) -> Option<Sound> {
        self.sound
    }

    /// Whether it is playing (not paused and not at its end).
    #[must_use]
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Whether it played to its end (never, while looping). Playing it
    /// again starts over.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Whether it starts over at its end.
    #[must_use]
    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Starts over at its end, for screens in the world. Default `false`.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
        if looping {
            self.finished = false;
        }
    }

    /// The bus its sound plays on (the default bus, `sfx`, unless set).
    #[must_use]
    pub fn bus(&self) -> AudioBus {
        self.bus
    }

    /// Plays its sound on `bus` from now on, for example a `"cutscene"`
    /// bus with its own volume slider. A playing sound moves at once.
    pub fn set_bus(&mut self, ctx: &mut Ctx, bus: AudioBus) {
        self.bus = bus;
        if self.voice.is_some() {
            self.stop_sound(&mut ctx.audio);
            self.start_sound(&mut ctx.audio);
        }
    }

    /// Its sound's volume: 0 silent, 1 (default) as recorded.
    #[must_use]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Changes its sound's volume (0 silent, 1 as recorded), also while it
    /// plays.
    pub fn set_volume(&mut self, ctx: &mut Ctx, volume: f32) {
        self.volume = if volume.is_finite() { volume.max(0.0) } else { 1.0 };
        if let Some(voice) = self.voice {
            ctx.audio.set_volume(voice, self.volume, 0.0);
        }
    }

    /// Plays from where it is; from the start if it had finished.
    pub fn play(&mut self, ctx: &mut Ctx) {
        if self.playing {
            return;
        }
        if self.finished {
            self.finished = false;
            // The first frame decodes now, or the old frame stays up; the
            // next update reports a bad file.
            let _ = self.seek_to(ctx, 0.0);
        }
        self.playing = true;
        self.start_sound(&mut ctx.audio);
    }

    /// Pauses: the frame stays and the sound stops; [`play`](Self::play)
    /// goes on from here.
    pub fn pause(&mut self, ctx: &mut Ctx) {
        self.playing = false;
        self.stop_sound(&mut ctx.audio);
    }

    /// Jumps to `seconds` (clamped to the video), playing or paused: the
    /// frame at that time is decoded from the keyframe before it and
    /// shown, and the sound goes on from there.
    ///
    /// # Errors
    /// [`ErrorKind::Decode`] if a frame on the way doesn't decode.
    pub fn seek(&mut self, ctx: &mut Ctx, seconds: f32) -> Result<(), Error> {
        self.finished = false;
        self.seek_to(ctx, f64::from(seconds))?;
        if self.playing {
            self.stop_sound(&mut ctx.audio);
            self.start_sound(&mut ctx.audio);
        }
        Ok(())
    }

    /// Moves the video on by this frame's time ([`Time::frame_dt`]) if it
    /// is playing, and shows the frame for the new time. Call it once a
    /// frame, from a frame system.
    ///
    /// The picture follows the frame clock and the sound the audio
    /// device's; both run in real time. When they drift more than a tenth
    /// of a second apart, the one behind catches up: after frames that
    /// didn't run while the sound played on (a load that blocked, a game
    /// stopped in the debugger, a browser tab in the background) the
    /// picture skips ahead to the sound, and a sound that started late
    /// (a browser waiting for the first click) starts again at the
    /// picture's time.
    ///
    /// [`Time::frame_dt`]: crate::time::Time::frame_dt
    ///
    /// # Errors
    /// [`ErrorKind::Decode`] if a frame doesn't decode (the last good
    /// frame stays shown).
    pub fn update(&mut self, ctx: &mut Ctx) -> Result<(), Error> {
        let dt = ctx.time.frame_dt();
        self.advance(ctx, dt)?;
        self.follow_sound(ctx)
    }

    /// [`update`](Self::update) by `seconds` instead of the frame's time:
    /// for playing at another speed (the sound doesn't follow), or for
    /// stepping a video in a test.
    ///
    /// # Errors
    /// As [`update`](Self::update).
    pub fn advance(&mut self, ctx: &mut Ctx, seconds: f32) -> Result<(), Error> {
        if !self.playing || !seconds.is_finite() || seconds <= 0.0 {
            return Ok(());
        }
        let length = self.info.seconds;
        let mut time = self.time + f64::from(seconds);
        if time >= length {
            if self.looping && length > 0.0 {
                time %= length;
                self.seek_to(ctx, time)?;
                self.stop_sound(&mut ctx.audio);
                self.start_sound(&mut ctx.audio);
                return Ok(());
            }
            time = length;
            self.playing = false;
            self.finished = true;
            self.voice = None;
        }
        self.time = time;
        self.show(ctx, time)
    }

    /// Brings the picture and the sound together again when they are more
    /// than [`SYNC_SECONDS`] apart (see [`update`](Self::update)).
    fn follow_sound(&mut self, ctx: &mut Ctx) -> Result<(), Error> {
        let (true, Some(voice)) = (self.playing, self.voice) else {
            return Ok(());
        };
        let Some(heard) = ctx.audio.soundtrack_position(voice) else {
            return Ok(());
        };
        let drift = heard - self.time;
        if drift > SYNC_SECONDS {
            // The sound played on without the picture: skip ahead to it.
            self.advance(ctx, drift as f32)
        } else if drift < -SYNC_SECONDS {
            // The sound is late: it starts again where the picture is.
            self.stop_sound(&mut ctx.audio);
            self.start_sound(&mut ctx.audio);
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Frees its texture and sound, and stops the sound.
    pub fn free(mut self, ctx: &mut Ctx) {
        self.stop_sound(&mut ctx.audio);
        if let Some(sound) = self.sound {
            ctx.audio.free(sound);
        }
        ctx.gfx.free(self.texture);
    }

    /// Goes to `seconds` (clamped), decoding from the keyframe before it.
    fn seek_to(&mut self, ctx: &mut Ctx, seconds: f64) -> Result<(), Error> {
        let time = seconds.clamp(0.0, self.info.seconds);
        self.time = time;
        let target = self.frame_at(time);
        let keyframe = self.keyframe_at_or_before(target);
        self.restart_at(keyframe)?;
        self.show(ctx, time)
    }

    /// Index of the frame to show at `time`: the last whose time is at or
    /// before it (the first frame before that). WebM times are whole
    /// milliseconds, while a game's clock adds up `f32` steps that land a
    /// hair short of them (1.8 + 0.5 gives 2.2999999), so a frame starts
    /// a tenth of a millisecond early.
    fn frame_at(&self, time: f64) -> usize {
        const EARLY: f64 = 1e-4;
        self.info
            .frames
            .partition_point(|f| f.0 <= time + EARLY)
            .saturating_sub(1)
    }

    fn keyframe_at_or_before(&self, index: usize) -> usize {
        (0..=index).rev().find(|&i| self.info.frames[i].1).unwrap_or(0)
    }

    /// The demuxer and decoder start over at keyframe `index`.
    fn restart_at(&mut self, index: usize) -> Result<(), Error> {
        // Forward: read on from where the demuxer is; back: from the start.
        let skip = if index >= self.next {
            index - self.next
        } else {
            self.demuxer.restart().map_err(|e| Error::new(ErrorKind::Decode, e))?;
            index
        };
        self.demuxer.skip(skip).map_err(|e| Error::new(ErrorKind::Decode, e))?;
        flush(&mut self.decoder);
        // A flushed decoder has forgotten the sequence header, which only
        // the first frame carries.
        if index > 0 && !self.info.sequence_header.is_empty() {
            decode(&mut self.decoder, &self.info.sequence_header, index, None)?;
        }
        self.next = index;
        self.shown = None;
        Ok(())
    }

    /// Decodes up to the frame for `time` and puts it in the texture.
    fn show(&mut self, ctx: &mut Ctx, time: f64) -> Result<(), Error> {
        let target = self.frame_at(time);
        if self.shown == Some(target) {
            return Ok(());
        }
        // Behind (looped or sought back) or far ahead: decoding starts
        // again at the last keyframe on the way.
        let keyframe = self.keyframe_at_or_before(target);
        if target < self.next || keyframe > self.next {
            self.restart_at(keyframe)?;
        }
        let mut decoded = false;
        while self.next <= target {
            let index = self.next;
            let frame = self.demuxer.next().map_err(|e| Error::new(ErrorKind::Decode, e))?;
            self.next += 1;
            let Some((_, _, data)) = frame else {
                break;
            };
            // Only the target frame is converted to pixels.
            let want = index == target;
            if decode(&mut self.decoder, data, index, want.then_some(&mut self.rgba[..]))? {
                decoded = true;
            }
        }
        self.shown = Some(target);
        if decoded {
            ctx.gfx.update_texture(self.texture, &self.rgba);
        }
        Ok(())
    }

    fn start_sound(&mut self, audio: &mut Audio) {
        let Some(sound) = self.sound else {
            return;
        };
        let voice = audio.play_soundtrack(sound, self.bus, self.time);
        if self.volume != 1.0 {
            audio.set_volume(voice, self.volume, 0.0);
        }
        self.voice = Some(voice);
    }

    fn stop_sound(&mut self, audio: &mut Audio) {
        if let Some(voice) = self.voice.take() {
            audio.stop(voice);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn new_decoder() -> Result<Av1, Error> {
    // A few threads: enough for 1080p, and leave the game its cores.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    Av1::new(threads).map_err(|e| Error::new(ErrorKind::Decode, e))
}

#[cfg(target_arch = "wasm32")]
fn new_decoder() -> Result<Av1, Error> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "videos don't play in the web build (no AV1 decoder for wasm32): see the guide's \"Video\" chapter",
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn flush(decoder: &mut Av1) {
    decoder.flush();
}

#[cfg(target_arch = "wasm32")]
fn flush(decoder: &mut Av1) {
    match *decoder {}
}

/// Decodes frame `index`; with `rgba`, writes the picture it shows there.
/// Whether it wrote a picture.
#[cfg(not(target_arch = "wasm32"))]
fn decode(decoder: &mut Av1, data: &[u8], index: usize, rgba: Option<&mut [u8]>) -> Result<bool, Error> {
    let picture = decoder
        .decode(data, index as i64)
        .map_err(|e| Error::new(ErrorKind::Decode, format!("frame {index}: {e}")))?;
    let (Some(rgba), Some(picture)) = (rgba, picture) else {
        return Ok(false);
    };
    let planes = picture.planes().ok_or_else(|| {
        Error::new(
            ErrorKind::Unsupported,
            "its pictures are more than 8 bits deep; encode with ffmpeg -pix_fmt yuv420p",
        )
    })?;
    if planes.width * planes.height * 4 != rgba.len() {
        return Err(Error::new(
            ErrorKind::Unsupported,
            format!(
                "frame {index} is {} x {}, not the size the file declares; ponko plays videos of one size",
                planes.width, planes.height
            ),
        ));
    }
    yuv::to_rgba(&planes, rgba);
    Ok(true)
}

#[cfg(target_arch = "wasm32")]
fn decode(decoder: &mut Av1, _data: &[u8], _index: usize, _rgba: Option<&mut [u8]>) -> Result<bool, Error> {
    match *decoder {}
}

impl Audio {
    /// Keeps a video's encoded `bytes` as its soundtrack, `seconds` long.
    pub(crate) fn add_soundtrack(&mut self, bytes: Arc<[u8]>, seconds: f32) -> Result<Sound, String> {
        let id = self.backend.soundtrack(bytes, seconds)?;
        self.sounds_live.insert(id);
        self.ledger.made(crate::inventory::AssetClass::Sound, id);
        Ok(Sound::new(id, seconds))
    }

    /// Seconds into soundtrack voice `playing` the mixer has played up to:
    /// the clock the picture follows. `None` when nothing is mixed (a
    /// headless run without an offline output, no `audio` feature) or the
    /// voice isn't heard yet.
    pub(crate) fn soundtrack_position(&self, playing: Playing) -> Option<f64> {
        if self.silent_output() {
            return None;
        }
        self.backend.soundtrack_position(playing.0)
    }

    /// Plays soundtrack `sound` on `bus` from `from` seconds in. Like
    /// music, it is never made virtual.
    pub(crate) fn play_soundtrack(&mut self, sound: Sound, bus: AudioBus, from: f64) -> Playing {
        let playing = if self.silent_output() {
            self.silent += 1;
            Playing(self.silent)
        } else {
            Playing(self.backend.play_soundtrack(sound.id, bus, from))
        };
        self.started.push(SoundStarted {
            playing,
            sound,
            bus,
            volume: 1.0,
            speed: 1.0,
            looping: false,
        });
        playing
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const CLIP: &[u8] = include_bytes!("../tests/fixtures/video/clip.webm");

    #[test]
    fn demuxer_lists_every_frame_with_keyframes_every_half_second() {
        let (_, info) = demux::Demuxer::new(CLIP.into()).unwrap();
        assert_eq!((info.width, info.height, info.codec.as_str()), (64, 64, AV1));
        assert_eq!(info.audio_codec.as_deref(), Some(VORBIS));
        assert_eq!(info.frames.len(), 20);
        let keyframes: Vec<usize> = (0..20).filter(|&i| info.frames[i].1).collect();
        assert_eq!(keyframes, [0, 5, 10, 15]);
        assert!((info.frames[7].0 - 0.7).abs() < 1e-6);
        assert!((info.seconds - 2.0).abs() < 0.05, "{}", info.seconds);
    }

    #[test]
    fn demuxer_skips_to_the_asked_frame() {
        let (mut demuxer, info) = demux::Demuxer::new(CLIP.into()).unwrap();
        for index in [15, 5, 0, 10] {
            demuxer.restart().unwrap();
            demuxer.skip(index).unwrap();
            let (at, keyframe, _) = demuxer.next().unwrap().unwrap();
            assert_eq!((at, keyframe), info.frames[index]);
        }
    }

    #[test]
    fn decoder_shows_one_picture_per_frame_with_its_timestamp() {
        let (mut demuxer, _) = demux::Demuxer::new(CLIP.into()).unwrap();
        let mut decoder = Av1::new(2).unwrap();
        let mut index = 0;
        while let Some((_, _, data)) = demuxer.next().unwrap() {
            let picture = decoder.decode(data, index).unwrap().expect("a picture per frame");
            assert_eq!(picture.timestamp(), index);
            let planes = picture.planes().unwrap();
            assert_eq!(
                (planes.width, planes.height, planes.subsampling),
                (64, 64, yuv::Subsampling::Both)
            );
            assert_eq!(planes.matrix, yuv::Matrix::Bt709);
            assert!(!planes.full_range);
            index += 1;
        }
        assert_eq!(index, 20);
    }

    #[test]
    fn decoder_rejects_data_that_is_not_av1() {
        let mut decoder = Av1::new(1).unwrap();
        assert!(decoder.decode(&[0xff; 64], 0).is_err());
        // It still decodes a real file afterwards, from a keyframe.
        decoder.flush();
        let (mut demuxer, _) = demux::Demuxer::new(CLIP.into()).unwrap();
        let (_, _, data) = demuxer.next().unwrap().unwrap();
        assert!(decoder.decode(data, 0).unwrap().is_some());
    }
}
