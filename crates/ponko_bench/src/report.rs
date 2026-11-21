//! Benchmark results and their machine-readable export.
//!
//! Every benchmark adds one [`Record`]. With `PONKO_BENCH_JSON=<path>` set,
//! `main` writes them, together with the build and machine context, as one
//! JSON document (see [`to_json`] and the crate README for the schema).

use std::fmt::Write as _;
use std::time::Duration;

/// Bumped when a field changes meaning or disappears. New fields don't bump it.
pub const SCHEMA_VERSION: u32 = 1;

/// Versions of the comparison crates, as locked in the workspace `Cargo.lock`.
pub const BEVY_ECS_VERSION: &str = "0.20.1";
pub const HECS_VERSION: &str = "0.11.2";
/// The bevy_ecs features `ponko_bench`'s `Cargo.toml` turns on (with its
/// defaults). `dependency_versions_and_features_match_cargo_toml` keeps the
/// three constants in step with `Cargo.toml`.
pub const BEVY_ECS_FEATURES: &[&str] = &["default", "multi_threaded"];

/// How a benchmark was timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// State built once, then the operation timed repeatedly on it.
    Steady,
    /// State rebuilt (untimed) before every timed operation.
    Fresh,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Steady => "steady",
            Kind::Fresh => "fresh",
        }
    }
}

/// One benchmark's timing summary.
#[derive(Clone, Debug)]
pub struct Record {
    pub name: String,
    pub kind: Kind,
    /// Untimed runs of the operation before measuring.
    pub warmup: usize,
    /// Timed runs, one sample each.
    pub runs: usize,
    pub min: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub max: Duration,
    pub mean: Duration,
    /// Bytes the ECS world reports as allocated for component storage
    /// (`World::allocated_bytes`), measured after the timed runs. `None` where
    /// the benchmark has no such world or the library can't report it.
    pub world_bytes: Option<usize>,
    /// Heap allocations (and reallocations) per timed run, averaged over the
    /// timed runs, counted for the whole process by the runner's allocator.
    /// `None` when the runner didn't count them.
    pub allocs_per_run: Option<f64>,
    /// Bytes those allocations requested, per timed run.
    pub alloc_bytes_per_run: Option<f64>,
}

impl Record {
    /// Summarizes `times` (sorted in place). Percentiles use the nearest-rank
    /// method, so with 10 runs p95 is the slowest run.
    ///
    /// # Panics
    /// If `times` is empty.
    pub fn new(name: &str, kind: Kind, warmup: usize, times: &mut [Duration]) -> Self {
        assert!(!times.is_empty(), "benchmark {name} has no timed runs");
        times.sort();
        let total: Duration = times.iter().sum();
        Record {
            name: name.to_owned(),
            kind,
            warmup,
            runs: times.len(),
            min: times[0],
            p50: nearest_rank(times, 50),
            p95: nearest_rank(times, 95),
            p99: nearest_rank(times, 99),
            max: times[times.len() - 1],
            mean: total / times.len() as u32,
            world_bytes: None,
            allocs_per_run: None,
            alloc_bytes_per_run: None,
        }
    }
}

impl Record {
    /// Records `counted` allocations over `runs` timed runs.
    pub fn set_allocs(&mut self, counted: crate::alloc::Counts, runs: usize) {
        let runs = runs.max(1) as f64;
        self.allocs_per_run = Some(counted.count as f64 / runs);
        self.alloc_bytes_per_run = Some(counted.bytes as f64 / runs);
    }
}

