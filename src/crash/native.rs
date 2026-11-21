//! Crash reports for crashes that are not panics: a signal on Linux and
//! macOS (`SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE`, `SIGABRT`), an unhandled
//! exception or the C runtime's `SIGABRT` on Windows.
//!
//! The guide's "Crashes that are not panics" has the design. In short: a
//! crashing process may hold any lock (the allocator's, standard output's,
//! the engine's), so the handler only copies bytes prepared ahead of time
//! into a file opened ahead of time, with async-signal-safe calls (`write`,
//! `clock_gettime`, `nanosleep`, `sigaction`, `raise`; on Windows `WriteFile`,
//! `GetSystemTimeAsFileTime`, `Sleep`). It allocates nothing, takes no lock
//! and formats nothing beyond writing digits into a buffer on its stack.
//! The next start of the game turns what it wrote into an ordinary report.
//!
//! What is prepared, in static memory read with atomic loads:
//! - [`PREPARED`]: the report's fixed part as text (the game, the versions,
//!   the system, the GPU, the settings), rebuilt off the handler whenever one
//!   of them changes, into the idle one of two halves before switching.
//! - [`OUTPUT`]: the last bytes the game printed, line by line, filled by the
//!   threads that copy standard output and error ([`keep_line`]).
//! - The pending file, `crash-reports/.native-<pid>-<n>` in the game's data
//!   folder, open and locked while the run lasts.

// The handler's statics are shared by the Unix and Windows paths; a
// platform with neither only keeps the parsing (the next start's half).
#![cfg_attr(not(any(unix, windows)), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// The first line of a pending file.
const MAGIC: &str = "ponko native crash";

/// The pending files' names start with this, in [`super::FOLDER`].
const PENDING_PREFIX: &str = ".native-";

/// The size of each half of [`PREPARED`]: the fixed part is cut there.
const HALF: usize = 4096;

/// How many of the last printed bytes are kept for a native crash.
pub(crate) const OUTPUT_BYTES: usize = 16 * 1024;

/// A line longer than this goes into [`OUTPUT`] cut, so one huge line can't
/// push out every other.
const OUTPUT_LINE: usize = 1024;

/// How long the handler lets the copy threads pass on what was printed just
/// before the crash, when the game's output is kept (something was).
const SETTLE_MS: u64 = 50;

/// Text prepared for the handler: two halves of atomic bytes, the engine
/// fills the idle one and then switches `active` to it.
struct Prepared {
    halves: [[AtomicU8; HALF]; 2],
    lens: [AtomicUsize; 2],
    active: AtomicUsize,
}

static PREPARED: Prepared = Prepared {
    halves: [const { [const { AtomicU8::new(0) }; HALF] }; 2],
    lens: [const { AtomicUsize::new(0) }; 2],
    active: AtomicUsize::new(0),
};

/// Only one thread prepares at a time (never the handler).
static PREPARING: Mutex<()> = Mutex::new(());

/// The ring of the last printed bytes: `OUTPUT_END` counts every byte ever
/// kept, and byte `i` lives at `i % OUTPUT_BYTES`.
static OUTPUT: [AtomicU8; OUTPUT_BYTES] = [const { AtomicU8::new(0) }; OUTPUT_BYTES];
static OUTPUT_END: AtomicUsize = AtomicUsize::new(0);

/// A crash was written (or a panic reported) in this run: the crashes that
/// follow it (the `abort` after Rust's stack overflow message, a second
/// thread) write nothing.
static WRITTEN: AtomicBool = AtomicBool::new(false);

/// Makes `text` the fixed part the handler writes. Off the handler only.
pub(crate) fn prepare(text: &str) {
    let _one = PREPARING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let idle = 1 - PREPARED.active.load(Ordering::Acquire);
    let bytes = &text.as_bytes()[..text.len().min(HALF)];
    for (slot, &b) in PREPARED.halves[idle].iter().zip(bytes) {
        slot.store(b, Ordering::Relaxed);
    }
    PREPARED.lens[idle].store(bytes.len(), Ordering::Relaxed);
    PREPARED.active.store(idle, Ordering::Release);
}

