### Added
- `ponko::net::Rollback` and `RollbackGame`: GGPO-style rollback netcode for
  small deterministic games. Every tick runs at once with predicted inputs
  for players whose inputs haven't arrived; a real input that differs loads
  the state saved before its tick and re-simulates. Input delay, a
  prediction window that stalls a peer too far behind, packets that repeat
  unacknowledged inputs over any transport (2–8 players in a mesh), and
  checksums of confirmed ticks for desync detection. The game saves and
  loads its own state. Rust-only (generic over the game's types).
- The guide's networking chapter has a "Rollback" section: how to use it,
  what state must be captured (and what can't be yet), measured costs of
  typed copies, `World::save`/`load` and rapier state at 100, 1,000 and
  10,000 entities, and the decision to ship it limited.
- `ponko_bench rollback/` measures them.
