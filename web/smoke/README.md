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
   **Storage** (`--after-reload`): once the game's writes are stored (its
   `saving` field is no longer `true`), the page is reloaded, and the new page
   must report the given fields (`coin_dash`: the settings it saved when the
   settings screen was left are `Loaded` from the browser's storage, which is
   `persistent`).
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
| `lights2d` | `examples/lights2d.rs` | none | `crab.png` fetched and decoded 32x32; 2D lights with stencil shadows and normal maps drawing on the page's backend (WebGL2 in CI); at least 7 lights

To add one: publish the state with the helper (or the same attributes by hand),
add an entry to `fixtures.json` (`example`, `features`, `asset`, `assetSize`
or none, `assetSource` or none, `presses`, `min`, `afterReload`, `engineGpu`,
`music`), and name it in the CI job. Every
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
  and the engine's message with the reason the page's own `device.lost`
  got (“Device was destroyed”). Where the game opened no WebGPU device (the
  browser has none, so it runs on WebGL2) the case is unsupported, not
  failed; a run where every case was unsupported exits 2, or 0 with
  `--allow-unsupported`. Chromium's headless shell can't present a WebGPU
  canvas and loses the device by itself before the runner does; the run
  says which (“device lost by the browser” or “by the runner”). The other
  cases hide WebGPU, so that loss can't stop the game before they do.

The callback is subscribed late, after the failure, so the run also checks
that a late subscriber gets it at once. `runtime-failures.test.mjs` checks
the template's handling and the runner's checks without a browser.

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
so far in Chromium 156 (headless shell) on an Apple Silicon Mac.

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
`web/build.sh --threads web_threads` (`--build` builds it first). It serves
the build twice and opens each in a new browser context:

- **With** the cross-origin isolation headers
  (`Cross-Origin-Opener-Policy: same-origin`,
  `Cross-Origin-Embedder-Policy: require-corp`):
  - the engine must start two or more workers;
  - a parallel query, a batch of four view systems (threshold zero), two
    jobs and an asset group (a texture and a `with` file from `assets/`)
    must each run on two or more of them, and never on the page's thread.
- **Without** the headers: everything runs on the page's thread, and the
  console says why (`it runs on the page's thread alone`).

Both runs must reach the same checksum at tick 120 and read the same file.
Two more modes:
- `--case worker-panic` panics inside the parallel query on a worker. The
  console must say `the game stops`, the page must still answer, and the
  ticks must stop.
- `--bench` times the fixture's workloads (`par`, `sweep`, `interleaved`,
  `jobs`: those of `crates/ponko_bench`) with `?threads=1` and with the
  default, three page loads each (`--repeat`). It prints the medians, their
  spread, and each speed-up, saying whether it is beyond the noise.
  `--dir <an ordinary build> --threads 1` times a build without threads.
- `--case gamepads` replaces `navigator.getGamepads()` on the isolated page
  with fed pads (an Xbox pad by its Chromium `id`), and steps them: no pad,
  a pad connects, South, d-pad right, a quarter-pulled left trigger and both
  sticks, everything let go, the pad unplugged. After each step the fixture's
  `pads` must show what the engine saw: the pad in slot 0 with its name,
  family and USB ids, exactly those buttons down, the axes (stick y up stays
  negative), one South press, then no pad.
- `--case warning` needs no browser: it builds the example
  `examples/web_threads_warning.rs` with threads and `-D warnings`. The
  example's only code is an unused function that only a build with threads
  compiles, so the build must fail on it, as an error from `-D warnings`.
  If it passes, the threaded build lost `-D warnings` (or its atomics).

`--build` (and `--case warning`) add `-D warnings` to the build's flags, so a
warning in the threaded build fails the check. `web/build.sh --threads` keeps
the caller's flags: cargo reads only the first of `CARGO_ENCODED_RUSTFLAGS`,
`RUSTFLAGS` and the target's flags that is set, and the thread flags go into
that one (`build.test.mjs` checks this with `--print-env`).

```sh
node web/smoke/threads.mjs --build
node web/smoke/threads.mjs --case worker-panic
node web/smoke/threads.mjs --case gamepads
node web/smoke/threads.mjs --case warning
node web/smoke/threads.mjs --bench --workloads par,sweep --repeat 3
```

