### Added
- `ponko::reflect::world_hash(&world)`: one `u64` for everything reflected in
  a world, for determinism checks (replays, lockstep peers, tests). It reads
  components in place without making JSON (0.5 ms for 100,000 entities with
  three components, against 1.1 s for hashing the JSON save), and doesn't
  depend on memory or reflection order. C `ponko_world_hash`, C++
  `world.hash()`.
- `World::query_ref::<Q>()`: read-only queries on a shared `&World`, several at
  once, with the same filters, `get`, `single`, `count` and `iter`. A
  component the world hasn't seen matches nothing instead of being registered.
- `ctx.input.start_recording()` and `ctx.input.stop_recording()` record live
  input into an `InputScenario` that replays it on the same ticks;
  `ctx.input.is_recording()` says whether one is running. C
  `ponko_ctx_start_input_recording` and `ponko_ctx_stop_input_recording`,
  C++ `ctx.start_input_recording()` and `ctx.stop_input_recording()`.
- `SkyShader` is a handle like the others: `ctx.is_live`, `ctx.free` (a sky
  still set to it shows the atmosphere) and `LiveCounts::sky_shaders`.
- `Graphics::try_update_texture` returns a wrong byte count or a render
  target's texture as `ErrorKind::Invalid`.

### Changed
- `Rng::unit_vec2` (and C++ `rng::unit_vec2`) no longer uses `sin` and `cos`:
  it is bit-identical on every platform now, and gives a different sequence
  of vectors for the same seed than before.
- `SpatialGrid::nearest` goes outward ring by ring from the query's cell, so
  a long radius costs about what a short one does (10,000 searches among
  100,000 entities: 1.65 ms against 402 ms in a radius of 16 cells). Of two
  entries at the same distance the one with the lower entity id is
  returned, and `accept` is asked about each entry at most once.
- A positional sound's `priority` (`d3::AudioSource::priority`) follows
  changes while it plays, instead of applying only when it starts.
- `ponko_ctx_update_texture` on a render target's texture returns
  `PONKO_INVALID_ARGUMENT` instead of a caught panic.
