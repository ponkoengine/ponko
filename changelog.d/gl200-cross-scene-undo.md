### Added
- Editor: a node attached to the root or a node of another glTF scene
  whose file didn't load (when the scene was opened) is listed among its
  scene's overrides as attached to that parent, "which is missing", while
  it waits on the file's parent. Its **Revert** forgets the link (one undo
  step; undo keeps it again, and it is made as soon as that scene spawns).
- Saves with a node attached to another glTF scene stay readable by
  builds from before such links: they skip the link's field and leave the
  node on its file parent.
