### Added
- Editor: selecting a glTF scene's root or node shows the file it came from
  and every override of the scene (changed, added and removed components,
  other parents, deleted nodes), each with a **Revert** button, and
  **Revert all**, as the prefab panel lists a prefab instance's.
- glTF scenes keep nodes deleted in the editor deleted, and nodes attached
  to another parent (another node, the scene's root, an entity the save
  keeps, or none) on it, through a re-export and a reopen. A scene's saved
  record takes `"deleted": true` and `"parent"` (`"root"`,
  `{ "node": path }` or `{ "entity": id }`) beside a node's
  `"components"` and `"remove"`; older records read as before.
- Editor: Play then Stop keeps glTF scenes: Stop spawns them again from
  their files with how they were edited before Play, and the undo history,
  the edits kept from play and the selection point at their new nodes.

### Fixed
- Editor: undoing the delete of a glTF scene's node brings it back as a
  node of its scene, with its mesh, colliders and light from the file (it
  came back as a bare entity that scene saves kept as well).
- Editor: the hierarchy names a glTF scene's nodes by their names in the
  file.
