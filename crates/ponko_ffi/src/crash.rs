//! Crash reports from C (Rust `ponko::crash`): turn them on or off, name
//! the game's version, run a hook with each report, list the reports of
//! earlier runs, and write one for the game's own fatal error.
//!
//! ```c
//! static void on_crash(const PonkoCrashReport* report, void* user) {
//!     fprintf(stderr, "Sorry! A report is in %s\n", report->path ? report->path : "(not saved)");
//! }
//! PonkoCrashReports reports = {.struct_size = sizeof reports, .version = PONKO_CRASH_REPORTS_VERSION};
//! reports.mode = PONKO_CRASH_REPORTS_DEFAULT; // on in release builds with a window
//! reports.game_version = "1.4.2";
//! reports.on_crash = on_crash;
//! ponko_game_crash_reports(game, &reports);
//! ```

use std::ffi::{CString, c_char, c_void};
use std::mem::{offset_of, size_of};

use ponko::crash::{CrashKind, CrashReport, CrashReports};

use crate::ctx::{ctx_access, text};
use crate::descriptors::{Versioned, read_desc};
use crate::game::{build, ok};
use crate::{Failure, PonkoCtx, PonkoGame, PonkoStatus, boundary, error, invalid, write_text};

/// The version of `PonkoCrashReports` this header describes.
pub const PONKO_CRASH_REPORTS_VERSION: u32 = 1;

/// Whether a game writes crash reports (`PonkoCrashReports::mode`).
/// ```
/// assert_eq!(ponko_ffi::PonkoCrashReportsMode::PONKO_CRASH_REPORTS_OFF as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoCrashReportsMode {
    /// The build's default: on in release builds of a game with a window,
    /// off in development builds (debug assertions) and headless runs.
    PONKO_CRASH_REPORTS_DEFAULT = 0,
    /// On in every build, also headless (a dedicated server, a test).
    PONKO_CRASH_REPORTS_ON = 1,
    /// Off in every build.
    PONKO_CRASH_REPORTS_OFF = 2,
}

/// What stopped the game (`PonkoCrashReport::kind`, Rust `CrashKind`).
/// ```
/// assert_eq!(ponko_ffi::PonkoCrashKind::PONKO_CRASH_NATIVE as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoCrashKind {
    /// Rust code panicked: the engine's.
    PONKO_CRASH_PANIC = 0,
    /// A fatal error: the engine couldn't go on, or the game said so with
    /// `ponko_crash_report`.
    PONKO_CRASH_ERROR = 1,
    /// A crash that is not a panic: a segmentation fault or an `abort()`
    /// (in the game's C or C++ code too), an access violation on Windows.
    /// Its report is finished, and the hook runs, at the game's next start.
    PONKO_CRASH_NATIVE = 2,
}

/// One crash report, as the hook gets it (Rust `CrashReport`). Every string
/// is NUL-terminated UTF-8 and lives only while the hook runs: copy what you
/// keep. Optional fields are null when there is nothing to say. Later
/// versions of the library may add fields at the end.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoCrashReport {
    /// A `PonkoCrashKind`.
    pub kind: u32,
    /// What went wrong: the panic's message, or the error's.
    pub message: *const c_char,
    /// For a panic, where in the source it happened; null otherwise.
    pub location: *const c_char,
    /// The name of the thread that panicked, when it has one; else null.
    pub thread: *const c_char,
    /// When it happened, in UTC: `2026-11-21 14:03:07 UTC`.
    pub time: *const c_char,
    /// Seconds since the game started.
    pub uptime_secs: f64,
    /// The game's title.
    pub game: *const c_char,
    /// The game's version (`PonkoCrashReports::game_version`); null if not set.
    pub game_version: *const c_char,
    /// The engine's version.
    pub engine_version: *const c_char,
    /// The operating system, its version and the CPU architecture.
    pub os: *const c_char,
    /// The GPU the renderer opened, `name (backend, type)`; null before the
    /// first frame and in headless runs.
    pub gpu: *const c_char,
    /// The backtrace, paths made relative.
    pub backtrace: *const c_char,
    /// The report's name in the game's storage
    /// (`crash-reports/2026-11-21_14-03-07.txt`); null when it wasn't saved.
    pub file: *const c_char,
    /// Where the report is on the disk; null in a browser and when it wasn't saved.
    pub path: *const c_char,
    /// For a native crash, the signal or exception (`SIGSEGV`); else null.
    pub signal: *const c_char,
    /// The whole report as the file has it.
    pub text: *const c_char,
}

