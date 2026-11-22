//! The bevy_ecs rows of the ECS matrix: each workload of
//! `crates/ponko_bench/src/ecs_suite.rs` and `archetypes.rs`, written the
//! way bevy_ecs's documentation recommends (a kept `QueryState` iterated
//! with `iter_mut().for_each`, `par_iter_mut` on the compute task pool,
//! `spawn_batch`, `Commands` applied with `World::flush`). The tests check
//! that every workload leaves the same world as ponko_ecs's.

use bevy::ecs as bevy_ecs;
use bevy::ecs::query::{QueryState, With, Without};
use std::hint::black_box;

use crate::archetypes::{ENTITIES, Mk, bits};
use crate::ecs_suite::*;
use crate::{Bench, DT, Pos, Spr, Tag, Vel, ent};

type World = bevy_ecs::world::World;
type Entity = bevy_ecs::entity::Entity;
type BevyMove = QueryState<(&'static mut Pos, &'static Vel)>;
type BevySteer = QueryState<(&'static mut Pos, &'static mut Vel)>;

pub fn homogeneous(n: usize) -> World {
    let mut w = World::new();
    w.spawn_batch((0..n).map(ent));
    w
}

pub fn move_bevy(w: &mut World, q: &mut BevyMove, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

pub fn steer_bevy(w: &mut World, q: &mut BevySteer, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, mut v)| steer(&mut p, &mut v, dt));
}

pub fn par_steer_bevy(w: &mut World, q: &mut BevySteer, dt: f32) {
    q.par_iter_mut(w).for_each(|(mut p, mut v)| steer(&mut p, &mut v, dt));
}

/// Starts bevy's compute pool (all logical cores, its default) once.
pub fn bevy_pool() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
}

pub type BevyChurn = Churn<World, Entity>;

pub fn bevy_churn(n: usize) -> BevyChurn {
    Churn::new(n, World::new(), |w, i| w.spawn(ent(i)).id())
}

pub fn replace_bevy(c: &mut BevyChurn, k: usize) {
    c.replace(
        k,
        |w, e| {
            assert!(w.despawn(e));
        },
        |w, i| w.spawn(ent(i)).id(),
    );
}

/// `ecs/commands`: the same replacement recorded into the world's
/// `Commands` (the queue a system's `Commands` writes to), then applied.
pub fn replace_commands_bevy(c: &mut BevyChurn, k: usize) {
    let mut cmds = c.w.commands();
    for _ in 0..k {
        let e = c.live.pop_front().expect("churn world is empty");
        cmds.entity(e).despawn();
        c.live.push_back(cmds.spawn(ent(c.next)).id());
        c.next += 1;
    }
    c.w.flush();
}

pub fn add_bevy(c: &mut BevyChurn) {
    for &e in &c.live {
        c.w.entity_mut(e).insert(Tag);
    }
}
pub fn remove_bevy(c: &mut BevyChurn) {
    for &e in &c.live {
        c.w.entity_mut(e).remove::<Tag>();
    }
}
pub fn add_remove_bevy(c: &mut BevyChurn) {
    add_bevy(c);
    remove_bevy(c);
}

pub fn bevy_split(n: usize) -> World {
    let mut w = World::new();
    for i in 0..n {
        let base = ent(i);
        match split(i) {
            (true, true) => w.spawn((base.0, base.1, base.2, Tag, Boost(2.0))),
            (true, false) => w.spawn((base.0, base.1, base.2, Tag)),
            (false, true) => w.spawn((base.0, base.1, base.2, Boost(2.0))),
            (false, false) => w.spawn(base),
        };
    }
    w
}

