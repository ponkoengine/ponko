### Added
- `scripts/api-usage.py` lists, from the API snapshots, where every public
  item is called (reference games, examples, cookbook recipes, guide
  snippets, tests, the C and C++ bindings, the other crates) and which items
  nothing calls by name. Its report, `scripts/api-usage.md`, groups those by
  module with the decision recorded for each in
  `scripts/api-usage-decisions.txt`; `api-usage.py check` (in the check
  list) keeps every decision pointing at a real item.
- Tests now call the unused items that are real features: controllers and
  joysticks, snapshot buffers and clock sync, latest-wins filters, prediction
  numbering, AI trees and considerations, plural tables, value schemas,
  world hash parts, tile animations, waves and currents, camera shake, rigs
  and pixel snapping, 2D skeleton graphs and constraints, UI read-back, touch
  control places and styles. The rest are kept, each with its reason in the
  report; nothing was removed or renamed.
