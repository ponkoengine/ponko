//! Crash reports for crashes that are not panics (`ponko::crash`, kind
//! `native`): a child process (this test binary run again, with
//! `PONKO_NATIVE_CRASH_CHILD` set) crashes in native code, and the game's
//! next start finds the report it left.
//!
//! The child installs handlers of its own first, which end it with
//! `_exit(42)`. The engine's handler writes the report and then hands the
//! crash on to them, as it does to any handler that was there before, so the
//! child exits with code 42 instead of dying of the signal: no core file, no
//! system crash report and no crash dialog for it. The test that lets a child
//! really die of the signal runs on Linux, and elsewhere only with
//! `PONKO_CRASH_TESTS_DIE=1` (macOS and Windows keep a system crash report of
//! it, and Windows may show its error dialog).
//!
//! No window opens: the games run headless.
#![cfg(any(unix, windows))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ponko::crash::{self, CrashKind, CrashReport, CrashReports};
use ponko::prelude::*;
use ponko::storage::Storage;

/// What the child does: `segv` or `abort`.
const CHILD: &str = "PONKO_NATIVE_CRASH_CHILD";
/// `1`: the child installs its exiting handlers first.
const HANDLED: &str = "PONKO_NATIVE_CRASH_HANDLED";
/// The child game's data folder.
const DIR: &str = "PONKO_NATIVE_CRASH_DIR";
/// The exit code of the child's own handlers.
const EXIT_CODE: i32 = 42;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_turn() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-native-crash-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The child: does nothing unless the parent asked it to crash.
#[test]
fn child_that_crashes_when_asked() {
    let Ok(case) = std::env::var(CHILD) else {
        return;
    };
    let dir = PathBuf::from(std::env::var_os(DIR).unwrap_or_default());
    if std::env::var(HANDLED).as_deref() == Ok("1") {
        child::install_exiting_handlers();
    }
    Game::new("Space Crab", 1, 1)
        .data_dir(&dir)
        .crash_reports(CrashReports::on().game_version("1.2.3"))
        .run_headless(1, move |_ctx| {
            println!("the crab's last words");
            child::print_like_c(b"a C library's last words\n");
            std::thread::sleep(Duration::from_millis(100));
            match case.as_str() {
                "abort" => child::abort(),
                _ => child::segfault(),
            };
        });
    // Never reached: the crash ends the process.
    std::process::exit(1);
}

/// Runs the child and waits for it.
fn crash_child(case: &str, handled: bool, dir: &Path) -> Output {
    let exe = std::env::current_exe().unwrap();
    Command::new(exe)
        .args([
            "child_that_crashes_when_asked",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, case)
        .env(HANDLED, if handled { "1" } else { "0" })
        .env(DIR, dir)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// Starts the game again in `dir`: returns the reports its hook got.
fn next_start(dir: &Path) -> Vec<CrashReport> {
    let seen: Arc<Mutex<Vec<CrashReport>>> = Arc::default();
    let into = seen.clone();
    Game::new("Space Crab", 1, 1)
        .data_dir(dir)
        .crash_reports(CrashReports::on().on_crash(move |report| into.lock().unwrap().push(report.clone())))
        .run_headless(0, |_ctx| {});
    let seen = seen.lock().unwrap().clone();
    seen
}

fn pending_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir.join(crash::FOLDER))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(".native-"))
                .collect()
        })
        .unwrap_or_default()
}

/// The report the next start wrote, after `output` crashed in `dir`.
fn the_report(dir: &Path, output: &Output) -> (CrashReport, String) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let seen = next_start(dir);
    assert_eq!(seen.len(), 1, "one report for the crash; the child said:\n{stderr}");
    let names = crash::saved_reports(&Storage::in_folder(dir));
    assert_eq!(names.len(), 1, "{names:?}");
    let text = String::from_utf8(Storage::in_folder(dir).read(&names[0]).unwrap()).unwrap();
    assert_eq!(seen[0].text(), text, "the hook gets what the file holds");
    assert_eq!(seen[0].file(), Some(names[0].as_str()));
    assert!(pending_files(dir).is_empty(), "{:?}", pending_files(dir));
    (seen[0].clone(), text)
}