/// The game's code that runs with each report: `report` and its strings
/// are valid only during the call.
/// ```
/// let none: ponko_ffi::PonkoCrashHookFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoCrashHookFn = Option<unsafe extern "C" fn(report: *const PonkoCrashReport, user: *mut c_void)>;

/// Whether a game writes crash reports and what runs with each (Rust
/// `CrashReports`), for `ponko_game_crash_reports`. A versioned descriptor:
/// set `struct_size` and `version`; a zeroed rest is the build's default
/// with no version and no hook.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoCrashReports {
    /// `sizeof(PonkoCrashReports)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_CRASH_REPORTS_VERSION` as the caller compiled it.
    pub version: u32,
    /// A `PonkoCrashReportsMode`.
    pub mode: u32,
    /// The game's version, for the report's `game version` line (copied);
    /// null: `not set`.
    pub game_version: *const c_char,
    /// Runs with each report, right after it was saved; null for none.
    pub on_crash: PonkoCrashHookFn,
    /// Passed to `on_crash`.
    pub user: *mut c_void,
}

const _: () = assert!(offset_of!(PonkoCrashReports, struct_size) == 0 && offset_of!(PonkoCrashReports, version) == 4);
impl Versioned for PonkoCrashReports {
    const NAME: &'static str = "PonkoCrashReports";
    const VERSION: u32 = PONKO_CRASH_REPORTS_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, user) + size_of::<*mut c_void>()),
            _ => None,
        }
    }
}

/// The hook and its user pointer, shared with the panic hook (any thread).
struct Hook {
    call: unsafe extern "C" fn(*const PonkoCrashReport, *mut c_void),
    user: *mut c_void,
}

// SAFETY: the caller of `ponko_game_crash_reports` promises the hook may
// run on any thread with `user` (its Ownership paragraph).
unsafe impl Send for Hook {}
// SAFETY: as above; the hook only passes `user` on.
unsafe impl Sync for Hook {}

/// A C string of `s`, NULs taken out (a report's text may hold any bytes).
fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).unwrap_or_default()
}

/// Runs the hook with `report` turned into C.
fn call_hook(hook: &Hook, report: &CrashReport) {
    let s = |v: &str| c_string(v);
    let opt = |v: Option<&str>| v.map(c_string);
    let gpu = report
        .gpu()
        .map(|g| c_string(&format!("{} ({}, {})", g.name, g.backend, g.device_type)));
    let path = report.path().map(|p| c_string(&p.to_string_lossy()));
    let (message, location, thread, time) = (
        s(report.message()),
        opt(report.location()),
        opt(report.thread()),
        s(report.time()),
    );
    let (game, game_version, engine_version, os) = (
        s(report.game()),
        opt(report.game_version()),
        s(report.engine_version()),
        s(report.os()),
    );
    let (backtrace, file, signal, text) = (
        s(report.backtrace()),
        opt(report.file()),
        opt(report.signal()),
        s(&report.text()),
    );
    let ptr = |v: &Option<CString>| v.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    let c = PonkoCrashReport {
        kind: match report.kind() {
            CrashKind::Panic => PonkoCrashKind::PONKO_CRASH_PANIC,
            CrashKind::Native => PonkoCrashKind::PONKO_CRASH_NATIVE,
            _ => PonkoCrashKind::PONKO_CRASH_ERROR,
        } as u32,
        message: message.as_ptr(),
        location: ptr(&location),
        thread: ptr(&thread),
        time: time.as_ptr(),
        uptime_secs: report.uptime_secs(),
        game: game.as_ptr(),
        game_version: ptr(&game_version),
        engine_version: engine_version.as_ptr(),
        os: os.as_ptr(),
        gpu: ptr(&gpu),
        backtrace: backtrace.as_ptr(),
        file: ptr(&file),
        path: ptr(&path),
        signal: ptr(&signal),
        text: text.as_ptr(),
    };
    // SAFETY: the caller's hook takes its own `user`; `c` and every string
    // it points to live until the call returns.
    unsafe { (hook.call)(&c, hook.user) };
}

