//! [`Perf`], the performance service at `ctx.perf`: where a frame's time
//! and allocations go.
//!
//! **Maturity: tested**, from the guide chapter `profiling.md`. Game: none. Tests:
//! `tests/profiling.rs`, `tests/perf_report.rs`, `pass_timing_render_test.rs`. Platforms: Linux,
//! macOS and Windows, headless (the workspace tests in the list of checks). Graphics: offscreen
//! render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on Linux and
//! macOS only where a GPU is found. Missing: a reference game that uses it.
//!
//! **What lives here:** each system's time ([`Perf::system_timings`]), the
//! last frame's drawing cost ([`Perf::render_stats`], [`Perf::gpu_passes`]),
//! frame-time statistics ([`Perf::frame_stats`]), the on-screen HUD
//! ([`Perf::set_hud`]), Perfetto traces ([`Perf::record_trace`]), the deeper
//! profiling switch ([`Perf::set_profiling`]) with named scopes
//! ([`Perf::scope`], [`Perf::scope_timings`]) and the report that sums it all
//! up ([`Perf::report`]).
//!
//! **When to call it:** reads are valid in any system and describe the
//! frame (or headless tick) before the current one. Switches (`set_hud`,
//! `set_profiling`, `record_trace`) take effect from the next frame; flip
//! them from a frame system or setup.
//!
//! **What it needs:** system timings, scopes and the report work headless.
//! Render statistics, GPU passes, frame statistics, the HUD and traces need
//! a window (feature `window`): without one they are zero, empty or do
//! nothing. GPU pass times also need a GPU with timestamp queries.

use std::path::PathBuf;

use crate::render::RenderStats;

/// How long one system took. See [`Perf::system_timings`].
#[derive(Debug, Clone, PartialEq)]
pub struct SystemTiming {
    /// The system's function name, e.g. `my_game::movement`.
    pub name: &'static str,
    /// `true` for simulation systems (they may run several times per frame).
    pub sim: bool,
    /// Time spent in the system during the last frame (all fixed steps
    /// together), in milliseconds.
    pub last_ms: f32,
    /// Smoothed average of `last_ms`.
    pub avg_ms: f32,
}

/// Timings, statistics and profiling switches: `ctx.perf`.
///
/// - **What lives here:** each system's time, the last frame's drawing cost
///   and GPU passes, frame-time statistics, the on-screen HUD, Perfetto
///   traces, the profiling switch with named scopes, and the report that
///   sums it all up.
/// - **When to call it:** reads work in any system and describe the frame
///   (or headless tick) before the current one. Switches (`set_hud`,
///   `set_profiling`, `record_trace`) apply from the next frame.
/// - **What it needs:** system timings, scopes and the report work
///   headless. Render statistics, GPU passes, frame statistics, the HUD and
///   traces need a window; without one they are zero, empty or do nothing.
///
/// ```
/// use ponko::prelude::*;
///
/// fn report_slow_systems(ctx: &mut Ctx) {
///     for t in ctx.perf.system_timings() {
///         if t.avg_ms > 1.0 {
///             println!("{}: {:.2} ms", t.name, t.avg_ms);
///         }
///     }
/// }
/// Game::new("t", 1, 1).sim_system(report_slow_systems).run_headless(2, |_| {});
/// ```
pub struct Perf {
    pub(crate) timings: Vec<SystemTiming>,
    /// Each timed system's place in the schedule, parallel to `timings`.
    pub(crate) schedule: Vec<crate::diagnostics::ScheduleSlot>,
    /// What the schedule plan is built from when diagnostics ask for it.
    pub(crate) plan_source: crate::diagnostics::PlanSource,
    /// How batches were dispatched, parallel to `timings`: a batch counts
    /// at the index of its first system.
    pub(crate) dispatch: Vec<crate::diagnostics::Dispatch>,
    pub(crate) last_render_stats: RenderStats,
    /// The renderer's extra numbers for the last drawn frame, `None`
    /// before the first frame and without a renderer.
    pub(crate) last_render_detail: Option<crate::render::RenderDetail>,
    /// Frame statistics, the HUD, traces and benchmark runs.
    pub(crate) profiler: crate::profiler::Profiler,
    /// Scopes, allocation counts and the history behind `report`.
    pub(crate) state: crate::perf_report::PerfState,
    /// The renderer's per-pass GPU times, copied after every frame.
    pub(crate) gpu_passes: Vec<crate::perf::GpuPass>,
    /// The passes the renderer couldn't time, copied after every frame.
    pub(crate) gpu_passes_not_timed: Vec<&'static str>,
    /// Why the latest frame has no usable GPU time, copied after every frame.
    pub(crate) gpu_untimed: Option<GpuUntimed>,
    /// Passes of the latest timed frame the GPU timer had no room for, and
    /// the passes a frame it can time now, copied after every frame.
    pub(crate) gpu_passes_over_limit: u32,
    pub(crate) gpu_pass_limit: u32,
    /// The last [`RENDER_HISTORY`] drawn frames, oldest first, for the
    /// render inspector. Filled in place: nothing is allocated per frame.
    pub(crate) render_history: std::collections::VecDeque<RenderFrame>,
    /// How long the last frame waited for the display, copied after every frame.
    pub(crate) display_wait_ms: f32,
    /// The GPU memory the renderer held when last counted (while profiling
    /// is on, about twice a second), for the report.
    pub(crate) gpu_memory: Option<crate::perf::GpuMemory>,
    /// How often the game wants its GPU passes timed.
    pub(crate) gpu_timing: GpuTiming,
    /// No window or GPU: there are no frames to trace or draw statistics for.
    pub(crate) headless: bool,
    /// Entities alive when the last system finished, for the report.
    pub(crate) entities: usize,
}

