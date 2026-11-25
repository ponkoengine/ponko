### Added
- Game sessions through the lobby server's relay: `Net::host_relay` and
  `Net::join_relay` run a lockstep session, `NetMessages::host_relay` and
  `NetMessages::join_relay` a message endpoint (and replication on top),
  between the members of one lobby room, with the same API as over UDP.
  The session owns the `LobbyClient` and polls it itself, also while a
  lockstep tick waits; `relay_lobby()` gives it back for its state and
  events. Tested through the real server with scripted latency, reordering
  and loss, and over UDP on loopback.
- A TURN client on the desktop (feature `webrtc`): `RtcConfig::turn` now
  works outside browsers too, over UDP with long-term credentials
  (allocations, refreshes, permissions, Send and Data indications), so two
  desktop peers behind strict NATs connect through a TURN server such as
  coturn. `RtcConfig::relay_only` offers relayed addresses only (a
  browser's `iceTransportPolicy: "relay"`), and `RtcConfig::turn_servers`
  lists the TURN servers.
- `LobbyLimits::trusted_proxies` and `ponko-lobby`'s `LOBBY_TRUSTED_PROXIES`:
  browser connections from a named reverse proxy count as the client
  address from its `X-Forwarded-For` header, so the limits per address
  apply per player behind a TLS proxy; the header of anyone else is
  ignored. `TrustedProxy` reads addresses and ranges. In the engine,
  `NetMessages::web_peer` gives a browser connection's `WebPeer` (its TCP
  address and header) and `WebPeer::client_ip` resolves the client
  address through the proxies a server trusts.
- The guide's "Putting a game online" chapter: one setup end to end, with
  `ponko-lobby`, Caddy for `wss://` and coturn for STUN and TURN.

### Changed
- `ponko-lobby` logs `trusted_proxies` in its `lobby_listening` line.
- The `webrtc` feature brings `sha1` (already in the tree) and `md-5` for
  TURN's message integrity.
