//! Frame profiling: rolling frame-time statistics, per-pass GPU times, an
//! on-screen performance HUD, traces for Perfetto, and timed benchmark runs.
//!
//! Everything here is driven by the windowed game loop, once per frame, and
//! allocates nothing per frame in steady state (a running trace or benchmark
//! reserves its storage when it starts).
//!
//! Environment variables (read when the game starts):
//! - `PONKO_HUD=1`: show the performance HUD.
//! - `PONKO_BENCH=<frames>`: after `PONKO_BENCH_WARMUP` frames (default
//!   240), measure that many frames with vsync off, print a report, write it
//!   as JSON to `PONKO_BENCH_OUT` if set, and quit. With 3D, it also lists
//!   the meshes that drew the most triangles, and writes every mesh's counts
//!   as CSV to `PONKO_BENCH_MESHES` if set. Frames are drawn offscreen, so
//!   the display's refresh rate doesn't cap them; `PONKO_BENCH_PRESENT=1`
//!   shows them in the window instead (where it can take them).
//! - `PONKO_TRACE=<path.json>`: after `PONKO_TRACE_SKIP` frames (default
//!   240), record `PONKO_TRACE_FRAMES` frames (default 120) as a Chrome trace
//!   (open it in <https://ui.perfetto.dev>).
//! - `PONKO_VSYNC=0` or `1`: overrides [`Game::vsync`](crate::game::Game::vsync).

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::perf::SystemTiming;
use crate::render::RenderStats;

/// One GPU pass of a frame, from [`Perf::gpu_passes`](crate::perf::Perf::gpu_passes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuPass {
    /// What the pass draws ("shadows 0", "opaque", "bloom", ...). Passes
    /// with the same name in one frame are added together.
    pub name: &'static str,
    /// Its GPU time in milliseconds, smoothed over recent frames.
    pub ms: f32,
}

/// A pass in one frame's raw GPU timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PassSpan {
    pub name: &'static str,
    /// Start, in ms after the frame's first timestamp.
    pub start_ms: f32,
    pub ms: f32,
}

/// Frame-time statistics over the last few seconds, from
/// [`Perf::frame_stats`](crate::perf::Perf::frame_stats). All times in milliseconds;
/// the CPU and GPU parts are averages.
///
/// What limits the frame rate: if `gpu_ms` is close to `avg_ms`, the GPU;
/// if `systems_ms + render_cpu_ms` is, the CPU; if `display_wait_ms` is
/// large, the display (vsync), and there is room to spare.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameStats {
    /// Frames the statistics cover.
    pub frames: usize,
    /// Frames per second, from the average frame time.
    pub fps: f32,
    /// Average time from one frame to the next.
    pub avg_ms: f32,
    /// Median frame time.
    pub p50_ms: f32,
    /// 95th percentile frame time: 1 frame in 20 is slower.
    pub p95_ms: f32,
    /// 99th percentile frame time: the stutters.
    pub p99_ms: f32,
    /// The slowest frame.
    pub max_ms: f32,
    /// CPU time in systems (game logic, physics, collecting what to draw).
    pub systems_ms: f32,
    /// CPU time spent drawing: preparing GPU data, recording and submitting.
    pub render_cpu_ms: f32,
    /// Time waiting for the display to take a frame (idle, not work).
    pub display_wait_ms: f32,
    /// GPU time of a frame, if the GPU can measure it.
    pub gpu_ms: Option<f32>,
}

/// What the game loop measured for one frame.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameSample {
    /// Since the previous frame started.
    pub frame_ms: f32,
    /// When this frame's systems started, in ms after the frame started.
    pub systems_start_ms: f32,
    pub systems_ms: f32,
    /// When drawing started, in ms after the frame started.
    pub render_start_ms: f32,
    /// Drawing's CPU time, display wait included.
    pub render_ms: f32,
    pub wait_ms: f32,
    pub gpu_ms: Option<f32>,
    /// The frame reached the screen (not skipped or hidden).
    pub presented: bool,
}

