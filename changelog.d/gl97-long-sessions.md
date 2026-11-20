### Added
- 3D shaders can read whole and fractional elapsed seconds separately or use
  `ponko_elapsed_time_wrapped` for smooth whole-second cycles after days of
  play. Existing `frame.time` values retain their meaning and precision.
- C and C++ expose double-precision normalized time for 3D animators and
  2D skeletons. Existing float accessors retain their signatures.

### Changed
- 3D materials, material shaders, compute passes, screen effects and texture
  arrays reuse freed slots under new handle IDs. Existing references to freed
  resources keep their fallback behavior after reuse. Screen effects drop
  their pipelines when freed.
