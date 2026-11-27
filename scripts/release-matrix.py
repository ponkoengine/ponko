#!/usr/bin/env python3
"""The release-time CI: a small GitHub Actions workflow made from checks.yml.

    release-matrix.py write    # regenerate scripts/release-matrix.yml
    release-matrix.py check    # fail if it is out of date with checks.yml
    release-matrix.py selftest

The project has no CI (CONTRIBUTING.md, "Releasing"). The maintainers' own
machines cover macOS, Windows x64 and the arm64 Linux C library. At a release
candidate, hosted runners do only what those machines cannot:

- `linux` (ubuntu-24.04, x86_64): the Linux steps of the checks.yml jobs
  `test`, `templates`, `test-features`, `native` and `package-smoke`, as
  named steps of ONE job in one build tree;
- `windows-arm64` (windows-11-arm): the Windows on Arm C library archive,
  built and smoke-tested;
- `first-run` (ubuntu-24.04): scripts/first-run.py against crates.io, after
  `cargo publish`.

This script writes that workflow, `scripts/release-matrix.yml`, from
`scripts/checks.yml`, so the two cannot drift apart: a step added to one of
those jobs lands in the Linux job by itself, and a step this script does not
understand (an unknown `if:`, a local action, a requirement it cannot install)
stops it. The file is NOT enabled: it lives under scripts/, which GitHub
never reads, and Actions is off for the repository. The maintainer turns
Actions on for one run, puts the file on a branch of its own and pushes it:

    git switch --detach v0.2.0-rc.1
    mkdir -p .github/workflows && cp scripts/release-matrix.yml .github/workflows/
    git switch -c release-matrix/checks-0.2.0-rc.1
    git add .github/workflows/release-matrix.yml && git commit -m "Release CI"
    git push origin release-matrix/checks-0.2.0-rc.1

The branch name picks the one job that runs: `release-matrix/checks-<version>`
the Linux checks, `release-matrix/c-library-<version>` (from the tag) the
Windows on Arm archive, `release-matrix/first-run-<version>` (after
`cargo publish`) the first run. Then the branch is deleted and Actions turned
off again. scripts/release-0.2.md, "Release-time CI", has the rules.

How a checks.yml step becomes a Linux step:
- its `if:` is evaluated for Linux: a `runner.os` test that is false drops
  the step, one that is true is removed; every step then also gets
  `!cancelled() && steps.setup.outcome == 'success'` (unless it is a
  `failure()` step), so one failure still reports the rest but a broken setup
  stops the job;
- its name becomes `<job> / <name>` (a step without one is named by its
  command);
- `local-*` keys go (only run-checks.py reads them), checkout, toolchain and
  cache steps go (the job has one setup and saves no cache), apt installs are
  merged (each package installed once, at its first step), a
  `cache-cargo-install-action` becomes `cargo install --locked`;
- `${{ matrix.os }}` becomes `linux`, artifacts are kept 3 days.
"""

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CHECKS = os.path.join(HERE, "checks.yml")
OUT = os.path.join(HERE, "release-matrix.yml")

# The checks.yml jobs the Linux job runs, in this order. Builds are shared
# where features are: `test` builds the workspace with default features,
# `templates` reuses it for the command line tool, `test-features` adds one
# feature set per step, `native` builds the C library in release, and
# `package-smoke` comes last because it installs software Vulkan (lavapipe)
# for its window runs: every render test before it finds no adapter and
# skips with a logged reason (scripts/release-0.2.md says why).
SOURCES = ("test", "templates", "test-features", "native", "package-smoke")

# Steps moved to the end of the job: the frame-pointer build is a second
# build tree of its own, and the image margins and the failed images are read
# once every render test has run.
LAST = (
    ("test", "Native crash backtraces with frame pointers (Linux)"),
    ("test", "Margins of the image comparisons"),
    ("test", "Keep the images of failed render tests"),
)

# How the runner gets what a Linux step's `local-requires` names. A
# requirement missing here stops the script: say how the runner installs it.
REQUIRES = {
    "cmake": [],  # on the runner image
    "ctest": [],  # with cmake
    "cbindgen": [],  # the `native` job installs it (cargo install --locked)
    "dbus-run-session": ["dbus", "at-spi2-core"],  # the AT-SPI bus launcher
}

# Job-level env of a source job that the Linux job replaces: it has one
# build tree (CARGO_TARGET_DIR below).
JOB_ENV_DROPPED = ("CARGO_TARGET_DIR",)

