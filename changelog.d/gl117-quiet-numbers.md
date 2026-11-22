### Changed
- Numbers taken on a busy machine were measured again on an idle one
  (issue #117; Apple M4 Max, 2026-11-22, revision `9235ea05`, three runs
  each, load average recorded): the ECS baseline
  (`crates/ponko_bench/baselines/ecs.json`) and the render baseline
  (`baselines/render.json`, `render.csv`), both re-recorded with their
  documented commands, and `ECS.md`, `UPSCALING.md` and `INSTANCE_CELLS.md`; in the
  guide, the chapters on skeletons, debug drawing, behavior trees, spatial
  search, determinism, hierarchy, attachments, replication, the lobby,
  rollback, code mods, video, streaming and loading, UI frames and group
  fades, the sky kit and water, global illumination's culling,
  reflections and thin objects, upscaling, transparency, baked lighting,
  area lights, stereo views, crowds and the battle showcase. Most numbers
  dropped to a third or less of what the busy machine gave.
- Numbers that moved the wrong way are written as such: a code mod's call
  costs 58 ns against 46 ns measured before (it came with tying a mod's
  game pointer to its `with_code` call), the lobby load test peaks at
  122 MiB against 104 MiB, and the thin-objects trace in `High` at 1440p
  takes 0.21 to 0.24 ms against its 0.2 ms target, and a crowd's CPU path
  (WebGL2's limits) prepares and records in 0.55 ms against 0.44 ms. Memory per entity in
  the headless example (96 bytes) is explained: `Sprite` gained a 20-byte
  `region` after the 76-byte figure was taken.
- The render baseline shows the GPU slower in all eight scenes than on
  2026-11-20 (frames up to 27% slower, `shadow-heavy` 5.02 to 6.35 ms):
  the `high` preset now turns on screen reflections' thin objects, whose
  pass costs 0.34 to 2.31 ms in these scenes against about 0.2 ms over the
  floor and rails it was measured on. Written up in `RENDER_SCENES.md` and
  the guide; not changed here.
