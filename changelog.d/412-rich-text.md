### Added
- Rich text in retained UI labels: `Ui::set_rich` (or `UiBuilder::rich`)
  reads a node's text as markup, `[color=#ffcc00]`, `[b]`, `[font=name]`,
  `[size=24]`, `[u]`, `[link=target]` and `[img=name alt="A"]`, with `[[`
  for a bracket; anything else in brackets is shown as it is. Runs wrap,
  align and line up on one baseline; inline images (button glyphs) are as
  tall as the text. `Ui::set_inline_image` and `Ui::set_inline_font` name
  images and fonts for it (the font named `bold` is `[b]`'s, else the
  node's font is drawn heavier). A click on a link sends
  `UiEvent::LinkClicked(node, index)`, and `Ui::link` gives its target.
  `Ui::accessible_name` is the text without markup, images as their alt
  text. Keyed text (`Ui::set_text_key`) can be rich: the string table
  entry holds the markup and text arguments are escaped. With
  `text-shaping` each run is shaped and right-to-left runs are put in
  visual order. Rust-only for now.
- `tests/fixtures/render/rich_text_dialogue.png`: a wrapped dialogue line
  with a colored name and an inline button glyph, and
  `rich_text_arabic.png`, a right-to-left line with a colored first word.
