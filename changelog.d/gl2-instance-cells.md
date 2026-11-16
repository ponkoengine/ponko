### Added
- Instance cells: an `InstanceBatch` can take its instances one spatial cell
  at a time. `Graphics::add_instance_cell(batch, transforms, colors)` uploads
  a cell's instances and returns an `InstanceCell`;
  `replace_instance_cell` gives it new ones and `Ctx::free` removes it,
  without uploading the batch's other instances again. On the GPU-driven
  path a freed cell's records go to the next cells. Cells are culled and
  drawn in every view the batch is (camera, sun cascades, target cameras,
  point and spot light shadows) on either path, and cells of an automatic
  batch take the path the whole batch would. Freeing a batch frees its
  cells. `Graphics::instance_batch_len` counts its cells' instances too.
- `Graphics::stream_instance_cell(batch, job)` takes a `Job` that builds an
  `InstanceCellData` on a worker thread and uploads the cell in the frame's
  upload step, within the upload budget; `instance_cell_state` and
  `instance_cell_len` say where it is.
- Meshes that live instance cells draw are kept under a mesh residency
  budget; a cell added while its mesh is evicted draws once it is back.
- `LiveCounts::instance_cells`.
- C and C++: `ponko_ctx_add_instance_cell`, `ponko_ctx_replace_instance_cell`,
  `ponko_ctx_instance_cell_len`, `ponko_ctx_free_instance_cell` and
  `PonkoInstanceCell`; `ctx::add_instance_cell` and friends in `ponko.hpp`.
- A benchmark of streaming cells against making the batch again
  (`crates/ponko_bench/INSTANCE_CELLS.md`): a cell uploads in about 0.08 ms
  against 2 to 4.6 ms per cell that came or went when the batch is made
  again, on one machine.
