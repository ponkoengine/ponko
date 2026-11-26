### Fixed
- Crowds draw on Direct3D 12: the crowd vertex shader no longer writes to a
  local array at an index known only at run time, which FXC (Direct3D 12's
  default shader compiler) rejected when it built the crowd motion-vector
  pipeline.
- On Direct3D 12, instance cells drawn together match each cell drawn
  alone: the mesh vertex entry points read `instance_index` themselves, so
  naga adds the draw's first instance to it (Direct3D 12's instance ID
  starts at 0 whatever the first instance is).
- A render test that panics on Direct3D 12 no longer leaves the test binary
  waiting forever for its stuck GPU work: the device of a panicked test is
  kept alive instead of dropped.
- Render tests honour `WGPU_BACKEND` (`dx12`, `vulkan`, `metal`, `gl`), so
  either backend can be tested on Windows; the offscreen benchmarks too.
- The Python check scripts read and write their files as UTF-8, so they run
  on Windows, where the default encoding is not.
- `python3 scripts/fmt-workspace.py --check` checks the workspace format in
  batches; the format check uses it, because `cargo fmt --all` passes
  Windows' command-line limit in a checkout with a long path.
- Render tests hold on more adapters: blend rounding a level apart, an
  exact colour compare on a half level, the GPU memory figure counting
  textures as the allocator places them, and the GI negative control.
- `ponko cook`'s copy of the material shader parts matches the engine's
  mesh entry points again.
