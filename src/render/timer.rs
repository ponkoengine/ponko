//! GPU frame timing with timestamp queries.
//!
//! Every timed pass writes a timestamp when it starts and when it ends; the
//! frame's GPU time runs from the earliest of them to the latest, so it
//! includes any time the GPU idles between passes.
//! The results come back through a small ring of readback buffers, so reading
//! them never waits for the GPU: a frame's time shows up one to three frames
//! later. If every buffer is still in flight, that frame isn't timed.

use crate::render::traffic::NewBuffer;
use std::sync::{Arc, Mutex, PoisonError};

use crate::profiler::{GpuPass, PassSpan};
use std::sync::atomic::{AtomicU8, Ordering};

const SLOTS: usize = 3;
/// Most passes timed in one frame.
const MAX_PASSES: u32 = 32;
const QUERIES: u32 = MAX_PASSES * 2;
const BYTES: u64 = QUERIES as u64 * 8;
const FREE: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Timestamps the frame in this slot wrote.
    written: u32,
    /// The name of each timed pass, in order: pass `i` wrote timestamps
    /// `2i` and `2i + 1`.
    names: Vec<&'static str>,
    submitted: Option<(u64, web_time::Instant)>,
}

pub(crate) struct GpuTimer {
    frame_number: u64,
    last_sample: Option<(f32, u64, web_time::Instant)>,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: [Slot; SLOTS],
    /// Nanoseconds per timestamp tick.
    period: f64,
    /// The slot this frame writes to, if one was free.
    current: Option<usize>,
    next: usize,
    /// This frame's pass names so far; pass `i` writes timestamps `2i`
    /// and `2i + 1`. Parts of a frame are recorded on several threads.
    names: Mutex<Vec<&'static str>>,
    /// Smoothed GPU time of the frame's passes, in milliseconds.
    pub smoothed_ms: Option<f32>,
    /// Smoothed time of each pass, in the order of the latest frame.
    pub passes: Vec<GpuPass>,
    /// The latest timed frame's passes, unsmoothed, with their start times.
    pub last_frame: Vec<PassSpan>,
    /// Bumped whenever a frame's timestamps come back.
    pub frames_read: u64,
    /// Reused by `smooth_passes`, so reading results allocates nothing.
    scratch: Vec<GpuPass>,
    /// The last timestamp of the latest frame read.
    last_end: u64,
    /// Reused by `read_frame` to sort a frame's passes.
    order: Vec<(u64, u64, &'static str)>,
}

impl GpuTimer {
    /// `None` if the GPU can't do timestamp queries.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERIES,
        });
        let resolve = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size: BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slot = || Slot {
            buffer: device.new_buffer(&wgpu::BufferDescriptor {
                label: Some("timestamp readback"),
                size: BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            written: 0,
            names: Vec::with_capacity(MAX_PASSES as usize),
            submitted: None,
        };
        Some(Self {
            frame_number: 0,
            last_sample: None,
            queries,
            resolve,
            slots: [slot(), slot(), slot()],
            period: f64::from(queue.get_timestamp_period()),
            current: None,
            next: 0,
            names: Mutex::new(Vec::with_capacity(MAX_PASSES as usize)),
            smoothed_ms: None,
            passes: Vec::new(),
            last_frame: Vec::with_capacity(MAX_PASSES as usize),
            frames_read: 0,
            scratch: Vec::with_capacity(MAX_PASSES as usize),
            last_end: 0,
            order: Vec::with_capacity(MAX_PASSES as usize),
        })
    }

    /// Picks up finished readbacks. Call once per frame, before `begin_frame`.
    pub fn collect(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        // Oldest first (slots are used in turn from `next`), so each frame
        // can start where the one before it ended.
        for k in 0..SLOTS {
            let i = (self.next + k) % SLOTS;
            if self.slots[i].state.load(Ordering::Acquire) != READY {
                continue;
            }
            let ok = match self.slots[i].buffer.get_mapped_range(..) {
                Ok(view) => {
                    let t: &[u64] = bytemuck::cast_slice(&view);
                    let slot = &self.slots[i];
                    let not_before = self.last_end;
                    read_frame(
                        &t[..slot.written as usize],
                        &slot.names,
                        self.period,
                        not_before,
                        &mut self.last_frame,
                        &mut self.order,
                    )
                }
                Err(_) => None,
            };
            let slot = &mut self.slots[i];
            slot.buffer.unmap();
            slot.state.store(FREE, Ordering::Release);
            let Some((ms, end)) = ok else { continue };
            self.last_end = end;
            // A bogus sample (clock reset, overflow) would ruin the average.
            if !(ms.is_finite() && ms < 10_000.0) {
                continue;
            }
            self.last_sample = slot.submitted.map(|(frame, at)| (ms, frame, at));
            self.frames_read += 1;
            self.smoothed_ms = Some(match self.smoothed_ms {
                None => ms,
                Some(avg) => avg + (ms - avg) * 0.1,
            });
            smooth_passes(&mut self.passes, &mut self.scratch, &self.last_frame);
        }
    }

    /// The newest frame whose timestamps came back: its own GPU time in ms,
    /// unsmoothed, and how many frames ago it was drawn (counting the one
    /// drawn now as 0; readbacks arrive a few frames late, so it is at
    /// least 1). `None` before the first.
    pub(crate) fn latest_frame(&self) -> Option<(f32, u64)> {
        self.last_sample
            .map(|(ms, frame, _)| (ms, self.frame_number.saturating_sub(frame)))
    }

    /// Raw GPU time with source-frame age. A late readback is not fresh
    /// merely because it was collected just now. CPU time is never a proxy.
    #[cfg(feature = "3d")]
    pub(crate) fn quality_sample(&self) -> Option<crate::three::autotune::Sample> {
        self.last_sample.map(|(ms, frame, at)| crate::three::autotune::Sample {
            sequence: self.frames_read,
            ms,
            fresh: self.frame_number.saturating_sub(frame) <= 8
                && at.elapsed() <= std::time::Duration::from_millis(250),
        })
    }

    /// Starts timing a frame, if a readback slot is free.
    pub fn begin_frame(&mut self) {
        self.frame_number = self.frame_number.wrapping_add(1);
        let i = self.next;
        self.names.get_mut().unwrap_or_else(PoisonError::into_inner).clear();
        self.current = (self.slots[i].state.load(Ordering::Acquire) == FREE).then_some(i);
        if self.current.is_some() {
            self.next = (i + 1) % SLOTS;
        }
    }

    /// The timestamp writes for the frame's next pass, called `name` in
    /// the results: `None` when this frame isn't timed or already timed
    /// `MAX_PASSES` passes.
    pub fn pass(&self, name: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.current?;
        let at = {
            let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
            let at = names.len() as u32 * 2;
            if at + 2 > QUERIES {
                return None;
            }
            names.push(name);
            at
        };
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(at),
            end_of_pass_write_index: Some(at + 1),
        })
    }

    /// Copies the timestamps out. Call after the pass, before submitting.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let names = self.names.get_mut().unwrap_or_else(PoisonError::into_inner);
        let used = names.len() as u32 * 2;
        if let Some(i) = self.current {
            if used == 0 {
                self.current = None;
                return;
            }
            encoder.resolve_query_set(&self.queries, 0..used, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.slots[i].buffer, 0, u64::from(used) * 8);
            self.slots[i].written = used;
            let slot = &mut self.slots[i];
            slot.names.clear();
            slot.names.extend_from_slice(names);
        }
    }

    /// Starts reading this frame's timestamps back. Call after submitting.
    pub fn submitted(&mut self) {
        if let Some(i) = self.current.take() {
            let slot = &mut self.slots[i];
            slot.submitted = Some((self.frame_number, web_time::Instant::now()));
            slot.state.store(MAPPING, Ordering::Release);
            let state = slot.state.clone();
            slot.buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                state.store(if result.is_ok() { READY } else { FREE }, Ordering::Release);
            });
        }
    }
}

