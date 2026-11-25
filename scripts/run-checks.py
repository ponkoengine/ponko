#!/usr/bin/env python3
"""Run the local Ponko check inventory and write JSON and text reports."""

import argparse
import datetime
import functools
import itertools
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
import time

ROOT = Path(__file__).resolve().parents[1]
SYSTEMS = {"Darwin": "macOS", "Linux": "Linux", "Windows": "Windows"}
RUNNERS = {"ubuntu": "Linux", "macos": "macOS", "windows": "Windows"}
EXPRESSION = re.compile(r"\$\{\{\s*(.*?)\s*\}\}")
GPU_RAN = re.compile(r"render test ran \[[^\n]+?\] on (.+?)(?: as the \S+ tier)?$")
GPU_SKIPPED = re.compile(r"render test SKIPPED \[[^\n]+?\]: (.+)")


def fields(lines, indent, sequence=False):
    """Read one level of the inventory's block mappings or list of mappings."""
    prefix = " " * indent + ("- " if sequence else "")
    pattern = re.compile(rf"^{prefix}([\w.-]+):(?: |$)(.*)$")
    starts = [(i, match) for i, line in enumerate(lines) if (match := pattern.match(line))]
    for (index, match), end in zip(starts, [i for i, _ in starts[1:]] + [len(lines)]):
        yield match[1], match[2], lines[index + 1:end]


def scalar(value):
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\042'":
        return value[1:-1]
    return value


def inline_list(value):
    if not (value.startswith("[") and value.endswith("]")):
        raise ValueError(f"expected a bracketed list, got {value!r}")
    return [scalar(word.strip()) for word in value[1:-1].split(",") if word.strip()]


def mapping(lines, indent):
    result = {}
    for key, value, children in fields(lines, indent):
        if value in ("|", "|-", ">", ">-"):
            # The inventory uses folded blocks only for single shell commands.
            body_lines = [line for line in children if not line.strip() or line.startswith(" " * (indent + 2))]
            body = textwrap.dedent("\n".join(body_lines)).strip("\n")
            if value.startswith(">"):
                body = " ".join(line.strip() for line in body.splitlines()
                                if line.strip() and not line.lstrip().startswith("#"))
            result[key] = body
        elif value:
            result[key] = scalar(value)
        else:
            result[key] = mapping(children, indent + 2)
    return result


def matrix_rows(strategy):
    matrix = strategy.get("matrix", {})
    if not matrix:
        return [{}]
    if "include" in matrix:
        if len(matrix) != 1:
            raise ValueError("matrix include must be a standalone list of rows")
        return matrix["include"]
    return [dict(zip(matrix, values)) for values in itertools.product(
        *(inline_list(value) for value in matrix.values())
    )]


def sequence(lines, indent):
    return [{key: scalar(value), **mapping(children, indent + 2)}
            for key, value, children in fields(lines, indent, sequence=True)]


def read_inventory(text):
    """Read the subset used by checks.yml, without loading workflow actions."""
    top = {key: lines for key, _, lines in fields(text.splitlines(), 0)}
    jobs = {}
    for name, _, lines in fields(top["jobs"], 2):
        job = mapping(lines, 4)
        sections = {key: body for key, _, body in fields(lines, 4)}
        job["steps"] = sequence(sections["steps"], 6)
        strategy = job.get("strategy", {})
        if "include" in strategy.get("matrix", {}):
            matrix_lines = next(body for key, _, body in fields(sections["strategy"], 6)
                                if key == "matrix")
            include_lines = next(body for key, _, body in fields(matrix_lines, 8)
                                 if key == "include")
            strategy["matrix"]["include"] = sequence(include_lines, 10)
        job["matrix"] = matrix_rows(strategy)
        if not any("run" in step or "uses" in step for step in job["steps"]):
            raise ValueError(f"{name}: no steps")
        jobs[name] = job
    return mapping(top.get("env", []), 2), jobs


def expand(value, row):
    def substitute(match):
        key, _, default = match[1].partition("||")
        key = key.strip()
        if key.startswith("matrix."):
            return row.get(key[len("matrix."):], scalar(default.strip()))
        if key == "github.workspace":
            return "$GITHUB_WORKSPACE"
        if key == "runner.temp":
            return "$RUNNER_TEMP"
        return match[0]
    return EXPRESSION.sub(substitute, value)


