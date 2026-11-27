### Added
- `ponko_cli`, a new crate with the `ponko` command: `ponko new <path>
  [--2d | --3d | --headless]` makes a game project from a template, against
  the released engine or, with `--path <checkout>` / `--git <url>`, another
  one. Each project has the engine features for its kind, a fast debug
  profile, an `assets/` folder, headless tests, an `.mcp.json` for
  `ponko-mcp` and an `AGENTS.md` for coding agents. The templates
  (`crates/ponko_cli/templates`) also work with `cargo generate`. The check
  list's `templates` job builds and tests each one against the current
  engine. Getting started uses it.
