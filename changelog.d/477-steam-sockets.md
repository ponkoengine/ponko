### Added

- `NetMessages::steam_sockets_host` (feature `steam`): the message protocol
  over Steam Networking Sockets. Lobby members open a connection to the
  host, relayed through Valve's network; the host refuses connections from
  outside its lobby and closes a member's connection when they leave it,
  and a dropped connection is dialed again. For hosts that run on their own
  (a headless process signed in to a Steam account) and games that want
  connections. `steam_host` (Steam Networking Messages) stays the default.
  Players join either with the same `steam_join`, which follows the host's
  choice; a Sockets host publishes its own protocol value, so older joiners
  refuse it with an error instead of timing out.
- C and C++: `ponko_net_messages_steam_sockets_host`,
  `ponko::net_messages::steam_sockets_host`.
- Guide: "Steam Networking Sockets: connections" in the networking chapter.
