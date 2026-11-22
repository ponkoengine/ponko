//! A `GameRollback` session allocates nothing per frame once warm: the
//! saved states (with change stamps), the event store and the inputs reuse
//! their memory, also on frames that roll back. (Its own test binary: it
//! counts every allocation of the process.)
//!
//! The counting allocator counts only while profiling is on. Profiling
//! reserves its whole frame history when it turns on, so no frame is
//! excluded: every frame from `WARM` on is checked, and the history fills
//! (and would have doubled, at 64, 128 and 256 frames) inside that window.
//! The engine's rollback work is also measured on its own, through its
//! profile scopes (`rollback/save`, `rollback/load`).

use std::sync::{Arc, Mutex};

use ponko::net::{GameRollback, Rollback, RollbackGame};
use ponko::perf::{CountingAllocator, ScopeReport, alloc_counts};
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

/// The frame from which allocations are checked.
const WARM: u64 = 100;
/// The frame that turns profiling (and so counting) on: early enough that the
/// profiler has seen every system and scope the session runs (a scope's
/// first appearance allocates its place in the report, once) before `WARM`.
const PROFILE_FROM: u64 = 50;

struct Measured {
    /// Allocations between the ends of two frame systems: a frame's tick
    /// (with any rollback) and the engine's frame.
    per_frame: Vec<u64>,
    rollbacks: u64,
    /// The rollback scopes of the profiler's report at the end.
    scopes: Vec<ScopeReport>,
}

/// 400 frames of a session whose remote input arrives 4 frames late, with
/// `setup` adding to the world.
fn measure(game: Game, setup: impl FnOnce(&mut Ctx) + 'static) -> Measured {
    // Scope totals are the process's: one measurement at a time.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut remote: Rollback<Pad, ()> = Rollback::new(2, 1).checksum_every(0);
    // Packets in flight, 4 frames late, in reused buffers.
    let mut ring: Vec<(u64, Vec<u8>)> = (0..8).map(|_| (u64::MAX, Vec::with_capacity(1500))).collect();
    let mut frame = 0u64;
    let mut last: Option<u64> = None;
    let result = Arc::new(Mutex::new(Measured {
        per_frame: Vec::with_capacity(400),
        rollbacks: 0,
        scopes: Vec::new(),
    }));
    let out = Arc::clone(&result);
    game.headless_frames(true)
        .sim_system(|ctx: &mut Ctx| {
            let d = ctx.net.rollback_inputs::<Pad>().map_or(0.0, |i| f32::from(i[1].0));
            ctx.world.query::<&mut Pos>().for_each(|p| p.0 += d);
        })
        .system(move |ctx: &mut Ctx| {
            let now = alloc_counts().count;
            let mut out = out.lock().unwrap();
            if let Some(before) = last {
                out.per_frame.push(now - before);
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
            out.rollbacks = local.stats().rollbacks;
            if frame == PROFILE_FROM {
                // Counted, and in the report, from here on.
                ctx.perf.set_profiling(true);
            }
            if frame == 399 {
                out.scopes = ctx
                    .perf
                    .report()
                    .scopes
                    .into_iter()
                    .filter(|s| s.name.starts_with("rollback/"))
                    .collect();
            }
            frame += 1;
            drop(out);
            last = Some(alloc_counts().count);
        })
        .run_headless(400, move |ctx| {
            ctx.world.track_changes::<Pos>();
            for i in 0..1000 {
                ctx.world.spawn((Pos(i as f32),));
            }
            setup(ctx);
            let mut session = GameRollback::<Pad>::new(2, 0).checksum_every(0);
            session.set_input(pad(0, 0));
            ctx.net.start_rollback(session);
        });
    Arc::try_unwrap(result).ok().unwrap().into_inner().unwrap()
}

fn scope<'a>(m: &'a Measured, name: &str) -> &'a ScopeReport {
    m.scopes
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no `{name}` scope in {:?}", m.scopes))
}

#[test]
fn a_session_allocates_nothing_per_frame_once_warm() {
    let m = measure(Game::new("alloc", 1, 1), |_| {});
    assert!(m.rollbacks > 50, "the late input rolls back often: {}", m.rollbacks);
    for name in ["rollback/save", "rollback/load"] {
        let s = scope(&m, name);
        assert!(s.calls_per_frame > 0.1, "{name} runs: {s:?}");
        assert_eq!(s.allocs_per_frame, 0.0, "{name} reuses its memory: {s:?}");
    }
    // With physics compiled in, every save also copies rapier's world, and
    // rapier allocates for every copy, even of an empty world (see the test
    // below): the whole frame then allocates.
    if cfg!(any(feature = "physics", feature = "physics3d")) {
        return;
    }
    let warm: Vec<(usize, u64)> = m
        .per_frame
        .iter()
        .enumerate()
        // Every frame after `WARM` is checked, none excluded.
        .filter(|&(i, &n)| n > 0 && i as u64 > WARM)
        .map(|(i, &n)| (i, n))
        .collect();
    assert!(warm.is_empty(), "frames that allocated once warm: {warm:?}");
}

/// With 2D physics, the world, the events and the change positions still
/// reuse their memory; the physics state is rapier's, which has no way to
/// copy into memory it already holds (every `clone` allocates), so its
/// scope's allocations are only printed.
#[cfg(feature = "physics")]
#[test]
fn with_physics_the_world_part_still_allocates_nothing() {
    use ponko::d2::{Collider, RigidBody, Transform};
    let m = measure(Game::new("alloc", 1, 1).physics(vec2(0.0, 980.0)), |ctx| {
        ctx.world.spawn((
            Transform::at(vec2(0.0, 300.0)),
            RigidBody::fixed(),
            Collider::rect(vec2(4000.0, 20.0)),
        ));
        for i in 0..100 {
            let at = vec2((i % 20) as f32 * 11.0 - 100.0, 280.0 - (i / 20) as f32 * 11.0);
            ctx.world.spawn((
                Transform::at(at),
                RigidBody::dynamic(),
                Collider::rect(vec2(10.0, 10.0)),
            ));
        }
    });
    for name in ["rollback/save", "rollback/load"] {
        let s = scope(&m, name);
        assert!(s.calls_per_frame > 0.1, "{name} runs: {s:?}");
        assert_eq!(s.allocs_per_frame, 0.0, "{name} reuses its memory: {s:?}");
    }
    for name in ["rollback/save physics", "rollback/load physics"] {
        let s = scope(&m, name);
        eprintln!(
            "{name}: {:.0} allocations, {:.3} ms per frame",
            s.allocs_per_frame, s.avg_ms
        );
    }
}
