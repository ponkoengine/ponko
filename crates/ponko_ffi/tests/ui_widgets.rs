//! The retained UI's widget set through the C API: every builder, the
//! widgets' state, the extended events, theme and input, the immediate
//! toggle, slider and progress bar, and the accessibility state, in headless
//! runs (no window).
use ponko_ffi::*;
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{null, null_mut};

use PonkoStatus::*;
use PonkoUiEventKind::*;

type Ctx = *mut PonkoCtx;

const SCREEN: PonkoVec2 = PonkoVec2 { x: 800.0, y: 600.0 };
const S: u64 = PONKO_UI_SCREEN;

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

fn last_error() -> String {
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

#[track_caller]
fn ok(status: PonkoStatus) {
    assert_eq!(status, PONKO_OK, "{}", last_error());
}

unsafe fn id_of(status: impl FnOnce(*mut u64) -> PonkoStatus) -> u64 {
    let mut id = PONKO_UI_NONE;
    ok(status(&mut id));
    id
}

fn fixed(width: f32, height: f32) -> PonkoUiLayout {
    ponko_ui_layout_fixed(width, height)
}

unsafe fn root(ctx: Ctx) -> u64 {
    unsafe {
        let layout = ponko_ui_layout_column(800.0);
        let root = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, PONKO_UI_NONE, &layout, o));
        ok(ponko_ctx_ui_push_screen(ctx, S, root, false));
        root
    }
}

fn input_ex() -> PonkoUiInputEx {
    let mut i: PonkoUiInputEx = unsafe { std::mem::zeroed() };
    (i.struct_size, i.version) = (size_of::<PonkoUiInputEx>() as u32, PONKO_UI_INPUT_EX_VERSION);
    ok(unsafe { ponko_ui_input_ex_default(&mut i) });
    i.dt = 1.0 / 60.0;
    i
}

unsafe fn frame(ctx: Ctx, input: &PonkoUiInputEx) {
    ok(unsafe { ponko_ctx_ui_frame_ex(ctx, S, SCREEN, input) });
}

unsafe fn center(ctx: Ctx, id: u64) -> PonkoVec2 {
    let mut r = PonkoRect::default();
    ok(unsafe { ponko_ctx_ui_rect(ctx, S, id, &mut r) });
    PonkoVec2 {
        x: (r.min.x + r.max.x) * 0.5,
        y: (r.min.y + r.max.y) * 0.5,
    }
}

unsafe fn click_at(ctx: Ctx, at: PonkoVec2) {
    unsafe {
        let mut i = input_ex();
        i.input.pointer = at;
        (i.input.pressed, i.input.held) = (true, true);
        frame(ctx, &i);
        (i.input.pressed, i.input.held, i.input.released) = (false, false, true);
        frame(ctx, &i);
    }
}

unsafe fn events(ctx: Ctx) -> Vec<PonkoUiEventEx> {
    let mut all = Vec::new();
    loop {
        let mut e: PonkoUiEventEx = unsafe { std::mem::zeroed() };
        (e.struct_size, e.version) = (size_of::<PonkoUiEventEx>() as u32, PONKO_UI_EVENT_EX_VERSION);
        match unsafe { ponko_ctx_ui_next_event_ex(ctx, S, &mut e) } {
            PONKO_OK => all.push(e),
            PONKO_NOT_FOUND => return all,
            other => panic!("{other:?}: {}", last_error()),
        }
    }
}

unsafe fn selected(ctx: Ctx, id: u64) -> u64 {
    let mut i = 0;
    ok(unsafe { ponko_ctx_ui_selected(ctx, S, id, &mut i) });
    i
}

unsafe fn item(ctx: Ctx, id: u64, index: u64) -> String {
    let mut len = 0;
    ok(unsafe { ponko_ctx_ui_item(ctx, S, id, index, null_mut(), 0, &mut len) });
    let mut buffer = vec![0u8; len + 1];
    ok(unsafe { ponko_ctx_ui_item(ctx, S, id, index, buffer.as_mut_ptr().cast(), buffer.len(), &mut len) });
    String::from_utf8(buffer[..len].to_vec()).unwrap()
}

fn key(code: PonkoKey) -> PonkoActionBinding {
    PonkoActionBinding {
        kind: 0,
        pad: 0,
        first: code as u32,
        second: 0,
        third: 0,
        fourth: 0,
        deadzone: 0.0,
    }
}