ARTIFACT_DAYS = 3
SETUP_IF = "steps.setup.outcome == 'success'"

LOCAL_KEY = re.compile(r"^\s+(- )?local-[a-z]+:")
JOB_KEY = re.compile(r"^  ([a-z0-9-]+):\s*$")
STEP_START = re.compile(r"^      - ")
FIELD = re.compile(r"^      (?:- |  )([a-z-]+):(?: (.*))?$")
APT = re.compile(r"^sudo apt-get install -y (.+)$")
RUNNER_OS = re.compile(r"^runner\.os (==|!=) '([A-Za-z]+)'$")
STATUS = ("!cancelled()", "failure()", "always()", "success()")

HEADER = """\
# GENERATED by scripts/release-matrix.py from scripts/checks.yml: do not edit.
#
# NOT ENABLED. The release-time CI: one run per release candidate, only for
# what the maintainers' machines can't test (scripts/release-0.2.md,
# "Release-time CI"). GitHub never reads scripts/; the maintainer turns
# Actions on, copies this file to .github/workflows/ on a branch of its own,
# pushes it, then deletes the branch and turns Actions off. The branch picks
# the one job that runs:
#   release-matrix/checks-<version>     linux          ubuntu-24.04
#   release-matrix/c-library-<version>  windows-arm64  windows-11-arm
#   release-matrix/first-run-<version>  first-run      ubuntu-24.04
# Never merge it into main. No re-runs: reproduce a failure locally.
name: Release CI

on:
  push:
    branches: ["release-matrix/**"]

concurrency:
  group: release-matrix-${{ github.ref }}
  cancel-in-progress: true

permissions:
  contents: read
"""

LINUX_HEAD = """\
  # The Linux steps of the checks.yml jobs test, templates, test-features,
  # native and package-smoke, in one build tree. Each step is named
  # `<checks.yml job> / <step>`; one failing step doesn't stop the others.
  linux:
    if: startsWith(github.ref_name, 'release-matrix/checks-')
    name: Linux checks
    runs-on: ubuntu-24.04
    timeout-minutes: 120
    env:
      CARGO_TARGET_DIR: ${{ github.workspace }}/target
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
"""

LINUX_TOOLCHAIN = """\
      - uses: dtolnay/rust-toolchain@stable
        id: setup
"""

LINUX_SKIPS = """\
      - name: Render tests skipped on this runner (no GPU adapter before package-smoke)
        if: ${{ !cancelled() && steps.setup.outcome == 'success' }}
        run: |
          log=target/render-test-output/skips.log
          if [ -f "$log" ]; then
            echo "$(wc -l < "$log") render tests skipped:"
            sort "$log"
          else
            echo "No render test skipped."
          fi
"""

C_LIBRARY = """\
  # From the tag: the C library archive for Windows on Arm, built and
  # smoke-tested (the C and C++ tests run) on a hosted Windows on Arm runner
  # (scripts/c-library.py; CONTRIBUTING.md, "The C library archives"). The
  # maintainer downloads the artifact into dist/ with the other five.
  windows-arm64:
    if: startsWith(github.ref_name, 'release-matrix/c-library-')
    name: C library (windows-arm64)
    runs-on: windows-11-arm
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-pc-windows-msvc
      - name: Build the archive
        shell: bash
        run: |
          version="v${GITHUB_REF_NAME#release-matrix/c-library-}"
          python scripts/c-library.py build --target aarch64-pc-windows-msvc --version "$version"
      - name: Smoke test (builds examples/cpp against the archive and runs its tests)
        shell: bash
        run: |
          version="v${GITHUB_REF_NAME#release-matrix/c-library-}"
          python scripts/c-library.py smoke "target/c-library/ponko-$version-windows-arm64.zip"
      - uses: actions/upload-artifact@v7
        with:
          name: ponko-c-library-windows-arm64
          path: target/c-library/ponko-*-windows-arm64.zip
          if-no-files-found: error
          retention-days: 7
"""

