//! [`Game::run_server`]: the headless loop paced to wall time, with stop
//! signals, a tick-rate report and structured logs. The public types are in
//! [`crate::server`].

use std::time::Duration;

use serde_json::json;
use web_time::Instant;

use ponko_ecs::World;

use super::{ConfiguredGame, Game, SetupResult, connect_hotpatch, end_timings, forget_seen_removals, run_setups};
use crate::ctx::Ctx;
use crate::server::{self, LogLevel, ServerConfig, ServerStatus, StopReason, TickStats, signals};
use crate::time::{Clock, FrameTiming};

/// How long a paused server (control endpoint) waits for tools per pass.
const PAUSED_WAIT: Duration = Duration::from_millis(50);

impl ConfiguredGame {
    /// Runs as a dedicated server with the stored setup callbacks:
    /// [`Game::run_server`].
    ///
    /// # Panics
    /// As [`Game::run_server`].
    pub fn run_server(self, config: ServerConfig) -> World {
        self.game.run_server(config, |_| {})
    }
}

impl Game {
    /// Runs as a dedicated server: headless (no window, GPU or sound, as
    /// [`run_headless`](Self::run_headless)), one tick every `1 /`
    /// [`fixed_hz`](Self::fixed_hz) seconds of wall time, until a stop
    /// signal, a [`StopHandle`](crate::server::StopHandle),
    /// [`max_ticks`](ServerConfig::max_ticks) or `ctx.quit()`. Returns the
    /// world, whose [`ServerStatus`] says why it stopped.
    ///
    /// A stop request sets [`ServerStatus::stopping`] and starts the
    /// [`shutdown_grace`](ServerConfig::shutdown_grace): ticks keep running
    /// so systems can tell clients and save, until they call `ctx.quit()` or
    /// the grace ends. A second signal stops at once. It logs
    /// `server_start`, a `tick_report` every
    /// [`report_every`](ServerConfig::report_every), `server_stopping` and
    /// `server_stopped`, in the [`log_format`](ServerConfig::log_format)
    /// (JSON lines on standard output by default); see [`crate::server`].
    ///
    /// Message endpoints ([`messages`](Self::messages)) are polled before
    /// every tick, so packets wait at most one tick. The
    /// [control endpoint](Self::control_endpoint) works as in a headless
    /// run: tools can pause and step the server.
    ///
    /// With `PONKO_SMOKE=<ticks>` set (what `ponko package` does to check a
    /// package), it runs that many ticks as fast as it can, as
    /// [`run`](Self::run) does, prints `ponko: smoke run passed` and returns.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::server::{ServerConfig, ServerStatus, StopReason};
    ///
    /// let world = Game::new("server", 1, 1)
    ///     .fixed_hz(100)
    ///     .run_server(ServerConfig::new().max_ticks(10).signals(false), |_| {});
    /// let status = world.resource::<ServerStatus>();
    /// assert_eq!((status.tick, status.stopping), (10, Some(StopReason::MaxTicks)));
    /// ```
    ///
    /// # Panics
    /// If a stored setup callback or plugin fails, or a system panics.
    pub fn run_server<S, U>(mut self, config: ServerConfig, setup: S) -> World
    where
        S: FnOnce(&mut Ctx) -> U,
        U: SetupResult,
    {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(ticks) = super::smoke_ticks(std::env::var(super::ENV_SMOKE).ok().as_deref()) {
            let world = self.smoke_server(ticks, setup);
            eprintln!("ponko: smoke run passed: {ticks} ticks, {} entities", world.len());
            return world;
        }

        server::set_log_format(config.log);
        let _signals = config.signals.then(signals::install);
        let seen_at_start = signals::Seen::now();

        connect_hotpatch();
        let hz = self.fixed_hz;
        let fixed_dt = Clock::new(hz).fixed_dt();
        let tick_len = Duration::from_secs_f64(fixed_dt);
        let mut ctx = self.build_ctx(true);
        let control = self.start_control(&mut ctx, true);
        ctx.world.insert_resource(ServerStatus {
            target_hz: hz,
            ..ServerStatus::default()
        });
        ctx.set_in_setup(true);
        run_setups(std::mem::take(&mut self.setups), &mut ctx).unwrap_or_else(|e| panic!("{e}"));
        let frame_fn = setup(&mut ctx).into_frame_fn();
        match frame_fn {
            Some(f) if self.headless_frames => self.add_setup_closure(f),
            other => drop(other),
        }
        ctx.set_in_setup(false);
        ctx.apply_commands();
        ctx.gfx.discard_draws();
        // A setup may have replaced the status; the server owns it.
        if ctx.world.get_resource::<ServerStatus>().is_none() {
            ctx.world.insert_resource(ServerStatus {
                target_hz: hz,
                ..ServerStatus::default()
            });
        }

        server::log(
            LogLevel::Info,
            "server_start",
            json!({
                "name": self.title,
                "engine": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
                "hz": hz,
                "listen": ctx.net.messages.as_ref().and_then(|m| m.local_addr()).map(|a| a.to_string()),
                "control": ctx.control_addr.map(|a| a.to_string()),
                "signals": config.signals,
                "shutdown_grace_s": config.shutdown_grace.as_secs_f64(),
            }),
        );

        let timing = FrameTiming {
            dt: fixed_dt as f32,
            seconds: fixed_dt,
            fixed_steps: 1,
            alpha: 0.0,
        };
        let reports = !config.report_every.is_zero();
        let expected = (config.report_every.as_secs_f64() * f64::from(hz)).ceil() as usize + 1;
        let mut stats = TickStats::new(expected);
        let started = Instant::now();
        let mut window_start = started;
        let mut next_tick = started;
        let mut ran: u64 = 0;
        let mut stopping: Option<(StopReason, Instant)> = None;
        let mut signals_at_stop = 0;
        self.begin_timings(&mut ctx);

        let reason = loop {
            // Stop requests: signals, the handle, the tick limit.
            let (count, signal) = seen_at_start.since(signals::Seen::now());
            let asked = signal
                .or_else(|| config.stop.is_stopping().then_some(StopReason::Requested))
                .or_else(|| config.max_ticks.filter(|&max| ran >= max).map(|_| StopReason::MaxTicks));
            match (stopping, asked) {
                (None, Some(reason)) => {
                    server::log(
                        LogLevel::Info,
                        "server_stopping",
                        json!({ "reason": reason.name(), "tick": ran, "grace_s": config.shutdown_grace.as_secs_f64() }),
                    );
                    ctx.world.resource_mut::<ServerStatus>().stopping = Some(reason);
                    if config.shutdown_grace.is_zero() {
                        break reason;
                    }
                    stopping = Some((reason, Instant::now() + config.shutdown_grace));
                    signals_at_stop = count;
                }
                (Some((reason, deadline)), _) => {
                    if Instant::now() >= deadline {
                        break reason;
                    }
                    if count > signals_at_stop {
                        server::log(LogLevel::Warn, "server_stop_forced", json!({ "tick": ran }));
                        break reason;
                    }
                }
                (None, None) => {}
            }

            // Wait for the tick's time; tools and packets are served first.
            ctx.net.begin_frame();
            if let Some(control) = &control {
                control.poll(&mut ctx);
                if ctx.quit_requested() {
                    break stopping.map_or(StopReason::Quit, |(r, _)| r);
                }
                if ctx.time.is_paused() && !ctx.take_one_step() {
                    control.wait(&mut ctx, PAUSED_WAIT);
                    next_tick = Instant::now();
                    continue;
                }
            }
            if !ctx.net.lockstep.ready(ctx.time.sim_tick()) {
                if !ctx.net.lockstep.wait() {
                    break stopping.map_or(StopReason::Quit, |(r, _)| r);
                }
                continue;
            }
            let now = Instant::now();
            if next_tick > now {
                std::thread::sleep(next_tick - now);
                // Packets that came in while sleeping reach this tick.
                ctx.net.begin_frame();
            }

            let tick_start = Instant::now();
            ctx.begin_frame(timing);
            ctx.frame_prepare_services();
            if self.headless_frames {
                self.run_headless_frame(&mut ctx);
            } else {
                self.begin_timings(&mut ctx);
                ctx.set_in_sim(true);
                self.run_sim_tick(&mut ctx);
                ctx.set_in_sim(false);
                end_timings(&mut ctx);
                forget_seen_removals(&mut ctx, self.sim_systems.iter());
            }
            ctx.gfx.discard_draws();
            ctx.input.end_frame();
            let tick_end = Instant::now();
            let work = tick_end - tick_start;
            if ctx.perf.state.on {
                ctx.end_perf_frame(work.as_secs_f32() * 1000.0);
            }
            ran += 1;
            ctx.world.resource_mut::<ServerStatus>().tick = ran;
            stats.record(work.as_secs_f32() * 1000.0);

            // The next tick's time: one tick later, or now when too far behind.
            next_tick += tick_len;
            if tick_end > next_tick {
                stats.late();
                let behind = tick_end - next_tick;
                let limit = tick_len * config.max_catch_up;
                if behind > limit {
                    let dropped = (behind.as_secs_f64() / fixed_dt) as u64;
                    stats.skipped(dropped);
                    next_tick = tick_end;
                }
            }

            if reports && tick_end - window_start >= config.report_every {
                let report = finish_report(&mut stats, &ctx, tick_end - window_start, hz);
                window_start = tick_end;
                server::log_report(&report);
                ctx.world.resource_mut::<ServerStatus>().report = Some(report);
            }

            if ctx.quit_requested() {
                break stopping.map_or(StopReason::Quit, |(r, _)| r);
            }
        };

        // The last window's numbers, so short runs report too.
        if reports && stats.ticks() > 0 {
            let report = finish_report(&mut stats, &ctx, Instant::now() - window_start, hz);
            server::log_report(&report);
            ctx.world.resource_mut::<ServerStatus>().report = Some(report);
        }
        ctx.world.resource_mut::<ServerStatus>().stopping = Some(reason);
        server::log(
            LogLevel::Info,
            "server_stopped",
            json!({
                "reason": reason.name(),
                "ticks": ran,
                "uptime_s": started.elapsed().as_secs_f64(),
            }),
        );
        std::mem::take(&mut ctx.world)
    }
}

