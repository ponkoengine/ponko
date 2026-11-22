### Added
- Material shaders can call `texture_mip_bias()`: the mip bias the engine
  samples material textures with (negative while the temporal upscaler
  runs, else 0), for a shader's own `textureSampleBias`.

### Changed
- While `Upscaler::Temporal` draws the frame smaller than the window,
  material textures and decals are sampled with a mip bias of `log2` of
  the render scale, so they keep the detail of the window's resolution
  instead of reading a coarser mip level. Shadow maps and the stretch
  keep no bias.
- The temporal antialiasing resolve reads one depth sample a pixel
  instead of every sample: about 0.8 ms less GPU time at 2560x1440 on an
  Apple M4 Max (1.08 ms instead of 1.84 to 2.06), with the same pictures.
- The motion pass is skipped on frames where nothing draws into it (no
  moved model, moving `vertex` function, crowd or held item) and its
  image already holds only zeros. The guide's "Temporal antialiasing"
  section has the costs.
