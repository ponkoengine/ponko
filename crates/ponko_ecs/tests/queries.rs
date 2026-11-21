//! Queries: fetch kinds, filters, iterators, aliasing rules.

mod common;

use common::*;
use ponko_ecs::*;

#[test]
fn query_matches_across_archetypes() {
    let mut w = World::new();
    w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0)));
    w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0), Tag));
    w.spawn((Pos(0.0, 0.0),));
    w.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0);
    let sum: f32 = w.query::<&Pos>().iter().map(|p| p.0).sum();
    assert_eq!(sum, 2.0);
    assert_eq!(w.query::<&Pos>().count(), 3);
    assert_eq!(w.query::<(&Pos, &Vel)>().count(), 2);
}

#[test]
fn iter_and_for_each_agree() {
    let mut w = World::new();
    for i in 0..100 {
        let e = w.spawn((Pos(i as f32, 0.0),));
        if i % 3 == 0 {
            w.insert(e, Tag);
        }
        if i % 5 == 0 {
            w.insert(e, Vel(0.0, 0.0));
        }
    }
    let mut a = Vec::new();
    w.query::<(Entity, &Pos)>().for_each(|(e, p)| a.push((e, p.0)));
    let b: Vec<_> = w.query::<(Entity, &Pos)>().iter().map(|(e, p)| (e, p.0)).collect();
    assert_eq!(a, b);
    assert_eq!(a.len(), 100);
    for (e, x) in a {
        assert_eq!(w.get::<Pos>(e).unwrap().0, x);
    }
}

#[test]
fn filters() {
    let mut w = World::new();
    w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0)));
    w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0), Frozen));
    w.spawn((Pos(0.0, 0.0), Tag));
    w.query::<(&mut Pos, &Vel)>()
        .filter::<Without<Frozen>>()
        .for_each(|(p, v)| p.0 += v.0);
    assert_eq!(w.query::<&Pos>().iter().filter(|p| p.0 == 1.0).count(), 1);
    assert_eq!(w.query::<&Pos>().filter::<With<Tag>>().count(), 1);
    assert_eq!(
        w.query::<&Pos>()
            .filter::<With<Vel>>()
            .filter::<Without<Frozen>>()
            .count(),
        1
    );
    assert_eq!(w.query::<&Pos>().filter::<Without<Vel>>().iter().count(), 1);
}

#[test]
fn optional_components() {
    let mut w = World::new();
    w.spawn((Pos(1.0, 0.0), Vel(5.0, 0.0)));
    w.spawn((Pos(2.0, 0.0),));
    let mut seen = Vec::new();
    w.query::<(&Pos, Option<&Vel>)>()
        .for_each(|(p, v)| seen.push((p.0, v.map(|v| v.0))));
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(seen, vec![(1.0, Some(5.0)), (2.0, None)]);
    w.query::<Option<&mut Vel>>().for_each(|v| {
        if let Some(v) = v {
            v.0 = 0.0;
        }
    });
    assert_eq!(w.query::<&Vel>().iter().next(), Some(&Vel(0.0, 0.0)));
}

#[test]
fn query_get_single_entity() {
    let mut w = World::new();
    let a = w.spawn((Pos(1.0, 0.0), Vel(2.0, 0.0)));
    let b = w.spawn((Pos(3.0, 0.0),));
    let mut q = w.query::<(&mut Pos, &Vel)>();
    let (p, v) = q.get(a).unwrap();
    p.0 += v.0;
    assert!(q.get(b).is_none());
    assert_eq!(w.get::<Pos>(a), Some(&Pos(3.0, 0.0)));
}

#[test]
#[should_panic(expected = "remove the duplicate access or split it into sequential queries")]
fn aliasing_query_panics() {
    let mut w = World::new();
    let _ = w.query::<(&mut Pos, &Pos)>();
}

#[test]
fn same_component_twice_read_only_is_fine() {
    let mut w = World::new();
    w.spawn((Pos(1.0, 0.0),));
    w.query::<(&Pos, &Pos)>().for_each(|(a, b)| assert_eq!(a, b));
}

