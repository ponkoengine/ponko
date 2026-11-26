# Low-end hardware and the Steam Deck

Issue [#510](https://github.com/ponkoengine/ponko/issues/510): the three reference games and the 100,000-unit example at 60 fps on the machines most small games are played on, or the reason they are not. The runs need the hardware; the script and the sheet below make each one a few minutes of work.

Read [the conventions](README.md#conventions) first.

## The machines

Run everything below on each of:

1. an **integrated Intel GPU** from the last five years;
2. an **older discrete card with 2 to 4 GB**;
3. a **Steam Deck**, the native Linux build (desktop mode; build there, or copy a build made on Linux).

The target is 60 fps on the first two and 40 fps on the Deck. State the Deck separately.

## Run it

On the machine, from a checkout of the revision you are testing (a release build; close other programs; plug the laptop in and set its power mode to its best):

```sh
python3 scripts/checklists/lowend_bench.py --out results/<machine-name>
```

It builds each game once, then runs it as a benchmark (`PONKO_BENCH=600`, vsync off; book/src/performance.md, "Benchmark runs"): the game opens a window for about a minute, then quits. Games, in order: `coin_dash`, `ember_hop`, `shard_grove`, `legions` (100,000 units). Nothing is sent anywhere. It writes `<game>.json`, `<game>.log` and `table.md` into the folder, and the table has the frame-time percentiles, the fps and the time from launch to the start of the warm-up (a stand-in for the load time). Options: `--game <name>` (repeatable), `--frames N`, `--tier baseline` (force a lower GPU tier, to see what the fallback does), `--dry-run` (print the commands).

The benchmark's output does not say which adapter the engine opened, which tier it chose, which graphics preset or how much GPU memory it holds. Get those from a **normal** run of each game, with its control endpoint on (book/src/control.md), a few seconds after the first frame:

```sh
# terminal 1 (release builds need the control-env feature; add the game's own features):
PONKO_CONTROL=1 cargo run --release --features control-env --example shard_grove
# terminal 2, while the game is running:
python3 scripts/checklists/perf_get.py
```

`perf_get.py` prints the adapter (name, backend, driver, GPU tier), the graphics quality level and the GPU memory by class. Copy the adapter, tier, preset (the quality level) and the total GPU memory into the last column of the table.

## What to fill in, for each machine and game

Copy this into a comment, one table per machine (the script's `table.md` has the first columns filled; the Deck's target is 40 fps, the others 60):

```text
Machine: ______ (CPU ______, RAM ______, GPU ______, driver ______, OS ______)
Date: ______   Revision: ______   Power mode / plugged in: ______
```

| Game | Adapter, backend | Tier the engine picked | Preset | p50 / p95 / p99 ms | fps | GPU memory | Load time s | Holds target (60, or 40 on the Deck)? If not: why |
|---|---|---|---|---|---|---|---|---|
| coin_dash | | | | | | | | |
| ember_hop | | | | | | | | |
| shard_grove | | | | | | | | |
| legions (100,000 units) | | | | | | | | |

"Holds the target" means the 99th percentile frame time is under 16.7 ms (25 ms for 40 fps) with the preset the engine chose itself, not one you set. If it does not, record what the engine chose, what you had to change, and the result.

## The Steam Deck checks

Each with a line to fill: date ______ · Deck model and SteamOS version ______ · result ______ · issue # ______.

1. **Built-in controls with no configuration.** Start each game from desktop mode, and again from Game mode as a non-Steam game. Look for: the Deck's sticks, d-pad, buttons and triggers work with no controller configuration; the prompts show gamepad glyphs.
2. **On-screen keyboard.** In a text field (the UI menu, `cargo run --example ui_menu --features accessibility`) press Steam+X or tap the field. Look for: the on-screen keyboard opens and typed text arrives.
3. **1280 x 800.** Start each game fullscreen. Look for: it fits the screen with nothing cut off, UI is readable, and the 3D game's picture is right.
4. **Suspend and resume.** Play, press the power button, wake it. Look for: the game is still running, sound returns, and the frame rate is the same as before.

## Anything that fails

Anything that fails, or needs a manual setting to work, becomes an issue (title: the game, the machine, the symptom). The guide gets a "minimum hardware" section with these machines as its evidence once the three tables are in: say in your comment that the numbers are ready for it.

## Where to report

The filled tables and Deck lines go in ONE comment per machine on [#510](https://github.com/ponkoengine/ponko/issues/510). Attach the `results/<machine-name>` folder (zip it) if you can.
