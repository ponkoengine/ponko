### Added
- A dependency audit with cargo-deny (`deny.toml`, `cargo deny check`, in
  the check list and every release's checks): licenses (the set the C
  library's third-party notices carry), RustSec advisories, duplicate crate
  versions (each accepted one listed with a reason), banned crates and
  allowed sources. CONTRIBUTING.md has "Dependency audit" for what to do when
  it fails.
