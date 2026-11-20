### Changed
- 3D frames make fewer heap allocations and command buffers: the town
  scene of the render benchmark went from 1222 to 745 allocations a frame
  with one camera (1844 to 1255 with two, 2788 to 2113 with four) and from
  13 to 2 command buffers.
  - The small buffer writes made while a frame is prepared are gathered
    into one staging write and copied into place at the start of the
    frame's first command encoder, instead of one `write_buffer` each.
  - Consecutive small point and spot light shadow views are drawn in one
    pass of the shadow atlas.
  - A frame's passes are recorded into a few command encoders of about
    equal work (one for a small frame, at most one per core and eight in
    all), instead of one per pass, so heavy frames still record in parallel.
  - The occlusion grid and its bind group are reused between frames.
- The render benchmark has `PONKO_RENDER_VIEWS=minimap` (a quarter-sized
  view with the costly effects off) and `PONKO_ALLOC_SITES=n`, which prints
  where a frame's allocations are made (each one's call stack, grouped).
