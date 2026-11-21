//! Crash reports: when a shipped game panics or stops on a fatal engine
//! error, a text file in the player's data folder says what happened, so
//! the player can send it and the developer can fix it.
//!
//! A report holds the panic's message, where it happened and the
//! backtrace, the game's and the engine's versions, the operating system,
//! the GPU and its driver, the graphics settings, and the last lines the
//! game printed. Nothing else: no user name, no machine name, and every
//! path in it is made relative to the game's folders (`<game>/`,
//! `<data>/`, `~/`). The guide's "Crash reports" section lists the fields.
//!
//! Turn reports on or off with [`Game::crash_reports`](crate::game::Game::crash_reports).
//! By default they are on in release builds of a game with a window, and
//! off in development builds (with debug assertions) and headless runs.
//! A report goes to [`FOLDER`] in [`ctx.storage`](crate::ctx::Ctx::storage)
//! (so the game needs an app id), where the newest [`KEPT`] are kept.
//! [`CrashReports::on_crash`] runs the game's own code with each report: to
//! show its own message, or to send the file somewhere. The engine itself
//! never sends anything; [`saved_reports`] lists the reports of earlier
//! runs, for a game that offers to send them at its next start.
//!
//! A crash that is not a panic (a segmentation fault or an `abort()` in a
//! native library, an access violation on Windows) leaves a report too, of
//! kind [`CrashKind::Native`]: the engine's signal handler (unhandled
//! exception filter on Windows) writes what was prepared ahead of time into
//! a file opened ahead of time, and the game's next start turns it into a
//! report and runs the hook with it. The guide's "Crashes that are not
//! panics" has the design.
//!
//! In a browser the report goes to the page's storage and to the page, as
//! a `ponkocrashreport` event on `window` (and `window.ponkoCrashReport`):
//! the page `ponko package --web` makes offers it as a file to save.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use web_time::{Instant, SystemTime, UNIX_EPOCH};

use crate::ctx::Ctx;
use crate::render::GpuAdapter;
use crate::storage::Storage;

pub(crate) mod native;

/// The folder of [`ctx.storage`](crate::ctx::Ctx::storage) that holds the
/// reports, one `.txt` file each, named after the time (UTC) they were
/// written: `crash-reports/2026-11-21_14-03-07.txt`.
pub const FOLDER: &str = "crash-reports";

/// How many reports are kept: writing one deletes the oldest beyond this.
pub const KEPT: usize = 10;

/// How many of the last lines the game printed go into a report.
pub const LOG_LINES: usize = 100;

/// The game's code that runs with each report, see [`CrashReports::on_crash`].
type Hook = Arc<dyn Fn(&CrashReport) + Send + Sync>;

/// Whether a game writes crash reports, and what runs with each: the
/// setting of [`Game::crash_reports`](crate::game::Game::crash_reports).
///
/// `true` and `false` convert into it, for the common case:
///
/// ```
/// # use ponko::prelude::*;
/// let game = Game::new("Space Crab", 1280, 720)
///     .app_id("space-crab")
///     .crash_reports(true); // also in development builds
/// ```
///
/// The full form names the game's version and runs code with each report:
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::crash::CrashReports;
/// let game = Game::new("Space Crab", 1280, 720).app_id("space-crab").crash_reports(
///     CrashReports::default()
///         .game_version(env!("CARGO_PKG_VERSION"))
///         .on_crash(|report| eprintln!("Space Crab stopped: {}", report.message())),
/// );
/// ```
#[derive(Clone, Default)]
#[must_use]
pub struct CrashReports {
    /// `None`: the build's default (see [`CrashReports::default`]).
    on: Option<bool>,
    game_version: Option<String>,
    hook: Option<Hook>,
}

impl std::fmt::Debug for CrashReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrashReports")
            .field("on", &self.on)
            .field("game_version", &self.game_version)
            .field("on_crash", &self.hook.is_some())
            .finish()
    }
}

impl From<bool> for CrashReports {
    fn from(on: bool) -> Self {
        if on { Self::on() } else { Self::off() }
    }
}

impl CrashReports {
    /// Reports on, in every build and also in headless runs (a dedicated
    /// server's crashes, a test).
    ///
    /// ```
    /// assert!(ponko::crash::CrashReports::on().is_on(false));
    /// ```
    pub fn on() -> Self {
        Self {
            on: Some(true),
            ..Self::default()
        }
    }

    /// Reports off, in every build.
    ///
    /// ```
    /// assert!(!ponko::crash::CrashReports::off().is_on(false));
    /// ```
    pub fn off() -> Self {
        Self {
            on: Some(false),
            ..Self::default()
        }
    }

    /// The game's version, for the report's `game version` line: usually
    /// `env!("CARGO_PKG_VERSION")`, the version in the game's `Cargo.toml`.
    /// Without it the report says `not set`.
    pub fn game_version(mut self, version: impl Into<String>) -> Self {
        self.game_version = Some(version.into());
        self
    }

    /// Runs `hook` with each report, right after it was saved (with
    /// reports off, never). The engine shows and sends nothing itself:
    /// this is where a game shows its own message (a dialog, a file it
    /// opens) or sends the report to its developers.
    ///
    /// For a panic it runs inside the panic hook, before the game stops,
    /// on the thread that panicked: it must not panic itself (a panic in a
    /// panic hook aborts at once) and should be quick. Sending a report
    /// over the network is better left to the next start, from
    /// [`saved_reports`]. [`CrashReport::text`] is the file's text, and
    /// [`CrashReport::path`] the file on the disk.
    ///
    /// For a crash that is not a panic ([`CrashKind::Native`]) it runs at
    /// the game's next start, as that run's reports start, with the report
    /// of the run that crashed.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// use ponko::crash::CrashReports;
    /// let game = Game::new("Space Crab", 1280, 720).app_id("space-crab").crash_reports(
    ///     CrashReports::default().on_crash(|report| {
    ///         if let Some(path) = report.path() {
    ///             eprintln!("Sorry! A crash report was saved to {}.", path.display());
    ///         }
    ///     }),
    /// );
    /// ```
    pub fn on_crash(mut self, hook: impl Fn(&CrashReport) + Send + Sync + 'static) -> Self {
        self.hook = Some(Arc::new(hook));
        self
    }

