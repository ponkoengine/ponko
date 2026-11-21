//! Profiling scopes: time a piece of your own code and see it next to the
//! systems in the overlay, the benchmark reports and `ctx.perf.report()`.
//!
//! A scope costs one relaxed atomic load while profiling is off (see
//! [`Perf::set_profiling`](crate::perf::Perf::set_profiling)), and two clock reads
//! and three atomic adds while it is on. Scopes may be used from any thread
//! (rayon workers included): their times add up, so a scope that runs on four
//! threads at once reports four times the wall time.
//!
//! With the [`CountingAllocator`](crate::perf::CountingAllocator) installed, a
//! scope entered with [`profile_scope!`](crate::profile_scope) or
//! [`ProfileScope::enter`] also counts the allocations made on its own thread
//! while it is open ([`thread_alloc_counts`](crate::perf::thread_alloc_counts)):
//! a scope inside a parallel loop counts that loop's work on every thread
//! that runs it, and nothing the thread pool allocates for itself.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock, PoisonError};

use web_time::Instant;

/// How many things want profiling right now (a game with the overlay on, a
/// benchmark, `set_profiling(true)`). Scopes and allocation counting only do
/// their work while it is above zero.
pub(crate) static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Whether anything wants profiling right now.
#[inline]
pub(crate) fn active() -> bool {
    ACTIVE.load(Relaxed) > 0
}

pub(crate) fn enable() {
    ACTIVE.fetch_add(1, Relaxed);
}

pub(crate) fn disable() {
    // Never below zero, even if a test drops things in a strange order.
    let _ = ACTIVE.try_update(Relaxed, Relaxed, |n| n.checked_sub(1));
}

/// Nanoseconds since the first call; never 0 (0 means "not timing").
fn now_ns() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    (START.get_or_init(Instant::now).elapsed().as_nanos() as u64).max(1)
}

/// Every scope that has run while profiling was on, in the order they first
/// ran. Slots are never removed, so an index stays valid.
static REGISTRY: Mutex<Vec<&'static ProfileScope>> = Mutex::new(Vec::new());
/// Scopes made by name (`Perf::scope`, C and C++), one per name.
static INTERNED: Mutex<Vec<&'static ProfileScope>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One named scope: where [`profile_scope!`](crate::profile_scope) adds up
/// the time spent inside it.
///
/// You rarely name this type: the macro makes one per call site, and
/// [`Perf::scope`](crate::perf::Perf::scope) looks one up by name.
#[derive(Debug)]
pub struct ProfileScope {
    name: &'static str,
    ns: AtomicU64,
    calls: AtomicU64,
    /// Allocations made inside it on its own thread (guards only).
    allocs: AtomicU64,
    registered: AtomicBool,
}

