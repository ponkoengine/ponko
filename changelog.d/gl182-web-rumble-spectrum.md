### Added

- Trigger rumble in web builds where the browser has it: a pad whose
  `vibrationActuator` lists the `trigger-rumble` effect (in `effects`, or
  through the older `canPlayEffectType`) reports
  `capabilities.trigger_rumble` and plays the trigger levels with the body
  motors' as one `trigger-rumble` effect, replayed before the browser's cap
  like body rumble. On any other pad the trigger levels are dropped and the
  body motors still play (`dual-rumble`), as on the desktop; a rumble of the
  triggers alone plays nothing there. In the ordinary web build and in
  builds with threads.
- `web/smoke/threads.mjs --case gamepads` (and `gamepads-plain`) also feed
  two pads, one whose actuator lists `trigger-rumble`, and check that a
  rumble with trigger levels reaches each as the right effect.
- `web/smoke/spectrum.mjs` and the fixture `examples/web_spectrum.rs`: the
  water kit's CPU wave spectrum timed in a headless browser at 64, 128 and
  256 cells, with and without WASM SIMD.
