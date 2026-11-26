### Fixed
- WebGL2: a 3D scene without shadows (`lighting.shadows = false`) drew
  nothing. wgpu's GL backend makes a one-layer texture an ordinary 2D
  texture, which the shaders' shadow-map array sampler can't read, so the
  browser refused every draw ("Mismatch between texture format and sampler
  type"). The sun's shadow map and its 1x1 stand-ins now get a second,
  unused layer on GL, as texture arrays already did. The `lights` example
  draws on WebGL2 again.

### Added
- The 3D reference game plays its walk-through in the browser:
  `?smoke=walkthrough` in the address of the `shard_grove` web build, and
  the web smoke runner's new `--until <field>:<value>` (fixture fields
  `until`, `untilSeconds`) waits for a game playing itself to get there.
  Its web state also reports clicks, the focused menu button, the tally
  and the engine's adapter.
- A `web-examples` job in `scripts/checks.yml` (slow, a Mac with its GPU):
  every gallery example built for the web and played in headless Chromium
  on WebGL2 and WebGPU and in WebKit, and Shard Grove's keys and
  walk-through on WebGPU and WebGL2.
- Twelve more examples in the web gallery: `skeleton2d`, `ai_guards`,
  `prefabs`, `move_sprite`, `ui_widgets`, `sky_kit`, `stylized_sky`,
  `reflection_refresh`, `post_volumes`, `split_screen`, `crowd` and
  `water`. The gallery's README names every example left out and why, and
  a test keeps it so.

### Changed
- The web gallery check fails a page whose browser logged a WebGL error (a
  refused call), not only a blank canvas.
