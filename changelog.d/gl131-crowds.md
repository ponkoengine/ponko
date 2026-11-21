### Added
- Crowds are drawn into global illumination's probes: culled in the
  probes' sun shadow map and six face directions (keeping in a face
  direction only the characters at least one probe pixel wide from a probe
  updated that frame), so they bounce and block light as models do.
- Window views (`SceneSettings::views`) cull and draw crowds for their own
  camera, on both the GPU and the CPU path.
- Crowds take part in occlusion culling: with the camera's
  `occlusion_culling` on, characters the depth grid hides are left out of
  the camera's view (on the GPU path the cull pass reads the grid as a
  uniform).

### Fixed
- A crowd's animation clock stays precise in long sessions: it keeps its
  epoch in 64-bit seconds and moves the characters' clip times on every
  16 seconds of game time, so a day into the game the clips step and write
  motion vectors as finely as at its start (they were 32-bit seconds since
  the game began, about 1/128 s steps after 24 hours).
