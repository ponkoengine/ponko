//! UI animation, driven headless: state transitions, screens coming on and
//! going off, tweens and timelines, read back as values over time.

use ponko::prelude::*;
use ponko::skeleton2d::Bezier;
use ponko::ui::{
    Ui, UiEvent, UiId, UiInput, UiLayout, UiRepeat, UiScreenTransition, UiTheme, UiTimeline, UiTransition, UiTween,
};

const VIEW: Vec2 = Vec2::new(640.0, 480.0);

/// A frame `dt` seconds after the last, with no input; its events.
fn tick(ui: &mut Ui, dt: f32) -> Vec<UiEvent> {
    ui.frame(
        VIEW,
        &UiInput {
            dt,
            ..Default::default()
        },
    );
    ui.events().collect()
}

/// Frames of `dt` until `seconds` have passed (the last one shorter).
fn run_for(ui: &mut Ui, seconds: f32, dt: f32) -> Vec<UiEvent> {
    let mut events = Vec::new();
    let mut left = seconds;
    while left > 1e-6 {
        let step = dt.min(left);
        events.extend(tick(ui, step));
        left -= step;
    }
    events
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn close_color(a: Color, b: Color) -> bool {
    close(a.r, b.r) && close(a.g, b.g) && close(a.b, b.b) && close(a.a, b.a)
}

/// A screen with one button, under a theme whose state changes take
/// `seconds`, linearly.
fn button_screen(seconds: f32) -> (Ui, UiId) {
    let mut ui = Ui::default();
    ui.set_theme(UiTheme {
        transition: UiTransition::new(seconds).with_ease(Bezier::LINEAR),
        ..UiTheme::dark()
    });
    let root = ui.root().layout(UiLayout::fill().padding(20.0)).id();
    let button = ui.button(root, "Play").layout(UiLayout::fixed(160.0, 40.0)).id();
    ui.push_modeless(root);
    tick(&mut ui, 0.0);
    (ui, button)
}

fn hover(ui: &mut Ui, at: Vec2, dt: f32) {
    ui.frame(
        VIEW,
        &UiInput {
            pointer: at,
            dt,
            ..Default::default()
        },
    );
}

#[test]
fn a_state_change_eases_the_fill_to_the_new_state_and_ends_exactly_on_it() {
    let (mut ui, button) = button_screen(0.2);
    let theme = *ui.theme();
    let at = ui.rect(button).unwrap().center();
    assert_eq!(ui.drawn(button).unwrap().fill, theme.control, "idle at rest");
    hover(&mut ui, at, 0.0);
    assert_eq!(
        ui.drawn(button).unwrap().fill,
        theme.control,
        "the transition starts where it was"
    );
    let mut last = 0.0;
    for frame in 1..=10 {
        hover(&mut ui, at, 0.01);
        let fill = ui.drawn(button).unwrap().fill;
        let k = (fill.r - theme.control.r) / (theme.hover.r - theme.control.r);
        assert!(close(k, frame as f32 * 0.05), "linear: {k} at frame {frame}");
        assert!(k > last);
        last = k;
    }
    hover(&mut ui, at, 0.5);
    assert_eq!(
        ui.drawn(button).unwrap().fill,
        theme.hover,
        "exactly the hover fill at the end"
    );
}

#[test]
fn an_interrupted_state_change_turns_round_from_what_is_shown() {
    let (mut ui, button) = button_screen(0.2);
    let theme = *ui.theme();
    let at = ui.rect(button).unwrap().center();
    hover(&mut ui, at, 0.0);
    hover(&mut ui, at, 0.1);
    let half = ui.drawn(button).unwrap().fill;
    assert!(close(half.r, (theme.control.r + theme.hover.r) / 2.0));
    // The pointer leaves half way: no jump, back towards the idle fill.
    hover(&mut ui, vec2(600.0, 460.0), 0.0);
    assert!(close_color(ui.drawn(button).unwrap().fill, half), "no jump");
    hover(&mut ui, vec2(600.0, 460.0), 0.1);
    let back = ui.drawn(button).unwrap().fill;
    assert!(close(back.r, (half.r + theme.control.r) / 2.0), "{back:?}");
    hover(&mut ui, vec2(600.0, 460.0), 0.2);
    assert_eq!(ui.drawn(button).unwrap().fill, theme.control);
}

#[test]
fn disabling_fades_and_pressing_scales_with_the_theme() {
    let (mut ui, button) = button_screen(0.1);
    let theme = UiTheme {
        press_scale: 0.9,
        ..*ui.theme()
    };
    ui.set_theme(theme);
    let at = ui.rect(button).unwrap().center();
    let held = UiInput {
        pointer: at,
        pressed: true,
        held: true,
        dt: 0.0,
        ..Default::default()
    };
    ui.frame(VIEW, &held);
    ui.frame(
        VIEW,
        &UiInput {
            dt: 0.05,
            ..held.clone()
        },
    );
    let pressing = ui.drawn(button).unwrap();
    assert!(
        pressing.scale < 1.0 && pressing.scale > 0.9,
        "half way: {}",
        pressing.scale
    );
    let place = ui.rect(button).unwrap();
    assert_eq!(pressing.rect.center(), place.center(), "scaled around its centre");
    assert_eq!(ui.rect(button).unwrap(), place, "layout keeps its place");
    ui.frame(
        VIEW,
        &UiInput {
            dt: 0.1,
            ..held.clone()
        },
    );
    assert!(close(ui.drawn(button).unwrap().scale, 0.9));
    ui.frame(
        VIEW,
        &UiInput {
            pointer: at,
            released: true,
            dt: 0.2,
            ..Default::default()
        },
    );
    hover(&mut ui, vec2(600.0, 460.0), 0.2);
    assert_eq!(ui.drawn(button).unwrap().scale, 1.0);
    ui.set_enabled(button, false);
    tick(&mut ui, 0.05);
    let fill = ui.drawn(button).unwrap().fill;
    assert!(fill != theme.control && fill != theme.disabled, "half way to disabled");
    tick(&mut ui, 0.1);
    assert_eq!(ui.drawn(button).unwrap().fill, theme.disabled);
}

#[test]
fn the_default_theme_changes_state_at_once() {
    let mut ui = Ui::default();
    let root = ui.root().id();
    let button = ui.button(root, "OK").layout(UiLayout::fixed(80.0, 30.0)).id();
    ui.push_modeless(root);
    tick(&mut ui, 0.0);
    let at = ui.rect(button).unwrap().center();
    hover(&mut ui, at, 0.0);
    assert_eq!(ui.drawn(button).unwrap().fill, ui.theme().hover);
}

#[test]
fn reduced_motion_makes_state_and_screen_transitions_instant() {
    let (mut ui, button) = button_screen(0.3);
    ui.set_reduced_motion(true);
    let at = ui.rect(button).unwrap().center();
    hover(&mut ui, at, 0.01);
    assert_eq!(ui.drawn(button).unwrap().fill, ui.theme().hover);
    let dialog = ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
    let fade = UiScreenTransition::fade(0.3);
    ui.set_screen_transition(dialog, fade, fade);
    ui.push_modal(dialog);
    tick(&mut ui, 0.01);
    assert_eq!(ui.drawn(dialog).unwrap().opacity, 1.0);
    ui.pop_screen();
    tick(&mut ui, 0.01);
    assert!(!ui.is_animating(dialog));
}

/// The opacity, offset and scale screen `root` is drawn with.
fn pose(ui: &Ui, root: UiId) -> (f32, Vec2, f32) {
    let drawn = ui.drawn(root).unwrap();
    let place = ui.rect(root).unwrap();
    (drawn.opacity, drawn.rect.center() - place.center(), drawn.scale)
}

#[test]
fn a_screen_fades_slides_and_scales_in_over_its_enter_transition() {
    let mut ui = Ui::default();
    let menu = ui.root().layout(UiLayout::centered(300.0, 200.0)).id();
    let play = ui.button(menu, "Play").layout(UiLayout::row(40.0)).id();
    let enter = UiScreenTransition::fade(0.4)
        .with_offset(vec2(0.0, 40.0))
        .with_scale(0.5)
        .with_ease(Bezier::LINEAR);
    ui.set_screen_transition(menu, enter, UiScreenTransition::NONE);
    ui.push_modeless(menu);
    tick(&mut ui, 0.0);
    let (opacity, offset, scale) = pose(&ui, menu);
    assert_eq!((opacity, scale), (0.0, 0.5), "away pose before any time passes");
    assert!(close(offset.y, 40.0));
    tick(&mut ui, 0.1);
    let (opacity, offset, scale) = pose(&ui, menu);
    assert!(close(opacity, 0.25) && close(offset.y, 30.0) && close(scale, 0.625));
    // A child moves and scales with its screen.
    let child = ui.drawn(play).unwrap();
    assert!(close(child.scale, 0.625) && close(child.opacity, 0.25));
    // Input uses the place: the button takes the click at once.
    assert!(ui.is_shown(play));
    tick(&mut ui, 0.3);
    assert_eq!(pose(&ui, menu), (1.0, Vec2::ZERO, 1.0));
    assert_eq!(ui.drawn(menu).unwrap().rect, ui.rect(menu).unwrap(), "exactly at rest");
}

#[test]
fn a_popped_screen_keeps_drawing_while_it_goes_and_takes_no_input() {
    let mut ui = Ui::default();
    let menu = ui.root().layout(UiLayout::fill()).id();
    let under = ui.button(menu, "Under").layout(UiLayout::fixed(200.0, 40.0)).id();
    ui.push_modeless(menu);
    let dialog = ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
    let ok = ui.button(dialog, "OK").layout(UiLayout::row(40.0)).id();
    let exit = UiScreenTransition::fade(0.2).with_ease(Bezier::LINEAR);
    ui.set_screen_transition(dialog, UiScreenTransition::NONE, exit);
    ui.push_modal(dialog);
    tick(&mut ui, 0.0);
    // Escape closes it: popped at once, drawn while it fades.
    let events = {
        ui.frame(
            VIEW,
            &UiInput {
                cancel: true,
                ..Default::default()
            },
        );
        ui.events().collect::<Vec<_>>()
    };
    assert_eq!(events, [UiEvent::ScreenPopped(dialog)]);
    assert!(!ui.is_screen(dialog) && !ui.is_shown(ok));
    assert_eq!(ui.top_screen(), Some(menu));
    tick(&mut ui, 0.1);
    assert!(close(ui.drawn(dialog).unwrap().opacity, 0.5));
    // The screen below has the input while the dialog goes.
    let at = ui.rect(under).unwrap().center();
    assert_eq!(ui.hit_test(at), Some(under));
    tick(&mut ui, 0.1);
    assert!(!ui.is_animating(dialog));
    assert_eq!(ui.drawn(dialog).unwrap().opacity, 1.0, "back at rest once gone");
    assert_eq!(ui.hit_test(at), Some(under));
}

#[test]
fn pushing_a_screen_back_while_it_goes_turns_it_round_without_a_jump() {
    let mut ui = Ui::default();
    let menu = ui.root().layout(UiLayout::centered(300.0, 200.0)).id();
    let t = UiScreenTransition::slide(vec2(-100.0, 0.0), 0.2).with_ease(Bezier::LINEAR);
    ui.set_screen_transition(menu, t, t);
    ui.push_modeless(menu);
    run_for(&mut ui, 0.3, 0.05);
    assert_eq!(pose(&ui, menu).1, Vec2::ZERO);
    ui.pop_screen();
    tick(&mut ui, 0.1);
    let half = pose(&ui, menu).1.x;
    assert!(close(half, -50.0), "{half}");
    ui.push_modeless(menu);
    tick(&mut ui, 0.0);
    assert!(close(pose(&ui, menu).1.x, half), "turns round from where it is");
    tick(&mut ui, 0.1);
    assert!(close(pose(&ui, menu).1.x, half / 2.0));
    tick(&mut ui, 0.2);
    assert_eq!(pose(&ui, menu).1, Vec2::ZERO);
    assert!(ui.is_shown(menu));
}

#[test]
fn removing_a_screen_while_it_goes_drops_it() {
    let mut ui = Ui::default();
    let menu = ui.root().id();
    ui.set_screen_transition(menu, UiScreenTransition::NONE, UiScreenTransition::fade(1.0));
    ui.push_modeless(menu);
    ui.pop_screen();
    tick(&mut ui, 0.1);
    assert!(ui.is_animating(menu));
    ui.remove(menu);
    tick(&mut ui, 0.1);
    assert!(ui.drawn(menu).is_none() && !ui.is_animating(menu));
}

#[test]
fn tweens_on_layout_and_paint_properties_reach_their_values() {
    let mut ui = Ui::default();
    let root = ui.root().layout(UiLayout::fill().gap(0.0)).id();
    let panel = ui.panel(root).layout(UiLayout::fixed(100.0, 40.0)).id();
    let after = ui.panel(root).layout(UiLayout::fixed(50.0, 40.0)).id();
    let label = ui.label(root, "Gold").id();
    ui.push_modeless(root);
    tick(&mut ui, 0.0);
    let linear = |t: UiTween| t.seconds(0.2).with_ease(Bezier::LINEAR);
    ui.animate(panel, linear(UiTween::height(80.0)));
    ui.animate(label, linear(UiTween::background(Color::RED).from(Color::BLACK)));
    ui.animate(label, linear(UiTween::text_color(Color::YELLOW)));
    let start_y = ui.rect(after).unwrap().min.y;
    tick(&mut ui, 0.1);
    assert!(close(ui.rect(panel).unwrap().size().y, 60.0), "half way in layout");
    assert!(
        close(ui.rect(after).unwrap().min.y, start_y + 20.0),
        "its neighbour makes room"
    );
    assert!(close(ui.paint(label).unwrap().background.r, 0.5));
    tick(&mut ui, 0.1);
    assert_eq!(ui.rect(panel).unwrap().size().y, 80.0);
    assert_eq!(ui.paint(label).unwrap().background, Color::RED);
    assert_eq!(ui.paint(label).unwrap().color, Color::YELLOW);
    ui.animate(panel, UiTween::width(10.0).seconds(0.0));
    tick(&mut ui, 0.0);
    assert_eq!(ui.rect(panel).unwrap().size().x, 10.0, "zero seconds: at once");
}

#[test]
fn a_new_tween_takes_over_its_property_from_where_it_is() {
    let mut ui = Ui::default();
    let root = ui.root().id();
    ui.push_modeless(root);
    ui.animate(root, UiTween::opacity(0.0).seconds(1.0).with_ease(Bezier::LINEAR));
    let events = run_for(&mut ui, 0.5, 0.1);
    assert!(events.is_empty());
    assert!(close(ui.drawn(root).unwrap().opacity, 0.5));
    ui.animate(root, UiTween::opacity(1.0).seconds(1.0).with_ease(Bezier::LINEAR));
    tick(&mut ui, 0.0);
    assert!(close(ui.drawn(root).unwrap().opacity, 0.5), "no jump");
    tick(&mut ui, 0.5);
    assert!(close(ui.drawn(root).unwrap().opacity, 0.75));
    // The first tween was taken over: only the second reports its end.
    let events = run_for(&mut ui, 1.0, 0.1);
    assert_eq!(events, [UiEvent::AnimationDone(root)]);
}

#[test]
fn a_timeline_sequences_holds_start_values_and_reports_its_end_once() {
    let mut ui = Ui::default();
    let root = ui.root().id();
    let item = ui.label(root, "Item").id();
    ui.push_modeless(root);
    let linear = |t: UiTween| t.seconds(0.2).with_ease(Bezier::LINEAR);
    let timeline = UiTimeline::new()
        .at(0.2, linear(UiTween::opacity(1.0).from(0.0)))
        .with(linear(UiTween::offset(Vec2::ZERO).from(vec2(0.0, 20.0))))
        .then(linear(UiTween::scale(2.0)));
    assert!(close(timeline.seconds(), 0.6));
    ui.play(item, &timeline);
    // Before it starts, its start values hold (a staggered entrance).
    assert_eq!(ui.drawn(item).unwrap().opacity, 1.0, "drawn only after a layout");
    tick(&mut ui, 0.0);
    assert_eq!(ui.drawn(item).unwrap().opacity, 0.0, "see-through from the first frame");
    tick(&mut ui, 0.1);
    assert_eq!(ui.drawn(item).unwrap().opacity, 0.0, "still waiting");
    tick(&mut ui, 0.2);
    let d = ui.drawn(item).unwrap();
    assert!(close(d.opacity, 0.5));
    assert!(close(d.rect.min.y - ui.rect(item).unwrap().min.y, 10.0));
    assert_eq!(d.scale, 1.0, "the scale waits for the fade");
    tick(&mut ui, 0.2);
    assert!(close(ui.drawn(item).unwrap().scale, 1.5));
    let events = run_for(&mut ui, 0.5, 0.05);
    assert_eq!(events, [UiEvent::AnimationDone(item)]);
    let d = ui.drawn(item).unwrap();
    assert_eq!((d.opacity, d.scale), (1.0, 2.0), "the values stay");
}

#[test]
fn a_property_tweened_twice_on_a_timeline_hands_over_between_them() {
    let mut ui = Ui::default();
    let root = ui.root().id();
    ui.push_modeless(root);
    let linear = |t: UiTween| t.seconds(0.5).with_ease(Bezier::LINEAR);
    let blink = UiTimeline::new()
        .then(linear(UiTween::opacity(0.0)))
        .then(linear(UiTween::opacity(1.0)));
    ui.play(root, &blink);
    tick(&mut ui, 0.25);
    assert!(close(ui.drawn(root).unwrap().opacity, 0.5));
    tick(&mut ui, 0.5);
    assert!(close(ui.drawn(root).unwrap().opacity, 0.5), "on the way back");
    tick(&mut ui, 0.25);
    assert_eq!(ui.drawn(root).unwrap().opacity, 1.0);
}

#[test]
fn looping_and_ping_pong_timelines_repeat_until_stopped() {
    let mut ui = Ui::default();
    let root = ui.root().id();
    let a = ui.label(root, "A").id();
    let b = ui.label(root, "B").id();
    let c = ui.label(root, "C").id();
    ui.push_modeless(root);
    let grow = UiTween::scale(2.0).from(1.0).seconds(1.0).with_ease(Bezier::LINEAR);
    let once = UiTimeline::new().then(grow);
    ui.play(a, &once.clone().repeat(UiRepeat::Loop));
    ui.play(b, &once.clone().repeat(UiRepeat::PingPong));
    ui.play(c, &once.repeat(UiRepeat::Times(2)));
    let events = run_for(&mut ui, 1.25, 0.25);
    assert!(events.is_empty());
    assert!(close(ui.drawn(a).unwrap().scale, 1.25), "looped back to the start");
    assert!(close(ui.drawn(b).unwrap().scale, 1.75), "on its way back");
    let events = run_for(&mut ui, 1.0, 0.25);
    assert_eq!(events, [UiEvent::AnimationDone(c)], "twice, then done");
    assert_eq!(ui.drawn(c).unwrap().scale, 2.0);
    run_for(&mut ui, 7.0, 0.25);
    assert!(ui.is_animating(a) && ui.is_animating(b));
    ui.stop_animations(a);
    ui.stop_animations(b);
    let held = ui.drawn(a).unwrap().scale;
    let events = run_for(&mut ui, 0.5, 0.25);
    assert!(events.is_empty() && !ui.is_animating(a));
    assert_eq!(ui.drawn(a).unwrap().scale, held);
}

/// Values at the same moments come out the same at any frame rate.
#[test]
fn animations_are_frame_rate_independent() {
    fn sample(fps: f32) -> Vec<(f32, f32, Color)> {
        let mut ui = Ui::default();
        ui.set_theme(UiTheme {
            transition: UiTransition::new(0.3),
            ..UiTheme::dark()
        });
        let menu = ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
        let button = ui.button(menu, "Go").layout(UiLayout::fixed(160.0, 40.0)).id();
        ui.set_screen_transition(
            menu,
            UiScreenTransition::zoom(0.5, 0.6).with_ease(Bezier::EASE_IN_OUT),
            UiScreenTransition::NONE,
        );
        ui.push_modeless(menu);
        ui.layout(VIEW);
        // Everything starts in one frame at time zero: input lands on
        // frames, so a state change waits for the next one.
        let at = ui.rect(button).unwrap().center();
        ui.animate(button, UiTween::opacity(0.2).seconds(0.5).with_ease(Bezier::BACK_OUT));
        hover(&mut ui, at, 0.0);
        let mut samples = Vec::new();
        // 0.1 s apart, each reached in whole frames.
        let per = (0.1 * fps).round() as u32;
        for _ in 0..8 {
            for _ in 0..per {
                hover(&mut ui, at, 0.1 / per as f32);
            }
            samples.push((
                ui.drawn(menu).unwrap().scale,
                ui.drawn(button).unwrap().opacity,
                ui.drawn(button).unwrap().fill,
            ));
        }
        samples
    }
    let reference = sample(30.0);
    for fps in [60.0, 144.0, 240.0] {
        for (i, (a, b)) in sample(fps).iter().zip(&reference).enumerate() {
            assert!(
                close(a.0, b.0) && close(a.1, b.1),
                "{fps} fps, sample {i}: {a:?} vs {b:?}"
            );
            assert!(close_color(a.2, b.2), "{fps} fps, sample {i}");
        }
    }
}

#[test]
fn no_time_holds_every_animation() {
    let (mut ui, button) = button_screen(0.2);
    ui.animate(button, UiTween::opacity(0.0).seconds(0.2));
    tick(&mut ui, 0.05);
    let before = ui.drawn(button).unwrap();
    for _ in 0..100 {
        tick(&mut ui, 0.0);
    }
    assert_eq!(ui.drawn(button).unwrap(), before);
}

#[test]
fn a_removed_node_stops_its_animations_and_sends_no_event() {
    let mut ui = Ui::default();
    let root = ui.root().id();
    let toast = ui.label(root, "Saved").id();
    ui.push_modeless(root);
    ui.animate(toast, UiTween::opacity(0.0).seconds(0.2));
    tick(&mut ui, 0.1);
    ui.remove(toast);
    let events = run_for(&mut ui, 0.5, 0.1);
    assert!(events.is_empty() && !ui.is_animating(toast));
}