    /// Whether reports are on for a run (`headless`: without a window). By
    /// default: on in release builds (without debug assertions) of a game
    /// with a window, off in development builds and headless runs;
    /// [`on`](Self::on) and [`off`](Self::off) decide for every build.
    ///
    /// ```
    /// use ponko::crash::CrashReports;
    /// let default = CrashReports::default();
    /// assert_eq!(default.is_on(false), !cfg!(debug_assertions));
    /// assert!(!default.is_on(true), "headless runs report only when asked");
    /// ```
    #[must_use]
    pub fn is_on(&self, headless: bool) -> bool {
        self.on.unwrap_or_else(|| default_on(cfg!(debug_assertions), headless))
    }
}

/// The default of [`CrashReports::is_on`]: release builds with a window.
fn default_on(debug_assertions: bool, headless: bool) -> bool {
    !debug_assertions && !headless
}

/// What stopped the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum CrashKind {
    /// Rust code panicked: the game's own or the engine's.
    Panic,
    /// A fatal error: the engine couldn't go on (no usable GPU, the
    /// window couldn't be made), or the game said so with [`report`].
    Error,
    /// A crash that is not a panic: a signal on Linux and macOS (a
    /// segmentation fault, an `abort()` in a C library), an unhandled
    /// exception on Windows. Its report is finished at the game's next
    /// start; [`CrashReport::signal`] names the signal.
    Native,
}

/// One crash report: what [`CrashReports::on_crash`] gets and what the file
/// holds ([`text`](Self::text)). Serializes to JSON with `serde` (the
/// fields' names are the guide's), for a game that sends it that way.
#[derive(Debug, Clone, Serialize)]
pub struct CrashReport {
    kind: CrashKind,
    message: String,
    location: Option<String>,
    thread: Option<String>,
    time: String,
    uptime_secs: f64,
    game: String,
    game_version: Option<String>,
    engine_version: String,
    os: String,
    gpu: Option<GpuAdapter>,
    settings: Vec<(String, String)>,
    backtrace: String,
    log: Option<Vec<String>>,
    file: Option<String>,
    signal: Option<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl CrashReport {
    /// A panic or an error.
    #[must_use]
    pub fn kind(&self) -> CrashKind {
        self.kind
    }

    /// What went wrong: the panic's message, or the error's.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// For a panic, where in the source it happened (`src/main.rs:12:5`).
    #[must_use]
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    /// The name of the thread that panicked (`main`), when it has one.
    #[must_use]
    pub fn thread(&self) -> Option<&str> {
        self.thread.as_deref()
    }

    /// When it happened, in UTC: `2026-11-21 14:03:07 UTC`.
    #[must_use]
    pub fn time(&self) -> &str {
        &self.time
    }

    /// Seconds since the game started.
    #[must_use]
    pub fn uptime_secs(&self) -> f64 {
        self.uptime_secs
    }

    /// The game's title, as given to [`Game::new`](crate::game::Game::new).
    #[must_use]
    pub fn game(&self) -> &str {
        &self.game
    }

    /// The game's version ([`CrashReports::game_version`]).
    #[must_use]
    pub fn game_version(&self) -> Option<&str> {
        self.game_version.as_deref()
    }

    /// The engine's version.
    #[must_use]
    pub fn engine_version(&self) -> &str {
        &self.engine_version
    }

    /// The operating system, its version and the CPU architecture
    /// (`macos 15.1 aarch64`); in a browser, the browser's user agent.
    #[must_use]
    pub fn os(&self) -> &str {
        &self.os
    }

    /// The GPU the renderer opened, with its driver: `None` before the
    /// first frame was drawn and in headless runs.
    #[must_use]
    pub fn gpu(&self) -> Option<&GpuAdapter> {
        self.gpu.as_ref()
    }

    /// The graphics settings at the last frame, as names and values: the
    /// window's size and mode, vsync, and in 3D the quality preset, render
    /// scale, multisampling, shadows, antialiasing, ambient occlusion and
    /// automatic quality.
    #[must_use]
    pub fn settings(&self) -> &[(String, String)] {
        &self.settings
    }

    /// The backtrace of the panic (or of the [`report`] call), with paths
    /// made relative. Release builds without debug info show fewer names.
    #[must_use]
    pub fn backtrace(&self) -> &str {
        &self.backtrace
    }

    /// The last [`LOG_LINES`] lines the game printed (standard output and
    /// error), oldest first, paths made relative: `None` where they
    /// couldn't be kept (in a browser, or if the system refused).
    #[must_use]
    pub fn log(&self) -> Option<&[String]> {
        self.log.as_deref()
    }

    /// The report's name in [`ctx.storage`](crate::ctx::Ctx::storage)
    /// (`crash-reports/2026-11-21_14-03-07.txt`): `None` when it couldn't be
    /// saved (the game has no app id, or the disk refused).
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Where the report is on the disk, to show the player or open: `None`
    /// in a browser and when it wasn't saved.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// For a [native](CrashKind::Native) crash, the signal or the Windows
    /// exception that stopped the game: `SIGSEGV`, `SIGABRT`,
    /// `EXCEPTION_ACCESS_VIOLATION`.
    #[must_use]
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }

