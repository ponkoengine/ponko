//! The on-screen controls' look and place: a game restyles and places
//! them, the player moves and resizes them, and their arrangement is saved
//! with the settings. Also the mouse events desktop systems make up for a
//! finger. Headless frames, fingers and the mouse fed as events.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::ui::touch::{TouchControls, TouchLayout, TouchPlace, TouchShow, TouchStyle};

const VIEW: Vec2 = Vec2::new(960.0, 540.0);

fn actions(ctx: &mut Ctx) {
    ctx.input
        .set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
    ctx.input
        .set_action("fire", Action::Button(vec![ButtonBinding::Key(Key::F)]));
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
        .show(TouchShow::Always)
}

/// A node's centre and half its width, as laid out.
fn disc(ctx: &Ctx, name: &str) -> (Vec2, f32) {
    let rect = ctx.ui.rect(ctx.ui.find(name).unwrap()).unwrap();
    ((rect.min + rect.max) / 2.0, (rect.max.x - rect.min.x) / 2.0)
}

#[test]
fn a_game_restyles_and_places_its_controls() {
    let red = TouchStyle {
        button: Color::rgba(1.0, 0.1, 0.1, 0.5),
        held: Color::rgba(1.0, 0.3, 0.3, 0.9),
        label: Color::rgba(1.0, 1.0, 0.0, 1.0),
        size: 1.5,
        ..TouchStyle::default()
    };
    let checked = Rc::new(RefCell::new(0));
    let done = checked.clone();
    Game::new("styled", 960, 540)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            if ctx.time.frame() < 2 {
                return;
            }
            let plain = ctx.input.touch_control_center("jump").unwrap();
            let fire = ctx.input.touch_control_center("fire").unwrap();
            assert_eq!(fire, vec2(120.0, 120.0), "where the game put it");
            let (centre, radius) = disc(ctx, "touch.fire");
            assert!(centre.distance(fire) < 0.5, "drawn there: {centre}");
            let (_, plain_radius) = disc(ctx, "touch.jump");
            // Pixel-snapped rectangles.
            assert!(
                (radius / plain_radius - 1.5).abs() < 0.03,
                "half as big again: {radius} {plain_radius}"
            );
            let paint = ctx.ui.paint(ctx.ui.find("touch.fire").unwrap()).unwrap();
            assert_eq!(paint.color, red.button, "its own color");
            let jump = ctx.ui.paint(ctx.ui.find("touch.jump").unwrap()).unwrap();
            assert_eq!(jump.color, TouchStyle::default().button, "the others keep the set's");
            assert!(plain.x > fire.x);
            *done.borrow_mut() += 1;
        })
        .run_headless(3, |ctx| {
            actions(ctx);
            ctx.input.set_touch_controls(
                controls()
                    .control_style("fire", red)
                    .place("fire", TouchPlace::new(vec2(0.0, 0.0), vec2(120.0, 120.0))),
            );
        });
    assert!(*checked.borrow() > 0);
}

#[test]
fn a_held_restyled_button_shows_its_held_color() {
    let style = TouchStyle {
        button: Color::rgba(0.0, 0.0, 1.0, 0.4),
        held: Color::rgba(0.0, 1.0, 0.0, 0.8),
        ..TouchStyle::default()
    };
    let colors = Rc::new(RefCell::new(Vec::new()));
    let log = colors.clone();
    Game::new("styled", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, |ctx: &mut Ctx| {
            if ctx.time.frame() == 2 {
                let at = ctx.input.touch_control_center("jump").unwrap();
                ctx.input.feed(InputEvent::touch_start(1, at));
            }
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let jump = ctx.ui.find("touch.jump").unwrap();
            log.borrow_mut().push(ctx.ui.paint(jump).unwrap().color);
        })
        .run_headless(4, |ctx| {
            actions(ctx);
            ctx.input.set_touch_controls(controls().style(style));
        });
    let colors = colors.borrow();
    assert_eq!(colors[1], style.button);
    assert_eq!(colors[3], style.held, "held from the frame after the finger went down");
}