/// Outside profiling, GPU passes are timed on one frame in this many: the
/// timestamp writes cost the graphics library a few heap allocations for
/// every timed pass. See [`Perf::gpu_passes`].
pub(crate) const GPU_TIMING_EVERY: u32 = 16;

/// How often a game times its GPU passes ([`Perf::set_gpu_timing`]).
///
/// Timing a pass costs the graphics library a few heap allocations, so the
/// default times one frame in 16. A game that wants fresher
/// [`gpu_passes`](Perf::gpu_passes), or none at all, says so here, without
/// turning on profiling (which also times scopes and counts allocations).
/// The overlay, traces, benchmark runs and automatic quality read every
/// frame's times, so they get every frame while they are on, whatever this
/// says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuTiming {
    /// The engine decides: one frame in 16, and every frame while something
    /// reads every frame, including [`Perf::set_profiling`].
    #[default]
    Auto,
    /// Times every `n`th frame (`Every(1)` is every frame; 0 counts as 1),
    /// also while profiling is on.
    Every(u32),
    /// Never times the GPU: [`gpu_passes`](Perf::gpu_passes) is empty and
    /// `gpu_ms` is `None`, except while the overlay, a trace, a benchmark run
    /// or automatic quality reads the times.
    Off,
}

/// How many drawn frames the render inspector's history keeps.
pub(crate) const RENDER_HISTORY: usize = 120;

/// Why a headless game refuses to record a trace.
pub(crate) const HEADLESS_TRACE: &str = "headless games have no frames to trace";

/// One drawn frame's rendering cost, as the render inspector's history
/// keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RenderFrame {
    /// [`Time::frame`](crate::time::Time::frame) of the frame.
    pub frame: u64,
    pub cpu_ms: f32,
    /// The GPU time smoothed over recent frames, as it stood after this
    /// frame; `None` without timestamp queries or before the first timed
    /// frame.
    pub gpu_smoothed_ms: Option<f32>,
    /// The newest single frame's own GPU time that had come back by then,
    /// and how many drawn frames before this one it was drawn (timestamps
    /// come back late): GPU time can't be known for the frame itself yet.
    pub gpu_latest: Option<(f32, u64)>,
    pub draw_calls: usize,
    pub models: usize,
    pub triangles: usize,
    pub shadow_triangles: usize,
    pub upload_bytes: u64,
}

