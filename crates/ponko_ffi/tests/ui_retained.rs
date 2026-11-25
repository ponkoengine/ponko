//! The retained UI through the C API: building a tree, layout, input, events,
//! screens, ids and errors, in headless runs (no window).
use ponko_ffi::*;
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

type Ctx = *mut PonkoCtx;

struct Scenario {
    body: unsafe fn(Ctx),
    failure: Option<String>,
}

unsafe extern "C" fn trampoline(ctx: Ctx, user: *mut c_void) {
    let scenario = unsafe { &mut *user.cast::<Scenario>() };
    let body = scenario.body;
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| unsafe { body(ctx) })) {
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()));
        scenario.failure = Some(message.unwrap_or_else(|| "panicked".into()));
    }
}

/// Runs `body` once as a sim system of a headless game and reports its failure.
fn run(body: unsafe fn(Ctx)) {
    let mut scenario = Scenario { body, failure: None };
    unsafe {
        let game = ponko_game_new(c"ui".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_sim_system(
                game,
                c"s".as_ptr(),
                1,
                Some(trampoline),
                (&mut scenario as *mut Scenario).cast()
            ),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 1, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    if let Some(failure) = scenario.failure {
        panic!("{failure}");
    }
}

fn px(value: f32) -> PonkoUiLength {
    PonkoUiLength {
        kind: PonkoUiLengthKind::PONKO_UI_LENGTH_POINTS as u32,
        value,
    }
}
fn pct(value: f32) -> PonkoUiLength {
    PonkoUiLength {
        kind: PonkoUiLengthKind::PONKO_UI_LENGTH_PERCENT as u32,
        value,
    }
}
fn all(length: PonkoUiLength) -> PonkoUiSides {
    PonkoUiSides {
        top: length,
        right: length,
        bottom: length,
        left: length,
    }
}
fn size(width: f32, height: f32) -> PonkoUiLayout {
    PonkoUiLayout {
        width: px(width),
        height: px(height),
        flex_shrink: 0.0,
        ..ponko_ui_layout_default()
    }
}
fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}
const SCREEN: PonkoVec2 = PonkoVec2 { x: 800.0, y: 600.0 };

unsafe fn id_of(status: impl FnOnce(*mut u64) -> PonkoStatus) -> u64 {
    let mut id = PONKO_UI_NONE;
    assert_eq!(status(&mut id), PONKO_OK, "{}", last_error());
    assert_ne!(id, PONKO_UI_NONE);
    id
}
fn last_error() -> String {
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}
unsafe fn panel(ctx: Ctx, parent: u64, layout: &PonkoUiLayout) -> u64 {
    unsafe { id_of(|out| ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, parent, layout, out)) }
}
unsafe fn button(ctx: Ctx, parent: u64, text: &CStr, layout: &PonkoUiLayout) -> u64 {
    unsafe { id_of(|out| ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, parent, text.as_ptr(), layout, out)) }
}
unsafe fn rect(ctx: Ctx, id: u64) -> PonkoRect {
    let mut r = PonkoRect::default();
    assert_eq!(unsafe { ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, id, &mut r) }, PONKO_OK);
    r
}
unsafe fn frame(ctx: Ctx, input: PonkoUiInput) {
    assert_eq!(
        unsafe { ponko_ctx_ui_frame(ctx, PONKO_UI_SCREEN, SCREEN, &input) },
        PONKO_OK,
        "{}",
        last_error()
    );
}
unsafe fn click(ctx: Ctx, at: PonkoVec2) {
    unsafe {
        let mut input = ponko_ui_input_default();
        input.pointer = at;
        input.pressed = true;
        input.held = true;
        frame(ctx, input);
        input.pressed = false;
        input.held = false;
        input.released = true;
        frame(ctx, input);
    }
}
unsafe fn events(ctx: Ctx) -> Vec<(PonkoUiEventKind, u64)> {
    let mut all = Vec::new();
    loop {
        let mut event = PonkoUiEvent {
            kind: PonkoUiEventKind::PONKO_UI_EVENT_CLICKED,
            id: 0,
        };
        match unsafe { ponko_ctx_ui_next_event(ctx, PONKO_UI_SCREEN, &mut event) } {
            PONKO_OK => all.push((event.kind, event.id)),
            PONKO_NOT_FOUND => return all,
            other => panic!("{other:?}: {}", last_error()),
        }
    }
}
unsafe fn text_of(ctx: Ctx, id: u64) -> String {
    let mut len = 0;
    assert_eq!(
        unsafe { ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, id, null_mut(), 0, &mut len) },
        PONKO_OK
    );
    let mut buffer = vec![0u8; len + 1];
    assert_eq!(
        unsafe {
            ponko_ctx_ui_text(
                ctx,
                PONKO_UI_SCREEN,
                id,
                buffer.as_mut_ptr().cast::<c_char>(),
                buffer.len(),
                &mut len,
            )
        },
        PONKO_OK
    );
    assert_eq!(buffer[len], 0);
    String::from_utf8(buffer[..len].to_vec()).unwrap()
}
unsafe fn value_of(ctx: Ctx, id: u64) -> f32 {
    let mut value = -1.0;
    assert_eq!(
        unsafe { ponko_ctx_ui_value(ctx, PONKO_UI_SCREEN, id, &mut value) },
        PONKO_OK
    );
    value
}

// ---- widgets, layout and input ---------------------------------------------------

