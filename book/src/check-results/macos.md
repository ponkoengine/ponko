# Check results: macOS

> **Maturity: not a feature.** A record of runs of the list of checks on a Mac.

What ran of the list of checks (`scripts/checks.yml`) on macOS, when, on what,
and what came out. Each run adds a section at the top; older runs stay below.
A check that is skipped or needs a person is listed as such, never counted as
passed. The commands are the `run:` lines of `scripts/checks.yml`, with the
environment at its top (`RUSTFLAGS=-D warnings`, `RUSTDOCFLAGS=-D warnings`,
`CARGO_PROFILE_DEV_DEBUG=0`) unless a row says otherwise; the core part
(format, the scripts, `cargo deny`, the workspace and seven clippy builds, the
examples, `cargo test --workspace`, the C header, the debug C and C++ tests,
the guide's snippets and `mdbook build`) ran as one script with the default
environment.

Later runs can use `python3 scripts/run-checks.py`, which reads the same
list and writes a report (`target/check-results/`). It runs on a Mac only the
jobs written for macOS; the platform-neutral jobs written for `ubuntu-latest`
(clippy, docs, Miri, the web build and the browser runs, templates and more)
ran on this Mac by hand, as recorded below.

## 2026-11-26: five harder agent tasks, replayed and tried by an AI coding agent (#630)

The suite's five new tasks ("The agent acceptance suite" in
[Working with AI agents](../agents.md)), on the `tooling/630-harder-suite`
branch, on the same Mac (Apple M4 Max, macOS 27.2.1), debug builds, busy
(load average 30 to 800 on 14 cores: other agents were compiling).

**Replay.** `cargo test -p ponko_mcp --test agent_suite`: passed, 11/11
tasks solved, 36/36 checks, 50/50 negative controls caught, equal to the new
baseline (108 s; the code task about 55 s of it, its engine build in
`target/agent-suite` done before). Each new task's checker was also given
a second, different correct fix, which it passed: a guard that carries the
rest of its step round a corner, a prefab of 60 health with the spawner's
scale of 0.5 kept, spikes moved off the path instead of removed.

**An AI coding agent.** The same kind of agent as the run below (a CLI
coding agent with a shell, driving `agent_suite mcp TASK` with
`scripts/mcp-session.py`, headless), but this time it had written the five
tasks earlier the same day: it knew each answer before it started, and the
cannon's landing points from tuning the task. So this run shows the tasks
can be finished with the tools as documented and that the checkers accept
a real session's work; it says nothing about how hard they are. That needs
a fresh session of another agent, at the release cut. Seconds are
wall-clock from starting the server to the submission.

| Task | Solved | Checks | Tool calls | Tool errors | Submissions | Seconds | Scripted steps |
|---|---|---|---|---|---|---|---|
| `lost-points` | yes | 4/4 | 23 | 0 | 1 | 30 | 18 |
| `cannon-range` | yes | 3/3 | 15 | 0 | 1 | 17 | 15 |
| `tough-bats` | yes | 3/3 | 22 | 0 | 1 | 27 | 21 |
| `hold-the-gate` | yes | 3/3 | 95 | 0 | 1 | 33 | 11 |
| `stuck-guard` | yes | 5/5 | 23 | 0 | 1 | 31 | 14 |
| **total** | **5/5** | **18/18** | 178 | 0 | 5 | 138 | 79 |

What each run used, and what it ran into:

- `lost-points`: a scenario holding Right, then `take_events` showed no
  `Collected` event; after the radius fix the events showed five coins
  collected and a `Hurt` that took all five points at the spikes. The
  spikes were removed from the level with `load_scene`, `despawn_entity`
  and `save_scene`.
- `cannon-range`: three shots (400, 620, 648), each a file edit, one tick
  for the reload, `set_resource` on the trigger, `run_ticks` and
  `take_events` for the landing point.
- `tough-bats`: `watch` with the bat count and the `Spawning` resource
  showed a new bat every 3 s; the prefab was changed with `spawn_prefab`,
  `set_component` and `save_prefab`, the level's bats with `load_scene`,
  `set_component` and `save_scene`; a second `watch` after a fresh load
  showed the new timing. `Spawning` read 0 before the first tick; it now
  starts with the file's numbers.
- `hold-the-gate`: `save_world` at the start, then all 15 pairs, each a
  file edit, `load_world` and a 3,600-tick `run_ticks`: one pair holds
  (`bend` and `corner`); the two that cover the most road let two and three
  walkers through. 95 calls, all of the sweep.
- `stuck-guard`: two `get_entity` reads a tick apart showed the guard
  stepping back and forth across the first corner; one edit, then
  `quit_instance` and `launch` (`release: false`: 1.5 s to rebuild and
  start); `Reached` events showed two laps. `submit_task` took 8 s (a
  build and a fresh start, then 60 position reads).
- Friction fixed on the branch: `subscribe_events` replied `"types": []`
  when it collects everything, and accepted a misspelled type that would
  never collect anything. It now lists the types it collects and names
  unknown ones with a hint.
- Not changed: every tool call opens a new connection to the game, which
  accepts them every 20 ms, so a call takes about 45 ms here (840 calls in
39 s). That doesn't slow
  an agent, but a checker that reads a position every tick has to keep to
  a few windows (`stuck-guard` reads 60 positions, not 400).

## 2026-11-26: the agent acceptance suite, replayed and tried by a real agent (#528, section 7)

A rehearsal of the cut's step ("The agent acceptance suite", "At a
release", in [Working with AI agents](../agents.md)), on `main` at
`4957c566`, on the same Mac (Apple M4 Max, macOS 27.2.1), debug build
(`cargo build -p ponko_mcp --example agent_suite`), busy (load average 12
to 30 on 14 cores: other agents were compiling).

**Replay.** `cargo run -p ponko_mcp --example agent_suite -- replay`: 6/6
tasks solved, 18/18 checks, 22/22 negative controls caught, equal to the
0.2.0 baseline (34 s). `cargo test -p ponko_mcp --test agent_suite`: passed
(35 s). Nothing had to be fixed: no tool change since the suite was
written broke a scripted solution.

