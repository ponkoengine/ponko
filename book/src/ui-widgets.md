# Widget gallery

The retained UI ([Retained game UI](ui.md)) has a set of widgets beyond
buttons, labels, toggles and sliders: everything a settings menu, a shop
and an inventory need. Each one is a node like any other. It takes a
layout, a paint and a name, and it raises the same `UiEvent`s. Each
works with the mouse, the keyboard and a gamepad, and each has a role
that screen readers announce.

The full gallery is `examples/ui_widgets.rs` (`cargo run --example
ui_widgets`). `tests/ui_widgets.rs` drives every widget headless, and
`src/render/ui_widgets_render_test.rs` keeps reference images of each one
in the dark and light themes, open and closed.

## The widgets

| Widget | Builder (Rust / C++) | Keys and gamepad | Role | Events |
|---|---|---|---|---|
| Dropdown | `dropdown(parent, options)` / `add_dropdown` | confirm opens; arrows move; confirm chooses; cancel closes | combo box (open or closed) | `Changed` |
| Tabs | `tabs(parent)`, `tab(tabs, title)` / `add_tabs`, `add_tab` | arrows move between tabs; confirm chooses one | tab list, tab, tab panel | `Changed` on the strip |
| Radio group | `radio_group(parent)`, `radio(group, text)` / `add_radio_group`, `add_radio` | arrows move; confirm checks one | radio group, radio button (n of m) | `Changed` on the group |
| Checkbox | `toggle(parent, text)` / `add_toggle` | confirm flips it | checkbox | `Changed` |
| Progress bar | `progress_bar(parent)` / `add_progress_bar` | none (it takes no input) | progress bar (0 to 1) | none |
| Spinner | `spinner(parent)` / `add_spinner` | none | spinner (busy) | none |
| Number field | `number_field(parent, value)` / `add_number_field` | left and right step; digits type a value; confirm keeps it | spin button (value, range and step) | `Changed` |
| List | `list(parent, row_height)` / `add_list(parent, h)` | arrows choose; confirm submits | list, list item (n of m) | `Changed`, `Submitted`, `RowsShown` |
| Grid | `grid_view(parent, columns, row_height)` / `add_list(parent, h, columns)` | arrows move in two dimensions | grid, grid cell | as a list |
| Modal dialog | `push_modal(root)` | Tab and arrows stay inside it; cancel closes it | dialog | `ScreenPopped` |
| Context menu | `.context_menu(entries)` / `set_context_menu` | the secondary button, Shift+F10 or a gamepad's North opens it | menu, menu item | `Chosen(node, entry)` |
| Split panes | `split(parent, axis)` / `add_split` | arrows move the divider | splitter (the first pane's share of the length the divider leaves; 0.5, equal panes, at first) | `Changed` |
| Drag and drop | `.draggable()`, `.drop_target()` / `set_draggable`, `set_drop_target` | confirm picks up; arrows choose a target; confirm drops; cancel puts it back | | `Dropped(item, onto)` |
| Key binding | `key_binding(parent, binding)` / `add_key_binding` | confirm listens (it shows "Press a key"); the next key, mouse or gamepad button binds; Escape stops | button | `Changed` |
| Tooltip | `.tooltip(text)` / `set_tooltip` | shown at once when keys focus the node | the node's description | none |

"Confirm" is Enter or a gamepad's South button, and "cancel" is Escape or
East. "Arrows" means the arrow keys, the d-pad or the left stick. These
are the same `UiInput` fields the engine fills from the keyboard and from
any gamepad.

## A settings page

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/ui_widgets.rs:settings}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:widgets_settings}}
```

</div>
</div>

A dropdown, a tab strip, a radio group and a list each have one chosen
entry, read with `selected` (`None` or `ui::no_index` when nothing is
chosen). `set_selected` chooses one from code without raising an event. A
radio group starts with nothing chosen. A tab strip and a dropdown start
on their first entry.

## An inventory: lists, drag and drop, menus

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/ui_widgets.rs:inventory}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:widgets_inventory}}
```

</div>
</div>

