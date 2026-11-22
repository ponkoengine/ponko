### Added
- The scoreboard against Bevy has a second table with both engines tuned
  (`python3 bench/compare/scoreboard.py --tuned`, and part of the default
  run): one shipping profile on both sides (fat LTO, one codegen unit,
  `panic = "abort"`), Bevy's pipelined rendering on with frames timed from
  one finished frame to the next, only the Bevy features each workload
  needs, rapier's `parallel` and `simd8` features on both sides, and on
  each side the faster of its documented ways to do a workload, measured.
  The build rows are made again for the small game with Bevy's
  `dynamic_linking` and both engines' unused features off. Results in
  `bench/compare/results/scoreboard-tuned.*` and the guide's performance
  chapter; `bench/compare/README.md` names the source of every choice.
- `ponko_bench` features `rapier-parallel` and `rapier-simd8`, and
  `scripts/build-times.sh` reads extra cargo arguments from
  `BUILD_TIMES_CARGO_ARGS`.

### Fixed
- The scoreboard's notes said Ponko draws its 2D rows with 4x MSAA. Its 2D
  renderer draws with one sample, like Bevy's 2D camera in the scoreboard.
