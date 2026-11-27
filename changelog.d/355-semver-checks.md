### Added
- `python3 scripts/semver-checks.py run` checks the public API of every
  published crate against its last crates.io release (`cargo-semver-checks`,
  the `semver` job of the check list). A break fails a change to a
  `release-*` branch, and on `main` passes only for a pull request with the
  `breaking` label (`--breaking`) and a `### Breaking changes` fragment.
  CONTRIBUTING.md explains how to read a failure and what the tool does not
  catch.
