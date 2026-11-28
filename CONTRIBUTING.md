# Contributing to Ponko

<a id="start-with-what-you-know"></a>
Everyone is welcome, whether this is your first pull request or your
hundredth. Bug reports, questions, benchmarks on your hardware, typo fixes,
docs, examples, tests and code all count, and none of them is too small.
You do not need to know the whole engine, or C and C++, to fix one thing:
a Rust-only pull request is welcome, and maintainers do the guide pages, C
and C++ bindings and other platforms a change may also need.

There is no contributor license agreement (CLA) and no sign-off line to add.

<a id="quick-start"></a><a id="setting-up"></a><a id="making-a-change"></a>
## Your first change in four steps

You need stable Rust (`rustup update stable`) and Git. Per system:

- **Linux:** the ALSA and udev headers, for sound and gamepads
  (`sudo apt-get install libasound2-dev libudev-dev`).
- **macOS:** the Xcode command line tools (`xcode-select --install`).
- **Windows:** Visual Studio or its Build Tools with the "Desktop development
  with C++" workload, which Rust needs for linking.
- For the C and C++ side only: CMake and a C++17 compiler.

1. **Clone and build.** The first build compiles every dependency, so it is
   the slow one; later builds only rebuild what changed.
   ```sh
   git clone https://github.com/ponkoengine/ponko && cd ponko
   python3 scripts/run-checks.py --quick
   ```
