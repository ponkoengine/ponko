### Added
- Fuzz targets for replication, the lobby protocol (server and client),
  the control endpoint's requests, Aseprite files and cooked sheets, LDtk
  and Tiled levels, and cooked textures (`fuzz/README.md`).
- A load test with hostile clients for the lobby server
  (`crates/ponko_lobby/tests/hostile.rs`): its memory stays flat while they
  flood it, and honest rooms keep working.
- The guide's "Security: what reads untrusted bytes": every entry point,
  who can send it bytes, what a bad actor wants and what stops them.
  `SECURITY.md` says what is in scope.
