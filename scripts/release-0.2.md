# Cutting 0.2: the exact commands

The order of work for `v0.2.0-rc.1` and, at least a week later, `v0.2.0`
(STABILITY.md, "Release candidates"). CONTRIBUTING.md, "Releasing", explains
each step; this page is the list to follow, with the numbers filled in. Every
command runs on a maintainer's machine from a clean checkout. Nothing here
runs by itself: there is no CI.

`V` below is the version of the release being cut: `0.2.0-rc.1` (then
`0.2.0-rc.2`, ... if a candidate needs fixing), and finally `0.2.0`.

## 0. Before the cut (once)

```sh
git switch main && git pull
python3 scripts/changelog.py --check --compile   # every fragment valid, every After: block builds
python3 scripts/changelog.py --areas             # where each entry will be told; fix with <!-- area: ... -->
python3 scripts/changelog.py --dry-run --version 0.2.0-rc.1 | less   # the fold, read as a user
```

Read `book/src/release-0.2.md` (the story at the top is written by hand; the
fold copies it to the top of the CHANGELOG section) and the prose that names a
version, which no script rewrites:

```sh
grep -rn "0\.1\b\|new in 0\.2\|until that release\|before 0\.2" \
  README.md STABILITY.md ROADMAP.md ARCHITECTURE.md book/src/*.md \
  | grep -v "migrating-to-\|release-0\.2\|CHANGELOG"
```

