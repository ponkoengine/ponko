# Browser smoke test

Checks that a web build actually runs in a browser, which compiling to
wasm can't: `smoke.mjs` serves a site built by `web/build.sh`, opens it in
a headless browser (Chromium, Firefox or WebKit; no window) and checks, in
order:

1. **Startup**: the game reaches its first frame (no panic, no page error).
   The engine's own GPU report (`Graphics::gpu_capabilities`) must then agree with
   the canvas: an engine that says `gl` on a canvas with a WebGPU context, or
   the reverse, fails.
2. **Assets**: `assets/index.json` and the fixture's asset were fetched
   (HTTP 200); a texture also decoded at its real size. In the 2D fixture, an
   image and streamed music from the game's in-memory asset source loaded by
   name without being fetched.
3. **Progress**: the fixed-step simulation keeps ticking (at least 60 ticks
   in 3 seconds, a third of the 60 Hz rate; a coarse bound, not a frame-rate
   assertion, kept above the `stall` self-test's 20 ticks).
4. **Input**: a click on the canvas reaches the game, then each `--press` key.
   **Music** (`--music required`, the 2D fixture): music the game started
   during setup waits for that click. Before it, the game reports
   `audio_unlocked: false` and the page has no `AudioContext` (a browser keeps
   one opened before a gesture suspended); within 10 s after it, the game
   reports `audio_unlocked: true`, an `AudioContext` is `running`, and buffers
   with sound in them have been played. The runner watches the page's
   `AudioContext` and `AudioBufferSourceNode.start` to see this.
5. **State minimums** (`--min`): fields the fixture must report at least
   (the 3D fixture's models and triangles drawn).
   **Storage** (`--after-reload`, `--reload`): once the game's writes are
   stored (its `saving` field is no longer `true`), the page is reloaded, and
   the new page must report the given fields (`coin_dash`: the settings it
   saved when the settings screen was left are `Loaded` from the browser's
   storage, which is `persistent`). A fixture can ask for several reloads,
   each a page load of its own with its own fields (`web_editor_save`: save,
   reload and revert, reload and find the revert held).
   **Persistent storage** (`--persist granted|refused`): the runner answers
   `navigator.storage.persist()` for the browser (headless Chromium always
   refuses, Firefox would ask), `persisted()` says whether an earlier page
   load was granted, and the state gets `persist_asks`, the asks of the page
   load. The fixture's `persist` fields for that answer are checked on the
   first page load and after each reload (`web_editor_save`: the first
   load's saves ask once; the later loads save nothing and never ask).
   **Clicks** (`--click <x>,<y>=<field>:<value>`): a click at a point of the
   canvas (pixels from its top left), then the field the state must reach:
   a button the game draws there. `--click @retry=<field>:<value>` finds
   the editor's **Retry** button (for a glTF scene that didn't load) by
   asking the page: the editor (web builds) sets `<html data-ponko-retry>`
   to the places of its Retry buttons, `[[x, y], ...]` with each centre as a
   fraction of the window's width and height, and the runner waits for it
   (10 s) and clicks the first. A change of the status bar's layout moves
   the button and its answer along (`web_editor_unloaded`).
6. **Rendering**: the canvas is not blank (screenshot, or a copy of the
   canvas for WebGPU).

## Fixtures

A fixture is an example that publishes its state with the helper
`examples/common/web_smoke.rs`: `<html data-smoke>` every frame (ticks,
frames, clicks, size and whatever the fixture adds) and `<html data-smoke-gpu>`
once (the engine's adapter report as JSON). `fixtures.json` says what the
runner needs to know about each one, so `--fixture <name>` is all a run takes:

| Fixture | Example | Features | Checks |
|---|---|---|---|
| `web_smoke` | `examples/web_smoke.rs` | none | `crab.png` fetched and decoded 32x32; an image and streamed music loaded from an in-memory asset source; an asset group of the fetched image and a file from the source, ready (`group_ready` at least 1); the music, started at setup, heard after the first click; sprites drawn |
| `web_smoke_3d` | `examples/web_smoke_3d.rs` | `3d` | `models/house.glb` fetched and parsed; a sun with shadows; at least 1 model and 100 triangles drawn |
| `coin_dash` | `examples/coin_dash` | none | the complete game: its picture, keys and the screens they reach (see [A complete game, packaged](../../book/src/packaging.md#the-web-build)); after a reload, the settings it saved come back from the browser's storage |
| `web_editor_save` | `examples/web_editor_save.rs` | `editor` | `crab.png` fetched and decoded 32x32; three page loads: a new scene and one over a file the build ships are saved and read back at once from the browser's storage (`read_back` 1, `listed` 2); after a reload both come back over the fetched files and the one over the build's file is reverted, read again at once (`found` `Saved`, `reverted` 1); after another reload the revert held and the other save is still there (`found` `Build`, `kept` `Loaded`). It also reports the browser's answer to its request for persistent storage (`persistent`) |
| `web_editor_unloaded` | `examples/web_editor_unloaded.rs` | `editor`, `3d` | the page opens in edit mode (`query`: `?editor=smoke/unloaded/scene.json`, `editMode`: time is paused, so frames must keep coming instead of ticks, and the editor takes the clicks); the scene's post doesn't load (setup saved a broken copy of its file in the browser), the fixture reverts the copy, and a click on **Retry** in the editor's panel of scenes that didn't load (`clicks`: `@retry`, the place the page reports) spawns it and hangs the yard's wall off it again (`failed` 1, then `linked` 1) |
| `lights2d` | `examples/lights2d.rs` | none | `crab.png` fetched and decoded 32x32; 2D lights with stencil shadows and normal maps drawing on the page's backend (WebGL2 in the listed check); at least 7 lights

To add one: publish the state with the helper (or the same attributes by hand),
add an entry to `fixtures.json` (`example`, `features`, `asset`, `assetSize`
or none, `assetSource` or none, `presses`, `min`, `afterReload` (one reload)
or `reloads` (several: a list of steps, each a list of `<field>:<value>`),
`persist` (per `--persist` answer, the fields of the first page load and of
each reload), `clicks`, `query` (the page's address after `/`, like
`?editor=levels/a.json`), `editMode`, `engineGpu`, `music`), and name it in the `web-smoke` job of `scripts/checks.yml`. Every
fixture needs `?smoke_case=panic` and `?smoke_case=stall` handled if it is to
run the self-tests; the helper does that.

## Startup failures

Build `web/build.sh web_smoke` once, then run these cases on the same build:

```sh
for case in missing-asset no-graphics context-lost; do
  node web/smoke/smoke.mjs --fixture web_smoke --backend webgl2 --case "$case"
done
```

The runner returns HTTP 404 for the required texture, blocks both graphics
backends, or loses the real WebGL2 context before setup, respectively. Each
case checks the error from the generated callback and the visible text in
the template. The runner records both in `report.json`.

## Runtime failures

`runtime-failures.mjs` stops the 2D fixture after it started, four ways,
and checks that each reaches the page (the guide's
[Failures while playing](../../book/src/web.md#failures-while-playing)):

```sh
web/build.sh web_smoke
node web/smoke/runtime-failures.mjs                       # all four cases
node web/smoke/runtime-failures.mjs --case context-lost   # one of them
# A lost WebGPU device where a WebGPU canvas presents (Chromium's new
# headless mode; the headless shell loses the device by itself):
node web/smoke/runtime-failures.mjs --case device-lost --chromium-mode new \
  --browser-arg --use-webgpu-adapter=swiftshader --allow-unsupported
```

- `runtime-panic`: the fixture panics in a frame after 20 ticks. The page
  must show “The game stopped: ” and the panic's message (not the trap the
  abort throws after it), and `onPonkoFailure` must give kind `Panic`, the
  message and the panic's location.
- `failure-text`: the same panic, with the fixture's own text registered
  through `ponko::web::on_failure`: the page shows that text, and the
  callback's `text` holds it.
- `context-lost`: on WebGL2, the runner loses the canvas's context while
  the game runs: kind `ContextLost` and its message on the page.
- `device-lost`: on WebGPU, the runner destroys the game's `GPUDevice`
  while the game runs (no browser lets a page crash its GPU process; a
  destroyed device is lost the same way to the game): kind `DeviceLost`,
  `reason` `Destroyed` (`Unknown` when the browser lost it first, as the
  page's own `device.lost` says), and the engine's message for that reason
  with the browser's words in it (“The WebGPU device was destroyed while
  the game was running: Device was destroyed.”). Where the game draws no WebGPU canvas (the
  browser has no WebGPU, or can't present one, as Chromium's headless shell
  on a Mac can't, so the game runs on WebGL2) the case is unsupported, not
  failed; a run where every case was unsupported exits 2, or 0 with
  `--allow-unsupported`. Chromium's headless shell can't present a WebGPU
  canvas and loses the device by itself before the runner does; the run
  says which (“device lost by the browser” or “by the runner”). The other
  cases hide WebGPU, so that loss can't stop the game before they do.

Every case also checks the crash report the engine hands the page
(`window.ponkoCrashReport`, the guide's [Crash reports](../../book/src/crash-reports.md#in-the-browser)):
kind `panic` for the panics and `error` for the lost context and the lost
device, with the same message as the failure, in its text too.

The callback is subscribed late, after the failure, so the run also checks
that a late subscriber gets it at once. `runtime-failures.test.mjs` checks
the template's handling and the runner's checks without a browser.

## WebKit audio

With the 2D fixture built, run these checks in headless WebKit:

```sh
env -u WEBKIT_GST_MAX_NUMBER_OF_AUDIO_OUTPUT_CHANNELS node web/smoke/smoke.mjs --fixture web_smoke --browser webkit --backend webgl2
env -u WEBKIT_GST_MAX_NUMBER_OF_AUDIO_OUTPUT_CHANNELS node web/smoke/smoke.mjs --fixture web_smoke --browser webkit --backend webgl2 --audio-output zero-channels
env -u WEBKIT_GST_MAX_NUMBER_OF_AUDIO_OUTPUT_CHANNELS node web/smoke/smoke.mjs --fixture web_smoke --browser webkit --backend webgl2 --audio-output resume-rejected
```

The first run uses the browser's actual channel report. The second forces a
zero report on the real destination and verifies that the engine schedules
stereo music after the click and logs the fallback once. The third makes
`AudioContext.resume()` reject and checks that the game reports audio as
unavailable, schedules no buffers, and logs the reason once.

## WebGPU first

`webgpu-fallback.mjs` checks that a web build draws with WebGPU where the
browser has a working one, and that each way WebGPU can fail before the
game draws ends on WebGL2, drawing, with one console line saying why
(the guide's [WebGPU first, WebGL2 as the
fallback](../../book/src/web.md#webgpu-first-webgl2-as-the-fallback)). Each
failure is faked in the page before the game's wasm starts:

| Case | Faked | Must end |
|---|---|---|
| `webgpu` | nothing | on WebGPU: a WebGPU canvas, the engine's backend `webgpu`, `ponko: drawing with WebGPU` and no fallback line |
| `no-webgpu` | no `navigator.gpu` | on WebGL2, the line saying `navigator.gpu is missing` |
| `no-adapter` | `requestAdapter()` resolves to `null` (a stand-in `navigator.gpu` where the browser has none) | on WebGL2, the line saying `requestAdapter() found no adapter` |
| `device-fails` | `GPUAdapter.requestDevice()` rejects | on WebGL2, the line with the browser's (here the runner's) words |
| `device-lost` | `requestDevice()` returns a device already destroyed | on WebGL2, the line saying the device was lost as it opened |
| `no-present` | every WebGPU canvas hands out an invalid frame texture (as the software fallback adapter of Chromium's headless shell on macOS does) | on WebGL2, the line saying a canvas can't show WebGPU frames because a test frame failed |

"On WebGL2" means: the game ticks, the canvas has a WebGL2 context and is not
blank, the engine reports backend `gl`, and there is no startup error, page
error or panic. `webgpu`, `device-fails`, `device-lost` and `no-present`
need a real WebGPU adapter; without one they are unsupported, not failed. `--repeat <n>`
(default 3) loads the `webgpu` and `no-webgpu` pages n times each and
reports the median time to the first frame from navigation start (the
fixture's first published state).

```sh
web/build.sh --features 3d web_smoke_3d
node web/smoke/webgpu-fallback.mjs --dir target/web/web_smoke_3d --chromium-mode new
node web/smoke/webgpu-fallback.mjs --case device-lost --chromium-mode new   # one case
```

`webgpu-fallback.test.mjs` checks its decisions without a browser.

## On a real GPU

`--require-gpu` makes a Chromium run (`--case ok`) a gate for the
machine's GPU instead of a report: the run fails (exit 1, kind `no-gpu`)
when the browser lacks the backend asked for, when its WebGPU adapter is a
fallback adapter or a software renderer (SwiftShader, llvmpipe, lavapipe,
...), when its WebGL2 renderer is one, or when the engine's own adapter
(`data-smoke-gpu`) is of the CPU type or names one. It also leaves out
`--enable-unsafe-swiftshader`, so the browser can't quietly fall back. A
WebGPU run that fell back to WebGL2 already fails as `backend`. It can't go
with `--allow-unsupported`. Use the new headless mode: the headless shell
has no GPU.

```sh
node web/smoke/smoke.mjs --fixture web_smoke_3d --chromium-mode new --backend webgpu --require-gpu
```

The check list's `web-gpu` job (macOS) runs it for the 2D and 3D fixtures
on WebGPU and the 3D one on WebGL2, then `frame-times.mjs` once.

## Frame times

`frame-times.mjs` times the 3D fixture's frames (`web/build.sh --features
3d web_smoke_3d` first, or `--build`) on WebGPU and WebGL2 in Chromium's
new headless mode, on a real GPU (it fails otherwise, as `--require-gpu`
does). A page script notes `performance.now()` each time the fixture
publishes a new frame count (once per engine frame), on a cross-origin
isolated page for a fine clock; each load skips `--warmup` frames (120)
and times the next `--frames` (1,000). It prints each load's median, 95th
and 99th percentile and slowest frame, the engine's own split from the
fixture's `frame_stats` (CPU in systems and drawing, GPU where the browser
times passes), the load average, and a summary over the `--repeat` loads
(3) per backend.

| Option | Default | |
|---|---|---|
| `--pacing uncapped\|display` | `uncapped` | `uncapped` turns off vsync and the frame-rate limit, so a frame time is the frame's cost; `display` keeps the browser's 60 Hz |
| `--backends webgpu,webgl2` | both | |
| `--frames <n>`, `--warmup <n>`, `--repeat <n>` | 1000, 120, 3 | |
| `--dir`, `--out`, `--timeout <s>` | `target/web/web_smoke_3d`, `target/web-smoke/frame-times-<pacing>`, 60 per load | |

```sh
node web/smoke/frame-times.mjs
node web/smoke/frame-times.mjs --pacing display --frames 600
```

The guide's [Frame times](../../book/src/web.md#frame-times) has the
numbers and what they mean. `frame-times.test.mjs` checks the arithmetic.

## Multiplayer

`multiplayer.mjs` checks [browser multiplayer](../../book/src/networking.md#in-the-browser):
the lockstep example (`examples/lockstep.rs`) with four players. It starts a
native host (`lockstep host 127.0.0.1:0 --players 4 --web 127.0.0.1:0
--headless 720`, a bot at its keys) and a native client joining it over UDP,
then opens two tabs of the web build in one headless browser, which join the
host over WebSocket (`?server=ws://...`). Each tab holds an arrow key. The
tabs publish the example's state in `<html data-smoke>`; the native peers
print theirs when their ticks are done. It passes when all four:

- play with 4 players, each with its own number;
- reach tick 300 and report the same fingerprint of the world there (every
  crab's position and push, bit for bit: wasm and native agree);
- saw every player steer before it, the same number of ticks: each tab's and
  each bot's input reached everyone;
- saw no desync (the session's own checksums) and didn't lose the session.

```sh
node web/smoke/multiplayer.mjs --build          # web/build.sh lockstep + cargo build --example lockstep
node web/smoke/multiplayer.mjs --dir target/web/lockstep --native target/debug/examples/lockstep
```

Options: `--browser`, `--ticks <n>` (the native peers' run, at least 400;
default 720), `--out <dir>` (default `target/web-smoke/multiplayer`: its
`report.json` has every peer's state and output) and `--run-timeout <s>`
(default 240). The page runs on WebGL2 (WebGPU hidden): the test is about
the network. `multiplayer.test.mjs` tests its checks without a browser.

## Replication

`replication.mjs` checks [replication in the browser](../../book/src/replication.md#dedicated-servers-and-browsers):
the reference game (`examples/net_arena`) as a web build joining a native
dedicated server. It starts `net_arena server 127.0.0.1:0 --web 127.0.0.1:0`
(no window; free ports, printed as `serving on ..., browsers on ws://...`),
then opens one headless tab of the web build with `?server=ws://...`. The tab
publishes in `<html data-smoke>` whether it joined, the replicas it holds and
how far each replicated orb moved since it first saw it. It passes when the
tab:

- joined (the server's declarations match) without a replication error;
- holds the server's orbs and its own player (spawned for it by the server,
  replicated back and marked as its own);
- saw an orb move at least 5 units: the server's changes reach the browser.

```sh
node web/smoke/replication.mjs --build          # web/build.sh net_arena + cargo build --example net_arena
node web/smoke/replication.mjs --dir target/web/net_arena --native target/debug/examples/net_arena
```

Options: `--browser`, `--out <dir>` (default `target/web-smoke/replication`:
its `report.json` has the tab's last state, its console and the server's
output) and `--run-timeout <s>` (default 120; a run takes a few seconds once
built). The page runs on WebGL2 (WebGPU hidden): the test is about the
network. `replication.test.mjs` tests its checks without a browser. Checked
so far in Chromium 159 (headless shell) on an Apple Silicon Mac.

## The web gallery

`gallery.mjs` checks the [web gallery](../gallery/README.md) the same way,
for pages whose examples publish no state: every page must start its game,
keep drawing, survive a click and show a canvas that isn't blank, or, for a
demo that needs WebGPU on a page without it, say so instead of starting. The
gallery's README has how to run it; `gallery-lib.test.mjs` tests its
decisions.

## Threads

`threads.mjs` checks a web build with threads (the guide's "Threads on the
web"): the fixture `examples/web_threads.rs`, built with
`web/build.sh --threads --features 3d,physics3d,bench-internals web_threads`
(`--build` builds it first). It serves
the build twice and opens each in a new browser context:

- **With** the cross-origin isolation headers
  (`Cross-Origin-Opener-Policy: same-origin`,
  `Cross-Origin-Embedder-Policy: require-corp`):
  - the engine must start two or more workers;
  - a parallel query, a batch of four view systems (threshold zero), two
    jobs and an asset group (a texture and a `with` file from `assets/`)
    must each run on two or more of them, and never on the page's thread;
  - at least two of the engine's own parallel passes must go through the
    frame pool's runner (`passes_shared`), among them the retained 3D
    culling of 20,000 models (`three/shared_cull`, which must also find
    models to draw), the drawing of 256 2D skeletons
    (`skeleton2d/shared_draw`), the moves of 32 3D characters
    (`physics3d/shared_characters`) and a small light bake
    (`three/shared_bake`); the JSON save of the game's world must not
    (`reflect/shared_save`: in the browser it is faster on the page's
    thread);
  - every timed pass must have a time (`pass_ms`).
- **Without** the headers: everything runs on the page's thread, and the
  console says why (`it runs on the page's thread alone`).

Both runs must reach the same checksum at tick 120 (the 3D frame's counts,
the skeletons' vertices, the characters' places, the bake's hash and the
JSON save's length and hash are in it; the frame and the save are compared
on their own too) and read the same file. The run prints a table of the
timed passes: whether each was shared out, and its median time with threads
and on the page's thread alone (the guide's list of parallel loops comes
from it).
More modes:
- `--case worker-panic` panics inside the parallel query on a worker. The
  console must say `the game stops`, the page must still answer, and the
  ticks must stop.
- `--bench` times the fixture's workloads (`par`, `sweep`, `interleaved`,
  `jobs`: those of `crates/ponko_bench`; `boids`: one tick of
  `examples/boids.rs`, 20,000 boids) with `?threads=1` and with the
  default, three page loads each (`--repeat`). It prints the medians, their
  spread, and each speed-up, saying whether it is beyond the noise.
  `--dir <an ordinary build> --threads 1` times a build without threads.
- `--case gamepads` replaces `navigator.getGamepads()` on the isolated page
  with fed pads (an Xbox pad by its Chromium `id`), and steps them: no pad,
  a pad connects, South, d-pad right, a quarter-pulled left trigger and both
  sticks, everything let go, the pad unplugged. After each step the fixture's
  `pads` must show what the engine saw: the pad in slot 0 with its name,
  family and USB ids, exactly those buttons down, the axes (stick y up stays
  negative), one South press, then no pad. The pads have a fake
  `vibrationActuator` that records its calls (and refuses an effect over
  the browser's 5 s cap, as Chromium does); pressing West makes the fixture
  rumble the pad for 2.5 s, and the calls must be `dual-rumble` effects at
  the asked levels, played again before the cap with no gap, and a `reset`
  at the end. Then two pads, the first with an actuator whose `effects`
  list `trigger-rumble`, the second with a `dual-rumble`-only one (which
  refuses `trigger-rumble`): the fixture must report `trigger_rumble` true
  and false, and North on both (a rumble of the weak motor and both
  triggers for 1.5 s) must reach the first as `trigger-rumble` effects
  with all four levels and the second as `dual-rumble` ones with the
  trigger levels dropped, as on the desktop, each stopped at the end.
  Real rumble can't be felt in a headless browser: these calls are the
  proof.
- `--case gamepads-plain` is the same case on the ordinary build of the
  fixture (`--build` makes it, in `target/web/web_threads_plain`), whose pads
  come from the same backend built without threads (it replaced gilrs on
  the web; the guide's web chapter says why). Both apply gilrs's dead zone
  (0.1, rescaled), and the expected axes take it out the same way.
- `--case service-worker` builds the fixture with `ponko-isolate.js`
  (`web/build.sh --threads --isolation-worker`, in
  `target/web/web_threads_sw`) and serves it **without** the headers. The
  page must reload once through the service worker (`ponko-isolate.js:
  reloading once` in the console), be controlled by it, be cross-origin
  isolated, and pass every check of the isolated run above.
- `--case warning` needs no browser: it builds the example
  `examples/web_threads_warning.rs` with threads and `-D warnings`. The
  example's only code is an unused function that only a build with threads
  compiles, so the build must fail on it, as an error from `-D warnings`.
  If it passes, the threaded build lost `-D warnings` (or its atomics).

`web/build.sh --threads --clippy` runs the nightly clippy on the threaded
configuration (the engine and the fixture, same toolchain and flags), which
has lints the stable clippy lacks; the check list runs it.

`--build` (and `--case warning`) add `-D warnings` to the build's flags, so a
warning in the threaded build fails the check. `web/build.sh --threads` keeps
the caller's flags: cargo reads only the first of `CARGO_ENCODED_RUSTFLAGS`,
`RUSTFLAGS` and the target's flags that is set, and the thread flags go into
that one (`build.test.mjs` checks this with `--print-env`).

```sh
node web/smoke/threads.mjs --build
node web/smoke/threads.mjs --case worker-panic
node web/smoke/threads.mjs --case gamepads
node web/smoke/threads.mjs --build --case gamepads-plain
node web/smoke/threads.mjs --case warning
node web/smoke/threads.mjs --bench --workloads par,sweep --repeat 3
```

Exit codes and `report.json` (in `--out`, default
`target/web-smoke/threads`) are as for `smoke.mjs`. `threads.test.mjs`
tests its checks and its summary without a browser. Checked so far in
Chromium 159 (headless shell) on an Apple Silicon Mac; Firefox and WebKit
are not tried yet.

## Spectrum

`spectrum.mjs` times the water kit's CPU wave spectrum in the browser (the
water guide's "Waves from a spectrum"): the fixture
`examples/web_spectrum.rs`, in three builds (`--build` makes them):
`scalar` (`web/build.sh --features 3d web_spectrum`), `simd128`
(`-C target-feature=+simd128` for the whole module, added to the rust
flags cargo reads) and `simd` (`web/build.sh --simd`: that, and the
`wasm-simd` feature: rustfft's SIMD FFT; the feature without the flag
doesn't compile). Each page load times one new moment of the field
(`Waves::height_at` at a new time, what a WebGL2 upload costs) and one with
the sway's speed (`Waves::sample`, what buoyancy costs) at 64, 128 and 256
cells, eight moments per reading because the browser's clock is coarse.
The builds take turns, three loads each (`--repeat`). The check is that each
build says it was built as asked and timed every size; the times are
printed (median of the loads' medians, their range, each SIMD build's
speed-up over `scalar` and whether it is beyond the noise) and kept in
`report.json`, never passed or failed on.

```sh
node web/smoke/spectrum.mjs --build
node web/smoke/spectrum.mjs --variants scalar,simd --repeat 5
```

## Video

`video.mjs` checks that videos play in the browser build
(`examples/web_video.rs`, the video guide's "In a browser"). The fixture
plays the engine's test clip (`tests/fixtures/video/clip.webm`, whose
frames carry their number in a gray quarter) through `ponko::video::Video`,
which in the browser takes its frames from the browser's own AV1 decoder
(WebCodecs), and reports what it saw; the runner checks
(`video-lib.mjs`, `checkVideo`):

- every frame shown is the one `frame_time` names (read from its pixels),
  with the red, green and blue quarters in place;
- the frames come in order, at least 12 of the 20, ending on the last;
- the picture is at most two frames behind the video's clock (the browser
  hands decoded frames back later);
- paused, seeking to 1.3 s and back to 0.4 s shows frames 13 and 4 within
  60 updates each.

```sh
node web/smoke/video.mjs --build      # web/build.sh --features video web_video, then run
```

It runs on WebGL2, like the other Chromium runs. A browser without
WebCodecs AV1 (`VideoDecoder.isConfigSupported`) exits 2, unsupported.


## C and C++

`cpp.mjs` runs a C++ game in the browser (the web guide's "C and C++"): the
getting-started game of the guide, the `first_game_full` snippet of
`examples/cpp/guide.cpp` exactly as the guide shows it, with a `main` that
plays it. `--build` writes that program to `target/web-cpp-src/` and builds
it with `web/build-cpp.sh` (which needs the WASI SDK, in `$WASI_SDK_PATH`).
In headless Chromium, on WebGL2, the game must start (the page's status line goes away
with no page error), fetch `crab.png`, draw something, and move its crab
right while ArrowRight is held. That last step checks that the C++ systems
keep running every frame and that `pos`, a local of the game's function
its system captures by reference, outlives the call to `run()`, which in a
browser doesn't return. `report.json` has the module's size (and gzipped)
and the startup time, from navigation to the game's first frame.

```sh
WASI_SDK_PATH=/opt/wasi-sdk node web/smoke/cpp.mjs --build
```

## Running it

```sh
cd web/smoke && npm ci
# One browser's download is 100 to 200 MB; here inside this folder:
PLAYWRIGHT_BROWSERS_PATH=$PWD/.browsers node node_modules/playwright-core/cli.js install --only-shell chromium
cd ../..
PLAYWRIGHT_BROWSERS_PATH=$PWD/web/smoke/.browsers \
  node web/smoke/smoke.mjs --fixture web_smoke --build --backend webgl2
```

`--build` runs `web/build.sh` for the fixture (with its Cargo features) into
`target/web/<example>`; without it, build that folder yourself (for the 3D
fixture, `web/build.sh --features 3d web_smoke_3d`), or point `--dir` at any
built site. For the other browsers install `firefox` or `webkit` (and
`chromium`, not `--only-shell`, for `--chromium-mode new`), then pass
`--browser firefox`.

Without `PLAYWRIGHT_BROWSERS_PATH`, Playwright uses its usual cache folder.
Remove `web/smoke/.browsers` and `web/smoke/node_modules` when done; both are
git-ignored. The wasm build is a full extra copy of the dependencies:
`rm -rf target/wasm32-unknown-unknown target/web` afterwards.

`node --test web/smoke/*.test.mjs` runs the runner's own tests (no browser or build).

Options:

| Option | Meaning |
|---|---|
| `--fixture <name>` | A fixture of `fixtures.json`: its example, features, asset, key presses and minimums are the defaults (`--build` builds it first). |
| `--dir <site>` | The built site, instead of `target/web/<example>`. |
| `--browser chromium` / `firefox` / `webkit` | The browser (default `chromium`). |
| `--chromium-mode shell` / `new` | Chromium's headless shell (default) or the full browser in its new headless mode, which has the compositor a WebGPU canvas is presented through. |
| `--backend webgl2` | Hide WebGPU from the page, so the engine takes its WebGL2 fallback. |
| `--backend webgpu` | Require WebGPU; "unsupported" if the browser has no adapter or can't show a WebGPU canvas. |
| `--backend auto` | Let the engine choose; the report says which it used. |
| `--case missing-asset` / `panic` / `stall` | Self-tests: answer 404 for the fixture's asset, panic during setup (`?smoke_case=panic`), or pause the simulation (`?smoke_case=stall`). The run passes only if that failure is detected. |
| `--timeout <s>` | Seconds to wait for the first frame (default 90). |
| `--run-timeout <s>` | The whole run's deadline (default 300): past it the run fails as `browser-hang`, naming the step it was on, and exits (which kills the browser). |
| `--allow-unsupported` | Exit 0 instead of 2 when the browser lacks the backend (the report and the last line still say UNSUPPORTED). |
| `--asset <file>` / `--asset-size <W>x<H>` / `none` | Without a fixture: the file under `assets/` that must be fetched (default `crab.png`), and the size it must decode to. |
| `--asset-source <W>x<H>` / `none` | The size the image from the game's in-memory asset source must decode to (the state's `source`); its streamed music must load too, and nothing under `assets/embedded/` may be fetched. Default `4x2` without a fixture. |
| `--press <Key>=<field>:<value>` | After the click, press the key (Playwright key name: `Enter`, `Escape`, `Q`) and wait up to 10 s for the reported state's `<field>` to become `<value>`. Repeatable, in order. |
| `--min <field>=<n>` | The reported state's numeric `<field>` must be at least `n`. Repeatable. |
| `--after-reload <field>:<value>` | After the presses, reload the page (once the state's `saving` is not `true`); the new page's `<field>` must then be `<value>`. Repeatable: each adds a field to the last reload step. |
| `--reload` | Adds another reload step: the page is reloaded again after the one before, and the `--after-reload` fields that follow are checked on that page load. Repeatable. |
| `--engine-gpu required` | Fail when the fixture publishes no engine GPU report (fixtures set this). |
| `--music required` / `none` | Check that music started at setup waits for the first click and is heard after it (the fixture reports `audio_unlocked`). `required` for fixtures with `"music": true`. |
| `--no-trace` / `--chrome-trace` | Skip the Playwright trace, or also write Chromium's GPU trace (it slows a heavy page a lot, enough to trip the stall check on a 3D scene: use it on the 2D fixture, or to look at one run). |
| `--out <dir>` | Where artifacts go (default `target/web-smoke/<fixture>-<browser>-<backend>-<case>`). |
| `--browser-arg <flag>` | Extra browser flag (repeatable). |

## Results

Exit codes: **0** passed, **1** the game failed (or an installed browser would
not start), **2** unsupported (the browser isn't installed, or lacks the
backend asked for), **3** bad arguments or no build. "Unsupported" is decided
without the engine: WebGPU counts as available only if a plain WebGPU canvas,
cleared to red by the runner itself and read back in the same task, is red.
It is never a pass without a reason: the last line, `report.json` and the
summary table all say `UNSUPPORTED` and why (no `navigator.gpu`, no adapter,
no canvas), even under `--allow-unsupported`.

A browser that stops answering can't hang a run: once the page is open,
every browser call (reading the state, a click, a key, a screenshot) has a
30 s deadline, a page whose content process crashes fails at once
(`browser-crash`), and the whole run has `--run-timeout`. A hang fails as
`browser-hang` with the step it was on and the last lines of `browser.log`.
A hung run is not retried: so far every hang has been a real, repeatable
failure, and a retry would hide the next one. The list of checks also gives
each browser step a `timeout-minutes` of a few minutes.

WebKit on Linux needs a sound server and a channel count. It opens its audio
output through GStreamer and reads an AudioContext's output channels
(`destination.maxChannelCount`) from GStreamer's list of audio devices.
Without a sound server its content process crashes when a page asks, which
the engine does when it opens its audio at a game's first click (the log
shows GStreamer's PulseAudio device provider failing to connect, then
"double free or corruption"). With one, the list can still come back empty,
the count is 0, and the engine's audio output refuses to open (the report's
`audio` shows a running context with 0 output channels and 0 buffers). The
listed check starts PulseAudio with a null sink and sets
`WEBKIT_GST_MAX_NUMBER_OF_AUDIO_OUTPUT_CHANNELS=2` for the WebKit runs; do
the same on a headless Linux machine
(`pulseaudio --daemonize=yes --exit-idle-time=-1`, then
`pactl load-module module-null-sink`).

Each run writes to its `--out` folder:

- `report.json`: browser version, platform, launch options, the WebGPU adapter
  and WebGL2 renderer the browser offers, the backend the game's canvas used,
  **the engine's own report** (`engine.gpu`: adapter, limits and every renderer
  feature with its fallback; `engine.summary`), startup time, total elapsed
  time, the state before and after the progress check, canvas statistics, every
  request with its status, console messages, page errors and the outcome
  (`kind`: `pass`, `asset`, `panic`, `page-error`, `stall`, `startup-timeout`,
  `input`, `state`, `blank`, `backend`, `launch`, `unsupported`,
  `browser-hang`, `browser-crash`, ...) with the `step` the run was on.
- `console.log`: the page's console.
- `browser.log`: the browser's own output (its process's stdout and stderr,
  launch and exit, through Playwright's `pw:browser` channel). A browser that
  hangs or crashes often says why only here.
- `trace.zip`: Playwright's trace (actions, requests, screenshots over time);
  open it with `node node_modules/playwright-core/cli.js show-trace trace.zip`.
- `chrome-trace.json` (`--chrome-trace`, Chromium): the GPU process and
  compositor frames, for chrome://tracing or Perfetto.
- `canvas.png`: the canvas after the click; `page.png`: the page on failure.

`node web/smoke/summary.mjs target/web-smoke` prints every run's report as a
table.

## What runs where

| Where | Browser | WebGL2 (forced) | WebGPU |
|---|---|---|---|
| The list of checks (`web-smoke` job, Ubuntu, no GPU) | Chromium (headless shell and full browser), Firefox, WebKit from Playwright | Must pass in Chromium and WebKit, for the 2D and 3D fixtures (software GL); `coin_dash` and the multiplayer check (`multiplayer.mjs`) in Chromium. Firefox is tried and reported: headless Firefox has no WebGL2 on that machine, so it reads "unsupported" | Tried; passes as "unsupported", with the reason, where the browser can't show a WebGPU canvas; where it can, the game must draw through it |
| The list of checks (`web-smoke-runner`) | none | the runner's own tests only (and the gallery's) | |
| The list of checks (`web-gpu` job, a Mac with its GPU) | Chromium new headless mode | Must pass on the GPU for the 3D fixture (`--require-gpu`: ANGLE on Metal, no software renderer) | **Must pass** on the GPU for the 2D and 3D fixtures (`--require-gpu`); unsupported, a fallback to WebGL2 or a software adapter fails; then `frame-times.mjs` once per backend |
| A release (the gallery check, Ubuntu, no GPU) | Chromium headless shell | Every gallery page must pass (software GL) | Not tried; the demos that need WebGPU must show their message |
| Apple Silicon Mac, the gallery (`gallery.mjs`) | Chromium 159 | Every page passes in the headless shell; the 2 demos that need WebGPU (compute passes) show their message | Every page passes in new headless mode on the Metal adapter |
| Apple Silicon Mac, Chromium new headless mode | Chromium 159 | Passes (ANGLE on Metal) | **Passes** on the Metal adapter: the engine reports `webgpu`, the canvas draws |
| Apple Silicon Mac, `webgpu-fallback.mjs` (3D fixture) | Chromium 159 new headless mode | Each of the five faked failures ends on WebGL2 and draws the scene, with its one console line | Passes on the Metal adapter and on SwiftShader (`--use-webgpu-adapter=swiftshader`): the same scene, `ponko: drawing with WebGPU` |
| Apple Silicon Mac, Chromium headless shell | same | Passes (ANGLE on SwiftShader) | Unsupported: a SwiftShader fallback adapter exists, but a WebGPU canvas can't be shown. With `--backend auto` the engine's test frame fails and the game draws with WebGL2 |
| Apple Silicon Mac, Chromium new headless mode, SwiftShader (`--use-webgpu-adapter=swiftshader`) | same | | The 2D fixture passes. The 3D fixture fails as a `stall`: on a busy machine the software adapter's first 3D frames block the page for 5 to 20 s, then it runs at about 3 frames a second (`book/src/check-results/macos.md`, 2026-11-26) |
| Apple Silicon Mac, `multiplayer.mjs` | Chromium 159 headless shell | Passes: two tabs, a native host and a native UDP client agree on the world at tick 300 | Not tried (WebGPU is hidden) |
| Apple Silicon Mac, WebKit (Playwright build) | WebKit 27.2 | Passes | Passes on its own adapter |
| Apple Silicon Mac, Firefox (Playwright build) | Firefox 159.0 (`firefox-1572`, playwright-core 1.65.1) | Passes for the 2D and 3D fixtures (Apple GPU through WebGL2); `--backend auto` passes on WebGL2 | Unsupported: headless Firefox has no WebGPU adapter there |
| Apple Silicon Mac, `cpp.mjs` | Chromium 159 headless shell | Passes: the guide's C++ game starts in 0.8 s (a busy machine), draws its crab and moves it 200 px in the second ArrowRight is held | Not tried (WebGPU is hidden: the headless shell can't present it) |
| Apple Silicon Mac, `threads.mjs` | Chromium 159 headless shell | Passes on whichever backend the page picks (the check is about threads, not drawing): workers on the isolated page, one thread without the headers, the same world; a worker's panic stops the game | Not tried on its own |

Not covered yet: Firefox on a GPU-less Linux machine (no WebGL2), WebGPU on
a GPU-less Linux machine (the `web-smoke` job tries it and reports), and
mobile browsers.

## First-load cost

`first-load.mjs` measures what a web build costs to start rather than
whether it works: the requests and bytes before the first frame, the asset
phase (first asset request to last asset byte) and the time to the first
frame, the median of `--runs` fresh loads in headless Chromium, served with
range requests and the cache off, on a network Chromium emulates (`--latency
<ms>`, `--mbps <Mbit/s>`; both `0` for none). Give it several `--dir`s to
compare, such as a package made with and without `--no-pack`:

```sh
node web/smoke/first-load.mjs --dir target/web-loose/ember-hop-0.1.0-web \
     --dir target/web-pack/ember-hop-0.1.0-web --latency 40 --mbps 20
```

The page must publish `<html data-smoke>` (the reference games and the
fixtures do). The guide's packaging chapter has the numbers for Ember Hop.

## Key labels

`key-labels.mjs` checks that prompts in a browser show what is printed on
the player's keys (the guide's [Key labels follow the keyboard
layout](../../book/src/controllers.md#key-labels-follow-the-keyboard-layout)).
The 2D fixture reports WASD as labelled for the player (`key_labels` in its
state, `"W/A/S/D"` on a US keyboard). The runner gives the page a French
(AZERTY) keyboard three ways, before the game starts:

- `layout-map`: `navigator.keyboard.getLayoutMap()` answers with a French
  map: the game shows `"Z/Q/S/D"` without a key pressed.
- `no-layout-map`: the page has no `navigator.keyboard`, as in Firefox,
  Safari and any page outside a secure context: `"W/A/S/D"`. In Chromium
  the runner then presses the key at W's place typing `z` (through the
  DevTools protocol, which can send a key whose character differs from its
  place), and the label is learned from it: `"Z/A/S/D"`.
- `refused`: `getLayoutMap()` rejects (a frame that may not ask): US labels,
  and the game keeps running.

```sh
web/build.sh web_smoke
node web/smoke/key-labels.mjs                          # all three, Chromium
node web/smoke/key-labels.mjs --browser webkit         # no press step there
node web/smoke/key-labels.mjs --case layout-map
```

Run on an Apple Silicon Mac: Chromium (headless shell) and WebKit pass all
three; Playwright's Firefox does not start on that machine (see above).
