#!/usr/bin/env python3
"""The cross-engine scoreboard: every workload on Ponko and on Bevy, one
command, results as JSON and as a Markdown table.

    python3 bench/compare/scoreboard.py              # everything (about an hour)
    python3 bench/compare/scoreboard.py --runtime    # only the run-time workloads
    python3 bench/compare/scoreboard.py --builds     # only build times and sizes
    python3 bench/compare/scoreboard.py --only sprites,ui
    python3 bench/compare/scoreboard.py --tuned      # the second table: both engines tuned
    python3 bench/compare/scoreboard.py --tuned --runtime

It builds `ponko_bench` (feature `render`) in the engine's workspace and
`bench/compare` in its own, once each, in release; then runs each workload
three times per engine, one process at a time, alternating the engines, with
the 1-minute load average written next to every run (it waits up to ten
minutes for a load under 3 first, and marks a run taken above it). Build
times and sizes come from `scripts/build-times.sh` (debug: three clean
builds, then rebuilds after one-line edits) and one clean release build and
one wasm build per project, each project made fresh in a folder outside
the checkout.

`--tuned` makes the second table: each engine as a careful game ships it
(`bench/compare/README.md`, "The tuned table"): the shipping profile `ship`
on both sides (fat LTO, one codegen unit, `panic = "abort"`), only the Bevy
features each group of workloads needs (one tuned Bevy program per group),
Bevy's pipelined rendering on, rapier's `parallel` feature on both sides,
and on each side the faster of its documented ways to do a workload
(`TUNED_PATHS`, measured). Its build rows use each engine's fast-compile
advice and the shipping profile. Without `--runtime`, `--builds` or
`--tuned`, both tables are made.

Writes `bench/compare/results/scoreboard.json` and `scoreboard.md` (the
first table), `scoreboard-tuned.json` and `scoreboard-tuned.md` (the
second), and prints the tables. `bench/compare/README.md` describes every workload and
what is and isn't the same on both sides.
"""

import argparse
import gzip
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
RESULTS = os.path.join(HERE, "results")
PONKO_BENCH = os.path.join(ROOT, "target", "release", "ponko_bench")
FIRST_FRAME = os.path.join(ROOT, "target", "release", "first_frame")
COMPARE = os.path.join(HERE, "target", "release", "ponko_compare")

# The ECS rows: one process runs all three libraries (see src/ecs.rs).
ECS_ROWS = [
    ("ECS: spawn and despawn 100k in a world of 1M", "ecs/spawn-despawn-1m/ponko", "ecs/spawn-despawn-1m/bevy"),
    ("ECS: iterate 1M (one archetype)", "ecs/iter-1m/ponko", "ecs/iter-1m/bevy"),
    ("ECS: iterate 16k fragmented over 1,024 archetypes", "archetypes-1024/ponko", "archetypes-1024/bevy"),
    ("ECS: add and remove a component, 1M entities", "ecs/add-remove-1m/ponko", "ecs/add-remove-1m/bevy"),
    ("ECS: parallel iterate 1M (all cores)", "ecs/par-1m/ponko-par", "ecs/par-1m/bevy-par"),
    ("ECS: commands, 100k spawns and despawns in a world of 1M", "ecs/commands-1m/ponko", "ecs/commands-1m/bevy"),
]
ECS_FILTER = "|".join(sorted({row[1] for row in ECS_ROWS} | {row[2] for row in ECS_ROWS}))

SCENE_ENV = {"PONKO_RENDER_FRAMES": "300", "PONKO_RENDER_WARMUP": "60", "PONKO_RENDER_MATCHED": "1"}

# ---- the tuned table

# The shipping profile, the same on both sides. bench/compare's Cargo.toml
# has it as [profile.ship]; the engine's workspace gets it from here.
SHIP = ["--config", 'profile.ship.inherits="release"', "--config", 'profile.ship.lto="fat"',
        "--config", "profile.ship.codegen-units=1", "--config", 'profile.ship.panic="abort"']
