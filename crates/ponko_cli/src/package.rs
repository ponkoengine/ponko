//! `ponko package`: a game, built in release, laid out and archived the way
//! players download it, then started once headless to check it runs.
//! `ponko notices`: only the third-party notices, for a library.
//!
//! What goes where is [`layout`] (pure, tested for every platform); the
//! archives and icons are [`archive`]; the notices are [`notices`]; the
//! smoke run is [`smoke`]. This module runs Cargo and ties them together.

mod archive;
mod layout;
mod notices;
mod smoke;

use layout::{Built, FileCopy, Format, Game, Metadata, Os, PackMeta, Platform, Source};
use serde_json::Value;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const USAGE: &str = "\
ponko package: build a game and package it the way players download it

Usage:
  ponko package [--bin <name> | --example <name>] [options]

Run it in the game's folder. It builds the game in release, collects its
assets, licenses and THIRD-PARTY-NOTICES.txt, lays them out for the platform,
archives them and starts the packaged game once headless (`PONKO_SMOKE`) to
check it runs. The files under assets/ ship as one asset pack, assets.pack
(as `ponko pack` makes it), which the game reads where its assets/ folder
would be. The pack holds them cooked (as `ponko cook` makes them: textures
compressed when the game reads KTX2, meshes optimized, sounds in FLAC,
shaders checked), under the same names. What goes in comes from `[package.metadata.ponko]` in the game's
Cargo.toml (name, id, icon, assets, files, pack); see the guide's packaging
chapter.

What it makes, by platform (the host's unless --target or --web):
  Windows  a folder with the .exe, zipped                 (--format zip)
  macOS    an app bundle (Name.app) in a disk image       (--format dmg; zip
           where there is no hdiutil)
  Linux    a folder, as a .tar.gz                         (--format tar.gz)
  Web      a folder to put on any static web host         (--web)

Options:
  --bin <name>, --example <name>  The program (default: the package's binary)
  -p, --package <name>            The package, in a workspace
  --manifest-path <path>          The game's Cargo.toml (default: found from here)
  --target <triple>               Build for another platform (cross-compiling)
  --web                           Build for browsers (wasm32-unknown-unknown;
                                  needs wasm-bindgen, see the guide)
  --threads                       With --web: build with worker threads
                                  (nightly with rust-src, or the toolchain
                                  PONKO_WEB_TOOLCHAIN names). Adds
                                  ponko-worker.js and files that send the
                                  cross-origin isolation headers on Netlify,
                                  Cloudflare Pages, Vercel and Apache; the
                                  guide's \"Threads on the web\" has the rest
  --features <list>, --no-default-features   As for cargo build
  --profile <name>                The Cargo profile (default: release)
  --locked                        As for cargo build
  --no-build                      Package the program as already built
  --no-pack                       Ship the assets as loose files in assets/
                                  instead of one assets.pack (cooked too)
  --no-cook                       Ship the source files as they are
  --format <list>                 folder, zip, tar.gz, dmg (comma-separated);
                                  `folder` alone: the plain folder, no archive
                                  and no app bundle
  --out <dir>                     Where to put the package (default: target/package)
  --no-smoke                      Don't start the packaged game
  --smoke-ticks <n>               Ticks of the smoke run (default: 120)
  --smoke-timeout <seconds>       How long it may take (default: 120)
  --sign <program>                Run <program> to sign: `<program> sign <path>`
                                  before archiving (the .app, or the program),
                                  `<program> archive <path>` for each archive
                                  (see the guide; without it a macOS app is
                                  sealed ad hoc, which is not a trusted signature)";

pub const NOTICES_USAGE: &str = "\
ponko notices: write THIRD-PARTY-NOTICES.txt for a crate

Usage:
  ponko notices --out <file> [-p <package>] [--title <text>] [--manifest-path <path>]
                [--target <triple>] [--features <list>] [--no-default-features]

Lists every library the crate's library links on the platform (its normal
dependencies with the given features), with their licenses, the way
`ponko package` writes them for a game. For shipping a library, such as a
C library built from Rust.";

/// The parsed arguments of `ponko package` (and `ponko notices`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub bin: Option<String>,
    pub example: Option<String>,
    pub package: Option<String>,
    pub manifest_path: Option<PathBuf>,
    pub target: Option<String>,
    pub web: bool,
    /// `--threads`: a web build with worker threads.
    pub threads: bool,
    pub features: Option<String>,
    pub no_default_features: bool,
    pub profile: Option<String>,
    pub locked: bool,
    pub no_build: bool,
    pub no_pack: bool,
    pub no_cook: bool,
    pub formats: Option<Vec<Format>>,
    pub out: Option<PathBuf>,
    pub no_smoke: bool,
    pub smoke_ticks: Option<u64>,
    pub smoke_timeout: Option<u64>,
    pub sign: Option<PathBuf>,
    /// `ponko notices` only.
    pub title: Option<String>,
}

