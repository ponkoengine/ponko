//! Dedicated servers: [`Game::run_server`](crate::game::Game::run_server)
//! runs the simulation headless in real time, at the game's
//! [`fixed_hz`](crate::game::Game::fixed_hz), until the process is asked to
//! stop.
//!
//! **Maturity: tested**, from the guide chapter `servers.md`. Game: none. Tests: `tests/server.rs`,
//! `tests/server_signals.rs`; the server container is built in the list of checks. Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks); the container on
//! Linux. Graphics: not involved. Missing: a reference game and a deployed server.
//!
//! What it adds to a [headless run](crate::game::Game::run_headless):
//!
//! - **Pacing**: one tick every `1 / fixed_hz` seconds of wall time. A tick
//!   that runs long makes the next ones start at once to catch up, up to
//!   [`ServerConfig::max_catch_up`] ticks; a longer stall drops the backlog
//!   (counted as skipped ticks) instead of running a burst.
//! - **Stopping cleanly**: `SIGINT` and `SIGTERM` (what `docker stop`,
//!   Kubernetes and systemd send; Ctrl+C, Ctrl+Break on Windows) or a
//!   [`StopHandle`] set [`ServerStatus::stopping`]. The game's systems see
//!   it, say goodbye to their clients, save, and call `ctx.quit()`; the
//!   server stops after [`ServerConfig::shutdown_grace`] at the latest. A
//!   second signal stops it at once.
//! - **A tick-rate report**: every [`ServerConfig::report_every`], the ticks
//!   per second it reached, tick times (mean, p99, max), late and skipped
//!   ticks, entities, connected peers and memory, as a log line and in
//!   [`ServerStatus::report`].
//! - **Structured logs**: one JSON object per line on standard output
//!   ([`LogFormat::Json`], the default), which container runtimes and log
//!   collectors read as is. [`log`] writes the game's own events the same way.
//!
//! ```
//! use ponko::prelude::*;
//! use ponko::server::{ServerConfig, ServerStatus};
//!
//! // A server that stops by itself after 30 ticks (half a second).
//! let world = Game::new("server", 1, 1)
//!     .fixed_hz(60)
//!     .run_server(ServerConfig::new().max_ticks(30).signals(false), |_| {});
//! assert_eq!(world.resource::<ServerStatus>().tick, 30);
//! ```
//!
//! The guide's "Dedicated servers" chapter has the project template, the
//! container image and what to watch in production.

#![deny(missing_docs)]

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

