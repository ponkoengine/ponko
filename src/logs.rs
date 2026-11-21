//! Keeps the game's recent stdout and stderr, so tools connected through the
//! control endpoint can read them (`logs.get`, MCP `read_logs`), also for
//! games started from a terminal or an IDE rather than by the tool.
//!
//! While a control endpoint is open, standard output and error point at
//! pipes (file descriptors 1 and 2 on Unix; the standard handles and the C
//! runtime's descriptors on Windows). A thread per stream copies everything
//! to where it went before (the terminal still shows it all) and keeps the
//! last [`MAX_LINES`] lines. That covers Rust's `println!`, C and C++
//! `printf`/`std::cout`, and panic messages.

// Only filled by a capture; on other platforms the ring stays empty.
#![cfg_attr(not(any(unix, windows)), allow(dead_code))]

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::{Value, json};

/// Lines kept, oldest dropped first.
pub(crate) const MAX_LINES: usize = 5_000;
/// Longer lines are cut, so one huge line can't fill the memory.
const MAX_LINE_BYTES: usize = 4_096;

/// The kept lines. `next` numbers lines from the start of the capture, so a
/// reader can ask for what's new since its last read.
struct Ring {
    lines: VecDeque<String>,
    next: u64,
}

static RING: Mutex<Ring> = Mutex::new(Ring {
    lines: VecDeque::new(),
    next: 0,
});

fn push_line(bytes: &[u8]) {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let cut = &bytes[..bytes.len().min(MAX_LINE_BYTES)];
    let line = String::from_utf8_lossy(cut).into_owned();
    let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
    if ring.lines.len() == MAX_LINES {
        ring.lines.pop_front();
    }
    ring.lines.push_back(line);
    ring.next += 1;
}

/// The `logs.get` answer. Without `since`: the newest `max` lines. With it:
/// up to `max` lines from line number `since` on, oldest first. `next` is the
/// `since` for the following read; `missed` counts lines dropped (the ring
/// is full) before they could be read.
pub(crate) fn get(since: Option<u64>, max: usize) -> Result<Value, String> {
    if !capturing() {
        return Err(if cfg!(any(unix, windows)) {
            "this game's output isn't captured (the pipes couldn't be set up); check the terminal it runs in".into()
        } else {
            "game output isn't captured on this platform yet; check the terminal the game runs in".into()
        });
    }
    let ring = RING.lock().unwrap_or_else(|e| e.into_inner());
    let first = ring.next - ring.lines.len() as u64;
    let (from, missed) = match since {
        None => (ring.next.saturating_sub(max as u64).max(first), 0),
        Some(s) => (s.clamp(first, ring.next), first.saturating_sub(s)),
    };
    let lines: Vec<&str> = ring
        .lines
        .iter()
        .skip((from - first) as usize)
        .take(max)
        .map(String::as_str)
        .collect();
    let next = from + lines.len() as u64;
    Ok(json!({ "lines": lines, "next": next, "missed": missed }))
}

/// The last `max` lines kept, oldest first, for a crash report: `None`
/// when output isn't captured. Never waits long for the lock (a copy
/// thread holds it only to add a line).
pub(crate) fn last_lines(max: usize) -> Option<Vec<String>> {
    if !capturing() {
        return None;
    }
    // Give the copy threads a moment to pass on what was just printed.
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    std::thread::sleep(std::time::Duration::from_millis(20));
    for _ in 0..50 {
        match RING.try_lock() {
            Ok(ring) => return Some(newest(&ring, max)),
            Err(std::sync::TryLockError::Poisoned(ring)) => return Some(newest(&ring.into_inner(), max)),
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(std::time::Duration::from_millis(1)),
        }
    }
    None
}

fn newest(ring: &Ring, max: usize) -> Vec<String> {
    let skip = ring.lines.len().saturating_sub(max);
    ring.lines.iter().skip(skip).cloned().collect()
}

#[cfg(any(unix, windows))]
pub(crate) use capture::{Capture, capturing};

#[cfg(not(any(unix, windows)))]
pub(crate) struct Capture;

