//! The ready-made screen for arranging the on-screen controls
//! (`Ctx::arrange_touch_controls`): one call opens it, fingers and the
//! mouse move and resize the controls on it, Reset puts back the game's
//! placement, Save keeps the arrangement in the player's storage, and
//! Cancel or Escape put back what it had. Headless frames, input scenarios.
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};
use ponko::ui::UiEvent;
use ponko::ui::touch::{TouchControls, TouchLayout, TouchPlace, TouchShow};

fn actions(ctx: &mut Ctx) {
    ctx.input
        .set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
    ctx.input
        .set_action("fire", Action::Button(vec![ButtonBinding::Key(Key::F)]));
    ctx.input
        .set_action("pause", Action::Button(vec![ButtonBinding::Key(Key::Escape)]));
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

/// A game's controls, shown on touch screens only: the screen shows them
/// on a desktop too.
fn controls() -> TouchControls {
    TouchControls::new()
        .stick("move")
        .button("jump", "A")
        .button("fire", "B")
        .corner_button("pause", "II")
        .show(TouchShow::Auto)
}

/// A settings page with a button of the game's own, under the screen.
fn settings(ctx: &mut Ctx) {
    let root = ctx.ui.root().layout(UiLayout::fill()).id();
    ctx.ui
        .button(root, "Arrange controls")
        .name("settings.arrange")
        .layout(UiLayout::anchored(0.0, 0.0, 1.0, 1.0));
    ctx.ui.push_modeless(root);
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-touch-arrange-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Where things are on the open screen in a `size` window at UI scale
/// `scale`, two frames in: the centres of the screen's named nodes and of
/// the controls, and the rectangles of the panel and the controls' discs.
#[derive(Debug, Default)]
struct Spots {
    nodes: HashMap<String, Rect>,
    controls: HashMap<String, (Vec2, f32)>,
}

impl Spots {
    fn at(&self, node: &str) -> Vec2 {
        self.nodes[node].center()
    }
    fn control(&self, action: &str) -> Vec2 {
        self.controls[action].0
    }
}

const PARTS: [&str; 12] = [
    "card",
    "title",
    "note",
    "picked",
    "smaller",
    "bigger",
    "size",
    "opacity",
    "reset",
    "cancel",
    "save",
    "picked_size",
];

fn spots(size: (u32, u32), scale: f32) -> Spots {
    let found = Rc::new(RefCell::new(Spots::default()));
    let out = found.clone();
    Game::new("spots", size.0, size.1)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let mut spots = out.borrow_mut();
            for part in PARTS {
                let name = format!("touch.arrange.{part}");
                let id = ctx.ui.find(&name).unwrap_or_else(|| panic!("no {name}"));
                spots.nodes.insert(part.to_string(), ctx.ui.rect(id).unwrap());
            }
            for action in ["move", "jump", "fire", "pause"] {
                let zone = ctx.input.touch_control_center(action).unwrap();
                let rect = ctx.ui.rect(ctx.ui.find(&format!("touch.{action}")).unwrap()).unwrap();
                spots.controls.insert(action.to_string(), (zone, rect.size().x / 2.0));
            }
        })
        .run_headless(2, move |ctx| {
            ctx.ui.set_scale(scale);
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            settings(ctx);
            ctx.arrange_touch_controls();
        });
    Rc::try_unwrap(found).unwrap().into_inner()
}

/// What the game saw each tick: actions down, the game's own button
/// clicked, the arrange screen closed.
#[derive(Debug, Default, Clone)]
struct Seen {
    pressed_any: bool,
    settings_clicked: bool,
    closed: bool,
}

type Log = Rc<RefCell<Vec<Seen>>>;

