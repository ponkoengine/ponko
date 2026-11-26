### Fixed

- glTF materials draw each map with its own texture coordinates and support independent texture transforms animated through `KHR_animation_pointer`. Materials support up to four distinct transforms, with per-map overrides in Rust and version 5 of the C and C++ material descriptor.
