//! Shard Grove: a small third-person 3D game, the engine's 3D reference.
//! Guide a warden through a walled grove: find six shards while crawlers
//! patrol and chase on a navigation mesh, stun them with a pulse, save at
//! the shrines, and leave by the gate once it opens.
//!
//!   `cargo run --release --example shard_grove --features navmesh,physics3d`
//!
//! Move: WASD, the arrow keys or the left stick. Turn the view: Q / E or the
//! right stick. Jump: Space or South. Pulse: F, Left Shift or West. Pause:
//! Esc, P or Start. The menus take the mouse, Tab and the arrows, and Enter.
//!
//! The rules live in `game.rs`, which `tests/shard_grove.rs` also drives
//! without a window (`cargo test --test shard_grove --features
//! navmesh,physics3d`). This file only opens the window, and offers the
//! walk-through the packaged smoke test uses:
//!
//!   `shard_grove --smoke headless [--report report.json]`   no window, no GPU
//!   `shard_grove --smoke window [--report report.json]`     a real window
//!
//! Package it with `ponko package --example shard_grove --features
//! navmesh,physics3d` (the root `Cargo.toml` says what goes in); the web
//! build is `web/build.sh --features navmesh,physics3d shard_grove`, and
//! `?smoke=walkthrough` in its address plays the walk-through in the browser.

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
    use game::smoke::*;
    // `?smoke=walkthrough` in the address: the game plays the walk-through
    // by itself (the web smoke test's `shard_grove_walkthrough` fixture).
    let walkthrough = web_walkthrough_asked();
    let game = if walkthrough { build_smoke() } else { game::build() };
    let game = game.add(ponko::prelude::Phase::Frame, publish_web_state);
    if walkthrough {
        game.setup(|ctx: &mut ponko::prelude::Ctx| setup_smoke(ctx, None, "web"))
            .run();
    } else {
        game.configured().run();
    }
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
            "shard_grove soak",
            game::WINDOW,
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
    eprintln!("usage: shard_grove [--smoke headless|window [--report <file>]]");
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
            eprintln!("shard_grove: the walk-through did not finish");
            std::process::exit(1);
        }
    }
}
