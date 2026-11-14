### Added
- Soft particles: `ParticleSettings::softness` (3D, `with_softness`) fades
  particles out over that many world units in front of the scene behind
  them, so smoke meets walls and the ground without a hard edge.
- Target cameras draw particles, trails and lines, sorted and facing each
  camera, so they show in planar reflections and monitors.
  `Camera::particles` (default on) and `TargetCamera::with_particles` turn
  them off per camera; reflection probe captures leave them out.

### Changed
- The particles of all alpha emitters are sorted far to near together, so
  overlapping emitters mix correctly; additive emitters are drawn whole at
  their centre's depth. Neighbouring particles of one image and blend share
  a draw.
- See-through instance batches on the CPU path draw the main camera's
  visible instances far to near (across their distance levels). Measured:
  about 2 ms a frame for 100,000 visible instances while the camera moves,
  nothing while it stands still.