/// Keeps one printed line for a native crash's report. Called by the
/// threads that copy standard output and error; two of them may run at
/// once, each writes the bytes it reserved.
pub(crate) fn keep_line(line: &[u8]) {
    let line = &line[..line.len().min(OUTPUT_LINE)];
    let start = OUTPUT_END.fetch_add(line.len() + 1, Ordering::AcqRel);
    for (i, &b) in line.iter().chain(b"\n").enumerate() {
        OUTPUT[(start + i) % OUTPUT_BYTES].store(b, Ordering::Relaxed);
    }
}

/// A line of the fixed part: `key: value`, the value on one line.
pub(crate) fn line(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.extend(value.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }));
    out.push('\n');
}

/// The fixed part's first lines: written once per run, before the rest.
pub(crate) fn header_start() -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    out
}

/// Where the handler writes: a file descriptor or a handle.
trait Sink {
    fn put(&self, bytes: &[u8]);
}

/// What the handler knows about the crash.
struct Crash<'a> {
    /// `SIGSEGV`, `EXCEPTION_ACCESS_VIOLATION`.
    name: &'a [u8],
    /// The signal's number in decimal, the exception's code in hex.
    number: Number,
    description: &'a [u8],
    /// For a memory fault, the address it touched.
    address: Option<usize>,
    time_ms: u64,
}

#[derive(Clone, Copy)]
enum Number {
    Dec(u64),
    /// Windows' exception codes.
    #[cfg_attr(not(windows), allow(dead_code))]
    Hex(u64),
}

/// Writes `n` as digits into `buf`, returning them: no formatting machinery.
fn digits(buf: &mut [u8; 24], n: u64, base: u64) -> &[u8] {
    let mut at = buf.len();
    let mut n = n;
    loop {
        at -= 1;
        let d = (n % base) as u8;
        buf[at] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        n /= base;
        if n == 0 {
            break;
        }
    }
    if base == 16 {
        at -= 2;
        buf[at] = b'0';
        buf[at + 1] = b'x';
    }
    &buf[at..]
}

/// The handler's work: the prepared part, the crash, then the kept output.
/// Async-signal-safe: atomic loads, a buffer on the stack, `sink.put`.
fn write_crash(sink: &impl Sink, crash: &Crash<'_>) {
    let mut chunk = [0u8; 256];
    let active = PREPARED.active.load(Ordering::Acquire);
    let len = PREPARED.lens[active].load(Ordering::Relaxed).min(HALF);
    copy_out(
        sink,
        &mut chunk,
        (0..len).map(|i| PREPARED.halves[active][i].load(Ordering::Relaxed)),
    );

    let mut num = [0u8; 24];
    sink.put(b"signal: ");
    sink.put(crash.name);
    sink.put(b"\nsignal-number: ");
    sink.put(match crash.number {
        Number::Dec(n) => digits(&mut num, n, 10),
        Number::Hex(n) => digits(&mut num, n, 16),
    });
    sink.put(b"\ndescription: ");
    sink.put(crash.description);
    if let Some(address) = crash.address {
        sink.put(b"\naddress: ");
        sink.put(digits(&mut num, address as u64, 16));
    }
    sink.put(b"\ntime-ms: ");
    sink.put(digits(&mut num, crash.time_ms, 10));

    let end = OUTPUT_END.load(Ordering::Acquire);
    let kept = end.min(OUTPUT_BYTES);
    let from = end - kept;
    sink.put(b"\noutput-from: ");
    sink.put(digits(&mut num, from as u64, 10));
    sink.put(b"\noutput:\n");
    copy_out(
        sink,
        &mut chunk,
        (from..end).map(|i| OUTPUT[i % OUTPUT_BYTES].load(Ordering::Relaxed)),
    );
}