/// How [`Game::run_server`](crate::game::Game::run_server) runs. Every
/// setting has a default; `ServerConfig::new()` is a server that reports
/// every 10 seconds in JSON lines and stops at once on `SIGINT`/`SIGTERM`.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub(crate) report_every: Duration,
    pub(crate) log: LogFormat,
    pub(crate) shutdown_grace: Duration,
    pub(crate) max_catch_up: u32,
    pub(crate) signals: bool,
    pub(crate) max_ticks: Option<u64>,
    pub(crate) stop: StopHandle,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerConfig {
    /// The defaults: a report every 10 s, JSON lines, no shutdown grace,
    /// catch up at most 5 ticks, `SIGINT`/`SIGTERM` handled, no tick limit.
    ///
    /// ```
    /// use ponko::server::ServerConfig;
    /// use std::time::Duration;
    /// let config = ServerConfig::new();
    /// // The defaults are all overridable.
    /// let quiet = config.report_every(Duration::ZERO).signals(false);
    /// assert!(!quiet.stop_handle().is_stopping());
    /// ```
    pub fn new() -> Self {
        Self {
            report_every: Duration::from_secs(10),
            log: LogFormat::Json,
            shutdown_grace: Duration::ZERO,
            max_catch_up: 5,
            signals: true,
            max_ticks: None,
            stop: StopHandle::new(),
        }
    }

    /// How often the tick-rate report is logged and stored in
    /// [`ServerStatus::report`]. `Duration::ZERO` turns it off.
    ///
    /// ```
    /// # use std::time::Duration;
    /// let config = ponko::server::ServerConfig::new().report_every(Duration::from_secs(60));
    /// ```
    pub fn report_every(mut self, every: Duration) -> Self {
        self.report_every = every;
        self
    }

    /// How the server's log lines look (JSON lines by default). It also sets
    /// how [`log`] writes, for the whole process.
    ///
    /// ```
    /// use ponko::server::{LogFormat, ServerConfig};
    /// let config = ServerConfig::new().log_format(LogFormat::Text);
    /// ```
    pub fn log_format(mut self, format: LogFormat) -> Self {
        self.log = format;
        self
    }

    /// How long the server keeps ticking after a stop was asked for, so the
    /// game can tell its clients, save and call `ctx.quit()` (which ends the
    /// run sooner). Zero (the default) stops after the current tick, without
    /// running another. Stay under the orchestrator's kill timeout: `docker
    /// stop` waits 10 s by default, Kubernetes 30 s.
    ///
    /// ```
    /// # use std::time::Duration;
    /// let config = ponko::server::ServerConfig::new().shutdown_grace(Duration::from_secs(2));
    /// ```
    pub fn shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    /// How many ticks the server runs back to back to catch up after a slow
    /// tick (default 5). Past that it drops the backlog and counts the
    /// dropped ticks as skipped in the report: clients see a hitch rather
    /// than a burst of fast-forwarded ticks.
    ///
    /// ```
    /// let config = ponko::server::ServerConfig::new().max_catch_up(2);
    /// ```
    pub fn max_catch_up(mut self, ticks: u32) -> Self {
        self.max_catch_up = ticks;
        self
    }

    /// Whether `SIGINT` and `SIGTERM` (Ctrl+C and Ctrl+Break on Windows)
    /// stop the server (default true). The handlers are installed for the
    /// run and the previous ones restored after it. Turn it off when the
    /// program handles signals itself, or for servers inside tests; a
    /// [`StopHandle`] still works.
    ///
    /// ```
    /// let config = ponko::server::ServerConfig::new().signals(false);
    /// ```
    pub fn signals(mut self, on: bool) -> Self {
        self.signals = on;
        self
    }

    /// Stops the server after `ticks` ticks, as if asked to stop
    /// ([`StopReason::MaxTicks`]); the shutdown grace still applies. For
    /// tests and timed runs; by default a server runs until it is stopped.
    ///
    /// ```
    /// let config = ponko::server::ServerConfig::new().max_ticks(600);
    /// ```
    pub fn max_ticks(mut self, ticks: u64) -> Self {
        self.max_ticks = Some(ticks);
        self
    }

    /// A handle that stops this server from another thread, such as an
    /// admin command or a test. Clones stop the same server.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::server::ServerConfig;
    ///
    /// let config = ServerConfig::new().signals(false);
    /// let stop = config.stop_handle();
    /// stop.stop(); // before the run: it stops after setup, with no tick
    /// let world = Game::new("server", 1, 1).run_server(config, |_| {});
    /// assert_eq!(world.resource::<ponko::server::ServerStatus>().tick, 0);
    /// ```
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }
}

/// How server log lines look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LogFormat {
    /// One JSON object per line: `{"ts":"2026-11-14T09:30:00.250Z",
    /// "level":"info","event":"tick_report",...}`. For containers and log
    /// collectors.
    #[default]
    Json,
    /// `2026-11-14T09:30:00.250Z info tick_report hz=60.0 ...`, for a terminal.
    Text,
    /// No lines from the server loop; [`log`] writes nothing either.
    Off,
}

/// Stops a [`Game::run_server`](crate::game::Game::run_server) run from
/// any thread: [`ServerConfig::stop_handle`].
#[derive(Clone, Debug, Default)]
pub struct StopHandle(Arc<AtomicBool>);

impl StopHandle {
    fn new() -> Self {
        Self::default()
    }