A list or grid keeps nodes only for the rows on screen and reuses them as
it scrolls. A list of a million items has the same number of nodes as a
list of twenty. Give it text items with `set_items`, or only a length
with `set_list_len` and draw the cells yourself. `list_rows(list)` gives
each row node with the item it shows now, and `RowsShown` says when that
changed.

Scrolling a 300 × 480 list of 24-unit rows costs about the same whatever
the list holds. These are medians of 300 scrolled frames each, in a
release build on an Apple M4 Max (`cargo run --release -p ponko_bench --
ui/list`):

| Items | Per scrolled frame |
|---|---|
| 100 | 0.046 ms |
| 10 000 | 0.050 ms |
| 1 000 000 | 0.052 ms |

When a dragged node is let go on a drop target, the UI raises
`Dropped(item, onto)` and moves nothing itself: the game decides. If it
accepts the drop, it moves the node with `set_parent`.

## Events

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/ui_widgets.rs:events}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:widgets_events}}
```

</div>
</div>

C reads these events with `ponko_ctx_ui_next_event_ex`. Its
`PonkoUiEventEx` also holds the drop target (`other`) and the chosen
entry (`index`). The older `ponko_ctx_ui_next_event` still works but
skips those kinds, and both functions read from the same queue.

```c
{{#include ../../examples/cpp/guide.c:ui_widgets_c}}
```

## A modal dialog

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/ui_widgets.rs:dialog}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:shop_dialog}}
```

</div>
</div>

A screen pushed with `push_modal` traps the focus: Tab, the arrows and
the d-pad cycle through its controls and never reach the screens below
it. It also takes every pointer press, dims what is under it, and is the
only thing a screen reader sees until it closes.

## Themes

`UiTheme::dark()` and `UiTheme::light()` are the two built-in themes.
Besides colors, a theme has three tokens the widgets read: `spacing` (the
room inside pop-ups, rows and tabs; a control's text stays three quarters of it in
from the control's sides), `radius` (corner rounding, 0 for
square corners) and `tooltip_delay` (seconds of hovering before a tooltip
shows). Both built-in themes give text in every control at least 4.5:1
contrast against the control's fill in each state (idle, hovered, focused,
pressed, disabled), the WCAG minimum for body text; a test checks this.
To make a custom theme, start from either one:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/ui_widgets.rs:theme}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:widgets_theme}}
```

</div>
</div>

In C, `PonkoUiThemeEx` holds the whole theme, the tokens included. Fill
one with `ponko_ui_theme_ex_dark` or `ponko_ui_theme_ex_light`, then
apply it with `ponko_ctx_ui_set_theme_ex`.

## Immediate widgets with the same names

The immediate UI (`ctx.immediate_ui()`) uses the same names for the
widgets it shares, so debug panels read like the retained tree:
`toggle(rect, label, &mut on)`, `slider(rect, &mut value)` and
`progress_bar(rect, value)`, next to `button`, `label` and `panel`. C has
`ponko_ctx_ui_toggle`, `ponko_ctx_ui_slider` and
`ponko_ctx_ui_progress_bar`. In C++ they are `ctx::ui_toggle`,
`ctx::ui_slider` and `ctx::ui_progress_bar`.

```rust
# use ponko::prelude::*;
# fn debug(ctx: &mut Ctx, paths: &mut bool, volume: &mut f32) {
let ui = &mut ctx.immediate_ui();
ui.toggle(Rect::new(20.0, 20.0, 160.0, 24.0), "Paths", paths);
ui.slider(Rect::new(20.0, 50.0, 160.0, 20.0), volume);
ui.progress_bar(Rect::new(20.0, 80.0, 160.0, 8.0), 0.4);
# }
```

## Screen readers

Every widget has a role: combo box, tab list, tab, tab panel, radio
group, radio button, progress bar, spinner, spin button, list, list item,
grid, grid cell, menu, menu item or splitter. `UiAccessNode` adds what
the role needs:
- `selected` for the chosen entry,
- `expanded` for a dropdown that is open or closed,
- `position` for "3 of 12",
- a number value with its range and step.

C reads these with `ponko_ui_access_state_ex`, and C++ with
`ui::access::state_of`. [Screen reader records](ui-accessibility.md)
explains how the tree reaches the system's screen reader.