impl Options {
    pub fn parse(args: &[String], usage: &str) -> Result<Options, String> {
        let mut o = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
                _ => (arg.as_str(), None),
            };
            let mut value = || {
                inline
                    .clone()
                    .or_else(|| args.next().cloned())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| format!("{flag} needs a value\n\n{usage}"))
            };
            let number = |v: String| {
                v.parse::<u64>()
                    .map_err(|_| format!("{flag} takes a number, not `{v}`"))
            };
            match flag {
                "--bin" => o.bin = Some(value()?),
                "--example" => o.example = Some(value()?),
                "-p" | "--package" => o.package = Some(value()?),
                "--manifest-path" => o.manifest_path = Some(value()?.into()),
                "--target" => o.target = Some(value()?),
                "--web" => o.web = true,
                "--threads" => o.threads = true,
                "--features" | "-F" => o.features = Some(value()?),
                "--no-default-features" => o.no_default_features = true,
                "--profile" => o.profile = Some(value()?),
                "--release" => o.profile = Some("release".into()),
                "--locked" => o.locked = true,
                "--no-build" => o.no_build = true,
                "--no-pack" => o.no_pack = true,
                "--no-cook" => o.no_cook = true,
                "--format" => o.formats = Some(Format::parse_list(&value()?)?),
                "--out" => o.out = Some(value()?.into()),
                "--no-smoke" => o.no_smoke = true,
                "--smoke-ticks" => o.smoke_ticks = Some(number(value()?)?.max(1)),
                "--smoke-timeout" => o.smoke_timeout = Some(number(value()?)?.max(1)),
                "--sign" => o.sign = Some(value()?.into()),
                "--title" => o.title = Some(value()?),
                other => return Err(format!("unknown argument `{other}`\n\n{usage}")),
            }
        }
        if o.bin.is_some() && o.example.is_some() {
            return Err("give --bin or --example, not both".into());
        }
        if o.web && o.target.as_deref().is_some_and(|t| t != WASM) {
            return Err(format!("--web builds for {WASM}; leave out --target"));
        }
        if o.threads && !o.web {
            return Err("--threads is for web builds (desktop builds always have threads): add --web".into());
        }
        Ok(o)
    }
}

const WASM: &str = "wasm32-unknown-unknown";

/// What a web build with threads compiles with (each after `-C`): atomics
/// in every crate, and a shared, imported memory (at most 1 GiB) with the
/// thread-local symbols wasm-bindgen needs, which the linker doesn't add by
/// itself. As in `web/build.sh --threads`.
const WEB_THREAD_CODEGEN: [&str; 8] = [
    "target-feature=+atomics,+bulk-memory,+mutable-globals",
    "link-arg=--shared-memory",
    "link-arg=--max-memory=1073741824",
    "link-arg=--import-memory",
    "link-arg=--export=__wasm_init_tls",
    "link-arg=--export=__tls_size",
    "link-arg=--export=__tls_align",
    "link-arg=--export=__tls_base",
];

/// [`WEB_THREAD_CODEGEN`] as rustc arguments, `-C <option>` each: the same
/// words as `web/build.sh` passes, so the two builds share cargo's cache.
fn web_thread_flags() -> Vec<&'static str> {
    WEB_THREAD_CODEGEN.iter().flat_map(|o| ["-C", *o]).collect()
}

/// The toolchain a web build with threads uses (it rebuilds the standard
/// library with `-Zbuild-std`): `PONKO_WEB_TOOLCHAIN`, else `nightly`.
fn web_threads_toolchain(env: impl Fn(&str) -> Option<String>) -> String {
    env("PONKO_WEB_TOOLCHAIN")
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "nightly".into())
}

/// The environment of a web build with threads: the toolchain for rustup's
/// `cargo`, and [`web_thread_flags`] added where cargo reads them. Cargo
/// uses the first of `CARGO_ENCODED_RUSTFLAGS`, `RUSTFLAGS` and the
/// target's flags that is set, so the flags go into that one and keep what
/// it had.
fn web_threads_env(env: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let mut out = vec![("RUSTUP_TOOLCHAIN".to_string(), web_threads_toolchain(&env))];
    let flags = web_thread_flags().join(" ");
    let set = |name: &str| env(name).filter(|v| !v.is_empty());
    if let Some(encoded) = set("CARGO_ENCODED_RUSTFLAGS") {
        let mut all = web_thread_flags();
        all.extend(encoded.split('\x1f'));
        out.push(("CARGO_ENCODED_RUSTFLAGS".into(), all.join("\x1f")));
    } else if let Some(plain) = set("RUSTFLAGS") {
        out.push(("RUSTFLAGS".into(), format!("{flags} {plain}")));
    } else {
        let name = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
        let value = match set(name) {
            Some(own) => format!("{flags} {own}"),
            None => flags,
        };
        out.push((name.into(), value));
    }
    out
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Runs a program and returns its standard output; its errors go to ours.
fn output(mut cmd: Command, what: &str) -> Result<String, String> {
    cmd.stderr(Stdio::inherit());
    let out = cmd.output().map_err(|e| format!("can't run {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed ({})", out.status));
    }
    String::from_utf8(out.stdout).map_err(|_| format!("{what} printed something that is not UTF-8"))
}

fn json(text: &str, what: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| format!("{what}: {e}"))
}

pub fn host_triple() -> Result<String, String> {
    let mut cmd = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    cmd.arg("-vV");
    let text = output(cmd, "rustc -vV")?;
    text.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(|h| h.trim().to_string())
        .ok_or_else(|| "rustc -vV names no host".into())
}

/// The game's package as `cargo metadata --no-deps` describes it.
struct Project {
    manifest: PathBuf,
    package: Value,
    workspace_root: PathBuf,
    target_dir: PathBuf,
}

