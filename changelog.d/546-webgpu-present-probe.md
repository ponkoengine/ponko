### Fixed
- Web: a browser whose WebGPU adapter opens devices but can't show a frame
  on a canvas (such as the software fallback adapter of Chromium's headless
  shell on macOS) no longer stops the game on its first frame. Before giving
  the game's canvas a WebGPU context, the engine clears and presents one test
  frame on a small `OffscreenCanvas` of its own inside error scopes; if it
  fails, the console says `ponko: falling back to WebGL2: a canvas can't show
  WebGPU frames: ...` and the game draws with WebGL2. The test frame adds
  about 1 ms to a WebGPU start on a GPU and about 5 ms on a software adapter.
