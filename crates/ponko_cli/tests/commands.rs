//! Every command and option `ponko --help` shows: each command answers
//! `--help` and `ponko help <command>`, accepts every option its help lists,
//! and the guide's chapter on the command (`book/src/cli.md`) names each
//! one. The cheap commands also run for real in an empty folder. A command
//! or option added to a help text without its code or its documentation
//! fails here.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A folder of its own for one test, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(what: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ponko-cli-commands-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `ponko` in `dir`, with its settings folder there too (so `--last`
/// and `--recent` never read the person's own): success, stdout, stderr.
fn ponko(args: &[&str], dir: &Path) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .args(args)
        .current_dir(dir)
        .env("PONKO_CONFIG_DIR", dir.join("config"))
        .env_remove("PONKO_EDITOR")
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The commands `ponko --help` lists under "Commands:".
fn commands(usage: &str) -> Vec<String> {
    let list = &usage[usage.find("Commands:").expect("a list of commands")..];
    list.lines()
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

/// The options a help text names (`--out`, `-p`), each once, with whether
/// the help gives it a value (`--out <file>`).
fn options(help: &str) -> Vec<(String, bool)> {
    let mut found: Vec<(String, bool)> = Vec::new();
    let bytes = help.as_bytes();
    for (i, _) in help.match_indices('-') {
        let before = if i == 0 { b' ' } else { bytes[i - 1] };
        if !matches!(before, b' ' | b'\n' | b'[' | b'(' | b'`' | b',' | b'|') {
            continue;
        }
        let rest = &help[i..];
        let len = rest
            .char_indices()
            .find(|&(j, c)| j > 0 && !(c.is_ascii_alphanumeric() || c == '-'))
            .map_or(rest.len(), |(j, _)| j);
        let flag = &rest[..len];
        let name = flag.trim_start_matches('-');
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        if flag.starts_with("--") || name.len() == 1 {
            let takes_value = rest[len..].starts_with(" <");
            match found.iter_mut().find(|(f, _)| f == flag) {
                Some(seen) => seen.1 |= takes_value,
                None => found.push((flag.to_string(), takes_value)),
            }
        }
    }
    found
}

/// A value an option accepts, for checking that the option is known.
fn value_for(flag: &str) -> &'static str {
    match flag {
        "--format" => "zip",
        "--platform" => "desktop",
        "--max-entry-size" | "--max-pack-size" => "1GiB",
        "--max-entries" | "--smoke-ticks" | "--smoke-timeout" | "--tolerance" | "--threshold" => "1",
        _ => "x",
    }
}

/// The part of the guide's chapter about `command`: from its heading to
/// the next.
fn chapter_section(chapter: &str, command: &str) -> String {
    let heading = format!("## `ponko {command}`");
    let start = chapter
        .find(&heading)
        .unwrap_or_else(|| panic!("book/src/cli.md has no section \"{heading}\""));
    let body = &chapter[start + heading.len()..];
    body[..body.find("\n## ").unwrap_or(body.len())].to_string()
}

#[test]
fn every_command_has_its_help_its_options_and_its_guide_section() {
    let dir = TempDir::new("options");
    let (ok, usage, err) = ponko(&["--help"], &dir.0);
    assert!(ok, "{err}");
    assert_eq!(ponko(&[], &dir.0).1, usage, "no arguments shows the usage");
    assert_eq!(ponko(&["help"], &dir.0).1, usage);
    let chapter_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../book/src/cli.md");
    let chapter = std::fs::read_to_string(&chapter_file).expect("the guide's chapter on the command");
    let commands = commands(&usage);
    assert!(
        commands.len() >= 9 && commands.contains(&"package".into()),
        "{commands:?}"
    );
    for (option, _) in options(&usage) {
        assert!(
            chapter.contains(&option),
            "book/src/cli.md doesn't name `ponko {option}`"
        );
    }
    for command in &commands {
        if command == "help" {
            continue;
        }
        let (ok, help, err) = ponko(&["help", command], &dir.0);
        assert!(ok, "ponko help {command}: {err}");
        assert!(help.starts_with(&format!("ponko {command}: ")), "{help}");
        let (ok, same, err) = ponko(&[command, "--help"], &dir.0);
        assert!(ok && same == help, "ponko {command} --help: {err}");
        assert!(
            usage.contains(&format!("  ponko {command} ")),
            "a usage line for {command}"
        );

        let section = chapter_section(&chapter, command);
        let options = options(&help);
        assert!(!options.is_empty(), "{command} lists no options");
        for (flag, takes_value) in options {
            assert!(
                section.contains(&format!("`{flag}")),
                "book/src/cli.md, section `ponko {command}`, doesn't name {flag}"
            );
            // Two images first for image-diff, so the option is what's judged.
            let mut args = vec![command.as_str()];
            if command == "image-diff" {
                args.extend(["missing-a.png", "missing-b.png"]);
            }
            args.push(&flag);
            if takes_value {
                args.push(value_for(&flag));
            }
            let (_, _, err) = ponko(&args, &dir.0);
            for refusal in ["unknown option", "unknown argument", "doesn't take", "no command"] {
                assert!(
                    !err.contains(refusal),
                    "`ponko {}` refuses an option its help lists:\n{err}",
                    args.join(" ")
                );
            }
        }
    }
    let (ok, _, err) = ponko(&["help", "unpack"], &dir.0);
    assert!(!ok && err.contains("no command `unpack`"), "{err}");
}