fn project(options: &Options, cwd: &Path) -> Result<Project, String> {
    let manifest = match &options.manifest_path {
        Some(m) => cwd.join(m),
        None => {
            let mut cmd = Command::new(cargo());
            cmd.current_dir(cwd)
                .args(["locate-project", "--message-format", "plain"]);
            PathBuf::from(output(cmd, "cargo locate-project")?.trim())
        }
    };
    let mut cmd = Command::new(cargo());
    cmd.current_dir(cwd)
        .args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"])
        .arg(&manifest);
    let meta = json(&output(cmd, "cargo metadata")?, "cargo metadata")?;
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let same = |p: &Value| {
        p["manifest_path"]
            .as_str()
            .is_some_and(|m| Path::new(m).canonicalize().ok() == manifest.canonicalize().ok())
    };
    let package = match &options.package {
        Some(name) => packages
            .iter()
            .find(|p| p["name"] == name.as_str())
            .cloned()
            .ok_or_else(|| format!("no package `{name}` in this workspace"))?,
        None => match packages.iter().find(|p| same(p)) {
            Some(p) => p.clone(),
            None if packages.len() == 1 => packages[0].clone(),
            None => {
                let names: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
                return Err(format!(
                    "{} is a workspace: pick the game with -p (one of {})",
                    manifest.display(),
                    names.join(", ")
                ));
            }
        },
    };
    let path = |v: &Value, what: &str| {
        v.as_str()
            .map(PathBuf::from)
            .ok_or(format!("cargo metadata has no {what}"))
    };
    Ok(Project {
        manifest: path(&package["manifest_path"], "manifest path")?,
        workspace_root: path(&meta["workspace_root"], "workspace root")?,
        target_dir: path(&meta["target_directory"], "target directory")?,
        package,
    })
}

/// (target name, is an example) of the program to package.
fn program(options: &Options, package: &Value) -> Result<(String, bool), String> {
    let targets = package["targets"].as_array().cloned().unwrap_or_default();
    let of_kind = |kind: &str| -> Vec<String> {
        targets
            .iter()
            .filter(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == kind)))
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    };
    let name = package["name"].as_str().unwrap_or_default();
    if let Some(example) = &options.example {
        let examples = of_kind("example");
        return if examples.contains(example) {
            Ok((example.clone(), true))
        } else {
            Err(format!(
                "{name} has no example `{example}` (it has: {})",
                examples.join(", ")
            ))
        };
    }
    let bins = of_kind("bin");
    if let Some(bin) = &options.bin {
        return if bins.contains(bin) {
            Ok((bin.clone(), false))
        } else {
            Err(format!("{name} has no binary `{bin}` (it has: {})", bins.join(", ")))
        };
    }
    match (bins.as_slice(), package["default_run"].as_str()) {
        ([one], _) => Ok((one.clone(), false)),
        (_, Some(default)) => Ok((default.to_string(), false)),
        ([], _) => Err(format!(
            "{name} has no binary to package (src/main.rs); give --example <name> for an example"
        )),
        (many, None) => Err(format!(
            "{name} has several binaries: pick one with --bin ({})",
            many.join(", ")
        )),
    }
}

/// Every file of an asset entry (a folder or a file), hidden files left out.
fn asset_files(dir: &Path, rel: &Path, out: &mut Vec<FileCopy>) -> Result<(), String> {
    let path = dir.join(rel);
    let meta = std::fs::metadata(&path).map_err(|_| {
        format!(
            "[package.metadata.ponko] assets: `{}` does not exist next to Cargo.toml",
            rel.display()
        )
    })?;
    if meta.is_file() {
        out.push(FileCopy {
            from: path,
            to: rel.to_path_buf(),
        });
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .filter(|n| !n.to_string_lossy().starts_with('.'))
        .collect();
    entries.sort();
    for name in entries {
        asset_files(dir, &rel.join(name), out)?;
    }
    Ok(())
}

/// The game described by its metadata, with every file it names found.
fn game(
    project: &Project,
    target: &str,
    example: bool,
    notes: &mut Vec<String>,
) -> Result<(Game, Option<Vec<u8>>, PackMeta), String> {
    let p = &project.package;
    let dir = project.manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
    let meta = Metadata::from_cargo(&p["metadata"], example.then_some(target))?;
    let package_name = p["name"].as_str().unwrap_or(target);
    let name = meta
        .name
        .clone()
        .unwrap_or_else(|| if example { target } else { package_name }.to_string());
    let slug = Some(layout::slug(&name))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| layout::slug(target));
    let id = match meta.id {
        Some(id) => id,
        None => {
            let id = format!("local.{slug}");
            notes.push(format!(
                "no `id` in [package.metadata.ponko]: the app id is `{id}`; set a reverse-DNS id you own, such as `com.example.{slug}`"
            ));
            id
        }
    };
    let mut assets = Vec::new();
    let asset_list = match meta.assets {
        Some(list) => list,
        None if dir.join("assets").is_dir() => vec!["assets".to_string()],
        None => Vec::new(),
    };
    for a in &asset_list {
        asset_files(&dir, &layout::package_path(a)?, &mut assets)?;
    }
    let files = match meta.files {
        Some(list) => list
            .into_iter()
            .map(|(from, to)| {
                let path = dir.join(&from);
                if path.is_file() {
                    Ok(FileCopy {
                        from: path,
                        to: to.into(),
                    })
                } else {
                    Err(format!(
                        "[package.metadata.ponko] files: `{from}` is not a file next to Cargo.toml"
                    ))
                }
            })
            .collect::<Result<Vec<_>, String>>()?,
        // The license files beside Cargo.toml.
        None => {
            let mut found: Vec<FileCopy> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let upper = n.to_ascii_uppercase();
                    ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"]
                        .iter()
                        .any(|p| upper.starts_with(p))
                        .then(|| FileCopy {
                            from: e.path(),
                            to: n.into(),
                        })
                })
                .collect();
            found.sort_by(|a, b| a.to.cmp(&b.to));
            found
        }
    };
    let icon = match &meta.icon {
        Some(icon) => {
            let path = dir.join(icon);
            let png =
                std::fs::read(&path).map_err(|e| format!("[package.metadata.ponko] icon: can't read `{icon}`: {e}"))?;
            archive::check_icon(&png).map_err(|e| format!("[package.metadata.ponko] icon `{icon}`: {e}"))?;
            Some((path, png))
        }
        None => None,
    };
    let game = Game {
        name,
        slug,
        id,
        version: p["version"].as_str().unwrap_or("0.0.0").to_string(),
        target: target.to_string(),
        icon: icon.as_ref().map(|i| i.0.clone()),
        assets,
        files,
    };
    Ok((game, icon.map(|i| i.1), meta.pack.unwrap_or_default()))
}