SHIP_PROFILE = '\n[profile.ship]\ninherits = "release"\nlto = "fat"\ncodegen-units = 1\npanic = "abort"\n'
# rapier's documented performance features, on both sides (README: both
# measured faster here, `simd8` by a little).
RAPIER_FEATURES = ["rapier-parallel", "rapier-simd8"]
# The Bevy groups: one tuned program each, with only that group's features.
BEVY_GROUPS = {"ecs": "headless", "sim": "headless", "physics": "headless", "sprites": "sprites",
               "first-frame": "sprites", "ui": "ui", "scenes": "scenes"}
# The faster of each engine's documented ways to do a workload, measured
# (README, "The tuned table"): environment variables the programs read.
TUNED_PATHS = {
    "ponko": {"sim": {"PONKO_COMPARE_SIM": "par"}, "sprites": {"PONKO_COMPARE_MOVE": "par"}},
    "bevy": {"sim": {"PONKO_COMPARE_SIM": "par"}, "sprites": {"PONKO_COMPARE_MOVE": "par", "PONKO_COMPARE_SPRITES": "mesh2d"},
             "ui": {"PONKO_COMPARE_UI": "grid"}},
}
# The tuned small games: the features they use and nothing else.
PONKO_TUNED_FEATURES = ["window"]
BEVY_TUNED_FEATURES = ["2d_bevy_render", "ui_bevy_render", "default_app", "bevy_winit", "std",
                       "multi_threaded", "default_font", "webgl2", "x11", "wayland"]


class Binaries:
    """Which programs a table runs and what they are told."""

    def __init__(self, tuned):
        self.tuned = tuned
        out = "ship" if tuned else "release"
        self.ponko_bench = os.path.join(ROOT, "target", out, "ponko_bench")
        self.first_frame = os.path.join(ROOT, "target", out, "first_frame")

    def compare(self, key):
        if not self.tuned:
            return COMPARE
        return os.path.join(HERE, "target", "ship", f"tuned-{BEVY_GROUPS[key]}")

    def env(self, side, key):
        if not self.tuned:
            return {}
        env = dict(TUNED_PATHS[side].get(key, {}))
        if key == "first-frame" and side == "ponko":
            env["PONKO_COMPARE_EXIT"] = "1"
        return env


def log(msg):
    print(msg, flush=True)


def sh(cmd, cwd=ROOT, env=None, check=True, capture=True):
    full_env = dict(os.environ)
    full_env.update(env or {})
    r = subprocess.run(cmd, cwd=cwd, env=full_env, text=True, capture_output=capture)
    if check and r.returncode != 0:
        sys.stderr.write((r.stdout or "") + (r.stderr or ""))
        raise SystemExit(f"failed ({r.returncode}): {' '.join(cmd)}")
    return r


def out(cmd):
    try:
        return subprocess.run(cmd, text=True, capture_output=True).stdout.strip()
    except OSError:
        return ""


def load():
    m = re.search(r"averages?:\s*([\d.]+)", out(["uptime"]))
    return float(m.group(1)) if m else -1.0


def quiet():
    """Waits (at most 10 minutes) for a 1-minute load under 3."""
    waited = 0
    while load() > 3 and waited < 600:
        time.sleep(60)
        waited += 60
    l1 = load()
    return {"load": l1, "under_load": l1 > 3}


def machine():
    gb = int(out(["sysctl", "-n", "hw.memsize"]) or 0) // 2**30
    perf, eff = out(["sysctl", "-n", "hw.perflevel0.physicalcpu"]), out(["sysctl", "-n", "hw.perflevel1.physicalcpu"])
    batt = out(["pmset", "-g", "batt"]).splitlines()
    return {
        "cpu": out(["sysctl", "-n", "machdep.cpu.brand_string"]),
        "cores": f"{perf} performance + {eff} efficiency" if perf else out(["sysctl", "-n", "hw.ncpu"]),
        "memory_gb": gb,
        "os": f"{out(['sw_vers', '-productName'])} {out(['sw_vers', '-productVersion'])}",
        "power": batt[0].split("'")[1] if batt and "'" in batt[0] else "unknown",
        "rustc": out(["rustc", "--version"]),
        "ponko_rev": out(["git", "-C", ROOT, "rev-parse", "--short=8", "HEAD"]),
        "ponko_dirty": bool(out(["git", "-C", ROOT, "status", "--porcelain", "--untracked-files=no"])),
        "bevy": bevy_version(),
        "date": time.strftime("%Y-%m-%d"),
    }


