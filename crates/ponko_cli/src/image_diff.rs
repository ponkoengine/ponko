use ponko_image_diff::{Tolerance, compare_files};
use std::path::Path;

pub const USAGE: &str = "\
ponko image-diff: compare two PNG images

Usage: ponko image-diff <a.png> <b.png> [--tolerance <0..255>] [--threshold <percent>]
                        [--heatmap <out.png>]

Prints the comparison as JSON (`summary`, `within`, `heatmap`). Exits with 1
when the share of pixels differing by more than the tolerance is over the
threshold, or when an image can't be read or the sizes differ.

Options:
  --tolerance <0..255>     How far a channel may differ and still count as
                           equal (default 0)
  --threshold <percent>    The share of pixels that may differ (default 0)
  --heatmap <out.png>      Write where they differ as an image (a new file)

Guide: https://ponko.dev/guide/agents.html";

pub fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(USAGE.into());
    }
    let mut tolerance = Tolerance {
        channel: 0,
        outliers: 0.0,
        edge: 0,
    };
    let mut output = None;
    let mut images = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .map(String::as_str)
                .ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))
        };
        match arg.as_str() {
            "--tolerance" => tolerance.channel = value()?.parse().map_err(|_| "tolerance must be 0..255")?,
            "--threshold" => {
                let percent: f32 = value()?.parse().map_err(|_| "threshold must be 0..100")?;
                if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                    return Err("threshold must be 0..100".into());
                }
                tolerance.outliers = percent / 100.0;
            }
            "--heatmap" => output = Some(Path::new(value()?)),
            other if other.starts_with("--") => return Err(format!("unknown option `{other}`\n\n{USAGE}")),
            image => images.push(image),
        }
    }
    let [a, b] = images[..] else {
        return Err(format!("give two images, not {}\n\n{USAGE}", images.len()));
    };
    let d = compare_files(Path::new(a), Path::new(b), tolerance, output)?;
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