/// The asset pack of the game's files under `assets/`, if it has any, and
/// lines for the report; cooked with `cook`.
fn pack_assets(
    game: &Game,
    dir: &Path,
    meta: &PackMeta,
    cook: Option<crate::cook::Settings>,
) -> Result<Option<(Vec<u8>, String)>, String> {
    let files = files_under_assets(game);
    if files.is_empty() {
        return Ok(None);
    }
    let settings = crate::pack::Settings {
        exclude: &meta.exclude,
        textures: meta.textures,
        compress: true,
        cook,
    };
    let (packed, converted, cooked) = crate::pack::pack_files(&files, &dir.join("assets"), &settings)?;
    let mut line = format!(
        "assets: {} files ({}) in {} ({}, {} deflated)",
        packed.files,
        crate::pack::size(packed.unpacked),
        crate::pack::DEFAULT_PACK,
        crate::pack::size(packed.bytes.len() as u64),
        packed.deflated
    );
    if !converted.is_empty() {
        line.push_str(&format!(", {} textures converted to KTX2", converted.len()));
    }
    match cooked {
        Some(summary) => line.push_str(&format!("\nassets {summary}")),
        None => line.push_str(", not cooked"),
    }
    Ok(Some((packed.bytes, line)))
}

/// The game's files under `assets/`: (name inside it, `/`-separated; file).
fn files_under_assets(game: &Game) -> Vec<(String, PathBuf)> {
    game.assets
        .iter()
        .filter_map(|a| {
            let inside = a.to.strip_prefix("assets").ok()?;
            let name = inside
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            Some((name, a.from.clone()))
        })
        .collect()
}

/// Cooks the game's files under `assets/` into `out` (emptied first) and
/// points the game's asset list at the cooked files, for a package of
/// loose files (`--no-pack`, `pack = false`): the same names, the same
/// cooking as in a pack. Returns the line for the report.
fn cook_loose(game: &mut Game, out: &Path, cook: &crate::cook::Settings) -> Result<Option<String>, String> {
    let files = files_under_assets(game);
    if files.is_empty() {
        return Ok(None);
    }
    let read: Vec<(String, Vec<u8>)> = files
        .into_iter()
        .map(|(name, path)| {
            std::fs::read(&path)
                .map(|bytes| (name, bytes))
                .map_err(|e| format!("can't read {}: {e}", path.display()))
        })
        .collect::<Result<_, _>>()?;
    let config = crate::cook::config_of(&read)?;
    let cooked = crate::cook::cook_files(read, &config, cook)?;
    crate::cook::write_folder(out, &cooked.files)?;
    game.assets.retain(|a| !a.to.starts_with("assets"));
    game.assets.extend(cooked.files.iter().map(|(name, _)| FileCopy {
        from: out.join(name),
        to: Path::new("assets").join(name),
    }));
    game.assets.sort_by(|a, b| a.to.cmp(&b.to));
    Ok(Some(format!("assets: loose files, {}", cooked.summary())))
}

/// `--features` and `--no-default-features`, for build and tree alike.
fn feature_args(options: &Options) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(f) = &options.features {
        out.extend(["--features".to_string(), f.clone()]);
    }
    if options.no_default_features {
        out.push("--no-default-features".into());
    }
    out
}

/// Builds the program; returns the executable (or the .wasm for the web)
/// and the features the engine (the `ponko` library) was built with, when
/// cargo said (not with `--no-build`).
fn build(
    options: &Options,
    project: &Project,
    target: &str,
    example: bool,
    triple: &str,
    explicit: bool,
) -> Result<(PathBuf, Option<Vec<String>>), String> {
    let profile = options.profile.as_deref().unwrap_or("release");
    let package = project.package["name"].as_str().unwrap_or_default();
    let kind = if example { "example" } else { "bin" };
    if options.no_build {
        let mut dir = project.target_dir.clone();
        if explicit {
            dir.push(triple);
        }
        dir.push(match profile {
            "dev" | "test" => "debug",
            "bench" => "release",
            other => other,
        });
        if example {
            dir.push("examples");
        }
        let exe = dir.join(Platform::from_triple(triple)?.exe_name(target));
        return if exe.is_file() {
            Ok((exe, None))
        } else {
            Err(format!(
                "--no-build: {} does not exist; build the {kind} first",
                exe.display()
            ))
        };
    }
    let threads = if options.threads { ", with threads" } else { "" };
    println!("ponko package: building {kind} `{target}` ({profile}, {triple}{threads})");
    let mut cmd = if options.threads {
        // rustup's `cargo`, which picks the toolchain from RUSTUP_TOOLCHAIN
        // (`$CARGO` is the current toolchain's own); its rustc, not ours.
        let mut cmd = Command::new("cargo");
        cmd.env_remove("RUSTC")
            .envs(web_threads_env(|k| std::env::var(k).ok()))
            .arg("-Zbuild-std=panic_abort,std");
        cmd
    } else {
        Command::new(cargo())
    };
    cmd.args([
        "build",
        "--message-format",
        "json-render-diagnostics",
        "--profile",
        profile,
    ])
    .arg("--manifest-path")
    .arg(&project.manifest)
    .args(["-p", package, &format!("--{kind}"), target])
    .args(feature_args(options));
    if options.locked {
        cmd.arg("--locked");
    }
    if let Some(t) = options.target.as_deref().or(options.web.then_some(WASM)) {
        cmd.args(["--target", t]);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("can't run cargo build: {e}"))?;
    let mut found = None;
    let mut engine = None;
    let reader = BufReader::new(child.stdout.take().ok_or("cargo build has no output")?);
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(features) = engine_features(&msg) {
            engine = Some(features);
        }
        let ours = msg["reason"] == "compiler-artifact"
            && msg["target"]["name"] == target
            && msg["target"]["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == kind));
        if !ours {
            continue;
        }
        if let Some(exe) = msg["executable"].as_str() {
            found = Some(PathBuf::from(exe));
        } else if let Some(wasm) = msg["filenames"]
            .as_array()
            .and_then(|f| f.iter().filter_map(Value::as_str).find(|f| f.ends_with(".wasm")))
        {
            found = Some(PathBuf::from(wasm));
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("cargo build failed ({status})"));
    }
    let found = found.ok_or_else(|| format!("cargo build made no program for `{target}`"))?;
    Ok((found, engine))
}