def bevy_version():
    m = re.search(r'^bevy = \{ version = "=([^"]+)"', open(os.path.join(HERE, "Cargo.toml")).read(), re.M)
    return m.group(1)


def build():
    log("building ponko_bench (release, feature render) and bench/compare (release)...")
    sh(["cargo", "build", "--release", "-p", "ponko_bench", "--features", "render", "--bins"], capture=False)
    sh(["cargo", "build", "--release"], cwd=HERE, capture=False)
    time.sleep(20)


def build_tuned(only):
    log("building the tuned programs (profile ship: fat LTO, one codegen unit, panic = abort)...")
    features = ",".join(["render"] + RAPIER_FEATURES)
    sh(["cargo", "build", *SHIP, "--profile", "ship", "-p", "ponko_bench", "--features", features, "--bins"],
       capture=False)
    groups = sorted({g for key, g in BEVY_GROUPS.items() if not only or key in only})
    for group in groups:
        features = [group] + (RAPIER_FEATURES if group == "headless" else [])
        sh(["cargo", "build", "--profile", "ship", "--no-default-features", "--features", ",".join(features)],
           cwd=HERE, capture=False)
        shutil.copy(os.path.join(HERE, "target", "ship", "ponko_compare"),
                    os.path.join(HERE, "target", "ship", f"tuned-{group}"))
    time.sleep(20)


def run_json(cmd, env=None, cwd=ROOT):
    """Runs `cmd` with PONKO_BENCH_JSON set; returns (document, load)."""
    state = quiet()
    fd, path = tempfile.mkstemp(suffix=".json")
    os.close(fd)
    try:
        sh(cmd, cwd=cwd, env={**(env or {}), "PONKO_BENCH_JSON": path})
        with open(path) as f:
            return json.load(f), state
    finally:
        os.unlink(path)


def p50_of(doc, name):
    """The median frame (ms) of record `name`, in either engine's document."""
    for r in doc.get("results", []):
        if r["name"] == name:
            return r["p50_ms"]
    for s in doc.get("scenes", []):  # bench/compare's scenes document
        if f"render/scene {s['name']} frame" == name:
            return s["timings_ms"]["frame"]["p50"]
    raise SystemExit(f"no result {name!r} in {sorted(r['name'] for r in doc.get('results', []))}")


def runtime_workloads(bins):
    """(key, rows, ponko run, bevy run): a run returns {row title: p50}."""

    def ecs():
        doc, state = run_json([bins.compare("ecs"), "ecs", ECS_FILTER])
        return {t: (p50_of(doc, p), p50_of(doc, b)) for t, p, b in ECS_ROWS}, state

    def pair(key, title, ponko_args, bevy_args, ponko_name, bevy_name=None, env=None):
        def ponko():
            doc, state = run_json([bins.ponko_bench, *ponko_args], {**(env or {}), **bins.env("ponko", key)})
            return p50_of(doc, ponko_name), state

        def bevy():
            doc, state = run_json([bins.compare(key), *bevy_args], {**(env or {}), **bins.env("bevy", key)},
                                  cwd=HERE)
            return p50_of(doc, bevy_name or ponko_name), state

        return (key, (title, ponko, bevy))

    rows = [
        pair("sprites", "Sprites: 100k moving, one frame drawn", ["compare/sprites-100k"],
             ["sprites", "compare/sprites-100k"], "compare/sprites-100k"),
        pair("sprites", "Sprites: 100k moving, a different tint each, one frame drawn",
             ["compare/sprites-100k-tinted"],
             ["sprites", "compare/sprites-100k-tinted"], "compare/sprites-100k-tinted"),
        pair("sprites", "Sprites: 1M moving, one frame drawn", ["compare/sprites-1m"],
             ["sprites", "compare/sprites-1m"], "compare/sprites-1m"),
        pair("scenes", "3D: 100k moving meshes (16 kinds, instanced) with a 4-cascade sun shadow",
             ["render/scene dynamic-world"], ["scenes", "dynamic-world"],
             "render/scene dynamic-world frame", env={**SCENE_ENV, "PONKO_RENDER_SCALE": "5"}),
        pair("scenes", "3D: 1,000 skinned characters (16 joints) with a sun shadow",
             ["render/scene animated"], ["scenes", "animated"],
             "render/scene animated frame", env={**SCENE_ENV, "PONKO_RENDER_SCALE": "2.5"}),
        pair("physics", "2D physics: 10k boxes falling into a pile, one tick", ["compare/physics-10k"],
             ["physics"], "compare/physics-10k"),
        pair("ui", "UI: 10k nodes, still, one frame drawn", ["compare/ui-10k-still"],
             ["ui", "compare/ui-10k-still"], "compare/ui-10k-still"),
        pair("ui", "UI: 10k nodes, all recolored every frame, one frame drawn", ["compare/ui-10k-recolor"],
             ["ui", "compare/ui-10k-recolor"], "compare/ui-10k-recolor"),
        pair("sim", "Headless sim: 1M entities, one tick", ["compare/sim-1m"], ["sim"], "compare/sim-1m"),
    ]
    return ecs, rows


