### Added
- The editor lists the glTF scenes of the open scene that didn't load in a
  panel above the status bar, one row each with **Retry**, the file and
  why. A retry that loads it spawns the scene where it was saved, with its
  overrides, and hangs the nodes of other scenes saved on its nodes off
  them again; one that fails says why in its row.
- The browser smoke runner takes `--persist granted|refused` (it answers
  `navigator.storage.persist()` for the browser and counts the asks),
  `--click <x>,<y>=<field>:<value>`, and from `fixtures.json` a page
  `query` and `editMode` (a page opened in the editor). The new
  `web_editor_unloaded` fixture clicks **Retry** in the editor's panel.

### Changed
- The editor's web build no longer asks the browser for persistent storage
  before setup: it only looks whether its storage is persistent, and the
  first save on the page asks (once), as saving is something the person
  did. `Assets::browser_saves_persistent` is `Some(false)` until then.
  File > Saved in this browser says the next save asks, and the warning
  that the browser may clear saves goes once it grants it.
