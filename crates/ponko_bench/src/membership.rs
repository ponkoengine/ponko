//! Archetype component membership and structural-transition costs (issue #144).
//!
//! Everything here goes through ponko_ecs's public API, so the group needs no
//! change to the engine. Group name: `membership/`; run it with
//! `cargo run --release -p ponko_bench -- membership`.
//!
//! What is measured, for archetypes of 2, 8, 16, 32 and 64 components:
//!
//! - `membership/has/*`: `Archetype::has` (a linear scan of the sorted id
//!   list) against candidate layouts built from the same ids: binary search,
//!   a bitset, a flat id→index table, rank by counting, a width hybrid and an
//!   8-wide block compare. Hits and misses, with the
//!   archetype's ids packed at the bottom of the id space ("dense") or
//!   scattered over 256 ids ("sparse"). Misses compare ids that are not in the
//!   set, which scans the whole list.
//! - `membership/index/*`: the same for a component's column index
//!   (`column_index` in storage.rs): a linear `position`, the public
//!   `column_ptr` wrapper, a bitset with popcount rank, the flat table,
//!   binary search and the same three extra candidates.
//! - `membership/colmap/*`: the column mapping `move_row` computes for every
//!   structural change (for each column of the source, where is it in the
//!   destination?): `n` linear scans, as before #144, against the merge pass
//!   over the two sorted id lists the engine uses now, a bitset and a table.
//! - `membership/transition/*`: real `insert_dynamic`/`remove_dynamic` pairs
//!   on a world: `warm` (edge cached, one component toggled), the same for a
//!   component with a drop function (`with-drop-fn`), `cycle32` (32
//!   different components toggled in turn, so 64 edges are in use) and `cold`
//!   (fresh world: the first add and remove, which resolve the edge and
//!   create the archetype).
//! - `membership/edge-lookup/*`: the transition-edge cache lookup itself
//!   with SipHash (before #144) against the one-multiply hasher the engine
//!   uses now (`ComponentIdHasher` in storage.rs) and a short linear list,
//!   for 1 to 64 edges.
//! - `membership/query/*`: a real `(&mut P, &V)` query over 4096 archetypes of
//!   4 to 64 components (4 entities each), with the queried components the
//!   first or the last ids of every archetype, hot and after a 128 MiB cache
//!   flush (the first frame after the world was out of cache), with heap
//!   allocations per run. `lookup-*` rows do only the two column lookups over
//!   the same archetypes with each candidate. Results and the conclusion:
//!   `crates/ponko_bench/MEMBERSHIP.md`.
//!
//! Candidates are interleaved round by round, so slow drift on a busy machine
//! affects all of them alike. Each line shows nanoseconds per operation.
//! Lookups are independent of each other, so they measure throughput, not
//! latency.

use ponko_ecs::{Archetype, ComponentId, Entity, Without, World};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::hint::black_box;
use std::time::Instant;

/// Rust component that every benchmark entity starts with (id 0).
#[derive(ponko_ecs::Component)]
struct Seed(#[allow(dead_code)] u32);

/// Components registered in the world: ids `0..REGISTERED`.
const REGISTERED: usize = 320;
/// Archetypes draw their components from `0..SPREAD`; ids `SPREAD..REGISTERED`
/// are never part of an archetype's set (misses, and the components that
/// transitions add and remove).
const SPREAD: usize = 256;
const WIDTHS: [usize; 5] = [2, 8, 16, 32, 64];
const LOOKUPS: usize = 16_384;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// The archetype's ids are `0..n`.
    Dense,
    /// `0` and `n - 1` pseudo-random ids below [`SPREAD`].
    Sparse,
}

impl Layout {
    fn name(self) -> &'static str {
        match self {
            Layout::Dense => "dense",
            Layout::Sparse => "sparse",
        }
    }
}

// ---- deterministic helpers --------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// The sorted component indexes of an `n`-component archetype.
fn pick(layout: Layout, n: usize) -> Vec<usize> {
    match layout {
        Layout::Dense => (0..n).collect(),
        Layout::Sparse => {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ n as u64);
            let mut pool: Vec<usize> = (1..SPREAD).collect();
            rng.shuffle(&mut pool);
            let mut set: Vec<usize> = std::iter::once(0).chain(pool.into_iter().take(n - 1)).collect();
            set.sort_unstable();
            set
        }
    }
}

/// A world with `REGISTERED` components; `ids[i].index() == i`.
fn registered_world() -> (World, Vec<ComponentId>) {
    let mut w = World::new();
    let mut ids = vec![w.register::<Seed>()];
    for i in 1..REGISTERED {
        // SAFETY: plain 4-byte values, no destructor.
        ids.push(unsafe { w.raw().register_component(&format!("c{i}"), 4, 4, None) });
    }
    assert!(ids.iter().enumerate().all(|(i, id)| id.index() == i));
    (w, ids)
}

