<!-- area: Rendering -->
### Added
- `Color::lerp(other, t)` mixes two colors in all four channels (alpha too), on
  the sRGB values the type holds; `t` is clamped to 0 to 1. `Color::hex_rgba`
  takes `0xRRGGBBAA`, and `"#ff8800".parse::<Color>()` reads `#rgb`, `#rrggbb`
  and `#rrggbbaa` (the `#` optional, either case); a bad string is an
  `ErrorKind::Invalid` error that names the problem. Rust only for now: a C or
  C++ `PonkoColor` is four floats.
- Two cookbook recipes, "Invincibility frames after a hit" and "Forgiving
  jumps: coyote time and a jump buffer", each run by a test.
- The guide's GPU regression tests chapter says how to run them on a card with
  little memory (`-- --test-threads=4`).
