//! `ponko new`: a new game project from a template.

use crate::templates::Kind;
use std::path::{Path, PathBuf};

pub const USAGE: &str = "\
ponko new: make a new game project from a template

Usage:
  ponko new <path> [--2d | --3d | --headless | --server] [--path <engine> | --git <url>]

The project goes in the folder <path> (it must not exist, or be empty); its
name is the folder's name.

Templates:
  --2d        A 2D game: a window, input, drawing (the default)
  --3d        A 3D game: a mesh, a camera and a sun (the engine's `3d` feature)
  --headless  A simulation without a window, GPU or sound
  --server    A dedicated game server: headless, a bot client, a Dockerfile

The engine dependency (the released version from crates.io by default):
  --path <engine>  An engine checkout on this machine
  --git <url>      An engine Git repository (its default branch)

Each project has a fast dev profile, an assets/ folder, headless tests
(`cargo test`), an .mcp.json for ponko-mcp and an AGENTS.md for coding agents.";

/// Where the generated project gets the engine from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Engine {
    /// crates.io, the version the template names.
    Released,
    /// A local checkout (as given; made absolute when generating).
    Path(PathBuf),
    /// A Git repository.
    Git(String),
}

/// The parsed arguments of `ponko new`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub target: PathBuf,
    pub kind: Kind,
    pub engine: Engine,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut target = None;
        let mut kind = None;
        let mut engine = Engine::Released;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
                _ => (arg.as_str(), None),
            };
            let mut value = |name: &str| {
                inline
                    .clone()
                    .or_else(|| args.next().cloned())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| format!("{name} needs a value\n\n{USAGE}"))
            };
            let picked = match flag {
                "--2d" => Some(Kind::TwoD),
                "--3d" => Some(Kind::ThreeD),
                "--headless" => Some(Kind::Headless),
                "--server" => Some(Kind::Server),
                "--path" | "--git" => {
                    if engine != Engine::Released {
                        return Err("give --path or --git once".into());
                    }
                    let v = value(flag)?;
                    engine = if flag == "--path" {
                        Engine::Path(v.into())
                    } else {
                        Engine::Git(v)
                    };
                    None
                }
                f if f.starts_with('-') => return Err(format!("unknown option `{f}`\n\n{USAGE}")),
                _ => {
                    if target.is_some() {
                        return Err(format!("one project path only, got another: `{arg}`"));
                    }
                    target = Some(PathBuf::from(arg));
                    None
                }
            };
            if let Some(k) = picked {
                if kind.is_some_and(|old| old != k) {
                    return Err("pick one of --2d, --3d, --headless and --server".into());
                }
                kind = Some(k);
            }
        }
        Ok(Options {
            target: target.ok_or_else(|| format!("give the new project's path\n\n{USAGE}"))?,
            kind: kind.unwrap_or(Kind::TwoD),
            engine,
        })
    }
}

/// Words a package can't be called: Rust keywords (the name is also the
/// library's), and crates the project depends on or Rust provides.
const RESERVED: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "const",
    "continue",
    "crate",
    "dyn",
    "else",
    "enum",
    "extern",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "static",
    "struct",
    "super",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "abstract",
    "become",
    "box",
    "do",
    "final",
    "gen",
    "macro",
    "override",
    "priv",
    "try",
    "typeof",
    "unsized",
    "virtual",
    "yield",
    "ponko",
    "serde",
    "std",
    "core",
    "alloc",
    "test",
    "proc_macro",
];

/// Checks a package name; returns the library (crate) name for it.
pub fn crate_name(name: &str) -> Result<String, String> {
    let first = name.chars().next().ok_or("the project needs a name")?;
    if !first.is_ascii_alphabetic() {
        return Err(format!("`{name}`: a project name starts with a letter"));
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        return Err(format!(
            "`{name}`: a project name has only letters, digits, `-` and `_`, not `{c}`"
        ));
    }
    let snake = name.replace('-', "_");
    if RESERVED.contains(&snake.to_ascii_lowercase().as_str()) {
        return Err(format!(
            "`{name}` is reserved (a Rust keyword or a crate the project uses): pick another name"
        ));
    }
    Ok(snake)
}

/// A TOML basic string.
fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The template's manifest with the engine dependency pointed at `engine`
/// (the template names the released version: `ponko = { version = "..." ... }`).
pub fn set_engine(manifest: &str, engine: &Engine) -> Result<String, String> {
    let source = match engine {
        Engine::Released => return Ok(manifest.to_string()),
        Engine::Path(p) => format!("path = {}", toml_string(&p.to_string_lossy())),
        Engine::Git(url) => format!("git = {}", toml_string(url)),
    };
    let mut found = false;
    let mut out = String::with_capacity(manifest.len() + 64);
    for line in manifest.split_inclusive('\n') {
        if line.starts_with("ponko = {") && !found {
            let start = line
                .find("version = \"")
                .ok_or("the template's ponko dependency has no version")?;
            let end = line[start + 11..]
                .find('"')
                .ok_or("the template's ponko version is not closed")?
                + start
                + 12;
            out.push_str(&line[..start]);
            out.push_str(&source);
            out.push_str(&line[end..]);
            found = true;
        } else {
            out.push_str(line);
        }
    }
    if !found {
        return Err("the template has no `ponko = { ... }` dependency line".into());
    }
    Ok(out)
}

