//! Ember Hop: a small platformer built on the engine's own levels, physics,
//! prefabs, scenes, particles, sound files and save slots. Carry an ember
//! spirit through two caves made in the Tiled map editor: collect sparks,
//! burn or stomp the drips, light braziers (they save the run), and reach
//! the lantern at the end of each cave.
//!
//!   `cargo run --release --example ember_hop --features physics`
//!
//! Move: arrow keys, A / D, the left stick or the d-pad. Jump: Space, Up, W
//! or South (hold for higher). Flare: X, J, Shift or West. Pause: Esc, P or
//! Start. On the title: Enter starts a run, C (North) continues the saved one.
//!
//! The rules live in `game.rs`, which `tests/ember_hop.rs` also drives without
//! a window (`cargo test --test ember_hop --features physics`). This file only
//! opens the window, and offers the walk-through the packaged smoke test uses:
//!
//!   `ember_hop --smoke headless [--report report.json]`   no window, no GPU
//!   `ember_hop --smoke window [--report report.json]`     a real window
//!
//! Package it with `ponko package --example ember_hop --features physics`
//! (the root `Cargo.toml` says what goes in); the web build is
//! `web/build.sh --features physics ember_hop`.

mod game;

#[cfg(all(
    feature = "bench-internals",
    feature = "window",
    feature = "3d",
    feature = "audio",
    not(target_arch = "wasm32")
))]
#[path = "../support/soak.rs"]
mod soak;

#[cfg(target_arch = "wasm32")]
fn main() {
    game::build()
        .add(ponko::prelude::Phase::Frame, game::smoke::publish_web_state)
        .configured()
        .run();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(all(
        feature = "bench-internals",
        feature = "window",
        feature = "3d",
        feature = "audio",
        not(target_arch = "wasm32")
    ))]
    if args.first().is_some_and(|arg| arg == "--soak") {
        soak::run(
            &args[1..],
            |base| game::smoke::smoke_game(base).configured(),
            |ctx| game::smoke::setup_smoke(ctx, None, "soak"),
            |ctx| {
                let smoke = ctx.world.resource::<game::smoke::Smoke>();
                let tally = ctx.world.resource::<game::Tally>();
                soak::Work {
                    cycles: smoke.cycles,
                    saves: u64::from(tally.saves),
                    loads: u64::from(tally.loads),
                    deaths: smoke.deaths,
                }
            },
            "ember_hop soak",
            (game::WINDOW.x as u32, game::WINDOW.y as u32),
        );
        return;
    }
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => game::build().configured().run(),
        ["--smoke", mode, rest @ ..] if matches!(*mode, "headless" | "window") => {
            let report = match rest {
                [] => None,
                ["--report", path] => Some(std::path::PathBuf::from(path)),
                _ => usage(),
            };
            smoke(mode, report);
        }
        _ => usage(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn usage() -> ! {
    eprintln!("usage: ember_hop [--smoke headless|window [--report <file>]]");
    std::process::exit(2);
}

/// Plays the walk-through (see `game::smoke`) and exits with 0 if it finished.
#[cfg(not(target_arch = "wasm32"))]
fn smoke(mode: &str, report: Option<std::path::PathBuf>) {
    use game::smoke::*;
    let name = mode.to_string();
    let setup = move |ctx: &mut ponko::prelude::Ctx| setup_smoke(ctx, report, &name);
    if mode == "window" {
        build_smoke().setup(setup).run();
    } else {
        let world = build_smoke().setup(setup).run_headless(MAX_TICKS + 1);
        if !world.resource::<Smoke>().finished() {
            eprintln!("ember_hop: the walk-through did not finish");
            std::process::exit(1);
        }
    }
}
