<!-- area: C and C++ -->
### Added

- C and C++: a brain row in a query loop (also a parallel one) can be signalled with a value, as Rust's `Brain::signal_with`: `ponko_ai_brain_signal_num`, `_entity`, `_vec2` and `_vec3` (C++ `ai::brain_row::signal_num`, `signal_entity`, `signal_vec2`, `signal_vec3`).
- C and C++: a water body's pending contact ripples are readable (Rust `Water::waiting_contacts`): `ponko_ctx_water_waiting_contacts`, `ctx.water_waiting_contacts`, so a test sees a floating body's ripple switch by its effect.
- C and C++: a UI of the game's own, such as a world panel's, has its safe area set and read (Rust `Ui::set_safe_area` and `safe_area`): `ponko_ctx_ui_set_safe_area`, `ponko_ctx_ui_safe_area`, and the UI's `set_safe_area`, `clear_safe_area` and `safe_area` in C++.

### Changed

- C++: `ai::each` forwards to the typed `world().each`, which the guide names as the one way to loop over brains. Existing callers compile as before.
