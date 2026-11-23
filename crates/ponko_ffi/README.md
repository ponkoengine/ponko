# ponko_ffi

The C ABI of [Ponko](https://github.com/ponkoengine/ponko): the library that
C and C++ games link. Its Rust docs, on <https://docs.rs/ponko_ffi/>,
describe the safety contract of every function.

The headers are not in this crate. Get them, with prebuilt static and shared
libraries for Linux, macOS and Windows, from the
[release](https://github.com/ponkoengine/ponko/releases) of the same
version, or from the repository:

- `include/ponko.h`: the C API, generated from this crate with cbindgen;
- `include/ponko.hpp`: a header-only C++17 wrapper (`ponko::game`, `ponko::ctx`, ...).

To build the library yourself, `cargo build --release -p ponko_ffi` in the
repository. The C and C++ chapter of the guide covers building, linking and
the examples: <https://ponko.dev/guide/c-cpp.html>. The ABI has its own
version (`ponko_abi_version()`); see
[STABILITY.md](../../STABILITY.md#c-abi).

The crate's tests compare the header in the repository with cbindgen's
output, so they run from a checkout, not from the packaged crate.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
