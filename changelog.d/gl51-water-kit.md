### Added

- `ponko::water`, a water kit for lakes, rivers and seas (`3d` feature):
  `WaterDesc` presets (`lake`, `river`, `sea`), screen-space, planar or
  sky reflections, refraction with depth absorption, shore foam, flow maps
  for rivers, Gerstner waves with a matching CPU height query
  (`Water::height_at`, `Water::sample`), an underwater screen effect,
  contact ripples (`Water::touch`, compute shaders; not on WebGL2) and
  `WaterQuality` presets. `WaterPlugin` keeps it up to date; nothing is
  made until water is spawned.
- `Buoyancy` (`physics3d`): bodies float at the depth their density gives,
  ride the waves and drift with currents, on the fixed step.
- `d3::Physics::rotation` and `d3::Physics::velocity_at`.
- C and C++: `PonkoWaterDesc` (versioned), `ponko_ctx_spawn_water`,
  `ponko_ctx_water_sample`, `ponko_ctx_water_touch`,
  `ponko_ctx_set_buoyancy_box` / `_sphere` and `ponko_game_water`;
  `ponko::water_desc` and `ctx::spawn_water` in `ponko.hpp`.
- The `water` example and the guide's "Water: lakes, rivers and seas" chapter.