/// Passes `bytes` to `sink` through `chunk`.
fn copy_out(sink: &impl Sink, chunk: &mut [u8; 256], bytes: impl Iterator<Item = u8>) {
    let mut n = 0;
    for b in bytes {
        chunk[n] = b;
        n += 1;
        if n == chunk.len() {
            sink.put(&chunk[..n]);
            n = 0;
        }
    }
    if n > 0 {
        sink.put(&chunk[..n]);
    }
}

/// A run's pending file, open and locked: the handler writes into it. A
/// run that ends without crashing deletes it.
pub(crate) struct Armed {
    path: PathBuf,
    file: Option<std::fs::File>,
}

impl Drop for Armed {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            sys::disarm(file);
        }
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Opens this run's pending file in `data`'s crash folder and installs the
/// handlers (once per process). `None` where it can't (no folder, a
/// platform without them): the run then keeps only panic reports.
pub(crate) fn arm(data: &Path) -> Option<Armed> {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let folder = data.join(super::FOLDER);
    std::fs::create_dir_all(&folder).ok()?;
    let n = RUNS.fetch_add(1, Ordering::Relaxed);
    let path = folder.join(format!("{PENDING_PREFIX}{}-{n}", std::process::id()));
    let file = sys::open_locked(&path).ok()?;
    WRITTEN.store(false, Ordering::Release);
    sys::arm(&file);
    sys::install();
    Some(Armed { path, file: Some(file) })
}

/// A panic was reported: an `abort` that follows it writes nothing.
pub(crate) fn panic_reported() {
    WRITTEN.store(true, Ordering::Release);
}

/// The pending files that earlier runs left in `data`'s crash folder and no
/// running copy of the game holds: an empty one is deleted (that run ended
/// without crashing, or was killed), a full one is returned with its
/// contents and deleted.
pub(crate) fn collect(data: &Path) -> Vec<Vec<u8>> {
    let folder = data.join(super::FOLDER);
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut names: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(PENDING_PREFIX))
        .map(|e| e.path())
        .collect();
    names.sort();
    let mut left = Vec::new();
    for path in names {
        // Held by a running copy of the game (or by another run in this
        // process): not ours to touch.
        let Some(bytes) = sys::read_if_unheld(&path) else {
            continue;
        };
        let _ = std::fs::remove_file(&path);
        if !bytes.is_empty() {
            left.push(bytes);
        }
    }
    left
}

/// What a pending file says, for the report the next start writes.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Left {
    pub game: String,
    pub game_version: Option<String>,
    pub engine_version: String,
    pub os: String,
    pub started_ms: Option<u64>,
    pub gpu: Option<crate::render::GpuAdapter>,
    pub settings: Vec<(String, String)>,
    pub output_kept: bool,
    pub signal: Option<String>,
    pub signal_number: Option<String>,
    pub description: Option<String>,
    pub address: Option<String>,
    pub time_ms: Option<u64>,
    /// The kept output, whole lines only, oldest first.
    pub output: Vec<String>,
}

