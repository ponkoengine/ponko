//! The retained UI's widget set from C (Rust `Ui::dropdown`, `Ui::tabs`,
//! `Ui::list`, ...): builders for each widget, their state, the events and
//! theme tokens ABI 2's fixed structs can't hold, and the immediate toggle,
//! slider and progress bar.
//!
//! Builders take what `ponko_ctx_ui_add_panel` takes (`target`, `parent`,
//! `layout`, `out_id`). Indices are `uint64_t`; `PONKO_UI_NO_INDEX` is "no
//! choice". Text lists are arrays of `count` NUL-terminated strings.
//!
//! - `PonkoUiEventEx` (`ponko_ctx_ui_next_event_ex`): every event kind,
//!   with the chosen entry's index and the drop target.
//! - `PonkoUiThemeEx`: `PonkoUiTheme` plus spacing, corner radius and the
//!   tooltip delay; the light and dark themes.
//! - `PonkoUiInputEx` (`ponko_ctx_ui_frame_ex`): `PonkoUiInput` plus the
//!   frame time (spinners, tooltips), the secondary button and menu key
//!   (context menus) and the press a key-binding field records.
use std::ffi::c_char;
use std::mem::{offset_of, size_of};

use ponko::actions::ButtonBinding;
use ponko::prelude::PressedInput;
use ponko::ui::{Ui, UiAxis, UiEvent, UiId, UiTheme, UiWidget};

use crate::actions::PonkoActionBinding;
use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::types::{PonkoRect, PonkoVec2, to_c};
use crate::ui_retained::{
    PonkoUiEventKind, PonkoUiInput, PonkoUiLayout, PonkoUiTheme, Start, add, check_output, copy_string, existing,
    id_to_c, optional_text, plain, theme_from_c, theme_to_c, ui_input_from_c, with_ui,
};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid};

/// "No item": what `ponko_ctx_ui_selected` writes when nothing is chosen,
/// and what `ponko_ctx_ui_set_selected` takes to choose nothing.
/// ```
/// assert_eq!(ponko_ffi::PONKO_UI_NO_INDEX, u64::MAX);
/// ```
pub const PONKO_UI_NO_INDEX: u64 = u64::MAX;

/// Which way split panes divide (Rust `UiAxis`).
/// ```
/// assert_eq!(ponko_ffi::PonkoUiAxis::PONKO_UI_AXIS_VERTICAL as u32, 1);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiAxis {
    /// Side by side; the divider moves left and right.
    #[default]
    PONKO_UI_AXIS_HORIZONTAL,
    /// One above the other; the divider moves up and down.
    PONKO_UI_AXIS_VERTICAL,
}

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

// ---- helpers -------------------------------------------------------------------

/// `count` C strings.
///
/// # Safety
/// `items` readable for `count` NUL-terminated strings (or null with 0).
unsafe fn strings<'a>(items: *const *const c_char, count: usize) -> Result<Vec<&'a str>, Failure> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if items.is_null() {
        return Err(invalid("items is null"));
    }
    // SAFETY: caller contract.
    let items = unsafe { std::slice::from_raw_parts(items, count) };
    items
        .iter()
        .map(|&s| {
            if s.is_null() {
                return Err(invalid("an item is null"));
            }
            // SAFETY: caller contract.
            unsafe { optional_text(s) }
        })
        .collect()
}

fn index_to_c(index: Option<usize>) -> u64 {
    index.map_or(PONKO_UI_NO_INDEX, |i| i as u64)
}

fn index_from_c(index: u64) -> Option<usize> {
    (index != PONKO_UI_NO_INDEX).then_some(index as usize)
}

/// A node of kind `widget`, or an error naming what was expected.
fn of_kind(ui: &Ui, id: u64, widget: UiWidget, what: &str) -> Result<UiId, Failure> {
    let id = existing(ui, id)?;
    if ui.widget(id) != Some(widget) {
        return Err(crate::invalid_owned(format!("node is not {what}")));
    }
    Ok(id)
}

/// A key, mouse button or gamepad button binding from C (kinds
/// `PONKO_ACTION_KEY`, `PONKO_ACTION_MOUSE`, `PONKO_ACTION_GAMEPAD_BUTTON`).
pub(crate) fn binding_from_c(b: &PonkoActionBinding) -> Result<ButtonBinding, Failure> {
    Ok(match b.kind {
        0 => ButtonBinding::Key(crate::actions::key(b.first)?),
        1 => ButtonBinding::Mouse(crate::actions::mouse(b.first)?),
        2 => ButtonBinding::Gamepad {
            pad: crate::actions::pad(b.pad)?,
            button: crate::actions::button(b.first)?,
        },
        _ => return Err(invalid("a key binding is a key, a mouse button or a gamepad button")),
    })
}

fn binding_to_c(b: &ButtonBinding) -> Option<PonkoActionBinding> {
    crate::pressed::describe(PressedInput::Button(b.clone()))
}