#[cfg(not(any(unix, windows)))]
impl Capture {
    pub(crate) fn start() -> Option<Self> {
        None
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn capturing() -> bool {
    false
}

#[cfg(any(unix, windows))]
mod capture {
    use std::fs::File;
    #[cfg(unix)]
    use std::io::Read;
    use std::io::{PipeReader, Write};
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::Duration;
    #[cfg(windows)]
    use std::{
        cell::UnsafeCell,
        sync::Arc,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[cfg(unix)]
    use super::unix as sys;
    #[cfg(windows)]
    use super::windows as sys;

    /// How long a restore waits for the copy threads to pass on the last
    /// output. Longer only if a child process still holds the pipe.
    const DRAIN_WAIT: Duration = Duration::from_millis(500);

    /// Standard output or standard error.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum Std {
        Out,
        Err,
    }

    /// Size of the copy thread's buffer.
    const COPY_BUF: usize = 8192;

    /// Windows: the copy thread writes to the real stream in pieces this big,
    /// so that after being killed at most one piece is in doubt.
    #[cfg(windows)]
    const WRITE_PIECE: usize = 512;

    /// Windows: what the copy thread has taken from the pipe but not yet
    /// written to the real stream. The thread keeps it here, in memory that
    /// outlives it, because `ExitProcess` (what `std::process::exit` calls)
    /// kills the thread wherever it is: the exit hook then passes this on,
    /// and the rest of the pipe, itself.
    ///
    /// The kernel itself records how much each read took (`status`), before
    /// the read returns: a thread killed right as its read returns, before
    /// it ran another instruction, still leaves that count behind.
    #[cfg(windows)]
    struct InFlight {
        buf: UnsafeCell<[u8; COPY_BUF]>,
        /// The last read's result; its byte count is the part of `buf` that
        /// holds output not yet fully passed on (0: none).
        status: UnsafeCell<sys::IoStatus>,
        /// How many of those were already written.
        written: AtomicUsize,
    }

    // SAFETY: `buf` and `status` are only written by the copy thread (and
    // the kernel on its behalf), and only read by others (the exit hook)
    // once that thread is dead, or by the thread itself.
    #[cfg(windows)]
    unsafe impl Sync for InFlight {}

    #[cfg(windows)]
    impl InFlight {
        fn new() -> Self {
            Self {
                buf: UnsafeCell::new([0; COPY_BUF]),
                status: UnsafeCell::new(sys::IoStatus::default()),
                written: AtomicUsize::new(0),
            }
        }

        /// Copy thread: reads what's in the pipe into the buffer. 0: the
        /// pipe was closed (or failed).
        fn read_from(&self, reader: &PipeReader) -> usize {
            self.written.store(0, Ordering::Release);
            // SAFETY: only the copy thread calls this, with nothing in
            // flight (the count is 0), so nobody reads `buf` or `status`.
            unsafe { sys::read_recorded(reader, self.buf.get().cast(), COPY_BUF, self.status.get()) }
        }

        /// Copy thread: the first `n` bytes read.
        fn chunk(&self, n: usize) -> &[u8] {
            assert!(n <= COPY_BUF);
            // SAFETY: only the copy thread calls this, between two reads;
            // `n` is within the buffer.
            unsafe { std::slice::from_raw_parts(self.buf.get().cast::<u8>(), n) }
        }

        /// Copy thread: writes the `n` bytes just read to `out`, keeping
        /// track of how far it got.
        fn pass_on(&self, n: usize, out: &mut File) {
            let data = self.chunk(n);
            let mut at = 0;
            while at < n {
                let end = (at + WRITE_PIECE).min(n);
                if out.write_all(&data[at..end]).is_err() {
                    break;
                }
                at = end;
                self.written.store(at, Ordering::Release);
            }
            // SAFETY: only the copy thread writes `status` (outside reads).
            unsafe { sys::IoStatus::clear(self.status.get()) };
        }

        /// What the copy thread took but didn't write. Only call this when
        /// the thread is dead.
        fn left(&self) -> &[u8] {
            // SAFETY: the copy thread is dead, so nothing writes `status`.
            let len = unsafe { sys::IoStatus::bytes(self.status.get()) }.min(COPY_BUF);
            let written = self.written.load(Ordering::Acquire);
            if written < len {
                // SAFETY: the copy thread is dead, so nothing writes `buf`;
                // `written < len <= COPY_BUF`.
                unsafe { std::slice::from_raw_parts(self.buf.get().cast::<u8>().add(written), len - written) }
            } else {
                &[]
            }
        }
    }

    /// One redirected stream: how to put it back, and the thread copying.
    struct Stream {
        which: Std,
        saved: sys::Saved,
        done: Receiver<()>,
        /// Windows: the copy thread, to tell whether it was killed.
        #[cfg(windows)]
        copier: thread::JoinHandle<()>,
        /// Windows: what the copy thread holds, for the exit hook.
        #[cfg(windows)]
        in_flight: Arc<InFlight>,
        /// Windows: the pipe and the real stream again, for the exit hook,
        /// which passes on what's left itself (see `register_exit_hook`).
        #[cfg(windows)]
        at_exit: Option<(PipeReader, File)>,
    }

    /// The process-wide capture, shared by every open endpoint: started by
    /// the first, undone when the last closes (or the process exits).
    struct State {
        users: usize,
        streams: Vec<Stream>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        users: 0,
        streams: Vec::new(),
    });

    fn state() -> std::sync::MutexGuard<'static, State> {
        STATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn capturing() -> bool {
        !state().streams.is_empty()
    }

    /// Keeps the capture on while alive.
    pub(crate) struct Capture(());

    impl Capture {
        /// Starts capturing (or joins the running capture). `None` if the
        /// pipes couldn't be set up; output then just isn't kept.
        pub(crate) fn start() -> Option<Self> {
            let mut s = state();
            if s.users == 0 {
                let mut streams = Vec::new();
                for which in [Std::Out, Std::Err] {
                    match redirect(which) {
                        Ok(stream) => streams.push(stream),
                        Err(_) => {
                            undo(&mut streams, true);
                            return None;
                        }
                    }
                }
                s.streams = streams;
                register_exit_hook();
            }
            // Whichever thread runs the game may be the one that exits.
            #[cfg(windows)]
            sys::watch_this_thread(on_thread_end);
            s.users += 1;
            Some(Capture(()))
        }
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            let mut s = state();
            s.users -= 1;
            if s.users == 0 {
                undo(&mut s.streams, true);
            }
        }
    }

