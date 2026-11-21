# Upscaling: a temporal upscaler, and the platforms' own (#140)

Render scale (`Post::render_scale` below 1) draws the 3D frame smaller and
stretches it to the window with one bilinear pass and an optional
sharpening filter (`Post::sharpen`). Detail smaller than a drawn pixel is
gone. Other engines render at a fraction of the output size and rebuild the
detail over time from a jittered camera and motion vectors. This is the
evaluation of doing that here, on the existing TAA history and motion
vectors, and of the platform upscalers a game could opt into.

## Verdict

The built-in temporal upscaler measures well, so it ships as a quality
setting: `Post::upscaler = Upscaler::Temporal` (C: `PonkoPostEx` version 3,
`upscaler`), off by default. It roughly halves the error of today's
stretch at the same scale on every scene, with fewer badly wrong pixels
and no trail behind moving models, and at 67% the whole frame costs less
GPU time than native at both 1440p and 4K. No preset turns it on: Low has
no TAA, and WebGL2 can't run TAA.

## How it works

It reuses TAA: the jittered camera, the object motion vectors plus the
camera motion worked out from depth, the nearest-depth dilation, the
disocclusion test and the color clipping. What changes:

- The 3D frame draws at the render size with TAA on, but TAA's own resolve
  at the render size is skipped. After tone mapping, one pass
  (`fs_upscale` in `taa.wgsl`) replaces the bilinear stretch: it reads the
  tone mapped render-size image, the motion vectors, depth and a
  window-size history (`Rgba16Float`, two images), and writes the new
  history and the window's picture.
- Each window pixel weights the 3x3 render-size samples around it by
  their jittered position's distance to the pixel's center
  (`exp(-2.29 d^2)`, d in window pixels), and blends them into the history
  in proportion to that weight: a pixel no sample landed near this frame
  keeps its history. The fresh share is TAA's (luma-weighted, 0.1 at rest,
  more when moving); the sum of the weights is normalized by its average over the
  jitter sequence.
- The jitter sequence is longer than TAA's 16 phases: `ceil(8 / scale^2)`
  phases, clamped to 16..128 (18 at 67%, 32 at 50%), so every window pixel
  is covered several times per cycle.
- With no usable history (first frame, a cut, just uncovered) the pixel
  starts from a smooth upsample of the drawn frame.
- `taa_sharpness` sharpens the result against that smooth upsample, clamped
  to the neighbourhood's range so it can't ring.
- Depth is read from sample 0 only (with MSAA, all samples cost 1.5 ms
  more at 1440p for no visible gain).

## Method

`src/render/upscale_render_test.rs`, two ignored tests, run by hand in
release:

```sh
PONKO_UPSCALE_EVAL_OUT=<dir> cargo test --release -p ponko --features 3d --lib \
    render::upscale_render_test -- --ignored --nocapture --test-threads 1
```

- `upscaling_evaluation`: a 480x272 window, five scenes, 60 frames each,
  scored from frame 30 (the still scene from frame 50). The reference is
  the same scene drawn at twice the size (4x MSAA) and averaged down.
  The error is the mean absolute difference in 8-bit sRGB levels, and the
  share of pixels off by more than a fifth of the range. It writes every
  method's last frame, a difference image of each, and `errors.md`.
- `upscaling_gpu_time`: 2560x1440 and 3840x2160 offscreen, four thousand
  models, sun shadows, TAA, ambient occlusion, reflections and bloom (about
  the High preset), the camera moving slowly. 30 warm-up frames, then the
  median of 60 frames of the engine's pass timers; the frame time is from
  the first pass's start to the last pass's end (passes overlap on a
  tile-based GPU, so their sum would count some time twice).

Scenes: *still* (fine stripes on a far wall, thin posts, a checker floor:
convergence); *camera_pan* (the camera moves sideways); *thin_motion* (a
box crossing the stripes and posts: ghosting); *disocclusion* (a box moving
fast enough to uncover a wide strip every frame); *foliage* (cutout leaves,
alpha-tested, on a moving camera).