unsafe fn choices_dropdowns_tabs_and_radios(ctx: Ctx) {
    unsafe {
        let root = root(ctx);
        let names: [*const c_char; 3] = [c"Low".as_ptr(), c"Medium".as_ptr(), c"High".as_ptr()];
        let layout = fixed(200.0, 32.0);
        let quality = id_of(|o| ponko_ctx_ui_add_dropdown(ctx, S, root, names.as_ptr(), 3, &layout, o));
        assert_eq!(selected(ctx, quality), 0);
        assert_eq!(item(ctx, quality, 2), "High");
        assert_eq!(
            ponko_ctx_ui_item(ctx, S, quality, 3, null_mut(), 0, &mut 0),
            PONKO_NOT_FOUND
        );

        // Open it with a click, then choose the next option with the keys.
        frame(ctx, &input_ex());
        click_at(ctx, center(ctx, quality));
        let mut owner = 0;
        ok(ponko_ctx_ui_popup_owner(ctx, S, &mut owner));
        assert_eq!(owner, quality);
        let mut i = input_ex();
        i.input.direction = PonkoVec2 { x: 0.0, y: 1.0 };
        frame(ctx, &i);
        let mut i = input_ex();
        i.input.confirm = true;
        frame(ctx, &i);
        assert_eq!(selected(ctx, quality), 1);
        assert!(
            events(ctx)
                .iter()
                .any(|e| e.kind == PONKO_UI_EVENT_CHANGED && e.id == quality)
        );
        ok(ponko_ctx_ui_popup_owner(ctx, S, &mut owner));
        assert_eq!(owner, PONKO_UI_NONE);

        // Choosing by hand makes no event; NO_INDEX chooses nothing.
        ok(ponko_ctx_ui_set_selected(ctx, S, quality, PONKO_UI_NO_INDEX));
        assert_eq!(selected(ctx, quality), PONKO_UI_NO_INDEX);
        ok(ponko_ctx_ui_set_selected(ctx, S, quality, 2));
        assert_eq!(selected(ctx, quality), 2);

        // Tabs: the first is chosen, its page is shown.
        let tabs = id_of(|o| ponko_ctx_ui_add_tabs(ctx, S, root, null(), o));
        let audio = id_of(|o| ponko_ctx_ui_add_tab(ctx, S, tabs, c"Audio".as_ptr(), o));
        let video = id_of(|o| ponko_ctx_ui_add_tab(ctx, S, tabs, c"Video".as_ptr(), o));
        assert_ne!(audio, video);
        let second = id_of(|o| ponko_ctx_ui_tab_button(ctx, S, tabs, 1, o));
        assert_eq!(ponko_ctx_ui_tab_button(ctx, S, tabs, 2, &mut 0), PONKO_NOT_FOUND);
        frame(ctx, &input_ex());
        assert_eq!(selected(ctx, tabs), 0);
        click_at(ctx, center(ctx, second));
        assert_eq!(selected(ctx, tabs), 1);
        let mut shown = false;
        ok(ponko_ctx_ui_is_shown(ctx, S, video, &mut shown));
        assert!(shown);
        ok(ponko_ctx_ui_is_shown(ctx, S, audio, &mut shown));
        assert!(!shown);
        // A tab goes only into a tab strip.
        assert_eq!(
            ponko_ctx_ui_add_tab(ctx, S, root, c"x".as_ptr(), &mut 0),
            PONKO_INVALID_ARGUMENT
        );

        let group = id_of(|o| ponko_ctx_ui_add_radio_group(ctx, S, root, null(), o));
        let layout = fixed(160.0, 24.0);
        let _off = id_of(|o| ponko_ctx_ui_add_radio(ctx, S, group, c"Off".as_ptr(), &layout, o));
        let on = id_of(|o| ponko_ctx_ui_add_radio(ctx, S, group, c"On".as_ptr(), &layout, o));
        assert_eq!(
            ponko_ctx_ui_add_radio(ctx, S, root, c"x".as_ptr(), null(), &mut 0),
            PONKO_INVALID_ARGUMENT
        );
        frame(ctx, &input_ex());
        events(ctx);
        click_at(ctx, center(ctx, on));
        assert_eq!(selected(ctx, group), 1);
        assert!(
            events(ctx)
                .iter()
                .any(|e| e.kind == PONKO_UI_EVENT_CHANGED && e.id == group)
        );
    }
}

