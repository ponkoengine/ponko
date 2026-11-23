# Changelog fragments

Pull requests don't edit `CHANGELOG.md`: every merge would make the other
open pull requests conflict in it. Add a fragment here instead, one file per
change, and the release folds them in.

- Name it after the pull request or topic: `changelog.d/<pr-or-topic>.md`,
  for example `changelog.d/1234.md` or `changelog.d/action-ids.md`.
- Write one or more `### Heading` sections, each starting with `- ` bullets.
  Use `Breaking changes` (entries with a replacement and code, see below) and the
  [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) headings
  (`Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`); a
  larger change may use its own heading. Continuation lines are indented
  by two spaces, as in `CHANGELOG.md`.
- A change that breaks a published crate's API needs a `### Breaking changes`
  section, and the pull request the `breaking` label: the Semver check
  fails a break it finds without both, and fails the label without the
  section (CONTRIBUTING.md, "Breaking changes: the migration note").

```md
### Added
- `World::example()` does the thing, with the details users need.

### Fixed
- Saving a world no longer drops empty archetypes.
```

## Breaking changes

A `### Breaking changes` section holds **entries**, not plain bullets, because
they become the migration chapter of the release. Each entry has
four parts, in this order, with continuation lines indented by two spaces:

````md
### Breaking changes
- `World::despawn_all` is `World::clear` now.
  Every entity goes at once, as before; only the name changed.
  Instead: call `World::clear`; it also resets the change ticks.
  Before:
  ```rust
  world.despawn_all();
  ```
  After:
  ```rust
  world.clear();
  ```
````

1. **What changed**: the first line, one sentence naming the item. It is the
   entry's heading in the migration chapter, so keep it short and specific.
   More lines may follow it, before `Instead:`.
2. **`Instead:`**: what to write instead, in a sentence (it may continue on
   the following lines). Say "removed, nothing replaces it, because ..." if
   that is the truth.
3. **`Before:`** then 4. **`After:`**: a fenced code block each, in this order,
   with the language (`rust`, `c`, `cpp`, `toml`, ...). Real code the reader
   can search their project for, not a description. The blocks are never
   compiled (the "before" code no longer builds), so keep them small and
   check them by eye or against the new API docs.

`--check` refuses an entry without any of these. The release folds the entry's
first lines and `Instead:` into the CHANGELOG's **Breaking changes** block
(the short form, and the GitHub release's notes) and the whole entry into
`book/src/migrating-to-X.Y.md`. `### Breaking` is accepted as the heading too.

Where it is cheap, don't break at once: keep the old item for one minor as
`#[deprecated(note = "use ...")]` and list that under `### Deprecated`. The
`Breaking changes` entry comes with the removal in the next minor
(STABILITY.md, "Releases"; CONTRIBUTING.md has the rule of thumb for "cheap").
Only a minor release (0.y.0) carries breaking entries; the release checks refuse
them in a patch release.

Validate the fragments with `python3 scripts/changelog.py --check` before
opening the pull request.

## At release time

```sh
python3 scripts/changelog.py --version 0.2.0 --dry-run  # preview, change nothing
python3 scripts/changelog.py --version 0.2.0            # fold the fragments in, delete them
```

Bullets go to the end of the Unreleased subsection with the same heading
(created if missing, `Breaking changes` first), in file-name order. Then rename `[Unreleased]` to the
version as usual. `--version` is only needed when a fragment has a breaking
entry: then the script also writes `book/src/migrating-to-0.2.md` and adds it
to `book/src/SUMMARY.md` (entries of a later fold, for example after a release
candidate, are added to the same chapter). `release-check.py` refuses the
release of a minor whose CHANGELOG lists breaking changes without that chapter.