unsafe fn widgets_lay_out_and_react(ctx: Ctx) {
    unsafe {
        let root = panel(
            ctx,
            PONKO_UI_NONE,
            &PonkoUiLayout {
                flex_direction: PonkoUiFlexDirection::PONKO_UI_FLEX_COLUMN as u32,
                padding: all(px(10.0)),
                gap: PonkoUiGap {
                    column: px(5.0),
                    row: px(5.0),
                },
                ..size(400.0, 300.0)
            },
        );
        let label =
            id_of(|out| ponko_ctx_ui_add_label(ctx, PONKO_UI_SCREEN, root, c"Title".as_ptr(), &size(100.0, 20.0), out));
        let go = button(ctx, root, c"Go", &size(200.0, 40.0));
        let toggle = id_of(|out| {
            ponko_ctx_ui_add_toggle(
                ctx,
                PONKO_UI_SCREEN,
                root,
                c"Gift".as_ptr(),
                false,
                &size(100.0, 20.0),
                out,
            )
        });
        let slider = id_of(|out| ponko_ctx_ui_add_slider(ctx, PONKO_UI_SCREEN, root, 0.25, &size(200.0, 20.0), out));
        let input = id_of(|out| {
            ponko_ctx_ui_add_text_input(ctx, PONKO_UI_SCREEN, root, c"hi".as_ptr(), &size(200.0, 30.0), out)
        });
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, root, false), PONKO_OK);
        frame(ctx, ponko_ui_input_default());

        // A padded column with gaps: each child sits below the last.
        let at = |id| {
            let r = rect(ctx, id);
            (r.min.x, r.min.y, r.max.x, r.max.y)
        };
        assert_eq!(at(root), (0.0, 0.0, 400.0, 300.0));
        assert_eq!(at(label), (10.0, 10.0, 110.0, 30.0));
        assert_eq!(at(go), (10.0, 35.0, 210.0, 75.0));
        assert_eq!(at(toggle), (10.0, 80.0, 110.0, 100.0));
        assert_eq!(at(slider), (10.0, 105.0, 210.0, 125.0));
        assert_eq!(at(input), (10.0, 130.0, 210.0, 160.0));
        assert_eq!(text_of(ctx, label), "Title");
        assert_eq!(value_of(ctx, slider), 0.25);

        let mut hit = 0;
        assert_eq!(
            ponko_ctx_ui_hit_test(ctx, PONKO_UI_SCREEN, v2(50.0, 50.0), &mut hit),
            PONKO_OK
        );
        assert_eq!(hit, go);
        assert_eq!(
            ponko_ctx_ui_hit_test(ctx, PONKO_UI_SCREEN, v2(700.0, 500.0), &mut hit),
            PONKO_OK
        );
        assert_eq!(hit, PONKO_UI_NONE);

        // A button click: press, then release over it.
        click(ctx, v2(50.0, 50.0));
        assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_CLICKED, go)]);
        assert!(events(ctx).is_empty(), "events are taken once");
        let mut focus = 0;
        assert_eq!(ponko_ctx_ui_focus(ctx, PONKO_UI_SCREEN, &mut focus), PONKO_OK);
        assert_eq!(focus, go);

        // Disabled controls ignore clicks, and come back when enabled.
        assert_eq!(ponko_ctx_ui_set_enabled(ctx, PONKO_UI_SCREEN, go, false), PONKO_OK);
        click(ctx, v2(50.0, 50.0));
        assert!(events(ctx).is_empty());
        assert_eq!(ponko_ctx_ui_set_enabled(ctx, PONKO_UI_SCREEN, go, true), PONKO_OK);

        // A toggle flips; a slider follows the pointer.
        click(ctx, v2(20.0, 90.0));
        assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_CHANGED, toggle)]);
        assert_eq!(value_of(ctx, toggle), 1.0);
        // The slider changes while the pointer is down on it, so read its
        // events in that same frame (each frame starts with an empty queue).
        let mut press = ponko_ui_input_default();
        press.pointer = v2(110.0, 115.0);
        press.pressed = true;
        press.held = true;
        frame(ctx, press);
        assert_eq!(value_of(ctx, slider), 0.5);
        assert!(events(ctx).contains(&(PonkoUiEventKind::PONKO_UI_EVENT_CHANGED, slider)));
        frame(
            ctx,
            PonkoUiInput {
                released: true,
                pointer: press.pointer,
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(ponko_ctx_ui_set_value(ctx, PONKO_UI_SCREEN, slider, 7.0), PONKO_OK);
        assert_eq!(value_of(ctx, slider), 1.0, "values are clamped");
        assert!(events(ctx).is_empty(), "setters queue no events");

        // Typing into a focused input, then editing and submitting.
        click(ctx, v2(205.0, 145.0));
        events(ctx);
        let text = c"!?";
        frame(
            ctx,
            PonkoUiInput {
                text: text.as_ptr(),
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(text_of(ctx, input), "hi!?");
        assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_CHANGED, input)]);
        frame(
            ctx,
            PonkoUiInput {
                backspace: true,
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(text_of(ctx, input), "hi!");
        events(ctx);
        frame(
            ctx,
            PonkoUiInput {
                confirm: true,
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_SUBMITTED, input)]);
        assert_eq!(
            ponko_ctx_ui_set_text(ctx, PONKO_UI_SCREEN, input, c"a\tb".as_ptr()),
            PONKO_OK
        );
        assert_eq!(text_of(ctx, input), "ab", "inputs drop control characters");

        // Reading text: a short buffer gets the length and an empty string.
        let mut small = [7 as c_char; 2];
        let mut len = 0;
        assert_eq!(
            ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, label, small.as_mut_ptr(), small.len(), &mut len),
            PONKO_OK
        );
        assert_eq!((len, small[0]), (5, 0));
        assert_eq!(
            ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, label, small.as_mut_ptr(), 0, &mut len),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, label, null_mut(), 1, &mut len),
            PONKO_INVALID_ARGUMENT
        );
        // The outputs are checked first: a null output is an invalid
        // argument even for a node or panel that doesn't exist (not "not
        // found"), and nothing is read.
        for (target, node) in [
            (PONKO_UI_SCREEN, 999_999),
            (PONKO_UI_SCREEN, label),
            (u64::MAX - 1, label),
        ] {
            assert_eq!(
                ponko_ctx_ui_text(ctx, target, node, small.as_mut_ptr(), small.len(), null_mut()),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(
                ponko_ctx_ui_name(ctx, target, node, null_mut(), 4, &mut len),
                PONKO_INVALID_ARGUMENT
            );
        }
        assert_eq!(
            ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, 999_999, small.as_mut_ptr(), small.len(), &mut len),
            PONKO_NOT_FOUND,
            "with good outputs, the missing node is the error"
        );

        // The tab key walks the controls in order.
        frame(
            ctx,
            PonkoUiInput {
                next: true,
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(ponko_ctx_ui_focus(ctx, PONKO_UI_SCREEN, &mut focus), PONKO_OK);
        assert_ne!(focus, PONKO_UI_NONE);
    }
}
#[test]
fn widgets_lay_out_and_react_to_input() {
    run(widgets_lay_out_and_react);
}

