//! Video in the browser: the pictures are decoded by the browser's own AV1
//! decoder (WebCodecs' `VideoDecoder`), behind the same [`Video`] API as
//! on the desktop.
//!
//! The demuxer, the clock, seeking, looping and the sound are the
//! desktop's; only the decoder differs. The browser decodes on its own
//! threads and hands each frame back later (a callback between two of the
//! game's frames), so this side sends frames a few ahead of the one due,
//! keeps the decoded ones until their time comes, and shows the newest
//! one due. Each frame shown is drawn into an `OffscreenCanvas` and read
//! back as RGBA (`VideoFrame.copyTo` would give the pixels only through a
//! promise), then put in the texture like any other pixels.
//!
//! The browser's objects live in a registry on the page's thread, as the
//! WebSocket transport's do: a [`WebAv1`] is only an id, so a `Video`
//! stays `Send` and `Sync` like every resource.
//!
//! [`Video`]: super::Video

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use js_sys::{Object, Reflect, Uint8Array, Uint8ClampedArray};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::demux::Info;
use super::{OnceLock, Video};
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};

/// Frames sent ahead of the one due, so the next ones are decoded by the
/// time they are: a few frames' worth, few enough that the browser's
/// decoder never runs out of the frames it lends.
const AHEAD: usize = 3;

#[wasm_bindgen]
extern "C" {
    type VideoDecoder;
    #[wasm_bindgen(constructor, catch)]
    fn new(init: &Object) -> Result<VideoDecoder, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn configure(this: &VideoDecoder, config: &Object) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn decode(this: &VideoDecoder, chunk: &EncodedVideoChunk) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn reset(this: &VideoDecoder) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn close(this: &VideoDecoder) -> Result<(), JsValue>;
    #[wasm_bindgen(method, getter, js_name = decodeQueueSize)]
    fn decode_queue_size(this: &VideoDecoder) -> u32;

    type EncodedVideoChunk;
    #[wasm_bindgen(constructor, catch)]
    fn new(init: &Object) -> Result<EncodedVideoChunk, JsValue>;

    type VideoFrame;
    #[wasm_bindgen(method, getter)]
    fn timestamp(this: &VideoFrame) -> f64;
    #[wasm_bindgen(method)]
    fn close(this: &VideoFrame);

    type OffscreenCanvas;
    #[wasm_bindgen(constructor, catch)]
    fn new(width: u32, height: u32) -> Result<OffscreenCanvas, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getContext)]
    fn get_context(this: &OffscreenCanvas, kind: &str, options: &Object) -> Result<JsValue, JsValue>;

    type Context2d;
    #[wasm_bindgen(method, catch, js_name = drawImage)]
    fn draw_frame(this: &Context2d, frame: &VideoFrame, x: f64, y: f64, width: f64, height: f64)
    -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = getImageData)]
    fn get_image_data(this: &Context2d, x: f64, y: f64, width: f64, height: f64) -> Result<ImageData, JsValue>;

    type ImageData;
    #[wasm_bindgen(method, getter)]
    fn data(this: &ImageData) -> Uint8ClampedArray;
}

/// What the browser has handed back and not yet been shown or dropped.
#[derive(Default)]
struct Ready {
    /// Decoded frames, by frame index (their chunk's timestamp).
    frames: Vec<(usize, VideoFrame)>,
    /// Frames before this index are of no use any more: closed on arrival.
    floor: usize,
    /// The decoder's error, if it stopped.
    error: Option<String>,
}

impl Ready {
    fn clear(&mut self) {
        for (_, frame) in self.frames.drain(..) {
            frame.close();
        }
    }
}

/// One video's browser objects.
struct Entry {
    decoder: VideoDecoder,
    config: Object,
    ready: Rc<RefCell<Ready>>,
    canvas: Context2d,
    /// The next chunk sent is the first since the decoder was configured:
    /// a key chunk, which must carry the sequence header.
    first: bool,
    _canvas: OffscreenCanvas,
    _output: Closure<dyn FnMut(VideoFrame)>,
    _error: Closure<dyn FnMut(JsValue)>,
}

