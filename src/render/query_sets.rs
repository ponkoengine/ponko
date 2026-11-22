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
//! rest for a game's own sets. When none is left, the timer that asked
//! goes without, with one warning, and its device keeps working. In the
//! engine's own tests, which run many devices at once, a timer instead
//! waits a while for another test to give one back.
//!
//! A dropped set isn't freed at once: `wgpu` keeps it until its device has
//! seen the GPU finish the work that used it (a poll or a submission after
//! that), and only then lets the driver free it. Until then it still counts
//! against Metal's 32, so a lease is given back then too, not when the set
//! is dropped: a timer dropped and made again while its frames are still
//! on the GPU would otherwise hold more sets than the budget says (see
//! `a_quick_drop_and_create_cycle_never_holds_more_sets_than_it_counts`).

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
    /// given back. `poll` lets the asking device free the sets it dropped
    /// (see the top), first when the budget is full and then every few
    /// milliseconds of the wait: a test that drops a timer and makes
    /// another on one device would otherwise wait for itself.
    fn take(&'static self, wait: std::time::Duration, poll: impl Fn()) -> Result<Lease, u32> {
        const POLL_EVERY: std::time::Duration = std::time::Duration::from_millis(5);
        let deadline = web_time::Instant::now() + wait;
        let mut polled = false;
        loop {
            {
                let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
                if *held < self.budget {
                    *held += 1;
                    return Ok(Lease {
                        ledger: Some(self),
                        gpu: None,
                        #[cfg(test)]
                        counted: None,
                    });
                }
                if polled {
                    let left = deadline.saturating_duration_since(web_time::Instant::now());
                    if left.is_zero() {
                        return Err(*held);
                    }
                    drop(
                        self.freed
                            .wait_timeout(held, left.min(POLL_EVERY))
                            .unwrap_or_else(PoisonError::into_inner),
                    );
                }
            }
            // Not under the lock: a set freed by the poll gives its lease
            // back, which takes it.
            poll();
            polled = true;
        }
    }

    fn give_back(&self) {
        *self.held.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
        self.freed.notify_one();
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
    /// The device that holds the set: the lease is given back once it has
    /// let go of the set (see the top).
    gpu: Option<(wgpu::Device, wgpu::Queue)>,
    /// Tests only: this thread's count of leases not yet given back.
    #[cfg(test)]
    counted: Option<&'static std::sync::atomic::AtomicI32>,
}

/// A lease's share of the budget, given back when this is dropped: by the
/// queue once it has finished the work submitted so far, or with the
/// queue if it goes first.
struct GiveBack {
    ledger: Option<&'static Ledger>,
    #[cfg(test)]
    counted: Option<&'static std::sync::atomic::AtomicI32>,
}

