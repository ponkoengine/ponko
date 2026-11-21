### Added
- `Replication::resend_stats(peer)` on the server: records sent again
  early (the acknowledgements skipped their packet) and on the timeout,
  packets called lost that were only late, the resend timeout now (in
  ticks), the timeouts in a row that doubled it, and the reordering
  tolerated. A new `#[non_exhaustive]` `ResendStats`, so `PeerStats`
  doesn't change. From C, `ponko_ctx_replication_resend_stats` and
  `PonkoReplicationResendStats`; from C++, `ctx::replication_resend_stats`.
- `NetMessages::loopback_link(players, LoopbackLink)`: the in-memory
  loopback over a link that loses a share of the datagrams and delays
  each by a fixed time plus jitter, so some overtake others; seeded, the
  same datagrams on every run.
- The replication benchmark has a scenario on a link that loses 5% of the
  datagrams each way and reorders them (`1 in 10 move, 5% lost,
  reordered`), and reports the records resent per second.