thread_local! {
    static DECODERS: RefCell<BTreeMap<u32, Entry>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT_ID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// A browser AV1 decoder: an id into the page's registry.
pub(crate) struct WebAv1 {
    id: u32,
}

fn js_error(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| Reflect::get(e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

fn set(object: &Object, key: &str, value: impl Into<JsValue>) {
    // Setting a property of a plain object can't fail.
    let _ = Reflect::set(object, &key.into(), &value.into());
}

/// The WebCodecs codec string for an AV1 track (`av01.P.LLT.DD`): profile,
/// level and tier from the file's `av1C` record, else the main profile at
/// level 4.0 (the browser decodes any level its profile allows).
pub(crate) fn codec_string(config: Option<[u8; 4]>) -> String {
    let Some([_, b1, b2, _]) = config else {
        return "av01.0.08M.08".into();
    };
    let profile = b1 >> 5;
    let level = b1 & 0x1f;
    let tier = if b2 & 0x80 != 0 { 'H' } else { 'M' };
    let depth = match (b2 & 0x40 != 0, b2 & 0x20 != 0) {
        (true, true) => 12,
        (true, false) => 10,
        _ => 8,
    };
    format!("av01.{profile}.{level:02}{tier}.{depth:02}")
}

impl WebAv1 {
    /// A decoder for the video `info` describes.
    ///
    /// # Errors
    /// [`ErrorKind::Unsupported`] if the browser has no WebCodecs (or no
    /// `OffscreenCanvas`), or turns the codec down at once.
    pub fn new(info: &Info) -> Result<Self, Error> {
        let unsupported = |what: String| {
            Error::new(
                ErrorKind::Unsupported,
                format!("this browser can't decode the video: {what}"),
            )
        };
        let global = js_sys::global();
        for api in ["VideoDecoder", "EncodedVideoChunk", "OffscreenCanvas"] {
            if !Reflect::has(&global, &api.into()).unwrap_or(false) {
                return Err(unsupported(format!("it has no {api} (WebCodecs)")));
            }
        }
        let ready = Rc::new(RefCell::new(Ready::default()));
        let output = {
            let ready = ready.clone();
            Closure::<dyn FnMut(VideoFrame)>::new(move |frame: VideoFrame| {
                let mut ready = ready.borrow_mut();
                let index = frame.timestamp().max(0.0) as usize;
                if index < ready.floor {
                    frame.close();
                } else {
                    ready.frames.push((index, frame));
                }
            })
        };
        let error = {
            let ready = ready.clone();
            Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                ready.borrow_mut().error = Some(js_error(&e));
            })
        };
        let init = Object::new();
        set(&init, "output", output.as_ref());
        set(&init, "error", error.as_ref());
        let decoder = VideoDecoder::new(&init).map_err(|e| unsupported(js_error(&e)))?;
        let config = Object::new();
        set(&config, "codec", codec_string(info.config));
        set(&config, "codedWidth", info.width);
        set(&config, "codedHeight", info.height);
        set(&config, "optimizeForLatency", true);
        decoder.configure(&config).map_err(|e| unsupported(js_error(&e)))?;
        let canvas = OffscreenCanvas::new(info.width, info.height).map_err(|e| unsupported(js_error(&e)))?;
        let options = Object::new();
        set(&options, "willReadFrequently", true);
        let context = canvas
            .get_context("2d", &options)
            .ok()
            .filter(|c| !c.is_null())
            .ok_or_else(|| unsupported("its OffscreenCanvas has no 2D context".into()))?
            .unchecked_into::<Context2d>();
        let id = NEXT_ID.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1));
            id
        });
        DECODERS.with(|d| {
            d.borrow_mut().insert(
                id,
                Entry {
                    decoder,
                    config,
                    ready,
                    canvas: context,
                    first: true,
                    _canvas: canvas,
                    _output: output,
                    _error: error,
                },
            );
        });
        Ok(Self { id })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Entry) -> Result<T, String>) -> Result<T, Error> {
        DECODERS
            .with(|d| {
                let mut decoders = d.borrow_mut();
                let entry = decoders.get_mut(&self.id).ok_or("the video's decoder is gone")?;
                if let Some(e) = entry.ready.borrow().error.clone() {
                    return Err(format!("the browser's decoder stopped: {e}"));
                }
                f(entry)
            })
            .map_err(|e| Error::new(ErrorKind::Decode, e))
    }

    /// Forgets every frame sent so far: the next one sent must be the
    /// keyframe `index`.
    fn reset(&mut self, index: usize) -> Result<(), Error> {
        self.with(|entry| {
            let mut ready = entry.ready.borrow_mut();
            ready.clear();
            ready.floor = index;
            drop(ready);
            entry.decoder.reset().map_err(|e| js_error(&e))?;
            entry.decoder.configure(&entry.config).map_err(|e| js_error(&e))?;
            entry.first = true;
            Ok(())
        })
    }

    /// Chunks sent and not yet decoded.
    fn queued(&self) -> u32 {
        self.with(|entry| Ok(entry.decoder.decode_queue_size())).unwrap_or(0)
    }

    /// Sends frame `index` (`data`, a keyframe or not). The first chunk
    /// after a reset gets the sequence header in front, which encoders put
    /// in the first frame only.
    fn send(&mut self, data: &[u8], index: usize, keyframe: bool, sequence_header: &[u8]) -> Result<(), Error> {
        self.with(|entry| {
            let bytes = if entry.first && index > 0 && !sequence_header.is_empty() {
                let bytes = Uint8Array::new_with_length((sequence_header.len() + data.len()) as u32);
                bytes
                    .subarray(0, sequence_header.len() as u32)
                    .copy_from(sequence_header);
                bytes
                    .subarray(sequence_header.len() as u32, bytes.length())
                    .copy_from(data);
                bytes
            } else {
                Uint8Array::from(data)
            };
            entry.first = false;
            let init = Object::new();
            set(&init, "type", if keyframe { "key" } else { "delta" });
            // Microseconds by the API; the frame's index here, given back
            // with the decoded frame.
            set(&init, "timestamp", index as f64);
            set(&init, "data", bytes);
            let chunk = EncodedVideoChunk::new(&init).map_err(|e| js_error(&e))?;
            entry.decoder.decode(&chunk).map_err(|e| js_error(&e))
        })
    }

    /// The newest decoded frame at or before `target`, read into `rgba`
    /// (`width * height * 4` bytes), and its index; the decoded frames
    /// before it are dropped. `None`: none decoded yet.
    fn take(&mut self, target: usize, rgba: &mut [u8], width: u32, height: u32) -> Result<Option<usize>, Error> {
        self.with(|entry| {
            let mut ready = entry.ready.borrow_mut();
            let Some(newest) = ready.frames.iter().map(|f| f.0).filter(|&i| i <= target).max() else {
                return Ok(None);
            };
            ready.floor = newest;
            let mut shown = Ok(None);
            ready.frames.retain(|(index, frame)| {
                if *index == newest && shown.as_ref().is_ok_and(Option::is_none) {
                    shown = read(&entry.canvas, frame, rgba, width, height).map(|()| Some(newest));
                }
                let keep = *index > newest;
                if !keep {
                    frame.close();
                }
                keep
            });
            shown
        })
    }
}