// ---- builders -------------------------------------------------------------------

/// Adds a dropdown choosing one of `count` options (Rust `Ui::dropdown`),
/// showing the first. Choosing another queues `PONKO_UI_EVENT_CHANGED`; read
/// it with `ponko_ctx_ui_selected`. See `ponko_ctx_ui_add_panel` for the rest.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_dropdown(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, ponko_ffi::PONKO_UI_NONE, std::ptr::null(), 0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `options` readable for `count` NUL-terminated strings
/// (or null with 0); `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_dropdown(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    options: *const *const c_char,
    count: usize,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let options = strings(options, count)?;
        add(ctx, target, parent, UiWidget::Dropdown, layout, out_id, plain(""))?;
        let id = *out_id;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_items(id, options);
            Ok(())
        })
    })
}

/// Builds a widget with Rust's builder (which sets up its parts and
/// styles), then the C layout over it when one is given.
///
/// # Safety
/// Forwards the caller's contract for `ctx`, `layout` and `out_id`.
unsafe fn build(
    ctx: *mut PonkoCtx,
    target: u64,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
    make: impl FnOnce(&mut Ui) -> Result<UiId, Failure>,
) -> Result<(), Failure> {
    if out_id.is_null() {
        return Err(invalid("output is null"));
    }
    // SAFETY: forwarded caller contract.
    let style = unsafe { layout.as_ref() }
        // SAFETY: as above.
        .map(|_| unsafe { crate::ui_retained::style_or_default(layout) })
        .transpose()?;
    // SAFETY: as above.
    unsafe {
        with_ui(ctx, target, |ui| {
            let id = make(ui)?;
            if let Some(style) = style {
                ui.set_layout(id, ponko::ui::UiLayout::unstable_custom(style));
            }
            out(out_id, id.bits())
        })
    }
}

/// Adds a tab strip (Rust `Ui::tabs`): a bar of tabs over the page of the
/// chosen one. Add tabs with `ponko_ctx_ui_add_tab`. `parent` must exist.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_tabs(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_tabs(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(ui.tabs(parent).id())
        })
    })
}

/// Adds a tab titled `title` to a tab strip and writes the id of its page
/// (a column panel: add the tab's content to it) to `out_page`. The first
/// tab is chosen; choosing another queues `PONKO_UI_EVENT_CHANGED` on the
/// strip. `ponko_ctx_ui_tab_button` gives the tab itself.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_tab(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, c"Audio".as_ptr(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `title` null or NUL-terminated; `out_page` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_tab(
    ctx: *mut PonkoCtx,
    target: u64,
    tabs: u64,
    title: *const c_char,
    out_page: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let title = optional_text(title)?;
        build(ctx, target, std::ptr::null(), out_page, |ui| {
            let tabs = of_kind(ui, tabs, UiWidget::Tabs, "a tab strip")?;
            Ok(ui.tab(tabs, title).id())
        })
    })
}

/// Writes the id of tab number `index` of a tab strip to `out_tab`;
/// `PONKO_NOT_FOUND` past the last.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_tab_button(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_tab` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_tab_button(
    ctx: *mut PonkoCtx,
    target: u64,
    tabs: u64,
    index: u64,
    out_tab: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let tabs = of_kind(ui, tabs, UiWidget::Tabs, "a tab strip")?;
            let tab = ui
                .tab_button(tabs, index as usize)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no tab at that index"))?;
            out(out_tab, tab.bits())
        })
    })
}

/// Adds a radio group (Rust `Ui::radio_group`): a column of radio buttons,
/// none checked at first. Add them with `ponko_ctx_ui_add_radio`; choosing one queues
/// `PONKO_UI_EVENT_CHANGED` on the group.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_radio_group(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_radio_group(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(ui.radio_group(parent).id())
        })
    })
}

/// Adds a radio button labelled `text` to a radio group.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_radio(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, c"Low".as_ptr(), std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `text` null or NUL-terminated; `layout` null or
/// readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_radio(
    ctx: *mut PonkoCtx,
    target: u64,
    group: u64,
    text: *const c_char,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let text = optional_text(text)?;
        build(ctx, target, layout, out_id, |ui| {
            let group = of_kind(ui, group, UiWidget::RadioGroup, "a radio group")?;
            Ok(ui.radio(group, text).id())
        })
    })
}

/// Adds a progress bar filled to `value` (0 to 1; change it with
/// `ponko_ctx_ui_set_value`). It takes no input.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_progress_bar(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, ponko_ffi::PONKO_UI_NONE, 0.5, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_progress_bar(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    value: f32,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let start = Start {
            text: "",
            paint: None,
            value: Some(value),
        };
        add(ctx, target, parent, UiWidget::Progress, layout, out_id, start)
    })
}

/// Adds a spinner, a busy indicator that turns while shown (24 units square
/// with a null layout).
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_spinner(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_spinner(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(ui.spinner(parent).id())
        })
    })
}