FIRST_RUN = """\
  # After `cargo publish`: a new user's first run from crates.io
  # (scripts/first-run.py; the version is the branch name's suffix). macOS
  # and Windows run it on the maintainers' machines. One target folder for
  # its projects: each would otherwise build the engine again.
  first-run:
    if: startsWith(github.ref_name, 'release-matrix/first-run-')
    name: First run from crates.io (Linux)
    runs-on: ubuntu-24.04
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v7
        with:
          sparse-checkout: |
            scripts/first-run.py
            book/src/getting-started.md
          sparse-checkout-cone-mode: false
      - name: Install ALSA and udev headers (audio, gamepads)
        run: sudo apt-get update -q && sudo apt-get install -y libasound2-dev libudev-dev
      - uses: dtolnay/rust-toolchain@stable
      - name: First run
        env:
          CARGO_TARGET_DIR: ${{ runner.temp }}/first-run-target
        run: |
          version="${GITHUB_REF_NAME#release-matrix/first-run-}"
          cp scripts/first-run.py "$RUNNER_TEMP/"
          cd "$RUNNER_TEMP"
          python3 first-run.py "$version" --dir "$RUNNER_TEMP/first-run"
"""


def fail(message):
    raise SystemExit(f"release-matrix: {message}")


def top_level(text, key):
    """The lines of a top-level mapping `key:` (without the key line)."""
    lines = text.splitlines()
    start = lines.index(f"{key}:")
    block = []
    for line in lines[start + 1:]:
        if line and not line.startswith((" ", "#")):
            break
        block.append(line)
    while block and not block[-1].strip():
        block.pop()
    return block


def jobs(text):
    """{job name: its lines}, comments above a job going with it."""
    lines = text.splitlines()
    start = lines.index("jobs:") + 1
    found, name, pending = {}, None, []
    for line in lines[start:]:
        m = JOB_KEY.match(line)
        if m:
            name = m.group(1)
            found[name] = pending + [line]
            pending = []
        elif line.startswith("  #") or not line.strip():
            pending.append(line)
        elif name:
            found[name] += pending + [line]
            pending = []
    for body in found.values():
        while body and not body[-1].strip():
            body.pop()
    return found


def parse_job(name, lines):
    """(job env {key: value}, job local-requires, [step lines])."""
    try:
        at = lines.index("    steps:")
    except ValueError:
        fail(f"{name}: no steps")
    env, requires, key = {}, [], None
    for line in lines[:at]:
        m = re.match(r"^    ([a-z-]+):(?: (.*))?$", line)
        if m:
            key = m.group(1)
            if key == "local-requires":
                requires = parse_list(m.group(2), name)
            continue
        m = re.match(r"^      ([A-Z_]+): (.*)$", line)
        if key == "env" and m:
            env[m.group(1)] = m.group(2)
    steps, pending = [], []
    for line in lines[at + 1:]:
        if STEP_START.match(line):
            steps.append(pending + [line])
            pending = []
        elif line.strip().startswith("#") or not line.strip():
            pending.append(line)
        elif steps:
            steps[-1] += pending + [line]
            pending = []
        else:
            fail(f"{name}: a line before the first step: {line.strip()}")
    for step in steps:
        while step and not step[-1].strip():
            step.pop()
    return env, requires, steps


def parse_list(value, where):
    m = re.match(r"^\[(.*)\]$", (value or "").strip())
    if not m:
        fail(f"{where}: local-requires is not a [list]: {value}")
    return [item.strip() for item in m.group(1).split(",") if item.strip()]


def fields(step):
    """The step's top-level fields: {key: (line index, value or None)}."""
    found = {}
    for i, line in enumerate(step):
        m = FIELD.match(line)
        if m:
            found[m.group(1)] = (i, m.group(2))
    return found


def linux_if(expression, where):
    """The `if:` terms left for Linux, or None when the step doesn't run there."""
    if expression is None:
        return []
    inner = expression.strip()
    m = re.match(r"^\$\{\{(.*)\}\}$", inner)
    inner = (m.group(1) if m else inner).strip()
    if "||" in inner:
        fail(f"{where}: an `if:` with ||, which this script can't evaluate: {expression}")
    terms = []
    for term in (t.strip() for t in inner.split("&&")):
        m = RUNNER_OS.match(term)
        if m:
            on_linux = (m.group(2) == "Linux") == (m.group(1) == "==")
            if not on_linux:
                return None
        elif term in STATUS:
            terms.append(term)
        else:
            fail(f"{where}: an `if:` term this script can't evaluate: {term}")
    return terms