/// The `pct`th percentile of sorted `times`, nearest-rank.
fn nearest_rank(sorted: &[Duration], pct: usize) -> Duration {
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

/// Build and machine context recorded with the results.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub rustc: String,
    pub git_rev: String,
    /// Tracked files differ from `git_rev`; `None` if git couldn't tell.
    pub git_dirty: Option<bool>,
    pub os: &'static str,
    pub arch: &'static str,
    pub cpu: String,
    pub logical_cores: usize,
    pub rayon_threads: usize,
    pub debug_assertions: bool,
    pub filter: Option<String>,
    pub unix_time: u64,
    /// The 1, 5 and 15 minute load averages when the run started (how busy
    /// the machine was), or `"unknown"`.
    pub load_average: String,
    /// The machine's name from `PONKO_BENCH_MACHINE`, so runs on one named
    /// machine group together (`bench_track compare` and `history`). `None`:
    /// `bench_track` makes one from the OS, architecture and CPU.
    pub machine: Option<String>,
    /// The alignment in bytes of every function of the build, when it was
    /// built with `scripts/bench-aligned.sh` (`PONKO_BENCH_CODE_ALIGN`);
    /// `None` for an ordinary build, whose layout moves with any change.
    pub code_align: Option<u32>,
}

/// `PONKO_BENCH_CODE_ALIGN` when this binary was compiled.
pub fn build_code_align() -> Option<u32> {
    option_env!("PONKO_BENCH_CODE_ALIGN").and_then(|v| v.parse().ok())
}