def first_frame(cmd, cwd, env=None):
    state = quiet()
    start = time.perf_counter()
    sh(cmd, cwd=cwd, env=env)
    return (time.perf_counter() - start) * 1e3, state


def summary(values):
    return {"median": statistics.median(values), "min": min(values), "max": max(values), "runs": values}


def runtime(runs, only, bins):
    results = {}
    ecs, pairs = runtime_workloads(bins)

    def add(title, side, value, state):
        row = results.setdefault(title, {"unit": "ms", "ponko": [], "bevy": [], "loads": []})
        row[side].append(value)
        row["loads"].append({"side": side, **state})

    for i in range(runs):
        if not only or "ecs" in only:
            log(f"ecs run {i + 1}")
            values, state = ecs()
            for title, (p, b) in values.items():
                add(title, "ponko", p, state)
                add(title, "bevy", b, state)
        for key, (title, ponko, bevy) in pairs:
            if only and key not in only:
                continue
            log(f"{title}: run {i + 1}")
            # Alternate which engine goes first, so a slow minute hits both.
            order = [("ponko", ponko), ("bevy", bevy)] if i % 2 == 0 else [("bevy", bevy), ("ponko", ponko)]
            for side, f in order:
                value, state = f()
                add(title, side, value, state)
                log(f"  {side}: {value:.3f} ms (load {state['load']})")
        if not only or "first-frame" in only:
            title = "Time to first frame (process start to first offscreen frame done, process exit included)"
            sides = [("ponko", [bins.first_frame], ROOT, bins.env("ponko", "first-frame")),
                     ("bevy", [bins.compare("first-frame"), "first-frame"], HERE, bins.env("bevy", "first-frame"))]
            if i == 0:
                # One untimed launch each first: the first launch after a
                # build reads the binary from disk and fills the driver's
                # shader cache, which a player's second launch doesn't pay.
                for side, cmd, cwd, env in sides:
                    sh(cmd, cwd=cwd, env=env)
            for side, cmd, cwd, env in sides:
                value, state = first_frame(cmd, cwd, env)
                add(title, side, value, state)
                log(f"  first frame {side}: {value:.0f} ms (load {state['load']})")
    for row in results.values():
        row["ponko"], row["bevy"] = summary(row["ponko"]), summary(row["bevy"])
    return results


# ------------------------------------------------------------------ builds


def seconds(text):
    """cargo's '45.83s' or '1m 05s'."""
    m = re.match(r"(?:(\d+)m\s*)?([\d.]+)s", text.strip())
    return (int(m.group(1) or 0) * 60 + float(m.group(2))) if m else None


def make_project(name, work):
    """A fresh copy of a build-time project. `<name>-tuned`: the same game
    with only the engine features it uses and the shipping profile."""
    tuned = name.endswith("-tuned")
    base = name.removesuffix("-tuned")
    dest = make_base_project(base, work, name)
    if tuned:
        toml = os.path.join(dest, "Cargo.toml")
        text = open(toml).read()
        if base.startswith("ponko"):
            features = ", ".join(f'"{f}"' for f in PONKO_TUNED_FEATURES)
            old = f'ponko = {{ path = "{ROOT}" }}'
            assert old in text, text
            text = text.replace(old, f'ponko = {{ path = "{ROOT}", default-features = false, features = [{features}] }}')
        else:
            features = ", ".join(f'"{f}"' for f in BEVY_TUNED_FEATURES)
            old = 'bevy = { version = "=0.20.1" }'
            assert old in text, text
            text = text.replace(old, f'bevy = {{ version = "=0.20.1", default-features = false, features = [{features}] }}')
        text = text.replace("\n[workspace]\n", SHIP_PROFILE + "\n[workspace]\n")
        assert "[profile.ship]" in text, text
        open(toml, "w").write(text)
    return dest


