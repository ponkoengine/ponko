use ponko_ecs::*;
#[derive(Component)]
struct Value(usize);
#[derive(Component)]
struct Marker<const N: usize>;

fn fragmented(w: &mut World) {
    for bits in 0..128 {
        let e = w.spawn((Value(bits),));
        macro_rules! m {
            ($n:literal) => {
                if bits & (1 << $n) != 0 {
                    w.insert(e, Marker::<$n>);
                }
            };
        }
        m!(0);
        m!(1);
        m!(2);
        m!(3);
        m!(4);
        m!(5);
        m!(6);
    }
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn cached_queries_follow_registration_growth_moves_and_empty_layouts() {
    let mut w = World::new();
    fragmented(&mut w);
    assert_eq!(
        w.query::<&Value>()
            .cached()
            .filter::<With<Marker<0>>>()
            .filter::<Without<Marker<1>>>()
            .count(),
        32
    );
    // Register a previously unknown optional component after warming the cache.
    assert_eq!(w.query::<(&Value, Option<&Marker<9>>)>().cached().count(), 128);
    let e = w.spawn((Value(500), Marker::<0>, Marker::<9>));
    for i in 0..512 {
        w.spawn((Value(i), Marker::<0>, Marker::<9>));
    }
    assert_eq!(
        w.query::<&Value>()
            .cached()
            .filter::<With<Marker<0>>>()
            .filter::<Without<Marker<1>>>()
            .count(),
        545
    );
    w.insert(e, Marker::<1>);
    w.remove::<Marker<1>>(e);
    w.query::<&mut Value>()
        .cached()
        .filter::<With<Marker<9>>>()
        .for_each(|v| v.0 += 1);
    assert_eq!(w.get::<Value>(e).unwrap().0, 501);
    let ids: Vec<_> = w
        .query::<(Entity, &Value)>()
        .cached()
        .filter::<With<Marker<9>>>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in ids {
        w.despawn(e);
    }
    assert_eq!(w.query::<&Value>().cached().filter::<With<Marker<9>>>().count(), 0);
    w.spawn((Value(1), Marker::<0>, Marker::<9>));
    assert_eq!(w.query::<&Value>().cached().filter::<With<Marker<9>>>().count(), 1);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn signatures_optional_filters_and_worlds_remain_independent() {
    let mut w = World::new();
    fragmented(&mut w);
    assert_eq!(w.query::<&Value>().cached().filter::<With<Marker<0>>>().count(), 64);
    assert_eq!(w.query::<&Value>().cached().filter::<Without<Marker<0>>>().count(), 64);
    assert_eq!(
        w.query::<&Value>()
            .cached()
            .filter::<With<Marker<0>>>()
            .filter::<Without<Marker<0>>>()
            .count(),
        0
    );
    let expected: Vec<_> = w
        .query::<(&Value, Option<&Marker<0>>)>()
        .cached()
        .iter()
        .map(|(v, m)| (v.0, m.is_some()))
        .collect();
    let mut actual = Vec::new();
    w.query::<(&Value, Option<&Marker<0>>)>()
        .cached()
        .for_each(|(v, m)| actual.push((v.0, m.is_some())));
    assert_eq!(actual, expected);
    let mut other = World::new();
    other.register::<Marker<0>>();
    fragmented(&mut other);
    assert_eq!(other.query::<&Value>().cached().filter::<With<Marker<0>>>().count(), 64);
}

#[cfg(feature = "parallel")]
#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn cached_parallel_order_matches_sequential() {
    let mut w = World::new();
    fragmented(&mut w);
    let expected: Vec<_> = w
        .query::<&Value>()
        .cached()
        .filter::<With<Marker<0>>>()
        .iter()
        .map(|v| v.0)
        .collect();
    let mut actual = Vec::new();
    w.query::<&Value>()
        .cached()
        .filter::<With<Marker<0>>>()
        .par_collect_into(&mut actual, |v| v.0);
    assert_eq!(actual, expected);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn cached_typed_filters_keep_distinct_signatures() {
    let mut w = World::new();
    fragmented(&mut w);
    type Either = Or<(With<Marker<0>>, With<Marker<1>>)>;
    type EitherNot = Or<(With<Marker<0>>, Without<Marker<1>>)>;
    type Narrow = (Either, Without<Marker<2>>);
    for _ in 0..2 {
        // Same terms, different polarity or extra filters: separate cache entries.
        assert_eq!(w.query::<&Value>().cached().filter::<Either>().count(), 96);
        assert_eq!(w.query::<&Value>().cached().filter::<EitherNot>().count(), 96);
        assert_eq!(w.query::<&Value>().cached().filter::<Narrow>().count(), 48);
        let cached: Vec<usize> = w
            .query::<&Value>()
            .cached()
            .filter::<EitherNot>()
            .iter()
            .map(|v| v.0)
            .collect();
        let direct: Vec<usize> = w.query::<&Value>().filter::<EitherNot>().iter().map(|v| v.0).collect();
        assert_eq!(cached, direct);
        assert!(cached.iter().all(|&b| b & 1 != 0 || b & 2 == 0));
    }
}

// ---- the automatic policy -------------------------------------------------------

fn sparse(w: &mut World) -> usize {
    w.query::<&Value>()
        .filter::<With<Marker<0>>>()
        .filter::<Without<Marker<1>>>()
        .count()
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn a_repeated_selective_query_is_indexed_without_a_hint() {
    let mut w = World::new();
    fragmented(&mut w);
    assert_eq!(sparse(&mut w), 32);
    let first = w.query_cache_stats();
    assert_eq!(
        (first.signatures, first.indexed, first.hits),
        (1, 0, 0),
        "one use builds nothing"
    );
    assert_eq!(sparse(&mut w), 32);
    assert_eq!(sparse(&mut w), 32);
    let stats = w.query_cache_stats();
    assert_eq!(
        (stats.signatures, stats.indexed, stats.indices, stats.hits),
        (1, 1, 32, 2)
    );
    assert!(stats.bytes >= 32 * 4);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn a_query_that_matches_most_archetypes_is_scanned_unless_hinted() {
    let mut w = World::new();
    fragmented(&mut w);
    for _ in 0..4 {
        assert_eq!(w.query::<&Value>().count(), 128);
        assert_eq!(
            w.query::<&Value>()
                .filter::<Without<Marker<0>>>()
                .filter::<Without<Marker<1>>>()
                .count(),
            32
        );
    }
    // All of them, and 3/4 of them (`Or`): no index. 1/4: indexed.
    for _ in 0..4 {
        let either = w
            .query::<&Value>()
            .filter::<Or<(With<Marker<0>>, With<Marker<1>>)>>()
            .count();
        assert_eq!(either, 96);
    }
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed, stats.indices), (3, 1, 32));
    // The hint indexes a dense signature, for hinted and plain queries alike.
    assert_eq!(w.query::<&Value>().cached().count(), 128);
    assert_eq!(w.query::<&Value>().count(), 128);
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed, stats.indices), (3, 2, 32 + 128));
}

