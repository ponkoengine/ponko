### Added
- A reference lobby, matchmaking and relay server, `ponko-lobby` (the new
  `ponko_lobby` crate): rooms with six-character codes, join and leave,
  matchmaking queues that fill rooms by size, host migration, and a relay
  that forwards opaque bytes between members of a room for players who
  can't connect directly. Limits per address (requests and relay bytes per
  second, connections), size caps (requests, relay payloads, rooms, room
  text) and idle timeouts for connections and rooms. It runs in
  `Game::run_server` (JSON logs, clean stop on `SIGTERM`, `ponko package`
  smoke run) and has a container recipe (`crates/ponko_lobby/Dockerfile`).
  The library, `LobbyServer`, takes a game's own `Matchmaker` and
  `HostMigration` rules. Guide: "Lobbies, matchmaking and relays".
- `ponko::net::lobby` (feature `lobby`, off by default, no dependencies):
  `LobbyClient` with its events and the documented wire protocol, over UDP
  or WebSocket. C: `ponko_lobby_*`; C++: `ponko::lobby`.
- `NetMessages::peer_addr`: the address a connected peer's packets come
  from.
- A load test of the lobby (`cargo run --release -p ponko_lobby --example
  load_test`): 1000 rooms and 3300 clients in about 5 s, headless.