def make_base_project(name, work, folder):
    dest = os.path.join(work, folder)
    shutil.rmtree(dest, ignore_errors=True)
    if name == "ponko-small":
        template = os.path.join(ROOT, "crates", "ponko_cli", "templates", "2d")
        shutil.copytree(template, dest, ignore=shutil.ignore_patterns("tests", "*.md", ".mcp.json", "cargo-generate.toml"))
        liquid = os.path.join(dest, "Cargo.toml.liquid")
        text = open(liquid).read().replace("{{project-name}}", folder)
        text = text.replace('ponko = { version = "0.1" }', f'ponko = {{ path = "{ROOT}" }}')
        open(os.path.join(dest, "Cargo.toml"), "w").write(text + "\n[workspace]\n")
        os.unlink(liquid)
        for f in ("src/lib.rs", "src/main.rs"):
            p = os.path.join(dest, f)
            text = open(p).read().replace("{{project-name}}", folder).replace("{{crate_name}}", folder.replace("-", "_"))
            open(p, "w").write(text)
    else:
        shutil.copytree(os.path.join(HERE, "projects", name), dest)
        toml = os.path.join(dest, "Cargo.toml")
        text = open(toml).read().replace("PONKO_PATH", ROOT)
        open(toml, "w").write(text)
    return dest


def build_project(name, work):
    dest = make_project(name, work)
    tuned = name.endswith("-tuned")
    # The tuned Bevy game's debug builds link Bevy dynamically, as Bevy's
    # setup guide advises while developing (`bevy/dynamic_linking`).
    env = {"BUILD_TIMES_CARGO_ARGS": "--features bevy/dynamic_linking"} if tuned and name.startswith("bevy") else {}
    log(f"{name}: build-times.sh (debug, 3 clean builds) in {dest} {env}")
    text = sh(["bash", os.path.join(ROOT, "scripts", "build-times.sh"), dest, "dev", "3"], env=env).stdout
    log(text)
    row = {"clean_debug": [], "edit_lib_debug": [], "edit_main_debug": [], "link_debug": [], "loads": []}
    for line in text.splitlines():
        m = re.match(r"(clean|edit-lib|edit-main)-debug run\d+ (.+?) load=([\d.]+)", line)
        if m:
            row[m.group(1).replace("-", "_") + "_debug"].append(seconds(m.group(2)))
            row["loads"].append(float(m.group(3)))
        m = re.match(r"link-debug run\d+ ([\d.]+)s", line)
        if m:
            row["link_debug"].append(float(m.group(1)))
        m = re.match(r"binary-debug (\d+) bytes", line)
        if m:
            row["debug_binary_bytes"] = int(m.group(1))
    package = re.search(r'^name = "([^"]+)"', open(os.path.join(dest, "Cargo.toml")).read(), re.M).group(1)
    crate = package.replace("-", "_")
    # One clean release build (tuned: the shipping profile): its time and
    # the binary's size.
    profile, out = (["--profile", "ship"], "ship") if tuned else (["--release"], "release")
    row["profile"] = out
    shutil.rmtree(os.path.join(dest, "target"), ignore_errors=True)
    time.sleep(20)
    row["release_load"] = load()
    r = sh(["cargo", "build", *profile], cwd=dest)
    row["clean_release"] = seconds(re.search(r"Finished .* in (.+)", r.stderr).group(1))
    binary = os.path.join(dest, "target", out, package)
    row["release_binary_bytes"] = os.path.getsize(binary)
    shutil.copy(binary, binary + ".stripped")
    sh(["strip", binary + ".stripped"])
    row["release_stripped_bytes"] = os.path.getsize(binary + ".stripped")
    # The wasm of the release build, as cargo writes it (before wasm-bindgen
    # and wasm-opt), and gzipped.
    r = sh(["cargo", "build", *profile, "--target", "wasm32-unknown-unknown"], cwd=dest, check=False)
    wasm = os.path.join(dest, "target", "wasm32-unknown-unknown", out, f"{crate}.wasm")
    if not os.path.exists(wasm):
        wasm = os.path.join(dest, "target", "wasm32-unknown-unknown", out, f"{package}.wasm")
    if r.returncode == 0 and os.path.exists(wasm):
        row["wasm_bytes"] = os.path.getsize(wasm)
        row["wasm_gzip_bytes"] = len(gzip.compress(open(wasm, "rb").read(), 9))
    else:
        row["wasm_error"] = (r.stderr or "").strip().splitlines()[-12:]
        log(f"{name}: wasm build failed:\n" + "\n".join(row["wasm_error"]))
    shutil.rmtree(os.path.join(dest, "target"), ignore_errors=True)
    return row


