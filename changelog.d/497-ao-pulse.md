<!-- area: Rendering -->
### Fixed
- Temporal ambient occlusion (`Post::ao_temporal`) no longer pulses every
  eight frames on ground far away. All pixels used to turn their slices in
  lockstep, and the estimate was divided by the slice count. Ground seen at a
  grazing angle then read darker on some turns than on others. Now each
  pixel's occlusion is measured against what its slices would see
  unoccluded, and each pixel's turn is offset by a 4x4 pattern so
  neighbouring pixels sample different turns. The bilateral filter weights a
  neighbour by its distance off the pixel's surface and, along the surface,
  by how far apart the pixels lie, so it averages those turns on distant
  ground too. With only the AO history running, a still field 100 to 400 m
  away changed by a mean of up to 0.77 levels from frame to frame. It now
  changes by under 0.05 (under 0.15 at half resolution).

### Changed
- Ambient occlusion shades open ground seen at a grazing angle a little
  lighter, about 2 levels in the reference scene. Before, it counted part of
  open ground as occluded. Creases stay as dark as before. At half
  resolution (the High and Ultra presets) the AO passes cost the same as
  before, or slightly less. At full resolution the bilateral pass costs
  about 10% more (+0.04 ms at 1280x720 on an Apple M4 Max).