impl Perf {
    /// Adds a drawn frame to the render inspector's history, dropping the
    /// oldest once [`RENDER_HISTORY`] are kept.
    pub(crate) fn push_render_frame(&mut self, frame: u64) {
        let s = &self.last_render_stats;
        let entry = RenderFrame {
            frame,
            cpu_ms: s.cpu_ms,
            gpu_smoothed_ms: s.gpu_ms,
            gpu_latest: self.last_render_detail.and_then(|d| d.gpu_latest),
            draw_calls: s.draw_calls,
            models: s.instances,
            triangles: s.triangles,
            shadow_triangles: s.shadow_triangles,
            upload_bytes: s.uploads.bytes(),
        };
        if self.render_history.capacity() < RENDER_HISTORY {
            // Once, at the first drawn frame.
            self.render_history.reserve_exact(RENDER_HISTORY);
        }
        if self.render_history.len() == RENDER_HISTORY {
            self.render_history.pop_front();
        }
        self.render_history.push_back(entry);
    }

    pub(crate) fn new() -> Self {
        Self {
            timings: Vec::new(),
            schedule: Vec::new(),
            plan_source: crate::diagnostics::PlanSource::default(),
            dispatch: Vec::new(),
            last_render_stats: RenderStats::default(),
            last_render_detail: None,
            profiler: crate::profiler::Profiler::new(),
            state: crate::perf_report::PerfState::new(),
            gpu_passes: Vec::new(),
            gpu_passes_not_timed: Vec::new(),
            gpu_untimed: Some(GpuUntimed::NoGpu),
            gpu_passes_over_limit: 0,
            gpu_pass_limit: 0,
            render_history: std::collections::VecDeque::new(),
            display_wait_ms: 0.0,
            gpu_memory: None,
            gpu_timing: GpuTiming::Auto,
            headless: false,
            entities: 0,
        }
    }

    /// How long each system took, in the order they run (simulation systems
    /// first). For finding what's slow; the inspector will graph these.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn print_slow_systems(ctx: &mut Ctx) {
    ///     for t in ctx.perf.system_timings() {
    ///         if t.avg_ms > 1.0 {
    ///             println!("{}: {:.2} ms", t.name, t.avg_ms);
    ///         }
    ///     }
    /// }
    /// ```
    pub fn system_timings(&self) -> &[SystemTiming] {
        &self.timings
    }

    /// What drawing the previous frame cost: sprites, draw calls, CPU time
    /// and GPU time. All zero (and `gpu_ms` is `None`) in headless runs.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn overlay(ctx: &mut Ctx) {
    ///     let stats = ctx.perf.render_stats();
    ///     if let Some(gpu) = stats.gpu_ms {
    ///         println!("{} sprites: {:.2} ms CPU, {gpu:.2} ms GPU", stats.sprites, stats.cpu_ms);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(overlay).run_headless(1, |_| {});
    /// ```
    pub fn render_stats(&self) -> RenderStats {
        self.last_render_stats
    }

    /// Frame-time statistics over the last few seconds: frames per second,
    /// average and percentile frame times, and where the time went (systems,
    /// drawing on the CPU, waiting for the display, the GPU). Refreshed four
    /// times a second. All zero in headless runs.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn report(ctx: &mut Ctx) {
    ///     let s = ctx.perf.frame_stats();
    ///     if s.p99_ms > 2.0 * s.p50_ms {
    ///         println!("stutter: p99 {:.1} ms vs median {:.1} ms", s.p99_ms, s.p50_ms);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(report).run_headless(1, |_| {});
    /// ```
    pub fn frame_stats(&self) -> crate::perf::FrameStats {
        self.profiler.stats()
    }

    /// The GPU time of each pass of the last frames (shadow cascades, depth
    /// prepass, opaque, transparent, bloom, post, ...), smoothed, in the
    /// order they run. Empty in headless runs and on GPUs without timestamp
    /// queries.
    ///
    /// Timing passes costs the graphics library a few heap allocations per
    /// pass, so the GPU is timed on every 16th frame only, except while
    /// something reads every frame: profiling ([`set_profiling`](Self::set_profiling)),
    /// the HUD ([`set_hud`](Self::set_hud)), a trace
    /// ([`record_trace`](Self::record_trace)), a `PONKO_BENCH` run or
    /// automatic quality. Each sample then weighs as much as the frames it
    /// stands for, so the smoothed times settle where every-frame timing
    /// would put them; they only follow a change a little later.
    /// [`set_gpu_timing`](Self::set_gpu_timing) picks the cadence instead.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn slowest_pass(ctx: &mut Ctx) {
    ///     if let Some(p) = ctx.perf.gpu_passes().iter().max_by(|a, b| a.ms.total_cmp(&b.ms)) {
    ///         println!("slowest GPU pass: {} ({:.2} ms)", p.name, p.ms);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(slowest_pass).run_headless(1, |_| {});
    /// ```
    pub fn gpu_passes(&self) -> &[crate::perf::GpuPass] {
        &self.gpu_passes
    }