// Every combination gives the boolean expressions a complete truth table.
#[derive(Component)]
struct Bit<const N: usize>;
#[derive(Component)]
struct Bits(usize);
fn filter_world(rows: usize) -> World {
    let mut w = World::new();
    for bits in 0..128 {
        for _ in 0..rows {
            let e = w.spawn((Bits(bits),));
            macro_rules! bit {
                ($n:literal) => {
                    if bits & (1 << $n) != 0 {
                        w.insert(e, Bit::<$n>);
                    }
                };
            }
            bit!(0);
            bit!(1);
            bit!(2);
            bit!(3);
            bit!(4);
            bit!(5);
            bit!(6);
        }
    }
    w
}
fn truth<F: QueryFilter>(pred: impl Fn(usize) -> bool) {
    let mut w = filter_world(1);
    let mut q = w.query::<(Entity, &Bits)>().filter::<F>();
    let mut actual: Vec<_> = q.iter().map(|(_, v)| v.0).collect();
    actual.sort_unstable();
    let expected: Vec<_> = (0..128).filter(|&b| pred(b)).collect();
    assert_eq!(actual, expected);
    assert_eq!(q.count(), expected.len());
    q.for_each(|(e, v)| assert!(pred(v.0), "{e:?}"));
    drop(q);
    let entities: Vec<_> = w.query::<(Entity, &Bits)>().iter().map(|(e, b)| (e, b.0)).collect();
    let mut q = w.query::<&Bits>().filter::<F>();
    for (e, bits) in entities {
        assert_eq!(q.get(e).is_some(), pred(bits));
    }
}

#[test]
fn or_accepts_and_arms_and_nested_combinations() {
    truth::<Or<((With<Bit<0>>, With<Bit<1>>), Without<Bit<2>>)>>(|b| b & 3 == 3 || b & 4 == 0);
    truth::<Or<((With<Bit<0>>, Without<Bit<1>>), (With<Bit<2>>, Without<Bit<3>>))>>(|b| b & 3 == 1 || b & 12 == 4);
    type Nested = Or<((With<Bit<0>>, Or<(With<Bit<1>>, Without<Bit<2>>)>), (Without<Bit<3>>,))>;
    truth::<Nested>(|b| b & 1 != 0 && (b & 2 != 0 || b & 4 == 0) || b & 8 == 0);
    truth::<Or<(Or<((With<Bit<0>>, With<Bit<1>>), With<Bit<2>>)>, Without<Bit<3>>)>>(|b| {
        b & 3 == 3 || b & 4 != 0 || b & 8 == 0
    });
    truth::<(Or<((With<Bit<0>>, With<Bit<1>>), Without<Bit<2>>)>, Without<Bit<3>>)>(|b| {
        (b & 3 == 3 || b & 4 == 0) && b & 8 == 0
    });
}

#[test]
fn empty_singleton_repeated_and_contradictory_or_arms() {
    truth::<Or<()>>(|_| false);
    truth::<Or<((), With<Bit<0>>)>>(|_| true);
    truth::<Or<((With<Bit<0>>,),)>>(|b| b & 1 != 0);
    truth::<Or<((With<Bit<0>>, With<Bit<0>>), Or<()>)>>(|b| b & 1 != 0);
    truth::<Or<((With<Bit<0>>, Without<Bit<0>>), With<Bit<1>>)>>(|b| b & 2 != 0);
    truth::<Or<((Or<()>, With<Bit<0>>),)>>(|_| false);
}

#[test]
fn tuple_or_filters_allow_optional_mutable_fetches_and_builder_filters() {
    let mut w = filter_world(1);
    w.query::<(&mut Bits, Option<&mut Bit<0>>)>()
        .filter::<Or<((With<Bits>, With<Bit<0>>), Without<Bit<1>>)>>()
        .filter::<With<Bit<2>>>()
        .filter::<Without<Bit<3>>>()
        .for_each(|(v, optional)| {
            assert_eq!(optional.is_some(), v.0 & 1 != 0);
            assert!(v.0 & 1 != 0 || v.0 & 2 == 0);
            v.0 += 128;
        });
    w.query::<&Bits>()
        .for_each(|v| assert_eq!(v.0 >= 128, (v.0 & 1 != 0 || v.0 & 2 == 0) && v.0 & 12 == 4));
}

