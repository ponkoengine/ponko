//! The window's safe area (`Ctx::safe_area`): the part a phone's notch,
//! home bar and rounded corners leave. Fed as a phone reports it, the
//! on-screen controls and the ready-made arrange screen stay inside it, a
//! dragged control stops at its edge, and UI roots made
//! `inside_safe_area` are laid out in it and follow it when it changes.
//! Headless frames; no real notch (that needs a phone).
use std::cell::RefCell;
use std::rc::Rc;

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::ui::touch::{TouchControls, TouchShow};
use ponko::window::SafeInsets;

const ACTIONS: [&str; 4] = ["move", "jump", "fire", "pause"];

fn actions(ctx: &mut Ctx) {
    for (name, key) in [("jump", Key::Space), ("fire", Key::F), ("pause", Key::Escape)] {
        ctx.input.set_action(name, Action::Button(vec![ButtonBinding::Key(key)]));
    }
    ctx.input.set_action(
        "move",
        Action::Axis2(vec![Axis2Binding::Keys {
            left: Key::A,
            right: Key::D,
            up: Key::W,
            down: Key::S,
        }]),
    );
}

fn controls() -> TouchControls {
    TouchControls::new()
        .stick("move")
        .button("jump", "A")
        .button("fire", "B")
        .corner_button("pause", "II")
        .show(TouchShow::Always)
}

/// Phones as they report their safe areas, in logical pixels (a size,
/// a UI scale, the insets).
fn phones() -> [((u32, u32), f32, SafeInsets); 4] {
    [
        // Upright: the notch above, the home bar below.
        ((390, 844), 1.0, SafeInsets::new(47.0, 0.0, 34.0, 0.0)),
        // Sideways: the notch on one side, rounded corners on the other.
        ((844, 390), 1.0, SafeInsets::new(0.0, 47.0, 21.0, 47.0)),
        // A tablet-sized view at UI scale 2.
        ((1688, 780), 2.0, SafeInsets::new(0.0, 94.0, 42.0, 94.0)),
        // A camera hole in a corner, upright.
        ((412, 915), 1.0, SafeInsets::new(52.0, 0.0, 24.0, 0.0)),
    ]
}

/// What the screen shows two frames in: the controls' discs (centre and
/// radius, from their nodes and their input zones) and the arrange panel.
#[derive(Debug, Default)]
struct Shown {
    safe: Option<Rect>,
    discs: Vec<(String, Vec2, f32)>,
    zones: Vec<(String, Vec2)>,
    card: Option<Rect>,
}

fn shown(size: (u32, u32), scale: f32, insets: SafeInsets, arrange: bool) -> Shown {
    let found = Rc::new(RefCell::new(Shown::default()));
    let out = found.clone();
    Game::new("notch", size.0, size.1)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let mut s = out.borrow_mut();
            *s = Shown {
                safe: Some(ctx.safe_area()),
                ..Shown::default()
            };
            for action in ACTIONS {
                let node = ctx.ui.find(&format!("touch.{action}")).unwrap();
                let rect = ctx.ui.rect(node).unwrap();
                s.discs.push((action.into(), rect.center(), rect.size().x / 2.0));
                let zone = ctx.input.touch_control_center(action).unwrap();
                s.zones.push((action.into(), zone));
            }
            s.card = ctx.ui.find("touch.arrange.card").and_then(|id| ctx.ui.rect(id));
        })
        .run_headless(2, move |ctx| {
            ctx.ui.set_scale(scale);
            ctx.feed_safe_insets(insets);
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            if arrange {
                ctx.input.arrange_touch_controls();
            }
        });
    Rc::try_unwrap(found).unwrap().into_inner()
}

fn inside(r: Rect, area: Rect) -> bool {
    r.min.cmpge(area.min - Vec2::splat(0.5)).all() && r.max.cmple(area.max + Vec2::splat(0.5)).all()
}

fn disc(centre: Vec2, radius: f32) -> Rect {
    Rect {
        min: centre - Vec2::splat(radius),
        max: centre + Vec2::splat(radius),
    }
}

#[test]
fn touch_controls_and_the_arrange_screen_stay_inside_a_fed_safe_area() {
    for (size, scale, insets) in phones() {
        let s = shown(size, scale, insets, true);
        let safe = s.safe.unwrap();
        let window = Vec2::new(size.0 as f32, size.1 as f32);
        assert_eq!(
            (safe.min, safe.max),
            (
                Vec2::new(insets.left, insets.top),
                window - Vec2::new(insets.right, insets.bottom)
            ),
            "{size:?}: the safe area is the window less the insets"
        );
        for (action, centre, radius) in &s.discs {
            assert!(
                inside(disc(*centre, *radius), safe),
                "{size:?}: {action}'s disc at {centre} ({radius}) leaves the safe area {safe:?}"
            );
        }
        for ((action, zone), (_, centre, _)) in s.zones.iter().zip(&s.discs) {
            assert!(
                zone.distance(*centre) < 1.0,
                "{size:?}: {action} takes fingers where it is drawn ({zone}, {centre})"
            );
        }
        let card = s.card.expect("the arrange panel");
        assert!(inside(card, safe), "{size:?}: the panel {card:?} leaves {safe:?}");
    }
}