/// Sets whether the game writes crash reports and what runs with each
/// (Rust `Game::crash_reports`). A report is a text file in the game's
/// storage (`crash-reports/<time>.txt`, so the game needs an app id or a
/// data folder; the newest ten are kept) written when the engine panics or
/// stops on a fatal error, when the game calls `ponko_crash_report`, and,
/// at the next start, after a crash that is not a panic: a segmentation
/// fault or an `abort()` in the game's own C or C++ code too. The engine
/// shows and sends nothing itself; the hook is where a game shows its own
/// message. Without this call the build's default applies, with no
/// version and no hook. Bad mode: `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `reports` and `game_version` are read during the call (the
/// version is copied). `user` stays the caller's: it is passed to
/// `on_crash` whenever a report is written while the game runs, on the
/// thread that crashed (any thread), so it must stay valid until
/// `ponko_game_run` or `ponko_game_run_headless` returns. The hook runs
/// inside the crash handling: it must return normally, be quick, and not
/// crash itself.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_crash_reports(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `game` null or live; `reports` null or a readable descriptor with its
/// header set, its `game_version` null or NUL-terminated; `on_crash` safe to
/// call with `user` from any thread while the game runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_crash_reports(
    game: *mut PonkoGame,
    reports: *const PonkoCrashReports,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| match settings_from_c(reports) {
            Ok(settings) => ok(g.crash_reports(settings)),
            Err(e) => (g, Err(e)),
        })
    }
}

/// The Rust settings a C descriptor describes.
///
/// # Safety
/// As `ponko_game_crash_reports`.
unsafe fn settings_from_c(reports: *const PonkoCrashReports) -> Result<CrashReports, Failure> {
    // SAFETY: forwarded caller contract.
    let d = unsafe {
        read_desc(
            reports,
            PonkoCrashReports {
                struct_size: size_of::<PonkoCrashReports>() as u32,
                version: PONKO_CRASH_REPORTS_VERSION,
                mode: 0,
                game_version: std::ptr::null(),
                on_crash: None,
                user: std::ptr::null_mut(),
            },
        )?
    };
    let mut settings = match d.mode {
        0 => CrashReports::default(),
        1 => CrashReports::on(),
        2 => CrashReports::off(),
        _ => return Err(invalid("mode is not a PonkoCrashReportsMode")),
    };
    if !d.game_version.is_null() {
        // SAFETY: caller contract: NUL-terminated.
        settings = settings.game_version(unsafe { text(d.game_version)? });
    }
    if let Some(call) = d.on_crash {
        let hook = Hook { call, user: d.user };
        settings = settings.on_crash(move |report| call_hook(&hook, report));
    }
    Ok(settings)
}

/// Writes a crash report for the game's own fatal error, `message`, with a
/// backtrace of this call (Rust `ponko::crash::report`), and runs the hook
/// with it: for a game that is about to stop without crashing. Reports off
/// (or no game running on this thread's process): `PONKO_NOT_FOUND`. The
/// game goes on running; stopping is the caller's.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_crash_report(std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `message` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_crash_report(message: *const c_char) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller contract.
        let message = unsafe { text(message)? };
        ponko::crash::report(message)
            .map(|_| ())
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "crash reports are off"))
    })
}

/// The crash reports in the game's storage (Rust
/// `ponko::crash::saved_reports`), newest first, one name per line (`\n`
/// between them): read one with `ponko_ctx_storage_read`, delete it with
/// `ponko_ctx_storage_delete`. For a game that offers, at its next start,
/// to send the reports of earlier runs. `*out_len` is always the length;
/// the text is copied only when it fits.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_saved_crash_reports(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_saved_crash_reports(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let names = ctx_access(ctx, |c| Ok(ponko::crash::saved_reports(&c.storage)))?;
        write_text(buffer, capacity, out_len, &names.join("\n"))
    })
}