/// Turns one frame's timestamps into `spans` (cleared first) and returns
/// the frame's GPU time, from its first pass's start to its last pass's
/// end, and that end.
///
/// Every frame writes the same queries, and a pass the GPU skips or
/// finishes oddly (one with nothing to draw) can leave timestamps from an
/// earlier frame behind: a pass that ends before it begins, or begins before
/// `not_before` (the previous frame's end), is left out, as are unwritten
/// (zero) ones.
///
/// Each pass is charged from when the pass before it ended (or its own
/// start, if later) to its own end. Tile-based GPUs start a pass before the
/// last one ends, and a pass that waits for the one before it would
/// otherwise count that wait as its own, so the passes would add up to far
/// more than the frame. This way they add up to the frame's time, less any
/// idle gaps.
fn read_frame(
    t: &[u64],
    names: &[&'static str],
    period: f64,
    not_before: u64,
    spans: &mut Vec<PassSpan>,
    order: &mut Vec<(u64, u64, &'static str)>,
) -> Option<(f32, u64)> {
    let valid = |&[begin, end]: &[u64; 2]| begin != 0 && begin >= not_before && end >= begin;
    let pairs = t.as_chunks::<2>().0;
    let (first, last) = pairs
        .iter()
        .filter(|p| valid(p))
        .fold((u64::MAX, 0), |(lo, hi), &[b, e]| (lo.min(b), hi.max(e)));
    if last <= first {
        return None;
    }
    let ms = |ticks: u64| (ticks as f64 * period / 1e6) as f32;
    spans.clear();
    // Passes recorded on several threads were given their queries in no
    // particular order: put them in the order the GPU ran them.
    order.clear();
    order.extend(
        pairs
            .iter()
            .zip(names)
            .filter(|(p, _)| valid(p))
            .map(|(&[begin, end], &name)| (begin, end, name)),
    );
    order.sort_unstable_by_key(|&(begin, _, _)| begin);
    let mut done = first;
    for &(begin, end, name) in order.iter() {
        let start = begin.max(done).min(end);
        done = done.max(end);
        spans.push(PassSpan {
            name,
            start_ms: ms(start - first),
            ms: ms(end - start),
        });
    }
    Some((ms(last - first), last))
}

/// Folds one frame's spans into the smoothed per-pass times. Passes with the
/// same name are added; the order follows the frame, and passes the frame
/// didn't run are dropped.
fn smooth_passes(passes: &mut Vec<GpuPass>, out: &mut Vec<GpuPass>, frame: &[PassSpan]) {
    out.clear();
    for span in frame {
        match out.iter_mut().find(|p| p.name == span.name) {
            Some(p) => p.ms += span.ms,
            None => out.push(GpuPass {
                name: span.name,
                ms: span.ms,
            }),
        }
    }
    for p in out.iter_mut() {
        if let Some(old) = passes.iter().find(|o| o.name == p.name) {
            p.ms = old.ms + (p.ms - old.ms) * 0.1;
        }
    }
    std::mem::swap(passes, out);
}

impl GpuTimer {
    /// Counts the timestamp queries and their readback buffers as other
    /// memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.bytes(C::Other, BYTES);
        t.buffer(C::Other, &self.resolve);
        for slot in &self.slots {
            t.buffer(C::Other, &slot.buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_split_into_named_passes() {
        let mut spans = Vec::new();
        // Two passes; the GPU skipped the third's timestamps.
        let t = [1_000_000, 3_000_000, 3_000_000, 4_000_000, 0, 0];
        let (total, _) = read_frame(&t, &["a", "b", "c"], 1.0, 0, &mut spans, &mut Vec::new()).unwrap();
        assert!((total - 3.0).abs() < 1e-6);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].name, "b");
        assert!((spans[1].start_ms - 2.0).abs() < 1e-6 && (spans[1].ms - 1.0).abs() < 1e-6);
    }

    #[test]
    fn overlapping_passes_are_charged_from_the_previous_end() {
        let mut spans = Vec::new();
        // b begins while a still runs, waiting for it: b is charged 4..6
        // (each timestamp is one past its time: zero means unwritten).
        let t = [1, 5, 2, 7];
        let (total, end) = read_frame(&t, &["a", "b"], 1e6, 0, &mut spans, &mut Vec::new()).unwrap();
        assert_eq!(end, 7);
        assert_eq!(total, 6.0);
        assert_eq!(spans.iter().map(|s| s.ms).sum::<f32>(), total);
        assert_eq!((spans[1].start_ms, spans[1].ms), (4.0, 2.0));
    }

    #[test]
    fn stale_timestamps_from_skipped_passes_are_ignored() {
        let mut spans = Vec::new();
        // Pass "b" was skipped: its queries still hold the last frame's 1, 2;
        // "c" ends before it begins (its end is an older frame's).
        let t = [10, 14, 1, 2, 14, 16, 15, 4];
        let (total, end) = read_frame(&t, &["a", "b", "c", "d"], 1e6, 3, &mut spans, &mut Vec::new()).unwrap();
        assert_eq!((total, end), (6.0, 16));
        assert_eq!(spans.iter().map(|s| s.name).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn same_named_passes_add_up_and_smooth() {
        let span = |name, ms| PassSpan {
            name,
            start_ms: 0.0,
            ms,
        };
        let (mut passes, mut scratch) = (Vec::new(), Vec::new());
        smooth_passes(
            &mut passes,
            &mut scratch,
            &[span("shadows", 1.0), span("opaque", 4.0), span("shadows", 1.0)],
        );
        assert_eq!(
            passes,
            [
                GpuPass {
                    name: "shadows",
                    ms: 2.0
                },
                GpuPass {
                    name: "opaque",
                    ms: 4.0
                }
            ]
        );
        smooth_passes(&mut passes, &mut scratch, &[span("opaque", 14.0)]);
        assert_eq!(passes.len(), 1, "passes that stopped running are dropped");
        assert!((passes[0].ms - 5.0).abs() < 1e-5, "{}", passes[0].ms);
    }

    /// A device with those of `features` the adapter has. The shared helper
    /// (which needs the `3d` feature) logs the adapter, fails a missing one
    /// under `PONKO_REQUIRE_GPU` and applies `PONKO_GPU_TIER`.
    #[cfg(feature = "3d")]
    fn device(features: wgpu::Features) -> Option<(wgpu::Device, wgpu::Queue)> {
        let gpu = crate::render::render_test_support::gpu_with(|adapter| {
            Ok(wgpu::DeviceDescriptor {
                required_features: adapter.features() & features,
                ..Default::default()
            })
        })?;
        Some((gpu.device, gpu.queue))
    }

    /// The render-pass inspector reports GPU time only where this makes a
    /// timer: a device without timestamp queries gets none, so its GPU
    /// times are unavailable rather than zero. Offscreen, no window; skips
    /// without a GPU.
    #[cfg(feature = "3d")]
    #[test]
    fn devices_without_timestamp_queries_get_no_timer() {
        let Some((device, queue)) = device(wgpu::Features::empty()) else {
            return;
        };
        assert!(!device.features().contains(wgpu::Features::TIMESTAMP_QUERY));
        assert!(GpuTimer::new(&device, &queue).is_none());
    }

    #[cfg(feature = "3d")]
    #[test]
    fn quality_samples_expire_by_source_frame_and_submission_time() {
        let Some((device, queue)) = device(wgpu::Features::TIMESTAMP_QUERY) else {
            return;
        };
        let Some(mut timer) = GpuTimer::new(&device, &queue) else {
            crate::render::render_test_support::skip("needs timestamp queries");
            return;
        };
        assert!(timer.quality_sample().is_none());
        timer.frames_read = 1;
        timer.last_sample = Some((20.0, 1, web_time::Instant::now()));
        timer.frame_number = 8;
        assert!(timer.quality_sample().unwrap().fresh);
        timer.frame_number = 10;
        assert!(!timer.quality_sample().unwrap().fresh);
        timer.frame_number = 1;
        timer.last_sample = Some((20.0, 1, web_time::Instant::now() - std::time::Duration::from_secs(1)));
        assert!(!timer.quality_sample().unwrap().fresh);
    }

    /// A device with timestamp queries gets a timer, and it reports nothing
    /// until a frame's timestamps have actually come back.
    #[cfg(feature = "3d")]
    #[test]
    fn a_new_timer_reports_no_gpu_time_before_a_readback() {
        let Some((device, queue)) = device(wgpu::Features::TIMESTAMP_QUERY) else {
            return;
        };
        // An adapter (or a forced tier) without timestamp queries is a
        // skip, also under `PONKO_REQUIRE_GPU`: the test above covers it.
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            crate::render::render_test_support::skip("needs timestamp queries, which this device lacks");
            return;
        }
        let timer = GpuTimer::new(&device, &queue).expect("timestamps are supported");
        assert_eq!(timer.frames_read, 0);
        assert!(timer.smoothed_ms.is_none());
        assert!(timer.passes.is_empty());
    }
}
