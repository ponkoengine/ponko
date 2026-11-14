### Fixed
- The GPU memory report counts the 1x1 colour stand-in that 3D passes without
  a scene bind, and the SMAA colour target has a label, so allocator reports
  name every engine texture.
