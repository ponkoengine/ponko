### Changed
- Benchmark baselines and the numbers quoted in the docs were measured again
  on an idle machine: `crates/ponko_bench/baselines/` (the ECS
  matrix, the render scenes, and the matched Ponko and Bevy runs), the ECS and
  render-scene write-ups, `README.md`, the performance chapter and the
  chapters on crowds, replication, rollback costs, saves and 3D audio. The
  render baseline now has eight scenes (`single-material` is new). Two scenes
  are slower than the figures the docs quoted before: `alpha-heavy` (9.5 to
  17.5 ms a frame: see-through models are now drawn one by one in sorted order)
  and `animated` (4.2 to 5.8 ms: skinning keeps last frame's joints for motion
  vectors); both also draw more shadow casters than before.