    /// Asks the server to stop ([`StopReason::Requested`]). It finishes the
    /// current tick, then the shutdown grace begins.
    ///
    /// ```
    /// use ponko::server::ServerConfig;
    /// let stop = ServerConfig::new().stop_handle();
    /// assert!(!stop.is_stopping());
    /// stop.stop();
    /// assert!(stop.is_stopping());
    /// ```
    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether [`stop`](Self::stop) was called.
    ///
    /// ```
    /// use ponko::server::ServerConfig;
    /// let stop = ServerConfig::new().stop_handle();
    /// let from_another_thread = stop.clone();
    /// std::thread::spawn(move || from_another_thread.stop()).join().unwrap();
    /// assert!(stop.is_stopping());
    /// ```
    pub fn is_stopping(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Why a server stopped (or is stopping).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StopReason {
    /// `SIGINT`: Ctrl+C in a terminal.
    Interrupt,
    /// `SIGTERM`: `docker stop`, Kubernetes, systemd. On Windows, Ctrl+Break.
    Terminate,
    /// A [`StopHandle`] asked.
    Requested,
    /// [`ServerConfig::max_ticks`] ran out.
    MaxTicks,
    /// A system called `ctx.quit()` before anything asked it to stop.
    Quit,
}

impl StopReason {
    pub(crate) fn name(self) -> &'static str {
        match self {
            StopReason::Interrupt => "interrupt",
            StopReason::Terminate => "terminate",
            StopReason::Requested => "requested",
            StopReason::MaxTicks => "max_ticks",
            StopReason::Quit => "quit",
        }
    }
}

/// The running server, a resource in the world of a
/// [`Game::run_server`](crate::game::Game::run_server) run: systems read it
/// to notice a stop request, and it stays in the returned world.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::server::ServerStatus;
///
/// /// Says goodbye to everyone once a stop is asked for, then quits.
/// fn shut_down(ctx: &mut Ctx) {
///     if ctx.world.resource::<ServerStatus>().stopping.is_some() {
///         // tell the clients, save the world...
///         ctx.quit();
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, Serialize)]
#[non_exhaustive]
pub struct ServerStatus {
    /// The ticks run so far.
    pub tick: u64,
    /// The rate the server paces to: the game's `fixed_hz`.
    pub target_hz: u32,
    /// Set when a stop was asked for: the game has until the shutdown grace
    /// ends to wrap up. After the run, why it stopped.
    pub stopping: Option<StopReason>,
    /// The latest tick-rate report, if one was made.
    pub report: Option<TickReport>,
}

/// Ticks, timing and memory over one report window: logged as the
/// `tick_report` event and kept in [`ServerStatus::report`].
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TickReport {
    /// The wall time the report covers, in seconds.
    pub window_s: f64,
    /// Ticks run in the window.
    pub ticks: u64,
    /// The rate the server paces to.
    pub target_hz: u32,
    /// The rate it reached: `ticks / window_s`. Below `target_hz`, ticks
    /// take longer than their budget (`1000 / target_hz` ms).
    pub hz: f64,
    /// Mean time of one tick's work (systems and networking), milliseconds.
    pub tick_ms_mean: f64,
    /// 99th percentile of it.
    pub tick_ms_p99: f64,
    /// The slowest tick.
    pub tick_ms_max: f64,
    /// Ticks that ended after the next one was due.
    pub late_ticks: u64,
    /// Ticks dropped because the server fell more than
    /// [`ServerConfig::max_catch_up`] ticks behind.
    pub skipped_ticks: u64,
    /// Entities in the world.
    pub entities: usize,
    /// Peers connected to the game's message endpoint, if it has one.
    pub peers: Option<usize>,
    /// Resident memory now, bytes (Linux).
    pub rss_bytes: Option<u64>,
    /// The most resident memory so far, bytes (Linux, macOS and the other
    /// Unix systems).
    pub peak_rss_bytes: Option<u64>,
}

/// How a log line is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogLevel {
    /// Normal operation.
    Info,
    /// Something to look at.
    Warn,
    /// Something failed.
    Error,
}

