//! Change detection in a `GameRollback`: a tick run again shows `Changed`
//! filters and `removed` exactly what they showed the first time, because a
//! restore keeps every change stamp and removal as old as it was at the
//! save, and so does what each simulation system has seen. A system that
//! counts what changed writes the counts into the world, so the world after
//! many rollbacks only matches an offline run if every re-simulated tick
//! saw the same changes.

use std::sync::{Arc, Mutex};

use ponko::net::{GameRollback, Rollback, RollbackGame};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Pad(i8);

#[derive(Component, Clone, Copy, PartialEq, Debug)]
struct Hp(i32);

/// What `watch` saw, tick by tick, folded into one number.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
struct Seen {
    changed: u64,
    removed: u64,
}

const DELAY: u32 = 2;
const ACTIVE: u64 = 90;
const TICKS: u64 = 160;
const CROWD: usize = 40;

/// Changes every 3 ticks, so the late remote input is often mispredicted;
/// nothing from `ACTIVE` on.
fn pad(p: usize, t: u64) -> Pad {
    if t >= ACTIVE {
        return Pad::default();
    }
    Pad(((t / 3 * 7 + p as u64 * 3) % 5) as i8)
}

fn inputs(ctx: &Ctx) -> [Pad; 2] {
    if let Some(i) = ctx.net.rollback_inputs::<Pad>() {
        return [i[0], i[1]];
    }
    let t = ctx.time.sim_tick();
    let at = |p| t.checked_sub(u64::from(DELAY)).map_or(Pad::default(), |from| pad(p, from));
    [at(0), at(1)]
}

/// Each player hurts the members of the crowd its input picks; player 1
/// sometimes removes one and a new one comes.
fn push(ctx: &mut Ctx) {
    let pads = inputs(ctx);
    let tick = ctx.time.sim_tick();
    let crowd: Vec<Entity> = ctx.world.query::<Entity>().filter::<With<Hp>>().iter().collect();
    for (p, pad) in pads.iter().enumerate() {
        if pad.0 == 0 {
            continue;
        }
        let target = crowd[(pad.0 as usize * 5 + p + tick as usize) % crowd.len()];
        ctx.world.get_mut::<Hp>(target).unwrap().0 -= i32::from(pad.0);
        if p == 1 && pad.0 == 4 {
            ctx.commands.despawn(target);
            ctx.commands.spawn((Hp(10),));
        }
    }
}

/// Counts the values changed and removed since it last ran.
fn watch(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick() + 1;
    let changed = ctx.world.query::<&Hp>().filter::<Changed<Hp>>().count() as u64;
    let removed = ctx.world.removed::<Hp>().len() as u64;
    ctx.world.query::<&mut Seen>().for_each(|s| {
        s.changed = s.changed.wrapping_mul(31).wrapping_add(changed * tick);
        s.removed = s.removed.wrapping_mul(31).wrapping_add(removed * tick);
    });
}

fn game() -> Game {
    Game::new("changes", 1, 1)
        .headless_frames(true)
        .sim_system(push)
        .sim_system(watch)
}

fn setup(ctx: &mut Ctx) {
    ctx.world.track_changes::<Hp>();
    ctx.world.spawn((Seen { changed: 0, removed: 0 },));
    for _ in 0..CROWD {
        ctx.world.spawn((Hp(10),));
    }
}

fn result(world: &mut World) -> (Seen, Vec<i32>) {
    let seen = *world.query::<&Seen>().iter().next().unwrap();
    let mut hp: Vec<i32> = world.query::<&Hp>().iter().map(|h| h.0).collect();
    hp.sort_unstable();
    (seen, hp)
}

/// The remote player: a bare session whose game does nothing.
struct Remote;

impl RollbackGame<Pad> for Remote {
    type State = ();
    fn save(&mut self, (): &mut ()) {}
    fn load(&mut self, (): &()) {}
    fn advance(&mut self, _: &[Pad]) {}
}

#[test]
fn ticks_run_again_see_the_changes_they_saw_the_first_time() {
    let mut offline = game().run_headless(TICKS, setup);

    let mut remote: Rollback<Pad, ()> = Rollback::new(2, 1).input_delay(DELAY).checksum_every(0);
    // The remote's packets arrive 4 frames late.
    let mut in_flight: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut frame = 0u64;
    let rollbacks = Arc::new(Mutex::new(0));
    let out = Arc::clone(&rollbacks);
    let mut online = game()
        .system(move |ctx: &mut Ctx| {
            let t = ctx.time.sim_tick();
            let local = ctx.net.rollback::<Pad>().expect("the session runs");
            local.set_input(pad(0, t));
            remote.receive(local.packet());
            while remote.frame() <= t && remote.advance(&mut Remote, pad(1, remote.frame())).ran() {}
            in_flight.push((frame + 4, remote.packet().to_vec()));
            in_flight.retain(|(due, bytes)| {
                if *due <= frame {
                    local.receive(bytes);
                }
                *due > frame
            });
            *out.lock().unwrap() = local.stats().rollbacks;
            frame += 1;
        })
        .run_headless(TICKS, |ctx| {
            setup(ctx);
            let mut session = GameRollback::<Pad>::new(2, 0).input_delay(DELAY).checksum_every(0);
            session.set_input(pad(0, 0));
            ctx.net.start_rollback(session);
        });

    assert!(*rollbacks.lock().unwrap() > 10, "the late input rolls back often");
    let (offline, online) = (result(&mut offline), result(&mut online));
    assert!(offline.0.changed != 0 && offline.0.removed != 0, "values change and go");
    assert_eq!(
        online, offline,
        "every tick run again counted the changes and removals the offline run counted"
    );
}
