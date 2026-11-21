//! UI animations add no allocations to a frame: state transitions, screens
//! coming on and going off, looping timelines and layout tweens all run on
//! storage that keeps its capacity. (Taffy's flexbox layout allocates a few
//! scratch vectors for each root it lays out, animated or not, and a frame
//! where nothing changed lays out nothing; the test compares the same frames
//! with and without animation, with as many layouts, so that cost is in both
//! counts.)

use ponko::prelude::*;
use ponko::skeleton2d::Bezier;
use ponko::ui::{Ui, UiInput, UiLayout, UiRepeat, UiScreenTransition, UiTheme, UiTimeline, UiTransition, UiTween};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// Allocations made by this thread while it is counting.
    static COUNT: Cell<Option<u64>> = const { Cell::new(None) };
}

struct Counting;

// SAFETY: every call goes to the system allocator unchanged; the counter is
// a thread-local `Cell`, touched only through `try_with` so that a thread
// being torn down still allocates safely.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get().map(|n| n + 1)));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get().map(|n| n + 1)));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// How many times this thread allocated while running `work`.
fn allocations(work: impl FnOnce()) -> u64 {
    COUNT.with(|c| c.set(Some(0)));
    work();
    COUNT.with(|c| c.replace(None)).unwrap()
}

const VIEW: Vec2 = Vec2::new(800.0, 600.0);

/// A menu of 60 buttons and a dialog that comes and goes, played for eight
/// seconds of frames: the pointer sweeps over the buttons and presses them
/// (state changes, interrupted ones too), the dialog is pushed and popped,
/// a timeline is played again. With `animated`, every kind of animation is
/// on: theme transitions with a press scale, screen transitions, ping-pong
/// timelines of scale and background on ten buttons and a looping width;
/// without, the game sets that width itself every frame, so both lay out
/// as often. Returns the allocations and the layouts of the last five
/// seconds.
fn steady_allocations(animated: bool) -> (u64, u64) {
    let mut ui = Ui::default();
    if animated {
        ui.set_theme(UiTheme {
            transition: UiTransition::new(0.15),
            press_scale: 0.95,
            ..UiTheme::dark()
        });
    } else {
        ui.set_theme(UiTheme::dark());
    }
    let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(4.0)).id();
    let mut buttons = Vec::new();
    for i in 0..60 {
        buttons.push(ui.button(root, format!("Item {i}")).layout(UiLayout::row(30.0)).id());
    }
    let dialog = ui.root().layout(UiLayout::centered(300.0, 120.0)).id();
    ui.button(dialog, "OK").layout(UiLayout::row(40.0));
    let pulse = UiTimeline::new()
        .then(UiTween::scale(1.05).seconds(0.3).with_ease(Bezier::EASE_IN_OUT))
        .with(UiTween::background(Color::rgb(0.3, 0.2, 0.1)).seconds(0.3))
        .repeat(UiRepeat::PingPong);
    if animated {
        let fade = UiScreenTransition::fade(0.2).with_offset(vec2(0.0, 20.0));
        ui.set_screen_transition(root, fade, fade);
        ui.set_screen_transition(dialog, UiScreenTransition::zoom(0.8, 0.2).with_fade(), fade);
        for &b in &buttons[..10] {
            ui.play(b, &pulse);
        }
        ui.play(
            buttons[20],
            &UiTimeline::new()
                .then(UiTween::width(300.0).seconds(0.4))
                .then(UiTween::width(200.0).seconds(0.4))
                .repeat(UiRepeat::Loop),
        );
    }
    ui.push_modeless(root);
    let second = |ui: &mut Ui, frame0: usize| {
        for f in 0..60 {
            let n = frame0 + f;
            let over = buttons[n % buttons.len()];
            let pointer = ui.rect(over).map_or(Vec2::ZERO, |r| r.center());
            let input = UiInput {
                pointer,
                pressed: n.is_multiple_of(7),
                held: n % 7 < 3,
                released: n % 7 == 3,
                dt: 1.0 / 60.0,
                ..Default::default()
            };
            if !animated {
                // What the looping width tween does, by hand.
                let width = 200.0 + (n % 60) as f32;
                ui.set_layout(buttons[20], UiLayout::row(30.0).width(width));
            }
            ui.frame(VIEW, &input);
            for _ in ui.events() {}
            match n % 30 {
                0 => ui.push_modal(dialog),
                12 => {
                    ui.pop_screen();
                }
                20 if animated => ui.play(buttons[11], &pulse),
                _ => {}
            }
        }
    };
    // Warm up: the track list and the exiting screens grow to what they
    // hold at most.
    for s in 0..3 {
        second(&mut ui, s * 60);
    }
    assert_eq!(ui.is_animating(buttons[0]), animated);
    assert_eq!(ui.is_animating(buttons[20]), animated);
    let layouts = ui.layout_passes();
    let allocated = allocations(|| {
        for s in 3..8 {
            second(&mut ui, s * 60);
        }
    });
    (allocated, ui.layout_passes() - layouts)
}

#[test]
fn animated_frames_allocate_no_more_than_still_ones() {
    let (still, still_layouts) = steady_allocations(false);
    let (animated, animated_layouts) = steady_allocations(true);
    assert_eq!(animated_layouts, still_layouts, "both lay out as often");
    assert_eq!(
        animated, still,
        "five seconds of animated frames allocated {animated} times, the same frames without animation {still}"
    );
}

#[test]
fn the_counter_sees_allocations() {
    let counted = allocations(|| {
        let t = UiTimeline::new().then(UiTween::opacity(0.0));
        assert_eq!(t.steps().len(), 1);
    });
    assert!(counted > 0);
}
