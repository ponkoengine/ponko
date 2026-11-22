### Added

- The guide's new chapter, **Coming from Bevy** (`book/src/from-bevy.md`):
  Bevy 0.20.1's concepts mapped to Ponko's, fifteen common tasks side by
  side, what is different on purpose, what is missing or slower (with the
  scoreboard's numbers), a porting checklist, and Bevy's `move_sprite`
  example ported line by line. The Bevy snippets compile against the pinned
  Bevy (`bench/compare/tests/from_bevy.rs`, `cargo check --tests` in
  `bench/compare`).
- `examples/move_sprite`: the ported example, tested headlessly by
  `tests/move_sprite.rs`.

### Changed

- **Ponko vs Bevy vs Unity** (`book/src/comparison.md`) names the versions
  it compares (Bevy 0.20.1, Unity 6.3 LTS as the newest) and carries both
  scoreboard tables against Bevy, as a project starts and tuned. Its note
  on web builds no longer mentions a merge queue, which the project does not
  have.
