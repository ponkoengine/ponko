//! A regression check for the steady-state touch input path: fingers,
//! gestures bound in the action map and on-screen controls.
//!
//! One test per binary, and a thread-local counter: only the thread running
//! the game counts, so other threads in the process can't disturb the
//! zero-allocation assertion.
#[allow(unused_imports)]
use ponko::actions::InputEvent;
#[allow(unused_imports)]
use ponko::input::{Gamepad, GamepadAxis, GamepadStick};
use ponko::prelude::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation and deallocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn touches_gestures_and_onscreen_controls_allocate_nothing_once_warm() {
    use ponko::input::GestureAxis;
    use ponko::ui::touch::{TouchControls, TouchShow};
    let mut tick = 0u64;
    Game::new("allocations", 960, 540)
        .sim_system(move |ctx: &mut Ctx| {
            // The first cycle warms the buffers (the list of fingers, the
            // gestures of a frame); the next ones are counted.
            if tick == 40 {
                ALLOCS.set(0);
                TRACK.set(true);
            }
            let stick = ctx.input.touch_control_center("move").unwrap();
            let jump = ctx.input.touch_control_center("jump").unwrap();
            let at = vec2(480.0, 270.0);
            let t = (tick % 40) as f32;
            match tick % 40 {
                0 => {
                    ctx.input.feed(InputEvent::touch_start(1, stick));
                    ctx.input.feed(InputEvent::touch_start(2, jump));
                    ctx.input.feed(InputEvent::touch_start(3, at));
                }
                1..=9 => {
                    ctx.input.feed(InputEvent::touch_move(1, stick + vec2(t * 10.0, 0.0)));
                    ctx.input.feed(InputEvent::touch_move(3, at + vec2(0.0, t * 12.0)));
                }
                10 => {
                    ctx.input.feed(InputEvent::touch_end(3, at));
                    ctx.input.feed(InputEvent::touch_start(4, at));
                    ctx.input.feed(InputEvent::touch_start(5, at + vec2(100.0, 0.0)));
                }
                11..=19 => ctx
                    .input
                    .feed(InputEvent::touch_move(5, at + vec2(100.0 + t * 8.0, t * 4.0))),
                20 => {
                    for id in [1, 2, 4, 5] {
                        ctx.input.feed(InputEvent::touch_end(id, at));
                    }
                }
                21 | 23 => {
                    ctx.input.feed(InputEvent::touch_start(6, at));
                    ctx.input.feed(InputEvent::touch_end(6, at));
                }
                _ => {}
            }
            for name in ["jump", "dash", "zoom", "look", "move"] {
                std::hint::black_box(ctx.input.action_down(name));
                std::hint::black_box(ctx.input.action_axis(name));
                std::hint::black_box(ctx.input.action_axis2(name));
            }
            std::hint::black_box(ctx.input.gesture_events().len());
            std::hint::black_box(ctx.input.pressed_this_frame().count());
            tick += 1;
            if tick == 120 {
                TRACK.set(false);
                assert_eq!(ALLOCS.get(), 0);
            }
        })
        .run_headless(120, |ctx| {
            let mut map = ActionMap::default();
            map.set("jump", Action::Button(vec![ButtonBinding::Gesture(Gesture::Tap)]));
            map.set("dash", Action::Button(vec![ButtonBinding::Gesture(Gesture::DoubleTap)]));
            map.set(
                "zoom",
                Action::Axis(vec![AxisBinding::Gesture {
                    axis: GestureAxis::Pinch,
                    scale: 1.0,
                }]),
            );
            map.set("look", Action::Axis2(vec![Axis2Binding::Drag { scale: 1.0 }]));
            map.set("move", Action::Axis2(vec![]));
            ctx.input.set_actions(map);
            ctx.input.set_touch_controls(
                TouchControls::new()
                    .stick("move")
                    .button("jump", "A")
                    .show(TouchShow::Always),
            );
        });
}