fn watch(game: Game, log: &Log) -> Game {
    let log = log.clone();
    game.add(Phase::Frame, move |ctx: &mut Ctx| {
        let mut seen = Seen {
            pressed_any: ["move", "jump", "fire", "pause"]
                .iter()
                .any(|&a| ctx.input.action_pressed(a) || ctx.input.action_axis2(a) != Vec2::ZERO),
            ..Seen::default()
        };
        let settings = ctx.ui.find("settings.arrange");
        for event in ctx.ui.events() {
            match event {
                UiEvent::Clicked(id) if Some(id) == settings => seen.settings_clicked = true,
                UiEvent::ScreenPopped(_) => seen.closed = true,
                _ => {}
            }
        }
        log.borrow_mut().push(seen);
    })
}

fn saved(dir: &Path) -> Option<TouchLayout> {
    let text = std::fs::read_to_string(dir.join(TouchLayout::FILE)).ok()?;
    Some(TouchLayout::from_json(&text).unwrap())
}

// ANCHOR: fingers
#[test]
fn fingers_move_and_resize_the_controls_on_the_arrange_screen_and_save_keeps_them() {
    let s = spots((960, 540), 1.0);
    let dir = temp_dir("fingers");
    let log = Log::default();
    let top = vec2(480.0, 80.0);
    let scenario = InputScenario::new()
        // Drag the A button to the top middle.
        .touch_drag(2, 8, s.control("jump"), top)
        .expect(8, ScenarioCheck::ui("touch.arrange.picked").text("A"))
        // Tap B to pick it, then make it bigger twice.
        .tap(10, s.control("fire"))
        .expect(10, ScenarioCheck::ui("touch.arrange.picked").text("B"))
        .tap(12, s.at("bigger"))
        .tap(14, s.at("bigger"))
        .expect(14, ScenarioCheck::ui("touch.arrange.picked_size").text("120 %"))
        .tap(16, s.at("save"))
        .expect(17, ScenarioCheck::ui("touch.arrange").exists(false))
        .duration(19);
    let game = Game::new("fingers", 960, 540).headless_frames(true).data_dir(&dir);
    let (_, report) = watch(game, &log).run_headless_scenario(scenario, |ctx| {
        actions(ctx);
        ctx.input.set_touch_controls(controls());
        settings(ctx);
        let screen = ctx.arrange_touch_controls();
        assert_eq!(
            ctx.arrange_touch_controls(),
            screen,
            "opening it again is the same screen"
        );
    });
    report.assert_passed();
    let layout = saved(&dir).expect("Save wrote the layout");
    let jump = layout.adjusted("jump").place.expect("A was moved");
    assert_eq!(jump.centre(vec2(960.0, 540.0)), top, "where the finger let go");
    assert!((layout.adjusted("fire").size - 1.2).abs() < 1e-5, "B two steps bigger");
    let log = log.borrow();
    assert!(log.iter().all(|s| !s.pressed_any), "arranging pressed no action");
    assert!(log.iter().all(|s| !s.settings_clicked), "nor the page under it");
    assert!(log[16].closed, "the game hears it closed, in the frame of the tap");
    let _ = std::fs::remove_dir_all(dir);
}
// ANCHOR_END: fingers

