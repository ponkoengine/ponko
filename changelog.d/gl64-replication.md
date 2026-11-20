### Added
- Declared replication (`ponko::net::Replication`, `Game::replication`,
  `ctx.net.replication`): declare which components replicate, mark
  entities with `Replicated`, and the engine keeps every client's world in
  step with the server's. The server is the authority. Every tick it sends
  each client what changed, as deltas against the state that client
  acknowledged. Clients get `Replica` copies, spawned, updated and
  despawned for them, named by a `NetId` that is the same on every side.
  Off by default, and then free: nothing runs and nothing travels.
- `#[derive(Replicate)]` with per-field quantization
  (`#[replicate(min, max, step)]`, `angle`, `bits`, `skip`; `Quantize`,
  `Fields`, `ReplicateField`, `replicated_bits`): a position on a
  200-unit arena to the centimetre takes 15 bits instead of 32.
  `d2::Transform` and `d3::Transform` replicate and interpolate as they are.
- Client prediction: `Replication::input` and
  `Replication::predicted::<State, Input>(step)`. A client moves the
  entities it owns at once. The server applies each input once and in
  order. A disagreement is corrected by taking the server's state and
  replaying the newer inputs. Inputs and states are quantized on both
  sides after every step, so a pure step predicts exactly.
- `Replication::interpolated` shows a component between the values
  received, 0.1 s in the past (`interpolated_with` for your own
  `SnapshotConfig`).
- Reliable events both ways: `Replication::event`, then `to_server`,
  `to_client` and `to_clients`. They are read as `NetEvent<T>` (sender,
  tick, event) from `ctx.events`.
- A bandwidth budget per client (`Replication::budget`, 64 KB/s by
  default, `DEFAULT_BUDGET`). When changes don't fit, entities wait in
  priority order (`Replicated::with_priority`), and a client's own
  entities are always sent.
- Interest management: `Replication::interest` with distance rings and
  rates, `Replication::position`, `Replication::set_viewer`, and
  `Replicated::global`. An entity that leaves a client's interest is
  despawned there.
- Ownership (`Replicated::owned_by`, `set_owner`, `Replica::is_mine`), with
  authority epochs so a handoff drops the old owner's predictions.
  `Replicated::set_global` sets the interest flag on an existing marker.
- A schema check on join: a client whose declarations differ is refused,
  with a reason (`Replication::error`).
- Statistics: `Replication::peer_stats` (bytes, packets, records, deltas,
  deferred entities, losses per client) and `Replication::client_stats`
  (received, applied, stale packets, confirmed and corrected predictions).
- `NetMessages::game_clock`: an endpoint the game polls counts the game's
  time instead of the wall clock, so headless tests that run faster than
  real time still see resends and timeouts.
- Orb Arena (`examples/net_arena`), a multiplayer reference game built
  only on replication. It has a listen server, a dedicated server
  (`net_arena server`), clients (`net_arena join <addr>`) and browser
  clients over WebSocket. `tests/net_arena.rs` plays it headless with a
  dedicated server and two bots over a lossy network.
- The replication benchmark: 1,000 entities to 16 clients
  (`cargo run --release -p ponko_bench --bin replication`), with bytes per
  client and server CPU per tick.
- C and C++: `ponko_ctx_replication` declares components field by field.
  Further calls mark entities, read the role, net ids, replicas, clients
  and statistics, and set viewers: `ponko_ctx_replicate`,
  `ponko_ctx_replication_status`, `ponko_ctx_net_id`,
  `ponko_ctx_net_entity`, `ponko_ctx_replica`,
  `ponko_ctx_replication_clients`, `ponko_ctx_replication_set_viewer`,
  `ponko_ctx_replication_peer_stats`,
  `ponko_ctx_replication_client_stats`. `ponko_game_messages` gives a
  game its endpoint, and `ponko_net_messages_game_clock` switches it to
  game time. The C++ wrapper has `ctx::replicated<T>`,
  `ctx::replication`, `ctx::replicate` and friends. Inputs, prediction,
  events and interpolation are Rust-only for now.
- Guide: a new "Multiplayer: pick a model" entry page and a "Replication"
  chapter (declare, mark, host and join, prediction, events, interest and
  the budget, testing, the arena, costs, checks for a person). Lockstep,
  rollback and application messages are now one chapter under it, and so
  are dedicated servers.

