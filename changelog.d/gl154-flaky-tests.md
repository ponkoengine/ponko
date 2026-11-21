### Fixed
- Tests that failed on and off on a loaded machine now count work instead of
  time: the 2D skeleton allocation test fixes and starts its thread pool
  before measuring and allows exactly the pool's own allocations, the C API
  asset group restart test no longer expects a fixed number of loaded assets
  before the restart, and background-loading, streaming, job and server tests
  wait for a state, with long hang-detector limits, instead of a number of
  frames or a short deadline.
- The control endpoint's exit tests check that no reply is still owed when
  the endpoint stops, instead of timing the stop.

### Changed
- The guide's contributing chapter has a section on writing tests that hold on
  a busy machine.
