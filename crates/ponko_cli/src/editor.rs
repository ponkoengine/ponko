//! `ponko editor`: opens a game in edit mode. The editor is the game's own
//! binary, built with the engine's `editor` feature and started with
//! `PONKO_EDITOR` set: only the game's build knows its components, systems,
//! assets and rendering, so the editor runs inside it rather than attaching
//! to it from outside. This command is the `cargo run` that does that.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const USAGE: &str = "\
ponko editor: open a game in the editor

Usage: ponko editor [<project> | --last] [--open <file>] [--bin <name> | --example <name>]
                    [-p <package>] [--features <list>] [--release] [--print]
                    [-- <game arguments>]
       ponko editor --recent

Builds the game with the engine's `editor` feature and runs it in edit mode
(PONKO_EDITOR): its setup runs, time is paused, and its window becomes the
editor (hierarchy, inspector, viewport with gizmos, undo, save). The game's
dependency on the engine must be named `ponko`; inside the engine's own
repository its examples are run with `--features editor`.

Arguments:
  <project>                  The game's folder (default: the current one)

Options:
  --open <file>              A scene or prefab file in assets/ to open
                             (default: the world the game's setup makes)
  --bin <name>               Run this binary of the package
  --example <name>           Run this example instead
  -p, --package <name>       The package to run, in a workspace
  --features <list>          More Cargo features to build with
  --release                  An optimized build
  --print                    Print the command instead of running it
  --last                     The project opened last, at the file opened last
  --recent                   List the recent projects and their files

The editor remembers its layout, recent projects and files, and rebound
shortcuts in editor.json: in $PONKO_CONFIG_DIR if set, else %APPDATA%\\ponko
(Windows), ~/Library/Application Support/ponko (macOS), or
$XDG_CONFIG_HOME/ponko or ~/.config/ponko.

Guide: https://ponko.dev/guide/editor.html";

/// What `ponko editor` was asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Options {
    /// The game's folder.
    pub project: Option<PathBuf>,
    /// The scene or prefab to open, relative to the game's assets.
    pub open: Option<String>,
    /// Cargo's target selection and the like, passed to `cargo run`.
    pub cargo_args: Vec<String>,
    /// Features asked for with `--features`, joined to the editor's.
    pub features: Vec<String>,
    pub print: bool,
    /// Open the project (and file) opened last.
    pub last: bool,
    /// List the recent projects instead of running.
    pub recent: bool,
    /// Arguments after `--`, for the game.
    pub game_args: Vec<String>,
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
                "--" => {
                    o.game_args = it.by_ref().cloned().collect();
                }
                "--open" => o.open = Some(value(arg)?),
                "--features" | "-F" => o.features.push(value(arg)?),
                "--bin" | "--example" | "-p" | "--package" => {
                    let v = value(arg)?;
                    o.cargo_args.push(arg.clone());
                    o.cargo_args.push(v);
                }
                "--release" => o.cargo_args.push(arg.clone()),
                "--print" => o.print = true,
                "--last" => o.last = true,
                "--recent" => o.recent = true,
                other if other.starts_with('-') => {
                    return Err(format!("unknown option `{other}`\n\n{USAGE}"));
                }
                path if o.project.is_none() => o.project = Some(PathBuf::from(path)),
                extra => {
                    return Err(format!(
                        "one project only (`{extra}` too); open a file with `--open <file>`\n\n{USAGE}"
                    ));
                }
            }
        }
        if o.last && o.project.is_some() {
            return Err(format!("--last or a project, not both\n\n{USAGE}"));
        }
        Ok(o)
    }
}

/// The editor's settings file (the engine's `inspector_settings.rs` writes
/// it; the CLI only reads its recent projects).
pub(crate) fn settings_path() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if let Some(dir) = var("PONKO_CONFIG_DIR") {
        Some(dir)
    } else if cfg!(windows) {
        var("APPDATA").map(|d| d.join("ponko"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support/ponko"))
    } else {
        var("XDG_CONFIG_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".config")))
            .map(|d| d.join("ponko"))
    };
    dir.map(|d| d.join("editor.json"))
}

/// A recent project: its folder and its recent files, most recent first.
#[derive(Debug, PartialEq)]
pub(crate) struct Recent {
    pub path: String,
    pub files: Vec<String>,
}

/// The recent projects in settings file `path`, most recent first; none if
/// it doesn't exist.
pub(crate) fn recent(path: Option<&Path>) -> Result<Vec<Recent>, String> {
    let Some(path) = path else { return Ok(Vec::new()) };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let file: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let list = file.get("projects").and_then(|p| p.as_array());
    Ok(list
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Recent {
                path: p.get("path")?.as_str()?.to_string(),
                files: p
                    .get("files")
                    .and_then(|f| f.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str().map(String::from))
                    .collect(),
            })
        })
        .collect())
}

