//! Coin Dash: a small, complete game built only on the public `Game`/`Ctx`
//! API. A title screen, settings (UI scale, volume, rebindable controls,
//! language), rounds (collect every coin before the clock runs out), a pause,
//! a result screen and a saved best time, in English and Arabic. One picture
//! (`assets/coin_dash/coin.png`), the string tables (`assets/coin_dash/lang`)
//! and the Arabic font are files; the rest is drawn and synthesized in code.
//!
//!   `cargo run --release --example coin_dash --features text-shaping`
//!
//! (Without `text-shaping` the game offers only English: Arabic letters join
//! and run right to left only when shaped.)
//!
//! Move: arrow keys, WASD or the left stick. Enter / Space / South: start,
//! resume, play again. R / West: restart the round. Esc / P / Start: pause
//! (quit on the title screen). Q / East: back to the title. O / North: settings.
//!
//! The best time, most coins and the settings are saved in the player's data
//! folder through `ctx.storage` (`PONKO_DATA_DIR` moves it), or the browser's
//! storage on the web; a missing or damaged file just starts over.
//!
//! The rules live in `game.rs`, which `tests/coin_dash.rs` also drives without
//! a window (`cargo test --test coin_dash`), so the whole loop (title, play,
//! pause, result, settings, saving) is checked in CI. The game is a plugin
//! (`CoinDash`, made of one plugin per layer); this file only opens the
//! window, and offers the walk-through that the packaged smoke test uses:
//!
//!   `coin_dash --smoke headless [--report report.json]`   no window, no GPU
//!   `coin_dash --smoke window [--report report.json]`     a real window
//!
//! To package it for the desktop, run it from the package and test the package,
//! see `scripts/package-coin-dash.py` and the guide's "A complete game, packaged".
//! The web build is `web/build.sh coin_dash`.

mod game;

#[cfg(target_arch = "wasm32")]
fn main() {
    // Everything, startup included, is in the game's plugins.
    game::build().system(game::smoke::publish_web_state).configured().run();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        // Everything, startup included, is in the game's plugins.
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
    eprintln!("usage: coin_dash [--smoke headless|window [--report <file>]]");
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
            eprintln!("coin_dash: the walk-through did not finish");
            std::process::exit(1);
        }
    }
}
