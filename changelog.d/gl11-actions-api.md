### Changed
- **Breaking:** `ButtonBinding`, `AxisBinding`, `Axis2Binding`, `Action` and
  `InputEvent` are non-exhaustive; add `_` arms to external matches.
- **Breaking:** `ActionMap::from_json` and `to_json` return the engine `Error`;
  use `ErrorKind::Json` to classify failures. Decode messages retain location
  and action-map context. Serde traits and JSON format remain unchanged.

### Added
- `ButtonState::{Released, Held}` and `InputEvent::key`, `mouse` and
  `gamepad_button` constructors name synthetic held state. Boolean variants
  remain available; repeating held input does not repeat pressed edges.
- `InputEvent` supports `PartialEq` (ordinary float equality), and `ActionMap`
  equality compares persistent bindings, ignoring runtime state.

### Breaking changes
- Action and binding enums require wildcard match arms.
  Instead: handle unknown variants with a `_` arm.
  Before:
  ```rust
  match action { Action::Button(_) => 0, Action::Axis(_) => 1, Action::Axis2(_) => 2 }
  ```
  After:
  ```rust
  use ponko::actions::Action;
  # let action: Action = todo!();
  match action { Action::Button(_) => 0, Action::Axis(_) => 1, Action::Axis2(_) => 2, _ => 3 }
  ```
- Action-map JSON methods return engine errors.
  Instead: use `ponko::error::Error` and classify with `ErrorKind::Json`.
  Before:
  ```rust
  fn load(s: &str) -> Result<ActionMap, serde_json::Error> { ActionMap::from_json(s) }
  ```
  After:
  ```rust
  use ponko::actions::ActionMap;
  fn load(s: &str) -> Result<ActionMap, ponko::error::Error> { ActionMap::from_json(s) }
  ```
