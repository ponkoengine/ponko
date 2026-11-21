### Changed
- On the CPU path, instance cells of one mesh are drawn together: the
  visible instances of neighbouring ranges of the cells' shared vertex
  buffer join into one draw, so a field of 1,000 visible cells loaded
  together draws a few times a frame instead of once per cell and pass.
  Cells are still culled one by one.
- On the GPU-driven path, instance cells keep their records in pools of
  their own, each cell's block starting on a record (96 bytes) instead of
  on the storage binding alignment and a record (768 bytes where bindings
  align to 256): a small cell's records are no longer padded by up to
  768 bytes.