Exit codes and `report.json` (in `--out`, default
`target/web-smoke/threads`) are as for `smoke.mjs`. `threads.test.mjs`
tests its checks and its summary without a browser. Checked so far in
Chromium 156 (headless shell) on an Apple Silicon Mac; Firefox and WebKit
are not tried yet.

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
| `--after-reload <field>:<value>` | After the presses, reload the page (once the state's `saving` is not `true`); the new page's `<field>` must then be `<value>`. Repeatable. |
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
It is never a pass without a reason: the last line, `report.json` and the CI
summary table all say `UNSUPPORTED` and why (no `navigator.gpu`, no adapter,
no canvas), even under `--allow-unsupported`.

A browser that stops answering can't hang a run: once the page is open,
every browser call (reading the state, a click, a key, a screenshot) has a
30 s deadline, a page whose content process crashes fails at once
(`browser-crash`), and the whole run has `--run-timeout`. A hang fails as
`browser-hang` with the step it was on and the last lines of `browser.log`.
A hung run is not retried: so far every hang has been a real, repeatable
failure, and a retry would hide the next one. CI also gives each browser step
a `timeout-minutes` of a few minutes.

WebKit on Linux needs a sound server and a channel count. It opens its audio
output through GStreamer and reads an AudioContext's output channels
(`destination.maxChannelCount`) from GStreamer's list of audio devices.
Without a sound server its content process crashes when a page asks, which
the engine does when it opens its audio at a game's first click (the log
shows GStreamer's PulseAudio device provider failing to connect, then
"double free or corruption"). With one, the list can still come back empty,
the count is 0, and the engine's audio output refuses to open (the report's
`audio` shows a running context with 0 output channels and 0 buffers). CI
starts PulseAudio with a null sink and sets
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
table (and appends it to a GitHub job summary).

## What runs where

| Where | Browser | WebGL2 (forced) | WebGPU |
|---|---|---|---|
| CI (`web-smoke` job, `main` and `full-ci`, ubuntu-latest, no GPU) | Chromium (headless shell and full browser), Firefox, WebKit from Playwright | Must pass in Chromium and WebKit, for the 2D and 3D fixtures (software GL); `coin_dash` and the multiplayer check (`multiplayer.mjs`) in Chromium. Firefox is tried and reported: headless Firefox has no WebGL2 on that runner, so it reads "unsupported" | Tried; passes as "unsupported", with the reason, where the browser can't show a WebGPU canvas; where it can, the game must draw through it |
| CI, every pull request (`web-smoke-runner`) | none | the runner's own tests only (and the gallery's) | |
| Release workflow (`gallery` job, ubuntu-latest, no GPU) | Chromium headless shell | Every gallery page must pass (software GL) | Not tried; the demos that need WebGPU must show their message |
| Apple Silicon Mac, the gallery (`gallery.mjs`) | Chromium 156 | Every page passes in the headless shell; the 2 demos that need WebGPU (compute passes) show their message | Every page passes in new headless mode on the Metal adapter |
| Apple Silicon Mac, Chromium new headless mode | Chromium 156 | Passes (ANGLE on Metal) | **Passes** on the Metal adapter: the engine reports `webgpu`, the canvas draws |
| Apple Silicon Mac, Chromium headless shell | same | Passes (ANGLE on SwiftShader) | Unsupported: a SwiftShader fallback adapter exists, but a WebGPU canvas can't be shown |
| Apple Silicon Mac, `multiplayer.mjs` | Chromium 156 headless shell | Passes: two tabs, a native host and a native UDP client agree on the world at tick 300 | Not tried (WebGPU is hidden) |
| Apple Silicon Mac, WebKit (Playwright build) | WebKit 27 | Passes | Passes on its own adapter |
| Apple Silicon Mac, `threads.mjs` | Chromium 156 headless shell | Passes on whichever backend the page picks (the check is about threads, not drawing): workers on the isolated page, one thread without the headers, the same world; a worker's panic stops the game | Not tried on its own |

Not covered yet: Firefox on a Mac with its Playwright build here (it would
not start on the development machine, and on CI's Linux runner it has no
WebGL2, so Firefox has not drawn a frame anywhere yet), WebGPU on a
GPU-less Linux runner (the CI job tries it and reports), and mobile
browsers.

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