/// The features of the engine library, from a cargo build message about it.
fn engine_features(msg: &Value) -> Option<Vec<String>> {
    let engine = msg["reason"] == "compiler-artifact"
        && msg["target"]["name"] == "ponko"
        && msg["target"]["kind"]
            .as_array()
            .is_some_and(|k| k.iter().any(|k| k == "lib"));
    engine.then(|| {
        msg["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect()
    })
}

/// How `ponko package` cooks a game's assets for `platform`, given the
/// engine features it was built with (`None`: not known, so textures stay
/// as they are).
fn cook_settings(platform: &Platform, engine: Option<&[String]>, target_dir: &Path) -> crate::cook::Settings {
    crate::cook::Settings {
        platform: if platform.os == Os::Web {
            crate::cook::Platform::Web
        } else {
            crate::cook::Platform::Desktop
        },
        reads: crate::cook::Reads::from_features(engine.unwrap_or_default()),
        cache: Some(target_dir.join("cook-cache")),
    }
}

/// The third-party notices for what the program links on `triple`.
fn third_party_notices(
    options: &Options,
    project: &Project,
    triple: &str,
    title: &str,
    edges: &str,
    leave_out_root: bool,
) -> Result<(String, usize, Vec<String>), String> {
    let package = project.package["name"].as_str().unwrap_or_default();
    let mut cmd = Command::new(cargo());
    cmd.args([
        "tree", "-p", package, "-e", edges, "--target", triple, "--prefix", "none", "-f", "{p}",
    ])
    .arg("--manifest-path")
    .arg(&project.manifest)
    .args(feature_args(options));
    if options.locked {
        cmd.arg("--locked");
    }
    let mut linked = notices::tree_crates(&output(cmd, "cargo tree")?);
    if leave_out_root {
        // The game itself is not a third party.
        let version = project.package["version"].as_str().unwrap_or_default();
        linked.retain(|(n, v)| !(n == package && v == version));
    }
    let mut cmd = Command::new(cargo());
    cmd.args(["metadata", "--format-version", "1", "--filter-platform", triple])
        .arg("--manifest-path")
        .arg(&project.manifest);
    let crates = notices::crates(&json(&output(cmd, "cargo metadata")?, "cargo metadata")?)?;
    notices::render(title, triple, &linked, &crates, &project.workspace_root)
}

/// `ponko notices`: the third-party notices of a library.
pub fn notices_command(options: &Options, cwd: &Path) -> Result<String, String> {
    let out = options
        .out
        .as_ref()
        .ok_or_else(|| format!("give --out <file>\n\n{NOTICES_USAGE}"))?;
    let project = project(options, cwd)?;
    let triple = match &options.target {
        Some(t) => t.clone(),
        None => host_triple()?,
    };
    let package = project.package["name"].as_str().unwrap_or_default().to_string();
    let title = options.title.clone().unwrap_or(package);
    let (text, count, without) = third_party_notices(options, &project, &triple, &title, "normal", false)?;
    let out = cwd.join(out);
    std::fs::write(&out, text).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    let mut msg = format!("wrote {} ({count} libraries)", out.display());
    if !without.is_empty() {
        msg.push_str(&format!(
            "\nnote: {} libraries ship no license file; their declared license is listed",
            without.len()
        ));
    }
    Ok(msg)
}

/// The wasm-bindgen version the project's Cargo.lock names.
fn locked_wasm_bindgen(workspace_root: &Path) -> Option<String> {
    let lock = std::fs::read_to_string(workspace_root.join("Cargo.lock")).ok()?;
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == "name = \"wasm-bindgen\"" {
            return lines
                .next()?
                .strip_prefix("version = \"")?
                .strip_suffix('"')
                .map(str::to_string);
        }
    }
    None
}

/// Runs wasm-bindgen on the build into `dir` (as `web/build.sh` does).
fn bindgen(wasm: &Path, dir: &Path, workspace_root: &Path) -> Result<(), String> {
    let locked = locked_wasm_bindgen(workspace_root);
    let install = format!(
        "cargo install wasm-bindgen-cli --version {}",
        locked.as_deref().unwrap_or("<the wasm-bindgen version in Cargo.lock>")
    );
    let version = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|_| format!("the web build needs wasm-bindgen: {install}"))?;
    let have = String::from_utf8_lossy(&version.stdout);
    let have = have.split_whitespace().nth(1).unwrap_or_default();
    if let Some(locked) = &locked
        && have != locked
    {
        return Err(format!(
            "wasm-bindgen {have} is installed, but the game uses {locked}: {install}"
        ));
    }
    let _ = std::fs::remove_dir_all(dir);
    let status = Command::new("wasm-bindgen")
        .args([
            "--target",
            "web",
            "--no-typescript",
            "--remove-name-section",
            "--remove-producers-section",
        ])
        .arg("--out-dir")
        .arg(dir)
        .args(["--out-name", "game"])
        .arg(wasm)
        .status()
        .map_err(|e| format!("can't run wasm-bindgen: {e}"))?;
    if !status.success() {
        return Err(format!("wasm-bindgen failed ({status})"));
    }
    Ok(())
}

