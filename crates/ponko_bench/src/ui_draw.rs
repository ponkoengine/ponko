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
//! - `ui/draw-fade-group`: the same fade, the settings screen fading as
//!   one image (`Ui::set_fade_as_group`): its nodes go into an offscreen
//!   layer and keep their drawings; only the layer's opacity changes.
//! - `ui/draw-pulse`: the text color of the settings screen's 40 action
//!   labels goes back and forth, forever.
//! - `ui/draw-insert`: a slot (a panel with a label) is added to the HUD one
//!   frame and removed the next; the settings screen, drawn after the HUD,
//!   is the same.
//! - `ui/draw-scroll`: the settings screen's list of 40 actions scrolls
//!   0.7 units a frame, down and back up, forever: every row moves, onto
//!   other physical pixels.
//! - `ui/draw-tooltip`: the pointer moves over the settings screen, whose
//!   tooltip shows and follows it.
//! - `ui/draw-resize`: a row added to the settings screen, of a panel, a
//!   rich label and a wrapped paragraph: the panel is resized back and
//!   forth between two widths, every frame, moving the rich label sideways
//!   and giving the paragraph one width, then the other.
use std::hint::black_box;
use std::time::{Duration, Instant};

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::ui::{UiLayout, UiRepeat, UiTheme, UiTimeline, UiTween};

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
    FadeGroup,
    Pulse,
    Insert,
    Scroll,
    Tooltip,
    Resize,
}

pub fn run(b: &mut Bench) {
    for (name, change) in [
        ("ui/draw-still", Change::Still),
        ("ui/draw-fade", Change::Fade),
        ("ui/draw-fade-group", Change::FadeGroup),
        ("ui/draw-pulse", Change::Pulse),
        ("ui/draw-insert", Change::Insert),
        ("ui/draw-scroll", Change::Scroll),
        ("ui/draw-tooltip", Change::Tooltip),
        ("ui/draw-resize", Change::Resize),
    ] {
        if b.skips(name) {
            continue;
        }
        let mut world = Game::new("ui draw bench", 1280, 720).run_headless(0, move |ctx| {
            ctx.bench_ui_glyphs();
            let hud = build_ui(ctx);
            match change {
                Change::Fade | Change::FadeGroup => {
                    if change == Change::FadeGroup {
                        ctx.ui.set_fade_as_group(hud.menu, true);
                    }
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
                Change::Tooltip => {
                    ctx.ui.set_tooltip(hud.menu, "Settings apply at once");
                    let theme = UiTheme {
                        tooltip_delay: 0.0,
                        ..*ctx.ui.theme()
                    };
                    ctx.ui.set_theme(theme);
                }
                Change::Resize => {}
                Change::Still | Change::Insert | Change::Scroll => {}
            }
            let resized = (change == Change::Resize).then(|| resize_row(ctx, hud.menu));
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
                // Back and forth over 140 frames.
                let along = (frame % 140) as f32;
                let along = along.min(140.0 - along);
                match change {
                    Change::Scroll => ctx.ui.set_scroll(hud.list, vec2(0.0, along * 0.7)),
                    Change::Tooltip => {
                        let at = vec2(500.0 + along * 1.3, 300.0 + along * 0.4);
                        ctx.input.feed(InputEvent::MouseMove(at));
                    }
                    Change::Resize => {
                        let width = if frame % 2 == 0 { 40.0 } else { 64.0 };
                        let panel = resized.expect("the resized row");
                        ctx.ui.set_layout(panel, UiLayout::fixed(width, 20.0));
                    }
                    _ => {}
                }
                ctx.bench_ui_frame(DT);
                black_box(ctx.bench_ui_draw());
                let took = start.elapsed();
                if frame >= WARMUP {
                    times.push(took);
                }
            }
            if change == Change::Tooltip {
                assert!(ctx.ui.shown_tooltip().is_some(), "the tooltip shows while timed");
            }
            ctx.world.insert_resource(Times(times));
        });
        let mut times = world.remove_resource::<Times>().expect("the times").0;
        b.report(report::Record::new(name, report::Kind::Steady, WARMUP, &mut times));
    }
}

/// A row added to the settings screen `menu`: a panel (returned) before a
/// centred rich label and a wrapped paragraph that takes the rest.
fn resize_row(ctx: &mut Ctx, menu: ponko::ui::UiId) -> ponko::ui::UiId {
    let ui = &mut ctx.ui;
    let row = ui.panel(menu).layout(UiLayout::row(60.0).gap(8.0)).id();
    let panel = ui.panel(row).layout(UiLayout::fixed(40.0, 20.0)).id();
    ui.label(row, "[color=#fc0]Gold[/color] [b]x3[/b] [u]each[/u]")
        .rich()
        .align(ponko::ui::UiTextAlign::Center)
        .layout(UiLayout::fixed(140.0, 24.0));
    ui.label(
        row,
        "The window mode and the language take effect when the game restarts.",
    )
    .wrap()
    .layout(UiLayout::auto().grow(1.0));
    panel
}
