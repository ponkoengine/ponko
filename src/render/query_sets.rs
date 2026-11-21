//! How many timestamp query sets the engine may hold, process-wide.
//!
//! Metal backs each timestamp query set with a counter sample buffer, and
//! a process may hold only 32 of them, of any size, whatever device made
//! them: asking for a 33rd fails in the driver, and `wgpu` loses the device
//! that asked (measured on an Apple M4 Max with wgpu 30: sets of 2, 8, 64,
//! 384 and 4,096 queries all fail at the 33rd, and two devices share the 32;
//! Apple's documentation names no number). The number of queries in a set
//! doesn't matter there. Other backends have no such limit known.
//!
//! So every timestamp query set the engine makes takes a [`Lease`] first.
//! On Metal the engine takes at most [`ENGINE_SETS`] of the 32, leaving the
//! rest for a game's own sets (and for sets `wgpu` hasn't freed yet: a
//! dropped set lives until the GPU has finished the work that used it).
//! When none is left, the timer that asked goes without, with one warning,
//! and its device keeps working. In the engine's own tests, which run many
//! devices at once, a timer instead waits a while for another test to give
//! one back.

use std::sync::{Condvar, Mutex, PoisonError};

/// Timestamp query sets one process can hold on Metal.
pub(crate) const METAL_SETS: u32 = 32;
/// Of those, the most the engine takes.
pub(crate) const ENGINE_SETS: u32 = 28;

/// Query sets held against a budget.
pub(crate) struct Ledger {
    budget: u32,
    held: Mutex<u32>,
    freed: Condvar,
}

impl Ledger {
    pub(crate) const fn new(budget: u32) -> Self {
        Self {
            budget,
            held: Mutex::new(0),
            freed: Condvar::new(),
        }
    }

    /// One set, if the budget has room; waits up to `wait` for one to be
    /// given back.
    fn take(&'static self, wait: std::time::Duration) -> Result<Lease, u32> {
        let deadline = web_time::Instant::now() + wait;
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        while *held >= self.budget {
            let left = deadline.saturating_duration_since(web_time::Instant::now());
            if left.is_zero() {
                return Err(*held);
            }
            held = self
                .freed
                .wait_timeout(held, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *held += 1;
        Ok(Lease { ledger: Some(self) })
    }

    /// Sets held now.
    #[cfg(test)]
    fn held(&self) -> u32 {
        *self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The Metal sets of every device in this process.
static METAL: Ledger = Ledger::new(ENGINE_SETS);

/// The right to hold one timestamp query set. Declare it after the set in
/// a struct, so the set is dropped first.
pub(crate) struct Lease {
    /// `None` on a backend without a known limit.
    ledger: Option<&'static Ledger>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(ledger) = self.ledger {
            *ledger.held.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
            ledger.freed.notify_one();
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Tests only: refuse at once instead of waiting, on this thread.
    static REFUSE_WHEN_FULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests only: on this thread, a set is refused as soon as the budget is
/// used up, as in a game, instead of waiting for another test's.
#[cfg(all(test, feature = "3d"))]
pub(crate) fn refuse_when_full(on: bool) {
    REFUSE_WHEN_FULL.with(|r| r.set(on));
}

/// How long a set waits for room: never in a game; in the engine's tests,
/// long enough for the other tests to finish with theirs.
fn wait() -> std::time::Duration {
    #[cfg(test)]
    if !REFUSE_WHEN_FULL.with(std::cell::Cell::get) {
        return std::time::Duration::from_secs(30);
    }
    std::time::Duration::ZERO
}

/// The right to make one timestamp query set on `device`, for `what` (a
/// timer's name, for the warning). `None`, after one warning, when the
/// process already holds as many as the backend allows: the caller makes
/// no set and goes without timing.
pub(crate) fn lease(device: &wgpu::Device, what: &str) -> Option<Lease> {
    if device.adapter_info().backend != wgpu::Backend::Metal {
        return Some(Lease { ledger: None });
    }
    match METAL.take(wait()) {
        Ok(lease) => Some(lease),
        Err(held) => {
            eprintln!(
                "ponko: {what}: GPU timing is off for this device. This process already holds {held} timestamp query \
                 sets, the most the engine takes on Metal ({ENGINE_SETS} of the {METAL_SETS} a process may hold; \
                 asking for more loses the device). Drop another device's renderer to free one."
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A full budget refuses the next set at once (a game never waits for
    /// one), and a set given back can be taken again.
    #[test]
    fn a_full_budget_refuses_sets_until_one_is_given_back() {
        static LEDGER: Ledger = Ledger::new(3);
        let mut held: Vec<Lease> = (0..3).map(|_| LEDGER.take(Duration::ZERO).ok().unwrap()).collect();
        assert_eq!(LEDGER.held(), 3);
        assert_eq!(LEDGER.take(Duration::ZERO).err(), Some(3));
        held.pop();
        assert_eq!(LEDGER.held(), 2);
        held.push(LEDGER.take(Duration::ZERO).ok().unwrap());
        drop(held);
        assert_eq!(LEDGER.held(), 0);
    }

    /// In the engine's tests a set waits for another test to give one
    /// back, instead of going without.
    #[test]
    fn a_waiting_set_gets_the_one_given_back() {
        static LEDGER: Ledger = Ledger::new(1);
        let first = LEDGER.take(Duration::ZERO).ok().unwrap();
        let giver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(first);
        });
        assert!(LEDGER.take(Duration::from_secs(30)).is_ok());
        giver.join().unwrap();
    }

    /// The measurement behind [`METAL_SETS`]: on Metal, a process holds 32
    /// timestamp query sets of any size, shared by its devices. One device
    /// holds 16 sets of 4,096 queries (65,536 queries); a second then gets
    /// 16 sets of 8, and the 33rd set loses it. Ignored: it loses a device
    /// on purpose and takes every set of the process, so it runs alone:
    /// `cargo test -p ponko --features 3d --lib render::query_sets -- --ignored`.
    #[cfg(feature = "3d")]
    #[test]
    #[ignore = "loses a device on purpose; run alone"]
    fn metal_allows_32_timestamp_query_sets_per_process_of_any_size() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let gpu = || {
            crate::render::render_test_support::gpu_with(|adapter| {
                Ok(wgpu::DeviceDescriptor {
                    required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                    ..Default::default()
                })
            })
        };
        let (Some(first), Some(second)) = (gpu(), gpu()) else {
            return;
        };
        if first.device.adapter_info().backend != wgpu::Backend::Metal
            || !first.device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            crate::render::render_test_support::skip("measures Metal's timestamp query sets");
            return;
        }
        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        second
            .device
            .set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
        let make = |device: &wgpu::Device, count| {
            device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Timestamp,
                count,
            })
        };
        let mut held: Vec<_> = (0..16)
            .map(|_| make(&first.device, wgpu::QUERY_SET_MAX_QUERIES))
            .collect();
        let mut sets = 16;
        while !lost.load(Ordering::Acquire) && sets < 40 {
            held.push(make(&second.device, 8));
            let _ = second.device.poll(wgpu::PollType::Poll);
            sets += 1;
        }
        assert_eq!(sets, METAL_SETS + 1, "the device was lost making set {sets}");
    }
}