/// Adds a number field showing `value`, 0 to 100 in steps of 1 until
/// `ponko_ctx_ui_set_number_range` says otherwise. Arrows, its `-` and `+`
/// ends and typed digits change it; each change queues
/// `PONKO_UI_EVENT_CHANGED` (read it with `ponko_ctx_ui_number`).
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_number_field(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 5.0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_number_field(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    value: f64,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if !value.is_finite() {
            return Err(invalid("value is not finite"));
        }
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(ui.number_field(parent, value).id())
        })
    })
}

/// Adds a list of `row_height` unit rows (Rust `Ui::list`), or with
/// `columns` > 1 a grid of cells (`Ui::grid_view`). It keeps nodes only for
/// the rows on screen, so a list of a million items costs what one of ten
/// does. Give it items with `ponko_ctx_ui_set_items` or a length with
/// `ponko_ctx_ui_set_list_len`. Clicking or arrowing to an item queues
/// `PONKO_UI_EVENT_CHANGED`, confirm `PONKO_UI_EVENT_SUBMITTED`.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_list(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 1, 24.0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_list(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    columns: u32,
    row_height: f32,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if !(row_height.is_finite() && row_height > 0.0) || columns == 0 {
            return Err(invalid("a list needs rows taller than 0 and at least one column"));
        }
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(if columns > 1 {
                ui.grid_view(parent, columns as usize, row_height).id()
            } else {
                ui.list(parent, row_height).id()
            })
        })
    })
}

/// Adds split panes (Rust `Ui::split`): two panes and a divider between
/// them, dragged or arrowed. `ponko_ctx_ui_value` is the first pane's share
/// of the length the divider leaves (0.5, equal panes, at first; each move
/// queues `PONKO_UI_EVENT_CHANGED`). `ponko_ctx_ui_split_panes` gives
/// the panes to fill.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_split(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, ponko_ffi::PonkoUiAxis::PONKO_UI_AXIS_HORIZONTAL, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layout` null or readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_split(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    axis: PonkoUiAxis,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let axis = match axis {
            PonkoUiAxis::PONKO_UI_AXIS_HORIZONTAL => UiAxis::Horizontal,
            PonkoUiAxis::PONKO_UI_AXIS_VERTICAL => UiAxis::Vertical,
        };
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(ui.split(parent, axis).id())
        })
    })
}

/// Writes the first (left or top) and second pane of split panes.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_split_panes(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_first` and `out_second` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_split_panes(
    ctx: *mut PonkoCtx,
    target: u64,
    split: u64,
    out_first: *mut u64,
    out_second: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_first.is_null() || out_second.is_null() {
            return Err(invalid("output is null"));
        }
        with_ui(ctx, target, |ui| {
            let split = of_kind(ui, split, UiWidget::Split, "split panes")?;
            let (first, second) = ui.panes(split).ok_or_else(|| invalid("split panes lost a pane"))?;
            out(out_first, first.bits())?;
            out(out_second, second.bits())
        })
    })
}

/// Adds a key-binding field showing `binding` (null for none; kinds
/// `PONKO_ACTION_KEY`, `PONKO_ACTION_MOUSE`, `PONKO_ACTION_GAMEPAD_BUTTON`).
/// Activating it listens for the next key, mouse or gamepad button, which
/// becomes its binding (`PONKO_UI_EVENT_CHANGED`); Escape stops listening.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_ui_add_key_binding(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `binding` null or readable; `layout` null or
/// readable; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_add_key_binding(
    ctx: *mut PonkoCtx,
    target: u64,
    parent: u64,
    binding: *const PonkoActionBinding,
    layout: *const PonkoUiLayout,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let binding = binding.as_ref().map(binding_from_c).transpose()?;
        build(ctx, target, layout, out_id, |ui| {
            let parent = existing(ui, parent)?;
            Ok(ui.key_binding(parent, binding).id())
        })
    })
}

// ---- state ------------------------------------------------------------------------

/// Sets the options of a dropdown, the items of a list or the entries of
/// a node's context menu (`ponko_ctx_ui_set_context_menu`): `count` strings.
/// A list's length becomes `count`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_items(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `items` readable for `count` NUL-terminated strings
/// (or null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_items(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    items: *const *const c_char,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let items = strings(items, count)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_items(id, items);
            Ok(())
        })
    })
}

/// Reads item `index` of a dropdown or list into `buffer`, the way
/// `ponko_ctx_ui_text` reads a node's text; `PONKO_NOT_FOUND` past the last.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_item(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_item(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    index: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            let item = ui
                .items(id)
                .and_then(|items| items.get(index as usize))
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no item at that index"))?;
            copy_string(item, buffer, capacity, out_len)
        })
    })
}

