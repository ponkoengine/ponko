# Threads on the web

> **Maturity: [experimental](maturity.md).** Game: none. Tests: the threaded
> build is linted on nightly Rust. Platforms: the web, built with nightly Rust.
> Graphics: WebGL2 and WebGPU. Missing: the chapter calls it experimental; the
> thread-heaps build has not run in a browser since its last fix.

A web build can use more than one core. The frame's parallel work runs on
Web Workers that share the game's memory, as it does on threads natively:
- parallel queries (`par_for_each` and the other `par_*` calls);
- the [parallel scheduler](parallel-systems.md)'s batches;
- the ECS's own parallel sorts;
- the [hierarchy](hierarchy.md)'s update (attachments), when a level is big
  enough to share out (on the web, one that would take 0.4 ms on the
  page's thread: about 14,000 attached children);
- the engine's own parallel passes: [spatial grid](spatial.md) rebuilds
  (from 200,000 entries), [flow field](pathfinding.md) directions, 2D
  skeleton drawing and its copy into the frame's draw list (from 512
  skeletons on the web, 64 natively), 3D character moves (from 512
  characters on the web, 16 natively), the 3D renderer's
  culling (each model compared with its last frame and culled again for
  the camera and the shadow views) and [light baking](3d.md#baked-lighting)
  (`bake_lighting`).

[The engine's parallel loops](#the-engines-parallel-loops) lists every one,
with what it measured in a browser, and the few that stay on the page's
thread.

They all go through one runner: a pass started on the page's thread is
handed to the frame's workers and the page's thread spins until it is done,
as for a parallel query. A pass that cuts its work into pieces cuts it for
the frame pool's threads, not for the page's own (one): the 3D culling, for
one, takes its single-pass path only when the game really has one thread.
Each pass gives the same result on any number of threads (the native tests
run each on one thread and on all of them).

[Jobs](streaming.md) run on workers of their own. The asset reads and
decodes of [asset groups](loading.md) and streaming are jobs, so they run
there too.

Two things are needed: a **build with threads**, which takes a nightly
toolchain, and a page served with **two headers** (or, on a host that can't
send them, [a service worker](#hosts-that-cant-send-the-headers) that adds
them). Without the headers,
the same build runs on the page's thread alone and says why in the
browser's console. The ordinary web build (`web/build.sh` without
`--threads`) is unchanged: it has no threads and needs neither.

## Building with threads

Once:

```sh
rustup toolchain install nightly --component rust-src
```

Then, with [`web/build.sh`](../../web/build.sh) and
[`web/ponko-worker.js`](../../web/ponko-worker.js) copied from the
engine's repository, or with [`ponko package`](packaging.md#the-web-folder):

```sh
web/build.sh --threads --bin my_game     # -> target/web/my_game/
ponko package --web --threads            # -> target/package/my-game-0.1.0-web/
```

Both build the same way. The folder gets `ponko-worker.js` beside
`game.js`; it starts one worker thread. The engine starts the workers
itself, so a page of your own needs no code for them (a canvas's
`data-worker="path/to/ponko-worker.js"` attribute names the script if it
sits elsewhere).

`PONKO_WEB_TOOLCHAIN=nightly-2026-11-01` picks a dated nightly instead of
the latest, for a build that doesn't change under you.

### Why nightly: what the toolchains say

- **Rust.** The standard library that rustup ships for
  `wasm32-unknown-unknown` is compiled without atomics. Threads need every
  crate built with `-C target-feature=+atomics,+bulk-memory,+mutable-globals`,
  the standard library included. Rebuilding the standard library takes
  `-Zbuild-std=panic_abort,std`, which only nightly has (with the `rust-src`
  component). The `panic_abort` in it is not a choice: such a build has no
  unwinding.
- **The linker.** For this target, rustc doesn't make the memory shared by
  itself. Without more flags the module gets an ordinary memory, and no
  worker can share it. The build adds `--shared-memory`, `--import-memory`,
  `--max-memory=1073741824` and exports of the thread-local storage symbols
  (`__wasm_init_tls`, `__tls_size`, `__tls_align`, `__tls_base`), which
  wasm-bindgen needs to set up each thread.
- **wasm-bindgen** (0.2.130, the version the engine locks) sees the shared
  memory and writes a `game.js` that can start the module on more threads:
  - `initSync({ module, memory })` in each worker, with the module and
    memory of the page;
  - the start function (the game's `main`) runs on the first thread only.

  It doesn't start any workers: that is the engine's job
  (`ponko-worker.js`).
- **The browser** only allows shared memory (`SharedArrayBuffer`) on a page
  that is *cross-origin isolated*, which the two headers below make it.
  Chromium still loads the build on a page without them: the module and
  its shared memory are made. The HTML standard, though, forbids sending
  that memory to a worker from such a page. So the engine checks
  `crossOriginIsolated` first, and stays on one thread when it is false.
  That is why one build serves both kinds of page.

## The two headers

Every file of the game must come with:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

`ponko package --web --threads` writes the files that make common hosts
send them, beside `index.html`. A game's own file at one of those places
wins. Hosts that read none of them ignore them.

### Which hosts work

What matters is what a host lets you configure: response headers for the
game's files (then threads work with nothing more), or nothing (then
[the service worker](#hosts-that-cant-send-the-headers) gives the page the
headers itself, if the game's page is the top-level page). From each host's
own documentation, November 2026:

| Host | What you can configure | Threads | Written by `ponko package` |
|---|---|---|---|
| Netlify | headers, in a `_headers` file in the published folder (or `[[headers]]` in `netlify.toml`) | with the headers | `_headers` |
| Cloudflare Pages, Cloudflare Workers static assets | headers, in a `_headers` file in the output folder | with the headers | `_headers` |
| Vercel | headers, in `vercel.json` (`headers`, by `source` path) | with the headers | `vercel.json` |
| Apache (with `mod_headers`) | headers, in `.htaccess` where the server allows overrides | with the headers | `.htaccess` |
| Firebase Hosting | headers, in `firebase.json` (`hosting.headers`), which sits beside the folder, not in it | with the headers | no: add the two headers there |
| Azure Static Web Apps | headers, in `staticwebapp.config.json` (`globalHeaders`) | with the headers | no: add them there |
| AWS Amplify Hosting | headers, in `customHttp.yml` or the console's custom headers | with the headers | no: add them there |
| Render (static sites) | headers, in the site's dashboard settings (path `/*`) | with the headers | no: set them there |
| nginx | the server's configuration: `add_header Cross-Origin-Opener-Policy same-origin always;` and the same for `Cross-Origin-Embedder-Policy require-corp`, in the site's `location` | with the headers | no: server configuration |
| Caddy | the server's configuration: `header Cross-Origin-Opener-Policy same-origin` and `header Cross-Origin-Embedder-Policy require-corp` in the site block | with the headers | no: server configuration |
| GitHub Pages | no response headers | with the service worker | `--isolation-worker` |
| GitLab Pages | redirects and error pages, no response headers | with the service worker | `--isolation-worker` |
| itch.io | the game runs in a frame on itch.io's own page, so only itch.io can isolate it; its HTML5 documentation lists no header setting (the game's edit page has had a `SharedArrayBuffer` option: check there) | only if itch.io isolates its page; the service worker can't help inside another site's frame | no |

Any other static host is one of the two kinds: if it lets you set response
headers, set the two above for every file; if it doesn't, use the service
worker. A site that shows the game in a frame of its own page (a game
portal) must isolate that page itself.

On your own computer, `python3 -m http.server` sends neither header. This
serves the game's folder with them, on <http://localhost:8000>:

```sh
cd target/web/my_game && python3 -c '
import http.server
class Isolated(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()
http.server.test(Isolated, port=8000)'
```

`require-corp` has a cost: the page may then load files from other origins
only if they allow it (with `Cross-Origin-Resource-Policy` or CORS
headers). The game's own files come from the same origin and are fine.
Fonts, scripts or images from another site, and the page inside an
`<iframe>` of another site, need those sites' cooperation.

## Hosts that can't send the headers

[`web/ponko-isolate.js`](../../web/ponko-isolate.js) gives the page the two
headers without the server: a script on the page registers it as a
[service worker](https://developer.mozilla.org/en-US/docs/Web/API/Service_Worker_API),
which then adds `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` to every response from the
game's own origin, and the page reloads once to come through it. Add it
with:

```sh
web/build.sh --threads --isolation-worker --bin my_game
ponko package --web --threads --isolation-worker
```

Both put `ponko-isolate.js` beside `index.html` and load it in the page's
`<head>`, before the game's script. A page of your own adds
`<script src="ponko-isolate.js"></script>` to its `<head>`. It must stay
its own file, on the game's origin, beside the page or in a folder above
it: a service worker serves the folder it is in and those below it.

**When to use it.** Only on hosts that can't send headers ([the table
above](#which-hosts-work)): real headers cost nothing, the worker costs the
things below.

**What it costs.**
- **The first visit loads the page twice.** The page arrives without the
  headers; the script registers the worker and, once it is active, reloads.
  In the browser test the reload came before the game had logged anything,
  so the player sees the loading line twice. A forced reload (Shift+Reload)
  skips service workers, so the page arrives without the headers again and
  the script reloads once more.
- **Every request of the game's origin goes through the worker**, which
  copies each response's headers and adds two. The body is passed on as it
  streams.
- **It stays.** The browser keeps the worker for the origin, and every page
  in its folder and below gets the headers, with what they break: a page
  isolated this way can't talk to pop-up windows of other sites, so sign-in
  and payment windows that report back to the page stop working there, and
  files from other sites need their cooperation (above). Serve such pages
  from another folder or origin.
- **Where it can't work, nothing changes**: without a secure context
  (HTTPS, or `localhost`), without service workers (some browsers turn them
  off in private windows), inside another site's frame (the page around it
  must be isolated too), or when the page still isn't isolated after its
  one reload. The script says which in the console (`ponko-isolate.js: not
  isolating the page: ...`), never reloads more than once per tab, and the
  game runs on the page's thread alone.

`<script src="ponko-isolate.js" data-coep="credentialless"></script>` sends
`Cross-Origin-Embedder-Policy: credentialless` instead: files from other
sites then load without cookies instead of having to allow it. Chromium
(96 and later) and Firefox (119 and later) support it, Safari doesn't, and
there the page isn't isolated and the game runs on one thread. Keep
`require-corp` unless the game loads files from sites you don't control.

`node web/smoke/threads.mjs --build --case service-worker` proves it: it
serves the threaded fixture built with `--isolation-worker` *without* the
headers, and the page must reload once, be controlled by the worker, be
cross-origin isolated and pass every check of the isolated run. In headless
Chromium 156 it did: 13 worker threads, every pass that goes to the
workers went there, and the same checksum as the run with real headers.

## What runs where

When the game starts, it logs one of these to the console:

```text
ponko: running on 13 worker threads (and 6 for jobs)
ponko: this build can use threads, but the page isn't cross-origin isolated (...); it runs on the page's thread alone
```

- **The frame's threads.** By default the engine starts one fewer than the
  browser's `navigator.hardwareConcurrency`, at most 16: the page's thread
  waits while they work. `Game::web_threads` sets the number. Each thread
  holds its own stack, so a game with little parallel work can ask for
  fewer:

  ```rust,no_run
  # use ponko::prelude::*;
  Game::new("My Game", 800, 600)
      .web_threads(4) // 1 or 0: none, the page's thread alone
      .setup(|ctx| {
          // ...
  #       let _ = ctx;
      })
      .run();
  ```

  On the desktop the setting does nothing.
- **Jobs** have half as many workers of their own (at least one), so busy
  jobs never hold up a frame, as natively. [Asset groups](loading.md) and
  streamed textures and meshes read and decode on them. Saves
  (`World::save`, `save_world`, `save_world_in_background`) build the
  world's JSON on the page's thread, where it is faster (see
  [below](#the-engines-parallel-loops)), unless the build has
  [one heap per thread](#one-heap-per-thread), and encode and write it
  there too, because the browser's storage belongs to it.
- **Everything else** stays on the page's thread: your systems, drawing
  (the GPU's command recording too: WebGPU objects can't be sent to a
  worker), sound and input.
- **Controllers** work as in the ordinary web build (`ctx.input.gamepads()`
  and the rest of [Input](input.md)), read the same way: the browser's
  gamepad objects can't be sent between threads once the memory is shared,
  so the engine keeps none. Every frame it calls `navigator.getGamepads()`
  on the page's thread, copies the buttons and axes into plain numbers and
  compares them with the last frame's, which become the engine's events.
  Pads are read in the browser's standard layout, with gilrs's dead zone.
  Rumble goes to the pad's `vibrationActuator` where the browser has one
  (trigger rumble where it lists `trigger-rumble`), in both builds, played again every second so the browser's 5-second cap
  never ends it (see [Controllers](controllers.md#what-each-platform-supports)).
  A game's own
  `GamepadBackend` replaces it as usual.

`ponko::web::threads()` says how many frame threads the game runs on (0 on
the page's thread alone).

## When the page isn't isolated

The same build then runs the way an ordinary web build does: everything
on the page's thread, one piece after another, with the warning above in
the console. Nothing else changes: a world simulated with threads and
without ends up the same (the browser test checks this, below). If the
workers can't start, for example because `ponko-worker.js` is missing, the
game also stays on one thread, after at most 20 seconds, and says why.

## The engine's parallel loops

Every place the engine runs work in parallel (each use of rayon outside
benchmarks and tests), and where it runs in a web build with threads. The
times come from the browser fixture (`node web/smoke/threads.mjs --build`
prints them): each pass timed around its call on the page's thread, so with
threads the time includes handing it to the workers and waiting for them;
the median of 20 ticks (the bake runs once). Headless Chromium 156 on an
Apple M4 Max (14 cores), 13 frame threads, against the same build on a page
that isn't isolated. One page load each, with other builds running (a load
average of 100 to 150): the larger differences are far beyond that noise,
the ones under half a millisecond are not.

| Loop | In the fixture | Where it runs | 13 threads (ms) | Page's thread alone (ms) |
|---|---|---|---|---|
| Parallel queries, view systems, the scheduler's batches, the ECS's parallel sorts (the 2D sprite pass's parallel query too) | see [Numbers](#numbers) | the frame pool, through the ECS's runner | see [Numbers](#numbers) | |
| [Hierarchy](hierarchy.md) update (`hierarchy/shared_level`) | 20,000 attached children | the frame pool, when a level would take 0.4 ms on the page's thread ([thresholds](#when-sharing-out-pays)) | 0.57 | 0.60 |
| [Spatial grid](spatial.md) rebuild (`spatial/shared_rebuild`) | 200,000 entries | the frame pool | 1.37 | 1.70 |
| [Flow field](pathfinding.md) directions (`flow/shared_directions`) | 256 × 256 cells | the frame pool | 0.92 | 1.30 |
| 3D culling, retained (`three/shared_cull`) | 20,000 models | the frame pool | 0.36 | 0.90 |
| 2D skeleton drawing (`skeleton2d/shared_draw`) | 1,024 skeletons of two sprites | the frame pool, from 512 skeletons (64 natively) | 0.26 | 0.30 |
| 3D character moves (`physics3d/shared_characters`) | 600 characters walking | the frame pool, from 512 characters (16 natively) | 6.98 | 2.59 |
| [Light baking](3d.md#baked-lighting) (`three/shared_bake`): gathering, smoothing, probes | a floor, a ball and a wall, 16 samples | the frame pool | 21.9 | 52.3 |
| Entities' JSON of a world save (`World::save`) | 40,000 entities, 4 MB of JSON | **the page's thread**: on the workers it took 17 times as long ([below](#one-heap-per-thread)) | 67.6 | 68.5 |
| 3D command recording | | **the page's thread**, one encoder after another: WebGPU objects can't be sent to a worker | | |
| Jobs (asset reads and decodes, streaming) | | the job workers | | |
| Video frames (YUV to RGB) | | not in web builds: the `video` feature decodes on the desktop only | | |
| The C API's parallel loops (`ponko_ffi`) | | not in web builds: C and C++ games have none yet | | |
| The Unity importer (`ponko_unity`) | | not in web builds: a desktop tool | | |

What the numbers say:
- **The bake (2.4×), the 3D culling (2.5×) and the flow field (1.4×) gain
  from the workers.** The spatial grid's rebuild (1.2×) is the same within
  the noise.
- **The small passes are shared out only from sizes where that pays in a
  browser** (below): the fixture's 20,000 children and 1,024 skeletons are
  past their thresholds and come out the same as on the page's thread in
  this run, with the rest of the frame going on.
- **The characters were slower on the workers in the fixture** (7.0 ms
  against 2.6 ms), although their moves alone, in the threshold sweep, are
  twice as fast shared out at 512. The fixture's characters stand on the
  floor, and rapier's character controller allocates for every collider a
  character touches when it looks for the ground: on the workers, those
  allocations wait for each other on the browser's one allocator lock
  ([below](#one-heap-per-thread)). The sweep starts each move off the
  ground, which most likely skips that query. Below 512 characters they
  stay on the page's thread (the threshold was 16 before, where sharing
  out lost too); a game with more characters standing on the ground is
  likely faster with fewer of them moving on the workers, which nobody has
  measured yet.
- **The JSON save stays on the page's thread**: the browser's threads share
  one allocator lock, and the save is many small allocations, so on the
  workers it took 17 times as long as on the page's thread. The desktop
  keeps it parallel. [One heap per thread](#one-heap-per-thread) changes
  that.

### When sharing out pays

Handing a pass to the 13 workers and waiting for them costs 0.05 to 0.1 ms
in a browser, more than on the desktop, so the hierarchy, the 2D skeleton
drawing and the 3D characters have thresholds of their own in a web build
with threads. `node web/smoke/threads.mjs --case thresholds` times each at
several sizes, on the page's thread and shared out, in one page load (the
medians of 15 runs each, taking turns), then runs each just below and at
its threshold, where it must stay on the page's thread and be shared out,
and end on the same result as on the page's thread alone. Headless
Chromium 156, Apple M4 Max, 13 frame threads, with other builds running (a
load average of about 100); the two numbers are the lowest and highest of
three page loads:

| Pass | Size | Page's thread (ms) | Shared out (ms) |
|---|---|---|---|
| Hierarchy (attached children, placed by lookups) | 2,048 | 0.06 | 0.13 |
| | 4,096 | 0.12 | 0.13 to 0.15 |
| | 8,192 | 0.23 to 0.26 | 0.17 to 0.21 |
| | 16,384 | 0.46 | 0.26 to 0.30 |
| | 65,536 | 1.89 | 0.82 to 0.93 |
| 2D skeletons (two sprites each) | 128 | 0.04 | 0.09 to 0.13 |
| | 256 | 0.08 | 0.10 to 0.17 |
| | 512 | 0.16 | 0.10 to 0.18 |
| | 1,024 | 0.32 | 0.16 to 0.24 |
| | 4,096 | 1.28 to 1.30 | 0.38 to 0.50 |
| 3D characters (the moves alone, on a floor) | 128 | 0.06 | 0.08 to 0.09 |
| | 256 | 0.13 | 0.10 to 0.19 |
| | 512 | 0.27 to 0.28 | 0.13 to 0.14 |

So, in a web build with threads:
- **the hierarchy** shares a level out when its first 256 children, timed,
  project it over 0.4 ms on the page's thread (about 14,000 children; 50 µs
  natively). Sharing pays from about 5,000, but by less than 0.1 ms up to
  8,000;
- **2D skeletons** are drawn on the workers from 512 (64 natively): the
  first size where sharing never lost;
- **3D characters** move on the workers from 512 (16 natively): at 256 one
  run was faster each way.

Below a threshold, staying on the page's thread costs at most about 0.1 ms
against sharing out; past it, sharing saves more the bigger the pass. On
the desktop the thresholds stay as they were.

## One heap per thread

The standard allocator of a web build is one heap behind one lock, which
the page's thread can only spin on (browsers don't let it block). Threads
that allocate at the same time wait for each other, so allocation-heavy
work is slow on the workers. A build with threads can instead give each
thread a heap of its own ([talc](https://crates.io/crates/talc), through
the `orx-parallel-wasm-allocator` crate): each thread allocates from its
own heap, and a block freed on another thread goes back to the one it came
from. It is not the default, because it made allocation on the page's
thread, where your systems run, slower:

| Pass | Standard allocator (ms) | One heap per thread (ms) |
|---|---|---|
| JSON world save (24,000 entities, 3 MB), on the page's thread | 41 | 56 to 63 |
| The same, on the workers | 698 to 700 | 38 to 40 |
| Parsing that JSON, on the page's thread | 13.2 to 15.3 | 14.9 to 15.1 |
| 100,000 blocks of 32 bytes allocated and freed, on the page's thread | 1.32 | 1.47 to 1.53 |
| A vector grown to 1,000,000 numbers by pushes, on the page's thread | 0.53 | 0.58 |
| A parallel query over 20,000 entities making a small vector and a string on each | 73 to 78 | 1.6 to 1.8 |
| `game_bg.wasm` | 21,634 KiB | 21,639 KiB |

From `node web/smoke/threads.mjs --build --case allocator` (three page
loads of each build, on the same busy machine as above; the ranges are the
lowest and highest median). The save on the page's thread, split up, is
slower in every part: building the JSON values (33 against 42 ms), writing
the text (5.6 against 9.1 ms) and dropping the values (2.6 against 6.4 ms).

So with the standard allocator the save stays on the page's thread (41 ms
there against 700 ms on the workers), and with one heap per thread it goes
to the workers (38 to 40 ms, against 56 to 63 ms on the page's thread):
about the same time as the default build. One heap per thread is worth it
for a game whose parallel code allocates (a parallel query that collects a
path or formats a label for each item, 40 times faster), and a loss for one
whose page thread allocates a lot. To try it:

```sh
RUSTFLAGS="--cfg ponko_web_thread_heaps" web/build.sh --threads --bin my_game
```

It is experimental. In the threads fixture's check, a page of a build with
it stopped on a misaligned read of a byte buffer (after the browser lost
its WebGPU device, which the standard build survives): the heaps aligned a
block only to what was asked, where the standard allocator always aligns
to 8 bytes. Every block is now aligned to 8 bytes too; that build hasn't
been run in a browser since.

Only a web build with threads takes it: the ordinary web build and native
builds ignore the flag, and keep the standard allocator and their size.
Such a build has its own global allocator, so a game that sets one (with
`#[global_allocator]`, such as `ponko::perf::CountingAllocator`) can't use
it. A worker's first two allocations (its stack and its thread-local
block, made before the thread has thread-local storage) read which heap to
use from the bottom of the memory, the deepest end of the page thread's
stack, which is normally untouched and so names the first heap.

## Limits

- **The page's thread never blocks.** Browsers don't let it, so while the
  workers run a parallel query, or while `Job::wait` waits, it spins.
  For the same reason, a job must not hold a lock (a `Mutex` or `RwLock`)
  that your systems take too: the page's thread would then wait for it,
  which the browser stops with an error. Send results back as the job's
  return value.
- **A panic on a worker stops the game.** A build with threads can't unwind
  (above), so a panic in a parallel query or a job ends the game, with the
  panic and `ponko: ...; the game stops` in the console, and on the page
  as a `ThreadStopped` failure with the worker's panic message (see
  [Failures while playing](web.md#failures-while-playing)). A job's panic
  isn't handed to `Job::take` as it is natively.
- **Memory**: at most 1 GiB, which the build declares up front, because a
  shared memory must have a maximum.
- **Thread-local values on workers are never dropped**: the workers live
  as long as the page.
- **A nightly toolchain.** The ordinary web build stays on stable, without
  threads.

## Warnings fail the build

`web/build.sh --threads` adds its flags to whichever of
`CARGO_ENCODED_RUSTFLAGS`, `RUSTFLAGS` and the target's flags cargo reads
(the first that is set), and keeps what is there. So the check list's
`RUSTFLAGS="-D warnings"` fails a threaded build on a warning, as it does
the others; code that only a build with threads compiles is checked too.
`web/smoke/threads.mjs --case warning` proves it: it builds an example
whose only code is an unused function behind
`target_feature = "atomics"`, and that build must fail on it.

## Numbers

The engine's simulation benchmarks (the workloads of `crates/ponko_bench`),
in headless Chromium 156, one thread (`?threads=1`) against the default (13
frame threads, 6 for jobs). The machine was an Apple M4 Max (14 cores) with
other builds running: a load average of about 100. These are medians of
three page loads, 60 timed runs each after 20 untimed ones.

Run again on the same machine while it was idle (2026-11-22, revision
`9235ea05`, 1-minute load 1.3, `--build --bench --repeat 3`), every
workload came out **slower**, the one-thread runs too: `par` 4.10 and
1.73 ms, `sweep` 16.06 and 4.39, `interleaved` 8.87 and 4.11, `jobs` 2.92
and 1.96 (one thread, then the default threads), speed-ups 1.5× to 3.7×.
The one-thread `par` run takes three times what it took on the busy
machine, which load alone can't explain; the likeliest cause is that macOS
runs the headless browser's threads on the slower efficiency cores when
nothing else asks for the fast ones, which was not confirmed. Until it is,
neither set is a clean measurement: the table keeps the busy machine's.

| Workload | One thread (ms) | Default threads (ms) | Speed-up | What limits it |
|---|---|---|---|---|
| `par`: `par_for_each` over 100,000 entities | 1.29 | 0.36 (0.35 to 0.37) | 3.5× | probably the cost of waking 13 threads and waiting for them, next to 1.3 ms of work |
| `sweep`: eight independent view systems, one batch | 16.66 | 2.28 (2.28 to 3.18) | 7.3× | eight systems, each on one thread: at most 8× |
| `interleaved`: producers and consumers in two batches | 8.37 | 2.25 | 3.7× | four systems per batch: at most 4× |
| `jobs`: 16 jobs, from spawning to the last result | 1.32 | 0.33 (0.32 to 0.34) | 4.0× | six job workers, so three rounds: at most 5.3× |

The ranges are the lowest and highest median of the three page loads.
Every speed-up is far beyond the noise: the one-thread runs varied by 2% at
most from load to load, and the threaded ones by up to 40% (`sweep`, which
a busy machine disturbs most).

**What a build with threads costs without them.** The same workloads
on one thread give 1.29, 16.65, 8.36 and 1.31 ms in the ordinary
build, and 1.29, 16.69, 8.39 and 1.31 ms in the build with threads, on a
page that runs it on one thread. That is the same within 0.5%, from runs
taken one after the other.

The fixture, `examples/web_threads.rs`, has the workloads;
`node web/smoke/threads.mjs --bench` runs them (`--dir` and `--threads 1`
time an ordinary build).

## Testing it in a browser

[`web/smoke/threads.mjs`](../../web/smoke/README.md#threads) builds the
fixture with threads and opens it in headless Chromium twice. Served with
the headers, the parallel query, the batch systems, the jobs and the asset
group must run on two or more workers, and never on the page's thread; the
hierarchy must share a level out, and the engine's own passes (the
fixture rebuilds a spatial grid of 200,000 entries, computes a flow field,
collects the 3D frame of 20,000 models, draws 1,024 2D skeletons, walks 600
3D characters and bakes a small scene's light, each past its threshold) must go through the frame
pool's runner (`passes_shared`, from the scope each opens there), the 3D
culling, the skeletons, the characters and the bake among them; its world's
JSON save must not. Every pass must report its time, and the runner prints
the table [above](#the-engines-parallel-loops). The runner builds the
fixture with the `3d`, `physics3d` and `bench-internals` features, for the
3D collection and the skeleton drawing without a GPU.
Served without them, everything must run on the page's thread, and the
console must say why. Both runs must end with the same world (the same 3D
frame, skeleton vertices, characters' places, bake and JSON save too), and
must read the same file. With `--case service-worker` it serves a build
with [the service worker](#hosts-that-cant-send-the-headers) without the
headers, and the page must reach the isolated run's results. With `--case gamepads` it feeds the isolated page fake
browser gamepads and checks that the engine sees each one connect, press,
move and leave. With `--case thresholds` it times the small passes at
several sizes and checks each on both sides of its threshold
([above](#when-sharing-out-pays)); with `--case allocator` it builds the
fixture again with [one heap per thread](#one-heap-per-thread) and compares
the two builds. The sweeps hide WebGPU from the page, so the engine draws
with WebGL2: they time the CPU only, and on a busy machine the headless
shell's WebGPU canvas once lost its device at the first frame.

```sh
node web/smoke/threads.mjs --build                # the checks
node web/smoke/threads.mjs --case worker-panic    # a worker's panic stops the game
node web/smoke/threads.mjs --case gamepads        # fed gamepads reach the game
node web/smoke/threads.mjs --build --case service-worker   # isolated without headers
node web/smoke/threads.mjs --case warning         # a warning fails the build
node web/smoke/threads.mjs --bench --repeat 3     # the numbers above
node web/smoke/threads.mjs --case thresholds      # where sharing out pays, both sides of each threshold
node web/smoke/threads.mjs --build --case allocator   # the standard allocator against one heap per thread
```

So far this runs in Chromium only. Firefox and Safari also allow shared
memory on isolated pages, but nobody has tried the game in them yet.

## C and C++

C and C++ games have no web builds yet ([Web builds](web.md#c-and-c)), so
threads on the web are for Rust games for now.