    /// Chooses how often the GPU passes are timed: [`GpuTiming::Auto`] (the
    /// default), every `n`th frame, or never. It is separate from
    /// [`set_profiling`](Self::set_profiling): a game that wants fresh
    /// [`gpu_passes`](Self::gpu_passes) every frame says
    /// `GpuTiming::Every(1)` and pays only for the timestamps, not for
    /// timing scopes and counting allocations. The change takes effect on
    /// the next frame. The overlay, traces, benchmark runs and automatic
    /// quality still get every frame's times while they are on.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// use ponko::perf::GpuTiming;
    /// fn fresh_gpu_times(ctx: &mut Ctx) {
    ///     ctx.perf.set_gpu_timing(GpuTiming::Every(1));
    /// }
    /// # Game::new("t", 1, 1).system(fresh_gpu_times).run_headless(1, |_| {});
    /// ```
    pub fn set_gpu_timing(&mut self, timing: GpuTiming) {
        self.gpu_timing = timing;
    }

    /// What [`set_gpu_timing`](Self::set_gpu_timing) chose (not what the
    /// overlay or a trace may force meanwhile).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// use ponko::perf::GpuTiming;
    /// # fn f(ctx: &mut Ctx) {
    /// assert_eq!(ctx.perf.gpu_timing(), GpuTiming::Auto);
    /// # }
    /// ```
    pub fn gpu_timing(&self) -> GpuTiming {
        self.gpu_timing
    }

    /// The passes of the latest timed frame that ran but couldn't be timed:
    /// their timestamps didn't come back usable (the GPU skipped a pass with
    /// nothing to do, or the backend writes none for that kind of pass).
    /// They are left out of [`gpu_passes`](Self::gpu_passes) rather than
    /// shown as 0 ms; the overlay, [`report`](Self::report) and traces list
    /// them as not timed. Empty without timestamp queries (then nothing is
    /// timed at all) and when every pass was timed.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn untimed(ctx: &mut Ctx) {
    ///     for name in ctx.perf.gpu_passes_not_timed() {
    ///         println!("{name}: not timed");
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(untimed).run_headless(1, |_| {});
    /// ```
    pub fn gpu_passes_not_timed(&self) -> &[&'static str] {
        &self.gpu_passes_not_timed
    }

    /// How many passes of the latest timed frame the GPU timer had no room
    /// for: they drew untimed and are in neither
    /// [`gpu_passes`](Self::gpu_passes) nor
    /// [`gpu_passes_not_timed`](Self::gpu_passes_not_timed). The timer
    /// grows to the passes a frame has, up to a limit the query sets set
    /// (336 passes a frame), so this is 0 unless a frame has more passes
    /// than that, and for the frame or two while the timer grows. The
    /// overlay, [`report`](Self::report) and the HUD say it when it isn't 0.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn untimed(ctx: &mut Ctx) {
    ///     if ctx.perf.gpu_passes_over_limit() > 0 {
    ///         println!("{} passes weren't timed", ctx.perf.gpu_passes_over_limit());
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(untimed).run_headless(1, |_| {});
    /// ```
    pub fn gpu_passes_over_limit(&self) -> u32 {
        self.gpu_passes_over_limit
    }

    /// Why the latest frame has no usable GPU time, or `None` when it has:
    /// no GPU (headless), no timestamp queries, the timer refused by the
    /// query-set budget, timing turned off, timestamps not back yet, or the
    /// device lost ([`GpuUntimed`]). While it is `Some`, `gpu_ms` is `None`
    /// or the GPU times shown are from an earlier frame. The overlay, the
    /// report's warnings, the render-pass inspector and the editor's GPU
    /// panel say the same.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// use ponko::perf::GpuUntimed;
    /// fn why(ctx: &mut Ctx) {
    ///     // Headless: there is no GPU to time.
    ///     assert_eq!(ctx.perf.gpu_untimed(), Some(GpuUntimed::NoGpu));
    /// }
    /// # Game::new("t", 1, 1).system(why).run_headless(1, |_| {});
    /// ```
    pub fn gpu_untimed(&self) -> Option<GpuUntimed> {
        self.gpu_untimed
    }

