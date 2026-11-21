//! The ECS benchmark matrix: the same operations in ponko_ecs, bevy_ecs and
//! hecs at 1k, 100k and 1M entities. Methodology, limits of each comparison
//! and the recorded baselines are in `ECS.md`; run it with
//! `cargo run --release -p ponko_bench -- ecs/`.
//!
//! Every result is named `ecs/<case>-<size>/<library>[-<variant>]`:
//!
//! - `iter`: `pos += vel * dt` over `(Pos, Vel, Spr)` entities in one archetype.
//! - `par`: a heavier per-entity update (steering with `sin`/`sin_cos`/
//!   `rem_euclid`), on one thread and on all of them. ponko also runs it on
//!   rayon pools of 1, 2, 4, 8 threads (up to the core count) entered with
//!   `ThreadPool::install`, which is the scaling curve, and on rayon's
//!   global pool called straight from the benchmark thread (`ponko-par`, the
//!   setup that matches bevy's); bevy runs it on its default compute pool
//!   (every core). hecs has no parallel iteration and runs the serial loop
//!   only.
//! - `spawn-loop` / `spawn-batch`: spawn `n` entities into an empty world one
//!   by one or with `spawn_batch`.
//! - `spawn-despawn`: a world of `n` keeps its size while a tenth of it is
//!   replaced per run (despawn the oldest, spawn a new one).
//! - `add-remove`: insert a marker into every one of `n` entities, then
//!   remove it again (two archetype moves per entity).
//! - `optional`: `(&mut Pos, &Vel, Option<&Boost>)` over four archetypes, half
//!   with `Boost`. `optional-par`: the same with `par_for_each` (ponko
//!   only), on every core and on a pool of one thread.
//! - `with` / `without`: `(&mut Pos, &Vel)` filtered by a marker half the
//!   entities have, over the same four archetypes.
//! - `game`: one tick of a small game: four entity layouts and five systems,
//!   including despawning expired projectiles and spawning replacements.
//!
//! Correctness of every workload is checked apart from timing by the tests
//! at the bottom (`cargo test -p ponko_bench`), on small sizes.

use bevy_ecs::component::Component;
use bevy_ecs::query::{QueryState, With, Without};
use std::collections::VecDeque;
use std::hint::black_box;

use crate::{Bench, DT, Pos, Spr, Tag, Vel, ent};

/// Entity counts of the matrix.
pub const SIZES: [usize; 3] = [1_000, 100_000, 1_000_000];
/// Entity counts of the game-like workload.
pub const GAME_SIZES: [usize; 2] = [10_000, 100_000];

#[derive(Component, Clone, Copy, Debug, PartialEq, ponko_ecs::Component)]
pub struct Boost(pub f32);
#[derive(Component, Clone, Copy, Debug, PartialEq, ponko_ecs::Component)]
pub struct Hp(pub i32);
#[derive(Component, Clone, Copy, Debug, PartialEq, ponko_ecs::Component)]
pub struct Life(pub u32);
#[derive(Component, Clone, Copy, Debug, PartialEq, ponko_ecs::Component)]
pub struct Ai {
    pub phase: f32,
}

/// `1k`, `100k`, `1m`.
pub fn label(n: usize) -> String {
    if n >= 1_000_000 && n.is_multiple_of(1_000_000) {
        format!("{}m", n / 1_000_000)
    } else if n >= 1000 && n.is_multiple_of(1000) {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

/// Timed runs for a case whose work grows with `n`.
fn runs(n: usize, small: usize, mid: usize, large: usize) -> usize {
    if n <= 10_000 {
        small
    } else if n <= 100_000 {
        mid
    } else {
        large
    }
}

// ---- homogeneous iteration ---------------------------------------------------

type BevyMove = QueryState<(&'static mut Pos, &'static Vel)>;

pub fn ponko_homogeneous(n: usize) -> ponko_ecs::World {
    let mut w = ponko_ecs::World::new();
    w.spawn_batch((0..n).map(ent));
    w
}

pub fn bevy_homogeneous(n: usize) -> bevy_ecs::world::World {
    let mut w = bevy_ecs::world::World::new();
    w.spawn_batch((0..n).map(ent));
    w
}

pub fn hecs_homogeneous(n: usize) -> hecs::World {
    let mut w = hecs::World::new();
    w.spawn_batch((0..n).map(ent));
    w
}

pub fn move_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

pub fn move_bevy(w: &mut bevy_ecs::world::World, q: &mut BevyMove, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

pub fn move_hecs(w: &mut hecs::World, dt: f32) {
    w.query_mut::<(&mut Pos, &Vel)>().into_iter().for_each(|(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

fn iteration(b: &mut Bench, n: usize) {
    let l = label(n);
    let r = runs(n, 1000, 200, 100);
    b.steady_with_bytes(
        &format!("ecs/iter-{l}/ponko"),
        r,
        || ponko_homogeneous(n),
        |w| move_ponko(w, black_box(DT)),
        |w| Some(w.allocated_bytes()),
    );
    b.steady(
        &format!("ecs/iter-{l}/bevy"),
        r,
        || {
            let mut w = bevy_homogeneous(n);
            let q = w.query::<(&mut Pos, &Vel)>();
            (w, q)
        },
        |(w, q)| move_bevy(w, q, black_box(DT)),
    );
    b.steady(
        &format!("ecs/iter-{l}/hecs"),
        r,
        || hecs_homogeneous(n),
        |w| move_hecs(w, black_box(DT)),
    );
}

// ---- parallel scaling ----------------------------------------------------------

/// Wander steering: a few transcendental calls per entity, so the loop is
/// compute-bound and can scale with cores.
#[inline]
pub fn steer(p: &mut Pos, v: &mut Vel, dt: f32) {
    let a = (p.x * 0.013 + p.y * 0.007).sin() * std::f32::consts::PI;
    let (s, c) = a.sin_cos();
    v.x = v.x * 0.9 + c * 0.1;
    v.y = v.y * 0.9 + s * 0.1;
    p.x = (p.x + v.x * dt).rem_euclid(4096.0);
    p.y = (p.y + v.y * dt).rem_euclid(4096.0);
}

type BevySteer = QueryState<(&'static mut Pos, &'static mut Vel)>;

pub fn steer_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &mut Vel)>().for_each(|(p, v)| steer(p, v, dt));
}

pub fn par_steer_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &mut Vel)>().par_for_each(|(p, v)| steer(p, v, dt));
}

pub fn steer_bevy(w: &mut bevy_ecs::world::World, q: &mut BevySteer, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, mut v)| steer(&mut p, &mut v, dt));
}

