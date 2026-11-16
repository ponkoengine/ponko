### Changed
- Screen-space reflections blur by a cone: the footprint grows with the
  surface's roughness and with how far each reflected ray went before it
  hit, so a contact reflection on a rough floor stays sharp while a
  distant one blurs, instead of one blur per roughness. Footprints under
  two half-size pixels blur the trace's own level, so thin bright details
  keep their brightness; rays that find nothing take their nearest hits'
  footprint instead of the coarsest level's halo. 1920x1080, Apple M4 Max,
  High preset screen effects: +0.08 ms a frame (+0.14 ms with reflections
  and no ambient occlusion, which now writes the first level every frame).

### Fixed
- A camera with an infinite far plane (`Camera3D::far = f32::INFINITY`)
  no longer counts every frame as a camera cut: temporal antialiasing,
  temporal ambient occlusion and screen-space reflections kept starting
  over, and the reflections never showed.