fn write_plan(out: &Path, plan: &layout::Plan) -> Result<(), String> {
    for e in &plan.entries {
        let path = out.join(&e.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        let done = match &e.source {
            Source::Copy(from) => std::fs::copy(from, &path)
                .map(drop)
                .map_err(|err| format!("can't copy {} to {}: {err}", from.display(), path.display())),
            Source::Text(text) => std::fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display())),
            Source::Bytes(bytes) => std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display())),
        };
        done?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if e.executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .map_err(|err| format!("{}: {err}", path.display()))?;
        }
    }
    Ok(())
}

/// Runs the signing hook: `<program> <stage> <path>`, with the package's
/// details in `PONKO_PACKAGE_*` variables.
fn sign_hook(
    program: &Path,
    stage: &str,
    path: &Path,
    game: &Game,
    platform: &Platform,
    plan_dir: &Path,
) -> Result<(), String> {
    println!("ponko package: {} {stage} {}", program.display(), path.display());
    let status = Command::new(program)
        .arg(stage)
        .arg(path)
        .env("PONKO_PACKAGE_STAGE", stage)
        .env("PONKO_PACKAGE_PATH", path)
        .env("PONKO_PACKAGE_DIR", plan_dir)
        .env("PONKO_PACKAGE_PLATFORM", platform.label())
        .env("PONKO_PACKAGE_TARGET", &platform.triple)
        .env("PONKO_PACKAGE_NAME", &game.name)
        .env("PONKO_PACKAGE_ID", &game.id)
        .env("PONKO_PACKAGE_VERSION", &game.version)
        .status()
        .map_err(|e| format!("can't run the signing program {}: {e}", program.display()))?;
    if !status.success() {
        return Err(format!("the signing program failed at `{stage}` ({status})"));
    }
    Ok(())
}

/// Seals a macOS app ad hoc: Gatekeeper still asks the player to allow it,
/// but the bundle is consistent (an unsealed bundle reads as "damaged").
fn seal_ad_hoc(app: &Path) -> Result<(), String> {
    for args in [
        &["--force", "--deep", "--sign", "-"][..],
        &["--verify", "--deep", "--strict"],
    ] {
        let status = Command::new("codesign")
            .args(args)
            .arg(app)
            .status()
            .map_err(|e| format!("can't run codesign: {e}"))?;
        if !status.success() {
            return Err(format!("codesign {} failed ({status})", args.join(" ")));
        }
    }
    Ok(())
}