def expand_env(env, row):
    # Hosted secrets, event metadata and tokens do not exist in a local run.
    return {key: value for key, raw in env.items()
            if not EXPRESSION.search(value := expand(raw, row))}


def condition_reason(condition, system):
    condition = condition.removeprefix("${{").removesuffix("}}").strip()
    for clause in condition.split("&&"):
        clause = clause.strip()
        if clause in ("", "!cancelled()"):
            continue
        match = re.fullmatch(r"runner\.os\s*(==|!=)\s*'([^']+)'", clause)
        if not match:
            return f"hosted condition ({condition})"
        if (system == match[2]) != (match[1] == "=="):
            return f"condition excludes {system} ({condition})"
    return None


@functools.lru_cache(maxsize=None)
def rustup(*args):
    if not shutil.which("rustup"):
        return ""
    return probe(["rustup", *args]) or ""


def probe(argv, cwd=ROOT):
    try:
        return subprocess.check_output(argv, cwd=cwd, stderr=subprocess.DEVNULL,
                                       encoding="utf-8", errors="replace", timeout=15).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def toolchain_reason(channel, components=(), targets=()):
    if not channel:
        return None
    if not rustup("run", channel, "rustc", "-Vv"):
        return f"Rust toolchain {channel} is not installed"
    if components:
        available = rustup("component", "list", "--installed", "--toolchain", channel).splitlines()
        missing = [item for item in components
                   if not any(line == item or line.startswith(item + "-") for line in available)]
        if missing:
            return f"Rust {channel} components missing: {', '.join(missing)}"
    if targets:
        available = rustup("target", "list", "--installed", "--toolchain", channel).splitlines()
        missing = set(targets) - set(available)
        if missing:
            return f"Rust {channel} targets missing: {', '.join(sorted(missing))}"
    return None


def requirements_reason(requirements, env=None):
    for item in requirements:
        kind, _, value = item.partition(":")
        if value.startswith("$"):
            if env is None:
                continue
            value = env.get(value[1:], "")
            if not value:
                return f"prerequisite value for {item} is unavailable"
        if not value:
            if not shutil.which(kind):
                return f"command {kind} is not installed"
        elif kind == "env":
            if not os.environ.get(value):
                return f"set {value} before running this check"
        elif kind == "pkg":
            if probe(["pkg-config", "--modversion", value]) is None:
                return f"pkg-config cannot find {value}"
        elif kind in ("rust", "component", "target"):
            channel, _, name = value.partition("/")
            reason = toolchain_reason(channel, [name] if kind == "component" else [],
                                      [name] if kind == "target" else [])
            if reason:
                return reason
        else:
            raise ValueError(f"unknown local requirement {item}")
    return None


def requirements(fields):
    return inline_list(fields.get("local-requires", "[]"))


def job_toolchain(job):
    for step in job["steps"]:
        action = step.get("uses", "")
        if action.startswith("dtolnay/rust-toolchain@"):
            args = step.get("with", {})
            channel = args.get("toolchain", action.split("@", 1)[1])
            if not EXPRESSION.search(channel):
                return channel, split_names(args.get("components", "")), split_names(args.get("targets", ""))
    return None, [], []


def split_names(value):
    return [item.strip() for item in value.split(",") if item.strip()]


def dependencies(job):
    value = job.get("needs", "")
    return inline_list(value) if value.startswith("[") else ([value] if value else [])


def select_jobs(jobs, args):
    chosen = set()
    for name, job in jobs.items():
        tags = set(inline_list(job.get("local-tags", "[]")))
        if (not args.job or name in args.job) and (not args.tag or tags.intersection(args.tag)):
            if not args.quick or "quick" in tags:
                chosen.add(name)
    pending = list(chosen)
    while pending:
        for need in dependencies(jobs[pending.pop()]):
            if need not in chosen:
                chosen.add(need)
                pending.append(need)
    return chosen


def planning_order(jobs):
    ordered = []
    visiting = set()

    def visit(name):
        if name in ordered:
            return
        if name in visiting:
            raise ValueError(f"cyclic job dependency at {name}")
        visiting.add(name)
        for need in dependencies(jobs[name]):
            visit(need)
        visiting.remove(name)
        ordered.append(name)

    for name in jobs:
        visit(name)
    return ordered