At the time of writing that is `STABILITY.md`, `ROADMAP.md` ("Where things
stand"), `ARCHITECTURE.md`, and the chapters getting-started (`ponko_cli` "new
in 0.2; until that release, install it from the repository"), plugins, c-cpp,
comparison, control, servers, packaging and web-threads.

One-time, a maintainer: four crates are new on crates.io (`ponko_aseprite`,
`ponko_cli`, `ponko_image_diff`, `ponko_lobby`). The first `cargo publish`
makes its token's user their only owner; add the other maintainers
afterwards (`cargo owner --add <user> <crate>`).

## 1. The release pull request (on a branch)

```sh
git switch -c release/V
python3 scripts/changelog.py --version V        # fold: CHANGELOG.md, migrating-to-0.2.md, SUMMARY.md
python3 scripts/changelog.py --notes V          # regenerate the counts and breaking list of release-0.2.md
python3 scripts/release-check.py bump V         # every crate, the sibling requirements, the guide and templates
cargo update --workspace                        # Cargo.lock follows the new versions
python3 scripts/maturity.py --write             # the new chapters' rows in maturity.md
python3 scripts/check-book-snippets.py          # the guide's snippets, and the migration chapter's After: code
mdbook build book
```

`release-check.py bump` prints every file it changed: the root `Cargo.toml`,
the ten publishable `crates/*/Cargo.toml`, the five
`crates/ponko_cli/templates/*/Cargo.toml.liquid` and the guide chapters that
show `ponko = "0.1"`. For a candidate those lines become
`ponko = "0.2.0-rc.1"` (Cargo never picks a candidate for `"0.2"`, so the
candidate's `ponko new` would make projects that don't build); the `0.2.0`
bump turns them into `"0.2"`. For `0.2.0` itself, rename the folded
`## [Unreleased]` to `## [0.2.0] - YYYY-MM-DD`, put a new empty
`## [Unreleased]` above it and fix the two links at the bottom
(CONTRIBUTING.md, step 1). A candidate keeps `## [Unreleased]`.

Then the checks, on this branch:

```sh
python3 scripts/release-check.py check vV       # versions, notes size, migration chapter, pins, licenses
cargo deny fetch && cargo deny --locked check -D warnings
python3 scripts/semver-checks.py run            # the report against 0.1.0 on crates.io (a minor may break)
cargo publish --workspace --dry-run --locked    # package and build every crate as crates.io will
cargo run -p ponko_mcp --example agent_suite -- replay
python3 scripts/run-checks.py                   # the whole list on this machine
```

For `rc.1` (once per minor) also try a real agent on the agent acceptance
suite, one fresh session per task, and put
`target/debug/examples/agent_suite scores /tmp/suite/*.result.json` in the
pull request: "At a release" in `book/src/agents.md` has the steps, and
`book/src/check-results/macos.md` the 2026-11-26 rehearsal to compare with.

The checks on the other machines: `run-checks.py` on the Windows PCs
(Windows x64, DirectX 12 and Vulkan, AMD and NVIDIA), and the Linux x86_64
checks on a Linux machine, or in one run of the release-time CI with the
markers of what changed since the last candidate (for `rc.1` of a minor,
`linux-all`; see "Release-time CI" below). For `rc.1` also record the coverage numbers and
the benchmark baseline (CONTRIBUTING.md, step 3) in the pull request. Open
the pull request and merge it.

## 2. Tag, build, publish

```sh
git switch main && git pull
git tag -a vV -m "Ponko V" && git push origin vV
git switch --detach vV
python3 scripts/release-check.py check vV --on-branch
```

The six C library archives, each on its machine (CONTRIBUTING.md, "The C
library archives"), gathered in `dist/`:

```sh
python3 scripts/c-library.py build --target aarch64-apple-darwin          # and x86_64-apple-darwin, on the Mac
python3 scripts/c-library.py build --target aarch64-unknown-linux-gnu --zig --sysroot target/sysroot-arm64
python3 scripts/c-library.py build                                        # linux-x86_64, on Linux
python scripts/c-library.py build --target x86_64-pc-windows-msvc         # on Windows
python3 scripts/c-library.py smoke target/c-library/<archive>             # each one
python3 scripts/c-library.py smoke target/c-library/ponko-vV-linux-arm64.tar.gz --docker   # arm64 Linux, in a glibc 2.28 container (Docker)
```

`windows-arm64` is built on a hosted Windows on Arm runner (no maintainer
machine has the ARM64 tools): push the `release-matrix/windows-arm64/V`
branch of the release-time CI (below), then fetch the archive into `dist/`
(the artifact is kept 7 days):

```sh
gh run list --branch release-matrix/windows-arm64/V --workflow "Release CI"   # the run's id, once it is green
gh run download <id> -n ponko-c-library-windows-arm64 -D dist
```

The job builds the archive (`c-library.py build --target
aarch64-pc-windows-msvc`) and runs its smoke on the Arm runner, which runs
the C and C++ tests; a red job means no archive. (A Windows machine with the
ARM64 build tools makes the same archive with the same two commands.)

Checksums and the GitHub release (`--prerelease` for a candidate):

```sh
cd dist && shasum -a 256 ponko-vV-* > SHA256SUMS && cat SHA256SUMS && cd ..
python3 scripts/release-check.py notes vV notes.md
gh release create vV dist/ponko-vV-* dist/SHA256SUMS --title "Ponko V" --notes-file notes.md --prerelease
cargo publish --workspace --locked              # a crate owner's token; rerun if it stops halfway
python3 scripts/release-check.py published      # every crates.io version has its tag
```

## 3. After publishing

```sh
python3 scripts/first-run.py V                 # on the Mac and a Windows PC: what a new user does, from crates.io
```

Linux: push the `release-matrix/first-run/V` branch of the release-time CI
(below), or run the same command on a Linux machine. Then
redeploy the website (its latest Cloudflare Pages deployment, retried) so
https://ponko.dev/guide/ shows the release page and the migration chapter.
For `0.2.0` (not a candidate) publish the web gallery to `gh-pages`
(CONTRIBUTING.md, step 5.7). Delete `dist/`, `notes.md` and
`target/c-library`.

## 4. A week of rc.1, then 0.2.0

- Announce the candidate with `cargo add ponko@0.2.0-rc.1`; Cargo never
  picks it otherwise.
- A regression is fixed on `main` through a pull request with its fragment,
  and `rc.2` follows from step 1 (the next fold adds to the same migration
  chapter).
- No regression after a week: `0.2.0` is the last candidate with its version
  changed. Step 1 with `V=0.2.0` (the fold has nothing new; `bump`, the
  section rename and `--notes 0.2.0`), then steps 2 and 3 without
  `--prerelease`.

## Release-time CI

There is no CI, and GitHub Actions is off for the repository. Hosted runners
run only at a release, and only for what the maintainers' own machines can't
test:

| What | Where |
|---|---|
| macOS (arm64, x86_64): checks, C library archives, first run | the Mac |
| Windows x64 (DirectX 12 and Vulkan, AMD and NVIDIA): checks, C library archive, first run | the Windows PCs |
| Linux arm64 C library archive and its C/C++ tests | the Mac, in Docker (`c-library.py smoke --docker`) |
| Linux x86_64 checks | hosted `ubuntu-24.04`, job `linux`, when marked |
| Windows on Arm C library archive | hosted `windows-11-arm`, job `windows-arm64`, when marked |
| Linux first run from crates.io | hosted `ubuntu-24.04`, job `first-run`, when marked |

The workflow is `scripts/release-matrix.yml`, generated from
`scripts/checks.yml` by `python3 scripts/release-matrix.py write`; the
checks job ("Release scripts") fails if it is out of date, so a step added
to one of those `checks.yml` jobs reaches it by itself. It is not enabled:
GitHub never reads `scripts/`.

### Markers

**Nothing runs by default.** The branch it is pushed on,
`release-matrix/<markers>/<version>`, names what to test, markers joined by
`+`: `release-matrix/linux-native+windows-arm64/0.2.0-rc.1`. A branch
without a known marker runs no job. No marker is part of another, so a
marker can't select something by accident (`release-matrix.py selftest`
checks that, and that each marker selects exactly its steps).

| Marker | What it tests | Runner | Estimate |
|---|---|---|---|
| `linux-tests` | `checks.yml` `test`: the workspace tests (unit, integration, doc), the examples build, the native crash test with frame pointers | `ubuntu-24.04` | 35–45 min |
| `linux-templates` | `templates`: the command line tool's tests, each `ponko new` template generated and tested | `ubuntu-24.04` | about 15 min |
| `linux-features` | `test-features`: the feature sets (no default features, editor, video, WebRTC, Steam, accessibility over AT-SPI, ...) | `ubuntu-24.04` | 45–55 min |
| `linux-native` | `native`: the C header, the C library's exports, the C and C++ test programs (cmake, ctest), the C++ validation bundle | `ubuntu-24.04` | about 15 min |
| `linux-package` | `package-smoke`: the packaged reference games, headless and in a window under Xvfb with software Vulkan | `ubuntu-24.04` | about 20 min |
| `linux-all` | all five above | `ubuntu-24.04` | 80–100 min |
| `windows-arm64` | the Windows on Arm C library: `c-library.py build --target aarch64-pc-windows-msvc` and its smoke (the C and C++ tests run); artifact `ponko-c-library-windows-arm64`, kept 7 days | `windows-11-arm` | 30–45 min |
| `first-run` | after `cargo publish`: `scripts/first-run.py V` against crates.io | `ubuntu-24.04` | about 15 min |

The Linux markers share ONE job and one build tree: the job runs when any of
them is there, and each group of steps only with its own marker (or
`linux-all`), so `linux-tests+linux-native` builds once. The timeouts are
150 minutes (`linux`: above the `linux-all` estimate, since a timeout loses
the whole run), 60 (`windows-arm64`) and 30 (`first-run`).

**Rust version.** `linux` and `windows-arm64` use the Rust release the
maintainers' machines checked the release revision with (`RUST` in
`release-matrix.py`, set at each cut and regenerated), not a floating
`stable`: with `-D warnings`, a lint added by a newer compiler would fail
every build step of a run. `first-run` uses `stable`, what a new user
installs.

