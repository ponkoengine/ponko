### Added
- Temporal upscaling: `Post::upscaler = Upscaler::Temporal` (with
  `AntiAliasing::Taa` and a `render_scale` below 1) rebuilds the detail a
  lower render scale loses over frames, from TAA's jitter, motion vectors
  and history, instead of stretching the frame. It replaces TAA's resolve
  with one pass into a window-sized history; moving models leave no trail.
  `Upscaler::Bilinear`, the stretch, stays the default and is unchanged.
  C: `PonkoPostEx` version 3 adds `upscaler` (`PonkoUpscaler`); C++:
  `ponko::upscaler`. The guide's "Temporal upscaling" section has the
  measured images and GPU times, and what render scale does and doesn't do.
- `crates/ponko_bench/UPSCALING.md`: the evaluation, and what FSR, DLSS,
  XeSS and MetalFX would need from the renderer, their licences, GPUs, and
  how wgpu 30 hands out native textures for them.
- The inspector's quality object has `upscaler`.