    /// How long the last frame waited for the display (ms): idle time, not
    /// work. For benchmarks that subtract it from the frame time.
    #[doc(hidden)]
    #[must_use]
    pub fn display_wait_ms(&self) -> f32 {
        self.display_wait_ms
    }

    /// Shows or hides the performance HUD: frame times, CPU and GPU time,
    /// the slowest GPU passes and systems, drawn in the top-left corner.
    /// `PONKO_HUD=1` shows it from the start.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn toggle_hud(ctx: &mut Ctx) {
    ///     if ctx.input.key_pressed(Key::F3) {
    ///         let on = ctx.perf.hud();
    ///         ctx.perf.set_hud(!on);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(toggle_hud).run_headless(1, |_| {});
    /// ```
    pub fn set_hud(&mut self, on: bool) {
        self.profiler.hud = on;
        self.sync_profiling();
    }

    /// Whether the performance HUD is showing.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let on = ctx.perf.hud();
    /// # }
    /// ```
    pub fn hud(&self) -> bool {
        self.profiler.hud
    }

    /// Records the next `frames` frames as a Chrome trace at `path`: each
    /// frame's systems, drawing and display wait on one track, and the GPU
    /// passes on another. Open it in <https://ui.perfetto.dev>. Does nothing
    /// in headless runs.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn capture(ctx: &mut Ctx) {
    ///     if ctx.input.key_pressed(Key::F4) {
    ///         ctx.perf.record_trace("frames.json", 120);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(capture).run_headless(1, |_| {});
    /// ```
    pub fn record_trace(&mut self, path: impl Into<PathBuf>, frames: usize) {
        if !self.headless {
            self.profiler.start_trace(path.into(), frames.max(1));
        }
    }

    /// [`record_trace`](Self::record_trace) into a file the control
    /// endpoint claimed. Refused (and the file removed) where nothing would
    /// write it: headless.
    pub(crate) fn record_claimed_trace(
        &mut self,
        claimed: crate::control::ClaimedFile,
        frames: usize,
    ) -> Result<(), String> {
        if self.headless {
            claimed.abandon();
            return Err(HEADLESS_TRACE.into());
        }
        self.profiler.start_claimed_trace(claimed, frames.max(1));
        Ok(())
    }

    /// Turns on the deeper profiling: your [`profile_scope!`](crate::profile_scope)
    /// scopes and [`Perf::scope`] calls are timed, allocations are counted
    /// (if [`CountingAllocator`] is installed), and
    /// [`report`](Self::report) has numbers. The overlay
    /// ([`set_hud`](Self::set_hud)) and `PONKO_BENCH` switch it on by themselves. While
    /// it is off, scopes cost one atomic load.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn debug_keys(ctx: &mut Ctx) {
    ///     if ctx.input.key_pressed(Key::F5) {
    ///         let on = !ctx.perf.profiling();
    ///         ctx.perf.set_profiling(on);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(debug_keys).run_headless(1, |_| {});
    /// ```
    pub fn set_profiling(&mut self, on: bool) {
        self.state.explicit = on;
        self.sync_profiling();
    }

    /// Whether scopes are being timed and allocations counted right now.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// if ctx.perf.profiling() { /* build an expensive debug string */ }
    /// # }
    /// ```
    pub fn profiling(&self) -> bool {
        self.state.on
    }

    /// Times the code until the returned guard is dropped, as a scope called
    /// `name` (see [`profile_scope!`](crate::profile_scope), which is cheaper
    /// in hot code: this looks the name up each time). Times add up across
    /// calls. The guard borrows nothing, so the code it times is free to use
    /// all of `ctx`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn ai(ctx: &mut Ctx) {
    ///     let _timing = ctx.perf.scope("ai/pathing");
    ///     for _t in ctx.world.query::<&mut ponko::d2::Transform>().iter() { /* ... */ }
    /// }
    /// # Game::new("t", 1, 1).sim_system(ai).run_headless(1, |_| {});
    /// ```
    pub fn scope(&self, name: &str) -> crate::scope::ScopeGuard {
        if !crate::scope::active() {
            return crate::scope::ScopeGuard::off();
        }
        crate::perf::scope_named(name).enter()
    }

