use ponko_image_diff::{Tolerance, compare_files};
use std::path::Path;

pub const USAGE: &str = "ponko image-diff a.png b.png [--tolerance <0..255>] [--threshold <percent>] [--heatmap out.png]\nDefaults: tolerance 0, threshold 0%. Prints JSON; exit 1 if percent over tolerance exceeds threshold or inputs are invalid. Heatmap must be a new file.";

pub fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(USAGE.into());
    }
    if args.len() < 2 {
        return Err(USAGE.into());
    }
    let mut tolerance = Tolerance {
        channel: 0,
        outliers: 0.0,
        edge: 0,
    };
    let mut output = None;
    let (options, remainder) = args[2..].as_chunks::<2>();
    for pair in options {
        match pair[0].as_str() {
            "--tolerance" => tolerance.channel = pair[1].parse().map_err(|_| "tolerance must be 0..255")?,
            "--threshold" => {
                let percent: f32 = pair[1].parse().map_err(|_| "threshold must be 0..100")?;
                if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                    return Err("threshold must be 0..100".into());
                }
                tolerance.outliers = percent / 100.0;
            }
            "--heatmap" => output = Some(Path::new(&pair[1])),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    if !remainder.is_empty() {
        return Err(USAGE.into());
    }
    let d = compare_files(Path::new(&args[0]), Path::new(&args[1]), tolerance, output)?;
    let summary = serde_json::to_string_pretty(
        &serde_json::json!({"summary": d, "within": d.within(tolerance), "heatmap": output}),
    )
    .unwrap();
    if d.within(tolerance) {
        Ok(summary)
    } else {
        println!("{summary}");
        Err("images exceed threshold".into())
    }
}
