### Added
- `ponko::reflect::WorldHashParts`: `world_hash` part by part (each
  reflected component by name, each resource as `resource:Name`), with
  `total` (equal to `world_hash`) and `differing` to name the parts that
  differ between two worlds.
- A browser smoke test for replication (`web/smoke/replication.mjs`): a
  native dedicated server of the reference game (`examples/net_arena`) and
  a headless browser tab of its web build, which must join, hold its own
  player and see the replicated orbs move. `net_arena server` takes
  `--web <addr>` and prints where it listens; the web build joins the
  server its page names (`?server=ws://host:port`).
- The replication benchmark has a `1 in 20 move` scenario (5% of the
  entities change each tick, and only they are borrowed mutably) and takes
  a scenario filter (`-- 3 "1 in 20"`).

### Changed
- Replication's server reads and compares only the entities that changed.
  It tracks the replicated components' changes (once a client has joined)
  and each tick reads only entities whose replicated components changed or
  were removed, new or handed-over ones, and a rolling 1/64 of the rest (so
  a write the world can't see still goes out within about a second); per
  client it compares only those with their baselines. With 1,000 entities
  of which 5% change each tick, 16 clients: 0.58 ms per server tick
  instead of 0.75 ms (medians, alternating builds on a busy machine), with
  identical traffic; with every entity changing, no difference beyond the
  noise. Tracking costs 8 bytes per replicated component of each entity, on
  the server only.
- Replicated events travel in binary: MessagePack with fields by position,
  after a varint event type and tick (they were JSON after a fixed 10-byte
  header). Bytes per event: a hit (`NetId`, `f32`, `Vec2`, `bool`) 26
  instead of 76, a two-letter chat line 9 instead of 24, an enum variant 12
  instead of 22 to 29. Maps with keys that aren't strings (a
  `HashMap<(u8, u8), u8>`), which JSON refused, now travel. Servers and
  clients of different builds refuse each other on join, as before.
- **Breaking:** an event type that leaves fields out when serializing
  (`#[serde(skip_serializing_if = ...)]`) no longer arrives: with fields by
  position the receiver can't tell them apart and drops the event. Remove
  the attribute and send the field as it is (an `Option` travels as
  `None`).
- Lockstep's desync check hashes the world with the public `world_hash`
  (through `WorldHashParts`), reading components in place, instead of
  making the world's JSON save every check; a `Desync` still names the
  components that differ. The lockstep protocol version is 2: a peer of an
  older build is ignored (and times out) instead of seeing a desync at
  every check.
