### Added
- Rectangle and tube lights were measured for the clustered path and are
  not shipped: the guide's "Area lights: measured, not shipped" has the GPU
  cost per light at 1920x1080 (a tube 2.1x a point light, a rectangle 3.5x
  with a representative point and 5.6x with LTC), their error against a
  sampled reference, the WebGL2 check, why their shadows aren't affordable,
  and how to get their look with emissive surfaces, baked lighting, global
  illumination and lamps. The comparison is an offscreen test
  (`src/render/area_lights_render_test.rs`).