def plan_checks(inventory, system, args):
    common_env, jobs = inventory
    chosen = select_jobs(jobs, args)
    result = []
    for name in planning_order(jobs):
        job = jobs[name]
        job_tags = inline_list(job.get("local-tags", "[]"))
        for row in job["matrix"]:
            runners = inline_list(job["local-os"]) if "local-os" in job else [
                RUNNERS[expand(job["runs-on"], row).split("-", 1)[0]]
            ]
            channel, components, targets = job_toolchain(job)
            reason = None
            if system not in runners:
                reason = f"requires {', '.join(runners)} (this host: {system})"
            elif name not in chosen:
                reason = "excluded by job/tag/quick filter"
            else:
                reason = (job.get("local-skip") or requirements_reason(requirements(job))
                          or toolchain_reason(channel, components, targets))
            job_env = {**expand_env(common_env, row), **expand_env(job.get("env", {}), row)}
            steps = []
            for index, step in enumerate(job["steps"], start=1):
                if "run" not in step:
                    continue  # setup/cache/upload actions are never executed
                tags = set(job_tags).union(inline_list(step.get("local-tags", "[]")))
                command = expand(step["run"], row)
                step_reason = reason or step.get("local-skip") or condition_reason(step.get("if", ""), system)
                if not step_reason:
                    if "windowed" in tags and not args.include_windowed:
                        step_reason = "opens a window (requires --include-windowed)"
                    elif EXPRESSION.search(command):
                        step_reason = "requires a hosted expression"
                    elif step.get("shell", "bash") == "bash" and not bash_path(system):
                        step_reason = "Bash is not installed"
                    else:
                        step_reason = requirements_reason(requirements(step))
                steps.append({
                    "name": expand(step.get("name", f"Step {index}"), row),
                    "command": command,
                    "reason": step_reason,
                    "env": expand_env(step.get("env", {}), row),
                    "requires": requirements(step),
                    "cwd": expand(step.get("working-directory", "."), row),
                    "shell": step.get("shell", "bash"),
                    "timeout": int(step.get("timeout-minutes", "0")) * 60,
                })
            result.append({
                "job": name, "name": expand(job.get("name", name), row), "matrix": row,
                "reason": reason, "env": job_env, "toolchain": channel, "steps": steps,
                "needs": dependencies(job), "tags": job_tags,
                "timeout": int(job.get("timeout-minutes", "0")) * 60,
            })
    return result


def bash_path(system):
    if system == "Windows":
        # Prefer Git Bash to the Windows system launcher for WSL.
        git = shutil.which("git")
        if git:
            for parent in Path(git).parents:
                candidate = parent / "bin" / "bash.exe"
                if candidate.is_file():
                    return str(candidate)
    return shutil.which("bash")


