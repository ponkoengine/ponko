### Changed
- A glTF scene that fails to spawn again when the editor's Stop puts the
  world back (its file broke or went missing while playing) is listed in
  the panel of scenes that didn't load, with its reason and a **Retry** that
  works as for scenes that failed at start, and the error line names it;
  before, only the console said. A scene that spawns at Stop leaves the
  panel.
- The editor in a web build sets `<html data-ponko-retry>` to where its
  Retry buttons are (`[x, y]` fractions of the window), and the browser
  smoke runner's `--click @retry=<field>:<value>` clicks the first one it
  finds, so the `web_editor_unloaded` fixture no longer depends on the
  status bar's layout. `ci.yml`'s list of checks gains the fixture's step
  and the `web_editor_save` runs with `--persist granted` and
  `--persist refused`.
