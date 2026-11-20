# Peer to peer over WebRTC

A [message endpoint](networking.md#application-messages-rust) normally
runs over UDP, or over WebSocket for browsers. With the engine's `webrtc`
feature it can also run over **WebRTC data channels**: browser to browser,
browser to desktop and desktop to desktop, directly between the players and
through most home routers. A server is only needed to introduce the players
(signaling) and to tell each one its public address (STUN). The
[lobby server](lobby.md) does both.

The endpoint behaves like any other: the same lanes, events, handshake,
limits and `ctx.net.messages`. Only how the packets travel changes, so a
game that already speaks messages over UDP needs nothing else.

## With the lobby

Every player owns a [`LobbyClient`](lobby.md#the-client) and is in a room.
The room's host hosts, every other member joins the host by its member id,
and the lobby relays the few signaling messages between them:

<div class="lang-pair">
<div>

```rust
use ponko::net::NetMessages;
use ponko::net::lobby::LobbyClient;
use ponko::net::rtc::RtcConfig;

let mut lobby = LobbyClient::join("lobby.example.com:7700")?;
// ... once in a room (LobbyEvent::Joined):
let config = RtcConfig::new().stun("stun:lobby.example.com:3478");
if let Some(host) = lobby.room().map(|room| room.host) {
    let endpoint = if lobby.is_host() {
        NetMessages::host_rtc(lobby.signaling(), config, 8)?
    } else {
        NetMessages::join_rtc(lobby.signaling(), host.to_bits().into(), config)?
    };
    ctx.net.messages = Some(endpoint);
}
// Every frame: keep polling the lobby, which carries the signaling.
lobby.poll()?;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:webrtc}}
```

</div>
</div>

Signaling messages never show up as `LobbyEvent::Relayed`: the lobby client
hands them to the endpoint. `endpoint.rtc_signal_peer(peer)` says which
lobby member a connected peer is. In a browser the code is the same; the
lobby client joins over WebSocket (`LobbyClient::join_web`).

If the host leaves, the room [migrates](lobby.md#rooms-hosts-and-host-migration)
as usual: the new host calls `host_rtc`, the others drop their endpoint and
`join_rtc` the new host.

## With any signaling channel

`Signaling` is a pair of queues the endpoint reads and writes. Anything
that can carry a few short messages to a known peer works as signaling: the
lobby, a game's own server, a chat room. Take the messages to send from
`next_outgoing()` (a peer id and bytes) and hand what arrives to
`deliver(from, bytes)`. Peer ids are the game's own numbers (the lobby uses
member ids); the joining side names its host's id. This is the
`net_webrtc` example, two games in one process with a thread carrying the
messages between their handles:

```rust
{{#include ../../examples/net_webrtc.rs:webrtc}}
```

Run it with
`cargo run --example net_webrtc --no-default-features --features webrtc`.
Signaling messages start with fixed bytes (`Signaling::is_signal`), so a
channel that carries other traffic too can pick them out. Each one is a
little JSON (an offer, an answer, an address or a goodbye); an offer is
about 1 KiB.

## STUN, TURN and NATs

Most players sit behind a router that shares one public address (a NAT). A
**STUN** server tells each peer the public address its packets come from;
the peers swap these through signaling and try every pair of addresses
until one works (ICE). `RtcConfig::stun("stun:host:port")` adds one.

| Who | Without STUN | With STUN | Needs TURN |
|---|---|---|---|
| Same machine or local network | connects | connects | |
| One side has a public address (a server, port forwarding) | connects | connects | |
| Both behind ordinary home routers (cone NATs) | no | connects | |
| One side behind a strict (symmetric) NAT, the other public | should (only tested with STUN) | connects | |
| Both behind strict NATs (some mobile networks, some offices) | no | no | yes |

**TURN** relays the traffic of peers that can't reach each other at all.
`RtcConfig::turn(url, username, credential)` adds a server **in browsers
only**: the desktop side has no TURN client and ignores it. A desktop peer
still connects to a browser that uses TURN, since the browser's relay
address is just one more address to try. Players who can't connect at all
can fall back to the [lobby's relay](lobby.md#direct-or-relayed), which
works through any NAT.

On the desktop, `RtcConfig::bind` picks the UDP socket's address (a fixed
port is easier to forward on a router), and `host_address` offers a public
address the machine has, such as a server's. Host names in `stun:` URLs
are looked up when the endpoint is made.

## Lanes

Each connection has one data channel, unordered and without retransmits,
so it behaves like UDP. The message protocol runs on top of it exactly as
over UDP: `Lane::Reliable` and `Lane::ReliableUnordered` are made reliable
by the engine, and `Lane::Unreliable` stays unreliable, so a lost position
update never holds up the next one. The handshake, heartbeats, timeouts,
`max_clients` and message size limits are the endpoint's usual ones.

## Deploy the lobby as signaling and STUN server

`ponko-lobby` (see [Run the server](lobby.md#run-the-server)) answers STUN
requests on a UDP port of its own when `LOBBY_STUN` is set:

```sh
LOBBY_STUN=0.0.0.0:3478 LOBBY_WEB=0.0.0.0:7701 cargo run -p ponko_lobby
```

- Open UDP 7700 (desktop lobby clients), TCP 7701 (browser lobby clients;
  behind a TLS proxy for `wss://`, as in [the browser chapter](networking.md#in-the-browser))
  and UDP 3478 (STUN). The game traffic itself goes between the players
  and never touches the server.
- STUN answers are rate limited per address (20 a second, bursts of 40),
  so the port can't be used to flood someone else. The `lobby_report` log
  line counts `stun_answered` and `stun_rate_limited`.
- STUN has to see the players' real public addresses: run it on a public
  address, not behind a load balancer or proxy that rewrites them.
- Any other STUN server works too. TURN needs a server of its own
  (coturn, or a hosted one); the lobby has none.

**Security.** WebRTC encrypts every connection (DTLS), keyed by
fingerprints in the offer and answer. Those travel through the lobby, which
neither authenticates players nor encrypts its UDP traffic, so someone who
can rewrite traffic between a player and the lobby could put themselves in
the middle. Serve browsers through `wss://` and run the lobby where its
traffic can't be tampered with, as for the lobby itself. The endpoint
checks what arrives as with any network input: it ignores signaling from
peers it isn't talking to, keeps at most `max_clients + 8` connections
being set up, gives up on one after 30 seconds, and reads no signaling
message over 16 KiB.

## Limits

- **Star, not mesh.** Every client connects to the host, as with UDP;
  clients don't connect to each other.
- **No TURN on the desktop** (see above).
- **mDNS addresses are skipped on the desktop.** Browsers hide local
  addresses behind `*.local` names, which the desktop side can't look up;
  a desktop and a browser on one local network then connect through STUN
  instead (the router must allow that, as most do).
- **One UDP socket per endpoint** on the desktop, polled on the game thread
  with the endpoint: no async runtime and no extra threads.
- **A browser tab must stay visible**, as for every browser game: hidden
  tabs get no frames, so their endpoint stops polling.

## Which WebRTC library

The desktop side uses [str0m](https://crates.io/crates/str0m), the
browser side the browser's own `RTCPeerConnection` through web-sys. The
candidates, checked from their crates (`cargo info`, `cargo tree`) in
November 2026:

| Crate | Version | License | Model | Crates it builds (new to the engine) | Notes |
|---|---|---|---|---|---|
| **str0m**, `aws-lc-rs` crypto | 0.25.0 | MIT/Apache-2.0 | sans-IO, no runtime | 91 (49) | Picked: polled from the game thread like the rest of the engine. No TURN client. |
| str0m, `rust-crypto` | 0.25.0 | MIT/Apache-2.0 | sans-IO | 119 (71) | Still builds aws-lc through its DTLS stack, so no lighter |
| rtc (webrtc-rs, sans-IO) | 0.21.1 | MIT/Apache-2.0 | sans-IO | 157 (100) | Has TURN and mDNS; about twice the new crates |
| webrtc (webrtc-rs) | 0.21.1 | MIT/Apache-2.0 | async (tokio or smol) | 179 | Needs an async runtime the engine doesn't have |
| datachannel (libdatachannel) | 0.16.1 | MPL-2.0 | C++ library, own threads | 79 | Builds C++ with cmake; MPL-2.0 |
| matchbox_socket | 0.15.0 | MIT/Apache-2.0 | async | 216 | Brings its own signaling protocol; webrtc-rs on the desktop |

## What it costs

The feature is off by default and adds nothing to a build without it. With
it, the desktop build compiles str0m, its DTLS and SCTP stacks and aws-lc
(a C library: building it needs a C compiler). Measured on an Apple
silicon laptop that was building other projects at the same time,
release builds of the `net_messages` and `net_webrtc`
examples (`--no-default-features`):

| | `net_messages` (UDP) | `net_webrtc` | Difference |
|---|---|---|---|
| Binary | 7.4 MB | 14 MB | about +6.6 MB |
| Stripped | 5.8 MB | 11 MB | about +5.2 MB |
| Crates in the build (`cargo tree`, headless) | 104 | 156 | +52 |
| Release build | 3.6 min from clean | 4.7 min more on top | |

The build times are of a busy machine and only show the order of size.

The browser build adds no crates beyond more web-sys bindings.

## What was checked, and what wasn't

Checked, in `cargo test`:

- Two endpoints connect over str0m and every lane delivers both ways, on an
  in-memory network on the simulated clock (no sleeps, no internet).
- Reliable lanes survive a link that drops every fifth packet, and
  unreliable messages are never delivered stale.
- Two peers behind simulated cone NATs connect with the in-memory STUN
  server and not without it; a peer behind a symmetric NAT reaches a public
  host, two symmetric NATs don't connect (TURN's case).
- A host takes several clients, refuses offers beyond its limit, ignores
  strangers, and gives a rejoining client a fresh connection.
- The same over real UDP sockets on loopback.
- Through `ponko-lobby`: two room members connect peer to peer with the
  lobby as signaling and STUN server (`crates/ponko_lobby/tests/webrtc.rs`),
  and the server's STUN responder answers and rate limits. The STUN
  messages match the RFC 5769 test vector.
- C: `ponko_net_messages_rtc_host` and `_join` connect a room member.
- The browser code compiles and passes clippy for `wasm32-unknown-unknown`.

**Not checked**, and worth a person's time before relying on it:

- **No browser ran.** The browser side (`RTCPeerConnection` through web-sys)
  was never run in a browser, so browser to browser and browser to desktop
  are untested. A test answers a hand-written offer in Chromium's format
  (with an mDNS and a server-reflexive address), not one captured from a
  real browser.
- **No real NAT.** NAT traversal was tested against simulated NATs only,
  never across real routers or the internet.
- **No public STUN or TURN server** was used, and TURN not at all.

To try it by hand: run `ponko-lobby` with `LOBBY_STUN`, then two copies of
a game that uses the code above, on two networks, one of them in a
browser.

## C and C++

`ponko_net_messages_rtc_host(lobby, stun_urls, bind, max_clients, &endpoint)`
and `ponko_net_messages_rtc_join(lobby, host_member, stun_urls, bind, &endpoint)`
make an endpoint signaled through a lobby client (`lobby::rtc_host` and
`lobby::rtc_join` in C++); `stun_urls` is a space-separated list or null.
`ponko_net_messages_rtc_peer` (`net_messages::rtc_peer`) gives a peer's
member id. Build `ponko_ffi` with `--features webrtc`; without it the calls
fail with an error that says so. `Signaling` on its own channel and TURN
are Rust-only, and C and C++ games don't build for the web.

## Testing

```sh
cargo test -p ponko --no-default-features --features webrtc,lobby --lib -- net::rtc net::stun
cargo test -p ponko_lobby --features webrtc --test webrtc --test stun
cargo test -p ponko_ffi --features webrtc --test rtc
```
