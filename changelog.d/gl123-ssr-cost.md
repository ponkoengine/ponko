### Changed
- Screen-space reflections trace from a single-sampled copy of the depth,
  made by a new full-screen pass just before the trace (`reflection depth`
  in the GPU profiler, 4 bytes a pixel). Reading the multisampled depth at
  every step was most of the trace's cost: with the copy the trace takes 48
  steps instead of 32 and still costs less than before, about 0.38 to
  0.46 ms for copy and trace together at 1920x1080 on an Apple M4 Max, down
  from 0.52 to 0.63 ms (back near its cost before the contact reflections
  were made solid). Contact reflections without TAA are also less noisy.

### Fixed
- Thin things far away (rails, cables, poles) are no longer reflected as
  wide blobs without TAA: a crossing allows at most two pixels of the ray's
  travel instead of two of its steps, which far along the ray were tens of
  pixels. Their reflections are now 1.6 to 2 times as tall as they are seen,
  down from 2.2 to 4.2 times.
