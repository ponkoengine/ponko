### Fixed
- Realtime global illumination leaks less light through thin walls near
  their top. Probes inside a wall just below its top weren't found to be in
  the wall: part of what their neighbours saw that way passed over the top
  into the next room. They now are, so they no longer light a dark room
  with the light from the room next door. With a 10 cm wall between a lit
  and a dark room on a plane of probes 2 units apart, the dark room's floor
  went from 5% to 1.4% of the lit floor's brightness (sums of the image's
  8-bit channels), and the lit room got no darker.