pub fn par_steer_bevy(w: &mut bevy_ecs::world::World, q: &mut BevySteer, dt: f32) {
    q.par_iter_mut(w).for_each(|(mut p, mut v)| steer(&mut p, &mut v, dt));
}

pub fn steer_hecs(w: &mut hecs::World, dt: f32) {
    w.query_mut::<(&mut Pos, &mut Vel)>()
        .into_iter()
        .for_each(|(p, v)| steer(p, v, dt));
}

/// Starts bevy's compute pool (all logical cores, its default) once.
pub fn bevy_pool() {
    bevy_tasks::ComputeTaskPool::get_or_init(bevy_tasks::TaskPool::default);
}

/// Thread counts for the ponko scaling curve: 1, 2, 4, 8 below the core
/// count, then the core count.
pub fn thread_counts() -> Vec<usize> {
    let max = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut v: Vec<usize> = [1, 2, 4, 8].into_iter().filter(|&t| t < max).collect();
    v.push(max);
    v
}

fn parallel(b: &mut Bench, n: usize) {
    let l = label(n);
    let r = runs(n, 500, 100, 30);
    b.steady(
        &format!("ecs/par-{l}/ponko-serial"),
        r,
        || ponko_homogeneous(n),
        |w| steer_ponko(w, black_box(DT)),
    );
    for t in thread_counts() {
        b.steady(
            &format!("ecs/par-{l}/ponko-t{t}"),
            r,
            || {
                let pool = rayon::ThreadPoolBuilder::new().num_threads(t).build().unwrap();
                (ponko_homogeneous(n), pool)
            },
            |(w, pool)| pool.install(|| par_steer_ponko(w, black_box(DT))),
        );
    }
    // The global pool, called from this thread with no `install`, as bevy's
    // `par_iter_mut` is called.
    b.steady(
        &format!("ecs/par-{l}/ponko-par"),
        r,
        || ponko_homogeneous(n),
        |w| par_steer_ponko(w, black_box(DT)),
    );
    b.steady(
        &format!("ecs/par-{l}/bevy-serial"),
        r,
        || {
            let mut w = bevy_homogeneous(n);
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
            let mut w = bevy_homogeneous(n);
            let q = w.query::<(&mut Pos, &mut Vel)>();
            (w, q)
        },
        |(w, q)| par_steer_bevy(w, q, black_box(DT)),
    );
    b.steady(
        &format!("ecs/par-{l}/hecs-serial"),
        r,
        || hecs_homogeneous(n),
        |w| steer_hecs(w, black_box(DT)),
    );
}

// ---- spawning and churn ----------------------------------------------------------

fn spawning(b: &mut Bench, n: usize) {
    let l = label(n);
    let r = runs(n, 200, 20, 10);
    b.run(&format!("ecs/spawn-loop-{l}/ponko"), r, ponko_ecs::World::new, |w| {
        for i in 0..n {
            w.spawn(ent(i));
        }
    });
    b.run(&format!("ecs/spawn-batch-{l}/ponko"), r, ponko_ecs::World::new, |w| {
        w.spawn_batch((0..n).map(ent))
    });
    b.run(
        &format!("ecs/spawn-loop-{l}/bevy"),
        r,
        bevy_ecs::world::World::new,
        |w| {
            for i in 0..n {
                w.spawn(ent(i));
            }
        },
    );
    b.run(
        &format!("ecs/spawn-batch-{l}/bevy"),
        r,
        bevy_ecs::world::World::new,
        |w| {
            w.spawn_batch((0..n).map(ent));
        },
    );
    b.run(&format!("ecs/spawn-loop-{l}/hecs"), r, hecs::World::new, |w| {
        for i in 0..n {
            w.spawn(ent(i));
        }
    });
    b.run(&format!("ecs/spawn-batch-{l}/hecs"), r, hecs::World::new, |w| {
        w.spawn_batch((0..n).map(ent));
    });
}

