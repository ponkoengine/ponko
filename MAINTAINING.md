# Maintaining Ponko

Procedures for maintainers, and for anyone whose change touches the public
API or the dependencies. A first contribution needs none of this:
[CONTRIBUTING.md](CONTRIBUTING.md) is the place to start. How a release is
cut is in [RELEASING.md](RELEASING.md); who decides and who merges is in
[GOVERNANCE.md](GOVERNANCE.md).

- [Breaking changes: the migration note](#breaking-changes-the-migration-note)
- [API snapshots](#api-snapshots)
- [Semver checks](#semver-checks)
- [Dependency audit](#dependency-audit)
- [Nightly runs on maintainer machines](#nightly-runs-on-maintainer-machines)

## Breaking changes: the migration note

[STABILITY.md](STABILITY.md) promises that every break comes with what to use
instead. A change that breaks code written against the last release (a
renamed, moved or removed item, a changed signature, behaviour or default; the
semver check below finds some of them, you and the reviewer the rest) carries
its migration note in its own pull request:

- A changelog fragment with a `### Breaking changes` section
  ([changelog.d/README.md](changelog.d/README.md)). Each entry says **what
  changed**, **what to write instead**, and shows the code **before and
  after**:

  ````md
  ### Breaking changes
  - `Foo::bar` takes a `Baz` now.
    Instead: wrap the argument, `Baz::new(x)`.
    Before:
    ```rust
    foo.bar(x);
    ```
    After:
    ```rust
    foo.bar(Baz::new(x));
    ```
  ````

  `python3 scripts/changelog.py --check` refuses an entry that
  lacks any of the three.
- The **`breaking` label** on the pull request. The two go together, in both
  directions: the Semver check fails a pull request that breaks the API
  without the label and a fragment, and fails one that has the label but no
  such fragment (the tool does not see every break, and a label is a promise
  of a migration note). Run the check again with `--breaking` once the
  label is on.
- **Deprecate first where it is cheap**, as STABILITY.md's "Releases" says: a
  replaced item stays for one minor as `#[deprecated(note = "use X")]` (Rust)
  or with a note in the header (C), pointing at its replacement, and the next
  minor removes it. It is cheap when the old item can forward to the new one
  without keeping a second implementation alive: a rename, a moved item, a
  method that became a builder step. It is not cheap when the old and new
  behaviour can't coexist (a changed default, a changed data layout, a type
  that gained a required field): remove it at once, with the entry above.
  Deprecating is itself a change that goes under `### Deprecated`, not
  `Breaking changes`, and names the replacement too. The removal, a minor
  later, is the change that gets the `Breaking changes` entry.

At release time `scripts/changelog.py --version X.Y.Z` folds the entries into
the **Breaking changes** block of the CHANGELOG (the short form: what changed
and what to write instead, which is also the GitHub release's notes) and writes
the same entries, with their snippets, to `book/src/migrating-to-X.Y.md`, which
it lists in `book/src/SUMMARY.md`. Nobody writes the chapter by hand. See
[RELEASING.md](RELEASING.md).

## API snapshots

The `api/` folder holds a generated listing of the public API of every
published crate, one item per line, made with
[cargo-public-api](https://github.com/cargo-public-api/cargo-public-api). The
**API snapshots** check (`python3 scripts/api-snapshots.py check`) regenerates
it and fails when it differs from the
committed files, so a pull request that adds, renames, widens or removes a
public item shows it as a diff, and the reviewer decides whether the surface
is meant. 1.0 needs a deliberate surface; this is how it stays one. It is a
quick check (no tests): `update` took 1.9 minutes on a busy laptop with a warm
dependency build, and a rerun is under half a minute.

**After changing public API, run one command and commit the result:**

```sh
python3 scripts/api-snapshots.py update
git add api
```

- **What is listed.** `ponko`, `ponko_ecs`, `ponko_ffi`, `ponko_mcp` and
  `ponko_unity` (not proc-macro or binary-only crates), as
  `api/<crate>/<set>.txt`: `default` (default features), `no-default-features`
  (for `ponko`: headless), `docs-rs` (the features docs.rs builds: all the
  public ones, `3d` included) and, for `ponko`, `3d` (the defaults plus `3d`).
  A set that lists the same as `default` has no file. Blanket and auto-trait
  impls are left out as noise; derived impls (`Clone`, `Debug`) stay.
- **Experimental items** (`scripts/experimental-api.txt`, STABILITY.md
  "Maturity levels") are listed apart, in `api/<crate>/experimental.txt`,
  from every set: the other files are exactly the API the versions cover. A
  change to them still shows up in the diff. Adding an item to the list (or
  taking one off) moves its lines on the next `update`;
  `python3 scripts/experimental-api.py check` says when the list, the guide's
  levels, `api/` and the C header disagree.
- **Reading a failure.** The output shows the diff and the command above. If
  you cannot run the tool, say so in the pull request and a maintainer
  regenerates the folder. A diff you did
  not intend means something became public by accident (`pub` instead of
  `pub(crate)`, a `pub use` that pulls in more than it should): fix the code,
  not the snapshot.
- **Merge conflicts in `api/`** (two pull requests added public API): do not
  merge the files by hand. Resolve the source, then run the command again.
- **Tools.** The script pins `cargo-public-api` and the nightly toolchain it
  needs together (`TOOL_VERSION` and `NIGHTLY`), because the tool reads rustdoc
  JSON, whose format changes with every nightly; `pins` prints them, so there
  is one place to change them. The script tells you the
  install commands if something is missing: `cargo install --locked
  cargo-public-api --version <pin>` and `rustup toolchain install <nightly>
  --profile minimal` (the nightly does not become your default). It also
  needs the audio and udev headers on Linux, like every build here. The
  listing is the one for x86_64 Linux: on macOS the script builds
  for `x86_64-apple-darwin` (same CPU types, no Linux libraries needed; it
  tells you the one `rustup target add` command) and drops the few items that
  exist only on macOS, so `update` gives the same text as on Linux. Another
  host (Windows) needs a maintainer on Linux or macOS to regenerate them. To move to
  a newer nightly, pick a tool release whose README supports it, change both
  pins, run `update`, and review the diff: it should only change where the
  tool's changelog says it does.
- **Relation to the Semver check** (below). The snapshot shows *every*
  change to the public API, growth included, and compares with the commit
  you are based on. Semver shows only what *breaks* code that compiled against
  the last crates.io release, and compares with that release. A new function
  fails the snapshot check (update the snapshot) and passes the semver check; a
  removed function fails both (update the snapshot, and add the `breaking`
  label and fragment, or undo the break). The snapshot cannot say whether a
  change breaks anything: that is what the semver check and the reviewer do with
  the diff.

## Semver checks

The **Semver checks** (`python3 scripts/semver-checks.py run`, the `semver`
job of [scripts/checks.yml](scripts/checks.yml)) compare the public API of every published
crate with its last release on crates.io, using
[cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks), and
reports what a change breaks. [STABILITY.md](STABILITY.md) promises that a
patch release (0.1.x) keeps code compiling that compiled against 0.1.0; this is
the check behind that promise. It is a quick check (a few minutes); run it
before merging a change to public API.

- **What is checked.** `ponko`, `ponko_ecs`, `ponko_ffi`, `ponko_mcp` and
  `ponko_unity`, each with the features docs.rs builds for it
  (`[package.metadata.docs.rs]` in its Cargo.toml; not `hotpatch` or
  `bench-internals`, which are not API). A new publishable crate is picked up
  by itself. `ponko_macros` is listed as not checked: the tool reads no API
  from a proc-macro crate.
- **Experimental items** don't count. A crate whose every `Failed in:` line
  names an item of `scripts/experimental-api.txt` (or one of its cargo
  features) is reported as `experimental`, not as a break, on any branch: they
  are outside the promise (STABILITY.md, "Unstable"). A name the tool prints
  that a covered item shares (a short type name) never excuses a failure.
- **What fails.** On a `release-*` branch (patch releases) and on a pull request
  into one, every break fails the check. On `main`, breaks pile up until the next
  minor release, so the comparison with crates.io only reports them. What
  fails a pull request is a break *it* adds (found by comparing with the commit
  it is based on), unless it carries the **`breaking` label** *and* a
  [changelog fragment](changelog.d/README.md) with a `### Breaking changes`
  entry (what changed, what to write instead, before and after code). Both
  are needed: the label tells the reviewer, the fragment becomes the migration
  chapter. The check also fails the other way round: the label without such a
  fragment, whether or not the tool found a break.

**Reading a failure.** Read the table at the end of the output (or the
section "before this change: <crate>"). Each failed lint has a name
(`inherent_method_missing`), a description, a link to the Cargo reference,
and a `Failed in:` list of items with the file they were in at the last release.

**Acting on it:**

1. *Undo the break* if it was not meant: keep the old item next to the new
   one and mark it `#[deprecated(note = "use ...")]`, or make the type
   `#[non_exhaustive]` before it grows. On a release branch this is the only way.
2. *Announce it* if it is meant (on `main` only): add the fragment, add the
   `breaking` label, and **run the check again with `--breaking`**.
   Maintainers add the label; a contributor can ask for it in the pull request.
3. *Tool errors* (the check could not build a crate, or the tool does not read
   this Rust version) are not breaks and no label excuses them. See the note at
   the top of the `semver` job in checks.yml about the pinned tool and Rust
   versions.

To run it, with the pinned tool (`cargo install --locked
cargo-semver-checks --version 0.51.0`; it needs the Linux audio and udev headers
the other builds need), after `git fetch origin`:

```sh
python3 scripts/semver-checks.py run                      # a change for main
python3 scripts/semver-checks.py run --breaking           # one with the `breaking` label
python3 scripts/semver-checks.py run --base release-0.1   # one for a release branch
```

It compares with `git merge-base origin/<base> HEAD`; the output says what a
failing change would need. For one crate,
`cargo semver-checks check-release -p ponko_unity --default-features --release-type minor`
shows the lints directly.

**What it does not catch.** The tool compares the public API as rustdoc
describes it, with a fixed list of lints (`cargo semver-checks check-release
--list`); a break no lint describes passes. Know these gaps when you review:

- Behaviour with an unchanged signature: a different result, panic, default
  value, ordering, timing, or determinism; what a saved world or a
  save file contains; the fields of the control endpoint and MCP messages.
- Many type changes in fields, parameters and return values, and tightened
  generic bounds, are covered only where a lint exists for them.
- Features outside the checked set (`hotpatch`, `bench-internals`) and
  platform-specific API on other targets: only the Linux target with the
  docs.rs features is built.
- The C and C++ headers (`include/`) and the layout of `#[repr(C)]` types as
  C sees them: the tool reads the Rust side of `ponko_ffi` only. The C ABI tests
  and `scripts/check-ffi-coverage.py` are what guard the header.
- Anything behind `#[doc(hidden)]` (such as `ponko::__ffi`), which is
  unstable by definition, and a new crate's first release (nothing to compare).
- Proc-macro crates (`ponko_macros`): the names and attributes of the derives
  are not compared.
- Dependencies: when a public type from `glam`, `wgpu` or another crate in our
  API changes major version, our API breaks without a lint saying so.

## Dependency audit

[`deny.toml`](deny.toml) holds the policy that [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)
checks: allowed licenses, security advisories (the RustSec database), banned
and duplicate crates, and allowed sources (crates.io only). Run it on any
pull request that changes `Cargo.toml`, `Cargo.lock` or `deny.toml`;
maintainers also run it on `main` from time to time (and before a release),
since new advisories appear without any change here:

```sh
cargo install --locked cargo-deny --version 0.20.2   # once; the pinned version
cargo deny --locked check -D warnings
```

When it fails, the output names the crate and the dependency chain that
brought it in:

- **A license is not allowed** (`licenses`). Prefer a dependency with an
  allowed license. The list in `deny.toml` is what the C library's
  third-party notices can carry; adding a license is a maintainer decision:
  say in the pull request why it is acceptable for the release archives.
  A crate that declares no license is refused by the notices too.
- **An advisory** (`advisories`: vulnerability, unmaintained, unsound or
  yanked). Update the dependency, with the smallest bump that fixes it
  (`cargo update -p <crate>`; `cargo tree -i <crate>` shows who needs it), and
  run the full checks. If nothing fixed exists, add an `ignore` entry to
  `deny.toml` with the advisory id and a `reason`: how we reach the crate and
  why that is acceptable, and what ends the exception. Never ignore without a
  reason.
- **A duplicate version** (`bans`). Align versions when you can (a newer
  release of the parent, or `cargo update`). A duplicate a dependency we don't
  control forces gets a `skip` entry with a `reason` naming the dependency
  that needs the old version. Don't skip a duplicate caused by our own
  requirements: update those.
- **A banned crate or a source that is not crates.io.** Both are deliberate:
  the release archives must not need a system TLS stack, and releases build
  from crates.io alone. Change the policy only with a maintainer's agreement.
- **A stale exception** (a warning, an error with `-D warnings`): the
  `skip` or `ignore` entry matches nothing any more. Delete it.

A new advisory found on `main` is handled the same way: fix, or ignore with a
reason, in a pull request.

## Nightly runs on maintainer machines

Use a dedicated `main` checkout on each macOS, Windows, and Linux test
machine. Provision the tools under the scheduler account and configure
authenticated `gh` with permission to manage issues in this repository.
Schedule one invocation per OS and keep the report directories outside the
checkout.

`--publish DIRECTORY` replaces the OS's latest status files in that directory
after each completed run. The file pairs are `macos.json` and `macos.md`,
`windows.json` and `windows.md`, and `linux.json` and `linux.md`. Copy these
files to the website's static assets for its status page. Use the Markdown
table for the README or link to the status page from the README. Use the
UTC completion time to show the age of the result. Keep the raw
`results.json` and `summary.txt` in the machine's private report directory.

`python3 scripts/nightly-issue.py /path/to/results.json` previews the issue
action using the scheduler account's GitHub access. Add `--apply` to
create or update the open issue titled “Nightly checks failing on <OS>”,
or close it when a run passes. A run that executes zero checks publishes
an incomplete status and leaves issues alone. Use full nightly reports for
issue updates.

On macOS and Linux, point the scheduler at a Bash script like this one.
It sends a fresh report to the issue updater even when a check fails.

```sh
#!/usr/bin/env bash
set -euo pipefail
cd /path/to/ponko
test "$(git branch --show-current)" = main
dirty=$(git status --porcelain)
test -z "$dirty"
git pull --ff-only origin main

python=/path/to/python3
reports=/path/to/private/ponko-latest
status=/path/to/public-status-staging
rm -f "$reports/results.json"
result=0
"$python" scripts/run-checks.py --out "$reports" --publish "$status" || result=$?
if test -f "$reports/results.json"; then
  "$python" scripts/nightly-issue.py "$reports/results.json" --apply
fi
exit "$result"
```

On Windows, use a wrapper that initializes the Visual Studio developer
environment, then runs a PowerShell script like this one.

```powershell
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
Set-Location 'C:/path/to/ponko'
$branch = git branch --show-current
if ($LASTEXITCODE -ne 0 -or $branch -ne 'main') { exit 2 }
$dirty = git status --porcelain
if ($LASTEXITCODE -ne 0 -or $dirty) { exit 2 }
git pull --ff-only origin main
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$reports = 'C:/path/to/private/ponko-latest'
$status = 'C:/path/to/public-status-staging'
$report = Join-Path $reports 'results.json'
if (Test-Path -LiteralPath $report) { Remove-Item -LiteralPath $report }
py -3 scripts/run-checks.py --out $reports --publish $status
$result = $LASTEXITCODE
if (Test-Path -LiteralPath $report) {
  py -3 scripts/nightly-issue.py $report --apply
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
exit $result
```

Configure the scheduler with the needed tool `PATH` and keep stdout and
stderr to diagnose checkout or reporting errors. On a dedicated test host
with an active graphical session, add `--include-windowed` to the runner
invocation to include the windowed lanes.

| Scheduler | Nightly setup |
| --- | --- |
| macOS launchd | Create a per-user LaunchAgent in `~/Library/LaunchAgents`. Set `ProgramArguments` to the absolute Bash and script paths, `StartCalendarInterval` to hour 2 and minute 0, and `StandardOutPath` and `StandardErrorPath` to log files. Load it with `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/org.ponko.checks.plist`. |
| Linux systemd | Create a user service in `~/.config/systemd/user/ponko-checks.service` with `Type=oneshot` and `ExecStart=/bin/bash /absolute/path/to/script`. Create `ponko-checks.timer` with `OnCalendar=*-*-* 02:00:00` and `Persistent=true` in `[Timer]` and `WantedBy=timers.target` in `[Install]`. Run `systemctl --user enable --now ponko-checks.timer`. Use a system service or enable user lingering if it must run after logout. |
| Windows Task Scheduler | Create a daily trigger at 02:00 and use the Visual Studio environment wrapper as the action. Have the wrapper pass through the PowerShell script's exit code. Select “Do not start a new instance” when an invocation overlaps another. |

For the first invocation on each machine, omit `--apply` to check the
revision, environment, skip reasons, public files, and proposed issue
action. Enable issue updates once those match the expected schedule.