#[test]
fn without_a_safe_area_the_controls_would_sit_under_the_notch() {
    // Negative control: the same phone with nothing reported puts the
    // stick and the corner button where the insets above cover.
    for (size, scale, insets) in phones() {
        let s = shown(size, scale, SafeInsets::NONE, true);
        let window = Vec2::new(size.0 as f32, size.1 as f32);
        let covered = Rect {
            min: Vec2::new(insets.left, insets.top),
            max: window - Vec2::new(insets.right, insets.bottom),
        };
        let under = s
            .discs
            .iter()
            .filter(|(_, c, r)| !inside(disc(*c, *r), covered))
            .count();
        assert!(under > 0, "{size:?}: some control lies under {insets:?} with no safe area");
    }
}

#[test]
fn a_control_dragged_into_the_notch_stops_at_its_edge() {
    let insets = SafeInsets::new(0.0, 47.0, 21.0, 47.0);
    let found = Rc::new(RefCell::new(None));
    let out = found.clone();
    Game::new("drag", 844, 390)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let tick = ctx.time.frame();
            let jump = ctx.input.touch_control_center("jump").unwrap();
            match tick {
                1 => ctx.input.feed(InputEvent::touch_start(1, jump)),
                2 => ctx.input.feed(InputEvent::touch_move(1, Vec2::new(-200.0, 450.0))),
                3 => ctx.input.feed(InputEvent::touch_end(1, Vec2::new(-200.0, 450.0))),
                5 => {
                    let node = ctx.ui.rect(ctx.ui.find("touch.jump").unwrap()).unwrap();
                    *out.borrow_mut() = Some((jump, node, ctx.safe_area()));
                }
                _ => {}
            }
        })
        .run_headless(6, move |ctx| {
            ctx.feed_safe_insets(insets);
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            ctx.input.edit_touch_controls(true);
        });
    let (centre, node, safe) = found.borrow().expect("the drag ran");
    assert!(inside(node, safe), "dragged into the corner, {node:?} stays in {safe:?}");
    let radius = node.size().x / 2.0;
    assert!(
        (centre.x - radius - safe.min.x).abs() < 1.0 && (centre.y + radius - safe.max.y).abs() < 1.0,
        "it stops against the notch's and the home bar's edges: {centre} ({radius}) in {safe:?}"
    );
}

#[test]
fn ui_roots_inside_the_safe_area_lay_out_in_it_and_follow_it_when_it_changes() {
    let found = Rc::new(RefCell::new(Vec::new()));
    let out = found.clone();
    Game::new("hud", 844, 390)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let rect = |name: &str| ctx.ui.rect(ctx.ui.find(name).unwrap()).unwrap();
            out.borrow_mut().push((
                ctx.safe_area(),
                rect("backdrop"),
                rect("hud"),
                rect("hud.score"),
                rect("dialog"),
            ));
            if ctx.time.frame() == 2 {
                // The phone turns the other way: the notch moves right. The
                // UI lays out before the next frame's systems.
                ctx.feed_safe_insets(SafeInsets::new(0.0, 47.0, 21.0, 0.0));
            }
        })
        .run_headless(4, |ctx| {
            ctx.feed_safe_insets(SafeInsets::new(0.0, 0.0, 21.0, 47.0));
            // A backdrop that fills the whole screen, under the notch too.
            let backdrop = ctx.ui.root().layout(UiLayout::fill()).name("backdrop").id();
            ctx.ui.show_hud(backdrop);
            // A HUD and its score in the top left corner, clear of the notch.
            let hud = ctx.ui.root().layout(UiLayout::fill()).name("hud").inside_safe_area().id();
            ctx.ui
                .label(hud, "Score 0")
                .name("hud.score")
                .layout(UiLayout::fixed(120.0, 30.0));
            ctx.ui.show_hud(hud);
            // A dialog centred in the safe area.
            let dialog = ctx
                .ui
                .root()
                .layout(UiLayout::centered(200.0, 100.0))
                .name("dialog")
                .inside_safe_area()
                .id();
            ctx.ui.push_modeless(dialog);
        });
    let frames = found.borrow();
    let window = Rect::new(0.0, 0.0, 844.0, 390.0);
    for (i, &(safe, backdrop, hud, score, dialog)) in frames.iter().enumerate() {
        assert_eq!(backdrop, window, "frame {i}: the backdrop fills the window");
        assert_eq!(hud, safe, "frame {i}: the HUD fills the safe area");
        assert_eq!(score.min, safe.min, "frame {i}: the score sits in the safe area's corner");
        assert!(
            (dialog.center() - safe.center()).length() < 1.0,
            "frame {i}: the dialog {dialog:?} is centred in {safe:?}"
        );
    }
    assert_eq!(frames[0].0.min.x, 47.0, "the notch on the left at first");
    let last = frames.last().unwrap();
    assert_eq!(
        (last.0.min.x, last.0.max.x),
        (0.0, 797.0),
        "then on the right, and the HUD followed"
    );
}
