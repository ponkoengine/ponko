### Added
- HDR display output, off by default: `Game::hdr_output(HdrOutput::on())`
  opens the window in an HDR format and color space where the display system
  offers one (scRGB on Windows and macOS, PQ/HDR10 next, the extended sRGB
  canvas in browsers with WebGPU). The 3D frame's tone mapping (ACES, AgX,
  Neutral, or none) keeps its SDR look below near white and opens its
  highlights up to the peak; sprites and UI stay at paper white.
  `ctx.gfx.set_hdr_output` changes paper white and peak and switches between
  HDR and SDR while the game runs; `ctx.gfx.hdr_status()` reports the
  encoding, the brightness in use, what the display reports and why HDR is
  off (`HdrOffReason`). Off, the frame is drawn exactly as before. C:
  `PonkoHdrOutput`, `PonkoHdrStatus`, `ponko_game_hdr_output`,
  `ponko_ctx_set_hdr_output`, `ponko_ctx_hdr_output`, `ponko_ctx_hdr_status`;
  C++: `game::hdr_output`, `ctx::hdr_output`, `ctx::set_hdr_output`,
  `ctx::hdr_status`. The guide's "HDR displays" chapter has the platform
  matrix and what to check on a real HDR display.
