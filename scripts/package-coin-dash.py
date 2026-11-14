#!/usr/bin/env python3
"""Packages the `coin_dash` example with `ponko package` and plays the package.

    python3 scripts/package-coin-dash.py package [--out DIR] [--profile release|dev] [--no-build]
    python3 scripts/package-coin-dash.py smoke --package DIR [--mode headless|window]
    python3 scripts/package-coin-dash.py selftest
    python3 scripts/package-coin-dash.py notices --crate ponko_ffi --title T --out FILE

Packaging is the generic `ponko package` command (crates/ponko_cli), which any
game uses; this script is the reference game's own entry point to it, kept for
CI and the release workflow. `package` runs
`ponko package --example coin_dash --format folder --no-smoke`, which lays out
the plain folder (the program, its assets, the licenses, CONTENT-NOTICES.txt,
README.txt and THIRD-PARTY-NOTICES.txt) as `<out>/coin-dash-<version>-<os>-<arch>/`.
Coin Dash's `[package.metadata.ponko.examples.coin_dash]` table in the root
Cargo.toml says what goes in.

`smoke` is the reference game's walk-through, more than the generic smoke run
of `ponko package`: it unpacks the folder somewhere else (a path with a space
and a non-ASCII letter), starts the game from an unrelated working directory,
and checks what it reports and leaves behind. With `--mode headless` the game
runs without a window or a GPU (every platform); with `--mode window` it opens
a real window and draws (needs a display and a GPU; software rendering is enough).

Exit codes: 0 passed, 1 the package failed a check, 2 unsupported (the window
mode found no display or no GPU, and --allow-unsupported was not given),
3 bad arguments or no build. See book/src/packaging.md.

`notices` runs `ponko notices`: only THIRD-PARTY-NOTICES.txt, for another
workspace crate's library (with its default features, for this platform). The
release workflow ships it in the C library archives (`--crate ponko_ffi`).
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLE = "coin_dash"
EXE = EXAMPLE + (".exe" if os.name == "nt" else "")
# The engine features the game is built with: see `settings::languages`.
FEATURES = "text-shaping"
ASSET_DIR = Path("assets") / EXAMPLE
COIN = ASSET_DIR / "coin.png"
LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE")


class Failure(Exception):
    """A check of the package failed (exit code 1)."""


class Unsupported(Exception):
    """The window mode can't run here (exit code 2)."""


def ponko(*args):
    """Runs the `ponko` command of this checkout (crates/ponko_cli)."""
    return subprocess.run(["cargo", "run", "--quiet", "-p", "ponko_cli", "--", *args], cwd=ROOT).returncode


# ---- package ----------------------------------------------------------------


def cmd_package(args):
    out = Path(args.out).resolve()
    command = ["package", "--example", EXAMPLE, "--profile", args.profile, "--out", str(out)]
    # Text shaping draws the game's Arabic (joined letters, right to left).
    command += ["--features", FEATURES, "--format", "folder", "--no-smoke"]
    if args.no_build:
        command.append("--no-build")
    code = ponko(*command)
    return 3 if code else 0


def cmd_notices(args):
    return ponko("notices", "-p", args.crate, "--title", args.title, "--out", str(Path(args.out).resolve()))


# ---- smoke ------------------------------------------------------------------

SCREENS = ["Title", "Settings", "Title", "Playing", "Result(Won)", "Title"]


def expected_default_save(home):
    """Where the game keeps progress with no PONKO_DATA_DIR (the engine's `ctx.storage`)."""
    if os.name == "nt":
        return home / "AppData" / "coin-dash" / "progress.json"
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "coin-dash" / "progress.json"
    return home / ".local" / "share" / "coin-dash" / "progress.json"


def clean_env():
    """The environment without anything that could point the game back at the checkout."""
    drop = ("CARGO", "COIN_DASH", "PONKO", "CRABBY", "RUST")
    return {k: v for k, v in os.environ.items() if not k.upper().startswith(drop)}


def check(condition, message):
    if not condition:
        raise Failure(message)


