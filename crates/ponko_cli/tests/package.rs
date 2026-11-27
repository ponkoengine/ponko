//! End to end: a fresh project from `ponko new` (against this engine
//! checkout, `--path`) packages for the host with the one command
//! `ponko package`: a release build, the host's download (dmg on a Mac with
//! hdiutil, zip on Windows, tar.gz on Linux) and a passing smoke run.
//!
//! Ignored by default: it builds the engine in release, minutes on a cold
//! machine. Run it with `cargo test -p ponko_cli --test package -- --ignored`;
//! CI's "Project templates" job does. The build output goes to
//! `CARGO_TARGET_DIR` if it is set, else to `target/ponko-package-e2e`,
//! deleted afterwards.

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
#[ignore = "builds the engine in release (minutes); CI's template job runs it"]
fn a_fresh_project_packages_for_the_host_with_one_command() {
    let root = workspace_root();
    // Outside the workspace, so the project isn't taken for one of its members.
    let projects = std::env::temp_dir().join(format!("ponko-package-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&projects);
    std::fs::create_dir_all(&projects).unwrap();
    let _projects = Remove(projects.clone());
    let (target, _target) = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => (PathBuf::from(dir), None),
        None => {
            let dir = root.join("target/ponko-package-e2e");
            (dir.clone(), Some(Remove(dir)))
        }
    };

    let made = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .current_dir(&projects)
        .args(["new", "fresh_game", "--path"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));

    let packaged = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .current_dir(projects.join("fresh_game"))
        .arg("package")
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&packaged.stdout);
    assert!(
        packaged.status.success(),
        "ponko package: {report}\n{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    assert!(report.contains("smoke run: passed"), "{report}");

    let (label, archive) = if cfg!(windows) {
        ("windows", "zip")
    } else if cfg!(target_os = "macos") {
        let dmg = Command::new("hdiutil").arg("help").output().is_ok();
        ("macos", if dmg { "dmg" } else { "zip" })
    } else {
        ("linux", "tar.gz")
    };
    let arch = std::env::consts::ARCH;
    let name = format!("fresh-game-0.1.0-{label}-{arch}");
    let out = target.join("package");
    assert!(
        out.join(format!("{name}.{archive}")).is_file(),
        "no {name}.{archive} in {report}"
    );
    let folder = out.join(&name);
    let inner = if label == "macos" {
        folder.join("fresh_game.app/Contents/Resources")
    } else {
        folder.clone()
    };
    // The assets ship in one pack, which the smoke run read them from.
    assert!(report.contains("in assets.pack"), "{report}");
    assert!(report.contains("assets cooked "), "the pack is cooked: {report}");
    for file in ["assets.pack", "THIRD-PARTY-NOTICES.txt"] {
        assert!(inner.join(file).is_file(), "{file} in {}", inner.display());
    }
    assert!(!inner.join("assets").exists(), "no loose assets/ beside the pack");
    let pack = std::fs::read(inner.join("assets.pack")).unwrap();
    let names = String::from_utf8_lossy(&pack);
    assert!(
        pack.starts_with(b"PONKOPAK") && names.contains("settings.json"),
        "the pack holds the template's settings.json"
    );
    let notices = std::fs::read_to_string(inner.join("THIRD-PARTY-NOTICES.txt")).unwrap();
    assert!(
        // The project depends on this workspace's engine, whose version the
        // tool shares.
        notices.contains(concat!("  ponko ", env!("CARGO_PKG_VERSION"), ": MIT OR Apache-2.0")),
        "the engine is in the notices: {notices}"
    );
    assert!(
        !notices.contains("  fresh_game 0.1.0"),
        "the game itself is not a third party"
    );
}
