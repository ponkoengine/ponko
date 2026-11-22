### Added
- Guide: "VR and XR: where the engine stands" (`book/src/xr.md`), the
  headset evaluation: what stereo drawing shares between the eyes today
  (shadows, skinning, global illumination) and what runs per eye (culling,
  every camera pass), the frame timing and input a headset runtime asks
  for, the platforms that could be tested, and the decision (later),
  recorded in `ROADMAP.md`.
- An offscreen stereo prototype (`src/render/stereo_render_test.rs`): the
  3D reference game's level drawn for two eyes through two views, with
  tests for the eyes' disparity and the shared shadow cascades, and an
  ignored benchmark of a stereo frame against one view.
