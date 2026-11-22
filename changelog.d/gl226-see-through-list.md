### Changed
- Ribbons and lines (`Trail`, `Line`) and the instances of see-through
  instance batches sorted on the CPU are drawn far to near together with
  the see-through models and the particles, by view depth, instead of
  after all of them: a ribbon or a particle between two panes of glass is
  tinted by the far one and dimmed by the near one, whether the panes are
  models or instances of one batch. Crowds and GPU-driven see-through
  batches follow the last see-through model, and 3D text comes last, as
  before.
- See-through models of one mesh are drawn together, in one instanced
  draw, where the models drawn between them in far-to-near order cover
  none of the same pixels (by screen rectangles from the meshes' bounding
  spheres): the same picture, to the byte, with fewer pipeline, material
  and draw changes. Skinned models, models drawn from a GPU buffer, models
  whose shader has a `vertex` function and meshes with several
  see-through parts are never moved. Particles, ribbons, lines and text
  set a pipeline or image only when it changes.

### Added
- `perf.get`'s `render_inspector.counts.see_through_changes` (and
  `see_through_changes` in `PONKO_BENCH_OUT` reports and the rendering
  baseline's scene reports): the pipelines, bind groups, vertex and index
  buffers the see-through list set in the last frame, and its draws.
  `counts.state_changes` now includes the particles', ribbons', lines' and
  3D text's.