/// Writes the chosen option of a dropdown, tab of a tab strip, button of a
/// radio group or item of a list to `out_index`; `PONKO_UI_NO_INDEX` when
/// nothing is chosen.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_selected(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_index` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_selected(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_index: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_index, index_to_c(ui.selected(id)))
        })
    })
}

/// Chooses `index` (or nothing with `PONKO_UI_NO_INDEX`) of a dropdown, tab
/// strip, radio group or list, without queuing an event. An index past the
/// end chooses nothing.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_selected(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_selected(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    index: u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_selected(id, index_from_c(index));
            Ok(())
        })
    })
}

/// Writes a number field's value to `out_value`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_number(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_number(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_value: *mut f64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::NumberField, "a number field")?;
            out(out_value, ui.number(id).unwrap_or(0.0))
        })
    })
}

/// Sets a number field's value, kept in its range, without queuing an
/// event.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_number(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 3.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_number(ctx: *mut PonkoCtx, target: u64, id: u64, value: f64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if !value.is_finite() {
            return Err(invalid("value is not finite"));
        }
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::NumberField, "a number field")?;
            ui.set_number(id, value);
            Ok(())
        })
    })
}

/// Sets a number field's range and step (`min` <= `max`, `step` > 0).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_number_range(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 0.0, 10.0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_number_range(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    min: f64,
    max: f64,
    step: f64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if !(min.is_finite() && max.is_finite() && step.is_finite() && min <= max && step > 0.0) {
            return Err(invalid("a number range needs min <= max and a step above 0"));
        }
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::NumberField, "a number field")?;
            ui.set_number_range(id, min, max, step);
            Ok(())
        })
    })
}

/// Sets how many items a list or grid has, for items it draws without text
/// (cells the game fills in by `ponko_ctx_ui_list_item`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_list_len(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 100) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_list_len(ctx: *mut PonkoCtx, target: u64, id: u64, len: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::List, "a list")?;
            ui.set_list_len(id, len as usize);
            Ok(())
        })
    })
}

/// Writes which item a list row node shows now to `out_index`
/// (`PONKO_UI_NO_INDEX` for a row off screen). Rows are reused as the list
/// scrolls: look again after `PONKO_UI_EVENT_ROWS_SHOWN`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_list_item(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_index` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_list_item(
    ctx: *mut PonkoCtx,
    target: u64,
    row: u64,
    out_index: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let row = existing(ui, row)?;
            out(out_index, index_to_c(ui.list_item(row)))
        })
    })
}

/// Scrolls a list so item `index` is on screen.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_scroll_to_item(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_scroll_to_item(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    index: u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::List, "a list")?;
            ui.scroll_to_item(id, index as usize);
            Ok(())
        })
    })
}

/// Writes a key-binding field's binding to `out_binding` and whether it
/// has one to `out_bound`.
/// ```
/// let mut b = ponko_ffi::PonkoActionBinding { kind: 0, pad: 0, first: 0, second: 0, third: 0, fourth: 0, deadzone: 0.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_binding(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut b, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_binding` and `out_bound` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_binding(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_binding: *mut PonkoActionBinding,
    out_bound: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_binding.is_null() || out_bound.is_null() {
            return Err(invalid("output is null"));
        }
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::KeyBinding, "a key-binding field")?;
            match ui.binding(id).and_then(binding_to_c) {
                Some(b) => {
                    out(out_binding, b)?;
                    out(out_bound, true)
                }
                None => out(out_bound, false),
            }
        })
    })
}

/// Sets a key-binding field's binding (null for none) without queuing an
/// event.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_binding(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `binding` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_binding(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    binding: *const PonkoActionBinding,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let binding = binding.as_ref().map(binding_from_c).transpose()?;
        with_ui(ctx, target, |ui| {
            let id = of_kind(ui, id, UiWidget::KeyBinding, "a key-binding field")?;
            ui.set_binding(id, binding);
            Ok(())
        })
    })
}

/// Writes whether a key-binding field is listening for a press.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_is_capturing(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_capturing` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_is_capturing(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_capturing: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_capturing, ui.is_capturing(id))
        })
    })
}

/// Gives a node a tooltip (null or empty for none): shown when the pointer
/// rests on it for the theme's tooltip delay, at once when keys focus it,
/// and read by screen readers as its description.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_tooltip(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, c"Hi".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `text` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_tooltip(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    text: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let text = optional_text(text)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_tooltip(id, text);
            Ok(())
        })
    })
}

/// Writes the node whose tooltip shows now, and where, to `out_id` and
/// `out_rect`; `PONKO_NOT_FOUND` when none shows.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_shown_tooltip(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, &mut 0, &mut Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_id` and `out_rect` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_shown_tooltip(
    ctx: *mut PonkoCtx,
    target: u64,
    out_id: *mut u64,
    out_rect: *mut PonkoRect,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_id.is_null() || out_rect.is_null() {
            return Err(invalid("output is null"));
        }
        with_ui(ctx, target, |ui| {
            let (id, rect) = ui
                .shown_tooltip()
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no tooltip shows"))?;
            out(out_id, id.bits())?;
            out(out_rect, to_c(rect))
        })
    })
}

