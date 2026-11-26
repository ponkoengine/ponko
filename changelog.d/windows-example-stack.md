### Fixed
- Windows (MSVC): the examples are linked with an 8 MB main-thread stack, the
  size Linux and macOS give. With Windows' default of 1 MB, about 40 of them
  (`window`, `sprites` and every 3D example among them) stopped at startup in
  a debug build with "thread 'main' has overflowed its stack".
