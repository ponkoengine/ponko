### Changed
- **Breaking:** three types keep one public path each. None of the removed
  paths was in a release (they were added and dropped on the development
  branch before 0.3.0), so only code written against that branch changes:
  `ponko::texture::DrawSpace` is `ponko::debug::DrawSpace` (also in the
  prelude); `ponko::input::TouchShow` is `ponko::ui::touch::TouchShow`;
  `ponko::ui::Bezier` is `ponko::skeleton2d::Bezier`, the one easing curve of
  skeleton clips and UI transitions. Change the `use` line; the types, their
  values and the C and C++ names are unchanged.

### Fixed
- The offscreen timing helper of the GPU tests times each frame once: it
  begins the timer's frame itself and submits the timestamps with the frame
  in one submission, as a game does. The debug-line timing test no longer
  begins every frame twice.
- The GPU tests that were seen failing on a heavily loaded machine measure
  what they claim without a timing assumption: the GPU memory total is
  compared with the adapter's figure once the driver has given back what
  was dropped, and automatic quality's fresh sample every frame allows a
  lone frame whose timestamps the GPU left unusable. The C++ test of
  allocation-free query filters asks for one clean profile run of three
  (a system's count is the whole process's, so another thread's
  allocation could land in it), and its net-bytes check likewise retries.

### Added
- Tests: the frame uniform's fields, in order and at their offsets, are the
  same in the Rust struct, the engine's mesh shader and the cook's copy of
  its prelude; a crowd drawn on a device without a texture slot for the
  reflections' spread image (the baseline tier) reflects as a model does.
