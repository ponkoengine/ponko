### Added

- `scripts/check-feature-coverage.py` fails when a cargo feature of a workspace
  crate has no clippy or test line in `.github/workflows/ci.yml` (or no doc
  line, when docs.rs builds the crate with it), or when a browser runner in
  `web/smoke` or `web/gallery` is not run there. What may lack a line is
  listed with a reason in `scripts/feature-checks.txt`, and the check fails
  for an entry that is not needed any more. `--list` shows each feature's
  checks, `--selftest` checks the parsing.
- The check list covers the `editor`, `video`, `lobby` and `webrtc` features
  (clippy lines, test steps, and docs with every public feature), and also
  `hotpatch`, `profiling`, the C library's optional features, the ECS's `serde`
  and the benchmark crate's drivers.
- The C and C++ test programs run against a library built with `webrtc` in
  the native job (`-DPONKO_WEBRTC=ON` in `examples/cpp`); without the feature,
  `ponko_ffi`'s `lobby` test holds that the calls fail and name it.
- The component hooks and required components tests run under Miri's Tree
  Borrows model, with the parallel path's.
- The threaded web build (`web/smoke/threads.mjs`) and replication in the
  browser (`web/smoke/replication.mjs`) run in the web smoke job.
- "Adding or changing a cargo feature" in `CONTRIBUTING.md`.

### Fixed

- A web build with threads (`web/build.sh --threads`) compiled again: the
  built-in controller backend held the browser's gamepad objects, which can't
  be sent between threads once the memory is shared, and `GamepadBackend` is
  `Send`. Such a build now starts no built-in controllers (a game's own
  `GamepadBackend` works); the ordinary web build is unchanged.