/// A world that keeps `n` live entities, oldest first, and the index of the
/// next one to spawn.
pub struct Churn<W, E> {
    pub w: W,
    pub live: VecDeque<E>,
    pub next: usize,
}

impl<W, E> Churn<W, E> {
    fn new(n: usize, mut w: W, mut spawn: impl FnMut(&mut W, usize) -> E) -> Self {
        let mut live = VecDeque::with_capacity(n);
        for i in 0..n {
            live.push_back(spawn(&mut w, i));
        }
        Churn { w, live, next: n }
    }

    /// Replaces the `k` oldest entities with new ones.
    fn replace(&mut self, k: usize, mut despawn: impl FnMut(&mut W, E), mut spawn: impl FnMut(&mut W, usize) -> E) {
        for _ in 0..k {
            let e = self.live.pop_front().expect("churn world is empty");
            despawn(&mut self.w, e);
            self.live.push_back(spawn(&mut self.w, self.next));
            self.next += 1;
        }
    }
}

pub type PonkoChurn = Churn<ponko_ecs::World, ponko_ecs::Entity>;
pub type BevyChurn = Churn<bevy_ecs::world::World, bevy_ecs::entity::Entity>;
pub type HecsChurn = Churn<hecs::World, hecs::Entity>;

pub fn ponko_churn(n: usize) -> PonkoChurn {
    Churn::new(n, ponko_ecs::World::new(), |w, i| w.spawn(ent(i)))
}
pub fn bevy_churn(n: usize) -> BevyChurn {
    Churn::new(n, bevy_ecs::world::World::new(), |w, i| w.spawn(ent(i)).id())
}
pub fn hecs_churn(n: usize) -> HecsChurn {
    Churn::new(n, hecs::World::new(), |w, i| w.spawn(ent(i)))
}

pub fn replace_ponko(c: &mut PonkoChurn, k: usize) {
    c.replace(
        k,
        |w, e| {
            assert!(w.despawn(e));
        },
        |w, i| w.spawn(ent(i)),
    );
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
pub fn replace_hecs(c: &mut HecsChurn, k: usize) {
    c.replace(k, |w, e| w.despawn(e).unwrap(), |w, i| w.spawn(ent(i)));
}

// Each half is split out so the tests can check the world between them.
pub fn add_ponko(c: &mut PonkoChurn) {
    for &e in &c.live {
        assert!(c.w.insert(e, Tag));
    }
}
pub fn remove_ponko(c: &mut PonkoChurn) {
    for &e in &c.live {
        assert!(c.w.remove::<Tag>(e).is_some());
    }
}
pub fn add_remove_ponko(c: &mut PonkoChurn) {
    add_ponko(c);
    remove_ponko(c);
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
pub fn add_hecs(c: &mut HecsChurn) {
    for &e in &c.live {
        c.w.insert_one(e, Tag).unwrap();
    }
}
pub fn remove_hecs(c: &mut HecsChurn) {
    for &e in &c.live {
        c.w.remove_one::<Tag>(e).unwrap();
    }
}
pub fn add_remove_hecs(c: &mut HecsChurn) {
    add_hecs(c);
    remove_hecs(c);
}

fn churn(b: &mut Bench, n: usize) {
    let l = label(n);
    let k = (n / 10).max(1);
    let r = runs(n, 500, 50, 10);
    b.steady_with_bytes(
        &format!("ecs/spawn-despawn-{l}/ponko"),
        r,
        || ponko_churn(n),
        |c| replace_ponko(c, k),
        |c| Some(c.w.allocated_bytes()),
    );
    b.steady(
        &format!("ecs/spawn-despawn-{l}/bevy"),
        r,
        || bevy_churn(n),
        |c| replace_bevy(c, k),
    );
    b.steady(
        &format!("ecs/spawn-despawn-{l}/hecs"),
        r,
        || hecs_churn(n),
        |c| replace_hecs(c, k),
    );
    b.steady_with_bytes(
        &format!("ecs/add-remove-{l}/ponko"),
        r,
        || ponko_churn(n),
        add_remove_ponko,
        |c| Some(c.w.allocated_bytes()),
    );
    b.steady(
        &format!("ecs/add-remove-{l}/bevy"),
        r,
        || bevy_churn(n),
        add_remove_bevy,
    );
    b.steady(
        &format!("ecs/add-remove-{l}/hecs"),
        r,
        || hecs_churn(n),
        add_remove_hecs,
    );
}

// ---- optional and filtered queries ---------------------------------------------

/// Entity `i` has `Tag` when `i % 2 == 0` and `Boost` when `i % 4 < 2`: four
/// archetypes of equal size, interleaved by spawn order.
fn split(i: usize) -> (bool, bool) {
    (i.is_multiple_of(2), i % 4 < 2)
}

pub fn ponko_split(n: usize) -> ponko_ecs::World {
    let mut w = ponko_ecs::World::new();
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

pub fn bevy_split(n: usize) -> bevy_ecs::world::World {
    let mut w = bevy_ecs::world::World::new();
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

pub fn hecs_split(n: usize) -> hecs::World {
    let mut w = hecs::World::new();
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

#[inline]
fn boosted(p: &mut Pos, v: &Vel, boost: Option<&Boost>, dt: f32) {
    let s = boost.map_or(1.0, |b| b.0) * dt;
    p.x += v.x * s;
    p.y += v.y * s;
}

type BevyOptional = QueryState<(&'static mut Pos, &'static Vel, Option<&'static Boost>)>;
type BevyWith = QueryState<(&'static mut Pos, &'static Vel), With<Tag>>;
type BevyWithout = QueryState<(&'static mut Pos, &'static Vel), Without<Tag>>;

pub fn optional_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &Vel, Option<&Boost>)>()
        .for_each(|(p, v, b)| boosted(p, v, b, dt));
}
/// [`optional_ponko`] with `par_for_each`.
pub fn optional_par_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &Vel, Option<&Boost>)>()
        .par_for_each(|(p, v, b)| boosted(p, v, b, dt));
}
pub fn optional_bevy(w: &mut bevy_ecs::world::World, q: &mut BevyOptional, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v, b)| boosted(&mut p, v, b, dt));
}
pub fn optional_hecs(w: &mut hecs::World, dt: f32) {
    w.query_mut::<(&mut Pos, &Vel, Option<&Boost>)>()
        .into_iter()
        .for_each(|(p, v, b)| boosted(p, v, b, dt));
}