// ---- screens --------------------------------------------------------------------

unsafe fn screens_stack_and_pop(ctx: Ctx) {
    unsafe {
        let base = panel(ctx, PONKO_UI_NONE, &size(800.0, 600.0));
        let base_button = button(ctx, base, c"Open", &size(100.0, 40.0));
        let dialog = panel(ctx, PONKO_UI_NONE, &size(300.0, 200.0));
        let child = button(ctx, dialog, c"Ok", &size(80.0, 30.0));
        let mut top = 0;
        assert_eq!(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &mut top), PONKO_OK);
        assert_eq!(top, PONKO_UI_NONE);
        let (mut keyboard, mut pointer) = (true, true);
        assert_eq!(ponko_ctx_ui_wants_keyboard(ctx, &mut keyboard), PONKO_OK);
        assert!(!keyboard);

        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, base, false), PONKO_OK);
        assert_eq!(
            ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, base, false),
            PONKO_INVALID_ARGUMENT,
            "twice"
        );
        assert_eq!(
            ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, base_button, false),
            PONKO_INVALID_ARGUMENT,
            "has a parent"
        );
        assert_eq!(
            ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, 4242, false),
            PONKO_NOT_FOUND
        );
        frame(ctx, ponko_ui_input_default());
        assert_eq!(ponko_ctx_ui_wants_keyboard(ctx, &mut keyboard), PONKO_OK);
        assert!(keyboard);
        assert_eq!(ponko_ctx_ui_wants_pointer(ctx, PONKO_UI_SCREEN, &mut pointer), PONKO_OK);
        assert!(pointer, "the pointer rests over the base screen");

        // A modal dialog owns input: the button under it is out of reach.
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, dialog, true), PONKO_OK);
        assert_eq!(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &mut top), PONKO_OK);
        assert_eq!(top, dialog);
        click(ctx, v2(10.0, 10.0));
        assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_CLICKED, child)]);
        click(ctx, v2(500.0, 500.0));
        assert!(events(ctx).is_empty());

        // Cancel pops it and says so; the pop consumes that frame's input.
        frame(
            ctx,
            PonkoUiInput {
                cancel: true,
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_SCREEN_POPPED, dialog)]);
        assert_eq!(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &mut top), PONKO_OK);
        assert_eq!(top, base);

        // Popped screens stay usable.
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, dialog, false), PONKO_OK);
        let mut popped = 0;
        assert_eq!(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &mut popped), PONKO_OK);
        assert_eq!(popped, dialog);
        assert_eq!(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &mut popped), PONKO_OK);
        assert_eq!(popped, base);
        assert_eq!(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &mut popped), PONKO_OK);
        assert_eq!(popped, PONKO_UI_NONE, "nothing left to pop");

        // Removing a screen's root drops it from the stack.
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, base, false), PONKO_OK);
        assert_eq!(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, base), PONKO_OK);
        assert_eq!(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &mut top), PONKO_OK);
        assert_eq!(top, PONKO_UI_NONE);
    }
}
#[test]
fn screens_stack_modal_and_pop() {
    run(screens_stack_and_pop);
}

unsafe fn passes(ctx: Ctx) -> u64 {
    let mut n = u64::MAX;
    assert_eq!(
        unsafe { ponko_ctx_ui_layout_passes(ctx, PONKO_UI_SCREEN, &mut n) },
        PONKO_OK
    );
    n
}

unsafe fn still_screen_costs_no_layout(ctx: Ctx) {
    unsafe {
        let root = panel(ctx, PONKO_UI_NONE, &size(400.0, 300.0));
        let score = id_of(|out| {
            ponko_ctx_ui_add_label(
                ctx,
                PONKO_UI_SCREEN,
                root,
                c"Score: 0".as_ptr(),
                &size(200.0, 30.0),
                out,
            )
        });
        assert_eq!(ponko_ctx_ui_show_hud(ctx, PONKO_UI_SCREEN, root), PONKO_OK);
        frame(ctx, ponko_ui_input_default());
        let laid_out = passes(ctx);
        assert!(laid_out >= 1, "the first frame lays the tree out");
        for _ in 0..10 {
            // A HUD system writing the text the label already shows.
            assert_eq!(
                ponko_ctx_ui_set_text(ctx, PONKO_UI_SCREEN, score, c"Score: 0".as_ptr()),
                PONKO_OK
            );
            frame(ctx, ponko_ui_input_default());
        }
        assert_eq!(passes(ctx), laid_out, "still frames cost no layout");
        assert_eq!(
            ponko_ctx_ui_set_text(ctx, PONKO_UI_SCREEN, score, c"Score: 10".as_ptr()),
            PONKO_OK
        );
        frame(ctx, ponko_ui_input_default());
        assert_eq!(passes(ctx), laid_out + 1, "a changed text lays out once");
        assert_eq!(
            ponko_ctx_ui_layout_passes(ctx, PONKO_UI_SCREEN, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    }
}
#[test]
fn a_still_screen_costs_no_layout() {
    run(still_screen_costs_no_layout);
}

unsafe fn screen_modes(ctx: Ctx) {
    unsafe {
        use PonkoUiScreenMode::*;
        let base = panel(ctx, PONKO_UI_NONE, &size(100.0, 100.0));
        let dialog = panel(ctx, PONKO_UI_NONE, &size(300.0, 200.0));
        let child = button(ctx, dialog, c"Ok", &size(80.0, 30.0));
        let mut pointer = true;
        let mut mode = u32::MAX;
        assert_eq!(
            ponko_ctx_ui_screen_mode(ctx, PONKO_UI_SCREEN, dialog, &mut mode),
            PONKO_NOT_FOUND
        );
        assert_eq!(mode, u32::MAX);
        assert_eq!(
            ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, base, PONKO_UI_SCREEN_MODE_MODELESS as u32),
            PONKO_OK
        );
        // A modeless dialog leaves the pointer outside it to the game.
        assert_eq!(
            ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, dialog, PONKO_UI_SCREEN_MODE_MODELESS as u32),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_screen_mode(ctx, PONKO_UI_SCREEN, dialog, &mut mode),
            PONKO_OK
        );
        assert_eq!(mode, PONKO_UI_SCREEN_MODE_MODELESS as u32);
        frame(
            ctx,
            PonkoUiInput {
                pointer: v2(500.0, 500.0),
                ..ponko_ui_input_default()
            },
        );
        assert_eq!(ponko_ctx_ui_wants_pointer(ctx, PONKO_UI_SCREEN, &mut pointer), PONKO_OK);
        assert!(!pointer, "modeless: nothing under the pointer");
        let mut popped = 0;
        assert_eq!(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &mut popped), PONKO_OK);

        // Modal owns it, exactly as `ponko_ctx_ui_push_screen(.., true)` does.
        for push in [
            |ctx, d| ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, d, PONKO_UI_SCREEN_MODE_MODAL as u32),
            |ctx, d| ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, d, true),
        ] as [unsafe fn(Ctx, u64) -> PonkoStatus; 2]
        {
            assert_eq!(push(ctx, dialog), PONKO_OK);
            assert_eq!(
                ponko_ctx_ui_screen_mode(ctx, PONKO_UI_SCREEN, dialog, &mut mode),
                PONKO_OK
            );
            assert_eq!(mode, PONKO_UI_SCREEN_MODE_MODAL as u32);
            assert_eq!(
                ponko_ctx_ui_screen_mode(ctx, PONKO_UI_SCREEN, dialog, null_mut()),
                PONKO_INVALID_ARGUMENT
            );
            frame(
                ctx,
                PonkoUiInput {
                    pointer: v2(500.0, 500.0),
                    ..ponko_ui_input_default()
                },
            );
            assert_eq!(ponko_ctx_ui_wants_pointer(ctx, PONKO_UI_SCREEN, &mut pointer), PONKO_OK);
            assert!(pointer, "modal: the pointer outside is the dialog's");
            click(ctx, v2(10.0, 10.0));
            assert_eq!(events(ctx), [(PonkoUiEventKind::PONKO_UI_EVENT_CLICKED, child)]);
            assert_eq!(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &mut popped), PONKO_OK);
            assert_eq!(popped, dialog);
        }

        // An unknown mode is an error, not undefined behavior, and pushes nothing.
        assert_eq!(
            ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, dialog, 7),
            PONKO_INVALID_ARGUMENT
        );
        let mut top = 0;
        assert_eq!(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &mut top), PONKO_OK);
        assert_eq!(top, base);
    }
}
#[test]
fn screen_modes_match_the_old_bool() {
    run(screen_modes);
}

