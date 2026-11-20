//! `ponko editor` as a program: `--print` shows the `cargo run` it would
//! start, without building or opening anything.

use std::path::Path;
use std::process::Command;

fn ponko(args: &[&str], dir: &Path) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn print_in_the_engine_runs_an_example_with_its_editor_feature() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (ok, out, err) = ponko(&["editor", "--example", "editor", "--open", "a.json", "--print"], &root);
    assert!(ok, "{err}");
    assert!(
        out.contains("PONKO_EDITOR=a.json") && out.contains("run --example editor --features editor"),
        "{out}"
    );
}

#[test]
fn a_folder_without_a_cargo_project_is_an_error() {
    let (ok, _, err) = ponko(
        &["editor", "--print", "no-such-game"],
        Path::new(env!("CARGO_MANIFEST_DIR")),
    );
    assert!(!ok && err.contains("no Cargo.toml"), "{err}");
}