#[test]
fn players_move_controls_by_dragging_them_in_edit_mode_with_a_finger_or_the_mouse() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let clicks = Rc::new(RefCell::new(0));
    let click_log = clicks.clone();
    Game::new("arrange", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, |ctx: &mut Ctx| {
            let jump = ctx.input.touch_control_center("jump").unwrap();
            let stick = ctx.input.touch_control_center("move").unwrap();
            match ctx.time.frame() {
                // The settings screen's "arrange controls": a see-through
                // screen with a Done button, over the controls.
                1 => ctx.input.edit_touch_controls(true),
                // A finger drags the jump button to the top middle.
                2 => ctx.input.feed(InputEvent::touch_start(1, jump)),
                3 => ctx.input.feed(InputEvent::touch_move(1, vec2(480.0, 100.0))),
                4 => ctx.input.feed(InputEvent::touch_end(1, vec2(480.0, 100.0))),
                // The mouse drags the stick a bit up.
                5 => {
                    ctx.input.feed(InputEvent::MouseMove(stick));
                    ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
                }
                6 => ctx.input.feed(InputEvent::MouseMove(stick - vec2(0.0, 150.0))),
                7 => ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false)),
                8 => ctx.input.edit_touch_controls(false),
                _ => {}
            }
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            for event in ctx.ui.events() {
                if event.clicked().is_some() {
                    *click_log.borrow_mut() += 1;
                }
            }
            log.borrow_mut().push((
                ctx.input.action_down("jump"),
                ctx.input.touch_control_center("jump").unwrap(),
                ctx.input.touch_control_center("move").unwrap(),
                ctx.ui.find("touch.controls").is_some_and(|root| ctx.ui.is_hud(root)),
            ));
        })
        .run_headless(10, |ctx| {
            actions(ctx);
            ctx.input.set_touch_controls(controls());
            // Off a touch screen, under a screen: shown only while arranged.
            ctx.input.set_touch_controls_show(TouchShow::Auto);
            let root = ctx
                .ui
                .root()
                .layout(UiLayout::fill())
                .background(Color::TRANSPARENT)
                .id();
            ctx.ui
                .button(root, "Done")
                .name("done")
                .layout(UiLayout::fixed(100.0, 40.0));
            ctx.ui.push_modeless(root);
        });
    let seen = seen.borrow();
    let (_, jump_before, stick_before, shown_before) = seen[0];
    assert!(!shown_before, "a desktop with a screen open: not shown");
    assert!(seen[2].3, "shown while the player arranges them");
    assert!(seen.iter().all(|s| !s.0), "moving the button never pressed it");
    assert_ne!(jump_before, vec2(480.0, 100.0));
    assert_eq!(seen[3].1, vec2(480.0, 100.0), "it follows the finger");
    assert_eq!(seen[7].2, stick_before - vec2(0.0, 150.0), "the mouse moved the stick");
    assert!(!seen[9].3, "hidden again when done");
    assert_eq!(*clicks.borrow(), 0, "the UI under the controls was not clicked");
}

