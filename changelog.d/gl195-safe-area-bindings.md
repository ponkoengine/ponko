### Added

- The window's safe area from C and C++: `ponko_ctx_window_safe_area`,
  `_safe_insets` and `_feed_safe_insets` (`PonkoSafeInsets`; C++
  `ctx.window_safe_area()`, `window_safe_insets()` and
  `window_feed_safe_insets(insets)`), and UI roots laid out inside it with
  `ponko_ctx_ui_set_inside_safe_area` and `_inside_safe_area` (C++
  `set_inside_safe_area` and `inside_safe_area`).
- A scenario step that feeds safe area insets, so a layout for a notched
  phone is checked without a device: `InputScenario::safe_insets(tick, insets)`,
  the `safe_insets` step of the JSON form (also for the control endpoint and
  the `run_input_scenario` tool), `ponko_input_scenario_safe_insets` and C++
  `input_scenario::safe_insets`.
