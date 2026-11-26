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
//! - [`MODULES`]: each loaded module's address range, build id and path,
//!   refreshed off the handler ([`refresh_modules`]), so the handler can
//!   write each frame's module and offset.
//!
//! The handler also writes the crash's backtrace as raw addresses: on Unix
//! a walk of the frame records from the crashed thread's registers, with
//! every read bounds-checked and made through a pipe (an unmapped address
//! fails the `write` with `EFAULT` instead of faulting again); on Windows
//! the system's table-driven unwinder. The next start names the frames
//! whose binary is still the same ([`describe_frames`]).

// The handler's statics are shared by the Unix and Windows paths; a
// platform with neither only keeps the parsing (the next start's half).
#![cfg_attr(not(any(unix, windows)), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};

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

/// The most frames the handler writes.
const MAX_FRAMES: usize = 64;

/// The most modules [`MODULES`] holds.
const MAX_MODULES: usize = 1024;

/// The bytes of module text (`<id> <path>`) each half of [`MODULES`] holds.
const MODULE_TEXT: usize = 64 * 1024;

/// A loaded module (the program or a shared library): its address range,
/// its build id (`-` when it has none) and its path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Module {
    pub start: usize,
    pub end: usize,
    pub id: String,
    pub path: String,
}

/// One half of [`ModuleTable`]: the modules as atomic values.
struct ModuleHalf {
    count: AtomicUsize,
    start: [AtomicUsize; MAX_MODULES],
    end: [AtomicUsize; MAX_MODULES],
    text_at: [AtomicU32; MAX_MODULES],
    text_len: [AtomicU32; MAX_MODULES],
    text: [AtomicU8; MODULE_TEXT],
}

impl ModuleHalf {
    const fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            start: [const { AtomicUsize::new(0) }; MAX_MODULES],
            end: [const { AtomicUsize::new(0) }; MAX_MODULES],
            text_at: [const { AtomicU32::new(0) }; MAX_MODULES],
            text_len: [const { AtomicU32::new(0) }; MAX_MODULES],
            text: [const { AtomicU8::new(0) }; MODULE_TEXT],
        }
    }
}

/// The loaded modules, for the handler: two halves, filled off the handler
/// into the idle one before switching, like [`PREPARED`].
struct ModuleTable {
    halves: [ModuleHalf; 2],
    active: AtomicUsize,
}

impl ModuleTable {
    const fn new() -> Self {
        Self {
            halves: [ModuleHalf::new(), ModuleHalf::new()],
            active: AtomicUsize::new(0),
        }
    }

    /// Makes `modules` the table the handler reads. One caller at a time.
    fn publish(&self, modules: &[Module]) {
        let idle = 1 - self.active.load(Ordering::Acquire);
        let half = &self.halves[idle];
        let mut at = 0usize;
        let count = modules.len().min(MAX_MODULES);
        for (i, module) in modules.iter().take(count).enumerate() {
            half.start[i].store(module.start, Ordering::Relaxed);
            half.end[i].store(module.end, Ordering::Relaxed);
            let id = if module.id.is_empty() { "-" } else { &module.id };
            let text = format!("{id} {}", module.path.replace(['\n', '\r'], " "));
            // A table out of room keeps the range, without a name.
            let len = if at + text.len() <= MODULE_TEXT { text.len() } else { 0 };
            for (slot, &b) in half.text[at..at + len].iter().zip(text.as_bytes()) {
                slot.store(b, Ordering::Relaxed);
            }
            half.text_at[i].store(at as u32, Ordering::Relaxed);
            half.text_len[i].store(len as u32, Ordering::Relaxed);
            at += len;
        }
        half.count.store(count, Ordering::Relaxed);
        self.active.store(idle, Ordering::Release);
    }
}

static MODULES: ModuleTable = ModuleTable::new();

/// The loaded modules' signature (their count) at the last refresh.
static MODULES_SEEN: Mutex<Option<u64>> = Mutex::new(None);

/// Refreshes [`MODULES`] if a module was loaded or unloaded since the last
/// time (or always, with `force`). Off the handler only.
pub(crate) fn refresh_modules(force: bool) {
    let mut seen = MODULES_SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let signature = sys::module_signature();
    if !force && *seen == Some(signature) {
        return;
    }
    *seen = Some(signature);
    MODULES.publish(&sys::modules());
}

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
    /// The crashing instruction, then the return addresses of its callers.
    frames: &'a [usize],
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