/// Gives a node a context menu of `count` entries (none for no menu): the
/// right button on it, or the menu input (Shift+F10, a gamepad's North)
/// while it has focus, opens it. Choosing an entry queues
/// `PONKO_UI_EVENT_CHOSEN` (`ponko_ctx_ui_next_event_ex`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_context_menu(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `entries` readable for `count` NUL-terminated
/// strings (or null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_context_menu(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    entries: *const *const c_char,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let entries = strings(entries, count)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_context_menu(id, entries);
            Ok(())
        })
    })
}

/// Opens a node's context menu below it, as the menu input does.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_open_context_menu(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_open_context_menu(ctx: *mut PonkoCtx, target: u64, id: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.open_context_menu(id);
            Ok(())
        })
    })
}

/// Writes the dropdown or node whose pop-up (options, context menu) is
/// open to `out_id`; `PONKO_UI_NONE` when none is.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_popup_owner(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_popup_owner(ctx: *mut PonkoCtx, target: u64, out_id: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_ui(ctx, target, |ui| out(out_id, id_to_c(ui.popup_owner()))) })
}

/// Closes the open pop-up, if any, choosing nothing.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_close_popup(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_close_popup(ctx: *mut PonkoCtx, target: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            ui.close_popup();
            Ok(())
        })
    })
}

/// Makes a node draggable (or not): the pointer drags it, confirm picks it
/// up and drops it on the focused drop target. Letting go on a drop target
/// queues `PONKO_UI_EVENT_DROPPED`; the game moves it
/// (`ponko_ctx_ui_set_parent`) if it accepts.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_draggable(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_draggable(ctx: *mut PonkoCtx, target: u64, id: u64, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_draggable(id, on);
            Ok(())
        })
    })
}

/// Makes a node a drop target for dragged nodes (or not).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_drop_target(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_drop_target(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    on: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_drop_target(id, on);
            Ok(())
        })
    })
}

/// Writes the node being dragged or carried to `out_id`; `PONKO_UI_NONE`
/// when none is.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_dragging(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_dragging(ctx: *mut PonkoCtx, target: u64, out_id: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_ui(ctx, target, |ui| out(out_id, id_to_c(ui.dragging()))) })
}

/// Moves node `id` under `parent`, as its last child (a dropped item into
/// its new container). `PONKO_INVALID_ARGUMENT` for a move into itself or
/// one of its own children.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_parent(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_parent(ctx: *mut PonkoCtx, target: u64, id: u64, parent: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let (id, parent) = (existing(ui, id)?, existing(ui, parent)?);
            if !ui.set_parent(id, parent) {
                return Err(invalid("a node can't move into itself or its own children"));
            }
            Ok(())
        })
    })
}

// ---- events ---------------------------------------------------------------------

/// The version of `PonkoUiEventEx` this header describes.
pub const PONKO_UI_EVENT_EX_VERSION: u32 = 1;

/// One thing that happened, every kind included, in a layout that can grow
/// (`PonkoUiEvent` plus what the widget set's events carry). Set
/// `struct_size` and `version`, then call `ponko_ctx_ui_next_event_ex`
/// until it fails.
///
/// Version 1: `kind`, `id`, `other`, `index`.
/// ```
/// use ponko_ffi::*;
/// let mut e: PonkoUiEventEx = unsafe { std::mem::zeroed() };
/// (e.struct_size, e.version) = (size_of::<PonkoUiEventEx>() as u32, PONKO_UI_EVENT_EX_VERSION);
/// assert_eq!(unsafe { ponko_ctx_ui_next_event_ex(std::ptr::null_mut(), PONKO_UI_SCREEN, &mut e) } as u32, 1);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoUiEventEx {
    /// `sizeof(PonkoUiEventEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_UI_EVENT_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// What happened.
    pub kind: PonkoUiEventKind,
    /// The node it happened to (see the kinds).
    pub id: u64,
    /// The drop target of `PONKO_UI_EVENT_DROPPED`; `PONKO_UI_NONE` for the
    /// other kinds.
    pub other: u64,
    /// The chosen entry of `PONKO_UI_EVENT_CHOSEN`; 0 for the other kinds.
    pub index: u64,
}
const _: () = assert!(offset_of!(PonkoUiEventEx, struct_size) == 0 && offset_of!(PonkoUiEventEx, version) == 4);

impl Versioned for PonkoUiEventEx {
    const NAME: &'static str = "PonkoUiEventEx";
    const VERSION: u32 = PONKO_UI_EVENT_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, index: u64)),
            _ => None,
        }
    }
}