#[test]
#[should_panic(expected = "remove the duplicate access or split it into sequential queries")]
fn tuple_or_filters_do_not_bypass_optional_mutable_alias_checks() {
    let mut w = World::new();
    let _ = w
        .query::<(Option<&mut Bits>, &Bits)>()
        .filter::<Or<((With<Bits>, Without<Bits>),)>>();
}

#[test]
fn cached_tuple_or_filters_track_growth_and_distinct_signatures() {
    let mut w = filter_world(1);
    assert!(w.raw_ref().archetypes().len() >= 128);
    type F = Or<((With<Bit<0>>, With<Bit<1>>), Without<Bit<2>>)>;
    type G = Or<((With<Bit<0>>, Without<Bit<1>>), Without<Bit<2>>)>;
    let e = w.spawn((Bits(0), Bit::<0>, Bit::<1>));
    for pass in 0..3 {
        let direct: Vec<_> = w.query::<&Bits>().filter::<F>().iter().map(|b| b.0).collect();
        let cached: Vec<_> = w.query::<&Bits>().cached().filter::<F>().iter().map(|b| b.0).collect();
        assert_eq!(cached, direct);
        let direct: Vec<_> = w.query::<&Bits>().filter::<G>().iter().map(|b| b.0).collect();
        let cached: Vec<_> = w.query::<&Bits>().cached().filter::<G>().iter().map(|b| b.0).collect();
        assert_eq!(cached, direct);
        if pass == 0 {
            w.insert(e, Bit::<2>);
        }
        if pass == 1 {
            w.spawn((Bits(999), Bit::<0>, Bit::<1>, Bit::<7>));
            w.remove::<Bit<1>>(e);
        }
    }
}

#[cfg(feature = "parallel")]
#[test]
fn parallel_tuple_or_filters_match_sequential_order_and_mutation() {
    let mut w = filter_world(if cfg!(miri) { 2 } else { 600 });
    type F = Or<((With<Bit<0>>, With<Bit<1>>), Without<Bit<2>>)>;
    let expected: Vec<_> = w.query::<&Bits>().filter::<F>().iter().map(|b| b.0).collect();
    let mut got = vec![usize::MAX];
    w.query::<&Bits>()
        .cached()
        .filter::<F>()
        .par_collect_into(&mut got, |b| b.0);
    assert_eq!(got, expected);
    w.query::<&mut Bits>()
        .cached()
        .filter::<F>()
        .par_for_each(|b| b.0 += 128);
    w.query::<&Bits>()
        .for_each(|b| assert_eq!(b.0 >= 128, b.0 & 3 == 3 || b.0 & 4 == 0));
}

#[test]
fn view_tuple_or_filters_handle_unregistered_components_per_arm() {
    #[derive(Component)]
    struct Never;
    let mut w = filter_world(1);
    let mut state = ViewState::new().write::<Bits>().read::<Bit<0>>();
    state.run(&mut w, |mut view| {
        assert_eq!(
            view.query::<Entity>()
                .filter::<Or<((With<Never>, With<Bit<0>>), With<Bit<1>>)>>()
                .count(),
            64
        );
        assert_eq!(
            view.query::<Entity>()
                .filter::<Or<((Without<Never>, With<Bit<0>>), With<Bit<1>>)>>()
                .count(),
            96
        );
        assert_eq!(
            view.query::<Entity>().filter::<Or<((With<Never>,), Or<()>)>>().count(),
            0
        );
        assert_eq!(view.query::<Entity>().filter::<Or<((), With<Never>)>>().count(), 128);
        view.query::<(&mut Bits, Option<&Bit<0>>)>()
            .filter::<Or<((With<Bit<0>>, With<Bit<1>>), Without<Never>)>>()
            .for_each(|(b, m)| assert_eq!(m.is_some(), b.0 & 1 != 0));
    });
}

#[test]
#[should_panic(expected = "at most 4 `Or` filters per query")]
fn expanded_tuple_or_clauses_obey_inline_capacity() {
    let mut w = World::new();
    let _ = w
        .query::<Entity>()
        .filter::<Or<((With<Bit<0>>, With<Bit<1>>, With<Bit<2>>), (With<Bit<3>>, With<Bit<4>>))>>();
}

