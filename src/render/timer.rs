//! GPU frame timing with timestamp queries.
//!
//! Every timed pass writes a timestamp when it starts and when it ends; the
//! frame's GPU time runs from the earliest of them to the latest, so it
//! includes any time the GPU idles between passes.
//! The results come back through a small ring of readback buffers, so reading
//! them never waits for the GPU: a frame's time shows up a few frames later.
//! If every buffer is still in flight, that frame isn't timed.
//!
//! A frame's timestamps are copied out only once the GPU has finished that
//! frame, by a later frame's command buffer: Metal writes a compute pass's
//! timestamps (and sometimes the last pass's end) after a copy in the same
//! command buffer, or in one submitted right after it, has read them, so
//! that copy would read the previous frame's values. Each frame in flight
//! writes its own range of queries for that.
//!
//! A timer can time only every `n`th frame ([`GpuTimer::set_every`]):
//! timestamp writes cost the graphics library a few heap allocations for
//! each timed pass, so a game times every frame only while something reads
//! every frame (see `Ctx::gpu_timing_every`). The smoothed times weigh each
//! sample by the frames it stands for, so they settle where an every-frame
//! timer's would.

use crate::render::traffic::NewBuffer;
use std::sync::{Arc, Mutex, PoisonError};

use crate::profiler::{GpuPass, PassSpan};
use std::sync::atomic::{AtomicU8, Ordering};

/// Frames in flight at once: drawn, finished, copied out, read back.
const SLOTS: usize = 6;
/// Most passes timed in one frame.
const MAX_PASSES: u32 = 32;
/// Queries of one frame: the slot `i` writes `i * QUERIES..(i + 1) * QUERIES`.
const QUERIES: u32 = MAX_PASSES * 2;
/// Bytes of one frame's timestamps (a multiple of the resolve alignment).
const BYTES: u64 = QUERIES as u64 * 8;
const FREE: u8 = 0;
/// Submitted; the GPU may still be writing its timestamps.
const RUNNING: u8 = 1;
/// The GPU finished the frame: its timestamps can be copied out.
const DONE: u8 = 2;
/// The copy is recorded in the frame being drawn, not submitted yet.
const COPYING: u8 = 3;
const MAPPING: u8 = 4;
const READY: u8 = 5;

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Timestamps the frame in this slot wrote.
    written: u32,
    /// The name of each timed pass, in order: pass `i` wrote timestamps
    /// `2i` and `2i + 1`.
    names: Vec<&'static str>,
    submitted: Option<(u64, web_time::Instant)>,
    /// How many frames this slot's frame stands for: the untimed frames
    /// since the timed one before it, and itself.
    covers: u32,
}

/// Which frames are timed: every `every`th one, none if 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cadence {
    every: u32,
    /// Frames begun since the last timed frame was submitted, this one included.
    since: u32,
}

impl Cadence {
    /// Times every frame.
    pub(crate) const EVERY_FRAME: Self = Self { every: 1, since: 0 };

    /// Starts a frame: whether it is due to be timed.
    fn begin(&mut self) -> bool {
        self.since = self.since.saturating_add(1);
        self.every != 0 && self.since >= self.every
    }

    /// A frame was timed and submitted: how many frames it stands for.
    fn timed(&mut self) -> u32 {
        std::mem::replace(&mut self.since, 0).max(1)
    }
}

/// The smoothing weight of a sample that stands for `frames` frames: what
/// `frames` samples of the same time, weighed 0.1 each, would add up to.
fn weight(frames: u32) -> f32 {
    1.0 - 0.9f32.powi(frames.clamp(1, 64) as i32)
}

pub(crate) struct GpuTimer {
    queue: wgpu::Queue,
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
    /// The latest timed frame's passes whose timestamps didn't come back
    /// usable (the GPU skipped an empty pass, or the backend wrote none),
    /// once each: not timed, rather than 0 ms.
    pub not_timed: Vec<&'static str>,
    /// Bumped whenever a frame's timestamps come back.
    pub frames_read: u64,
    /// Reused by `smooth_passes`, so reading results allocates nothing.
    scratch: Vec<GpuPass>,
    /// The last timestamp of the latest frame read.
    last_end: u64,
    /// Reused by `read_frame` to sort a frame's passes.
    order: Vec<Charge>,
    cadence: Cadence,
    /// A frame due to be timed found every readback slot in flight, and no
    /// frame's timestamps have come back since.
    starved: bool,
    /// The right to hold `queries` (see `query_sets.rs`). Declared last,
    /// so the queries are dropped before it is given back.
    _lease: super::query_sets::Lease,
}