    /// Points `which` at a new pipe and starts the thread that copies from
    /// it to where the stream went before.
    fn redirect(which: Std) -> std::io::Result<Stream> {
        flush(which);
        let (saved, reader, out) = sys::redirect(which)?;
        #[cfg(windows)]
        let at_exit = reader.try_clone().ok().zip(out.try_clone().ok());
        #[cfg(windows)]
        let in_flight = Arc::new(InFlight::new());
        let (done, _copier) = spawn_copier(
            which,
            reader,
            out,
            #[cfg(windows)]
            in_flight.clone(),
        )?;
        Ok(Stream {
            which,
            saved,
            done,
            #[cfg(windows)]
            copier: _copier,
            #[cfg(windows)]
            in_flight,
            #[cfg(windows)]
            at_exit,
        })
    }

    /// Copies everything from `reader` to `out` and keeps it line by line,
    /// until the pipe's write end is closed.
    fn spawn_copier(
        which: Std,
        #[cfg_attr(windows, allow(unused_mut))] mut reader: PipeReader,
        mut out: File,
        #[cfg(windows)] in_flight: Arc<InFlight>,
    ) -> std::io::Result<(Receiver<()>, thread::JoinHandle<()>)> {
        let (done_tx, done) = mpsc::channel();
        let name = if which == Std::Out {
            "ponko-logs-out"
        } else {
            "ponko-logs-err"
        };
        let copier = thread::Builder::new().name(name.into()).spawn(move || {
            #[cfg(unix)]
            let mut buf = [0u8; COPY_BUF];
            let mut partial = Vec::new();
            loop {
                #[cfg(unix)]
                let n = match reader.read(&mut buf) {
                    Ok(n) if n > 0 => n,
                    _ => break,
                };
                #[cfg(windows)]
                let n = match in_flight.read_from(&reader) {
                    0 => break,
                    n => n,
                };
                #[cfg(unix)]
                let _ = out.write_all(&buf[..n]);
                #[cfg(windows)]
                in_flight.pass_on(n, &mut out);
                #[cfg(unix)]
                partial.extend_from_slice(&buf[..n]);
                #[cfg(windows)]
                partial.extend_from_slice(in_flight.chunk(n));
                let mut start = 0;
                while let Some(i) = partial[start..].iter().position(|&b| b == b'\n') {
                    super::push_line(&partial[start..start + i]);
                    start += i + 1;
                }
                partial.drain(..start);
                if partial.len() > super::MAX_LINE_BYTES {
                    super::push_line(&partial);
                    partial.clear();
                }
            }
            if !partial.is_empty() {
                super::push_line(&partial);
            }
            let _ = done_tx.send(());
        })?;
        Ok((done, copier))
    }

