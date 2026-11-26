### Added
- The guide's chapter "The `ponko` command" (`book/src/cli.md`) lists every command and option `--help` shows;
  a test of `ponko_cli` fails when one is missing there or isn't accepted, and runs `pack`, `cook`,
  `textures`, `new`, `editor --print` and `notices` for real in a temporary folder.
- Headless tests open every menu of the editor, list its entries and use each one, and use every
  button of the menu bar and the gizmo row, every entry of a hierarchy row's menu, every shortcut,
  and every section and button of the overlay inspector (in an empty game too): a new entry that
  does nothing fails them. What only a person can judge is in
  `scripts/checklists/editor-and-inspector.md`.

### Fixed
- The editor's **Window → Assets panel** hid and showed nothing: the frame wrote back the layout it
  had read before the menu ran.
- A `**/` pattern matches files at the top of the folder too: `--exclude '**/*.psd'`,
  `pack = { exclude = ["**/*.psd"] }`, `cook.json` and `textures.json` patterns (and the editor's
  import settings, which read `cook.json`) left out or matched only files in subfolders before.
- The hierarchy's right-click menu, the gizmo's Move, Rotate and Scale tooltips, and the overlay
  inspector's Duplicate, Despawn and Redo tooltips and help lines show the keys as bound now,
  not the default keys after a rebinding.
- `ponko image-diff` names an unknown option instead of printing only its usage, takes options
  before or between the two images, and has a full `--help`; `ponko help help` and, in a build
  without the `textures` feature, `ponko help textures` answer instead of "no command".
- `ponko --help` lists `ponko new --plugin`, `ponko editor --last` and `--recent`, and
  `ponko pack --list`; `ponko notices --help` describes each of its options.
- The editor's **3D** view button says why it is greyed out in a game without the `3d` feature.

### Changed
- `ponko notices` refuses the options only `ponko package` uses, and `ponko package` refuses
  `--title`: both share one parser, so `ponko notices --web` or `ponko package --title Game` were
  accepted and did nothing. Drop such an option from a script; `ponko notices` takes `--out`, `-p`,
  `--title`, `--manifest-path`, `--target`, `--features`, `--no-default-features` and `--locked`.
