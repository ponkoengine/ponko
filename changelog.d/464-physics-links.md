### Fixed
- The 2D and 3D physics worlds no longer keep a record for every entity
  index ever used. Their per-entity links are packed and give memory back
  when bodies leave, so a streaming world that spawns and despawns many
  entities holds memory for its live bodies only. Simulation results are
  unchanged.