def builds(work, names=("ponko-hello", "bevy-hello", "ponko-small", "bevy-small")):
    return {name: build_project(name, work) for name in names}


# The tuned table's build rows: the small game of each engine.
TUNED_BUILDS = ("ponko-small-tuned", "bevy-small-tuned")


# ------------------------------------------------------------------ output


def verdict(p, b):
    """`ponko faster`, `bevy faster` or `within noise` (the ranges overlap)."""
    if p["max"] >= b["min"] and b["max"] >= p["min"]:
        return "within noise"
    return "Ponko faster" if p["median"] < b["median"] else "**Bevy faster**"


def fmt(v, unit="ms"):
    if unit == "ms":
        return f"{v:.3f}" if v < 10 else f"{v:.1f}"
    return f"{v:.2f}"


def table(doc, first=None):
    """The Markdown table of `doc`. `first`: the first table's document, for
    the tuned table's comparison column."""
    tuned = first is not None
    lines = []
    m = doc["machine"]
    build = ("the shipping profile (fat LTO, one codegen unit, panic = abort) on both sides" if tuned
             else "release builds")
    lines.append(
        f"Measured {m['date']} at `{m['ponko_rev']}`{' (with uncommitted changes)' if m['ponko_dirty'] else ''} "
        f"on {m['cpu']} ({m['cores']} cores, {m['memory_gb']} GB), {m['os']}, {m['rustc']}, "
        f"Bevy {m['bevy']}; {build}, {doc['runs']} runs per engine, one process at a time.\n"
    )
    by_source = {}
    for part, source in doc.get("power", {}).items():
        by_source.setdefault(source, []).append(part)
    if len(by_source) == 1:
        lines.append(f"Power: {next(iter(by_source))}.\n")
    elif by_source:
        # The machine changed power source between runs: say which rows ran on which.
        main = max(by_source, key=lambda src: len(by_source[src]))
        rest = "; ".join(f"{', '.join(parts)} on {src}" for src, parts in by_source.items() if src != main)
        lines.append(f"Power: {main}, except {rest}.\n")
    if doc.get("runtime"):
        if tuned:
            lines.append("| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | | First table: Bevy / Ponko |")
            lines.append("|---|---:|---:|---:|---|---|")
        else:
            lines.append("| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | |")
            lines.append("|---|---:|---:|---:|---|")
        for title, row in doc["runtime"].items():
            p, b = row["ponko"], row["bevy"]
            line = (
                f"| {title} | {fmt(p['median'])} ({fmt(p['min'])}–{fmt(p['max'])}) | "
                f"{fmt(b['median'])} ({fmt(b['min'])}–{fmt(b['max'])}) | {b['median'] / p['median']:.2f}× | {verdict(p, b)} |"
            )
            if tuned:
                old = first.get("runtime", {}).get(title)
                line += f" {old['bevy']['median'] / old['ponko']['median']:.2f}×, {verdict(old['ponko'], old['bevy'])} |" if old else " – |"
            lines.append(line)
        loads = [l["load"] for row in doc["runtime"].values() for l in row["loads"]]
        lines.append(f"\nMedians of {doc['runs']} runs, the range in brackets; 1-minute load {min(loads):.2f} to {max(loads):.2f}.")
    if doc.get("builds"):
        b = doc["builds"]
        med = lambda v: statistics.median(v) if v else float("nan")
        mb = lambda v: f"{v / 1e6:.1f} MB" if v else "–"
        if tuned:
            names = list(TUNED_BUILDS)
            lines.append("\n| | Ponko small game, tuned | Bevy small game, tuned |")
            lines.append("|---|---:|---:|")
            release = "Clean shipping-profile build (s, one run)"
            wasm = "wasm (shipping profile, before wasm-bindgen) / gzipped"
        else:
            names = ["ponko-hello", "bevy-hello", "ponko-small", "bevy-small"]
            lines.append("\n| | Ponko hello | Bevy hello | Ponko small game | Bevy small game |")
            lines.append("|---|---:|---:|---:|---:|")
            release = "Clean release build (s, one run)"
            wasm = "wasm (release, before wasm-bindgen) / gzipped"
        rng = lambda v: f" ({min(v):.1f}–{max(v):.1f})" if tuned and v else ""
        rows = [
            ("Clean debug build (s)", lambda r: f"{med(r['clean_debug']):.1f}{rng(r['clean_debug'])}"),
            ("Debug rebuild after a one-line change in src/lib.rs (s)", lambda r: f"{med(r['edit_lib_debug']):.2f}"),
            ("... in src/main.rs (s)", lambda r: f"{med(r['edit_main_debug']):.2f}"),
            (release, lambda r: f"{r['clean_release']:.1f}"),
            ("Binary / stripped", lambda r: f"{mb(r['release_binary_bytes'])} / {mb(r['release_stripped_bytes'])}"),
            (wasm, lambda r: f"{mb(r.get('wasm_bytes'))} / {mb(r.get('wasm_gzip_bytes'))}" if r.get("wasm_bytes") else "build failed"),
        ]
        if not tuned:
            rows[4] = ("Release binary / stripped", rows[4][1])
        for label, f in rows:
            lines.append(f"| {label} | " + " | ".join(f(b[n]) if n in b else "–" for n in names) + " |")
    return "\n".join(lines) + "\n"


