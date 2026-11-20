### Added
- The editor's asset panel, along the bottom of its window: the assets
  folder as a tree, a thumbnail per texture, model, sound, prefab and scene
  (made on a worker thread), search (Ctrl+F), drag a texture, model or
  prefab into the view to place it (a texture becomes a sprite in 2D),
  double-click a scene or prefab to open it, a sound's preview, and the
  selected asset's import settings: its rule in `ponko cook`'s `cook.json`,
  shown with where it comes from and changed in place.
- Prefab instances in the editor: a prefab placed in the editor keeps its
  file in the new reflected `hierarchy::PrefabInstance` component, and the
  inspector lists how the instance differs from its prefab (changed
  components with their fields, added entities, missing parts), with
  Revert, Revert all, Apply to prefab (which updates the scene's other
  instances, keeping their overrides) and Open prefab. A saved prefab
  document has Apply to scenes, which updates its instances in every scene.
- Play mode in the editor: Play, Pause, Step and Stop (Ctrl+P,
  Ctrl+Shift+P, F10, Ctrl+.) and the speed on a play bar. Stop puts the
  world back exactly as it was at Play; edits made through the editor while
  playing are listed, and the ones ticked are kept as one undo step.
- The editor remembers its panel sizes, the asset panel, recent projects
  and files, and rebound shortcuts in `editor.json` in the user's settings
  folder (`PONKO_CONFIG_DIR` overrides it). File → Open recent, and
  Help → Shortcuts (F1) lists every shortcut and rebinds it.
- `ponko editor --recent` lists the recent projects and their files, and
  `ponko editor --last` opens the project and file opened last.

### Fixed
- The editor's selection rings and gizmo are drawn only over the view, no
  longer over the panels.
- The editor's hierarchy marks prefab instances with a glyph its fonts
  have (it drew a missing-glyph box).
