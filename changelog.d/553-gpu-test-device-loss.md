### Changed
- GPU tests: at most `PONKO_GPU_TESTS_AT_ONCE` tests (default one) hold a
  device at once in a test process, so the suites survive the default
  thread count on drivers that lose the device with several at once. Tests
  without a GPU still run in parallel.
- GPU tests: a test that finds no adapter or device fails once another test
  of the same run has opened one (the device was lost), instead of skipping
  and passing. Every skip is also appended to
  `target/render-test-output/skips.log`, so a run can count them.

### Fixed
- Point and area lights no longer give NaN where the light is straight
  behind the viewer on GPUs that compute `pow` as `exp2(y * log2(x))`
  (AMD's Vulkan driver): the GGX Fresnel term's `v·h` could round past 1.
  Through global illumination's probes the NaN turned whole frames black.
- Crash reports on Windows name a function from its own first byte, not the
  symbol before it: `backtrace`'s dbghelp path looks one byte back twice for
  an address.