impl GpuTimer {
    /// `None` if the GPU can't do timestamp queries, or (after one
    /// warning) if this process already holds all the timestamp query sets
    /// the engine takes (see `query_sets.rs`).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let lease = super::query_sets::lease(device, "GPU frame timer")?;
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERIES * SLOTS as u32,
        });
        let resolve = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size: BYTES * SLOTS as u64,
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
            covers: 1,
        };
        Some(Self {
            queue: queue.clone(),
            frame_number: 0,
            last_sample: None,
            queries,
            resolve,
            slots: std::array::from_fn(|_| slot()),
            period: f64::from(queue.get_timestamp_period()),
            current: None,
            next: 0,
            names: Mutex::new(Vec::with_capacity(MAX_PASSES as usize)),
            smoothed_ms: None,
            passes: Vec::new(),
            last_frame: Vec::with_capacity(MAX_PASSES as usize),
            not_timed: Vec::with_capacity(MAX_PASSES as usize),
            frames_read: 0,
            scratch: Vec::with_capacity(MAX_PASSES as usize),
            last_end: 0,
            order: Vec::with_capacity(MAX_PASSES as usize),
            cadence: Cadence::EVERY_FRAME,
            starved: false,
            _lease: lease,
        })
    }

    /// Why [`new`](Self::new) gave no timer on `device`: it has no
    /// timestamp queries, or the query-set budget refused one.
    pub fn why_none(device: &wgpu::Device) -> crate::perf::GpuUntimed {
        if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            crate::perf::GpuUntimed::QuerySetBudget
        } else {
            crate::perf::GpuUntimed::NoTimestamps
        }
    }

    /// Why the latest frame has no usable GPU time, if it has none: timing
    /// is off, or no timed frame's timestamps have come back (none yet, or
    /// the readbacks are all in flight).
    pub fn untimed(&self) -> Option<crate::perf::GpuUntimed> {
        if self.cadence.every == 0 {
            Some(crate::perf::GpuUntimed::Off)
        } else if self.frames_read == 0 || self.starved {
            Some(crate::perf::GpuUntimed::NotReady)
        } else {
            None
        }
    }

    /// Times every `n`th frame from now on (1, the default: every frame;
    /// 0: none). Frames in between write no timestamps. Turned off, the
    /// times shown so far are dropped, and readbacks still in flight are
    /// discarded.
    pub fn set_every(&mut self, n: u32) {
        self.cadence.every = n;
        if n == 0 {
            self.smoothed_ms = None;
            self.passes.clear();
            self.last_frame.clear();
            self.last_sample = None;
        }
    }

    /// Whether the frame begun last writes timestamps (the scene
    /// benchmark resolves only those).
    #[cfg(all(feature = "3d", not(target_arch = "wasm32"), any(test, feature = "bench-internals")))]
    pub fn timing_this_frame(&self) -> bool {
        self.current.is_some()
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
                        &mut self.not_timed,
                        &mut self.order,
                    )
                }
                Err(_) => None,
            };
            let slot = &mut self.slots[i];
            slot.buffer.unmap();
            slot.state.store(FREE, Ordering::Release);
            if self.cadence.every == 0 {
                continue;
            }
            let Some((ms, end)) = ok else { continue };
            self.last_end = end;
            // A bogus sample (clock reset, overflow) would ruin the average.
            if !(ms.is_finite() && ms < 10_000.0) {
                continue;
            }
            self.last_sample = slot.submitted.map(|(frame, at)| (ms, frame, at));
            self.frames_read += 1;
            self.starved = false;
            let w = weight(slot.covers);
            self.smoothed_ms = Some(match self.smoothed_ms {
                None => ms,
                Some(avg) => avg + (ms - avg) * w,
            });
            smooth_passes(&mut self.passes, &mut self.scratch, &self.last_frame, w);
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

    /// Starts timing a frame, if it is due (see [`set_every`](Self::set_every))
    /// and a readback slot is free.
    pub fn begin_frame(&mut self) {
        self.frame_number = self.frame_number.wrapping_add(1);
        let i = self.next;
        self.names.get_mut().unwrap_or_else(PoisonError::into_inner).clear();
        let due = self.cadence.begin();
        self.current = (due && self.slots[i].state.load(Ordering::Acquire) == FREE).then_some(i);
        if due && self.current.is_none() {
            self.starved = true;
        }
        if self.current.is_some() {
            self.next = (i + 1) % SLOTS;
        }
    }

    /// The timestamp writes for the frame's next pass, called `name` in
    /// the results: `None` when this frame isn't timed or already timed
    /// `MAX_PASSES` passes.
    pub fn pass(&self, name: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let slot = self.current?;
        let at = {
            let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
            let at = names.len() as u32 * 2;
            if at + 2 > QUERIES {
                return None;
            }
            names.push(name);
            slot as u32 * QUERIES + at
        };
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(at),
            end_of_pass_write_index: Some(at + 1),
        })
    }

    /// Copies out the timestamps of the frames the GPU has finished, and
    /// closes this frame's list of passes. Call once per frame, timed or
    /// not, after the passes and before submitting this encoder.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        // Only now are their timestamps all written (see the module docs).
        for (i, slot) in self.slots.iter().enumerate() {
            if slot.state.load(Ordering::Acquire) != DONE {
                continue;
            }
            let first = i as u32 * QUERIES;
            let offset = u64::from(first) * 8;
            encoder.resolve_query_set(&self.queries, first..first + slot.written, &self.resolve, offset);
            encoder.copy_buffer_to_buffer(&self.resolve, offset, &slot.buffer, 0, u64::from(slot.written) * 8);
            slot.state.store(COPYING, Ordering::Release);
        }
        let names = self.names.get_mut().unwrap_or_else(PoisonError::into_inner);
        let used = names.len() as u32 * 2;
        if let Some(i) = self.current {
            if used == 0 {
                self.current = None;
                return;
            }
            let slot = &mut self.slots[i];
            slot.written = used;
            slot.names.clear();
            slot.names.extend_from_slice(names);
        }
    }

    /// Call after submitting the encoder [`resolve`](Self::resolve) wrote
    /// to: starts reading back the timestamps it copied, and waits for the
    /// GPU to finish this frame before its own are copied.
    pub fn submitted(&mut self) {
        for slot in &self.slots {
            if slot.state.load(Ordering::Acquire) != COPYING {
                continue;
            }
            slot.state.store(MAPPING, Ordering::Release);
            let state = slot.state.clone();
            slot.buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                state.store(if result.is_ok() { READY } else { FREE }, Ordering::Release);
            });
        }
        if let Some(i) = self.current.take() {
            let slot = &mut self.slots[i];
            slot.covers = self.cadence.timed();
            slot.submitted = Some((self.frame_number, web_time::Instant::now()));
            slot.state.store(RUNNING, Ordering::Release);
            let state = slot.state.clone();
            self.queue.on_submitted_work_done(move || {
                let _ = state.compare_exchange(RUNNING, DONE, Ordering::AcqRel, Ordering::Acquire);
            });
        }
    }
}

