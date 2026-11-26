### Added
- Errors name the fix for the first-hour mistakes: a missing asset lists the
  folders searched (each once) and the near-miss names next to it, an
  unreflected component names `world.reflect::<T>()`, a double mutable query
  says to split it, a left-out format names its Cargo feature, and freed or
  foreign GPU buffers, meshes, materials and texture arrays say what to make
  instead. `should_panic(expected)` tests in `tests/error_messages.rs` pin
  the wording.
- Guide chapter "When something fails: reading the message", and a
  "Panics and error messages" section in Contributing.
- `scripts/collect-panics.py` lists every `panic!`, `assert!`, `unwrap` and
  `expect` in the engine and ECS with its reviewed classification
  (`scripts/panic-audit.json`, internal invariants marked as such);
  `--check` fails on a new or changed site until it is reviewed, `--record`
  writes the decision.

### Changed
- Caller-reachable panics across the engine and ECS (queries, views,
  resources, reflection, jobs, UI, animation, networking, instances, GPU
  buffers, 2D skeletons) now say how to fix the call, not only what failed.

### Breaking changes
- `ponko_image_diff::compare`, `heatmap` and `compare_files` return `ponko_image_diff::Error`.
  The error keeps the failure's kind, the file and the underlying cause.
  Instead: match `err.kind()` (with a `_` arm), or turn it into a string with `String::from(err)`.
  Before:
  ```rust
  let diff: Result<Diff, String> = compare_files(a, b, tolerance, None);
  ```
  After:
  ```rust
  use ponko_image_diff::{compare_files, Diff};
  # let (a, b, tolerance) = todo!();
  let diff: Result<Diff, String> = compare_files(a, b, tolerance, None).map_err(String::from);
  ```
