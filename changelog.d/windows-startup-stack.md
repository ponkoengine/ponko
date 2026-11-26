### Fixed
- Windows: a game's windowed startup no longer overflows the main thread's
  1 MB stack in a debug build ("thread 'main' has overflowed its stack" before
  setup ran). The renderer's 3D pass and the `Ctx` are kept on the heap
  instead of being moved through the startup's stack frames, and the windowed
  loop takes the setup closure boxed, so it is compiled with the engine rather
  than in the game's crate. A debug build's windowed startup with a 3D mesh
  now runs in under 272 KB of stack (it needed more than 1 MB), on Direct3D 12
  and Vulkan; a release build too. The examples are no longer linked with an 8 MB stack on Windows:
  they don't need it (#591).
- Tests run startup on a thread with 512 KB of stack: the headless startup
  always, the whole windowed one on Windows as an opt-in check (it opens a
  window).