/// `--recent`'s list.
fn list_recent(projects: &[Recent]) -> String {
    if projects.is_empty() {
        return "no recent projects: `ponko editor <project>` opens one".into();
    }
    let mut out = String::new();
    for p in projects {
        out.push_str(&p.path);
        out.push('\n');
        for f in &p.files {
            out.push_str(&format!("    {f}\n"));
        }
    }
    out.trim_end().to_string()
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Whether the manifest is the engine's own: its `[package]` is named
/// `ponko`, so the feature is `editor` rather than `ponko/editor`.
fn is_the_engine(manifest: &str) -> bool {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            return value.trim().trim_matches('"') == "ponko";
        }
    }
    false
}

/// The `cargo run` that opens the game in the editor; `manifest` is the
/// text of the project's Cargo.toml.
pub(crate) fn command(options: &Options, project: &Path, manifest: &str) -> Command {
    let editor = if is_the_engine(manifest) {
        "editor"
    } else {
        "ponko/editor"
    };
    let features = std::iter::once(editor.to_string())
        .chain(options.features.iter().cloned())
        .collect::<Vec<_>>()
        .join(",");
    let mut cmd = Command::new(cargo());
    cmd.current_dir(project)
        .arg("run")
        .args(&options.cargo_args)
        .args(["--features", &features])
        .env("PONKO_EDITOR", options.open.as_deref().unwrap_or("1"))
        // An editor run never quits by itself or takes screenshots.
        .env_remove("PONKO_SMOKE")
        .env_remove("PONKO_SHOT");
    if !options.game_args.is_empty() {
        cmd.arg("--").args(&options.game_args);
    }
    cmd
}

/// The command as a shell line, for `--print`.
fn describe(cmd: &Command) -> String {
    let mut line = String::new();
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            line.push_str(&format!("{}={} ", key.to_string_lossy(), value.to_string_lossy()));
        }
    }
    line.push_str(&cmd.get_program().to_string_lossy());
    for arg in cmd.get_args() {
        line.push(' ');
        line.push_str(&arg.to_string_lossy());
    }
    if let Some(dir) = cmd.get_current_dir() {
        line = format!("(cd {} && {line})", dir.display());
    }
    line
}