// ---- grid, absolute positions, scrolling -------------------------------------------

unsafe fn grid_absolute_and_scroll(ctx: Ctx) {
    unsafe {
        let fr = PonkoUiTrack {
            kind: PonkoUiTrackKind::PONKO_UI_TRACK_FR as u32,
            value: 1.0,
        };
        let columns = [fr, fr, fr];
        let rows = [PonkoUiTrack {
            kind: PonkoUiTrackKind::PONKO_UI_TRACK_POINTS as u32,
            value: 40.0,
        }];
        let grid = panel(
            ctx,
            PONKO_UI_NONE,
            &PonkoUiLayout {
                display: PonkoUiDisplay::PONKO_UI_DISPLAY_GRID as u32,
                grid_columns: columns.as_ptr(),
                grid_column_count: columns.len(),
                grid_auto_rows: rows.as_ptr(),
                grid_auto_row_count: rows.len(),
                ..size(300.0, 200.0)
            },
        );
        let cells: Vec<u64> = (0..3).map(|_| panel(ctx, grid, &ponko_ui_layout_default())).collect();
        let wide = panel(
            ctx,
            grid,
            &PonkoUiLayout {
                grid_column_end: PonkoUiGridLine {
                    kind: PonkoUiGridLineKind::PONKO_UI_GRID_SPAN as u32,
                    value: 2,
                },
                ..ponko_ui_layout_default()
            },
        );
        // The corner is a relative box with an absolutely placed child.
        let corner = panel(
            ctx,
            PONKO_UI_NONE,
            &PonkoUiLayout {
                position: PonkoUiPosition::PONKO_UI_POSITION_RELATIVE as u32,
                ..size(200.0, 100.0)
            },
        );
        let pinned = panel(
            ctx,
            corner,
            &PonkoUiLayout {
                position: PonkoUiPosition::PONKO_UI_POSITION_ABSOLUTE as u32,
                inset: PonkoUiSides {
                    left: px(10.0),
                    top: px(20.0),
                    ..all(PonkoUiLength {
                        kind: PonkoUiLengthKind::PONKO_UI_LENGTH_AUTO as u32,
                        value: 0.0,
                    })
                },
                width: pct(0.5),
                ..size(0.0, 30.0)
            },
        );
        // Scrolling: tall content in a short area.
        let scroll_area = {
            let id =
                id_of(|out| ponko_ctx_ui_add_scroll(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &size(100.0, 100.0), out));
            panel(ctx, id, &size(100.0, 400.0));
            id
        };
        for root in [grid, corner, scroll_area] {
            assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, root, false), PONKO_OK);
        }
        frame(ctx, ponko_ui_input_default());

        let at = |id| {
            let r = rect(ctx, id);
            (r.min.x, r.min.y, r.max.x, r.max.y)
        };
        assert_eq!(at(cells[0]), (0.0, 0.0, 100.0, 40.0));
        assert_eq!(at(cells[2]), (200.0, 0.0, 300.0, 40.0));
        assert_eq!(
            at(wide),
            (0.0, 40.0, 200.0, 80.0),
            "starts a new row and spans two columns"
        );
        assert_eq!(
            at(pinned),
            (10.0, 20.0, 110.0, 50.0),
            "half of the parent wide, inset from its corner"
        );

        assert_eq!(
            ponko_ctx_ui_set_scroll(ctx, PONKO_UI_SCREEN, scroll_area, v2(0.0, 50.0)),
            PONKO_OK
        );
        frame(ctx, ponko_ui_input_default());
        let mut offset = PonkoVec2::default();
        assert_eq!(
            ponko_ctx_ui_scroll(ctx, PONKO_UI_SCREEN, scroll_area, &mut offset),
            PONKO_OK
        );
        assert_eq!(offset, v2(0.0, 50.0));
        assert_eq!(
            ponko_ctx_ui_set_scroll(ctx, PONKO_UI_SCREEN, scroll_area, v2(0.0, 9999.0)),
            PONKO_OK
        );
        frame(ctx, ponko_ui_input_default());
        assert_eq!(
            ponko_ctx_ui_scroll(ctx, PONKO_UI_SCREEN, scroll_area, &mut offset),
            PONKO_OK
        );
        assert_eq!(offset, v2(0.0, 300.0), "clamped to the content");

        // Changing a layout later moves things on the next layout.
        let moved = PonkoUiLayout {
            margin: PonkoUiSides {
                left: px(25.0),
                ..all(px(0.0))
            },
            ..size(100.0, 40.0)
        };
        assert_eq!(
            ponko_ctx_ui_set_layout(ctx, PONKO_UI_SCREEN, cells[0], &moved),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
        assert_eq!(rect(ctx, cells[0]).min.x, 25.0);
        assert_eq!(
            ponko_ctx_ui_set_layout(ctx, PONKO_UI_SCREEN, cells[0], null()),
            PONKO_OK,
            "null is the default"
        );
        assert_eq!(ponko_ctx_ui_set_scale(ctx, PONKO_UI_SCREEN, 2.0), PONKO_OK);
        assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
        assert_eq!(rect(ctx, grid).max.x, 600.0, "the user scale applies to geometry");
    }
}
#[test]
fn grid_absolute_and_scroll_layouts() {
    run(grid_absolute_and_scroll);
}

