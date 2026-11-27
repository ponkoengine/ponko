<!-- area: C and C++ -->
### Fixed
- The C library builds with `video` on arm64 Linux: the shared library
  (`libponko_ffi.so`) failed to link because the AV1 decoder's assembly
  reads a table with a PC-relative address. `ponko_ffi` now links it with
  `-Wl,-Bsymbolic` there (only with `video`, only on arm64 Linux), keeping
  the assembly. A shared library of your own that takes in
  `libponko_ffi.a` with `video` on arm64 Linux needs the same flag (the
  guide's "Video", C and C++).