impl ProfileScope {
    /// A scope called `name`, for a `static`. ([`profile_scope!`](crate::profile_scope) does this for you.)
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            ns: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            allocs: AtomicU64::new(0),
            registered: AtomicBool::new(false),
        }
    }

    /// The name it shows up under.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Starts timing until the returned guard is dropped, and counting the
    /// allocations made on this thread until then. Does nothing (apart from
    /// one atomic load) while profiling is off.
    ///
    /// ```
    /// use ponko::perf::ProfileScope;
    /// static SCOPE: ponko::perf::ProfileScope = ponko::perf::ProfileScope::new("expensive");
    /// let _timing = SCOPE.enter();
    /// ```
    #[inline]
    pub fn enter(&'static self) -> ScopeGuard {
        let token = self.begin();
        ScopeGuard {
            scope: self,
            token,
            allocs: if token == 0 {
                0
            } else {
                crate::alloc_count::thread_alloc_counts().count
            },
        }
    }

    /// Starts timing and returns a token for [`end`](Self::end); 0 when
    /// profiling is off. For code that can't hold a guard (the C API).
    /// Scopes timed this way count no allocations unless they end with
    /// [`end_counting`](Self::end_counting): only the caller knows where its
    /// thread's count stood.
    #[inline]
    pub fn begin(&'static self) -> u64 {
        if ACTIVE.load(Relaxed) == 0 {
            return 0;
        }
        if !self.registered.load(Relaxed) && !self.registered.swap(true, Relaxed) {
            lock(&REGISTRY).push(self);
        }
        now_ns()
    }

    /// Adds the time since [`begin`](Self::begin) returned `token`.
    #[inline]
    pub fn end(&self, token: u64) {
        if token == 0 {
            return;
        }
        let elapsed = now_ns().saturating_sub(token);
        self.ns.fetch_add(elapsed, Relaxed);
        self.calls.fetch_add(1, Relaxed);
    }

    /// [`end`](Self::end) for a scope whose caller read its thread's
    /// allocation count ([`thread_alloc_counts`](crate::perf::thread_alloc_counts)`().count`)
    /// right after [`begin`](Self::begin), as `allocs_at_begin`: also counts
    /// the allocations made on this thread since then, as a guard from
    /// [`enter`](Self::enter) does. Call it on the thread that began the
    /// scope. The C API's `ponko_profile_end` uses it.
    ///
    /// ```
    /// use ponko::perf::{ProfileScope, thread_alloc_counts};
    /// static SCOPE: ProfileScope = ProfileScope::new("parse");
    /// let token = SCOPE.begin();
    /// let allocs = thread_alloc_counts().count;
    /// // ... the work ...
    /// SCOPE.end_counting(token, allocs);
    /// ```
    #[inline]
    pub fn end_counting(&self, token: u64, allocs_at_begin: u64) {
        if token != 0 {
            let made = crate::alloc_count::thread_alloc_counts()
                .count
                .wrapping_sub(allocs_at_begin);
            self.allocs.fetch_add(made, Relaxed);
        }
        self.end(token);
    }

    fn totals(&self) -> Totals {
        Totals {
            ns: self.ns.load(Relaxed),
            calls: self.calls.load(Relaxed),
            allocs: self.allocs.load(Relaxed),
        }
    }
}

/// A scope's running totals, since the process started.
#[derive(Clone, Copy, Default)]
struct Totals {
    ns: u64,
    calls: u64,
    allocs: u64,
}

/// Times the code until it is dropped. From
/// [`profile_scope!`](crate::profile_scope) or [`ProfileScope::enter`].
#[must_use = "the scope times until this is dropped; bind it to a variable"]
pub struct ScopeGuard {
    scope: &'static ProfileScope,
    token: u64,
    /// This thread's allocation count when the scope was entered.
    allocs: u64,
}

impl ScopeGuard {
    /// A guard that times nothing, for when profiling is off.
    pub(crate) fn off() -> Self {
        static OFF: ProfileScope = ProfileScope::new("off");
        Self {
            scope: &OFF,
            token: 0,
            allocs: 0,
        }
    }
}

impl Drop for ScopeGuard {
    #[inline]
    fn drop(&mut self) {
        if self.token != 0 {
            let made = crate::alloc_count::thread_alloc_counts()
                .count
                .wrapping_sub(self.allocs);
            self.scope.allocs.fetch_add(made, Relaxed);
        }
        self.scope.end(self.token);
    }
}

/// The scope called `name`, made the first time it is asked for. Names are
/// kept for the life of the process.
pub fn scope_named(name: &str) -> &'static ProfileScope {
    let mut interned = lock(&INTERNED);
    if let Some(found) = interned.iter().find(|s| s.name == name) {
        return found;
    }
    let scope: &'static ProfileScope = Box::leak(Box::new(ProfileScope::new(Box::leak(name.into()))));
    interned.push(scope);
    scope
}

/// Times the rest of the block as a scope called `$name`.
///
/// It shows up next to the systems in the performance overlay, in
/// `PONKO_BENCH` reports and in [`Perf::report`](crate::perf::Perf::report).
/// While profiling is off it costs one atomic load. Times are inclusive: a
/// scope inside another is part of it.
///
/// ```
/// use ponko::prelude::*;
///
/// fn ai(ctx: &mut Ctx) {
///     profile_scope!("ai");
///     {
///         profile_scope!("ai/pathing");
///         // ...
///     }
///     ctx.world.query::<&mut ponko::d2::Transform>().for_each(|_t| {});
/// }
/// # Game::new("t", 1, 1).sim_system(ai).run_headless(1, |_| {});
/// ```
#[doc(hidden)]
#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        let _ponko_scope_guard = {
            static SCOPE: $crate::perf::ProfileScope = $crate::perf::ProfileScope::new($name);
            SCOPE.enter()
        };
    };
}