def convert_step(job, step, seen_packages, job_env):
    """The step as Linux job lines, or [] when it doesn't run on Linux."""
    f = fields(step)
    where = f"{job}: {(f.get('name') or f.get('uses') or f.get('run') or (0, '?'))[1]}"
    for line in step:
        if LOCAL_KEY.match(line) and line.lstrip().startswith("- "):
            fail(f"{where}: a step that starts with a local-* key")
    terms = linux_if(f["if"][1] if "if" in f else None, where)
    if terms is None:
        return [], []
    requires = parse_list(f["local-requires"][1], where) if "local-requires" in f else []
    uses = f["uses"][1] if "uses" in f else None
    if uses:
        if uses.startswith(("actions/checkout@", "dtolnay/rust-toolchain@", "Swatinem/rust-cache@")):
            return [], requires
        if uses.startswith("./"):
            fail(f"{where}: a local action GitHub no longer has: {uses}")
    run = f["run"][1] if "run" in f else None
    apt = APT.match(run or "")
    if apt:
        new = [p for p in apt.group(1).split() if p not in seen_packages]
        seen_packages.update(new)
        if not new:
            return [], requires
        step = [line for line in step if not line.startswith("        if:")]
        i = fields(step)["run"][0]
        step[i] = f"        run: sudo apt-get install -y {' '.join(new)}"
        terms = None  # a setup step: plain success() gating
    body = [line for line in step if not LOCAL_KEY.match(line)]
    if uses and uses.startswith("taiki-e/cache-cargo-install-action@"):
        tool = next((line.split("tool:", 1)[1].strip() for line in body if "tool:" in line), None)
        if not tool or "@" not in tool:
            fail(f"{where}: cache-cargo-install-action without `tool: name@version`")
        crate, version = tool.split("@", 1)
        start = next(i for i, line in enumerate(body) if STEP_START.match(line))
        body = body[:start] + [
            f"      - name: Install {crate} (no cache)",
            f'        run: cargo install --locked {crate} --version "{version}"',
        ]
    body = [line.replace("${{ matrix.os }}", "linux") for line in body]
    body = [re.sub(r"retention-days: \d+", f"retention-days: {ARTIFACT_DAYS}", line) for line in body]
    if any("upload-artifact@" in line for line in body) and terms != ["failure()"]:
        fail(f"{where}: an artifact upload that isn't `if: failure()`")
    # The name: `<job> / <name>`, or the command for a step without one.
    f = fields(body)
    start = next(i for i, line in enumerate(body) if STEP_START.match(line))
    if "name" in f:
        i, name = f["name"]
        title = f"{job} / {name}"
        body[i] = body[i].replace(f"name: {name}", f"name: {json.dumps(title, ensure_ascii=False)}", 1)
    elif "run" in f:
        title = f"{job} / {short(f['run'][1])}"
        first = body[start]
        body[start] = f"      - name: {json.dumps(title, ensure_ascii=False)}"
        body.insert(start + 1, "        " + first[len("      - "):])
    else:
        fail(f"{where}: a step with neither a name nor a command")
    # The condition: the step's own Linux terms, run unless cancelled or the
    # setup failed. Setup steps (apt) keep the default.
    body = [line for line in body if not re.match(r"^      (- |  )if:", line)]
    if terms is not None:
        if "failure()" in terms:
            condition = "failure()"
        else:
            rest = [t for t in terms if t not in STATUS]
            condition = " && ".join(["!cancelled()", SETUP_IF] + rest)
        name_line = next(i for i, line in enumerate(body) if '  name: "' in line or '- name: "' in line)
        body.insert(name_line + 1, f"        if: ${{{{ {condition} }}}}")
    if job_env and terms is not None:
        body = with_env(body, job_env)
    return body, requires


def short(command):
    """A command as a step name: no expressions, at most about 60 characters."""
    command = re.sub(r"\s*\"?[^\s\"]*\$\{\{[^}]*\}\}[^\s\"]*\"?", "", command).strip()
    if len(command) <= 60:
        return command
    return command[:command.rindex(" ", 0, 60)] + " ..."


def with_env(body, extra):
    """`extra` added to the step's env (the step's own values win)."""
    f = fields(body)
    if "env" in f:
        i = f["env"][0]
        own = set()
        j = i + 1
        while j < len(body) and body[j].startswith("          "):
            own.add(body[j].split(":", 1)[0].strip())
            j += 1
        add = [f"          {k}: {v}" for k, v in extra.items() if k not in own]
        return body[:j] + add + body[j:]
    i = next(i for i, line in enumerate(body) if line.startswith("        if:"))
    return body[:i + 1] + ["        env:"] + [f"          {k}: {v}" for k, v in extra.items()] + body[i + 1:]


