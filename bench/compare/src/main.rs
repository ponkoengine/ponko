//! The other engine's side of the cross-engine scoreboard: the same
//! workloads as Ponko's `compare/...` rows, the ECS matrix and the
//! rendering baseline's scenes, written with Bevy (pinned in `Cargo.toml`,
//! printed in every result). A crate of its own, with its own workspace and
//! lock file, so no other engine ever enters the engine's dependency tree.
//!
//! ```sh
//! cd bench/compare
//! cargo run --release -- <group> [filter]
//! ```
//!
//! Groups: `ecs` (the ECS matrix: ponko_ecs, bevy_ecs and hecs, compiling
//! `crates/ponko_bench/src/ecs_suite.rs` and `archetypes.rs` as they are),
//! `scenes` (the rendering baseline's matched scenes), `sim`, `physics`,
//! `sprites`, `ui` and `first-frame`. `PONKO_BENCH_JSON=<file>` writes the
//! results. `scoreboard.py` runs every group on both engines and writes the
//! table; `README.md` has the method.

mod ecs;
mod scenes;
mod workloads;

// The ECS matrix's shared files name these at their crate's root.
pub use ecs::{Bench, DT, Pos, Spr, Tag, Vel, ent};

#[allow(dead_code)]
#[path = "../../../crates/ponko_bench/src/ecs_suite.rs"]
mod ecs_suite;

#[allow(dead_code)]
#[path = "../../../crates/ponko_bench/src/archetypes.rs"]
mod archetypes;

mod ecs_bevy;

/// The Bevy version `Cargo.toml` pins (`bevy = "=..."`); a test keeps the
/// two in step.
pub const BEVY_VERSION: &str = "0.20.1";

fn main() {
    let mut args = std::env::args().skip(1);
    let group = args.next().unwrap_or_default();
    let filter = args.next().unwrap_or_default();
    println!("Bevy {BEVY_VERSION}, group {group:?}");
    match group.as_str() {
        "ecs" => ecs::run(&filter),
        "scenes" => scenes::run(&filter),
        "sim" | "physics" | "sprites" | "ui" | "first-frame" => workloads::run(&group, &filter),
        _ => {
            eprintln!("usage: compare <ecs|scenes|sim|physics|sprites|ui|first-frame> [filter]");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_printed_bevy_version_is_the_pinned_one() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains(&format!("bevy = {{ version = \"={}\"", super::BEVY_VERSION)),
            "Cargo.toml pins another Bevy than BEVY_VERSION"
        );
    }
}
