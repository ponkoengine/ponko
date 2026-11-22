### Added
- Key labels from C and C++: `ponko_ctx_key_label` (a key's label on the
  player's keyboard), `ponko_ctx_glyph_label` (a glyph's),
  `ponko_ctx_key_labels_changed` and `ponko_ctx_set_key_labels` with
  `PonkoKeyLabel`; C++ `ctx.key_label`, `layout_glyph_label`,
  `key_labels_changed` and `set_key_labels`.
- Key labels follow the active xkb layout on Linux and the BSDs, under X11
  (the X server's XKB map and state) and Wayland (the compositor's keymap
  and active layout), from the first frame and as the player switches
  layouts.
- In a browser, key labels come from the Keyboard Layout Map
  (`navigator.keyboard.getLayoutMap()`) where it exists (Chromium-based
  browsers, secure pages), asked when the page opens and regains focus;
  elsewhere they are still learned from the keys pressed. The headless
  runner `web/smoke/key-labels.mjs` checks both and a refused map.

### Changed
- On macOS and Windows the system's layout read when the window regains
  focus now reaches `ctx.input.key_labels()` at the start of the next
  frame rather than in the focus event (the change edge is unchanged).