impl Game {
    /// The smoke run of a server: `ticks` ticks as fast as possible, with a
    /// [`ServerStatus`] the systems can read (and its tick counted), but no
    /// pacing, signals or logs.
    #[cfg(not(target_arch = "wasm32"))]
    fn smoke_server<S, U>(mut self, ticks: u64, setup: S) -> World
    where
        S: FnOnce(&mut Ctx) -> U,
        U: SetupResult,
    {
        let hz = self.fixed_hz;
        self.setups.insert(
            0,
            Box::new(move |ctx: &mut Ctx| {
                ctx.world.insert_resource(ServerStatus {
                    target_hz: hz,
                    ..ServerStatus::default()
                });
                Ok(())
            }),
        );
        self.sim_system(|ctx: &mut Ctx| ctx.world.resource_mut::<ServerStatus>().tick += 1)
            .run_headless(ticks, setup)
    }
}

fn finish_report(stats: &mut TickStats, ctx: &Ctx, window: Duration, hz: u32) -> server::TickReport {
    let peers = ctx.net.messages.as_ref().map(|m| m.connected_peers());
    stats.finish(window, hz, ctx.world.len(), peers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smoke_run_gives_systems_a_server_status() {
        let world = Game::new("smoke", 1, 1)
            .fixed_hz(20)
            .sim_system(|ctx: &mut Ctx| {
                // What a server's systems do; must not panic in a smoke run.
                let _ = ctx.world.resource::<ServerStatus>().stopping;
            })
            .smoke_server(12, |_| {});
        let status = world.resource::<ServerStatus>();
        assert_eq!((status.tick, status.target_hz, status.stopping), (12, 20, None));
    }
}
