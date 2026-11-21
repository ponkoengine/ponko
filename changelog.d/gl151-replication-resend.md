### Added
- Replicated events as raw bytes, for C and C++ games or any game that
  encodes its own: `Replication::raw_event(name)` declares one,
  `to_server_raw`, `to_client_raw` and `to_clients_raw` send its bytes
  (returning an `Error` for an undeclared name or past
  `MAX_RAW_EVENT_BYTES`), and they arrive unchanged as
  `NetEvent<RawEvent>` in `ctx.events`, reliable and in one order with
  typed events. The C and C++ calls that wrap them come next.

### Changed
- Replication sends each change, spawn and despawn once, and again only
  when the client hasn't acknowledged it within a resend timeout (its
  measured round trip plus a margin, between two ticks and a second) or
  when it changed again meanwhile, instead of in every packet until
  acknowledged. In the replication benchmark, with 1 in 20 of 1,000
  entities moving each tick, a client's traffic fell from the 64 KB/s
  budget (63,893 B/s) to 31,266 B/s.