/// Takes the oldest queued event into the fields of `out_event`'s version,
/// or fails with `PONKO_NOT_FOUND` when none are left, as
/// `ponko_ctx_ui_next_event` does, with every kind. Read events with one of
/// the two: each takes from the same queue. A kind Rust gains before this
/// header has a constant for it is skipped.
/// # Safety
/// `ctx` null or live; `out_event` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_next_event_ex(
    ctx: *mut PonkoCtx,
    target: u64,
    out_event: *mut PonkoUiEventEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_event.is_null() {
            return Err(invalid("PonkoUiEventEx output is null"));
        }
        with_ui(ctx, target, |ui| {
            use PonkoUiEventKind::*;
            let (kind, id, other, index) = loop {
                let Some(event) = ui.next_event() else {
                    return Err(error(PonkoStatus::PONKO_NOT_FOUND, "no UI events left"));
                };
                break match event {
                    UiEvent::Clicked(id) => (PONKO_UI_EVENT_CLICKED, id, None, 0),
                    UiEvent::Changed(id) => (PONKO_UI_EVENT_CHANGED, id, None, 0),
                    UiEvent::Submitted(id) => (PONKO_UI_EVENT_SUBMITTED, id, None, 0),
                    UiEvent::ScreenPopped(id) => (PONKO_UI_EVENT_SCREEN_POPPED, id, None, 0),
                    UiEvent::Chosen(id, index) => (PONKO_UI_EVENT_CHOSEN, id, None, u64::from(index)),
                    UiEvent::Dropped(item, onto) => (PONKO_UI_EVENT_DROPPED, item, Some(onto), 0),
                    UiEvent::RowsShown(id) => (PONKO_UI_EVENT_ROWS_SHOWN, id, None, 0),
                    UiEvent::AnimationDone(id) => (PONKO_UI_EVENT_ANIMATION_DONE, id, None, 0),
                    _ => continue,
                };
            };
            let event = PonkoUiEventEx {
                struct_size: size_of::<PonkoUiEventEx>() as u32,
                version: PONKO_UI_EVENT_EX_VERSION,
                kind,
                id: id.bits(),
                other: id_to_c(other),
                index,
            };
            write_desc(out_event, &event)
        })
    })
}

// ---- theme ----------------------------------------------------------------------

/// The version of `PonkoUiThemeEx` this header describes.
pub const PONKO_UI_THEME_EX_VERSION: u32 = 2;

/// A whole theme (Rust `UiTheme`) in a layout that can grow: `PonkoUiTheme`
/// plus the tokens it has no room for. Fill one with
/// `ponko_ui_theme_ex_dark`, `ponko_ui_theme_ex_light` or
/// `ponko_ctx_ui_theme_ex`, change what you like (a custom theme), then
/// `ponko_ctx_ui_set_theme_ex`.
///
/// Version 1: `theme`, `spacing`, `radius`, `tooltip_delay`. Version 2
/// adds `transition` and `press_scale`.
/// ```
/// use ponko_ffi::*;
/// let mut t: PonkoUiThemeEx = unsafe { std::mem::zeroed() };
/// (t.struct_size, t.version) = (size_of::<PonkoUiThemeEx>() as u32, PONKO_UI_THEME_EX_VERSION);
/// assert_eq!(unsafe { ponko_ui_theme_ex_light(&mut t) }, PonkoStatus::PONKO_OK);
/// assert_eq!(t.radius, 4.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoUiThemeEx {
    /// `sizeof(PonkoUiThemeEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_UI_THEME_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Colors, font, text sizes and border width.
    pub theme: PonkoUiTheme,
    /// Space between and inside the widgets' parts (pop-up rows, tab
    /// padding) in logical units.
    pub spacing: f32,
    /// Corner radius of controls and panels in logical units; 0 for square.
    pub radius: f32,
    /// Seconds the pointer rests on a node before its tooltip shows.
    pub tooltip_delay: f32,
    /// How a control's fill and size follow its state (idle, hovered,
    /// focused, pressed, disabled); zero seconds: at once (version 2).
    pub transition: crate::ui_animation::PonkoUiTransition,
    /// The size a control is drawn at while pressed, around its centre;
    /// 1 leaves it alone (version 2).
    pub press_scale: f32,
}
const _: () = assert!(offset_of!(PonkoUiThemeEx, struct_size) == 0 && offset_of!(PonkoUiThemeEx, version) == 4);

impl Versioned for PonkoUiThemeEx {
    const NAME: &'static str = "PonkoUiThemeEx";
    const VERSION: u32 = PONKO_UI_THEME_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, tooltip_delay: f32)),
            2 => Some(end_of!(Self, press_scale: f32)),
            _ => None,
        }
    }
}

fn theme_ex_to_c(t: &UiTheme) -> PonkoUiThemeEx {
    PonkoUiThemeEx {
        struct_size: size_of::<PonkoUiThemeEx>() as u32,
        version: PONKO_UI_THEME_EX_VERSION,
        theme: theme_to_c(t),
        spacing: t.spacing,
        radius: t.radius,
        tooltip_delay: t.tooltip_delay,
        transition: crate::ui_animation::transition_to_c(t.transition),
        press_scale: t.press_scale,
    }
}