// Four expanded clauses, each with four terms: exactly the inline limit.
type FullOr = Or<(
    (Or<(With<Bit<0>>, With<Bit<1>>)>, Or<(With<Bit<2>>, With<Bit<3>>)>),
    (Or<(With<Bit<4>>, With<Bit<5>>)>, Or<(With<Bit<6>>, Without<Bit<0>>)>),
)>;

#[test]
fn expanded_tuple_or_terms_fit_at_the_inline_limit() {
    truth::<FullOr>(|b| (b & 3 != 0 && b & 12 != 0) || (b & 48 != 0 && (b & 64 != 0 || b & 1 == 0)));
}

#[test]
#[should_panic(expected = "at most 16 components across a query's `Or` filters")]
fn expanded_tuple_or_terms_panic_above_the_inline_limit() {
    let mut w = World::new();
    let _ = w.query::<Entity>().filter::<Or<(FullOr, With<Bit<1>>)>>();
}

// ---- the only match ------------------------------------------------------------------------

#[test]
fn single_gives_the_only_match_and_says_why_there_is_none() {
    let mut w = World::new();
    assert_eq!(w.query::<&Pos>().single().err(), Some(SingleError::NoEntities));
    let player = w.spawn((Pos(1.0, 2.0), Tag));
    assert_eq!(w.query::<(Entity, &Pos)>().single(), Ok((player, &Pos(1.0, 2.0))));
    w.query::<&mut Pos>().single().unwrap().0 = 5.0;
    assert_eq!(w.get::<Pos>(player), Some(&Pos(5.0, 2.0)));

    w.spawn((Pos(0.0, 0.0),));
    w.spawn((Pos(0.0, 0.0),));
    assert_eq!(w.query::<&Pos>().single().err(), Some(SingleError::MultipleEntities));
    // Filters narrow it to one again, across archetypes.
    assert_eq!(w.query::<Entity>().filter::<With<Tag>>().single(), Ok(player));
    assert_eq!(
        w.query::<Entity>().filter::<With<Vel>>().single(),
        Err(SingleError::NoEntities)
    );
    assert_eq!(SingleError::NoEntities.to_string(), "no entity matches the query");
    assert_eq!(
        SingleError::MultipleEntities.to_string(),
        "more than one entity matches the query"
    );
}

/// Moves `e` to the archetype of `mask` (bits 0 to 6 of the `Bit`s).
fn set_bits(w: &mut World, e: Entity, mask: u32) {
    macro_rules! bits {
        ($($n:literal)*) => {$(
            if mask >> $n & 1 != 0 {
                w.insert(e, Bit::<$n>);
            } else {
                w.remove::<Bit<$n>>(e);
            }
        )*};
    }
    bits!(0 1 2 3 4 5 6);
}

#[test]
fn queries_skip_emptied_archetypes_and_find_refilled_ones() {
    // 128 archetypes with `Pos`, filled once each, then all but a few
    // emptied again: scans visit the world's list of archetypes with
    // entities, and must see an archetype again as soon as it refills, with
    // the archetype index (from 64 archetypes on) and without.
    for policy in [QueryCachePolicy::new(), QueryCachePolicy::off()] {
        let mut w = World::new();
        w.set_query_cache_policy(policy);
        let e = w.spawn((Pos(1.0, 0.0),));
        for mask in 0..128 {
            set_bits(&mut w, e, mask);
        }
        let kept = w.spawn((Pos(2.0, 0.0), Vel(0.0, 0.0)));
        set_bits(&mut w, e, 0b101);
        // Many empty archetypes (some of them reclaimed, at most 64 kept).
        assert!(w.raw_ref().archetypes().iter().filter(|a| a.is_empty()).count() >= 64);
        for round in 0..3 {
            let mut seen = Vec::new();
            w.query::<(Entity, &Pos)>().for_each(|(e, _)| seen.push(e));
            let mut sorted = seen.clone();
            sorted.sort();
            let mut expected = [e, kept];
            expected.sort();
            assert_eq!(sorted, expected, "round {round}");
            assert_eq!(
                w.query::<(Entity, &Pos)>().iter().map(|(e, _)| e).collect::<Vec<_>>(),
                seen
            );
            assert_eq!(w.query::<&Pos>().filter::<With<Bit<2>>>().count(), 1);
            assert_eq!(w.query::<&Pos>().filter::<With<Bit<1>>>().count(), 0);
            assert!(w.query::<&Pos>().filter::<With<Bit<1>>>().cached().single().is_err());
            #[cfg(feature = "parallel")]
            {
                let mut out = Vec::new();
                w.query::<(Entity, &Pos)>().par_collect_into(&mut out, |(e, _)| e);
                assert_eq!(out, seen);
            }
            let mut rows = 0;
            let pos = w.component_id::<Pos>().unwrap();
            w.raw().for_each_chunk(&[pos], |chunk| rows += chunk.len());
            assert_eq!(rows, 2);
            // Refill an archetype that was empty, and empty the one `e` was in.
            set_bits(&mut w, e, if round % 2 == 0 { 0b1_0110 } else { 0b101 });
            assert_eq!(
                w.query::<&Pos>().filter::<With<Bit<1>>>().count(),
                usize::from(round % 2 == 0)
            );
            set_bits(&mut w, e, 0b101);
        }
    }
}

