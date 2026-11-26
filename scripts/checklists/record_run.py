#!/usr/bin/env python3
"""Turn what a check run produced into a record to paste into an issue.

    python3 scripts/checklists/record_run.py checks [--results FILE] [--hardware TEXT]
    python3 scripts/checklists/record_run.py render LOG [LOG ...]
    python3 scripts/checklists/record_run.py machine

`checks` reads the `results.json` that `python3 scripts/run-checks.py` wrote
(default `target/check-results/results.json`) and prints a Markdown block:
the date, revision, machine, the counts, every failed step and every GPU test
that skipped. `render` counts the lines the renderer's GPU tests print with
`--nocapture` (`render test ran [...] on <adapter>` and `render test SKIPPED
[...]: <why>`) and the `test result:` lines of cargo. `machine` prints the
machine description alone. Nothing is sent anywhere: copy the output.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

RAN = re.compile(r"render test ran \[(?P<name>[^\]]+)\] on (?P<adapter>.+)$")
SKIPPED = re.compile(r"render test SKIPPED \[(?P<name>[^\]]+)\]: (?P<why>.+)$")
RESULT = re.compile(r"test result: (?P<state>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; (?P<ignored>\d+) ignored")


def run_text(*command):
    """The output of a command, or '' when it is missing or fails."""
    if not shutil.which(command[0]):
        return ""
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=20, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return ""
    return done.stdout.strip() if done.returncode == 0 else ""


def os_release():
    """`PRETTY_NAME` of /etc/os-release on Linux, else ''."""
    try:
        text = Path("/etc/os-release").read_text(encoding="utf-8")
    except OSError:
        return ""
    found = re.search(r'^PRETTY_NAME="?([^"\n]*)"?$', text, re.M)
    return found[1] if found else ""


def machine_lines():
    """What can be read from the machine itself. The person adds the rest."""
    system = platform.system()
    lines = [f"- OS: {os_release() or platform.platform()}", f"- Architecture: {platform.machine()}"]
    if system == "Linux":
        lines.append(f"- Kernel: {platform.release()}")
        session = os.environ.get("XDG_SESSION_TYPE") or "unknown"
        desktop = os.environ.get("XDG_CURRENT_DESKTOP") or "unknown"
        lines.append(f"- Session: {session}, desktop {desktop}")
        gpus = [line for line in run_text("lspci").splitlines() if re.search(r"VGA|3D|Display", line)]
        lines += [f"- GPU (lspci): {re.sub(r'^[0-9a-f:.]+ ', '', gpu)}" for gpu in gpus]
        if not gpus:
            lines.append("- GPU: (lspci not found: write it down)")
        driver = run_text("glxinfo", "-B")
        found = re.search(r"OpenGL renderer string: (.+)", driver)
        if found:
            lines.append(f"- OpenGL renderer: {found[1]}")
    elif system == "Darwin":
        chip = run_text("sysctl", "-n", "machdep.cpu.brand_string")
        lines.append(f"- Chip: {chip or 'unknown'}")
    return lines


def checks_record(report, hardware):
    jobs = report["jobs"]
    passed = failed = skipped = 0
    failures, gpu_skips = [], set()
    for job in jobs:
        label = job.get("name") or job.get("job") or "job"
        for step in job["steps"]:
            code = step.get("returncode")
            if code is None:
                skipped += 1
            elif code == 0:
                passed += 1
            else:
                failed += 1
                failures.append(f"{label} / {step['name']}")
            gpu_skips.update(step.get("gpu_skips", []))
    result = "PASS" if report.get("exit_code") == 0 else "FAIL" if report.get("exit_code") == 1 else "NO CHECKS RAN"
    lines = [
        f"**Result: {result}** ({passed} passed, {failed} failed, {skipped} skipped)",
        "",
        f"- Date finished: {report['finished_at']}",
        f"- Revision: `{report['revision']}`" + (" (dirty checkout)" if report.get("dirty") else ""),
        f"- Run on: {report['os']} {report['os_version']} ({report['architecture']})",
    ]
    for key, version in report.get("toolchains", {}).items():
        lines.append(f"- Rust {key}: {(version or 'unavailable').splitlines()[0]}")
    lines.append("- GPU adapters the tests reported: " + (", ".join(report.get("gpu_adapters", [])) or "none"))
    lines += machine_lines()
    lines.append(f"- Hardware (you fill in: CPU, RAM, GPU, driver): {hardware or '________'}")
    lines += ["", "Failed steps (each needs an issue):"]
    lines += [f"- {name}" for name in failures] or ["- none"]
    lines += ["", "GPU tests that skipped:"]
    lines += [f"- {why}" for why in sorted(gpu_skips)] or ["- none reported"]
    return "\n".join(lines) + "\n"


def count_render(text):
    """Counts from the output of a `render_test` run with `--nocapture`."""
    ran, skipped, adapters, reasons = set(), set(), {}, {}
    totals = {"passed": 0, "failed": 0, "ignored": 0}
    for raw in text.splitlines():
        line = raw.strip()
        if found := RAN.search(line):
            ran.add(found["name"])
            adapters[found["adapter"]] = adapters.get(found["adapter"], 0) + 1
        elif found := SKIPPED.search(line):
            skipped.add(found["name"])
            reasons[found["why"]] = reasons.get(found["why"], 0) + 1
        elif found := RESULT.search(line):
            for key in totals:
                totals[key] += int(found[key])
    return {"ran": len(ran), "skipped": len(skipped), "adapters": adapters, "reasons": reasons, **totals}


def render_record(name, counts):
    lines = [
        f"**{name}**: {counts['passed']} passed, {counts['failed']} failed, {counts['ignored']} ignored by cargo; "
        f"the tests reported {counts['ran']} ran on a GPU and {counts['skipped']} skipped.",
    ]
    lines += [f"- adapter: {adapter} ({n} tests)" for adapter, n in sorted(counts["adapters"].items())]
    lines += [f"- skipped, {n}x: {why}" for why, n in sorted(counts["reasons"].items(), key=lambda kv: -kv[1])]
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    one = sub.add_parser("checks", help="a record from a run-checks.py results.json")
    one.add_argument("--results", type=Path, default=Path("target/check-results/results.json"))
    one.add_argument("--hardware", default="")
    two = sub.add_parser("render", help="count the GPU tests in saved cargo output")
    two.add_argument("logs", nargs="+", type=Path)
    sub.add_parser("machine", help="print what can be read from this machine")
    args = parser.parse_args(argv)
    if args.command == "checks":
        try:
            report = json.loads(args.results.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            print(f"cannot read {args.results}: {error}", file=sys.stderr)
            return 2
        print(f"Record, {datetime.date.today().isoformat()}\n")
        print(checks_record(report, args.hardware), end="")
    elif args.command == "render":
        for log in args.logs:
            try:
                text = log.read_text(encoding="utf-8", errors="replace")
            except OSError as error:
                print(f"cannot read {log}: {error}", file=sys.stderr)
                return 2
            print(render_record(log.name, count_render(text)))
    else:
        print("\n".join(machine_lines()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
