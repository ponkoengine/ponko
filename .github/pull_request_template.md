## What changed

<!-- What this pull request does and why. Link the issue it fixes, if any. -->

## How it was checked

<!-- There is no CI: nothing runs on this pull request by itself. List the commands you ran (and on which OS). CONTRIBUTING.md, "Which checks your change needs", says which apply. Keep one of the two lists below and delete the other. -->

**Docs only** (a typo, a README, a guide page, no code):

- [ ] `mdbook build book` passes, if the guide changed (and `python3 scripts/check-book-snippets.py`, if a page with Rust code changed)

**Code:**

- [ ] `cargo fmt --all --check` (on Windows `python3 scripts/fmt-workspace.py --check`), `cargo clippy` and the tests of the changed crates pass
- [ ] New public behavior has tests
- [ ] The guide (`book/src/`) is updated, or not needed (maintainers can help)
- [ ] The C API and C++ wrapper are updated, or not needed (maintainers can help)
- [ ] A `changelog.d/` fragment is added if users will notice the change (don't edit `CHANGELOG.md`)
