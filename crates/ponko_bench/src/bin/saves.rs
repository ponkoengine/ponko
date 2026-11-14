//! World saves at scale: size and time of a save slot of 1M entities, as
//! JSON written on the game's thread (what `save_world` did before) and in
//! the binary format written in the background (`SaveFormat::Binary`,
//! `save_world_in_background`), and of loading each back.
//!
//! Run with: `cargo run --release -p ponko_bench --bin saves [-- entities]`
//!
//! The world: every entity has a `Transform2D` and a `Health` (two `f32`s),
//! half also a `Velocity` (two `f32`s). Each step runs `RUNS` times on a
//! fresh copy of the work; the table shows the median and the fastest run.
//! Uses the system allocator (the main benchmark binary counts allocations,
//! which slows the parallel snapshot down).
use std::time::{Duration, Instant};

use ponko::prelude::*;
use ponko::reflect::SaveFormat;
use ponko::storage::Storage;
use serde::{Deserialize, Serialize};

const RUNS: usize = 3;

#[derive(Serialize, Deserialize, Component)]
struct Health {
    current: f32,
    max: f32,
}

#[derive(Serialize, Deserialize, Component)]
struct Velocity {
    v: [f32; 2],
}

fn world(n: usize) -> World {
    let mut w = World::new();
    w.reflect::<ponko::d2::Transform>();
    w.reflect::<Health>();
    w.reflect::<Velocity>();
    for i in 0..n {
        let f = i as f32;
        let t = ponko::d2::Transform::at(vec2(f * 0.37 % 1000.0, f * 1.13 % 700.0));
        let h = Health {
            current: (i % 100) as f32,
            max: 100.0,
        };
        if i % 2 == 0 {
            w.spawn((t, h, Velocity { v: [f.sin(), f.cos()] }));
        } else {
            w.spawn((t, h));
        }
    }
    w
}

/// Median and fastest of `RUNS` runs of `f`; the last run's result.
fn time<R>(mut f: impl FnMut() -> R) -> (Duration, Duration, R) {
    let mut times = Vec::with_capacity(RUNS);
    let mut last = None;
    for _ in 0..RUNS {
        let t = Instant::now();
        let r = f();
        times.push(t.elapsed());
        // Freed outside the timing.
        last = Some(r);
    }
    times.sort();
    (times[RUNS / 2], times[0], last.expect("RUNS > 0"))
}

fn row(what: &str, (median, min): (Duration, Duration)) {
    println!(
        "{what:<52} median {:>8.1} ms   min {:>8.1} ms",
        median.as_secs_f64() * 1e3,
        min.as_secs_f64() * 1e3
    );
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .map_or(1_000_000, |a| a.parse().expect("entity count"));
    let dir = std::env::temp_dir().join(format!("ponko-bench-saves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let threads = rayon::current_num_threads();
    println!("{n} entities, {threads} threads, {RUNS} runs each\n");
    let mut w = world(n);
    let one_thread = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("pool");

    // The parts.
    let (m, f, save) = time(|| one_thread.install(|| w.save()));
    row("snapshot (World::save), 1 thread", (m, f));
    let (m, f, _) = time(|| w.save());
    row(&format!("snapshot (World::save), {threads} threads"), (m, f));
    let (m, f, json) = time(|| SaveFormat::Json.encode(&save));
    row("encode JSON (pretty)", (m, f));
    let (m, f, binary) = time(|| SaveFormat::Binary.encode(&save));
    row("encode binary", (m, f));
    let (m, f, _) = time(|| SaveFormat::decode(&json).expect("JSON"));
    row("decode JSON", (m, f));
    let (m, f, _) = time(|| SaveFormat::decode(&binary).expect("binary"));
    row("decode binary", (m, f));
    let (m, f, _) = time(|| drop(w.save()));
    row("snapshot and free it (freeing moves to the job)", (m, f));
    drop(save);

    // Saving a slot, as a game does. Before: JSON, on the game's thread,
    // with the snapshot on one thread (the code before the binary format).
    println!();
    let mut storage = Storage::in_folder(&dir);
    let (m, f, ()) = time(|| {
        one_thread.install(|| storage.save_world(&w, "slot")).expect("saved");
    });
    row("before: save_world, JSON, game thread blocked", (m, f));
    let json_size = std::fs::metadata(dir.join("saves/slot.json")).expect("written").len() as usize;

    storage.set_save_format(SaveFormat::Binary);
    let (m, f, ()) = time(|| storage.save_world(&w, "slot").expect("saved"));
    row("save_world, binary, game thread blocked", (m, f));
    // The call is the game's pause; the job then encodes and writes.
    let (mut calls, mut written) = (Vec::with_capacity(RUNS), Vec::with_capacity(RUNS));
    for _ in 0..RUNS {
        let start = Instant::now();
        let job = storage.save_world_in_background(&w, "slot").expect("started");
        calls.push(start.elapsed());
        job.wait().expect("written");
        written.push(start.elapsed());
    }
    calls.sort();
    written.sort();
    row(
        "after: save_world_in_background, binary, game thread",
        (calls[RUNS / 2], calls[0]),
    );
    row("after: ... until the file is written", (written[RUNS / 2], written[0]));
    let binary_size = std::fs::metadata(dir.join("saves/slot.save")).expect("written").len() as usize;

    // Loading a slot: the file's bytes, decoding, and World::load.
    println!();
    let json_file = std::fs::read(dir.join("saves/slot.json"));
    assert!(json_file.is_err(), "saving in binary removed the JSON slot");
    std::fs::write(dir.join("slot.json"), &json).expect("write");
    // Alternated, so a busy machine slows both alike.
    let mut load = |file: &str| {
        let start = Instant::now();
        let bytes = std::fs::read(dir.join(file)).expect("read");
        w.load(&SaveFormat::decode(&bytes).expect("a save")).expect("loads");
        start.elapsed()
    };
    let (mut json_loads, mut binary_loads) = (Vec::new(), Vec::new());
    for _ in 0..RUNS {
        json_loads.push(load("slot.json"));
        binary_loads.push(load("saves/slot.save"));
    }
    json_loads.sort();
    binary_loads.sort();
    row(
        "before: load a JSON slot (read, decode, World::load)",
        (json_loads[RUNS / 2], json_loads[0]),
    );
    row(
        "after: load a binary slot (read, decode, World::load)",
        (binary_loads[RUNS / 2], binary_loads[0]),
    );

    println!(
        "\nsize: JSON {:.1} MiB, binary {:.1} MiB ({:.1}x smaller)",
        mib(json_size),
        mib(binary_size),
        json_size as f64 / binary_size as f64
    );
    assert_eq!(w.len(), n);
    let _ = std::fs::remove_dir_all(&dir);
}
