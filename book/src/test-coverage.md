# Workspace test coverage

<!-- Maturity: not a feature. How test coverage is measured. -->

Coverage records which Rust source lines the tests execute. It includes the
headless and offscreen GPU tests in the default workspace suite and the
doctests. Run it on a machine where the offscreen tests can acquire a GPU
adapter, and check the test log for adapter skips before recording a baseline.

```sh
rustup component add llvm-tools-preview --toolchain nightly
cargo install --locked cargo-llvm-cov --version 0.9.2
cargo +nightly llvm-cov --workspace --doctests --json --output-path target/coverage.json
python3 scripts/coverage-report.py target/coverage.json
```

The Rust compiler guide explains the instrumentation. The cargo-llvm-cov
documentation describes nightly doctest support. [Rust compiler guide](https://doc.rust-lang.org/rustc/instrument-coverage.html) · [cargo-llvm-cov documentation](https://github.com/taiki-e/cargo-llvm-cov)

The table groups mapped Rust lines by crate and top-level source module.
It excludes integration tests, examples and separate `*_test.rs` files.
Inline tests can contribute lines when LLVM maps them in the source file.
The total divides executed lines by mapped lines across the workspace.
Source compiled out on the host or by the selected features is absent from
the denominator. Record the revision, toolchain, OS and GPU adapter with
each baseline. Keep the raw export in `target/`.

## Public functions

Build rustdoc JSON with the default features for the workspace's Rust
libraries, then pass those files alongside the coverage export.

```sh
for crate in ponko ponko_aseprite ponko_ecs ponko_ffi ponko_image_diff ponko_lobby ponko_mcp ponko_unity; do
    cargo +nightly rustdoc --lib -p "$crate" -- -Z unstable-options --output-format json || exit
done
python3 scripts/coverage-report.py target/coverage.json --rustdoc target/doc/ponko*.json
```

The inventory includes public free functions, public inherent methods and
public trait defaults with bodies. It uses rustdoc's public paths and LLVM's
function counts. Any executed generic instantiation counts as evidence for
that source function. An **unexecuted** body has a mapped entry with zero
executions. An **unmapped** body has no matching entry. That can happen when
a generic function has no compiled instantiation or the source is emitted
by a macro. Inspect the source and any platform-specific uses before using
either result in an API stability decision.


The inventory at `01718c9b` contains 126 unexecuted bodies and 14 unmapped
bodies. [Public API inventory](https://github.com/ponkoengine/ponko/blob/main/scripts/coverage-unexecuted-api.md)

## Baseline

Measured on the 0.2.0-rc.1 candidate (`main` at `de9ff614` with the
version bump; #703 to #705 since changed only WGSL and tests) on
2026-11-27, on macOS 27.2.1
with an Apple M4 Max running Metal, Rust `1.102.0-nightly (739adecd4
2026-11-24)`, cargo-llvm-cov `0.9.2`, and `RUST_TEST_THREADS=4`. The
instrumented workspace run passed 11,690 tests, including headless tests,
offscreen GPU tests and doctests, with no render test skipped for want of
an adapter. The 74 tests ignored by the default workspace suite remain
outside the run. The previous baseline, at `01718c9b`, was 90.0% (230,846
of 256,615 lines).

| Module | Lines executed | Lines mapped | Coverage |
|---|---:|---:|---:|
| `ponko` | 12 | 34 | 35.3% |
| `ponko::access` | 483 | 503 | 96.0% |
| `ponko::actions` | 436 | 455 | 95.8% |
| `ponko::ai` | 1,765 | 1,861 | 94.8% |
| `ponko::alloc_count` | 175 | 175 | 100.0% |
| `ponko::anim` | 226 | 238 | 95.0% |
| `ponko::aseprite` | 412 | 463 | 89.0% |
| `ponko::asset_cache` | 56 | 56 | 100.0% |
| `ponko::assets` | 3,662 | 3,942 | 92.9% |
| `ponko::attach` | 178 | 179 | 99.4% |
| `ponko::audio` | 5,139 | 5,593 | 91.9% |
| `ponko::camera` | 1,511 | 1,650 | 91.6% |
| `ponko::camera2d` | 135 | 171 | 78.9% |
| `ponko::color` | 179 | 182 | 98.4% |
| `ponko::console` | 383 | 445 | 86.1% |
| `ponko::control` | 4,244 | 4,478 | 94.8% |
| `ponko::crash` | 1,541 | 1,756 | 87.8% |
| `ponko::ctx` | 950 | 1,205 | 78.8% |
| `ponko::d2` | 13 | 13 | 100.0% |
| `ponko::d3` | 11 | 11 | 100.0% |
| `ponko::debug` | 1,130 | 1,241 | 91.1% |
| `ponko::diagnostics` | 307 | 316 | 97.2% |
| `ponko::draw_space` | 7 | 7 | 100.0% |
| `ponko::ecs` | 66 | 87 | 75.9% |
| `ponko::edit2d` | 312 | 315 | 99.0% |
| `ponko::edit3d` | 589 | 601 | 98.0% |
| `ponko::editor` | 1,216 | 1,321 | 92.1% |
| `ponko::editor_document` | 360 | 397 | 90.7% |
| `ponko::error` | 130 | 135 | 96.3% |
| `ponko::events` | 616 | 627 | 98.2% |
| `ponko::flow` | 226 | 230 | 98.3% |
| `ponko::frame_pool` | 24 | 24 | 100.0% |
| `ponko::game` | 2,936 | 3,982 | 73.7% |
| `ponko::gamepad` | 55 | 153 | 35.9% |
| `ponko::gamepad_sdl` | 1,186 | 1,228 | 96.6% |
| `ponko::gamepad_web` | 460 | 462 | 99.6% |
| `ponko::gfx` | 787 | 927 | 84.9% |
| `ponko::handle` | 565 | 578 | 97.8% |
| `ponko::hierarchy` | 1,091 | 1,144 | 95.4% |
| `ponko::hot_reload` | 132 | 135 | 97.8% |
| `ponko::input` | 5,134 | 5,586 | 91.9% |
| `ponko::inspector` | 1,188 | 1,434 | 82.8% |
| `ponko::inspector_debug` | 205 | 211 | 97.2% |
| `ponko::inspector_keys` | 203 | 230 | 88.3% |
| `ponko::inspector_scene` | 945 | 1,185 | 79.7% |
| `ponko::inventory` | 374 | 404 | 92.6% |
| `ponko::jobs` | 242 | 246 | 98.4% |
| `ponko::light2d` | 583 | 625 | 93.3% |
| `ponko::locale` | 869 | 912 | 95.3% |
| `ponko::logs` | 182 | 200 | 91.0% |
| `ponko::math` | 9 | 9 | 100.0% |
| `ponko::mip_limit` | 60 | 60 | 100.0% |
| `ponko::mods` | 1,062 | 1,150 | 92.3% |
| `ponko::nav3d` | 1,578 | 1,637 | 96.4% |
| `ponko::net` | 13,236 | 13,821 | 95.8% |
| `ponko::particles` | 16 | 16 | 100.0% |
| `ponko::particles2d` | 725 | 752 | 96.4% |
| `ponko::perf` | 231 | 373 | 61.9% |
| `ponko::perf_report` | 1,004 | 1,101 | 91.2% |
| `ponko::physics` | 1,179 | 1,208 | 97.6% |
| `ponko::physics3d` | 3,887 | 4,121 | 94.3% |
| `ponko::physics3d_moved` | 12 | 18 | 66.7% |
| `ponko::physics_links` | 142 | 153 | 92.8% |
| `ponko::prelude` | 8 | 8 | 100.0% |
| `ponko::profiler` | 434 | 591 | 73.4% |
| `ponko::query` | 14 | 14 | 100.0% |
| `ponko::random` | 169 | 184 | 91.8% |
| `ponko::reflect` | 3,783 | 4,120 | 91.8% |
| `ponko::render` | 38,181 | 42,044 | 90.8% |
| `ponko::residency` | 261 | 272 | 96.0% |
| `ponko::scenario` | 1,166 | 1,213 | 96.1% |
| `ponko::scene` | 674 | 736 | 91.6% |
| `ponko::schedule` | 760 | 783 | 97.1% |
| `ponko::schema` | 159 | 159 | 100.0% |
| `ponko::scope` | 202 | 204 | 99.0% |
| `ponko::server` | 383 | 401 | 95.5% |
| `ponko::sheet` | 131 | 166 | 78.9% |
| `ponko::skeleton2d` | 5,964 | 6,691 | 89.1% |
| `ponko::spatial` | 371 | 387 | 95.9% |
| `ponko::sprite_material` | 270 | 316 | 85.4% |
| `ponko::state` | 516 | 527 | 97.9% |
| `ponko::storage` | 716 | 774 | 92.5% |
| `ponko::streaming` | 1,086 | 1,166 | 93.1% |
| `ponko::suggest` | 36 | 37 | 97.3% |
| `ponko::system_view` | 276 | 285 | 96.8% |
| `ponko::systems` | 48 | 56 | 85.7% |
| `ponko::terrain` | 4 | 5 | 80.0% |
| `ponko::text` | 1,107 | 1,163 | 95.2% |
| `ponko::texture` | 114 | 141 | 80.9% |
| `ponko::three` | 30,834 | 33,577 | 91.8% |
| `ponko::tilemap` | 1,922 | 2,169 | 88.6% |
| `ponko::time` | 413 | 466 | 88.6% |
| `ponko::ui` | 11,318 | 11,810 | 95.8% |
| `ponko::water` | 2,255 | 2,516 | 89.6% |
| `ponko::web_rumble` | 272 | 275 | 98.9% |
| `ponko::window` | 656 | 727 | 90.2% |
| `ponko_aseprite` | 164 | 175 | 93.7% |
| `ponko_aseprite::render` | 361 | 409 | 88.3% |
| `ponko_aseprite::tiles` | 87 | 93 | 93.5% |
| `ponko_bench` | 57 | 1,236 | 4.6% |
| `ponko_bench::actions` | 0 | 53 | 0.0% |
| `ponko_bench::ai` | 0 | 204 | 0.0% |
| `ponko_bench::alloc` | 82 | 188 | 43.6% |
| `ponko_bench::archetypes` | 136 | 217 | 62.7% |
| `ponko_bench::bin` | 966 | 1,680 | 57.5% |
| `ponko_bench::debug_draw` | 0 | 76 | 0.0% |
| `ponko_bench::determinism` | 0 | 70 | 0.0% |
| `ponko_bench::ecs_suite` | 647 | 861 | 75.1% |
| `ponko_bench::extraction` | 0 | 108 | 0.0% |
| `ponko_bench::frame2d` | 0 | 131 | 0.0% |
| `ponko_bench::gltf_scenes` | 0 | 123 | 0.0% |
| `ponko_bench::hooks` | 59 | 109 | 54.1% |
| `ponko_bench::membership` | 284 | 768 | 37.0% |
| `ponko_bench::mods` | 0 | 123 | 0.0% |
| `ponko_bench::nav3d` | 0 | 69 | 0.0% |
| `ponko_bench::net` | 0 | 51 | 0.0% |
| `ponko_bench::own` | 499 | 522 | 95.6% |
| `ponko_bench::par_archetypes` | 0 | 86 | 0.0% |
| `ponko_bench::particles2d` | 0 | 50 | 0.0% |
| `ponko_bench::physics` | 0 | 175 | 0.0% |
| `ponko_bench::physics3d` | 0 | 400 | 0.0% |
| `ponko_bench::query_cache` | 0 | 65 | 0.0% |
| `ponko_bench::query_filters` | 73 | 141 | 51.8% |
| `ponko_bench::query_policy` | 294 | 473 | 62.2% |
| `ponko_bench::render_report` | 1,426 | 1,450 | 98.3% |
| `ponko_bench::report` | 240 | 269 | 89.2% |
| `ponko_bench::required` | 68 | 136 | 50.0% |
| `ponko_bench::rollback` | 0 | 432 | 0.0% |
| `ponko_bench::schedule` | 0 | 187 | 0.0% |
| `ponko_bench::scoreboard` | 60 | 136 | 44.1% |
| `ponko_bench::skeleton2d` | 0 | 165 | 0.0% |
| `ponko_bench::sparse_set` | 616 | 803 | 76.7% |
| `ponko_bench::sprites` | 0 | 76 | 0.0% |
| `ponko_bench::streaming` | 83 | 206 | 40.3% |
| `ponko_bench::text` | 0 | 45 | 0.0% |
| `ponko_bench::tilemap` | 0 | 45 | 0.0% |
| `ponko_bench::ui_draw` | 0 | 92 | 0.0% |
| `ponko_bench::ui_list` | 0 | 45 | 0.0% |
| `ponko_cli` | 125 | 135 | 92.6% |
| `ponko_cli::cook` | 3,128 | 3,648 | 85.7% |
| `ponko_cli::editor` | 274 | 291 | 94.2% |
| `ponko_cli::image_diff` | 38 | 41 | 92.7% |
| `ponko_cli::new` | 329 | 348 | 94.5% |
| `ponko_cli::pack` | 653 | 688 | 94.9% |
| `ponko_cli::package` | 3,528 | 4,761 | 74.1% |
| `ponko_cli::schema` | 64 | 81 | 79.0% |
| `ponko_cli::templates` | 164 | 167 | 98.2% |
| `ponko_cli::textures` | 741 | 822 | 90.1% |
| `ponko_ecs` | 17 | 17 | 100.0% |
| `ponko_ecs::access` | 54 | 54 | 100.0% |
| `ponko_ecs::borrow` | 170 | 172 | 98.8% |
| `ponko_ecs::bundle` | 44 | 44 | 100.0% |
| `ponko_ecs::change` | 720 | 738 | 97.6% |
| `ponko_ecs::commands` | 298 | 299 | 99.7% |
| `ponko_ecs::entity` | 454 | 460 | 98.7% |
| `ponko_ecs::hooks` | 381 | 388 | 98.2% |
| `ponko_ecs::par` | 58 | 61 | 95.1% |
| `ponko_ecs::query` | 2,483 | 2,528 | 98.2% |
| `ponko_ecs::raw` | 153 | 156 | 98.1% |
| `ponko_ecs::required` | 65 | 66 | 98.5% |
| `ponko_ecs::slots` | 173 | 177 | 97.7% |
| `ponko_ecs::snapshot` | 364 | 381 | 95.5% |
| `ponko_ecs::storage` | 634 | 661 | 95.9% |
| `ponko_ecs::view` | 824 | 846 | 97.4% |
| `ponko_ecs::world` | 1,638 | 1,679 | 97.6% |
| `ponko_ffi` | 868 | 892 | 97.3% |
| `ponko_ffi::access` | 224 | 228 | 98.2% |
| `ponko_ffi::action_lookup` | 168 | 179 | 93.9% |
| `ponko_ffi::actions` | 476 | 480 | 99.2% |
| `ponko_ffi::ai` | 535 | 547 | 97.8% |
| `ponko_ffi::ai_columns` | 318 | 319 | 99.7% |
| `ponko_ffi::ai_world` | 377 | 378 | 99.7% |
| `ponko_ffi::animator` | 712 | 723 | 98.5% |
| `ponko_ffi::aseprite` | 53 | 59 | 89.8% |
| `ponko_ffi::asset_groups` | 343 | 351 | 97.7% |
| `ponko_ffi::asset_sources` | 206 | 212 | 97.2% |
| `ponko_ffi::attach` | 21 | 21 | 100.0% |
| `ponko_ffi::attach3d` | 12 | 12 | 100.0% |
| `ponko_ffi::audio3d` | 315 | 315 | 100.0% |
| `ponko_ffi::audio_curves` | 127 | 129 | 98.4% |
| `ponko_ffi::audio_mixer` | 490 | 501 | 97.8% |
| `ponko_ffi::audio_music` | 558 | 564 | 98.9% |
| `ponko_ffi::buffer_motion` | 22 | 22 | 100.0% |
| `ponko_ffi::builtins` | 72 | 87 | 82.8% |
| `ponko_ffi::camera_rigs` | 975 | 986 | 98.9% |
| `ponko_ffi::changes` | 298 | 298 | 100.0% |
| `ponko_ffi::collision_ex` | 532 | 555 | 95.9% |
| `ponko_ffi::collision_files` | 37 | 40 | 92.5% |
| `ponko_ffi::component_hooks` | 287 | 301 | 95.3% |
| `ponko_ffi::console` | 131 | 153 | 85.6% |
| `ponko_ffi::contacts3d` | 330 | 331 | 99.7% |
| `ponko_ffi::controllers` | 669 | 708 | 94.5% |
| `ponko_ffi::crash` | 131 | 136 | 96.3% |
| `ponko_ffi::crowd` | 261 | 290 | 90.0% |
| `ponko_ffi::crowd_clips` | 114 | 120 | 95.0% |
| `ponko_ffi::ctx` | 1,118 | 1,155 | 96.8% |
| `ponko_ffi::debug` | 556 | 562 | 98.9% |
| `ponko_ffi::descriptors` | 461 | 463 | 99.6% |
| `ponko_ffi::descriptors_render` | 734 | 738 | 99.5% |
| `ponko_ffi::diagnostics` | 25 | 26 | 96.2% |
| `ponko_ffi::dimensions` | 110 | 123 | 89.4% |
| `ponko_ffi::filters` | 184 | 184 | 100.0% |
| `ponko_ffi::flow` | 172 | 172 | 100.0% |
| `ponko_ffi::game` | 548 | 586 | 93.5% |
| `ponko_ffi::gltf_scenes` | 182 | 186 | 97.8% |
| `ponko_ffi::gpu` | 168 | 177 | 94.9% |
| `ponko_ffi::grid` | 146 | 149 | 98.0% |
| `ponko_ffi::handles` | 334 | 363 | 92.0% |
| `ponko_ffi::hdr_output` | 130 | 144 | 90.3% |
| `ponko_ffi::headless_ui` | 89 | 93 | 95.7% |
| `ponko_ffi::hierarchy` | 165 | 168 | 98.2% |
| `ponko_ffi::image_limits` | 146 | 148 | 98.6% |
| `ponko_ffi::ime` | 349 | 356 | 98.0% |
| `ponko_ffi::instance_cells` | 77 | 77 | 100.0% |
| `ponko_ffi::key_labels` | 89 | 89 | 100.0% |
| `ponko_ffi::keys` | 169 | 171 | 98.8% |
| `ponko_ffi::levels` | 537 | 552 | 97.3% |
| `ponko_ffi::lightmap_bake` | 444 | 462 | 96.1% |
| `ponko_ffi::lights2d` | 289 | 291 | 99.3% |
| `ponko_ffi::loaders` | 81 | 81 | 100.0% |
| `ponko_ffi::lobby` | 311 | 332 | 93.7% |
| `ponko_ffi::locale` | 306 | 316 | 96.8% |
| `ponko_ffi::lod` | 128 | 128 | 100.0% |
| `ponko_ffi::media` | 448 | 453 | 98.9% |
| `ponko_ffi::messages` | 409 | 415 | 98.6% |
| `ponko_ffi::mod_code` | 96 | 136 | 70.6% |
| `ponko_ffi::mods` | 172 | 176 | 97.7% |
| `ponko_ffi::morph` | 243 | 262 | 92.7% |
| `ponko_ffi::motion` | 18 | 18 | 100.0% |
| `ponko_ffi::nav3d` | 373 | 400 | 93.2% |
| `ponko_ffi::nav_view` | 104 | 110 | 94.5% |
| `ponko_ffi::net` | 163 | 174 | 93.7% |
| `ponko_ffi::particles` | 258 | 260 | 99.2% |
| `ponko_ffi::particles2d` | 237 | 245 | 96.7% |
| `ponko_ffi::perf` | 398 | 417 | 95.4% |
| `ponko_ffi::physics` | 482 | 497 | 97.0% |
| `ponko_ffi::physics3d` | 960 | 1,017 | 94.4% |
| `ponko_ffi::physics3d_stream` | 337 | 340 | 99.1% |
| `ponko_ffi::physics_pause` | 57 | 57 | 100.0% |
| `ponko_ffi::prediction` | 404 | 420 | 96.2% |
| `ponko_ffi::prefab` | 42 | 43 | 97.7% |
| `ponko_ffi::pressed` | 80 | 81 | 98.8% |
| `ponko_ffi::queries2d` | 542 | 545 | 99.4% |
| `ponko_ffi::queries3d` | 242 | 244 | 99.2% |
| `ponko_ffi::query_cache` | 171 | 171 | 100.0% |
| `ponko_ffi::ragdoll` | 135 | 135 | 100.0% |
| `ponko_ffi::reflect` | 168 | 209 | 80.4% |
| `ponko_ffi::replicate` | 522 | 599 | 87.1% |
| `ponko_ffi::replication` | 466 | 485 | 96.1% |
| `ponko_ffi::replication_raw` | 342 | 350 | 97.7% |
| `ponko_ffi::required` | 39 | 39 | 100.0% |
| `ponko_ffi::residency` | 369 | 399 | 92.5% |
| `ponko_ffi::resources` | 204 | 206 | 99.0% |
| `ponko_ffi::retarget` | 251 | 263 | 95.4% |
| `ponko_ffi::rollback` | 380 | 393 | 96.7% |
| `ponko_ffi::rtc` | 59 | 63 | 93.7% |
| `ponko_ffi::save_migrations` | 383 | 389 | 98.5% |
| `ponko_ffi::saves` | 69 | 69 | 100.0% |
| `ponko_ffi::scenario` | 432 | 440 | 98.2% |
| `ponko_ffi::scenario_touch` | 78 | 78 | 100.0% |
| `ponko_ffi::scene` | 63 | 63 | 100.0% |
| `ponko_ffi::scene_lighting` | 205 | 206 | 99.5% |
| `ponko_ffi::scene_passes` | 111 | 114 | 97.4% |
| `ponko_ffi::shader_status` | 128 | 132 | 97.0% |
| `ponko_ffi::skeleton2d` | 744 | 766 | 97.1% |
| `ponko_ffi::skin` | 453 | 457 | 99.1% |
| `ponko_ffi::sky_kit` | 234 | 240 | 97.5% |
| `ponko_ffi::slice_loaders` | 177 | 191 | 92.7% |
| `ponko_ffi::sounds_started` | 51 | 51 | 100.0% |
| `ponko_ffi::sprite_material` | 222 | 225 | 98.7% |
| `ponko_ffi::states` | 267 | 270 | 98.9% |
| `ponko_ffi::steam` | 93 | 96 | 96.9% |
| `ponko_ffi::steam_lobby` | 199 | 206 | 96.6% |
| `ponko_ffi::steam_services` | 212 | 228 | 93.0% |
| `ponko_ffi::stencil` | 143 | 158 | 90.5% |
| `ponko_ffi::storage` | 189 | 191 | 99.0% |
| `ponko_ffi::storage_slots` | 48 | 48 | 100.0% |
| `ponko_ffi::stream` | 363 | 366 | 99.2% |
| `ponko_ffi::strips` | 272 | 277 | 98.2% |
| `ponko_ffi::terrain_files` | 26 | 26 | 100.0% |
| `ponko_ffi::text3d` | 148 | 150 | 98.7% |
| `ponko_ffi::text_shaping` | 124 | 127 | 97.6% |
| `ponko_ffi::three` | 2,813 | 2,911 | 96.6% |
| `ponko_ffi::tiles` | 179 | 186 | 96.2% |
| `ponko_ffi::touch` | 484 | 500 | 96.8% |
| `ponko_ffi::touch_layout` | 319 | 324 | 98.5% |
| `ponko_ffi::types` | 136 | 137 | 99.3% |
| `ponko_ffi::ui_access` | 501 | 525 | 95.4% |
| `ponko_ffi::ui_animation` | 377 | 387 | 97.4% |
| `ponko_ffi::ui_focus_hud` | 222 | 240 | 92.5% |
| `ponko_ffi::ui_immediate` | 60 | 60 | 100.0% |
| `ponko_ffi::ui_panel3d` | 120 | 120 | 100.0% |
| `ponko_ffi::ui_retained` | 1,794 | 1,817 | 98.7% |
| `ponko_ffi::ui_widgets` | 1,043 | 1,070 | 97.5% |
| `ponko_ffi::vehicle3d` | 157 | 158 | 99.4% |
| `ponko_ffi::video` | 171 | 238 | 71.8% |
| `ponko_ffi::viewports` | 385 | 396 | 97.2% |
| `ponko_ffi::water` | 642 | 656 | 97.9% |
| `ponko_ffi::window` | 678 | 913 | 74.3% |
| `ponko_image_diff` | 237 | 242 | 97.9% |
| `ponko_lobby` | 857 | 925 | 92.6% |
| `ponko_lobby::load` | 170 | 192 | 88.5% |
| `ponko_lobby::proxy` | 89 | 90 | 98.9% |
| `ponko_lobby::stun` | 72 | 75 | 96.0% |
| `ponko_macros` | 112 | 123 | 91.1% |
| `ponko_macros::replicate` | 97 | 123 | 78.9% |
| `ponko_mcp` | 1,766 | 1,925 | 91.7% |
| `ponko_mcp::web_bridge` | 0 | 168 | 0.0% |
| `ponko_unity` | 177 | 185 | 95.7% |
| `ponko_unity::animation` | 903 | 961 | 94.0% |
| `ponko_unity::baked` | 371 | 416 | 89.2% |
| `ponko_unity::binary` | 425 | 453 | 93.8% |
| `ponko_unity::builtin` | 59 | 59 | 100.0% |
| `ponko_unity::fbx` | 677 | 734 | 92.2% |
| `ponko_unity::glb` | 391 | 398 | 98.2% |
| `ponko_unity::hierarchy` | 487 | 516 | 94.4% |
| `ponko_unity::humanoid` | 350 | 375 | 93.3% |
| `ponko_unity::import` | 1,063 | 1,274 | 83.4% |
| `ponko_unity::material` | 133 | 153 | 86.9% |
| `ponko_unity::mesh` | 324 | 382 | 84.8% |
| `ponko_unity::project` | 68 | 84 | 81.0% |
| `ponko_unity::terrain` | 67 | 69 | 97.1% |
| `ponko_unity::texture` | 88 | 109 | 80.7% |
| `ponko_unity::value` | 129 | 142 | 90.8% |
| **Total** | **246,559** | **273,433** | **90.2%** |

## Modules below 40%

The review uses 40% mapped-line coverage to flag engine modules with more
than three fifths of their mapped lines unexecuted. The `ponko_bench`
modules remain visible in the baseline for context. Their workloads are
run as release benchmarks with their own commands and timing results.

| Module | Baseline | Decision |
|---|---:|---|
| `ponko` | 35.3% | The crate root: its 34 mapped lines are the crate documentation's example, a `no_run` doctest that compiles but never runs, and the `bench_sharing` hooks, which only the release benchmarks call. |
| `ponko::gamepad` | 35.9% | The tests exercise event translation. Verify the native input source with two real controllers, hot plugging, and rumble on Linux or Windows. [Manual controller checks](https://github.com/ponkoengine/ponko/issues/484) |
| `ponko_mcp::web_bridge` | 0.0% | A measurement gap, not a missing test: `tests/web_bridge.rs` drives the bridge through the `ponko-mcp` binary with a fake browser, and stops the process with a kill, so the child never writes its coverage profile. |