def check_package(package):
    for rel in (EXE, COIN, *LICENSE_FILES, "CONTENT-NOTICES.txt", "THIRD-PARTY-NOTICES.txt", "README.txt"):
        check((package / rel).is_file(), f"the package has no {rel}")
    third = (package / "THIRD-PARTY-NOTICES.txt").read_text(encoding="utf-8")
    check(len(third) > 10_000, "THIRD-PARTY-NOTICES.txt is nearly empty")
    for needed in ("ponko ", "serde ", "wgpu "):
        check(needed in third, f"THIRD-PARTY-NOTICES.txt does not mention {needed.strip()}")
    content = (package / "CONTENT-NOTICES.txt").read_text(encoding="utf-8")
    shipped = [p.relative_to(package / "assets") for p in (package / "assets").rglob("*") if p.is_file()]
    check(shipped, "the package ships no assets")
    for path in shipped:
        check(path.as_posix() in content, f"assets/{path.as_posix()} is shipped but not named in CONTENT-NOTICES.txt")


def play(exe, mode, workdir, env, report, timeout):
    """Runs the walk-through from `workdir`; returns (exit code, stdout, stderr)."""
    cmd = [str(exe), "--smoke", mode, "--report", str(report)]
    done = subprocess.run(
        cmd, cwd=workdir, env=env, text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=timeout
    )
    return done.returncode, done.stdout, done.stderr


def read_report(path, label):
    check(path.is_file(), f"{label}: the game wrote no report")
    return json.loads(path.read_text(encoding="utf-8"))


def check_report(report, label, mode, progress, rounds):
    check(report["finished"], f"{label}: the walk-through did not finish: {report}")
    check(report["mode"] == mode, f"{label}: mode {report['mode']}")
    check(report["screens"] == SCREENS, f"{label}: screens {report['screens']}")
    check(report["asset_size"] == [32, 32], f"{label}: the coin picture came back as {report['asset_size']}")
    check(report["progress_at_start"] == progress, f"{label}: progress was {report['progress_at_start']}, not {progress}")
    check((report["rounds_played"], report["rounds_won"]) == (rounds, rounds), f"{label}: rounds {report}")


def check_progress_file(path, label, rounds):
    check(path.is_file(), f"{label}: no progress file at {path}")
    data = json.loads(path.read_text(encoding="utf-8"))
    check(data["version"] == 1 and data["rounds_won"] == rounds, f"{label}: progress file {data}")


def smoke_headless(package, work, timeout):
    exe = package / EXE
    cwd = work / "elsewhere"
    cwd.mkdir()
    env = clean_env()

    # 1. A first run, saving where the environment says (PONKO_DATA_DIR
    # replaces the system's data folder; the game's app id is added below it).
    data = work / "data" / "coin-dash"
    save = data / "progress.json"
    env["PONKO_DATA_DIR"] = str(work / "data")
    code, out, err = play(exe, "headless", cwd, env, work / "report1.json", timeout)
    check(code == 0, f"first run exited {code}:\n{err}{out}")
    check_report(read_report(work / "report1.json", "first run"), "first run", "headless", "Fresh", 1)
    check_progress_file(save, "first run", 1)
    check((data / "settings.json").is_file(), "leaving the settings did not save them")
    print("  ok: first run (assets found from an unrelated folder, progress and settings saved)")

    # 2. A second run starts from what the first left.
    code, out, err = play(exe, "headless", cwd, env, work / "report2.json", timeout)
    check(code == 0, f"second run exited {code}:\n{err}{out}")
    report = read_report(work / "report2.json", "second run")
    check_report(report, "second run", "headless", "Loaded", 2)
    check(report["settings_at_start"] == "Loaded", f"second run: settings were {report['settings_at_start']}")
    check_progress_file(save, "second run", 2)
    print("  ok: second run (progress and settings reloaded)")

    # 3. A damaged file is set aside and the game starts over.
    save.write_text("{ not a save", encoding="utf-8")
    code, out, err = play(exe, "headless", cwd, env, work / "report3.json", timeout)
    check(code == 0, f"run with a damaged save exited {code}:\n{err}{out}")
    check_report(read_report(work / "report3.json", "damaged save"), "damaged save", "headless", "Recovered", 1)
    check((data / "progress.json.bad").read_text(encoding="utf-8") == "{ not a save", "the damaged file was not kept")
    check_progress_file(save, "damaged save", 1)
    print("  ok: a damaged save is recovered from")

    # 4. The default place: no PONKO_DATA_DIR, a home folder of our own.
    home = work / "home"
    home.mkdir()
    env = clean_env()
    env.update(HOME=str(home), USERPROFILE=str(home), APPDATA=str(home / "AppData"))
    env.pop("XDG_DATA_HOME", None)
    code, out, err = play(exe, "headless", cwd, env, work / "report4.json", timeout)
    check(code == 0, f"run with the default save place exited {code}:\n{err}{out}")
    default = expected_default_save(home)
    check_progress_file(default, "default place", 1)
    check((default.parent / "settings.json").is_file(), "settings were not saved in the default place")
    print(f"  ok: default save place ({default.parent.name}/ under the data folder)")