#[test]
fn despawning_the_last_entity_in_any_row_order_empties_the_archetype() {
    // `World::despawn` notes an emptied archetype from where the removed
    // row was (issue #448), not from the archetype's length: whichever row
    // goes last, the archetype must count as empty, so scans skip it and
    // the world reclaims it once more than it keeps are empty.
    let mut w = World::new();
    let survivor = w.spawn((Pos(0.0, 0.0),));
    for mask in 0..128u32 {
        let three: Vec<Entity> = (0..3)
            .map(|i| {
                let e = w.spawn((Pos(i as f32, 0.0),));
                set_bits(&mut w, e, mask);
                e
            })
            .collect();
        let order = match mask % 3 {
            0 => [0, 1, 2], // the first row each time: the last row moves in
            1 => [2, 1, 0], // the last row each time: nothing moves
            _ => [1, 2, 0], // a middle row, then the last, then the only one
        };
        for i in order {
            assert!(w.despawn(three[i]));
        }
        assert_eq!(w.query::<&Pos>().count(), 1, "mask {mask:#b}");
    }
    assert_eq!(w.query::<Entity>().iter().collect::<Vec<_>>(), [survivor]);
    // 128 bit sets and their steps were built, but empty archetypes were
    // reclaimed and their slots reused.
    let slots = w.raw_ref().archetypes().len();
    assert!(slots < 128, "{slots} archetype slots");
    // An emptied archetype refills and is found again.
    let back = w.spawn((Pos(5.0, 0.0),));
    set_bits(&mut w, back, 0b11);
    assert_eq!(w.query::<&Pos>().filter::<With<Bit<1>>>().count(), 1);
}

#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Opt<const N: u8>(u32);

/// Entity `i` has `Pos` and `Opt<k>` for each bit `k` set in `i % 16`:
/// sixteen archetypes, every combination of four optional components.
fn bits_world() -> (World, Vec<(Entity, u32)>) {
    let mut w = World::new();
    let mut model = Vec::new();
    for i in 0..16 * 9u32 {
        let m = i % 16;
        let e = w.spawn((Pos(i as f32, 0.0),));
        if m & 1 != 0 {
            w.insert(e, Opt::<0>(i));
        }
        if m & 2 != 0 {
            w.insert(e, Opt::<1>(i));
        }
        if m & 4 != 0 {
            w.insert(e, Opt::<2>(i));
        }
        if m & 8 != 0 {
            w.insert(e, Opt::<3>(i));
        }
        model.push((e, m));
    }
    (w, model)
}

/// The bits of `i` that `options` names (`Some(k)`: bit `k`), packed in order.
fn present(options: &[Option<u32>]) -> u32 {
    options.iter().enumerate().map(|(j, o)| (o.is_some() as u32) << j).sum()
}

