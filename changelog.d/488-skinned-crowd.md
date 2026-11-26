<!-- area: Rendering -->
### Changed
- GPU skinning poses all skinned models of one mesh with one compute dispatch
  (up to 512 models each) into shared vertex buffers, instead of one dispatch,
  bind group and vertex buffer per model. Passes draw a posed model from the
  shared buffer at its first vertex, so consecutive models of one mesh keep
  their buffers bound. On an Apple M4 Max, 1,000 skinned figures of 16 joints
  (the `animated` render scene at scale 2.5) went from 3.8 to 0.06 ms of
  skinning and from 6.0 to 2.9 ms of GPU time a frame, with 2,847 vertex and
  index buffer changes a frame down to 22. Images are unchanged. The CPU
  fallback (no compute shaders) still poses each model into its own buffer.
