### Added

- Trigger rumble in web builds where the browser has it: a pad whose
  `vibrationActuator` lists the `trigger-rumble` effect (in `effects`, or
  through the older `canPlayEffectType`) reports
  `capabilities.trigger_rumble` and plays the trigger levels with the body
  motors' as one `trigger-rumble` effect, replayed before the browser's cap
  like body rumble. On any other pad the trigger levels are dropped and the
  body motors still play (`dual-rumble`), as on the desktop; a rumble of the
  triggers alone plays nothing there.
- The `wasm-simd` feature: rustfft's WASM SIMD FFT for the water kit's CPU
  wave spectrum in web builds, to be built with
  `-C target-feature=+simd128` (it refuses to compile without: rustfft's
  SIMD code alone made a moment three times slower). In headless Chromium
  a new moment is then 1.4 to 1.6 times faster (0.16 instead of 0.24 ms at
  128 cells, 0.72 instead of 1.12 ms at 256).
- `web/smoke/spectrum.mjs` and the fixture `examples/web_spectrum.rs`: the
  CPU wave spectrum timed in a headless browser at 64, 128 and 256 cells,
  plain, with `simd128` alone and with `wasm-simd`. The water guide's "On
  the web" has the numbers and the web's default (128 cells, about 0.6 ms
  of a WebGL2 frame at worst).
- `web/smoke/threads.mjs --case gamepads` (and `gamepads-plain`) also feed
  two pads, one whose actuator lists `trigger-rumble`, and check that a
  rumble with trigger levels reaches each as the right effect.

### Changed

- The ordinary web build reads gamepads with the engine's own backend, the
  one builds with threads used, instead of gilrs (now a desktop-only
  dependency): an 85 KB smaller module (16 KB compressed), the same
  latency (the next frame), and rumble played on the pad's browser index
  instead of finding the pad again by its name. The web chapter of the
  guide has the measurements.

### Fixed

- Web builds with threads apply gilrs's dead zone (a tenth of the travel,
  rescaled) to sticks and triggers, as the ordinary web build and the
  desktop do; small stick movements no longer read as input there.
