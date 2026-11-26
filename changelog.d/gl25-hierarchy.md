### Breaking changes
- `systems::follow_attachments` places only the subtrees whose root moved or whose attachments changed, so a transform written straight into an attached entity lasts.
  A still subtree costs nothing per tick. A value written into an attached
  entity's `Transform` used to be reset every tick and now stays until its
  subtree is placed again. Chains have no depth limit any more (they were
  broken past 64 links), and an attachment whose parent has no transform is
  left where it is instead of being despawned.
  Instead: to move an attached entity, change its `Attached`/`Attached3D`
  offset (or move its parent), as before.
  Before:
  ```rust
  if let Some(t) = ctx.world.get_mut::<Transform2D>(sword) { t.position.y += 4.0; }
  ```
  After:
  ```rust
  use ponko::attach::Attached;
  # let sword: Entity = todo!();
  if let Some(a) = ctx.world.get_mut::<Attached>(sword) { a.offset.y += 4.0; }
  ```

### Added
- An entity hierarchy (`ponko::hierarchy`, guide chapter "The entity
  hierarchy"): attachments are its parent links, and each parent keeps its
  children in order in a reflected `Children` component that the engine
  maintains. `WorldHierarchy` (in the prelude) adds `set_parent` (keeps the
  child where it is, refuses loops), `remove_parent`, `set_child_index`,
  `parent`, `children`, `descendants` (depth first, in child order),
  `ancestors`, `despawn_recursive` and `update_hierarchy` to `World`. Saves,
  scenes and prefabs keep the order; the editor's hierarchy panel shows it.
- C: `ponko_set_parent`, `ponko_remove_parent`, `ponko_parent`,
  `ponko_children`, `ponko_descendants`, `ponko_ancestors`,
  `ponko_set_child_index`, `ponko_despawn_recursive`,
  `ponko_update_hierarchy`; C++: the same as `ponko::world` methods.
- Benchmarks `hierarchy/100k-{deep,wide}-{moving,still}`,
  `hierarchy/100k-wide-churn`, `hierarchy/100k-pairs-{first-update,moving}`
  and `hierarchy/100k-wide-moving-3d` in `ponko_bench`.

### Changed
- `follow_attachments` turns on change tracking for `Attached`,
  `Attached3D` and `Children` the first time it runs.
- The editor places 2D attachments right after an edit too, not only 3D
  ones. Its 3D transform helper also accepts chains deeper than 64 links.