#[test]
fn dropdowns_tabs_and_radio_groups_choose_from_c() {
    run(choices_dropdowns_tabs_and_radios);
}

unsafe fn values_lists_and_splits(ctx: Ctx) {
    unsafe {
        let root = root(ctx);
        let bar = id_of(|o| ponko_ctx_ui_add_progress_bar(ctx, S, root, 0.25, &fixed(200.0, 8.0), o));
        let mut v = 0.0;
        ok(ponko_ctx_ui_value(ctx, S, bar, &mut v));
        assert_eq!(v, 0.25);
        let _spinner = id_of(|o| ponko_ctx_ui_add_spinner(ctx, S, root, null(), o));

        let field = id_of(|o| ponko_ctx_ui_add_number_field(ctx, S, root, 5.0, &fixed(120.0, 28.0), o));
        ok(ponko_ctx_ui_set_number_range(ctx, S, field, 0.0, 10.0, 2.0));
        let (mut min, mut max, mut step) = (1.0, 1.0, 1.0);
        ok(ponko_ctx_ui_number_range(ctx, S, field, &mut min, &mut max, &mut step));
        assert_eq!((min, max, step), (0.0, 10.0, 2.0));
        assert_eq!(
            ponko_ctx_ui_number_range(ctx, S, field, &mut min, null_mut(), &mut step),
            PONKO_INVALID_ARGUMENT
        );
        ok(ponko_ctx_ui_set_number(ctx, S, field, 40.0));
        let mut n = 0.0;
        ok(ponko_ctx_ui_number(ctx, S, field, &mut n));
        assert_eq!(n, 10.0, "kept in its range");
        assert_eq!(
            ponko_ctx_ui_set_number_range(ctx, S, field, 1.0, 0.0, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_ui_number(ctx, S, bar, &mut n), PONKO_INVALID_ARGUMENT);

        // A list of a million items keeps nodes only for its visible rows.
        let list = id_of(|o| ponko_ctx_ui_add_list(ctx, S, root, 1, 24.0, &fixed(300.0, 240.0), o));
        ok(ponko_ctx_ui_set_list_len(ctx, S, list, 1_000_000));
        let mut len = 0;
        ok(ponko_ctx_ui_list_len(ctx, S, list, &mut len));
        assert_eq!(len, 1_000_000);
        assert_eq!(ponko_ctx_ui_list_len(ctx, S, field, &mut len), PONKO_INVALID_ARGUMENT);
        frame(ctx, &input_ex());
        let access = ponko_ui_access_new();
        let mut changes = std::mem::zeroed::<PonkoUiAccessChanges>();
        ok(ponko_ui_access_update(access, ctx, S, &mut changes));
        let mut count = 0;
        ok(ponko_ui_access_count(access, &mut count));
        ponko_ui_access_free(access);
        assert!(count < 60, "{count} nodes for a million items");
        ok(ponko_ctx_ui_scroll_to_item(ctx, S, list, 500_000));
        frame(ctx, &input_ex());
        let mut r = PonkoRect::default();
        ok(ponko_ctx_ui_rect(ctx, S, list, &mut r));
        let row = id_of(|o| {
            ponko_ctx_ui_hit_test(
                ctx,
                S,
                PonkoVec2 {
                    x: r.min.x + 10.0,
                    y: r.max.y - 10.0,
                },
                o,
            )
        });
        let mut shows = 0;
        ok(ponko_ctx_ui_list_item(ctx, S, row, &mut shows));
        assert_eq!(shows, 500_000, "the item scrolled to sits in the bottom row");
        assert!(
            events(ctx)
                .iter()
                .any(|e| e.kind == PONKO_UI_EVENT_ROWS_SHOWN && e.id == list)
        );
        assert_eq!(
            ponko_ctx_ui_add_list(ctx, S, root, 0, 24.0, null(), &mut 0),
            PONKO_INVALID_ARGUMENT
        );

        // Text items through set_items; a grid wraps them.
        let grid = id_of(|o| ponko_ctx_ui_add_list(ctx, S, root, 4, 48.0, &fixed(200.0, 96.0), o));
        let names: [*const c_char; 2] = [c"Sword".as_ptr(), c"Shield".as_ptr()];
        ok(ponko_ctx_ui_set_items(ctx, S, grid, names.as_ptr(), 2));
        assert_eq!(item(ctx, grid, 1), "Shield");
        assert_eq!(ponko_ctx_ui_set_items(ctx, S, grid, null(), 2), PONKO_INVALID_ARGUMENT);

        let split = id_of(|o| {
            ponko_ctx_ui_add_split(
                ctx,
                S,
                root,
                PonkoUiAxis::PONKO_UI_AXIS_HORIZONTAL,
                &fixed(400.0, 100.0),
                o,
            )
        });
        let (mut a, mut b) = (0, 0);
        ok(ponko_ctx_ui_split_panes(ctx, S, split, &mut a, &mut b));
        frame(ctx, &input_ex());
        let (mut ra, mut rb) = (PonkoRect::default(), PonkoRect::default());
        ok(ponko_ctx_ui_rect(ctx, S, a, &mut ra));
        ok(ponko_ctx_ui_rect(ctx, S, b, &mut rb));
        let width = |r: PonkoRect| r.max.x - r.min.x;
        // Equal panes at first, each half of what the 6 unit divider leaves.
        assert!(
            ra.max.x < rb.min.x && width(ra) == 197.0 && width(rb) == 197.0 && rb.max.x == 400.0,
            "{ra:?} {rb:?}"
        );
        assert_eq!(
            ponko_ctx_ui_split_panes(ctx, S, root, &mut a, &mut b),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn values_lists_and_split_panes_work_from_c() {
    run(values_lists_and_splits);
}

unsafe fn bindings_tooltips_menus_and_drags(ctx: Ctx) {
    unsafe {
        let root = root(ctx);
        let layout = fixed(160.0, 28.0);
        let jump = key(PonkoKey::PONKO_KEY_SPACE);
        let field = id_of(|o| ponko_ctx_ui_add_key_binding(ctx, S, root, &jump, &layout, o));
        let mut b = key(PonkoKey::PONKO_KEY_F);
        let mut bound = false;
        ok(ponko_ctx_ui_binding(ctx, S, field, &mut b, &mut bound));
        assert!(bound);
        assert_eq!(b.first, PonkoKey::PONKO_KEY_SPACE as u32);

        // Click to listen, then the next press becomes the binding.
        frame(ctx, &input_ex());
        click_at(ctx, center(ctx, field));
        let mut capturing = false;
        ok(ponko_ctx_ui_is_capturing(ctx, S, field, &mut capturing));
        assert!(capturing);
        let mut i = input_ex();
        i.has_captured = true;
        i.captured = key(PonkoKey::PONKO_KEY_F);
        frame(ctx, &i);
        ok(ponko_ctx_ui_binding(ctx, S, field, &mut b, &mut bound));
        assert_eq!((bound, b.first), (true, PonkoKey::PONKO_KEY_F as u32));
        assert!(
            events(ctx)
                .iter()
                .any(|e| e.kind == PONKO_UI_EVENT_CHANGED && e.id == field)
        );
        ok(ponko_ctx_ui_set_binding(ctx, S, field, null()));
        ok(ponko_ctx_ui_binding(ctx, S, field, &mut b, &mut bound));
        assert!(!bound);
        let axis = PonkoActionBinding { kind: 4, ..jump };
        assert_eq!(ponko_ctx_ui_set_binding(ctx, S, field, &axis), PONKO_INVALID_ARGUMENT);

        // A tooltip shows after the pointer rests for the theme's delay.
        let button = id_of(|o| ponko_ctx_ui_add_button(ctx, S, root, c"Save".as_ptr(), &layout, o));
        let mut tooltip = [0 as c_char; 32];
        let mut tooltip_len = 0;
        assert_eq!(
            ponko_ctx_ui_tooltip(ctx, S, button, null_mut(), 0, &mut tooltip_len),
            PONKO_NOT_FOUND
        );
        ok(ponko_ctx_ui_set_tooltip(ctx, S, button, c"Saves the game".as_ptr()));
        ok(ponko_ctx_ui_tooltip(ctx, S, button, null_mut(), 0, &mut tooltip_len));
        assert_eq!(tooltip_len, c"Saves the game".to_bytes().len());
        ok(ponko_ctx_ui_tooltip(
            ctx,
            S,
            button,
            tooltip.as_mut_ptr(),
            tooltip.len(),
            &mut tooltip_len,
        ));
        assert_eq!(CStr::from_ptr(tooltip.as_ptr()), c"Saves the game");
        let mut shown = (0, PonkoRect::default());
        assert_eq!(
            ponko_ctx_ui_shown_tooltip(ctx, S, &mut shown.0, &mut shown.1),
            PONKO_NOT_FOUND
        );
        frame(ctx, &input_ex());
        let mut i = input_ex();
        i.input.pointer = center(ctx, button);
        i.dt = 0.5;
        for _ in 0..3 {
            frame(ctx, &i);
        }
        ok(ponko_ctx_ui_shown_tooltip(ctx, S, &mut shown.0, &mut shown.1));
        assert_eq!(shown.0, button);

        // A context menu: the secondary button opens it, an entry makes CHOSEN.
        let entries: [*const c_char; 2] = [c"Rename".as_ptr(), c"Delete".as_ptr()];
        ok(ponko_ctx_ui_set_context_menu(ctx, S, button, entries.as_ptr(), 2));
        events(ctx);
        let mut i = input_ex();
        i.input.pointer = center(ctx, button);
        i.secondary = true;
        frame(ctx, &i);
        let mut owner = 0;
        ok(ponko_ctx_ui_popup_owner(ctx, S, &mut owner));
        assert_eq!(owner, button);
        ok(ponko_ctx_ui_close_popup(ctx, S));
        ok(ponko_ctx_ui_open_context_menu(ctx, S, button));
        ok(ponko_ctx_ui_popup_owner(ctx, S, &mut owner));
        assert_eq!(owner, button);
        for (direction, confirm) in [(1.0, false), (0.0, true)] {
            let mut i = input_ex();
            i.input.direction = PonkoVec2 { x: 0.0, y: direction };
            i.input.confirm = confirm;
            frame(ctx, &i);
        }
        let chosen: Vec<_> = events(ctx)
            .into_iter()
            .filter(|e| e.kind == PONKO_UI_EVENT_CHOSEN)
            .collect();
        assert_eq!(chosen.len(), 1);
        assert_eq!((chosen[0].id, chosen[0].index), (button, 1));

        // Drag a node onto a drop target, then move it there.
        let row = ponko_ui_layout_row(60.0);
        let bag = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, root, &row, o));
        let chest = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, root, &row, o));
        let gem = id_of(|o| ponko_ctx_ui_add_button(ctx, S, bag, c"Gem".as_ptr(), &fixed(48.0, 48.0), o));
        ok(ponko_ctx_ui_set_draggable(ctx, S, gem, true));
        ok(ponko_ctx_ui_set_drop_target(ctx, S, chest, true));
        frame(ctx, &input_ex());
        events(ctx);
        let (from, to) = (center(ctx, gem), center(ctx, chest));
        let mut i = input_ex();
        i.input.pointer = from;
        (i.input.pressed, i.input.held) = (true, true);
        frame(ctx, &i);
        i.input.pressed = false;
        for t in [0.3, 0.7, 1.0] {
            i.input.pointer = PonkoVec2 {
                x: from.x + (to.x - from.x) * t,
                y: from.y + (to.y - from.y) * t,
            };
            frame(ctx, &i);
        }
        let mut dragging = 0;
        ok(ponko_ctx_ui_dragging(ctx, S, &mut dragging));
        assert_eq!(dragging, gem);
        (i.input.held, i.input.released) = (false, true);
        frame(ctx, &i);
        let dropped: Vec<_> = events(ctx)
            .into_iter()
            .filter(|e| e.kind == PONKO_UI_EVENT_DROPPED)
            .collect();
        assert_eq!(dropped.len(), 1);
        assert_eq!((dropped[0].id, dropped[0].other), (gem, chest));
        ok(ponko_ctx_ui_set_parent(ctx, S, gem, chest));
        assert_eq!(ponko_ctx_ui_set_parent(ctx, S, chest, gem), PONKO_INVALID_ARGUMENT);
        ok(ponko_ctx_ui_dragging(ctx, S, &mut dragging));
        assert_eq!(dragging, PONKO_UI_NONE);
    }
}

