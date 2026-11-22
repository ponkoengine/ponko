//! The ECS matrix, run with all three libraries in one process: the
//! ponko_ecs and hecs rows from `crates/ponko_bench/src/ecs_suite.rs` and
//! `archetypes.rs` (compiled here as they are), the bevy_ecs rows from
//! `ecs_bevy.rs`. The harness times exactly as `ponko_bench`'s does
//! (`Bench::steady`: 5 untimed runs, then every run timed on the same state;
//! `Bench::run`: the state rebuilt untimed before each run, one warm-up), so
//! the rows line up with `crates/ponko_bench/ECS.md`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub const DT: f32 = 1.0 / 60.0;

#[derive(bevy::prelude::Component, ponko_ecs::Component, Clone, Copy, Default, Debug, PartialEq)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
}
#[derive(bevy::prelude::Component, ponko_ecs::Component, Clone, Copy, Default, Debug, PartialEq)]
pub struct Vel {
    pub x: f32,
    pub y: f32,
}
/// Stand-in for a sprite component: texture id + tint (as `ponko_bench`'s).
#[derive(bevy::prelude::Component, ponko_ecs::Component, Clone, Copy, Default)]
pub struct Spr {
    pub _tex: u32,
    pub _tint: [f32; 4],
}
#[derive(bevy::prelude::Component, ponko_ecs::Component, Clone, Copy, Default)]
pub struct Tag;

pub fn ent(i: usize) -> (Pos, Vel, Spr) {
    (Pos { x: i as f32, y: 0.0 }, Vel { x: 1.0, y: 0.5 }, Spr::default())
}

/// bevy_ecs's `Component` for the shared files' own components, written out
/// as `#[derive(Component)]` writes it (table storage, mutable).
macro_rules! bevy_component {
    ($($t:ty),* $(,)?) => {$(
        impl bevy::ecs::component::Component for $t {
            const STORAGE_TYPE: bevy::ecs::component::StorageType = bevy::ecs::component::StorageType::Table;
            type Mutability = bevy::ecs::component::Mutable;
        }
    )*};
}
bevy_component!(
    crate::ecs_suite::Boost,
    crate::ecs_suite::Hp,
    crate::ecs_suite::Life,
    crate::ecs_suite::Ai,
);
impl<const K: usize> bevy::ecs::component::Component for crate::archetypes::Mk<K> {
    const STORAGE_TYPE: bevy::ecs::component::StorageType = bevy::ecs::component::StorageType::Table;
    type Mutability = bevy::ecs::component::Mutable;
}

/// `ponko_bench`'s harness, timing the same way; records go to JSON.
pub struct Bench {
    filter: Option<String>,
    pub records: Vec<Value>,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

impl Bench {
    pub fn new(filter: &str) -> Self {
        Bench {
            filter: (!filter.is_empty()).then(|| filter.to_owned()),
            records: Vec::new(),
        }
    }

    pub fn skips(&self, name: &str) -> bool {
        self.filter
            .as_ref()
            .is_some_and(|f| !f.split('|').any(|part| name.contains(part)))
    }

    pub fn steady<S>(&mut self, name: &str, runs: usize, setup: impl FnOnce() -> S, op: impl FnMut(&mut S)) {
        self.steady_with_bytes(name, runs, setup, op, |_| None);
    }

    pub fn steady_with_bytes<S>(
        &mut self,
        name: &str,
        runs: usize,
        setup: impl FnOnce() -> S,
        mut op: impl FnMut(&mut S),
        _bytes: impl FnOnce(&S) -> Option<usize>,
    ) {
        const WARMUP: usize = 5;
        if self.skips(name) {
            return;
        }
        let mut s = setup();
        for _ in 0..WARMUP {
            op(&mut s);
        }
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            op(&mut s);
            times.push(t.elapsed());
        }
        black_box(&s);
        self.report(name, "steady", WARMUP, times);
    }

    pub fn run<S>(&mut self, name: &str, runs: usize, mut setup: impl FnMut() -> S, mut op: impl FnMut(&mut S)) {
        const WARMUP: usize = 1;
        if self.skips(name) {
            return;
        }
        let mut times = Vec::with_capacity(runs);
        for i in 0..runs + WARMUP {
            let mut s = setup();
            let t = Instant::now();
            op(&mut s);
            let el = t.elapsed();
            if i >= WARMUP {
                times.push(el);
            }
            drop(black_box(s));
        }
        self.report(name, "fresh", WARMUP, times);
    }

    pub fn report(&mut self, name: &str, kind: &str, warmup: usize, times: Vec<Duration>) {
        let mut values: Vec<f64> = times.into_iter().map(ms).collect();
        let d = crate::common::Dist::of(&mut values);
        println!(
            "{name:<40} median {:>9.4} ms   p95 {:>9.4} ms   min {:>9.4} ms",
            d.p50, d.p95, d.min
        );
        self.records.push(json!({
            "name": name, "kind": kind, "warmup": warmup, "runs": d.samples,
            "min_ms": d.min, "p50_ms": d.p50, "p95_ms": d.p95, "max_ms": d.max, "mean_ms": d.mean,
        }));
    }
}

pub fn run(filter: &str) {
    let mut b = Bench::new(filter);
    let env = crate::workloads::env();
    crate::archetypes::run(&mut b);
    crate::ecs_bevy::archetypes(&mut b);
    crate::ecs_suite::run(&mut b);
    crate::ecs_bevy::run(&mut b);
    crate::workloads::write_results("ecs", env, b.records);
}