**A real agent.** An AI coding agent (a CLI coding agent with a shell, no
MCP client of its own) did each task through `agent_suite mcp TASK`,
driven with `scripts/mcp-session.py`, headless, no window. It read each
task's goal from the server's instructions only, and opened the scripted
solutions only after all six were scored. Seconds are wall-clock from
starting the server to the submission, the agent's own thinking included.

| Task | Solved | Checks | Tool calls | Tool errors | Submissions | Seconds | Scripted steps |
|---|---|---|---|---|---|---|---|
| `tune-speed` | yes | 3/3 | 18 | 1 | 1 | 28 | 7 |
| `coin-level` | yes | 4/4 | 13 | 1 | 1 | 24 | 11 |
| `immortal-enemy` | yes | 3/3 | 15 | 0 | 1 | 28 | 14 |
| `slow-crowd` | yes | 4/4 | 9 | 0 | 1 | 16 | 8 |
| `broken-shader` | yes | 2/2 | 3 | 0 | 1 | 14 | 5 |
| `hidden-score` | yes | 2/2 | 9 | 0 | 1 | 17 | 6 |
| **total** | **6/6** | **18/18** | 67 | 2 | 6 | 127 | 51 |

Where it got stuck, and what changed for it in the same pull request:

- `tune-speed`: its first scenario held `ArrowRight`, the browser's key
  name; the error listed the engine's names and the next call used
  `Right`. The `run_input_scenario` schema now lists the key names. Eight
  of its 18 calls were a hot-reload poll that went on after the value had
  changed (the agent's loop, not a tool).
- `coin-level`: `entity_stats` with only `with`, to count the coins,
  was refused for a missing `fields`. `fields` is optional now: without it
  the tool counts the matches.
- `hidden-score`: `expect_ui`'s fields (`node`, `shown`, `text`) were not
  in the tool's schema; the agent guessed them right from the guide. The
  schema now shows an `expect_ui` step.
- `broken-shader`: `shader_status` gave the right line, column and source
  line, but naga's message names IR handles, not types ("the `return`
  expression Some([10]) does not match the declared return type
  Some([5])"); the agent fixed it from the source line. Filed as #629.
- `immortal-enemy`, `slow-crowd`: no friction.

Compared with the scripted solutions, the agent made the same fix in every
task, mostly with the same values (speed 350 where the script writes 340;
the same five coin positions, armor 0.25, cell size 24, shader and HUD
fixes). The agent is the same kind of model that wrote the tasks and had
read the suite's chapter before it started, so this run shows the tasks can
be finished with the tools as documented; it is weak evidence of how hard
they are. Harder tasks are #630.

## 2026-11-26: every web example and Shard Grove in the browser (#528, sections 1, 5 and 6a)

`main` at `b7ca3382` plus the branch that adds the `web-examples` job, the
same Mac (Apple M4 Max, macOS 27.2.1, Node 26.13.1, wasm-bindgen 0.2.130,
playwright-core 1.65.1: Chromium 159.0.8154.6, WebKit 27.2). The machine was
busy throughout (load average 30 to 86 on 14 cores: other agents were
compiling). "On the GPU" is Chromium's new headless mode: WebGPU on the
Metal adapter (`apple`, `metal-3`, not a fallback), WebGL2 on ANGLE's Metal
renderer for the Apple M4 Max. "Software" is the headless shell: WebGL2 on
ANGLE over SwiftShader. Every run was headless; no window opened.

**What claims the web.** The gallery's list (`web/gallery/examples.json`,
49 examples after this branch adds 12) and the `web/smoke` fixtures and
examples (`web_smoke`, `web_smoke_3d`, `coin_dash`, `lights2d`, the editor
fixtures, `lockstep`, `net_arena`, `web_threads`, `web_spectrum`,
`web_video`), which the `web-smoke` job runs; its runs of 2026-11-26 are
recorded below. The
gallery's README names every other example and why it isn't on the web, and
`node --test web/gallery` now fails for one that is neither.

| Step (`web-examples` job) | Result | Time |
|---|---|---|
| Build: `node web/gallery/build.mjs` (49 examples) and `web/build.sh --features navmesh,physics3d shard_grove` | pass (release wasm: 1.1 GB of sites in `target/web/gallery`, 4.5 GB of build output in `target/wasm32-unknown-unknown`, deleted afterwards) | |
| Every gallery page, Chromium, WebGL2 on the GPU | **pass**, 49 of 49 (47 run on WebGL2, the 2 compute demos show their WebGPU message) | 85 s |
| Every gallery page, Chromium, WebGPU on the GPU | **pass**, 49 of 49, all on WebGPU | 90 s |
| Every gallery page, WebKit (its own choice: WebGPU) | **pass**, 49 of 49 | 97 s |
| Every gallery page, Chromium, WebGL2 (software) | 48 of 49 in the job's run: `shaders` timed out reading the canvas box (30 s) while its software-GL frames held the page, with a native test build running beside it; it passed alone twice (23 s, 35 s), and the check now allows 60 s, as for its screenshot | 543 s |
| Shard Grove, keys (`--fixture shard_grove`, `--require-gpu`): Tab, Enter, Escape, Enter, Escape | **pass** on WebGPU and on WebGL2 | 5 s, 6 s |
| Shard Grove, the whole walk-through (`--fixture shard_grove_walkthrough`, `--require-gpu`) | **pass** on WebGPU (36.7 s, 1,999 ticks, 2,394 frames) and on WebGL2 (36.7 s, 1,999 ticks, 2,393 frames): 6 shards, 1 save, 1 load, 2 pulses, 2 stuns, 1 win, no hit or loss | 46 s, 52 s |
| Also run: the walk-through in the headless shell (software WebGL2) and in WebKit | **pass**: 36.1 s, 787 frames (about 22 a second on SwiftShader); WebKit 36.4 s on WebGPU | 53 s, 49 s |
| Regression: the 3D fixture (`web_smoke_3d`, sun shadows) on the GPU | pass on WebGPU and WebGL2 | 12 s, 4 s |

**Found and fixed on the branch:**