    /// Puts every stream back where it was and lets the threads finish
    /// passing on what's left.
    fn undo(streams: &mut Vec<Stream>, flush_std: bool) {
        for s in streams.iter_mut() {
            if flush_std {
                flush(s.which);
            }
            sys::restore(s.which, &mut s.saved);
        }
        for s in streams.drain(..) {
            let _ = s.done.recv_timeout(DRAIN_WAIT);
        }
    }

    fn flush(which: Std) {
        if which == Std::Out {
            let _ = std::io::stdout().flush();
        } else {
            let _ = std::io::stderr().flush();
        }
    }

    /// Restores the streams when the process ends without dropping the
    /// capture (`std::process::exit` skips destructors), so the last lines a
    /// game prints still reach the terminal.
    ///
    /// Two ways out, two hooks:
    /// - Returning from `main`, or C's `exit`: `atexit` handlers run, with
    ///   the copy threads still alive. They finish passing on the pipe.
    /// - Windows, `std::process::exit`: Rust calls `ExitProcess`, which runs
    ///   no `atexit` handlers, and ends every other thread (the copy
    ///   threads) before it runs the exiting thread's fiber-local storage
    ///   callbacks. That callback (see `sys::watch_this_thread`) passes on
    ///   what's left itself: what the dead copy thread held, then the pipe,
    ///   reading only what's in it (never waiting).
    fn register_exit_hook() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        extern "C" fn restore_at_exit() {
            finish_at_exit(false);
        }
        // SAFETY: registers a plain function with no captured state.
        ONCE.call_once(|| unsafe {
            libc::atexit(restore_at_exit);
        });
    }

    /// Windows: runs when a thread that called `Capture::start` ends, which
    /// includes it calling `ExitProcess`.
    #[cfg(windows)]
    unsafe extern "system" fn on_thread_end(_data: *const std::ffi::c_void) {
        finish_at_exit(true);
    }

    /// The exit hooks' work. With `only_if_killed`, it does nothing unless
    /// the copy threads are gone: then the thread ending is just a thread,
    /// not the process.
    fn finish_at_exit(only_if_killed: bool) {
        // try_lock: never wait at exit (a thread may hold it forever).
        // No flushing either: another thread may hold Rust's stdout lock.
        let Ok(mut s) = STATE.try_lock() else {
            return;
        };
        #[cfg(windows)]
        {
            let killed = !s.streams.is_empty() && s.streams.iter().all(|stream| sys::is_dead(&stream.copier));
            if killed {
                for stream in &mut s.streams {
                    sys::restore(stream.which, &mut stream.saved);
                    if let Some((reader, out)) = &mut stream.at_exit {
                        let _ = out.write_all(stream.in_flight.left());
                        sys::drain(reader, out);
                    }
                }
                s.streams.clear();
                return;
            }
        }
        if !only_if_killed {
            undo(&mut s.streams, false);
        }
    }
}

/// Unix: file descriptors 1 and 2 point at the pipes (`dup2`), which covers
/// Rust, C and C++ output alike.
#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io::PipeReader;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

    use super::capture::Std;

    /// The descriptor the stream used before.
    pub(super) struct Saved(OwnedFd);

    fn fd(which: Std) -> RawFd {
        if which == Std::Out { 1 } else { 2 }
    }

    /// Returns what to restore, the pipe's read end, and where the output went.
    pub(super) fn redirect(which: Std) -> std::io::Result<(Saved, PipeReader, File)> {
        let fd = fd(which);
        // SAFETY: dup of a valid (or invalid, then -1) descriptor number.
        let copy = unsafe { libc::dup(fd) };
        if copy < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `copy` is a fresh descriptor that nothing else owns.
        let original = unsafe { OwnedFd::from_raw_fd(copy) };
        let (reader, writer) = std::io::pipe()?;
        let out = File::from(original.try_clone()?);
        // SAFETY: both descriptors are valid; fd now refers to the pipe.
        if unsafe { libc::dup2(writer.as_raw_fd(), fd) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // `writer` closes here: `fd` is the pipe's only write end, so the
        // copy thread sees the end of the stream once `fd` is restored.
        Ok((Saved(original), reader, out))
    }

    pub(super) fn restore(which: Std, saved: &mut Saved) {
        // SAFETY: valid descriptors; closes this fd's pipe write end.
        unsafe { libc::dup2(saved.0.as_raw_fd(), fd(which)) };
    }
}

/// Windows: Rust writes to the process's standard handles and C and C++
/// to the C runtime's descriptors 1 and 2, so both point at the pipes.
#[cfg(windows)]
mod windows {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::fs::File;
    use std::io::{PipeReader, PipeWriter};
    use std::os::windows::io::{AsRawHandle, BorrowedHandle, IntoRawHandle};
    use std::sync::OnceLock;
    use std::thread::JoinHandle;

