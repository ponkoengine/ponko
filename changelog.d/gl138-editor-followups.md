### Added
- Editor: **Apply to prefab** works on an instance of a prefab that includes
  other prefabs, writing each included prefab back as its link with how
  this instance changes it, and a selected part of an included prefab has
  **Apply to <name>**, which writes it into the included prefab's own file;
  every instance in the scene using it follows, in one undo step.
- Prefab files: an included prefab's entry takes `"remove"` (components its
  root doesn't get) and `"parts"` (changes to its other entities, by their
  place in its `PrefabParts`) beside `"components"`.
- glTF scenes keep how their nodes were edited (moved, components changed,
  added or removed) as overrides in the scene's saved record, put back when
  the file is exported again (hot reload) and when a loaded save spawns the
  file again; the editor saves them with the scene and spawns a scene's
  glTF files again when it opens it.
- Editor: model thumbnails are drawn by the engine's renderer on the game's
  GPU (offscreen, one a frame, the software drawing first) and kept in the
  user's cache folder under a hash of the model and the files it names
  (`PONKO_THUMBNAIL_CACHE` names another folder).
- Editor: a texture dropped on a model in the 3D view paints it; a
  `ModelMaterial` can be `{ "texture": "path" }`, a material of that image
  made when the scene or prefab naming it loads.

### Changed
- Editor: the 2D and 3D cameras draw into the free view between the panels
  (their `rect`), so what they look at is in its middle; the view follows
  resized panels, but not during a drag.

### Fixed
- Editor: a click on a texture, model or prefab tile in the asset panel
  selects it (it started a drag instead).
- Editor: 3D clicks and gizmos follow a camera whose rect isn't the whole
  window.
