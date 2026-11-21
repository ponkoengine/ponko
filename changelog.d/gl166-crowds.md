### Added
- `CrowdLevel::fade` and `with_fade`: over that distance before its
  `max_distance` a crowd level crossfades into the level that starts there
  (or, with none, fades out to nothing) through complementary dither
  patterns, instead of popping. On the GPU path a crowd with a fade adds
  one indirect draw per view, level and mesh part for its crossfading
  characters; the CPU path draws them in runs of their own. Not in C or
  C++ yet.

### Changed
- Crowds are culled by each clip's own bounds: the bake keeps a box per
  clip (every level, every frame of the clip), and each character is
  tested with the box of the clip it plays (and of the one it blends to)
  under its transform, as an oriented box against the views, a world box
  against occlusion culling's depth grid and its half diagonal for global
  illumination's probe size cut, on both paths. Before, every character
  used one sphere around every clip. In the tests' scenes occlusion
  culling now keeps 4 of 26 characters behind a low wall (26 before) and
  the probes' face views 1,047 characters over 8 frames (3,128 before).