pub fn with_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &Vel)>()
        .filter::<ponko_ecs::With<Tag>>()
        .for_each(|(p, v)| {
            p.x += v.x * dt;
            p.y += v.y * dt;
        });
}
pub fn with_bevy(w: &mut bevy_ecs::world::World, q: &mut BevyWith, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}
pub fn with_hecs(w: &mut hecs::World, dt: f32) {
    w.query_mut::<(&mut Pos, &Vel)>()
        .with::<&Tag>()
        .into_iter()
        .for_each(|(p, v)| {
            p.x += v.x * dt;
            p.y += v.y * dt;
        });
}

pub fn without_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &Vel)>()
        .filter::<ponko_ecs::Without<Tag>>()
        .for_each(|(p, v)| {
            p.x += v.x * dt;
            p.y += v.y * dt;
        });
}
pub fn without_bevy(w: &mut bevy_ecs::world::World, q: &mut BevyWithout, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}
pub fn without_hecs(w: &mut hecs::World, dt: f32) {
    w.query_mut::<(&mut Pos, &Vel)>()
        .without::<&Tag>()
        .into_iter()
        .for_each(|(p, v)| {
            p.x += v.x * dt;
            p.y += v.y * dt;
        });
}

fn queries(b: &mut Bench, n: usize) {
    let l = label(n);
    let r = runs(n, 1000, 200, 100);
    b.steady(
        &format!("ecs/optional-{l}/ponko"),
        r,
        || ponko_split(n),
        |w| optional_ponko(w, black_box(DT)),
    );
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
        &format!("ecs/optional-{l}/hecs"),
        r,
        || hecs_split(n),
        |w| optional_hecs(w, black_box(DT)),
    );
    // The same loop with `par_for_each`: on every core, and on a pool of
    // one thread, which times the loop's own code without the other cores.
    b.steady(
        &format!("ecs/optional-par-{l}/ponko"),
        r,
        || ponko_split(n),
        |w| optional_par_ponko(w, black_box(DT)),
    );
    b.steady(
        &format!("ecs/optional-par-{l}/ponko-1-thread"),
        r,
        || {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
            (ponko_split(n), pool)
        },
        |(w, pool)| pool.install(|| optional_par_ponko(w, black_box(DT))),
    );
    b.steady(
        &format!("ecs/with-{l}/ponko"),
        r,
        || ponko_split(n),
        |w| with_ponko(w, black_box(DT)),
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
        &format!("ecs/with-{l}/hecs"),
        r,
        || hecs_split(n),
        |w| with_hecs(w, black_box(DT)),
    );
    b.steady(
        &format!("ecs/without-{l}/ponko"),
        r,
        || ponko_split(n),
        |w| without_ponko(w, black_box(DT)),
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
    b.steady(
        &format!("ecs/without-{l}/hecs"),
        r,
        || hecs_split(n),
        |w| without_hecs(w, black_box(DT)),
    );
}

// ---- a mixed, game-like workload ---------------------------------------------------

/// What entity `i` of the starting world is (out of every 20): 10 mobs
/// `(Pos, Vel, Hp, Ai)`, 4 projectiles `(Pos, Vel, Life)`, 5 props
/// `(Pos, Spr)` and 1 pickup `(Pos, Spr, Tag)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Mob,
    Projectile,
    Prop,
    Pickup,
}

pub fn role(i: usize) -> Role {
    match i % 20 {
        0..=9 => Role::Mob,
        10..=13 => Role::Projectile,
        14..=18 => Role::Prop,
        _ => Role::Pickup,
    }
}

/// Ticks a newly spawned projectile lives.
pub const PROJECTILE_LIFE: u32 = 60;

fn mob(i: usize) -> (Pos, Vel, Hp, Ai) {
    (
        Pos {
            x: i as f32,
            y: (i % 97) as f32,
        },
        Vel { x: 1.0, y: 0.5 },
        Hp(50 + (i % 50) as i32),
        Ai {
            phase: (i % 360) as f32 * 0.01,
        },
    )
}