// ---- paint -----------------------------------------------------------------------

unsafe fn paint_and_images(ctx: Ctx) {
    unsafe {
        let node = button(ctx, PONKO_UI_NONE, c"Look", &ponko_ui_layout_default());
        let mut paint = ponko_ui_paint_default();
        assert_eq!(paint.align, PonkoUiTextAlign::PONKO_UI_TEXT_ALIGN_START as u32);
        paint.align = PonkoUiTextAlign::PONKO_UI_TEXT_ALIGN_CENTER as u32;
        paint.wrap = true;
        paint.border_width = 2.0;
        paint.font_size = 24.0;
        assert_eq!(ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, node, &paint), PONKO_OK);
        assert_eq!(
            ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, node, null()),
            PONKO_OK,
            "null is the default"
        );

        paint.align = 9;
        assert_eq!(
            ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, node, &paint),
            PONKO_INVALID_ARGUMENT
        );
        paint = ponko_ui_paint_default();
        paint.font_size = f32::NAN;
        assert_eq!(
            ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, node, &paint),
            PONKO_INVALID_ARGUMENT
        );
        paint = ponko_ui_paint_default();
        paint.border_width = -1.0;
        assert_eq!(
            ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, node, &paint),
            PONKO_INVALID_ARGUMENT
        );
        paint = ponko_ui_paint_default();
        paint.font = PonkoFont { id: 99 };
        assert_eq!(
            ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, node, &paint),
            PONKO_INVALID_ARGUMENT,
            "{}",
            last_error()
        );

        let mut texture = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(
            ponko_ctx_create_texture(ctx, 2, 2, [255u8; 16].as_ptr(), 16, &mut texture),
            PONKO_OK
        );
        let image =
            id_of(|out| ponko_ctx_ui_add_image(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, texture, &size(64.0, 64.0), out));
        let mut textured = ponko_ui_paint_default();
        textured.has_texture = true;
        textured.texture = texture;
        textured.slice = 1.0;
        assert_eq!(ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, image, &textured), PONKO_OK);
    }
}
#[test]
fn paint_and_images_are_checked() {
    run(paint_and_images);
}

// ---- ids and errors ------------------------------------------------------------------

unsafe fn stale_ids_and_bad_arguments(ctx: Ctx) {
    unsafe {
        let root = panel(ctx, PONKO_UI_NONE, &ponko_ui_layout_default());
        let child = button(ctx, root, c"x", &ponko_ui_layout_default());
        let grandchild = id_of(|out| ponko_ctx_ui_add_label(ctx, PONKO_UI_SCREEN, child, c"y".as_ptr(), null(), out));
        let mut sink;

        // A failed add leaves no node behind: the next id follows the last one.
        let next = {
            let probe = panel(ctx, root, &ponko_ui_layout_default());
            probe + 1
        };
        let bad = PonkoUiLayout {
            display: 99,
            ..ponko_ui_layout_default()
        };
        sink = 123;
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, &bad, &mut sink),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(sink, 123, "the output stays untouched on failure");
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, 9999, null(), &mut sink),
            PONKO_NOT_FOUND
        );
        assert_eq!(panel(ctx, root, &ponko_ui_layout_default()), next);
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, null(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_add_label(
                ctx,
                PONKO_UI_SCREEN,
                root,
                [0xffu8, 0].as_ptr().cast(),
                null(),
                &mut sink
            ),
            PONKO_INVALID_ARGUMENT,
            "text must be UTF-8"
        );

        // Removing a node takes its whole subtree and its events with it.
        assert_eq!(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, child), PONKO_OK);
        let mut r = PonkoRect::default();
        for gone in [child, grandchild] {
            assert_eq!(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, gone, &mut r), PONKO_NOT_FOUND);
            assert_eq!(
                ponko_ctx_ui_set_text(ctx, PONKO_UI_SCREEN, gone, c"z".as_ptr()),
                PONKO_NOT_FOUND
            );
            assert_eq!(
                ponko_ctx_ui_set_layout(ctx, PONKO_UI_SCREEN, gone, null()),
                PONKO_NOT_FOUND
            );
            assert_eq!(
                ponko_ctx_ui_set_enabled(ctx, PONKO_UI_SCREEN, gone, true),
                PONKO_NOT_FOUND
            );
            assert_eq!(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, gone), PONKO_NOT_FOUND);
        }
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, child, null(), &mut sink),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &mut r),
            PONKO_NOT_FOUND
        );

        // Null pointers are errors, never crashes.
        assert_eq!(
            ponko_ctx_ui_value(ctx, PONKO_UI_SCREEN, root, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, root, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_next_event(ctx, PONKO_UI_SCREEN, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_ui_insert_text(ctx, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_ui_next_event(
                ctx,
                PONKO_UI_SCREEN,
                &mut PonkoUiEvent {
                    kind: PonkoUiEventKind::PONKO_UI_EVENT_CLICKED,
                    id: 0,
                }
            ),
            PONKO_NOT_FOUND
        );

        // A bad track array is rejected before anything changes.
        let tracks = PonkoUiLayout {
            grid_columns: null(),
            grid_column_count: 3,
            ..ponko_ui_layout_default()
        };
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, &tracks, &mut sink),
            PONKO_INVALID_ARGUMENT
        );
    }
    assert_eq!(
        unsafe { ponko_ctx_ui_set_scale(null_mut(), PONKO_UI_SCREEN, 1.0) },
        PONKO_INVALID_ARGUMENT
    );
}
#[test]
fn stale_ids_and_bad_arguments_fail_cleanly() {
    run(stale_ids_and_bad_arguments);
}

