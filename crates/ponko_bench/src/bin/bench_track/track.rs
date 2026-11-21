//! Reading result documents, comparing two sets of runs, and the history
//! table. Pure functions: everything here is tested without running a
//! benchmark.
//!
//! Two kinds of document are read, both with `schema_version` 1:
//!
//! - **instructions**: written by `bench_track counts`. `env.method` is
//!   `"instructions"`; every result has `samples`, the instructions one
//!   operation took in each repeated run.
//! - **wall**: written by `ponko_bench` (`PONKO_BENCH_JSON`). Every result
//!   gives one sample: its median time, `p50_ms`. Repeated runs are separate
//!   documents.
//!
//! A side of a comparison is one or more documents; the samples of a
//! benchmark are pooled across them.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

/// How a document measured its benchmarks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Instructions per operation, counted by cachegrind.
    Instructions,
    /// Milliseconds per operation (each run's median).
    Wall,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Instructions => "instructions",
            Method::Wall => "wall",
        }
    }

    /// The smallest change that counts, whatever the spread: instruction
    /// counts barely move between runs, wall-clock medians do.
    pub fn default_floor(self) -> f64 {
        match self {
            Method::Instructions => 0.01,
            Method::Wall => 0.05,
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Method::Instructions => "instructions",
            Method::Wall => "ms",
        }
    }
}

/// One result document, reduced to what comparisons need.
#[derive(Clone, Debug)]
pub struct Doc {
    pub method: Method,
    /// The machine the numbers come from: `env.machine`, or one made from
    /// the OS, architecture and CPU name.
    pub machine: String,
    pub git_rev: String,
    pub rustc: String,
    /// Commit time (seconds since 1970), or the time of the run.
    pub time: u64,
    /// The workloads' source hash (instruction documents only).
    pub workloads: Option<String>,
    /// `env.code_align`: the function alignment of an aligned build
    /// (`scripts/bench-aligned.sh`), `None` for an ordinary one.
    pub code_align: Option<u64>,
    /// Samples of every benchmark, in document order.
    pub results: Vec<(String, Vec<f64>)>,
}

/// Reads a result document.
pub fn parse_doc(text: &str) -> Result<Doc, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let version = v["schema_version"].as_u64();
    if version != Some(1) {
        return Err(format!("unsupported schema_version {:?} (1 is supported)", version));
    }
    let env = &v["env"];
    let s = |key: &str| env[key].as_str().map(str::to_owned);
    let method = match env["method"].as_str() {
        None | Some("wall") => Method::Wall,
        Some("instructions") => Method::Instructions,
        Some(other) => return Err(format!("unknown env.method {other:?}")),
    };
    let machine = match s("machine").filter(|m| !m.is_empty()) {
        Some(m) => m,
        None => machine_slug(
            &s("os").unwrap_or_default(),
            &s("arch").unwrap_or_default(),
            &s("cpu").unwrap_or_default(),
        ),
    };
    let time = env["commit_time"].as_u64().or(env["unix_time"].as_u64()).unwrap_or(0);
    let mut results = Vec::new();
    for r in v["results"].as_array().ok_or("no results array")? {
        let name = r["name"].as_str().ok_or("a result without a name")?.to_owned();
        let samples: Vec<f64> = match method {
            Method::Instructions => r["samples"]
                .as_array()
                .ok_or_else(|| format!("{name}: no samples"))?
                .iter()
                .filter_map(Value::as_f64)
                .collect(),
            Method::Wall => r["p50_ms"].as_f64().into_iter().collect(),
        };
        if samples.is_empty() {
            return Err(format!("{name}: no numbers"));
        }
        results.push((name, samples));
    }
    Ok(Doc {
        method,
        machine,
        git_rev: s("git_rev").unwrap_or_else(|| "unknown".into()),
        rustc: s("rustc").unwrap_or_else(|| "unknown".into()),
        time,
        workloads: s("workloads"),
        code_align: env["code_align"].as_u64(),
        results,
    })
}

