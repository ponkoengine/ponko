### Added
- HDR output switches on while the game runs: `ctx.gfx.set_hdr_output` with
  `enabled` turns a window opened in SDR to HDR from the next frame, with no
  restart (the renderer's pipelines for the window's format are made again
  once; the game's textures and meshes stay). Switching off and on again
  costs a surface configuration.
- HDR10 metadata: `HdrOutput::max_content_light_nits` (MaxCLL) and
  `max_frame_average_nits` (MaxFALL), with the peak in use as the mastering
  peak, sent through DXGI's `SetHDRMetaData` on Windows and
  `CAMetalLayer.EDRMetadata` for PQ on Apple platforms. `HdrMetadata` and
  `HdrStatus::metadata` report what the display was given (`None` on
  Vulkan, in browsers and on macOS's scRGB, where there is no API for it or
  no need). C: `PonkoHdrOutput` version 2 (`max_content_light_nits`,
  `max_frame_average_nits`), `PonkoHdrStatus` version 2 (`metadata_sent`,
  `min_mastering_nits`, `max_mastering_nits`, `max_content_light_nits`,
  `max_frame_average_nits`); C++ through `ponko::hdr_output` and
  `ponko::hdr_status`.

### Changed
- `HdrStatus::reason` is `Off` for a window whose HDR output is off,
  whether or not it was ever on; `HdrOffReason::NotRequested` is no longer
  reported (kept so code matching on it builds).
- wgpu-hal 30.0.2 (from 30.0.1): 30.0.1 crashed configuring a Metal surface
  for PQ output.

### Fixed
- With HDR output on, the inspector keeps its SDR colours and brightness:
  it is drawn as into an SDR window and laid over the frame at paper white
  (it used to look lighter). UI and debug drawing stay within 8-bit
  rounding of the SDR frame, checked by an offscreen test.