unsafe fn text_typed_through_the_context(ctx: Ctx) {
    unsafe {
        let input = id_of(|out| {
            ponko_ctx_ui_add_text_input(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, null(), &size(200.0, 30.0), out)
        });
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, input, false), PONKO_OK);
        // Frames without input text read none; the context's own typed text
        // feeds the engine's frame, not this explicit one.
        assert_eq!(ponko_ctx_ui_insert_text(ctx, c"abc".as_ptr()), PONKO_OK);
        assert_eq!(text_of(ctx, input), "");
    }
}
#[test]
fn inserted_text_waits_for_the_engine_frame() {
    run(text_typed_through_the_context);
}

#[test]
fn named_areas_lines_and_repeat_tracks_layout_and_validate() {
    unsafe fn scenario(ctx: Ctx) {
        unsafe {
            let tracks = [PonkoUiTrack {
                kind: PonkoUiTrackKind::PONKO_UI_TRACK_FR as u32,
                value: 1.0,
            }];
            let components = [PonkoUiTemplateTrack {
                track: tracks[0],
                repetition: 1,
                count: 2,
                tracks: tracks.as_ptr(),
                track_count: 1,
                line_names: null(),
                line_name_count: 0,
            }];
            let mut layout = size(300.0, 200.0);
            layout.display = PonkoUiDisplay::PONKO_UI_DISPLAY_GRID as u32;
            layout.grid_template_columns = components.as_ptr();
            layout.grid_template_column_count = 1;
            let rows = [
                PonkoUiTrack { kind: 1, value: 40.0 },
                PonkoUiTrack { kind: 1, value: 160.0 },
            ];
            layout.grid_rows = rows.as_ptr();
            layout.grid_row_count = 2;
            let root = panel(ctx, PONKO_UI_NONE, &layout);
            let area_rows = [c"header header".as_ptr(), c"side main".as_ptr()];
            assert_eq!(
                ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, root, area_rows.as_ptr(), 2),
                PONKO_OK
            );
            let child = panel(ctx, root, &ponko_ui_layout_default());
            assert_eq!(
                ponko_ctx_ui_set_grid_area(ctx, PONKO_UI_SCREEN, child, c"main".as_ptr()),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, root, false), PONKO_OK);
            assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
            let r = rect(ctx, child);
            assert_eq!((r.min.x, r.min.y, r.max.x, r.max.y), (150.0, 40.0, 300.0, 200.0));
            for bad in [
                [c"a a".as_ptr(), c"a b".as_ptr()],
                [c"a".as_ptr(), c"a b".as_ptr()],
                [c"a !".as_ptr(), c"b b".as_ptr()],
            ] {
                assert_eq!(
                    ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, root, bad.as_ptr(), 2),
                    PONKO_INVALID_ARGUMENT
                );
                assert!(!last_error().is_empty());
            }
            // Invalid edits preserve both layout and placement.
            assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
            assert_eq!(rect(ctx, child).min.x, 150.0);
            let dots = [c"... .".as_ptr(), c". main".as_ptr()];
            assert_eq!(
                ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, root, dots.as_ptr(), 2),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
            assert_eq!((rect(ctx, child).min.x, rect(ctx, child).min.y), (150.0, 40.0));
            assert_eq!(
                ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, root, null(), 0),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_area(ctx, PONKO_UI_SCREEN, child, c"".as_ptr()),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, root, area_rows.as_ptr(), 2),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_area(ctx, PONKO_UI_SCREEN, child, c"main".as_ptr()),
                PONKO_OK
            );
            let names = [c"left".as_ptr(), c"right".as_ptr()];
            assert_eq!(
                ponko_ctx_ui_set_grid_line_names(ctx, PONKO_UI_SCREEN, root, 0, names.as_ptr(), 2),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_named_line(ctx, PONKO_UI_SCREEN, child, 0, false, c"left".as_ptr(), 1),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_named_line(ctx, PONKO_UI_SCREEN, child, 0, true, c"right".as_ptr(), 1),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
            assert_eq!((rect(ctx, child).min.x, rect(ctx, child).max.x), (0.0, 300.0));
            assert_eq!(
                ponko_ctx_ui_set_grid_line_names(ctx, PONKO_UI_SCREEN, root, 2, names.as_ptr(), 2),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, root, null(), 1),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_area(ctx, PONKO_UI_SCREEN, child, null()),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_area(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, c"main".as_ptr()),
                PONKO_NOT_FOUND
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_area(ctx, 0, child, c"main".as_ptr()),
                PONKO_INVALID_ARGUMENT
            );
            for mode in [2, 3] {
                let fixed = [PonkoUiTrack { kind: 1, value: 100.0 }];
                let auto = [PonkoUiTemplateTrack {
                    track: fixed[0],
                    repetition: mode,
                    count: 0,
                    tracks: fixed.as_ptr(),
                    track_count: 1,
                    line_names: null(),
                    line_name_count: 0,
                }];
                layout.grid_template_columns = auto.as_ptr();
                let root = panel(ctx, PONKO_UI_NONE, &layout);
                let item = panel(ctx, root, &ponko_ui_layout_default());
                let mut third = ponko_ui_layout_default();
                third.grid_column_start = PonkoUiGridLine { kind: 1, value: 3 };
                let last = panel(ctx, root, &third);
                assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, root, false), PONKO_OK);
                assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
                assert_eq!(rect(ctx, item).max.x, 100.0);
                assert_eq!(rect(ctx, last).min.x, if mode == 2 { 200.0 } else { 100.0 });
                let invalid = [PonkoUiTemplateTrack {
                    tracks: tracks.as_ptr(),
                    ..auto[0]
                }];
                layout.grid_template_columns = invalid.as_ptr();
                let mut out = PONKO_UI_NONE;
                assert_eq!(
                    ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &layout, &mut out),
                    PONKO_INVALID_ARGUMENT
                );
                assert_eq!(out, PONKO_UI_NONE);
            }
            let repeat_names = [c"begin".as_ptr(), c"finish".as_ptr()];
            let named_repeat = [PonkoUiTemplateTrack {
                line_names: repeat_names.as_ptr(),
                line_name_count: 2,
                ..components[0]
            }];
            layout.grid_template_columns = named_repeat.as_ptr();
            let named_root = panel(ctx, PONKO_UI_NONE, &layout);
            let named_item = panel(ctx, named_root, &ponko_ui_layout_default());
            assert_eq!(
                ponko_ctx_ui_set_grid_named_line(ctx, PONKO_UI_SCREEN, named_item, 0, false, c"begin".as_ptr(), 2),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_set_grid_named_line(ctx, PONKO_UI_SCREEN, named_item, 0, true, c"finish".as_ptr(), -1),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, named_root, false),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
            assert_eq!(
                (rect(ctx, named_item).min.x, rect(ctx, named_item).max.x),
                (150.0, 300.0)
            );
            let invalid_names = [PonkoUiTemplateTrack {
                line_name_count: 1,
                ..named_repeat[0]
            }];
            layout.grid_template_columns = invalid_names.as_ptr();
            let mut unused = PONKO_UI_NONE;
            assert_eq!(
                ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &layout, &mut unused),
                PONKO_INVALID_ARGUMENT
            );
            for (mode, count, len) in [(1, 0, 1), (1, 65536, 1), (4, 1, 1), (1, 2, 0)] {
                let bad = [PonkoUiTemplateTrack {
                    repetition: mode,
                    count,
                    track_count: len,
                    ..components[0]
                }];
                layout.grid_template_columns = bad.as_ptr();
                let mut out = PONKO_UI_NONE;
                assert_eq!(
                    ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &layout, &mut out),
                    PONKO_INVALID_ARGUMENT
                );
            }
        }
    }
    run(scenario);
}