#[test]
fn key_bindings_tooltips_context_menus_and_drags_work_from_c() {
    run(bindings_tooltips_menus_and_drags);
}

fn theme_ex() -> PonkoUiThemeEx {
    let mut t: PonkoUiThemeEx = unsafe { std::mem::zeroed() };
    (t.struct_size, t.version) = (size_of::<PonkoUiThemeEx>() as u32, PONKO_UI_THEME_EX_VERSION);
    t
}

unsafe fn themes_access_and_immediate(ctx: Ctx) {
    unsafe {
        let (mut dark, mut light) = (theme_ex(), theme_ex());
        ok(ponko_ui_theme_ex_dark(&mut dark));
        ok(ponko_ui_theme_ex_light(&mut light));
        assert_ne!(dark.theme.panel, light.theme.panel);
        // A custom theme: the light one, square and roomier.
        light.radius = 0.0;
        light.spacing = 12.0;
        ok(ponko_ctx_ui_set_theme_ex(ctx, S, &light));
        let mut back = theme_ex();
        ok(ponko_ctx_ui_theme_ex(ctx, S, &mut back));
        assert_eq!(back, light);
        light.tooltip_delay = f32::NAN;
        assert_eq!(ponko_ctx_ui_set_theme_ex(ctx, S, &light), PONKO_INVALID_ARGUMENT);
        let mut stale = theme_ex();
        stale.version = 99;
        assert_ne!(ponko_ctx_ui_theme_ex(ctx, S, &mut stale), PONKO_OK);

        // Roles and widget state for screen readers.
        let root = root(ctx);
        let names: [*const c_char; 3] = [c"A".as_ptr(), c"B".as_ptr(), c"C".as_ptr()];
        let pick = id_of(|o| ponko_ctx_ui_add_dropdown(ctx, S, root, names.as_ptr(), 3, &fixed(100.0, 30.0), o));
        let field = id_of(|o| ponko_ctx_ui_add_number_field(ctx, S, root, 3.0, &fixed(100.0, 30.0), o));
        frame(ctx, &input_ex());
        let access = ponko_ui_access_new();
        let mut changes = std::mem::zeroed::<PonkoUiAccessChanges>();
        ok(ponko_ui_access_update(access, ctx, S, &mut changes));
        let mut node = std::mem::zeroed::<PonkoUiAccessNode>();
        ok(ponko_ui_access_find(access, pick, &mut node));
        assert_eq!(node.role, PonkoUiRole::PONKO_UI_ROLE_COMBO_BOX);
        let mut state: PonkoUiAccessStateEx = std::mem::zeroed();
        (state.struct_size, state.version) = (
            size_of::<PonkoUiAccessStateEx>() as u32,
            PONKO_UI_ACCESS_STATE_EX_VERSION,
        );
        ok(ponko_ui_access_state_ex(access, pick, &mut state));
        assert_eq!(state.expanded, PonkoUiExpanded::PONKO_UI_EXPANDED_CLOSED);
        ok(ponko_ui_access_find(access, field, &mut node));
        assert_eq!(node.role, PonkoUiRole::PONKO_UI_ROLE_SPIN_BUTTON);
        ok(ponko_ui_access_state_ex(access, field, &mut state));
        assert_eq!((state.min, state.max, state.step), (0.0, 100.0, 1.0));
        ponko_ui_access_free(access);

        // The immediate widgets.
        let rect = PonkoRect {
            min: PonkoVec2 { x: 10.0, y: 10.0 },
            max: PonkoVec2 { x: 170.0, y: 34.0 },
        };
        let (mut on, mut changed) = (false, true);
        ok(ponko_ctx_ui_toggle(ctx, rect, c"Paths".as_ptr(), &mut on, &mut changed));
        assert!(!on && !changed, "no click, no change");
        let mut volume = 0.5;
        ok(ponko_ctx_ui_slider(ctx, rect, &mut volume, &mut changed));
        assert!(volume == 0.5 && !changed);
        ok(ponko_ctx_ui_progress_bar(ctx, rect, 0.5));
        assert_eq!(
            ponko_ctx_ui_slider(ctx, rect, null_mut(), &mut changed),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn themes_access_state_and_immediate_widgets_work_from_c() {
    run(themes_access_and_immediate);
}

#[test]
fn null_contexts_fail_cleanly() {
    unsafe {
        let mut e: PonkoUiEventEx = std::mem::zeroed();
        (e.struct_size, e.version) = (size_of::<PonkoUiEventEx>() as u32, PONKO_UI_EVENT_EX_VERSION);
        assert_eq!(
            ponko_ctx_ui_next_event_ex(null_mut(), S, &mut e),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_next_event_ex(null_mut(), S, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_frame_ex(null_mut(), S, SCREEN, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ui_theme_ex_dark(null_mut()), PONKO_INVALID_ARGUMENT);
    }
}
