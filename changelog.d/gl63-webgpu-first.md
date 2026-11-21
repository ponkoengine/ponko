### Changed
- Web builds open WebGPU more carefully before they commit to it: an
  adapter, a device and the device's first round trip to the GPU come
  before the canvas gets its WebGPU context. A browser with no
  `navigator.gpu`, no adapter, a device that won't open or a device lost
  as it opens now draws with WebGL2 instead of failing to start (before, the
  last two stopped the game with a startup error), and the console says why
  in one line (`ponko: falling back to WebGL2: ...`).

### Added
- The web guide's "WebGPU first, WebGL2 as the fallback": the order, the
  console lines, and a table of what each backend draws, generated from the
  GPU tier code and checked by a test (`PONKO_UPDATE_BOOK=1` rewrites it).
- `web/smoke/webgpu-fallback.mjs`: each way into the WebGL2 fallback, and
  the WebGPU path itself, in a headless browser, with the time to the first
  frame on each.
