<!-- area: Rendering -->
### Fixed

- Web games no longer stop at random while starting in Safari on iOS
  ("mapped GPU buffer is 0 bytes"). Safari there sometimes gives an empty
  range for a buffer made mapped, so on WebGPU new buffers are now made
  empty and filled through the queue. Native builds still map them.