#[test]
#[cfg_attr(miri, ignore = "the policy's threshold: safe code, and 32 archetypes to build")]
fn small_worlds_scan_whatever_the_hint() {
    let mut w = World::new();
    // 32 layouts: under the default threshold of 64.
    for bits in 0..32usize {
        let e = w.spawn((Value(bits),));
        if bits & 1 != 0 {
            w.insert(e, Marker::<0>);
        }
        if bits & 2 != 0 {
            w.insert(e, Marker::<1>);
        }
        if bits & 4 != 0 {
            w.insert(e, Marker::<2>);
        }
        if bits & 8 != 0 {
            w.insert(e, Marker::<3>);
        }
        if bits & 16 != 0 {
            w.insert(e, Marker::<4>);
        }
    }
    assert!(w.raw_ref().archetypes().len() < w.query_cache_policy().min_archetypes);
    for _ in 0..4 {
        assert_eq!(sparse(&mut w), 8);
        assert_eq!(w.query::<&Value>().filter::<With<Marker<0>>>().cached().count(), 16);
    }
    assert_eq!(
        w.query_cache_stats(),
        World::new().query_cache_stats(),
        "nothing is remembered"
    );
    // The threshold is the policy's: lower it and the same world indexes.
    let mut policy = QueryCachePolicy::new();
    policy.min_archetypes = 16;
    w.set_query_cache_policy(policy);
    for _ in 0..3 {
        assert_eq!(sparse(&mut w), 8);
    }
    assert_eq!(w.query_cache_stats().indexed, 1);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn a_query_and_a_world_can_opt_out() {
    let mut w = World::new();
    fragmented(&mut w);
    for _ in 0..4 {
        assert_eq!(
            w.query::<&Value>()
                .filter::<With<Marker<0>>>()
                .filter::<Without<Marker<1>>>()
                .uncached()
                .count(),
            32
        );
    }
    assert_eq!(
        w.query_cache_stats().signatures,
        0,
        "an opted-out query leaves the index alone"
    );
    for _ in 0..3 {
        assert_eq!(sparse(&mut w), 32);
    }
    assert_eq!(w.query_cache_stats().indexed, 1);
    // The world: off drops the index and ignores hints.
    w.set_query_cache_policy(QueryCachePolicy::off());
    assert!(w.query_cache_policy().is_off());
    let off = w.query_cache_stats();
    assert_eq!((off.signatures, off.indexed, off.indices, off.bytes), (0, 0, 0, 0));
    for _ in 0..4 {
        assert_eq!(sparse(&mut w), 32);
        assert_eq!(w.query::<&Value>().filter::<With<Marker<0>>>().cached().count(), 64);
    }
    assert_eq!(w.query_cache_stats(), off);
    // And back on.
    w.set_query_cache_policy(QueryCachePolicy::default());
    for _ in 0..3 {
        assert_eq!(sparse(&mut w), 32);
    }
    assert_eq!(w.query_cache_stats().indexed, 1);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn equivalent_queries_share_one_index_entry() {
    let mut w = World::new();
    fragmented(&mut w);
    type Both = (With<Marker<0>>, With<Marker<1>>);
    let mut results = Vec::new();
    let mut ask = |_: &mut World, values: Vec<usize>| results.push(values);
    // Filter order, `with` against a typed filter, a fetched component
    // against a filtered one, a one-arm `Or`, a repeated filter, optional
    // data, mutability, and an `Added`/`Changed`-free tuple nesting.
    let values = w
        .query::<&Value>()
        .filter::<With<Marker<0>>>()
        .filter::<With<Marker<1>>>()
        .iter()
        .map(|v| v.0)
        .collect();
    ask(&mut w, values);
    let values = w
        .query::<&Value>()
        .filter::<With<Marker<1>>>()
        .filter::<With<Marker<0>>>()
        .iter()
        .map(|v| v.0)
        .collect();
    ask(&mut w, values);
    let values = w.query::<&Value>().filter::<Both>().iter().map(|v| v.0).collect();
    ask(&mut w, values);
    let values = w
        .query::<(&Value, &Marker<0>)>()
        .filter::<With<Marker<1>>>()
        .iter()
        .map(|(v, _)| v.0)
        .collect();
    ask(&mut w, values);
    let values = w
        .query::<&Value>()
        .filter::<(Or<(With<Marker<0>>,)>, (With<Marker<1>>,), With<Marker<1>>)>()
        .iter()
        .map(|v| v.0)
        .collect();
    ask(&mut w, values);
    let values = w
        .query::<(&mut Value, Option<&Marker<5>>)>()
        .filter::<Both>()
        .iter()
        .map(|(v, _)| v.0)
        .collect();
    ask(&mut w, values);
    // A clause the rest of the filter already guarantees.
    let values = w
        .query::<&Value>()
        .filter::<(Both, Or<(With<Marker<0>>, With<Marker<6>>)>)>()
        .iter()
        .map(|v| v.0)
        .collect();
    ask(&mut w, values);
    assert!(results.iter().all(|r| r == &results[0]) && results[0].len() == 32);
    assert_eq!(w.query_cache_stats().signatures, 1);
    // The runtime form of the same filter, through the chunk path.
    let (value, m0, m1) = (
        w.register::<Value>(),
        w.register::<Marker<0>>(),
        w.register::<Marker<1>>(),
    );
    let filter = DynFilter::All(vec![DynFilter::With(m1), DynFilter::Or(vec![DynFilter::With(m0)])])
        .compile()
        .unwrap();
    let mut rows = 0;
    w.raw()
        .query_chunks(ChunkQuery::new(&[value]).filter(&filter), |chunk| rows += chunk.len());
    w.raw()
        .query_chunks(ChunkQuery::new(&[value, m0, m1]), |chunk| rows += chunk.len());
    w.raw().for_each_chunk(&[m1, value, m0], |chunk| rows += chunk.len());
    assert_eq!(rows, 3 * 32);
    assert_eq!(w.query_cache_stats().signatures, 1);
    // A different condition is a different entry.
    assert_eq!(
        w.query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<Without<Marker<1>>>()
            .count(),
        32
    );
    assert_eq!(w.query_cache_stats().signatures, 2);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn an_automatic_index_follows_registration_new_archetypes_moves_and_empty_archetypes() {
    #[derive(Component)]
    struct Late;
    let mut w = World::new();
    fragmented(&mut w);
    let all = |w: &mut World| {
        let auto: Vec<usize> = w
            .query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<1>>>()
            .iter()
            .map(|v| v.0)
            .collect();
        let scanned: Vec<usize> = w
            .query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<1>>>()
            .uncached()
            .iter()
            .map(|v| v.0)
            .collect();
        assert_eq!(auto, scanned);
        auto
    };
    for _ in 0..3 {
        assert_eq!(all(&mut w).len(), 32);
    }
    assert_eq!(w.query_cache_stats().indexed, 1);
    let hits = w.query_cache_stats().hits;
    // A component registered after the index was built changes nothing.
    w.register::<Late>();
    assert_eq!(all(&mut w).len(), 32);
    // A new archetype that matches, and one that doesn't.
    let e = w.spawn((Value(1000), Marker::<0>, Marker::<1>, Late));
    w.spawn((Value(2000), Marker::<0>, Late));
    assert_eq!(all(&mut w).len(), 33);
    assert_eq!(w.query_cache_stats().indices, 33);
    // Moves: out of the matching archetype, and back in.
    w.remove::<Marker<1>>(e);
    assert_eq!(all(&mut w).len(), 32);
    w.insert(e, Marker::<1>);
    assert_eq!(all(&mut w).last(), Some(&1000));
    // Despawns that empty an archetype: it stays listed, and fills again.
    w.despawn(e);
    assert_eq!(all(&mut w).len(), 32);
    w.spawn((Value(3000), Marker::<0>, Marker::<1>, Late));
    assert_eq!(all(&mut w).last(), Some(&3000));
    // `clear` and `reset` empty every archetype and keep them.
    w.clear();
    assert_eq!(all(&mut w), []);
    let back = w.spawn((Value(7), Marker::<0>, Marker::<1>));
    assert_eq!(all(&mut w), [7]);
    w.reset();
    assert_eq!(all(&mut w), []);
    assert!(w.spawn_at(back, (Value(8), Marker::<0>, Marker::<1>)));
    assert_eq!(all(&mut w), [8]);
    assert_eq!(w.query_cache_stats().indexed, 1);
    assert!(w.query_cache_stats().hits > hits + 8, "the index was in use throughout");
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn unused_signatures_are_dropped_when_the_table_is_full() {
    let mut w = World::new();
    fragmented(&mut w);
    let mut policy = QueryCachePolicy::new();
    policy.max_signatures = 2;
    w.set_query_cache_policy(policy);
    let a = |w: &mut World| {
        w.query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<1>>>()
            .count()
    };
    let b = |w: &mut World| {
        w.query::<&Value>()
            .filter::<With<Marker<2>>>()
            .filter::<With<Marker<3>>>()
            .count()
    };
    let c = |w: &mut World| {
        w.query::<&Value>()
            .filter::<With<Marker<4>>>()
            .filter::<With<Marker<5>>>()
            .count()
    };
    for _ in 0..3 {
        assert_eq!((a(&mut w), b(&mut w)), (32, 32));
    }
    assert_eq!(w.query_cache_stats().indexed, 2);
    // `c` has no room while `a` and `b` are in use; it still answers.
    for _ in 0..200 {
        assert_eq!((a(&mut w), b(&mut w), c(&mut w)), (32, 32, 32));
    }
    assert_eq!(
        (w.query_cache_stats().signatures, w.query_cache_stats().evictions),
        (2, 0)
    );
    // `b` stops: it is dropped, and `c` takes its place.
    for _ in 0..200 {
        assert_eq!((a(&mut w), c(&mut w)), (32, 32));
    }
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed, stats.evictions), (2, 2, 1));
    assert_eq!(b(&mut w), 32);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn the_memory_bound_keeps_the_index_small() {
    let mut w = World::new();
    fragmented(&mut w);
    let mut policy = QueryCachePolicy::new();
    policy.max_index_bytes = 40 * 4;
    w.set_query_cache_policy(policy);
    for _ in 0..3 {
        assert_eq!(
            w.query::<&Value>()
                .filter::<With<Marker<0>>>()
                .filter::<With<Marker<1>>>()
                .count(),
            32
        );
        assert_eq!(
            w.query::<&Value>()
                .filter::<With<Marker<2>>>()
                .filter::<With<Marker<3>>>()
                .count(),
            32
        );
        assert_eq!(w.query::<&Value>().cached().filter::<With<Marker<4>>>().count(), 64);
    }
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed, stats.indices), (3, 1, 32));
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn chunk_queries_use_the_index_and_take_the_same_hints() {
    let mut w = World::new();
    fragmented(&mut w);
    let (value, m0, m1) = (
        w.register::<Value>(),
        w.register::<Marker<0>>(),
        w.register::<Marker<1>>(),
    );
    let filter = DynFilter::Without(m1).compile().unwrap();
    let ids = [value, m0];
    let rows = |w: &mut World, cache: Option<QueryCache>| {
        let mut rows = Vec::new();
        let query = ChunkQuery::new(&ids).writes(0).filter(&filter);
        let collect = |chunk: Chunk<'_>| rows.extend_from_slice(chunk.entities);
        match cache {
            None => w.raw().query_chunks(query, collect),
            Some(cache) => w.raw().query_chunks(query.cache(cache), collect),
        }
        rows
    };
    let scanned = rows(&mut w, Some(QueryCache::Never));
    assert_eq!((scanned.len(), w.query_cache_stats().signatures), (32, 0));
    for _ in 0..3 {
        assert_eq!(rows(&mut w, None), scanned);
    }
    assert_eq!((w.query_cache_stats().indexed, w.query_cache_stats().hits), (1, 2));
    assert_eq!(rows(&mut w, Some(QueryCache::Auto)), scanned);
    assert_eq!(rows(&mut w, Some(QueryCache::Always)), scanned);
    // The typed query with the same meaning uses the same entry.
    let typed: Vec<Entity> = w
        .query::<(Entity, &Value, &Marker<0>)>()
        .filter::<Without<Marker<1>>>()
        .iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(typed, scanned);
    assert_eq!(w.query_cache_stats().signatures, 1);
}

#[cfg(feature = "parallel")]
#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn automatic_parallel_queries_keep_their_order() {
    let mut w = World::new();
    fragmented(&mut w);
    let expected: Vec<_> = w
        .query::<&Value>()
        .uncached()
        .filter::<With<Marker<0>>>()
        .filter::<With<Marker<3>>>()
        .iter()
        .map(|v| v.0)
        .collect();
    for _ in 0..3 {
        let mut actual = Vec::new();
        w.query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<3>>>()
            .par_collect_into(&mut actual, |v| v.0);
        assert_eq!(actual, expected);
    }
    assert_eq!(w.query_cache_stats().hits, 2);
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn change_detection_is_the_same_with_and_without_the_index() {
    let stamps = |hint: QueryCache| {
        let mut w = World::new();
        fragmented(&mut w);
        w.track_changes::<Value>();
        let seen = w.advance_change_tick();
        for _ in 0..3 {
            let q = w
                .query::<&mut Value>()
                .filter::<With<Marker<0>>>()
                .filter::<With<Marker<1>>>();
            let mut q = match hint {
                QueryCache::Always => q.cached(),
                QueryCache::Never => q.uncached(),
                _ => q,
            };
            q.for_each(|v| v.0 += 1);
        }
        let changed: Vec<(Entity, usize)> = w
            .query::<(Entity, &Value)>()
            .filter::<Changed<Value>>()
            .since(seen)
            .iter()
            .map(|(e, v)| (e, v.0))
            .collect();
        (changed, w.change_tick())
    };
    let scanned = stamps(QueryCache::Never);
    assert_eq!(scanned.0.len(), 32);
    assert_eq!(stamps(QueryCache::Auto), scanned);
    assert_eq!(stamps(QueryCache::Always), scanned);
}

#[derive(Component)]
struct Partner(Entity);
#[derive(Component)]
struct Score(u32);

/// A lookup reads the entity table and the archetypes while the query
/// loops. Finding the query's archetypes through the index (and building
/// it) must leave what the lookup reads alone; Miri checks that it does.
#[test]
fn a_lookup_works_while_its_query_uses_the_index() {
    // Under Miri a small world with the threshold lowered: the same code.
    let mut w = if cfg!(miri) {
        small()
    } else {
        let mut w = World::new();
        fragmented(&mut w);
        w
    };
    let on = w.query_cache_policy();
    let scored = w.spawn((Score(0), Marker::<1>));
    for _ in 0..3 {
        w.spawn((Partner(scored), Marker::<0>, Marker::<2>));
    }
    let policies = [on, QueryCachePolicy::off()];
    for (round, policy) in policies.into_iter().enumerate() {
        w.set_query_cache_policy(policy);
        // First use, promotion, then hits; hinted and not.
        for _ in 0..3 {
            w.query::<&Partner>()
                .filter::<With<Marker<0>>>()
                .with_lookup::<&mut Score>()
                .for_each(|partner, others| others.get_mut(partner.0).unwrap().0 += 1);
            w.query::<&Partner>()
                .filter::<With<Marker<2>>>()
                .cached()
                .with_lookup::<&mut Score>()
                .for_each(|partner, others| others.get_mut(partner.0).unwrap().0 += 1);
        }
        assert_eq!(w.get::<Score>(scored).unwrap().0, 18 * (round as u32 + 1));
        assert_eq!(w.query_cache_stats().indexed, if round == 0 { 2 } else { 0 });
    }
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn a_policy_above_100_percent_is_kept_as_100() {
    let mut w = World::new();
    let mut policy = QueryCachePolicy::new();
    policy.max_match_percent = 200;
    w.set_query_cache_policy(policy);
    assert_eq!(w.query_cache_policy().max_match_percent, 100);
    // And it means every archetype: a query that matches all of them is
    // indexed on its second use.
    fragmented(&mut w);
    for _ in 0..2 {
        assert_eq!(w.query::<&Value>().count(), 128);
    }
    assert_eq!(w.query_cache_stats().indexed, 1);
}

// ---- small worlds ---------------------------------------------------------------------------
//
// The index with a threshold of one archetype, in worlds of 16: every path
// that reads it, cheap enough for Miri.

/// 16 archetypes (every subset of four markers), one entity in each, and
/// a policy that indexes from the first archetype on.
fn small() -> World {
    let mut w = World::new();
    let mut policy = QueryCachePolicy::new();
    policy.min_archetypes = 1;
    w.set_query_cache_policy(policy);
    for bits in 0..16usize {
        let e = w.spawn((Value(bits),));
        macro_rules! m {
            ($($n:literal)*) => {$(
                if bits & (1 << $n) != 0 {
                    w.insert(e, Marker::<$n>);
                }
            )*};
        }
        m!(0 1 2 3);
    }
    w
}

/// What every way of running one query sees, in order.
fn every_path(w: &mut World, hint: QueryCache) -> Vec<Vec<usize>> {
    fn q<'w>(w: &'w mut World, hint: QueryCache) -> Query<'w, (Entity, &'w mut Value)> {
        let q = w
            .query::<(Entity, &mut Value)>()
            .filter::<With<Marker<0>>>()
            .filter::<Or<(With<Marker<1>>, Without<Marker<2>>)>>();
        match hint {
            QueryCache::Always => q.cached(),
            QueryCache::Never => q.uncached(),
            _ => q,
        }
    }
    let mut seen = Vec::new();
    let mut each = Vec::new();
    q(w, hint).for_each(|(_, v)| each.push(v.0));
    let entities: Vec<Entity> = q(w, hint).iter().map(|(e, _)| e).collect();
    seen.push(q(w, hint).iter().map(|(_, v)| v.0).collect());
    seen.push(vec![q(w, hint).count()]);
    seen.push(
        entities
            .iter()
            .map(|&e| q(w, hint).get(e).map(|(_, v)| v.0).unwrap())
            .collect(),
    );
    #[cfg(feature = "parallel")]
    {
        let mut collected = Vec::new();
        q(w, hint).par_collect_into(&mut collected, |(_, v)| v.0);
        seen.push(collected);
        let sum = std::sync::atomic::AtomicUsize::new(0);
        q(w, hint).par_for_each(|(_, v)| {
            sum.fetch_add(v.0, std::sync::atomic::Ordering::Relaxed);
        });
        seen.push(vec![sum.into_inner()]);
    }
    // Chunk queries, typed ids and a runtime filter.
    let ids = [w.register::<Value>(), w.register::<Marker<0>>()];
    let filter = DynFilter::Or(vec![
        DynFilter::With(w.register::<Marker<1>>()),
        DynFilter::Without(w.register::<Marker<2>>()),
    ])
    .compile()
    .unwrap();
    let mut rows = Vec::new();
    w.raw()
        .query_chunks(ChunkQuery::new(&ids).filter(&filter).cache(hint), |chunk| {
            rows.push(chunk.len());
        });
    seen.push(vec![rows.iter().sum()]);
    let mut rows = 0;
    w.raw().for_each_chunk(&ids, |chunk| rows += chunk.len());
    seen.push(vec![rows]);
    seen.push(each);
    seen
}

#[test]
fn every_query_path_reads_a_small_index_and_sees_what_a_scan_sees() {
    let mut w = small();
    let scanned = every_path(&mut w, QueryCache::Never);
    assert_eq!(scanned[1], [6], "6 of 16 archetypes");
    let stats = w.query_cache_stats();
    assert_eq!(
        (stats.signatures, stats.indexed),
        (1, 0),
        "only the chunk query without a hint was noticed"
    );
    for hint in [QueryCache::Auto, QueryCache::Always, QueryCache::Auto] {
        assert_eq!(every_path(&mut w, hint), scanned, "{hint:?}");
    }
    let stats = w.query_cache_stats();
    assert!(stats.indexed >= 2 && stats.hits > 8, "{stats:?}");

    // New archetypes, moves, an emptied archetype, a late component,
    // `clear` and `reset`: the index follows, on every path.
    let e = w.spawn((Value(100), Marker::<0>, Marker::<1>, Marker::<5>));
    let moved = w.spawn((Value(101), Marker::<0>, Marker::<2>));
    assert_eq!(
        every_path(&mut w, QueryCache::Auto),
        every_path(&mut w, QueryCache::Never)
    );
    w.remove::<Marker<2>>(moved);
    w.despawn(e);
    w.register::<Marker<9>>();
    let after = every_path(&mut w, QueryCache::Auto);
    assert_eq!(after, every_path(&mut w, QueryCache::Never));
    assert_eq!(after[1], [7]);
    w.clear();
    assert_eq!(every_path(&mut w, QueryCache::Auto)[1], [0]);
    w.spawn((Value(7), Marker::<0>));
    assert_eq!(every_path(&mut w, QueryCache::Auto)[0], [7]);
    w.reset();
    assert_eq!(every_path(&mut w, QueryCache::Auto)[1], [0]);
    w.spawn((Value(8), Marker::<0>, Marker::<1>, Marker::<2>));
    assert_eq!(
        every_path(&mut w, QueryCache::Auto),
        every_path(&mut w, QueryCache::Never)
    );
}

#[test]
fn a_small_full_index_sweeps_and_a_small_budget_scans() {
    let mut w = small();
    let mut policy = w.query_cache_policy();
    policy.max_signatures = 2;
    w.set_query_cache_policy(policy);
    let frame = |w: &mut World| {
        let a = w
            .query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<1>>>()
            .count();
        let b = w
            .query::<&Value>()
            .filter::<With<Marker<2>>>()
            .filter::<With<Marker<3>>>()
            .count();
        let c = w
            .query::<&Value>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<3>>>()
            .count();
        assert_eq!((a, b, c), (4, 4, 4));
    };
    // Enough refusals for a sweep.
    for _ in 0..36 {
        frame(&mut w);
    }
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed), (2, 2));
    // Room for three archetype numbers: a signature that matches four scans.
    policy.max_index_bytes = 12;
    w.set_query_cache_policy(policy);
    for _ in 0..3 {
        frame(&mut w);
        assert_eq!(w.query::<&Value>().filter::<With<Marker<0>>>().cached().count(), 8);
    }
    let stats = w.query_cache_stats();
    assert_eq!((stats.indexed, stats.indices), (0, 0));
}