    /// The report as the file has it: a line per field, then the
    /// settings, the backtrace and the log.
    #[must_use]
    pub fn text(&self) -> String {
        let mut t = String::new();
        let kind = match self.kind {
            CrashKind::Panic => "panic",
            CrashKind::Error => "error",
            CrashKind::Native => "native",
        };
        let _ = writeln!(t, "{} crash report", self.game);
        let _ = writeln!(t, "kind: {kind}");
        if let Some(signal) = &self.signal {
            let _ = writeln!(t, "signal: {signal}");
        }
        let _ = writeln!(t, "message: {}", indent_more(&self.message));
        if let Some(location) = &self.location {
            let _ = writeln!(t, "location: {location}");
        }
        if let Some(thread) = &self.thread {
            let _ = writeln!(t, "thread: {thread}");
        }
        let _ = writeln!(t, "time: {}", self.time);
        let _ = writeln!(t, "uptime: {:.1} s", self.uptime_secs);
        let _ = writeln!(t, "game version: {}", self.game_version.as_deref().unwrap_or("not set"));
        let _ = writeln!(t, "engine version: {}", self.engine_version);
        let _ = writeln!(t, "os: {}", self.os);
        match &self.gpu {
            Some(gpu) => {
                let _ = writeln!(t, "gpu: {} ({}, {})", gpu.name, gpu.backend, gpu.device_type);
                let driver = [gpu.driver.as_str(), gpu.driver_info.as_str()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = writeln!(
                    t,
                    "gpu driver: {}",
                    if driver.is_empty() { "not reported" } else { &driver }
                );
            }
            None => {
                let _ = writeln!(t, "gpu: not opened");
            }
        }
        let _ = writeln!(t, "\nsettings:");
        for (name, value) in &self.settings {
            let _ = writeln!(t, "  {name}: {value}");
        }
        let _ = writeln!(t, "\nbacktrace:");
        for line in self.backtrace.lines() {
            let _ = writeln!(t, "  {line}");
        }
        match &self.log {
            Some(lines) => {
                let _ = writeln!(t, "\nlast {} log lines:", lines.len());
                for line in lines {
                    let _ = writeln!(t, "  {line}");
                }
            }
            None => {
                let _ = writeln!(t, "\nlog: not kept on this platform");
            }
        }
        t
    }
}

/// A message over several lines, its later lines indented under the first.
fn indent_more(text: &str) -> String {
    text.replace('\n', "\n  ")
}

/// The names of the crash reports in `storage`, newest first: reports of
/// earlier runs, for a game that offers to send them at its next start.
/// Read one with [`Storage::read`], and [`Storage::delete`] it once sent.
/// Empty when storage is off.
///
/// ```
/// # use ponko::prelude::*;
/// # let dir = std::env::temp_dir().join(format!("ponko-doc-crash-{}", std::process::id()));
/// Game::new("Space Crab", 1280, 720).data_dir(&dir).run_headless(0, |ctx| {
///     for name in ponko::crash::saved_reports(&ctx.storage) {
///         let text = ctx.storage.read(&name).unwrap_or_default();
///         // ...ask the player, send `text`, then:
///         ctx.storage.delete(&name).unwrap();
///     }
/// });
/// # let _ = std::fs::remove_dir_all(dir);
/// ```
#[must_use]
pub fn saved_reports(storage: &Storage) -> Vec<String> {
    let mut names: Vec<String> = storage
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| is_report_name(name))
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    names
}

fn is_report_name(name: &str) -> bool {
    name.strip_prefix(FOLDER)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|rest| rest.ends_with(".txt") && !rest.contains('/'))
}

/// Writes a crash report for a fatal error the game hit itself, as the
/// engine does for its own (the window or the GPU couldn't be opened):
/// for a game that stops without panicking. Returns the report, or `None`
/// when no running game has reports on. Runs
/// [`on_crash`](CrashReports::on_crash) like a panic does.
///
/// ```
/// # use ponko::prelude::*;
/// # let dir = std::env::temp_dir().join(format!("ponko-doc-report-{}", std::process::id()));
/// Game::new("Space Crab", 1280, 720)
///     .data_dir(&dir)
///     .crash_reports(true)
///     .run_headless(0, |ctx| {
///         let report = ponko::crash::report("the save file is damaged beyond repair").unwrap();
///         assert_eq!(report.kind(), ponko::crash::CrashKind::Error);
///         assert!(ctx.storage.exists(report.file().unwrap()));
///     });
/// # let _ = std::fs::remove_dir_all(dir);
/// ```
pub fn report(message: impl std::fmt::Display) -> Option<CrashReport> {
    let shared = active()?;
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    // A panic in the game's hook while this report is written gets no
    // report of its own (it would run the hook again, inside a panic hook).
    let was_writing = WRITING.with(|w| w.replace(true));
    let report = shared.write(CrashKind::Error, &message.to_string(), None, backtrace);
    WRITING.with(|w| w.set(was_writing));
    Some(report)
}

/// The engine's own fatal errors: a run that ends with an error.
pub(crate) fn report_error(error: &crate::error::Error) {
    let _ = report(error);
}

/// What a run with reports on shares with the panic hook.
pub(crate) struct Shared {
    /// Where reports go: a copy of the game's storage (`None`: off).
    storage: Mutex<Option<Storage>>,
    game: String,
    game_version: Option<String>,
    hook: Option<Hook>,
    os: String,
    started: Instant,
    /// When the run started, in milliseconds since 1970 (for a native
    /// crash's uptime, which the next start works out).
    started_ms: u64,
    paths: PathScrubber,
    /// Updated at each frame.
    snapshot: Mutex<Snapshot>,
    /// A panic was reported: the panics that follow it in this run (a
    /// thread that saw it, an abort) say less, and get none.
    panicked: std::sync::atomic::AtomicBool,
}

/// What the last frame left for a report.
#[derive(Default)]
struct Snapshot {
    gpu: Option<GpuAdapter>,
    settings: Settings,
}

/// The graphics settings at the last frame (copied each frame, so plain
/// values only: no allocation per frame).
#[derive(Clone, Copy, Default, PartialEq)]
struct Settings {
    framed: bool,
    window: glam::Vec2,
    scale: f32,
    /// 0 windowed, 1 borderless, 2 exclusive fullscreen.
    mode: u8,
    vsync: bool,
    #[cfg(feature = "3d")]
    three: Option<Settings3d>,
}

