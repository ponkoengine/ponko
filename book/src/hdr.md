# HDR displays

> **Maturity: [experimental](maturity.md).** Game: none. Tests: unit tests of
> the choice logic and offscreen encoding tests (`hdr_render_test.rs`).
> Platforms: the encoding was checked offscreen on Apple M-series (Metal); the
> other backends by reading wgpu's code. Graphics: Metal offscreen; DirectX 12
> and Vulkan only from source. Missing: no HDR display has been looked at on
> any system (the chapter's platform matrix).

On an HDR display a game can show highlights (the sun on metal, a fire, a
lamp) several times brighter than white, while its UI stays at a comfortable
brightness. HDR output is **off by default**, and then nothing changes: the
frame is drawn exactly as on an SDR display, with no extra pass.

Turn it on before the window opens, or at any time while the game runs
(a settings menu): no restart is needed.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
use ponko::render::{HdrOffReason, HdrOutput};

let mut hdr = HdrOutput::on();
hdr.paper_white_nits = 200.0; // UI and sprites
Game::new("Bright", 1280, 720)
    .hdr_output(hdr)
    .run_headless(1, |ctx| {
        let status = ctx.gfx.hdr_status();
        if !status.available {
            // the settings menu greys out its HDR switch
        }
        if status.reason == Some(HdrOffReason::Off) {
            // HDR is available but switched off
        }
        let mut now = ctx.gfx.hdr_output();
        now.peak_nits = 600.0; // the player's slider
        now.max_frame_average_nits = 300.0; // HDR10 metadata
        ctx.gfx.set_hdr_output(now); // takes effect next frame
    });
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:hdr_output}}
```

</div>
</div>

## What the settings mean

- **`enabled`**: output in HDR where the window's display system offers it.
  While the game runs, switching it switches the window between SDR and
  HDR output from the next frame, without a restart and without making
  anything of the game again. The first switch to HDR of a window opened
  in SDR moves its drawing into the frame image (below): the sprite
  pipeline and the passes that end the 3D frame are made again for its
  format, once; textures, meshes and the rest stay. After that a switch
  only reconfigures the window's surface. Off, `hdr_status().reason` is
  `Off`.
- **`paper_white_nits`** (default 200): how bright SDR white is: the UI,
  sprites, text, and the 3D frame up to near white. 80 to 400 is usual;
  players in bright rooms want more. On macOS and in browsers the system
  sets SDR white from its brightness slider, so there it only scales the
  peak.
- **`peak_nits`** (default 0): the brightest highlight. 0 asks the display:
  Windows reports the panel's peak in nits and macOS its headroom over SDR
  white right now (it changes with brightness and battery); with neither,
  1000 nits. A game's calibration screen sets it from the player's choice.
- **`max_content_light_nits`** and **`max_frame_average_nits`** (default
  0): HDR10 metadata, see [below](#hdr10-metadata). 0 means the peak and
  paper white.

`ctx.gfx.hdr_status()` says what the window does: whether HDR is
`available` (the display system offers an HDR color space) and `active`,
the `encoding`, the paper white, peak and `headroom` (peak over paper white)
in use, what the display reported, and the `reason` when HDR is off.
`available` says nothing of the panel: Windows and macOS offer HDR on an SDR
display too and tone map it down.

## How the frame is made

With HDR on, everything is drawn into one floating-point image in which 1.0
is paper white, and one full-screen pass encodes it for the display:

| Encoding | Image | Where | 1.0 is |
|---|---|---|---|
| scRGB (linear, BT.709) | 16-bit float | Windows (DX12, Vulkan), macOS (Metal EDR) | 80 nits on Windows, so paper white is written as `paper / 80`; the system's SDR white on macOS |
| PQ (HDR10, BT.2020) | 10-bit | Windows and Vulkan without scRGB, macOS as a fallback | absolute nits: paper white is written as `paper_white_nits` |
| Extended sRGB | 16-bit float | Browsers with an HDR canvas (WebGPU, `toneMapping: "extended"`) | the browser's SDR white |

The renderer picks the first the surface offers, in that order per backend
(`hdr_output.rs`). wgpu 30 does the platform work from the configured color
space: Metal's `wantsExtendedDynamicRangeContent` and the layer's color
space, DXGI's `SetColorSpace1`, Vulkan's `VkColorSpaceKHR`, the canvas's
`toneMapping`.

- **The 3D frame** keeps its tone mapper's look: below 80% of white the SDR
  curve, bit for bit; above it the highlights open up to the headroom,
  meeting the curve smoothly and keeping the SDR picture's hue. This holds
  for ACES, AgX and Khronos PBR Neutral alike; with `Tonemap::None`, light
  is kept as it is up to the peak. Split toning and lookup tables grade the
  SDR-range picture as before.
- **Sprites and UI** are not tone mapped: their white is paper white.
- **Debug drawing** (`ctx.debug`) keeps its colours: 2D shapes are drawn
  like sprites, and 3D lines go through the tone mapper below its knee,
  where HDR and SDR are the same curve.
- **The inspector** (egui) is drawn as into an SDR window, into an image
  of the window's SDR format, and laid over the frame by the output pass
  at paper white: its colours and blending are those of SDR. An offscreen
  test compares the UI, debug lines (2D and 3D) and an inspector panel
  with HDR on against the SDR frame (`hdr_render_test.rs`); the largest
  difference is under a fifth of the tolerance (0.004 + 3%).
- **Screenshots** of an HDR frame are SDR: what is brighter than white clips.
- **Cost**: one full-screen pass and one 8-byte-per-pixel image (16 MB at
  1920x1080), from the first time HDR is on. With the inspector open, one
  more 4-byte-per-pixel image (8 MB). A window that never turns HDR on has
  none of them.

## HDR10 metadata

A display tone maps a PQ signal with the content's static metadata: the
mastering display's peak and black (SMPTE ST 2086), the brightest pixel
(MaxCLL) and the brightest frame average (MaxFALL, CTA-861.3). The engine
sends the peak in use as the mastering peak, black 0,
`max_content_light_nits` (or the peak) and `max_frame_average_nits` (or
paper white), each at most the peak, again whenever they change:

| Platform | How | Sent for |
|---|---|---|
| Windows (DX12) | `IDXGISwapChain4::SetHDRMetaData` | PQ and scRGB |
| macOS, iOS (Metal) | `CAMetalLayer.EDRMetadata` (HDR10) | PQ only |
| Vulkan (Windows, Linux) | not sent: `vkSetHdrMetadataEXT` needs the `VK_EXT_hdr_metadata` device extension, which wgpu 30 doesn't enable | — |
| Browsers | not sent: canvases have no metadata API | — |

On macOS scRGB, the engine's first choice there, is relative to the
system's SDR white and the system tone maps it to the display itself, so it
takes no metadata. `hdr_status().metadata` says what was sent; on Vulkan,
in browsers, on macOS scRGB and in SDR it is `None`.

## Platform matrix

What was checked for this feature, and how. "Offscreen" means the engine's
GPU tests (`src/render/hdr_render_test.rs`), which draw into float images on
the test machine's GPU and compare against reference math; they need no HDR
display and cannot show what a display does with the signal. "Source" means
reading wgpu 30.0.1's backend code for what it reports and sets.

| Platform | Choice logic | Encoding math | Seen on an HDR display |
|---|---|---|---|
| macOS (Metal EDR, scRGB) | unit test with the capabilities wgpu-hal's Metal backend reports (source) | offscreen, on Apple M-series (Metal) | no: needs a person |
| Windows (DX12, scRGB or PQ) | unit test with the DX12 backend's report (source) | offscreen (same shader; run on Metal) | no: needs a person |
| Vulkan (Windows, Linux) | unit tests: scRGB, then PQ, then extended sRGB, or SDR when the driver exposes none | offscreen (same shader) | no: needs a person |
| Web, WebGPU (extended sRGB canvas) | unit test with what wgpu's WebGPU backend reports (source) | offscreen (same shader) | no: needs a person; wasm not built for this |
| Web, WebGL2; OpenGL | unit test: no HDR (wgpu reports sRGB only), status `Unsupported` | — | — |
| iOS, Android | not checked | — | — |

Known gaps, from wgpu rather than the engine:

- **HDR10 metadata on Vulkan and the web**: none (see above); a PQ display
  tone maps with its defaults there.
- **wgpu-hal 30.0.1** crashed configuring a Metal surface for PQ (it read
  the color space name one pointer too shallow); the engine needs 30.0.2,
  which a test configuring a bare `CAMetalLayer` for PQ covers.
- **Peak on Linux/Vulkan and the web**: wgpu reports no nits there; the peak
  is 1000 nits unless the game sets one.
- **iOS** reports no headroom in wgpu 30 (`UIScreen.currentEDRHeadroom` is a
  wgpu TODO).

## Needs a person with an HDR display

The tests can't see a display. With an HDR monitor or laptop, HDR turned on in
the system settings:

1. Run a 3D example with `Game::hdr_output(HdrOutput::on())` added (for
   example the 3D tutorial's scene with a bright emissive or the sun in
   view). Print `ctx.gfx.hdr_status()` once a second.
   - Windows: HDR on in Settings, Display. Expect `active: true`,
     `encoding: ScRgb`, `display_peak_nits` near the panel's rating.
   - macOS (a MacBook Pro XDR or Pro Display XDR): expect `ScRgb` and
     `display_headroom` above 1, falling as you raise the brightness.
   - Chrome on an HDR display (WebGPU): expect `ExtendedSrgb`.
2. Look at it: highlights (the sun's glint, emissives) must be clearly brighter
   than the UI's white; the UI must look as bright as other SDR windows on the
   desktop at the default paper white (raise `paper_white_nits` if it looks
   dim on Windows, where it should match Windows's "SDR content brightness"
   slider at about 200).
3. Mid-tones (skin, grass, grey walls) must look the same as with HDR off:
   switch `enabled` off and on with a key and compare.
4. Move the window to an SDR monitor and back (Windows, macOS): the picture
   must stay correct, the status's display values follow within two seconds.
5. Set `peak_nits` to 400, then 1000: highlights get brighter, the UI doesn't.
6. Take a screenshot (`ctx.gfx.queue_screenshot`) with HDR on: the PNG looks
   like the SDR frame, highlights clipped.
7. Turn HDR off in the system settings while the game runs: the game keeps
   drawing (the system tone maps), no errors.

8. Switch HDR on from a game started with it off (a key calling
   `set_hdr_output`): the window turns HDR without a flicker longer than a
   frame, and back.
9. Open the inspector (`` ` ``) with HDR on: its panels look as bright and
   coloured as with HDR off.
10. Windows with a PQ (HDR10) monitor, and macOS forced to PQ: the status
    shows `metadata: Some(..)`; the monitor's info menu (where it has one)
    shows the MaxCLL and MaxFALL the game set.

Report what you saw with the platform, display model and the status printout,
so the matrix above can say who verified what.

## C and C++

`PonkoHdrOutput` (a [versioned descriptor](c-cpp.md): set `struct_size` and
`version`, then `ponko_hdr_output_defaults`) goes to `ponko_game_hdr_output`
before the window opens and to `ponko_ctx_set_hdr_output` while the game runs
(switching HDR on and off, version 2 adds the HDR10 metadata fields);
`ponko_ctx_hdr_output` reads it back. `ponko_ctx_hdr_status` fills a versioned
`PonkoHdrStatus` (`PonkoHdrEncoding`, `PonkoHdrOffReason`; unknown display
values are 0; version 2 adds `metadata_sent` and the metadata sent). C++ wraps them as `ponko::hdr_output_defaults()`,
`game::hdr_output`, `ctx::hdr_output`, `ctx::set_hdr_output` and
`ctx::hdr_status`, as in the example above.