#[test]
fn the_mouse_moves_and_resizes_the_controls_and_cancel_puts_them_back() {
    let s = spots((960, 540), 1.0);
    let dir = temp_dir("mouse");
    let stick = s.control("move");
    let to = stick + vec2(60.0, -120.0);
    let size = s.nodes["size"];
    let positions = Rc::new(RefCell::new(Vec::new()));
    let log = positions.clone();
    let scenario = InputScenario::new()
        // Drag the stick up and right with the mouse.
        .at(2, InputEvent::MouseMove(stick))
        .at(2, InputEvent::Mouse(MouseButton::Left, true))
        .at(3, InputEvent::MouseMove(stick + vec2(30.0, -60.0)))
        .at(4, InputEvent::MouseMove(to))
        .at(5, InputEvent::Mouse(MouseButton::Left, false))
        .expect(5, ScenarioCheck::ui("touch.arrange.picked").text("move stick"))
        // Smaller, then every control bigger with the Size slider.
        .click(7, "touch.arrange.smaller")
        .expect(7, ScenarioCheck::ui("touch.arrange.picked_size").text("90 %"))
        .at(9, InputEvent::MouseMove(vec2(size.max.x - 1.0, size.center().y)))
        .at(9, InputEvent::Mouse(MouseButton::Left, true))
        .at(10, InputEvent::Mouse(MouseButton::Left, false))
        .click(12, "touch.arrange.cancel")
        .expect(13, ScenarioCheck::ui("touch.arrange").exists(false))
        .duration(15);
    let (_, report) = Game::new("mouse", 960, 540)
        .headless_frames(true)
        .data_dir(&dir)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            log.borrow_mut().push((
                ctx.input.touch_control_center("move").unwrap(),
                ctx.input.touch_layout().clone(),
            ));
        })
        .run_headless_scenario(scenario, |ctx| {
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            ctx.arrange_touch_controls();
        });
    report.assert_passed();
    let positions = positions.borrow();
    assert_eq!(positions[5].0, to, "the stick followed the mouse");
    assert!((positions[7].1.adjusted("move").size - 0.9).abs() < 1e-5);
    assert!(
        positions[10].1.size > 1.9,
        "Size at its right end: {}",
        positions[10].1.size
    );
    assert_eq!(positions[14].1, TouchLayout::default(), "Cancel put back the layout");
    assert_eq!(positions[14].0, stick, "and the stick");
    assert!(saved(&dir).is_none(), "nothing was saved");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reset_puts_back_the_games_placement_and_escape_closes_without_keeping_changes() {
    let s = spots((960, 540), 1.0);
    let dir = temp_dir("reset");
    let mut moved = TouchLayout {
        opacity: 0.5,
        ..TouchLayout::default()
    };
    moved.adjust("jump").place = Some(TouchPlace::new(vec2(0.5, 0.0), vec2(0.0, 80.0)));
    moved.adjust("fire").size = 1.5;
    let layouts = Rc::new(RefCell::new(Vec::new()));
    let log = layouts.clone();
    let start = moved.clone();
    let scenario = InputScenario::new()
        .tap(2, s.at("reset"))
        .tap(4, s.at("save"))
        .expect(5, ScenarioCheck::ui("touch.arrange").exists(false))
        // Opened again (by a frame system on tick 6): a drag, then Escape.
        .touch_drag(8, 12, s.control("jump"), vec2(100.0, 270.0))
        .at(14, InputEvent::Key(Key::Escape, true))
        .at(15, InputEvent::Key(Key::Escape, false))
        .expect(15, ScenarioCheck::ui("touch.arrange").exists(false))
        .duration(17);
    let (_, report) = Game::new("reset", 960, 540)
        .headless_frames(true)
        .data_dir(&dir)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            if ctx.time.frame() == 7 {
                ctx.arrange_touch_controls();
            }
            log.borrow_mut().push((
                ctx.input.touch_layout().clone(),
                ctx.input.touch_control_center("jump").unwrap(),
                ctx.ui.find("touch.arrange.size").and_then(|id| ctx.ui.value(id)),
            ));
        })
        .run_headless_scenario(scenario, move |ctx| {
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            ctx.input.set_touch_layout(start.clone());
            ctx.arrange_touch_controls();
        });
    report.assert_passed();
    let layouts = layouts.borrow();
    assert_eq!(layouts[1].0, moved, "as the player left it");
    assert_eq!(layouts[2].0, TouchLayout::default(), "Reset: the game's placement");
    assert_eq!(layouts[2].1, s.control("jump"));
    assert_eq!(layouts[2].2, Some(1.0 / 3.0), "the Size slider back at 1");
    assert_eq!(saved(&dir), Some(TouchLayout::default()));
    assert_ne!(layouts[12].1, s.control("jump"), "dragged on the reopened screen");
    assert_eq!(layouts[16].0, TouchLayout::default(), "Escape put it back");
    assert_eq!(layouts[16].1, s.control("jump"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_panel_fits_phones_and_tablets_either_way_up_and_hides_no_control() {
    for (size, scale) in [
        ((390, 844), 1.0),
        ((844, 390), 1.0),
        ((1688, 780), 2.0),
        ((1024, 768), 1.0),
        ((1366, 1024), 1.0),
        ((2048, 1536), 2.0),
    ] {
        let s = spots(size, scale);
        let view = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
        let card = s.nodes["card"];
        assert!(
            view.contains(card.min) && view.contains(card.max),
            "{size:?}: the panel {card:?} is inside the view"
        );
        for part in PARTS {
            let r = s.nodes[part];
            assert!(
                r.min.cmpge(card.min - Vec2::splat(0.5)).all() && r.max.cmple(card.max + Vec2::splat(0.5)).all(),
                "{size:?}: {part} {r:?} is inside the panel {card:?}"
            );
        }
        for (action, &(centre, radius)) in &s.controls {
            let near = centre.clamp(card.min, card.max);
            assert!(
                near.distance(centre) >= radius - 0.5,
                "{size:?}: the panel {card:?} hides {action} at {centre} ({radius})"
            );
        }
        // Buttons a thumb can hit: at least 36 pixels tall at scale 1.
        for part in ["smaller", "bigger", "reset", "cancel", "save"] {
            assert!(s.nodes[part].size().y >= 36.0 * scale - 0.5, "{size:?}: {part}");
        }
    }
}

#[test]
fn a_press_on_the_panel_is_the_panels_even_over_a_control() {
    // A control the game put in the middle of a small view: wherever the
    // panel goes, it covers part of it.
    let size = (400, 300);
    let found = Rc::new(RefCell::new(None));
    let out = found.clone();
    let scenario = InputScenario::new().duration(3);
    let (_, report) = Game::new("shield", size.0, size.1)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            if ctx.time.frame() != 1 {
                return;
            }
            let card = ctx.ui.rect(ctx.ui.find("touch.arrange.card").unwrap()).unwrap();
            let jump = ctx.input.touch_control_center("jump").unwrap();
            // A point of the panel within the button's reach.
            let at = jump.clamp(card.min + Vec2::splat(2.0), card.max - Vec2::splat(2.0));
            ctx.input.feed(InputEvent::touch_start(1, at));
            ctx.input.feed(InputEvent::touch_move(1, at + vec2(40.0, 0.0)));
            ctx.input.feed(InputEvent::touch_end(1, at + vec2(40.0, 0.0)));
            *out.borrow_mut() = Some((at.distance(jump), jump, ctx.input.touch_control_center("jump").unwrap()));
        })
        .run_headless_scenario(scenario, move |ctx| {
            actions(ctx);
            ctx.input
                .set_touch_controls(controls().place("jump", TouchPlace::new(vec2(0.5, 0.5), Vec2::ZERO)));
            ctx.arrange_touch_controls();
        });
    report.assert_passed();
    let (distance, before, after) = found.borrow().unwrap();
    assert!(distance < 30.0, "the finger was on the button too: {distance}");
    assert_eq!(after, before, "the panel took the press; the button stayed");
}

#[test]
fn the_game_ending_edit_mode_closes_the_screen_and_keeps_the_arrangement() {
    Game::new("end", 960, 540)
        .headless_frames(true)
        .add(Phase::Frame, |ctx: &mut Ctx| match ctx.time.frame() {
            1 => {
                let jump = ctx.input.touch_control_center("jump").unwrap();
                ctx.input.feed(InputEvent::touch_start(1, jump));
                ctx.input.feed(InputEvent::touch_end(1, vec2(100.0, 100.0)));
                ctx.input.edit_touch_controls(false);
            }
            3 => {
                assert!(ctx.ui.find("touch.arrange").is_none(), "closed");
                assert_eq!(ctx.input.touch_control_center("jump"), Some(vec2(100.0, 100.0)));
            }
            _ => {}
        })
        .run_headless(4, |ctx| {
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            ctx.arrange_touch_controls();
        });
}