#[test]
#[cfg_attr(miri, ignore = "a big world; Miri runs the same paths in the small worlds below")]
fn single_reads_through_the_index() {
    let mut w = World::new();
    fragmented(&mut w);
    let all = |w: &mut World| {
        w.query::<&Value>()
            .cached()
            .filter::<(With<Marker<0>>, With<Marker<1>>, With<Marker<2>>, With<Marker<3>>)>()
            .filter::<(With<Marker<4>>, With<Marker<5>>, With<Marker<6>>)>()
            .single()
            .map(|v| v.0)
    };
    assert_eq!(all(&mut w), Ok(127));
    let before = w.query_cache_stats().hits;
    assert_eq!(all(&mut w), Ok(127));
    assert_eq!(
        w.query_cache_stats().hits,
        before + 1,
        "the second call reads the index"
    );
    assert_eq!(
        w.query::<&Value>().cached().filter::<With<Marker<6>>>().single().err(),
        Some(SingleError::MultipleEntities)
    );
    ViewState::new().read::<Value>().run(&mut w, |mut view| {
        let found = view
            .query::<&Value>()
            .cached()
            .filter::<(With<Marker<0>>, With<Marker<1>>, With<Marker<2>>, With<Marker<3>>)>()
            .filter::<(With<Marker<4>>, With<Marker<5>>, With<Marker<6>>)>()
            .single()
            .map(|v| v.0);
        assert_eq!(found, Ok(127));
        let several = view.query::<&Value>().cached().filter::<With<Marker<6>>>().single();
        assert_eq!(several.err(), Some(SingleError::MultipleEntities));
    });
}

