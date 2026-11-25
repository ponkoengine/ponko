# Validation bundles

> **Maturity: [tested](maturity.md).** Game: none. Tests:
> `tests/validation_bundle.rs` and the `validation-bundles` job. Platforms:
> Linux, headless. Graphics: not involved. Missing: a reference game that uses
> it.

A validation bundle is one directory that says what a run of a game
observed, and with what: the engine version, commit, compiler and features,
the scenario and seed, every command that was run, the outcome of each
check, the logs, and the size and SHA-256 hash of every file. An agent (or a
person) hands it to a reviewer in place of "I ran it and it worked".

The tool is the `validation_bundle` example. It never opens a window unless
you pass `--windowed`.

```sh
cargo run --example validation_bundle -- --out target/bundles/buggy --variant buggy
cargo run --example validation_bundle -- --out target/bundles/fixed --variant fixed --launch
cargo run --example validation_bundle -- compare target/bundles/buggy target/bundles/fixed
```

A bundle shows what one run observed. It is not proof of pixel identity or
of visual correctness, and it does not replace the tests.

## The reference interaction

The example carries a small game: a body that jumps while a button is held.
A tick-indexed [input scenario](actions.md#input-scenarios) presses the
button on one tick and releases it later; the game writes a row per tick
into a reflected `Trace` resource. Four checks are held against the trace:
the press arrives on its tick, the jump rises, there is one jump per press,
and the body is back on the ground at the end.

`--variant buggy` jumps again while the button is still held, on purpose:
that run fails `one-jump-per-press` with the tick of the second jump.
`--variant fixed` passes. `--seed N` sets the game's seed.

## In this process, or as a launched game

Without `--launch` the game runs headless inside the tool.

With `--launch` the tool builds the game with `cargo build`, starts it as its
own process and drives it through the [MCP tools](agents.md) an agent uses:
`launch`, `instance_capabilities`, `run_input_scenario`, `instance_info`,
`run_ticks`, `get_resource`, `read_logs`, `system_timings`, `perf_report`,
`screenshot` (windowed runs) and `quit_instance`. Each call is in the
manifest with its arguments and whether it succeeded. The trace and the run
log of a launched run are byte-for-byte those of the in-process run.

| Option | What it does |
|---|---|
| `--launch` | build this example and start it |
| `--launch-program PATH` | start a game that is already built (no build step) |
| `--launch-cwd DIR` | the cargo project to build and start in |
| `--launch-example NAME`, `--launch-bin NAME` | which target to build |
| `--launch-release` | build with `--release` |
| `--launch-arg ARG`, `--launch-env NAME=VALUE` | repeatable; passed to the game. Only the names of environment variables are recorded |
| `--build-timeout-secs N`, `--launch-timeout-secs N` | bounds for the build and for the game to open its control endpoint |
| `--windowed` | open a window and keep a screenshot |

## What a failed run looks like

The tool exits 1 when the run failed, 2 when it could not write the bundle;
the bundle is written either way. `result.failure.kind` in the manifest
says which part failed, so a broken build is never mistaken for a broken
game:

| Kind | Meaning | Where to look |
|---|---|---|
| `step_failed`, `step_timeout` | a `--step` command exited non-zero or could not be started, or ran past `--step-timeout-secs` (its status is under `commands`) | `logs/step-N.log` |
| `build_failed`, `build_timeout` | `cargo build` of the game failed or ran past its bound | `logs/build.log` |
| `launch_failed` | the game exited before its control endpoint opened | `logs/process-output.log` |
| `launch_timeout` | the game was still running but never opened its control endpoint | `logs/process-output.log` |
| `scenario_failed` | the scenario could not be run to its end | `scenario-report.json` |
| `assertion` | a check failed; the failure names the check and its tick | `logs/run.log`, `trace.csv` |

Output of commands and of the game is kept bounded: the start and the end,
with the number of omitted bytes in between.

## What is in the directory

| File | Kind | Holds |
|---|---|---|
| `manifest.json` | | everything below, and the hash of every other file |
| `trace.csv` | `trace` | one row per tick of the reference game |
| `logs/run.log` | `run-log` | the game's banner, its jump lines and each check's outcome |
| `logs/step-N.log`, `logs/build.log` | `step-log` | output of a `--step` command, and of the build |
| `logs/process-output.log` | `process-output` | what the launch printed |
| `logs/game.log` | `game-log` | the launched game's stdout and stderr, read with `read_logs` |
| `scenario-report.json` | `scenario-report` | the engine's report of the input scenario |
| `scenario.json` | `scenario` | the scenario file, for a `--scenario` run |
| `performance/report.json`, `performance/system-timings.json` | `performance` | the performance report and per-system timings |
| `screenshots/final.png` | `screenshot` | the last frame of a `--windowed` run |

The manifest (`"schema": "ponko-validation-bundle"`, with a
`schema_version`) has these sections: `build` (engine version, commit and
whether the checkout was dirty, compiler, features, target), `scenario`,
`settings`, `commands`, `assertions` (each with expected, actual, outcome
and tick), `result`, `instance` (for a launched game), `metrics`,
`redaction` and `artifacts`.

The commit comes from `git rev-parse HEAD` and the compiler from
`rustc --version`; `--commit`, `--rustc` and `--features a,b` say them
instead, and `--no-capture` leaves them `unavailable`. For a launched game
the features are the ones the game itself reports.

Anything a run can't produce is marked `unavailable` with the reason, never
left out silently: screenshots of a headless run, performance without
`--perf` or `--launch`, wall time (omitted so that identical runs make
identical bundles).

## Performance

`--perf` profiles the in-process run (`Game::profile_headless`); a launched
run always keeps the game's `perf_report` and `system_timings`. These are
wall-clock measurements of one run on one machine. They differ between runs
of the same setup, so `compare` lists them as not compared. Use the
[benchmarks](performance.md) to make a performance claim.

## Screenshots and visual evidence

A headless run draws nothing, and says so. `--launch --windowed` opens the
game's window and keeps its last frame as `screenshots/final.png`.

A screenshot in a bundle is an image nobody has looked at yet. It becomes
visual evidence when someone looks and records what they saw:

```sh
cargo run --example validation_bundle -- inspect target/bundles/fixed \
  --image screenshots/final.png --verdict ok \
  --note "the body rests on the ground line" --by reviewer
```

The verdict is `ok`, `wrong` or `unclear`, and a note is required. The note
goes into `inspections.json` next to the manifest together with the image's
hash, so it stops counting if the image is replaced. `compare` shows each
image as inspected (with its verdict) or `not inspected`, and never treats
two screenshots as equal because their bytes are.

## Comparing two bundles

```sh
cargo run --example validation_bundle -- compare A B          # text
cargo run --example validation_bundle -- compare A B --json   # for a tool
```

`compare` exits 0 when both bundles are intact and have the same setup and
the same result, and 1 otherwise. It prints:

- whether each bundle's files are the ones its manifest lists;
- setup differences, by path into the manifest
  (`settings.variant: "buggy" -> "fixed"`);
- commands that were the same but went differently (a tool call that was
  answered in one run and not in the other, how a build ended, how much
  output was cut). What was asked is setup; how it went is not, so these are
  named but don't make two runs differ;
- each result, with the failed check and its tick;
- checks that came out differently, with both actual values;
- deterministic files that differ (`trace.csv`, `logs/run.log`);
- files it does not compare, and why: performance, screenshots and process
  output vary between runs of the same setup.

Two runs of one setup compare as the same. `verify DIR` checks one bundle
alone: it names every file that is missing, changed or not listed.

## What is redacted

Control tokens, passwords and authorization values in any output are
replaced by `<redacted>`. Values of environment variables given with
`--launch-env` are never recorded, only their names. `--redact-paths` swaps
the working directory, the home directory and the bundle directory for
`<CWD>`, `<HOME>` and `<BUNDLE>` and leaves the rest of each path alone.
The temporary directory the MCP server uses is recorded as `<scratch>`. The
manifest's `redaction` section lists what was applied. Read a bundle before
you publish it: redaction covers known patterns, not everything a game may
print.

## A game of your own (Rust, C or C++)

`--scenario FILE` puts a launched game of your own through an
[input-scenario file](actions.md#input-scenarios) in place of the reference
interaction. Its input steps are fed in through the game's real input path,
and its check steps (`expect_action`, `expect_resource`, `expect_count`) are
the assertions: the bundle passes when every check holds, and otherwise
names the failed step, its tick, and the expected and actual values. The
game gets only the arguments and environment you give it.

The game must be startable headless, must open its
[control endpoint](control.md#turning-it-on) when `launch` sets
`PONKO_CONTROL=1`, and should
[reflect](agents.md#making-your-game-agent-friendly) the state the checks
read. A release build honors `PONKO_CONTROL` only with the engine's
`control-env` feature, so for a C++ game build the C library with it, then
start the built executable:

```sh
cargo build -p ponko_ffi --release --features control-env
cargo run --example validation_bundle -- --out target/bundles/battle \
  --launch-program target/native/ponko_cpp_battle \
  --scenario examples/cpp/battle-validation.json \
  --launch-env UNITS=400 --launch-env SEED=7 --launch-env TICKS=1000000
```

`examples/cpp/battle-validation.json` checks the C++ battle's reflected
`Unit` and `Base` counts and its `Score` resource:

```json
{{#include ../../examples/cpp/battle-validation.json}}
```

`scripts/validation-bundle-native.sh` runs exactly this, and then the same
game against a check it can't meet.

## In the list of checks

`scripts/validation-bundles.sh` makes the reference bundles (broken and
corrected, in-process and launched), verifies each, requires the launched
trace to equal the in-process one and two launched runs to compare as the
same, and writes the broken-against-corrected comparison (by default to
`target/validation-bundles`). The list of checks (`scripts/checks.yml`) runs
it headless on Linux in its `validation-bundles` job, and its native job runs
`scripts/validation-bundle-native.sh` against the C++ battle on Linux and
macOS.

## Limits

- The listed check runs headless, so it makes no screenshot. The windowed path needs
  a machine with a display. It was run by hand on macOS: the window opened,
  the checks passed with the same trace as the headless run, and the kept
  frame was inspected and recorded as described above.
- The list of checks does not run the native script on Windows.
- A launched run needs the game to open the control endpoint
  (`PONKO_CONTROL=1`, which `launch` sets and which a release build honors
  only with the `control-env` feature); a release game built without it
  fails with `launch_failed`, which says so.