/// A machine name made from what a document records about it, for
/// documents without `env.machine`: `macos-aarch64-apple-m4-max`.
pub fn machine_slug(os: &str, arch: &str, cpu: &str) -> String {
    let raw = format!("{os}-{arch}-{cpu}").to_lowercase();
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// Median of `xs` (the mean of the middle two for an even count).
pub fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return f64::NAN;
    }
    if !n.is_multiple_of(2) {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// How far apart the runs are: `(max - min) / median`, 0 for one run.
pub fn spread(xs: &[f64]) -> f64 {
    let m = median(xs);
    if xs.len() < 2 || m == 0.0 {
        return 0.0;
    }
    let (lo, hi) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
        (lo.min(x), hi.max(x))
    });
    (hi - lo) / m.abs()
}

/// When a change counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// The smallest relative change that counts (0.01 = 1%).
    pub floor: f64,
    /// A change also has to exceed this many times the larger spread of
    /// the two sides.
    pub noise_factor: f64,
}

impl Rule {
    pub fn for_method(method: Method) -> Self {
        Rule {
            floor: method.default_floor(),
            noise_factor: 2.0,
        }
    }

    /// The threshold for two sides with these spreads.
    pub fn threshold(&self, spread_base: f64, spread_new: f64) -> f64 {
        self.floor.max(self.noise_factor * spread_base.max(spread_new))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Slower,
    Faster,
    /// Within the threshold: noise.
    Same,
}

/// One benchmark measured on both sides.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub base: f64,
    pub new: f64,
    pub base_runs: usize,
    pub new_runs: usize,
    pub spread_base: f64,
    pub spread_new: f64,
    /// `(new - base) / base`.
    pub change: f64,
    pub threshold: f64,
    pub verdict: Verdict,
}

/// Compares one benchmark's samples.
pub fn compare_samples(name: &str, base: &[f64], new: &[f64], rule: Rule) -> Row {
    let (b, n) = (median(base), median(new));
    let (sb, sn) = (spread(base), spread(new));
    let change = if b == 0.0 { 0.0 } else { (n - b) / b };
    let threshold = rule.threshold(sb, sn);
    let verdict = if change > threshold {
        Verdict::Slower
    } else if change < -threshold {
        Verdict::Faster
    } else {
        Verdict::Same
    };
    Row {
        name: name.to_owned(),
        base: b,
        new: n,
        base_runs: base.len(),
        new_runs: new.len(),
        spread_base: sb,
        spread_new: sn,
        change,
        threshold,
        verdict,
    }
}

/// The documents of one side, pooled.
#[derive(Clone, Debug)]
pub struct Side {
    pub method: Method,
    pub machine: String,
    pub git_revs: Vec<String>,
    pub rustc: Vec<String>,
    pub workloads: Vec<String>,
    /// Function alignments of the side's builds (`"128"`, or `"none"`).
    pub code_align: Vec<String>,
    /// Benchmark name to every sample, in first-seen order.
    pub samples: Vec<(String, Vec<f64>)>,
}

/// Pools documents into one side. They must share the method and machine.
pub fn pool(docs: &[Doc]) -> Result<Side, String> {
    let first = docs.first().ok_or("no documents")?;
    let mut side = Side {
        method: first.method,
        machine: first.machine.clone(),
        git_revs: Vec::new(),
        rustc: Vec::new(),
        workloads: Vec::new(),
        code_align: Vec::new(),
        samples: Vec::new(),
    };
    for d in docs {
        if d.method != side.method {
            return Err(format!(
                "documents mix methods ({} and {})",
                side.method.as_str(),
                d.method.as_str()
            ));
        }
        if d.machine != side.machine {
            return Err(format!("documents mix machines ({} and {})", side.machine, d.machine));
        }
        let add = |list: &mut Vec<String>, v: &str| {
            if !list.iter().any(|x| x == v) {
                list.push(v.to_owned());
            }
        };
        add(&mut side.git_revs, &d.git_rev);
        add(&mut side.rustc, &d.rustc);
        add(
            &mut side.code_align,
            &d.code_align.map_or_else(|| "none".to_owned(), |a| a.to_string()),
        );
        if let Some(w) = &d.workloads {
            add(&mut side.workloads, w);
        }
        for (name, xs) in &d.results {
            match side.samples.iter_mut().find(|(n, _)| n == name) {
                Some((_, all)) => all.extend_from_slice(xs),
                None => side.samples.push((name.clone(), xs.clone())),
            }
        }
    }
    Ok(side)
}