impl LogLevel {
    fn name(self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

const FORMAT_JSON: u8 = 0;
const FORMAT_TEXT: u8 = 1;
const FORMAT_OFF: u8 = 2;
static FORMAT: AtomicU8 = AtomicU8::new(FORMAT_JSON);

pub(crate) fn set_log_format(format: LogFormat) {
    let value = match format {
        LogFormat::Json => FORMAT_JSON,
        LogFormat::Text => FORMAT_TEXT,
        LogFormat::Off => FORMAT_OFF,
    };
    FORMAT.store(value, Ordering::Relaxed);
}

/// Writes one structured log line to standard output, the way the server
/// loop writes its own: in JSON lines (the default),
/// `{"ts":"...","level":"info","event":"player_joined","peer":3}`. `fields`
/// is a JSON object whose entries are added to the line (anything else goes
/// in a `"value"` entry). The format follows the running server's
/// [`ServerConfig::log_format`].
///
/// ```
/// use ponko::server::{LogLevel, log};
/// log(LogLevel::Info, "player_joined", serde_json::json!({ "peer": 3 }));
/// ```
pub fn log(level: LogLevel, event: &str, fields: Value) {
    let format = FORMAT.load(Ordering::Relaxed);
    if format == FORMAT_OFF {
        return;
    }
    let line = format_line(format == FORMAT_JSON, &timestamp(now_unix()), level, event, fields);
    // One write per line, so lines from several threads don't interleave.
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
}

/// One log line, newline included.
fn format_line(json: bool, ts: &str, level: LogLevel, event: &str, fields: Value) -> String {
    let fields = match fields {
        Value::Object(map) => map,
        Value::Null => serde_json::Map::new(),
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            map
        }
    };
    if json {
        // ts, level and event first, for people reading the raw lines.
        let mut text = format!(
            "{{\"ts\":{},\"level\":\"{}\",\"event\":{}",
            Value::from(ts),
            level.name(),
            Value::from(event)
        );
        for (key, value) in fields {
            if !matches!(key.as_str(), "ts" | "level" | "event") {
                text.push_str(&format!(",{}:{value}", Value::from(key)));
            }
        }
        text.push_str("}\n");
        text
    } else {
        let mut text = format!("{ts} {} {event}", level.name());
        for (key, value) in fields {
            match value {
                Value::String(s) => text.push_str(&format!(" {key}={s}")),
                Value::Null => {}
                other => text.push_str(&format!(" {key}={other}")),
            }
        }
        text.push('\n');
        text
    }
}

fn now_unix() -> Duration {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
}

/// RFC 3339 in UTC with milliseconds: `2026-11-14T09:30:00.250Z`.
fn timestamp(since_epoch: Duration) -> String {
    let secs = since_epoch.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        since_epoch.subsec_millis()
    )
}

/// The calendar date of a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The tick timings of one report window. The buffer is reserved once, so
/// recording a tick never allocates; past its capacity only the count, sum
/// and maximum grow.
pub(crate) struct TickStats {
    samples: Vec<f32>,
    ticks: u64,
    sum_ms: f64,
    max_ms: f32,
    late: u64,
    skipped: u64,
}

impl TickStats {
    pub(crate) fn new(expected: usize) -> Self {
        Self {
            samples: Vec::with_capacity(expected.clamp(64, 1 << 16)),
            ticks: 0,
            sum_ms: 0.0,
            max_ms: 0.0,
            late: 0,
            skipped: 0,
        }
    }

    pub(crate) fn record(&mut self, ms: f32) {
        if self.samples.len() < self.samples.capacity() {
            self.samples.push(ms);
        }
        self.ticks += 1;
        self.sum_ms += f64::from(ms);
        self.max_ms = self.max_ms.max(ms);
    }

    /// Ticks recorded in this window.
    pub(crate) fn ticks(&self) -> u64 {
        self.ticks
    }

    pub(crate) fn late(&mut self) {
        self.late += 1;
    }

    pub(crate) fn skipped(&mut self, ticks: u64) {
        self.skipped += ticks;
    }

    /// The window's report; starts the next window.
    pub(crate) fn finish(
        &mut self,
        window: Duration,
        target_hz: u32,
        entities: usize,
        peers: Option<usize>,
    ) -> TickReport {
        self.samples.sort_unstable_by(f32::total_cmp);
        // Nearest rank: the smallest sample with 99% of them at or below it.
        let p99 = match self.samples.len() {
            0 => 0.0,
            n => self.samples[(n * 99).div_ceil(100) - 1],
        };
        let window_s = window.as_secs_f64();
        let report = TickReport {
            window_s,
            ticks: self.ticks,
            target_hz,
            hz: if window_s > 0.0 {
                self.ticks as f64 / window_s
            } else {
                0.0
            },
            tick_ms_mean: if self.ticks > 0 {
                self.sum_ms / self.ticks as f64
            } else {
                0.0
            },
            tick_ms_p99: f64::from(p99),
            tick_ms_max: f64::from(self.max_ms),
            late_ticks: self.late,
            skipped_ticks: self.skipped,
            entities,
            peers,
            rss_bytes: rss_bytes(),
            peak_rss_bytes: peak_rss_bytes(),
        };
        self.samples.clear();
        (self.ticks, self.sum_ms, self.max_ms, self.late, self.skipped) = (0, 0.0, 0.0, 0, 0);
        report
    }
}

pub(crate) fn log_report(report: &TickReport) {
    let fields = serde_json::to_value(report).unwrap_or(Value::Null);
    let level = if report.skipped_ticks > 0 || report.hz < f64::from(report.target_hz) * 0.95 {
        LogLevel::Warn
    } else {
        LogLevel::Info
    };
    log(level, "tick_report", fields);
}

