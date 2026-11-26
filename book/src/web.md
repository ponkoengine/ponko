# Web builds: games in the browser

> **Maturity: [tested](maturity.md).** Game: Coin Dash in headless Chromium
> and WebKit; Ember Hop builds for the web. Tests: the browser smoke runs
> (`web/smoke`). Platforms: the web, through headless Chromium and WebKit.
> Graphics: WebGL2 in both browsers; WebGPU and WebGL2 on a real GPU in the
> local check run's `web-gpu` job (headless Chromium on macOS, Metal).
> Missing: WebGPU on Windows and Linux GPUs in a recorded run, and Firefox
> (its headless build has no WebGL2).

The same game code runs in a browser tab. **Try it:** the examples build
into a gallery (below), with a page for each: what it shows, its controls,
whether it needs WebGPU and its source at that release. Among them:

- The battle: two armies of
  10,000 units on flow fields. Click to rally, `A` to attack, `D` to defend.
  Add `?inspector=open` to the address to open the [inspector](inspector.md).
- Flow fields: 20,000 agents
  find their way to where you click.
- A 3D scene: materials, a sun with
  shadows and instanced models.

Games draw with WebGPU where the browser has a working one, and WebGL2
where it doesn't ([WebGPU first, WebGL2 as the
fallback](#webgpu-first-webgl2-as-the-fallback)). Running the game requires
at least one usable graphics backend.

## Building your game

Once:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130   # the wasm-bindgen version in your Cargo.lock
```

Then, from your game's folder, with
[`web/build.sh`](../../web/build.sh)
and [`web/index.html`](../../web/index.html)
copied from the engine's repository:

```sh
web/build.sh --bin my_game               # -> target/web/my_game/
python3 -m http.server -d target/web/my_game 8000
```

and open <http://localhost:8000>. In the engine's repository,
`web/build.sh battle` builds an example. `--simd` builds with WASM SIMD (a
faster CPU wave spectrum for the water; see the water guide's "On the
web"), for browsers that have it.

The output folder is a static site: put it on any web host (GitHub Pages,
itch.io, your own server). It holds the page, the game (`game_bg.wasm`) and
a copy of `assets/`.

For the version players download, build with the templates' size profile:
`web/build.sh --profile dist-web --bin my_game` (or `ponko package --web
--profile dist-web`). For the 2D template, `game_bg.wasm` is 15.1 MB in a
release build (3.4 MB as hosts send it, brotli-compressed) and 10.2 MB with
`dist-web` (2.8 MB), at about a minute and a half of build time instead of 39 s; [Build
times and download sizes](performance.md#download-sizes) has the
measurements. A project made by hand adds the profile to its `Cargo.toml`:

```toml
[profile.dist-web]
inherits = "release"
lto = "fat"
codegen-units = 1
opt-level = "s"
```

## The gallery of examples

`web/gallery/` in the engine's repository builds the gallery: every example
listed in `web/gallery/examples.json`, built with `web/build.sh`, with a page
each and an index page, as one static site:

```sh
node web/gallery/build.mjs               # -> target/web/gallery/
python3 -m http.server -d target/web/gallery 8000
```

A demo marked `"webgpu": "required"` there needs WebGPU; on a browser without
it, its page says so and what to try instead of starting it. The others run
with WebGPU or, where the browser lacks it, WebGL2. Each release builds the
gallery for its version and publishes it (CONTRIBUTING.md, "Releasing"). Adding an example is one line in
`examples.json`; the gallery's
[README](../../web/gallery/README.md)
has the fields, and how to check every page in a headless browser.

## WebGPU first, WebGL2 as the fallback

One build carries both: the same `game_bg.wasm` draws with the browser's
WebGPU where it works and with WebGL2 where it doesn't, with nothing to
choose in the game or the page. On startup the engine:

1. looks for `navigator.gpu`;
2. asks it for an adapter (`requestAdapter()`), then the adapter for a
   device;
3. clears and presents one test frame on a small canvas of its own (an
   `OffscreenCanvas`, never the game's), inside error scopes: some
   browsers offer an adapter that opens devices but can't show a frame,
   such as the software fallback adapter of Chromium's headless shell on
   macOS;
4. waits for that frame's round trip to the GPU, to see that the browser
   didn't lose the device at once (its GPU process gone, the driver
   refusing) and that the frame drew without an error;
5. only then gives the game's canvas its WebGPU context.

If any step fails, the canvas is still untouched, so the engine starts over
on WebGL2 and logs one line in the browser's console saying why, such as
`ponko: falling back to WebGL2: navigator.gpu.requestAdapter() found no
adapter` (or `... this browser has no WebGPU`, `... the WebGPU adapter could
not open a device: <the browser's words>`, `... the WebGPU device was lost
as it opened: <the browser's words>`, `... a canvas can't show WebGPU
frames: a test frame failed: <the browser's words>`). A WebGPU start logs
`ponko: drawing with WebGPU`. Only when WebGL2 fails too does the page show a startup error
(next sections). A WebGPU device lost later, while the game runs, is a
[failure while playing](#failures-while-playing): a game can't switch
backends mid-game.

The game finds out which it got from `ctx.gfx.gpu_capabilities()`:
`adapter.backend` is `webgpu` or `gl`, and the report's `tier` and
per-feature entries say what is drawn (see [GPU tiers](3d.md#gpu-tiers)).
`ctx.scene3d.set_recommended_quality(caps)` picks settings to match.

What each draws, at the least each gives: WebGPU at WebGPU's default limits
and without optional features (a browser often gives more, decals and global
illumination included where its adapter has the texture slots; the report
is the exact answer), WebGL2 at the `webgl2` tier's floor, with the one
sample per pixel a browser's WebGL2 draws the 3D frame with. The table is
made by the engine from the same reports (`src/render/capabilities.rs`,
test `the_guides_web_backend_table_is_generated_from_the_tier_code`), which
fails when it is out of date; `PONKO_UPDATE_BOOK=1 cargo test -p ponko
--features 3d --lib web_backend_table` rewrites it.

<!-- web-backend-table:start -->
| | WebGPU | WebGL2 (the fallback) |
|---|---|---|
| `GpuTier` | `baseline` | `webgl2` |
| largest 2D texture | 8192² | 2048² |
| sampled textures per shader stage | 16 | 16 |
| storage buffers per shader stage | 8 | 0 |
| compute workgroups per dimension | 65535 | 0 |
| `timestamps` | no, no GPU pass times; RenderStats::gpu_ms is None | no, no GPU pass times; RenderStats::gpu_ms is None |
| `compute` | yes | no, compute passes and the effects that need them are skipped (see the other entries) |
| `storage_buffers` | yes | no, GPU buffers are not created and compute passes using them do nothing; material shaders reading `items` and GPU-buffer instance setup report an error with a fix to use CPU-backed instance batches or model entities |
| `compute_culling` | yes | no, instance batches are culled on the CPU, cell by cell, and drawn with direct draws (the same picture); crowds are culled on the CPU and drawn from frames posed when they are made |
| `gpu_skinning` | yes | no, skinned and morphed meshes are posed on the CPU |
| `occlusion_culling` | yes | no, off: hidden models are drawn |
| `temporal_antialiasing` | yes | no, FXAA instead of TAA |
| `ambient_occlusion` | yes | no, off |
| `contact_shadows` | yes | no, off |
| `screen_space_reflections` | yes | no, off |
| `global_illumination` | no, off: sky and ambient light only | no, off: sky and ambient light only |
| `decals` | no, decals are not drawn | no, decals are not drawn |
| `multisampling` | yes | no, no multisampling: edges are smoothed only by FXAA or SMAA, if chosen |
<!-- web-backend-table:end -->

**Size and startup.** One `.wasm` holds both backends: wgpu's WebGPU binding,
and its WebGL2 backend with the shader translator, which the WebGL2 path
needs and the WebGPU path never runs. Measured on the smoke fixtures
(`web/build.sh`, release, wasm-bindgen 0.2.130; bytes of `game_bg.wasm`):

| Build | Raw | gzip -9 | brotli -q 11 |
|---|---|---|---|
| 2D fixture (`web_smoke`), both backends | 15,144,997 | 5,252,906 | 3,378,309 |
| 2D fixture, WebGPU only (wgpu without `webgl`, an experiment) | 13,181,687 | 4,596,210 | 2,970,584 |
| 3D fixture (`web_smoke_3d`, `--features 3d`), both backends | 19,176,434 | 6,487,015 | 4,129,117 |
| 3D fixture, WebGPU only (the same experiment) | 17,188,373 | 5,818,912 | 3,724,727 |

So the fallback costs about 2 MB of the raw module and about 0.4 MB
(11 to 14 %) of what a server sends brotli-compressed; the engine has no
switch to leave it out, since every browser without a working WebGPU would
then show nothing.

Time to the first frame (from the page's navigation start to the 3D
fixture's first published state; median of five fresh loads each, served
from the same machine) in Chromium 156's new headless mode on an Apple
M4 Max, idle apart from the fixture's own build just before (2026-11-22,
revision `9235ea05`, two runs of `webgpu-fallback.mjs --repeat 5`):
**WebGPU 108 to 109 ms** (106 to 114), **WebGL2 230 to 231 ms** (229 to
245, after a first load of 1,070 ms in the first run, the browser's
first start). The pages that try WebGPU first and fall back (no adapter,
a refused device, a device lost as it opens) took 228 to 234 ms, one load
each: the WebGPU attempt adds nothing measurable to the WebGL2 start.
On a busy machine (load average about 100) the same runs gave 200 and
298 ms, and 248 and 370 ms.

The test frame (step 3) costs little: from the device opening to the
game's canvas getting its context, the 3D fixture took a median of 11 ms
before it and 11 to 12 ms with it on the Metal adapter, and 10 to 11 ms
before and 16 ms with it on SwiftShader, Chromium's software adapter
(Chromium 156 new headless mode, Apple M4 Max, 2026-11-26, two runs of
seven fresh loads each, on a busy machine at load average 55 to 65). A
fresh browser's first page load took 2 to 17 ms more than before. Times to the first
frame on that busy machine moved by more between loads of the same build
than the test frame costs.

`web/smoke/webgpu-fallback.mjs` checks each way into WebGL2 in a headless
browser, faking the failure in the page before the game starts (see the
[runner's README](../../web/smoke/README.md#webgpu-first)).

### Frame times

The 3D smoke fixture (`web_smoke_3d`: a glTF house, seven spheres with
different materials, one with a normal map, a ground plane, a sun with
shadows; about 4,100 triangles at 800 × 600) on each backend, in
Chromium 156's new headless mode on an Apple M4 Max, on its GPU through
Metal (WebGL2 through ANGLE's Metal renderer), measured by
`web/smoke/frame-times.mjs` on 2026-11-26 (revision `9cbc953f`, release
build). The machine was busy: other builds ran throughout, with a load
average of 34 to 46 on its 14 cores.

**Method.** A page script notes `performance.now()` each time the fixture
publishes a new frame count (once per engine frame), on a cross-origin
isolated page, where that clock has a resolution of a few microseconds. A
frame time is the gap from one engine frame to the next. Each page load
waits 120 frames, then times the next 1,000 (600 at the display's rate);
three loads per backend, one browser. *Uncapped* turns Chromium's vsync
and frame-rate limit off (`--disable-gpu-vsync --disable-frame-rate-limit`),
so a frame starts as soon as the last is done: its time is the frame's
cost. *At the display's rate* keeps the browser's pacing (60 Hz headless).
The engine's own split (`ctx.perf.frame_stats()`, published by the fixture)
is read at the end of each load.

| Backend | Pacing | Median frame (ms) | p95 frame (ms) | Engine: systems / drawing (CPU, ms) | GPU (ms) |
|---|---|---|---|---|---|
| WebGPU | uncapped | 0.19 (0.19 to 0.20) | 0.66 (0.52 to 0.76) | 0.02 to 0.03 / 0.12 to 0.16 | 0.20 to 0.23 |
| WebGL2 | uncapped | 1.19 (1.19 to 1.23) | 3.27 (3.25 to 3.28) | 0.05 / 1.44 to 1.47 | not timed |
| WebGPU | 60 Hz | 16.67 (16.66 to 16.67) | 18.11 (17.87 to 18.17) | 0.10 / 0.33 | 0.97 to 3.16 |
| WebGL2 | 60 Hz | 16.66 (16.66 to 16.67) | 18.12 (17.99 to 18.13) | 0.10 / 0.56 to 0.60 | not timed |

The first number is the median over the three loads, the range the
lowest and highest load. What they say:

- Both backends hold 60 Hz with a wide margin: at the display's rate the
  median frame is 16.7 ms and the slowest of 3,600 is 18.8 ms, the
  browser's own pacing jitter.
- Uncapped, a WebGL2 frame costs about six times a WebGPU frame, and
  nearly all of it is the engine's drawing on the CPU (1.45 ms against
  0.14): WebGL2 is many small GL calls from WebAssembly into the browser,
  and the work of translating them, where WebGPU records a few command
  buffers. The scene is small, so this is the per-frame overhead of each
  path more than the cost of drawing; a heavier scene adds GPU time on
  both.
- WebGPU's GPU time comes from timestamp queries (the browser exposes
  them; WebGL2 can't time the GPU). Paced at 60 Hz it reads higher (about
  1 ms, once 3 ms) than uncapped (0.2 ms): the GPU clocks down between
  frames when it has little to do, so this is not extra work.

`node web/smoke/frame-times.mjs` repeats the measurement (`--pacing display`
for the paced rows; the [runner's
README](../../web/smoke/README.md#frame-times) has the options); the local
check run's `web-gpu` job runs it once per backend. Record it again for each
release, with the machine's load. Firefox and Safari are not measured here.

## What's different in a browser

**Assets are fetched before setup.** There's no file system, so the build
script lists your `assets/` folder in `assets/index.json`, and the engine
downloads every file in it before running your setup. A web package from
`ponko package --web` ships them in one `assets.pack` instead, named by the
page's canvas (`data-pack="assets.pack"`), which the engine fetches with at
most two range requests ([Assets in one
pack](packaging.md#assets-in-one-pack-ponko-pack)). `load_texture` and
the other loading functions then work as usual.

**One thread, unless you build with threads.** Browsers run WebAssembly on
several threads only on pages served with two particular headers, so an
ordinary web build uses one. Parallel queries (`par_for_each`) still work,
just on a single core: the battle's 20,000 units take about 4.5 ms per tick
in Chrome, against 0.5 ms native on all cores of the same machine. Plan for
tens of thousands of units on the web, rather than millions. A build with
threads (`web/build.sh --threads`, on nightly Rust) runs parallel queries,
the parallel scheduler and jobs on Web Workers where the page has those
headers, and on one thread where it doesn't: see [Threads on the
web](web-threads.md).

**Measure time with the engine's `Instant`.** `std::time::Instant::now()`
panics in browsers. `ponko::perf::Instant` is the same type on desktop and
works on the web:

```rust
# use ponko::prelude::*;
let start = ponko::perf::Instant::now();
// ... work ...
let ms = start.elapsed().as_secs_f64() * 1000.0;
# let _ = ms;
```

**Settings come from the address, not the environment.** There are no
environment variables in a browser. The inspector reads `?inspector=1`
(on), `?inspector=open` (on and open) or `?inspector=0` instead of
`PONKO_INSPECTOR`.

**The page is yours.** The engine draws into `<canvas id="ponko">` if the
page has one, and adds a canvas otherwise. `web/index.html` makes the canvas
fill the page; change it to put the game anywhere. Browsers only allow
sound once the player has clicked, tapped or pressed a key: music and other
looping sounds started before that wait and start then, one-shots are
dropped, and `ctx.audio.unlocked()` says whether browser audio is ready. Music
loaded with `load_stream` is decoded as it plays rather than all at once
(see [Sound in a browser](audio.md#in-a-browser)).

**Typing with an IME.** Browsers send composition events only to text
boxes, so while a text input (or your own widget, through
`ctx.input.set_ime_allowed`) wants IME, the engine focuses a hidden
`<textarea>` placed at the caret, where the browser opens its candidate
window, and reads the composition, the committed text and the keys from it.
See [Typing with an input method](ui.md#typing-with-an-input-method-ime).

**Gamepads are read by the engine itself.** A web build reads
`navigator.getGamepads()` once a frame on the page's thread, in the
browser's standard layout and with gilrs's dead zone, so pads read as on
the desktop; rumble goes to the pad's `vibrationActuator` (trigger rumble
where it lists `trigger-rumble`; see
[Controllers](controllers.md#what-each-platform-supports)). The ordinary
build used gilrs's web backend until the engine's own (written for builds
with threads, whose memory can't hold the browser's gamepad objects)
replaced it, measured on the threads fixture's ordinary build in headless
Chromium 156:

- **Code size**: the module is 85 KB smaller (15.01 MB instead of 15.09;
  16 KB smaller compressed with Brotli or gzip) and its JavaScript glue
  2.5 KB smaller.
- **Latency**: the same. Every fed button press reached the game on the
  next frame (60 presses with the engine's backend, 120 with gilrs), a
  median 14.2 to 14.9 ms after the press against 14.2 to 14.7 with gilrs:
  the wait for that frame.
- **What gilrs needed worked around**: it has no rumble on the web and
  doesn't say where in the browser's list a pad is, so rumble found the pad
  again by its name and its place among pads of that name, a guess two
  identical pads could get wrong; and builds with threads read pads
  differently from the ordinary build (no dead zone there). One backend
  now does both builds, keeps the pad's browser index and plays rumble on
  it directly, and the headless checks (`web/smoke/threads.mjs --case
  gamepads` and `gamepads-plain`) compare exact values for both.

gilrs stays the desktop's backend.

**Multiplayer over WebSocket.** A browser can't open UDP sockets, so a web
build joins with `Net::join_web` or `NetMessages::join_web`, and the host
takes browser players with `accept_web` next to its desktop players (see
[In the browser](networking.md#in-the-browser)).

**Developer control.** An opt-in build can connect to a local bridge so
MCP tools can inspect the game, step it and capture its canvas. See
[Controlling a browser tab](control.md#controlling-a-browser-tab).

**Not available on the web:** [hot reload](data-files.md) of files, [hot
patching](hot-patching.md) of code, and APIs that write native files
(`ctx.gfx.save_screenshot`, saving the world to a path), and [video
playback](video.md#in-a-browser) (the page's own `<video>` element plays
intros instead). The player's own
files do work: `ctx.storage` keeps them in the browser's storage (see [Saves
and settings](storage.md#in-the-browser)). `println!` output goes
nowhere; panics appear in the browser's developer console and on the page
(see [Failures while playing](#failures-while-playing)).

## Startup errors

The generated page keeps “Loading…” visible until window creation, graphics
initialization and setup finish. It displays the engine's error if neither
WebGPU nor WebGL2 can start, the WebGL2 context is lost during startup, or
setup returns an error. Use `try_setup` with fallible loading calls for assets
the game needs to start:

```rust,no_run
# use ponko::prelude::*;
Game::new("My Game", 800, 600)
    .try_setup(|ctx| {
        let _logo = ctx.gfx.try_load_texture("logo.png")?;
        Ok(())
    })
    .run();
```

Custom pages can subscribe using the `onPonkoStartup(callback)` export from
the generated `game.js`. Register it after calling `init()` and handling
winit's “Using exceptions for control flow” exception. The callback runs once
with `null` on success or an `Error` with a `kind` (a Rust `ErrorKind` variant
name, such as `"Unsupported"` or `"NotFound"`) and a `message`. A callback
registered after startup receives the result immediately.

```js
import init, { onPonkoStartup } from "./game.js";

try {
  await init();
} catch (error) {
  if (!String(error).includes("Using exceptions for control flow")) throw error;
}

onPonkoStartup((error) => {
  const status = document.getElementById("status");
  if (error) status.textContent = error.message;
  else status.remove();
});
```

In Rust browser builds with the `window` feature, register
`ponko::web::on_startup` before running the game to receive the same
`Result<(), ponko::error::Error>`. `Game::try_run` can return errors from
creating the event loop synchronously. Later initialization and setup report
through the callback. This contract covers recoverable startup errors.
Panics, in setup or later, are reported as failures (next section).

## Failures while playing

A web build can't recover from a panic: it aborts, and the game stops. The
generated page shows why instead of a frozen canvas: “The game stopped: ”
and the panic's message, in place of the game. The same goes for the
engine's other fatal failures in the browser: the WebGL2 context or the
WebGPU device lost while the game runs (the browser reset its GPU process;
the game can't draw on a restored context or a new device, so the page asks
for a reload; a lost WebGPU device's message says whether the device was
destroyed or lost for another reason, with the browser's own words, such as
“Device was destroyed”), and a worker thread of a
[threads](web-threads.md) build that stopped, with that worker's panic
message. The panic also stays in the developer console, with its location.
A game with [crash reports](crash-reports.md#in-the-browser) on also gets
the long version for its developers, which the page offers as a file.

The game can say it in its own words. Register `ponko::web::on_failure`
before running the game: it runs once, right before the game stops, with
the failure's `kind()` (`FailureKind::Panic`, `ContextLost`, `DeviceLost`
or `ThreadStopped`), `message()`, for a panic `location()`, and for a lost
WebGPU device `device_lost_reason()`: `DeviceLostReason::Destroyed` when
something called the device's `destroy()` (the page, an extension, the
browser tearing the page down), `Unknown` for anything else (the GPU
process crashed or was reset, the driver failed), so the two can be worded
differently. Return the text the page should show, or `None` to keep the
page's own:

```rust,no_run
# use ponko::prelude::*;
#[cfg(target_arch = "wasm32")]
ponko::web::on_failure(|failure| {
    Some(format!("Sorry, the game crashed ({failure}). Reload the page to play again."))
});
Game::new("My Game", 800, 600).run(|_ctx| {});
```

For a panic the callback runs inside the panic hook: keep it short, and
don't panic in it (a second panic aborts before the page hears of the
first). Several callbacks may be registered; all run, in order, and the
last text wins.

Custom pages subscribe with the `onPonkoFailure(callback)` export of
`game.js`, after `init()` as for `onPonkoStartup`. The callback runs at
most once, with an `Error` whose `kind` is `"Panic"`, `"ContextLost"`,
`"DeviceLost"` or `"ThreadStopped"`, `message` the failure's message, `location` a panic's
source position (else `null`), `reason` a lost WebGPU device's
`"Destroyed"` or `"Unknown"` (else `null`) and `text` the game's own text
from `on_failure` (else `null`). Only the first failure is reported: the
JavaScript exception a panic's abort throws afterwards is not. A callback
registered after the failure runs immediately.

```js
import init, { onPonkoFailure } from "./game.js";

try {
  await init();
} catch (error) {
  if (!String(error).includes("Using exceptions for control flow")) throw error;
}

onPonkoFailure((failure) => {
  const status = document.getElementById("status");
  status.textContent = failure.text ?? `The game stopped: ${failure.message}`;
});
```

## Testing in a browser

`web/smoke/` in the engine's repository runs a web build in a headless
browser (Chromium, Firefox or WebKit) and checks that it starts, fetches its
assets, keeps simulating, reacts to a click and draws something. It also reads
the engine's own report of the GPU it opened (`ctx.gfx.gpu_capabilities()`) and
checks that it agrees with the canvas, so a page that fell back to WebGL2
can't pass as WebGPU. A 2D fixture, a 3D fixture (built with `--features 3d`:
a glTF model fetched and parsed in the browser, with a sun and shadows) and
the complete-game reference `coin_dash` ([A complete game,
packaged](packaging.md#the-web-build)) are built and checked there. The
list of checks runs WebGL2 in all three browsers; WebGPU is tried too, and a
browser that can't show a WebGPU canvas is reported as unsupported, with the
reason, never as a pass. Its `web-gpu` job is the gate for WebGPU: on a
machine with a GPU (a Mac), it runs the 2D and 3D fixtures on WebGPU and the
3D one on WebGL2 with `--require-gpu`, which fails, instead of reporting,
when WebGPU is unsupported, when the browser or the engine is on a software
renderer such as SwiftShader or a fallback adapter, or when the engine fell
back to WebGL2; then it measures the [frame times](#frame-times). The 2D fixture's startup failure cases check both
the callback's error and the message shown on the page. Each run leaves a
report, a trace and a screenshot.
See its
[README](../../web/smoke/README.md)
for running it, adding a fixture for another example, and what runs where.

## C and C++

C and C++ games build for the browser too. Once, besides the Rust setup
above, install the [WASI SDK](https://github.com/WebAssembly/wasi-sdk),
version 33 or newer (its C++ library has exceptions, which `ponko.hpp`
uses), in `/opt/wasi-sdk`, or anywhere with `WASI_SDK_PATH` pointing at it.
Then, from your game's folder, with
[`web/build-cpp.sh`](../../web/build-cpp.sh) from a checkout of the engine:

```sh
../ponko/web/build-cpp.sh -o target/web/my_game main.cpp world.cpp
python3 -m http.server -d target/web/my_game 8000
```

The script compiles your sources with the WASI SDK's clang for wasm32, links
them with its C and C++ libraries into the engine's ordinary web build, and
writes the same static site as `web/build.sh`: the page, `game.js`,
`game_bg.wasm` and a copy of your `assets/`. `--features` picks
`ponko_ffi`'s features, and flags after `--` go to the compiler
(`-- -DNDEBUG -Isrc`). The guide's first game
([Getting started](getting-started.md)), built this way, is 23.2 MB of
WebAssembly (7.9 MB compressed with gzip) and reaches its first frame
**0.2 s** after the page is opened (198 ms, 195 to 201 in five page loads),
in headless Chromium 156 on WebGL2 on an Apple M4 Max laptop, idle (a
1-minute load of 2.6 to 2.9), WASI SDK 34, on 2026-11-26 at revision
`060b5cf4` (`node web/smoke/cpp.mjs --build`, then five runs without
`--build`). The earlier 0.5 s was measured while other builds were
running. That is more than a Rust game's build, because the
library's C API reaches the engine's 3D renderer and navigation whatever
the game calls, and `ponko_ffi`'s default features (2D and 3D physics, the
inspector, text shaping, sound) are all in it.

What's different from the desktop, besides [the above](#whats-different-in-a-browser):

- **`run()` doesn't return.** The browser runs the game's frames, so
  `ponko_game_run` (`game.run(...)` in C++) leaves your `main` without
  returning while the game plays: the code after it never runs, and nothing
  in the functions that called it is destroyed. Locals your systems capture
  by reference stay valid, as they do on the desktop, where `run()` blocks.
  Save the game when the player asks, not after `run()`.
- **The standard library has no files.** `printf` and `std::cout` go to
  the browser's console, `std::cerr` to its errors. `fopen` finds no file:
  load your data through the engine's assets (they are fetched before
  setup), and keep saves with the engine's [storage](storage.md). `sleep` returns at once,
  `std::thread` can't start a thread, and `exit()` stops the game with an
  error. The build stops, naming them, if your code calls any other function
  the browser doesn't have.
- **C++ exceptions** work, with WebAssembly exception handling: the game
  needs Chrome 137, Firefox 131 or Safari 18.4 or newer.

Emscripten, the usual compiler for C and C++ on the web, can't build the
engine: its window, input and graphics run on winit and wasm-bindgen, which
have no Emscripten backend. `web/smoke/cpp.mjs` builds the guide's first
game this way and checks in a headless browser that it starts, draws and
moves its crab while the right arrow key is held.
