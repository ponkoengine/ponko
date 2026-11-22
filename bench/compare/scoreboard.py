#!/usr/bin/env python3
"""The cross-engine scoreboard: every workload on Ponko and on Bevy, one
command, results as JSON and as a Markdown table.

    python3 bench/compare/scoreboard.py              # everything (about an hour)
    python3 bench/compare/scoreboard.py --runtime    # only the run-time workloads
    python3 bench/compare/scoreboard.py --builds     # only build times and sizes
    python3 bench/compare/scoreboard.py --only sprites,ui

It builds `ponko_bench` (feature `render`) in the engine's workspace and
`bench/compare` in its own, once each, in release; then runs each workload
three times per engine, one process at a time, alternating the engines, with
the 1-minute load average written next to every run (it waits up to ten
minutes for a load under 3 first, and marks a run taken above it). Build
times and sizes come from `scripts/build-times.sh` (debug: three clean
builds, then rebuilds after one-line edits) and one clean release build and
one wasm build per project, each project made fresh in a folder outside
the checkout.

Writes `bench/compare/results/scoreboard.json` and `scoreboard.md`, and
prints the table. `bench/compare/README.md` describes every workload and
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


def runtime_workloads():
    """(key, rows, ponko run, bevy run): a run returns {row title: p50}."""

    def ecs():
        doc, state = run_json([COMPARE, "ecs", ECS_FILTER])
        return {t: (p50_of(doc, p), p50_of(doc, b)) for t, p, b in ECS_ROWS}, state

    def pair(title, ponko_cmd, bevy_cmd, ponko_name, bevy_name=None, env=None):
        def ponko():
            doc, state = run_json(ponko_cmd, env)
            return p50_of(doc, ponko_name), state

        def bevy():
            doc, state = run_json(bevy_cmd, env, cwd=HERE)
            return p50_of(doc, bevy_name or ponko_name), state

        return (title, ponko, bevy)

    rows = [
        pair("Sprites: 100k moving, one frame drawn", [PONKO_BENCH, "compare/sprites-100k"],
             [COMPARE, "sprites", "compare/sprites-100k"], "compare/sprites-100k"),
        pair("Sprites: 100k moving, a different tint each, one frame drawn",
             [PONKO_BENCH, "compare/sprites-100k-tinted"],
             [COMPARE, "sprites", "compare/sprites-100k-tinted"], "compare/sprites-100k-tinted"),
        pair("Sprites: 1M moving, one frame drawn", [PONKO_BENCH, "compare/sprites-1m"],
             [COMPARE, "sprites", "compare/sprites-1m"], "compare/sprites-1m"),
        pair("3D: 100k moving meshes (16 kinds, instanced) with a 4-cascade sun shadow",
             [PONKO_BENCH, "render/scene dynamic-world"], [COMPARE, "scenes", "dynamic-world"],
             "render/scene dynamic-world frame", env={**SCENE_ENV, "PONKO_RENDER_SCALE": "5"}),
        pair("3D: 1,000 skinned characters (16 joints) with a sun shadow",
             [PONKO_BENCH, "render/scene animated"], [COMPARE, "scenes", "animated"],
             "render/scene animated frame", env={**SCENE_ENV, "PONKO_RENDER_SCALE": "2.5"}),
        pair("2D physics: 10k boxes falling into a pile, one tick", [PONKO_BENCH, "compare/physics-10k"],
             [COMPARE, "physics"], "compare/physics-10k"),
        pair("UI: 10k nodes, still, one frame drawn", [PONKO_BENCH, "compare/ui-10k-still"],
             [COMPARE, "ui", "compare/ui-10k-still"], "compare/ui-10k-still"),
        pair("UI: 10k nodes, all recolored every frame, one frame drawn", [PONKO_BENCH, "compare/ui-10k-recolor"],
             [COMPARE, "ui", "compare/ui-10k-recolor"], "compare/ui-10k-recolor"),
        pair("Headless sim: 1M entities, one tick", [PONKO_BENCH, "compare/sim-1m"], [COMPARE, "sim"],
             "compare/sim-1m"),
    ]
    keys = ["sprites", "sprites", "sprites", "scenes", "scenes", "physics", "ui", "ui", "sim"]
    return ecs, list(zip(keys, rows))


def first_frame(cmd, cwd):
    state = quiet()
    start = time.perf_counter()
    sh(cmd, cwd=cwd)
    return (time.perf_counter() - start) * 1e3, state


def summary(values):
    return {"median": statistics.median(values), "min": min(values), "max": max(values), "runs": values}


def runtime(runs, only):
    results = {}
    ecs, pairs = runtime_workloads()

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
            if i == 0:
                # One untimed launch each first: the first launch after a
                # build reads the binary from disk and fills the driver's
                # shader cache, which a player's second launch doesn't pay.
                sh([FIRST_FRAME], cwd=ROOT)
                sh([COMPARE, "first-frame"], cwd=HERE)
            for side, cmd, cwd in [("ponko", [FIRST_FRAME], ROOT), ("bevy", [COMPARE, "first-frame"], HERE)]:
                value, state = first_frame(cmd, cwd)
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
    dest = os.path.join(work, name)
    shutil.rmtree(dest, ignore_errors=True)
    if name == "ponko-small":
        template = os.path.join(ROOT, "crates", "ponko_cli", "templates", "2d")
        shutil.copytree(template, dest, ignore=shutil.ignore_patterns("tests", "*.md", ".mcp.json", "cargo-generate.toml"))
        liquid = os.path.join(dest, "Cargo.toml.liquid")
        text = open(liquid).read().replace("{{project-name}}", name)
        text = text.replace('ponko = { version = "0.1" }', f'ponko = {{ path = "{ROOT}" }}')
        open(os.path.join(dest, "Cargo.toml"), "w").write(text + "\n[workspace]\n")
        os.unlink(liquid)
        for f in ("src/lib.rs", "src/main.rs"):
            p = os.path.join(dest, f)
            text = open(p).read().replace("{{project-name}}", name).replace("{{crate_name}}", "ponko_small")
            open(p, "w").write(text)
    else:
        shutil.copytree(os.path.join(HERE, "projects", name), dest)
        toml = os.path.join(dest, "Cargo.toml")
        text = open(toml).read().replace("PONKO_PATH", ROOT)
        open(toml, "w").write(text)
    return dest


def build_project(name, work):
    dest = make_project(name, work)
    log(f"{name}: build-times.sh (debug, 3 clean builds) in {dest}")
    text = sh(["bash", os.path.join(ROOT, "scripts", "build-times.sh"), dest, "dev", "3"]).stdout
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
    crate = name.replace("-", "_")
    # One clean release build: its time and the binary's size.
    shutil.rmtree(os.path.join(dest, "target"), ignore_errors=True)
    time.sleep(20)
    row["release_load"] = load()
    r = sh(["cargo", "build", "--release"], cwd=dest)
    row["clean_release"] = seconds(re.search(r"Finished .* in (.+)", r.stderr).group(1))
    binary = os.path.join(dest, "target", "release", name)
    row["release_binary_bytes"] = os.path.getsize(binary)
    shutil.copy(binary, binary + ".stripped")
    sh(["strip", binary + ".stripped"])
    row["release_stripped_bytes"] = os.path.getsize(binary + ".stripped")
    # The wasm of the release build, as cargo writes it (before wasm-bindgen
    # and wasm-opt), and gzipped.
    r = sh(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown"], cwd=dest, check=False)
    wasm = os.path.join(dest, "target", "wasm32-unknown-unknown", "release", f"{crate}.wasm")
    if not os.path.exists(wasm):
        wasm = os.path.join(dest, "target", "wasm32-unknown-unknown", "release", f"{name}.wasm")
    if r.returncode == 0 and os.path.exists(wasm):
        row["wasm_bytes"] = os.path.getsize(wasm)
        row["wasm_gzip_bytes"] = len(gzip.compress(open(wasm, "rb").read(), 9))
    else:
        row["wasm_error"] = (r.stderr or "").strip().splitlines()[-12:]
        log(f"{name}: wasm build failed:\n" + "\n".join(row["wasm_error"]))
    shutil.rmtree(os.path.join(dest, "target"), ignore_errors=True)
    return row


def builds(work):
    return {name: build_project(name, work) for name in ["ponko-hello", "bevy-hello", "ponko-small", "bevy-small"]}


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


def table(doc):
    lines = []
    m = doc["machine"]
    lines.append(
        f"Measured {m['date']} at `{m['ponko_rev']}`{' (with uncommitted changes)' if m['ponko_dirty'] else ''} "
        f"on {m['cpu']} ({m['cores']} cores, {m['memory_gb']} GB), {m['os']}, {m['rustc']}, "
        f"Bevy {m['bevy']}; release builds, {doc['runs']} runs per engine, one process at a time.\n"
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
        lines.append("| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | |")
        lines.append("|---|---:|---:|---:|---|")
        for title, row in doc["runtime"].items():
            p, b = row["ponko"], row["bevy"]
            lines.append(
                f"| {title} | {fmt(p['median'])} ({fmt(p['min'])}–{fmt(p['max'])}) | "
                f"{fmt(b['median'])} ({fmt(b['min'])}–{fmt(b['max'])}) | {b['median'] / p['median']:.2f}× | {verdict(p, b)} |"
            )
        loads = [l["load"] for row in doc["runtime"].values() for l in row["loads"]]
        lines.append(f"\nMedians of {doc['runs']} runs, the range in brackets; 1-minute load {min(loads):.2f} to {max(loads):.2f}.")
    if doc.get("builds"):
        b = doc["builds"]
        med = lambda v: statistics.median(v) if v else float("nan")
        mb = lambda v: f"{v / 1e6:.1f} MB" if v else "–"
        lines.append("\n| | Ponko hello | Bevy hello | Ponko small game | Bevy small game |")
        lines.append("|---|---:|---:|---:|---:|")
        names = ["ponko-hello", "bevy-hello", "ponko-small", "bevy-small"]
        rows = [
            ("Clean debug build (s)", lambda r: f"{med(r['clean_debug']):.1f}"),
            ("Debug rebuild after a one-line change in src/lib.rs (s)", lambda r: f"{med(r['edit_lib_debug']):.2f}"),
            ("... in src/main.rs (s)", lambda r: f"{med(r['edit_main_debug']):.2f}"),
            ("Clean release build (s, one run)", lambda r: f"{r['clean_release']:.1f}"),
            ("Release binary / stripped", lambda r: f"{mb(r['release_binary_bytes'])} / {mb(r['release_stripped_bytes'])}"),
            ("wasm (release, before wasm-bindgen) / gzipped", lambda r: f"{mb(r.get('wasm_bytes'))} / {mb(r.get('wasm_gzip_bytes'))}" if r.get("wasm_bytes") else "build failed"),
        ]
        for label, f in rows:
            lines.append(f"| {label} | " + " | ".join(f(b[n]) if n in b else "–" for n in names) + " |")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runtime", action="store_true", help="only the run-time workloads")
    ap.add_argument("--builds", action="store_true", help="only build times and sizes")
    ap.add_argument("--only", default="", help="comma list: ecs,sprites,scenes,physics,ui,sim,first-frame")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--work", default=os.path.join(tempfile.gettempdir(), "ponko-scoreboard"),
                    help="where the build-time projects are made (outside the checkout)")
    args = ap.parse_args()
    only = {k for k in args.only.split(",") if k}
    os.makedirs(RESULTS, exist_ok=True)
    path = os.path.join(RESULTS, "scoreboard.json")
    doc = json.load(open(path)) if os.path.exists(path) else {}
    doc["machine"] = machine()
    doc["runs"] = args.runs
    power = doc.setdefault("power", {})
    if not args.builds:
        build()
        fresh = runtime(args.runs, only)
        doc["runtime"] = {**doc.get("runtime", {}), **fresh} if only else fresh
        for title in fresh:
            power[title] = doc["machine"]["power"]
    if not args.runtime and not only:
        os.makedirs(args.work, exist_ok=True)
        doc["builds"] = builds(args.work)
        power["builds"] = doc["machine"]["power"]
    with open(path, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    md = table(doc)
    open(os.path.join(RESULTS, "scoreboard.md"), "w").write(md)
    print("\n" + md)
    log(f"written: {path} and scoreboard.md")


if __name__ == "__main__":
    main()
