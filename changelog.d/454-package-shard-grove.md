### Changed
- The list of checks packages the 3D reference game (`ponko package --example
  shard_grove`) in `package-smoke` and plays the package headless from an
  unrelated folder, as it does for the other two reference games. A sheet in
  `scripts/checklists/` covers what needs a person: the frame-time baseline
  per GPU tier, and opening the game in a WebGPU browser and in a window on
  each OS.