def smoke_missing_assets(package, work, timeout):
    """A package without its assets must fail loudly, naming the file."""
    broken = work / "no-assets"
    shutil.copytree(package, broken)
    shutil.rmtree(broken / "assets")
    env = clean_env()
    env["PONKO_DATA_DIR"] = str(work / "missing")
    cwd = work / "elsewhere"
    code, out, err = play(broken / EXE, "headless", cwd, env, work / "report-missing.json", timeout)
    check(code != 0, "a package without assets still ran")
    check("coin_dash/coin.png" in err, f"the failure does not name the missing file:\n{err}")
    check(not (work / "report-missing.json").exists(), "a report was written without the assets")
    print("  ok: missing assets fail with a message naming coin_dash/coin.png")


# The engine's own messages when it cannot get a GPU or a window. Only these
# mean "unsupported here"; any other crash is a failure, whatever platform
# error it carries.
NO_WINDOW = (
    "ponko: no compatible GPU found",
    "ponko: could not create a GPU surface",
    "ponko: could not open the GPU device",
    "ponko: could not start the event loop",
    "ponko: could not create the window",
)


def is_unsupported(err):
    return any(text in err for text in NO_WINDOW)


def smoke_window(package, work, timeout, allow_unsupported):
    exe = package / EXE
    cwd = work / "elsewhere"
    cwd.mkdir()
    env = clean_env()
    save = work / "data" / "coin-dash" / "progress.json"
    env["PONKO_DATA_DIR"] = str(work / "data")
    report = work / "report-window.json"
    try:
        code, out, err = play(exe, "window", cwd, env, report, timeout)
    except subprocess.TimeoutExpired:
        raise Failure(f"the window run did not finish in {timeout} s")
    if code != 0 and not report.exists() and is_unsupported(err):
        if allow_unsupported:
            reason = next(line for line in err.splitlines() if is_unsupported(line))[:200]
            if os.environ.get("GITHUB_ACTIONS"):
                # Shows on the run's summary, so a skipped window is never silent.
                print(f"::warning title=coin_dash window smoke skipped::{reason}")
            raise Unsupported("no display or GPU here: " + reason)
        raise Failure("the window could not open (use --allow-unsupported where that is expected):\n" + err)
    check(code == 0, f"the window run exited {code}:\n{err}{out}")
    data = read_report(report, "window run")
    check_report(data, "window run", "window", "Fresh", 1)
    check(data["frames"] > 0, "window run: no frame was drawn")
    check_progress_file(save, "window run", 1)
    print(f"  ok: window run ({data['frames']} frames drawn, {data['ticks']} ticks)")


def cmd_smoke(args):
    package = Path(args.package).resolve()
    if not (package / EXE).is_file():
        # The folder `package --out` made: use the one package in it.
        inside = [p for p in package.iterdir() if (p / EXE).is_file()] if package.is_dir() else []
        if len(inside) == 1:
            package = inside[0]
    if not (package / EXE).is_file():
        print(f"smoke: {package / EXE} does not exist (run `package` first)", file=sys.stderr)
        return 3
    # Unpack to a place outside the checkout, with a space and a non-ASCII letter in its path.
    work = Path(tempfile.mkdtemp(prefix="coin dash ünpacked "))
    unpacked = work / "unpacked" / package.name
    try:
        shutil.copytree(package, unpacked)
        print(f"smoke: {args.mode}, unpacked at {unpacked}")
        check_package(unpacked)
        print("  ok: files, licenses and notices are in the package")
        if args.mode == "headless":
            smoke_headless(unpacked, work, args.timeout)
            smoke_missing_assets(unpacked, work, args.timeout)
        else:
            smoke_window(unpacked, work, args.timeout, args.allow_unsupported)
    except Unsupported as e:
        print(f"UNSUPPORTED: {e}")
        return 0 if args.allow_unsupported else 2
    except Failure as e:
        print(f"FAIL: {e}")
        return 1
    except subprocess.TimeoutExpired as e:
        print(f"FAIL: {' '.join(map(str, e.cmd))} did not finish in {args.timeout} s")
        return 1
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print("PASS")
    return 0


