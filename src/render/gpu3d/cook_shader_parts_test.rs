//! The engine's code around a game's shader, as `ponko cook` checks
//! shaders with it (`crates/ponko_cli/src/cook/shaders/`): each part file
//! is a composition with the game's code replaced by a marker, and the mesh
//! prelude, which most kinds share, by another.
//!
//! When a prelude, entry point or composition changes, this test fails;
//! `PONKO_UPDATE_COOK_SHADERS=1 cargo test --features 3d --lib
//! cook_shader_parts` writes the files again.

use super::shaders::{MESH_PRELUDE, compose_compute, compose_effect, compose_material};

const PRELUDE: &str = "//@@ponko-mesh-prelude@@\n";
const GAME: &str = "//@@ponko-game@@\n";
/// What stands for the game's code while composing.
const MARK: &str = "/*@@ponko-game@@*/";

/// A composition of `MARK` (and `extra` after it) as a part file.
fn part(composed: &str, extra: &str) -> String {
    let (before, after) = composed.split_once(MARK).expect("the marker is in the composition");
    let after = after.strip_prefix(extra).expect("the extra code follows the marker");
    let before = match before.strip_prefix(MESH_PRELUDE) {
        Some(rest) => format!("{PRELUDE}{rest}"),
        None => before.to_string(),
    };
    // The engine adds a newline after the game's code itself.
    format!("{before}{GAME}{}", after.strip_prefix('\n').unwrap_or(after))
}

fn parts() -> Vec<(&'static str, String)> {
    let update = "\nfn update(i: u32) {}\n";
    vec![
        ("mesh_prelude.wgsl", MESH_PRELUDE.to_string()),
        ("material.wgsl", part(&compose_material(MARK, true, true), "")),
        (
            "sprite.wgsl",
            part(&crate::sprite_material::compose_sprite_shader(MARK), ""),
        ),
        ("sky.wgsl", part(&super::compose_sky(MARK), "")),
        ("effect.wgsl", part(&compose_effect(MARK), "")),
        ("compute.wgsl", part(&compose_compute(MARK), "")),
        (
            "compute_update.wgsl",
            part(&compose_compute(&format!("{MARK}{update}")), update),
        ),
    ]
}

#[test]
fn cook_shader_parts_match_the_engine() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/ponko_cli/src/cook/shaders");
    let update = std::env::var_os("PONKO_UPDATE_COOK_SHADERS").is_some();
    let mut stale = Vec::new();
    for (file, text) in parts() {
        let path = dir.join(file);
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(file);
        }
    }
    assert!(
        stale.is_empty(),
        "ponko cook's shader parts are out of date ({}): run \
         PONKO_UPDATE_COOK_SHADERS=1 cargo test --features 3d --lib cook_shader_parts",
        stale.join(", ")
    );
}
