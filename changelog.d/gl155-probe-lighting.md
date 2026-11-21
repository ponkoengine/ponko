### Fixed
- Realtime global illumination settles much sooner in bright closed rooms.
  Each bounce of the probes' light takes a round of probe updates, and
  white walls keep most of the light, so it took over a thousand frames to
  settle. Surfaces now take the light each probe heads for (the bounces
  still to come, estimated from how much its light still grows), while
  the probes keep bouncing the light as it is. In a closed white room lit
  by a lamp the roof is within 6% of its settled light from 400 frames on,
  where it was 17% short.
- Bounced light no longer leaks through a thin wall that runs through a
  plane of probes. A probe buried in such a wall saw the light on both
  sides of it; its neighbours' distances now give it away and surfaces
  leave it out. The floor beside a 10 cm wall on a plane of probes 2 units
  apart dropped from 192 to 24 (out of 765; the lit side's floor: 423).
  The GPU cost is the same within the measurement noise.