/// A whole comparison.
#[derive(Clone, Debug)]
pub struct Comparison {
    pub method: Method,
    pub rule: Rule,
    pub rows: Vec<Row>,
    pub only_base: Vec<String>,
    pub only_new: Vec<String>,
    /// Why the two sides may not be comparable (different machine,
    /// compiler or workloads). Empty when nothing differs.
    pub warnings: Vec<String>,
}

impl Comparison {
    pub fn count(&self, v: Verdict) -> usize {
        self.rows.iter().filter(|r| r.verdict == v).count()
    }
}

/// Compares two sides. Different methods are an error; a different machine
/// is an error unless `any_machine`.
pub fn compare(base: &Side, new: &Side, rule: Rule, any_machine: bool) -> Result<Comparison, String> {
    if base.method != new.method {
        return Err(format!(
            "the sides measured different things ({} and {})",
            base.method.as_str(),
            new.method.as_str()
        ));
    }
    let mut warnings = Vec::new();
    if base.machine != new.machine {
        let msg = format!("different machines: {} and {}", base.machine, new.machine);
        if !any_machine {
            return Err(format!("{msg} (pass --any-machine to compare anyway)"));
        }
        warnings.push(msg);
    }
    if base.rustc != new.rustc {
        warnings.push(format!(
            "different compilers: {} and {}; part of a change can be the compiler's",
            base.rustc.join(", "),
            new.rustc.join(", ")
        ));
    }
    if base.code_align != new.code_align {
        warnings.push(format!(
            "different function alignment ({} and {}): part of a change can be where the code fell in \
             the binary; build both sides with scripts/bench-aligned.sh",
            base.code_align.join(", "),
            new.code_align.join(", ")
        ));
    }
    if base.workloads != new.workloads {
        warnings.push(format!(
            "different workload sources ({} and {}): the workloads themselves changed",
            or_unknown(&base.workloads),
            or_unknown(&new.workloads)
        ));
    }
    let fewest = |side: &Side| side.samples.iter().map(|(_, xs)| xs.len()).min().unwrap_or(0);
    let fewest = fewest(base).min(fewest(new));
    if fewest < 3 {
        warnings.push(format!(
            "a side has only {fewest} run(s) of a benchmark, too few to see its spread: \
             repeat runs (3 or more) for a trustworthy threshold"
        ));
    }
    let mut rows = Vec::new();
    let mut only_base = Vec::new();
    for (name, b) in &base.samples {
        match new.samples.iter().find(|(n, _)| n == name) {
            Some((_, n)) => rows.push(compare_samples(name, b, n, rule)),
            None => only_base.push(name.clone()),
        }
    }
    let only_new = new
        .samples
        .iter()
        .filter(|(n, _)| !base.samples.iter().any(|(b, _)| b == n))
        .map(|(n, _)| n.clone())
        .collect();
    Ok(Comparison {
        method: base.method,
        rule,
        rows,
        only_base,
        only_new,
        warnings,
    })
}

fn or_unknown(list: &[String]) -> String {
    if list.is_empty() {
        "unknown".to_owned()
    } else {
        list.join(", ")
    }
}

/// A value in the method's unit, for tables.
fn value(method: Method, v: f64) -> String {
    match method {
        Method::Instructions => group_thousands(v.round() as i64),
        Method::Wall if v >= 100.0 => format!("{v:.1}"),
        Method::Wall if v >= 1.0 => format!("{v:.3}"),
        Method::Wall => format!("{v:.5}"),
    }
}

fn group_thousands(v: i64) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

fn signed_pct(v: f64) -> String {
    format!("{}{:.1}%", if v >= 0.0 { "+" } else { "" }, v * 100.0)
}

/// The first line of every comparison report: tools (and the pull request
/// comment workflow) find the counts there without parsing the table.
pub fn marker(c: &Comparison) -> String {
    format!(
        "<!-- bench-track: slower={} faster={} -->",
        c.count(Verdict::Slower),
        c.count(Verdict::Faster)
    )
}

