### Added
- Inspector: the **Debug drawing** section lists every 3D camera (the
  window's, each extra view's, each target camera's) with its
  `Camera::debug_draw` as three buttons: **Default**, **On** and **Off**.
- glTF scenes keep a node attached to the root or a node of another glTF
  scene on it through a save, a reopen and the editor's Play/Stop,
  whichever scene spawns first. A scene's saved record takes an `"id"`
  (older records are numbered on load), and a node attached to another
  scene's root or node takes `"scene_parent": { "scene": id, "node": path,
  "link": offset }` (no `"node"` for a root).

### Fixed
- Editor: a click on a hierarchy row selects its entity. The row's drag
  area lay over it and egui gave the click to nothing, so only clicks in
  the view selected.
