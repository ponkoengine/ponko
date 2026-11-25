//! Schedule Kira's mix in Web Audio. Keeping the destination's default
//! configuration lets browsers accept stereo even if the device query
//! transiently reports zero channels.

use js_sys::Float32Array;
use kira::backend::{Backend, Renderer};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, AudioScheduledSourceNode};

const BLOCK_FRAMES: u32 = 2048;

static LOGGED_FALLBACK: AtomicBool = AtomicBool::new(false);
static LOGGED_FAILURE: AtomicBool = AtomicBool::new(false);

pub(in crate::audio) fn warn_unavailable(reason: &str) {
    if !LOGGED_FAILURE.swap(true, Ordering::Relaxed) {
        web_sys::console::warn_1(&JsValue::from_str(&format!(
            "ponko: Browser audio is unavailable: {reason}"
        )));
    }
}

fn web_error(operation: &'static str) -> impl FnOnce(JsValue) -> String {
    move |error| format!("{operation}: {error:?}")
}

pub(in crate::audio) struct BrowserOutput {
    context: AudioContext,
    channels: u16,
    stream: Option<Rc<Stream>>,
    status: BrowserStatus,
}

#[derive(Clone, Default)]
pub(in crate::audio) struct BrowserStatus(Rc<RefCell<Weak<Stream>>>);

impl BrowserStatus {
    pub(in crate::audio) fn ready(&self) -> bool {
        self.0
            .borrow()
            .upgrade()
            .is_some_and(|stream| stream.ready.get() && stream.context.state() == AudioContextState::Running)
    }
}

impl Backend for BrowserOutput {
    type Settings = BrowserStatus;
    type Error = String;

    fn setup(status: BrowserStatus, _: usize) -> Result<(Self, u32), String> {
        let context = AudioContext::new().map_err(web_error("opening AudioContext"))?;
        let max_channels = context.destination().max_channel_count();
        let channels = super::output_channels(max_channels);
        if max_channels == 0 && !LOGGED_FALLBACK.swap(true, Ordering::Relaxed) {
            web_sys::console::warn_1(&JsValue::from_str(
                "ponko: Browser audio reported zero output channels. Trying stereo.",
            ));
        }
        let sample_rate = context.sample_rate() as u32;
        Ok((
            Self {
                context,
                channels,
                stream: None,
                status,
            },
            sample_rate,
        ))
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), String> {
        let context = &self.context;
        let make_buffer = || {
            context
                .create_buffer(self.channels.into(), BLOCK_FRAMES, context.sample_rate())
                .map_err(web_error("creating the output buffer"))
        };
        let buffers = [make_buffer()?, make_buffer()?];
        let resumed = context.resume().map_err(web_error("resuming AudioContext"))?;
        let stream = Rc::new_cyclic(|weak: &Weak<Stream>| Stream {
            context: context.clone(),
            ready: Cell::new(false),
            mix: RefCell::new(Mix {
                renderer,
                channels: self.channels,
                interleaved: vec![0.0; BLOCK_FRAMES as usize * usize::from(self.channels)],
                channel: vec![0.0; BLOCK_FRAMES as usize],
                // Web Audio's copyToChannel requires JS-owned memory in threaded builds.
                js_channel: Float32Array::new_with_length(BLOCK_FRAMES),
                next_time: 0.0,
                buffers,
                sources: [None, None],
            }),
            on_ended: std::array::from_fn(|slot| {
                let weak = weak.clone();
                Closure::new(move || {
                    if let Some(stream) = weak.upgrade()
                        && stream.ready.get()
                        && let Err(error) = stream.schedule(slot)
                    {
                        stream.fail(&error);
                    }
                })
            }),
        });
        let weak = Rc::downgrade(&stream);
        *self.status.0.borrow_mut() = weak.clone();
        spawn_local(async move {
            let result = JsFuture::from(resumed).await;
            if let Some(stream) = weak.upgrade() {
                match result
                    .map_err(web_error("resuming AudioContext"))
                    .and_then(|_| stream.schedule(0))
                    .and_then(|_| stream.schedule(1))
                {
                    Ok(()) => stream.ready.set(true),
                    Err(error) => stream.fail(&error),
                }
            }
        });
        self.stream = Some(stream);
        Ok(())
    }
}

impl Drop for BrowserOutput {
    fn drop(&mut self) {
        // Detach pending callbacks before releasing their Rust closures.
        self.stream.take();
        if let Ok(closed) = self.context.close() {
            spawn_local(async move {
                let _ = JsFuture::from(closed).await;
            });
        }
    }
}

struct Stream {
    context: AudioContext,
    ready: Cell<bool>,
    mix: RefCell<Mix>,
    on_ended: [Closure<dyn FnMut()>; 2],
}

struct Mix {
    renderer: Renderer,
    channels: u16,
    interleaved: Vec<f32>,
    channel: Vec<f32>,
    js_channel: Float32Array,
    next_time: f64,
    buffers: [AudioBuffer; 2],
    sources: [Option<AudioBufferSourceNode>; 2],
}

impl Stream {
    fn schedule(&self, slot: usize) -> Result<(), String> {
        let mut mix = self.mix.borrow_mut();
        let Mix {
            renderer,
            channels,
            interleaved,
            channel,
            js_channel,
            next_time,
            buffers,
            sources,
        } = &mut *mix;
        if let Some(old) = sources[slot].take() {
            AsRef::<AudioScheduledSourceNode>::as_ref(&old).set_onended(None);
            let _ = old.disconnect();
        }
        let buffer = &buffers[slot];
        renderer.on_start_processing();
        renderer.process(interleaved, *channels);
        for i in 0..usize::from(*channels) {
            for (out, frame) in channel.iter_mut().zip(interleaved.chunks_exact(usize::from(*channels))) {
                *out = frame[i];
            }
            js_channel.copy_from(channel);
            buffer
                .copy_to_channel_with_f32_array(js_channel, i as i32)
                .map_err(web_error("copying the output samples"))?;
        }
        let source = self
            .context
            .create_buffer_source()
            .map_err(web_error("creating an output source"))?;
        source.set_buffer(Some(buffer));
        source
            .connect_with_audio_node(&self.context.destination())
            .map_err(web_error("connecting the audio output"))?;
        let now = self.context.current_time();
        let duration = buffer.duration();
        if *next_time <= now {
            *next_time = now + duration;
        }
        source
            .start_with_when(*next_time)
            .map_err(web_error("starting the output source"))?;
        *next_time += duration;
        AsRef::<AudioScheduledSourceNode>::as_ref(&source)
            .set_onended(Some(self.on_ended[slot].as_ref().unchecked_ref()));
        sources[slot] = Some(source);
        Ok(())
    }

    fn fail(&self, error: &str) {
        self.ready.set(false);
        warn_unavailable(error);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        for source in self.mix.get_mut().sources.iter().flatten() {
            AsRef::<AudioScheduledSourceNode>::as_ref(source).set_onended(None);
        }
    }
}
