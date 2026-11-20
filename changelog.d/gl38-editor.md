### Added
- The editor: `ponko editor [<project>] [--open <file>]` builds a game with
  the new `editor` cargo feature and opens it in edit mode (`PONKO_EDITOR`).
  The game's window becomes a level editor: a menu bar (new, open, save,
  save as scene or prefab, reload), a hierarchy (drag to reparent the whole
  selection, multi-select, rename with F2 or a double click, a right-click
  menu), the component inspector with add and remove, and the viewport with
  move, rotate and scale gizmos and snapping in 2D as well as 3D. Every edit
  is undoable, drags and dragged numbers are one step each, the window title
  and status bar mark unsaved changes, closing with unsaved changes asks
  first, and a file changed on disk is opened again (or, with unsaved
  changes, offered). See the guide's "The editor" chapter.
- `hierarchy::EntityName`: a reflected name for an entity, shown and edited
  by the editor's hierarchy.
- The control endpoint's `editor.document` (open, reload or start a
  document and report it; MCP tool `editor_document`), `as` (`scene` or
  `prefab`) on `editor.save`, and `dirty` in the editor methods' replies.
- The inspector's 2D editing has a gizmo too: move along an axis, rotate,
  scale (W, E, R), world or local axes, and snapping.

### Changed
- `editor.transform` and `editor.drag` work on 2D games (they needed the
  `3d` feature before).
- `capabilities` lists the `editor` feature.