#[cfg(feature = "3d")]
#[derive(Clone, Copy, PartialEq)]
struct Settings3d {
    preset: Option<crate::render::GraphicsQuality>,
    render_scale: f32,
    msaa: u32,
    shadows: bool,
    shadow_cascades: u32,
    shadow_resolution: u32,
    anti_aliasing: crate::render::AntiAliasing,
    ambient_occlusion: bool,
    autotune: bool,
}

impl Settings {
    fn lines(&self) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        if !self.framed {
            lines.push(("frames".into(), "none drawn yet".into()));
            return lines;
        }
        let mode = match self.mode {
            1 => "borderless fullscreen",
            2 => "exclusive fullscreen",
            _ => "windowed",
        };
        lines.push((
            "window".into(),
            format!(
                "{}x{} at {}x scale, {mode}",
                self.window.x.round(),
                self.window.y.round(),
                self.scale
            ),
        ));
        lines.push(("vsync".into(), on_off(self.vsync).into()));
        #[cfg(feature = "3d")]
        if let Some(s) = &self.three {
            let preset = s.preset.map_or_else(|| "custom".to_string(), |q| format!("{q:?}"));
            lines.push(("quality preset".into(), preset));
            lines.push(("render scale".into(), format!("{}", s.render_scale)));
            lines.push(("multisampling".into(), format!("{}x", s.msaa)));
            let shadows = if s.shadows {
                format!("{} cascades of {}²", s.shadow_cascades, s.shadow_resolution)
            } else {
                "off".into()
            };
            lines.push(("shadows".into(), shadows));
            lines.push(("antialiasing".into(), format!("{:?}", s.anti_aliasing)));
            lines.push(("ambient occlusion".into(), on_off(s.ambient_occlusion).into()));
            lines.push(("automatic quality".into(), on_off(s.autotune).into()));
        }
        lines
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

impl Shared {
    /// Builds the report, saves it, tells the page (web) and runs the
    /// game's hook.
    fn write(&self, kind: CrashKind, message: &str, location: Option<String>, backtrace: String) -> CrashReport {
        let (gpu, settings) = {
            // Never wait here: the frame that holds it may be the one that panicked.
            match self.snapshot.try_lock() {
                Ok(s) => (s.gpu.clone(), s.settings.lines()),
                Err(std::sync::TryLockError::Poisoned(s)) => {
                    let s = s.into_inner();
                    (s.gpu.clone(), s.settings.lines())
                }
                Err(std::sync::TryLockError::WouldBlock) => (None, Vec::new()),
            }
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let log =
            crate::logs::last_lines(LOG_LINES).map(|lines| lines.iter().map(|line| self.paths.scrub(line)).collect());
        let mut report = CrashReport {
            kind,
            message: self.paths.scrub(message),
            location: location.map(|l| self.paths.scrub(&l)),
            thread: std::thread::current().name().map(str::to_owned),
            time: format!("{} UTC", utc(now, ' ', ':')),
            uptime_secs: self.started.elapsed().as_secs_f64(),
            game: self.game.clone(),
            game_version: self.game_version.clone(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            os: self.os.clone(),
            gpu,
            settings,
            backtrace: self.paths.scrub(&backtrace),
            log,
            file: None,
            signal: None,
            path: None,
        };
        self.save(&mut report, now);
        #[cfg(all(target_arch = "wasm32", feature = "window"))]
        page::tell(&report);
        if let Some(hook) = &self.hook {
            hook(&report);
        }
        report
    }

    /// The fixed part of a native crash's report, as the handler writes it
    /// (see [`native`]): rebuilt when the GPU or the settings change.
    fn native_fixed(&self, gpu: Option<&GpuAdapter>, settings: &Settings) -> String {
        let mut t = native::header_start();
        native::line(&mut t, "game", &self.game);
        if let Some(version) = &self.game_version {
            native::line(&mut t, "game-version", version);
        }
        native::line(&mut t, "engine-version", env!("CARGO_PKG_VERSION"));
        native::line(&mut t, "os", &self.os);
        native::line(&mut t, "started-ms", &self.started_ms.to_string());
        if let Some(gpu) = gpu {
            native::line(&mut t, "gpu-name", &gpu.name);
            native::line(&mut t, "gpu-backend", &gpu.backend);
            native::line(&mut t, "gpu-type", &gpu.device_type);
            native::line(&mut t, "gpu-driver", &gpu.driver);
            native::line(&mut t, "gpu-driver-info", &gpu.driver_info);
        }
        for (name, value) in settings.lines() {
            native::line(&mut t, "setting", &format!("{name} = {value}"));
        }
        native::line(
            &mut t,
            "output-kept",
            if crate::logs::capturing() { "yes" } else { "no" },
        );
        t
    }

    /// Turns what a native crash of an earlier run left into a report:
    /// saves it (named after the crash's time) and runs the game's hook.
    fn finish_native(&self, left: &native::Left) -> CrashReport {
        let crashed_ms = left.time_ms.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64)
        });
        let uptime_secs = left
            .started_ms
            .map_or(0.0, |started| crashed_ms.saturating_sub(started) as f64 / 1000.0);
        let settings = if left.settings.is_empty() {
            Settings::default().lines()
        } else {
            left.settings.clone()
        };
        let output = left.output.iter().skip(left.output.len().saturating_sub(LOG_LINES));
        let mut report = CrashReport {
            kind: CrashKind::Native,
            message: self.paths.scrub(&left.message()),
            location: None,
            thread: None,
            time: format!("{} UTC", utc(crashed_ms / 1000, ' ', ':')),
            uptime_secs,
            game: if left.game.is_empty() {
                self.game.clone()
            } else {
                left.game.clone()
            },
            game_version: left.game_version.clone(),
            engine_version: if left.engine_version.is_empty() {
                "not known".to_string()
            } else {
                left.engine_version.clone()
            },
            os: if left.os.is_empty() {
                self.os.clone()
            } else {
                left.os.clone()
            },
            gpu: left.gpu.clone(),
            settings,
            backtrace: self.paths.scrub(&native::describe_frames(left)),
            log: left
                .output_kept
                .then(|| output.map(|line| self.paths.scrub(line)).collect()),
            file: None,
            signal: left.signal.clone(),
            path: None,
        };
        self.save(&mut report, crashed_ms / 1000);
        if let Some(hook) = &self.hook {
            hook(&report);
        }
        report
    }