/// A template file's text with the placeholders filled in.
pub fn fill(text: &str, project: &str, krate: &str) -> String {
    text.replace("{{project-name}}", project)
        .replace("{{crate_name}}", krate)
}

/// An absolute path without Windows' `\\?\` prefix, which some tools reject.
fn absolute(path: &Path) -> Result<PathBuf, String> {
    let full = path
        .canonicalize()
        .map_err(|e| format!("engine path `{}`: {e}", path.display()))?;
    let text = full.to_string_lossy();
    Ok(match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => full,
    })
}

/// Makes the project; returns what to tell the user.
pub fn generate(options: &Options, cwd: &Path) -> Result<String, String> {
    let dir = cwd.join(&options.target);
    let project = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            format!(
                "`{}` has no folder name to name the project after",
                options.target.display()
            )
        })?
        .to_string();
    let krate = crate_name(&project)?;

    let engine = match &options.engine {
        Engine::Path(p) => {
            let p = absolute(&cwd.join(p))?;
            let manifest = std::fs::read_to_string(p.join("Cargo.toml")).unwrap_or_default();
            if !manifest.lines().any(|l| l.trim().replace(' ', "") == "name=\"ponko\"") {
                return Err(format!(
                    "`{}` is not a Ponko checkout (no Cargo.toml for `ponko`)",
                    p.display()
                ));
            }
            Engine::Path(p)
        }
        other => other.clone(),
    };

    let existed = dir.exists();
    if existed {
        let mut entries = std::fs::read_dir(&dir).map_err(|e| format!("`{}`: {e}", dir.display()))?;
        if entries.next().is_some() {
            return Err(format!("`{}` already exists and is not empty", dir.display()));
        }
    }
    let write_all = || -> Result<(), String> {
        for (name, text) in options.kind.files() {
            let mut text = fill(text, &project, &krate);
            if *name == "Cargo.toml" {
                text = set_engine(&text, &engine)?;
            }
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("can't create `{}`: {e}", parent.display()))?;
            }
            std::fs::write(&path, text).map_err(|e| format!("can't write `{}`: {e}", path.display()))?;
        }
        Ok(())
    };
    if let Err(e) = write_all() {
        // Leave nothing half made behind (but never a folder that was there).
        if !existed {
            let _ = std::fs::remove_dir_all(&dir);
        }
        return Err(e);
    }

    let kind = match options.kind {
        Kind::TwoD => "2D game",
        Kind::ThreeD => "3D game",
        Kind::Headless => "headless simulation",
        Kind::Server => "dedicated server",
    };
    let from = match &engine {
        Engine::Released => "the released engine from crates.io".to_string(),
        Engine::Path(p) => format!("the engine at {}", p.display()),
        Engine::Git(url) => format!("the engine from {url}"),
    };
    let run = match options.kind {
        Kind::Headless => "cargo run",
        Kind::Server => "cargo run      # the server; `cargo run --example client` joins it",
        Kind::TwoD | Kind::ThreeD => "cargo run      # opens a window",
    };
    Ok(format!(
        "Made the {kind} `{project}` in {}, using {from}.\n\n  cd {}\n  cargo test     # headless tests\n  {run}\n\n\
         Agents: AGENTS.md, and .mcp.json for ponko-mcp (cargo install ponko_mcp).",
        dir.display(),
        options.target.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    /// A fresh folder under the system's temp folder, removed on drop.
    struct Temp(PathBuf);
    impl Temp {
        fn new(tag: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("ponko-cli-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_default_is_a_2d_game_on_the_released_engine() {
        let o = parse(&["my_game"]).unwrap();
        assert_eq!(
            o,
            Options {
                target: "my_game".into(),
                kind: Kind::TwoD,
                engine: Engine::Released
            }
        );
    }

    #[test]
    fn flags_pick_the_template_and_the_engine_in_any_order() {
        let o = parse(&["--3d", "--path", "../engine", "game"]).unwrap();
        assert_eq!((o.kind, o.engine), (Kind::ThreeD, Engine::Path("../engine".into())));
        let o = parse(&["sim", "--git=https://example.com/ponko", "--headless"]).unwrap();
        assert_eq!(
            (o.kind, o.engine),
            (Kind::Headless, Engine::Git("https://example.com/ponko".into()))
        );
    }

    #[test]
    fn conflicting_or_missing_arguments_are_errors() {
        assert!(parse(&[]).unwrap_err().starts_with("give the new project's path"));
        assert!(parse(&["a", "b"]).unwrap_err().contains("one project path only"));
        assert!(parse(&["a", "--2d", "--3d"]).unwrap_err().contains("pick one of"));
        assert!(parse(&["a", "--path", "x", "--git", "y"]).unwrap_err().contains("once"));
        assert!(parse(&["a", "--git"]).unwrap_err().starts_with("--git needs a value"));
        assert!(
            parse(&["a", "--release"])
                .unwrap_err()
                .starts_with("unknown option `--release`")
        );
    }

    #[test]
    fn project_names_follow_cargo_rules() {
        assert_eq!(crate_name("my-game").unwrap(), "my_game");
        assert_eq!(crate_name("Game2").unwrap(), "Game2");
        assert!(crate_name("2game").unwrap_err().contains("starts with a letter"));
        assert!(crate_name("my game").unwrap_err().contains("not ` `"));
        for reserved in ["fn", "ponko", "serde", "test", "self"] {
            assert!(crate_name(reserved).unwrap_err().contains("reserved"), "{reserved}");
        }
    }

    #[test]
    fn the_engine_dependency_keeps_its_features() {
        let manifest = "[dependencies]\nponko = { version = \"0.1\", features = [\"3d\"] }\nserde = \"1\"\n";
        assert_eq!(set_engine(manifest, &Engine::Released).unwrap(), manifest);
        assert_eq!(
            set_engine(manifest, &Engine::Path(r#"C:\a "b""#.into())).unwrap(),
            "[dependencies]\nponko = { path = \"C:\\\\a \\\"b\\\"\", features = [\"3d\"] }\nserde = \"1\"\n"
        );
        assert_eq!(
            set_engine(manifest, &Engine::Git("https://example.com/p".into())).unwrap(),
            "[dependencies]\nponko = { git = \"https://example.com/p\", features = [\"3d\"] }\nserde = \"1\"\n"
        );
        assert!(set_engine("[dependencies]\n", &Engine::Git("x".into())).is_err());
    }

    #[test]
    fn every_template_manifest_can_point_at_another_engine() {
        for kind in Kind::ALL {
            let manifest = kind.files()[0].1;
            let changed = set_engine(manifest, &Engine::Git("https://example.com/p".into())).unwrap();
            assert!(
                changed.contains("ponko = { git = \"https://example.com/p\""),
                "{}",
                kind.folder()
            );
        }
    }

    #[test]
    fn generating_fills_in_the_name_and_writes_every_file() {
        let tmp = Temp::new("fill");
        for kind in Kind::ALL {
            let target = PathBuf::from(format!("my-{}", kind.folder()));
            let options = Options {
                target: target.clone(),
                kind,
                engine: Engine::Released,
            };
            let message = generate(&options, &tmp.0).unwrap();
            assert!(message.contains(&format!("`my-{}`", kind.folder())), "{message}");
            let dir = tmp.0.join(&target);
            for (name, _) in kind.files() {
                let text = std::fs::read_to_string(dir.join(name)).unwrap();
                assert!(!text.contains("{{"), "{name} still has a placeholder");
            }
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            assert!(manifest.contains(&format!("name = \"my-{}\"", kind.folder())));
            let main = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
            assert!(main.contains(&format!("my_{}::", kind.folder())), "{main}");
            assert!(!dir.join("cargo-generate.toml").exists());
        }
    }

    #[test]
    fn generating_refuses_a_folder_with_files_and_a_bad_engine_path() {
        let tmp = Temp::new("refuse");
        std::fs::create_dir_all(tmp.0.join("taken")).unwrap();
        std::fs::write(tmp.0.join("taken/keep.txt"), "mine").unwrap();
        let options = Options {
            target: "taken".into(),
            kind: Kind::TwoD,
            engine: Engine::Released,
        };
        assert!(generate(&options, &tmp.0).unwrap_err().contains("not empty"));
        assert_eq!(std::fs::read_to_string(tmp.0.join("taken/keep.txt")).unwrap(), "mine");

        let options = Options {
            target: "game".into(),
            kind: Kind::TwoD,
            engine: Engine::Path("taken".into()),
        };
        assert!(generate(&options, &tmp.0).unwrap_err().contains("not a Ponko checkout"));
        assert!(!tmp.0.join("game").exists(), "nothing made");
    }

    #[test]
    fn an_engine_path_is_written_absolute() {
        let tmp = Temp::new("path");
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let options = Options {
            target: "game".into(),
            kind: Kind::Headless,
            engine: Engine::Path(engine.clone()),
        };
        generate(&options, &tmp.0).unwrap();
        let manifest = std::fs::read_to_string(tmp.0.join("game/Cargo.toml")).unwrap();
        let wanted = toml_string(&absolute(&engine).unwrap().to_string_lossy());
        assert!(manifest.contains(&format!("ponko = {{ path = {wanted}")), "{manifest}");
    }
}