/// The comparison as Markdown: a summary line, the rule, every benchmark
/// with both medians, the change, both spreads and the threshold.
pub fn render_comparison(c: &Comparison, base_label: &str, new_label: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}", marker(c));
    let (slower, faster) = (c.count(Verdict::Slower), c.count(Verdict::Faster));
    let headline = match (slower, faster) {
        (0, 0) => "no benchmark moved beyond noise".to_owned(),
        (s, 0) => format!("{s} slower"),
        (0, f) => format!("{f} faster"),
        (s, f) => format!("{s} slower, {f} faster"),
    };
    let _ = writeln!(s, "### Benchmarks ({}): {headline}\n", c.method.as_str());
    let runs = c.rows.first().map_or((0, 0), |r| (r.base_runs, r.new_runs));
    let _ = writeln!(
        s,
        "`{base_label}` against `{new_label}`: the median of {} and {} runs, in {} per operation. \
         A change counts when it is beyond the threshold: the larger of {} and {} times the \
         larger spread (`(max - min) / median` of a side's runs).\n",
        runs.0,
        runs.1,
        c.method.unit(),
        pct(c.rule.floor),
        c.rule.noise_factor,
    );
    for w in &c.warnings {
        let _ = writeln!(s, "> **Note:** {w}.\n");
    }
    s.push_str("| Benchmark | base | new | change | spread base / new | threshold | |\n");
    s.push_str("|---|---:|---:|---:|---:|---:|---|\n");
    for r in &c.rows {
        let verdict = match r.verdict {
            Verdict::Slower => "**slower**",
            Verdict::Faster => "faster",
            Verdict::Same => "",
        };
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} / {} | {} | {verdict} |",
            r.name,
            value(c.method, r.base),
            value(c.method, r.new),
            signed_pct(r.change),
            pct(r.spread_base),
            pct(r.spread_new),
            pct(r.threshold),
        );
    }
    if !c.only_base.is_empty() {
        let _ = writeln!(s, "\nOnly in `{base_label}`: {}.", ticked(&c.only_base));
    }
    if !c.only_new.is_empty() {
        let _ = writeln!(s, "\nOnly in `{new_label}`: {}.", ticked(&c.only_new));
    }
    s
}

fn ticked(names: &[String]) -> String {
    names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

/// One machine's history, oldest first.
pub fn sort_history(docs: &mut [Doc]) {
    docs.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.git_rev.cmp(&b.git_rev)));
}

/// A machine's history as Markdown, newest first, at most `limit` rows:
/// every benchmark's median, with ▲ (slower) or ▼ (faster) where it moved
/// beyond noise against the row before. `row` names what a row is
/// (`"commit"`, `"release"`); its `git_rev` labels it.
pub fn render_history(
    machine: &str,
    docs: &[Doc],
    rule_for: impl Fn(Method) -> Rule,
    limit: usize,
    row: &str,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "## {machine}\n");
    let Some(last) = docs.last() else {
        s.push_str("No results yet.\n");
        return s;
    };
    let method = last.method;
    let rule = rule_for(method);
    let mut names: Vec<&str> = Vec::new();
    for d in docs.iter().rev().take(limit) {
        for (n, _) in &d.results {
            if !names.contains(&n.as_str()) {
                names.push(n);
            }
        }
    }
    let _ = writeln!(
        s,
        "Median {} per operation. ▲ slower, ▼ faster than the {row} below it, beyond the \
         larger of {} and {} times the spread.\n",
        method.unit(),
        pct(rule.floor),
        rule.noise_factor
    );
    let _ = write!(s, "| {row} | date |");
    for n in &names {
        let _ = write!(s, " `{n}` |");
    }
    s.push_str("\n|---|---|");
    s.push_str(&"---:|".repeat(names.len()));
    s.push('\n');
    let start = docs.len().saturating_sub(limit);
    for i in (start..docs.len()).rev() {
        let d = &docs[i];
        let rev: String = d.git_rev.chars().take(10).collect();
        let _ = write!(s, "| `{rev}` | {} |", date(d.time));
        for n in &names {
            let cell = match d.results.iter().find(|(r, _)| r == n) {
                None => String::new(),
                Some((_, xs)) => {
                    let mut cell = value(method, median(xs));
                    let before = i
                        .checked_sub(1)
                        .and_then(|j| docs[j].results.iter().find(|(r, _)| r == n));
                    if let Some((_, prev)) = before.filter(|_| docs[i - 1].method == method) {
                        let row = compare_samples(n, prev, xs, rule);
                        match row.verdict {
                            Verdict::Slower => {
                                let _ = write!(cell, " ▲{}", signed_pct(row.change));
                            }
                            Verdict::Faster => {
                                let _ = write!(cell, " ▼{}", signed_pct(row.change));
                            }
                            Verdict::Same => {}
                        }
                    }
                    cell
                }
            };
            let _ = write!(s, " {cell} |");
        }
        s.push('\n');
    }
    s
}

