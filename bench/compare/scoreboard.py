#!/usr/bin/env python3
"""The cross-engine scoreboard: every workload on Ponko and on Bevy, one
command, results as JSON and as a Markdown table.

    python3 bench/compare/scoreboard.py              # everything (about an hour)
    python3 bench/compare/scoreboard.py --runtime    # only the run-time workloads
    python3 bench/compare/scoreboard.py --builds     # only build times and sizes
    python3 bench/compare/scoreboard.py --only sprites,ui
    python3 bench/compare/scoreboard.py --tuned      # the second table: both engines tuned
    python3 bench/compare/scoreboard.py --tuned --runtime
    python3 bench/compare/scoreboard.py --machine NAME --backend vulkan   # results/NAME/, both engines on Vulkan

On Windows, run it with `py` or `python` if `python3` is the Microsoft Store
stub; the scripts it calls need Git for Windows' `bash` and `perl`.

It builds `ponko_bench` (feature `render`) in the engine's workspace and
`bench/compare` in its own, once each, in release; then runs each workload
three times per engine, one process at a time, alternating the engines, with
the 1-minute load average written next to every run (it waits up to ten
minutes for a load under 3 first, and marks a run taken above it; on
Windows the load is the number of busy logical processors averaged over 5
seconds, see `scripts/cpu_load.py`). Build
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
second), and prints the tables. `--machine NAME` writes them to
`results/NAME/` instead, one folder per machine (and backend).
`--backend` sets `WGPU_BACKEND` for every run of both engines and records
it; the GPU adapters the programs report are recorded with the results. `bench/compare/README.md` describes every workload and
what is and isn't the same on both sides.
"""

import argparse
import gzip
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
# The checkout as a path inside Cargo.toml: TOML strings treat `\` as an
# escape, and Cargo takes forward slashes on Windows too.
ROOT_TOML = ROOT.replace("\\", "/")
RESULTS = os.path.join(HERE, "results")
WINDOWS = sys.platform == "win32"
EXE = ".exe" if WINDOWS else ""
COMPARE = os.path.join(HERE, "target", "release", "ponko_compare" + EXE)

sys.path.insert(0, os.path.join(ROOT, "scripts"))
import cpu_load  # noqa: E402  (scripts/cpu_load.py: the load figure on every OS)

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
        self.ponko_bench = os.path.join(ROOT, "target", out, "ponko_bench" + EXE)
        self.first_frame = os.path.join(ROOT, "target", out, "first_frame" + EXE)

    def compare(self, key):
        if not self.tuned:
            return COMPARE
        return os.path.join(HERE, "target", "ship", f"tuned-{BEVY_GROUPS[key]}{EXE}")

    def env(self, side, key):
        if not self.tuned:
            return {}
        env = dict(TUNED_PATHS[side].get(key, {}))
        if key == "first-frame" and side == "ponko":
            env["PONKO_COMPARE_EXIT"] = "1"
        return env


def log(msg):
    print(msg, flush=True)


class Failed(SystemExit):
    """A command that exited with an error; `reason` is its telling line."""

    def __init__(self, message, reason):
        super().__init__(message)
        self.reason = reason


def failure_reason(output):
    """The most telling line of a failed program's output: its first error
    or panic message, without terminal colors."""
    lines = [re.sub(r"\x1b\[[0-9;]*m", "", line).strip() for line in output.splitlines()]
    for marker in ("ERROR", "panicked at", "error:"):
        for i, line in enumerate(lines):
            if marker in line:
                # A panic's message is on the line after "panicked at".
                text = lines[i + 1] if marker == "panicked at" and i + 1 < len(lines) else line
                return text[-200:]
    return (lines[-1] if lines else "")[-200:]


