### Breaking changes
- `ScenarioCheck` is `#[non_exhaustive]` and has a new `Ui` variant.
  Code outside the engine can no longer match it exhaustively.
  Instead: add a `_` arm to a `match` on it, and build checks with
  `ScenarioCheck::action`, `resource`, `count` and `ui`.
  Before:
  ```rust
  match check {
      ScenarioCheck::Action { name, .. } => println!("action {name}"),
      ScenarioCheck::Resource { name, .. } => println!("resource {name}"),
      ScenarioCheck::Count { component, .. } => println!("count {component}"),
  }
  ```
  After:
  ```rust
  use ponko::scenario::ScenarioCheck;
  # let check: ScenarioCheck = todo!();
  match check {
      ScenarioCheck::Action { name, .. } => println!("action {name}"),
      ScenarioCheck::Resource { name, .. } => println!("resource {name}"),
      ScenarioCheck::Count { component, .. } => println!("count {component}"),
      ScenarioCheck::Ui { node, .. } => println!("ui {node}"),
      _ => {}
  }
  ```

### Added
- `Game::headless_frames(true)`: headless runs (`run_headless`,
  `run_headless_scenario`, `profile_headless`) run whole frames, the retained
  UI and the frame, `Animate`, `Late` and `Draw` systems included, in a virtual
  window of the game's size, with no window or GPU. Off by default; a
  simulation-only headless run is unchanged. C: `ponko_game_headless_frames`;
  C++: `game::headless_frames`.
- Input scenarios act on the retained UI by node name: `InputScenario::click`
  and `type_text` (JSON `ui_click`, `ui_type`), and `ScenarioCheck::ui` checks
  that a node exists, is shown, has the focus or has a text (JSON
  `expect_ui`). With headless frames, a tick's input reaches the UI in the
  same frame and its checks run after the whole frame. C:
  `ponko_input_scenario_click`, `ponko_input_scenario_type_text`,
  `ponko_input_scenario_expect_ui`; C++: `input_scenario::click`, `type_text`,
  `scenario_check::ui`.
- `Ui::is_shown`: the node is on a screen of the stack and not hidden. C:
  `ponko_ctx_ui_is_shown`; C++: `ui::tree::is_shown`.
- `tests/coin_dash_menus.rs` plays the coin_dash menus headlessly, checking
  the UI at every step; the benchmarks `headless/sim-only-10k-ticks` and
  `headless/frames-10k-ticks`.