impl Drop for GiveBack {
    fn drop(&mut self) {
        if let Some(ledger) = self.ledger {
            ledger.give_back();
        }
        #[cfg(test)]
        if let Some(counted) = self.counted {
            counted.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let back = GiveBack {
            ledger: self.ledger,
            #[cfg(test)]
            counted: self.counted,
        };
        let Some((device, queue)) = self.gpu.take() else {
            return;
        };
        // The set, dropped just before, lives until the work submitted so
        // far is done and the device has seen it: `wgpu` frees what that
        // work used before it runs these callbacks. If the device is idle,
        // the poll gives it back now.
        queue.on_submitted_work_done(move || drop(back));
        let _ = device.poll(wgpu::PollType::Poll);
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

#[cfg(test)]
thread_local! {
    /// Tests only: where this thread counts its leases not yet given back.
    static COUNTED: std::cell::Cell<Option<&'static std::sync::atomic::AtomicI32>> = const { std::cell::Cell::new(None) };
}

/// The right to make one timestamp query set on `device` (whose queue is
/// `queue`), for `what` (a timer's name, for the warning). `None`, after
/// one warning, when the process already holds as many as the backend
/// allows: the caller makes no set and goes without timing.
pub(crate) fn lease(device: &wgpu::Device, queue: &wgpu::Queue, what: &str) -> Option<Lease> {
    lease_with(device, queue, Some(what))
}

/// [`lease`] for a caller that has a fallback and says nothing when
/// refused: a timer asking for a second set to replace its own.
pub(crate) fn lease_quietly(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Lease> {
    lease_with(device, queue, None)
}

fn lease_with(device: &wgpu::Device, queue: &wgpu::Queue, what: Option<&str>) -> Option<Lease> {
    let mut lease = if device.adapter_info().backend == wgpu::Backend::Metal {
        let poll = || {
            let _ = device.poll(wgpu::PollType::Poll);
        };
        match METAL.take(wait(), poll) {
            Ok(lease) => lease,
            Err(held) => {
                let what = what?;
                eprintln!(
                    "ponko: {what}: GPU timing is off for this device. This process already holds {held} timestamp \
                     query sets, the most the engine takes on Metal ({ENGINE_SETS} of the {METAL_SETS} a process may \
                     hold; asking for more loses the device). Drop another device's renderer to free one."
                );
                return None;
            }
        }
    } else {
        Lease {
            ledger: None,
            gpu: None,
            #[cfg(test)]
            counted: None,
        }
    };
    lease.gpu = Some((device.clone(), queue.clone()));
    #[cfg(test)]
    {
        lease.counted = COUNTED.with(std::cell::Cell::get);
        if let Some(counted) = lease.counted {
            counted.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
    Some(lease)
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
        let mut held: Vec<Lease> = (0..3)
            .map(|_| LEDGER.take(Duration::ZERO, || ()).ok().unwrap())
            .collect();
        assert_eq!(LEDGER.held(), 3);
        assert_eq!(LEDGER.take(Duration::ZERO, || ()).err(), Some(3));
        held.pop();
        assert_eq!(LEDGER.held(), 2);
        held.push(LEDGER.take(Duration::ZERO, || ()).ok().unwrap());
        drop(held);
        assert_eq!(LEDGER.held(), 0);
    }

    /// In the engine's tests a set waits for another test to give one
    /// back, instead of going without.
    #[test]
    fn a_waiting_set_gets_the_one_given_back() {
        static LEDGER: Ledger = Ledger::new(1);
        let first = LEDGER.take(Duration::ZERO, || ()).ok().unwrap();
        let giver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(first);
        });
        assert!(LEDGER.take(Duration::from_secs(30), || ()).is_ok());
        giver.join().unwrap();
    }

    /// A device for the tests below, with timestamp queries; `None` (a
    /// skip) without them.
    #[cfg(feature = "3d")]
    fn timed_gpu() -> Option<crate::render::render_test_support::TestGpu> {
        let gpu = crate::render::render_test_support::gpu_with(|adapter| {
            Ok(wgpu::DeviceDescriptor {
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            })
        })?;
        if !gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            crate::render::render_test_support::skip("needs timestamp queries");
            return None;
        }
        Some(gpu)
    }

    /// Timestamp query sets `device` really holds: made and not yet freed
    /// by `wgpu` (its internal counters, on in the engine's tests).
    #[cfg(feature = "3d")]
    fn sets_held(device: &wgpu::Device) -> i32 {
        device.get_internal_counters().hal.query_sets.read() as i32
    }

    /// Keeps the GPU busy for a while: `rounds` of a loop in each of 65,536
    /// invocations, submitted and not waited for.
    #[cfg(feature = "3d")]
    fn keep_busy(device: &wgpu::Device, queue: &wgpu::Queue, rounds: u32) {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("busy"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "@group(0) @binding(0) var<storage, read_write> out: array<u32>;
                     @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
                         var x = id.x;
                         for (var i = 0u; i < {rounds}u; i++) {{ x = x * 1664525u + 1013904223u; }}
                         out[id.x] = x;
                     }}"
                )
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("busy"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("busy"),
            size: 65_536 * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("busy"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: out.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(1024, 1, 1);
        }
        queue.submit([encoder.finish()]);
    }

    /// One frame of `timer` on the GPU: a timed clear of a small target,
    /// submitted and not waited for.
    #[cfg(feature = "3d")]
    fn timed_frame(device: &wgpu::Device, queue: &wgpu::Queue, timer: &mut crate::render::timer::GpuTimer) {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        timer.begin_frame();
        let mut encoder = device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timer.pass("clear"),
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        timer.resolve(&mut encoder);
        queue.submit([encoder.finish()]);
        timer.submitted();
    }

    /// Counts this thread's leases not yet given back while it lives.
    #[cfg(feature = "3d")]
    struct Counting(&'static std::sync::atomic::AtomicI32);

    #[cfg(feature = "3d")]
    impl Counting {
        fn new() -> Self {
            let counted: &'static _ = Box::leak(Box::new(std::sync::atomic::AtomicI32::new(0)));
            COUNTED.with(|c| c.set(Some(counted)));
            Self(counted)
        }
        fn get(&self) -> i32 {
            self.0.load(std::sync::atomic::Ordering::Acquire)
        }
    }

    #[cfg(feature = "3d")]
    impl Drop for Counting {
        fn drop(&mut self) {
            COUNTED.with(|c| c.set(None));
        }
    }

    /// A timer dropped while its frame is still on the GPU leaves its
    /// query set alive: `wgpu` frees it only once its device has seen that
    /// work finish. Before, its lease was given back at the drop, so the
    /// budget counted one set fewer than the process held. Now the lease
    /// stays counted until the device has let go of the set. Offscreen,
    /// one device; skips without timestamp queries.
    #[cfg(feature = "3d")]
    #[test]
    fn a_dropped_timer_counts_its_set_until_the_device_has_let_go_of_it() {
        let Some(gpu) = timed_gpu() else {
            return;
        };
        let (device, queue) = (&gpu.device, &gpu.queue);
        let counting = Counting::new();
        let base = sets_held(device);
        // Longer each time, until the frame is still on the GPU when its
        // timer is dropped.
        let mut rounds = 20_000;
        let still_held = loop {
            keep_busy(device, queue, rounds);
            let mut timer = crate::render::timer::GpuTimer::new(device, queue).expect("a timer");
            timed_frame(device, queue, &mut timer);
            assert_eq!(sets_held(device) - base, 1);
            drop(timer);
            let held = sets_held(device) - base;
            eprintln!(
                "busy {rounds} rounds: after the drop the device still holds {held} set, {} counted",
                counting.get()
            );
            assert!(counting.get() >= held, "every set the device holds is counted");
            if held > 0 || rounds >= 20_000 * 64 {
                break held;
            }
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            rounds *= 4;
        };
        assert_eq!(still_held, 1, "a set dropped with its frame on the GPU is freed late");
        assert_eq!(counting.get(), 1, "and its lease is still counted");
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert_eq!(sets_held(device) - base, 0, "the device has let go of it");
        assert_eq!(counting.get(), 0, "and the lease is given back");
    }

    /// Frame and upload timers dropped and made again many times in a row
    /// (as a game recreating its renderer would), on one device and on
    /// three, with their frames still on the GPU: no timer is refused, no
    /// device is lost, and the leases counted are never fewer than the
    /// sets the devices really hold, though some of those were dropped
    /// already (the count before this held up to that many sets fewer, so
    /// with the engine's 28 taken a cycle could pass Metal's 32). To leave
    /// the process's sets to the other tests, the cycle lets its devices
    /// catch up whenever it counts more than 8. Offscreen; skips without
    /// timestamp queries.
    #[cfg(feature = "3d")]
    #[test]
    fn a_quick_drop_and_create_cycle_never_holds_more_sets_than_it_counts() {
        use crate::render::timer::GpuTimer;
        use crate::render::upload_timer::UploadTimer;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        for devices in [1, 3] {
            let counting = Counting::new();
            let mut gpus = Vec::new();
            for _ in 0..devices {
                let Some(gpu) = timed_gpu() else {
                    return;
                };
                let lost = Arc::new(AtomicBool::new(false));
                let flag = lost.clone();
                gpu.device
                    .set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
                let base = sets_held(&gpu.device);
                gpus.push((gpu, lost, base));
            }
            let held = |gpus: &[(crate::render::render_test_support::TestGpu, Arc<AtomicBool>, i32)]| {
                gpus.iter().map(|(g, _, base)| sets_held(&g.device) - base).sum::<i32>()
            };
            let (mut refused, mut freed_late) = (0, 0);
            for round in 0..60 {
                for (gpu, lost, _) in &gpus {
                    if round == 0 {
                        keep_busy(&gpu.device, &gpu.queue, 20_000);
                    }
                    let mut timer = GpuTimer::new(&gpu.device, &gpu.queue);
                    let uploads = UploadTimer::new(&gpu.device, &gpu.queue);
                    refused += usize::from(timer.is_none()) + usize::from(uploads.is_none());
                    let live = i32::from(timer.is_some()) + i32::from(uploads.is_some());
                    if let Some(timer) = timer.as_mut() {
                        timed_frame(&gpu.device, &gpu.queue, timer);
                    }
                    freed_late = freed_late.max(held(&gpus) - live);
                    assert!(
                        counting.get() >= held(&gpus),
                        "round {round}: every set held is counted"
                    );
                    drop((timer, uploads));
                    assert!(
                        counting.get() >= held(&gpus),
                        "round {round}: every set held is counted"
                    );
                    assert!(!lost.load(Ordering::Acquire), "round {round}: the device was lost");
                }
                if counting.get() > 8 {
                    for (gpu, ..) in &gpus {
                        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
                    }
                }
            }
            for (gpu, lost, _) in &gpus {
                assert!(gpu.device.poll(wgpu::PollType::wait_indefinitely()).is_ok());
                assert!(!lost.load(Ordering::Acquire), "a device was lost");
            }
            eprintln!(
                "{devices} device(s): {refused} timers refused, up to {freed_late} sets held after their timers \
                 were dropped; {} counted, {} held at the end",
                counting.get(),
                held(&gpus)
            );
            assert_eq!(refused, 0, "no timer is refused");
            assert_eq!(
                (counting.get(), held(&gpus)),
                (0, 0),
                "all given back once the devices let go"
            );
        }
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