/// One scope's times, from [`Perf::scope_timings`](crate::perf::Perf::scope_timings).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScopeTiming {
    /// The scope's name.
    pub name: &'static str,
    /// Time spent in it during the last frame, in milliseconds.
    pub last_ms: f32,
    /// Smoothed average of `last_ms`.
    pub avg_ms: f32,
    /// How many times it ran during the last frame.
    pub calls: u32,
}

/// The scopes' times for one game: the differences between the process-wide
/// totals at the end of each frame.
#[derive(Default)]
pub(crate) struct ScopeState {
    /// Totals seen at the last frame, by registry index.
    seen: Vec<Totals>,
    /// One per distinct name; only ever grows, so indices stay valid.
    pub timings: Vec<ScopeTiming>,
    /// Allocations made inside each of `timings` during the last frame, on
    /// the threads it ran on.
    pub allocs: Vec<u32>,
}

impl ScopeState {
    /// Takes the totals so far as the starting point (used when profiling
    /// turns on, so earlier time isn't counted).
    pub fn rebase(&mut self) {
        self.seen.clear();
        read_all(|_, scope| self.seen.push(scope.totals()));
    }

    /// Ends a frame: `timings` get the time scopes took since the last call.
    pub fn end_frame(&mut self) {
        for t in &mut self.timings {
            t.last_ms = 0.0;
            t.calls = 0;
        }
        self.allocs.fill(0);
        let (seen, timings, allocs) = (&mut self.seen, &mut self.timings, &mut self.allocs);
        read_all(|i, scope| {
            if seen.len() <= i {
                seen.push(Totals::default());
            }
            let now = scope.totals();
            let was = std::mem::replace(&mut seen[i], now);
            let (d_ns, d_calls) = (now.ns.saturating_sub(was.ns), now.calls.saturating_sub(was.calls));
            let d_allocs = now.allocs.saturating_sub(was.allocs);
            if d_calls == 0 {
                return;
            }
            let slot = match timings.iter().position(|t| t.name == scope.name) {
                Some(slot) => slot,
                None => {
                    timings.push(ScopeTiming {
                        name: scope.name,
                        last_ms: 0.0,
                        avg_ms: 0.0,
                        calls: 0,
                    });
                    allocs.push(0);
                    timings.len() - 1
                }
            };
            timings[slot].last_ms += d_ns as f32 / 1e6;
            timings[slot].calls += d_calls as u32;
            allocs[slot] = allocs[slot].saturating_add(d_allocs.min(u64::from(u32::MAX)) as u32);
        });
        for t in timings {
            t.avg_ms += (t.last_ms - t.avg_ms) * 0.05;
        }
    }
}

/// Calls `f` with every registered scope and its registry index.
fn read_all(mut f: impl FnMut(usize, &'static ProfileScope)) {
    for (i, scope) in lock(&REGISTRY).iter().enumerate() {
        f(i, scope);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scopes are process-wide, so tests use names nobody else does and read
    // only their own.
    #[test]
    fn scopes_add_up_their_time_and_calls() {
        static SCOPE: ProfileScope = ProfileScope::new("scope_test/add_up");
        enable();
        let mut state = ScopeState::default();
        state.rebase();
        for _ in 0..3 {
            let _g = SCOPE.enter();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        state.end_frame();
        disable();
        let t = state.timings.iter().find(|t| t.name == "scope_test/add_up").unwrap();
        assert_eq!(t.calls, 3);
        assert!(t.last_ms >= 6.0, "{t:?}");
        state.end_frame();
        let t = state.timings.iter().find(|t| t.name == "scope_test/add_up").unwrap();
        assert_eq!((t.calls, t.last_ms), (0, 0.0), "a quiet frame resets last_ms");
        assert!(t.avg_ms > 0.0);
    }

    #[test]
    fn names_map_to_one_scope() {
        let a = scope_named("scope_test/named");
        let b = scope_named(&String::from("scope_test/named"));
        assert!(std::ptr::eq(a, b));
        assert_eq!(a.name(), "scope_test/named");
    }

    #[test]
    fn scopes_work_across_threads() {
        static SCOPE: ProfileScope = ProfileScope::new("scope_test/threads");
        enable();
        let mut state = ScopeState::default();
        state.rebase();
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    let _g = SCOPE.enter();
                });
            }
        });
        state.end_frame();
        disable();
        let t = state.timings.iter().find(|t| t.name == "scope_test/threads").unwrap();
        assert_eq!(t.calls, 4);
    }
}
