<!-- area: Rendering -->
### Changed
- A material shader's pipelines (about ten per set) are built at once on the
  frame pool instead of one after the other, natively on every backend but
  GL. Each pipeline compiles its shaders when it is created, so this shortens
  the 3D renderer's start and the first use of a shader: on a 6-core desktop
  a water render test (three renderers) went from 9.5 to 4.6 s on Direct3D 12
  with DXC and about halved on Vulkan. With FXC, Direct3D 12's default
  compiler, the gain is smaller (8 to 37%), as it mostly compiles one shader
  at a time. A pipeline the GPU rejects is built again on the calling thread,
  so a game shader's error is reported as before.

### Fixed
- Direct3D 12 with FXC (its default shader compiler) no longer spends 18 to
  30 s compiling the global illumination probe update in a debug build, and
  over 8 minutes with a release build's optimizations, each time a renderer
  starts global illumination. wgpu zeroed the shader's workgroup memory from
  one thread, which FXC expanded store by store; the shader writes all of it
  before reading, so the pipeline is now built without that zeroing (1 and
  4 s). A global illumination render test went from 184 to 38 s.
