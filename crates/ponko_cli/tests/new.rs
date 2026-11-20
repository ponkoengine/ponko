//! End to end: `ponko new` makes each template against this engine checkout
//! (`--path`), and `cargo check --all-targets` builds the project and its
//! tests.
//!
//! Ignored by default: it type-checks the engine once per template's
//! features, minutes on a cold machine. Run it with
//! `cargo test -p ponko_cli --test new -- --ignored`; CI's "Project
//! templates" job does. The build output goes to `CARGO_TARGET_DIR` if it is
//! set, else to `target/ponko-new-e2e`, deleted afterwards.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Deletes a folder when dropped, also when the test fails.
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "builds the engine for every template (minutes); CI's template job runs it"]
fn every_template_generates_a_project_that_builds_against_this_engine() {
    let root = workspace_root();
    // Outside the workspace, so the projects aren't taken for its members.
    let projects = std::env::temp_dir().join(format!("ponko-new-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&projects);
    std::fs::create_dir_all(&projects).unwrap();
    let _projects = Remove(projects.clone());
    // One target folder for all of them: the caller's `CARGO_TARGET_DIR` (CI
    // caches it), else one inside this workspace's, deleted afterwards.
    let (target, _target) = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => (PathBuf::from(dir), None),
        None => {
            let dir = root.join("target/ponko-new-e2e");
            (dir.clone(), Some(Remove(dir)))
        }
    };

    for (flag, name) in [
        ("--2d", "e2e-2d"),
        ("--3d", "e2e-3d"),
        ("--headless", "e2e-headless"),
        ("--server", "e2e-server"),
        ("--plugin", "e2e-plugin"),
    ] {
        let made = Command::new(env!("CARGO_BIN_EXE_ponko"))
            .current_dir(&projects)
            .args(["new", name, flag, "--path"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "ponko new {name} {flag}: {}",
            String::from_utf8_lossy(&made.stderr)
        );

        let dir = projects.join(name);
        // A plugin crate is a library: no main, assets or MCP config.
        let files: &[&str] = if flag == "--plugin" {
            &["Cargo.toml", "AGENTS.md", "src/lib.rs", "tests/plugin.rs"]
        } else {
            &[
                "Cargo.toml",
                ".mcp.json",
                "AGENTS.md",
                "assets/settings.json",
                "src/lib.rs",
                "src/main.rs",
            ]
        };
        for file in files {
            assert!(dir.join(file).is_file(), "{name}/{file}");
        }
        let checked = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(&dir)
            .args(["check", "--all-targets", "--quiet"])
            .env("CARGO_TARGET_DIR", &target)
            .status()
            .unwrap();
        assert!(checked.success(), "cargo check in the generated {name}");
    }
}
