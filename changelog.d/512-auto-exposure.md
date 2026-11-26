### Added
- Auto exposure (`Post::auto_exposure`, an `AutoExposure`): the exposure
  follows the lit frame's brightness, metered as its mean log luminance with
  a center weight and an optional mask texture, held between a min and a
  max in stops, moved by a compensation, and adapting at separate brighten
  and darken speeds. It starts at the target on the first frame and after
  `reset_temporal_history`, adapts per camera in split screen, works with
  every tone map, HDR output, every GPU tier and WebGL2, and costs nothing
  while off. `Post::lerp` blends it.
- C and C++: `PonkoPostEx` version 4 with the `auto_exposure` fields
  (`ponko::post_ex`); older callers leave them as they are.
