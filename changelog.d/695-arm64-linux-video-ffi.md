<!-- area: C and C++ -->
### Fixed
- The C library builds with `video` on arm64 Linux and Android: the shared
  library (`libponko_ffi.so`) failed to link because the AV1 decoder's
  assembly reads its tables with PC-relative addresses. `ponko_ffi` now
  links it with `-Wl,-Bsymbolic` there (only with `video`), keeping the
  assembly. A shared library of your own that takes in `libponko_ffi.a`
  with `video` on those systems needs the same flag (the guide's "Video",
  C and C++).