- `lights` drew nothing on WebGL2 (a blank canvas on the GPU, and the
  console full of `GL_INVALID_OPERATION: glDrawElementsInstanced: Mismatch
  between texture format and sampler type`). With shadows off, the sun's
  shadow map is a 1x1 texture of one layer; wgpu's GL backend makes a
  one-layer texture a `TEXTURE_2D`, and the shaders read it through an array
  sampler, so the browser refused every draw. With shadows on (two or more
  cascades) the same build drew. Fixed: on GL the map and the 1x1 array
  stand-ins get a second layer (as texture arrays already did). After the
  fix `lights` on WebGL2 draws the same picture as on WebGPU, with no GL
  error. The Metal render tests of shadows and memory (16) pass unchanged.
- The software-GL gallery run had passed `lights` all the same: its
  screenshot was 0.12% off the background, just above the blank threshold.
  The gallery check now fails a page whose browser logged a WebGL error;
  the unfixed build fails with it, the fixed one passes.
- Shard Grove's web build only published its state. It now plays its
  walk-through in the page with `?smoke=walkthrough`, and the runner's
  `--until` waits for it.
- 12 examples with a picture weren't in the gallery although its README said
  every one was: `skeleton2d`, `ai_guards`, `prefabs`, `move_sprite`,
  `ui_widgets`, `sky_kit`, `stylized_sky`, `reflection_refresh`,
  `post_volumes`, `split_screen`, `crowd`, `water`. Each built and passed on
  WebGPU and WebGL2 on the GPU before it was added, and in all four runs
  above after.

## 2026-11-26: the ten-minute reference-game soaks (#509)

The `soak` job's command, run once per workload, sequentially, from the
`tooling/509-soak` branch at `1be6de65` (`main` at `719e6ec4` merged), on an
Apple M4 Max (14 cores, macOS 27.2.1, Metal, adapter "Apple M4 Max (Metal)"),
offscreen GPU drawing with the offline 48 kHz mixer:

```sh
python3 scripts/soak.py --seconds 600 --sample 10 --warmup 120 --out target/soak-10m
```

The machine was busy all the time: load average 28 to 50 for the first three
runs and rising to 111 (one minute) during the last one (other agents were
compiling). Each game ran 600 s in one process; figures below are after the
120-second warm-up.

| Game | Cycles | Saves, loads, deaths | Resizes, restores | RSS | GPU (renderer, adapter) | Live handles | Allocations per frame | Frame p50, p99 (median) | Verdict |
|---|---:|---|---|---|---|---|---|---|---|
| Coin Dash | 32 | 32, 32, 16 | 60, 30 | 31 to 54 MiB | 6.4, 8.4 MiB, constant | 3 textures, 5 sounds, 1 font, constant | 91 to 100 | 1.2 ms, 9.7 ms | pass |
| Ember Hop | 16 | 66, 33, 17 | 60, 30 | 35 to 38 MiB | 6.1, 8.0 MiB, constant | 9 textures, 9 sounds, constant | 110 to 150 | 1.5 ms, 10.0 ms | pass |
| Shard Grove | 12 | 13, 26, 13 | 60, 30 | 76 to 129 MiB | 306.0, 316.6 MiB, constant | 13 textures, 8 meshes, 21 materials, 9 sounds, constant | 813 to 1,949 | 10.8 ms, 32.3 ms | **fail** (allocations slope), see below |
| Streaming | 440 | none (no such mechanics) | 60, 30 | 76 to 139 MiB | 523.5, 528.7 MiB, constant | 524 to 548 tile meshes | 554 to 589 | 15.4 ms, 32.8 ms | **fail** (frame p50 and p99 slopes), see below |

Audio voices stayed at 1 to 4 real and 0 to 4 virtual. RSS only fell after
warm-up (macOS compresses idle pages of a busy machine); its fitted slopes
were -181, 2, -177 and 20 MiB/h against a bound of 196 MiB/h for this length.

Nothing grew. The two failures came from the bounds and the sampling, not
from the games:

- **Shard Grove, allocations per frame** (slope 45.6/h, bound 32.0/h): the
  readings alternated on every sample, 807 to 850 and 1,838 to 1,966, each
  flat over the run. The runner shrinks, minimizes and restores every 20 s,
  so every other 10-second sample held one such cycle, which costs this
  3D game about 450,000 allocations each time. Averaged over 20-second windows the series
  is 1,332 to 1,392 with a slope of -3.3/h. Ember Hop showed the same
  alternation, smaller (110 to 150). Change: `--sample` must now be a
  multiple of 20 s, and the job samples every 20 s.
- **Streaming, frame p50 and p99** (35/h against 15.5/h, 80/h against
  77/h): p50 jumped between 5 and 19 ms from one sample to the next while
  meshes, GPU bytes, allocations and frames per interval (about 490 per 10 s)
  stayed flat, and the machine's load doubled during the run. A fixed 2 ms
  noise allowance cannot hold on a shared machine. Change: a frame-time
  percentile may also change once by half its post-warm-up median, and every
  sample now records the one-minute load average.

Judged again with these rules (`--evaluate`, consecutive 10-second samples
merged into 20-second ones, frame percentiles averaged by frames, which is an
approximation), all four pass: allocation slopes -2.1, 4.2, -3.3 and -47.4/h
against 32/h. With the 10-second samples as recorded and only the new frame
allowance, streaming's p50 and p99 pass (35 against 58/h, 80 against 125/h).
No issue was filed: no quantity grew. The four-hour windowed runs on macOS,
Windows and Linux need a person (`scripts/checklists/soak.md`).

## 2026-11-26: Playwright 1.65.1 (#601) and the fuzz targets (#528, section 3)

