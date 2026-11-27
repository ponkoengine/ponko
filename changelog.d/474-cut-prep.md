<!-- area: Development and releases -->
### Added
- The changelog and the migration chapter are told by area (Rendering,
  Audio, C and C++, ...), breaking changes first, each migration entry under a
  short heading of its own. `changelog.py --areas` shows where each fragment
  goes; `<!-- area: ... -->` on a fragment's first line overrides the guess.
- Every breaking entry's `After:` code compiles: Rust, C, C++, WGSL and TOML
  blocks are built against the current API by `changelog.py --check --compile`
  and by the guide's snippet check.
- A release page for 0.2, "What's new in 0.2" in the guide: a story written by
  hand and a part `changelog.py --notes X.Y.Z` generates (the changes by area
  and every breaking change, linked to its migration entry). The fold copies
  the story to the top of the CHANGELOG section.
- `release-check.py bump X.Y.Z` sets every crate, the requirements between
  them and the guide's and templates' `ponko = "X.Y"` lines in one step.
- `scripts/first-run.py X.Y.Z`: a new user's first run from crates.io
  (install the CLI, make and test each template, build the guide's first
  program), for each OS after a release.
- `scripts/release-0.2.md`: the 0.2 cut as one list of commands, and
  `scripts/release-matrix.yml`, a release-time CI workflow made from the
  check list that is not enabled (it runs only from a branch a maintainer
  pushes at a release, for the Linux checks, the Windows on Arm C library
  and the Linux first run).

### Changed
- A GitHub release whose CHANGELOG section is too long for its notes keeps the
  story and the breaking changes and links to the rest of the section.
- `release-check.py check` also checks the version every guide chapter tells
  people to depend on, not only getting-started and the tutorials.
- A release candidate's templates and guide name the candidate
  (`ponko = "0.2.0-rc.1"`): Cargo never picks a candidate for `"0.2"`.