fn theme_ex_from_c(t: &PonkoUiThemeEx) -> Result<UiTheme, Failure> {
    let tokens = [t.spacing, t.radius, t.tooltip_delay, t.press_scale];
    if tokens.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(invalid(
            "UI spacing, radius, tooltip delay and press scale must be finite and not negative",
        ));
    }
    Ok(UiTheme {
        spacing: t.spacing,
        radius: t.radius,
        tooltip_delay: t.tooltip_delay,
        transition: crate::ui_animation::transition_from_c(&t.transition)?,
        press_scale: t.press_scale,
        ..theme_from_c(&t.theme, &UiTheme::default())?
    })
}

/// Fills the fields of `out_theme`'s version with the dark theme (Rust
/// `UiTheme::dark`): the default colors, controls a shade lighter than
/// panels, round corners.
/// # Safety
/// `out_theme` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_theme_ex_dark(out_theme: *mut PonkoUiThemeEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(out_theme, &theme_ex_to_c(&UiTheme::dark())) })
}

/// Fills the fields of `out_theme`'s version with the light theme (Rust
/// `UiTheme::light`): light panels, white controls, dark text, a blue
/// accent, round corners.
/// # Safety
/// `out_theme` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_theme_ex_light(out_theme: *mut PonkoUiThemeEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(out_theme, &theme_ex_to_c(&UiTheme::light())) })
}

/// Writes a UI's whole theme into the fields of `out_theme`'s version.
/// # Safety
/// `ctx` null or live; `out_theme` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_theme_ex(
    ctx: *mut PonkoCtx,
    target: u64,
    out_theme: *mut PonkoUiThemeEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_theme.is_null() {
            return Err(invalid("PonkoUiThemeEx output is null"));
        }
        with_ui(ctx, target, |ui| write_desc(out_theme, &theme_ex_to_c(ui.theme())))
    })
}

/// Replaces a UI's whole theme, as `ponko_ctx_ui_set_theme` does; the
/// tokens a caller's older version doesn't have are the default theme's.
/// # Safety
/// `ctx` null or live; `theme` null or a readable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_theme_ex(
    ctx: *mut PonkoCtx,
    target: u64,
    theme: *const PonkoUiThemeEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let theme = theme_ex_from_c(&read_desc(theme, theme_ex_to_c(&UiTheme::default()))?)?;
        ctx_access(ctx, |c| {
            if !c.gfx.is_live(theme.font) {
                return Err(invalid("unknown font"));
            }
            crate::ui_retained::ui_of(c, target)?.set_theme(theme);
            Ok(())
        })
    })
}

// ---- input ----------------------------------------------------------------------

/// The version of `PonkoUiInputEx` this header describes.
pub const PONKO_UI_INPUT_EX_VERSION: u32 = 1;

/// One frame of input with everything the widget set reads, in a layout
/// that can grow, for `ponko_ctx_ui_frame_ex`. Fill it with
/// `ponko_ui_input_ex_default`.
///
/// Version 1: `input`, `dt`, `secondary`, `menu`, `has_captured`,
/// `captured`.
/// ```
/// use ponko_ffi::*;
/// let mut i: PonkoUiInputEx = unsafe { std::mem::zeroed() };
/// (i.struct_size, i.version) = (size_of::<PonkoUiInputEx>() as u32, PONKO_UI_INPUT_EX_VERSION);
/// assert_eq!(unsafe { ponko_ui_input_ex_default(&mut i) }, PonkoStatus::PONKO_OK);
/// assert!(!i.secondary && !i.has_captured);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoUiInputEx {
    /// `sizeof(PonkoUiInputEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_UI_INPUT_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// What `PonkoUiInput` holds.
    pub input: PonkoUiInput,
    /// Seconds since the last frame: spinners turn and tooltips wait by it.
    pub dt: f32,
    /// The secondary (right) button went down: opens the context menu under
    /// the pointer.
    pub secondary: bool,
    /// The menu key (Shift+F10, a gamepad's North): opens the focused
    /// node's context menu.
    pub menu: bool,
    /// `captured` holds this frame's first press, for a listening
    /// key-binding field.
    pub has_captured: bool,
    /// The key, mouse or gamepad button pressed this frame.
    pub captured: PonkoActionBinding,
}
const _: () = assert!(offset_of!(PonkoUiInputEx, struct_size) == 0 && offset_of!(PonkoUiInputEx, version) == 4);

impl Versioned for PonkoUiInputEx {
    const NAME: &'static str = "PonkoUiInputEx";
    const VERSION: u32 = PONKO_UI_INPUT_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, captured: PonkoActionBinding)),
            _ => None,
        }
    }
}

