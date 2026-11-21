//! `cargo run --release -p mods_eval --features wasmtime,wasmi,rhai,lua`:
//! per-call cost of the drip's update in each runtime, whether the results
//! match the Rust baseline bit for bit, and how each stops a hostile mod.
use std::time::{Duration, Instant};

fn main() {
    const DRIPS: usize = 1000;
    let budget = Duration::from_millis(1500);
    let mut baseline: Option<Vec<(u32, u32)>> = None;
    for mut rt in mods_eval::runtimes() {
        eprintln!("[{}] warm", rt.name());
        // Warm up and check results: 600 ticks of 1000 drips.
        let mut drips: Vec<(f32, f32)> = (0..DRIPS)
            .map(|i| (5.0 + (i % 90) as f32, if i % 2 == 0 { 1.0 } else { -1.0 }))
            .collect();
        for _ in 0..600 {
            for d in &mut drips {
                *d = rt.update(d.0, d.1, 28.0 + (d.0 % 3.0), 1.0 / 60.0);
            }
        }
        let bits: Vec<(u32, u32)> = drips.iter().map(|d| (d.0.to_bits(), d.1.to_bits())).collect();
        let same = match &baseline {
            None => {
                baseline = Some(bits);
                "baseline".to_string()
            }
            Some(b) => {
                let diff = b.iter().zip(&bits).filter(|(a, b)| a != b).count();
                if diff == 0 {
                    "bit-identical".into()
                } else {
                    format!("{diff} of {DRIPS} drips differ")
                }
            }
        };
        // Time: as many ticks as fit in the budget.
        let start = Instant::now();
        let mut calls = 0u64;
        while start.elapsed() < budget {
            for d in &mut drips {
                *d = rt.update(d.0, d.1, 28.0, 1.0 / 60.0);
            }
            calls += DRIPS as u64;
        }
        let ns = start.elapsed().as_nanos() as f64 / calls as f64;
        std::hint::black_box(&drips);
        eprintln!("[{}] spin", rt.name());
        let t = Instant::now();
        let spin = rt.spin();
        let spin_ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("[{}] grow", rt.name());
        let t = Instant::now();
        let grow = rt.grow();
        let grow_ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "{:40} {:8.1} ns/update  {:>20}  spin: {:?} ({spin_ms:.1} ms)  grow: {:?} ({grow_ms:.1} ms)",
            rt.name(),
            ns,
            same,
            spin.err().map(|e| e.chars().take(80).collect::<String>()),
            grow.err().map(|e| e.chars().take(80).collect::<String>()),
        );
    }
}