def sh(cmd, cwd=ROOT, env=None, check=True, capture=True):
    full_env = dict(os.environ)
    full_env.update(env or {})
    r = subprocess.run(cmd, cwd=cwd, env=full_env, text=True, capture_output=capture)
    if check and r.returncode != 0:
        output = (r.stdout or "") + (r.stderr or "")
        sys.stderr.write(output)
        raise Failed(f"failed ({r.returncode}): {' '.join(cmd)}", failure_reason(output))
    return r


def out(cmd):
    try:
        return subprocess.run(cmd, text=True, capture_output=True).stdout.strip()
    except OSError:
        return ""


def load():
    """The 1-minute load average; on Windows the busy logical processors
    (scripts/cpu_load.py). -1 if it can't be read."""
    return cpu_load.load()


def text(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def powershell(command):
    """A PowerShell one-liner's output (Windows), or ""."""
    return out(["powershell", "-NoProfile", "-NonInteractive", "-Command", command])


def windows_machine():
    import ctypes
    import winreg

    def reg(path, name):
        try:
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, path) as key:
                return str(winreg.QueryValueEx(key, name)[0]).strip()
        except OSError:
            return ""

    class Memory(ctypes.Structure):
        _fields_ = [("length", ctypes.c_ulong), ("load", ctypes.c_ulong), ("total", ctypes.c_ulonglong),
                    ("avail", ctypes.c_ulonglong), ("page_total", ctypes.c_ulonglong),
                    ("page_avail", ctypes.c_ulonglong), ("virtual_total", ctypes.c_ulonglong),
                    ("virtual_avail", ctypes.c_ulonglong), ("extended", ctypes.c_ulonglong)]

    class Power(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("battery_flag", ctypes.c_ubyte), ("percent", ctypes.c_ubyte),
                    ("saver", ctypes.c_ubyte), ("life", ctypes.c_ulong), ("full_life", ctypes.c_ulong)]

    mem = Memory()
    mem.length = ctypes.sizeof(Memory)
    ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(mem))
    power = Power()
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(power)):
        source = {0: "Battery Power", 1: "AC Power"}.get(power.ac, "unknown")
        if power.battery_flag == 128:
            source += " (no battery)"
    else:
        source = "unknown"
    cpu_key = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"
    version = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"
    product = reg(version, "ProductName")
    build = reg(version, "CurrentBuildNumber")
    # ProductName still says "Windows 10" on Windows 11 (build 22000 and up).
    if build.isdigit() and int(build) >= 22000:
        product = product.replace("Windows 10", "Windows 11")
    cores = powershell("(Get-CimInstance Win32_Processor | Measure-Object -Sum NumberOfCores).Sum")
    gpus = powershell("Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name + ' | ' + $_.DriverVersion }")
    return {
        "cpu": reg(cpu_key, "ProcessorNameString") or platform.processor(),
        "cores": f"{cores} cores, {os.cpu_count()} threads" if cores else f"{os.cpu_count()} threads",
        "memory_gb": round(mem.total / 2**30),
        "os": f"{product} {reg(version, 'DisplayVersion')} (build {build}.{reg(version, 'UBR')})".replace("  ", " "),
        "power": source,
        "gpus": [dict(zip(("name", "driver"), (x.strip() for x in line.split("|")))) for line in gpus.splitlines()
                 if line.strip()],
    }