/// Projectile number `c`; the starting ones have staggered lifetimes so a
/// steady trickle expires every tick.
fn projectile(c: usize, life: u32) -> (Pos, Vel, Life) {
    (
        Pos {
            x: (c % 1000) as f32,
            y: (c % 7) as f32,
        },
        Vel { x: 3.0, y: -1.0 },
        Life(life),
    )
}

fn start_life(i: usize) -> u32 {
    1 + (i % PROJECTILE_LIFE as usize) as u32
}

fn prop(i: usize) -> (Pos, Spr) {
    (
        Pos {
            x: (i % 512) as f32,
            y: (i / 512) as f32,
        },
        Spr {
            _tex: i as u32 % 64,
            _tint: [1.0; 4],
        },
    )
}

#[inline]
fn think(ai: &mut Ai, v: &mut Vel, p: &Pos, dt: f32) {
    ai.phase += dt;
    let (s, c) = (ai.phase + p.x * 0.001).sin_cos();
    v.x = c * 2.0;
    v.y = s * 2.0;
}

#[inline]
fn wear(hp: &mut Hp, v: &Vel) {
    hp.0 -= (v.x > 0.0) as i32;
    if hp.0 <= 0 {
        hp.0 = 100;
    }
}

/// Folds a drawn sprite into the order-independent frame checksum.
#[inline]
fn draw(acc: u64, p: &Pos, s: &Spr) -> u64 {
    acc.wrapping_add(u64::from(s._tex) ^ u64::from(p.x.to_bits()))
}

/// One game world plus the state its systems keep between ticks.
pub struct Game<W, E, Q> {
    pub w: W,
    /// Expired projectiles of this tick, reused so a tick doesn't allocate.
    dead: Vec<E>,
    /// Number of the next projectile to spawn.
    next: usize,
    /// The draw system's checksum of the last tick.
    pub drawn: u64,
    queries: Q,
}

pub type PonkoGame = Game<ponko_ecs::World, ponko_ecs::Entity, ()>;

pub fn ponko_game(n: usize) -> PonkoGame {
    let mut w = ponko_ecs::World::new();
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
    Game {
        w,
        dead: Vec::new(),
        next: n,
        drawn: 0,
        queries: (),
    }
}

