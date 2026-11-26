### Added
- `Graphics::video_memory()`: what the operating system says about the GPU's
  own memory for the game (`perf::VideoMemory`: the card's size, the budget
  the game may use before the system moves some of it to system memory, and
  what it uses now), for settings screens that warn when the chosen quality
  won't fit. Windows (DXGI, any backend) for now; `None` elsewhere.
- `Graphics::gpu_memory()`: the GPU memory the engine holds, by class, as
  the performance report counts it, for games to read directly.