`main` at `f308b7f6` plus the branch that moves `web/smoke` to
playwright-core 1.65.1 (Chromium 159.0.8154.6 `chromium-1255`, Firefox 159.0
`firefox-1572`, WebKit 27.2 `webkit-2388`), on the same Mac (Apple M4 Max,
macOS 27.2.1, Node 26.13.1, wasm-bindgen 0.2.130, WASI SDK 34 arm64-macos).
The machine was busy all the time (load average 60 to 92 on 14 cores: other
agents were compiling). Linux is not covered here: the `web-smoke` job on
Linux needs the Linux machine (#500).

### The `web-smoke` and `web-gpu` jobs

`scripts/run-checks.py` skips `web-smoke` on a Mac (its `runs-on` is
Ubuntu), so its steps ran one by one, headless, as written in
`scripts/checks.yml` (`RUSTFLAGS=-D warnings`), after
`npm ci` and `playwright-core install chromium firefox webkit`; the `web-gpu`
job ran from its own steps. Times are wall-clock.

| Check | Result | Time |
|---|---|---|
| `web-smoke`: runner tests (`node --test web/smoke/*.test.mjs web/gallery/*.test.mjs`) | pass, 177 tests | 4 s |
| `web-smoke`: build the fixtures | pass | 8 min 23 s |
| `web-smoke`: Chromium WebGL2: 2D fixture, 3D fixture, 2D lights | pass | 5 s, 10 s, 13 s |
| `web-smoke`: Chromium WebGL2: control endpoint (builds `ponko-mcp` and the web-control fixture) | pass | 1 min 22 s |
| `web-smoke`: Chromium WebGL2: editor saves (kept, persistent storage granted, refused); a glTF scene that didn't load and its Retry | pass | 15 s, 15 s, 16 s, 13 s |
| `web-smoke`: Chromium WebGL2: `coin_dash`; multiplayer (two tabs, two native players) | pass | 17 s; 24 s |
| `web-smoke`: Chromium: failures after startup | **fail**, then pass after the fix below | 64 s; 6 s |
| `web-smoke`: Chromium: key labels; a lost WebGPU device (new headless mode, SwiftShader); WebGPU first and each way back to WebGL2 | pass | 13 s; 4 s; 16 s |
| `web-smoke`: Chromium WebGPU: headless shell (reported unsupported: a WebGPU canvas reads back black) | pass | 2 s |
| `web-smoke`: Chromium WebGPU: new headless mode, SwiftShader | 2D fixture passes; the 3D fixture stalls (the known, accepted case, #609, `stall` as before, and the step passes on exactly that) | 12 s |
| `web-smoke`: Chromium, whatever the engine picks | pass, on WebGL2 | 4 s |
| `web-smoke`: **Firefox 159**, WebGL2 (2D and 3D fixtures; Apple GPU through WebGL2) | **pass**; it launches now | 13 s |
| `web-smoke`: Firefox, WebGPU; Firefox, whatever the engine picks | pass: WebGPU unsupported ("headless firefox has no WebGPU adapter here"); `auto` passes on WebGL2 | 4 s; 5 s |
| `web-smoke`: WebKit 27.2, WebGL2 (2D and 3D fixtures); WebGPU | pass; pass (drawn with WebGPU) | 15 s; 5 s |
| `web-smoke`: WebKit audio: zero reported channels, rejected resume | **fail** (zero channels, in the first full run; it passed in three reruns before the fix), then pass after the fix below | 19 s; 13 s |
| `web-smoke`: self-tests (missing asset, startup panic, stalled simulation) | pass | 6 s |
| `web-smoke`: web gallery pages (Chromium, WebGL2) | pass | 1 min 6 s |
| `web-smoke`: Chromium: replication; video through the AV1 decoder | pass | 2 s; 27 s |
| `web-smoke`: Chromium: the guide's C++ game (`web/build-cpp.sh`, WASI SDK 34) | pass: started in 786 ms, 23 079 727 bytes of wasm | 4 min 37 s |
| `web-smoke`: Chromium: threaded web build (all eight cases) | pass | 7 min 20 s |
| `web-smoke`: Chromium: the water spectrum with and without WASM SIMD | pass | 4 min 1 s |
| `web-gpu`: WebGPU on the GPU, 2D and 3D fixtures (`--require-gpu`, Apple M4 Max, Metal) | pass | 16 s |
| `web-gpu`: WebGL2 on the GPU, 3D fixture (ANGLE on Metal) | pass | 5 s |
| `web-gpu`: frame times, 3D fixture, `frame-times.mjs --repeat 1` | pass: WebGPU median 0.23 ms, p95 4.33 ms; WebGL2 median 2.66 ms, p95 9.92 ms (1000 frames each, load 64) | 7 s |

Two things failed on the first pass with 1.65.1 and are fixed in the branch
(neither is the Playwright move itself):

- `runtime-failures.mjs`, the `device-lost` case, in the headless shell: since
  #609 the engine falls back to WebGL2 when a WebGPU canvas can't present
  (the shell's SwiftShader fallback adapter can't), so the device the runner
  destroyed was the abandoned test device and the game ran on. The case now
  needs the game to draw a WebGPU canvas and reads "unsupported" otherwise;
  the new headless mode, with SwiftShader or on the Metal adapter, still
  loses the game's device and passes.
- WebKit, zero-channel audio: once, in the first full run, the runner's fake of
  `maxChannelCount` (set on the context's destination object) was gone when it
  read the channel count back, because WebKit may hand out a new wrapper for
  `destination`; the runner sets it on the prototype now (six runs in a row
  pass).

The `web-gpu` 3D fixture on WebGPU stalled once in five runs at load 70 to
90 (2 to 26 ticks in 3 s, 60 wanted), then passed in all four reruns and in
the job's own run. Chromium 159's headless shell and new headless mode
otherwise behave as Chromium 156 did.

### Fuzz targets on the release revision (#528, section 3)

`fuzz/seed-corpus.sh`, then `fuzz/run-all.sh 60` (every target of
`fuzz/targets.txt`, 60 s each, nightly 1.102.0 (2026-11-24), cargo-fuzz 0.13.2, libFuzzer
with `-rss_limit_mb=2048 -timeout=10`) on `main` at `f308b7f6`
(the branch changes no engine code, only the web smoke runner). The machine was
busy (load 60 to 90), so the numbers of executions are low and say nothing
about the targets' speed. **No findings** (no crash, timeout, out-of-memory
or slow unit) in any of the 17 targets. The whole run took 32 min 27 s with
the builds.

| Target | Seconds | Executions | New units | Peak RSS |
|---|---|---|---|---|
| `scene` | 60 | 5 678 | 314 | 485 MB |
| `prefab` | 60 | 191 | 84 | 140 MB |
| `gltf` | 60 | 40 | 27 | 148 MB |
| `unity_yaml` | 60 | 883 | 156 | 114 MB |
| `unity_binary` | 60 | 48 141 | 448 | 311 MB |
| `action_map` | 60 | 487 | 108 | 101 MB |
| `asset_pack` | 60 | 17 695 | 169 | 353 MB |
| `lockstep` | 60 | 63 420 | 654 | 462 MB |
| `messages` | 60 | 3 170 | 201 | 272 MB |
| `replication` | 60 | 8 300 | 394 | 492 MB |
| `lobby` | 60 | 13 | 10 | 106 MB |
| `control` | 60 | 681 | 174 | 209 MB |
| `aseprite` | 60 | 777 | 118 | 202 MB |
| `levels` | 60 | 605 | 127 | 252 MB |
| `cooked_texture` | 60 | 23 | 7 | 102 MB |
| `sound` | 60 | 332 | 45 | 543 MB |
| `webm_audio` | 60 | 54 | 24 | 177 MB |

The fuzz build output and corpora were deleted afterwards (nothing of them is
committed).

## 2026-11-26: the three failing web steps again (#546)

The web steps that failed on 2026-11-25, run again on the branch that fixes
#546 (`main` at `20c5329c` plus the fix), on the same machine, toolchains and
browsers (Playwright 1.64.1: Chromium 156.0.8078.4, firefox-1555). The
machine was busy (load average 35 to 90 on 14 cores).

| Check | Command | Result |
|---|---|---|
| `web-smoke`: Chromium, whatever the engine picks | `node web/smoke/smoke.mjs --fixture web_smoke --browser chromium --backend auto --allow-unsupported` | **Passes** (3 runs of 3) on WebGL2. The headless shell's SwiftShader fallback adapter opens a device but can't present; the engine's test frame now catches it (`ponko: falling back to WebGL2: a canvas can't show WebGPU frames: a test frame failed: [Invalid Texture] is invalid due to a previous error.`) and the game draws with WebGL2. |
| `web-smoke`: Chromium, WebGPU first and each way back to WebGL2 | as written (new headless mode, SwiftShader), and on the Metal adapter | **Passes**, both: `webgpu` draws with WebGPU, the five failure cases (with the new `no-present`) end on WebGL2 with their one line. The engine before the fix fails `no-present` (it stays on WebGPU and the canvas is blank). |
| `web-smoke`: Chromium, WebGPU, new headless mode, 3D fixture | `node web/smoke/smoke.mjs --fixture web_smoke_3d --browser chromium --chromium-mode new --backend webgpu --browser-arg --use-webgpu-adapter=swiftshader --allow-unsupported` | **Expected to fail on macOS** (`stall`); the step in `scripts/checks.yml` accepts this one failure there and nothing else, and passes (the 2D fixture passes). Not the engine's: see below. |
| `web-smoke`: Firefox, WebGL2 and WebGPU | `node web/smoke/smoke.mjs --fixture web_smoke --browser firefox --backend webgl2 --allow-unsupported` (and the others) | **Fails to launch**, as before. Cause: the Firefox build of the pinned Playwright, see below. |
| `web-gpu` (#568): WebGPU on the GPU, 2D and 3D; WebGL2 on the GPU, 3D; frame times | the job's three steps | **Pass**: both fixtures draw with WebGPU on the Apple M4 Max (`--require-gpu`), the 3D fixture with WebGL2 on ANGLE's Metal renderer, and `frame-times.mjs --repeat 1` (median frame 0.20 ms on WebGPU, 1.11 ms on WebGL2, uncapped, at load 56). |
| `web`: Clippy for wasm32 | `cargo clippy -p ponko --target wasm32-unknown-unknown --examples`; `--lib --example web_smoke -- -D warnings`; `--features 3d --examples`; `--features editor,3d -- -D warnings` (with `RUSTFLAGS=-D warnings`) | pass |

**Why the 3D fixture stalls on SwiftShader.** The engine is not stuck. Over
25 s the page's main thread is blocked for 5 to 20 s after the first 4
frames (a `page.evaluate` takes that long to answer), then the game runs on
at about 3 frames a second with the simulation ticking. The block is
shorter on a second load in the same browser (19, 8, then 4.5 s) and
varies with the machine's load. A page without the engine that draws an
800 × 600 canvas each animation frame on the same adapter does the same once
its frames are slow (a fragment shader that keeps SwiftShader busy for
about a second a frame): 3 frames, then a 4.5 s block. With a light shader
it runs at 30 frames a second with no block. So it is SwiftShader, a CPU
renderer, on a machine with other builds running, and Chromium holding the
page while the GPU process is behind on the first 3D frames. The runner
reads that as a stalled simulation. The same step passes on the Metal
adapter. The engine's pacing is the same on both: in a browser it draws
only when winit asks for a redraw, which is the browser's
`requestAnimationFrame` (`src/game/window.rs`, `unpaced` is never true on
wasm32).

**Why Playwright's Firefox doesn't start.** Playwright 1.64.1's Firefox
(Firefox 157.0 Nightly, `firefox-1555`) exits at once on macOS 27.2.1 with
"Could not find profile folder." for every profile folder it is given: new
or used, absolute or relative, inside or outside the agent's command sandbox,
with or without Playwright. The installed Firefox 159.0 starts headless with
the same arguments. Playwright 1.65.1's Firefox (159.0, `firefox-1572`)
starts too: run through `smoke.mjs` with it installed, the 2D and 3D
fixtures pass on WebGL2, `--backend auto` passes on WebGL2 and WebGPU is
reported unsupported (no adapter in headless Firefox). The runner is fine;
moving the lockfile to 1.65.1 also moves Chromium and WebKit, so it needs
the whole `web-smoke` job run again and is left to its own issue (#601).

**Start-up cost of the test frame.** From the device opening to the game's
canvas getting its context (3D fixture, new headless mode, two runs of seven
fresh loads, load 55 to 65): Metal 11 ms before, 11 to 12 ms after;
SwiftShader 10 to 11 ms before, 16 ms after. Times to the first frame moved
by more between loads of the same build than that.

## 2026-11-25: `main` at `de9557f5`

**Machine.** Apple M4 Max (14 CPU cores, 32 GPU cores, 36 GB), macOS 27.2.1
(26B131), Xcode 27.2, Apple clang 21.0.0, CMake 4.5.1. GPU adapter: Apple M4
Max through wgpu's Metal backend (the render tests' reference images were made
on the same adapter and backend). The machine was busy: other builds ran
beside these checks (load average 20 to 55 on 14 cores).

**Toolchains.** Rust 1.100.0 (stable), nightly 1.102.0 (2026-11-19) for the
sanitizers, Miri and the threaded web build, nightly-2026-11-06 for the API
listing, 1.95 for the MSRV check, 1.99.1 for the semver check;
cargo-semver-checks 0.51.0, cargo-public-api 0.52.0, cargo-deny 0.20.2,
cbindgen 0.29.2, wasm-bindgen 0.2.130, mdBook 0.5.4; Node 26.13.1 with
Playwright's Chromium 156.0.8078.4 (chromium-1248), Firefox (firefox-1555) and
WebKit 27 (webkit-2370); WASI SDK 34 (arm64-macos); SDL 3.4.20 from Homebrew.

**Result.** Everything that applies to macOS and runs without a person passes
on the branch that fixes what failed (pull request "macOS: run the list of
checks", the fixes listed below), except three web steps, listed under
"Failing". Times are wall-clock on the busy machine. The core part ran again
on the final head of the pull request, `main` at `203151c4` merged with the
fixes: every step
passed, with 11 307 tests passed, 0 failed and 69 ignored (the 68 below and
one new benchmark, `render::auto_exposure_render_test::auto_exposure_cost`),
and the C and C++ tests 15 of 15.

### Fixed in the same pull request

These failed on `de9557f5` and pass with the fix.

| Check | What failed | Fix |
|---|---|---|
| `fmt`: Release scripts (`release-check.py selftest`) | `ponko_aseprite` and `ponko_lobby` had no `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` | copies of the root files; fixed on `main` meanwhile (#533) |
| `fmt`: C ABI test coverage (`check-ffi-coverage.py`) | the maturity line of `web-threads.md` ("nightly Rust only") matched the Rust-only pattern | reworded; fixed on `main` the same way meanwhile (#541) |
| `api-snapshots` (`api-snapshots.py check`) | `World::insert_many`, `World::remove_many` missing; joystick items out of order | regenerated `api/`; fixed on `main` meanwhile (#538), and again after #548 (#560) |
| `test-features`: engine with every image format | `assets::group::tests::without_workers_other_assets_load_as_before` expected a GIF to be unsupported, which it is only without `image-gif` | the test expects `Decode` with the decoder, `Unsupported` without |
| `docs`: every public feature | the step lacked `mods-wasm`, which docs.rs builds; the code-mod links in `ponko::mods` broke | `mods-wasm` added to the step |
| `native`: `ctest` (Release) | `cpp_wrapper` aborted: clang removed the `new`/`delete` pair the allocation count measures | the pointers go through a volatile sink |
| `sanitizers`: the nightly build | nightly 1.102 reports `recursion_depth_exceeding_limit` proving the texture tables `Sync` through wgpu; `-D warnings` failed the build | `#![recursion_limit = "256"]` in `ponko` |
| `sanitizers`: `ponko_ffi` tests, ASan | `lobby::a_c_game_makes_joins_and_plays_in_a_room` read the leaver's events before the server's `Left` reached it | the test waits for it |
| `web`: Clippy, wasm-simd with simd128 | with the list's `RUSTFLAGS` set, Cargo ignored `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS`, so the feature refused to build | the step passes `+simd128` in `RUSTFLAGS` |

One more, a reporting fix: `tests/ui_access_macos.rs` printed its skipped
window check as "ok" and counted it in "12 passed"; it now prints "11 passed;
1 skipped".

### Failing

| Check | Command | Result |
|---|---|---|
| `web-smoke`: Chromium, whatever the engine picks | `node web/smoke/smoke.mjs --fixture web_smoke --browser chromium --backend auto --allow-unsupported` | **Fails.** The headless shell offers a SwiftShader fallback adapter that can't present to a canvas; the engine starts on WebGPU and the device is lost on the first frame, instead of falling back to WebGL2. Filed: #546. |
| `web-smoke`: Firefox, WebGL2 and WebGPU | `node web/smoke/smoke.mjs --fixture web_smoke --browser firefox --backend webgl2 --allow-unsupported` (and `web_smoke_3d`, and `--backend webgpu`) | **Fails to launch**: Playwright's Firefox (firefox-1555) exits at start in headless mode on this Mac. Known: `web/smoke/README.md`, "Not covered yet". |
| `web-smoke`: Chromium, WebGPU, new headless mode, 3D fixture | `node web/smoke/smoke.mjs --fixture web_smoke_3d --browser chromium --chromium-mode new --backend webgpu --browser-arg --use-webgpu-adapter=swiftshader --allow-unsupported` | **Fails** ("stall"), three runs alike: 4 frames, then none; Chromium logs `CVDisplayLinkCreateWithCGDisplay failed` and lost shared images. The step's SwiftShader adapter is the Linux runner's software stand-in; on this Mac's own adapter (the same command without `--browser-arg`) both fixtures draw with WebGPU and pass, as does the 2D fixture on SwiftShader. |

### Passed

The core part, one script (the quick checks of AGENTS.md and the full test
run; 30 min 26 s in all):

| Check | Command | Result |
|---|---|---|
| Format | `cargo fmt --all --check`; `cargo fmt --check --manifest-path bench/compare/Cargo.toml` | pass |
| Scripts | `check-feature-coverage.py`, `collect-panics.py --check`, `changelog.py --check`, `check-naming.py`, `project-url.py check`, `check-ffi-coverage.py`, `check-cpp-api.py`, `api-snapshots.py check`, `check-unsafe.py`, `check-ffi-ownership.py`, `check-fuzz-list.py` | pass (with the fixes above) |
| Dependencies | `cargo deny check` | pass |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings`; `-p ponko` with default features, `--no-default-features`, `--features 3d,basis-universal,gltf-meshopt`, `--features editor`, `--features webrtc,mods-wasm,physics`; `-p ponko_bench --features render` | pass |
| Examples | `cargo build --workspace --examples` | pass |
| Tests | `cargo test --workspace --no-fail-fast` (unit, integration and doc tests: the macOS line of the `test` job and more) | **11 122 passed, 0 failed, 68 ignored** (see "Skipped and ignored") in 444 test binaries and doc-test runs |
| C header | `cargo run -p ponko_ffi --example generate_header` compared with `include/ponko.h` | pass |
| C and C++, debug | `cmake -S examples/cpp -B target/native -DCMAKE_BUILD_TYPE=Debug -DPONKO_WEBRTC=ON ...`, `ctest` | 15 of 15 |
| Guide | `python3 scripts/check-book-snippets.py`; `mdbook build book` | pass |

The render tests ran on the Apple M4 Max with Metal: run again with
`PONKO_REQUIRE_GPU=1` and `--nocapture`, the engine's library tests printed no
`render test SKIPPED` line (869 render tests passed, 46 ignored benchmarks), and
239 image comparisons (`target/render-test-output/comparisons.log`) were within
their tolerances against references made on the same adapter.

The other jobs, one row per step:

| Job | Step | Result | Time |
|---|---|---|---|
| `leaks` | Files; commit messages | pass | 0 s |
| `fmt` | Line endings | pass | 0 s |
| `fmt` | Release scripts (after the fix) | pass | 1 s |
| `fmt` | C ABI test coverage (after the fix) | pass | 35 s |
| `fmt` | Changelog self-test | pass | 0 s |
| `fmt` | C ABI pointer ownership self-test | pass | 0 s |
| `fmt` | Unsafe inventory self-test | pass | 0 s |
| `reference-images` | The script's own checks; every image against its metadata; this branch's changes (none) | pass | 3 s |
| `feature-deps` | Reduced builds' dependencies, and the self-test | pass | 6 s |
| `test-features` | ECS without rayon | pass | 16 s |
| `test-features` | Engine with no default features | pass | 4 min 14 s |
| `test-features` | Command line tool feature subsets (6 builds) | pass | 1 min 9 s |
| `test-features` | 2D engine with JPEG only | pass | 58 s |
| `test-features` | Engine with every image format (after the fix) | pass | 1 min 22 s |
| `test-features` | Compressed textures | pass | 4 min 51 s |
| `test-features` | glTF with compressed geometry | pass | 1 min 7 s |
| `test-features` | Water spectrum with wasm-simd | pass | 1 min 30 s |
| `test-features` | Text shaping and system fonts | pass | 3 min 13 s |
| `test-features` | Editor | pass | 1 min 45 s |
| `test-features` | aseprite-source | pass | 38 s |
| `test-features` | SDL gamepad backend (macOS only; SDL 3.4.20) | pass | 48 s |
| `test-features` | Video | pass | 2 min 35 s |
| `test-features` | Lobby client and WebRTC endpoints | pass | 4 min 19 s |
| `test-features` | C library with optional features | pass | 1 min 17 s |
| `test-features` | C library with code mods | pass | 1 min 0 s |
| `test-features` | ECS with serde | pass | 3 s |
| `test-features` | Profiling feature | pass | 30 s |
| `test-features` | Engine with hotpatch | pass | 39 s |
| `test-features` | View systems in batches on threads (`PONKO_PARALLEL_SYSTEMS=always`) | pass | 30 s |
| `test-features` | Physics with cross-platform determinism, and the platformer | pass | 37 s |
| `test-features` | Steam transport | pass | 2 min 15 s |
| `test-features` | Accessibility adapter (the macOS `NSView` checks; the window check skipped, see below) | pass | 1 min 38 s |
| `test-features` | Clippy, accessibility adapter (macOS) | pass | 21 s |
| `clippy` | The matrix legs the core part doesn't cover (27 builds, each `-- -D warnings`) | pass | 6 min 37 s |
| `msrv` | `cargo +1.95 check --workspace --all-targets --features ponko/physics,ponko/steam` | pass | 39 s |
| `docs` | Workspace docs; every public feature (after the fix) | pass | 52 s |
| `semver` | Self-test; `semver-checks.py run` with Rust 1.99.1 (the crates.io comparison is the report; no break added against the base) | pass | 4 min 46 s |
| `api-snapshots` | Regenerate the listing and compare it with api/ (after the fix) | pass | 3 min 16 s |
| `api-snapshots` | Self-test and pins; one public path per item (`check-naming.py --selftest`, `--paths`) | pass | 37 s |
| `book` | Maturity levels and self-test | pass | 0 s |
| `native` | Generated C header (cbindgen) against `include/ponko.h` | pass | 2 s |
| `native` | Release library with `control-env,webrtc`; its exports | pass | 3 min 48 s |
| `native` | C and C++ tests, Release (after the fix): 15 of 15 | pass | 4 min 8 s |
| `native` | Validation bundle of the C++ battle | pass | 38 s |
| `sanitizers` | Nightly debug library, ASan and UBSan build (after the fix) | pass | 2 min 39 s |
| `sanitizers` | C and C++ test programs, ASan and UBSan: 15 of 15 | pass | 7 s |
| `sanitizers` | `ponko_ffi` tests, ASan (after the fix): 628 passed | pass | 5 min 5 s |
| `web` | Clippy and checks for wasm32 (examples, image formats, 3D, tutorials, Basis Universal, meshopt, accessibility, system fonts, editor, editor with 3D) | pass | 2 min 32 s |
| `web` | wasm-simd without simd128 is refused | pass | 24 s |
| `web` | Clippy, wasm-simd with simd128 (after the fix) | pass | 28 s |
| `web` | Threaded build, clippy (`web/build.sh --threads --clippy`, and with `3d,bench-internals`) | pass | 54 s |
| `web` | `web/build.sh battle` | pass | 1 min 43 s |
| `web-smoke-runner` | `npm ci`; `node --test web/smoke/*.test.mjs web/gallery/*.test.mjs` | pass | 2 s |
| `web-smoke` | Build the fixtures (2D, 3D, coin_dash, ember_hop, lights2d, the two editor fixtures, lockstep, net_arena) | pass | 5 min 14 s |
| `web-smoke` | Chromium, WebGL2: 2D fixture, 3D fixture, 2D lights | pass | 30 s |
| `web-smoke` | Chromium, WebGL2: editor saves (kept, persistent storage granted, refused); a glTF scene that didn't load and its Retry | pass | 21 s |
| `web-smoke` | Chromium, WebGL2: coin_dash | pass | 5 s |
| `web-smoke` | Chromium, multiplayer (two tabs, two native players) | pass | 13 s |
| `web-smoke` | Chromium, failures after startup; key labels | pass | 7 s |
| `web-smoke` | Chromium, a lost WebGPU device (new headless mode; as written, with SwiftShader, and on the Metal adapter) | pass | 8 s |
| `web-smoke` | Chromium, WebGPU first and each way back to WebGL2 (as written, and on the Metal adapter) | pass | 16 s |
| `web-smoke` | Chromium, the water spectrum with and without WASM SIMD | pass | 3 min 18 s |
| `web-smoke` | Chromium, video through the browser's AV1 decoder | pass | 1 min 11 s |
| `web-smoke` | Chromium, the guide's C++ game (WASI SDK 34, arm64-macos) | pass | 2 min 59 s |
| `web-smoke` | Chromium, threaded web build (all eight cases) | pass | 5 min 43 s |
| `web-smoke` | Chromium, replication | pass | 2 s |
| `web-smoke` | Chromium, WebGPU, headless shell (reported unsupported: no canvas) | pass | 2 s |
| `web-smoke` | Chromium, WebGPU, new headless mode on the Metal adapter (2D and 3D fixtures, drawn with WebGPU) | pass | 10 s |
| `web-smoke` | WebKit, WebGL2 (2D and 3D fixtures); WebKit, WebGPU (drawn with WebGPU) | pass | 15 s |
| `web-smoke` | Self-tests (missing asset, startup panic, stalled simulation) | pass | 5 s |
| `web-smoke` | Web gallery pages (Chromium, WebGL2) | pass | 1 min 3 s |
| `package-smoke` | The script's own checks | pass | 0 s |
| `package-smoke` | Package; smoke test, headless, from an unrelated folder | pass | 45 s |
| `package-smoke` | The host's download (an app in a dmg) with `ponko package`'s own smoke run | pass | 10 s |
| `package-smoke` | The platformer's download, and its walk-through from the package, headless (both levels, a save and a load, the progress file) | pass | 1 min 26 s |
| `validation-bundles` | `bash scripts/validation-bundles.sh target/validation-bundles` | pass | 20 s |
| `templates` | The CLI's tests, with the three ignored template tests | pass | 5 min 39 s |
| `templates` | Each template's own tests (2d, 3d, headless, server) | pass | 2 min 29 s |
| `miri` | ECS, stacked borrows, 1/3, 2/3, 3/3 | pass | 29 min 18 s |
| `miri` | ECS parallel path, tree borrows | pass | 15 min 44 s |
| `miri` | Engine unsafe code, tree borrows (`check-unsafe.py --miri`) | pass | 8 min 21 s |

### Needs a person

These open a window or need a service a person starts. The exact commands:

| Check | Command | Why |
|---|---|---|
| `package-smoke`: Smoke test, in a window | `python3 scripts/package-coin-dash.py smoke --package target/package --mode window` (after the `package` step) | opens the game's window |
| `package-smoke`: the platformer's walk-through, in a window | from an unrelated folder: `PONKO_DATA_DIR=<empty folder> <target/package-ember/ember-hop-*/…/ember_hop> --smoke window --report report.json`, then the step's `report.json` check | opens the game's window |
| `test-features`: accessibility, the engine's own window | `cargo test -p ponko --features accessibility --test ui_access_window -- --ignored`; `PONKO_ACCESS_WINDOW=1 cargo test -p ponko --features accessibility --test ui_access_macos` | opens a window (built here, not run) |
| `server-container` | `cargo build -p ponko_cli --no-default-features`, `ponko new game-server --server --path .`, `docker run --rm -i hadolint/hadolint:v2.12.0 < game-server/Dockerfile`, then the job's package, `docker build` and run steps | the Docker daemon was not running; starting Docker Desktop opens its app |

### Not run here

- Linux and Windows only: `windows-unit-build`, `windows-unit`,
  `windows-gpu-tiers`, `windows-examples`; the native crash backtraces with
  frame pointers (Linux); the AT-SPI adapter tests (Linux); the PulseAudio
  sound server for WebKit (Linux). `ui_access_platforms` holds no tests on
  macOS (its adapters are the Windows and Linux ones).
- GitHub only: the reference images' pull request label and artifact steps,
  and the leak check of a pull request's title and description (run with
  `scripts/leak-check.sh text` when the pull request is opened).

### Skipped and ignored

No test skipped for lack of a GPU or a GPU feature (above). The 68 ignored
tests of `cargo test --workspace` are ignored on purpose, each with its reason
in the code:

- 54 benchmarks and measurements (55 on the final head; `render::*_cost`, `*_benchmark`,
  `*_gpu_time`, `water::spectrum` timings and the like; "run in release" or
  "run by hand"), which assert nothing about time;
- 10 that write fixture files (`write_*_fixture`, `write_the_committed_replay`);
- 3 template builds (`ponko_cli` `tests/new.rs`, `tests/package.rs`,
  `tests/server.rs`), which the `templates` job runs with `--ignored`: passed
  here;
- 1 that loses its Metal device on purpose and runs alone
  (`render::query_sets::tests::metal_allows_32_timestamp_query_sets_per_process_of_any_size`):
  run alone with `--ignored --exact`, passed;
- 1 ignored doc example (`alloc_count::CountingAllocator`).

`tests/ui_access_macos.rs` skips its window check (above).