#[test]
fn a_moved_control_is_kept_in_the_players_layout_anchored_to_its_nearest_corner() {
    Game::new("arrange", 960, 540).run_headless(0, |ctx| {
        actions(ctx);
        ctx.input.set_touch_controls(controls());
        ctx.input.edit_touch_controls(true);
        let jump = ctx.input.touch_control_center("jump").unwrap();
        ctx.input.feed(InputEvent::touch_start(1, jump));
        ctx.input.feed(InputEvent::touch_move(1, vec2(900.0, 60.0)));
        ctx.input.feed(InputEvent::touch_end(1, vec2(900.0, 60.0)));
        let place = ctx.input.touch_layout().adjusted("jump").place.unwrap();
        assert_eq!(place.anchor, vec2(1.0, 0.0), "the top right corner");
        assert_eq!(place.offset, vec2(-60.0, 60.0));
        // A control dragged off the screen stays on it.
        let fire = ctx.input.touch_control_center("fire").unwrap();
        ctx.input.feed(InputEvent::touch_start(2, fire));
        ctx.input.feed(InputEvent::touch_end(2, vec2(2000.0, -500.0)));
        let fire = ctx.input.touch_control_center("fire").unwrap();
        assert!(fire.x < VIEW.x && fire.y > 0.0, "{fire}");
        // A new set of controls from the game keeps the player's places.
        ctx.input.set_touch_controls(controls());
        assert_eq!(ctx.input.touch_control_center("jump"), Some(vec2(900.0, 60.0)));
    });
}

#[test]
fn the_players_layout_is_saved_with_the_settings_and_restored() {
    let dir = std::env::temp_dir().join(format!("ponko-touch-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Game::new("arrange", 960, 540).data_dir(&dir).run_headless(0, |ctx| {
        actions(ctx);
        ctx.input.set_touch_controls(controls());
        let mut layout = ctx.input.touch_layout().clone();
        layout.adjust("jump").place = Some(TouchPlace::new(vec2(0.5, 0.0), vec2(0.0, 80.0)));
        layout.adjust("fire").size = 2.0;
        layout.opacity = 0.5;
        ctx.input.set_touch_layout(layout);
        ctx.input.save_touch_layout(&mut ctx.storage).unwrap();
    });
    let paints = Rc::new(RefCell::new(None));
    let log = paints.clone();
    Game::new("arrange", 960, 540)
        .data_dir(&dir)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let jump = ctx.ui.paint(ctx.ui.find("touch.jump").unwrap()).unwrap().color;
            *log.borrow_mut() = Some((jump, ctx.input.touch_control_center("jump").unwrap()));
        })
        .run_headless(1, |ctx| {
            actions(ctx);
            ctx.input.restore_touch_layout(&ctx.storage).unwrap();
            ctx.input.set_touch_controls(controls());
            assert_eq!(ctx.input.touch_control_center("jump"), Some(vec2(480.0, 80.0)));
            assert_eq!(ctx.input.touch_layout().adjusted("fire").size, 2.0);
        });
    let (color, _) = paints.borrow().unwrap();
    assert!(
        (color.a - TouchStyle::default().button.a * 0.5).abs() < 1e-6,
        "half as opaque: {color:?}"
    );
    // No file: nothing changes. A broken file: an error, the layout kept.
    std::fs::remove_dir_all(&dir).unwrap();
    Game::new("arrange", 960, 540).data_dir(&dir).run_headless(0, |ctx| {
        ctx.input.restore_touch_layout(&ctx.storage).unwrap();
        assert_eq!(*ctx.input.touch_layout(), TouchLayout::default());
        ctx.storage.write(TouchLayout::FILE, b"{ not json").unwrap();
        assert!(ctx.input.restore_touch_layout(&ctx.storage).is_err());
        assert_eq!(*ctx.input.touch_layout(), TouchLayout::default());
    });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_press_the_system_makes_up_for_a_finger_does_not_click_twice() {
    // Windows sends a mouse press and release after a finger's (Linux on
    // X11 moves the cursor with the first finger). The finger clicks once.
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let log = clicks.clone();
    let button = vec2(480.0, 270.0);
    Game::new("emulated", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, move |ctx: &mut Ctx| match ctx.time.frame() {
            2 => {
                ctx.input.feed(InputEvent::touch_start(1, button));
                ctx.input.feed(InputEvent::MouseMove(button));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
            }
            3 => {
                ctx.input.feed(InputEvent::touch_end(1, button));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));
            }
            4 => {
                // The mouse made up for the lift, a frame late.
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));
                assert!(!ctx.input.mouse_pressed(MouseButton::Left), "not the mouse's");
            }
            // A real click, later, at the same place, is the mouse's.
            7 => {
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));
                assert!(ctx.input.mouse_pressed(MouseButton::Left));
            }
            _ => {}
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let play = ctx.ui.find("play").unwrap();
            for event in ctx.ui.events() {
                if event == UiEvent::Clicked(play) {
                    log.borrow_mut().push(ctx.time.frame());
                }
            }
        })
        .run_headless(9, |ctx| {
            let root = ctx.ui.root().layout(UiLayout::fill()).id();
            ctx.ui
                .button(root, "Play")
                .name("play")
                .layout(UiLayout::anchored(0.25, 0.25, 0.5, 0.5));
            ctx.ui.push_modeless(root);
        });
    assert_eq!(
        *clicks.borrow(),
        [3, 7],
        "the finger's tap and the real click, once each"
    );
}

