//! `ponko schema`: writes the JSON Schema of the glTF custom properties a
//! game reads (the engine's convention plus the game's reflected
//! components), for an editor or a script to check a model's properties
//! before it is exported. It runs the game once with `PONKO_SCHEMA=<file>`:
//! the game runs its setup headless (no window), writes the file and
//! quits.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const USAGE: &str = "\
ponko schema: write the glTF custom properties schema of a game

Usage: ponko schema [--out <file>] [--bin <name> | --example <name>] [-p <package>]
                    [--features <list>] [--manifest-path <Cargo.toml>]

Builds and runs the game once without a window (PONKO_SCHEMA=<file>): its
setup runs, so the components it reflects are in the schema, then it writes
the schema and quits. The schema describes a glTF node's extras: the
`ponko:` keys and every reflected component, for editors to validate
custom properties before exporting. The game needs the `3d` feature.

Options:
  --out <file>               Where to write it (default: ponko-schema.json)
  --bin <name>               Run this binary of the package
  --example <name>           Run this example instead
  -p, --package <name>       The package to run, in a workspace
  --features <list>          Cargo features to build with
  --manifest-path <path>     The package's Cargo.toml

Guide: https://ponko.dev/guide/blender.html";

/// What `ponko schema` was asked for.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Options {
    pub out: Option<PathBuf>,
    /// Cargo's target selection and the like, passed to `cargo run` as they are.
    pub cargo_args: Vec<String>,
}

impl Options {
    pub(crate) fn parse(args: &[String]) -> Result<Self, String> {
        let mut o = Options::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = |name: &str| {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value\n\n{USAGE}"))
            };
            match arg.as_str() {
                "--out" | "-o" => o.out = Some(PathBuf::from(value(arg)?)),
                "--bin" | "--example" | "-p" | "--package" | "--features" | "--manifest-path" => {
                    let v = value(arg)?;
                    o.cargo_args.push(arg.clone());
                    o.cargo_args.push(v);
                }
                other => return Err(format!("unknown option `{other}`\n\n{USAGE}")),
            }
        }
        Ok(o)
    }
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// The `cargo run` that writes the schema to `out`.
pub(crate) fn command(options: &Options, out: &Path, cwd: &Path) -> Command {
    let mut cmd = Command::new(cargo());
    cmd.current_dir(cwd)
        .args(["run", "--quiet"])
        .args(&options.cargo_args)
        .env("PONKO_SCHEMA", out)
        // A schema run never needs these, and must not open anything.
        .env_remove("PONKO_SMOKE")
        .env_remove("PONKO_CONTROL");
    cmd
}

/// Runs `ponko schema`; returns what to print.
pub(crate) fn schema_command(options: &Options, cwd: &Path) -> Result<String, String> {
    let out = cwd.join(
        options
            .out
            .clone()
            .unwrap_or_else(|| PathBuf::from("ponko-schema.json")),
    );
    // A schema left from an earlier run must not pass for this one.
    let _ = std::fs::remove_file(&out);
    let status = command(options, &out, cwd)
        .status()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("the game failed to write the schema ({status})"));
    }
    let text = std::fs::read_to_string(&out).map_err(|_| {
        format!(
            "the game ran but wrote no schema to {}: is it a Ponko game with the `3d` feature, started with `Game::run`?",
            out.display()
        )
    })?;
    let schema: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{} isn't JSON: {e}", out.display()))?;
    let keys = schema["properties"]
        .as_object()
        .map_or(0, |p| p.len().saturating_sub(1));
    Ok(format!("wrote {} ({keys} properties)", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn options_pass_cargo_target_selection_through() {
        let o = parse(&["--out", "s.json", "--example", "yard", "--features", "3d"]).unwrap();
        assert_eq!(o.out, Some(PathBuf::from("s.json")));
        assert_eq!(o.cargo_args, ["--example", "yard", "--features", "3d"]);
        assert!(parse(&["--bin"]).unwrap_err().contains("--bin needs a value"));
        assert!(parse(&["--window"]).unwrap_err().contains("unknown option `--window`"));
    }

    #[test]
    fn the_run_sets_the_schema_file_and_no_window_options() {
        let o = parse(&["--bin", "game"]).unwrap();
        let cmd = command(&o, Path::new("/tmp/out.json"), Path::new("/tmp"));
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["run", "--quiet", "--bin", "game"]);
        let schema = cmd.get_envs().find(|(k, _)| *k == "PONKO_SCHEMA").and_then(|(_, v)| v);
        assert_eq!(schema, Some(std::ffi::OsStr::new("/tmp/out.json")));
    }
}
