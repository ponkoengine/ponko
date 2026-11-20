# ponko_lobby

A reference lobby, matchmaking and relay server for games made with
[Ponko](https://ponko.dev/): rooms with six-character codes, join and leave,
matchmaking queues that fill rooms by size, host migration, and a relay for
players who can't connect to each other directly, with rate and size limits.

Clients use the engine's `ponko::net::lobby::LobbyClient` (feature `lobby`),
over UDP or, from a browser, WebSocket.

```sh
cargo run -p ponko_lobby                       # the server, on UDP 0.0.0.0:7700
LOBBY_WEB=0.0.0.0:7701 cargo run -p ponko_lobby  # browsers too, on ws://
cargo run --release -p ponko_lobby --example load_test   # 1000 rooms, headless
docker build -f crates/ponko_lobby/Dockerfile -t ponko-lobby .   # from the repository root
```

It is a reference, not a service: no accounts, nothing kept across restarts,
no anti-cheat, and the relay forwards bytes it never reads. The guide's
"Lobbies, matchmaking and relays" chapter
(<https://ponko.dev/guide/lobby.html>) says what it limits and how to
run it; the library (`LobbyServer`) takes a game's own matchmaking and host
migration rules.
