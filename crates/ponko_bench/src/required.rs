//! Required components: the ECS matrix's ponko rows again, with requirements.
//!
//! - `ponko-requires-elsewhere`: a component the workload never uses (`Hp`)
//!   requires another. The requirement is that this costs nothing: compare
//!   with the `ponko` row.
//! - `ponko-required`: `Pos` requires `Vel` (the value `ent` gives it), and
//!   the workloads spawn `(Pos, Spr)`: every spawn fills in a `Vel`. The
//!   entities end up as the `ponko` row's, so the difference is what
//!   leaving a component to its requirement costs.
//!
//! `crates/ponko_bench/REQUIRED.md` has the method and the numbers;
//! `cargo run --release -p ponko_bench -- 'ecs/'` runs them with the matrix.

use std::hint::black_box;

use ponko_ecs::World;

use crate::ecs_suite::{Hp, Life, PonkoChurn, add_remove_ponko, label, move_ponko, replace_ponko};
use crate::{Bench, DT, Pos, Spr, Vel, ent};

/// `Hp`, which the matrix's workloads never use, requires a `Life`.
pub fn requires_elsewhere(mut w: World) -> World {
    w.require::<Hp, _>(|| Life(1));
    w
}

/// `Pos` requires the `Vel` that `ent` gives.
pub fn pos_requires_vel(mut w: World) -> World {
    w.require::<Pos, _>(|| Vel { x: 1.0, y: 0.5 });
    w
}

/// `ent(i)` without its `Vel`, which the requirement fills in.
fn ent_required(i: usize) -> (Pos, Spr) {
    (Pos { x: i as f32, y: 0.0 }, Spr::default())
}

fn churn(n: usize, w: World, spawn: fn(&mut World, usize) -> ponko_ecs::Entity) -> PonkoChurn {
    let mut c = crate::ecs_suite::ponko_churn(0);
    c.w = w;
    for i in 0..n {
        let e = spawn(&mut c.w, i);
        c.live.push_back(e);
    }
    c.next = n;
    c
}

fn spawn_plain(w: &mut World, i: usize) -> ponko_ecs::Entity {
    w.spawn(ent(i))
}

fn spawn_required(w: &mut World, i: usize) -> ponko_ecs::Entity {
    w.spawn(ent_required(i))
}

/// `replace_ponko` with the bundle left to its requirement.
fn replace_required(c: &mut PonkoChurn, k: usize) {
    for _ in 0..k {
        let e = c.live.pop_front().expect("churn world is empty");
        assert!(c.w.despawn(e));
        let e = spawn_required(&mut c.w, c.next);
        c.live.push_back(e);
        c.next += 1;
    }
}

fn runs(n: usize, small: usize, mid: usize, large: usize) -> usize {
    if n <= 10_000 {
        small
    } else if n <= 100_000 {
        mid
    } else {
        large
    }
}

/// The requirement rows, for every size of the matrix.
pub fn run(b: &mut Bench) {
    for n in crate::ecs_suite::SIZES {
        let l = label(n);
        let v = "ponko-requires-elsewhere";
        let setup = requires_elsewhere;
        b.steady(
            &format!("ecs/iter-{l}/{v}"),
            runs(n, 1000, 200, 100),
            || {
                let mut w = setup(World::new());
                w.spawn_batch((0..n).map(ent));
                w
            },
            |w| move_ponko(w, black_box(DT)),
        );
        let r = runs(n, 200, 20, 10);
        b.run(
            &format!("ecs/spawn-loop-{l}/{v}"),
            r,
            || setup(World::new()),
            |w| {
                for i in 0..n {
                    w.spawn(ent(i));
                }
            },
        );
        b.run(
            &format!("ecs/spawn-batch-{l}/{v}"),
            r,
            || setup(World::new()),
            |w| w.spawn_batch((0..n).map(ent)),
        );
        let k = (n / 10).max(1);
        let rc = runs(n, 500, 50, 10);
        b.steady(
            &format!("ecs/spawn-despawn-{l}/{v}"),
            rc,
            || churn(n, setup(World::new()), spawn_plain),
            |c| replace_ponko(c, k),
        );
        b.steady(
            &format!("ecs/add-remove-{l}/{v}"),
            rc,
            || churn(n, setup(World::new()), spawn_plain),
            add_remove_ponko,
        );

        let v = "ponko-required";
        b.run(
            &format!("ecs/spawn-loop-{l}/{v}"),
            r,
            || pos_requires_vel(World::new()),
            |w| {
                for i in 0..n {
                    w.spawn(ent_required(i));
                }
            },
        );
        b.run(
            &format!("ecs/spawn-batch-{l}/{v}"),
            r,
            || pos_requires_vel(World::new()),
            |w| w.spawn_batch((0..n).map(ent_required)),
        );
        b.steady(
            &format!("ecs/spawn-despawn-{l}/{v}"),
            rc,
            || churn(n, pos_requires_vel(World::new()), spawn_required),
            |c| replace_required(c, k),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entity's components and values, sorted: two worlds that hold
    /// the same entities compare equal.
    fn contents(w: &mut World) -> Vec<(u32, u32, u32, u32)> {
        let mut v: Vec<_> = w
            .query::<(&Pos, &Vel, &Spr)>()
            .iter()
            .map(|(p, v, _)| (p.x.to_bits(), p.y.to_bits(), v.x.to_bits(), v.y.to_bits()))
            .collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn the_required_variant_makes_the_same_entities() {
        let n = 1000;
        let mut plain = crate::ecs_suite::ponko_churn(n);
        let mut required = churn(n, pos_requires_vel(World::new()), spawn_required);
        replace_ponko(&mut plain, 100);
        replace_required(&mut required, 100);
        assert_eq!(required.w.len(), n);
        assert_eq!(contents(&mut plain.w), contents(&mut required.w));
        let mut batch = pos_requires_vel(World::new());
        batch.spawn_batch((0..n).map(ent_required));
        let mut batch_plain = World::new();
        batch_plain.spawn_batch((0..n).map(ent));
        assert_eq!(contents(&mut batch), contents(&mut batch_plain));
    }

    #[test]
    fn requirements_elsewhere_leave_the_workloads_unchanged() {
        let n = 1000;
        let mut plain = crate::ecs_suite::ponko_churn(n);
        let mut elsewhere = churn(n, requires_elsewhere(World::new()), spawn_plain);
        for c in [&mut plain, &mut elsewhere] {
            replace_ponko(c, 100);
            add_remove_ponko(c);
            move_ponko(&mut c.w, DT);
        }
        assert_eq!(elsewhere.w.query::<&Life>().iter().count(), 0);
        assert_eq!(contents(&mut plain.w), contents(&mut elsewhere.w));
    }
}
