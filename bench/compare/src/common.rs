//! What every group shares: percentiles, the machine and writing files.

use serde_json::{Value, json};

/// Nearest-rank percentiles, as the ponko report computes them.
#[derive(Clone, Copy)]
pub struct Dist {
    pub samples: usize,
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

impl Dist {
    pub fn of(values: &mut [f64]) -> Dist {
        values.sort_by(f64::total_cmp);
        let rank = |pct: usize| values[(pct * values.len()).div_ceil(100).max(1) - 1];
        Dist {
            samples: values.len(),
            min: values[0],
            p50: rank(50),
            p95: rank(95),
            p99: rank(99),
            max: values[values.len() - 1],
            mean: values.iter().sum::<f64>() / values.len() as f64,
        }
    }

    pub fn json(&self) -> Value {
        let round = |v: f64| (v * 1e6).round() / 1e6;
        json!({
            "samples": self.samples,
            "min": round(self.min),
            "p50": round(self.p50),
            "p95": round(self.p95),
            "p99": round(self.p99),
            "max": round(self.max),
            "mean": round(self.mean),
        })
    }
}

/// First line of a command's output, or `"unknown"`.
pub fn command_line(cmd: &str, args: &[&str]) -> String {
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

/// The 1, 5 and 15 minute load averages (how busy the machine is), as the
/// ponko run records them, or `"unknown"`.
pub fn load_average() -> String {
    if cfg!(target_os = "macos") {
        let s = command_line("sysctl", &["-n", "vm.loadavg"]);
        return s.trim_matches(|c| c == '{' || c == '}' || c == ' ').to_owned();
    }
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

pub fn write(path: &std::ffi::OsStr, what: &str, text: String) {
    match std::fs::write(path, text) {
        Ok(()) => println!("{what} written to {}", path.to_string_lossy()),
        Err(e) => {
            eprintln!("could not write {}: {e}", path.to_string_lossy());
            std::process::exit(1);
        }
    }
}
