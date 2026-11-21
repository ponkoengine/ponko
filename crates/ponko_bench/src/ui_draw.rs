//! The retained UI's draw list when part of the UI changes (`ui/draw-...`):
//! the HUD and settings screen of `frame2d` (about 150 nodes), with text
//! drawn as with a GPU (each glyph pushed; a headless run skips them). One
//! timed frame is the UI's frame (input, animations, layout) and its draw
//! list, as a window's frame runs them.
//!
//! - `ui/draw-still`: nothing changes.
//! - `ui/draw-fade`: the settings screen fades from opaque to half
//!   see-through and back, forever: every one of its nodes is drawn with a
//!   new opacity each frame.
//! - `ui/draw-pulse`: the text color of the settings screen's 40 action
//!   labels goes back and forth, forever.
//! - `ui/draw-insert`: a slot (a panel with a label) is added to the HUD one
//!   frame and removed the next; the settings screen, drawn after the HUD,
//!   is the same.
use std::hint::black_box;
use std::time::{Duration, Instant};

use ponko::prelude::*;
use ponko::ui::{UiLayout, UiRepeat, UiTimeline, UiTween};

use crate::frame2d::build_ui;
use crate::{Bench, report};

const WARMUP: usize = 10;
const FRAMES: usize = 300;
const DT: f32 = 1.0 / 60.0;

/// Every timed frame.
struct Times(Vec<Duration>);

#[derive(Clone, Copy, PartialEq)]
enum Change {
    Still,
    Fade,
    Pulse,
    Insert,
}

pub fn run(b: &mut Bench) {
    for (name, change) in [
        ("ui/draw-still", Change::Still),
        ("ui/draw-fade", Change::Fade),
        ("ui/draw-pulse", Change::Pulse),
        ("ui/draw-insert", Change::Insert),
    ] {
        if b.skips(name) {
            continue;
        }
        let mut world = Game::new("ui draw bench", 1280, 720).run_headless(0, move |ctx| {
            ctx.bench_ui_glyphs();
            let hud = build_ui(ctx);
            match change {
                Change::Fade => {
                    let fade = UiTimeline::new()
                        .then(UiTween::opacity(0.5).from(1.0).seconds(0.5))
                        .repeat(UiRepeat::PingPong);
                    ctx.ui.play(hud.menu, &fade);
                }
                Change::Pulse => {
                    let pulse = UiTimeline::new()
                        .then(UiTween::text_color(Color::YELLOW).seconds(0.5))
                        .repeat(UiRepeat::PingPong);
                    for &label in &hud.actions {
                        ctx.ui.play(label, &pulse);
                    }
                }
                Change::Still | Change::Insert => {}
            }
            let mut slot = None;
            let mut times = Vec::with_capacity(FRAMES);
            for frame in 0..WARMUP + FRAMES {
                let start = Instant::now();
                if change == Change::Insert {
                    if let Some(added) = slot.take() {
                        ctx.ui.remove(added);
                    } else {
                        let added = ctx.ui.panel(hud.slots).layout(UiLayout::fixed(48.0, 48.0)).id();
                        ctx.ui.label(added, "9");
                        slot = Some(added);
                    }
                }
                ctx.bench_ui_frame(DT);
                black_box(ctx.bench_ui_draw());
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
