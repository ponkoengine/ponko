//! A `GameRollback` session allocates nothing per frame once warm: the
//! saved states, the event store and the inputs reuse their memory, also
//! on frames that roll back. (Its own test binary: it counts every
//! allocation of the process.)

use std::sync::{Arc, Mutex};

use ponko::net::{GameRollback, Rollback, RollbackGame};
use ponko::perf::{CountingAllocator, alloc_counts};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Pad(i8);

#[derive(Component, Clone, Copy)]
struct Pos(f32);

/// The remote player: a bare session whose game does nothing.
struct Remote;

impl RollbackGame<Pad> for Remote {
    type State = ();
    fn save(&mut self, (): &mut ()) {}
    fn load(&mut self, (): &()) {}
    fn advance(&mut self, _: &[Pad]) {}
}

/// Changes every 4 ticks, so the late remote input is often mispredicted.
fn pad(p: usize, t: u64) -> Pad {
    Pad(((t / 4 + p as u64) % 3) as i8 - 1)
}

#[test]
fn a_session_allocates_nothing_per_frame_once_warm() {
    let mut remote: Rollback<Pad, ()> = Rollback::new(2, 1).checksum_every(0);
    // Packets in flight, 4 frames late, in reused buffers.
    let mut ring: Vec<(u64, Vec<u8>)> = (0..8).map(|_| (u64::MAX, Vec::with_capacity(1500))).collect();
    let mut frame = 0u64;
    let mut last: Option<u64> = None;
    let counts = Arc::new(Mutex::new(Vec::with_capacity(400)));
    let out = Arc::clone(&counts);
    let stats = Arc::new(Mutex::new(None));
    let stats_out = Arc::clone(&stats);
    Game::new("alloc", 1, 1)
        .headless_frames(true)
        .sim_system(|ctx: &mut Ctx| {
            let d = ctx.net.rollback_inputs::<Pad>().map_or(0.0, |i| f32::from(i[1].0));
            ctx.world.query::<&mut Pos>().for_each(|p| p.0 += d);
        })
        .system(move |ctx: &mut Ctx| {
            // Everything since the end of the last frame system: the tick
            // (and any rollback) and the engine's frame.
            let now = alloc_counts().count;
            if let Some(before) = last {
                out.lock().unwrap().push(now - before);
            }
            let t = ctx.time.sim_tick();
            let local = ctx.net.rollback::<Pad>().expect("the session runs");
            local.set_input(pad(0, t));
            remote.receive(local.packet());
            while remote.frame() <= t && remote.advance(&mut Remote, pad(1, remote.frame())).ran() {}
            let slot = &mut ring[(frame % 8) as usize];
            slot.0 = frame + 4;
            slot.1.clear();
            slot.1.extend_from_slice(remote.packet());
            for s in &mut ring {
                if s.0 <= frame {
                    local.receive(&s.1);
                    s.0 = u64::MAX;
                }
            }
            *stats_out.lock().unwrap() = Some(local.stats());
            frame += 1;
            last = Some(alloc_counts().count);
        })
        .run_headless(400, |ctx| {
            for i in 0..1000 {
                ctx.world.spawn((Pos(i as f32),));
            }
            let mut session = GameRollback::<Pad>::new(2, 0).checksum_every(0);
            session.set_input(pad(0, 0));
            ctx.net.start_rollback(session);
        });
    let stats = stats.lock().unwrap().expect("frames ran");
    assert!(stats.rollbacks > 50, "the late input rolls back often: {stats:?}");
    let counts = counts.lock().unwrap();
    let warm = &counts[100..];
    assert!(
        warm.iter().all(|&n| n == 0),
        "allocations per frame once warm: {:?}",
        warm.iter().filter(|&&n| n > 0).collect::<Vec<_>>()
    );
}
