### Changed
- A UI group fading as one image (`Ui::set_fade_as_group`) draws into a
  layer the size of what it draws (children reaching out of its rectangle
  included), snapped outwards to whole pixels and cut to the window, instead
  of a window-sized one: a 400 x 300 group on a 4K screen takes 0.6 MB
  instead of 33 MB. It draws the same pixels (checked against the
  window-sized layer on the GPU). Layer textures are kept from frame to
  frame and made in steps of 64 pixels, so a group that moves or changes
  size a little while it fades makes no texture after its first frame.
  GPU_NOTE

### Added
- Groups in world-space panels (`UiPanel3D`) fade as one image too, into
  layers in the panel's pixels drawn before the panel.