impl Left {
    /// Reads a pending file. Never fails: a file cut short (the process
    /// died while writing) gives what it has.
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let (head, output) = match text.split_once("\noutput:\n") {
            Some((head, output)) => (head, Some(output)),
            None => (text.as_ref(), None),
        };
        let mut left = Left::default();
        let mut gpu = crate::render::GpuAdapter::default();
        let mut has_gpu = false;
        let mut output_from = 0u64;
        for line in head.lines() {
            let Some((key, value)) = line.split_once(": ") else {
                continue;
            };
            let value = value.to_string();
            match key {
                "game" => left.game = value,
                "game-version" => left.game_version = Some(value),
                "engine-version" => left.engine_version = value,
                "os" => left.os = value,
                "started-ms" => left.started_ms = value.parse().ok(),
                "gpu-name" => (gpu.name, has_gpu) = (value, true),
                "gpu-backend" => gpu.backend = value,
                "gpu-type" => gpu.device_type = value,
                "gpu-driver" => gpu.driver = value,
                "gpu-driver-info" => gpu.driver_info = value,
                "setting" => {
                    if let Some((name, value)) = value.split_once(" = ") {
                        left.settings.push((name.to_string(), value.to_string()));
                    }
                }
                "output-kept" => left.output_kept = value == "yes",
                "signal" => left.signal = Some(value),
                "signal-number" => left.signal_number = Some(value),
                "description" => left.description = Some(value),
                "address" => left.address = Some(value),
                "time-ms" => left.time_ms = value.parse().ok(),
                "output-from" => output_from = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        if has_gpu {
            left.gpu = Some(gpu);
        }
        if let Some(output) = output {
            let mut lines: Vec<&str> = output.split('\n').collect();
            // The ring wrapped: its first line is the end of a longer one.
            if output_from > 0 && !lines.is_empty() {
                lines.remove(0);
            }
            // After the last newline: a line still being written.
            lines.pop();
            left.output = lines
                .into_iter()
                .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
                .collect();
        }
        left
    }

    /// The report's message: what stopped the game.
    pub(crate) fn message(&self) -> String {
        let Some(signal) = &self.signal else {
            return "the game stopped on a crash that is not a panic (the report was cut short)".to_string();
        };
        let mut message = signal.clone();
        if (signal.starts_with("EXCEPTION") || signal.starts_with("STATUS"))
            && let Some(code) = &self.signal_number
        {
            message.push_str(&format!(" ({code})"));
        }
        if let Some(description) = &self.description {
            message.push_str(": ");
            message.push_str(description);
        }
        if let Some(address) = &self.address {
            message.push_str(" at address ");
            message.push_str(address);
        }
        message
    }
}

/// The time a handler writes: milliseconds since 1970.
#[cfg(unix)]
fn now_ms() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a writable timespec; clock_gettime is async-signal-safe.
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &raw mut ts) };
    (ts.tv_sec as u64) * 1000 + (ts.tv_nsec as u64) / 1_000_000
}

