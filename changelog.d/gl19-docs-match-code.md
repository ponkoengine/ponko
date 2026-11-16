### Fixed
- The guide's `use ponko::...;` lines are checked against the API snapshot:
  `scripts/check-book-snippets.py` fails when a Rust block (or an included
  example region) imports a path that does not exist. The tutorials' prose now
  names where each import lives (`ponko::prelude`, `ponko::d2`/`d3`,
  `ponko::tilemap`, `ponko::render`) and says `game() -> ConfiguredGame`.
- `STABILITY.md` says where the math types live (`ponko::prelude`, not the
  crate root), writes `ctx.random()`, and lists the `UiLayout` methods that
  expose Taffy types (`custom`, `style`, `style_mut`, `into_style`) as unstable.
- `book/src/api.md` and the getting-started page say which engine version the
  guide matches (`main`, the unreleased 0.2) and how to build the matching
  reference locally with the right features.
- Links from the docs to files of the repository are relative, and the built
  guide rewrites them to the project's host (`scripts/project-url.py`, which
  reads the one line in `PROJECT_URL`). Links to old issue and pull request
  numbers are plain text. `project-url.py set <url>` moves every literal copy
  of the project URL, and `project-url.py check` fails on links to the old
  host and on relative links that point nowhere.
