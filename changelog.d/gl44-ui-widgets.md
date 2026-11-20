### Added
- The retained UI's widget set: `Ui::dropdown`, `tabs` and `tab`,
  `radio_group` and `radio`, `progress_bar`, `spinner`, `number_field`,
  `list` and `grid_view` (virtualized: nodes only for the rows on screen, so
  a million items cost what twenty do), `split` (`UiAxis`; its value is
  the first pane's share of the length the divider leaves, 0.5 for equal
  panes) and `key_binding` (it shows "Press a key" while it listens), with the builder methods `items`, `selected`, `range`,
  `step`, `tooltip`, `context_menu`, `draggable` and `drop_target`. Their
  state is read and set with `selected`/`set_selected`, `items`/`set_items`,
  `number`/`set_number`/`set_number_range`, `set_list_len`, `list_rows`,
  `list_item`, `scroll_to_item`, `binding`/`set_binding`/`is_capturing`,
  `set_tooltip`/`shown_tooltip`, `set_context_menu`/`open_context_menu`/
  `popup_owner`/`close_popup`, `set_draggable`, `set_drop_target`,
  `dragging` and `set_parent`. `Ui::widget(id)` names a node's widget.
- New `UiEvent`s: `Chosen(node, entry)` for a context-menu entry,
  `Dropped(item, onto)` for drag and drop (the game moves the node), and
  `RowsShown(list)` when a list's rows show other items.
- Every widget works with the pointer, the keyboard and a gamepad, and a
  modal screen keeps the focus inside it. Tooltips show after hovering for
  `UiTheme::tooltip_delay`, or at once on keyboard focus.
- Themes: `UiTheme::dark()` and `UiTheme::light()`, and the tokens
  `spacing`, `radius` (rounded corners) and `tooltip_delay`.
- Screen readers: the roles `ComboBox`, `TabList`, `Tab`, `TabPanel`,
  `RadioGroup`, `RadioButton`, `ProgressBar`, `Spinner`, `SpinButton`,
  `List`, `ListItem`, `Grid`, `GridCell`, `Menu`, `MenuItem` and `Splitter`,
  the value `UiAccessValue::Number`, and the node states `selected`,
  `expanded` and `position`.
- The immediate UI uses the same names: `toggle`, `slider` and
  `progress_bar`.
- C: a builder and accessors for every widget (`ponko_ctx_ui_add_dropdown`,
  `..._add_tabs`, `..._add_list`, ...), the descriptors `PonkoUiEventEx`
  (`ponko_ctx_ui_next_event_ex`), `PonkoUiThemeEx` (`ponko_ui_theme_ex_dark`
  and `_light`, `ponko_ctx_ui_theme_ex`, `ponko_ctx_ui_set_theme_ex`),
  `PonkoUiInputEx` (`ponko_ctx_ui_frame_ex`) and `PonkoUiAccessStateEx`
  (`ponko_ui_access_state_ex`), the new roles and event kinds, and
  `ponko_ctx_ui_toggle`, `ponko_ctx_ui_slider` and
  `ponko_ctx_ui_progress_bar`. C++: the matching `ui::tree` methods,
  `ui::theme_ex`, `ui::dark_theme`/`light_theme`, `ui::input_ex`, the new
  `ui::role` and `ui::event_kind` values, `ui::access::state_of`, and
  `ctx::ui_toggle`, `ui_slider` and `ui_progress_bar`.
- A test that every control's text has at least WCAG's 4.5:1 contrast
  against the control's own fill in every state (idle, hovered, focused,
  pressed, disabled) and theme, and GPU tests that check it on the drawn
  pixels.
- `examples/ui_widgets.rs` and the guide's "Widget gallery" page, with GPU
  reference images of every widget and a `ponko_bench` case (`ui/list`) for
  the list's flat cost.

### Changed
- **Breaking:** `UiInput` has the new fields `dt`, `secondary`, `menu` and
  `captured`. A struct literal without `..Default::default()` must add them.
  The engine fills them itself when it runs the UI.
- **Breaking:** `UiTheme` has the new fields `spacing`, `radius` and
  `tooltip_delay`. Build themes with `..UiTheme::default()`, `dark()` or
  `light()`. The default theme draws exactly as before (radius 0).
- **Breaking:** `UiAccessNode` has the new fields `selected`, `expanded` and
  `position`. A literal or exhaustive pattern must add them (or `..`).
- Controls draw their text in the middle of their height, at least three
  quarters of the theme's `spacing` in from their left and right edges, and buttons,
  number fields and key-binding fields centre it across too
  (`UiTheme::paint` gives them `UiTextAlign::Center`). A button's text used
  to sit at its top left: apps that relied on that set
  `paint.align = UiTextAlign::Start` on it; the default theme draws buttons
  otherwise as before. The reference images with buttons and checkboxes
  were redrawn.
- C++: `ui::tree::next_event` and `events` read `ponko_ctx_ui_next_event_ex`,
  so they now also return the `chosen`, `dropped` and `rows_shown` events.
  C's `ponko_ctx_ui_next_event` is unchanged and still skips those kinds.