fn have(program: &str) -> bool {
    Command::new(program)
        .arg("-h")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// A disk image of the package folder, with a link to /Applications to drag the app to.
fn dmg(out: &Path, plan: &layout::Plan, game: &Game, file: &Path) -> Result<(), String> {
    let stage = out.join(format!(".{}-dmg", plan.folder.display()));
    let _ = std::fs::remove_dir_all(&stage);
    copy_tree(&out.join(&plan.folder), &stage)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", stage.join("Applications")).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(file);
    let status = Command::new("hdiutil")
        .args([
            "create",
            "-quiet",
            "-volname",
            &game.name,
            "-fs",
            "HFS+",
            "-format",
            "UDZO",
            "-ov",
            "-srcfolder",
        ])
        .arg(&stage)
        .arg(file)
        .status()
        .map_err(|e| format!("can't run hdiutil: {e}"));
    let _ = std::fs::remove_dir_all(&stage);
    let status = status?;
    if !status.success() {
        return Err(format!("hdiutil create failed ({status})"));
    }
    Ok(())
}

/// Copies a folder (files keep their permissions).
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("can't copy {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// `ponko package`.
pub fn package_command(options: &Options, cwd: &Path) -> Result<String, String> {
    let project = project(options, cwd)?;
    let (target, example) = program(options, &project.package)?;
    let host = Platform::from_triple(&host_triple()?)?;
    let env_target = std::env::var("CARGO_BUILD_TARGET").ok().filter(|t| !t.is_empty());
    let (triple, explicit) = match (&options.target, options.web, env_target) {
        (Some(t), _, _) => (t.clone(), true),
        (None, true, _) => (WASM.to_string(), true),
        (None, false, Some(t)) => (t, true),
        (None, false, None) => (host.triple.clone(), false),
    };
    let platform = Platform::from_triple(&triple)?;
    let can_dmg = host.os == Os::Mac && have("hdiutil");
    let formats = options
        .formats
        .clone()
        .unwrap_or_else(|| Format::defaults(platform.os, can_dmg));
    let (app, archives) = layout::check_formats(&platform, &formats)?;
    if archives.contains(&Format::Dmg) && !can_dmg {
        return Err("a dmg needs hdiutil, so a Mac: use --format zip".into());
    }
    let mut notes = Vec::new();
    let (mut game, icon, pack_meta) = game(&project, &target, example, &mut notes)?;
    let out = cwd.join(
        options
            .out
            .clone()
            .unwrap_or_else(|| project.target_dir.join("package")),
    );

    let (built, engine) = build(options, &project, &target, example, &triple, explicit)?;
    // Packed or cooked after the build: the cook asks what the engine was
    // built to read.
    let cook =
        (pack_meta.cook && !options.no_cook).then(|| cook_settings(&platform, engine.as_deref(), &project.target_dir));
    if cook.is_some() && engine.is_none() {
        notes.push("textures are kept as they are: with --no-build the engine's features aren't known".into());
    }
    let mut loose_line = None;
    let pack = if pack_meta.enabled && !options.no_pack {
        let dir = project.manifest.parent().unwrap_or(Path::new("."));
        pack_assets(&game, dir, &pack_meta, cook)?
    } else {
        if let Some(cook) = &cook {
            let folder = format!("{}-{}", game.slug, platform.label());
            let cooked = project.target_dir.join("package-cooked").join(folder);
            loose_line = cook_loose(&mut game, &cooked, cook)?;
        }
        None
    };
    // An example also links its package's dev-dependencies; a binary is the game itself.
    let edges = if example { "normal,dev" } else { "normal" };
    let (notices, count, without) = third_party_notices(options, &project, &triple, &game.name, edges, !example)?;
    if !without.is_empty() {
        notes.push(format!(
            "{} libraries ship no license file in their source; their declared license and authors are listed",
            without.len()
        ));
    }

    let folder = layout::folder_name(&game, &platform);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let _ = std::fs::remove_dir_all(out.join(&folder));
    let web_dir = out.join(format!(".{folder}-wasm-bindgen"));
    let plan = if platform.os == Os::Web {
        bindgen(&built, &web_dir, &project.workspace_root)?;
        layout::plan(
            &game,
            &platform,
            false,
            Built::Web {
                dir: &web_dir,
                threads: options.threads,
            },
            &notices,
            icon.as_deref(),
            pack.as_ref().map(|p| p.0.as_slice()),
        )
    } else {
        layout::plan(
            &game,
            &platform,
            app,
            Built::Program(&built),
            &notices,
            icon.as_deref(),
            pack.as_ref().map(|p| p.0.as_slice()),
        )
    };
    let plan = plan.and_then(|plan| write_plan(&out, &plan).map(|()| plan));
    let _ = std::fs::remove_dir_all(&web_dir);
    let plan = plan?;
    let dir = out.join(&plan.folder);
    let mut report = vec![format!(
        "packaged {} for {} ({count} libraries in the notices)",
        dir.display(),
        platform.triple
    )];
    match &pack {
        Some((_, line)) => report.push(line.clone()),
        None if loose_line.is_some() => report.extend(loose_line.clone()),
        None if !game.assets.is_empty() => report.push("assets: loose files, not cooked".into()),
        None => {}
    }

    // Signing: the hook, else an ad-hoc seal for a macOS app made on a Mac.
    let signed_path = out.join(plan.app.as_ref().unwrap_or(&plan.program));
    match (&options.sign, &plan.app) {
        (Some(program), _) if platform.os != Os::Web => {
            sign_hook(program, "sign", &signed_path, &game, &platform, &dir)?;
        }
        (None, Some(app)) if host.os == Os::Mac => {
            seal_ad_hoc(&out.join(app))?;
            notes.push(
                "the app is sealed ad hoc, not signed: players are asked to allow it (see the guide's signing hook)"
                    .into(),
            );
        }
        (None, Some(_)) => notes.push("the app is not sealed: codesign runs only on a Mac".into()),
        _ => {}
    }

    // The smoke run, where the package can run here.
    let runs_here = platform.os == host.os && platform.arch == host.arch && platform.os != Os::Web;
    if options.no_smoke {
        report.push("smoke run: skipped (--no-smoke)".into());
    } else if !runs_here {
        report.push(format!(
            "smoke run: skipped, a {} package does not run on {}",
            platform.label(),
            host.triple
        ));
    } else {
        let ticks = options.smoke_ticks.unwrap_or(120);
        let timeout = Duration::from_secs(options.smoke_timeout.unwrap_or(120));
        println!("ponko package: smoke run, {ticks} ticks headless, from a copy elsewhere");
        report.push(format!("smoke run: {}", smoke::run(&out, &plan, ticks, timeout)?));
    }

    // The archives.
    let executables: BTreeSet<PathBuf> = plan
        .entries
        .iter()
        .filter(|e| e.executable)
        .map(|e| e.path.clone())
        .collect();
    for format in archives {
        let file = out.join(format!("{folder}.{}", format.extension()));
        match format {
            Format::Zip => archive::zip(&out, &plan.folder, &executables, &file)?,
            Format::TarGz => archive::tar_gz(&out, &plan.folder, &executables, &file)?,
            Format::Dmg => dmg(&out, &plan, &game, &file)?,
            Format::Folder => continue,
        }
        if let Some(program) = &options.sign {
            sign_hook(program, "archive", &file, &game, &platform, &dir)?;
        }
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        report.push(format!("archive: {} ({:.1} MB)", file.display(), size as f64 / 1e6));
    }
    if platform.os == Os::Web && options.threads {
        let headers: Vec<String> = layout::ISOLATION_HEADERS
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        report.push(format!(
            "threads: the workers start only where the page comes with the headers `{}`; \
             _headers, vercel.json and .htaccess in the folder send them on Netlify, Cloudflare Pages, \
             Vercel and Apache; elsewhere set them yourself (see the guide's \"Threads on the web\"). \
             Without them the game runs on one thread",
            headers.join("` and `")
        ));
    } else if platform.os == Os::Web {
        report.push(format!(
            "serve it with any static web server, e.g. `python3 -m http.server -d \"{}\" 8000`",
            dir.display()
        ));
    }
    report.extend(notes.into_iter().map(|n| format!("note: {n}")));
    Ok(report.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), USAGE)
    }

    #[test]
    fn loose_assets_are_cooked_under_their_names() {
        let dir = std::env::temp_dir().join(format!("ponko-loose-cook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("game/assets/sounds")).unwrap();
        let tone = crate::cook::flac::tests::tone(2_000, 1, 16);
        let wav = crate::cook::flac::tests::wav(22_050, 1, 16, &tone);
        std::fs::write(dir.join("game/assets/sounds/jump.wav"), &wav).unwrap();
        std::fs::write(dir.join("game/assets/rules.json"), b"{}").unwrap();
        std::fs::write(
            dir.join("game/assets/cook.json"),
            br#"{"assets": {"rules.json": "skip"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("game/LICENSE"), b"MIT").unwrap();
        let copy = |rel: &str| FileCopy {
            from: dir.join("game").join(rel),
            to: rel.into(),
        };
        let mut game = Game {
            name: "Loose".into(),
            slug: "loose".into(),
            id: "local.loose".into(),
            version: "0.1.0".into(),
            target: "loose".into(),
            icon: None,
            assets: vec![
                copy("assets/cook.json"),
                copy("assets/rules.json"),
                copy("assets/sounds/jump.wav"),
                copy("LICENSE"),
            ],
            files: Vec::new(),
        };
        let settings = crate::cook::Settings {
            platform: crate::cook::Platform::Desktop,
            reads: crate::cook::Reads::ALL,
            cache: None,
        };
        let out = dir.join("cooked");
        let line = cook_loose(&mut game, &out, &settings).unwrap().unwrap();
        assert!(line.starts_with("assets: loose files, cooked 1 files"), "{line}");
        let to: Vec<String> = game
            .assets
            .iter()
            .map(|a| a.to.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            to,
            ["LICENSE", "assets/sounds/jump.wav"],
            "skipped and settings files left out"
        );
        let jump = game.assets.iter().find(|a| a.to.ends_with("jump.wav")).unwrap();
        assert!(jump.from.starts_with(&out));
        assert!(std::fs::read(&jump.from).unwrap().starts_with(b"fLaC"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn options_parse_flags_and_values() {
        let o = parse(&[
            "--example",
            "coin_dash",
            "--profile=dev",
            "--format",
            "folder",
            "--out",
            "x",
            "--no-smoke",
            "--no-build",
            "--no-pack",
        ])
        .unwrap();
        assert_eq!(o.example.as_deref(), Some("coin_dash"));
        assert_eq!(o.profile.as_deref(), Some("dev"));
        assert_eq!(o.formats, Some(vec![Format::Folder]));
        assert_eq!(o.out, Some(PathBuf::from("x")));
        assert!(o.no_smoke && o.no_build && o.no_pack);
        assert_eq!(parse(&["--smoke-ticks", "0"]).unwrap().smoke_ticks, Some(1));
    }

    #[test]
    fn bad_options_are_refused_with_a_reason() {
        for (args, needle) in [
            (&["--bin", "a", "--example", "b"][..], "not both"),
            (&["--web", "--target", "x86_64-pc-windows-msvc"], "--web builds for"),
            (&["--threads"], "--threads is for web builds"),
            (&["--out"], "--out needs a value"),
            (&["--smoke-ticks", "many"], "takes a number"),
            (&["--zip"], "unknown argument `--zip`"),
        ] {
            let err = parse(args).unwrap_err();
            assert!(err.contains(needle), "{args:?}: {err}");
        }
    }

    #[test]
    fn web_threads_build_with_nightly_unless_the_variable_names_another() {
        assert!(parse(&["--web", "--threads"]).unwrap().threads);
        assert_eq!(web_threads_toolchain(|_| None), "nightly");
        let dated = |k: &str| (k == "PONKO_WEB_TOOLCHAIN").then(|| "nightly-2026-11-01".to_string());
        assert_eq!(web_threads_toolchain(dated), "nightly-2026-11-01");
        assert_eq!(
            web_threads_env(dated)[0],
            ("RUSTUP_TOOLCHAIN".into(), "nightly-2026-11-01".into())
        );
    }

    #[test]
    fn web_threads_flags_go_where_cargo_reads_them_and_keep_the_users() {
        let target = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
        let flags = web_thread_flags().join(" ");
        // Nothing set: the target's flags, which only the wasm build reads.
        assert_eq!(web_threads_env(|_| None)[1], (target.into(), flags.clone()));
        // The user's own flags stay, after ours, in the variable cargo reads.
        let cases = [
            (target, "--cfg x", target, format!("{flags} --cfg x")),
            ("RUSTFLAGS", "--cfg x", "RUSTFLAGS", format!("{flags} --cfg x")),
            (
                "CARGO_ENCODED_RUSTFLAGS",
                "--cfg\x1fx",
                "CARGO_ENCODED_RUSTFLAGS",
                format!("{}\x1f--cfg\x1fx", web_thread_flags().join("\x1f")),
            ),
        ];
        for (set, value, want_name, want) in cases {
            let env = |k: &str| (k == set).then(|| value.to_string());
            assert_eq!(web_threads_env(env)[1], (want_name.to_string(), want), "{set}");
        }
        assert!(flags.contains("+atomics") && flags.contains("--shared-memory") && flags.contains("--import-memory"));
    }

    #[test]
    fn the_package_builds_threads_with_the_flags_of_the_build_script() {
        // web/build.sh --threads and `ponko package --web --threads` must agree.
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/build.sh");
        if let Ok(text) = std::fs::read_to_string(script) {
            for option in WEB_THREAD_CODEGEN {
                let flag = format!("-C {option}");
                assert!(text.contains(&flag), "web/build.sh lacks `{flag}`");
            }
            assert!(text.contains("-Zbuild-std=panic_abort,std"));
        }
    }

    #[test]
    fn the_locked_wasm_bindgen_version_is_read_from_cargo_lock() {
        let dir = std::env::temp_dir().join(format!("ponko-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.lock"),
            "[[package]]\nname = \"wasm-bindgen-shared\"\nversion = \"9\"\n\n[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.130\"\n",
        )
        .unwrap();
        assert_eq!(locked_wasm_bindgen(&dir).as_deref(), Some("0.2.130"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