/// Turns one frame's timestamps into `spans` (cleared first) and returns
/// the frame's GPU time, from its first pass's start to its last pass's
/// end, and that end. The passes whose timestamps are unusable go to
/// `not_timed` (cleared first), once each.
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
/// more than the frame. A pass that runs entirely alongside time already
/// counted (a compute pass the GPU runs next to a render pass it doesn't
/// depend on) would get nothing that way: the passes running at once share
/// that time evenly instead. Either way they add up to the frame's time,
/// less any idle gaps.
fn read_frame(
    t: &[u64],
    names: &[&'static str],
    period: f64,
    not_before: u64,
    spans: &mut Vec<PassSpan>,
    not_timed: &mut Vec<&'static str>,
    order: &mut Vec<Charge>,
) -> Option<(f32, u64)> {
    let valid = |&[begin, end]: &[u64; 2]| begin != 0 && begin >= not_before && end >= begin;
    let pairs = t.as_chunks::<2>().0;
    not_timed.clear();
    for (pair, &name) in pairs.iter().zip(names) {
        if !valid(pair) && !not_timed.contains(&name) {
            not_timed.push(name);
        }
    }
    let (first, last) = pairs
        .iter()
        .filter(|p| valid(p))
        .fold((u64::MAX, 0), |(lo, hi), &[b, e]| (lo.min(b), hi.max(e)));
    if last <= first {
        return None;
    }
    let ms = |ticks: f64| (ticks * period / 1e6) as f32;
    spans.clear();
    // Passes recorded on several threads were given their queries in no
    // particular order: put them in the order the GPU ran them.
    order.clear();
    order.extend(
        pairs
            .iter()
            .zip(names)
            .filter(|(p, _)| valid(p))
            .map(|(&[begin, end], &name)| Charge {
                begin,
                end,
                from: begin,
                name,
            }),
    );
    order.sort_unstable_by_key(|c| c.begin);
    let mut done = first;
    for c in order.iter_mut() {
        c.from = c.begin.max(done).min(c.end);
        done = done.max(c.end);
    }
    for c in order.iter() {
        let (from, ticks) = charge(order, c);
        spans.push(PassSpan {
            name: c.name,
            start_ms: ms((from - first) as f64),
            ms: ms(ticks),
        });
    }
    timed_names_out(not_timed, spans);
    Some((ms((last - first) as f64), last))
}

/// A pass's timestamps while its frame is read: `from` is where its own
/// time starts (the previous pass's end, or its begin if later).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Charge {
    begin: u64,
    end: u64,
    from: u64,
    name: &'static str,
}

