<!-- area: Rendering -->
### Fixed
- Direct3D 12: a 3D frame drawn at less than the whole window (a split
  screen, or the main camera beside window views) could show blocks of old
  GPU memory, often spread over the whole picture by bloom. Images that the
  frame only writes by resolving multisampled passes into (the scene image,
  motion vectors, reflection normals, target cameras, global illumination's
  probes) are now cleared once when they are made, as Direct3D 12 requires
  for render targets placed in shared memory.