#[cfg(unix)]
mod sys {
    use std::os::fd::{AsRawFd, RawFd};
    use std::path::Path;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicI32, Ordering};

    use super::{Crash, Number, Sink};

    /// The pending file's descriptor while a run is armed, else -1.
    static FD: AtomicI32 = AtomicI32::new(-1);

    const SIGNALS: [libc::c_int; 5] = [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT];

    /// The handlers that were there before ours, in [`SIGNALS`]' order.
    static PREVIOUS: OnceLock<[libc::sigaction; 5]> = OnceLock::new();

    struct Fd(RawFd);

    impl Sink for Fd {
        fn put(&self, mut bytes: &[u8]) {
            while !bytes.is_empty() {
                // SAFETY: `bytes` is readable for its length; write is
                // async-signal-safe.
                let n = unsafe { libc::write(self.0, bytes.as_ptr().cast(), bytes.len()) };
                if n > 0 {
                    bytes = &bytes[n as usize..];
                } else if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                } else {
                    return;
                }
            }
        }
    }

    /// Opens (truncates) `path` and holds an exclusive `flock` on it while
    /// the file is open: the lock goes when the process ends, however.
    pub(super) fn open_locked(path: &Path) -> std::io::Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        // SAFETY: flock on a descriptor this function owns.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(file)
    }

    /// The file's contents, if no open file anywhere holds its lock.
    pub(super) fn read_if_unheld(path: &Path) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut file = std::fs::File::open(path).ok()?;
        // SAFETY: flock on a descriptor this function owns; the lock goes
        // when `file` is closed.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return None;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    pub(super) fn arm(file: &std::fs::File) {
        FD.store(file.as_raw_fd(), Ordering::Release);
    }

    pub(super) fn disarm(file: &std::fs::File) {
        let _ = FD.compare_exchange(file.as_raw_fd(), -1, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Installs the handlers once per process, remembering the ones before.
    pub(super) fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            // SAFETY: sigaction is plain data; all zero is a valid value
            // (SIG_DFL, no flags, an empty mask).
            let mut previous: [libc::sigaction; 5] = unsafe { std::mem::zeroed() };
            for (sig, old) in SIGNALS.iter().zip(previous.iter_mut()) {
                // SAFETY: reads the current action into `old`.
                unsafe { libc::sigaction(*sig, std::ptr::null(), old) };
            }
            let _ = PREVIOUS.set(previous);
            for sig in SIGNALS {
                // SAFETY: as above.
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                // SAFETY: `action.sa_mask` is a writable sigset_t.
                unsafe { libc::sigemptyset(&raw mut action.sa_mask) };
                // SAFETY: installs a handler that only does async-signal-safe work.
                unsafe { libc::sigaction(sig, &raw const action, std::ptr::null_mut()) };
            }
        });
    }

    fn describe(sig: libc::c_int) -> (&'static [u8], &'static [u8]) {
        match sig {
            libc::SIGSEGV => (b"SIGSEGV", b"segmentation fault (an invalid memory access)"),
            libc::SIGBUS => (b"SIGBUS", b"bus error (an invalid memory access)"),
            libc::SIGILL => (b"SIGILL", b"illegal instruction"),
            libc::SIGFPE => (b"SIGFPE", b"arithmetic error (such as an integer division by zero)"),
            libc::SIGABRT => (b"SIGABRT", b"abort (a library or the runtime gave up)"),
            _ => (b"signal", b"a fatal signal"),
        }
    }

    /// The handler: writes the crash once, then hands the signal on to the
    /// handler that was there before, or to the system's default.
    extern "C" fn on_signal(sig: libc::c_int, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
        let fd = FD.load(Ordering::Acquire);
        if fd >= 0 && !super::WRITTEN.swap(true, Ordering::AcqRel) {
            // Let the copy threads pass on what was printed just before.
            if super::OUTPUT_END.load(Ordering::Relaxed) > 0 {
                let pause = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: (super::SETTLE_MS * 1_000_000) as _,
                };
                // SAFETY: nanosleep is async-signal-safe.
                unsafe { libc::nanosleep(&raw const pause, std::ptr::null_mut()) };
            }
            let (name, description) = describe(sig);
            // A fault the kernel raised (si_code > 0) has an address.
            // SAFETY: the kernel passes a valid siginfo_t with SA_SIGINFO.
            let address = (!info.is_null() && sig != libc::SIGABRT && unsafe { (*info).si_code } > 0)
                // SAFETY: as above; si_addr is set for these signals.
                .then(|| unsafe { (*info).si_addr() } as usize);
            let crash = Crash {
                name,
                number: Number::Dec(sig as u64),
                description,
                address,
                time_ms: super::now_ms(),
            };
            super::write_crash(&Fd(fd), &crash);
        }
        hand_on(sig, info, context);
    }

    /// Puts back the action that was there before and runs it, as if this
    /// handler had never been installed.
    fn hand_on(sig: libc::c_int, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
        let index = SIGNALS.iter().position(|&s| s == sig);
        let previous = index.and_then(|i| PREVIOUS.get().map(|p| p[i]));
        // SAFETY: zeroed is SIG_DFL with no flags.
        let previous = previous.unwrap_or_else(|| unsafe { std::mem::zeroed() });
        // SAFETY: restores an action the system gave us.
        unsafe { libc::sigaction(sig, &raw const previous, std::ptr::null_mut()) };
        let handler = previous.sa_sigaction;
        if handler == libc::SIG_DFL || handler == libc::SIG_IGN {
            // The default (an ignored fault would only fault again): it
            // runs when this handler returns, with the signal unblocked.
            // SAFETY: plain signal calls, async-signal-safe.
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
        } else if previous.sa_flags & libc::SA_SIGINFO != 0 {
            // SAFETY: the previous handler was installed with SA_SIGINFO,
            // so it has this signature.
            let f: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                unsafe { std::mem::transmute(handler) };
            f(sig, info, context);
        } else {
            // SAFETY: without SA_SIGINFO a handler takes the signal only.
            let f: extern "C" fn(libc::c_int) = unsafe { std::mem::transmute(handler) };
            f(sig);
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

    use super::{Crash, Number, Sink};

    /// The pending file's handle while a run is armed, else 0.
    static HANDLE: AtomicIsize = AtomicIsize::new(0);
    /// The unhandled exception filter that was there before ours (0: none).
    static PREVIOUS_FILTER: AtomicUsize = AtomicUsize::new(0);
    /// The C runtime's `SIGABRT` handler that was there before ours.
    static PREVIOUS_ABORT: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);

    const FILE_SHARE_READ: u32 = 1;
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC000_0005;
    const EXCEPTION_IN_PAGE_ERROR: u32 = 0xC000_0006;

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        record: *mut ExceptionRecord,
        address: *mut c_void,
        parameters: u32,
        information: [usize; 15],
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    type Filter = unsafe extern "system" fn(*const ExceptionPointers) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetUnhandledExceptionFilter(filter: Option<Filter>) -> Option<Filter>;
        fn WriteFile(file: *mut c_void, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
        fn GetSystemTimeAsFileTime(time: *mut u64);
        fn Sleep(milliseconds: u32);
    }

    struct Handle(*mut c_void);

    impl Sink for Handle {
        fn put(&self, mut bytes: &[u8]) {
            while !bytes.is_empty() {
                let mut written = 0u32;
                let len = bytes.len().min(u32::MAX as usize) as u32;
                // SAFETY: `bytes` is readable for `len` bytes, `written` is
                // writable; WriteFile takes no lock of this process's.
                let ok = unsafe { WriteFile(self.0, bytes.as_ptr(), len, &raw mut written, std::ptr::null_mut()) };
                if ok == 0 || written == 0 {
                    return;
                }
                bytes = &bytes[written as usize..];
            }
        }
    }

    /// Opens (truncates) `path` shared for reading only: no other process
    /// can open it to write or delete while this one has it.
    pub(super) fn open_locked(path: &Path) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
    }

    /// The file's contents, if no process has it open (else the sharing
    /// mode refuses this open).
    pub(super) fn read_if_unheld(path: &Path) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
            .ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    pub(super) fn arm(file: &std::fs::File) {
        HANDLE.store(file.as_raw_handle() as isize, Ordering::Release);
    }

    pub(super) fn disarm(file: &std::fs::File) {
        let _ = HANDLE.compare_exchange(file.as_raw_handle() as isize, 0, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Installs the filter and the `SIGABRT` handler once per process.
    pub(super) fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            // SAFETY: installs a filter that only does the work listed in
            // the module's docs; returns the one before it.
            let previous = unsafe { SetUnhandledExceptionFilter(Some(on_exception)) };
            PREVIOUS_FILTER.store(previous.map_or(0, |f| f as usize), Ordering::Release);
            // SAFETY: installs a C runtime signal handler.
            let previous = unsafe { libc::signal(libc::SIGABRT, on_abort as *const () as libc::sighandler_t) };
            if previous != libc::SIG_ERR as libc::sighandler_t {
                PREVIOUS_ABORT.store(previous, Ordering::Release);
            }
        });
    }

    fn now_ms() -> u64 {
        let mut time = 0u64;
        // SAFETY: `time` is a writable FILETIME-sized value.
        unsafe { GetSystemTimeAsFileTime(&raw mut time) };
        // 100 ns ticks since 1601 to milliseconds since 1970.
        (time / 10_000).saturating_sub(11_644_473_600_000)
    }

    fn describe(code: u32) -> (&'static [u8], &'static [u8]) {
        match code {
            0xC000_0005 => (b"EXCEPTION_ACCESS_VIOLATION", b"an invalid memory access"),
            0xC000_0006 => (b"EXCEPTION_IN_PAGE_ERROR", b"a page of memory could not be read in"),
            0xC000_00FD => (b"EXCEPTION_STACK_OVERFLOW", b"stack overflow"),
            0xC000_001D => (b"EXCEPTION_ILLEGAL_INSTRUCTION", b"illegal instruction"),
            0xC000_0096 => (b"EXCEPTION_PRIV_INSTRUCTION", b"privileged instruction"),
            0xC000_0094 => (b"EXCEPTION_INT_DIVIDE_BY_ZERO", b"integer division by zero"),
            0xC000_008C => (b"EXCEPTION_ARRAY_BOUNDS_EXCEEDED", b"array bounds exceeded"),
            0xC000_0025 => (b"EXCEPTION_NONCONTINUABLE_EXCEPTION", b"a noncontinuable exception"),
            0x8000_0003 => (b"EXCEPTION_BREAKPOINT", b"a breakpoint with no debugger"),
            0xC000_0374 => (b"STATUS_HEAP_CORRUPTION", b"the heap is corrupted"),
            0xE06D_7363 => (b"EXCEPTION_CPP", b"a C++ exception nobody caught"),
            _ => (b"EXCEPTION", b"an unhandled exception"),
        }
    }

    fn write(crash: &Crash<'_>) {
        let handle = HANDLE.load(Ordering::Acquire);
        if handle == 0 || super::WRITTEN.swap(true, Ordering::AcqRel) {
            return;
        }
        if super::OUTPUT_END.load(Ordering::Relaxed) > 0 {
            // SAFETY: Sleep takes no lock of this process's.
            unsafe { Sleep(super::SETTLE_MS as u32) };
        }
        super::write_crash(&Handle(handle as *mut c_void), crash);
    }

    /// The unhandled exception filter: writes the crash once, then asks the
    /// filter before it, or lets Windows go on (its own error reporting).
    unsafe extern "system" fn on_exception(pointers: *const ExceptionPointers) -> i32 {
        // SAFETY: Windows passes valid exception pointers to the filter.
        let record = unsafe { pointers.as_ref().and_then(|p| p.record.as_ref()) };
        if let Some(record) = record {
            let (name, description) = describe(record.code);
            // For an access violation the second parameter is the address.
            let address = (matches!(record.code, EXCEPTION_ACCESS_VIOLATION | EXCEPTION_IN_PAGE_ERROR)
                && record.parameters >= 2)
                .then(|| record.information[1]);
            write(&Crash {
                name,
                number: Number::Hex(u64::from(record.code)),
                description,
                address,
                time_ms: now_ms(),
            });
        }
        match PREVIOUS_FILTER.load(Ordering::Acquire) {
            0 => EXCEPTION_CONTINUE_SEARCH,
            previous => {
                // SAFETY: the value came from SetUnhandledExceptionFilter,
                // a filter function.
                let previous: Filter = unsafe { std::mem::transmute(previous) };
                // SAFETY: called as Windows would have.
                unsafe { previous(pointers) }
            }
        }
    }

    /// The C runtime's `SIGABRT` handler (`abort()` in C code).
    extern "C" fn on_abort(sig: libc::c_int) {
        write(&Crash {
            name: b"SIGABRT",
            number: Number::Dec(sig as u64),
            description: b"abort (a library or the runtime gave up)",
            address: None,
            time_ms: now_ms(),
        });
        let previous = PREVIOUS_ABORT.load(Ordering::Acquire);
        // SAFETY: puts back the C runtime handler that was there before.
        unsafe { libc::signal(sig, previous) };
        if previous != libc::SIG_DFL && previous != libc::SIG_IGN {
            // SAFETY: the value came from `signal`, a handler function.
            let f: extern "C" fn(libc::c_int) = unsafe { std::mem::transmute(previous) };
            f(sig);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    use std::path::Path;

    pub(super) fn open_locked(_path: &Path) -> std::io::Result<std::fs::File> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    pub(super) fn read_if_unheld(_path: &Path) -> Option<Vec<u8>> {
        None
    }

    pub(super) fn arm(_file: &std::fs::File) {}

    pub(super) fn disarm(_file: &std::fs::File) {}

    pub(super) fn install() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bytes(std::cell::RefCell<Vec<u8>>);

    impl Sink for Bytes {
        fn put(&self, bytes: &[u8]) {
            self.0.borrow_mut().extend_from_slice(bytes);
        }
    }

    #[test]
    fn digits_are_written_without_formatting() {
        let mut buf = [0u8; 24];
        assert_eq!(digits(&mut buf, 0, 10), b"0");
        assert_eq!(digits(&mut buf, 1_795_269_787_123, 10), b"1795269787123");
        assert_eq!(digits(&mut buf, 0xc000_0005, 16), b"0xc0000005");
        assert_eq!(digits(&mut buf, u64::MAX, 16), b"0xffffffffffffffff");
        assert_eq!(digits(&mut buf, u64::MAX, 10), b"18446744073709551615");
    }

    #[test]
    fn what_the_handler_writes_reads_back_at_the_next_start() {
        let mut fixed = header_start();
        line(&mut fixed, "game", "Space\nCrab");
        line(&mut fixed, "game-version", "1.2.3");
        line(&mut fixed, "engine-version", "0.9.0");
        line(&mut fixed, "os", "macos 15.1 aarch64");
        line(&mut fixed, "started-ms", "1000");
        line(&mut fixed, "gpu-name", "Test GPU");
        line(&mut fixed, "gpu-backend", "metal");
        line(&mut fixed, "setting", "vsync = on");
        line(&mut fixed, "output-kept", "yes");
        prepare(&fixed);
        keep_line(b"the crab's last words");
        let sink = Bytes(Default::default());
        write_crash(
            &sink,
            &Crash {
                name: b"SIGSEGV",
                number: Number::Dec(11),
                description: b"segmentation fault (an invalid memory access)",
                address: Some(0x10),
                time_ms: 3500,
            },
        );
        let left = Left::parse(&sink.0.borrow());
        assert_eq!(left.game, "Space Crab", "a value stays on its line");
        assert_eq!(left.game_version.as_deref(), Some("1.2.3"));
        assert_eq!(left.engine_version, "0.9.0");
        assert_eq!(left.started_ms, Some(1000));
        assert_eq!(left.time_ms, Some(3500));
        assert_eq!(left.gpu.as_ref().map(|g| g.backend.as_str()), Some("metal"));
        assert_eq!(left.settings, [("vsync".to_string(), "on".to_string())]);
        assert!(left.output_kept);
        assert_eq!(
            left.message(),
            "SIGSEGV: segmentation fault (an invalid memory access) at address 0x10"
        );
        // Other tests in this process print too: the last line kept is ours.
        assert_eq!(left.output.last().map(String::as_str), Some("the crab's last words"));
    }

    #[test]
    fn a_file_cut_short_still_reads() {
        let left = Left::parse(b"ponko native crash\ngame: t\nsignal: SIGABRT\nsignal-num");
        assert_eq!(left.signal.as_deref(), Some("SIGABRT"));
        assert!(left.output.is_empty());
        assert!(Left::parse(b"").message().contains("cut short"));
        let windows = Left::parse(
            b"signal: EXCEPTION_ACCESS_VIOLATION\nsignal-number: 0xc0000005\ndescription: an invalid memory access\n",
        );
        assert_eq!(
            windows.message(),
            "EXCEPTION_ACCESS_VIOLATION (0xc0000005): an invalid memory access"
        );
    }

    #[test]
    fn a_wrapped_ring_drops_its_cut_first_line_and_the_unfinished_last() {
        let left = Left::parse(b"output-from: 7\noutput:\nof a long line\nwhole\nalso whole\nunfinish");
        assert_eq!(left.output, ["whole", "also whole"]);
        let left = Left::parse(b"output-from: 0\noutput:\nfirst\nsecond\n");
        assert_eq!(left.output, ["first", "second"]);
    }
}