def linux_machine():
    cpuinfo = text("/proc/cpuinfo")
    model = re.search(r"^model name\s*:\s*(.+)$", cpuinfo, re.M)
    cores = {(m.group(1), m.group(2)) for m in re.finditer(
        r"physical id\s*:\s*(\d+).*?core id\s*:\s*(\d+)", cpuinfo, re.S)}
    mem = re.search(r"^MemTotal:\s*(\d+) kB", text("/proc/meminfo"), re.M)
    os_name = re.search(r'^PRETTY_NAME="?([^"\n]+)', text("/etc/os-release"), re.M)
    power = "unknown"
    base = "/sys/class/power_supply"
    supplies = sorted(os.listdir(base)) if os.path.isdir(base) else []
    for supply in supplies:
        kind = text(os.path.join(base, supply, "type")).strip()
        if kind == "Mains":
            power = "AC Power" if text(os.path.join(base, supply, "online")).strip() == "1" else "Battery Power"
    if power == "unknown" and not any(text(os.path.join(base, s, "type")).strip() == "Battery" for s in supplies):
        power = "AC Power (no battery)"
    gpus = [line.split(": ", 1)[-1] for line in out(["lspci"]).splitlines()
            if re.search(r"VGA|3D controller|Display controller", line)]
    return {
        "cpu": model.group(1).strip() if model else platform.processor(),
        "cores": f"{len(cores)} cores, {os.cpu_count()} threads" if cores else f"{os.cpu_count()} threads",
        "memory_gb": round(int(mem.group(1)) / 2**20) if mem else 0,
        "os": f"{os_name.group(1) if os_name else 'Linux'}, kernel {platform.release()}",
        "power": power,
        "gpus": [{"name": g, "driver": ""} for g in gpus],
    }


def mac_machine():
    gb = int(out(["sysctl", "-n", "hw.memsize"]) or 0) // 2**30
    perf, eff = out(["sysctl", "-n", "hw.perflevel0.physicalcpu"]), out(["sysctl", "-n", "hw.perflevel1.physicalcpu"])
    batt = out(["pmset", "-g", "batt"]).splitlines()
    return {
        "cpu": out(["sysctl", "-n", "machdep.cpu.brand_string"]),
        "cores": f"{perf} performance + {eff} efficiency" if perf else out(["sysctl", "-n", "hw.ncpu"]),
        "memory_gb": gb,
        "os": f"{out(['sw_vers', '-productName'])} {out(['sw_vers', '-productVersion'])}",
        "power": batt[0].split("'")[1] if batt and "'" in batt[0] else "unknown",
    }


def quiet():
    """Waits (at most 10 minutes) for a 1-minute load under 3."""
    waited = 0
    while load() > 3 and waited < 600:
        time.sleep(60)
        waited += 60
    l1 = load()
    return {"load": l1, "under_load": l1 > 3}


def machine():
    """CPU, cores, memory, OS, power source (and GPUs where the OS lists
    them), the toolchain and the revision."""
    if WINDOWS:
        about = windows_machine()
    elif sys.platform == "darwin":
        about = mac_machine()
    else:
        about = linux_machine()
    return {
        **about,
        "backend": os.environ.get("WGPU_BACKEND", "default"),
        "rustc": out(["rustc", "--version"]),
        "ponko_rev": out(["git", "-C", ROOT, "rev-parse", "--short=8", "HEAD"]),
        "ponko_dirty": bool(out(["git", "-C", ROOT, "status", "--porcelain", "--untracked-files=no"])),
        "bevy": bevy_version(),
        "date": time.strftime("%Y-%m-%d"),
    }


def bevy_version():
    m = re.search(r'^bevy = \{ version = "=([^"]+)"', text(os.path.join(HERE, "Cargo.toml")), re.M)
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
        shutil.copy(os.path.join(HERE, "target", "ship", "ponko_compare" + EXE),
                    os.path.join(HERE, "target", "ship", f"tuned-{group}{EXE}"))
    time.sleep(20)


# Every GPU adapter a program reported, as written in its document.
ADAPTERS = set()


def collect_adapters(value):
    if isinstance(value, dict):
        for key, inner in value.items():
            if key == "adapter" and inner:
                ADAPTERS.add(json.dumps(inner, sort_keys=True) if isinstance(inner, dict) else str(inner))
            else:
                collect_adapters(inner)
    elif isinstance(value, list):
        for inner in value:
            collect_adapters(inner)