    use super::capture::Std;

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(id: u32) -> *mut c_void;
        fn SetStdHandle(id: u32, handle: *mut c_void) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn FlsAlloc(callback: Option<unsafe extern "system" fn(*const c_void)>) -> u32;
        fn FlsSetValue(index: u32, data: *const c_void) -> i32;
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtReadFile(
            file: *mut c_void,
            event: *mut c_void,
            apc: *mut c_void,
            apc_context: *mut c_void,
            status: *mut IoStatus,
            buffer: *mut c_void,
            length: u32,
            offset: *const i64,
            key: *const u32,
        ) -> i32;
    }

    const STATUS_PENDING: i32 = 0x103;
    const INFINITE: u32 = 0xFFFF_FFFF;

    /// An `IO_STATUS_BLOCK`: a read's result, filled in by the kernel.
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct IoStatus {
        /// The `NTSTATUS` (a pointer-sized union in C).
        status: isize,
        /// Bytes read.
        information: usize,
    }

    impl IoStatus {
        /// Bytes the last read took (0: none, or already passed on).
        ///
        /// # Safety
        /// `this` is valid, and nobody writes it meanwhile.
        pub(super) unsafe fn bytes(this: *const Self) -> usize {
            // SAFETY: per the caller; volatile, as the kernel wrote it.
            unsafe { std::ptr::addr_of!((*this).information).read_volatile() }
        }

        /// Marks the last read as passed on.
        ///
        /// # Safety
        /// `this` is valid, and nobody reads or writes it meanwhile.
        pub(super) unsafe fn clear(this: *mut Self) {
            // SAFETY: per the caller.
            unsafe { this.write_volatile(Self::default()) };
        }
    }

    /// Reads from the pipe into `buf`, blocking until something is there.
    /// The kernel records the byte count in `status` before the call
    /// returns, so it is there even if this thread is killed right after
    /// (unlike `Read::read`, whose count only exists in its return value).
    /// Returns that count; 0: the pipe was closed, or the read failed.
    ///
    /// # Safety
    /// `buf` is writable for `len` bytes and `status` is valid; nobody else
    /// touches either until this returns.
    pub(super) unsafe fn read_recorded(reader: &PipeReader, buf: *mut u8, len: usize, status: *mut IoStatus) -> usize {
        // SAFETY: per the caller; a live, synchronous pipe handle.
        unsafe {
            IoStatus::clear(status);
            let handle = reader.as_raw_handle();
            let null = std::ptr::null_mut();
            let mut result = NtReadFile(
                handle,
                null,
                null,
                null,
                status,
                buf.cast(),
                len.min(u32::MAX as usize) as u32,
                std::ptr::null(),
                std::ptr::null(),
            );
            if result == STATUS_PENDING {
                // Not for a pipe from `std::io::pipe`, which is synchronous.
                WaitForSingleObject(handle, INFINITE);
                result = std::ptr::addr_of!((*status).status).read_volatile() as i32;
            }
            if result >= 0 {
                IoStatus::bytes(status).min(len)
            } else {
                // End of file, a broken pipe, or an error: nothing taken.
                IoStatus::clear(status);
                0
            }
        }
    }

    const FLS_OUT_OF_INDEXES: u32 = 0xFFFF_FFFF;
    const WAIT_OBJECT_0: u32 = 0;

    /// Whether the thread has ended: it ran out, or the process killed it.
    pub(super) fn is_dead(thread: &JoinHandle<()>) -> bool {
        // SAFETY: a live thread handle, owned by the `JoinHandle`.
        unsafe { WaitForSingleObject(thread.as_raw_handle(), 0) == WAIT_OBJECT_0 }
    }

    /// Has `callback` run when the calling thread ends, which includes
    /// the thread calling `ExitProcess` (what `std::process::exit` does),
    /// after the process's other threads were killed. `atexit` handlers
    /// don't run on that path. Does nothing if called again on this thread.
    pub(super) fn watch_this_thread(callback: unsafe extern "system" fn(*const c_void)) {
        thread_local! {
            static WATCHED: Cell<bool> = const { Cell::new(false) };
        }
        static KEY: OnceLock<u32> = OnceLock::new();
        if WATCHED.get() {
            return;
        }
        // SAFETY: allocates a fiber-local storage slot, never freed.
        let key = *KEY.get_or_init(|| unsafe { FlsAlloc(Some(callback)) });
        if key == FLS_OUT_OF_INDEXES {
            return;
        }
        // The callback runs for a slot holding a non-null value.
        // SAFETY: `key` is a valid slot; the value is never dereferenced.
        if unsafe { FlsSetValue(key, std::ptr::without_provenance(1)) } != 0 {
            WATCHED.set(true);
        }
    }

    /// Copies what's in the pipe now to `out`, without waiting for more.
    pub(super) fn drain(reader: &mut PipeReader, out: &mut File) {
        use std::io::{Read, Write};
        let mut buf = [0u8; 8192];
        loop {
            let mut available = 0u32;
            let null = std::ptr::null_mut();
            // SAFETY: a live pipe handle; only the count is written.
            let ok = unsafe {
                PeekNamedPipe(
                    reader.as_raw_handle(),
                    null,
                    0,
                    null.cast(),
                    &mut available,
                    null.cast(),
                )
            };
            if ok == 0 || available == 0 {
                return;
            }
            let want = (available as usize).min(buf.len());
            match reader.read(&mut buf[..want]) {
                Ok(n) if n > 0 => {
                    let _ = out.write_all(&buf[..n]);
                }
                _ => return,
            }
        }
    }

    /// Where the stream went before, and the pipe end the standard handle
    /// now refers to (kept open until the restore; dropping it ends the copy).
    pub(super) struct Saved {
        handle: *mut c_void,
        /// The C runtime's descriptor before, if it had one open.
        crt: Option<i32>,
        writer: Option<PipeWriter>,
    }

    // SAFETY: a process-wide handle value, only passed back to SetStdHandle.
    unsafe impl Send for Saved {}

    fn ids(which: Std) -> (u32, i32) {
        if which == Std::Out {
            (STD_OUTPUT_HANDLE, 1)
        } else {
            (STD_ERROR_HANDLE, 2)
        }
    }

    pub(super) fn redirect(which: Std) -> std::io::Result<(Saved, PipeReader, File)> {
        let (id, crt_fd) = ids(which);
        // SAFETY: plain query of the process's standard handle.
        let handle = unsafe { GetStdHandle(id) };
        if handle.is_null() || handle as isize == -1 {
            // A GUI program without a console: nothing to capture.
            return Err(std::io::Error::other("no standard handle"));
        }
        // SAFETY: the standard handle stays open while we borrow it here.
        let out = unsafe { BorrowedHandle::borrow_raw(handle) }.try_clone_to_owned()?;
        let (reader, writer) = std::io::pipe()?;

        // SAFETY: CRT calls on descriptor numbers; failures return -1.
        let crt = unsafe {
            let before = libc::dup(crt_fd);
            if before >= 0 {
                let pipe_fd = libc::open_osfhandle(writer.try_clone()?.into_raw_handle() as isize, libc::O_BINARY);
                if pipe_fd >= 0 {
                    libc::dup2(pipe_fd, crt_fd);
                    libc::close(pipe_fd);
                }
                Some(before)
            } else {
                None
            }
        };
        // SAFETY: `writer` stays open (in `Saved`) while it's the standard handle.
        if unsafe { SetStdHandle(id, writer.as_raw_handle()) } == 0 {
            let err = std::io::Error::last_os_error();
            let mut saved = Saved {
                handle,
                crt,
                writer: Some(writer),
            };
            restore(which, &mut saved);
            return Err(err);
        }
        Ok((
            Saved {
                handle,
                crt,
                writer: Some(writer),
            },
            reader,
            File::from(out),
        ))
    }

    pub(super) fn restore(which: Std, saved: &mut Saved) {
        let (id, crt_fd) = ids(which);
        // SAFETY: puts back the handle and descriptor saved in `redirect`.
        unsafe {
            SetStdHandle(id, saved.handle);
            if let Some(before) = saved.crt.take() {
                libc::dup2(before, crt_fd);
                libc::close(before);
            }
        }
        // The last write end: the copy thread now sees the end of the stream.
        saved.writer = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_cut_and_numbered() {
        // The ring is global; only this test pushes without a capture.
        let before = RING.lock().unwrap().next;
        push_line(b"hello\r");
        push_line(&[b'x'; MAX_LINE_BYTES * 2]);
        let ring = RING.lock().unwrap();
        assert_eq!(ring.next, before + 2);
        let n = ring.lines.len();
        assert_eq!(ring.lines[n - 2], "hello");
        assert_eq!(ring.lines[n - 1].len(), MAX_LINE_BYTES);
    }
}
