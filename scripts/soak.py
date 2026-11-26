#!/usr/bin/env python3
"""Build and soak the reference games sequentially, with one process per game."""

import argparse
import csv
import html
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
GAMES = ("coin_dash", "ember_hop", "shard_grove", "streaming")
FEATURES = "bench-internals,physics,physics3d,navmesh,text-shaping"
MIB = 1024 ** 2
# Each entry is (growth allowed per hour, one-time noise allowance).
BOUNDS = {
    "rss_bytes": (16 * MIB, 24 * MIB),
    "gpu_bytes": (8 * MIB, 16 * MIB),
    "wgpu_bytes": (8 * MIB, 32 * MIB),
    "allocations_per_frame": (2, 4),
    "audio_real": (1, 4),
    "audio_virtual": (1, 4),
    "frame_ms_p50": (0.5, 2),
    "frame_ms_p95": (1, 5),
    "frame_ms_p99": (2, 10),
}


def trend(samples, metric, warmup, bound):
    """Fit a line in wall time and compare its upward slope to the bound."""
    post = [row for row in samples if row["seconds"] >= warmup]
    points = [(row["seconds"] / 3600, row[metric]) for row in post if row[metric] is not None]
    result = dict(samples=len(points), slope_per_hour=None, bound_per_hour=bound)
    if not points and post:
        return dict(result, status="unavailable")
    if len(points) < 3 or len(points) != len(post):
        return dict(result, status="insufficient")
    mean_x = sum(x for x, _ in points) / len(points)
    mean_y = sum(y for _, y in points) / len(points)
    variance = sum((x - mean_x) ** 2 for x, _ in points)
    slope = sum((x - mean_x) * (y - mean_y) for x, y in points) / variance
    return dict(result, status="pass" if slope <= bound else "fail", slope_per_hour=slope)


def graph(path, metric, samples, warmup):
    """A portable graph to attach to a report of growth."""
    values = [(r["seconds"], r[metric]) for r in samples if r[metric] is not None]
    low = min(y for _, y in values)
    high = max(y for _, y in values)
    end = samples[-1]["seconds"]
    points = " ".join(f"{90 + 640*t/end:.2f},{310 - 240*(y-low)/max(high-low, 1e-9):.2f}" for t, y in values)
    path.write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 760 365">'
        f'<rect width="760" height="365" fill="white"/>'
        f'<g font-family="sans-serif" fill="#222"><text x="90" y="32">{html.escape(metric)}</text>'
        f'<text x="8" y="75">{high:.5g}</text><text x="8" y="310">{low:.5g}</text>'
        f'<text x="90" y="340">0 s</text><text x="666" y="340">{end:.0f} s</text></g>'
        f'<path d="M90 70 V310 H730" fill="none" stroke="#555"/>'
        f'<path d="M{90 + 640*warmup/end:.2f} 70 V310" stroke="#777" stroke-dasharray="6 4"/>'
        f'<polyline points="{points}" fill="none" stroke="#245fba" stroke-width="2"/></svg>\n',
        encoding="utf-8",
    )


def evaluate(out, game, samples, args, code):
    post = [r for r in samples if r["seconds"] >= args.warmup]
    hours = (post[-1]["seconds"] - post[0]["seconds"]) / 3600 if len(post) > 1 else 1
    results = {}
    if samples:
        for name in samples[0]:
            if name.startswith("handles_"):
                # A live tile has five LOD meshes. Adjacent sample times can
                # straddle the starts/completions of several tile jobs.
                noise = 80 if game == "streaming" and name == "handles_meshes" else 2
                growth, slack = 1, noise
            elif name in BOUNDS:
                growth, slack = BOUNDS[name]
            else:
                continue
            results[name] = trend(samples, name, args.warmup, growth + slack / hours)
            if results[name]["status"] == "fail":
                graph(out / f"{name}.svg", name, samples, args.warmup)
        with (out / "samples.csv").open("w", newline="", encoding="utf-8") as csv_file:
            writer = csv.DictWriter(csv_file, fieldnames=list(samples[0]))
            writer.writeheader()
            writer.writerows(samples)
    failures = [
        name for name, result in results.items()
        if result["status"] != "pass"
        and (result["status"] != "unavailable"
             or (name != "wgpu_bytes" and not (args.headless and name == "gpu_bytes")))
    ]
    last = samples[-1] if samples else {}
    # A window closed early, a hung script and a renderer that never drew
    # must all fail even if every numeric metric was flat.
    if code != 0:
        failures.append(f"game exited with {code}")
    if last.get("seconds", -1) < args.seconds:
        failures.append("duration not reached")
    if not args.headless:
        drawn = "triangles" if game in ("shard_grove", "streaming") else "draw_calls"
        if not any(row.get(drawn, 0) > 0 for row in post):
            failures.append("no workload draw reported after warm-up")
    if last.get("cycles", 0) < 2:
        failures.append("fewer than two workload cycles completed")
    if last.get("resizes", 0) == 0:
        failures.append("no resize exercised")
    if last.get("restores", 0) == 0:
        failures.append("no minimize and restore cycle completed")
    if game != "streaming":
        for event in ("saves", "loads", "deaths"):
            if last.get(event, 0) == 0:
                failures.append(f"no {event} exercised")
    summary = {
        "game": game, "mode": "windowed" if args.windowed else "headless" if args.headless else "offscreen",
        "platform": platform.platform(), "revision": git_revision(), "warmup_seconds": args.warmup,
        "sample_seconds": args.sample, "pass": not failures, "failures": failures,
        "metrics": results, "samples": samples,
    }
    (out / "report.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"{game}: {'PASS' if summary['pass'] else 'FAIL'} ({out / 'report.json'})", flush=True)
    for failure in failures:
        print(f"  {failure}", flush=True)
    return summary["pass"]


def git_revision():
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()


def build(games, cargo):
    command = ["cargo", "build", "--locked", "--release", "-p", "ponko", "--features", FEATURES,
               "--message-format=json-render-diagnostics"]
    for game in games:
        command.extend(["--example", game])
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True) as build_process:
        for line in build_process.stdout:
            msg = json.loads(line)
            if msg["reason"] == "compiler-message" and msg["message"]["rendered"]:
                print(msg["message"]["rendered"], end="", flush=True)
            if msg["reason"] == "compiler-artifact" and msg["executable"]:
                name = msg["target"]["name"]
                if name in games:
                    cargo[name] = Path(msg["executable"])
        if (code := build_process.wait()) != 0:
            raise subprocess.CalledProcessError(code, command)


