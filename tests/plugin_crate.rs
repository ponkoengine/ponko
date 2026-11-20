//! A plugin crate outside the workspace builds against the engine's public
//! API and passes its own tests: `tests/plugin-crate`, which is the
//! `ponko new --plugin` template (`ponko_cli` checks they match).
//!
//! It is a separate Cargo project (a workspace of its own), so it sees
//! only what any crate on crates.io would: `pub` items, through a path
//! dependency on this checkout. It builds into this workspace's target
//! folder with the workspace's lock file and dev profile, so it reuses
//! what is already built and leaves no second target folder behind.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Removes a file when dropped, also when the test fails.
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn a_plugin_crate_outside_the_workspace_uses_only_public_api() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let krate = root.join("tests/plugin-crate");
    let mut files = Vec::new();
    rust_files(&krate, &mut files);
    assert!(files.len() >= 2, "the crate's lib and tests: {files:?}");
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // `#[doc(hidden)]` engine items (`ponko::__ffi`, ...) are public to
        // the compiler but not API: a plugin must not reach for them.
        assert!(
            !text.contains("ponko::__"),
            "{} uses a hidden engine item",
            file.display()
        );
    }
    let manifest = std::fs::read_to_string(krate.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains(r#"ponko = { path = "../..", default-features = false }"#),
        "a plain path dependency on this checkout: {manifest}"
    );
}

#[test]
fn a_plugin_crate_outside_the_workspace_builds_and_passes_its_tests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let krate = root.join("tests/plugin-crate");
    // The caller's target folder (CI caches it), else this workspace's:
    // `target/tmp`'s parent.
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_TARGET_TMPDIR")).parent().unwrap().to_path_buf(),
        PathBuf::from,
    );
    // The workspace's versions of every dependency, so nothing is resolved
    // anew (or downloaded) and the shared builds match.
    let lock = krate.join("Cargo.lock");
    std::fs::copy(root.join("Cargo.lock"), &lock).unwrap();
    let _lock = Remove(lock);
    let out = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&krate)
        .args(["test", "--quiet", "--offline", "--target-dir"])
        .arg(&target)
        // The workspace's dev profile (the template's own optimizes the
        // dependencies more), so the engine's dependencies are reused.
        .args(["--config", "profile.dev.package.\"*\".opt-level=1"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "the plugin crate failed to build or test:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
