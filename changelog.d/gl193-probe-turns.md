### Changed
- Realtime global illumination settles sooner after a light changes: the
  probes whose light is still changing get more of the per-frame updates
  (the renderer reads back how much each probe's light changed, a frame or
  two late). With a lamp switched on in a closed room inside the default
  probe grid, at the default 4 probes a frame, the room is within 5% of
  its settled light after 400 frames instead of 1200.