def selftest(args):
    """Checks the script's own decisions with made-up inputs (no build, no window)."""
    global play

    # The packaging's own decisions (licenses, layouts, archives) are tests of
    # `ponko package`: `cargo test -p ponko_cli`.

    # Which window-run errors are "unsupported here" and which are failures.
    for text in NO_WINDOW:
        check(is_unsupported(f"thread 'main' panicked at x.rs:1:1:\n{text}: details"), f"{text!r} is not unsupported")
    for text in ("OsError: something broke", "NotSupported", "Error: DISPLAY", "segmentation fault", "assertion failed"):
        check(not is_unsupported(text), f"{text!r} wrongly counts as unsupported")
    print("  ok: only the engine's own no-GPU / no-window messages are unsupported")

    # smoke_window with a fake program: crashes with a platform error are failures,
    # the engine's messages are unsupported (or a failure without --allow-unsupported).
    real_play = play
    try:
        def fake(stderr):
            return lambda *a, **k: (1, "", stderr)

        with tempfile.TemporaryDirectory() as tmp:
            for stderr, flag, expected in (
                ("thread 'main' panicked: OsError(NotSupported)\n", True, Failure),
                ("thread 'main' panicked: ponko: could not start the event loop: x\nnote: run with RUST_BACKTRACE=1\n", True, Unsupported),
                ("thread 'main' panicked: ponko: no compatible GPU found: x\n", False, Failure),
            ):
                play = fake(stderr)
                work = Path(tmp) / str(abs(hash((stderr, flag))))
                work.mkdir()
                try:
                    smoke_window(work, work, 5, flag)
                except expected:
                    pass
                else:
                    raise Failure(f"smoke_window did not raise {expected.__name__} for {stderr!r}")
    finally:
        play = real_play
    print("  ok: a window crash is a failure; no GPU/window is unsupported only with --allow-unsupported")
    print("PASS")
    return 0


def main():
    # The unpack folder has a non-ASCII letter in its name; never fail on printing it.
    sys.stdout.reconfigure(errors="replace")
    sys.stderr.reconfigure(errors="replace")
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("package", help="build the example and lay out the package folder (ponko package)")
    p.add_argument("--out", default=str(ROOT / "target" / "package"), help="folder to create the package in")
    p.add_argument("--profile", choices=["release", "dev"], default="release")
    p.add_argument("--no-build", action="store_true", help="use the example as already built")
    s = sub.add_parser("smoke", help="unpack a package elsewhere and play it")
    s.add_argument("--package", required=True, help="the folder `package` made")
    s.add_argument("--mode", choices=["headless", "window"], default="headless")
    s.add_argument("--allow-unsupported", action="store_true", help="the window mode passes where no display or GPU exists")
    s.add_argument("--timeout", type=int, default=240, help="seconds each run may take")
    sub.add_parser("selftest", help="check the script's own decisions with made-up inputs")
    n = sub.add_parser("notices", help="write THIRD-PARTY-NOTICES.txt for a workspace crate's library (ponko notices)")
    n.add_argument("--crate", required=True, help="the workspace crate, such as ponko_ffi")
    n.add_argument("--title", required=True, help="what the notices are for, such as 'the Ponko C library'")
    n.add_argument("--out", required=True, help="the file to write")
    args = parser.parse_args()
    commands = {"package": cmd_package, "smoke": cmd_smoke, "selftest": selftest, "notices": cmd_notices}
    return commands[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