def make_table(args, only, tuned):
    name = "scoreboard-tuned" if tuned else "scoreboard"
    path = os.path.join(RESULTS, f"{name}.json")
    doc = json.load(open(path)) if os.path.exists(path) else {}
    doc["machine"] = machine()
    doc["runs"] = args.runs
    if tuned:
        doc["tuned"] = {"ship": SHIP, "rapier": RAPIER_FEATURES, "bevy_groups": BEVY_GROUPS,
                        "paths": TUNED_PATHS, "ponko_small_features": PONKO_TUNED_FEATURES,
                        "bevy_small_features": BEVY_TUNED_FEATURES}
    power = doc.setdefault("power", {})
    if not args.builds:
        if tuned:
            build_tuned(only)
        else:
            build()
        fresh = runtime(args.runs, only, Binaries(tuned))
        doc["runtime"] = {**doc.get("runtime", {}), **fresh} if only else fresh
        for title in fresh:
            power[title] = doc["machine"]["power"]
    if not args.runtime and not only:
        os.makedirs(args.work, exist_ok=True)
        doc["builds"] = builds(args.work, TUNED_BUILDS) if tuned else builds(args.work)
        power["builds"] = doc["machine"]["power"]
    with open(path, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    first = None
    if tuned:
        first_path = os.path.join(RESULTS, "scoreboard.json")
        first = json.load(open(first_path)) if os.path.exists(first_path) else {}
    md = table(doc, first)
    open(os.path.join(RESULTS, f"{name}.md"), "w").write(md)
    print("\n" + md)
    log(f"written: {path} and {name}.md")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runtime", action="store_true", help="only the run-time workloads")
    ap.add_argument("--builds", action="store_true", help="only build times and sizes")
    ap.add_argument("--tuned", action="store_true", help="only the second table: both engines tuned")
    ap.add_argument("--only", default="", help="comma list: ecs,sprites,scenes,physics,ui,sim,first-frame")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--work", default=os.path.join(tempfile.gettempdir(), "ponko-scoreboard"),
                    help="where the build-time projects are made (outside the checkout)")
    args = ap.parse_args()
    only = {k for k in args.only.split(",") if k}
    os.makedirs(RESULTS, exist_ok=True)
    everything = not (args.runtime or args.builds or args.tuned or only)
    if not args.tuned:
        make_table(args, only, tuned=False)
    if args.tuned or everything:
        make_table(args, only, tuned=True)


if __name__ == "__main__":
    main()
