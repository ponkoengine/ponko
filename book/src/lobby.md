# Lobbies, matchmaking and relays

> **Maturity: [tested](maturity.md).** Game: none. Tests: the lobby client and
> server tests, on loopback. Platforms: Linux, macOS and Windows, headless
> (the workspace tests in the list of checks). Graphics: not involved.
> Missing: a reference game and a public deployment.

Players who want to play together first have to find each other. Ponko has
a reference lobby server for that, in three pieces:

- **`ponko-lobby`**, a dedicated server program (the `ponko_lobby` crate):
  rooms with six-character codes, join and leave, matchmaking queues that
  fill rooms by size, host migration, and a **relay** that forwards a
  room's traffic for players who can't connect to each other directly.
- **`LobbyClient`** in the engine (feature `lobby`, off by default): what a
  game uses to talk to it, over UDP or, from a browser, WebSocket.
- **`LobbyServer`**, the same server as a library, for a server of your own
  with your own matchmaking and host migration rules.

It is a *reference* server: small, readable, limited against abuse, meant to
run as it is for a small game or to start your own from. Read
[what it is not](#what-it-is-not) before you put it on the internet.

## Run the server

```sh
cargo run -p ponko_lobby                          # UDP 0.0.0.0:7700
LOBBY_WEB=0.0.0.0:7701 cargo run -p ponko_lobby   # and browsers on ws://
```

| Setting | Default | What |
|---|---|---|
| `LOBBY_UDP` | `0.0.0.0:7700` | Where clients connect over UDP |
| `LOBBY_WEB` | empty: off | Browser clients over WebSocket (`ws://`; put a TLS proxy in front for `wss://`, as in [the browser chapter](networking.md#in-the-browser)) |
| `LOBBY_STUN` | empty: off | Answers STUN requests on this UDP address (usually `0.0.0.0:3478`), for [WebRTC peers](webrtc.md#stun-turn-and-nats) |
| `LOBBY_MAX_CLIENTS` | 1024 | Clients the UDP endpoint takes |
| `LOBBY_HZ` | 100 | Polls per second; a relayed message waits at most one poll |
| `LOBBY_REPORT_SECS` | 10 | How often it logs a `lobby_report` line; 0: never |

It runs in [`Game::run_server`](servers.md#run_server), so it behaves like
any Ponko dedicated server: JSON log lines on standard output
(`lobby_listening`, `lobby_report` with rooms, connections, requests,
refusals and relay counters, the engine's `tick_report`), and a clean stop
on `SIGINT` or `SIGTERM`: every member in a room hears
`LeaveReason::ServerStopping`, then the connections close and the process
exits 0 within 2 seconds.

### In a container

`crates/ponko_lobby/Dockerfile` is the [dedicated-server container
recipe](servers.md#the-container-image) for this program: `ponko package`
builds, packages and smoke-runs it in a Rust image, and only the package
goes into a distroless, non-root runtime image. Build it from the root of
the engine's repository:

```sh
docker build -f crates/ponko_lobby/Dockerfile -t ponko-lobby .
docker run --rm --name lobby -p 7700:7700/udp -p 7701:7701/tcp ponko-lobby
docker stop lobby       # SIGTERM: members are told, then it exits
```

The package holds the 7.4 MB program (macOS, Apple silicon, release),
its licenses and `THIRD-PARTY-NOTICES.txt`; idle, it uses 3.9 MB of
memory. Publish the UDP port as UDP and the WebSocket port as TCP.

## The client

Turn on the engine's `lobby` feature, then own a `LobbyClient` and poll it
every frame, as you would a [message endpoint](networking.md#application-messages-rust):

<div class="lang-pair">
<div>

```rust
use ponko::net::Lane;
use ponko::net::lobby::{LobbyClient, LobbyEvent};

let mut lobby = LobbyClient::join("lobby.example.com:7700")?;
lobby.create_room(4)?; // sent once the server answers

// Every frame:
lobby.poll()?;
while let Some(event) = lobby.next_event() {
    match event {
        LobbyEvent::Joined(room) => println!("room code: {}", room.code),
        LobbyEvent::HostChanged(_) if lobby.is_host() => println!("we host the game now"),
        LobbyEvent::Relayed { from, bytes, .. } => println!("{from:?}: {} bytes", bytes.len()),
        LobbyEvent::Refused(error) => println!("refused: {error}"),
        _ => {}
    }
}
// In a room: to every other member, unreliable, through the server.
if lobby.room().is_some() {
    lobby.relay(None, Lane::Unreliable, b"position 3 4")?;
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:lobby}}
```

</div>
</div>

A friend joins with the code, in any letter case:
`lobby.join_room("kq7x2m")`. The client keeps its own state up to date:
`me()`, `room()` (code, host, members in the order they came in, size,
text), `is_host()`, `is_queued()`. A browser build joins the WebSocket port
with `LobbyClient::join_web("wss://lobby.example.com")`.

### Rooms, hosts and host migration

A room has a size (2 up to the server's cap, 16 by default), a host (its
creator, or the first player of a matchmaking group) and a short text the
host sets with `set_meta`: a game mode, a "started" flag, or the host's own
address for players who [can connect directly](#direct-or-relayed).

When the host leaves or loses its connection, the server's **host migration
rule** picks the next one, and every member gets `HostChanged`. The default
rule picks the member who has been in the room longest. The game moves its
authority with it: the new host takes over the simulation from what it
has. A rule may also close the room instead (every member gets
`Left(LeaveReason::RoomClosed)`), for games whose host can't hand over.

### Matchmaking

`lobby.queue("duel", 2, rating)` waits in the queue named `duel` for a room
of 2. Players only meet others in the same queue and size. The default rule
fills rooms first come, first served, as soon as enough players wait; the
first of them hosts. `Queued` says the player waits, `Joined` follows with
the room, and `cancel_queue` leaves the queue. The `rating` is for a
[rule of your own](#your-own-rules); the default ignores it.

### Direct or relayed

The lobby only introduces players. Once in a room they can play:

- **directly**: the host runs a [message endpoint](networking.md#application-messages-rust)
  or a [lockstep session](networking.md#host-and-join), puts its public
  address in the room's text, and the others join it. Fastest, but the
  host's port must be reachable (port forwarding, no strict NAT);
- **through the relay**: `relay(to, lane, bytes)` sends the game's own bytes
  to one member (`Some(id)`) or to every other member (`None`); they arrive
  as `LobbyEvent::Relayed` with the sender's id and lane. Works from behind
  any NAT, costs a trip through the server;
- **peer to peer over WebRTC** (feature `webrtc`): the lobby carries the
  setup messages and, with `LOBBY_STUN`, tells players their public
  addresses; then the players connect directly, also from browsers and
  behind most home routers. See [Peer to peer over WebRTC](webrtc.md).

The relay forwards **opaque bytes**: the server never reads, checks or
keeps them. It only checks that sender and receiver are in the same room,
the size cap and the sender's rate limit. Whatever the game sends, its
receiver has to check, as with any network input.

## Your own rules

The `ponko_lobby` library is the same server with hooks: a `Matchmaker`
picks groups from a queue (it runs whenever someone joins a queue and
every second, so a rule can widen with waiting time), and any
`FnMut(&RoomInfo) -> Option<MemberId>` is a host migration rule:

```rust
{{#include ../../crates/ponko_lobby/examples/custom_rules.rs:matchmaker}}
```

```rust
{{#include ../../crates/ponko_lobby/examples/custom_rules.rs:server}}
```

`LobbyServer` owns no thread: call `poll` in a loop (or from a system, as
`ponko-lobby` does), or `poll_elapsed` with a clock of your own in tests.
It may listen on several endpoints at once (IPv4 and IPv6, or more than
the 1024 clients one endpoint takes). Groups a rule returns with fewer
than 2 or more than `size` players, an index out of range or used twice
are skipped.

## Limits

Every limit is a field of `LobbyLimits`; each one has a test in
`crates/ponko_lobby/tests/limits.rs`.

| Limit | Default | Over it |
|---|---|---|
| Requests per address (create, join, leave, queue, ...) | 10 a second, bursts of 20 | Refused with `RateLimited` (told at most once a second) |
| Relay bytes per address | 256 KiB a second, bursts of 512 KiB | Dropped (counted in `relay_dropped`) |
| Connections per address | 16 | Refused with `TooManyConnections`, then closed |
| Request size | 1 KiB | Refused with `TooLarge` |
| Relay payload | 4 KiB (the client checks it too) | Dropped |
| Room size | 16 | Refused with `BadSize` |
| Room text, queue name | 256 bytes | Refused with `TooLarge` |
| Rooms | 10 000 | Refused with `ServerFull` |
| Idle connection: in no room and no queue | 60 s | Told `Left(Idle)`, then closed |
| Idle room: no request or relay message from any member | 10 min | Members told `Left(Idle)`, room closed |
| Silent link: no packet at all (a client that vanished, a half-open connection) | 10 s | Dropped by the endpoint, the member leaves its room |
| WebSocket handshake | 5 s, 64 at once | Connection closed |
| WebSocket connection that sends no packet | 30 s | Connection closed |

An **address** is an IP address, whatever the port, so every player behind
one home router shares the budgets; that is why the budgets are per second
and generous. A browser connection is an address of its own: the server
sees the WebSocket connection, not the browser's IP (behind a proxy it
would only see the proxy). `AddressKey::Connection` gives every connection
its own budgets instead, for a server behind such a proxy or for load
tests. Malformed requests are refused with `BadRequest` and don't close
the connection.

## What it is not

- **No accounts.** A member is a connection; its id is a number the server
  hands out and forgets when the connection closes. Anyone may connect.
- **No persistence.** Rooms and queues live in memory: a restart empties
  them, and one process is one lobby (no sharing between servers).
- **No anti-cheat and no game rules.** The relay forwards bytes it never
  reads; the host's game decides what is true.
- **No encryption or authentication.** Like the engine's UDP transports,
  traffic is plain: run it on a trusted network or behind a secure tunnel,
  and serve browsers through a TLS proxy.
- **Room codes are not secrets.** Anyone may try codes (at the request
  rate); a private game needs a check of its own after joining.
- **No TURN.** With `LOBBY_STUN` the server helps players behind most
  home routers connect [peer to peer over WebRTC](webrtc.md); players who
  still can't reach the host use the relay.

## Under load

`cargo run --release -p ponko_lobby --example load_test` runs the whole
server in one process with 1000 rooms of 3 made by code and 100 more by
matchmaking: 3300 clients over in-memory endpoints (the real protocol,
framing and reliability layer; only the sockets are left out), on a
simulated clock. Every member sends 20 relay messages of 64 bytes to its
room on the reliable lane, then every host leaves and the rooms migrate.

Measured on an Apple M4 Max (macOS, release build) on an idle machine
(2026-11-22, revision `9235ea05`, load average under 2.8), three runs:

| | |
|---|---|
| Whole run | 0.70–0.72 s wall, 0.70–0.72 s CPU |
| Rooms by code (create, then two joins each) | 16,100–16,600 rooms a second |
| Relay | 240,000–244,000 messages a second delivered, 14.7–14.9 MiB/s of payload, 132,000 of 132,000 delivered |
| Host migrations | 1100 of 1100 seen by every member |
| Peak memory | 122–124 MiB for the process: the server and all 3300 clients |

The rates are of the process, which runs the clients too: the server alone
is faster. With rooms of 8 (8800 clients, `load_test 1000 8`) the run took
2.4 s wall and CPU, delivered 634,000–654,000 relay messages a second and
peaked at 334–336 MiB. Earlier figures on this page were taken on a laptop
busy with other builds (4.6–6.4 s wall and 1.1–1.2 s CPU for the first
run), where the process mostly waited; the peak memory was 104–105 MiB
then, 18 MiB less than now, a change load doesn't explain and that is not
yet looked into. The same test with 40
rooms runs in `cargo test` (`the_load_test_fills_every_room_and_delivers_every_relay_message`).

### Hostile clients

`crates/ponko_lobby/tests/hostile.rs` runs the server for two simulated
minutes with two honest rooms relaying every 10 ms next to twelve hostile
connections, each sending four attacks every 10 ms: garbage, requests and
relay payloads far past the size limits, rooms made and left, queues under
names never seen before, guessed and real room codes, relay floods to
everyone and to members that don't exist. A third of them never read what
the server sends them, a quarter vanish without a goodbye. The test counts
the heap of the whole process (server and clients), sampled every
simulated second once the vanished connections have timed out, and checks
that it is flat: its average over the last 35 s within 512 KiB of the 35 s
before, and never more than 2 MiB above it. It also checks that the
vanished connections are dropped and the others kept, and that the honest
players keep their rooms and hear each other.

One run on an Apple silicon laptop that was building other projects
(debug build, 73 s wall for 120 simulated seconds): 371 588 requests, of
which 369 698 refused (359 746 for the rate limit), 133 862 relay messages
in and 76 973 dropped; the heap averaged 3.44 MiB from 50 to 85 s and
3.60 MiB from 85 to 120 s, with a peak of 4.28 MiB.

## The protocol

`ponko::net::lobby::protocol` documents what client and server say, for a
client or a server in another language: every message is one byte saying
what follows, then a JSON request or reply, or a relay frame (a member id
and the opaque payload), over the engine's message endpoint.

## C and C++

`ponko_lobby_join`, `_poll`, `_next_event`, `_create_room`, `_join_room`,
`_leave_room`, `_queue`, `_cancel_queue`, `_set_meta`, `_relay`, `_state`,
`_members`, `_room_meta`, `_disconnect` and `_free`, and `ponko::lobby` in
C++, are the client. A native game joins over UDP only:
`LobbyClient::join_web` is Rust-only, so a C or C++ game built for the
web has no lobby client. The server is Rust: a native game talks to `ponko-lobby`
as it is.
