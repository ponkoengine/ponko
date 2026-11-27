<!-- area: Development and releases -->
### Fixed
- `ponko_cli`'s tests pass at a release candidate's version: the template
  check expects a candidate's own `ponko = "0.y.0-rc.N"` pin (Cargo never
  picks a candidate for `"0.y"`), and the cook fixture tests compare the
  cooked Aseprite sheet and the kept-geometry model apart from the version
  the cook stamps into them (`meta.version`, each KTX2 image's
  `KTXwriter`), checking that stamp is the current version.
- The workspace docs build again with warnings denied (a broken link to
  `Ui::image` in the docs of `render_icon`), and the engine's tests
  build without warnings with a window but no audio (the clippy lines
  "engine with only a window", "with every image format" and "with text
  shaping and system fonts").
