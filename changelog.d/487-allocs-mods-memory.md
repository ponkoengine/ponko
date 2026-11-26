### Changed
- A code mod's game is checked once per `ModCode::call`, not once per host
  function: code swapped out of its `with_code` call, or into one it wasn't
  handed, still can't reach the game.
- `update_hierarchy` places a flat update (no link changed, every parent a
  root, a few children per moved root) in passes over the attached entities
  on all cores, without building, indexing and gathering a level of roots on
  the calling thread.
- On macOS, `KeyLabels::system()` reads the layout only with the `window`
  feature (`None` without it): linking Carbon mapped 4 MiB at start in every
  headless process.
- Only a hosting network endpoint holds the budget for answering unknown
  addresses (only a host answers them): a client endpoint is 392 bytes
  instead of 2,976. The
  lobby load test peaks at 108–111 MiB instead of 122–124 MiB.

### Added
- `ponko_bench` rows `physics3d/heightfield-257-copy-x20/*`: what a mirrored
  or stretched heightfield copy costs when its tile enters physics (about
  0.07 ms for 257 × 257 heights).
- `PONKO_RENDER_SSR_THIN` in the render scenes bench, and GPU tests holding a
  steady 3D frame's allocations to a budget and to the same count for ten
  times the models.