impl Env {
    /// Best effort: anything that can't be found is `"unknown"`.
    pub fn detect(filter: Option<String>) -> Self {
        Env {
            rustc: command_line("rustc", &["--version"]),
            git_rev: command_line("git", &["rev-parse", "HEAD"]),
            git_dirty: std::process::Command::new("git")
                .args(["status", "--porcelain", "--untracked-files=no"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| !o.stdout.is_empty()),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cpu: cpu_name(),
            logical_cores: std::thread::available_parallelism().map_or(0, |n| n.get()),
            rayon_threads: rayon::current_num_threads(),
            debug_assertions: cfg!(debug_assertions),
            filter,
            unix_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            load_average: load_average(),
            machine: std::env::var("PONKO_BENCH_MACHINE").ok().filter(|m| !m.is_empty()),
            code_align: build_code_align(),
        }
    }
}

/// First line of a command's stdout, or `"unknown"`.
fn command_line(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(|l| l.trim().to_owned()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn load_average() -> String {
    if cfg!(target_os = "macos") {
        // "{ 1.23 2.34 3.45 }"
        let s = command_line("sysctl", &["-n", "vm.loadavg"]);
        return s.trim_matches(|c| c == '{' || c == '}' || c == ' ').to_owned();
    }
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn cpu_name() -> String {
    if cfg!(target_os = "macos") {
        return command_line("sysctl", &["-n", "machdep.cpu.brand_string"]);
    }
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The results as one JSON document. Times are in milliseconds. `render`:
/// the rendering baseline's own section (a JSON object, see
/// `render_report`), written as the document's `render` field.
pub fn to_json(env: &Env, records: &[Record], render: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    let _ = writeln!(s, "  \"schema_version\": {SCHEMA_VERSION},");
    s.push_str("  \"env\": {\n");
    let _ = writeln!(s, "    \"rustc\": {},", json_str(&env.rustc));
    let _ = writeln!(s, "    \"git_rev\": {},", json_str(&env.git_rev));
    let dirty = env.git_dirty.map_or_else(|| "null".to_owned(), |d| d.to_string());
    let _ = writeln!(s, "    \"git_dirty\": {dirty},");
    let _ = writeln!(s, "    \"os\": {},", json_str(env.os));
    let _ = writeln!(s, "    \"arch\": {},", json_str(env.arch));
    let _ = writeln!(s, "    \"cpu\": {},", json_str(&env.cpu));
    let _ = writeln!(s, "    \"logical_cores\": {},", env.logical_cores);
    let _ = writeln!(s, "    \"rayon_threads\": {},", env.rayon_threads);
    let _ = writeln!(s, "    \"debug_assertions\": {},", env.debug_assertions);
    let window = if cfg!(feature = "render") { ", \"window\"" } else { "" };
    let _ = writeln!(
        s,
        "    \"ponko_features\": [\"physics\", \"physics3d\", \"navmesh\"{window}],"
    );
    let _ = writeln!(s, "    \"bevy_ecs\": {},", json_str(BEVY_ECS_VERSION));
    let features: Vec<String> = BEVY_ECS_FEATURES.iter().map(|f| json_str(f)).collect();
    let _ = writeln!(s, "    \"bevy_ecs_features\": [{}],", features.join(", "));
    let _ = writeln!(s, "    \"hecs\": {},", json_str(HECS_VERSION));
    let filter = env.filter.as_deref().map_or_else(|| "null".to_owned(), json_str);
    let _ = writeln!(s, "    \"filter\": {filter},");
    let _ = writeln!(s, "    \"load_average\": {},", json_str(&env.load_average));
    let machine = env.machine.as_deref().map_or_else(|| "null".to_owned(), json_str);
    let _ = writeln!(s, "    \"machine\": {machine},");
    let align = env.code_align.map_or_else(|| "null".to_owned(), |a| a.to_string());
    let _ = writeln!(s, "    \"code_align\": {align},");
    let _ = writeln!(s, "    \"unix_time\": {}", env.unix_time);
    s.push_str("  },\n  \"results\": [");
    for (i, r) in records.iter().enumerate() {
        s.push_str(if i == 0 { "\n" } else { ",\n" });
        let bytes = r.world_bytes.map_or_else(|| "null".to_owned(), |b| b.to_string());
        let per_run = |v: Option<f64>| v.map_or_else(|| "null".to_owned(), |v| format!("{v:.3}"));
        let _ = write!(
            s,
            "    {{\"name\": {}, \"kind\": \"{}\", \"warmup\": {}, \"runs\": {}, \
             \"min_ms\": {}, \"p50_ms\": {}, \"p95_ms\": {}, \"p99_ms\": {}, \"max_ms\": {}, \
             \"mean_ms\": {}, \"world_bytes\": {bytes}, \"allocs_per_run\": {}, \"alloc_bytes_per_run\": {}}}",
            json_str(&r.name),
            r.kind.as_str(),
            r.warmup,
            r.runs,
            ms(r.min),
            ms(r.p50),
            ms(r.p95),
            ms(r.p99),
            ms(r.max),
            ms(r.mean),
            per_run(r.allocs_per_run),
            per_run(r.alloc_bytes_per_run),
        );
    }
    s.push_str(if records.is_empty() { "]" } else { "\n  ]" });
    if let Some(render) = render {
        // Indented like the rest of the document.
        let _ = write!(s, ",\n  \"render\": {}", render.trim_end().replace('\n', "\n  "));
    }
    s.push_str("\n}\n");
    s
}

/// Milliseconds with nanosecond resolution, as a JSON number.
fn ms(d: Duration) -> String {
    format!("{:.6}", d.as_secs_f64() * 1e3)
}

/// A JSON string literal.
fn json_str(v: &str) -> String {
    let mut s = String::with_capacity(v.len() + 2);
    s.push('"');
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let mut times: Vec<_> = (1..=20).rev().map(d).collect();
        let r = Record::new("x", Kind::Steady, 5, &mut times);
        assert_eq!((r.min, r.p50, r.p95, r.max), (d(1), d(10), d(19), d(20)));
        assert_eq!(r.mean, Duration::from_micros(10_500));
        assert_eq!(r.runs, 20);

        let mut ten: Vec<_> = (1..=10).map(d).collect();
        let r = Record::new("y", Kind::Fresh, 1, &mut ten);
        assert_eq!((r.p50, r.p95), (d(5), d(10)));

        let r = Record::new("one", Kind::Fresh, 1, &mut [d(3)]);
        assert_eq!((r.min, r.p50, r.p95, r.max), (d(3), d(3), d(3), d(3)));
    }

    #[test]
    #[should_panic(expected = "benchmark empty has no timed runs")]
    fn empty_samples_panic() {
        Record::new("empty", Kind::Steady, 0, &mut []);
    }

    #[test]
    fn dependency_versions_and_features_match_cargo_toml() {
        let manifest = include_str!("../Cargo.toml");
        let line = |name: &str| {
            manifest
                .lines()
                .find(|l| l.starts_with(&format!("{name} =")))
                .unwrap_or_else(|| panic!("{name} is a dependency"))
        };
        let bevy = line("bevy_ecs");
        assert!(bevy.contains(&format!("version = \"{BEVY_ECS_VERSION}\"")), "{bevy}");
        // Every feature turned on is listed, and `default` stays on.
        let extra: Vec<String> = BEVY_ECS_FEATURES
            .iter()
            .filter(|&&f| f != "default")
            .map(|f| format!("\"{f}\""))
            .collect();
        assert!(bevy.contains(&format!("features = [{}]", extra.join(", "))), "{bevy}");
        assert!(!bevy.contains("default-features = false"), "{bevy}");
        assert_eq!(line("hecs"), format!("hecs = \"{HECS_VERSION}\""));
    }

    #[test]
    fn the_recorded_function_alignment_is_the_builds() {
        // `scripts/bench-aligned.sh test -p ponko_bench` (or the variable set
        // by hand) compiles and runs this with PONKO_BENCH_CODE_ALIGN=128.
        let at_run = std::env::var("PONKO_BENCH_CODE_ALIGN")
            .ok()
            .and_then(|v| v.parse().ok());
        assert_eq!(build_code_align(), at_run);
        assert_eq!(Env::detect(None).code_align, at_run);
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_str("a\"b\\c\nd\u{1}"), r#""a\"b\\c\nd\u0001""#);
    }

    #[test]
    fn json_document_lists_every_record() {
        let env = Env {
            filter: Some("iter".into()),
            machine: Some("lab-m4".into()),
            code_align: Some(128),
            ..Env::default()
        };
        let mut a = Record::new("iter/a", Kind::Steady, 5, &mut [d(2), d(1)]);
        a.world_bytes = Some(4096);
        a.allocs_per_run = Some(2.5);
        a.alloc_bytes_per_run = Some(64.0);
        let b = Record::new("spawn/b", Kind::Fresh, 1, &mut [d(3)]);
        let json = to_json(&env, &[a, b], None);
        assert!(json.contains("\"schema_version\": 1,"));
        assert!(json.contains("\"filter\": \"iter\","));
        assert!(json.contains("\"machine\": \"lab-m4\","));
        assert!(json.contains("\"code_align\": 128,"));
        assert!(json.contains(
            "{\"name\": \"iter/a\", \"kind\": \"steady\", \"warmup\": 5, \"runs\": 2, \"min_ms\": 1.000000, \
             \"p50_ms\": 1.000000, \"p95_ms\": 2.000000, \"p99_ms\": 2.000000, \"max_ms\": 2.000000, \
             \"mean_ms\": 1.500000, \"world_bytes\": 4096, \"allocs_per_run\": 2.500, \"alloc_bytes_per_run\": 64.000},\n"
        ));
        assert!(json.contains("\"name\": \"spawn/b\", \"kind\": \"fresh\""));
        assert!(
            json.contains("\"world_bytes\": null, \"allocs_per_run\": null, \"alloc_bytes_per_run\": null}\n  ]\n}\n")
        );
        // Balanced, so a JSON parser at least sees a closed document.
        assert_eq!(json.matches('{').count(), json.matches('}').count());
        assert_eq!(json.matches('[').count(), json.matches(']').count());

        let empty = to_json(&Env::default(), &[], None);
        assert!(empty.ends_with("\"results\": []\n}\n"));
        assert!(empty.contains("\"filter\": null,"));
        assert!(empty.contains("\"machine\": null,"));
        assert!(empty.contains("\"code_align\": null,"));
    }

    #[test]
    fn the_render_section_is_part_of_the_document() {
        let r = Record::new("render/scene x frame", Kind::Steady, 2, &mut [d(1)]);
        let json = to_json(&Env::default(), &[r], Some("{\n  \"scenes\": []\n}"));
        assert!(
            json.ends_with("\n  ],\n  \"render\": {\n    \"scenes\": []\n  }\n}\n"),
            "{json}"
        );
        let empty = to_json(&Env::default(), &[], Some("{}"));
        assert!(empty.ends_with("\"results\": [],\n  \"render\": {}\n}\n"), "{empty}");
        assert_eq!(json.matches('{').count(), json.matches('}').count());
    }
}