#[test]
fn an_unknown_option_is_refused_by_every_command() {
    let dir = TempDir::new("unknown");
    let (_, usage, _) = ponko(&["--help"], &dir.0);
    for command in commands(&usage).iter().filter(|c| *c != "help") {
        let mut args = vec![command.as_str()];
        if command == "image-diff" {
            args.extend(["a.png", "b.png"]);
        }
        args.push("--no-such-option");
        let (ok, _, err) = ponko(&args, &dir.0);
        assert!(!ok && err.contains("--no-such-option"), "ponko {command}: {err}");
    }
}

#[test]
fn the_asset_commands_do_their_job_in_an_empty_folder() {
    let dir = TempDir::new("assets");
    let root = &dir.0;
    std::fs::create_dir_all(root.join("assets/levels")).unwrap();
    std::fs::write(root.join("assets/levels/one.json"), "{\"entities\": []}").unwrap();
    std::fs::write(root.join("assets/notes.psd"), "layers").unwrap();

    let (ok, out, err) = ponko(&["pack", "--exclude", "**/*.psd"], root);
    assert!(ok, "{err}");
    assert!(out.contains("assets.pack"), "{out}");
    let (ok, out, err) = ponko(&["pack", "--list", "target/assets.pack"], root);
    assert!(ok, "{err}");
    assert!(out.contains("levels/one.json") && !out.contains("notes.psd"), "{out}");

    let (ok, _, err) = ponko(&["cook", "--no-cache", "--out", "cooked"], root);
    assert!(ok, "{err}");
    assert_eq!(
        std::fs::read_to_string(root.join("cooked/levels/one.json")).unwrap(),
        "{\"entities\": []}",
        "anything else is copied"
    );
    let (ok, _, err) = ponko(&["cook", "--pack", "cooked.pack", "--platform", "web"], root);
    assert!(ok, "{err}");
    assert!(root.join("cooked.pack").is_file());

    let (ok, _, err) = ponko(&["textures", "--check"], root);
    assert!(ok, "no image, so nothing to convert: {err}");
}

#[test]
fn new_then_editor_print_names_the_project() {
    let dir = TempDir::new("new");
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let engine = engine.canonicalize().unwrap();
    let (ok, out, err) = ponko(
        &["new", "crab_yard", "--3d", "--path", engine.to_str().unwrap()],
        &dir.0,
    );
    assert!(ok, "{err}");
    assert!(out.contains("crab_yard"), "{out}");
    let game = dir.0.join("crab_yard");
    assert!(game.join("Cargo.toml").is_file() && game.join("src/main.rs").is_file());

    let (ok, out, err) = ponko(&["editor", "crab_yard", "--open", "levels/one.json", "--print"], &dir.0);
    assert!(ok, "{err}");
    assert!(
        out.contains("PONKO_EDITOR=levels/one.json") && out.contains("editor"),
        "{out}"
    );
    let (ok, out, err) = ponko(&["editor", "--recent"], &dir.0);
    assert!(ok, "{err}");
    assert!(
        !out.contains("crab_yard"),
        "--print opens nothing, so nothing is recent: {out}"
    );
    let (ok, _, err) = ponko(&["editor", "--last", "--print"], &dir.0);
    assert!(!ok && !err.is_empty(), "nothing opened yet");
}

#[test]
fn notices_writes_the_libraries_of_a_crate() {
    let dir = TempDir::new("notices");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ponko_image_diff/Cargo.toml");
    let out_file = dir.0.join("NOTICES.txt");
    let (ok, out, err) = ponko(
        &[
            "notices",
            "--out",
            out_file.to_str().unwrap(),
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--title",
            "Image diff",
        ],
        &dir.0,
    );
    assert!(ok, "{err}");
    assert!(out.starts_with("wrote "), "{out}");
    let text = std::fs::read_to_string(&out_file).unwrap();
    assert!(text.contains("Image diff") && text.contains("image"), "{text}");
}