/// The handler's work: the prepared part, the crash, its frames with their
/// modules (from `modules`), then the kept output.
/// Async-signal-safe: atomic loads, buffers on the stack, `sink.put`.
fn write_crash(sink: &impl Sink, crash: &Crash<'_>, modules: &ModuleTable) {
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
    write_frames(sink, crash.frames, modules);

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

/// Writes one `frame:` line per address (`frame: <address> <module>
/// <offset>`, or the address alone outside every module), then one
/// `module: <index> <start> <id> <path>` line per module they used.
/// Async-signal-safe: atomic loads and buffers on the stack.
fn write_frames(sink: &impl Sink, frames: &[usize], modules: &ModuleTable) {
    let half = &modules.halves[modules.active.load(Ordering::Acquire)];
    let count = half.count.load(Ordering::Relaxed).min(MAX_MODULES);
    let mut used = [0u64; MAX_MODULES / 64];
    let mut num = [0u8; 24];
    for &address in frames.iter().take(MAX_FRAMES) {
        sink.put(b"\nframe: ");
        sink.put(digits(&mut num, address as u64, 16));
        let module = (0..count).find(|&i| {
            half.start[i].load(Ordering::Relaxed) <= address && address < half.end[i].load(Ordering::Relaxed)
        });
        if let Some(i) = module {
            sink.put(b" ");
            sink.put(digits(&mut num, i as u64, 10));
            sink.put(b" ");
            let start = half.start[i].load(Ordering::Relaxed);
            sink.put(digits(&mut num, address.wrapping_sub(start) as u64, 16));
            used[i / 64] |= 1 << (i % 64);
        }
    }
    let mut chunk = [0u8; 256];
    for i in (0..count).filter(|&i| used[i / 64] & (1 << (i % 64)) != 0) {
        sink.put(b"\nmodule: ");
        sink.put(digits(&mut num, i as u64, 10));
        sink.put(b" ");
        sink.put(digits(&mut num, half.start[i].load(Ordering::Relaxed) as u64, 16));
        sink.put(b" ");
        let at = (half.text_at[i].load(Ordering::Relaxed) as usize).min(MODULE_TEXT);
        let len = (half.text_len[i].load(Ordering::Relaxed) as usize).min(MODULE_TEXT - at);
        if len == 0 {
            sink.put(b"-");
        }
        copy_out(
            sink,
            &mut chunk,
            (at..at + len).map(|j| half.text[j].load(Ordering::Relaxed)),
        );
    }
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
        // Closed (and its lock let go) before the file is deleted.
        self.file = None;
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
    sys::install();
    sys::register_this_thread();
    refresh_modules(true);
    sys::arm(&file);
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
    /// The frames the handler walked, the crashing instruction first.
    pub frames: Vec<LeftFrame>,
    /// The modules the frames fell into.
    pub modules: Vec<LeftModule>,
}

/// A frame of a pending file: its address in the crashed run and, when it
/// fell into a module, the module's index and its offset there.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct LeftFrame {
    pub address: usize,
    pub module: Option<(usize, usize)>,
}

/// A module of a pending file.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LeftModule {
    pub index: usize,
    pub start: usize,
    pub id: String,
    pub path: String,
}

