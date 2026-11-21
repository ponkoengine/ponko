# Putting a game online

This page is one complete setup, from an empty server to players in a
match: the [lobby server](lobby.md) for rooms and its relay, a TLS proxy
so browsers can reach it over `wss://`, and a TURN server so
[WebRTC](webrtc.md) peers connect even from behind strict NATs. Follow it
as it is, then change what your game needs.

The names and addresses are placeholders: `lobby.example.org` for your
domain, `203.0.113.10` for the server's public IPv4 address (from the
documentation range), `CHANGE-ME` for a password. Use your own.

## What runs where

One Linux machine with a public address runs three programs:

| Program | Listens on | For |
|---|---|---|
| `ponko-lobby` | UDP 7700 (public), TCP 7701 (local only) | Rooms, matchmaking, the relay; desktop players over UDP, browsers through the proxy |
| Caddy | TCP 443 and 80 (public) | `wss://lobby.example.org` for browsers, with a certificate it gets by itself; forwards to 7701 |
| coturn | UDP 3478, UDP 49152–49999 (public) | STUN (public addresses) and TURN (relayed addresses) for WebRTC peers |

The game's own traffic goes one of three ways, and the page sets up all
three:

1. **Peer to peer over WebRTC**: directly between the players, through
   most home routers with STUN.
2. **Through TURN**: for WebRTC peers that can't reach each other (both
   behind strict NATs). coturn relays their encrypted packets.
3. **Through the lobby's relay** ([relay sessions](lobby.md#game-sessions-through-the-relay)):
   for lockstep games, which don't run over WebRTC, and for anything
   when you'd rather not run TURN.

Open these ports in the server's firewall, and nothing else:

```sh
ufw allow 7700/udp          # ponko-lobby, desktop players
ufw allow 80/tcp            # Caddy: certificates
ufw allow 443/tcp           # Caddy: wss:// for browsers
ufw allow 3478/udp          # coturn: STUN and TURN
ufw allow 49152:49999/udp   # coturn: relayed addresses
```

## 1. The lobby

