### Added
- `perf::net_alloc_bytes()`: bytes allocated minus bytes freed while
  profiling is on, counted by `CountingAllocator`. Two readings around a
  stretch of play tell how much of what it allocated is still held, the
  number a leak check watches. The counting allocator now also does one
  relaxed atomic load per free.
- Long-run tests: 200 000 headless ticks of spawning, despawning, deferred
  spawns, families, scene loads, events, handles, sounds, UI screens, game
  states, debug drawing and camera rigs keep memory, archetypes, the query
  cache and ids flat after warm-up; 2000 offscreen frames of textures,
  materials, meshes, instance cells and render targets bring the GPU
  memory report back to its baseline; ten simulated hours keep the clocks
  exact. The guide's "Very long sessions" (in "The simulation clock") says
  what holds and what the limits are.
- `Animator::normalized_time_f64` and `skeleton2d::Skeleton::normalized_time_f64`:
  the unwrapped normalized clock in full precision, exact after any length
  of play (`normalized_time` hands out the same clock as an `f32`).

### Changed
- Textures (render targets included), sounds and fonts reuse a freed one's
  table slot under a new handle, as meshes and GPU buffers already did: a
  game that makes and frees them all session keeps its tables (the engine's,
  the live-handle tables, the window renderer's and the audio backend's) the
  size of the most it held at once. Handles are still never given out twice
  (a slot is retired after 4095 uses), a freed handle stays dead, and a
  sprite or material still showing a freed texture draws white even after a
  new texture takes its slot. Handle ids no longer count up one by one.
- `Ui::remove` gives the removed nodes' slots back: the next nodes take
  them under new ids (a generation in `UiId`'s upper bits), so a UI that
  builds and removes menus all session no longer grows. Ids are still
  never given out twice, and a removed node's id stays stale.
- A looping sprite-sheet `Animation`'s `time` stays within one loop
  (seconds into the current loop) instead of counting up forever.

### Fixed
- `Ui::remove` kept every removed node's slot (about 400 bytes each): a
  three-node menu built and removed every few frames grew memory by about
  1.3 KB each time.
- A freed 3D material kept its own uniform buffer and bind group (128
  bytes of GPU memory each, for every material ever made); freed materials
  now share one.
- The window renderer made a new bind group for every texture it freed;
  freed textures now share one.
- `Time::elapsed` drifted from real time by the rounding of each frame's
  `dt` to `f32`: 1.9 ms after ten hours at 60 Hz (and away from
  `sim_time` in headless runs). It now adds the frames up in full precision.
- A looping sprite-sheet animation ran slow after hours of play (6% after
  ten hours at 60 Hz) as its `f32` time lost precision.
- Every texture, sound and font made kept memory after it was freed (about
  1, 32 and 280 bytes headless, and a table entry in the window renderer
  and the audio backend): by those figures 175 000 textures and sounds
  made and freed held about 5.8 MB; measured now, 4.7 KB (retired slots).
- A 3D `Animator` or 2D `Skeleton` in one looping state for hours kept its
  clock in an `f32`: after ten hours at 60 Hz it was 545 loops of a
  one-second clip ahead and stepped in 1/256 s. The clocks are `f64` now.