    /// Saves `report` in its own file and deletes the oldest beyond [`KEPT`].
    fn save(&self, report: &mut CrashReport, now: u64) {
        let mut storage = match self.storage.try_lock() {
            Ok(s) => s,
            Err(std::sync::TryLockError::Poisoned(s)) => s.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        let Some(storage) = storage.as_mut() else {
            return;
        };
        let stamp = utc(now, '_', '-');
        let name = (1..100)
            .map(|n| match n {
                1 => format!("{FOLDER}/{stamp}.txt"),
                n => format!("{FOLDER}/{stamp}_{n}.txt"),
            })
            .find(|name| !storage.exists(name));
        let Some(name) = name else {
            return;
        };
        report.file = Some(name.clone());
        report.path = storage.dir().map(|dir| dir.join(&name));
        if let Err(e) = storage.write(&name, report.text().as_bytes()) {
            eprintln!("ponko: could not save the crash report: {e}");
            report.file = None;
            report.path = None;
            return;
        }
        for old in saved_reports(storage).into_iter().skip(KEPT) {
            let _ = storage.delete(&old);
        }
        match &report.path {
            Some(path) => eprintln!("ponko: crash report saved to {}", path.display()),
            None => eprintln!("ponko: crash report saved as {name} in the browser's storage"),
        }
    }
}

/// The run with reports on, if any, for the panic hook.
static ACTIVE: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

fn active() -> Option<Arc<Shared>> {
    match ACTIVE.try_lock() {
        Ok(a) => a.clone(),
        Err(std::sync::TryLockError::Poisoned(a)) => a.into_inner().clone(),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Reports on for one run: kept in [`Ctx`], dropping it (the run ended)
/// turns them off again.
pub(crate) struct Running {
    shared: Arc<Shared>,
    /// The game's output, kept for the report's log (desktop).
    _logs: Option<crate::logs::Capture>,
    /// The pending file a native crash writes into (desktop).
    native: Option<native::Armed>,
    /// The settings the native crash's fixed part was last prepared with.
    native_settings: Settings,
    /// When the loaded modules were last checked for the native crash's
    /// frames.
    modules_checked: Instant,
    gpu_noted: bool,
    vsync: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
        if active.as_ref().is_some_and(|a| Arc::ptr_eq(a, &self.shared)) {
            *active = None;
        }
    }
}

/// Starts reports for a run, if `config` has them on: from now on a panic
/// (or [`report`]) writes one into `storage`'s folder.
pub(crate) fn start(
    config: &CrashReports,
    headless: bool,
    storage: &Storage,
    game: &str,
    vsync: bool,
) -> Option<Running> {
    if !config.is_on(headless) {
        return None;
    }
    let shared = Arc::new(Shared {
        storage: Mutex::new(storage.is_available().then(|| storage.reopen())),
        game: game.to_string(),
        game_version: config.game_version.clone(),
        hook: config.hook.clone(),
        os: os::describe(),
        started: Instant::now(),
        started_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        paths: PathScrubber::for_game(storage.dir()),
        snapshot: Mutex::new(Snapshot::default()),
        panicked: std::sync::atomic::AtomicBool::new(false),
    });
    install_panic_hook();
    *ACTIVE.lock().unwrap_or_else(PoisonError::into_inner) = Some(shared.clone());
    let logs = crate::logs::Capture::start();
    // Native crashes of earlier runs become reports now; then this run's
    // pending file is armed (see the guide's "Crashes that are not panics").
    let native = storage.dir().and_then(|dir| {
        for left in native::collect(dir) {
            shared.finish_native(&native::Left::parse(&left));
        }
        native::arm(dir)
    });
    if native.is_some() {
        native::prepare(&shared.native_fixed(None, &Settings::default()));
    }
    Some(Running {
        shared,
        _logs: logs,
        native,
        native_settings: Settings::default(),
        modules_checked: Instant::now(),
        gpu_noted: false,
        vsync,
    })
}

/// Keeps what a report says about the frame: the GPU (once) and the
/// graphics settings. Runs at each drawn frame while reports are on.
pub(crate) fn note_frame(ctx: &mut Ctx) {
    note_gpu(ctx);
    let Some(vsync) = ctx.crash.as_ref().map(|running| running.vsync) else {
        return;
    };
    let settings = Settings {
        framed: true,
        window: ctx.window_size(),
        scale: ctx.gfx.scale_factor,
        mode: match ctx.window.mode() {
            crate::window::WindowMode::Windowed => 0,
            crate::window::WindowMode::Borderless(_) => 1,
            _ => 2,
        },
        vsync,
        #[cfg(feature = "3d")]
        three: Some({
            let s = &ctx.scene3d;
            Settings3d {
                preset: s.quality(),
                render_scale: s.post.render_scale,
                msaa: s.post.msaa,
                shadows: s.lighting.shadows,
                shadow_cascades: s.lighting.shadow_cascades,
                shadow_resolution: s.lighting.shadow_resolution,
                anti_aliasing: s.post.anti_aliasing,
                ambient_occlusion: s.post.ambient_occlusion.enabled,
                autotune: s.autotune.policy.is_some(),
            }
        }),
    };
    let Some(running) = ctx.crash.as_mut() else {
        return;
    };
    // Never wait on the frame: a report being written holds it a moment.
    if let Ok(mut snapshot) = running.shared.snapshot.try_lock() {
        snapshot.settings = settings;
    }
    // The native crash's fixed part only changes with the settings (no
    // allocation in a frame that changed nothing).
    if running.native.is_some() && running.native_settings != settings {
        running.native_settings = settings;
        prepare_native(running);
    }
    // A library loaded since (a driver, a plugin): its frames need its range.
    if running.native.is_some() && running.modules_checked.elapsed().as_secs() >= 2 {
        running.modules_checked = Instant::now();
        native::refresh_modules(false);
    }
}

/// Prepares the native crash's fixed part again, with the GPU and settings
/// of the last frame.
fn prepare_native(running: &Running) {
    let gpu = match running.shared.snapshot.try_lock() {
        Ok(snapshot) => snapshot.gpu.clone(),
        Err(_) => None,
    };
    native::prepare(&running.shared.native_fixed(gpu.as_ref(), &running.native_settings));
    native::refresh_modules(false);
}

/// Keeps the GPU the renderer opened for a report, once: when the renderer
/// starts (so a panic in setup names it) or at the first frame.
pub(crate) fn note_gpu(ctx: &mut Ctx) {
    let Some(running) = ctx.crash.as_mut() else {
        return;
    };
    if running.gpu_noted {
        return;
    }
    let Some(renderer) = ctx.gfx.renderer.as_ref() else {
        return;
    };
    let gpu = renderer.capabilities().adapter.clone();
    if let Ok(mut snapshot) = running.shared.snapshot.try_lock() {
        snapshot.gpu = Some(gpu);
        running.gpu_noted = true;
    }
    if running.gpu_noted && running.native.is_some() {
        prepare_native(running);
    }
}

#[cfg(all(test, feature = "window", feature = "3d"))]
impl Running {
    /// Reports on for a test of the frame's snapshot: not the run's
    /// (nothing is saved, the panic hook and the log are left alone).
    pub(crate) fn for_test(game: &str, vsync: bool) -> Self {
        Self {
            shared: Arc::new(Shared {
                storage: Mutex::new(None),
                game: game.to_string(),
                game_version: None,
                hook: None,
                os: os::describe(),
                started: Instant::now(),
                started_ms: 0,
                paths: PathScrubber::default(),
                snapshot: Mutex::new(Snapshot::default()),
                panicked: std::sync::atomic::AtomicBool::new(false),
            }),
            _logs: None,
            native: None,
            native_settings: Settings::default(),
            modules_checked: Instant::now(),
            gpu_noted: false,
            vsync,
        }
    }

    /// The report a crash would write now, without saving it.
    pub(crate) fn preview(&self, message: &str) -> CrashReport {
        self.shared.write(CrashKind::Error, message, None, String::new())
    }
}

thread_local! {
    /// This thread is writing a report: a panic inside it gets none.
    static WRITING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Puts the report writer in front of the panic hook that is there (the
/// standard one, or the browser page's), once per process. It does
/// nothing while no run has reports on.
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            on_panic(info);
            previous(info);
        }));
    });
}

