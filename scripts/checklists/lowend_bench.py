#!/usr/bin/env python3
"""Run the reference games as benchmarks and write one table row for each.

    python3 scripts/checklists/lowend_bench.py --out results/deck [--frames 600] [--tier baseline]
                                               [--game coin_dash --game legions ...]

For every game it builds the release example once (not timed), then runs it
with `PONKO_BENCH=<frames>` and `PONKO_BENCH_OUT=<out>/<game>.json`
(book/src/performance.md, "Benchmark runs"): the game opens a window, plays
the warm-up and the measured frames, prints a report and quits. The row has
the frame-time percentiles from that JSON, the frames per second and the time
from launch to the start of the warm-up (a stand-in for the load time).
`--tier` sets `PONKO_GPU_TIER` to force a lower GPU tier on this machine.

It opens a window for a minute or so per game: run it on the machine you are
testing, not on one you are working at. The adapter, the preset and the GPU
memory are not in the benchmark's output: read them as the sheet
(lowend.md) says and add them to the row. `--dry-run` prints the commands.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

# game -> (example name, cargo features). The three reference games and the
# 100,000-unit example (book/src/legions.md).
GAMES = {
    "coin_dash": ("coin_dash", []),
    "ember_hop": ("ember_hop", ["physics"]),
    "shard_grove": ("shard_grove", ["navmesh", "physics3d"]),
    "legions": ("legions", []),
}
START_MARK = "ponko: benchmark:"


def cargo(example, features, *words):
    command = ["cargo", words[0], "--release", "--example", example]
    if features:
        command += ["--features", ",".join(features)]
    return command + list(words[1:])


def row_from_json(path, load_seconds):
    """A table row's cells from a benchmark JSON file."""
    data = json.loads(path.read_text(encoding="utf-8"))
    frame = data["frame_ms"]
    return {
        "frames": data["frames"],
        "fps": round(data["fps"], 1),
        "p50": round(frame["p50"], 2),
        "p95": round(frame["p95"], 2),
        "p99": round(frame["p99"], 2),
        "max": round(frame["max"], 2),
        "load": None if load_seconds is None else round(load_seconds, 1),
    }


def format_table(rows):
    lines = [
        "| Game | Frames | fps | p50 ms | p95 ms | p99 ms | max ms | Launch to warm-up s | Holds target? | Adapter, tier, preset, GPU memory |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---|---|",
    ]
    for game, cells in rows:
        if cells is None:
            lines.append(f"| {game} | did not finish: see its log | | | | | | | | |")
        else:
            load = "n/a" if cells["load"] is None else cells["load"]
            lines.append(
                f"| {game} | {cells['frames']} | {cells['fps']} | {cells['p50']} | {cells['p95']} | "
                f"{cells['p99']} | {cells['max']} | {load} | ________ | ________ |"
            )
    return "\n".join(lines) + "\n"


def run_game(game, out, frames, tier, timeout):
    example, features = GAMES[game]
    env = dict(os.environ, PONKO_BENCH=str(frames), PONKO_BENCH_OUT=str(out / f"{game}.json"))
    if tier:
        env["PONKO_GPU_TIER"] = tier
    log = out / f"{game}.log"
    started = time.monotonic()
    load = None
    with log.open("w", encoding="utf-8") as sink:
        process = subprocess.Popen(
            cargo(example, features, "run"), env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="replace", stdin=subprocess.DEVNULL,
        )
        try:
            for line in process.stdout:
                sink.write(line)
                if load is None and START_MARK in line:
                    load = time.monotonic() - started
                if time.monotonic() - started > timeout:
                    process.kill()
                    sink.write(f"\n(stopped after {timeout} s)\n")
                    break
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
    result = out / f"{game}.json"
    if process.returncode != 0 or not result.exists():
        return None
    return row_from_json(result, load)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", type=Path, required=True, help="folder for the JSON files, logs and table")
    parser.add_argument("--frames", type=int, default=600)
    parser.add_argument("--tier", choices=["desktop", "baseline", "webgl2"], help="force a lower GPU tier (PONKO_GPU_TIER)")
    parser.add_argument("--game", action="append", choices=sorted(GAMES), help="a game to run (repeatable; default: all)")
    parser.add_argument("--timeout", type=int, default=600, help="seconds one game may run (default 600)")
    parser.add_argument("--dry-run", action="store_true", help="print the commands and stop")
    args = parser.parse_args(argv)
    games = args.game or list(GAMES)
    if args.dry_run:
        for game in games:
            example, features = GAMES[game]
            print(" ".join(cargo(example, features, "build")))
            print(f"PONKO_BENCH={args.frames} PONKO_BENCH_OUT={args.out / (game + '.json')}"
                  + (f" PONKO_GPU_TIER={args.tier}" if args.tier else "") + " " + " ".join(cargo(example, features, "run")))
        return 0
    args.out.mkdir(parents=True, exist_ok=True)
    rows = []
    for game in games:
        example, features = GAMES[game]
        print(f"building {game} ...", flush=True)
        if subprocess.call(cargo(example, features, "build"), stdin=subprocess.DEVNULL) != 0:
            rows.append((game, None))
            continue
        print(f"running {game}: {args.frames} frames ...", flush=True)
        rows.append((game, run_game(game, args.out, args.frames, args.tier, args.timeout)))
    head = [
        f"Low-end run, {datetime.date.today().isoformat()}",
        "",
        f"- Machine: {platform.platform()} ({platform.machine()})",
        f"- Revision: {subprocess.run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip() or 'unknown'}",
        f"- Frames measured: {args.frames}" + (f", forced tier: {args.tier}" if args.tier else ""),
        "- Hardware (you fill in: CPU, RAM, GPU, driver): ________",
        "",
    ]
    text = "\n".join(head) + format_table(rows)
    (args.out / "table.md").write_text(text, encoding="utf-8", newline="\n")
    print("\n" + text)
    print(f"Written to {args.out / 'table.md'}")
    return 0 if all(cells is not None for _, cells in rows) else 1


if __name__ == "__main__":
    sys.exit(main())