def run_json(cmd, env=None, cwd=ROOT):
    """Runs `cmd` with PONKO_BENCH_JSON set; returns (document, load)."""
    state = quiet()
    fd, path = tempfile.mkstemp(suffix=".json")
    os.close(fd)
    try:
        sh(cmd, cwd=cwd, env={**(env or {}), "PONKO_BENCH_JSON": path})
        with open(path, encoding="utf-8") as f:
            doc = json.load(f)
        collect_adapters(doc)
        return doc, state
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
    if not values:  # every run failed
        return {"median": None, "min": None, "max": None, "runs": []}
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
                try:
                    value, state = f()
                except Failed as e:
                    # One engine failing a workload (a crash, the GPU out of
                    # memory) is a result too: record it and go on.
                    row = results.setdefault(title, {"unit": "ms", "ponko": [], "bevy": [], "loads": []})
                    row.setdefault("failures", {}).setdefault(side, []).append(e.reason)
                    log(f"  {side}: FAILED: {e.reason}")
                    continue
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
        text = read(toml)
        if base.startswith("ponko"):
            features = ", ".join(f'"{f}"' for f in PONKO_TUNED_FEATURES)
            old = f'ponko = {{ path = "{ROOT_TOML}" }}'
            assert old in text, text
            text = text.replace(old, f'ponko = {{ path = "{ROOT_TOML}", default-features = false, features = [{features}] }}')
        else:
            features = ", ".join(f'"{f}"' for f in BEVY_TUNED_FEATURES)
            old = 'bevy = { version = "=0.20.1" }'
            assert old in text, text
            text = text.replace(old, f'bevy = {{ version = "=0.20.1", default-features = false, features = [{features}] }}')
        text = text.replace("\n[workspace]\n", SHIP_PROFILE + "\n[workspace]\n")
        assert "[profile.ship]" in text, text
        write(toml, text)
    return dest


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def write(path, content):
    # newline="": keep \n line endings on Windows too.
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(content)


def make_base_project(name, work, folder):
    dest = os.path.join(work, folder)
    shutil.rmtree(dest, ignore_errors=True)
    if name == "ponko-small":
        template = os.path.join(ROOT, "crates", "ponko_cli", "templates", "2d")
        shutil.copytree(template, dest, ignore=shutil.ignore_patterns("tests", "*.md", ".mcp.json", "cargo-generate.toml"))
        liquid = os.path.join(dest, "Cargo.toml.liquid")
        text = read(liquid).replace("{{project-name}}", folder)
        # Whatever minor the template names (the release bump changes it).
        text = re.sub(r'ponko = \{ version = "[^"]+" \}', lambda _: f'ponko = {{ path = "{ROOT_TOML}" }}', text)
        write(os.path.join(dest, "Cargo.toml"), text + "\n[workspace]\n")
        os.unlink(liquid)
        for f in ("src/lib.rs", "src/main.rs"):
            p = os.path.join(dest, f)
            text = read(p).replace("{{project-name}}", folder).replace("{{crate_name}}", folder.replace("-", "_"))
            write(p, text)
    else:
        shutil.copytree(os.path.join(HERE, "projects", name), dest)
        toml = os.path.join(dest, "Cargo.toml")
        write(toml, read(toml).replace("PONKO_PATH", ROOT_TOML))
    return dest


def bash():
    """Git for Windows' bash on Windows (`bash` on the PATH may be WSL's,
    which can't run Windows' cargo); `bash` elsewhere."""
    if WINDOWS:
        for base in (os.environ.get("ProgramFiles", r"C:\Program Files"), os.environ.get("ProgramW6432", "")):
            candidate = os.path.join(base, "Git", "bin", "bash.exe")
            if base and os.path.exists(candidate):
                return candidate
    return "bash"