**Mark only what changed or what the release needs.** For `rc.1` of a minor
that is `linux-all` once, plus `windows-arm64` from the tag and `first-run`
after publishing. A later candidate marks only the parts its fixes touched
(a C API fix: `linux-native+windows-arm64`). Never `linux-all` to "just
check". GitHub restricted the maintainer's account once for hours of
Actions a day, and warned that bulk or near-continuous runs bring the
restriction back: one `rc.1` costs about 150 runner minutes (`linux-all`
about 90, `windows-arm64` about 40, `first-run` about 15), and that is
the budget.

### Rules

- **The maintainer turns Actions on and off**, in the repository settings
  (Actions, General), for that one run, and off again as soon as it
  finished (or an agent the maintainer asked to do exactly that). Nobody
  else enables it, adds anything to `.github/workflows/` on `main`, or
  pushes a `release-matrix/**` branch.
- **One run per release candidate** and marker set.
- **No re-runs to see if it passes.** A red step is reproduced locally
  (for Linux, the step's commands in an `ubuntu:24.04` container, as
  below), fixed on `main` through a pull request, and checked by the next
  candidate's run. A run that died of the runner itself (a network or
  runner outage, not a test) may be pushed once more, with the reason in
  the release pull request.
- No caches are saved (a run's cache would never be read again), the
  artifacts are kept 3 days (the failed render images) and 7 days (the
  archive), `concurrency` cancels a run that a newer push on the same
  branch replaces, the workflow can only read the repository, and its
  third-party actions are pinned to full commits.

### A run

```sh
# the maintainer: Settings, Actions, General: allow actions
git switch --detach vV          # or the release branch, before tagging
mkdir -p .github/workflows && cp scripts/release-matrix.yml .github/workflows/
git switch -c release-matrix/linux-all/V
git add .github/workflows/release-matrix.yml && git commit -m "Release CI for V"
git push origin release-matrix/linux-all/V
# once it finished:
git push origin --delete release-matrix/linux-all/V
# the maintainer: Settings, Actions, General: disable actions
```

`windows-arm64` is pushed from the tag, before the release is created;
`first-run` after `cargo publish`. Write in the release pull request which
markers ran, their result and their minutes. The workflow file never goes
to `main`.

### The Linux job

One job, one build tree (`CARGO_TARGET_DIR` is the checkout's `target/`),
every step named `<checks.yml job> / <step>`. `release-matrix.py` takes each
step of the five jobs, evaluates its `if:` for Linux (the macOS and Windows
steps go), drops the `local-*` keys, the checkout, toolchain and cache
steps, installs each apt package once and turns the cached `cbindgen`
install into `cargo install --locked`. A step it can't place (an `if:` it
can't evaluate, a local action, an action without a pinned commit, a
`local-requires` it doesn't know how to install) stops it, so the job can't
silently lose a check.

- **Order.** `test` (the workspace with default features) first, then
  `templates` (the command line tool, same build), the `test-features`
  feature sets, `native` (the C library in release, cmake and ctest), then
  `package-smoke`. The frame-pointer crash test (a second build tree), the
  image margins and the failed images come last.
- **Failures.** Every check step runs `if: !cancelled() &&
  steps.setup.outcome == 'success'` and its marker: one failing step
  doesn't stop the others, but a failed checkout, apt install or toolchain
  (the `setup` step) stops the job at once instead of letting every step
  fail until the timeout.
- **GPU.** The runner has no GPU. The render tests find no adapter and skip
  with a logged reason (`target/render-test-output/skips.log`), and the last
  step lists them: a skip is not a pass. Mesa's software Vulkan (lavapipe)
  is not installed for them: the reference images and tolerances are kept on
  real GPUs (Metal on the Mac, DirectX 12 and Vulkan on AMD and NVIDIA on the
  Windows PCs), and a lavapipe run of the render tests was not verified.
  `package-smoke` needs lavapipe for its window runs under Xvfb, so its apt
  step installs it and the job runs it after every test step.
- **Disk.** Every part in one build tree came to about 45 GB of `target/`
  in the container (arm64; x86_64 should be close), more than a runner's
  system disk has free. The first step, "Free disk space", removes SDKs the job never uses
  (Android, .NET, GHC, CodeQL, Swift: about 25 GB) and puts `target/` on the
  runner's second disk when `/mnt` has more room; it prints `df` before and
  after, so the run shows the margin. Not yet seen on a real runner.

### Checked without Actions

Changes to the workflow are checked without spending runner time:
`python3 scripts/release-matrix.py selftest` and `check`, `actionlint
scripts/release-matrix.yml`, and the Linux job's steps run in an
`ubuntu:24.04` container. What that covered on 2026-11-27 (Docker on an
Apple Silicon Mac, so `linux/arm64`, 5 build jobs; the runner is x86_64
with 4 slower vCPUs, so the estimates above are these times scaled up):

- every step of `linux-all` once, in order: 54 minutes (some dependencies
  were already built by an earlier attempt), `target/` 45 GB. The apt packages (including `dbus` and `at-spi2-core`
  for the AT-SPI step), cbindgen, cmake/ctest, the C++ validation bundle,
  the templates and the packaged games all work. 831 render tests skipped
  for want of an adapter, as intended. Failures found, all outside the
  workflow: the native crash tests without line tables (#693), a UDP test
  that overflowed Linux's default receive buffer (#694; #692 has since
  sent it in rounds, not re-run on Linux), and `ponko_ffi`
  with `video` not linking on aarch64 (#695; x86_64 should not be affected). The run also
  found a bug in the workflow itself, fixed since: `package-smoke`'s apt
  step ran only if nothing had failed before it;
- `linux-native+linux-package` again from the marker workflow: only those
  steps ran (7.8 minutes on the warm tree), all green, including both
  window runs under Xvfb with lavapipe and the free-disk step.

Not covered: the hosted runner image itself (what it preinstalls, its real
disk and speed), the `windows-arm64` job (no Windows on Arm machine; its
commands are those of "The C library archives") and `first-run` (needs a
published release). The first real run of each is the check; its minutes
go in the release pull request.