fn on_panic(info: &std::panic::PanicHookInfo<'_>) {
    if WRITING.with(std::cell::Cell::get) {
        return;
    }
    // A worker thread in a browser can't reach the page or its storage.
    #[cfg(all(target_arch = "wasm32", feature = "window"))]
    if web_sys::window().is_none() {
        return;
    }
    let Some(shared) = active() else {
        return;
    };
    if shared.panicked.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    // The abort that may follow (panic = "abort") is this crash, reported.
    native::panic_reported();
    WRITING.with(|w| w.set(true));
    let message = info
        .payload_as_str()
        .unwrap_or("Box<dyn Any> (a panic without a message)");
    let backtrace = from_the_panic(&std::backtrace::Backtrace::force_capture().to_string());
    shared.write(
        CrashKind::Panic,
        message,
        info.location().map(ToString::to_string),
        backtrace,
    );
    WRITING.with(|w| w.set(false));
}

/// A panic's backtrace without the frames of the panic machinery above it
/// (this hook, the standard library's panic handler): it starts at the
/// frame that panicked (`core::panicking::panic_fmt`, `panic_bounds_check`
/// or the game's own function).
fn from_the_panic(backtrace: &str) -> String {
    let lines: Vec<&str> = backtrace.lines().collect();
    let is_frame = |line: &str| {
        let line = line.trim_start();
        line.split_once(':')
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    let Some(unwind) = lines
        .iter()
        .position(|l| is_frame(l) && l.contains("rust_begin_unwind"))
    else {
        return backtrace.to_string();
    };
    match lines.iter().skip(unwind + 1).position(|l| is_frame(l)) {
        Some(next) => lines[unwind + 1 + next..].join("\n"),
        None => backtrace.to_string(),
    }
}

/// `secs` since 1970 as `YYYY-MM-DD<sep>hh<tsep>mm<tsep>ss` (UTC).
fn utc(secs: u64, sep: char, tsep: char) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}{sep}{:02}{tsep}{:02}{tsep}{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The calendar date of a day number since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Makes the paths in a report's text relative to the game's folders, so
/// the report names no user: the data folder becomes `<data>`, the
/// program's folder `<game>`, the working folder `<cwd>`, the home folder
/// `~`, and any other user's home folder (`/Users/<name>`, `/home/<name>`,
/// `C:\Users\<name>`, from a library's paths in a backtrace) `~` too.
#[derive(Clone, Default)]
struct PathScrubber {
    /// Folder prefixes and what they become, longest first.
    folders: Vec<(String, &'static str)>,
}

impl PathScrubber {
    fn for_game(data: Option<&Path>) -> Self {
        let mut folders = Vec::new();
        if let Some(data) = data {
            folders.push((data.to_path_buf(), "<data>"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(dir) = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
            {
                folders.push((dir, "<game>"));
            }
            if let Ok(dir) = std::env::current_dir() {
                folders.push((dir, "<cwd>"));
            }
            let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
            if let Some(home) = home.filter(|h| !h.is_empty()) {
                folders.push((PathBuf::from(home), "~"));
            }
        }
        Self::new(folders)
    }

    fn new(folders: Vec<(PathBuf, &'static str)>) -> Self {
        let mut folders: Vec<(String, &'static str)> = folders
            .into_iter()
            .map(|(path, label)| (path.to_string_lossy().trim_end_matches(['/', '\\']).to_string(), label))
            // The root (or nothing) is no folder to hide.
            .filter(|(path, _)| path.len() > 1 && !(path.len() <= 3 && path.ends_with(':')))
            .collect();
        // The longest first: `<data>` inside the home folder stays `<data>`.
        folders.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
        Self { folders }
    }

    fn scrub(&self, text: &str) -> String {
        let mut text = text.to_string();
        for (folder, label) in &self.folders {
            if text.contains(folder.as_str()) {
                text = text.replace(folder.as_str(), label);
            }
            // The same folder with the other separator (Windows paths
            // printed with `/`).
            if folder.contains('\\') {
                let other = folder.replace('\\', "/");
                text = text.replace(&other, label);
            }
        }
        scrub_homes(&text)
    }
}

/// Replaces every user's home folder in `text` with `~`.
fn scrub_homes(text: &str) -> String {
    const HOMES: [&str; 4] = ["/Users/", "/home/", "\\Users\\", "\\Documents and Settings\\"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, marker)) = HOMES
        .iter()
        .filter_map(|m| rest.find(m).map(|at| (at, *m)))
        .min_by_key(|(at, _)| *at)
    {
        let name_start = at + marker.len();
        // Windows user names may have spaces; the others don't.
        let spaces = marker.starts_with('\\');
        let name_len = rest[name_start..]
            .find(|c: char| {
                matches!(c, '/' | '\\' | '"' | '\'' | ')' | ']' | '>' | ',' | ';') || (c.is_whitespace() && !spaces)
            })
            .unwrap_or(rest.len() - name_start);
        if name_len == 0 {
            out.push_str(&rest[..name_start]);
            rest = &rest[name_start..];
            continue;
        }
        // A Windows drive (`C:`) before `\Users\` goes too.
        let bytes = rest.as_bytes();
        let start = if at >= 2 && bytes[at - 1] == b':' && bytes[at - 2].is_ascii_alphabetic() {
            at - 2
        } else {
            at
        };
        out.push_str(&rest[..start]);
        out.push('~');
        rest = &rest[name_start + name_len..];
    }
    out.push_str(rest);
    out
}

/// The operating system, its version and the CPU architecture.
mod os {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn describe() -> String {
        #[cfg(feature = "window")]
        if let Some(agent) = web_sys::window().and_then(|w| w.navigator().user_agent().ok()) {
            return format!("browser: {agent}");
        }
        "browser".to_string()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn describe() -> String {
        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        match (version(), kernel()) {
            (Some(version), Some(kernel)) => format!("{os} {version} {arch} ({kernel})"),
            (Some(version), None) => format!("{os} {version} {arch}"),
            (None, Some(kernel)) => format!("{os} {arch} ({kernel})"),
            (None, None) => format!("{os} {arch}"),
        }
    }

    /// The system's own version: macOS's product version, a Linux
    /// distribution's name, Windows' version and build.
    #[cfg(target_os = "macos")]
    fn version() -> Option<String> {
        let mut buf = [0u8; 64];
        let mut len = buf.len();
        // SAFETY: the name is a NUL-terminated string, `buf` has `len`
        // writable bytes and sysctlbyname writes at most that many,
        // updating `len`.
        let ok = unsafe {
            libc::sysctlbyname(
                c"kern.osproductversion".as_ptr(),
                buf.as_mut_ptr().cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        } == 0;
        if !ok {
            return None;
        }
        let text = &buf[..len.min(buf.len())];
        let text = text.split(|&b| b == 0).next().unwrap_or_default();
        Some(String::from_utf8_lossy(text).trim().to_string()).filter(|v| !v.is_empty())
    }

    #[cfg(target_os = "linux")]
    fn version() -> Option<String> {
        let release = std::fs::read_to_string("/etc/os-release").ok()?;
        release.lines().find_map(|line| {
            let value = line.strip_prefix("PRETTY_NAME=")?;
            Some(value.trim_matches('"').to_string()).filter(|v| !v.is_empty())
        })
    }

    #[cfg(windows)]
    fn version() -> Option<String> {
        #[repr(C)]
        struct OsVersionInfo {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            service_pack: [u16; 128],
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
        }
        let mut info = OsVersionInfo {
            size: size_of::<OsVersionInfo>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service_pack: [0; 128],
        };
        // SAFETY: `info` is an OSVERSIONINFOW with its size set, which
        // RtlGetVersion fills in; it always succeeds for that size.
        let status = unsafe { RtlGetVersion(&raw mut info) };
        (status == 0).then(|| format!("{}.{}.{}", info.major, info.minor, info.build))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows, target_arch = "wasm32")))]
    fn version() -> Option<String> {
        None
    }

    /// The kernel's name and release, from `uname` (never the machine's
    /// network name).
    #[cfg(unix)]
    fn kernel() -> Option<String> {
        // SAFETY: utsname is plain arrays of C chars; all zero is valid.
        let mut name: libc::utsname = unsafe { std::mem::zeroed() };
        // SAFETY: `name` is a writable utsname, which uname fills in.
        if unsafe { libc::uname(&raw mut name) } != 0 {
            return None;
        }
        let field = |chars: &[libc::c_char]| {
            let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
            String::from_utf8_lossy(&bytes).into_owned()
        };
        Some(format!("{} {}", field(&name.sysname), field(&name.release)))
    }

    #[cfg(not(any(unix, target_arch = "wasm32")))]
    fn kernel() -> Option<String> {
        None
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_system_is_named_with_its_architecture_and_no_machine_name() {
            let text = super::describe();
            assert!(text.starts_with(std::env::consts::OS), "{text}");
            assert!(text.contains(std::env::consts::ARCH), "{text}");
            #[cfg(any(target_os = "macos", windows))]
            assert!(super::version().is_some(), "the system's version is known: {text}");
            #[cfg(unix)]
            {
                assert!(super::kernel().is_some(), "{text}");
                // SAFETY: as in `kernel`.
                let mut name: libc::utsname = unsafe { std::mem::zeroed() };
                // SAFETY: as in `kernel`.
                assert_eq!(unsafe { libc::uname(&raw mut name) }, 0);
                let host: Vec<u8> = name
                    .nodename
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                let host = String::from_utf8_lossy(&host);
                if host.len() > 3 {
                    assert!(!text.contains(host.as_ref()), "the machine's name stays out: {text}");
                }
            }
        }
    }
}

/// The browser page learns of a report through an event.
#[cfg(all(target_arch = "wasm32", feature = "window"))]
mod page {
    use wasm_bindgen::JsValue;

    /// The `window` event a report sends, with `{ name, text, kind,
    /// message }` as its `detail`; the same object is `window.ponkoCrashReport`.
    pub(super) const EVENT: &str = "ponkocrashreport";

    pub(super) fn tell(report: &super::CrashReport) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let detail = js_sys::Object::new();
        let set = |name: &str, value: &str| {
            let _ = js_sys::Reflect::set(&detail, &JsValue::from_str(name), &JsValue::from_str(value));
        };
        set("name", report.file().unwrap_or("crash-reports/crash-report.txt"));
        set("text", &report.text());
        set(
            "kind",
            match report.kind() {
                super::CrashKind::Panic => "panic",
                super::CrashKind::Error => "error",
                super::CrashKind::Native => "native",
            },
        );
        set("message", report.message());
        let _ = js_sys::Reflect::set(&window, &JsValue::from_str("ponkoCrashReport"), &detail);
        let init = web_sys::CustomEventInit::new();
        init.set_detail(&detail);
        if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict(EVENT, &init) {
            let _ = window.dispatch_event(&event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_off_in_development_builds_and_headless_runs_by_default() {
        assert!(!default_on(true, false), "a development build");
        assert!(!default_on(true, true));
        assert!(!default_on(false, true), "a headless release run (a server, a test)");
        assert!(default_on(false, false), "a release build with a window");
        assert!(CrashReports::from(true).is_on(true));
        assert!(!CrashReports::from(false).is_on(false));
    }

    #[test]
    fn a_panics_backtrace_starts_where_it_panicked() {
        let trace = "   0: std::backtrace::Backtrace::create\n             at backtrace.rs:331:13\n   1: ponko::crash::on_panic\n   2: __rustc::rust_begin_unwind\n             at panicking.rs:676:5\n   3: core::panicking::panic_fmt\n   4: game::update\n             at src/main.rs:3:5";
        assert_eq!(
            from_the_panic(trace),
            "   3: core::panicking::panic_fmt\n   4: game::update\n             at src/main.rs:3:5"
        );
        // A backtrace without the panic handler (unsupported, or symbols
        // stripped) is kept whole.
        assert_eq!(from_the_panic("disabled backtrace"), "disabled backtrace");
    }

    #[test]
    fn times_are_written_in_utc() {
        assert_eq!(utc(0, ' ', ':'), "1970-01-01 00:00:00");
        // 2026-11-21 14:03:07 UTC.
        assert_eq!(utc(1_795_269_787, '_', '-'), "2026-11-21_14-03-07");
        // A leap day.
        assert_eq!(utc(951_782_400, ' ', ':'), "2000-02-29 00:00:00");
    }

    #[test]
    fn paths_are_made_relative_to_the_games_folders() {
        let paths = PathScrubber::new(vec![
            (PathBuf::from("/home/ana/.local/share/space-crab"), "<data>"),
            (PathBuf::from("/opt/games/space-crab"), "<game>"),
            (PathBuf::from("/home/ana"), "~"),
        ]);
        assert_eq!(
            paths.scrub("could not read /home/ana/.local/share/space-crab/saves/1.json"),
            "could not read <data>/saves/1.json"
        );
        assert_eq!(paths.scrub("/opt/games/space-crab/assets.pack"), "<game>/assets.pack");
        assert_eq!(paths.scrub("/home/ana/Desktop/x.png"), "~/Desktop/x.png");
    }

    #[test]
    fn every_users_home_folder_is_hidden() {
        assert_eq!(
            scrub_homes("at /Users/dev/.cargo/registry/src/wgpu-30/src/lib.rs:5"),
            "at ~/.cargo/registry/src/wgpu-30/src/lib.rs:5"
        );
        assert_eq!(scrub_homes("C:\\Users\\Ana Lee\\AppData"), "~\\AppData");
        assert_eq!(scrub_homes("C:\\Users\\ana\\AppData\\x"), "~\\AppData\\x");
        assert_eq!(scrub_homes("'/home/bob' and /home/carol/x"), "'~' and ~/x");
        assert_eq!(scrub_homes("no paths here"), "no paths here");
        assert_eq!(scrub_homes("ends with /home/"), "ends with /home/");
    }

    #[test]
    fn windows_folders_are_found_with_either_separator() {
        let paths = PathScrubber::new(vec![(PathBuf::from("D:\\Games\\Space Crab"), "<game>")]);
        assert_eq!(paths.scrub("D:\\Games\\Space Crab\\assets.pack"), "<game>\\assets.pack");
        assert_eq!(paths.scrub("D:/Games/Space Crab/assets.pack"), "<game>/assets.pack");
    }

    #[test]
    fn only_report_files_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("ponko-crash-list-{}", std::process::id()));
        let mut storage = Storage::in_folder(&dir);
        for name in [
            "crash-reports/2026-01-02_00-00-00.txt",
            "crash-reports/2026-01-01_00-00-00.txt",
            "crash-reports/2026-01-02_00-00-00_2.txt",
            "crash-reports/notes/old.txt",
            "crash-reports/readme.md",
            "saves/crash-reports.txt",
        ] {
            storage.write(name, b"x").unwrap();
        }
        assert_eq!(
            saved_reports(&storage),
            [
                "crash-reports/2026-01-02_00-00-00_2.txt",
                "crash-reports/2026-01-02_00-00-00.txt",
                "crash-reports/2026-01-01_00-00-00.txt",
            ]
        );
        assert!(saved_reports(&Storage::default()).is_empty(), "storage off: none");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_settings_of_a_run_that_drew_nothing_say_so() {
        assert_eq!(
            Settings::default().lines(),
            [("frames".to_string(), "none drawn yet".to_string())]
        );
    }
}
