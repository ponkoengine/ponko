### Added
- The release builds and tests the C library for Windows on Arm on a
  GitHub-hosted Windows on Arm runner (`release-matrix/windows-arm64/<version>`
  in the release-time CI), and `c-library.py smoke --docker` runs a Linux
  archive's C and C++ tests in a container whose glibc is the archives' 2.28
  floor, so the `linux-arm64` library is checked by running it.

### Fixed
- `examples/cpp` links the threads library, which programs on glibc before
  2.34 need for `std::thread`, and its asset-copy step no longer crashes CMake
  on arm64 Linux with glibc 2.28.
