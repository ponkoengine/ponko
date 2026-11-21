# Threads on the web

A web build can use more than one core. The frame's parallel work runs on
Web Workers that share the game's memory, as it does on threads natively:
- parallel queries (`par_for_each` and the other `par_*` calls);
- the [parallel scheduler](parallel-systems.md)'s batches;
- the ECS's own parallel sorts;
- the [hierarchy](hierarchy.md)'s update (attachments), when a pass is big
  enough to share out;
- the engine's own parallel passes: [spatial grid](spatial.md) rebuilds
  (from 200,000 entries), [flow field](pathfinding.md) directions, 2D
  skeleton drawing and its copy into the frame's draw list (from 64
  skeletons), 3D character moves (from 16 characters), the 3D renderer's
  culling (each model compared with its last frame and culled again for
  the camera and the shadow views) and the entities' JSON of a world save
  (`World::save`, and so `save_world` and `save_world_in_background`).

They all go through one runner: a pass started on the page's thread is
handed to the frame's workers and the page's thread spins until it is done,
as for a parallel query. A pass that cuts its work into pieces cuts it for
the frame pool's threads, not for the page's own (one): the 3D culling, for
one, takes its single-pass path only when the game really has one thread.
Each pass gives the same result on any number of threads (the native tests
run each on one thread and on all of them; a JSON save's text is the same
byte for byte).

[Jobs](streaming.md) run on workers of their own. The asset reads and
decodes of [asset groups](loading.md) and streaming are jobs, so they run
there too.

Two things are needed: a **build with threads**, which takes a nightly
toolchain, and a page served with **two headers**. Without the headers,
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

| Host | How | Written by `ponko package` |
|---|---|---|
| Netlify, Cloudflare Pages | `_headers` | yes |
| Vercel | `vercel.json` | yes |
| Apache (with `mod_headers`) | `.htaccess` | yes |
| nginx | `add_header Cross-Origin-Opener-Policy same-origin always;` and the same for `Cross-Origin-Embedder-Policy require-corp`, in the site's `location` | no: server configuration |
| Caddy | `header Cross-Origin-Opener-Policy same-origin` and `header Cross-Origin-Embedder-Policy require-corp` in the site block | no: server configuration |
| Firebase Hosting | a `headers` entry in `firebase.json`, which sits beside the folder, not in it | no |
| itch.io | its embed options for HTML games offer `SharedArrayBuffer` support; check on the page itself | no |
| GitHub Pages | can't set headers: the game runs on one thread | no |

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
  streamed textures and meshes read and decode on them. Saves in the
  background (`save_world_in_background`) build the world's JSON on the
  frame's workers, as `World::save` does, then encode and write it on the
  page's thread, because the browser's storage belongs to it.
- **Everything else** stays on the page's thread: your systems, drawing
  (the GPU's command recording too: WebGPU objects can't be sent to a
  worker), sound, input, and the engine's other internal parallel loops
  (baking, video frames), which there run one piece after another.
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
three page loads, 60 timed runs each after 20 untimed ones. Repeat them on a
quiet machine before quoting them.

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
hierarchy must share a level out, and at least two of the engine's own
passes (the fixture rebuilds a spatial grid of 200,000 entries, computes
a flow field, collects the 3D frame of 20,000 models and saves its world as
JSON) must go through the frame pool's runner (`passes_shared`, from the
scope each opens there), the 3D culling and the save among them. The
runner builds the fixture with the `3d` and `bench-internals` features,
for the 3D collection without a GPU.
Served without them, everything must run on the page's thread, and the
console must say why. Both runs must end with the same world (the same 3D
frame and the same JSON save too), and must read the same file. With `--case gamepads` it feeds the isolated page fake
browser gamepads and checks that the engine sees each one connect, press,
move and leave.

```sh
node web/smoke/threads.mjs --build                # the checks
node web/smoke/threads.mjs --case worker-panic    # a worker's panic stops the game
node web/smoke/threads.mjs --case gamepads        # fed gamepads reach the game
node web/smoke/threads.mjs --case warning         # a warning fails the build
node web/smoke/threads.mjs --bench --repeat 3     # the numbers above
```

So far this runs in Chromium only. Firefox and Safari also allow shared
memory on isolated pages, but nobody has tried the game in them yet.

## C and C++

C and C++ games have no web builds yet ([Web builds](web.md#c-and-c)), so
threads on the web are for Rust games for now.
