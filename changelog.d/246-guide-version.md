### Added
- Every guide page opens with a note that names the version the guide
  documents (the `version` in the root `Cargo.toml`) and how many unreleased
  changes `main` has on top of it. A guide built from a release tag says it
  documents exactly that release. The note is written by
  `scripts/guide-note.py` from `Cargo.toml` and `changelog.d/`, so a release
  bump updates it without an edit to any page.
- `scripts/release-check.py` also refuses a release while the dependency lines
  the guide's getting-started chapter, the tutorials and the project templates
  show (`ponko = "X.Y"`) name another minor than the tag.

### Changed
- The API reference chapter links the newest docs.rs release (`latest`)
  instead of a version written into each link, and says how to get the guide
  that matches a release (build it from the tag).
- The roadmap, the releases section of `STABILITY.md` and the guide's text now
  describe 0.2.0 as one release that carries the whole plan after 0.1.0 (2D,
  3D production and text, multiplayer and platforms, and the research that was
  waiting), with only the 1.0 freeze work outside it.

### Fixed
- The guide note linked the changelog with a path from another host; it uses
  the project's GitHub path.
- The text-shaping snippet in the UI chapter asked for `ponko = "0.3"`; it
  asks for 0.2.
- `ponko_aseprite` and `ponko_lobby` carry the root `LICENSE-MIT`,
  `LICENSE-APACHE` and `NOTICE`, like every published crate; the release check
  needs them and its self-test failed without.