Build the container image from the root of the engine's repository (see
[In a container](lobby.md#in-a-container)) and run it on the host's
network, with the WebSocket port on the loopback interface only:

```sh
docker build -f crates/ponko_lobby/Dockerfile -t ponko-lobby .
docker run -d --name lobby --restart on-failure --network host \
  -e LOBBY_UDP=0.0.0.0:7700 \
  -e LOBBY_WEB=127.0.0.1:7701 \
  -e LOBBY_TRUSTED_PROXIES=127.0.0.1 \
  ponko-lobby
docker logs lobby | head -1
```

The first log line is `lobby_listening` with the addresses and
`"trusted_proxies":["127.0.0.1/32"]`.

`LOBBY_TRUSTED_PROXIES` matters because every browser now arrives through
Caddy, from `127.0.0.1`. Without it the server can't tell browsers apart
by address, and gives every browser connection limits of its own. With
it, the server reads the client's address from the `X-Forwarded-For`
header Caddy adds, and only from Caddy: a header from anyone else is
ignored. Its limits per address (connections, requests, relayed bytes)
then count per player's IP address, as for desktop players
([Limits](lobby.md#limits)). Name only proxies you run: a trusted proxy
can claim any client address.

## 2. Caddy, for `wss://`

A page served over `https://` may only open `wss://` connections. Caddy
terminates TLS, gets and renews the certificate, and forwards the
WebSocket to the lobby. `/etc/caddy/Caddyfile`:

```text
lobby.example.org {
    reverse_proxy 127.0.0.1:7701
}
```

```sh
docker run -d --name caddy --restart on-failure --network host \
  -v /etc/caddy/Caddyfile:/etc/caddy/Caddyfile -v caddy_data:/data \
  caddy:2
```

Point the domain's DNS `A` record at `203.0.113.10` first: Caddy needs it
to get the certificate. Caddy forwards WebSocket upgrades and sets
`X-Forwarded-For` to the address it sees without further configuration.
With nginx instead, the location needs
`proxy_set_header Upgrade $http_upgrade;`,
`proxy_set_header Connection "upgrade";` and
`proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;`.

## 3. coturn, for STUN and TURN

coturn answers STUN requests and relays for WebRTC peers that can't reach
each other. It also answers STUN, so the lobby's own STUN responder
(`LOBBY_STUN`) stays off here: both want UDP 3478. `/etc/coturn/turnserver.conf`:

```text
listening-port=3478
external-ip=203.0.113.10
realm=example.org
lt-cred-mech
user=game:CHANGE-ME
min-port=49152
max-port=49999
fingerprint
no-tcp-relay
no-cli
# Never relay into the server's own networks.
denied-peer-ip=0.0.0.0-0.255.255.255
denied-peer-ip=10.0.0.0-10.255.255.255
denied-peer-ip=127.0.0.0-127.255.255.255
denied-peer-ip=169.254.0.0-169.254.255.255
denied-peer-ip=172.16.0.0-172.31.255.255
denied-peer-ip=192.168.0.0-192.168.255.255
```

```sh
docker run -d --name turn --restart on-failure --network host \
  -v /etc/coturn/turnserver.conf:/etc/coturn/turnserver.conf \
  coturn/coturn
```

Check it from another machine with coturn's own test client:
`turnutils_uclient -u game -w CHANGE-ME -y lobby.example.org` allocates a
relayed address and sends packets through it.

The user name and password ship inside the game, so anyone who has the
game can use this TURN server. That is common for small games; the
`denied-peer-ip` lines keep it from reaching your private networks, and
the port range caps how many allocations it holds. For more, give every
player short-lived credentials instead: coturn's `use-auth-secret` with a
`static-auth-secret`, and a server of yours that hands out a user name
(an expiry time) and a password (an HMAC of it). The engine takes any
user name and password, so nothing changes in the game.

## 4. The game

The game joins the lobby, gets into a room, then starts its session. A
desktop build joins over UDP, a web build over `wss://`:

```rust
use ponko::net::lobby::LobbyClient;
use ponko::net::rtc::RtcConfig;

# fn run() -> std::io::Result<()> {
let lobby = if cfg!(target_arch = "wasm32") {
    LobbyClient::join_web("wss://lobby.example.org")?
} else {
    LobbyClient::join("lobby.example.org:7700")?
};
// STUN and TURN from coturn: desktop and browser peers use both.
let rtc = RtcConfig::new()
    .stun("stun:lobby.example.org:3478")
    .turn("turn:lobby.example.org:3478", "game", "CHANGE-ME");
# let _ = (lobby, rtc);
# Ok(())
# }
```

Then, in the room ([`LobbyEvent::Joined`](lobby.md#the-client)):

- **A replicated or message game** connects peer to peer with
  `NetMessages::host_rtc(lobby.signaling(), rtc, n)` on the room's host and
  `join_rtc` on the others ([With the lobby](webrtc.md#with-the-lobby)).
  ICE tries the direct paths first and TURN last, so most players never
  touch coturn.
- **A lockstep game**, or a room that should skip WebRTC, runs its
  session through the lobby's relay with `Net::host_relay(lobby, n)` and
  `Net::join_relay(lobby)` (`NetMessages::host_relay` and `join_relay`
  for messages), as in
  [Game sessions through the relay](lobby.md#game-sessions-through-the-relay).

Let the room's host decide which, and say it in the room's text
(`set_meta("transport=relay")`), so every member starts the same kind of
session.

## Check it end to end

1. `docker logs lobby` shows `lobby_listening`; every 10 seconds a
   `lobby_report` line counts connections, rooms and relayed bytes.
2. Start the game on two desktop machines on different networks: one
   creates a room, the other joins its code. `lobby_report` shows
   `rooms: 1`.
3. Start a relayed session: `relay_in` and `relay_out` climb in
   `lobby_report` while the match runs.
4. Start a WebRTC session with one player behind a mobile hotspot (often
   a strict NAT): it connects, and coturn's log shows an allocation for
   that player.
5. Open the web build from `https://` and join: the browser's lobby
   connection goes through Caddy. Two browsers behind one home router
   share that router's limits, as two desktop players do.

## What was tested, and what wasn't

The engine's tests run every piece of this setup on one machine, without
the internet: relay sessions through the real lobby server with scripted
latency, reordering and loss (`crates/ponko_lobby/tests/relay_sessions.rs`),
the proxy header through a small reverse proxy in the test
(`crates/ponko_lobby/tests/proxy.rs`), and two desktop peers behind
simulated strict NATs connecting through a TURN server that runs inside
the test, in memory and over UDP on loopback
(`net::rtc_tests` and `net::turn`). **Not tested**: this exact
deployment on a real server, Caddy, coturn itself (the tests' TURN
server is a small one of the engine's own, speaking the same RFC 8656
messages), and real NATs. Run the checks above before you rely on it.