def read_samples(raw):
    if not raw.exists():
        return []
    return [json.loads(line) for line in raw.read_text(encoding="utf-8").splitlines()]


def reevaluate(game, args):
    """The exit code is not saved, so only the samples are judged again."""
    out = args.out.resolve() / game
    return evaluate(out, game, read_samples(out / "samples.jsonl"), args, 0)


def run_game(game, binary, args):
    out = args.out.resolve() / game
    out.mkdir(parents=True, exist_ok=False)
    raw = out / "samples.jsonl"
    mode = "windowed" if args.windowed else "headless" if args.headless else "offscreen"
    with tempfile.TemporaryDirectory(prefix="ponko-soak-", dir=out) as data:
        env = dict(os.environ, PONKO_DATA_DIR=data)
        command = [str(binary), "--soak", mode, str(args.seconds), str(args.sample), str(raw), data]
        print(f"{game}: running for {args.seconds}s ({mode})", flush=True)
        with (out / "game.log").open("w", encoding="utf-8") as log:
            try:
                code = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
                                      timeout=args.seconds + 60, check=False).returncode
            except subprocess.TimeoutExpired:
                code = "timeout"
    return evaluate(out, game, read_samples(raw), args, code)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", choices=GAMES, action="append", help="repeat to select multiple games")
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--sample", type=int, default=60)
    parser.add_argument("--warmup", type=int, default=120)
    parser.add_argument("--out", type=Path, help="new report directory (default: a fresh folder under target/soak)")
    parser.add_argument("--binaries", type=Path, help="use examples built with the documented feature set")
    parser.add_argument("--evaluate", action="store_true",
                        help="re-evaluate the samples already in --out with the current bounds; runs nothing")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--windowed", action="store_true", help="open a real window (attended run)")
    mode.add_argument("--headless", action="store_true", help="omit GPU drawing for diagnostics")
    args = parser.parse_args()
    if not (0 < args.sample <= args.seconds and args.warmup >= 0
            and args.seconds - args.warmup >= 2 * args.sample):
        parser.error("need at least three post-warm-up samples (decrease --sample or --warmup)")
    games = tuple(dict.fromkeys(args.game or GAMES))
    if args.evaluate:
        if args.out is None:
            parser.error("--evaluate needs --out")
        statuses = [reevaluate(game, args) for game in games if (args.out / game / "samples.jsonl").exists()]
        if not statuses:
            parser.error(f"no samples under {args.out}")
        return 0 if all(statuses) else 1
    if args.out is None:
        parent = ROOT / "target/soak"
        parent.mkdir(parents=True, exist_ok=True)
        args.out = Path(tempfile.mkdtemp(prefix="run-", dir=parent))
    for game in games:
        if (args.out / game).exists():
            parser.error(f"{args.out / game} already exists; use a new output directory")
    binaries = {}
    if args.binaries:
        suffix = ".exe" if os.name == "nt" else ""
        binaries = {game: args.binaries.resolve() / (game + suffix) for game in games}
    else:
        build(games, binaries)
    # Evaluate every selected game, even after a failure.
    statuses = [run_game(game, binaries[game], args) for game in games]
    return 0 if all(statuses) else 1


if __name__ == "__main__":
    raise SystemExit(main())
