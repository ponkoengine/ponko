### Changed
- `UiRole::as_str` names any role, including ones added later. From C, a role or value without an ABI-2
  constant reads as `PONKO_UI_ROLE_GROUP` or `PONKO_UI_ACCESS_VALUE_NONE`.

### Added
- `ui::ScreenMode { Modeless, Modal }` (`#[non_exhaustive]`, default
  `Modeless`) names how a pushed screen treats the ones below it:
  `Ui::push_screen_with_mode(root, mode)`, the shorthands `Ui::push_modal` and
  `Ui::push_modeless`, and `Ui::screen_mode(root)` to read it back.
- C: `PonkoUiScreenMode` and `ponko_ctx_ui_push_screen_with_mode(ctx, target,
  root, mode)`; an unknown mode is `PONKO_INVALID_ARGUMENT`. C++:
  `ponko::ui::screen_mode`, `tree::push_screen(root, mode)`, `push_modal` and
  `push_modeless`.
- `UiLayout::unstable_custom`, `unstable_style`, `unstable_style_mut` and
  `into_unstable_style`: the Taffy escape hatch, named so that its exemption
  from the stability promise is visible at the call site. STABILITY.md lists
  them, and the old names, as unstable.

### Deprecated
- `Ui::push_screen(root, modal: bool)`: use `push_modeless(root)`,
  `push_modal(root)` or `push_screen_with_mode(root, mode)`. C's
  `ponko_ctx_ui_push_screen` stays in ABI 2 (use
  `ponko_ctx_ui_push_screen_with_mode`); C++'s `push_screen(root, bool)`
  overload is `[[deprecated]]`.
- `UiLayout::custom`, `style`, `style_mut` and `into_style`: use
  `unstable_custom`, `unstable_style`, `unstable_style_mut` and
  `into_unstable_style`. Same behaviour.

### Breaking changes
- `UiRole`, `UiAccessValue` and `UiAccessAction` are `#[non_exhaustive]`: a `match` on one needs a `_` arm.
  New roles, values and assistive-technology requests can then come in minor
  releases.
  Instead: handle roles, values and requests added later with a `_` arm.
  Before:
  ```rust
  let polite = match role { UiRole::Button | UiRole::CheckBox | UiRole::Slider | UiRole::TextInput => true, UiRole::Group | UiRole::Dialog | UiRole::Label | UiRole::Image | UiRole::ScrollView => false };
  ```
  After:
  ```rust
  use ponko::ui::UiRole;
  # let role = UiRole::Button;
  let polite = matches!(role, UiRole::Button | UiRole::CheckBox | UiRole::Slider | UiRole::TextInput);
  ```