#[test]
fn late_columns_agree_across_cached_query_entry_points() {
    #[derive(Component)]
    struct Other(u32);
    #[derive(Component)]
    struct Extra(u16);

    let mut world = World::new();
    let mut policy = QueryCachePolicy::new();
    policy.min_archetypes = 1;
    world.set_query_cache_policy(policy);
    for value in 0..2 {
        let entity = world.spawn((
            Marker::<0>,
            Marker::<1>,
            Marker::<2>,
            Marker::<3>,
            Marker::<4>,
            Marker::<5>,
            Marker::<6>,
            Marker::<7>,
            Marker::<8>,
            Marker::<9>,
            Marker::<10>,
            Marker::<11>,
        ));
        world.insert(entity, Marker::<12>);
        world.insert(entity, Marker::<13>);
        world.insert(entity, Marker::<14>);
        world.insert(entity, Marker::<15>);
        world.insert(entity, Marker::<16>);
        world.insert(entity, Value(40 + value));
        world.insert(entity, Other(260 + value as u32));
        if value == 1 {
            world.insert(entity, Extra(7));
        }
    }
    let project =
        |(other, value, extra): (&Other, &Value, Option<&Extra>)| (other.0, value.0, extra.map(|extra| extra.0));
    let expected: Vec<_> = world
        .query::<(&Other, &Value, Option<&Extra>)>()
        .uncached()
        .iter()
        .map(project)
        .collect();
    assert_eq!(expected, [(260, 40, None), (261, 41, Some(7))]);

    let expected_pairs: Vec<_> = expected.iter().map(|&(other, value, _)| (other, value)).collect();
    let mut each = Vec::new();
    world
        .query::<(&Other, &Value, Option<&Extra>)>()
        .cached()
        .for_each(|item| each.push(project(item)));
    assert_eq!(each, expected);
    let iter: Vec<_> = world
        .query::<(&Other, &Value, Option<&Extra>)>()
        .cached()
        .iter()
        .map(project)
        .collect();
    assert_eq!(iter, expected);

    let mut state = ViewState::new().read::<Value>().read::<Other>().read::<Extra>();
    state.run(&mut world, |mut view| {
        let mut seen = Vec::new();
        view.query::<(&Other, &Value, Option<&Extra>)>()
            .cached()
            .for_each(|item| seen.push(project(item)));
        assert_eq!(seen, expected);
    });

    let ids = [
        world.component_id::<Other>().unwrap(),
        world.component_id::<Value>().unwrap(),
    ];
    let mut chunks = Vec::new();
    let mut collect_chunk = |chunk: Chunk<'_>| {
        for row in 0..chunk.len() {
            // SAFETY: the requested columns in their declared order, each
            // live for every row in the chunk.
            let (other, value) = unsafe {
                (
                    &*chunk.columns[0].cast::<Other>().add(row),
                    &*chunk.columns[1].cast::<Value>().add(row),
                )
            };
            chunks.push((other.0, value.0));
        }
    };
    world
        .raw()
        .query_chunks(ChunkQuery::new(&ids).cache(QueryCache::Always), &mut collect_chunk);
    world.raw().for_each_chunk(&ids, &mut collect_chunk);
    assert_eq!(&chunks[..2], expected_pairs);
    assert_eq!(&chunks[2..], expected_pairs);
    assert_eq!(world.query_cache_stats().signatures, 1);

    #[cfg(feature = "parallel")]
    {
        let mut parallel = Vec::new();
        world
            .query::<(&Other, &Value, Option<&Extra>)>()
            .cached()
            .par_collect_into(&mut parallel, project);
        assert_eq!(parallel, expected);
    }
    world.track_changes::<Value>();
    let seen = world.advance_change_tick();
    world
        .query::<(&Other, &mut Value)>()
        .cached()
        .for_each(|(other, value)| {
            assert_eq!(other.0 as usize - value.0, 220);
            value.0 += 1;
        });
    let changed: Vec<_> = world
        .query::<(&Other, &Value)>()
        .filter::<Changed<Value>>()
        .cached()
        .since(seen)
        .iter()
        .map(|(other, value)| (other.0, value.0))
        .collect();
    assert_eq!(changed, [(260, 41), (261, 42)]);
}
