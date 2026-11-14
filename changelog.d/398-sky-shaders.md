### Added
- Sky shaders: a game's own sky in WGSL (`fn sky(dir: vec3<f32>) -> vec3<f32>`),
  made with `Graphics::create_sky_shader` or `load_sky_shader` (hot reloaded) and
  shown with `Sky::shader`. The engine uses it for the background, the fog's
  color, reflections, the ambient light, transparent surfaces and reflection
  probe captures. `Sky::shader_params` passes four values (`frame.sky_params`);
  what fog, reflections and ambient light read is rebuilt at most every
  `SkyShader::REBUILD_FRAMES` frames when they change. `Sky::authored_ambient`
  keeps the authored ambient colors, and the sun keeps its authored color.
  Probes with `ProbeRefresh::LightingChanged` refresh when the sky shader or its
  values change. The `stylized_sky` example paints a sky through a day.
  C and C++ bindings don't have sky shaders yet.