/// Frames in the rolling window behind [`FrameStats`].
const WINDOW: usize = 240;
/// Seconds between refreshes of the statistics and the HUD text.
const REFRESH: f32 = 0.25;

pub(crate) struct Profiler {
    ring: Vec<FrameSample>,
    next: usize,
    sorted: Vec<f32>,
    stats: FrameStats,
    since_refresh: f32,
    /// Show the HUD.
    pub hud: bool,
    /// The HUD's text, rebuilt at each refresh.
    hud_text: String,
    trace: Option<Trace>,
    bench: Option<Bench>,
    pub(crate) frame_state_changes: Option<[u32; 5]>,
    /// The see-through list's part of them (see `RenderDetail`).
    pub(crate) frame_see_through_changes: Option<[u32; 5]>,
    /// Why the latest frame has no usable GPU time (the HUD says it).
    pub(crate) gpu_untimed: Option<crate::perf::GpuUntimed>,
    /// Passes of the latest timed frame the GPU timer had no room for, and
    /// the room it has (the HUD says it when not 0).
    pub(crate) gpu_over_limit: (u32, u32),
    /// Frames since the game started.
    frame: u64,
    /// The GPU timer's frame count when last looked at.
    gpu_frames_seen: u64,
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            ring: Vec::with_capacity(WINDOW),
            next: 0,
            sorted: Vec::with_capacity(WINDOW),
            stats: FrameStats::default(),
            since_refresh: 0.0,
            hud: false,
            hud_text: String::with_capacity(2048),
            trace: None,
            bench: None,
            frame_state_changes: None,
            frame_see_through_changes: None,
            gpu_untimed: None,
            gpu_over_limit: (0, 0),
            frame: 0,
            gpu_frames_seen: 0,
        }
    }

    /// Reads the `PONKO_*` settings described above.
    pub fn from_env() -> Self {
        Self::from_settings(|name| std::env::var_os(name))
    }

    fn from_settings(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Self {
        let num = |name: &str| get(name)?.to_str()?.trim().parse::<u64>().ok();
        let mut p = Self::new();
        p.hud = get("PONKO_HUD").is_some_and(|v| v != "0");
        if let Some(frames) = num("PONKO_BENCH").filter(|&n| n > 0) {
            p.bench = Some(Bench::new(
                num("PONKO_BENCH_WARMUP").unwrap_or(240),
                frames as usize,
                get("PONKO_BENCH_OUT").map(PathBuf::from),
            ));
        }
        if let Some(path) = get("PONKO_TRACE") {
            p.trace = Some(Trace::new(
                PathBuf::from(path),
                num("PONKO_TRACE_SKIP").unwrap_or(240),
                num("PONKO_TRACE_FRAMES").unwrap_or(120) as usize,
            ));
        }
        p
    }

    /// A trace is being recorded (it shows every frame's GPU passes).
    pub(crate) fn tracing(&self) -> bool {
        self.trace.is_some()
    }

    /// A benchmark run is set up: the game runs with vsync off.
    pub fn benchmarking(&self) -> bool {
        self.bench.is_some()
    }

    /// The frame just recorded was a benchmark's last warm-up frame.
    pub fn warmup_ends_now(&self) -> bool {
        self.bench.as_ref().is_some_and(|b| b.warmup == self.frame)
    }

    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    pub fn hud_text(&self) -> &str {
        &self.hud_text
    }

    pub fn hud_text_mut(&mut self) -> &mut String {
        &mut self.hud_text
    }

    /// Starts recording `frames` frames into a trace at `path`.
    pub fn start_trace(&mut self, path: PathBuf, frames: usize) {
        self.replace_trace(Trace::new(path, 0, frames));
    }

    /// [`start_trace`](Self::start_trace) into a file a tool claimed: it is
    /// written through that handle, and removed if the trace is replaced
    /// or can't be written.
    pub(crate) fn start_claimed_trace(&mut self, claimed: crate::control::ClaimedFile, frames: usize) {
        let mut trace = Trace::new(claimed.path.clone(), 0, frames);
        trace.claimed = Some(claimed);
        self.replace_trace(trace);
    }

    fn replace_trace(&mut self, trace: Trace) {
        if let Some(old) = self.trace.replace(trace)
            && let Some(claimed) = old.claimed
        {
            // Nobody will write it now.
            claimed.abandon();
        }
    }

    /// Records one frame. `gpu_frame` is the latest GPU timeline and how many
    /// the timer has read so far; a new one is used once. Returns `true`
    /// when a benchmark run has finished and the game should quit.
    pub fn record(
        &mut self,
        sample: FrameSample,
        systems: &[SystemTiming],
        gpu_passes: &[GpuPass],
        not_timed: &[&'static str],
        gpu_frame: Option<(&[PassSpan], u64)>,
        render: RenderStats,
    ) -> bool {
        self.frame += 1;
        if self.ring.len() < WINDOW {
            self.ring.push(sample);
        } else {
            self.ring[self.next] = sample;
        }
        self.next = (self.next + 1) % WINDOW;
        let new_gpu = match gpu_frame {
            Some((spans, read)) if read != self.gpu_frames_seen => {
                self.gpu_frames_seen = read;
                Some(spans)
            }
            _ => None,
        };
        if let Some(trace) = &mut self.trace
            && trace.record(self.frame, &sample, systems, new_gpu.map(|spans| (spans, not_timed)))
        {
            self.trace = None;
        }
        let mut quit = false;
        if let Some(bench) = &mut self.bench
            && bench.record(self.frame, &sample, systems, new_gpu)
        {
            bench.finish(render, self.frame_state_changes, self.frame_see_through_changes);
            self.bench = None;
            quit = true;
        }
        self.since_refresh += sample.frame_ms / 1000.0;
        if self.since_refresh >= REFRESH || self.stats.frames == 0 {
            self.since_refresh = 0.0;
            self.refresh(systems, gpu_passes, not_timed, render);
        }
        quit
    }

    fn refresh(
        &mut self,
        systems: &[SystemTiming],
        gpu_passes: &[GpuPass],
        not_timed: &[&'static str],
        render: RenderStats,
    ) {
        self.stats = summarize(&self.ring, &mut self.sorted);
        if self.hud {
            hud_text(
                &mut self.hud_text,
                &self.stats,
                systems,
                (gpu_passes, not_timed),
                self.gpu_untimed,
                render,
            );
            let (over, limit) = self.gpu_over_limit;
            if over > 0 {
                let _ = writeln!(self.hud_text, "{over} gpu passes not timed (room for {limit})");
            }
        }
    }
}

/// Percentile `p` (0..=1) of sorted `v`, nearest rank.
pub(crate) fn percentile(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let i = ((v.len() as f32 * p).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

/// Statistics over `samples`, using `sorted` as scratch.
fn summarize(samples: &[FrameSample], sorted: &mut Vec<f32>) -> FrameStats {
    let n = samples.len();
    if n == 0 {
        return FrameStats::default();
    }
    sorted.clear();
    sorted.extend(samples.iter().map(|s| s.frame_ms));
    sorted.sort_unstable_by(f32::total_cmp);
    let avg = |f: fn(&FrameSample) -> f32| samples.iter().map(f).sum::<f32>() / n as f32;
    let avg_ms = avg(|s| s.frame_ms);
    let (gpu_sum, gpu_n) = samples
        .iter()
        .filter_map(|s| s.gpu_ms)
        .fold((0.0, 0), |(sum, k), g| (sum + g, k + 1));
    FrameStats {
        frames: n,
        fps: if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 },
        avg_ms,
        p50_ms: percentile(sorted, 0.50),
        p95_ms: percentile(sorted, 0.95),
        p99_ms: percentile(sorted, 0.99),
        max_ms: sorted[n - 1],
        systems_ms: avg(|s| s.systems_ms),
        render_cpu_ms: avg(|s| s.render_ms - s.wait_ms),
        display_wait_ms: avg(|s| s.wait_ms),
        gpu_ms: (gpu_n > 0).then_some(gpu_sum / gpu_n as f32),
    }
}

/// Lines in the HUD for systems and for GPU passes.
const HUD_ROWS: usize = 8;

fn hud_text(
    out: &mut String,
    s: &FrameStats,
    systems: &[SystemTiming],
    (passes, not_timed): (&[GpuPass], &[&'static str]),
    untimed: Option<crate::perf::GpuUntimed>,
    r: RenderStats,
) {
    out.clear();
    let _ = writeln!(
        out,
        "{:.0} fps  {:.2} ms  (p50 {:.2}  p99 {:.2}  max {:.2})",
        s.fps, s.avg_ms, s.p50_ms, s.p99_ms, s.max_ms
    );
    let _ = write!(
        out,
        "cpu: systems {:.2}  render {:.2}  wait {:.2}",
        s.systems_ms, s.render_cpu_ms, s.display_wait_ms
    );
    match (s.gpu_ms, untimed) {
        (Some(g), None) => {
            let _ = writeln!(out, "   gpu {g:.2}");
        }
        // Times from an earlier frame, or none: say why.
        (Some(g), Some(why)) => {
            let _ = writeln!(out, "   gpu {g:.2} (stale: {})", why.name());
        }
        (None, Some(why)) => {
            let _ = writeln!(out, "   gpu not timed: {}", why.name());
        }
        (None, None) => out.push('\n'),
    }
    let _ = writeln!(out, "{} draw calls, {} sprites", r.draw_calls, r.sprites);
    if r.instances + r.shadow_instances > 0 {
        let _ = writeln!(
            out,
            "3d: {} models, {:.2}M tris; shadows {} models, {:.2}M tris\nrender cpu: collect {:.2}  record {:.2}  submit {:.2}",
            r.instances,
            r.triangles as f32 / 1e6,
            r.shadow_instances,
            r.shadow_triangles as f32 / 1e6,
            r.collect_ms,
            r.record_ms,
            r.submit_ms
        );
    }
    if !passes.is_empty() {
        out.push_str("-- gpu passes\n");
        let mut shown = [None; HUD_ROWS];
        top_n(passes.iter().map(|p| p.ms), &mut shown);
        for i in shown.into_iter().flatten() {
            let _ = writeln!(out, "{:>6.2}  {}", passes[i].ms, passes[i].name);
        }
    }
    // Not 0 ms: these ran, but their times didn't come back.
    for name in not_timed.iter().take(HUD_ROWS) {
        let _ = writeln!(out, "     -  {name} (not timed)");
    }
    if !systems.is_empty() {
        out.push_str("-- systems\n");
        let mut shown = [None; HUD_ROWS];
        top_n(systems.iter().map(|t| t.avg_ms), &mut shown);
        for i in shown.into_iter().flatten() {
            let name = systems[i].name;
            // The last two path segments: `game::movement`.
            let short = name.rmatch_indices("::").nth(1).map_or(name, |(at, _)| &name[at + 2..]);
            let _ = writeln!(out, "{:>6.2}  {short}", systems[i].avg_ms);
        }
    }
}

/// Indices of the largest values, largest first, into `out` (no allocation).
fn top_n(values: impl Iterator<Item = f32> + Clone, out: &mut [Option<usize>]) {
    for slot in 0..out.len() {
        let mut best: Option<(usize, f32)> = None;
        for (i, v) in values.clone().enumerate() {
            if out[..slot].contains(&Some(i)) {
                continue;
            }
            if best.is_none_or(|(_, b)| v > b) {
                best = Some((i, v));
            }
        }
        out[slot] = best.map(|(i, _)| i);
    }
}

/// A Chrome trace being recorded.
struct Trace {
    path: PathBuf,
    skip: u64,
    frames: usize,
    taken: usize,
    events: Vec<serde_json::Value>,
    /// Microseconds from the trace's start to the current frame's start.
    clock_us: f64,
    /// The file a tool claimed for it, written through this handle.
    claimed: Option<crate::control::ClaimedFile>,
}

impl Trace {
    fn new(path: PathBuf, skip: u64, frames: usize) -> Self {
        eprintln!(
            "ponko: recording a {frames}-frame trace after frame {skip} into {}",
            path.display()
        );
        Self {
            path,
            skip,
            frames,
            taken: 0,
            events: Vec::new(),
            clock_us: 0.0,
            claimed: None,
        }
    }

    /// Returns `true` once the trace is written.
    fn record(
        &mut self,
        frame: u64,
        s: &FrameSample,
        systems: &[SystemTiming],
        gpu: Option<(&[PassSpan], &[&'static str])>,
    ) -> bool {
        if frame <= self.skip {
            return false;
        }
        if self.taken == 0 {
            self.events.reserve(self.frames * (systems.len() + 40));
        }
        // The frame that just ran started `frame_ms` before the next one;
        // lay frames end to end.
        let t0 = self.clock_us;
        let span = |name: &str, tid: u32, start_ms: f32, ms: f32| {
            serde_json::json!({
                "name": name, "ph": "X", "pid": 1, "tid": tid,
                "ts": t0 + f64::from(start_ms) * 1000.0,
                "dur": f64::from(ms.max(0.0)) * 1000.0,
            })
        };
        let frame_end = s.render_start_ms + s.render_ms;
        self.events.push(span(&format!("frame {frame}"), 1, 0.0, frame_end));
        let mut at = s.systems_start_ms;
        for t in systems {
            if t.last_ms > 0.0 {
                self.events.push(span(t.name, 1, at, t.last_ms));
                at += t.last_ms;
            }
        }
        self.events.push(span("render", 1, s.render_start_ms, s.render_ms));
        self.events.push(span("display wait", 1, s.render_start_ms, s.wait_ms));
        if let Some((spans, not_timed)) = gpu {
            // GPU clocks aren't the CPU's: the passes are placed from this
            // frame's submit on. They belong to a frame a few frames back.
            let base = s.render_start_ms + s.render_ms;
            for p in spans {
                self.events.push(span(p.name, 2, base + p.start_ms, p.ms));
            }
            // Passes that ran but couldn't be timed: a mark, not a 0 ms slice.
            for name in not_timed {
                self.events.push(serde_json::json!({
                    "name": format!("{name} (not timed)"), "ph": "i", "s": "t", "pid": 1, "tid": 2,
                    "ts": t0 + f64::from(base) * 1000.0,
                }));
            }
        }
        self.clock_us += f64::from(s.frame_ms.max(frame_end)) * 1000.0;
        self.taken += 1;
        if self.taken < self.frames {
            return false;
        }
        let names = [(1, "CPU (main thread)"), (2, "GPU (a recent frame's passes)")];
        for (tid, name) in names {
            self.events.push(serde_json::json!({
                "name": "thread_name", "ph": "M", "pid": 1, "tid": tid, "args": { "name": name }
            }));
        }
        let doc = serde_json::json!({ "traceEvents": std::mem::take(&mut self.events), "displayTimeUnit": "ms" });
        let written = match self.claimed.take() {
            Some(mut claimed) => {
                use std::io::Write;
                let result = claimed.file().write_all(doc.to_string().as_bytes());
                if result.is_ok() {
                    claimed.keep();
                }
                // Otherwise the drop removes it.
                result
            }
            None => std::fs::write(&self.path, doc.to_string()),
        };
        match written {
            Ok(()) => eprintln!(
                "ponko: trace written to {} (open it in https://ui.perfetto.dev)",
                self.path.display()
            ),
            Err(e) => eprintln!("ponko: could not write the trace {}: {e}", self.path.display()),
        }
        true
    }
}

/// A benchmark run being measured.
struct Bench {
    warmup: u64,
    frames: usize,
    out: Option<PathBuf>,
    samples: Vec<FrameSample>,
    /// Frames that weren't drawn (window hidden, surface busy): not measured.
    skipped: u32,
    /// Total ms per system over the measured frames.
    systems: Vec<(&'static str, f64)>,
    /// Total ms per GPU pass, and GPU frames read.
    passes: Vec<(&'static str, f64)>,
    gpu_frames: u32,
}

impl Bench {
    fn new(warmup: u64, frames: usize, out: Option<PathBuf>) -> Self {
        eprintln!("ponko: benchmark: {warmup} warm-up frames, then {frames} measured frames, vsync off");
        Self {
            warmup,
            frames,
            out,
            samples: Vec::with_capacity(frames),
            skipped: 0,
            systems: Vec::with_capacity(64),
            passes: Vec::with_capacity(32),
            gpu_frames: 0,
        }
    }

    /// Returns `true` when every frame is measured.
    fn record(&mut self, frame: u64, s: &FrameSample, systems: &[SystemTiming], gpu: Option<&[PassSpan]>) -> bool {
        if frame <= self.warmup {
            return false;
        }
        if !s.presented {
            self.skipped += 1;
            return false;
        }
        self.samples.push(*s);
        for t in systems {
            add(&mut self.systems, t.name, f64::from(t.last_ms));
        }
        if let Some(spans) = gpu {
            self.gpu_frames += 1;
            for p in spans {
                add(&mut self.passes, p.name, f64::from(p.ms));
            }
        }
        self.samples.len() >= self.frames
    }

    fn finish(&mut self, render: RenderStats, state_changes: Option<[u32; 5]>, see_through: Option<[u32; 5]>) {
        let mut sorted = Vec::with_capacity(self.samples.len());
        let s = summarize(&self.samples, &mut sorted);
        let n = self.samples.len() as f64;
        let g = f64::from(self.gpu_frames.max(1));
        self.systems.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut passes = self.passes.clone();
        passes.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut report = String::new();
        let _ = writeln!(report, "\n== ponko benchmark: {} frames ==", s.frames);
        let _ = writeln!(
            report,
            "fps {:.1}   frame avg {:.2} ms   p50 {:.2}   p95 {:.2}   p99 {:.2}   max {:.2}",
            s.fps, s.avg_ms, s.p50_ms, s.p95_ms, s.p99_ms, s.max_ms
        );
        let _ = writeln!(
            report,
            "cpu: systems {:.2} ms   render {:.2} ms   display wait {:.2} ms   gpu {}",
            s.systems_ms,
            s.render_cpu_ms,
            s.display_wait_ms,
            s.gpu_ms.map_or("n/a".into(), |g| format!("{g:.2} ms"))
        );
        let _ = writeln!(report, "draw calls {}   sprites {}", render.draw_calls, render.sprites);
        let _ = writeln!(
            report,
            "3d: camera {} models / {:.2}M tris   shadows {} models / {:.2}M tris\nrender cpu (last frame): collect {:.2} ms   record {:.2} ms   submit {:.2} ms",
            render.instances,
            render.triangles as f64 / 1e6,
            render.shadow_instances,
            render.shadow_triangles as f64 / 1e6,
            render.collect_ms,
            render.record_ms,
            render.submit_ms
        );
        let _ = writeln!(
            report,
            "model state changes (last frame): {}\nof which the see-through list: {}",
            crate::render::state_changes_json(state_changes),
            crate::render::state_changes_json(see_through)
        );
        let u = render.uploads;
        let _ = writeln!(
            report,
            "gpu uploads (last frame): {} bytes ({} instances, {} uniforms, {} lights)   buffers created {} ({} outgrown)",
            u.bytes(),
            u.instances,
            u.uniforms,
            u.lights,
            u.buffers_created,
            u.reallocations
        );
        if self.skipped > 0 {
            let _ = writeln!(report, "({} frames weren't drawn and aren't counted)", self.skipped);
        }
        let _ = writeln!(report, "-- gpu passes (avg ms, each from the previous one's end) --");
        for (name, total) in &passes {
            let _ = writeln!(report, "{:>8.3}  {name}", total / g);
        }
        let _ = writeln!(report, "-- systems (avg ms) --");
        for (name, total) in self.systems.iter().take(15) {
            let _ = writeln!(report, "{:>8.3}  {name}", total / n);
        }
        eprint!("{report}");
        if let Some(path) = &self.out {
            let json = serde_json::json!({
                "frames": s.frames,
                "fps": s.fps,
                "frame_ms": { "avg": s.avg_ms, "p50": s.p50_ms, "p95": s.p95_ms, "p99": s.p99_ms, "max": s.max_ms },
                "cpu_ms": { "systems": s.systems_ms, "render": s.render_cpu_ms, "display_wait": s.display_wait_ms },
                "gpu_ms": s.gpu_ms,
                "state_changes": crate::render::state_changes_json(state_changes),
                "see_through_changes": crate::render::state_changes_json(see_through),
                "draw_calls": render.draw_calls,
                "instances": render.instances,
                "triangles": render.triangles,
                "shadow_instances": render.shadow_instances,
                "shadow_triangles": render.shadow_triangles,
                "collect_ms": render.collect_ms,
                "record_ms": render.record_ms,
                "submit_ms": render.submit_ms,
                "uploads": render.uploads.json(),
                "gpu_passes": passes.iter().map(|(n, t)| serde_json::json!({ "name": n, "ms": t / g })).collect::<Vec<_>>(),
                "systems": self.systems.iter().map(|(name, t)| serde_json::json!({ "name": name, "ms": t / n })).collect::<Vec<_>>(),
            });
            if let Err(e) = std::fs::write(path, serde_json::to_string_pretty(&json).unwrap_or_default()) {
                eprintln!("ponko: could not write {}: {e}", path.display());
            }
        }
    }
}

fn add(totals: &mut Vec<(&'static str, f64)>, name: &'static str, v: f64) {
    match totals.iter_mut().find(|(n, _)| *n == name) {
        Some((_, t)) => *t += v,
        None => totals.push((name, v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &'static [(&'static str, &'static str)]) -> Profiler {
        Profiler::from_settings(|name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).into()))
    }

    #[test]
    fn ponko_bench_sets_up_a_benchmark_run_only_for_a_positive_frame_count() {
        assert!(settings(&[("PONKO_BENCH", "3")]).benchmarking());
        assert!(!settings(&[("PONKO_BENCH", "0")]).benchmarking());
        assert!(!settings(&[("PONKO_BENCH", "")]).benchmarking());
        assert!(!settings(&[]).benchmarking());
    }

    #[test]
    fn ponko_hud_and_ponko_trace_are_read() {
        assert!(settings(&[("PONKO_HUD", "1")]).hud);
        assert!(!settings(&[("PONKO_HUD", "0")]).hud);
        assert!(settings(&[("PONKO_TRACE", "t.json")]).trace.is_some());
        assert!(settings(&[]).trace.is_none());
    }

    fn sample(ms: f32) -> FrameSample {
        FrameSample {
            frame_ms: ms,
            systems_ms: 1.0,
            render_ms: 2.0,
            wait_ms: 0.5,
            gpu_ms: Some(3.0),
            presented: true,
            ..Default::default()
        }
    }

    #[test]
    fn percentiles_follow_the_slow_frames() {
        let mut samples: Vec<_> = (0..99).map(|_| sample(4.0)).collect();
        samples.push(sample(40.0));
        let s = summarize(&samples, &mut Vec::new());
        assert_eq!(s.frames, 100);
        assert_eq!((s.p50_ms, s.p95_ms, s.p99_ms, s.max_ms), (4.0, 4.0, 4.0, 40.0));
        assert!((s.avg_ms - 4.36).abs() < 1e-4);
        assert!((s.render_cpu_ms - 1.5).abs() < 1e-6, "display wait isn't render work");
        assert_eq!(s.gpu_ms, Some(3.0));
    }

    #[test]
    fn the_rolling_window_keeps_the_latest_frames() {
        let mut p = Profiler::new();
        for i in 0..(WINDOW + 10) {
            p.record(sample(i as f32), &[], &[], &[], None, RenderStats::default());
        }
        p.refresh(&[], &[], &[], RenderStats::default());
        assert_eq!(p.stats().frames, WINDOW);
        assert_eq!(p.stats().max_ms, (WINDOW + 9) as f32);
    }

    #[test]
    fn hud_lists_the_slowest_passes_first() {
        let mut text = String::new();
        let passes = [
            GpuPass { name: "post", ms: 0.2 },
            GpuPass {
                name: "opaque",
                ms: 5.0,
            },
        ];
        hud_text(
            &mut text,
            &FrameStats::default(),
            &[],
            (&passes, &[]),
            None,
            RenderStats::default(),
        );
        let opaque = text.find("opaque").unwrap();
        assert!(opaque < text.find("post").unwrap(), "{text}");
    }

    /// A pass that ran but couldn't be timed is listed as such, not as 0 ms.
    #[test]
    fn hud_lists_passes_that_could_not_be_timed() {
        let mut text = String::new();
        let passes = [GpuPass {
            name: "opaque",
            ms: 5.0,
        }];
        hud_text(
            &mut text,
            &FrameStats::default(),
            &[],
            (&passes, &["crowd cull"]),
            None,
            RenderStats::default(),
        );
        assert!(text.contains("crowd cull (not timed)"), "{text}");
        assert!(!text.contains("0.00  crowd cull"), "{text}");
    }

    /// A frame with no usable GPU time says why, instead of leaving the GPU
    /// figure out.
    #[test]
    fn hud_says_why_the_gpu_is_not_timed() {
        let mut text = String::new();
        let untimed = crate::perf::GpuUntimed::QuerySetBudget;
        hud_text(
            &mut text,
            &FrameStats::default(),
            &[],
            (&[], &[]),
            Some(untimed),
            RenderStats::default(),
        );
        assert!(text.contains("gpu not timed: query_set_budget"), "{text}");
        let stats = FrameStats {
            gpu_ms: Some(2.0),
            ..FrameStats::default()
        };
        hud_text(
            &mut text,
            &stats,
            &[],
            (&[], &[]),
            Some(crate::perf::GpuUntimed::DeviceLost),
            RenderStats::default(),
        );
        assert!(text.contains("gpu 2.00 (stale: device_lost)"), "{text}");
        hud_text(&mut text, &stats, &[], (&[], &[]), None, RenderStats::default());
        assert!(text.contains("gpu 2.00\n"), "the control: {text}");
    }

    /// A trace shows timed passes as slices and the ones that couldn't be
    /// timed as marks, never as 0 ms slices.
    #[test]
    fn traces_mark_passes_that_could_not_be_timed() {
        let path = std::env::temp_dir().join(format!("ponko-not-timed-{}.json", std::process::id()));
        let mut t = Trace::new(path.clone(), 0, 2);
        let spans = [PassSpan {
            name: "opaque",
            start_ms: 0.0,
            ms: 2.0,
        }];
        assert!(!t.record(1, &sample(5.0), &[], Some((&spans, &["crowd cull"]))));
        assert!(t.record(2, &sample(5.0), &[], None));
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let events = doc["traceEvents"].as_array().unwrap();
        let named = |n: &str| events.iter().find(|e| e["name"] == n).cloned();
        assert_eq!(named("opaque").unwrap()["ph"], "X");
        let mark = named("crowd cull (not timed)").expect("a mark for the pass");
        assert_eq!(mark["ph"], "i");
        assert!(named("crowd cull").is_none(), "no 0 ms slice");
    }

    #[test]
    fn a_benchmark_ends_after_its_frames() {
        let mut b = Bench::new(2, 3, None);
        let spans = [PassSpan {
            name: "opaque",
            start_ms: 0.0,
            ms: 2.0,
        }];
        let done: Vec<bool> = (1..=5).map(|f| b.record(f, &sample(5.0), &[], Some(&spans))).collect();
        assert_eq!(done, [false, false, false, false, true]);
        assert_eq!(b.passes, [("opaque", 6.0)]);
    }
}
