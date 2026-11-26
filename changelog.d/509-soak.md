### Added
- Timed reference-game and terrain-streaming soaks (`scripts/soak.py`) with
  repeated play, save and restart paths, resize and minimize/restore cycles,
  CSV samples (memory, live handles by kind, GPU memory, allocations per
  frame, audio voices, frame-time percentiles, load average) and
  post-warm-up slope bounds. The ten-minute offscreen run is the `soak` job
  of the list of checks; the four-hour desktop run is in the release steps.
