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

// A tuned build compiles one group: the others' helpers go unused.
#![cfg_attr(
    not(feature = "stock"),
    allow(
        dead_code,
        unused_imports,
        unused_variables,
        unused_assignments,
        unreachable_code,
        unreachable_patterns
    )
)]

mod common;
mod ecs;
#[cfg(all(not(feature = "stock"), any(feature = "sprites", feature = "ui", feature = "scenes")))]
mod finished;
#[cfg(feature = "scenes")]
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
    let build = if cfg!(feature = "stock") {
        "Bevy's default features"
    } else {
        "tuned"
    };
    println!("Bevy {BEVY_VERSION} ({build}), group {group:?}");
    let built = workloads::GROUPS.iter().any(|(g, on)| *g == group && *on);
    match group.as_str() {
        "ecs" => ecs::run(&filter),
        #[cfg(feature = "scenes")]
        "scenes" => scenes::run(&filter),
        _ if built => workloads::run(&group, &filter),
        #[cfg(not(feature = "stock"))]
        "scenes" | "sim" | "physics" | "sprites" | "ui" | "first-frame" => {
            eprintln!("this build has no {group:?} group: build it with its feature (see Cargo.toml)");
            std::process::exit(2);
        }
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
