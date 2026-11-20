### Changed
- Screen-space reflections march in steps that grow from 1.5 pixels near
  the surface toward the ray's end, and whether a ray hits no longer
  depends on its per-pixel jitter: without TAA, contact reflections at low
  roughness are solid instead of grainy (an offscreen test bounds the noise
  at 1920x1080). The trace costs about 0.14 ms more at 1920x1080 on an
  Apple M4 Max (0.47 to about 0.6 ms).
- Rays that miss take the mean footprint of the nearest hits from two small
  images of the hits (4x4 and 16x16 of the trace's pixels, made by the
  reflection levels' dispatch) instead of a search in eight directions:
  wide blurred reflections fade out softly where they used to end in
  ragged, specked edges.
- Without ambient occlusion, the opaque pass reads the reflections'
  half-size footprint image directly where the device has a texture slot
  to spare, instead of a full-size pass copying it every frame.

### Fixed
- Screen-space reflections far from the surface no longer vanish at
  render heights of about 1000 pixels and more: the normal rebuilt from
  depth fell below a precision floor and turned toward the camera.
