### Fixed
- Windows (MSVC): the examples are linked with an 8 MB main-thread stack, the
  size Linux and macOS give. With Windows' default of 1 MB, 35 of them
  (`window`, `sprites` and most 3D examples among them) stopped at startup in
  a debug build with "thread 'main' has overflowed its stack" (#591; a
  game's own startup path is not fixed yet).
