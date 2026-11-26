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

The checks that need other operating systems: run `run-checks.py` on the
Linux and Windows machines, or, once per minor, on hosted runners with the
release-time matrix (`scripts/release-matrix.yml`, not enabled; see
"Release-time matrix" below). For `rc.1` also record the coverage numbers and
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
python scripts/c-library.py build --target x86_64-pc-windows-msvc         # and aarch64-pc-windows-msvc, on Windows
python3 scripts/c-library.py smoke target/c-library/<archive>             # each one
```

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
python3 scripts/first-run.py V                 # on macOS, Linux and Windows: what a new user does, from crates.io
```

Or push the first-run branch of the release-time matrix (below). Then
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

## Release-time matrix

`scripts/release-matrix.yml` is a GitHub Actions workflow made from the
multi-OS jobs of `scripts/checks.yml` (`python3 scripts/release-matrix.py
write`; the checks job fails if it is out of date). It is not enabled:
GitHub never reads `scripts/`. It may run once per minor, at the cut. A
maintainer turns Actions on in the repository settings, then:

```sh
git switch --detach vV          # or the release branch, before tagging
mkdir -p .github/workflows && cp scripts/release-matrix.yml .github/workflows/
git switch -c release-matrix/checks-V
git add .github/workflows/release-matrix.yml && git commit -m "Release matrix for V"
git push origin release-matrix/checks-V
```

After `cargo publish`, the same with the branch
`release-matrix/first-run-V` runs `scripts/first-run.py V` on the three
operating systems. When both are done: delete the branches
(`git push origin --delete release-matrix/checks-V`), turn Actions off again,
and write in the release pull request which jobs ran where. The workflow file
never goes to `main`.