def build_project(name, work):
    dest = make_project(name, work)
    tuned = name.endswith("-tuned")
    # The tuned Bevy game's debug builds link Bevy dynamically, as Bevy's
    # setup guide advises while developing (`bevy/dynamic_linking`).
    env = {"BUILD_TIMES_CARGO_ARGS": "--features bevy/dynamic_linking"} if tuned and name.startswith("bevy") else {}
    log(f"{name}: build-times.sh (debug, 3 clean builds) in {dest} {env}")
    script = os.path.join(ROOT, "scripts", "build-times.sh").replace("\\", "/")
    env = {**env, "PYTHON": sys.executable}
    text = sh([bash(), script, dest.replace("\\", "/"), "dev", "3"], env=env).stdout
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
    package = re.search(r'^name = "([^"]+)"', read(os.path.join(dest, "Cargo.toml")), re.M).group(1)
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
    binary = os.path.join(dest, "target", out, package + EXE)
    row["release_binary_bytes"] = os.path.getsize(binary)
    if shutil.which("strip") and not WINDOWS:
        shutil.copy(binary, binary + ".stripped")
        sh(["strip", binary + ".stripped"])
        row["release_stripped_bytes"] = os.path.getsize(binary + ".stripped")
    else:
        # MSVC keeps debug info in a separate .pdb: the .exe is what ships.
        row["release_stripped_bytes"] = None
    # The wasm of the release build, as cargo writes it (before wasm-bindgen
    # and wasm-opt), and gzipped.
    r = sh(["cargo", "build", *profile, "--target", "wasm32-unknown-unknown"], cwd=dest, check=False)
    wasm = os.path.join(dest, "target", "wasm32-unknown-unknown", out, f"{crate}.wasm")
    if not os.path.exists(wasm):
        wasm = os.path.join(dest, "target", "wasm32-unknown-unknown", out, f"{package}.wasm")
    if r.returncode == 0 and os.path.exists(wasm):
        row["wasm_bytes"] = os.path.getsize(wasm)
        with open(wasm, "rb") as f:
            row["wasm_gzip_bytes"] = len(gzip.compress(f.read(), 9))
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
    cores = m["cores"] if "cores" in str(m["cores"]) else f"{m['cores']} cores"
    lines.append(
        f"Measured {m['date']} at `{m['ponko_rev']}`{' (with uncommitted changes)' if m['ponko_dirty'] else ''} "
        f"on {m['cpu']} ({cores}, {m['memory_gb']} GB), {m['os']}, {m['rustc']}, "
        f"Bevy {m['bevy']}; {build}, {doc['runs']} runs per engine, one process at a time.\n"
    )
    if m.get("gpus") or m.get("backend", "default") != "default":
        gpus = "; ".join(f"{g['name']} (driver {g['driver']})" if g.get("driver") else g["name"]
                         for g in m.get("gpus", []))
        lines.append(f"GPU: {gpus or 'not listed'}; backend: {m.get('backend', 'default')}.\n")
    if doc.get("adapters"):
        lines.append("Adapters the programs reported: " + "; ".join(f"`{a}`" for a in doc["adapters"]) + ".\n")
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
        notes = []
        for title, row in doc["runtime"].items():
            p, b = row["ponko"], row["bevy"]
            failures = row.get("failures", {})

            def cell(side, v):
                failed = len(failures.get(side, []))
                if v["median"] is None:
                    return f"failed ({failed} of {failed} runs)"
                text = f"{fmt(v['median'])} ({fmt(v['min'])}–{fmt(v['max'])})"
                return text + (f", {failed} run{'s' if failed > 1 else ''} failed" if failed else "")

            if p["median"] is None or b["median"] is None:
                who = " and ".join(n for n, v in (("Ponko", p), ("Bevy", b)) if v["median"] is None)
                ratio, said = "–", f"**{who} failed**"
            else:
                ratio, said = f"{b['median'] / p['median']:.2f}×", verdict(p, b)
            line = f"| {title} | {cell('ponko', p)} | {cell('bevy', b)} | {ratio} | {said} |"
            if tuned:
                old = first.get("runtime", {}).get(title)
                ok = old and old["ponko"]["median"] is not None and old["bevy"]["median"] is not None
                line += f" {old['bevy']['median'] / old['ponko']['median']:.2f}×, {verdict(old['ponko'], old['bevy'])} |" if ok else " – |"
            lines.append(line)
            for side, reasons in failures.items():
                for reason in sorted(set(reasons)):
                    notes.append(f"- {title}, {side.capitalize()}: `{reason}`")
        loads = [l["load"] for row in doc["runtime"].values() for l in row["loads"]]
        lines.append(f"\nMedians of {doc['runs']} runs, the range in brackets; 1-minute load {min(loads):.2f} to {max(loads):.2f}.")
        if notes:
            lines.append("\nFailed runs:\n\n" + "\n".join(notes))
    if doc.get("builds"):
        b = doc["builds"]
        med = lambda v: statistics.median(v) if v else float("nan")
        mb = lambda v: f"{v / 1e6:.1f} MB" if v else "–"
        stripped = lambda r: (f"{mb(r['release_binary_bytes'])} / {mb(r['release_stripped_bytes'])}"
                              if r.get("release_stripped_bytes") else f"{mb(r['release_binary_bytes'])} / –")
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
            ("Binary / stripped", stripped),
            (wasm, lambda r: f"{mb(r.get('wasm_bytes'))} / {mb(r.get('wasm_gzip_bytes'))}" if r.get("wasm_bytes") else "build failed"),
        ]
        if not tuned:
            rows[4] = ("Release binary / stripped", rows[4][1])
        for label, f in rows:
            lines.append(f"| {label} | " + " | ".join(f(b[n]) if n in b else "–" for n in names) + " |")
    return "\n".join(lines) + "\n"


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def make_table(args, only, tuned):
    name = "scoreboard-tuned" if tuned else "scoreboard"
    results = args.results
    path = os.path.join(results, f"{name}.json")
    doc = load_json(path) if os.path.exists(path) else {}
    ADAPTERS.clear()
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
        doc["adapters"] = sorted(ADAPTERS | (set(doc.get("adapters", [])) if only else set()))
    if not args.runtime and not only:
        os.makedirs(args.work, exist_ok=True)
        doc["builds"] = builds(args.work, TUNED_BUILDS) if tuned else builds(args.work)
        power["builds"] = doc["machine"]["power"]
    write(path, json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    first = None
    if tuned:
        first_path = os.path.join(results, "scoreboard.json")
        first = load_json(first_path) if os.path.exists(first_path) else {}
    md = table(doc, first)
    write(os.path.join(results, f"{name}.md"), md)
    print("\n" + md)
    log(f"written: {path} and {name}.md")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runtime", action="store_true", help="only the run-time workloads")
    ap.add_argument("--builds", action="store_true", help="only build times and sizes")
    ap.add_argument("--tuned", action="store_true", help="only the second table: both engines tuned")
    ap.add_argument("--only", default="", help="comma list: ecs,sprites,scenes,physics,ui,sim,first-frame")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--machine", default="",
                    help="write to results/MACHINE/ (one folder per machine and backend) instead of results/")
    ap.add_argument("--backend", default="", choices=["", "dx12", "vulkan", "metal", "gl"],
                    help="the GPU backend for both engines (sets WGPU_BACKEND); default: each engine's own choice")
    ap.add_argument("--work", default=os.path.join(tempfile.gettempdir(), "ponko-scoreboard"),
                    help="where the build-time projects are made (outside the checkout)")
    args = ap.parse_args()
    only = {k for k in args.only.split(",") if k}
    if args.backend:
        os.environ["WGPU_BACKEND"] = args.backend
    args.results = os.path.join(RESULTS, args.machine) if args.machine else RESULTS
    os.makedirs(args.results, exist_ok=True)
    everything = not (args.runtime or args.builds or args.tuned or only)
    if not args.tuned:
        make_table(args, only, tuned=False)
    if args.tuned or everything:
        make_table(args, only, tuned=True)


if __name__ == "__main__":
    main()
