## What changed

<!-- What this pull request does and why. Link the issue it fixes, if any. -->

## How it was checked

<!-- There is no CI: nothing runs on this pull request by itself. List the commands you ran (and on which OS), tests added, and for performance changes release-build numbers before and after. The full list of checks is scripts/checks.yml. -->

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass
- [ ] New public behavior has tests and doctests
- [ ] The guide (`book/src/`) is updated, if this changes how games are written
- [ ] The C API and C++ wrapper are updated, or not needed (say why)
- [ ] A `changelog.d/` fragment is added if users will notice the change (don't edit `CHANGELOG.md`)
