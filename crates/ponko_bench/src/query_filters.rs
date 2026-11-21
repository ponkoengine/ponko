//! Equal-work typed-filter overhead: 1M rows over 16 archetypes.
//! Every filter matches every row, keeping iteration work identical.

use crate::{Bench, DT, M0, M1, M2, M3, N, Pos, Vel, ent};
use ponko_ecs::{Chunk, ChunkQuery, FilterShape, FilterTree, Or, With, Without, World, compile_tree};
use std::hint::black_box;

fn world() -> World {
    world_of(N)
}

/// `n` rows over the 16 archetypes of the markers.
fn world_of(n: usize) -> World {
    let mut w = World::new();
    for i in 0..n {
        let e = w.spawn(ent(i));
        if i & 1 != 0 {
            w.insert(e, M0);
        }
        if i & 2 != 0 {
            w.insert(e, M1);
        }
        if i & 4 != 0 {
            w.insert(e, M2);
        }
        if i & 8 != 0 {
            w.insert(e, M3);
        }
    }
    w
}

pub fn run(b: &mut Bench) {
    b.steady("query-filter/plain", 200, world, |w| {
        let dt = black_box(DT);
        w.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
            p.x += v.x * dt;
            p.y += v.y * dt;
        });
    });
    b.steady("query-filter/typed", 200, world, |w| {
        let dt = black_box(DT);
        w.query::<(&mut Pos, &Vel)>().filter::<With<Vel>>().for_each(|(p, v)| {
            p.x += v.x * dt;
            p.y += v.y * dt;
        });
    });
    b.steady("query-filter/nested-or-and", 200, world, |w| {
        let dt = black_box(DT);
        w.query::<(&mut Pos, &Vel)>()
            .filter::<Or<((With<Pos>, With<Vel>), Without<M0>)>>()
            .for_each(|(p, v)| {
                p.x += v.x * dt;
                p.y += v.y * dt;
            });
    });

    // The path the C API takes: raw column pointers per archetype, with and
    // without a runtime filter. `c-plain` is `ponko_query_with` without a
    // filter, so it shows what the filter hook costs a query that has none.
    b.steady("query-filter/c-plain", 200, world, |w| {
        let ids = [w.register::<Pos>(), w.register::<Vel>()];
        let dt = black_box(DT);
        w.raw()
            .query_chunks(ChunkQuery::new(&ids).writes(1), |chunk| step(&chunk, dt));
    });
    // The filtered C path compiles its filter on every call: the tree of
    // component numbers is walked into inline storage and each number is
    // looked up in the world (`ponko_query_filtered`). That is timed here too.
    let (pos, vel, m0) = {
        let mut w = world();
        (
            w.register::<Pos>().index() as u32,
            w.register::<Vel>().index() as u32,
            w.register::<M0>().index() as u32,
        )
    };
    let with = Tree::With(vel);
    let nested = Tree::Or(vec![
        Tree::All(vec![Tree::With(pos), Tree::With(vel)]),
        Tree::Without(m0),
    ]);
    for (name, tree) in [
        ("query-filter/c-filtered", with),
        ("query-filter/c-filtered-nested", nested),
    ] {
        b.steady(name, 200, world, |w| {
            let ids = [w.register::<Pos>(), w.register::<Vel>()];
            let dt = black_box(DT);
            let filter = compile_tree(&tree, |raw| w.component_at(raw as usize)).unwrap();
            w.raw()
                .query_chunks(ChunkQuery::new(&ids).writes(1).filter(&filter), |chunk| {
                    step(&chunk, dt)
                });
        });
    }
}

/// A filter as the C API holds it: a tree of component numbers.
enum Tree {
    With(u32),
    Without(u32),
    All(Vec<Tree>),
    Or(Vec<Tree>),
}

impl FilterTree for Tree {
    fn shape(&self) -> FilterShape<'_, Self> {
        match self {
            Tree::With(id) => FilterShape::With(*id),
            Tree::Without(id) => FilterShape::Without(*id),
            Tree::All(parts) => FilterShape::All(parts),
            Tree::Or(arms) => FilterShape::Or(arms),
        }
    }
}

/// `pos += vel * dt` over a chunk's two raw columns.
fn step(chunk: &Chunk<'_>, dt: f32) {
    let (pos, vel) = (chunk.columns[0].cast::<Pos>(), chunk.columns[1].cast::<Vel>());
    for i in 0..chunk.entities.len() {
        // SAFETY: both columns hold `entities.len()` rows of these types, and `Pos` is only written.
        unsafe {
            let (p, v) = (&mut *pos.add(i), &*vel.add(i));
            p.x += v.x * dt;
            p.y += v.y * dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Pos` of `w`, in entity order.
    fn positions(w: &mut World) -> Vec<(u32, f32, f32)> {
        let mut out = Vec::new();
        w.query::<(ponko_ecs::Entity, &Pos)>()
            .for_each(|(e, p)| out.push((e.index(), p.x, p.y)));
        out.sort_by_key(|r| r.0);
        out
    }

    /// The raw-column step of the C path moves every row once, with and
    /// without a runtime filter that matches every row.
    #[test]
    fn the_c_path_steps_every_row_once_with_and_without_a_filter() {
        let mut plain = world_of(64);
        let mut filtered = world_of(64);
        let ids = [plain.register::<Pos>(), plain.register::<Vel>()];
        plain
            .raw()
            .query_chunks(ChunkQuery::new(&ids).writes(1), |chunk| step(&chunk, DT));
        let ids = [filtered.register::<Pos>(), filtered.register::<Vel>()];
        let (pos, vel, m0) = (
            ids[0].index() as u32,
            ids[1].index() as u32,
            filtered.register::<M0>().index() as u32,
        );
        let nested = Tree::Or(vec![
            Tree::All(vec![Tree::With(pos), Tree::With(vel)]),
            Tree::Without(m0),
        ]);
        let filter = compile_tree(&nested, |raw| filtered.component_at(raw as usize)).unwrap();
        filtered
            .raw()
            .query_chunks(ChunkQuery::new(&ids).writes(1).filter(&filter), |chunk| {
                step(&chunk, DT)
            });
        let moved = positions(&mut plain);
        assert_eq!(moved.len(), 64);
        for (i, &(_, x, y)) in moved.iter().enumerate() {
            let (p, v, _) = ent(i);
            assert_eq!((x, y), (p.x + v.x * DT, p.y + v.y * DT));
        }
        assert_eq!(positions(&mut filtered), moved);
    }
}
