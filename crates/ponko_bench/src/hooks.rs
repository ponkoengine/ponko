//! Component hooks: the ECS matrix's ponko rows again, with hooks.
//!
//! - `ponko-hooks-elsewhere`: the world has hooks (`on_add`, `on_remove`,
//!   `on_replace`) on a component the workload never uses. The requirement
//!   is that this costs nothing: compare with the `ponko` row.
//! - `ponko-hooked`: the workload's own components have hooks that do the
//!   least a hook can (count into a field of a resource): `on_add` and
//!   `on_remove` on `Pos` for spawns and despawns, on `Tag` for add/remove.
//!   What using hooks costs, before the hook's own work.
//!
//! `crates/ponko_bench/HOOKS.md` has the method and the numbers;
//! `cargo run --release -p ponko_bench -- 'ecs/' ` runs them with the matrix.

use std::hint::black_box;

use ponko_ecs::{Entity, HookWorld, World};

use crate::ecs_suite::{Hp, PonkoChurn, add_remove_ponko, label, move_ponko, replace_ponko};
use crate::{Bench, DT, Pos, Tag, ent};

/// How many times hooks ran.
#[derive(Default)]
pub struct Calls(pub u64);

fn count(world: &mut HookWorld, _: Entity) {
    world.resource_mut::<Calls>().0 += 1;
}

fn count_replace(world: &mut HookWorld, _: Entity, _: &Hp) {
    world.resource_mut::<Calls>().0 += 1;
}

/// Hooks on `Hp`, which the matrix's workloads never use.
pub fn hooks_elsewhere(mut w: World) -> World {
    w.insert_resource(Calls::default());
    w.on_add::<Hp>(count);
    w.on_remove::<Hp>(count);
    w.on_replace::<Hp>(count_replace);
    w
}

/// Counting hooks on the workloads' own components.
pub fn hooked(mut w: World) -> World {
    w.insert_resource(Calls::default());
    w.on_add::<Pos>(count);
    w.on_remove::<Pos>(count);
    w.on_add::<Tag>(count);
    w.on_remove::<Tag>(count);
    w
}

/// Sets up a world's hooks.
type Setup = fn(World) -> World;

/// The two variants: name suffix and how the world is set up.
const VARIANTS: [(&str, Setup); 2] = [("ponko-hooks-elsewhere", hooks_elsewhere), ("ponko-hooked", hooked)];

fn churn_with(n: usize, setup: Setup) -> PonkoChurn {
    let mut c = crate::ecs_suite::ponko_churn(0);
    c.w = setup(World::new());
    for i in 0..n {
        let e = c.w.spawn(ent(i));
        c.live.push_back(e);
    }
    c.next = n;
    c
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

/// The hook rows, for every size of the matrix.
pub fn run(b: &mut Bench) {
    for n in crate::ecs_suite::SIZES {
        let l = label(n);
        for (variant, setup) in VARIANTS {
            b.steady(
                &format!("ecs/iter-{l}/{variant}"),
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
                &format!("ecs/spawn-loop-{l}/{variant}"),
                r,
                || setup(World::new()),
                |w| {
                    for i in 0..n {
                        w.spawn(ent(i));
                    }
                },
            );
            b.run(
                &format!("ecs/spawn-batch-{l}/{variant}"),
                r,
                || setup(World::new()),
                |w| w.spawn_batch((0..n).map(ent)),
            );
            let k = (n / 10).max(1);
            let r = runs(n, 500, 50, 10);
            b.steady(
                &format!("ecs/spawn-despawn-{l}/{variant}"),
                r,
                || churn_with(n, setup),
                |c| replace_ponko(c, k),
            );
            b.steady(
                &format!("ecs/add-remove-{l}/{variant}"),
                r,
                || churn_with(n, setup),
                add_remove_ponko,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(w: &World) -> u64 {
        w.resource::<Calls>().0
    }

    #[test]
    fn hooks_elsewhere_never_run_and_leave_the_workloads_unchanged() {
        let n = 1000;
        let mut plain = crate::ecs_suite::ponko_churn(n);
        let mut hooks = churn_with(n, hooks_elsewhere);
        for c in [&mut plain, &mut hooks] {
            replace_ponko(c, 100);
            add_remove_ponko(c);
            move_ponko(&mut c.w, DT);
        }
        assert_eq!(calls(&hooks.w), 0);
        let pos = |w: &mut World| {
            let mut v: Vec<(u32, u32)> = w
                .query::<&Pos>()
                .iter()
                .map(|p| (p.x.to_bits(), p.y.to_bits()))
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(pos(&mut plain.w), pos(&mut hooks.w));
    }

    #[test]
    fn hooked_workloads_run_one_hook_per_change() {
        let n = 1000;
        let mut c = churn_with(n, hooked);
        assert_eq!(calls(&c.w), n as u64, "one on_add per spawn");
        replace_ponko(&mut c, 100);
        assert_eq!(calls(&c.w), n as u64 + 200, "a despawn and a spawn each");
        add_remove_ponko(&mut c);
        assert_eq!(calls(&c.w), n as u64 + 200 + 2 * n as u64, "an add and a remove each");
        assert_eq!(c.w.len(), n);
    }
}