fn add(w: &mut World, e: Entity, id: ComponentId) {
    let v = 0u32;
    // SAFETY: `id` is a registered 4-byte component and `v` is 4 bytes.
    assert!(unsafe { w.raw().insert_dynamic(e, id, (&raw const v).cast()) });
}

/// A world with one entity whose archetype has exactly the components `set`
/// (sorted indexes, starting with 0). Ascending inserts: the intermediate
/// archetypes are the prefixes of `set`.
fn world_with(set: &[usize]) -> (World, Vec<ComponentId>, Entity) {
    let (mut w, ids) = registered_world();
    let e = w.spawn((Seed(0),));
    for &i in &set[1..] {
        add(&mut w, e, ids[i]);
    }
    (w, ids, e)
}

fn archetype_of<'a>(w: &'a World, set: &[ComponentId]) -> &'a Archetype {
    w.raw_ref()
        .archetypes()
        .iter()
        .find(|a| a.component_ids() == set)
        .expect("archetype exists")
}

// ---- candidate layouts ------------------------------------------------------

/// One bit per component id; the column index of a present id is the number of
/// set bits below it (per-word prefix counts plus a popcount in the word).
struct Bits {
    words: Box<[u64]>,
    prefix: Box<[u16]>,
}

impl Bits {
    fn new(ids: &[ComponentId]) -> Self {
        let mut words = vec![0u64; REGISTERED.div_ceil(64)];
        for id in ids {
            words[id.index() >> 6] |= 1 << (id.index() & 63);
        }
        let mut prefix = Vec::with_capacity(words.len());
        let mut sum = 0u16;
        for w in &words {
            prefix.push(sum);
            sum += w.count_ones() as u16;
        }
        Bits {
            words: words.into(),
            prefix: prefix.into(),
        }
    }

    #[inline]
    fn has(&self, id: ComponentId) -> bool {
        let i = id.index();
        self.words.get(i >> 6).is_some_and(|w| w >> (i & 63) & 1 != 0)
    }

    #[inline]
    fn index(&self, id: ComponentId) -> Option<usize> {
        let i = id.index();
        let w = *self.words.get(i >> 6)?;
        let bit = 1u64 << (i & 63);
        (w & bit != 0).then(|| self.prefix[i >> 6] as usize + (w & (bit - 1)).count_ones() as usize)
    }

    fn bytes(&self) -> usize {
        self.words.len() * 8 + self.prefix.len() * 2
    }
}

/// `table[id]` is the column index, or `u16::MAX`.
struct Table(Box<[u16]>);

impl Table {
    fn new(ids: &[ComponentId]) -> Self {
        let mut t = vec![u16::MAX; REGISTERED];
        for (col, id) in ids.iter().enumerate() {
            t[id.index()] = col as u16;
        }
        Table(t.into())
    }

    #[inline]
    fn index(&self, id: ComponentId) -> Option<usize> {
        match self.0.get(id.index()) {
            Some(&c) if c != u16::MAX => Some(c as usize),
            _ => None,
        }
    }

    fn bytes(&self) -> usize {
        self.0.len() * 2
    }
}

#[inline]
fn linear_index(ids: &[ComponentId], id: ComponentId) -> Option<usize> {
    ids.iter().position(|&c| c == id)
}

/// Linear scan up to 8 ids, binary search above.
#[inline]
fn hybrid_index(ids: &[ComponentId], id: ComponentId) -> Option<usize> {
    if ids.len() <= 8 {
        linear_index(ids, id)
    } else {
        ids.binary_search(&id).ok()
    }
}

/// Eight ids at a time: an equality mask per block (no branch inside a block,
/// so it can be one vector compare), and since the list is sorted, a block
/// whose last id is above `id` ends the search.
#[inline]
fn chunked_index(ids: &[ComponentId], id: ComponentId) -> Option<usize> {
    let (blocks, rest) = ids.as_chunks::<8>();
    for (k, block) in blocks.iter().enumerate() {
        let mut mask = 0u32;
        for (i, &c) in block.iter().enumerate() {
            mask |= u32::from(c == id) << i;
        }
        if mask != 0 {
            return Some(8 * k + mask.trailing_zeros() as usize);
        }
        if block[7] > id {
            return None;
        }
    }
    rest.iter().position(|&c| c == id).map(|i| 8 * blocks.len() + i)
}

/// Rank by counting: the number of ids below `id` is its column if it is
/// there. No early exit and no data-dependent branch, so the loop vectorizes
/// and costs the same wherever `id` sits in the list.
#[inline]
fn rank_index(ids: &[ComponentId], id: ComponentId) -> Option<usize> {
    let k = ids.iter().filter(|&&c| c < id).count();
    (ids.get(k) == Some(&id)).then_some(k)
}