Methods: *native_taa* (the window's size with TAA); *stretch_N* (4x MSAA at
N%, the bilinear stretch with `sharpen` 0.5: today's render scale);
*taa_stretch_N* (TAA at N%, then the stretch); *temporal_N* (TAA's inputs
at N%, the temporal upscaler, `taa_sharpness` 0.25).

Adapter: Apple M4 Max, Metal. The machine was busy with other builds and
tests (load average 75 to 136 during the runs), so the GPU times are noisy:
each is the median of three runs (each run 20 to 25 s), and differences
under about 0.5 ms are noise.

## Image quality

Mean error / share of pixels off by more than a fifth (lower is better):

| scene | native_taa | stretch_50 | taa_stretch_50 | temporal_50 | stretch_67 | taa_stretch_67 | temporal_67 |
|---|---:|---:|---:|---:|---:|---:|---:|
| still | 4.92 / 0.09% | 14.56 / 4.74% | 14.28 / 4.31% | 5.86 / 0.82% | 11.74 / 4.49% | 10.94 / 2.31% | 5.42 / 0.48% |
| camera_pan | 8.48 / 2.76% | 14.52 / 4.76% | 18.36 / 9.78% | 9.85 / 2.35% | 11.72 / 4.42% | 14.87 / 6.72% | 9.81 / 1.90% |
| thin_motion | 5.76 / 0.65% | 14.55 / 4.75% | 14.78 / 4.69% | 6.97 / 1.34% | 11.73 / 4.43% | 11.55 / 2.81% | 6.16 / 0.81% |
| disocclusion | 5.20 / 0.45% | 13.79 / 4.57% | 14.18 / 4.49% | 7.75 / 2.00% | 11.07 / 4.22% | 10.85 / 2.55% | 6.73 / 1.42% |
| foliage | 8.64 / 2.65% | 14.99 / 4.89% | 16.76 / 7.30% | 9.96 / 2.29% | 12.22 / 4.45% | 14.02 / 5.19% | 9.05 / 1.47% |

What the images show:

- *still*: at 50% the stripes and posts come back almost as in the
  reference; both stretches turn the stripes into a moire of blocks.
- *camera_pan*: the stripes stay defined and the thin posts stay whole
  while the camera moves; with the stretches they are blurred and broken.
  The temporal picture is a little softer than native TAA.
- *thin_motion*: no trail of the box over the stripes (the test
  `a_box_crossing_fine_stripes_leaves_no_trail_behind_it` checks it).
- *disocclusion*: the strip the box just uncovered is softer than the rest
  for a few frames (the smooth upsample it restarts from) and shows no
  color of the box. This is the scene with the largest gap to native.
- *foliage*: the green leaves survive at 50%, slightly smeared; the
  stretches lose most of them (alpha testing at the render size drops
  leaves smaller than a drawn pixel).

## GPU time

Frame GPU ms; the resolve and scaling columns are the TAA resolve pass at
the render size and the scaling pass (bilinear stretch, or the temporal
upscale):

| window | method | frame | TAA resolve | scaling |
|---|---|---:|---:|---:|
| 2560x1440 | native_taa | 7.02 | 1.74 | |
| 2560x1440 | taa_stretch_67 | 6.00 | 1.19 | 0.09 |
| 2560x1440 | temporal_67 | 6.38 | | 1.44 |
| 2560x1440 | taa_stretch_50 | 5.05 | 0.79 | 0.10 |
| 2560x1440 | temporal_50 | 5.43 | | 1.44 |
| 3840x2160 | native_taa | 14.50 | 3.98 | |
| 3840x2160 | taa_stretch_67 | 7.25 | 1.77 | 0.11 |
| 3840x2160 | temporal_67 | 7.26 | | 1.89 |
| 3840x2160 | taa_stretch_50 | 6.03 | 1.29 | 0.14 |
| 3840x2160 | temporal_50 | 6.45 | | 2.27 |

At 4K, 67% halves the frame (14.5 to 7.3 ms) and the temporal upscaler
costs the same as TAA plus the stretch. At 1440p this frame is less
pixel-bound: 67% saves about 0.6 ms with the temporal upscaler (1.0 ms
with the stretch). The temporal pass costs about the same at 50% as at
67%, since it runs once per window pixel.

## Follow-ups

- TAA's own resolve reads every MSAA depth sample of a 3x3 neighbourhood
  (36 loads per pixel with 4x MSAA); in the upscaler, reading sample 0
  alone saved 1.5 ms at 1440p, so the resolve may save a similar share.
- A negative mip bias on material textures while upscaling (about
  `log2(scale)`) would bring back texture detail that the render size's mip
  level drops.
- The platform upscalers below would need a pass writing full motion
  vectors and a single-sample depth.

## The platform upscalers

What a game would have to give each one, from their official documentation
(read 2026-11-21); nothing was downloaded or linked.

| | FSR (AMD) | DLSS Super Resolution (NVIDIA) | XeSS-SR (Intel) | MetalFX temporal scaler (Apple) |
|---|---|---|---|---|
| Color | render size, jittered, linear (HDR flag) | render size | render size, linear, HDR recommended | render size, jittered |
| Depth | render size, flags for inverted and infinite depth | yes | only with render-size motion vectors | yes, `isDepthReversed` flag |
| Motion vectors | current to previous frame, in pixels (scale field for other units), render or window size | yes, scale to [-1, 1] given | current to previous, pixels, R16G16 float; dilated window-size, or render-size plus depth | yes, with x/y scale; flag if they include the jitter |
| Jitter | offset in pixels per frame (Halton 2,3 suggested) | offset in pixels | offset in -0.5..0.5, at least 72 positions for the lowest mode | `jitterOffsetX/Y` per frame |
| Exposure | optional 1x1 texture, or auto exposure | optional 1x1 texture, else auto | optional texture or value, or auto | exposure texture, `preExposure`, or auto |
| Masks | reactive and transparency/composition masks (optional from FSR 4) | not in the guide read | responsive pixel mask, optional | reactive mask, optional |
| Other | frame time, camera near/far for debugging | camera matrices without jitter | | reset flag |
| APIs | DirectX 12, Vulkan | DirectX 11, DirectX 12, Vulkan (Windows, Linux); not Metal | DirectX 11 (Arc only), DirectX 12, Vulkan 1.1; Windows 10/11 x64 | Metal: macOS 13, iOS/iPadOS 16 and later |
| GPUs | FSR 3.1: compute shader model 6.2, typed UAV loads and RGBA16 unorm, no vendor named; FSR 4: AMD Radeon 7000/9000 series and some Ryzen APUs | NVIDIA RTX (Tensor cores); the licence allows NVIDIA GPUs only | shader model 6.4 with DP4a on any vendor; tuned for Intel Arc | Apple GPUs that `supportsDevice` accepts |
| Licence | MIT for the source files listed; prebuilt signed runtime DLLs under AMD's binary licence (redistribute unmodified, no reverse engineering, keep notices); FSR 4 needs the signed DLLs | NVIDIA RTX SDKs licence: proprietary, object code inside an application only, attribution with NVIDIA marks, notify NVIDIA before commercial release, may not be made subject to an open source licence | Intel Simplified Software License: binary only, redistribute unmodified, keep notices, no reverse engineering | part of the OS; no separate licence on the pages read |
| What ships | a runtime DLL next to the executable | the DLSS runtime DLL(s) next to the executable (through Streamline: signed plugin DLLs) | `libxess.dll` (or `libxess_dx11.dll`) plus the VC++ runtime | nothing: an OS framework |
| Binary size | not stated in the documentation | not stated | not stated | none (in the OS) |

Sources: [FSR 3.1 upscaler](https://gpuopen.com/manuals/fsr_sdk/techniques/super-resolution-upscaler/),
[FSR 4](https://gpuopen.com/manuals/fsr_sdk/techniques/super-resolution-ml/),
[FSR licence](https://gpuopen.com/manuals/fsr_sdk/license/),
[DLSS](https://developer.nvidia.com/rtx/dlss/get-started) and its
[licence](https://github.com/NVIDIA/DLSS/blob/main/LICENSE.txt),
[Streamline DLSS guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS.md),
[XeSS](https://github.com/intel/xess) and its
[developer guide](https://www.intel.com/content/www/us/en/developer/articles/technical/xess-sr-developer-guide.html),
[MetalFX](https://developer.apple.com/documentation/metalfx) and
[`MTLFXTemporalScaler`](https://developer.apple.com/documentation/metalfx/mtlfxtemporalscaler).

### What the renderer already has for them

Everything except the masks: a jittered render-size color (the TAA input,
HDR and linear), reversed-Z depth (multisampled: a vendor library wants a
single-sample copy, one resolve pass), the jitter sequence (Halton 2,3;
longer for upscaling), and motion vectors. The motion vectors are the
catch: the renderer keeps only the *object* motion in a texture (camera
motion is worked out from depth inside the resolve), so a vendor upscaler
needs one more full-screen pass that adds the camera motion and writes
full motion vectors in pixels. Exposure is known (`Post::exposure`), so
no auto exposure is needed. A reactive mask would need the transparent
pass and particles to write one more target.

### Can wgpu 30.0.1 hand them native textures?

Yes on the native backends, with `unsafe` code per backend (wgpu-hal):

- `Device::as_hal`, `Queue::as_hal`, `Texture::as_hal`,
  `TextureView::as_hal` and `CommandEncoder::as_hal_mut` give the backend's
  objects: on Vulkan `vk::Image` / `vk::ImageView` (`raw_handle`), the
  `ash::Device` and `vk::Queue`, and the encoder's `vk::CommandBuffer`; on
  Metal the `MTLTexture`, `MTLDevice` and the encoder's `MTLCommandBuffer`
  (`raw_command_buffer`); on DirectX 12 the `ID3D12Resource`
  (`raw_resource`), `ID3D12Device` and `ID3D12CommandQueue`, but **not** the
  encoder's command list (it is private in wgpu-hal 30.0.1), so a DirectX 12
  integration records into its own command list on the raw queue and orders
  it against wgpu's work with a fence (`raw_fence`).
- `Device::create_texture_from_hal` wraps a texture the library made.
- wgpu does not see what the library does to a texture's state: the
  integration must leave every texture in the layout or state wgpu expects.
- Not on WebGPU or WebGL2 (no native handles in a browser), so the web
  keeps the engine's own upscaler.

### Recommendation

Each of them is a per-backend, per-vendor `unsafe` integration with a
licence the engine can't carry by default (DLSS: proprietary terms with
attribution and notification; XeSS and the FSR DLLs: binary-only
redistribution; MetalFX: Apple only). They belong in optional plugin crates
a game opts into, not in the engine. What the engine can do for them is
keep the inputs ready: a pass writing full motion vectors in pixels, a
single-sample depth, and the jitter and exposure as public per-frame
values. That is a follow-up, worth doing when a game asks for one of them.