// ---- presets, theme, titles, names ------------------------------------------------

unsafe fn name_of(ctx: Ctx, id: u64) -> String {
    let mut len = 0;
    assert_eq!(
        unsafe { ponko_ctx_ui_name(ctx, PONKO_UI_SCREEN, id, null_mut(), 0, &mut len) },
        PONKO_OK
    );
    let mut buffer = vec![0u8; len + 1];
    let status = unsafe {
        ponko_ctx_ui_name(
            ctx,
            PONKO_UI_SCREEN,
            id,
            buffer.as_mut_ptr().cast::<c_char>(),
            buffer.len(),
            &mut len,
        )
    };
    assert_eq!(status, PONKO_OK);
    String::from_utf8(buffer[..len].to_vec()).unwrap()
}
unsafe fn find(ctx: Ctx, name: &CStr) -> u64 {
    let mut id = 0;
    assert_eq!(
        unsafe { ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, name.as_ptr(), &mut id) },
        PONKO_OK
    );
    id
}
unsafe fn paint_of(ctx: Ctx, id: u64) -> PonkoUiPaint {
    let mut paint = ponko_ui_paint_default();
    assert_eq!(
        unsafe { ponko_ctx_ui_paint(ctx, PONKO_UI_SCREEN, id, &mut paint) },
        PONKO_OK
    );
    paint
}

