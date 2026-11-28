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
  section (MAINTAINING.md, "Breaking changes: the migration note"). A
  `- **Breaking:**` bullet under another heading is refused: move it there.
- The release tells the entries by area (Rendering, Audio, C and C++, ...),
  guessed from the file name and text. `python3 scripts/changelog.py --areas`
  shows the guess; when it is wrong, make the first line of the fragment
  `<!-- area: Rendering -->` (the names are `AREAS` in the script).

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
   with the language (`rust`, `c`, `cpp`, `wgsl`, `toml`, ...). Real code
   the reader can search their project for, not a description. The `Before:`
   block is never compiled (it no longer builds). The `After:` block is:
   `python3 scripts/changelog.py --check --compile` (and the guide's
   `check-book-snippets.py`) builds it against the current API, so it must
   be code that compiles as shown:
   - `rust`: the body of `fn snippet(ctx: &mut Ctx, world: &mut World) ->
     Result<(), Box<dyn Error>>` with `use ponko::prelude::*;`, or whole
     items (`use`, `fn`, `struct`, ...). Give it what else it needs on hidden
     `# ` lines, as in the guide: `# use ponko::d3::Transform;`,
     `# let mesh: Handle<Mesh> = todo!();`. The reader sees the code without
     them.
   - `c`: the body of `void snippet(PonkoCtx *ctx, PonkoGame *game,
     PonkoSystemFn setup)` with `ponko.h` and `stdio.h`; `cpp`: of
     `void snippet(ponko::ctx& ctx, ponko::game& game)` with `ponko.hpp`,
     `<string>` and `<vector>`. Whole functions work too. Both are compiled
     with `-fsyntax-only`.
   - `wgsl`: whole functions; they are validated with the engine's mesh
     shader bindings in scope.
   - `toml`: parsed.
   Any other language is shown, not checked.

`--check` refuses an entry without any of these. The release folds the entry's
first lines and `Instead:` into the CHANGELOG's **Breaking changes** block
(the short form, and the GitHub release's notes) and the whole entry into
`book/src/migrating-to-X.Y.md`. `### Breaking` is accepted as the heading too.

Where it is cheap, don't break at once: keep the old item for one minor as
`#[deprecated(note = "use ...")]` and list that under `### Deprecated`. The
`Breaking changes` entry comes with the removal in the next minor
(STABILITY.md, "Releases"; MAINTAINING.md has the rule of thumb for "cheap").
Only a minor release (0.y.0) carries breaking entries; the release checks refuse
them in a patch release.

Validate the fragments with `python3 scripts/changelog.py --check` (and
`--check --compile` for a breaking entry) before opening the pull request.

## At release time

```sh
python3 scripts/changelog.py --version 0.2.0 --dry-run  # preview, change nothing
python3 scripts/changelog.py --version 0.2.0            # fold the fragments in, delete them
```

Bullets go to the end of the Unreleased subsection with the same heading
(created if missing, `Breaking changes` first), under the bold label of their
area, in file-name order within it. Then rename `[Unreleased]` to the
version as usual. `--version` is only needed when a fragment has a breaking
entry: then the script also writes `book/src/migrating-to-0.2.md` and adds it
to `book/src/SUMMARY.md` (entries of a later fold, for example after a release
candidate, are added to the same chapter). `release-check.py` refuses the
release of a minor whose CHANGELOG lists breaking changes without that chapter.

A minor also has a release page, `book/src/release-X.Y.md` ("What's new"): a
story written by hand at the top, which the fold copies to the top of the
CHANGELOG section, and below it a part `changelog.py --notes X.Y.Z`
generates from the release's CHANGELOG section and migration chapter (the
counts by area and the breaking changes, linked to their entries once the
fold has written the chapter; before it, it folds a scratch copy). The whole cut, command by command, is in
[scripts/release-0.2.md](../scripts/release-0.2.md).
