### Fixed
- Material normal maps lean surfaces the way glTF says: the map's red (+X)
  toward where the texture coordinate u grows, its green (+Y) up the image
  (toward smaller v). They leaned the opposite way on both axes, because the
  tangent frame built from screen derivatives took screen y down; bumps
  looked like dents. This applies to every material path that reads
  `MaterialDesc::normal_map` (models, crowds, skinned meshes and the
  screen-space reflections' normals); decal and terrain normal maps were
  already right. A lit offscreen test pins the direction with 1x1 maps on
  squares with plain, turned and mirrored texture coordinates, and the
  Khronos `NormalTangentTest` and `NormalTangentMirrorTest` sample models
  (fetched at a pinned revision into the git-ignored `.cache/known-content`)
  now show every normal-mapped hemisphere lit as the real one beside it.

### Breaking changes
- Material normal maps no longer lean surfaces the opposite way on both axes.
  A game that made its normal maps look right by flipping them (a negative
  `normal_strength`, or images with red and green inverted) now sees them
  inverted.
  Instead: undo the workaround: a positive `normal_strength`, and the
  images as exported (glTF and OpenGL style, green up).
  Before:
  ```rust
  let bumps = MaterialDesc { normal_map: Some(normals), normal_strength: -1.0, ..Default::default() };
  ```
  After:
  ```rust
  let bumps = MaterialDesc { normal_map: Some(normals), normal_strength: 1.0, ..Default::default() };
  ```