def linux_job(found):
    seen, apt_steps, checks, last, requires = set(), [], [], {}, []
    for job in SOURCES:
        env, job_requires, steps = parse_job(job, found[job])
        requires += job_requires
        job_env = {k: v for k, v in env.items() if k not in JOB_ENV_DROPPED}
        for step in steps:
            body, step_requires = convert_step(job, step, seen, job_env)
            requires += step_requires
            if not body:
                continue
            name = fields(step).get("name", (0, None))[1]
            if (job, name) in LAST:
                last[(job, name)] = body
            elif any(re.match(r"^        run: sudo apt-get install", line) for line in body):
                # The first install also updates the package lists; packages
                # a later job adds stay at that job's place.
                apt_steps.append(len(checks))
                checks.append(body)
            else:
                checks.append(body)
    missing = [f"{job} / {name}" for job, name in LAST if (job, name) not in last]
    if missing:
        fail(f"checks.yml has no step {', '.join(missing)} (LAST)")
    unknown = sorted({r for r in requires if r not in REQUIRES})
    if unknown:
        fail(f"a Linux step requires {', '.join(unknown)}: say in REQUIRES how the runner gets it")
    extra = [p for r in dict.fromkeys(requires) for p in REQUIRES[r] if p not in seen]
    if not apt_steps:
        fail("no apt-get install step in the Linux jobs")
    first = checks.pop(apt_steps[0])
    i = next(i for i, line in enumerate(first) if line.startswith("        run: sudo apt-get install"))
    packages = first[i].split("install -y ", 1)[1].split() + extra
    first[i] = f"        run: sudo apt-get update -q && sudo apt-get install -y {' '.join(packages)}"
    if extra:
        first.insert(i, f"        # And what the steps need (REQUIRES in the script): {' '.join(extra)}.")
    out = LINUX_HEAD.splitlines() + first + LINUX_TOOLCHAIN.splitlines()
    for body in checks:
        out += body
    for key in LAST:
        out += last[key]
    return out + LINUX_SKIPS.splitlines()


def generate(text):
    found = jobs(text)
    missing = [j for j in SOURCES if j not in found]
    if missing:
        fail(f"checks.yml has no job {', '.join(missing)}")
    env = top_level(text, "env")
    parts = [HEADER, "env:", *env]
    parts += ['  # One run per candidate: an incremental cache would never be reused.',
              '  CARGO_INCREMENTAL: "0"', "", "jobs:"]
    parts += linux_job(found) + [""]
    return "\n".join(parts) + "\n" + C_LIBRARY + "\n" + FIRST_RUN