impl Charge {
    /// Ran entirely within time an earlier pass already counts.
    fn alongside(&self) -> bool {
        self.from == self.end && self.begin < self.end
    }
}

/// Where pass `c`'s time starts and how many ticks it is charged: its own
/// span, or for a pass run alongside, its begin to its end, split evenly
/// with the passes running at the same time.
fn charge(all: &[Charge], c: &Charge) -> (u64, f64) {
    let (a, b) = if c.alongside() {
        (c.begin, c.end)
    } else {
        (c.from, c.end)
    };
    if b <= a {
        return (a, 0.0);
    }
    // The passes' ends cut [a, b] into pieces with the same passes
    // running throughout each.
    let mut cuts = [0u64; 2 * MAX_PASSES as usize + 2];
    cuts[0] = a;
    cuts[1] = b;
    let mut n = 2;
    for o in all {
        let start = if o.alongside() { o.begin } else { o.from };
        for at in [start, o.end] {
            if at > a && at < b && n < cuts.len() {
                cuts[n] = at;
                n += 1;
            }
        }
    }
    let cuts = &mut cuts[..n];
    cuts.sort_unstable();
    let mut ticks = 0.0;
    for w in cuts.windows(2) {
        let (x, y) = (w[0], w[1]);
        if y == x {
            continue;
        }
        let beside = all
            .iter()
            .filter(|o| o.alongside() && o.begin <= x && o.end >= y)
            .count();
        let owner = all
            .iter()
            .any(|o| !o.alongside() && o.from <= x && o.end >= y && o.from < o.end);
        let sharing = (beside + usize::from(owner)).max(1);
        ticks += (y - x) as f64 / sharing as f64;
    }
    (a, ticks)
}

/// Leaves out of `not_timed` the names that were timed elsewhere in the
/// frame (passes with the same name are added up, so they show).
fn timed_names_out(not_timed: &mut Vec<&'static str>, spans: &[PassSpan]) {
    not_timed.retain(|name| !spans.iter().any(|s| s.name == *name));
}

/// Folds one frame's spans into the smoothed per-pass times, the frame
/// weighing `weight` (0.1 for a frame that stands for itself only). Passes
/// with the same name are added; the order follows the frame, and passes the
/// frame didn't run are dropped.
fn smooth_passes(passes: &mut Vec<GpuPass>, out: &mut Vec<GpuPass>, frame: &[PassSpan], weight: f32) {
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
            p.ms = old.ms + (p.ms - old.ms) * weight;
        }
    }
    std::mem::swap(passes, out);
}

