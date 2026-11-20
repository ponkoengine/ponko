//! A global allocator that counts heap allocations, so every benchmark can
//! report how many it makes per timed run.
//!
//! It always counts (three relaxed atomic adds per allocation, for the
//! count, the bytes and the bytes still live, and one per free, on top of
//! the system allocator) and counts
//! the whole process: allocations on rayon or
//! bevy worker threads during a timed run are charged to that run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};

static COUNT: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
/// Bytes allocated and not yet freed.
static LIVE: AtomicI64 = AtomicI64::new(0);

/// The system allocator, counting allocations and reallocations.
pub struct Counting;

// SAFETY: every method forwards to `System` with the same arguments; the
// counting only touches atomics and never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: the caller's contract is `System`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as i64, Relaxed);
        // SAFETY: `ptr` came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        LIVE.fetch_sub(layout.size() as i64, Relaxed);
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[inline]
fn note(bytes: usize) {
    COUNT.fetch_add(1, Relaxed);
    BYTES.fetch_add(bytes as u64, Relaxed);
    LIVE.fetch_add(bytes as i64, Relaxed);
    if SITES_ON.load(Relaxed) {
        sites::record();
    }
}

static SITES_ON: AtomicBool = AtomicBool::new(false);

/// Where allocations come from: while [`sites::start`] .. [`sites::stop`]
/// runs, every allocation's call stack is kept (slow: only for a few
/// frames), then grouped by the engine function it came from and the
/// library call it went through. Only the render scenes (`render`
/// feature) read them.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub mod sites {
    use std::backtrace::Backtrace;
    use std::cell::Cell;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering::Relaxed;

    thread_local! {
        /// Inside `record`: its own allocations aren't recorded.
        static IN_RECORD: Cell<bool> = const { Cell::new(false) };
    }
    static TRACES: Mutex<Vec<Backtrace>> = Mutex::new(Vec::new());

    #[cold]
    pub(super) fn record() {
        if IN_RECORD.with(|r| r.replace(true)) {
            return;
        }
        let trace = Backtrace::force_capture();
        if let Ok(mut traces) = TRACES.lock() {
            traces.push(trace);
        }
        IN_RECORD.with(|r| r.set(false));
    }

    /// Starts keeping every allocation's call stack.
    pub fn start() {
        if let Ok(mut traces) = TRACES.lock() {
            traces.clear();
        }
        super::SITES_ON.store(true, Relaxed);
    }

    /// Stops, and gives each site (`engine function -> library call`, or
    /// the engine function alone when it allocated itself) with its count,
    /// most first.
    pub fn stop() -> Vec<(String, usize)> {
        super::SITES_ON.store(false, Relaxed);
        let traces = TRACES.lock().map(|mut t| std::mem::take(&mut *t)).unwrap_or_default();
        let mut sites: Vec<(String, usize)> = Vec::new();
        for trace in traces {
            let site = classify(&trace.to_string());
            match sites.iter_mut().find(|(s, _)| *s == site) {
                Some((_, n)) => *n += 1,
                None => sites.push((site, 1)),
            }
        }
        sites.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        sites
    }

    /// A frame's function name without its hash and leading number.
    fn name(line: &str) -> Option<&str> {
        let (index, rest) = line.trim_start().split_once(": ")?;
        index.parse::<usize>().ok()?;
        let rest = rest.trim();
        Some(match rest.rfind("::h") {
            Some(i) if rest[i + 3..].chars().all(|c| c.is_ascii_hexdigit()) => &rest[..i],
            _ => rest,
        })
    }

    fn engine(name: &str) -> bool {
        name.starts_with("ponko::") || name.starts_with("<ponko::")
    }

    /// The innermost frame past the allocator: what really allocated.
    fn allocator(name: &str) -> bool {
        name.starts_with("std::")
            || name.starts_with("<std::")
            || name.starts_with("alloc::")
            || name.starts_with("<alloc::")
            || name.starts_with("__rustc::")
            || name.starts_with("<core::")
            || name.starts_with("core::")
            || name.starts_with("<hashbrown::")
            || name.starts_with("hashbrown::")
            || name.starts_with("<smallvec::")
            || name.starts_with("<arrayvec::")
            || name.starts_with("ponko_bench::")
            || name.starts_with("_")
    }

    fn library(name: &str) -> bool {
        ["wgpu", "<wgpu", "naga", "<naga", "metal", "<metal", "objc", "<objc"]
            .iter()
            .any(|p| name.starts_with(p))
    }

    /// `engine fn -> library fn` for the innermost engine frame.
    fn classify(trace: &str) -> String {
        let frames: Vec<&str> = trace.lines().filter_map(name).collect();
        let inner = frames.iter().find(|f| !allocator(f)).copied().unwrap_or("?");
        let inner = if inner.len() > 160 { &inner[..160] } else { inner };
        // Rayon jobs name the engine closure they run inside their own frames.
        let at = frames
            .iter()
            .position(|f| engine(f))
            .or_else(|| frames.iter().position(|f| f.contains("ponko::")));
        let Some(at) = at else {
            // The outermost library call, or the outermost frame.
            let outer = frames
                .iter()
                .rev()
                .find(|f| library(f))
                .or_else(|| {
                    frames
                        .iter()
                        .rev()
                        .find(|f| !f.starts_with("std::") && !f.starts_with("<std::"))
                })
                .unwrap_or(&"?");
            if std::env::var_os("PONKO_ALLOC_SITES_DUMP").is_some() {
                eprintln!("---\n{trace}");
            }
            return format!("(outside the engine) {outer} [{inner}]");
        };
        let callee = frames[..at].iter().rev().find(|f| library(f));
        let caller = frames[at];
        let caller = if caller.len() > 110 { &caller[..110] } else { caller };
        match callee {
            Some(lib) => format!("{caller} -> {lib} [{inner}]"),
            None => format!("{caller} [{inner}]"),
        }
    }
}

/// Heap bytes the whole process has allocated and not freed yet.
pub fn live_bytes() -> i64 {
    LIVE.load(Relaxed)
}

/// Allocations (with reallocations) and bytes requested so far, in the
/// whole process. Take two readings and subtract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub count: u64,
    pub bytes: u64,
}

impl Counts {
    pub fn now() -> Self {
        Counts {
            count: COUNT.load(Relaxed),
            bytes: BYTES.load(Relaxed),
        }
    }

    /// What was counted between `earlier` and `self`.
    pub fn since(self, earlier: Counts) -> Counts {
        Counts {
            count: self.count - earlier.count,
            bytes: self.bytes - earlier.bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_are_counted() {
        let before = Counts::now();
        let v: Vec<u8> = black_box_vec(4096);
        let counted = Counts::now().since(before);
        drop(v);
        // Other tests allocate at the same time, so only a lower bound holds.
        assert!(counted.count >= 1 && counted.bytes >= 4096, "{counted:?}");
    }

    fn black_box_vec(n: usize) -> Vec<u8> {
        std::hint::black_box(Vec::with_capacity(n))
    }
}
