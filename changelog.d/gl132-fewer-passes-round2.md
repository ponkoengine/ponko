### Changed
- A window's 3D views go to the GPU in one submission with the main camera
  instead of one each: every frame's small buffer and texture writes are
  copied into place by its own first command buffer, so each view still
  reads its own camera, lights and culling data (issue #132). Growing an
  instance pool or the shared mesh arena joins that submission too. The
  render benchmark prints command buffers and submissions a frame and writes
  `submits` to its JSON.
- Light textures (light and decal records, per-cluster light and decal lists) are written
  through the frame's batched upload instead of one `write_texture` each.
- What remains of a frame's allocations is inside wgpu (about 24 for each
  pass, a few for each copy, about 25 for each submission); the guide's
  numbers for views were measured again (town scene: 767 for one camera, about
  440 to 495 for each full view, 159 for a minimap).

### Added
- Probe views for global illumination leave out whole instance cells too far
  from every probe for their widest instance to be a pixel wide, on the CPU
  and the GPU culling paths, with the same result as testing each instance:
  in a scene of 250,000 rocks over 1 km x 1 km the CPU path's preparing went
  from about 9 ms to about 2.8 ms a frame. `Cell` (instance cells) gains a
  `reach` field.