/// A number the handler wrote in hex (`0x1f`).
fn hex(text: &str) -> Option<usize> {
    usize::from_str_radix(text.strip_prefix("0x")?, 16).ok()
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
                "frame" => {
                    let mut parts = value.split(' ');
                    if let Some(address) = parts.next().and_then(hex) {
                        let index = parts.next().and_then(|i| i.parse().ok());
                        let offset = parts.next().and_then(hex);
                        left.frames.push(LeftFrame {
                            address,
                            module: index.zip(offset),
                        });
                    }
                }
                "module" => {
                    let mut parts = value.splitn(4, ' ');
                    let index = parts.next().and_then(|i| i.parse().ok());
                    let start = parts.next().and_then(hex);
                    if let (Some(index), Some(start)) = (index, start) {
                        left.modules.push(LeftModule {
                            index,
                            start,
                            id: parts.next().unwrap_or("-").to_string(),
                            path: parts.next().unwrap_or("").to_string(),
                        });
                    }
                }
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

/// A name the next start found for an address: a function, and where it is
/// when the binary has debug info. Inlined functions give several.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Symbol {
    pub name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// The report's backtrace for a native crash, named in this process: the
/// modules loaded now and the debug info they have.
pub(crate) fn describe_frames(left: &Left) -> String {
    if left.frames.is_empty() {
        return frames_text(left, &[], &mut |_| Vec::new());
    }
    frames_text(left, &sys::modules(), &mut resolve_here)
}

/// Names the instruction at `address` (in this process) from the debug
/// info, or from the exported symbols when there is none.
#[cfg(any(unix, windows))]
fn resolve_here(address: usize) -> Vec<Symbol> {
    let mut found = Vec::new();
    // `resolve` takes a return address and looks one byte back itself. On
    // Windows it looks back twice for an address (backtrace 0.3.77's dbghelp
    // path subtracts one in `address_or_ip` and again in
    // `resolve_with_inline`), so a function's first byte was looked up in
    // the padding before it and named the nearest preceding symbol.
    let back = if cfg!(windows) { 2 } else { 1 };
    backtrace::resolve(address.wrapping_add(back) as *mut std::ffi::c_void, |symbol| {
        let name = symbol.name().map(|n| format!("{n:#}")).unwrap_or_default();
        let file = symbol.filename().map(|f| f.display().to_string());
        if !name.is_empty() || file.is_some() {
            found.push(Symbol {
                name: if name.is_empty() { "?".to_string() } else { name },
                file,
                line: symbol.lineno(),
                column: symbol.colno(),
            });
        }
    });
    if found.is_empty()
        && let Some(name) = sys::exported_name(address)
    {
        found.push(Symbol {
            name,
            file: None,
            line: None,
            column: None,
        });
    }
    found
}

#[cfg(not(any(unix, windows)))]
fn resolve_here(_address: usize) -> Vec<Symbol> {
    Vec::new()
}

/// The backtrace's text: a frame is named (`resolve` with its address in
/// this process) only when its module is loaded in `current` under the same
/// path with the same build id; otherwise it keeps its module and offset.
fn frames_text(left: &Left, current: &[Module], resolve: &mut dyn FnMut(usize) -> Vec<Symbol>) -> String {
    use std::fmt::Write as _;
    if left.frames.is_empty() {
        return "no frames: the crash handler could not read the crashed thread's registers on this system".to_string();
    }
    let mut t = String::new();
    for (n, frame) in left.frames.iter().enumerate() {
        let module = frame
            .module
            .and_then(|(index, offset)| Some((left.modules.iter().find(|m| m.index == index)?, offset)));
        let Some((module, offset)) = module else {
            let _ = writeln!(t, "{n:>4}: {:#x} (in no module the run knew)", frame.address);
            continue;
        };
        let place = format!(
            "{} +{offset:#x}",
            if module.path.is_empty() { "?" } else { &module.path }
        );
        let here = current
            .iter()
            .find(|c| module.id != "-" && c.id == module.id && c.path == module.path);
        let symbols = match here {
            // A return address points after its call: look up the call.
            Some(here) => resolve((here.start + offset).saturating_sub(usize::from(n > 0))),
            None => Vec::new(),
        };
        if symbols.is_empty() {
            let why = if here.is_some() {
                "no symbols"
            } else {
                "not loaded now, or not the same file"
            };
            let _ = writeln!(t, "{n:>4}: {place} ({why})");
            continue;
        }
        for (i, symbol) in symbols.iter().enumerate() {
            if i == 0 {
                let _ = writeln!(t, "{n:>4}: {}", symbol.name);
            } else {
                let _ = writeln!(t, "      {}", symbol.name);
            }
            if let Some(file) = &symbol.file {
                let _ = match (symbol.line, symbol.column) {
                    (Some(line), Some(column)) => writeln!(t, "             at {file}:{line}:{column}"),
                    (Some(line), None) => writeln!(t, "             at {file}:{line}"),
                    _ => writeln!(t, "             at {file}"),
                };
            }
        }
        let _ = writeln!(t, "             in {place}");
    }
    if left.frames.len() == 1 && !cfg!(windows) {
        t.push_str("(only the crashing frame: this build keeps no frame pointers, see the guide's \"Backtraces of a native crash\")\n");
    }
    t.truncate(t.trim_end().len());
    t
}

/// A build id as hex digits.
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "linux", target_os = "android")),
    allow(dead_code)
)]
fn hex_id(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// For a module without a build id: its file's size and modification time,
/// or `-` (never the same) when the file can't be read.
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "linux", target_os = "android", windows)),
    allow(dead_code)
)]
fn file_id(path: &str) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return "-".to_string();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("file-{:x}-{modified:x}", meta.len())
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
    use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

    use super::{Crash, MAX_FRAMES, Module, Number, Sink};

    /// The pipe the frame walk reads memory through: `[read end, write end]`.
    static PROBE: [AtomicI32; 2] = [AtomicI32::new(-1), AtomicI32::new(-1)];

    /// How many thread stacks are recorded ahead of time.
    const STACK_SLOTS: usize = 8;

    /// Thread stacks recorded ahead of time: `[low, high)` each.
    static STACKS: [[AtomicUsize; 2]; STACK_SLOTS] = [const { [const { AtomicUsize::new(0) }; 2] }; STACK_SLOTS];
    static STACK_NEXT: AtomicUsize = AtomicUsize::new(0);

    /// For a thread whose stack wasn't recorded, how far above the stack
    /// pointer the walk may look.
    const UNKNOWN_STACK: usize = 8 << 20;

    /// Opens the pipe the walk reads through, once per process.
    pub(super) fn open_probe() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(open_probe_now);
    }

    fn open_probe_now() {
        let mut fds = [-1; 2];
        // SAFETY: `fds` has room for the two descriptors pipe writes.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return;
        }
        for fd in fds {
            // SAFETY: fcntl on descriptors this function just opened.
            unsafe {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK);
            }
        }
        PROBE[1].store(fds[1], Ordering::Release);
        PROBE[0].store(fds[0], Ordering::Release);
    }

    /// Reads the two words at `address`, or `None` where nothing readable
    /// is mapped: `write` from there into the pipe fails with `EFAULT`
    /// instead of faulting. Async-signal-safe.
    pub(super) fn read_pair(address: usize) -> Option<[usize; 2]> {
        let (r, w) = (PROBE[0].load(Ordering::Acquire), PROBE[1].load(Ordering::Acquire));
        if r < 0 || w < 0 {
            return None;
        }
        let mut words = [0usize; 2];
        let size = std::mem::size_of_val(&words);
        // SAFETY: write only reads `address` in the kernel, which checks it.
        let wrote = unsafe { libc::write(w, address as *const libc::c_void, size) };
        if wrote <= 0 {
            return None;
        }
        // SAFETY: `words` is writable for `size` bytes, at least `wrote`.
        let read = unsafe { libc::read(r, words.as_mut_ptr().cast(), wrote as usize) };
        (wrote as usize == size && read == wrote).then_some(words)
    }

    /// The address without a pointer authentication code in its high bits.
    fn strip(address: usize) -> usize {
        if cfg!(all(target_arch = "aarch64", target_vendor = "apple")) {
            address & ((1 << 47) - 1)
        } else if cfg!(target_arch = "aarch64") {
            address & ((1 << 48) - 1)
        } else {
            address
        }
    }

    /// Walks the frame records from `fp`: each holds the caller's frame
    /// pointer, then the return address. A record must lie in
    /// `[low, high)`, above the stack pointer and the record before, and be
    /// aligned; reads go through [`read_pair`]. Returns how many frames
    /// went into `frames`, the crashing instruction `pc` first.
    /// Async-signal-safe.
    pub(super) fn walk(
        pc: usize,
        fp: usize,
        sp: usize,
        (low, high): (usize, usize),
        frames: &mut [usize; MAX_FRAMES],
    ) -> usize {
        const WORD: usize = std::mem::size_of::<usize>();
        frames[0] = strip(pc);
        let mut n = 1;
        let mut floor = sp.max(low);
        let mut fp = fp;
        while n < MAX_FRAMES {
            let inside =
                fp >= floor && fp.is_multiple_of(WORD) && fp.checked_add(2 * WORD).is_some_and(|end| end <= high);
            if !inside {
                break;
            }
            let Some([next, ret]) = read_pair(fp) else {
                break;
            };
            let ret = strip(ret);
            if ret == 0 {
                break;
            }
            frames[n] = ret;
            n += 1;
            floor = fp + 2 * WORD;
            fp = next;
        }
        n
    }

    /// The stack the walk may read for a thread whose stack pointer is
    /// `sp`: a recorded one that holds it, else the 8 MiB above it.
    /// Async-signal-safe.
    pub(super) fn stack_of(sp: usize) -> (usize, usize) {
        STACKS
            .iter()
            .map(|slot| (slot[0].load(Ordering::Acquire), slot[1].load(Ordering::Acquire)))
            .find(|&(low, high)| low <= sp && sp < high)
            .unwrap_or((sp, sp.saturating_add(UNKNOWN_STACK)))
    }

    /// Records the calling thread's stack for the walk. Off the handler.
    pub(super) fn register_this_thread() {
        let Some((low, high)) = this_thread_stack() else {
            return;
        };
        let known = STACKS
            .iter()
            .any(|slot| slot[0].load(Ordering::Acquire) == low && slot[1].load(Ordering::Acquire) == high);
        if !known {
            let slot = &STACKS[STACK_NEXT.fetch_add(1, Ordering::Relaxed) % STACK_SLOTS];
            slot[0].store(low, Ordering::Release);
            slot[1].store(high, Ordering::Release);
        }
    }

    #[cfg(target_os = "linux")]
    fn this_thread_stack() -> Option<(usize, usize)> {
        // SAFETY: pthread_attr_t is plain data, filled by pthread_getattr_np
        // and destroyed after use; the out-pointers are writable locals.
        unsafe {
            let mut attr: libc::pthread_attr_t = std::mem::zeroed();
            if libc::pthread_getattr_np(libc::pthread_self(), &raw mut attr) != 0 {
                return None;
            }
            let mut base = std::ptr::null_mut();
            let mut size = 0;
            let ok = libc::pthread_attr_getstack(&raw const attr, &raw mut base, &raw mut size) == 0;
            libc::pthread_attr_destroy(&raw mut attr);
            ok.then(|| (base as usize, base as usize + size))
        }
    }

    #[cfg(target_vendor = "apple")]
    fn this_thread_stack() -> Option<(usize, usize)> {
        // SAFETY: both read the calling thread's own pthread record.
        let (high, size) = unsafe {
            let me = libc::pthread_self();
            (
                libc::pthread_get_stackaddr_np(me) as usize,
                libc::pthread_get_stacksize_np(me),
            )
        };
        (high > size).then(|| (high - size, high))
    }

    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
    fn this_thread_stack() -> Option<(usize, usize)> {
        None
    }

    /// The crashed thread's instruction, frame and stack pointers, from
    /// the context the system hands a `SA_SIGINFO` handler.
    #[cfg(all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"))]
    fn registers(context: *mut libc::c_void) -> Option<[usize; 3]> {
        // SAFETY: the kernel passes a valid ucontext_t to the handler.
        let uc = unsafe { context.cast::<libc::ucontext_t>().as_ref()? };
        let g = &uc.uc_mcontext.gregs;
        Some([
            g[libc::REG_RIP as usize] as usize,
            g[libc::REG_RBP as usize] as usize,
            g[libc::REG_RSP as usize] as usize,
        ])
    }

    #[cfg(all(target_os = "linux", target_env = "gnu", target_arch = "aarch64"))]
    fn registers(context: *mut libc::c_void) -> Option<[usize; 3]> {
        // SAFETY: the kernel passes a valid ucontext_t to the handler.
        let uc = unsafe { context.cast::<libc::ucontext_t>().as_ref()? };
        let m = &uc.uc_mcontext;
        Some([m.pc as usize, m.regs[29] as usize, m.sp as usize])
    }

    #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
    fn registers(context: *mut libc::c_void) -> Option<[usize; 3]> {
        // SAFETY: the kernel passes a valid ucontext_t whose machine
        // context it filled.
        let ss = unsafe { &context.cast::<libc::ucontext_t>().as_ref()?.uc_mcontext.as_ref()?.__ss };
        Some([ss.__pc as usize, ss.__fp as usize, ss.__sp as usize])
    }

    #[cfg(all(target_vendor = "apple", target_arch = "x86_64"))]
    fn registers(context: *mut libc::c_void) -> Option<[usize; 3]> {
        // SAFETY: as above.
        let ss = unsafe { &context.cast::<libc::ucontext_t>().as_ref()?.uc_mcontext.as_ref()?.__ss };
        Some([ss.__rip as usize, ss.__rbp as usize, ss.__rsp as usize])
    }

    #[cfg(not(any(
        all(
            target_os = "linux",
            target_env = "gnu",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(target_vendor = "apple", any(target_arch = "x86_64", target_arch = "aarch64")),
    )))]
    fn registers(_context: *mut libc::c_void) -> Option<[usize; 3]> {
        None
    }

    // dyld's image list (libSystem); the libc crate's copies are deprecated.
    #[cfg(target_vendor = "apple")]
    unsafe extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_header(index: u32) -> *const u8;
        fn _dyld_get_image_name(index: u32) -> *const libc::c_char;
    }

    /// The loaded modules' signature: it changes when one is loaded.
    #[cfg(target_vendor = "apple")]
    pub(super) fn module_signature() -> u64 {
        // SAFETY: reads dyld's image count.
        u64::from(unsafe { _dyld_image_count() })
    }

    /// The loaded modules: dyld's images, each from its Mach-O header in
    /// memory (`__TEXT`'s size for its range, `LC_UUID` for its id).
    #[cfg(target_vendor = "apple")]
    pub(super) fn modules() -> Vec<Module> {
        const MH_MAGIC_64: u32 = 0xfeed_facf;
        const LC_SEGMENT_64: u32 = 0x19;
        const LC_UUID: u32 = 0x1b;
        let mut modules = Vec::new();
        // SAFETY: reads dyld's image count.
        let count = unsafe { _dyld_image_count() };
        for i in 0..count {
            // SAFETY: dyld returns the header and name of a loaded image (or
            // null); a loaded image's header and load commands are mapped
            // and readable, `sizeofcmds` long after the 32-byte header.
            unsafe {
                let header = _dyld_get_image_header(i);
                let name = _dyld_get_image_name(i);
                if header.is_null() || name.is_null() || header.cast::<u32>().read_unaligned() != MH_MAGIC_64 {
                    continue;
                }
                let ncmds = header.add(16).cast::<u32>().read_unaligned();
                let sizeofcmds = header.add(20).cast::<u32>().read_unaligned() as usize;
                let (mut text_size, mut uuid) = (0usize, None);
                let mut at = 32usize;
                for _ in 0..ncmds {
                    if at + 8 > 32 + sizeofcmds {
                        break;
                    }
                    let cmd = header.add(at).cast::<u32>().read_unaligned();
                    let size = header.add(at + 4).cast::<u32>().read_unaligned() as usize;
                    if cmd == LC_UUID && size >= 24 {
                        uuid = Some(std::slice::from_raw_parts(header.add(at + 8), 16).to_vec());
                    } else if cmd == LC_SEGMENT_64 && size >= 48 {
                        let segname = std::slice::from_raw_parts(header.add(at + 8), 16);
                        if segname.starts_with(b"__TEXT\0") {
                            text_size = header.add(at + 32).cast::<u64>().read_unaligned() as usize;
                        }
                    }
                    if size < 8 {
                        break;
                    }
                    at += size;
                }
                let path = std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned();
                let id = match uuid {
                    Some(uuid) => super::hex_id(&uuid),
                    None => super::file_id(&path),
                };
                modules.push(Module {
                    start: header as usize,
                    end: header as usize + text_size,
                    id,
                    path,
                });
            }
        }
        modules
    }

    /// The loaded modules' signature: how many were loaded and unloaded.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) fn module_signature() -> u64 {
        unsafe extern "C" fn first(
            info: *mut libc::dl_phdr_info,
            _size: libc::size_t,
            data: *mut libc::c_void,
        ) -> libc::c_int {
            // SAFETY: dl_iterate_phdr passes a valid info and our `data`.
            unsafe {
                let info = &*info;
                *data.cast::<u64>() = (info.dlpi_adds << 32) ^ info.dlpi_subs;
            }
            1
        }
        let mut signature = 0u64;
        // SAFETY: the callback only reads `info` and writes `signature`.
        unsafe { libc::dl_iterate_phdr(Some(first), (&raw mut signature).cast()) };
        signature
    }

    /// The loaded modules: `dl_iterate_phdr`'s objects, each from its
    /// program headers (its `PT_LOAD` segments' range, its GNU build id).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) fn modules() -> Vec<Module> {
        unsafe extern "C" fn each(
            info: *mut libc::dl_phdr_info,
            _size: libc::size_t,
            data: *mut libc::c_void,
        ) -> libc::c_int {
            // SAFETY: dl_iterate_phdr passes a valid info (its headers and
            // segments are mapped) and our `data`.
            unsafe {
                let info = &*info;
                let found = &mut *data.cast::<Vec<(usize, usize, Option<Vec<u8>>, String)>>();
                if info.dlpi_phdr.is_null() {
                    return 0;
                }
                let base = info.dlpi_addr as usize;
                let headers = std::slice::from_raw_parts(info.dlpi_phdr, usize::from(info.dlpi_phnum));
                let (mut start, mut end, mut build_id) = (usize::MAX, 0usize, None);
                for h in headers {
                    let at = base.wrapping_add(h.p_vaddr as usize);
                    if h.p_type == libc::PT_LOAD {
                        start = start.min(at);
                        end = end.max(at + h.p_memsz as usize);
                    } else if h.p_type == libc::PT_NOTE && build_id.is_none() {
                        build_id = gnu_build_id(std::slice::from_raw_parts(at as *const u8, h.p_memsz as usize));
                    }
                }
                let name = if info.dlpi_name.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(info.dlpi_name).to_string_lossy().into_owned()
                };
                if start < end {
                    found.push((start, end, build_id, name));
                }
            }
            0
        }
        let mut found: Vec<(usize, usize, Option<Vec<u8>>, String)> = Vec::new();
        // SAFETY: the callback only reads `info` and pushes to `found`.
        unsafe { libc::dl_iterate_phdr(Some(each), (&raw mut found).cast()) };
        let program = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        found
            .into_iter()
            .map(|(start, end, build_id, name)| {
                // The program itself has no name in the list.
                let path = if name.is_empty() { program.clone() } else { name };
                let id = match build_id {
                    Some(id) => super::hex_id(&id),
                    None => super::file_id(&path),
                };
                Module { start, end, id, path }
            })
            .collect()
    }

    /// The GNU build id in an ELF note segment.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn gnu_build_id(mut notes: &[u8]) -> Option<Vec<u8>> {
        const NT_GNU_BUILD_ID: u32 = 3;
        let word = |b: &[u8], at: usize| {
            b.get(at..at + 4)
                .map(|w| u32::from_ne_bytes([w[0], w[1], w[2], w[3]]) as usize)
        };
        let up4 = |n: usize| n.div_ceil(4) * 4;
        while notes.len() >= 12 {
            let (name_size, desc_size, kind) = (word(notes, 0)?, word(notes, 4)?, word(notes, 8)?);
            let name = notes.get(12..12 + name_size)?;
            let desc_at = 12 + up4(name_size);
            let desc = notes.get(desc_at..desc_at + desc_size)?;
            if kind as u32 == NT_GNU_BUILD_ID && name == b"GNU\0" {
                return Some(desc.to_vec());
            }
            notes = notes.get(desc_at + up4(desc_size)..)?;
        }
        None
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
    pub(super) fn module_signature() -> u64 {
        0
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
    pub(super) fn modules() -> Vec<Module> {
        Vec::new()
    }

    /// The exported symbol at or before `address` and how far past it the
    /// address is (`name+0x40`), for a library without debug info: the
    /// nearest export, which a private function inside it is not.
    pub(super) fn exported_name(address: usize) -> Option<String> {
        // SAFETY: Dl_info is plain data; dladdr fills it for an address in
        // a loaded module and leaves its strings owned by the loader.
        unsafe {
            let mut info: libc::Dl_info = std::mem::zeroed();
            if libc::dladdr(address as *const libc::c_void, &raw mut info) == 0 || info.dli_sname.is_null() {
                return None;
            }
            let name = std::ffi::CStr::from_ptr(info.dli_sname).to_bytes();
            let name = format!("{:#}", backtrace::SymbolName::new(name));
            match address.checked_sub(info.dli_saddr as usize) {
                Some(0) | None => Some(name),
                Some(past) => Some(format!("{name}+{past:#x}")),
            }
        }
    }

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
            open_probe();
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
            let mut frames = [0usize; MAX_FRAMES];
            let walked = match registers(context) {
                Some([pc, fp, sp]) => walk(pc, fp, sp, stack_of(sp), &mut frames),
                None => 0,
            };
            let crash = Crash {
                name,
                number: Number::Dec(sig as u64),
                description,
                address,
                time_ms: super::now_ms(),
                frames: &frames[..walked],
            };
            super::write_crash(&Fd(fd), &crash, &super::MODULES);
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

    use super::{Crash, MAX_FRAMES, Module, Number, Sink};

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
        fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: *mut *mut c_void, hash: *mut u32) -> u16;
        fn GetCurrentProcess() -> *mut c_void;
        fn K32EnumProcessModules(process: *mut c_void, modules: *mut *mut c_void, bytes: u32, needed: *mut u32) -> i32;
        fn K32GetModuleInformation(process: *mut c_void, module: *mut c_void, info: *mut ModuleInfo, bytes: u32)
        -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }

    #[repr(C)]
    struct ModuleInfo {
        base: *mut c_void,
        size: u32,
        entry: *mut c_void,
    }

    /// The return addresses of the calling thread's frames, from the
    /// system's table-driven unwinder (64-bit Windows code keeps no
    /// frame-pointer chain); `from` drops the frames above that address
    /// (the exception's dispatch), when it is among them.
    fn capture(frames: &mut [usize; MAX_FRAMES], from: Option<usize>) -> usize {
        let mut raw = [std::ptr::null_mut::<c_void>(); MAX_FRAMES];
        // SAFETY: `raw` has room for MAX_FRAMES pointers; the unwinder
        // reads unwind tables already in memory and allocates nothing.
        let n = usize::from(unsafe {
            RtlCaptureStackBackTrace(0, MAX_FRAMES as u32, raw.as_mut_ptr(), std::ptr::null_mut())
        });
        let raw = &raw[..n.min(MAX_FRAMES)];
        let skip = from
            .and_then(|at| raw.iter().position(|&f| f as usize == at))
            .unwrap_or(0);
        let mut count = 0;
        for &f in &raw[skip..] {
            frames[count] = f as usize;
            count += 1;
        }
        count
    }

    /// The loaded modules' signature: how many there are.
    pub(super) fn module_signature() -> u64 {
        let mut needed = 0u32;
        // SAFETY: asks only for the size the list needs.
        unsafe { K32EnumProcessModules(GetCurrentProcess(), std::ptr::null_mut(), 0, &raw mut needed) };
        u64::from(needed)
    }

    /// The process's modules, each with its range and the PE header's
    /// time stamp and image size as its id.
    pub(super) fn modules() -> Vec<Module> {
        // SAFETY: the pseudo-handle of this process.
        let process = unsafe { GetCurrentProcess() };
        let mut handles = vec![std::ptr::null_mut::<c_void>(); super::MAX_MODULES];
        let mut needed = 0u32;
        let bytes = (handles.len() * std::mem::size_of::<*mut c_void>()) as u32;
        // SAFETY: `handles` is writable for `bytes`.
        if unsafe { K32EnumProcessModules(process, handles.as_mut_ptr(), bytes, &raw mut needed) } == 0 {
            return Vec::new();
        }
        handles.truncate((needed as usize / std::mem::size_of::<*mut c_void>()).min(super::MAX_MODULES));
        let mut modules = Vec::new();
        for module in handles {
            let mut info = ModuleInfo {
                base: std::ptr::null_mut(),
                size: 0,
                entry: std::ptr::null_mut(),
            };
            // SAFETY: `info` is a writable MODULEINFO.
            let ok = unsafe {
                K32GetModuleInformation(process, module, &raw mut info, std::mem::size_of::<ModuleInfo>() as u32)
            };
            if ok == 0 || info.base.is_null() {
                continue;
            }
            let mut name = [0u16; 1024];
            // SAFETY: `name` is writable for its length in u16s.
            let len = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) } as usize;
            let path = String::from_utf16_lossy(&name[..len.min(name.len())]);
            let start = info.base as usize;
            let size = info.size as usize;
            let id = pe_id(start, size).unwrap_or_else(|| super::file_id(&path));
            modules.push(Module {
                start,
                end: start + size,
                id,
                path,
            });
        }
        modules
    }

    /// A loaded image's id: its PE header's time stamp and image size, as
    /// symbol servers name them.
    fn pe_id(base: usize, size: usize) -> Option<String> {
        if size < 0x40 {
            return None;
        }
        // SAFETY: a loaded image's headers are mapped; every offset is
        // checked against the image's size first.
        unsafe {
            let pe = (base as *const u8).add(0x3c).cast::<u32>().read_unaligned() as usize;
            if pe + 24 + 60 > size || (base as *const u8).add(pe).cast::<u32>().read_unaligned() != 0x0000_4550 {
                return None;
            }
            let stamp = (base as *const u8).add(pe + 8).cast::<u32>().read_unaligned();
            let image = (base as *const u8).add(pe + 24 + 56).cast::<u32>().read_unaligned();
            Some(format!("{stamp:08X}{image:x}"))
        }
    }

    /// Debug info names a frame on Windows; there is nothing to fall back on.
    pub(super) fn exported_name(_address: usize) -> Option<String> {
        None
    }

    /// The walk needs no recorded stack on Windows.
    pub(super) fn register_this_thread() {}

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

    /// Writes the crash once, with the frames from `from` (the faulting
    /// instruction) down, or every frame of this thread.
    fn write(crash: Crash<'_>, from: Option<usize>) {
        let handle = HANDLE.load(Ordering::Acquire);
        if handle == 0 || super::WRITTEN.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut frames = [0usize; MAX_FRAMES];
        let n = capture(&mut frames, from);
        if super::OUTPUT_END.load(Ordering::Relaxed) > 0 {
            // SAFETY: Sleep takes no lock of this process's.
            unsafe { Sleep(super::SETTLE_MS as u32) };
        }
        let crash = Crash {
            frames: &frames[..n],
            ..crash
        };
        super::write_crash(&Handle(handle as *mut c_void), &crash, &super::MODULES);
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
            write(
                Crash {
                    name,
                    number: Number::Hex(u64::from(record.code)),
                    description,
                    address,
                    time_ms: now_ms(),
                    frames: &[],
                },
                Some(record.address as usize),
            );
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
        write(
            Crash {
                name: b"SIGABRT",
                number: Number::Dec(sig as u64),
                description: b"abort (a library or the runtime gave up)",
                address: None,
                time_ms: now_ms(),
                frames: &[],
            },
            None,
        );
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

    pub(super) fn register_this_thread() {}

    pub(super) fn module_signature() -> u64 {
        0
    }

    pub(super) fn modules() -> Vec<super::Module> {
        Vec::new()
    }
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
                frames: &[],
            },
            &ModuleTable::new(),
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

    fn module(start: usize, end: usize, id: &str, path: &str) -> Module {
        Module {
            start,
            end,
            id: id.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn the_handler_writes_each_frames_module_and_offset_and_the_next_start_reads_them() {
        let table = Box::new(ModuleTable::new());
        table.publish(&[
            module(0x1000, 0x2000, "aa11", "/games/space crab/crab"),
            module(0x3000, 0x4000, "bb22", "/usr/lib/libunused.so"),
            module(0x5000, 0x6000, "", "/usr/lib/libfoo.so"),
        ]);
        let sink = Bytes(Default::default());
        write_crash(
            &sink,
            &Crash {
                name: b"SIGSEGV",
                number: Number::Dec(11),
                description: b"segmentation fault",
                address: None,
                time_ms: 1,
                frames: &[0x1010, 0x5008, 0x9999, 0x1fff],
            },
            &table,
        );
        let left = Left::parse(&sink.0.borrow());
        let frame = |address, module| LeftFrame { address, module };
        assert_eq!(
            left.frames,
            [
                frame(0x1010, Some((0, 0x10))),
                frame(0x5008, Some((2, 0x8))),
                frame(0x9999, None),
                frame(0x1fff, Some((0, 0xfff))),
            ]
        );
        let modules: Vec<_> = left
            .modules
            .iter()
            .map(|m| (m.index, m.start, m.id.as_str(), m.path.as_str()))
            .collect();
        assert_eq!(
            modules,
            [
                (0, 0x1000, "aa11", "/games/space crab/crab"),
                (2, 0x5000, "-", "/usr/lib/libfoo.so"),
            ],
            "only the modules the frames used, a path with spaces whole, no id as -"
        );
        assert_eq!(left.signal.as_deref(), Some("SIGSEGV"), "the lines before still read");
    }

    #[test]
    fn a_frame_is_named_only_where_the_same_binary_is_loaded_now() {
        let mut left = Left::parse(
            b"frame: 0x1010 0 0x10\nframe: 0x5008 1 0x8\nframe: 0x7000 2 0x0\nframe: 0x42\n\
              module: 0 0x1000 aa11 /games/crab\nmodule: 1 0x5000 bb22 /usr/lib/libfoo.so\nmodule: 2 0x7000 - /usr/lib/libnoid.so\n",
        );
        // This run loaded the same game elsewhere, another libfoo, and the
        // library without an id.
        let now = [
            module(0x9_0000, 0x9_1000, "aa11", "/games/crab"),
            module(0xa_0000, 0xa_1000, "cc33", "/usr/lib/libfoo.so"),
            module(0xb_0000, 0xb_1000, "-", "/usr/lib/libnoid.so"),
        ];
        let mut asked = Vec::new();
        let text = frames_text(&left, &now, &mut |address| {
            asked.push(address);
            vec![Symbol {
                name: "crab::walk".to_string(),
                file: Some("src/walk.rs".to_string()),
                line: Some(12),
                column: Some(5),
            }]
        });
        assert_eq!(
            asked,
            [0x9_0010],
            "the crashing frame's address, moved to where the game is now"
        );
        assert!(
            text.starts_with("   0: crab::walk\n             at src/walk.rs:12:5\n             in /games/crab +0x10\n"),
            "{text}"
        );
        assert!(
            text.contains("   1: /usr/lib/libfoo.so +0x8 (not loaded now, or not the same file)"),
            "{text}"
        );
        assert!(
            text.contains("   2: /usr/lib/libnoid.so +0x0 (not loaded now"),
            "no id is never the same: {text}"
        );
        assert!(text.contains("   3: 0x42 (in no module the run knew)"), "{text}");

        // A return address is looked up one byte back, inside its call.
        left.frames.swap(0, 1);
        let mut asked = Vec::new();
        let _ = frames_text(&left, &now, &mut |address| {
            asked.push(address);
            Vec::new()
        });
        assert_eq!(asked, [0x9_000f]);

        let one = Left::parse(b"frame: 0x42\n");
        if !cfg!(windows) {
            assert!(frames_text(&one, &[], &mut |_| Vec::new()).contains("no frame pointers"));
        }
        assert!(frames_text(&Left::default(), &[], &mut |_| Vec::new()).starts_with("no frames"));
    }

    #[cfg(any(unix, windows))]
    #[inline(never)]
    fn a_function_to_find() -> usize {
        std::hint::black_box(7)
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn this_program_is_among_the_loaded_modules_with_a_stable_id_and_its_names() {
        let here = a_function_to_find as *const () as usize;
        let modules = sys::modules();
        let program = modules
            .iter()
            .find(|m| m.start <= here && here < m.end)
            .unwrap_or_else(|| panic!("no module holds {here:#x}: {modules:?}"));
        assert_ne!(program.id, "-", "{program:?}");
        assert!(!program.path.is_empty());
        assert!(sys::module_signature() > 0);
        let again = sys::modules();
        assert!(again.contains(program), "the same id each time");
        let names = resolve_here(here);
        assert!(names.iter().any(|s| s.name.contains("a_function_to_find")), "{names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_library_without_debug_info_is_named_by_its_exported_symbols() {
        let getpid = libc::getpid as *const () as usize;
        let name = sys::exported_name(getpid);
        assert!(name.as_deref().is_some_and(|n| n.ends_with("getpid")), "{name:?}");
        let name = sys::exported_name(getpid + 2);
        assert!(name.as_deref().is_some_and(|n| n.ends_with("getpid+0x2")), "{name:?}");
        assert_eq!(sys::exported_name(16), None, "no module there");
    }

    #[cfg(unix)]
    #[test]
    fn the_walk_follows_frame_records_up_the_stack_and_stops_at_a_broken_one() {
        sys::open_probe();
        let mut stack = vec![0usize; 64];
        let at = |i: usize, stack: &Vec<usize>| stack.as_ptr() as usize + i * std::mem::size_of::<usize>();
        let (four, ten, twenty, two) = (at(4, &stack), at(10, &stack), at(20, &stack), at(2, &stack));
        stack[4] = ten;
        stack[5] = 0x1111;
        stack[10] = twenty;
        stack[11] = 0x2222;
        // Back down the stack: a broken chain.
        stack[20] = two;
        stack[21] = 0x3333;
        let bounds = (at(0, &stack), at(64, &stack));
        let mut frames = [0usize; MAX_FRAMES];
        let n = sys::walk(0xaaaa, four, at(0, &stack), bounds, &mut frames);
        assert_eq!(frames[..n], [0xaaaa, 0x1111, 0x2222, 0x3333]);

        // The stack's top: a record that doesn't fit below it isn't read.
        let n = sys::walk(0xaaaa, four, at(0, &stack), (bounds.0, at(12, &stack)), &mut frames);
        assert_eq!(frames[..n], [0xaaaa, 0x1111, 0x2222]);

        // Below the stack pointer, misaligned, or a zero return address.
        let n = sys::walk(0xaaaa, four, at(5, &stack), bounds, &mut frames);
        assert_eq!(frames[..n], [0xaaaa]);
        let n = sys::walk(0xaaaa, four + 1, at(0, &stack), bounds, &mut frames);
        assert_eq!(frames[..n], [0xaaaa]);
        stack[11] = 0;
        let n = sys::walk(0xaaaa, four, at(0, &stack), bounds, &mut frames);
        assert_eq!(frames[..n], [0xaaaa, 0x1111]);
    }

    #[cfg(any(target_os = "linux", target_vendor = "apple"))]
    #[test]
    fn a_recorded_threads_stack_bounds_the_walk_on_that_thread() {
        let local = std::hint::black_box(0u64);
        let sp = &raw const local as usize;
        let unknown = (sp, sp + 8 * 1024 * 1024);
        sys::register_this_thread();
        let (low, high) = sys::stack_of(sp);
        assert!(low <= sp && sp < high, "{low:#x}..{high:#x} holds {sp:#x}");
        assert_ne!((low, high), unknown, "the recorded range, not the guess");
    }

    #[cfg(unix)]
    #[test]
    fn reading_an_unmapped_address_through_the_pipe_fails_without_faulting() {
        sys::open_probe();
        let words = [0x1234usize, 0x5678];
        assert_eq!(sys::read_pair(words.as_ptr() as usize), Some(words));
        // SAFETY: sysconf only reads a value.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        // SAFETY: maps two pages, readable, then takes the second's access
        // away; this test only touches the first.
        let pages = unsafe {
            let pages = libc::mmap(
                std::ptr::null_mut(),
                2 * page_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            assert_ne!(pages, libc::MAP_FAILED);
            assert_eq!(
                libc::mprotect(pages.cast::<u8>().add(page_size).cast(), page_size, libc::PROT_NONE),
                0
            );
            pages
        };
        let first = pages as usize;
        let second = first + page_size;
        assert_eq!(sys::read_pair(second), None, "no access");
        assert_eq!(sys::read_pair(16), None, "nothing mapped");
        // Half readable: the first word ends the readable page.
        assert_eq!(sys::read_pair(second - std::mem::size_of::<usize>()), None);
        assert_eq!(
            sys::read_pair(words.as_ptr() as usize),
            Some(words),
            "the pipe was left empty"
        );
        assert_eq!(sys::read_pair(first), Some([0, 0]));
        // SAFETY: unmaps the pages mapped above.
        unsafe { libc::munmap(pages, 2 * page_size) };
    }

    #[test]
    fn a_wrapped_ring_drops_its_cut_first_line_and_the_unfinished_last() {
        let left = Left::parse(b"output-from: 7\noutput:\nof a long line\nwhole\nalso whole\nunfinish");
        assert_eq!(left.output, ["whole", "also whole"]);
        let left = Left::parse(b"output-from: 0\noutput:\nfirst\nsecond\n");
        assert_eq!(left.output, ["first", "second"]);
    }
}
