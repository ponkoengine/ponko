### Added
- Opening a scene in the editor whose glTF scenes fail to load (a file
  missing or broken) names each file and why in the status bar's error
  line, not only on the console; the scene opens without them.
- The editor's web build asks the browser for persistent storage before
  setup. When the browser refuses, the status bar says it may clear the
  saves kept in the browser, and **File > Saved in this browser** repeats
  it. `Assets::browser_saves_persistent` is the browser's answer from code
  (`None` until it answers, and in other builds).
- The browser smoke runner takes several reload steps (`"reloads"` in
  `fixtures.json`, `--reload` on the command line), each a page load of
  its own with its own expected fields. The web editor's save fixture
  saves, reloads and reverts, then reloads again to check the revert held.

### Fixed
- The `web_editor_save` example run on the desktop no longer writes over
  the shipped `assets/levels/beach.json`.
