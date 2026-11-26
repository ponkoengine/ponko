### Changed
- The web smoke runner (`web/smoke`) uses playwright-core 1.65.1 (Chromium 159,
  Firefox 159, WebKit 27.2). The Firefox of 1.64.1 exits at once on macOS 27,
  so Firefox now runs on a Mac too.

### Fixed
- `runtime-failures.mjs --case device-lost` reports "unsupported" where the
  browser can't present a WebGPU canvas (Chromium's headless shell on a Mac)
  and the game runs on WebGL2, instead of failing after destroying an unused
  device.
- The runner's zero-channel audio check for WebKit sets the channel count on
  the audio destination's prototype, so it no longer fails now and then when
  WebKit returns a fresh `destination` object.
