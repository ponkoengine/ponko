# Editor and inspector: the look of the real windows

Issue [#613](https://github.com/ponkoengine/ponko/issues/613), reported on [#484](https://github.com/ponkoengine/ponko/issues/484). Headless tests open every menu of the editor, list its entries and use each one, press every shortcut, use every button of the menu bar, the gizmo row, a hierarchy row's menu and the overlay inspector, and run every `ponko` command and option (see "Tests" in [book/src/editor.md](../../book/src/editor.md) and [book/src/cli.md](../../book/src/cli.md)). They check what each entry does, not how it looks. What only a person can judge, in a real window:

Read [the conventions](README.md#conventions) first. Do it once per OS you have (macOS, Windows, Linux), at the display scale you use, and once more at 200% (HiDPI) or on a Retina screen if you have one.

## K1. The editor's menus

- **Run:** `ponko editor --example editor` (from the repository), then `ponko editor --example editor --features 3d` for the 3D one.
- **Look for:** **File**, **Edit**, **Author**, **Window** and **Help** each open next to their button, inside the window, with no entry cut off or overlapping. A shortcut is written beside each entry that has one (⌘ on macOS, Ctrl elsewhere), the same key the **Help → Shortcuts** list shows. Entries that can't act now (Undo with no history, Reload from disk for an untitled scene, Save selection as prefab with nothing selected) are greyed out, and nothing else is. **Open recent** shows a submenu once a file was opened. **Window → Assets panel** hides the asset panel and shows it again, and the view re-centres each time.
- **Record:** date ______ · hardware ______ · OS and scale ______ · result ______ · issue # if it failed ______

## K2. Shortcut text after rebinding

- **Run:** In **Help → Shortcuts**, rebind Duplicate to Ctrl+J (⌘J) and Move gizmo to M. Then open **Edit**, right-click a row of the hierarchy, and hover the gizmo's **Move** button.
- **Look for:** The Edit menu and the row's menu say Ctrl+J (⌘J) for Duplicate; the **Move** tooltip says M; nothing still shows the old keys. **Reset all** puts the defaults back everywhere.
- **Record:** date ______ · hardware ______ · OS ______ · result ______ · issue # if it failed ______

## K3. The bar, the panels and the dialogs

- **Run:** The editor from K1: use **Play**, **Pause**, **Step**, **Stop**; switch **2D** and **3D**; open each dialog (New scene with unsaved changes, Open, Save as, Save selection as prefab, the close question) and each **Author** window.
- **Look for:** No label is cut off or drawn over another at the window's smallest useful size (about 1024 × 640) and when maximised. In a 2D-only game the **3D** button is greyed out and its tooltip says the game lacks the `3d` feature. Each dialog is in front, readable, and closes with Esc. No panel or window is blank: an empty list says so in words.
- **Record:** date ______ · hardware ______ · OS and scale ______ · result ______ · issue # if it failed ______

## K4. The overlay inspector

- **Run:** `cargo run --example level` (a debug build has the inspector on), press the backquote key (`` ` ``) to show it, pause, and open each section (Edit, Hierarchy, Assets, Entities, Resources if the game has any, Systems, GPU passes, Debug drawing). Then select every entity in the hierarchy and press Delete until none is left, and open each section again.
- **Look for:** Every section opens onto something readable (an empty one says so: "no systems", "0 matching"); tooltips of Duplicate, Despawn and Redo name the keys; the help lines under the gizmo row name W, E, R (or the rebound keys) and Delete, Ctrl+D (⌘D), Ctrl+Z (⌘Z). Nothing overlaps the game's own drawing in a way that hides a control.
- **Record:** date ______ · hardware ______ · OS and scale ______ · result ______ · issue # if it failed ______

## K5. `ponko --help` in a terminal

- **Run:** `cargo install --path crates/ponko_cli`, then `ponko --help` and `ponko help <command>` for each command, in an 80-column terminal (Windows: PowerShell and cmd).
- **Look for:** No line wraps mid-word in 80 columns, the option lists line up, and quotes and `…` show as such (not as `?` or boxes).
- **Record:** date ______ · hardware ______ · OS and terminal ______ · result ______ · issue # if it failed ______

## Where to report

Paste the filled lines as ONE comment on [#484](https://github.com/ponkoengine/ponko/issues/484), and open an issue for each failure; put its number on the line.