/// Column mapping with one merge pass over two sorted id lists. Returns the
/// sum of the matched destination indexes (so the work cannot be optimized away).
fn merge_map(src: &[ComponentId], dst: &[ComponentId]) -> usize {
    let (mut j, mut sum) = (0, 0);
    for &s in src {
        while j < dst.len() && dst[j] < s {
            j += 1;
        }
        if j < dst.len() && dst[j] == s {
            sum += j;
        }
    }
    sum
}

fn linear_map(src: &[ComponentId], dst: &[ComponentId]) -> usize {
    src.iter().filter_map(|&s| linear_index(dst, s)).sum()
}

fn bits_map(src: &[ComponentId], dst: &Bits) -> usize {
    src.iter().filter_map(|&s| dst.index(s)).sum()
}

fn table_map(src: &[ComponentId], dst: &Table) -> usize {
    src.iter().filter_map(|&s| dst.index(s)).sum()
}

// ---- timing harness ---------------------------------------------------------

type Case<'a> = (&'static str, Box<dyn FnMut() -> usize + 'a>);

fn skips(filter: Option<&str>, name: &str) -> bool {
    filter.is_some_and(|f| !name.contains(f))
}

/// Times every case once per round, `rounds` rounds, so the cases share the
/// same noise. Prints `ns per op` (median and minimum over the rounds).
fn compare(filter: Option<&str>, group: &str, per_op: usize, rounds: usize, mut cases: Vec<Case<'_>>) {
    if cases
        .iter()
        .all(|(name, _)| skips(filter, &format!("membership/{group}/{name}")))
    {
        return;
    }
    let mut samples: Vec<Vec<f64>> = vec![Vec::with_capacity(rounds); cases.len()];
    for round in 0..rounds + 3 {
        for (i, (_, f)) in cases.iter_mut().enumerate() {
            let t = Instant::now();
            black_box(f());
            let ns = t.elapsed().as_secs_f64() * 1e9 / per_op as f64;
            if round >= 3 {
                samples[i].push(ns);
            }
        }
    }
    for ((name, _), s) in cases.iter().zip(samples.iter_mut()) {
        let full = format!("membership/{group}/{name}");
        if skips(filter, &full) {
            continue;
        }
        s.sort_by(f64::total_cmp);
        println!("{full:<48} median {:>9.2} ns   min {:>9.2} ns", s[s.len() / 2], s[0]);
    }
}

// ---- the benchmarks ---------------------------------------------------------

/// `LOOKUPS` ids from `pool`, in shuffled order, repeating `pool` as needed.
fn queries(pool: &[ComponentId], seed: u64) -> Vec<ComponentId> {
    let mut rng = Rng(seed);
    let mut q: Vec<ComponentId> = pool.iter().copied().cycle().take(LOOKUPS).collect();
    rng.shuffle(&mut q);
    q
}