impl GpuTimer {
    /// Counts the timestamp queries and their readback buffers as other
    /// memory (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.bytes(C::Other, "timestamp queries", BYTES);
        t.buffer(C::Other, "timestamp resolve", &self.resolve);
        for slot in &self.slots {
            t.buffer(C::Other, "timestamp readback", &slot.buffer);
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
        let (total, _) = read_frame(
            &t,
            &["a", "b", "c"],
            1.0,
            0,
            &mut spans,
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap();
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
        let (total, end) = read_frame(&t, &["a", "b"], 1e6, 0, &mut spans, &mut Vec::new(), &mut Vec::new()).unwrap();
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
        let (total, end) = read_frame(
            &t,
            &["a", "b", "c", "d"],
            1e6,
            3,
            &mut spans,
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!((total, end), (6.0, 16));
        assert_eq!(spans.iter().map(|s| s.name).collect::<Vec<_>>(), ["a", "c"]);
    }

    /// Passes whose timestamps came back unusable are listed as not timed,
    /// not shown as 0 ms; a name timed elsewhere in the frame isn't listed.
    #[test]
    fn passes_without_usable_timestamps_are_listed_as_not_timed() {
        let (mut spans, mut not_timed) = (Vec::new(), Vec::new());
        // "cull" was skipped (unwritten), "post" is stale (before the
        // previous frame's end), one "shadows" ran and one was skipped.
        let t = [10, 14, 0, 0, 1, 2, 14, 16, 0, 0];
        let names = ["shadows", "cull", "post", "opaque", "shadows"];
        read_frame(&t, &names, 1e6, 3, &mut spans, &mut not_timed, &mut Vec::new()).unwrap();
        assert_eq!(spans.iter().map(|s| s.name).collect::<Vec<_>>(), ["shadows", "opaque"]);
        assert_eq!(not_timed, ["cull", "post"]);
        assert!(spans.iter().all(|s| s.ms > 0.0));
    }

    /// A compute pass the GPU runs alongside a render pass (it began and
    /// ended while that one ran) used to be charged 0 ms; now the two share
    /// the time they ran together, and the passes still add up to the frame.
    #[test]
    fn a_pass_run_alongside_another_shares_their_time() {
        let (mut spans, mut not_timed) = (Vec::new(), Vec::new());
        // r1 0..10 (as 1..11: zero means unwritten), compute 2..6 within
        // it, r2 8..14 overlapping r1's end.
        let t = [1, 11, 3, 7, 9, 15];
        let (total, _) = read_frame(
            &t,
            &["r1", "compute", "r2"],
            1e6,
            0,
            &mut spans,
            &mut not_timed,
            &mut Vec::new(),
        )
        .unwrap();
        assert!(not_timed.is_empty());
        let by = |name| spans.iter().find(|s: &&PassSpan| s.name == name).unwrap();
        assert_eq!(total, 14.0);
        // r1 alone 0..2 and 6..10, half of 2..6.
        assert_eq!((by("r1").start_ms, by("r1").ms), (0.0, 8.0));
        assert_eq!((by("compute").start_ms, by("compute").ms), (2.0, 2.0));
        // r2 from r1's end, as before.
        assert_eq!((by("r2").start_ms, by("r2").ms), (10.0, 4.0));
        assert_eq!(spans.iter().map(|s| s.ms).sum::<f32>(), total);

        // Three at once: a long compute pass with a render pass and another
        // compute pass inside it, the two inner ones overlapping.
        let t = [1, 21, 5, 13, 9, 17];
        let (total, _) = read_frame(
            &t,
            &["c1", "r", "c2"],
            1e6,
            0,
            &mut spans,
            &mut not_timed,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(total, 20.0);
        assert!(spans.iter().all(|s| s.ms > 0.0), "{spans:?}");
        let sum = spans.iter().map(|s| s.ms).sum::<f32>();
        assert!((sum - total).abs() < 1e-4, "{sum} vs {total}");
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
            0.1,
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
        smooth_passes(&mut passes, &mut scratch, &[span("opaque", 14.0)], 0.1);
        assert_eq!(passes.len(), 1, "passes that stopped running are dropped");
        assert!((passes[0].ms - 5.0).abs() < 1e-5, "{}", passes[0].ms);
    }

    #[test]
    fn a_cadence_of_none_times_no_frame() {
        let mut c = Cadence::EVERY_FRAME;
        c.every = 0;
        assert!((0..100).all(|_| !c.begin()));
    }

    #[test]
    fn a_cadence_of_four_times_every_fourth_frame() {
        let mut c = Cadence::EVERY_FRAME;
        c.every = 4;
        let mut due = Vec::new();
        for _ in 0..12 {
            let d = c.begin();
            if d {
                assert_eq!(c.timed(), 4, "a timed frame stands for the three before it and itself");
            }
            due.push(d);
        }
        assert_eq!(due, [false, false, false, true].repeat(3));
        // A due frame that couldn't be timed (every readback in flight)
        // leaves the next one due, which then stands for both.
        assert!(!c.begin() && !c.begin() && !c.begin());
        assert!(c.begin());
        assert!(c.begin());
        assert_eq!(c.timed(), 5);
        let mut every = Cadence::EVERY_FRAME;
        assert!((0..5).all(|_| every.begin() && every.timed() == 1));
    }

    /// The numbers shown stay correct: a sample standing for 16 frames moves
    /// the smoothed times exactly as far as 16 every-frame samples of the
    /// same time would, so sampled and every-frame timing settle alike.
    #[test]
    fn a_sample_of_sixteen_frames_smooths_like_sixteen_samples() {
        let span = |name, ms| PassSpan {
            name,
            start_ms: 0.0,
            ms,
        };
        let start = [span("shadows", 1.0), span("opaque", 9.0)];
        let now = [span("shadows", 3.0), span("opaque", 5.0)];
        let (mut every, mut sampled, mut scratch) = (Vec::new(), Vec::new(), Vec::new());
        smooth_passes(&mut every, &mut scratch, &start, 0.1);
        smooth_passes(&mut sampled, &mut scratch, &start, 0.1);
        let mut every_ms = 10.0f32;
        for _ in 0..16 {
            smooth_passes(&mut every, &mut scratch, &now, 0.1);
            every_ms += (8.0 - every_ms) * 0.1;
        }
        smooth_passes(&mut sampled, &mut scratch, &now, weight(16));
        let sampled_ms = 10.0 + (8.0 - 10.0) * weight(16);
        assert!((every_ms - sampled_ms).abs() < 1e-4, "{every_ms} vs {sampled_ms}");
        for (e, s) in every.iter().zip(&sampled) {
            assert_eq!(e.name, s.name);
            assert!((e.ms - s.ms).abs() < 1e-4, "{}: {} vs {}", e.name, e.ms, s.ms);
        }
        assert!(
            (weight(1) - 0.1).abs() < 1e-6,
            "a frame of its own weighs 0.1, as before"
        );
        assert!(weight(0) == weight(1) && weight(1000) == weight(64) && weight(64) < 1.0);
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
        assert_eq!(GpuTimer::why_none(&device), crate::perf::GpuUntimed::NoTimestamps);
    }

    /// A timer says why it has no usable time: not ready before the first
    /// readback and while every readback slot is in flight, off when set
    /// to time no frame. Offscreen, no window; skips without a GPU or
    /// without timestamp queries.
    #[cfg(feature = "3d")]
    #[test]
    fn a_timer_says_why_the_latest_frame_has_no_gpu_time() {
        use crate::perf::GpuUntimed;
        let Some((device, queue)) = device(wgpu::Features::TIMESTAMP_QUERY) else {
            return;
        };
        let Some(mut timer) = GpuTimer::new(&device, &queue) else {
            crate::render::render_test_support::skip("needs timestamp queries");
            return;
        };
        assert_eq!(timer.untimed(), Some(GpuUntimed::NotReady), "nothing came back yet");
        timer.frames_read = 1;
        assert_eq!(timer.untimed(), None);
        // Every slot in flight: the frame due to be timed can't be.
        for slot in &timer.slots {
            slot.state.store(RUNNING, Ordering::Release);
        }
        timer.begin_frame();
        assert!(timer.current.is_none());
        assert_eq!(timer.untimed(), Some(GpuUntimed::NotReady), "starved");
        for slot in &timer.slots {
            slot.state.store(FREE, Ordering::Release);
        }
        timer.set_every(0);
        assert_eq!(timer.untimed(), Some(GpuUntimed::Off));
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

    /// Frames between the timed ones write no timestamps (each timed pass
    /// costs the graphics library allocations), and every timed frame's
    /// times come back. Offscreen, no window; skips without a GPU or
    /// without timestamp queries.
    #[cfg(feature = "3d")]
    #[test]
    fn a_timer_of_every_fourth_frame_writes_timestamps_on_those_frames_only() {
        let Some((device, queue)) = device(wgpu::Features::TIMESTAMP_QUERY) else {
            return;
        };
        let Some(mut timer) = GpuTimer::new(&device, &queue) else {
            crate::render::render_test_support::skip("needs timestamp queries");
            return;
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let run = |timer: &mut GpuTimer, frames: usize| {
            let mut timed = Vec::new();
            for _ in 0..frames {
                timer.collect(&device);
                timer.begin_frame();
                let mut encoder = device.create_command_encoder(&Default::default());
                let writes = timer.pass("clear");
                timed.push(writes.is_some());
                drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: writes,
                    occlusion_query_set: None,
                    multiview_mask: None,
                }));
                timer.resolve(&mut encoder);
                queue.submit([encoder.finish()]);
                timer.submitted();
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
            timer.collect(&device);
            timed
        };
        timer.set_every(4);
        let timed = run(&mut timer, 24);
        assert_eq!(timed, [false, false, false, true].repeat(6));
        // A pass that only clears can end on the tick it began (some GPUs),
        // and such a frame isn't counted: at most the 6 timed frames read.
        let read = timer.frames_read;
        assert!((1..=6).contains(&read), "{read} frames read of 6 timed");
        assert!(timer.smoothed_ms.is_some_and(|ms| ms.is_finite() && ms >= 0.0));
        assert_eq!(timer.passes.iter().map(|p| p.name).collect::<Vec<_>>(), ["clear"]);
        // Back to every frame (as the HUD asks): every frame is timed again.
        timer.set_every(1);
        assert!(run(&mut timer, 8).iter().all(|&t| t));
        assert!(
            (read + 1..=read + 8).contains(&timer.frames_read),
            "{}",
            timer.frames_read
        );
    }

    /// A compute pass with real work, between two render passes, gets a
    /// time above zero, and the frame's passes add up to no more than its
    /// GPU time. Metal used to read 0 for it: the timestamps were copied out
    /// in the frame's own command buffer, before the GPU had written the
    /// compute pass's, so they were the previous frame's and were dropped.
    /// Offscreen, no window; skips without a GPU or timestamp queries.
    #[cfg(feature = "3d")]
    #[test]
    fn a_compute_pass_with_real_work_gets_a_time_within_the_frame() {
        let Some((device, queue)) = device(wgpu::Features::TIMESTAMP_QUERY) else {
            return;
        };
        let Some(mut timer) = GpuTimer::new(&device, &queue) else {
            crate::render::render_test_support::skip("needs timestamp queries");
            return;
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("busy work"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;
@compute @workgroup_size(64) fn busy(@builtin(global_invocation_id) id: vec3<u32>) {
    var x = f32(id.x);
    for (var i = 0u; i < 2000u; i++) { x = sin(x) * 1.0001 + 0.5; }
    data[id.x] = x;
}
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    var x = p.x;
    for (var i = 0u; i < 200u; i++) { x = sin(x) * 1.0001 + 0.5; }
    return vec4<f32>(x, 0.0, 0.0, 1.0);
}
"#
                .into(),
            ),
        });
        let invocations = 1u32 << 17;
        let data = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(invocations) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("busy"),
            compilation_options: Default::default(),
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &compute.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: data.as_entire_binding(),
            }],
        });
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 512,
                height: 512,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        fn render(
            encoder: &mut wgpu::CommandEncoder,
            view: &wgpu::TextureView,
            draw: &wgpu::RenderPipeline,
            writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
        ) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: writes,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(draw);
            pass.draw(0..3, 0..1);
        }
        let mut checked = 0;
        for frame in 0..12 {
            timer.collect(&device);
            timer.begin_frame();
            let mut encoder = device.create_command_encoder(&Default::default());
            render(&mut encoder, &view, &draw, timer.pass("draw before"));
            {
                let writes = timer.pass("busy compute").map(|t| wgpu::ComputePassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                    end_of_pass_write_index: t.end_of_pass_write_index,
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: writes,
                });
                pass.set_pipeline(&compute);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(invocations / 64, 1, 1);
            }
            render(&mut encoder, &view, &draw, timer.pass("draw after"));
            timer.resolve(&mut encoder);
            queue.submit([encoder.finish()]);
            timer.submitted();
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            // The first frames' times come back from frame 2 on.
            if frame < 3 {
                continue;
            }
            let (frame_ms, _) = timer.latest_frame().expect("a frame's times came back");
            let spans = &timer.last_frame;
            let busy = spans.iter().find(|s| s.name == "busy compute");
            eprintln!("frame {frame_ms:.3} ms: {spans:?}, not timed {:?}", timer.not_timed);
            let busy = busy.expect("the compute pass is timed, not left out");
            assert!(busy.ms > 0.0, "the compute pass reads 0 ms: {spans:?}");
            assert!(timer.not_timed.is_empty(), "{:?}", timer.not_timed);
            let sum: f32 = spans.iter().map(|s| s.ms).sum();
            assert!(
                sum <= frame_ms * 1.0001 + 1e-4,
                "passes {sum} ms in a {frame_ms} ms frame"
            );
            checked += 1;
        }
        assert!(checked >= 8, "{checked} frames checked");
    }

    /// More devices with timers than Metal has query sets for: 20 devices,
    /// each with a frame timer and an upload timer, ask for 40 sets, where a
    /// process may hold 32 (`query_sets.rs`; before, the timers took 7 sets
    /// a device, so the fifth device was lost). No device is lost: on Metal
    /// the timers past the engine's 28 sets are refused before a set is
    /// made, and every device, timed or not, still draws. Elsewhere every
    /// timer is made. Offscreen, no window; skips without a GPU or
    /// timestamp queries.
    #[cfg(feature = "3d")]
    #[test]
    fn more_timed_devices_than_metal_has_query_sets_for_all_keep_working() {
        use super::super::query_sets::{ENGINE_SETS, refuse_when_full};
        use super::super::upload_timer::UploadTimer;
        use std::sync::atomic::AtomicBool;
        const DEVICES: usize = 20;
        // As in a game: refused at once, not after waiting for other tests.
        refuse_when_full(true);
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                refuse_when_full(false);
            }
        }
        let _restore = Restore;
        let mut devices = Vec::new();
        for _ in 0..DEVICES {
            let Some((device, queue)) = device(wgpu::Features::TIMESTAMP_QUERY) else {
                return;
            };
            if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
                crate::render::render_test_support::skip("needs timestamp queries");
                return;
            }
            let lost = Arc::new(AtomicBool::new(false));
            let flag = lost.clone();
            device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
            let timer = GpuTimer::new(&device, &queue);
            let uploads = UploadTimer::new(&device, &queue);
            devices.push((device, queue, lost, timer, uploads));
        }
        let made = devices
            .iter()
            .map(|d| usize::from(d.3.is_some()) + usize::from(d.4.is_some()))
            .sum::<usize>();
        if devices[0].0.adapter_info().backend == wgpu::Backend::Metal {
            // Other tests may hold some of the 28 right now.
            assert!(made <= ENGINE_SETS as usize, "{made} timers made");
            assert!(
                devices.iter().any(|d| d.3.is_none()),
                "some devices go without a frame timer"
            );
            // And say why: the budget, not a lack of timestamps.
            for d in devices.iter().filter(|d| d.3.is_none()) {
                assert_eq!(GpuTimer::why_none(&d.0), crate::perf::GpuUntimed::QuerySetBudget);
            }
        } else {
            assert_eq!(made, DEVICES * 2, "no limit is known on this backend");
        }
        for (i, (device, queue, lost, timer, _)) in devices.iter_mut().enumerate() {
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            if let Some(timer) = timer.as_mut() {
                timer.begin_frame();
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timer.as_ref().and_then(|t| t.pass("clear")),
                occlusion_query_set: None,
                multiview_mask: None,
            }));
            if let Some(timer) = timer.as_mut() {
                timer.resolve(&mut encoder);
            }
            queue.submit([encoder.finish()]);
            if let Some(timer) = timer.as_mut() {
                timer.submitted();
            }
            assert!(device.poll(wgpu::PollType::wait_indefinitely()).is_ok(), "device {i}");
            assert!(!lost.load(Ordering::Acquire), "device {i} was lost");
        }
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