pub fn tick_ponko(g: &mut PonkoGame, dt: f32) {
    let w = &mut g.w;
    w.query::<(&mut Ai, &mut Vel, &Pos)>()
        .for_each(|(ai, v, p)| think(ai, v, p, dt));
    w.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
    w.query::<(&mut Hp, &Vel)>().for_each(|(hp, v)| wear(hp, v));
    let dead = &mut g.dead;
    w.query::<(ponko_ecs::Entity, &mut Life)>().for_each(|(e, life)| {
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
    let mut acc = 0;
    w.query::<(&Pos, &Spr)>()
        .filter::<ponko_ecs::Without<Tag>>()
        .for_each(|(p, s)| acc = draw(acc, p, s));
    g.drawn = acc;
}

pub struct BevyQueries {
    think: QueryState<(&'static mut Ai, &'static mut Vel, &'static Pos)>,
    moves: BevyMove,
    wear: QueryState<(&'static mut Hp, &'static Vel)>,
    life: QueryState<(bevy_ecs::entity::Entity, &'static mut Life)>,
    draw: QueryState<(&'static Pos, &'static Spr), Without<Tag>>,
}

pub type BevyGame = Game<bevy_ecs::world::World, bevy_ecs::entity::Entity, BevyQueries>;

pub fn bevy_game(n: usize) -> BevyGame {
    let mut w = bevy_ecs::world::World::new();
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

pub type HecsGame = Game<hecs::World, hecs::Entity, ()>;

pub fn hecs_game(n: usize) -> HecsGame {
    let mut w = hecs::World::new();
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
    Game {
        w,
        dead: Vec::new(),
        next: n,
        drawn: 0,
        queries: (),
    }
}

pub fn tick_hecs(g: &mut HecsGame, dt: f32) {
    let w = &mut g.w;
    w.query_mut::<(&mut Ai, &mut Vel, &Pos)>()
        .into_iter()
        .for_each(|(ai, v, p)| think(ai, v, p, dt));
    move_hecs(w, dt);
    w.query_mut::<(&mut Hp, &Vel)>()
        .into_iter()
        .for_each(|(hp, v)| wear(hp, v));
    let dead = &mut g.dead;
    w.query_mut::<(hecs::Entity, &mut Life)>()
        .into_iter()
        .for_each(|(e, life)| {
            life.0 -= 1;
            if life.0 == 0 {
                dead.push(e);
            }
        });
    for e in g.dead.drain(..) {
        w.despawn(e).unwrap();
        w.spawn(projectile(g.next, PROJECTILE_LIFE));
        g.next += 1;
    }
    g.drawn = w
        .query_mut::<(&Pos, &Spr)>()
        .without::<&Tag>()
        .into_iter()
        .fold(0, |acc, (p, s)| draw(acc, p, s));
}

fn game(b: &mut Bench, n: usize) {
    let l = label(n);
    let r = runs(n, 500, 100, 30);
    b.steady_with_bytes(
        &format!("ecs/game-{l}/ponko"),
        r,
        || ponko_game(n),
        |g| tick_ponko(g, black_box(DT)),
        |g| Some(g.w.allocated_bytes()),
    );
    b.steady(
        &format!("ecs/game-{l}/bevy"),
        r,
        || bevy_game(n),
        |g| tick_bevy(g, black_box(DT)),
    );
    b.steady(
        &format!("ecs/game-{l}/hecs"),
        r,
        || hecs_game(n),
        |g| tick_hecs(g, black_box(DT)),
    );
}

pub fn run(b: &mut Bench) {
    for n in SIZES {
        iteration(b, n);
        parallel(b, n);
        spawning(b, n);
        churn(b, n);
        queries(b, n);
    }
    for n in GAME_SIZES {
        game(b, n);
    }
}

// Correctness of the workloads, checked apart from their timing: every
// library must build the same world and leave it in the same state.
#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: usize = 1000;

    type Bits = Vec<(u32, u32)>;

    fn bits(p: &Pos) -> (u32, u32) {
        (p.x.to_bits(), p.y.to_bits())
    }

    fn sorted(mut v: Bits) -> Bits {
        v.sort_unstable();
        v
    }

    fn ponko_pos(w: &mut ponko_ecs::World) -> Bits {
        sorted(w.query::<&Pos>().iter().map(bits).collect())
    }
    fn bevy_pos(w: &mut bevy_ecs::world::World) -> Bits {
        sorted(w.query::<&Pos>().iter(w).map(bits).collect())
    }
    fn hecs_pos(w: &mut hecs::World) -> Bits {
        sorted(w.query_mut::<&Pos>().into_iter().map(bits).collect())
    }

    /// Entities whose `y` moved off its starting value (every workload
    /// starts all entities at `y == 0` with a non-zero `vel.y`).
    fn moved(v: &Bits) -> usize {
        v.iter().filter(|&&(_, y)| y != 0.0f32.to_bits()).count()
    }

    #[test]
    fn sizes_are_labelled() {
        assert_eq!(SIZES.map(label), ["1k", "100k", "1m"]);
        assert_eq!(label(10_000), "10k");
        assert_eq!(label(1500), "1500");
    }

    #[test]
    fn thread_counts_end_at_the_core_count() {
        let t = thread_counts();
        let max = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(t.last(), Some(&max));
        assert!(t.windows(2).all(|p| p[0] < p[1]));
        assert_eq!(t[0], 1);
    }

    #[test]
    fn homogeneous_iteration_moves_every_entity_the_same() {
        let mut pw = ponko_homogeneous(SMALL);
        let mut bw = bevy_homogeneous(SMALL);
        let mut q = bw.query::<(&mut Pos, &Vel)>();
        let mut hw = hecs_homogeneous(SMALL);
        for _ in 0..3 {
            move_ponko(&mut pw, DT);
            move_bevy(&mut bw, &mut q, DT);
            move_hecs(&mut hw, DT);
        }
        let p = ponko_pos(&mut pw);
        assert_eq!(p.len(), SMALL);
        assert_eq!(moved(&p), SMALL);
        assert_eq!(p, bevy_pos(&mut bw));
        assert_eq!(p, hecs_pos(&mut hw));
    }

    #[test]
    fn parallel_steering_matches_the_serial_loop_in_every_library() {
        // Big enough for ponko to really split the work over threads.
        let n = 50_000;
        bevy_pool();
        let mut serial = ponko_homogeneous(n);
        let mut par = ponko_homogeneous(n);
        let mut global = ponko_homogeneous(n);
        let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        let mut bevy_serial = bevy_homogeneous(n);
        let mut qs = bevy_serial.query::<(&mut Pos, &mut Vel)>();
        let mut bevy_par = bevy_homogeneous(n);
        let mut qp = bevy_par.query::<(&mut Pos, &mut Vel)>();
        let mut hw = hecs_homogeneous(n);
        for _ in 0..3 {
            steer_ponko(&mut serial, DT);
            pool.install(|| par_steer_ponko(&mut par, DT));
            par_steer_ponko(&mut global, DT);
            steer_bevy(&mut bevy_serial, &mut qs, DT);
            par_steer_bevy(&mut bevy_par, &mut qp, DT);
            steer_hecs(&mut hw, DT);
        }
        let expected = ponko_pos(&mut serial);
        assert_eq!(moved(&expected), n);
        assert_eq!(expected, ponko_pos(&mut par));
        assert_eq!(expected, ponko_pos(&mut global));
        assert_eq!(expected, bevy_pos(&mut bevy_serial));
        assert_eq!(expected, bevy_pos(&mut bevy_par));
        assert_eq!(expected, hecs_pos(&mut hw));
    }

    #[test]
    fn batch_and_loop_spawning_build_the_same_world() {
        let mut lp = ponko_ecs::World::new();
        for i in 0..SMALL {
            lp.spawn(ent(i));
        }
        let mut batch = ponko_homogeneous(SMALL);
        assert_eq!(lp.len(), SMALL);
        assert_eq!(ponko_pos(&mut lp), ponko_pos(&mut batch));
        assert_eq!(ponko_pos(&mut batch), bevy_pos(&mut bevy_homogeneous(SMALL)));
        assert_eq!(ponko_pos(&mut batch), hecs_pos(&mut hecs_homogeneous(SMALL)));
    }

    #[test]
    fn churn_keeps_the_size_and_replaces_the_oldest_entities() {
        let k = SMALL / 10;
        let mut p = ponko_churn(SMALL);
        let mut b = bevy_churn(SMALL);
        let mut h = hecs_churn(SMALL);
        let first_p: Vec<_> = p.live.iter().take(2 * k).copied().collect();
        let first_h: Vec<_> = h.live.iter().take(2 * k).copied().collect();
        for _ in 0..2 {
            replace_ponko(&mut p, k);
            replace_bevy(&mut b, k);
            replace_hecs(&mut h, k);
        }
        assert_eq!(p.w.len(), SMALL);
        assert_eq!(h.w.len() as usize, SMALL);
        assert_eq!(b.w.query::<&Pos>().iter(&b.w).len(), SMALL);
        assert!(first_p.iter().all(|&e| !p.w.is_alive(e)));
        assert!(first_h.iter().all(|&e| !h.w.contains(e)));
        assert!(p.live.iter().all(|&e| p.w.is_alive(e)));
        // The live set is entities 2k..SMALL + 2k, by their starting x.
        let expected: Vec<u32> = (2 * k..SMALL + 2 * k).map(|i| (i as f32).to_bits()).collect();
        let xs = |v: Bits| {
            let mut x: Vec<u32> = v.into_iter().map(|(x, _)| x).collect();
            x.sort_unstable_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)));
            x
        };
        assert_eq!(xs(ponko_pos(&mut p.w)), expected);
        assert_eq!(xs(bevy_pos(&mut b.w)), expected);
        assert_eq!(xs(hecs_pos(&mut h.w)), expected);
    }

    #[test]
    fn add_remove_leaves_every_entity_without_the_marker() {
        let mut p = ponko_churn(SMALL);
        let mut b = bevy_churn(SMALL);
        let mut h = hecs_churn(SMALL);
        // Each run moves every entity there and back, so repeated runs work.
        for _ in 0..2 {
            add_remove_ponko(&mut p);
            add_remove_bevy(&mut b);
            add_remove_hecs(&mut h);
        }
        assert_eq!(p.w.query::<&Tag>().count(), 0);
        assert_eq!(p.w.query::<&Pos>().count(), SMALL);
        assert_eq!(b.w.query::<&Tag>().iter(&b.w).len(), 0);
        assert_eq!(b.w.query::<&Pos>().iter(&b.w).len(), SMALL);
        assert_eq!(h.w.query_mut::<&Tag>().into_iter().count(), 0);
        assert_eq!(h.w.query_mut::<&Pos>().into_iter().count(), SMALL);
        assert!(p.live.iter().all(|&e| p.w.get::<Pos>(e).is_some()));
    }

    #[test]
    fn the_add_half_gives_every_entity_the_marker() {
        let mut p = ponko_churn(SMALL);
        let mut b = bevy_churn(SMALL);
        let mut h = hecs_churn(SMALL);
        add_ponko(&mut p);
        add_bevy(&mut b);
        add_hecs(&mut h);
        assert_eq!(p.w.query::<(&Pos, &Tag)>().count(), SMALL);
        assert_eq!(b.w.query::<(&Pos, &Tag)>().iter(&b.w).len(), SMALL);
        assert_eq!(h.w.query_mut::<(&Pos, &Tag)>().into_iter().count(), SMALL);
        remove_ponko(&mut p);
        remove_bevy(&mut b);
        remove_hecs(&mut h);
        assert_eq!(p.w.query::<&Tag>().count(), 0);
        assert_eq!(b.w.query::<&Tag>().iter(&b.w).len(), 0);
        assert_eq!(h.w.query_mut::<&Tag>().into_iter().count(), 0);
    }

    #[test]
    fn split_worlds_have_four_equal_archetypes() {
        let mut w = ponko_split(SMALL);
        assert_eq!(w.query::<&Tag>().count(), SMALL / 2);
        assert_eq!(w.query::<&Boost>().count(), SMALL / 2);
        assert_eq!(w.query::<(&Tag, &Boost)>().count(), SMALL / 4);
        let mut b = bevy_split(SMALL);
        assert_eq!(b.query::<(&Tag, &Boost)>().iter(&b).len(), SMALL / 4);
        let mut h = hecs_split(SMALL);
        assert_eq!(h.query_mut::<(&Tag, &Boost)>().into_iter().count(), SMALL / 4);
    }

    #[test]
    fn optional_queries_visit_every_entity_and_boost_half() {
        let mut pw = ponko_split(SMALL);
        let mut bw = bevy_split(SMALL);
        let mut q = bw.query::<(&mut Pos, &Vel, Option<&Boost>)>();
        let mut hw = hecs_split(SMALL);
        optional_ponko(&mut pw, DT);
        optional_bevy(&mut bw, &mut q, DT);
        optional_hecs(&mut hw, DT);
        let mut par = ponko_split(SMALL);
        optional_par_ponko(&mut par, DT);
        assert_eq!(ponko_pos(&mut par), ponko_pos(&mut pw));
        let p = ponko_pos(&mut pw);
        assert_eq!(moved(&p), SMALL);
        let boosted_y = (0.5 * (2.0 * DT)).to_bits();
        assert_eq!(p.iter().filter(|&&(_, y)| y == boosted_y).count(), SMALL / 2);
        assert_eq!(p, bevy_pos(&mut bw));
        assert_eq!(p, hecs_pos(&mut hw));
    }

    #[test]
    fn filtered_queries_visit_exactly_the_tagged_or_untagged_half() {
        let mut pw = ponko_split(SMALL);
        let mut bw = bevy_split(SMALL);
        let mut q = bw.query_filtered::<(&mut Pos, &Vel), With<Tag>>();
        let mut hw = hecs_split(SMALL);
        with_ponko(&mut pw, DT);
        with_bevy(&mut bw, &mut q, DT);
        with_hecs(&mut hw, DT);
        let p = ponko_pos(&mut pw);
        assert_eq!(moved(&p), SMALL / 2);
        assert_eq!(p, bevy_pos(&mut bw));
        assert_eq!(p, hecs_pos(&mut hw));
        // The tagged entities are the even ones.
        assert_eq!(pw.query::<(&Pos, &Tag)>().iter().filter(|(p, _)| p.y == 0.0).count(), 0);

        let mut pw = ponko_split(SMALL);
        let mut bw = bevy_split(SMALL);
        let mut q = bw.query_filtered::<(&mut Pos, &Vel), Without<Tag>>();
        let mut hw = hecs_split(SMALL);
        without_ponko(&mut pw, DT);
        without_bevy(&mut bw, &mut q, DT);
        without_hecs(&mut hw, DT);
        let p = ponko_pos(&mut pw);
        assert_eq!(moved(&p), SMALL / 2);
        assert_eq!(p, bevy_pos(&mut bw));
        assert_eq!(p, hecs_pos(&mut hw));
        assert_eq!(pw.query::<(&Pos, &Tag)>().iter().filter(|(p, _)| p.y != 0.0).count(), 0);
    }

    /// Everything a game tick leaves behind, independent of storage order.
    #[derive(Debug, PartialEq)]
    struct GameState {
        pos: Bits,
        hp: Vec<i32>,
        life: Vec<u32>,
        drawn: u64,
        next: usize,
        entities: usize,
    }

    fn ponko_state(g: &mut PonkoGame) -> GameState {
        let mut hp: Vec<_> = g.w.query::<&Hp>().iter().map(|h| h.0).collect();
        let mut life: Vec<_> = g.w.query::<&Life>().iter().map(|l| l.0).collect();
        hp.sort_unstable();
        life.sort_unstable();
        GameState {
            pos: ponko_pos(&mut g.w),
            hp,
            life,
            drawn: g.drawn,
            next: g.next,
            entities: g.w.len(),
        }
    }

    fn bevy_state(g: &mut BevyGame) -> GameState {
        let w = &mut g.w;
        let mut hp: Vec<_> = w.query::<&Hp>().iter(w).map(|h| h.0).collect();
        let mut life: Vec<_> = w.query::<&Life>().iter(w).map(|l| l.0).collect();
        hp.sort_unstable();
        life.sort_unstable();
        let entities = w.query::<&Pos>().iter(w).len();
        GameState {
            pos: bevy_pos(w),
            hp,
            life,
            drawn: g.drawn,
            next: g.next,
            entities,
        }
    }

    fn hecs_state(g: &mut HecsGame) -> GameState {
        let w = &mut g.w;
        let mut hp: Vec<_> = w.query_mut::<&Hp>().into_iter().map(|h| h.0).collect();
        let mut life: Vec<_> = w.query_mut::<&Life>().into_iter().map(|l| l.0).collect();
        hp.sort_unstable();
        life.sort_unstable();
        GameState {
            pos: hecs_pos(w),
            hp,
            life,
            drawn: g.drawn,
            next: g.next,
            entities: w.len() as usize,
        }
    }

    #[test]
    fn game_ticks_leave_the_same_world_in_every_library() {
        let n = 2000;
        let ticks = 2 * PROJECTILE_LIFE as usize + 5;
        let mut p = ponko_game(n);
        let mut b = bevy_game(n);
        let mut h = hecs_game(n);
        for _ in 0..ticks {
            tick_ponko(&mut p, DT);
            tick_bevy(&mut b, DT);
            tick_hecs(&mut h, DT);
        }
        let state = ponko_state(&mut p);
        // The world keeps its size and layout while projectiles turn over.
        assert_eq!(state.entities, n);
        assert_eq!(state.life.len(), n / 5);
        assert!(state.next > n + n / 5, "every starting projectile expired");
        assert!(state.life.iter().all(|&l| (1..=PROJECTILE_LIFE).contains(&l)));
        assert_eq!(state.hp.len(), n / 2);
        assert_ne!(state.drawn, 0);
        assert_eq!(state, bevy_state(&mut b));
        assert_eq!(state, hecs_state(&mut h));
    }

    #[test]
    fn steady_game_ticks_reuse_their_buffers() {
        let mut g = ponko_game(2000);
        for _ in 0..PROJECTILE_LIFE {
            tick_ponko(&mut g, DT);
        }
        let cap = g.dead.capacity();
        for _ in 0..PROJECTILE_LIFE {
            tick_ponko(&mut g, DT);
        }
        assert_eq!(g.dead.capacity(), cap);
        assert!(g.dead.is_empty());
    }
}