fn lookups(filter: Option<&str>) {
    for layout in [Layout::Dense, Layout::Sparse] {
        for n in WIDTHS {
            let tag = format!("n{n}-{}", layout.name());
            let set = pick(layout, n);
            let (w, ids, _) = world_with(&set);
            let mine: Vec<ComponentId> = set.iter().map(|&i| ids[i]).collect();
            let arch = archetype_of(&w, &mine);
            let missing: Vec<ComponentId> = (0..REGISTERED).filter(|i| !set.contains(i)).map(|i| ids[i]).collect();
            let hit = queries(&mine, 1);
            let miss = queries(&missing, 2);
            let bits = Bits::new(&mine);
            let table = Table::new(&mine);

            for (kind, q) in [("hit", &hit), ("miss", &miss)] {
                let (arch, bits, mine) = (arch, &bits, &mine);
                let table = &table;
                compare(
                    filter,
                    "has",
                    LOOKUPS,
                    300,
                    vec![
                        (
                            leak(format!("{tag}/{kind}/linear(Archetype::has)")),
                            Box::new(move || q.iter().filter(|&&id| arch.has(id)).count()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/binary-search")),
                            Box::new(move || q.iter().filter(|&&id| mine.binary_search(&id).is_ok()).count()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/bitset")),
                            Box::new(move || q.iter().filter(|&&id| bits.has(id)).count()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/table")),
                            Box::new(move || q.iter().filter(|&&id| table.index(id).is_some()).count()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/rank-count")),
                            Box::new(move || q.iter().filter(|&&id| rank_index(mine, id).is_some()).count()),
                        ),
                    ],
                );
                compare(
                    filter,
                    "index",
                    LOOKUPS,
                    300,
                    vec![
                        (
                            leak(format!("{tag}/{kind}/linear(position)")),
                            Box::new(move || q.iter().filter_map(|&id| linear_index(mine, id)).sum()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/Archetype::column_ptr")),
                            Box::new(move || q.iter().filter_map(|&id| arch.column_ptr(id)).count()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/bitset-rank")),
                            Box::new(move || q.iter().filter_map(|&id| bits.index(id)).sum()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/table")),
                            Box::new(move || q.iter().filter_map(|&id| table.index(id)).sum()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/binary-search")),
                            Box::new(move || q.iter().filter_map(|&id| mine.binary_search(&id).ok()).sum()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/rank-count")),
                            Box::new(move || q.iter().filter_map(|&id| rank_index(mine, id)).sum()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/hybrid-linear8-binary")),
                            Box::new(move || q.iter().filter_map(|&id| hybrid_index(mine, id)).sum()),
                        ),
                        (
                            leak(format!("{tag}/{kind}/chunked8")),
                            Box::new(move || q.iter().filter_map(|&id| chunked_index(mine, id)).sum()),
                        ),
                    ],
                );
            }
        }
    }
}

/// Case names must outlive the closures; the benchmark process is short-lived.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The column mapping of an add (`n` → `n + 1` components) and of a remove.
fn colmap(filter: Option<&str>) {
    for layout in [Layout::Dense, Layout::Sparse] {
        for n in WIDTHS {
            let tag = format!("n{n}-{}", layout.name());
            let set = pick(layout, n);
            let (_, ids) = registered_world();
            let small: Vec<ComponentId> = set.iter().map(|&i| ids[i]).collect();
            // One extra component from the never-in-a-set range.
            let mut big = small.clone();
            big.push(ids[SPREAD + 7]);
            big.sort_unstable();
            for (dir, src, dst) in [("add", &small, &big), ("remove", &big, &small)] {
                let bits = Bits::new(dst);
                let table = Table::new(dst);
                let (bits, table) = (&bits, &table);
                compare(
                    filter,
                    "colmap",
                    COLMAP_REPS,
                    1000,
                    vec![
                        (
                            leak(format!("{tag}/{dir}/linear(before-#144)")),
                            Box::new(move || {
                                let mut s = 0;
                                for _ in 0..COLMAP_REPS {
                                    s += linear_map(black_box(src), black_box(dst));
                                }
                                s
                            }),
                        ),
                        (
                            leak(format!("{tag}/{dir}/merge(engine)")),
                            Box::new(move || {
                                let mut s = 0;
                                for _ in 0..COLMAP_REPS {
                                    s += merge_map(black_box(src), black_box(dst));
                                }
                                s
                            }),
                        ),
                        (
                            leak(format!("{tag}/{dir}/bitset-rank")),
                            Box::new(move || {
                                let mut s = 0;
                                for _ in 0..COLMAP_REPS {
                                    s += bits_map(black_box(src), black_box(bits));
                                }
                                s
                            }),
                        ),
                        (
                            leak(format!("{tag}/{dir}/table")),
                            Box::new(move || {
                                let mut s = 0;
                                for _ in 0..COLMAP_REPS {
                                    s += table_map(black_box(src), black_box(table));
                                }
                                s
                            }),
                        ),
                    ],
                );
            }
        }
    }
}

const TOGGLES: usize = 512;
/// Column mappings per timed round (one mapping is only a few ns for small archetypes).
const COLMAP_REPS: usize = 256;

/// Real structural changes on a world, per transition (one insert or one remove).
fn transitions(filter: Option<&str>) {
    for n in WIDTHS {
        let set = pick(Layout::Sparse, n);
        let name = format!("n{n}-sparse");
        if skips(filter, &format!("membership/transition/{name}")) {
            continue;
        }
        let (mut w, ids, e) = world_with(&set);
        let extra0 = ids[SPREAD];
        let with_drop = register_with_drop(&mut w);
        assert!(add_with_drop(&mut w, e, with_drop));
        assert!(w.raw().remove_dynamic(e, with_drop));
        let extras: Vec<ComponentId> = ids[SPREAD..SPREAD + 32].to_vec();
        // Visit every edge once, so each case below starts with the edges cached.
        for &x in &extras {
            add(&mut w, e, x);
            assert!(w.raw().remove_dynamic(e, x));
        }
        let w = std::cell::RefCell::new(w);
        compare(
            filter,
            "transition",
            2 * TOGGLES,
            300,
            vec![
                (
                    leak(format!("{name}/warm-1-edge")),
                    Box::new(|| {
                        let mut w = w.borrow_mut();
                        for _ in 0..TOGGLES {
                            add(&mut w, e, extra0);
                            w.raw().remove_dynamic(e, extra0);
                        }
                        0
                    }),
                ),
                (
                    leak(format!("{name}/warm-1-edge-with-drop-fn")),
                    Box::new(|| {
                        let mut w = w.borrow_mut();
                        for _ in 0..TOGGLES {
                            add_with_drop(&mut w, e, with_drop);
                            w.raw().remove_dynamic(e, with_drop);
                        }
                        0
                    }),
                ),
                (
                    leak(format!("{name}/cycle32-64-edges")),
                    Box::new(|| {
                        let mut w = w.borrow_mut();
                        for k in 0..TOGGLES {
                            let x = extras[k % extras.len()];
                            add(&mut w, e, x);
                            w.raw().remove_dynamic(e, x);
                        }
                        0
                    }),
                ),
            ],
        );
        // Every edge lookup hits after the first visit. A miss copies the id
        // list (`to_vec`/`collect` in world.rs), which allocates even when the
        // archetype it finds already exists, so the archetype count can't show
        // a miss but the allocation count can: one more pass over every edge
        // used above must not allocate.
        {
            let mut w = w.borrow_mut();
            let before = crate::alloc::Counts::now();
            add(&mut w, e, extra0);
            w.raw().remove_dynamic(e, extra0);
            add_with_drop(&mut w, e, with_drop);
            w.raw().remove_dynamic(e, with_drop);
            for &x in &extras {
                add(&mut w, e, x);
                w.raw().remove_dynamic(e, x);
            }
            let allocs = crate::alloc::Counts::now().since(before).count;
            assert_eq!(allocs, 0, "membership/transition/{name}: an edge lookup missed");
        }

        // Cold: a fresh world, timing the first add and remove of one component.
        let full = format!("membership/transition/{name}/cold-first-add+remove");
        if !skips(filter, &full) {
            let mut times = Vec::new();
            for round in 0..203 {
                let (mut w, ids, e) = world_with(&set);
                let t = Instant::now();
                add(&mut w, e, ids[SPREAD]);
                w.raw().remove_dynamic(e, ids[SPREAD]);
                let ns = t.elapsed().as_secs_f64() * 1e9 / 2.0;
                black_box(&w);
                if round >= 3 {
                    times.push(ns);
                }
            }
            times.sort_by(f64::total_cmp);
            println!(
                "{full:<48} median {:>9.2} ns   min {:>9.2} ns",
                times[times.len() / 2],
                times[0]
            );
        }
    }
}

/// Drop function of the `with-drop` component: removing a component that has
/// one moves the value out before dropping it.
unsafe extern "C-unwind" fn noop_drop(_: *mut u8) {}

/// Registers `with-drop`: 8 bytes, with a drop function that does nothing.
fn register_with_drop(w: &mut World) -> ComponentId {
    // SAFETY: plain 8-byte values; `noop_drop` accepts any of them.
    unsafe { w.raw().register_component("with-drop", 8, 8, Some(noop_drop)) }
}

/// Adds (or replaces) `e`'s `with-drop` value; false for a dead entity.
fn add_with_drop(w: &mut World, e: Entity, id: ComponentId) -> bool {
    let value = 0u64;
    // SAFETY: `id` is the 8-byte, 8-aligned `with-drop` and `value` is 8
    // aligned bytes.
    unsafe { w.raw().insert_dynamic(e, id, (&raw const value).cast()) }
}

/// Multiplicative hash of the id: the same `write_u32` as the engine's
/// `ComponentIdHasher`, which is crate-private.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8) | u64::from(b);
        }
    }
    fn write_u32(&mut self, n: u32) {
        self.0 = u64::from(n).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

/// The edge cache lookup alone: SipHash (the engine before #144), the
/// one-multiply hasher (the engine now) and a short list.
fn edge_lookup(filter: Option<&str>) {
    let (_, ids) = registered_world();
    for edges in [1, 4, 16, 64] {
        let keys: Vec<ComponentId> = ids[SPREAD..SPREAD + edges].to_vec();
        let sip: HashMap<ComponentId, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let fast: HashMap<ComponentId, usize, BuildHasherDefault<IdHasher>> =
            keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let list: Vec<(ComponentId, usize)> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let q = queries(&keys, 5);
        let (sip, fast, list, q) = (&sip, &fast, &list, &q);
        compare(
            filter,
            "edge-lookup",
            LOOKUPS,
            300,
            vec![
                (
                    leak(format!("e{edges}/hashmap-siphash(before-#144)")),
                    Box::new(move || q.iter().filter_map(|id| sip.get(id)).sum()),
                ),
                (
                    leak(format!("e{edges}/hashmap-multiply-hash(engine)")),
                    Box::new(move || q.iter().filter_map(|id| fast.get(id)).sum()),
                ),
                (
                    leak(format!("e{edges}/linear-list")),
                    Box::new(move || {
                        q.iter()
                            .filter_map(|id| list.iter().find(|(k, _)| k == id).map(|&(_, v)| v))
                            .sum()
                    }),
                ),
            ],
        );
    }
}

/// Bytes per archetype the candidate layouts would add, for a world of
/// `REGISTERED` component types.
fn memory(filter: Option<&str>) {
    if skips(filter, "membership/memory") {
        return;
    }
    let (_, ids) = registered_world();
    let set = pick(Layout::Sparse, 16);
    let mine: Vec<ComponentId> = set.iter().map(|&i| ids[i]).collect();
    println!(
        "membership/memory: per archetype, {REGISTERED} registered components: \
         bitset+rank {} B, flat table {} B; the id list it supplements is {} B at 16 components \
         (4 B each), and a column is 32 B each.",
        Bits::new(&mine).bytes(),
        Table::new(&mine).bytes(),
        16 * 4,
    );
}

// ---- query level: many archetypes, hot and cache-cold ------------------------

/// Archetypes in a query-level world; 4 entities each, so per-archetype work
/// (matching, column lookup) is most of a query's cost, as in worlds with many
/// component combinations. Fewer under Miri, which interprets every step.
const QUERY_ARCHETYPES: usize = if cfg!(miri) { 64 } else { 4096 };
const QUERY_ROWS: usize = 4;
/// Query-level widths: total components per archetype, the two queried ones
/// included. Two would leave no room for 4096 distinct archetypes.
const QUERY_WIDTHS: [usize; 5] = [4, 8, 16, 32, 64];
/// Bytes written between cold samples: more than the M4 Max's last-level
/// cache, so archetype structs, id lists and columns come from memory.
const FLUSH_BYTES: usize = 128 << 20;

#[derive(Clone, Copy, ponko_ecs::Component)]
struct P(f32);
#[derive(Clone, Copy, ponko_ecs::Component)]
struct V(f32);
/// In no archetype: `without::<Tag>()` matches everything and makes `has`
/// scan the whole id list (a miss).
#[derive(ponko_ecs::Component)]
struct Tag;

/// Where the two queried components sit in every archetype's sorted id list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Registered first: ids 0 and 1, so a linear scan finds them at once.
    First,
    /// Registered after the 257 others (`SPREAD` components and `Tag`): the last
    /// two ids, the scan's worst case.
    Last,
}

impl Place {
    fn name(self) -> &'static str {
        match self {
            Place::First => "first",
            Place::Last => "last",
        }
    }
}

/// `QUERY_ARCHETYPES` distinct archetypes of `width` components, each with
/// `P`, `V` and `width - 2` of 256 four-byte components, spawned in one go
/// (no intermediate archetypes). Returns the world and the ids of `P`, `V`.
fn query_world(width: usize, place: Place) -> (World, ComponentId, ComponentId) {
    let mut w = World::new();
    let mut pv = Vec::new();
    if place == Place::First {
        pv = vec![w.register::<P>(), w.register::<V>()];
    }
    let pool: Vec<ComponentId> = (0..SPREAD)
        // SAFETY: plain 4-byte values, no destructor.
        .map(|i| unsafe { w.raw().register_component(&format!("q{i}"), 4, 4, None) })
        .collect();
    w.register::<Tag>();
    if place == Place::Last {
        pv = vec![w.register::<P>(), w.register::<V>()];
    }
    let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ width as u64);
    let mut seen = std::collections::HashSet::new();
    let (p, v, zero) = (P(1.0), V(0.5), 0u32);
    while seen.len() < QUERY_ARCHETYPES {
        let mut picks = pool.clone();
        // Partial shuffle: the first `width - 2` are a uniform random subset.
        for i in 0..width - 2 {
            let j = i + (rng.next() % (picks.len() - i) as u64) as usize;
            picks.swap(i, j);
        }
        picks.truncate(width - 2);
        picks.sort_unstable();
        if !seen.insert(picks.clone()) {
            continue;
        }
        let mut parts: Vec<(ComponentId, *const u8)> =
            vec![(pv[0], (&raw const p).cast()), (pv[1], (&raw const v).cast())];
        parts.extend(picks.iter().map(|&id| (id, (&raw const zero).cast::<u8>())));
        for _ in 0..QUERY_ROWS {
            // SAFETY: every pointer is to a valid, `Copy` value of its component.
            unsafe { w.raw().spawn_dynamic(&parts) };
        }
    }
    assert_eq!(
        w.raw_ref().archetypes().iter().filter(|a| !a.is_empty()).count(),
        QUERY_ARCHETYPES
    );
    (w, pv[0], pv[1])
}

/// Times `f` `samples` times, each after `prepare` (untimed). Returns ns per
/// archetype (median, min) and the allocations of the last timed call.
fn time_each(samples: usize, mut prepare: impl FnMut(), mut f: impl FnMut() -> usize) -> (f64, f64, u64) {
    let mut times = Vec::with_capacity(samples);
    let mut allocs = 0;
    for round in 0..samples + 2 {
        prepare();
        let before = crate::alloc::Counts::now();
        let t = Instant::now();
        black_box(f());
        let ns = t.elapsed().as_secs_f64() * 1e9 / QUERY_ARCHETYPES as f64;
        allocs = crate::alloc::Counts::now().since(before).count;
        if round >= 2 {
            times.push(ns);
        }
    }
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], times[0], allocs)
}

/// A real query over 4096 archetypes: per archetype it matches the filters
/// (`has`) and finds two columns (`column_index`), then visits 4 rows. Hot:
/// back-to-back runs. Cold: 128 MiB read before every run, representing a
/// frame after the world was out of cache without dirtying the flush buffer.
/// `lookup-*` rows are the two column lookups alone over the same archetypes
/// (no rows visited), to show the share of the lookup.
fn query_level(filter: Option<&str>) {
    let flush = vec![1u64; FLUSH_BYTES / 8];
    for place in [Place::First, Place::Last] {
        for width in QUERY_WIDTHS {
            let tag = format!("n{width}-{}", place.name());
            if skips(filter, &format!("membership/query/{tag}")) {
                continue;
            }
            let (mut w, p, v) = query_world(width, place);
            let w = std::cell::RefCell::new(&mut w);
            type Run<'a> = (&'static str, Box<dyn FnMut() -> usize + 'a>);
            let cases: Vec<Run<'_>> = vec![
                (
                    "for_each(&mut P,&V)",
                    Box::new(|| {
                        let mut w = w.borrow_mut();
                        let mut n = 0;
                        w.query::<(&mut P, &V)>().for_each(|(a, b)| {
                            a.0 += b.0;
                            n += 1;
                        });
                        n
                    }),
                ),
                (
                    "for_each(&mut P,&V)+without<Tag>",
                    Box::new(|| {
                        let mut w = w.borrow_mut();
                        let mut n = 0;
                        w.query::<(&mut P, &V)>().filter::<Without<Tag>>().for_each(|(a, b)| {
                            a.0 += b.0;
                            n += 1;
                        });
                        n
                    }),
                ),
                (
                    "for_each(&mut P,&V).cached()",
                    Box::new(|| {
                        let mut w = w.borrow_mut();
                        let mut n = 0;
                        w.query::<(&mut P, &V)>().cached().for_each(|(a, b)| {
                            a.0 += b.0;
                            n += 1;
                        });
                        n
                    }),
                ),
                (
                    "lookup-linear",
                    Box::new(|| {
                        let w = w.borrow();
                        w.raw_ref()
                            .archetypes()
                            .iter()
                            .map(|a| {
                                let ids = a.component_ids();
                                linear_index(ids, p).unwrap_or(0) + linear_index(ids, v).unwrap_or(0)
                            })
                            .sum()
                    }),
                ),
                (
                    "lookup-binary-search",
                    Box::new(|| {
                        let w = w.borrow();
                        w.raw_ref()
                            .archetypes()
                            .iter()
                            .map(|a| {
                                let ids = a.component_ids();
                                ids.binary_search(&p).unwrap_or(0) + ids.binary_search(&v).unwrap_or(0)
                            })
                            .sum()
                    }),
                ),
                (
                    "lookup-hybrid-linear8-binary",
                    Box::new(|| {
                        let w = w.borrow();
                        w.raw_ref()
                            .archetypes()
                            .iter()
                            .map(|a| {
                                let ids = a.component_ids();
                                hybrid_index(ids, p).unwrap_or(0) + hybrid_index(ids, v).unwrap_or(0)
                            })
                            .sum()
                    }),
                ),
                (
                    "lookup-chunked8",
                    Box::new(|| {
                        let w = w.borrow();
                        w.raw_ref()
                            .archetypes()
                            .iter()
                            .map(|a| {
                                let ids = a.component_ids();
                                chunked_index(ids, p).unwrap_or(0) + chunked_index(ids, v).unwrap_or(0)
                            })
                            .sum()
                    }),
                ),
                (
                    "lookup-rank-count",
                    Box::new(|| {
                        let w = w.borrow();
                        w.raw_ref()
                            .archetypes()
                            .iter()
                            .map(|a| {
                                let ids = a.component_ids();
                                rank_index(ids, p).unwrap_or(0) + rank_index(ids, v).unwrap_or(0)
                            })
                            .sum()
                    }),
                ),
            ];
            for (name, mut f) in cases {
                let hot = time_each(200, || {}, &mut f);
                let cold = time_each(
                    25,
                    || {
                        let checksum = black_box(&flush)
                            .iter()
                            .step_by(8)
                            .fold(0u64, |sum, &sample| sum.wrapping_add(sample));
                        black_box(checksum);
                    },
                    &mut f,
                );
                println!(
                    "membership/query/{tag}/{name:<34} ns per archetype: hot median {:>6.2} min {:>6.2}   \
                     cold median {:>6.2} min {:>6.2}   allocs {}",
                    hot.0,
                    hot.1,
                    cold.0,
                    cold.1,
                    hot.2.max(cold.2)
                );
            }
        }
    }
}

pub fn run(filter: Option<&str>) {
    // An explicit query filter should avoid setting up the other groups.
    // Their temporary worlds and buffers affect the allocator before the
    // cache-cold query workload constructs its world.
    if filter.is_some_and(|filter| filter.starts_with("membership/query")) {
        query_level(filter);
        return;
    }
    memory(filter);
    lookups(filter);
    colmap(filter);
    edge_lookup(filter);
    transitions(filter);
    query_level(filter);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_agree_with_the_linear_scan() {
        let (_, ids) = registered_world();
        for layout in [Layout::Dense, Layout::Sparse] {
            for n in WIDTHS {
                let set = pick(layout, n);
                assert_eq!(set.len(), n);
                assert!(set.windows(2).all(|p| p[0] < p[1]), "sorted and unique");
                let mine: Vec<ComponentId> = set.iter().map(|&i| ids[i]).collect();
                let (bits, table) = (Bits::new(&mine), Table::new(&mine));
                for &id in &ids {
                    let want = linear_index(&mine, id);
                    assert_eq!(bits.index(id), want);
                    assert_eq!(table.index(id), want);
                    assert_eq!(bits.has(id), want.is_some());
                    assert_eq!(mine.binary_search(&id).ok(), want);
                    assert_eq!(rank_index(&mine, id), want);
                    assert_eq!(hybrid_index(&mine, id), want);
                    assert_eq!(chunked_index(&mine, id), want);
                }
            }
        }
    }

    #[test]
    fn column_maps_agree() {
        let (_, ids) = registered_world();
        for layout in [Layout::Dense, Layout::Sparse] {
            for n in WIDTHS {
                let small: Vec<ComponentId> = pick(layout, n).iter().map(|&i| ids[i]).collect();
                let mut big = small.clone();
                big.push(ids[SPREAD + 7]);
                big.sort_unstable();
                for (src, dst) in [(&small, &big), (&big, &small)] {
                    let want = linear_map(src, dst);
                    assert_eq!(merge_map(src, dst), want);
                    assert_eq!(bits_map(src, &Bits::new(dst)), want);
                    assert_eq!(table_map(src, &Table::new(dst)), want);
                }
            }
        }
    }

    #[test]
    fn archetype_has_the_requested_components() {
        for n in WIDTHS {
            let set = pick(Layout::Sparse, n);
            let (w, ids, e) = world_with(&set);
            let mine: Vec<ComponentId> = set.iter().map(|&i| ids[i]).collect();
            let a = archetype_of(&w, &mine);
            assert_eq!(a.len(), 1);
            assert_eq!(a.entities(), &[e]);
            assert!(mine.iter().all(|&id| a.has(id)));
            assert!(!a.has(ids[SPREAD]));
        }
    }

    #[test]
    fn warm_transitions_create_no_archetypes() {
        let set = pick(Layout::Sparse, 8);
        let (mut w, ids, e) = world_with(&set);
        for &x in &ids[SPREAD..SPREAD + 4] {
            add(&mut w, e, x);
            assert!(w.raw().remove_dynamic(e, x));
        }
        let before = w.raw_ref().archetypes().len();
        for k in 0..64 {
            let x = ids[SPREAD + k % 4];
            add(&mut w, e, x);
            assert!(w.raw().remove_dynamic(e, x));
        }
        assert_eq!(w.raw_ref().archetypes().len(), before);
    }

    /// A component with a drop function comes and goes like the others,
    /// through warm edges.
    #[test]
    fn a_component_with_a_drop_function_toggles_without_new_archetypes() {
        let set = pick(Layout::Sparse, 8);
        let (mut w, _ids, e) = world_with(&set);
        let with_drop = register_with_drop(&mut w);
        assert!(add_with_drop(&mut w, e, with_drop));
        assert!(add_with_drop(&mut w, e, with_drop), "a second add replaces");
        assert!(w.raw().remove_dynamic(e, with_drop));
        let before = w.raw_ref().archetypes().len();
        for _ in 0..16 {
            assert!(add_with_drop(&mut w, e, with_drop));
            assert!(w.raw().remove_dynamic(e, with_drop));
        }
        assert_eq!(w.raw_ref().archetypes().len(), before);
        w.despawn(e);
        assert!(!add_with_drop(&mut w, e, with_drop));
    }

    /// The query benchmark's world: `QUERY_ARCHETYPES` archetypes of
    /// `QUERY_ROWS` rows, each with `P` and `V`, wherever they were registered.
    #[test]
    fn the_query_worlds_hold_every_archetype_with_p_and_v() {
        for place in [Place::First, Place::Last] {
            let (mut w, p, v) = query_world(4, place);
            assert_ne!(p, v);
            let mut rows = 0;
            w.query::<(&P, &V)>().for_each(|(p, v)| {
                assert_eq!((p.0, v.0), (1.0, 0.5));
                rows += 1;
            });
            assert_eq!(rows, QUERY_ARCHETYPES * QUERY_ROWS);
        }
    }

    #[test]
    fn edge_lookup_maps_agree() {
        let (_, ids) = registered_world();
        let keys = &ids[SPREAD..SPREAD + 16];
        let fast: HashMap<ComponentId, usize, BuildHasherDefault<IdHasher>> =
            keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        assert!(keys.iter().enumerate().all(|(i, k)| fast.get(k) == Some(&i)));
    }
}