unsafe fn presets_theme_titles_and_names(ctx: Ctx) {
    unsafe {
        // A screen laid out with presets only.
        let mut root_layout = ponko_ui_layout_fill();
        root_layout.padding = all(px(10.0));
        let root = panel(ctx, PONKO_UI_NONE, &root_layout);
        let title = id_of(|out| {
            ponko_ctx_ui_add_title(
                ctx,
                PONKO_UI_SCREEN,
                root,
                c"Shop".as_ptr(),
                &ponko_ui_layout_row(40.0),
                out,
            )
        });
        let bar = panel(ctx, root, &ponko_ui_layout_row(50.0));
        let side = panel(ctx, bar, &ponko_ui_layout_column(120.0));
        let buy = button(ctx, bar, c"Buy", &ponko_ui_layout_fixed(100.0, 30.0));
        let map = panel(ctx, root, &ponko_ui_layout_anchored(0.75, 0.0, 0.25, 0.25));
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, root, false), PONKO_OK);
        let dialog = panel(ctx, PONKO_UI_NONE, &ponko_ui_layout_centered(200.0, 100.0));
        assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
        let r = |id| rect(ctx, id);
        assert_eq!((r(title).min.x, r(title).min.y, r(title).max.x), (10.0, 10.0, 790.0));
        assert_eq!((r(bar).min.y, r(bar).max.y), (50.0, 100.0));
        assert_eq!((r(side).min.x, r(side).max.x, r(side).max.y), (10.0, 130.0, 100.0));
        assert_eq!((r(buy).min.x, r(buy).max.x - r(buy).min.x), (130.0, 100.0));
        assert_eq!((r(map).min.x, r(map).max.x, r(map).max.y), (600.0, 800.0, 150.0));
        assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, dialog, true), PONKO_OK);
        assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, SCREEN), PONKO_OK);
        assert_eq!((r(dialog).min.x, r(dialog).min.y), (300.0, 250.0));
        let mut popped = 0;
        assert_eq!(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &mut popped), PONKO_OK);

        // The theme: titles in its title paint, buttons in its control
        // paint, and a node's own paint kept across a change.
        let mut theme = ponko_ui_theme_default();
        assert_eq!(ponko_ctx_ui_theme(ctx, PONKO_UI_SCREEN, &mut theme), PONKO_OK);
        assert_eq!(theme, ponko_ui_theme_default());
        assert_eq!(paint_of(ctx, title).font_size, theme.title_size);
        let green = PonkoColor {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        };
        let mut own = paint_of(ctx, buy);
        own.color = green;
        assert_eq!(ponko_ctx_ui_set_paint(ctx, PONKO_UI_SCREEN, buy, &own), PONKO_OK);
        theme.title = green;
        theme.title_size = 30.0;
        theme.control = theme.accent;
        theme.text = theme.accent;
        assert_eq!(ponko_ctx_ui_set_theme(ctx, PONKO_UI_SCREEN, &theme), PONKO_OK);
        let mut read = ponko_ui_theme_default();
        assert_eq!(ponko_ctx_ui_theme(ctx, PONKO_UI_SCREEN, &mut read), PONKO_OK);
        assert_eq!(read, theme);
        let (title_paint, buy_paint) = (paint_of(ctx, title), paint_of(ctx, buy));
        assert_eq!((title_paint.color, title_paint.font_size), (green, 30.0));
        assert_eq!(buy_paint.background, theme.accent, "follows the theme");
        assert_eq!(buy_paint.color, green, "set on the node: kept");
        // The immediate widgets follow the same theme.
        let mut style = ponko_ui_style_default();
        assert_eq!(ponko_ctx_ui_style(ctx, &mut style), PONKO_OK);
        assert_eq!((style.button, style.text), (theme.accent, theme.accent));
        // Bad themes change nothing.
        let mut bad = theme;
        bad.title_size = -2.0;
        assert_eq!(
            ponko_ctx_ui_set_theme(ctx, PONKO_UI_SCREEN, &bad),
            PONKO_INVALID_ARGUMENT
        );
        bad = theme;
        bad.font = PonkoFont { id: 999 };
        assert_eq!(
            ponko_ctx_ui_set_theme(ctx, PONKO_UI_SCREEN, &bad),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_set_theme(ctx, PONKO_UI_SCREEN, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_ui_theme(ctx, PONKO_UI_SCREEN, &mut read), PONKO_OK);
        assert_eq!(read, theme);

        // Names: found, moved, cleared, and gone with their node.
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, buy, c"shop.buy".as_ptr()),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, map, c"map".as_ptr()),
            PONKO_OK
        );
        assert_eq!(
            (find(ctx, c"shop.buy"), name_of(ctx, buy)),
            (buy, "shop.buy".to_string())
        );
        assert_eq!(find(ctx, c"shop.sell"), PONKO_UI_NONE);
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, side, c"shop.buy".as_ptr()),
            PONKO_OK
        );
        assert_eq!((find(ctx, c"shop.buy"), name_of(ctx, buy)), (side, String::new()));
        assert_eq!(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, side, null()), PONKO_OK);
        assert_eq!(find(ctx, c"shop.buy"), PONKO_UI_NONE);
        assert_eq!(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, map), PONKO_OK);
        assert_eq!(find(ctx, c"map"), PONKO_UI_NONE);
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, map, c"map".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, null(), &mut popped),
            PONKO_INVALID_ARGUMENT
        );
        let mut found = 0;
        assert_eq!(
            ponko_ctx_ui_try_find(ctx, PONKO_UI_SCREEN, c"map".as_ptr(), &mut found),
            PONKO_NOT_FOUND
        );
        assert!(
            last_error().starts_with(r#"unknown UI node "map" (known: "#),
            "{}",
            last_error()
        );
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, buy, c"map".as_ptr()),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_try_find(ctx, PONKO_UI_SCREEN, c"map".as_ptr(), &mut found),
            PONKO_OK
        );
        assert_eq!(found, buy);
        assert_eq!(
            ponko_ctx_ui_try_find(ctx, PONKO_UI_SCREEN, c"map".as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );

        // take_click: the last click once, other events dropped.
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, buy, c"buy".as_ptr()),
            PONKO_OK
        );
        let mut clicked = 0;
        assert_eq!(ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, &mut clicked), PONKO_OK);
        assert_eq!(clicked, PONKO_UI_NONE);
        let at = rect(ctx, buy);
        click(ctx, v2((at.min.x + at.max.x) * 0.5, (at.min.y + at.max.y) * 0.5));
        assert_eq!(ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, &mut clicked), PONKO_OK);
        assert_eq!(name_of(ctx, clicked), "buy");
        assert_eq!(ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, &mut clicked), PONKO_OK);
        assert_eq!(clicked, PONKO_UI_NONE);
        assert!(events(ctx).is_empty());
        assert_eq!(
            ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, null_mut()),
            PONKO_INVALID_ARGUMENT
        );

        // A title needs a parent.
        let mut id = 0;
        assert_eq!(
            ponko_ctx_ui_add_title(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, c"x".as_ptr(), null(), &mut id),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn presets_theme_titles_names_and_take_click_work_from_c() {
    run(presets_theme_titles_and_names);
}

/// Screen roots with `PONKO_UI_LENGTH_AUTO` margins share out the window's
/// free space, as children do: centered, in the middle, or pushed aside.
unsafe fn auto_margins_on_screen_roots(ctx: Ctx) {
    unsafe {
        let automatic = PonkoUiLength {
            kind: PonkoUiLengthKind::PONKO_UI_LENGTH_AUTO as u32,
            value: 0.0,
        };
        let mut across = ponko_ui_layout_fixed(200.0, 100.0);
        across.margin.left = automatic;
        across.margin.right = automatic;
        across.margin.top = px(40.0);
        let mut middle = ponko_ui_layout_fixed(200.0, 100.0);
        middle.margin = all(automatic);
        let mut pushed = ponko_ui_layout_fixed(200.0, 100.0);
        pushed.margin.left = automatic;
        let roots = [across, middle, pushed].map(|layout| panel(ctx, PONKO_UI_NONE, &layout));
        for root in roots {
            assert_eq!(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, root, false), PONKO_OK);
        }
        let corners = |window: PonkoVec2| {
            assert_eq!(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, window), PONKO_OK);
            roots.map(|root| {
                let r = rect(ctx, root);
                (r.min.x, r.min.y, r.max.x - r.min.x)
            })
        };
        assert_eq!(
            corners(v2(800.0, 600.0)),
            [(300.0, 40.0, 200.0), (300.0, 250.0, 200.0), (600.0, 0.0, 200.0)]
        );
        assert_eq!(
            corners(v2(1000.0, 800.0)),
            [(400.0, 40.0, 200.0), (400.0, 350.0, 200.0), (800.0, 0.0, 200.0)],
            "a bigger window moves them with it"
        );
    }
}

#[test]
fn auto_margins_place_screen_roots_from_c() {
    run(auto_margins_on_screen_roots);
}
