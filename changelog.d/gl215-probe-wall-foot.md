### Fixed

- Global illumination no longer takes probes in open air just under a
  wall's bottom edge for buried in the wall: the floor beside a gap under
  a raised partition got about half the bounced light it should. Each
  neighbour now reads the four distance texels around the way to the
  probe, a cone centred on it. Every probe inside a raised partition is
  still found, down to 0.1 units above its bottom edge.
- The guide quoted realtime global illumination's error in the red-walled
  room as 0.08; it is 0.05. The tests now check the renderer still gets
  the errors the guide quotes, and that the guide quotes them.