fn input_ex_default() -> PonkoUiInputEx {
    PonkoUiInputEx {
        struct_size: size_of::<PonkoUiInputEx>() as u32,
        version: PONKO_UI_INPUT_EX_VERSION,
        input: crate::ui_retained::ponko_ui_input_default(),
        dt: 0.0,
        secondary: false,
        menu: false,
        has_captured: false,
        captured: PonkoActionBinding {
            kind: 0,
            pad: 0,
            first: 0,
            second: 0,
            third: 0,
            fourth: 0,
            deadzone: 0.0,
        },
    }
}

/// Fills the fields of `out_input`'s version with a frame where nothing
/// happens.
/// # Safety
/// `out_input` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_input_ex_default(out_input: *mut PonkoUiInputEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(out_input, &input_ex_default()) })
}

/// `ponko_ctx_ui_frame` with the widget set's input: lays the tree out and
/// handles one frame of `input` (null for none).
/// # Safety
/// `ctx` null or live; `input` null or a readable descriptor with its
/// header set, its `input.text` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_frame_ex(
    ctx: *mut PonkoCtx,
    target: u64,
    viewport: PonkoVec2,
    input: *const PonkoUiInputEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let input = match input.is_null() {
            true => ponko::ui::UiInput::default(),
            false => {
                let i = read_desc(input, input_ex_default())?;
                if !i.dt.is_finite() || i.dt < 0.0 {
                    return Err(invalid("dt must be finite and not negative"));
                }
                ponko::ui::UiInput {
                    dt: i.dt,
                    secondary: i.secondary,
                    menu: i.menu,
                    captured: i.has_captured.then(|| binding_from_c(&i.captured)).transpose()?,
                    ..ui_input_from_c(&i.input)?
                }
            }
        };
        with_ui(ctx, target, |ui| {
            ui.frame(to_c(viewport), &input);
            Ok(())
        })
    })
}

// ---- immediate widgets ------------------------------------------------------------

/// Draws an immediate checkbox at `rect` (window pixels, as `ponko_ctx_ui_button`) labelled `label`; a
/// click flips `*on`, and `out_changed` is true on that frame (Rust
/// `immediate_ui().toggle`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_toggle(std::ptr::null_mut(), Default::default(), c"Paths".as_ptr(), &mut false, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `label` NUL-terminated; `on` and `out_changed`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_toggle(
    ctx: *mut PonkoCtx,
    rect: PonkoRect,
    label: *const c_char,
    on: *mut bool,
    out_changed: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let label = crate::ctx::text(label)?;
            let Some(on) = on.as_mut() else {
                return Err(invalid("on is null"));
            };
            if out_changed.is_null() {
                return Err(invalid("output is null"));
            }
            out(out_changed, c.immediate_ui().toggle(to_c(rect), label, on))
        })
    })
}

/// Draws an immediate slider at `rect` (window pixels, as `ponko_ctx_ui_button`) for `*value`, 0 to 1:
/// pressing on it and dragging sets it; `out_changed` is true on the frames
/// it changes (Rust `immediate_ui().slider`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_slider(std::ptr::null_mut(), Default::default(), &mut 0.5, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `value` and `out_changed` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_slider(
    ctx: *mut PonkoCtx,
    rect: PonkoRect,
    value: *mut f32,
    out_changed: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(value) = value.as_mut() else {
                return Err(invalid("value is null"));
            };
            if out_changed.is_null() {
                return Err(invalid("output is null"));
            }
            out(out_changed, c.immediate_ui().slider(to_c(rect), value))
        })
    })
}

/// Draws an immediate progress bar at `rect` (window pixels, as `ponko_ctx_ui_button`) filled to `value`,
/// 0 to 1 (Rust `immediate_ui().progress_bar`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_progress_bar(std::ptr::null_mut(), Default::default(), 0.5) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_progress_bar(ctx: *mut PonkoCtx, rect: PonkoRect, value: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.immediate_ui().progress_bar(to_c(rect), value);
            Ok(())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_theme_survives_the_trip_through_c() {
        for theme in [UiTheme::dark(), UiTheme::light()] {
            assert_eq!(theme_ex_from_c(&theme_ex_to_c(&theme)).ok(), Some(theme));
        }
        let mut bad = theme_ex_to_c(&UiTheme::dark());
        bad.radius = -1.0;
        assert!(theme_ex_from_c(&bad).is_err());
    }

    #[test]
    fn bindings_go_both_ways() {
        let b = ButtonBinding::Mouse(ponko::prelude::MouseButton::Right);
        let c = binding_to_c(&b).unwrap();
        assert_eq!(binding_from_c(&c).ok(), Some(b));
        let axis = PonkoActionBinding { kind: 4, ..c };
        assert!(binding_from_c(&axis).is_err());
    }

    #[test]
    fn no_index_is_none() {
        assert_eq!(index_from_c(PONKO_UI_NO_INDEX), None);
        assert_eq!(index_to_c(Some(3)), 3);
    }
}
