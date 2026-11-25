### Fixed
- Browser audio tries stereo when WebKit reports zero output channels.
  `Audio::unlocked()` stays false when the browser cannot start the output,
  and the engine logs the reason once.
