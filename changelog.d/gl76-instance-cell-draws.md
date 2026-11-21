### Changed
- Instance cells cost fewer draw calls. On the GPU-driven path, where the
  device has `Features::INDIRECT_FIRST_INSTANCE`, the cells of one batch
  share one bind group and are drawn with one multi-draw-indirect call per
  material and view, however many are visible (each cell was at least one
  draw per material and view); devices without it draw each cell on its
  own, as before. The engine now asks for `INDIRECT_FIRST_INSTANCE` when
  the adapter has it.
- On the CPU path, instance cells take ranges of one shared vertex buffer
  that hands a freed cell's range to the next cells, instead of a vertex
  buffer each; consecutive cells bind it once.