type BevyOptional = QueryState<(&'static mut Pos, &'static Vel, Option<&'static Boost>)>;
type BevyWith = QueryState<(&'static mut Pos, &'static Vel), With<Tag>>;
type BevyWithout = QueryState<(&'static mut Pos, &'static Vel), Without<Tag>>;

pub fn optional_bevy(w: &mut World, q: &mut BevyOptional, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v, b)| boosted(&mut p, v, b, dt));
}
pub fn with_bevy(w: &mut World, q: &mut BevyWith, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}
pub fn without_bevy(w: &mut World, q: &mut BevyWithout, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

pub struct BevyQueries {
    think: QueryState<(&'static mut Ai, &'static mut Vel, &'static Pos)>,
    moves: BevyMove,
    wear: QueryState<(&'static mut Hp, &'static Vel)>,
    life: QueryState<(Entity, &'static mut Life)>,
    draw: QueryState<(&'static Pos, &'static Spr), Without<Tag>>,
}

pub type BevyGame = Game<World, Entity, BevyQueries>;

pub fn bevy_game(n: usize) -> BevyGame {
    let mut w = World::new();
    for i in 0..n {
        match role(i) {
            Role::Mob => w.spawn(mob(i)),
            Role::Projectile => w.spawn(projectile(i, start_life(i))),
            Role::Prop => w.spawn(prop(i)),
            Role::Pickup => {
                let (p, s) = prop(i);
                w.spawn((p, s, Tag))
            }
        };
    }
    let queries = BevyQueries {
        think: w.query(),
        moves: w.query(),
        wear: w.query(),
        life: w.query(),
        draw: w.query_filtered(),
    };
    Game {
        w,
        dead: Vec::new(),
        next: n,
        drawn: 0,
        queries,
    }
}

pub fn tick_bevy(g: &mut BevyGame, dt: f32) {
    let (w, q) = (&mut g.w, &mut g.queries);
    q.think
        .iter_mut(w)
        .for_each(|(mut ai, mut v, p)| think(&mut ai, &mut v, p, dt));
    move_bevy(w, &mut q.moves, dt);
    q.wear.iter_mut(w).for_each(|(mut hp, v)| wear(&mut hp, v));
    let dead = &mut g.dead;
    q.life.iter_mut(w).for_each(|(e, mut life)| {
        life.0 -= 1;
        if life.0 == 0 {
            dead.push(e);
        }
    });
    for e in g.dead.drain(..) {
        w.despawn(e);
        w.spawn(projectile(g.next, PROJECTILE_LIFE));
        g.next += 1;
    }
    g.drawn = q.draw.iter(w).fold(0, |acc, (p, s)| draw(acc, p, s));
}

/// Every row of `ecs_suite::run`, for bevy_ecs.
pub fn run(b: &mut Bench) {
    for n in SIZES {
        let l = label(n);
        let r = runs(n, 1000, 200, 100);
        b.steady(
            &format!("ecs/iter-{l}/bevy"),
            r,
            || {
                let mut w = homogeneous(n);
                let q = w.query::<(&mut Pos, &Vel)>();
                (w, q)
            },
            |(w, q)| move_bevy(w, q, black_box(DT)),
        );
        let r = runs(n, 500, 100, 30);
        b.steady(
            &format!("ecs/par-{l}/bevy-serial"),
            r,
            || {
                let mut w = homogeneous(n);
                let q = w.query::<(&mut Pos, &mut Vel)>();
                (w, q)
            },
            |(w, q)| steer_bevy(w, q, black_box(DT)),
        );
        b.steady(
            &format!("ecs/par-{l}/bevy-par"),
            r,
            || {
                bevy_pool();
                let mut w = homogeneous(n);
                let q = w.query::<(&mut Pos, &mut Vel)>();
                (w, q)
            },
            |(w, q)| par_steer_bevy(w, q, black_box(DT)),
        );
        let r = runs(n, 200, 20, 10);
        b.run(&format!("ecs/spawn-loop-{l}/bevy"), r, World::new, |w| {
            for i in 0..n {
                w.spawn(ent(i));
            }
        });
        b.run(&format!("ecs/spawn-batch-{l}/bevy"), r, World::new, |w| {
            w.spawn_batch((0..n).map(ent));
        });
        let k = (n / 10).max(1);
        let r = runs(n, 500, 50, 10);
        b.steady(
            &format!("ecs/spawn-despawn-{l}/bevy"),
            r,
            || bevy_churn(n),
            |c| replace_bevy(c, k),
        );
        b.steady(
            &format!("ecs/commands-{l}/bevy"),
            r,
            || bevy_churn(n),
            |c| replace_commands_bevy(c, k),
        );
        b.steady(
            &format!("ecs/add-remove-{l}/bevy"),
            r,
            || bevy_churn(n),
            add_remove_bevy,
        );
        let r = runs(n, 1000, 200, 100);
        b.steady(
            &format!("ecs/optional-{l}/bevy"),
            r,
            || {
                let mut w = bevy_split(n);
                let q = w.query::<(&mut Pos, &Vel, Option<&Boost>)>();
                (w, q)
            },
            |(w, q)| optional_bevy(w, q, black_box(DT)),
        );
        b.steady(
            &format!("ecs/with-{l}/bevy"),
            r,
            || {
                let mut w = bevy_split(n);
                let q = w.query_filtered::<(&mut Pos, &Vel), With<Tag>>();
                (w, q)
            },
            |(w, q)| with_bevy(w, q, black_box(DT)),
        );
        b.steady(
            &format!("ecs/without-{l}/bevy"),
            r,
            || {
                let mut w = bevy_split(n);
                let q = w.query_filtered::<(&mut Pos, &Vel), Without<Tag>>();
                (w, q)
            },
            |(w, q)| without_bevy(w, q, black_box(DT)),
        );
    }
    for n in GAME_SIZES {
        let l = label(n);
        b.steady(
            &format!("ecs/game-{l}/bevy"),
            runs(n, 500, 100, 30),
            || bevy_game(n),
            |g| tick_bevy(g, black_box(DT)),
        );
    }
}

// ---- many small archetypes (`archetypes.rs`) ----------------------------------

/// Runs `$body` with `$m` naming the marker type for bit `$k`.
macro_rules! with_marker {
    ($k:expr, |$m:ident| $body:expr) => {
        match $k {
            0 => {
                type $m = Mk<0>;
                $body
            }
            1 => {
                type $m = Mk<1>;
                $body
            }
            2 => {
                type $m = Mk<2>;
                $body
            }
            3 => {
                type $m = Mk<3>;
                $body
            }
            4 => {
                type $m = Mk<4>;
                $body
            }
            5 => {
                type $m = Mk<5>;
                $body
            }
            6 => {
                type $m = Mk<6>;
                $body
            }
            7 => {
                type $m = Mk<7>;
                $body
            }
            8 => {
                type $m = Mk<8>;
                $body
            }
            9 => {
                type $m = Mk<9>;
                $body
            }
            _ => unreachable!("only 10 markers"),
        }
    };
}

pub fn bevy_world() -> World {
    let mut w = World::new();
    for i in 0..ENTITIES {
        let mut e = w.spawn(ent(i));
        for k in bits(i) {
            with_marker!(k, |M| e.insert(M::default()));
        }
    }
    w
}

fn count_bevy(w: &mut World) -> usize {
    w.query::<(&mut Pos, &Vel)>().iter_mut(w).len()
}

/// Every bevy row of `archetypes::run`.
pub fn archetypes(b: &mut Bench) {
    const BUILDS: usize = 1000;
    b.steady(
        "archetypes-1024/bevy",
        200,
        || {
            let mut w = bevy_world();
            let q = w.query::<(&mut Pos, &Vel)>();
            (w, q)
        },
        |(w, q)| move_bevy(w, q, black_box(DT)),
    );
    b.steady(&format!("query-build-x{BUILDS}/bevy"), 100, bevy_world, |w| {
        for _ in 0..BUILDS {
            black_box(w.query::<(&mut Pos, &Vel)>());
        }
    });
    b.steady(&format!("query-build+count-x{BUILDS}/bevy"), 100, bevy_world, |w| {
        for _ in 0..BUILDS {
            black_box(count_bevy(w));
        }
    });
    b.run("query-build+count-cold/bevy", 200, bevy_world, |w| {
        black_box(count_bevy(w));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: usize = 1000;
    type Bits = Vec<(u32, u32)>;

    fn sorted(mut v: Bits) -> Bits {
        v.sort_unstable();
        v
    }
    fn ponko_pos(w: &mut ponko_ecs::World) -> Bits {
        sorted(
            w.query::<&Pos>()
                .iter()
                .map(|p| (p.x.to_bits(), p.y.to_bits()))
                .collect(),
        )
    }
    fn bevy_pos(w: &mut World) -> Bits {
        sorted(
            w.query::<&Pos>()
                .iter(w)
                .map(|p| (p.x.to_bits(), p.y.to_bits()))
                .collect(),
        )
    }

    #[test]
    fn iteration_and_steering_move_every_entity_as_ponko_does() {
        let n = 50_000;
        bevy_pool();
        let (mut p, mut ps) = (ponko_homogeneous(n), ponko_homogeneous(n));
        let mut b = homogeneous(n);
        let mut q = b.query::<(&mut Pos, &Vel)>();
        let mut bs = homogeneous(n);
        let mut qs = bs.query::<(&mut Pos, &mut Vel)>();
        let mut bp = homogeneous(n);
        let mut qp = bp.query::<(&mut Pos, &mut Vel)>();
        for _ in 0..3 {
            move_ponko(&mut p, DT);
            move_bevy(&mut b, &mut q, DT);
            steer_ponko(&mut ps, DT);
            steer_bevy(&mut bs, &mut qs, DT);
            par_steer_bevy(&mut bp, &mut qp, DT);
        }
        assert_eq!(ponko_pos(&mut p), bevy_pos(&mut b));
        assert_eq!(ponko_pos(&mut ps), bevy_pos(&mut bs));
        assert_eq!(ponko_pos(&mut ps), bevy_pos(&mut bp));
    }

    #[test]
    fn churn_commands_and_add_remove_leave_the_same_world_as_ponko() {
        let k = SMALL / 10;
        let (mut p, mut b, mut bc) = (ponko_churn(SMALL), bevy_churn(SMALL), bevy_churn(SMALL));
        for _ in 0..3 {
            replace_ponko(&mut p, k);
            replace_bevy(&mut b, k);
            replace_commands_bevy(&mut bc, k);
            add_remove_bevy(&mut b);
        }
        assert_eq!(ponko_pos(&mut p.w), bevy_pos(&mut b.w));
        assert_eq!(ponko_pos(&mut p.w), bevy_pos(&mut bc.w));
        assert!(bc.live.iter().all(|&e| bc.w.get_entity(e).is_ok()));
        assert_eq!(b.w.query::<&Tag>().iter(&b.w).len(), 0);
        add_bevy(&mut b);
        assert_eq!(b.w.query::<(&Pos, &Tag)>().iter(&b.w).len(), SMALL);
    }

    #[test]
    fn filtered_and_optional_queries_match_ponko() {
        let (mut p1, mut p2, mut p3) = (ponko_split(SMALL), ponko_split(SMALL), ponko_split(SMALL));
        let (mut b1, mut b2, mut b3) = (bevy_split(SMALL), bevy_split(SMALL), bevy_split(SMALL));
        let mut q1 = b1.query::<(&mut Pos, &Vel, Option<&Boost>)>();
        let mut q2 = b2.query_filtered::<(&mut Pos, &Vel), With<Tag>>();
        let mut q3 = b3.query_filtered::<(&mut Pos, &Vel), Without<Tag>>();
        optional_ponko(&mut p1, DT);
        with_ponko(&mut p2, DT);
        without_ponko(&mut p3, DT);
        optional_bevy(&mut b1, &mut q1, DT);
        with_bevy(&mut b2, &mut q2, DT);
        without_bevy(&mut b3, &mut q3, DT);
        assert_eq!(ponko_pos(&mut p1), bevy_pos(&mut b1));
        assert_eq!(ponko_pos(&mut p2), bevy_pos(&mut b2));
        assert_eq!(ponko_pos(&mut p3), bevy_pos(&mut b3));
    }

    #[test]
    fn game_ticks_leave_the_same_world_as_ponko() {
        let n = 2000;
        let (mut p, mut b) = (ponko_game(n), bevy_game(n));
        for _ in 0..2 * PROJECTILE_LIFE as usize + 5 {
            tick_ponko(&mut p, DT);
            tick_bevy(&mut b, DT);
        }
        assert_eq!(p.drawn, b.drawn);
        assert_eq!(p.next, b.next);
        assert_eq!(ponko_pos(&mut p.w), bevy_pos(&mut b.w));
    }

    #[test]
    fn the_archetype_world_counts_every_entity() {
        assert_eq!(count_bevy(&mut bevy_world()), ENTITIES);
    }
}
