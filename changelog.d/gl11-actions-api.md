### Added
- `ButtonState::{Released, Held}` and `InputEvent::key`, `mouse` and
  `gamepad_button` constructors name synthetic held state. Boolean variants
  remain available; repeating held input does not repeat pressed edges.
- `InputEvent` supports `PartialEq` (ordinary float equality), and `ActionMap`
  equality compares persistent bindings, ignoring runtime state.

### Breaking changes
- `ButtonBinding`, `AxisBinding`, `Axis2Binding`, `Action` and `InputEvent` are `#[non_exhaustive]`: a `match` on one needs a `_` arm.
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
- `ActionMap::from_json` and `to_json` return the engine's `ponko::error::Error`.
  Decode messages keep the location and the action map's context; the serde
  traits and the JSON format are unchanged.
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