/// Draws `frame` into the canvas and reads its pixels into `rgba`.
fn read(canvas: &Context2d, frame: &VideoFrame, rgba: &mut [u8], width: u32, height: u32) -> Result<(), String> {
    let (w, h) = (f64::from(width), f64::from(height));
    canvas.draw_frame(frame, 0.0, 0.0, w, h).map_err(|e| js_error(&e))?;
    let pixels = canvas.get_image_data(0.0, 0.0, w, h).map_err(|e| js_error(&e))?.data();
    if pixels.length() as usize != rgba.len() {
        return Err(format!(
            "the frame read back is {} bytes, not {}",
            pixels.length(),
            rgba.len()
        ));
    }
    pixels.copy_to(rgba);
    Ok(())
}

impl Drop for WebAv1 {
    fn drop(&mut self) {
        let entry = DECODERS.with(|d| d.borrow_mut().remove(&self.id));
        if let Some(entry) = entry {
            entry.ready.borrow_mut().clear();
            let _ = entry.decoder.close();
        }
    }
}

impl Video {
    /// The demuxer and the browser's decoder start over at keyframe
    /// `index`. The frame shown stays up until a new one is decoded.
    pub(super) fn restart_at(&mut self, index: usize) -> Result<(), Error> {
        let skip = if index >= self.next {
            index - self.next
        } else {
            self.demuxer.restart().map_err(|e| Error::new(ErrorKind::Decode, e))?;
            index
        };
        self.demuxer.skip(skip).map_err(|e| Error::new(ErrorKind::Decode, e))?;
        self.decoder.reset(index)?;
        self.next = index;
        self.floor = index;
        Ok(())
    }

    /// Sends the frames up to a few past the one for `time` to the browser,
    /// and shows the newest decoded one due. A frame shows once the browser
    /// has decoded it: usually by the next update.
    pub(super) fn show(&mut self, ctx: &mut Ctx, time: f64) -> Result<(), Error> {
        let target = self.frame_at(time);
        // Behind what was kept (looped or sought back), or a keyframe ahead
        // not sent yet: start again at the last keyframe on the way.
        let keyframe = self.keyframe_at_or_before(target);
        if target < self.floor || keyframe > self.next {
            self.restart_at(keyframe)?;
        }
        let last = self.info.frames.len().saturating_sub(1);
        while self.next <= (target + AHEAD).min(last) && (self.decoder.queued() as usize) <= AHEAD {
            let index = self.next;
            let frame = self.demuxer.next().map_err(|e| Error::new(ErrorKind::Decode, e))?;
            self.next += 1;
            let Some((_, keyframe, data)) = frame else {
                break;
            };
            self.decoder.send(data, index, keyframe, &self.info.sequence_header)?;
        }
        let (width, height) = (self.info.width, self.info.height);
        if self.rgba.get().is_none() {
            self.rgba = OnceLock::from([0, 0, 0, 255].repeat(width as usize * height as usize));
        }
        let Some(rgba) = self.rgba.get_mut() else {
            return Ok(());
        };
        if let Some(index) = self.decoder.take(target, rgba, width, height)? {
            self.floor = index;
            if self.shown != Some(index) {
                self.shown = Some(index);
                ctx.gfx.update_texture(self.texture, rgba);
            }
        }
        Ok(())
    }
}
