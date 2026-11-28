# Releasing

For maintainers. A release is a version tag, and a maintainer builds and
publishes it from that tag on their own machine, so the tag, the crates on
crates.io and the CHANGELOG section always come from one commit. There is no
release service: the steps below are the whole procedure, written so a second
maintainer can do a release from them alone ([GOVERNANCE.md](GOVERNANCE.md#continuity)
says why that matters and what is still missing). There are
three kinds of tag, all in the steps below: a release from `main`
(`v0.2.0`), a release candidate before a minor (`v0.2.0-rc.1`, see
[Release candidates](#release-candidates)), and a patch release from a
release branch (`v0.1.1` on `release-0.1`, see
[Patch releases and backports](#patch-releases-and-backports)).
[scripts/release-0.2.md](scripts/release-0.2.md) is the same procedure as
one list of commands for the 0.2 cut (rc.1, a week, 0.2.0).

1. Fold the fragments in: `python3 scripts/changelog.py --version X.Y.Z`
   (`--dry-run` shows the result first). If any fragment breaks the API, this
   also writes the migration chapter, `book/src/migrating-to-X.Y.md` (named
   after the minor: `migrating-to-0.2.md` for 0.2.0 and its candidates), from
   the same entries and lists it in `book/src/SUMMARY.md`; read it, then check
   the book (`mdbook build book`). The entries are grouped by area (`--areas`
   lists them). For a minor, `python3 scripts/changelog.py --notes X.Y.Z`
   regenerates the counts and the breaking list of the release page,
   `book/src/release-X.Y.md`, below its hand-written story, which the fold
   copies to the top of the CHANGELOG section; `python3 scripts/maturity.py
   --write` adds the new chapters to the maturity list. A patch release may
   not carry breaking entries. In `CHANGELOG.md`,
   add a fresh empty `## [Unreleased]` heading above the folded content and
   rename the folded one to `## [X.Y.Z] - YYYY-MM-DD`; point the
   `[Unreleased]` link at `compare/vX.Y.Z...HEAD` and add `[X.Y.Z]` beside it.
   The section becomes the GitHub release's notes, which hold at most
   125,000 characters: a longer section is cut after its story and breaking
   changes, with a link to the rest (`release-check.py notes`).
2. `python3 scripts/release-check.py bump X.Y.Z` does this step and lists the
   files it changed (`--dry-run` first); then `cargo update --workspace`.
   Set `version = "X.Y.Z"` in the root `Cargo.toml` and in every published
   `crates/*/Cargo.toml` (`ponko_bench` is `publish = false`), including the
   `version = "..."` requirements the crates put on each other, and refresh
   `Cargo.lock` (`cargo check --workspace`).
   Also update the dependency lines the guide and the project templates
   show (`ponko = "X.Y"` in `book/src/getting-started.md`, both tutorials and
   `crates/ponko_cli/templates/*/Cargo.toml.liquid`); the check in step 3 names
   any that is left. The "Which version?" note under every guide title needs no
   edit: `scripts/guide-note.py` reads the version from the root `Cargo.toml`
   and counts the fragments left in `changelog.d/`, so once this step is merged
   the guide says it documents exactly that release.
3. `python3 scripts/release-check.py check vX.Y.Z` lists what still disagrees
   (it does not need the tag to exist), and `cargo publish --workspace
   --dry-run` packages and builds every crate the way crates.io will. Re-run
   the agent acceptance suite (`cargo run -p ponko_mcp --example agent_suite
   -- replay`; it is also part of `cargo test --workspace`) and, for a minor,
   try a real agent on it ("The agent acceptance suite" in the guide's agents
   chapter) and put the scores in the pull request.

   For every release, including candidates and patches, run the
   [workspace coverage measurement](book/src/test-coverage.md) on the
   release revision. Record the executed and mapped line counts, coverage
   percentage, OS, GPU adapter and Rust toolchain in the release notes.
   Coverage is recorded without a percentage gate.

   For a minor release (and its first candidate), record the wall-clock
   baseline on the named benchmark machine, idle, and add it to the pull
   request:

   ```sh
   v=X.Y.Z
   m=macos-aarch64-apple-m4-max   # what bench_track calls the machine (`<os>-<arch>-<cpu>`)
   mkdir -p crates/ponko_bench/releases/$v/$m
   for i in 1 2 3 4 5; do
     PONKO_BENCH_JSON=crates/ponko_bench/releases/$v/$m/run-$i.json \
       cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'
   done
   cargo run --release -p ponko_bench --bin bench_track -- \
     compare crates/ponko_bench/releases/<previous>/$m crates/ponko_bench/releases/$v/$m
   ```

   Before a minor release, run the four-hour reference-game soaks in a
   real window on macOS, Windows and Linux. Run this from the release
   checkout on each machine (use a fresh output directory per revision):

   ```sh
   python3 scripts/soak.py --windowed --seconds 14400 --sample 60 --warmup 600 --out target/soak-release
   ```

   Follow the [soak checklist](scripts/checklists/soak.md) and link the three
   machines' reports and any growth issues in the release pull request.

   Link the folder and the comparison in the CHANGELOG section (the release
   notes). Run the checks of [scripts/checks.yml](scripts/checks.yml) on the
   release branch (all of them for a minor; on every OS the matrix names:
   macOS on the Mac, Windows x64 on the Windows PCs, Linux x86_64 on a Linux
   machine or in a run of the [release-time CI](#release-time-ci); say in
   the pull request which ran where), then open the pull request and merge it.
4. Tag the merge commit on `main` (on `release-X.Y` for a patch release) and
   push the tag:

   ```sh
   git switch main && git pull     # release-X.Y for a patch release
   git tag -a vX.Y.Z -m "Ponko X.Y.Z" && git push origin vX.Y.Z
   ```

5. Build and publish from a clean checkout of the tag
   (`git switch --detach vX.Y.Z`), in this order:

   1. **Verify**: `python3 scripts/release-check.py check vX.Y.Z --on-branch`
      (the tag is `vX.Y.Z` or `vX.Y.Z-rc.N` on a commit merged to `main` or
      to `release-X.Y` of the tag's own minor, every crate is at the tag's
      version, `CHANGELOG.md` has a non-empty `## [X.Y.Z]` section that fits
      in a release body, a release whose section has **Breaking changes** is
      a minor one with its migration chapter written, listed in the book's
      summary and holding every entry, the sub-crates carry the root license
      files and no fragment is left over). Make sure the checks passed on
      this exact commit.
   2. **Build the six C library archives** (see
      [The C library archives](#the-c-library-archives)):
      `linux-x86_64`, `linux-arm64`, `macos-arm64`, `macos-x86_64`,
      `windows-x86_64` and `windows-arm64`, with
      `python3 scripts/c-library.py build --target <triple>`, and check
      each with `python3 scripts/c-library.py smoke <archive>` (`--docker`
      for `linux-arm64`; `windows-arm64` is built by the
      `release-matrix/windows-arm64/X.Y.Z` branch on a hosted Windows on Arm
      runner, see below).
   3. **Package the crates**: `cargo publish --workspace --dry-run`.
   4. **Create the GitHub release** from the CHANGELOG section, with the
      six archives gathered in one folder (`dist/`, from each machine's
      `target/c-library/`):

      ```sh
      python3 scripts/release-check.py notes vX.Y.Z notes.md
      ls dist   # ponko-vX.Y.Z-{linux,macos}-{x86_64,arm64}.tar.gz, ponko-vX.Y.Z-windows-{x86_64,arm64}.zip
      (cd dist && shasum -a 256 ponko-vX.Y.Z-* > SHA256SUMS)
      gh release create vX.Y.Z dist/ponko-vX.Y.Z-* dist/SHA256SUMS --title "Ponko X.Y.Z" --notes-file notes.md
      ```

      Add `--prerelease` for a candidate (`vX.Y.0-rc.N`).
   5. **Publish the crates**: `cargo publish --workspace`, with a crates.io
      API token of a crate owner (`cargo login`; scope it to publishing these
      crates, and revoke or let it expire afterwards). If it stops halfway,
      run it again: crates already published are skipped. Then check, on
      macOS, Linux and Windows, what a new user gets from crates.io:
      `python3 scripts/first-run.py X.Y.Z` (installs `ponko_cli`, makes and
      tests each template against exactly that version, builds the guide's
      first program). Linux may be the `first-run` marker of the
      [release-time CI](#release-time-ci).
   6. **Redeploy the website** (https://ponko.dev/, which builds the guide
      from `main`; its own repository, `ponkoengine/ponko-website`): retry
      its latest deployment in Cloudflare Pages, or let its next deployment
      pick the guide up. The API reference of the release appears on docs.rs
      by itself.
   7. **Publish the web gallery** of the examples (see
      `web/gallery/README.md`), for a release that is the newest version
      (not for a candidate, nor for a patch of an older minor): build it for
      the tag and check every page in headless Chromium, then put the folder
      on the `gh-pages` branch, which GitHub Pages serves
      (its address is derived from `PROJECT_URL`; there is no CI, so the
      maintainer pushes the branch by hand, and the gallery has no domain of
      its own):

      ```sh
      node --test web/gallery/*.test.mjs web/smoke/gallery-lib.test.mjs
      node web/gallery/build.mjs --ref vX.Y.Z --out target/web/gallery
      node web/smoke/gallery.mjs --dir target/web/gallery --browser chromium --backend webgl2
      ```

      Then push the branch with the commands in
      [web/gallery/README.md](web/gallery/README.md#publishing) (one commit,
      force-pushed), and open the address. A failed page stops only the
      gallery, not the release.

## The C library archives

`scripts/c-library.py` makes each archive from the tag's checkout. It builds
`ponko_ffi` with its default features (`cargo build --release --locked
--target <triple>`) and packs a folder `ponko-vX.Y.Z-<os>-<arch>` holding
`include/ponko.h` and `include/ponko.hpp`, the shared and static libraries
in `lib/` (`libponko_ffi.so` and `.a`, `libponko_ffi.dylib` and `.a`, or
`ponko_ffi.dll`, its import library `ponko_ffi.dll.lib` and
`ponko_ffi.lib`), `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, and
`THIRD-PARTY-NOTICES.txt` for every library linked into it on that target.
It refuses a library built for the wrong CPU and gives the macOS library the
install name `@rpath/libponko_ffi.dylib` (the linker writes the path it was
built at). The archive goes to `target/c-library/` as `.tar.gz` (`.zip` for
Windows). `targets` lists the six, `--dry-run` shows the commands and the
layout, and `--help` has the rest.

`smoke <archive>` unpacks it elsewhere, builds `examples/cpp` against its
`include/` and `lib/` the way the C/C++ chapter tells users to, and runs the
C and C++ tests with ctest. Where the programs can't run on the machine it
only builds them, and says so. Each `--target` is a whole extra release
build: delete `target/<triple>` once its archive is uploaded.

**macOS (Apple silicon): `macos-arm64` and `macos-x86_64`.** The two are
separate archives: the arm64 one keeps its 0.1.0 name and contents, each
download is half the size of a universal one (about 58 MB each against
117 MB), and each has its own third-party notices (the two targets link
slightly different crates). Users who want a universal library join them
with `lipo -create`.

```sh
rustup target add x86_64-apple-darwin
python3 scripts/c-library.py build --target aarch64-apple-darwin
python3 scripts/c-library.py build --target x86_64-apple-darwin
python3 scripts/c-library.py smoke target/c-library/ponko-vX.Y.Z-macos-arm64.tar.gz
python3 scripts/c-library.py smoke target/c-library/ponko-vX.Y.Z-macos-x86_64.tar.gz
```

The Intel programs run under Rosetta (`softwareupdate --install-rosetta`);
without it the smoke only builds them.

**Linux x86_64: `linux-x86_64`**, on the Linux machine, with
`sudo apt install libasound2-dev libudev-dev cmake g++`:
`python3 scripts/c-library.py build`, then `smoke` it. The library needs the
glibc of that machine or newer.

**Linux arm64: `linux-arm64`**, cross-built from the Mac (or the Linux
machine) with zig as the linker, for glibc 2.28:

```sh
brew install zig                 # or zig from ziglang.org on Linux
cargo install cargo-zigbuild --locked
rustup target add aarch64-unknown-linux-gnu
```

The linker needs the target's ALSA and udev libraries. Download the arm64
packages `libasound2`, `libasound2-dev`, `libudev1` and `libudev-dev` of a
Debian release (packages.debian.org, "arm64", or `apt-get download
<package>:arm64` on Debian/Ubuntu with the arm64 architecture added), then:

```sh
python3 scripts/c-library.py sysroot --out target/sysroot-arm64 <the four .deb files>
python3 scripts/c-library.py build --target aarch64-unknown-linux-gnu --zig --sysroot target/sysroot-arm64
python3 scripts/c-library.py smoke target/c-library/ponko-vX.Y.Z-linux-arm64.tar.gz --zig
```

The `--zig` smoke compiles and links the C and C++ programs for arm64 with
zig, which proves the link only. To run them, use an arm64 Linux machine
(copy the archive there, install `cmake g++ libasound2 libudev1` and run
`smoke` without `--zig`) or Docker, which runs arm64 Linux natively on an
Apple silicon Mac:

```sh
python3 scripts/c-library.py smoke target/c-library/ponko-vX.Y.Z-linux-arm64.tar.gz --docker
```

`--docker` unpacks the archive in a `rockylinux:8` container (arm64, glibc
2.28: the floor of the archive, so a library that needs a newer glibc fails
here), installs CMake, a newer GCC, ALSA and udev, builds `examples/cpp`
against the archive and runs its ctest programs. The container has no GPU:
the programs ctest runs are headless (no window, no GPU) and none is
skipped. It needs the network and Docker running (`open -a Docker`);
stop it afterwards and remove the image (`docker rmi rockylinux:8`). The
`linux-x86_64` archive takes the same flag (`--image` picks another image).

**Windows x64: `windows-x86_64` and `windows-arm64`**, in PowerShell, with
Python, CMake and Visual Studio 2022 (or its Build Tools) with the "Desktop
development with C++" workload. The arm64 archive also needs the component
"MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools (Latest)" (Visual Studio
Installer, Modify, Individual components; or `vs_installer.exe modify
--installPath <the Visual Studio folder> --add
Microsoft.VisualStudio.Component.VC.Tools.ARM64`). rustc finds the ARM64
linker by itself; nothing in `ponko_ffi` compiles C for Windows.

```powershell
rustup target add aarch64-pc-windows-msvc
python scripts/c-library.py build --target x86_64-pc-windows-msvc
python scripts/c-library.py build --target aarch64-pc-windows-msvc
python scripts/c-library.py smoke target/c-library/ponko-vX.Y.Z-windows-x86_64.zip
python scripts/c-library.py smoke target/c-library/ponko-vX.Y.Z-windows-arm64.zip
```

On an x64 machine the arm64 smoke builds the programs (`cmake -A ARM64`,
the Visual Studio generator) without running them; on a Windows on Arm
machine the same command runs them.

No maintainer machine has the ARM64 tools, so the release builds
`windows-arm64` on a GitHub-hosted Windows on Arm runner (`windows-11-arm`)
instead: the job `windows-arm64` of the [release-time CI](#release-time-ci),
run by pushing the branch `release-matrix/windows-arm64/X.Y.Z` from the tag.
It runs the two commands above for `aarch64-pc-windows-msvc`, so the archive
is smoke-tested by running the tests, and uploads it as the workflow artifact
`ponko-c-library-windows-arm64`. Download it into `dist/` with
`gh run download <run> -n ponko-c-library-windows-arm64 -D dist` (it is
kept 7 days), delete the branch, and attach it with the other five.

## Release-time CI

There is no CI, and GitHub Actions stays off. At a release, hosted runners
may do what the maintainers' machines can't: the Linux x86_64 checks
(`ubuntu-24.04`), the Windows on Arm C library archive (`windows-11-arm`)
and the Linux first run from crates.io. The workflow,
`scripts/release-matrix.yml`, is generated from `scripts/checks.yml` by
`scripts/release-matrix.py`. Nothing in it runs by default: the branch it is
pushed on, `release-matrix/<markers>/<version>` (markers joined by `+`),
marks each part to run.

| Marker | What it tests | Estimate |
|---|---|---|
| `linux-tests` | the workspace tests (`checks.yml` `test`) | 35–45 min |
| `linux-templates` | the `ponko new` templates (`templates`) | about 15 min |
| `linux-features` | the feature sets (`test-features`) | 45–55 min |
| `linux-native` | the C and C++ library and programs (`native`) | about 15 min |
| `linux-package` | the packaged games (`package-smoke`) | about 20 min |
| `linux-all` | all five, in one build | 80–100 min |
| `windows-arm64` | the Windows on Arm C library archive and its smoke | 30–45 min |
| `first-run` | `scripts/first-run.py` on Linux, after publishing | about 15 min |

The rules ([scripts/release-0.2.md](scripts/release-0.2.md#release-time-ci)
has the commands and the details):

- Only the maintainer turns Actions on, for that one run, and off again
  right after it. The workflow never goes to `main`.
- Mark only what changed or what the release needs (`linux-all` once for
  the first candidate of a minor; afterwards only the parts a fix touched).
  Never `linux-all` to "just check".
- One run per release candidate and marker set. No re-runs to see if a run
  passes: reproduce a failure locally (the Linux steps in an `ubuntu:24.04`
  container), fix it through a pull request, and let the next candidate's
  run check it.

## One-time setup (a maintainer, in settings)

1. **crates.io ownership**: every maintainer who publishes is an owner of
   each of the ten crates `ponko`, `ponko_aseprite`, `ponko_cli`,
   `ponko_ecs`, `ponko_ffi`, `ponko_image_diff`, `ponko_lobby`,
   `ponko_macros`, `ponko_mcp`, `ponko_unity` (and every crate added to the
   release later; `cargo owner --add <user> <crate>`). A crate's first
   publish makes only the publishing user its owner: add the others right
   after it. Never store a registry token in the
   repository or a file.
2. **GitHub Pages for the web gallery**: repository Settings, Pages,
   Source: "Deploy from a branch", branch `gh-pages`, folder `/` (the build
   writes `.nojekyll`).

## Release candidates

From 0.2.0 on, STABILITY.md has every minor go out as `vX.Y.0-rc.1` at least a
week before `vX.Y.0`. A candidate is released like any release (steps 1 to 5
above, on `main`) with these differences:

- **The version is `X.Y.0-rc.N`** in the root `Cargo.toml`, every published
  `crates/*/Cargo.toml` and the sibling requirements between them. The tag is
  `vX.Y.0-rc.N` with N counting from 1; `release-check.py` refuses any other
  suffix (`-beta.1`, `-rc`, `-rc.0`) and a candidate of a patch release.
- **Nobody gets it by accident.** crates.io and Cargo treat a version with a
  suffix as a pre-release: `cargo add ponko` and `ponko = "0.2"` never select
  it. A tester asks for it by writing it out: `cargo add ponko@0.2.0-rc.1`.
  The GitHub release is created with `--prerelease` (and a pre-release is
  never the "latest" one), and its notes start with a line saying so.
- **The CHANGELOG section may still be `[Unreleased]`.** The notes come from
  the first of `## [X.Y.0-rc.N]`, `## [X.Y.0]` and `## [Unreleased]` that
  exists, so a candidate needs no rename: fold the fragments in
  (`python3 scripts/changelog.py --version X.Y.0-rc.1`), keep the heading as it
  is and tag. The fragments still have to be folded in first, as for a
  release. The release itself, `vX.Y.0`, needs its own `## [X.Y.0]` section.
- **The migration chapter is needed already.** A candidate whose notes list
  `### Breaking changes` needs `book/src/migrating-to-X.Y.md`, listed in the
  summary and holding every breaking entry, because the people who try a
  candidate need it most. The fold writes it, and the next candidate's fold
  adds its new entries to the same chapter (all candidates and the release of
  a minor share the name).
- **A regression is fixed on `main`** through a pull request, and the next
  candidate, `rc.2`, follows. The release is the last candidate, unchanged but
  for the version: set `version = "X.Y.0"` and the section name, run
  `release-check.py check vX.Y.0`, and tag it.

## Patch releases and backports

`main` may hold breaking changes for the next minor as soon as a minor is
released, so patch releases (0.1.1, 0.1.2, ...) come from a branch, `release-X.Y`
(`release-0.1`), which holds only changes that keep the API (STABILITY.md,
"Releases"). Maintainers protect `release-*` like `main`: pull requests only,
no force pushes. The same checks apply to them: run the
[Semver check](MAINTAINING.md#semver-checks) with
`--base release-X.Y` on a pull request into a release branch (it fails any
break there), and run the full list of checks before tagging a patch release.

**Create the branch** from the tag of the minor's first release, once, after
that release is out (a maintainer, once per minor; branch rules are set up for
`release-*` already):

```sh
git fetch origin --tags
git branch release-0.1 v0.1.0 && git push origin release-0.1
```

**Backport a fix.** A fix always lands on `main` first, through its normal
pull request, with its `changelog.d/` fragment (a `### Fixed` or `### Added`
entry; a `### Breaking changes` one can never be backported, and the Semver check
will refuse it). To carry it over:

1. Put the **`backport`** label on the merged pull request (or its issue)
   saying "this belongs on the patch branch". `gh pr list --state merged
   --label backport` lists what still waits.
2. Cherry-pick the merge commit (one commit, since pull requests are squash
   merged) onto a branch of the release branch. `-x` writes the original
   commit's hash into the message:

   ```sh
   git fetch origin
   git switch -c backport/<number>-<topic> origin/release-0.1
   git cherry-pick -x <merge commit on main>
   ```

   The fragment comes along. On a conflict, resolve it keeping the fix and
   nothing else, `git cherry-pick --continue`, and make sure the change still
   compiles against the older code; `cargo test` the affected crates.
3. Open the pull request **against `release-0.1`**, labelled `backport`, with
   "Backport of #<number>" in its description. Run the checks it needs and the
   Semver check (`--base release-0.1`) as for any pull request; merge it when
   they pass.
4. Take the `backport` label off the original pull request: it has been
   carried over.

**Release it** with steps 1 to 5 above, with these changes: work from a branch
of `release-0.1` and open the release pull request against it (fold the
fragments, set `version = "0.1.1"`, add the `## [0.1.1]` section, run
`release-check.py check v0.1.1`); tag the merge commit on `release-0.1`; and,
after the release, bring its notes to `main` through a pull request: copy the
`## [0.1.1]` section and its link line into `CHANGELOG.md` there, and delete
the fragments the release folded in from `main`'s `changelog.d/` (a
cherry-picked fragment has the same file name on both branches), so the next
minor does not list the fix a second time. Never merge `release-0.1` into
`main`.

`release-check.py check --on-branch` accepts `vX.Y.Z` on `main` as well as
on the `release-X.Y` of its own minor: `v0.1.1` on `release-0.1` passes, on
`release-0.2` or on an unmerged branch it does not.

`python3 scripts/release-check.py published` fails if a version of our crates
on crates.io has no matching tag, which is what a publish from the wrong
commit looks like; run it after every release. To fix it, tag the commit the
crate was published from. Its sha is in the `.crate` file:

```sh
curl -L https://static.crates.io/crates/ponko/ponko-X.Y.Z.crate | tar -xzOf - ponko-X.Y.Z/.cargo_vcs_info.json
git tag vX.Y.Z <that sha> && git push origin vX.Y.Z
```

`0.0.1` was published by hand from the last commit of #167 before it was
squash-merged; its tag `v0.0.1` is on the squash commit, whose tree differs
only in repository URLs and docs wording.
