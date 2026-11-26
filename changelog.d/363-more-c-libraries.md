### Added
- C library downloads for three more platforms: Intel Macs
  (`macos-x86_64`), 64-bit Arm Linux (`linux-arm64`, glibc 2.28 or newer)
  and Windows on Arm (`windows-arm64`), next to `linux-x86_64`,
  `macos-arm64` and `windows-x86_64`. Same layout as before; the C/C++
  chapter lists them. `scripts/c-library.py` builds and smoke-tests each
  archive (CONTRIBUTING.md, "The C library archives").

### Fixed
- The macOS C library in the release archives has the install name
  `@rpath/libponko_ffi.dylib`, so a program linked against it finds it
  through its rpath instead of looking for the path it was built at.