def run_step(step, env, root, system, timeout):
    cwd = Path(step["cwd"].replace("$RUNNER_TEMP", env["RUNNER_TEMP"])
               .replace("$GITHUB_WORKSPACE", env["GITHUB_WORKSPACE"]))
    shell = step["shell"]
    if shell == "bash":
        exe = bash_path(system)
        command = ('python() { "$PONKO_CHECK_PYTHON" "$@"; }\n'
                   'python3() { "$PONKO_CHECK_PYTHON" "$@"; }\n' + step["command"])
        argv = [exe, "--noprofile", "--norc", "-eo", "pipefail", "-c", command]
    elif shell == "pwsh":
        exe = shutil.which("pwsh")
        argv = [exe, "-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference = 'Stop'\n" + step["command"] + "\nexit $LASTEXITCODE"]
    else:
        return 2, f"unsupported shell: {shell}"
    if not exe:
        return 127, f"shell is unavailable: {shell}"
    with tempfile.TemporaryFile() as output:
        try:
            process = subprocess.Popen(argv, cwd=root / cwd, env=env, stdout=output,
                                       stderr=subprocess.STDOUT, start_new_session=(os.name != "nt"))
        except OSError as error:
            return 127, str(error)
        try:
            status = process.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
            if os.name == "nt":
                subprocess.run(["taskkill", "/F", "/T", "/PID", str(process.pid)],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            else:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            process.wait()
            if isinstance(error, KeyboardInterrupt):
                raise
            output.write(f"\nTimed out after {timeout:.0f}s\n".encode())
            status = 124
        output.seek(0)
        return status, output.read().decode("utf-8", errors="replace")


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def label(job):
    matrix = job["matrix"]
    variant = ", ".join(f"{key}={value}" for key, value in matrix.items())
    return f'{job["job"]} [{variant}]' if variant else job["job"]


def run_checks(plan, root, system):
    started_at = utc_now()
    revision = probe(["git", "rev-parse", "HEAD"], cwd=root)
    tree = probe(["git", "status", "--porcelain"], cwd=root)
    rust = {}
    completed = {}
    results = []
    adapters = set()
    for job in plan:
        reason = job["reason"]
        for need in job["needs"]:
            if not reason and not completed.get(need):
                reason = f"prerequisite {need} did not pass"
        result = {key: job[key] for key in ("job", "name", "matrix")}
        result.update(reason=reason, steps=[], toolchain=None)
        results.append(result)
        if not reason and any(not s["reason"] for s in job["steps"]):
            channel = job["toolchain"] or "default"
            if channel not in rust:
                argv = ["rustc", "-Vv"] if channel == "default" else ["rustc", "+" + channel, "-Vv"]
                rust[channel] = probe(argv, cwd=root)
            result["toolchain"] = channel
        if reason:
            result["steps"] = [
                {"name": step["name"], "command": step["command"], "reason": reason,
                 "returncode": None, "duration_seconds": None} for step in job["steps"]
            ]
            continue
        with tempfile.TemporaryDirectory(prefix="ponko-checks-") as tmp:
            tmp = Path(tmp)
            env = {
                **os.environ, "GITHUB_WORKSPACE": root.as_posix(),
                "RUNNER_OS": system, "RUNNER_TEMP": tmp.as_posix(),
                "PONKO_CHECK_PYTHON": Path(sys.executable).as_posix(),
            }
            for key, value in job["env"].items():
                env[key] = value.replace("$GITHUB_WORKSPACE", env["GITHUB_WORKSPACE"]).replace(
                    "$RUNNER_TEMP", env["RUNNER_TEMP"]
                )
            if job["toolchain"]:
                env["RUSTUP_TOOLCHAIN"] = job["toolchain"]
            if "gpu" in job["tags"]:
                env.update(RUST_TEST_NOCAPTURE="1", NEXTEST_SUCCESS_OUTPUT="final")
            deadline = time.monotonic() + job["timeout"] if job["timeout"] else math.inf
            for step in job["steps"]:
                row = {key: step[key] for key in ("name", "command", "reason")}
                row.update(returncode=None, duration_seconds=None)
                result["steps"].append(row)
                row["reason"] = row["reason"] or requirements_reason(
                    [req for req in step["requires"] if ":$" in req], env
                )
                if row["reason"]:
                    continue
                for req in step["requires"]:
                    if req.startswith("rust:$"):
                        channel = env[req[len("rust:$"):]]
                        rust[channel] = rustup("run", channel, "rustc", "-Vv")
                        row["toolchain"] = channel
                start = time.monotonic()
                if start >= deadline:
                    row["reason"] = "job timeout elapsed"
                    continue
                timeout = min(step["timeout"] or math.inf, deadline - start)
                print(f'RUN  {label(job)} / {step["name"]}', flush=True)
                env_file = tmp / "step.env"
                env_file.write_text("", encoding="utf-8")
                status, output = run_step(step, {**env, **step["env"], "GITHUB_ENV": env_file.as_posix()},
                                          root, system, None if math.isinf(timeout) else max(0, timeout))
                row.update(returncode=status, duration_seconds=round(time.monotonic() - start, 3))
                if status:
                    row["failure_output"] = output
                else:
                    for line in env_file.read_text(encoding="utf-8-sig").splitlines():
                        key, sep, value = line.partition("=")
                        if sep and re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", key):
                            env[key] = value
                gpu_skips = set()
                for line in output.splitlines():
                    if match := GPU_RAN.search(line):
                        adapters.add(match[1])
                    if match := GPU_SKIPPED.search(line):
                        gpu_skips.add(match[1])
                if gpu_skips:
                    row["gpu_skips"] = sorted(gpu_skips)
                print(f'     rc={status} ({row["duration_seconds"]:.3f}s)', flush=True)
        statuses = [s["returncode"] for s in result["steps"] if s["returncode"] is not None]
        passed = bool(statuses) and not any(statuses)
        if not job["reason"]:
            name = job["job"]
            completed[name] = completed.get(name, True) and passed
    codes = [s["returncode"] for j in results for s in j["steps"] if s["returncode"] is not None]
    return {
        "schema_version": 1, "revision": revision, "dirty": bool(tree) if tree is not None else None,
        "started_at": started_at, "finished_at": utc_now(),
        "os": system, "os_version": (platform.mac_ver()[0] if system == "macOS" else platform.release()) or platform.release(), "architecture": platform.machine(),
        "toolchains": rust, "gpu_adapters": sorted(adapters), "jobs": results,
        "exit_code": 1 if any(codes) else (0 if codes else 2),
    }


def summary(report):
    lines = [
        f'Ponko checks at {report["revision"] or "unknown revision"}'
        + (" (dirty checkout)" if report["dirty"] else ""),
        f'{report["os"]} {report["os_version"]} ({report["architecture"]}) | {report["finished_at"]}',
    ]
    for key, version in report["toolchains"].items():
        lines.append(f'Rust {key}: {version.splitlines()[0] if version else "unavailable"}')
    lines.append("GPU adapters observed: " + (", ".join(report["gpu_adapters"]) or "none reported"))
    counts = {"PASS": 0, "FAIL": 0, "SKIP": 0}
    for job in report["jobs"]:
        if job["reason"]:
            lines.append(f'SKIP {label(job)}: {job["reason"]}')
            counts["SKIP"] += len(job["steps"])
            continue
        for step in job["steps"]:
            state = "SKIP" if step["reason"] else ("FAIL" if step["returncode"] else "PASS")
            counts[state] += 1
            result = step["reason"] or f'rc={step["returncode"]} {step["duration_seconds"]:.3f}s'
            command = step["command"].replace("\n", "\\n")
            lines.append(f'{state} {label(job)} / {step["name"]}: {result} | {command}')
            for why in step.get("gpu_skips", []):
                lines.append(f"     GPU test skipped: {why}")
    lines.append(" | ".join(f"{count} {name.lower()}" for name, count in counts.items()))
    return "\n".join(lines) + "\n"


def write_report(report, directory):
    directory.mkdir(parents=True, exist_ok=True)
    for name, content in (("results.json", json.dumps(report, indent=2) + "\n"), ("summary.txt", summary(report))):
        with tempfile.NamedTemporaryFile(mode="w", dir=directory, encoding="utf-8", delete=False) as handle:
            handle.write(content)
        Path(handle.name).replace(directory / name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--job", action="append", default=[], help="select a job ID (repeatable)")
    parser.add_argument("--tag", action="append", default=[], help="select a tag (repeatable)")
    parser.add_argument("--quick", action="store_true", help="run the quick subset")
    parser.add_argument("--list", action="store_true", help="show the plan and skip reasons")
    parser.add_argument("--include-windowed", action="store_true", help="allow jobs to open windows")
    parser.add_argument("--out", type=Path, default=ROOT / "target/check-results", help="report directory")
    parser.add_argument("--selftest", action="store_true", help="run synthetic runner tests")
    args = parser.parse_args()
    if args.selftest:
        return subprocess.call([sys.executable, str(ROOT / "scripts/test_run_checks.py")])
    system = SYSTEMS.get(platform.system())
    if not system:
        parser.error(f"unsupported operating system: {platform.system()}")
    try:
        inventory = read_inventory((ROOT / "scripts/checks.yml").read_text(encoding="utf-8"))
        jobs = inventory[1]
        for name in args.job:
            if name not in jobs:
                parser.error(f"unknown job {name!r}")
        tags = {tag for job in jobs.values() for tag in inline_list(job.get("local-tags", "[]"))}
        for tag in args.tag:
            if tag not in tags:
                parser.error(f"unknown job tag {tag!r}")
        plan = plan_checks(inventory, system, args)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(f"checks.yml: {error}")
    if args.list:
        for job in plan:
            print(f'{label(job)}: {job["reason"] or "local steps"}')
            if not job["reason"]:
                for step in job["steps"]:
                    pending = [req for req in step["requires"] if ":$" in req]
                    state = step["reason"] or (
                        f"run if available ({', '.join(pending)})" if pending else "run"
                    )
                    print(f'  {step["name"]}: {state}')
        return 0
    report = run_checks(plan, ROOT, system)
    write_report(report, args.out)
    print(summary(report), end="")
    print(f'Reports: {args.out / "results.json"}, {args.out / "summary.txt"}')
    return report["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