def selftest():
    sample = """\
env:
  A: "1"
  # comment
  B: two

jobs:
  # The first job.
  one:
    local-tags: [test]
    runs-on: ${{ matrix.os }}
    env:
      CARGO_TARGET_DIR: elsewhere
      KEEP: "1"
    steps:
      - uses: actions/checkout@v7
      - name: Install headers
        local-skip: Hosted only.
        if: runner.os == 'Linux'
        run: sudo apt-get install -y liba libb
      - uses: dtolnay/rust-toolchain@stable
      - uses: ./.github/actions/windows-linker
        if: runner.os == 'Windows'
      - uses: Swatinem/rust-cache@v2
      # A comment that goes with its step.
      - name: Tests
        run: cargo test
      - name: Mac only
        if: ${{ !cancelled() && runner.os == 'macOS' }}
        run: echo mac
      - name: Not on Windows
        local-requires: [dbus-run-session]
        if: ${{ !cancelled() && runner.os != 'Windows' }}
        env:
          KEEP: "2"
        run: echo linux
      - run: cmake --build x
      - name: Keep the images
        if: failure()
        uses: actions/upload-artifact@v7
        with:
          name: out-${{ matrix.os }}
          retention-days: 14
"""
    found = jobs(sample)
    assert list(found) == ["one"] and found["one"][0] == "  # The first job.", found
    assert top_level(sample, "env") == ['  A: "1"', "  # comment", "  B: two"]
    env, requires, steps = parse_job("one", found["one"])
    assert env == {"CARGO_TARGET_DIR": "elsewhere", "KEEP": '"1"'} and requires == [], env
    assert len(steps) == 10 and steps[5][0] == "      # A comment that goes with its step.", steps[5]
    seen, out = set(), []
    for step in steps:
        body, _ = convert_step("one", step, seen, {"KEEP": '"1"'})
        out.append(body)
    text = "\n".join(line for body in out for line in body)
    assert "local-" not in text and "checkout" not in text and "rust-cache" not in text, text
    assert "windows-linker" not in text and "echo mac" not in text, text
    assert out[1][0] == '      - name: "one / Install headers"' and "        if:" not in "\n".join(out[1]), out[1]
    assert seen == {"liba", "libb"}, seen
    assert out[5] == [
        "      # A comment that goes with its step.",
        '      - name: "one / Tests"',
        "        if: ${{ !cancelled() && steps.setup.outcome == 'success' }}",
        "        env:",
        '          KEEP: "1"',
        "        run: cargo test",
    ], out[5]
    # The step's own env wins over the job's.
    assert '          KEEP: "2"' in out[7] and '          KEEP: "1"' not in out[7], out[7]
    assert short('cmake -S a -B b -DX=ON "-DDIR=${{ github.workspace }}/t"') == "cmake -S a -B b -DX=ON"
    assert short("x " * 40).endswith(" ...") and len(short("x " * 40)) <= 64
    assert out[8][:2] == ['      - name: "one / cmake --build x"',
                          "        if: ${{ !cancelled() && steps.setup.outcome == 'success' }}"], out[8]
    assert "        if: ${{ failure() }}" in out[9] and "          name: out-linux" in out[9], out[9]
    assert "          retention-days: 3" in out[9], out[9]
    # A second install of the same packages disappears; a new one stays.
    again = ["      - name: More", "        run: sudo apt-get install -y libb libc"]
    assert convert_step("two", again, seen, {})[0][-1] == "        run: sudo apt-get install -y libc"
    assert convert_step("two", again, seen, {})[0] == []
    for bad, message in (
        (["      - uses: ./.github/actions/other"], "no longer has"),
        (["      - name: X", "        if: runner.os == 'Linux' || failure()", "        run: x"], "||"),
        (["      - name: X", "        if: github.event_name == 'push'", "        run: x"], "can't evaluate"),
        (["      - name: X", "        uses: actions/upload-artifact@v7"], "isn't `if: failure()`"),
    ):
        try:
            convert_step("x", bad, set(), {})
            raise AssertionError(f"passed: {bad}")
        except SystemExit as e:
            assert message in str(e), (message, e)
    # The real file.
    with open(CHECKS, encoding="utf-8") as f:
        real = generate(f.read())
    assert not re.search(r"^\s+(- )?local-", real, re.M) and "uses: ./" not in real
    runners = sorted(set(re.findall(r"runs-on: (\S+)", real)))
    assert runners == ["ubuntu-24.04", "windows-11-arm"], runners
    assert re.findall(r"^  ([a-z0-9-]+):$", real.split("\njobs:", 1)[1], re.M) == ["linux", "windows-arm64", "first-run"]
    assert "rust-cache" not in real and "cache-cargo-install-action" not in real
    assert real.count("timeout-minutes:") == 3 and "matrix" not in real.split("jobs:", 1)[1].replace("release-matrix", "")
    assert real.count("sudo apt-get update") == 2, "one package list update per job"
    names = re.findall(r'^      - name: "([a-z-]+) / ', real, re.M)
    assert set(names) == set(SOURCES), set(names)
    assert "c-library.py build --target aarch64-pc-windows-msvc" in real and "c-library.py smoke" in real
    assert "first-run.py \"$version\"" in real
    # package-smoke's install (lavapipe) comes after every test step.
    lavapipe = real.index("mesa-vulkan-drivers")
    assert lavapipe > real.rindex('name: "test-features / ') and lavapipe > real.rindex('name: "native / ')
    print("release-matrix.py selftest: ok")


def main():
    cmd = sys.argv[1:] or ["check"]
    if cmd == ["selftest"]:
        return selftest()
    with open(CHECKS, encoding="utf-8") as f:
        text = generate(f.read())
    if cmd == ["write"]:
        with open(OUT, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"wrote {os.path.relpath(OUT)}")
    elif cmd == ["check"]:
        try:
            with open(OUT, encoding="utf-8") as f:
                current = f.read()
        except FileNotFoundError:
            current = ""
        if current != text:
            raise SystemExit("release-matrix.yml is out of date with checks.yml: "
                             "run python3 scripts/release-matrix.py write")
        print("release-matrix.yml matches checks.yml")
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