/// Runs `ponko editor`; returns what to print.
/// `settings` is the editor's settings file (see [`settings_path`]).
pub(crate) fn editor_command(options: &Options, cwd: &Path, settings: Option<&Path>) -> Result<String, String> {
    if options.recent {
        return Ok(list_recent(&recent(settings)?));
    }
    let mut options = options.clone();
    let project = match &options.project {
        Some(p) => cwd.join(p),
        None if options.last => {
            let projects = recent(settings)?;
            let last = projects
                .first()
                .ok_or("no recent projects: `ponko editor <project>` opens one")?;
            if options.open.is_none() {
                options.open = last.files.first().cloned();
            }
            PathBuf::from(&last.path)
        }
        None => cwd.to_path_buf(),
    };
    let manifest_path = project.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|e| {
        format!(
            "{} has no Cargo.toml ({e}): `ponko editor` runs a Cargo project; a native game sets PONKO_EDITOR itself",
            project.display()
        )
    })?;
    if let Some(open) = &options.open
        && (Path::new(open).is_absolute() || open.split(['/', '\\']).any(|part| part == ".."))
    {
        return Err(format!(
            "--open takes a path relative to the game's assets/, not `{open}`"
        ));
    }
    let mut cmd = command(&options, &project, &manifest);
    if options.print {
        return Ok(describe(&cmd));
    }
    let status = cmd.status().map_err(|e| format!("can't run cargo: {e}"))?;
    if status.success() {
        Ok(String::new())
    } else {
        Err(format!("the editor exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    fn env(cmd: &Command, key: &str) -> Option<Option<String>> {
        cmd.get_envs()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    const GAME: &str = "[package]\nname = \"crab-yard\"\n\n[dependencies]\nponko = \"0.1\"\n";
    const ENGINE: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n[package]\nname = \"ponko\"\n";

    #[test]
    fn options_take_a_project_a_file_cargo_selection_and_game_arguments() {
        let o = parse(&[
            "games/yard",
            "--open",
            "levels/one.json",
            "--example",
            "yard",
            "--features",
            "3d",
            "--release",
            "--",
            "--fast",
        ])
        .unwrap();
        assert_eq!(o.project, Some(PathBuf::from("games/yard")));
        assert_eq!(o.open.as_deref(), Some("levels/one.json"));
        assert_eq!(o.cargo_args, ["--example", "yard", "--release"]);
        assert_eq!(o.features, ["3d"]);
        assert_eq!(o.game_args, ["--fast"]);
        assert!(parse(&["--open"]).unwrap_err().contains("--open needs a value"));
        assert!(parse(&["--window"]).unwrap_err().contains("unknown option `--window`"));
        assert!(parse(&["a", "b"]).unwrap_err().contains("one project only"));
    }

    #[test]
    fn a_game_gets_the_engines_editor_feature_and_the_file_to_open() {
        let o = parse(&["--open", "a.json", "--bin", "game", "--features", "3d", "--", "x"]).unwrap();
        let cmd = command(&o, Path::new("/tmp/game"), GAME);
        assert_eq!(
            args(&cmd),
            ["run", "--bin", "game", "--features", "ponko/editor,3d", "--", "x"]
        );
        assert_eq!(env(&cmd, "PONKO_EDITOR"), Some(Some("a.json".into())));
        assert_eq!(env(&cmd, "PONKO_SMOKE"), Some(None), "removed");
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/tmp/game")));
    }

    #[test]
    fn the_engines_own_examples_get_its_editor_feature() {
        let o = parse(&["--example", "editor"]).unwrap();
        let cmd = command(&o, Path::new("."), ENGINE);
        assert_eq!(args(&cmd), ["run", "--example", "editor", "--features", "editor"]);
        assert_eq!(
            env(&cmd, "PONKO_EDITOR"),
            Some(Some("1".into())),
            "no file: the setup's world"
        );
        assert!(!is_the_engine("[package]\nname = \"ponko_cli\"\n"));
        assert!(!is_the_engine("[dependencies]\nname = \"ponko\"\n"));
    }

    #[test]
    fn print_shows_the_command_without_running_it() {
        let dir = std::env::temp_dir().join(format!("ponko-editor-print-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), GAME).unwrap();
        let o = parse(&["--open", "levels/one.json", "--print"]).unwrap();
        let line = editor_command(&o, &dir, None);
        let missing = editor_command(&parse(&["--print", "nowhere"]).unwrap(), &dir, None);
        let escape = editor_command(&parse(&["--print", "--open", "../x.json"]).unwrap(), &dir, None);
        let _ = std::fs::remove_dir_all(&dir);
        let line = line.unwrap();
        assert!(
            line.contains("PONKO_EDITOR=levels/one.json") && line.ends_with("run --features ponko/editor)"),
            "{line}"
        );
        assert!(missing.unwrap_err().contains("no Cargo.toml"));
        assert!(escape.unwrap_err().contains("relative to the game's assets"));
    }

    /// The engine's settings tests read the same fixture.
    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/editor-settings.json")
    }

    #[test]
    fn recent_lists_the_projects_the_editor_remembered() {
        let projects = recent(Some(&fixture())).unwrap();
        assert_eq!(
            projects,
            [
                Recent {
                    path: "/home/me/games/space".into(),
                    files: vec!["levels/one.json".into(), "prefabs/ship.json".into()],
                },
                Recent {
                    path: "/home/me/games/farm".into(),
                    files: vec![],
                },
            ]
        );
        let o = parse(&["--recent"]).unwrap();
        let listed = editor_command(&o, Path::new("."), Some(&fixture())).unwrap();
        assert_eq!(
            listed,
            "/home/me/games/space\n    levels/one.json\n    prefabs/ship.json\n/home/me/games/farm"
        );
        let none = editor_command(&o, Path::new("."), Some(Path::new("/nowhere/editor.json"))).unwrap();
        assert!(none.starts_with("no recent projects"), "{none}");
    }

    #[test]
    fn last_opens_the_last_project_at_its_last_file() {
        let o = parse(&["--last", "--print"]).unwrap();
        // The fixture's project has no Cargo.toml here: the error names it.
        let err = editor_command(&o, Path::new("."), Some(&fixture())).unwrap_err();
        assert!(err.contains("/home/me/games/space"), "{err}");
        assert!(parse(&["--last", "games/a"]).unwrap_err().contains("not both"));

        let dir = std::env::temp_dir().join(format!("ponko-editor-last-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), GAME).unwrap();
        let settings = dir.join("editor.json");
        let json =
            serde_json::json!({ "projects": [{ "path": dir.display().to_string(), "files": ["levels/two.json"] }] });
        std::fs::write(&settings, json.to_string()).unwrap();
        let line = editor_command(&o, Path::new("/"), Some(&settings));
        let _ = std::fs::remove_dir_all(&dir);
        let line = line.unwrap();
        assert!(line.contains("PONKO_EDITOR=levels/two.json"), "{line}");
    }
}