    /// Each profiling scope's time in the last frame, in the order they
    /// first ran. Empty while profiling is off.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn report(ctx: &mut Ctx) {
    ///     for s in ctx.perf.scope_timings() {
    ///         println!("{}: {:.2} ms in {} calls", s.name, s.last_ms, s.calls);
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(report).run_headless(1, |_| {});
    /// ```
    pub fn scope_timings(&self) -> &[crate::scope::ScopeTiming] {
        &self.state.scopes.timings
    }

    /// Where time and allocations went over the last frames (about half a
    /// minute in a window; every tick of a [`Game::profile_headless`](crate::game::Game::profile_headless) run):
    /// frame-time percentiles, systems, scopes, allocations, the slowest
    /// frames and what was in them, drawing stats, and warnings with advice.
    /// `println!("{report}")` prints it. Empty until profiling is on (see
    /// [`set_profiling`](Self::set_profiling)).
    ///
    /// Its `entities` count is from the last time commands were applied
    /// (after each system and at the end of the frame), so entities the
    /// running system spawned straight into `ctx.world` aren't in it yet.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// fn dump(ctx: &mut Ctx) {
    ///     if ctx.input.key_pressed(Key::F6) {
    ///         println!("{}", ctx.perf.report());
    ///     }
    /// }
    /// # Game::new("t", 1, 1).system(dump).run_headless(1, |_| {});
    /// ```
    pub fn report(&self) -> crate::perf::PerfReport {
        let mut report = self.state.collector.report();
        report.entities = self.entities;
        if !self.headless && report.frames > 0 {
            let f = self.profiler.stats();
            let r = self.last_render_stats;
            let mut passes: Vec<_> = self
                .gpu_passes
                .iter()
                .map(|p| crate::perf::PassReport {
                    name: p.name.into(),
                    ms: p.ms,
                })
                .collect();
            passes.sort_by(|a, b| b.ms.total_cmp(&a.ms));
            report.render = Some(crate::perf::RenderReport {
                systems_ms: f.systems_ms,
                render_cpu_ms: f.render_cpu_ms,
                display_wait_ms: f.display_wait_ms,
                gpu_ms: f.gpu_ms,
                draw_calls: r.draw_calls,
                sprites: r.sprites,
                models: r.instances,
                triangles: r.triangles,
                shadow_models: r.shadow_instances,
                shadow_triangles: r.shadow_triangles,
                occluded: r.occluded,
                gpu_passes: passes,
            });
            report.gpu_memory.clone_from(&self.gpu_memory);
        }
        crate::perf_report::analyze(&mut report);
        if report.render.is_some() {
            crate::perf_report::warn_gpu_untimed(&mut report, self.gpu_untimed);
            crate::perf_report::warn_not_timed(&mut report, &self.gpu_passes_not_timed);
            crate::perf_report::warn_passes_over_limit(&mut report, self.gpu_passes_over_limit, self.gpu_pass_limit);
        }
        report
    }

    /// Which frames' GPU passes are timed: every `n`th, 0 for none. Every
    /// frame while something reads every frame (the HUD, a trace, a
    /// benchmark run, or automatic quality, `autotune`), whatever the game
    /// chose with [`set_gpu_timing`](Self::set_gpu_timing); otherwise what it
    /// chose, and for [`GpuTiming::Auto`] every frame while profiling is on
    /// and [`GPU_TIMING_EVERY`] otherwise.
    pub(crate) fn gpu_timing_every(&self, autotune: bool) -> u32 {
        if autotune || self.profiler.hud || self.profiler.benchmarking() || self.profiler.tracing() {
            return 1;
        }
        match self.gpu_timing {
            GpuTiming::Auto if self.state.explicit => 1,
            GpuTiming::Auto => GPU_TIMING_EVERY,
            GpuTiming::Every(n) => n.max(1),
            GpuTiming::Off => 0,
        }
    }

