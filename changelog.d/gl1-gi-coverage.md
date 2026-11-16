### Fixed
- Global illumination: the probes' views are lit with point and spot light
  shadows (the camera's shadow atlas, nothing drawn twice), so a shadowed
  light behind a wall no longer bounces light into the room past it.
- Global illumination: instance batches are in the probes' views and their
  sun shadow map, so they bounce and block light like models. They are
  culled for a bounded number of views however big the grid is: one per
  probe face on the CPU path (instances under a probe pixel left out), and
  seven on the GPU path, whose views each keep a list as long as the batch.
  Instance cells are in them as their batch is; a freed cell leaves every
  probe view from the next update.
- Global illumination: batches of distance levels on the CPU path leave
  instances under a probe pixel out of the probes' views, as single-mesh
  batches do, instead of drawing whole cells into every probe face. They
  keep 4 more bytes per instance for it.
- Batches of distance levels are bounded by a tighter sphere (one holding
  every level's bounds, instead of one around their bounding box), so
  every view culls them a little more tightly.

### Changed
- Global illumination's capture no longer collects particles it never
  draws. Transparent surfaces and particles stay out of the probes' views;
  the guide says why and what drawing them would cost.