#[test]
fn the_cursor_following_a_finger_leaves_the_finger_the_uis_pointer() {
    // X11 moves the cursor to the first finger before each of its events:
    // a finger that slides a little on a button still clicks it.
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let log = clicks.clone();
    let at = vec2(400.0, 270.0);
    Game::new("cursor follows", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, move |ctx: &mut Ctx| match ctx.time.frame() {
            2 => {
                ctx.input.feed(InputEvent::MouseMove(at));
                ctx.input.feed(InputEvent::touch_start(1, at));
            }
            3 => {
                ctx.input.feed(InputEvent::MouseMove(at + vec2(60.0, 0.0)));
                ctx.input.feed(InputEvent::touch_move(1, at + vec2(60.0, 0.0)));
            }
            4 => {
                ctx.input.feed(InputEvent::MouseMove(at + vec2(90.0, 0.0)));
                ctx.input.feed(InputEvent::touch_end(1, at + vec2(90.0, 0.0)));
            }
            _ => {}
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let play = ctx.ui.find("play").unwrap();
            for event in ctx.ui.events() {
                if event == UiEvent::Clicked(play) {
                    log.borrow_mut().push(ctx.time.frame());
                }
            }
        })
        .run_headless(6, |ctx| {
            let root = ctx.ui.root().layout(UiLayout::fill()).id();
            ctx.ui
                .button(root, "Play")
                .name("play")
                .layout(UiLayout::anchored(0.25, 0.25, 0.5, 0.5));
            ctx.ui.push_modeless(root);
        });
    assert_eq!(*clicks.borrow(), [4]);
}

#[test]
fn a_controls_editor_reads_back_and_re_anchors_places() {
    let red = TouchStyle {
        button: Color::rgba(1.0, 0.1, 0.1, 0.5),
        ..TouchStyle::default()
    };
    let fire = TouchPlace::new(vec2(1.0, 1.0), vec2(-80.0, -80.0));
    let controls = TouchControls::new()
        .stick("move")
        .button("fire", "B")
        .button("jump", "A")
        .control_style("fire", red)
        .place("fire", fire);
    // The editor shows each control as the game set it up.
    assert_eq!(controls.style_of("fire").button, red.button);
    assert_eq!(controls.style_of("jump").button, TouchStyle::default().button);
    assert_eq!(controls.place_of("fire"), Some(fire));
    assert_eq!(controls.place_of("jump"), None, "laid out by the engine");
    // The player drops the fire button at (100, 400) on a 960 x 540 view:
    // it is kept anchored to the bottom-left corner, so it stays put on
    // another screen size.
    let view = vec2(960.0, 540.0);
    let dropped = TouchPlace::nearest_corner(vec2(100.0, 400.0), view);
    assert_eq!(dropped.anchor, vec2(0.0, 1.0));
    assert_eq!(dropped.centre(view), vec2(100.0, 400.0));
    assert_eq!(dropped.centre(vec2(1280.0, 720.0)), vec2(100.0, 580.0));
}
