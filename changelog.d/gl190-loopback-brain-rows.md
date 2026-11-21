### Added
- Lossy loopback links from C and C++: `PonkoLoopbackLink` (a versioned
  descriptor: loss, delay, jitter, seed), `ponko_loopback_link_defaults`
  and `ponko_net_messages_loopback_link` connect endpoints in memory over a
  link that loses, delays and reorders datagrams, the same ones on every
  run (Rust `NetMessages::loopback_link`). `ponko_net_messages_poll_elapsed`
  polls on the caller's clock (Rust `NetMessages::poll_elapsed`), so a
  native test steps its endpoints frame by frame. C++:
  `ponko::loopback_link`, `loopback_link_defaults()`,
  `net_messages::loopback(players, link)` and `net_messages::poll_elapsed`.
- `ponko_ai_brain_wake` and `ponko_ai_brain_signal` on a query's brain
  row, inside `ponko_query` and from `ponko_query_par` threads (Rust
  `Brain::wake`, `Brain::signal`); C++ `ai::brain_row::wake` and `signal`.
- C++ typed loops take the brain and blackboard columns directly:
  `world_view::each` and `each_par` hand `ai::brain_row` and
  `ai::blackboard_row` parameters beside the components, with filters,
  instead of the separate `ai::each` (which stays). `world_view::count`
  counts them too.
- The guide shows a debug map camera from C and C++: a target camera with
  its own debug-draw flag while the window's camera leaves the drawing out.
