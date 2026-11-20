//! A whole 2D frame without a window (`frame2d/...`): 2,000 bouncing
//! sprites under a game's HUD and an open settings screen (about 150 UI
//! nodes: labels, a wrapped paragraph, sliders, toggles, buttons, a scroll
//! area). The time is the CPU side of one frame as a window runs it: the
//! UI's input and layout before the systems, the game's systems (movement,
//! the HUD's text), the UI's layout and draw list after them, and the
//! sprites collected, ordered and batched. Only the GPU is left out.
//!
//! - `frame2d/ui-still`: the HUD is written every frame with the text it
//!   already shows, as a game's HUD system does; nothing else changes.
//! - `frame2d/ui-counting`: the score's text changes every frame.
//! - `frame2d/no-ui`: the same frame with no UI, for the UI's share.
use std::hint::black_box;
use std::time::{Duration, Instant};

use ponko::d2::{Sprite, Transform as Transform2D};
use ponko::prelude::*;
use ponko::ui::{UiId, UiLayout};

use crate::{Bench, report};

const WARMUP: usize = 10;
const FRAMES: usize = 300;
const SPRITES: usize = 2_000;
const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

#[derive(Component)]
struct Vel(Vec2);

/// Every timed frame.
struct Times(Vec<Duration>);

#[derive(Clone, Copy, PartialEq)]
enum Ui {
    None,
    Still,
    Counting,
}

fn rand(seed: &mut u64) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 40) as f32 / (1u64 << 24) as f32
}

fn spawn(ctx: &mut Ctx) {
    let texture = ctx.gfx.create_texture(16, 16, &[255; 16 * 16 * 4]);
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    let batch: Vec<_> = (0..SPRITES)
        .map(|_| {
            let vel = vec2(rand(&mut seed) * 400.0 - 200.0, rand(&mut seed) * 400.0 - 300.0);
            (
                Transform2D::at(vec2(rand(&mut seed) * SIZE.x, rand(&mut seed) * SIZE.y)),
                Sprite::new(texture),
                Vel(vel),
            )
        })
        .collect();
    ctx.world.spawn_batch(batch);
}

fn bounce(ctx: &mut Ctx) {
    let dt = 1.0 / 60.0;
    ctx.world.query::<(&mut Transform2D, &mut Vel)>().for_each(|(t, v)| {
        let (p, v) = (&mut t.position, &mut v.0);
        v.y += 600.0 * dt;
        *p += *v * dt;
        if p.x < 0.0 || p.x > SIZE.x {
            p.x = p.x.clamp(0.0, SIZE.x);
            v.x = -v.x;
        }
        if p.y > SIZE.y {
            p.y = SIZE.y;
            v.y = -v.y.abs() * 0.85;
        }
    });
}

/// The labels the HUD system writes each frame.
struct Hud {
    score: UiId,
    coins: UiId,
    time: UiId,
}

/// A HUD along the top and a settings screen in the middle.
fn build_ui(ctx: &mut Ctx) -> Hud {
    let ui = &mut ctx.ui;
    let hud = ui.root().layout(UiLayout::fill().padding(12.0)).id();
    let bar = ui.panel(hud).layout(UiLayout::row(36.0).gap(16.0)).id();
    let score = ui.label(bar, "Score: 0").id();
    let coins = ui.label(bar, "Coins: 0").id();
    let time = ui.label(bar, "Time: 0:00").id();
    ui.progress_bar(bar).layout(UiLayout::fixed(160.0, 16.0));
    let slots = ui.panel(hud).layout(UiLayout::row(48.0).gap(8.0)).id();
    for i in 0..8 {
        let slot = ui.panel(slots).layout(UiLayout::fixed(48.0, 48.0)).id();
        ui.label(slot, format!("{}", i + 1));
    }
    ui.show_hud(hud);

    let menu = ui
        .root()
        .layout(UiLayout::centered(520.0, 600.0).padding(16.0).gap(8.0))
        .id();
    ui.title(menu, "Settings");
    ui.label(
        menu,
        "Changes apply at once. Some of them, such as the window mode and the \
         language, take effect when the game is restarted; the rest show now.",
    )
    .wrap();
    for name in ["Master", "Music", "Effects", "Voices", "Interface"] {
        let row = ui.panel(menu).layout(UiLayout::row(28.0).gap(8.0)).id();
        ui.label(row, name).layout(UiLayout::auto().width(120.0));
        ui.slider(row).layout(UiLayout::auto().grow(1.0));
    }
    for name in ["Subtitles", "Screen shake", "Show damage numbers", "Invert camera"] {
        ui.toggle(menu, name).layout(UiLayout::row(28.0));
    }
    let list = ui.scroll_area(menu).layout(UiLayout::fill().gap(4.0)).id();
    for i in 0..40 {
        let row = ui.panel(list).layout(UiLayout::row(24.0).gap(8.0)).id();
        ui.label(row, format!("Action {i}"))
            .layout(UiLayout::auto().width(200.0));
        ui.button(row, "Rebind");
    }
    let buttons = ui.panel(menu).layout(UiLayout::row(40.0).gap(8.0)).id();
    for name in ["Back", "Defaults", "Apply"] {
        ui.button(buttons, name).layout(UiLayout::auto().grow(1.0));
    }
    ui.push_modeless(menu);
    Hud { score, coins, time }
}

pub fn run(b: &mut Bench) {
    for (name, ui) in [
        ("frame2d/ui-still", Ui::Still),
        ("frame2d/ui-counting", Ui::Counting),
        ("frame2d/no-ui", Ui::None),
    ] {
        if b.skips(name) {
            continue;
        }
        let mut world = Game::new("frame2d bench", SIZE.x as u32, SIZE.y as u32).run_headless(0, move |ctx| {
            spawn(ctx);
            let hud = (ui != Ui::None).then(|| build_ui(ctx));
            let mut times = Vec::with_capacity(FRAMES);
            let mut text = String::new();
            for frame in 0..WARMUP + FRAMES {
                let start = Instant::now();
                if let Some(hud) = &hud {
                    ctx.bench_ui_update();
                    // The HUD system: every label written each frame.
                    let score = if ui == Ui::Counting { frame * 10 } else { 1200 };
                    for (id, value) in [(hud.score, score), (hud.coins, 37), (hud.time, 75)] {
                        use std::fmt::Write;
                        text.clear();
                        let _ = write!(text, "{value}");
                        ctx.ui.set_text(id, &text);
                    }
                }
                bounce(ctx);
                if hud.is_some() {
                    black_box(ctx.bench_ui_draw());
                }
                black_box(ctx.bench_sprite_frame());
                let took = start.elapsed();
                if frame >= WARMUP {
                    times.push(took);
                }
            }
            ctx.world.insert_resource(Times(times));
        });
        let mut times = world.remove_resource::<Times>().expect("the times").0;
        b.report(report::Record::new(name, report::Kind::Steady, WARMUP, &mut times));
    }
}