/// Resident memory now, from `/proc/self/statm` (Linux only).
fn rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: sysconf only reads a system constant.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Some(pages * u64::try_from(page).ok()?)
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// The peak resident memory, from `getrusage` (kilobytes on Linux and the
/// BSDs, bytes on Apple systems).
fn peak_rss_bytes() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: getrusage fills the zeroed struct it is given.
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 {
                return None;
            }
            usage
        };
        let max = u64::try_from(usage.ru_maxrss).ok()?;
        Some(if cfg!(target_vendor = "apple") { max } else { max * 1024 })
    }
    #[cfg(not(unix))]
    None
}

// --- Signals -------------------------------------------------------------

/// The stop signals seen, by kind, counted since the process started. The
/// handler only increments these (async-signal-safe); the server loop
/// compares them with the counts it saw when it started.
pub(crate) mod signals {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::StopReason;

    static INTERRUPTS: AtomicU32 = AtomicU32::new(0);
    static TERMINATES: AtomicU32 = AtomicU32::new(0);

    /// The signal counts at one moment.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Seen {
        interrupts: u32,
        terminates: u32,
    }

    impl Seen {
        pub(crate) fn now() -> Self {
            Self {
                interrupts: INTERRUPTS.load(Ordering::Acquire),
                terminates: TERMINATES.load(Ordering::Acquire),
            }
        }

        /// How many stop signals came since `self`, and the latest kind.
        pub(crate) fn since(self, later: Seen) -> (u32, Option<StopReason>) {
            let i = later.interrupts.wrapping_sub(self.interrupts);
            let t = later.terminates.wrapping_sub(self.terminates);
            let kind = if t > 0 {
                Some(StopReason::Terminate)
            } else if i > 0 {
                Some(StopReason::Interrupt)
            } else {
                None
            };
            (i + t, kind)
        }
    }

    #[cfg(any(unix, windows))]
    extern "C" fn on_signal(signal: libc::c_int) {
        if signal == libc::SIGINT {
            INTERRUPTS.fetch_add(1, Ordering::AcqRel);
        } else {
            TERMINATES.fetch_add(1, Ordering::AcqRel);
        }
        // The Windows C runtime resets a handler to the default after it
        // runs: install it again, so a second signal is also caught.
        #[cfg(windows)]
        // SAFETY: `signal` with a valid handler, as in `install`.
        unsafe {
            libc::signal(signal, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        }
    }

    /// Ctrl+Break in the Windows C runtime (not in the `libc` crate).
    #[cfg(windows)]
    const SIGBREAK: libc::c_int = 21;

    #[cfg(unix)]
    type Saved = Vec<(libc::c_int, libc::sigaction)>;
    #[cfg(windows)]
    type Saved = Vec<(libc::c_int, libc::sighandler_t)>;
    #[cfg(not(any(unix, windows)))]
    type Saved = ();

    /// How many runs hold the handlers, and the handlers they replaced.
    static INSTALLED: Mutex<(usize, Option<Saved>)> = Mutex::new((0, None));

    /// Installs the handlers (once, for nested or parallel servers) until
    /// the guard is dropped, which restores the previous ones.
    // Without signals (the web) the saved handlers are `()`.
    #[cfg_attr(not(any(unix, windows)), allow(clippy::unit_arg))]
    pub(crate) fn install() -> Guard {
        let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        if installed.0 == 0 {
            installed.1 = Some(install_handlers());
        }
        installed.0 += 1;
        Guard(())
    }

    /// Restores the previous handlers when the last server stops.
    pub(crate) struct Guard(());

    impl Drop for Guard {
        fn drop(&mut self) {
            let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
            installed.0 -= 1;
            if installed.0 == 0
                && let Some(saved) = installed.1.take()
            {
                restore_handlers(saved);
            }
        }
    }

    #[cfg(unix)]
    fn install_handlers() -> Saved {
        let mut saved = Vec::new();
        for signal in [libc::SIGINT, libc::SIGTERM] {
            // SAFETY: the structs are zeroed C data that sigaction fills or
            // reads; the handler only touches atomics.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                let mut old: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(signal, &action, &mut old) == 0 {
                    saved.push((signal, old));
                }
            }
        }
        saved
    }

    #[cfg(unix)]
    fn restore_handlers(saved: Saved) {
        for (signal, old) in saved {
            // SAFETY: `old` is what sigaction returned for this signal.
            unsafe {
                libc::sigaction(signal, &old, std::ptr::null_mut());
            }
        }
    }

    #[cfg(windows)]
    fn install_handlers() -> Saved {
        let mut saved = Vec::new();
        for signal in [libc::SIGINT, libc::SIGTERM, SIGBREAK] {
            // SAFETY: a valid handler for a signal the C runtime knows.
            let old = unsafe { libc::signal(signal, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t) };
            if old != libc::SIG_ERR as libc::sighandler_t {
                saved.push((signal, old));
            }
        }
        saved
    }

    #[cfg(windows)]
    fn restore_handlers(saved: Saved) {
        for (signal, old) in saved {
            // SAFETY: `old` is what `signal` returned for this signal.
            unsafe {
                libc::signal(signal, old);
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn install_handlers() -> Saved {}

    #[cfg(not(any(unix, windows)))]
    fn restore_handlers(_: Saved) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timestamps_are_rfc3339_utc_with_milliseconds() {
        assert_eq!(timestamp(Duration::ZERO), "1970-01-01T00:00:00.000Z");
        // 2000-02-29 (a leap day), 12:34:56.789.
        let t = Duration::from_millis(951_827_696_789);
        assert_eq!(timestamp(t), "2000-02-29T12:34:56.789Z");
        assert_eq!(
            timestamp(Duration::from_secs(1_798_761_599)),
            "2026-12-31T23:59:59.000Z"
        );
    }

    #[test]
    fn json_lines_put_ts_level_and_event_first_and_keep_them() {
        let line = format_line(
            true,
            "T",
            LogLevel::Warn,
            "joined",
            json!({ "peer": 3, "event": "spoofed" }),
        );
        assert!(line.ends_with('\n'));
        let value: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(value["ts"], "T");
        assert_eq!(value["level"], "warn");
        assert_eq!(value["event"], "joined", "a field can't replace the event name");
        assert_eq!(value["peer"], 3);
        assert!(
            line.starts_with(r#"{"ts":"T","level":"warn","event":"joined""#),
            "{line}"
        );
    }

    #[test]
    fn text_lines_list_fields_as_key_value() {
        let line = format_line(
            false,
            "T",
            LogLevel::Info,
            "joined",
            json!({ "peer": 3, "name": "ann" }),
        );
        assert_eq!(line, "T info joined name=ann peer=3\n");
        let line = format_line(false, "T", LogLevel::Error, "oops", json!(7));
        assert_eq!(line, "T error oops value=7\n");
    }

    #[test]
    fn tick_stats_report_rate_percentiles_and_reset() {
        let mut stats = TickStats::new(100);
        for i in 0..100 {
            stats.record(if i >= 98 { 50.0 } else { 1.0 });
        }
        stats.late();
        stats.skipped(3);
        let report = stats.finish(Duration::from_secs(2), 60, 7, Some(2));
        assert_eq!(
            (report.ticks, report.target_hz, report.entities, report.peers),
            (100, 60, 7, Some(2))
        );
        assert!((report.hz - 50.0).abs() < 1e-9);
        assert!((report.tick_ms_mean - 1.98).abs() < 1e-6);
        assert_eq!((report.tick_ms_p99, report.tick_ms_max), (50.0, 50.0));
        assert_eq!((report.late_ticks, report.skipped_ticks), (1, 3));
        let next = stats.finish(Duration::from_secs(1), 60, 0, None);
        assert_eq!(
            (next.ticks, next.late_ticks, next.skipped_ticks, next.tick_ms_max),
            (0, 0, 0, 0.0)
        );
    }

    #[test]
    fn recording_ticks_never_grows_the_buffer() {
        let mut stats = TickStats::new(64);
        let capacity = stats.samples.capacity();
        for _ in 0..10_000 {
            stats.record(1.0);
        }
        assert_eq!(stats.samples.capacity(), capacity);
        assert_eq!(stats.finish(Duration::from_secs(1), 60, 0, None).ticks, 10_000);
    }

    #[cfg(unix)]
    #[test]
    fn peak_memory_is_reported_on_unix() {
        assert!(peak_rss_bytes().unwrap() > 1 << 20, "at least a megabyte");
        #[cfg(target_os = "linux")]
        assert!(rss_bytes().unwrap() > 1 << 20);
    }
}
