### Added
- Build times and download sizes in the performance chapter: clean,
  one-line-change and link times of the 2D template and the 3D tutorial
  (debug and release), where a clean build goes (`cargo build --timings`,
  `cargo llvm-lines`), and desktop and wasm sizes under each profile.
  `scripts/build-times.sh <project> [dev|release]` repeats the measurement.
- The game templates have a `.cargo/config.toml` with the faster linkers
  for Linux (mold, lld) and Windows (`rust-lld.exe`) to switch on, and the
  2D and 3D templates two size profiles: `dist` (fat LTO, one codegen unit,
  stripped; 14.8 MB instead of 24.5 MB for the 2D template on macOS) and
  `dist-web` (the same optimized for size: a 10.2 MB `.wasm` instead of
  15.1 MB). `ponko package --profile dist` and `--web --profile dist-web`
  use them.
- `web/build.sh --profile <name>` builds with another Cargo profile than
  `release`.

### Changed
- The templates' debug profile builds dependencies with line tables only
  (`debug = "line-tables-only"`) and `syn` unoptimized: a clean debug build
  of the 2D template takes 45.8 s instead of 60.0 s on an M4 Max (3D
  tutorial: 63 s instead of 84 s), its target folder is 2.4 GB instead of
  3.9 GB, and rebuilds after an edit stay at 0.46 s. Backtraces keep file
  and line in the engine; a debugger no longer shows the engine's local
  variables. Projects made earlier can copy the lines from the guide's
  "The fast debug profile".