    /// Starts or stops the deeper profiling to match what's wanted: the
    /// overlay, a benchmark run, or `set_profiling`.
    pub(crate) fn sync_profiling(&mut self) {
        let wanted = self.state.explicit || self.profiler.hud || self.profiler.benchmarking();
        self.state.sync(wanted);
    }

    /// Ends a frame (or headless tick) that took `ms` for the report.
    pub(crate) fn end_frame(&mut self, ms: f32) {
        self.state.end_frame(ms, &self.timings);
    }

    /// Hands one frame's timings to the profiler (`gpu_frame` is the
    /// renderer's GPU spans of a finished frame, if it has one). `true` when
    /// a benchmark run is done and the game should quit.
    pub(crate) fn record_frame(
        &mut self,
        sample: crate::profiler::FrameSample,
        gpu_frame: Option<(&[crate::profiler::PassSpan], u64)>,
    ) -> bool {
        self.profiler.frame_state_changes = self.last_render_detail.and_then(|d| d.state_changes);
        self.profiler.frame_see_through_changes = self.last_render_detail.and_then(|d| d.see_through_changes);
        self.profiler.gpu_untimed = self.gpu_untimed;
        self.profiler.gpu_over_limit = (self.gpu_passes_over_limit, self.gpu_pass_limit);
        let quit = self.profiler.record(
            sample,
            &self.timings,
            &self.gpu_passes,
            &self.gpu_passes_not_timed,
            gpu_frame,
            self.last_render_stats,
        );
        self.end_frame(sample.frame_ms);
        if self.profiler.warmup_ends_now() {
            // A benchmark's report covers the measured frames only.
            self.state.collector.clear();
        }
        if quit {
            let report = self.report();
            eprint!("{report}");
            if let Some(path) = std::env::var_os("PONKO_BENCH_REPORT")
                && let Err(e) = report.save(&path)
            {
                eprintln!("ponko: could not write {}: {e}", std::path::Path::new(&path).display());
            }
        }
        quit
    }
}

// Detailed reports live beside the frame service.
#[doc(inline)]
pub use crate::alloc_count::{
    AllocCounts, CountingAllocator, alloc_counts, net_alloc_bytes, note_allocation, note_deallocation,
    thread_alloc_counts, thread_net_alloc_bytes,
};
pub use crate::perf_report::*;
pub use crate::profiler::{FrameStats, GpuPass};
#[doc(inline)]
pub use crate::scope::{ProfileScope, ScopeGuard, ScopeTiming, scope_named};

/// Named profiling scopes.
pub use crate::profile_scope;

/// A point in time for measuring how long something took, like
/// `std::time::Instant` (which it is, on desktop), but also working in web
/// builds, where `std::time::Instant::now()` panics. Re-exported from
/// `web-time`.
///
/// ```
/// use ponko::prelude::*;
/// let nav = ponko::flow::NavGrid::new(256, 256, 8.0);
/// let mut field = ponko::flow::FlowField::new();
/// let start = ponko::perf::Instant::now();
/// field.compute(&nav, &[vec2(40.0, 40.0)]);
/// println!("flow field in {:.2} ms", start.elapsed().as_secs_f64() * 1000.0);
/// ```
pub use web_time::Instant;

#[cfg(test)]
mod render_history_tests {
    use super::*;

    /// The render inspector's history costs a fixed, small amount: one
    /// allocation at the first drawn frame, never more.
    #[test]
    fn the_render_history_is_a_small_fixed_ring() {
        assert!(
            size_of::<RenderFrame>() <= 88,
            "bytes per kept frame: {}",
            size_of::<RenderFrame>()
        );
        let mut perf = Perf::new();
        assert_eq!(perf.render_history.capacity(), 0, "nothing until a frame is drawn");
        perf.push_render_frame(0);
        let ring = perf.render_history.as_slices().0.as_ptr();
        for frame in 1..1000 {
            perf.push_render_frame(frame);
        }
        assert_eq!(perf.render_history.len(), RENDER_HISTORY);
        // `reserve_exact` may give a little more; what matters is that the
        // first allocation is the only one.
        assert!(perf.render_history.capacity() >= RENDER_HISTORY);
        let (a, b) = perf.render_history.as_slices();
        let base = if b.is_empty() { a.as_ptr() } else { b.as_ptr() };
        assert_eq!(base, ring, "the same allocation, filled in place");
    }
}