2. **Pick an issue** and comment that you would like to take it
   ([Good first issues](#good-first-issues)). For a typo, skip this step.
3. **Branch from `main`** (`git switch -c fix/<topic>`), make one focused
   change, add a test for changed behavior, and run the checks it needs
   ([Which checks your change needs](#which-checks-your-change-needs)).
4. **Open a pull request to `main`** (`gh pr create --base main`, or the
   button on GitHub). Say what changed and which checks you ran. A draft, or
   a pull request with a failing check, is fine: say what you are unsure about.

<a id="finding-a-manageable-contribution"></a>
## Good first issues

Issues labelled
[`good first issue`](https://github.com/ponkoengine/ponko/labels/good%20first%20issue)
are kept for people new to the project: maintainers and their tools do not
take them. Each one says what to change, in which files, and how to test it.
[`help wanted`](https://github.com/ponkoengine/ponko/labels/help%20wanted)
marks larger work that is still self-contained; ask for a first step if the
scope is unclear. [ROADMAP.md](ROADMAP.md) explains the order of the bigger work.

### Claiming an issue

- **Ask on the issue.** An issue with no assignee and no open pull request
  is free. A maintainer assigns it to you; from then on nobody else starts
  it, and maintainers and their tools do not fold it into their own work.
- **Ask anything on the issue** while you work. An early draft pull request
  is welcome.
- **If you stop**, say so and you are unassigned; no reason needed. After two
  weeks with no comment, commit or draft, a maintainer asks on the issue;
  with no answer a week later, the issue is free again.
- **Issues assigned to a maintainer are taken.** Ask on one if you want to
  help, and a maintainer can split off a part for you.

<a id="checks-start-with-your-change"></a>
## Which checks your change needs

There is no CI: nothing runs on a pull request by itself. Run the checks for
your kind of change and list them in the pull request. If one needs tools or
hardware you do not have, say so; a maintainer runs it before merging.

| Change | Run |
|---|---|
| Docs only (a typo, a README, a guide page) | `mdbook build book` for a guide page; `python3 scripts/check-book-snippets.py` if it has Rust code |
| A Rust bug fix or feature | `cargo fmt --all --check`, `cargo clippy -p <crate> --all-targets -- -D warnings`, and the tests of the crate you changed (`cargo test -p <crate> --test <file>`), with a new test for the fix |
| A public API change | The Rust row, then `python3 scripts/api-snapshots.py update` and commit `api/`. A break also needs a migration note ([MAINTAINING.md](MAINTAINING.md#breaking-changes-the-migration-note)) |
| A cargo feature, or `Cargo.toml` dependencies | [Adding a cargo feature](book/src/contributing.md#adding-a-cargo-feature); for dependencies, `cargo deny` ([Dependency audit](MAINTAINING.md#dependency-audit)) |
| `unsafe` code, the C API, the C++ wrapper | [Unsafe code](book/src/contributing.md#unsafe-code): the inventory, Miri and the sanitizers; [Testing the C API](book/src/contributing.md#testing-the-c-api) |
| The renderer | The offscreen GPU tests, with the adapter they ran on and before/after images ([Reproductions and GPU evidence](book/src/contributing.md#reproductions-and-gpu-evidence)) |
| Performance | Release-build numbers before and after ([Performance changes](book/src/contributing.md#performance-changes)) |

`python3 scripts/run-checks.py --list` shows the whole list of checks and
which can run on your machine. The guide's
[contributor chapter](book/src/contributing.md) explains each check, and
what to do when it fails.

<a id="questions-bugs-and-ideas"></a>
## Getting help

Ask in [GitHub Discussions](https://github.com/ponkoengine/ponko/discussions),
so the answer stays findable for the next person:
[Q&A](https://github.com/ponkoengine/ponko/discussions/categories/q-a) for
questions, [Ideas](https://github.com/ponkoengine/ponko/discussions/categories/ideas)
for features you want to agree on before you build them, and
[Show and tell](https://github.com/ponkoengine/ponko/discussions/categories/show-and-tell)
for what you made. Bugs go in an
[issue](https://github.com/ponkoengine/ponko/issues/new), with the smallest
code that shows the problem, your OS and GPU. Security problems go to
[SECURITY.md](SECURITY.md), never to a public issue. Decisions are recorded on
GitHub, in the issue or pull request that raised them.

## How fast we reply

A new issue or a pull request from outside gets a first reply within three
days, a pull request gets a review within a week, and a new discussion gets
an answer within a week. One person maintains Ponko today, so a reply can
slip: if a week passes with no answer, ping on the issue or pull request.

## Changelog fragments

If users will notice your change, add a file `changelog.d/<pr-or-topic>.md`
with a `### Added`, `### Changed` or `### Fixed` section of `- ` bullets.
Do not edit `CHANGELOG.md`: the release folds the fragments in.
[changelog.d/README.md](changelog.d/README.md) has the format, and what a
breaking change needs.

## Using coding agents

You may use coding agents. You answer for what you submit, as if you had
typed it yourself:

- read and understand every line, and keep the change to one focused topic;
- check that the APIs and dependencies it uses exist and do what it says;
- run the checks yourself, and report what failed or was skipped;
- treat what the agent says it did as a claim, not as proof;
- keep private code, assets and secrets out of the pull request and out of
  the services the agent sends them to.

Reviewers review the change, not the tool.
[AGENTS.md](AGENTS.md) has the rules agents in this repository follow.

## Where things moved

This file used to hold the whole reference. The parts moved here:

| Old section | Now in |
|---|---|
| <a id="repository-layout"></a>Repository layout | [Contributor chapter](book/src/contributing.md#repository-layout) |
| <a id="local-checks"></a><a id="checks-that-need-a-person"></a>Local checks, checks that need a person | [Running local checks](book/src/contributing.md#running-local-checks) |
| <a id="adding-or-changing-a-cargo-feature"></a>Adding or changing a cargo feature | [Adding a cargo feature](book/src/contributing.md#adding-a-cargo-feature) |
| <a id="unsafe-code-and-sanitizers"></a>Unsafe code and sanitizers | [Unsafe code](book/src/contributing.md#unsafe-code) |
| <a id="performance-changes"></a>Performance changes | [Performance changes](book/src/contributing.md#performance-changes) |
| <a id="architecture-proposals"></a>Architecture proposals | [Architecture proposals](book/src/contributing.md#architecture-proposals) |
| <a id="reproductions-and-gpu-evidence"></a>Reproductions and GPU evidence | [Reproductions and GPU evidence](book/src/contributing.md#reproductions-and-gpu-evidence) |
| <a id="fuzzing"></a>Fuzzing | [Fuzzing](book/src/contributing.md#fuzzing) |
| <a id="breaking-changes-the-migration-note"></a>Breaking changes: the migration note | [MAINTAINING.md](MAINTAINING.md#breaking-changes-the-migration-note) |
| <a id="api-snapshots"></a>API snapshots | [MAINTAINING.md](MAINTAINING.md#api-snapshots) |
| <a id="semver-checks"></a>Semver checks | [MAINTAINING.md](MAINTAINING.md#semver-checks) |
| <a id="dependency-audit"></a>Dependency audit | [MAINTAINING.md](MAINTAINING.md#dependency-audit) |
| <a id="nightly-runs-on-maintainer-machines"></a>Nightly runs on maintainer machines | [MAINTAINING.md](MAINTAINING.md#nightly-runs-on-maintainer-machines) |
| <a id="releasing"></a>Releasing | [RELEASING.md](RELEASING.md) |
| <a id="the-c-library-archives"></a>The C library archives | [RELEASING.md](RELEASING.md#the-c-library-archives) |
| <a id="release-time-ci"></a>Release-time CI | [RELEASING.md](RELEASING.md#release-time-ci) |
| <a id="one-time-setup-a-maintainer-in-settings"></a>One-time setup | [RELEASING.md](RELEASING.md#one-time-setup-a-maintainer-in-settings) |
| <a id="release-candidates"></a>Release candidates | [RELEASING.md](RELEASING.md#release-candidates) |
| <a id="patch-releases-and-backports"></a>Patch releases and backports | [RELEASING.md](RELEASING.md#patch-releases-and-backports) |

<a id="license"></a>
## Conduct, governance and license

Everyone in the project follows the [code of conduct](CODE_OF_CONDUCT.md).
How decisions are made, who can merge and how someone becomes a maintainer
is in [GOVERNANCE.md](GOVERNANCE.md).

By contributing, you agree that your contributions are dual licensed under
[MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE), like the rest of the
project, without any additional terms or conditions.
