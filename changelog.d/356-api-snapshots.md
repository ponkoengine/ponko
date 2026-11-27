### Added
- `api/` holds a generated listing of the public API of every published crate
  (default, headless, `3d` and docs.rs feature sets), and
  `python3 scripts/api-snapshots.py check` (the `api-snapshots` job of the
  check list) fails with a diff when it is out of date, so every added,
  renamed or removed public item shows up in review.
  `python3 scripts/api-snapshots.py update` regenerates it (CONTRIBUTING.md,
  "API snapshots").