/// `YYYY-MM-DD` (UTC) of a Unix time.
pub fn date(unix: u64) -> String {
    // Civil-from-days, Howard Hinnant's algorithm.
    let days = (unix / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Groups documents by machine, each machine's oldest first.
pub fn by_machine(docs: Vec<Doc>) -> BTreeMap<String, Vec<Doc>> {
    let mut map: BTreeMap<String, Vec<Doc>> = BTreeMap::new();
    for d in docs {
        map.entry(d.machine.clone()).or_default().push(d);
    }
    for list in map.values_mut() {
        sort_history(list);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        Rule::for_method(Method::Wall)
    }

    #[test]
    fn medians_and_spreads() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert!(median(&[]).is_nan());
        assert_eq!(spread(&[5.0]), 0.0);
        assert!((spread(&[90.0, 100.0, 110.0]) - 0.2).abs() < 1e-12);
    }

    #[test]
    fn a_synthetic_regression_is_flagged() {
        // 20% slower, runs within 2% of each other.
        let row = compare_samples("ecs/iter", &[100.0, 101.0, 99.0], &[120.0, 121.0, 119.0], rule());
        assert_eq!(row.verdict, Verdict::Slower);
        assert!((row.change - 0.2).abs() < 1e-12);
        assert!((row.threshold - 0.05).abs() < 1e-12, "{row:?}");
    }

    #[test]
    fn noise_is_not_flagged() {
        // Medians 3% apart, inside the 5% floor.
        let row = compare_samples("ecs/iter", &[100.0, 104.0, 97.0], &[103.0, 99.0, 105.0], rule());
        assert_eq!(row.verdict, Verdict::Same, "{row:?}");
        // 8% apart, but the runs spread 6%: the threshold is 12%.
        let row = compare_samples("ecs/iter", &[100.0, 97.0, 103.0], &[108.0, 105.0, 111.0], rule());
        assert!((row.threshold - 0.12).abs() < 1e-9, "{row:?}");
        assert_eq!(row.verdict, Verdict::Same, "{row:?}");
        // The same change with tight runs counts.
        let row = compare_samples("ecs/iter", &[100.0, 100.5, 99.5], &[108.0, 108.5, 107.5], rule());
        assert_eq!(row.verdict, Verdict::Slower, "{row:?}");
    }

    #[test]
    fn one_wild_run_widens_the_threshold_instead_of_moving_the_median() {
        let row = compare_samples("x", &[100.0, 100.0, 100.0], &[100.0, 101.0, 180.0], rule());
        assert_eq!(row.new, 101.0);
        assert_eq!(row.verdict, Verdict::Same, "{row:?}");
    }

    #[test]
    fn faster_is_reported_as_faster() {
        let row = compare_samples("x", &[100.0, 100.0], &[80.0, 80.0], rule());
        assert_eq!(row.verdict, Verdict::Faster);
    }

    #[test]
    fn instruction_counts_use_a_one_percent_floor() {
        let r = Rule::for_method(Method::Instructions);
        let same = compare_samples("x", &[1000.0; 3], &[1008.0; 3], r);
        assert_eq!(same.verdict, Verdict::Same);
        let slower = compare_samples("x", &[1000.0; 3], &[1030.0; 3], r);
        assert_eq!(slower.verdict, Verdict::Slower);
        assert_eq!(slower.threshold, 0.01);
    }

    fn instructions_doc(machine: &str, rev: &str, time: u64, results: &[(&str, &[f64])]) -> String {
        let rows: Vec<String> = results
            .iter()
            .map(|(n, xs)| {
                let xs: Vec<String> = xs.iter().map(|x| x.to_string()).collect();
                format!(r#"{{"name": "{n}", "samples": [{}]}}"#, xs.join(", "))
            })
            .collect();
        format!(
            r#"{{"schema_version": 1, "env": {{"method": "instructions", "machine": "{machine}",
               "git_rev": "{rev}", "rustc": "rustc 1.0", "commit_time": {time}, "workloads": "abc"}},
               "results": [{}]}}"#,
            rows.join(", ")
        )
    }

    #[test]
    fn ponko_bench_documents_are_read_as_wall_clock_runs() {
        let text = r#"{"schema_version": 1,
            "env": {"rustc": "rustc 1.99.1", "git_rev": "abc", "os": "macos", "arch": "aarch64",
                    "cpu": "Apple M4 Max", "unix_time": 1794472447},
            "results": [{"name": "ecs/iter-1k/ponko", "kind": "steady", "p50_ms": 0.25, "min_ms": 0.2}]}"#;
        let d = parse_doc(text).unwrap();
        assert_eq!(d.method, Method::Wall);
        assert_eq!(d.machine, "macos-aarch64-apple-m4-max");
        assert_eq!(d.time, 1794472447);
        assert_eq!(d.results, vec![("ecs/iter-1k/ponko".to_owned(), vec![0.25])]);
    }

    #[test]
    fn unknown_schema_versions_are_refused() {
        let err = parse_doc(r#"{"schema_version": 2, "env": {}, "results": []}"#).unwrap_err();
        assert!(err.contains("schema_version"), "{err}");
    }

    #[test]
    fn repeated_wall_clock_runs_pool_into_one_side() {
        let doc = |p50: f64| {
            parse_doc(&format!(
                r#"{{"schema_version": 1, "env": {{"machine": "m", "git_rev": "a"}},
                    "results": [{{"name": "x", "p50_ms": {p50}}}]}}"#
            ))
            .unwrap()
        };
        let side = pool(&[doc(1.0), doc(3.0), doc(2.0)]).unwrap();
        assert_eq!(side.samples, vec![("x".to_owned(), vec![1.0, 3.0, 2.0])]);
        assert_eq!(side.git_revs, vec!["a".to_owned()]);
    }

    #[test]
    fn a_whole_comparison_names_the_slower_benchmark() {
        let base = parse_doc(&instructions_doc(
            "ci",
            "base",
            1,
            &[
                ("ecs/iter/10k", &[50_000.0, 50_000.0, 50_010.0]),
                ("ecs/get/10k", &[90_000.0; 3]),
            ],
        ))
        .unwrap();
        let new = parse_doc(&instructions_doc(
            "ci",
            "new",
            2,
            &[
                ("ecs/iter/10k", &[65_000.0, 65_002.0, 65_000.0]),
                ("ecs/get/10k", &[90_100.0; 3]),
            ],
        ))
        .unwrap();
        let (b, n) = (pool(&[base]).unwrap(), pool(&[new]).unwrap());
        let c = compare(&b, &n, Rule::for_method(Method::Instructions), false).unwrap();
        assert_eq!((c.count(Verdict::Slower), c.count(Verdict::Same)), (1, 1));
        let md = render_comparison(&c, "main", "pull request");
        assert!(md.starts_with("<!-- bench-track: slower=1 faster=0 -->\n"), "{md}");
        assert!(md.contains("1 slower"), "{md}");
        assert!(
            md.contains("| `ecs/iter/10k` | 50,000 | 65,000 | +30.0% | 0.0% / 0.0% | 1.0% | **slower** |"),
            "{md}"
        );
        assert!(
            md.contains("| `ecs/get/10k` | 90,000 | 90,100 | +0.1% | 0.0% / 0.0% | 1.0% |  |"),
            "{md}"
        );
        assert!(md.contains("the larger of 1.0% and 2 times"), "{md}");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn different_machines_are_refused_unless_asked() {
        let a = pool(&[parse_doc(&instructions_doc("a", "1", 1, &[("x", &[1.0])])).unwrap()]).unwrap();
        let b = pool(&[parse_doc(&instructions_doc("b", "2", 2, &[("x", &[1.0])])).unwrap()]).unwrap();
        let rule = Rule::for_method(Method::Instructions);
        assert!(compare(&a, &b, rule, false).unwrap_err().contains("different machines"));
        let c = compare(&a, &b, rule, true).unwrap();
        // The machines, and one run per side.
        assert_eq!(c.warnings.len(), 2, "{:?}", c.warnings);
        assert!(c.warnings[1].contains("only 1 run(s)"), "{:?}", c.warnings);
    }

    #[test]
    fn sides_built_with_different_function_alignment_are_flagged() {
        let doc = |align: &str| {
            parse_doc(&format!(
                r#"{{"schema_version": 1, "env": {{"machine": "m", "git_rev": "a", "code_align": {align}}},
                    "results": [{{"name": "x", "p50_ms": 1.0}}]}}"#
            ))
            .unwrap()
        };
        let aligned = pool(&[doc("128"), doc("128"), doc("128")]).unwrap();
        let plain = pool(&[doc("null"), doc("null"), doc("null")]).unwrap();
        let rule = Rule::for_method(Method::Wall);
        let c = compare(&aligned, &plain, rule, false).unwrap();
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        assert!(
            c.warnings[0].contains("different function alignment (128 and none)"),
            "{:?}",
            c.warnings
        );
        assert!(
            compare(&aligned, &aligned.clone(), rule, false)
                .unwrap()
                .warnings
                .is_empty()
        );
        assert!(
            compare(&plain, &plain.clone(), rule, false)
                .unwrap()
                .warnings
                .is_empty()
        );
    }

    #[test]
    fn benchmarks_on_one_side_only_are_listed() {
        let a =
            pool(&[parse_doc(&instructions_doc("m", "1", 1, &[("x", &[1.0]), ("gone", &[1.0])])).unwrap()]).unwrap();
        let b =
            pool(&[parse_doc(&instructions_doc("m", "2", 2, &[("x", &[1.0]), ("added", &[1.0])])).unwrap()]).unwrap();
        let c = compare(&a, &b, Rule::for_method(Method::Instructions), false).unwrap();
        assert_eq!(
            (c.only_base.clone(), c.only_new.clone()),
            (vec!["gone".to_owned()], vec!["added".to_owned()])
        );
        let md = render_comparison(&c, "a", "b");
        assert!(md.contains("Only in `a`: `gone`."), "{md}");
        assert!(md.contains("Only in `b`: `added`."), "{md}");
    }

    #[test]
    fn the_history_marks_the_commit_that_moved() {
        let docs: Vec<Doc> = [("c3", 30, 130.0), ("c1", 10, 100.0), ("c2", 20, 100.5)]
            .iter()
            .map(|(rev, t, v)| parse_doc(&instructions_doc("ci", rev, *t, &[("ecs/iter/10k", &[*v, *v, *v])])).unwrap())
            .collect();
        let map = by_machine(docs);
        let docs = &map["ci"];
        assert_eq!(
            docs.iter().map(|d| d.git_rev.as_str()).collect::<Vec<_>>(),
            ["c1", "c2", "c3"]
        );
        let md = render_history("ci", docs, Rule::for_method, 10, "commit");
        let lines: Vec<&str> = md.lines().filter(|l| l.starts_with("| `c")).collect();
        assert_eq!(lines.len(), 3, "{md}");
        assert!(lines[0].starts_with("| `c3` | 1970-01-01 | 130 ▲+29.4% |"), "{md}");
        assert_eq!(lines[1], "| `c2` | 1970-01-01 | 101 |");
        assert_eq!(lines[2], "| `c1` | 1970-01-01 | 100 |");
        let short = render_history("ci", docs, Rule::for_method, 1, "commit");
        assert_eq!(short.lines().filter(|l| l.starts_with("| `c")).count(), 1);
    }

    #[test]
    fn dates_and_numbers_print_plainly() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_794_472_447), "2026-11-12");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
        assert_eq!(group_thousands(-999), "-999");
        assert_eq!(
            machine_slug("linux", "x86_64", "AMD EPYC 7763 64-Core Processor"),
            "linux-x86_64-amd-epyc-7763-64-core-processor"
        );
    }
}
