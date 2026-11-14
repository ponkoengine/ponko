### Added
- Multiplayer in the browser: a web build joins a session over WebSocket
  with `Net::join_web` (lockstep) or `NetMessages::join_web` (messages), and
  a host takes browser players next to its UDP players with `accept_web`
  (`web_addr` says where). The host side is `tungstenite` on std sockets,
  polled from the game thread like UDP; plain `ws://`, so pages served over
  `https://` need a TLS proxy for `wss://`. A desktop game can join over
  WebSocket too.
- The lockstep example plays with up to 8 players, takes browser players
  with `--web <port>`, joins with `join-web <url>` or, as a web build, from
  its page's `?server=`, and plays headless with a bot (`--headless <ticks>`).
- `web/smoke/multiplayer.mjs`: two headless-browser tabs, a native host and a
  native UDP client play the lockstep example together and must reach the
  same world; part of CI's browser smoke job.
