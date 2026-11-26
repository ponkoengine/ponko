# Four-hour reference-game soaks

Before a minor release, run this on macOS, Windows and Linux from the same
release checkout. Use a desktop session and a real GPU. On Linux, use X11
for the scripted minimize and restore. Wayland clients cannot request a
window restore. Use the Visual Studio Developer PowerShell on Windows and
replace `python3` with `py -3` if needed.

```sh
python3 scripts/soak.py --windowed --seconds 14400 --sample 60 --warmup 600 --out target/soak-release
```

The four workloads run sequentially for four hours each, around 16 hours
plus build time. Prevent system sleep, leave the windows open and verify
that each one resizes, minimizes and restores on its own before leaving it to
run. If a window manager ignores a request, exercise minimize and restore
manually and record that in the sheet. Keep the desktop session logged in.
Use `--game coin_dash`, `--game ember_hop`, `--game shard_grove` or
`--game streaming` to distribute the work over several sessions. Use a new
output directory when repeating a game.

| OS, version, desktop session | CPU, RAM, GPU, driver | Revision and Rust version | Coin Dash | Ember Hop | Shard Grove | Streaming |
|---|---|---|---|---|---|---|
| macOS | | | | | | |
| Windows | | | | | | |
| Linux (X11) | | | | | | |

For each result record the elapsed time, completed cycles, observed
minimize/restore, verdict and any unavailable readings. Attach each game's
`samples.csv` and `report.json`. For a failing slope, file an issue with the
game, revision, hardware, exact command, measured slope, bound and the
metric's SVG. Attach the graph as a PNG if the tracker cannot display SVG
attachments. Include `game.log` for a crash or an incomplete run. Post the
filled lines and links in one comment on [#484](https://github.com/ponkoengine/ponko/issues/484)
(the checks that need a person)
and link the comment in the release pull request. See
[Reference-game soaks](../../book/src/sim-clock.md#reference-game-soaks) for
how to read the reports.
