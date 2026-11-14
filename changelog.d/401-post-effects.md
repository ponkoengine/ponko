### Added

- Split toning (`Post::split_toning`, `SplitToning`): tints for the
  shadows and the highlights, blended with soft light, and a balance. Off
  by default (grey tints); no pass of its own.
- Depth of field (`Post::depth_of_field`, `DepthOfField`): an optional
  full-screen pass before bloom that blurs what is out of focus by the
  scene depth, as a lens does. Off by default; about 0.8 ms at 1920x1080 on
  an Apple M4 Max, and no extra image.
- `Post::lerp` blends two looks: amounts and colors mix, an effect off at
  one end fades out, choices switch halfway.
- `PostVolume`: boxes of post settings with a blend distance, mixed over a
  base look by the camera's position (`PostVolume::blend`). The
  `post_volumes` example walks between two of them.
