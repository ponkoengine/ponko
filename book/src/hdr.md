# HDR displays

On an HDR display a game can show highlights (the sun on metal, a fire, a
lamp) several times brighter than white, while its UI stays at a comfortable
brightness. HDR output is **off by default**, and then nothing changes: the
frame is drawn exactly as on an SDR display, with no extra pass.

Turn it on before the window opens:

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
        if status.reason == Some(HdrOffReason::NotRequested) {
            // the settings menu says "applies after a restart"
        }
        let mut now = ctx.gfx.hdr_output();
        now.peak_nits = 600.0; // the player's slider
        ctx.gfx.set_hdr_output(now);
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
  While the game runs, switching it off and on again switches the window
  between SDR and HDR output (a settings menu), as long as the window
  opened with it on. A window opened without it stays SDR until the next
  start; `hdr_status().reason` is then `NotRequested`.
- **`paper_white_nits`** (default 200): how bright SDR white is: the UI,
  sprites, text, and the 3D frame up to near white. 80 to 400 is usual;
  players in bright rooms want more. On macOS and in browsers the system
  sets SDR white from its brightness slider, so there it only scales the
  peak.
- **`peak_nits`** (default 0): the brightest highlight. 0 asks the display:
  Windows reports the panel's peak in nits and macOS its headroom over SDR
  white right now (it changes with brightness and battery); with neither,
  1000 nits. A game's calibration screen sets it from the player's choice.

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
- **Screenshots** of an HDR frame are SDR: what is brighter than white clips.
- **Cost**: one full-screen pass and one 8-byte-per-pixel image (16 MB at
  1920x1080), only while the window was opened with HDR on. Off, there is
  neither.

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

- **No HDR10 metadata.** wgpu sets the PQ color space but writes no mastering
  metadata (`vkSetHdrMetadataEXT`, DXGI `SetHDRMetaData`), so a PQ display
  tone maps with its defaults. scRGB, picked first on Windows, needs none.
- **Peak on Linux/Vulkan and the web**: wgpu reports no nits there; the peak
  is 1000 nits unless the game sets one.
- **iOS** reports no headroom in wgpu 30 (`UIScreen.currentEDRHeadroom` is a
  wgpu TODO).
- The **inspector** (egui) draws into the HDR image as into an SDR one; its
  colors may look lighter with HDR on.

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

Report what you saw with the platform, display model and the status printout,
so the matrix above can say who verified what.

## C and C++

`PonkoHdrOutput` (a [versioned descriptor](c-cpp.md): set `struct_size` and
`version`, then `ponko_hdr_output_defaults`) goes to `ponko_game_hdr_output`
before the window opens and to `ponko_ctx_set_hdr_output` while the game runs;
`ponko_ctx_hdr_output` reads it back. `ponko_ctx_hdr_status` fills a versioned
`PonkoHdrStatus` (`PonkoHdrEncoding`, `PonkoHdrOffReason`; unknown display
values are 0). C++ wraps them as `ponko::hdr_output_defaults()`,
`game::hdr_output`, `ctx::hdr_output`, `ctx::set_hdr_output` and
`ctx::hdr_status`, as in the example above.
