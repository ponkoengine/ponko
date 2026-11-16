### Added
- `ponko::math`, a new root module: glam's `Vec2`, `Vec3`, `Vec4`, `Quat`,
  `Mat4`, `UVec2`, `EulerRot` and the constructors `vec2`, `vec3`, `vec4`,
  `uvec2`, re-exported as they are. It is their home; the prelude's math
  names are the same items. A game that writes `use ponko::*;` and depends on
  a crate named `math` now spells that crate `::math::...` (STABILITY.md,
  "The crate root and globs").
- `ponko::reflect::JsonValue` and `ponko::reflect::JsonMap`: `serde_json`'s
  `Value` and `Map<String, Value>` under the engine's names, so a game can
  name reflection and migration values without its own `serde_json`
  dependency. Reflection, save-format and migration signatures use them
  (the same types as before). `ponko_mcp` has `JsonValue`, `JsonMap` and
  `json!` for `Server::handle`.
- The prelude gains `ui::UiEvent`, `ui::UiLayout` and the gamepad types
  `input::Gamepad`, `GamepadAxis`, `GamepadButton` and `GamepadStick`: the
  tutorials' menus and gamepad bindings need no import beyond the prelude
  and their dimension module. `tests/first_game_imports.rs` lists what the
  tutorials and the reference games still import, with the reason for each.
- STABILITY.md, "Re-exported crates": what an upgrade of `glam`,
  `serde_json` or `steamworks` means before and after 1.0 (from 1.0 on, an
  incompatible `glam` or `serde_json` comes only with a Ponko major
  release), and that the prelude's contents change only in minor releases.

### Changed
- **Breaking:** the prelude no longer exports `AssetSource`, `MemorySource`,
  `PackSource` (import them from `ponko::assets`) or the doc-hidden
  `SetupResult` (`ponko::game::SetupResult`). Calling `read` or `contains`
  on a source needs the `AssetSource` trait in scope. A game's own type named
  `UiEvent`, `UiLayout`, `Gamepad`, `GamepadAxis`, `GamepadButton` or
  `GamepadStick` that it glob-imports next to the prelude is now ambiguous:
  import it by name, or use the engine's.

### Breaking changes
- The prelude no longer exports `AssetSource`, `MemorySource`, `PackSource` and `SetupResult`.
  They are what a game names only when it adds an asset source or writes a
  bound on fallible setup, so they stay in their modules.
  Instead: import them from `ponko::assets` (`ponko::game::SetupResult`);
  `source.read(name)` needs `AssetSource` in scope.
  Before:
  ```rust
  use ponko::prelude::*;
  ctx.assets.add_source(MemorySource::new("embedded").with("tuning.json", TUNING));
  ```
  After:
  ```rust
  use ponko::assets::MemorySource;
  use ponko::prelude::*;
  ctx.assets.add_source(MemorySource::new("embedded").with("tuning.json", TUNING));
  ```