fn assert_holds_the_crash(text: &str, signals: &[&str]) {
    for line in [
        "Space Crab crash report",
        "kind: native",
        "game version: 1.2.3",
        &format!("engine version: {}", env!("CARGO_PKG_VERSION")),
        &format!("os: {}", std::env::consts::OS),
        "gpu: not opened",
        "backtrace:\n  not kept for a crash that is not a panic",
        "  the crab's last words",
        "  a C library's last words",
    ] {
        assert!(text.contains(line), "the report has {line:?}:\n{text}");
    }
    assert!(
        signals.iter().any(|s| text.contains(&format!("signal: {s}\n"))),
        "the report names one of {signals:?}:\n{text}"
    );
}

#[test]
fn a_segmentation_fault_in_native_code_leaves_a_report_for_the_next_start() {
    let _turn = take_turn();
    let dir = temp_dir("segv");
    let output = crash_child("segv", true, &dir);
    assert_eq!(
        output.status.code(),
        Some(EXIT_CODE),
        "the crash went on to the handler that was there before:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (report, text) = the_report(&dir, &output);
    // macOS raises SIGBUS for some protected pages, Linux SIGSEGV.
    let signals: &[&str] = if cfg!(windows) {
        &["EXCEPTION_ACCESS_VIOLATION"]
    } else {
        &["SIGSEGV", "SIGBUS"]
    };
    assert_holds_the_crash(&text, signals);
    assert_eq!(report.kind(), CrashKind::Native);
    let message = report.message();
    assert!(
        message.contains("invalid memory access") && message.contains(" at address 0x"),
        "{message}"
    );
    assert!(report.uptime_secs() >= 0.1, "{}", report.uptime_secs());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_abort_in_native_code_leaves_a_report_for_the_next_start() {
    let _turn = take_turn();
    let dir = temp_dir("abort");
    let output = crash_child("abort", true, &dir);
    assert_eq!(
        output.status.code(),
        Some(EXIT_CODE),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (report, text) = the_report(&dir, &output);
    assert_holds_the_crash(&text, &["SIGABRT"]);
    assert_eq!(report.signal(), Some("SIGABRT"));
    assert!(report.message().starts_with("SIGABRT: abort"), "{}", report.message());
    let _ = std::fs::remove_dir_all(dir);
}

/// The default path: no handler before the engine's but the system's (and
/// Rust's own, for a fault): the process still dies of the signal.
#[test]
fn a_crash_still_ends_the_process_as_a_crash_after_the_report() {
    if !cfg!(target_os = "linux") && std::env::var("PONKO_CRASH_TESTS_DIE").as_deref() != Ok("1") {
        return;
    }
    let _turn = take_turn();
    let dir = temp_dir("die");
    let output = crash_child("segv", false, &dir);
    assert!(!output.status.success());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let signal = output.status.signal();
        assert!(
            signal == Some(libc::SIGSEGV) || signal == Some(libc::SIGBUS),
            "the child died of the signal: {:?}",
            output.status
        );
    }
    let (_report, text) = the_report(&dir, &output);
    assert!(text.contains("kind: native"), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_run_that_ends_normally_leaves_no_report_and_no_pending_file() {
    let _turn = take_turn();
    let dir = temp_dir("normal");
    let seen_pending = Arc::new(Mutex::new(Vec::new()));
    let into = seen_pending.clone();
    let while_running = dir.clone();
    Game::new("Space Crab", 1, 1)
        .data_dir(&dir)
        .crash_reports(true)
        .run_headless(1, move |_ctx| *into.lock().unwrap() = pending_files(&while_running));
    assert_eq!(seen_pending.lock().unwrap().len(), 1, "armed while it runs");
    assert!(pending_files(&dir).is_empty(), "{:?}", pending_files(&dir));
    assert!(next_start(&dir).is_empty());
    assert!(crash::saved_reports(&Storage::in_folder(&dir)).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_empty_pending_file_of_a_killed_run_is_cleared_without_a_report() {
    let _turn = take_turn();
    let dir = temp_dir("killed");
    std::fs::create_dir_all(dir.join(crash::FOLDER)).unwrap();
    std::fs::write(dir.join(crash::FOLDER).join(".native-1-0"), b"").unwrap();
    assert!(next_start(&dir).is_empty());
    assert!(pending_files(&dir).is_empty());
    assert!(crash::saved_reports(&Storage::in_folder(&dir)).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A second copy of the game, running at the same time, holds its own
/// pending file: a start leaves it alone.
#[test]
fn a_pending_file_another_running_copy_holds_is_left_alone() {
    let _turn = take_turn();
    let dir = temp_dir("held");
    let other = dir.clone();
    Game::new("Space Crab", 1, 1)
        .data_dir(&dir)
        .crash_reports(true)
        .run_headless(1, move |_ctx| {
            let held = pending_files(&other);
            assert_eq!(held.len(), 1);
            // Another start in the same folder while this run holds it.
            assert!(next_start(&other).is_empty());
            assert_eq!(pending_files(&other), held, "the running copy's file stays");
        });
    assert!(pending_files(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// What the child does to crash, as a native library would.
mod child {
    use super::EXIT_CODE;

    /// Writes straight to the standard output's descriptor, past Rust's
    /// `println!`, the way C's `printf` ends up.
    pub fn print_like_c(bytes: &[u8]) {
        // SAFETY: `bytes` is readable for its length.
        unsafe { libc::write(1, bytes.as_ptr().cast(), bytes.len() as _) };
    }

    pub fn abort() {
        // SAFETY: C's abort; it never returns.
        unsafe { libc::abort() }
    }

    #[cfg(unix)]
    extern "C" fn exit_now(_sig: libc::c_int) {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(EXIT_CODE) }
    }

    /// Handlers that end the process at once, installed before the game's.
    #[cfg(unix)]
    pub fn install_exiting_handlers() {
        for sig in [libc::SIGSEGV, libc::SIGBUS, libc::SIGABRT] {
            // SAFETY: zeroed is a valid sigaction; the handler only calls _exit.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = exit_now as *const () as libc::sighandler_t;
                libc::sigemptyset(&raw mut action.sa_mask);
                libc::sigaction(sig, &raw const action, std::ptr::null_mut());
            }
        }
    }

    /// A real memory fault: a write to a page with no access.
    #[cfg(unix)]
    pub fn segfault() {
        // SAFETY: maps one fresh page with no access, then writes to it,
        // which faults; the handlers never return here.
        unsafe {
            let page = libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            assert_ne!(page, libc::MAP_FAILED);
            std::ptr::write_volatile(page.cast::<u8>().add(16), 1);
        }
        unreachable!("the write faulted");
    }

    #[cfg(windows)]
    mod win {
        use std::ffi::c_void;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            pub fn SetUnhandledExceptionFilter(
                filter: Option<unsafe extern "system" fn(*const c_void) -> i32>,
            ) -> *const c_void;
            pub fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        }
        pub const MEM_COMMIT_RESERVE: u32 = 0x1000 | 0x2000;
        pub const PAGE_NOACCESS: u32 = 0x01;
    }

    #[cfg(windows)]
    unsafe extern "system" fn exit_from_filter(_pointers: *const std::ffi::c_void) -> i32 {
        // SAFETY: ends the process.
        unsafe { libc::_exit(EXIT_CODE) }
    }

    #[cfg(windows)]
    extern "C" fn exit_on_abort(_sig: libc::c_int) {
        // SAFETY: ends the process.
        unsafe { libc::_exit(EXIT_CODE) }
    }

    #[cfg(windows)]
    pub fn install_exiting_handlers() {
        // SAFETY: installs handlers that only end the process.
        unsafe {
            win::SetUnhandledExceptionFilter(Some(exit_from_filter));
            libc::signal(libc::SIGABRT, exit_on_abort as *const () as libc::sighandler_t);
        }
    }

    #[cfg(windows)]
    pub fn segfault() {
        // SAFETY: allocates one page with no access, then writes to it.
        unsafe {
            let page = win::VirtualAlloc(std::ptr::null_mut(), 4096, win::MEM_COMMIT_RESERVE, win::PAGE_NOACCESS);
            assert!(!page.is_null());
            std::ptr::write_volatile(page.cast::<u8>().add(16), 1);
        }
        unreachable!("the write faulted");
    }
}
