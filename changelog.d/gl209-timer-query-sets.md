<!-- area: Rendering -->
### Fixed
- A game that makes many devices in one process (tools, previews, tests) no
  longer loses one to the GPU timers on Metal. Metal lets a process hold 32
  timestamp query sets, of any size, across all its devices, and the 33rd
  loses the device. A renderer now holds 2 sets instead of 7 (the upload
  timer keeps its six frames in one set), and the engine takes at most 28,
  so 14 devices are timed at once; a device past that keeps drawing without
  GPU times, with one warning. The guide's "How many devices can be timed"
  section has the measurement.
