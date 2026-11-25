### Added
- Tests that global illumination's probes see point and spot light
  shadows, instance batches and glowing surfaces, with before/after
  reference images: an interior lit by a shadowed spot light
  (`gi_spot_interior`) and a room built of instance batches
  (`gi_instanced_room`), each rejecting the probes as they were before
  they saw that. The `gi_cost` benchmark also measures the probes
  without light shadows.
