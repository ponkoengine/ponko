# Shard Grove on real GPUs and in a browser

Issue [#454](https://github.com/ponkoengine/ponko/issues/454): the 3D reference game's performance baseline per GPU tier, and the game opened once in a WebGPU browser and in a window on each OS. Packaging it and playing the package headless is already in the list of checks (`package-smoke`); what is left needs real GPUs and a person. Read [the conventions](README.md#conventions) first.

Machine (run `python3 scripts/checklists/record_run.py machine` and paste the lines):

```text
revision: ______  date: ______
OS and version: ______
CPU / RAM: ______
GPU and driver: ______
```

## G1. Frame-time baseline per GPU tier

- **Run:** From a checkout of the revision, a release build, nothing else running, the laptop plugged in and in its best power mode:

  ```sh
  python3 scripts/checklists/lowend_bench.py --game shard_grove --out results/<machine-name>
  ```

  It runs `PONKO_BENCH=600` (vsync off) and nobody plays during the run, so the camera is the one the game starts with: the same view on every machine. Then get the adapter, the tier the engine picked and the preset from a normal run, as in [lowend.md](lowend.md) ("Run it", `perf_get.py`). Do this on one machine of each tier the engine has (`desktop`, `baseline`, `webgl2`: the machine's own tier, and `--tier baseline` on a desktop one shows what the fallback does, but a forced tier is not a baseline of a machine that has it).
- **Look for:** The 99th percentile frame time is under 16.7 ms with the preset the engine chose itself. Write the percentiles down even if it is not: the baseline is the number, not the target.
- **Record:** one line per machine, in the table below, and keep the run's `shard_grove.json`.

| Tier | Machine (CPU, GPU, driver, OS) | Adapter, backend | Preset | p50 / p95 / p99 ms | fps | Date |
|---|---|---|---|---|---|---|
| desktop | | | | | | |
| baseline | | | | | | |
| webgl2 | | | | | | |

## G2. Open it in a WebGPU browser

- **Run:** `web/build.sh --features navmesh,physics3d shard_grove && python3 -m http.server -d target/web/shard_grove 8000`, then open <http://localhost:8000/> in a browser that has a working WebGPU (the guide's web chapter says how the page chooses between WebGPU and WebGL2). Play for a minute: walk, jump, pulse, find a shard, save at a shrine, pause, reload the page and continue.
- **Look for:** The console logs `ponko: drawing with WebGPU` and no fallback line; the level, the characters and the sun's shadows draw; the sound plays after the first click; the saved game is still there after the reload; no error in the console.
- **Record:** date ______ · browser and version ______ · hardware ______ · result ______ · issue # if it failed ______

## G3. Open it in a window, on each OS

- **Run:** On each of Linux, macOS and Windows: `cargo run --release --example shard_grove --features navmesh,physics3d`. Play it to the end, with a gamepad if you have one: six shards, a save, quit to the title, continue, the gate. Then the packaged build of the same revision (`cargo run -p ponko_cli -- package --example shard_grove --features navmesh,physics3d`), started from another folder, and `shard_grove --smoke window --report report.json` from the package.
- **Look for:** The window opens at a sensible size and the game is smooth; resizing and full screen keep the picture; the pause and title menus take the mouse and the keyboard; the smoke run exits with 0 and `finished` is `true` in the report. Compare the first frames with the web build if you did G2.
- **Record, one line per OS:** date ______ · hardware (GPU, driver, OS version) ______ · result ______ · issue # if it failed ______

## Where to report

ONE comment on [#454](https://github.com/ponkoengine/ponko/issues/454) with the machine block, the G1 table and the lines of G2 and G3. Delete `target/` when you are done.
