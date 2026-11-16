### Changed
- Every public item of the UI, text, locale, audio, 2D and 3D physics,
  navigation, networking and server modules, and of the `ponko-mcp`,
  `ponko-unity` and `ponko` command-line crates, must now be documented:
  `#![deny(missing_docs)]` keeps it that way. The collision shapes, bodies,
  physics services, string tables, lockstep input calls and server controls
  gained runnable examples, and the functions that return an error or
  panic have `# Errors` and `# Panics` sections.
