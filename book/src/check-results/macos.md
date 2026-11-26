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
checks", nine fixes listed below), except three web steps, listed under
"Failing". Times are wall-clock on the busy machine.

### Fixed in the same pull request

These failed on `de9557f5` and pass with the fix.

| Check | What failed | Fix |
|---|---|---|
| `fmt`: Release scripts (`release-check.py selftest`) | `ponko_aseprite` and `ponko_lobby` had no `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` | copies of the root files |
| `fmt`: C ABI test coverage (`check-ffi-coverage.py`) | the maturity line of `web-threads.md` ("nightly Rust only") matched the Rust-only pattern | reworded; fixed on `main` the same way meanwhile (#541) |
| `api-snapshots` (`api-snapshots.py check`) | `World::insert_many`, `World::remove_many` missing; joystick items out of order | regenerated `api/` |
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

- 54 benchmarks and measurements (`render::*_cost`, `*_benchmark`,
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