#[test]
fn option_fetches_see_exactly_the_components_each_archetype_has() {
    let (mut w, model) = bits_world();
    let expect = |e: Entity, bits: &[u32]| {
        let m = model.iter().find(|x| x.0 == e).unwrap().1;
        bits.iter().enumerate().map(|(j, &k)| ((m >> k) & 1) << j).sum::<u32>()
    };
    let mut seen = 0;
    // One option, two (split loops), three (one plain loop), and two
    // spread over nested tuples: `for_each` sees what `iter` and the
    // model see, value included.
    w.query::<(Entity, &Pos, Option<&Opt<0>>)>().for_each(|(e, p, a)| {
        assert_eq!(present(&[a.map(|b| b.0)]), expect(e, &[0]));
        assert!(a.is_none_or(|b| b.0 as f32 == p.0));
        seen += 1;
    });
    w.query::<(Entity, Option<&Opt<1>>, &Pos, Option<&Opt<3>>)>()
        .for_each(|(e, a, p, b)| {
            assert_eq!(present(&[a.map(|b| b.0), b.map(|b| b.0)]), expect(e, &[1, 3]));
            assert!(
                [a.map(|b| b.0), b.map(|b| b.0)]
                    .iter()
                    .flatten()
                    .all(|&v| v as f32 == p.0)
            );
            seen += 1;
        });
    w.query::<(Entity, Option<&Opt<0>>, Option<&Opt<1>>, Option<&Opt<2>>)>()
        .for_each(|(e, a, b, c)| {
            assert_eq!(
                present(&[a.map(|b| b.0), b.map(|b| b.0), c.map(|b| b.0)]),
                expect(e, &[0, 1, 2])
            );
            seen += 1;
        });
    w.query::<((Entity, Option<&Opt<2>>), (&Pos, Option<&Opt<0>>))>()
        .for_each(|((e, a), (p, b))| {
            assert_eq!(present(&[a.map(|b| b.0), b.map(|b| b.0)]), expect(e, &[2, 0]));
            assert!(
                [a.map(|b| b.0), b.map(|b| b.0)]
                    .iter()
                    .flatten()
                    .all(|&v| v as f32 == p.0)
            );
            seen += 1;
        });
    assert_eq!(seen, 4 * model.len());
    // `Option<&mut T>` writes exactly the entities that have `T`.
    w.query::<(Option<&mut Opt<1>>, Option<&mut Opt<2>>)>()
        .for_each(|(a, b)| {
            if let Some(a) = a {
                a.0 += 1000;
            }
            if let Some(b) = b {
                b.0 += 2000;
            }
        });
    for &(e, m) in &model {
        let i = w.get::<Pos>(e).unwrap().0 as u32;
        assert_eq!(w.get::<Opt<1>>(e).map(|b| b.0), (m & 2 != 0).then_some(i + 1000));
        assert_eq!(w.get::<Opt<2>>(e).map(|b| b.0), (m & 4 != 0).then_some(i + 2000));
    }
}

#[test]
fn option_fetches_in_tracked_worlds_and_views_see_the_same() {
    let (mut w, model) = bits_world();
    w.track_changes::<Opt<1>>();
    let since = w.advance_change_tick();
    // The tracked loop: `Option<&mut T>` marks a change only where `T` is.
    let mut visited = 0;
    w.query::<(Option<&mut Opt<1>>, Option<&Opt<3>>)>().for_each(|(a, b)| {
        if let (Some(a), Some(b)) = (a, b) {
            a.0 = b.0 + 1;
        }
        visited += 1;
    });
    assert_eq!(visited, model.len());
    let mut changed: Vec<Entity> = w
        .query::<Entity>()
        .filter::<Changed<Opt<1>>>()
        .since(since)
        .iter()
        .collect();
    changed.sort();
    let mut with: Vec<Entity> = model.iter().filter(|x| x.1 & 2 != 0).map(|x| x.0).collect();
    with.sort();
    assert_eq!(changed, with);
    // A view runs the same split loops.
    let mut state = ViewState::new().read::<Pos>().read::<Opt<0>>().write::<Opt<2>>();
    let mut seen = 0;
    state.run(&mut w, |mut view| {
        view.query::<(&Pos, Option<&Opt<0>>, Option<&mut Opt<2>>)>()
            .for_each(|(p, a, b)| {
                let i = p.0 as u32;
                assert_eq!(a.map(|a| a.0), ((i % 16) & 1 != 0).then_some(i));
                assert_eq!(b.as_ref().map(|b| b.0), ((i % 16) & 4 != 0).then_some(i));
                if let Some(b) = b {
                    b.0 = 7;
                }
                seen += 1;
            });
    });
    assert_eq!(seen, model.len());
    assert_eq!(
        w.query::<&Opt<2>>().iter().filter(|b| b.0 == 7).count(),
        model.len() / 2
    );
}
