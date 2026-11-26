### Fixed
- Screen-space reflections showed nothing on Direct3D 12 on an AMD Radeon RX
  Vega once the frame was larger than 128 pixels on a side, when the opaque
  pass reads the reflections' spread image directly (a game's texture limits)
  and ambient occlusion is off. The first level of the reflections' image,
  which only says that reflections are there, was set once with a render-pass
  clear; the opaque pass read it back as zero. It is drawn once instead
  (#588).
