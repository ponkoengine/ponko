### Fixed
- C++: with `PONKO_COUNT_CPP_ALLOCATIONS`, `operator new` returned blocks
  aligned to 8 bytes on Apple silicon, where `new` promises 16
  (`max_align_t` is 8 bytes there): the size kept in front of each block
  now takes the default alignment of `new`. Found by the sanitizer build
  of the C++ tests.
