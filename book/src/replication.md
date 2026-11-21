# Replication: the server owns the world

With replication, one game is the **server**: it runs the rules and owns
every entity. The others are **clients**: they send their controls, and
the engine keeps their worlds in step with the server's. You don't write
packets. You declare which components replicate and mark the entities that
do; the engine does the rest:

- every tick it sends each client what changed among the entities that
  client should see, as small differences from what that client already
  acknowledged;
- it rounds each number to the precision you asked for, so a position
  takes 15 bits instead of 32;
- it stays within a bandwidth budget per client, sending the most
  important changes first;
- it spawns, updates and despawns each client's copies;
- a client's own player moves at once (it is **predicted**) and the
  server corrects it if it ever disagrees;
- reliable **events** go both ways for things that happen once.

It works over every transport (UDP, WebSocket for browsers, Steam relays)
and survives loss, jitter, reordering and duplication. The rest of this
chapter is the path in order; [Orb Arena](#orb-arena-the-reference-game)
puts every step in one small game.

Replication is off until you declare something, and then free: a game
that never calls `Game::replication` sends and computes nothing for it.

## Declare what replicates

A replicated component derives `Replicate`. Each field says how precisely
it travels; a field without an option travels exactly.

<div class="lang-pair">
<div>

```rust
use ponko::net::{Replicate, Replication};

// A plain component, the same on the server and every client.
#[derive(Component, Replicate, Clone, Copy, Default)]
struct Fighter {
    #[replicate(min = -1000.0, max = 1000.0, step = 0.01)]
    x: f32,
    #[replicate(bits = 7)]
    hp: u8,
}

// What replicates, declared the same way on every side.
let game = Game::new("Arena", 800, 600)
    .replication(Replication::new().component::<Fighter>());
# let _ = game;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:replication_declare}}
```

</div>
</div>

| Field option | Travels as | Bits |
|---|---|---|
| (none) | the exact value | the type's own (32 for `f32`, 1 for `bool`) |
| `#[replicate(min = -1000.0, max = 1000.0, step = 0.01)]` | rounded to the step, within the range (each component of a `Vec2` or `Vec3`) | enough for the steps: 18 here |
| `#[replicate(angle, bits = 12)]` | an angle in radians, wrapped to one turn | 12 |
| `#[replicate(bits = 7)]` | the low bits of an integer; for a `Quat`, the smallest three components | 7 (a `Quat`: 2 + 3 × 7) |
| `#[replicate(skip)]` | not sent: each side keeps its own value | 0 |

Fields of type `f32`, `bool`, `u8`, `u16`, `u32`, `i8`, `i16`, `i32`,
`Vec2`, `Vec3`, `Quat` and `NetId` work as they are; implement
`ReplicateField` for your own (an enum, say), or write `Replicate::fields`
by hand. `ponko::net::replicated_bits::<T>()` says what a value costs. The
engine's `d2::Transform` and `d3::Transform` replicate exactly, so the
simplest declaration is `Replication::new().interpolated::<Transform>()`.

Every side declares the same types in the same order: a client sends a
hash of its declarations when it joins, and the server refuses one that
differs, with a reason (`ctx.net.replication.error()`). Put the
declarations in one function both sides call, as Orb Arena's
`replication()` does.

## Mark entities

On the server, an entity replicates when it carries `Replicated`, and its
declared components go to the clients. Despawning it (or removing the
marker) despawns it on every client.

<div class="lang-pair">
<div>

```rust
# use ponko::net::{Replicate, Replication};
# #[derive(Component, Replicate, Clone, Copy, Default)]
# struct Fighter {
#     #[replicate(min = -1000.0, max = 1000.0, step = 0.01)]
#     x: f32,
#     #[replicate(bits = 7)]
#     hp: u8,
# }
use ponko::net::{PeerId, Replicated};

let world = Game::new("Arena server", 1, 1)
    // A real server hosts: .messages(NetMessages::host("0.0.0.0:7777", 16)?)
    .replication(Replication::new().component::<Fighter>())
    .run_headless(3, |ctx| {
        ctx.world.spawn((
            Fighter { x: 1.5, hp: 100 },
            // The first client to join controls it.
            Replicated::owned_by(PeerId::from_bits(1)),
        ));
    });
# let _ = world;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:replication_server}}
```

</div>
</div>

| `Replicated::...` | Means |
|---|---|
| `new()` | the server owns it (an orb, a door, a monster) |
| `owned_by(peer)` | a player controls it: that client's input moves it, it is in every packet to that client whatever the budget, and that client sees it as `Replica::is_mine` |
| `.with_priority(4.0)` | it matters more when the budget can't send everything (default 1) |
| `.global()` | every client gets it, whatever [interest](#interest-and-the-budget) says (the match clock, a scoreboard) |
| `set_owner(Some(peer))` | hands it over: clients see a new authority epoch (`Replica::epoch`) and the old owner's predictions stop counting |

At the end of the tick an entity was marked in, the server gives it a
`NetId` (`Replicated::net_id`). Ids are the same on every side, so they are
what events and your own data use to name an entity:
`ctx.net.replication.entity(id)` finds it on any side.

On a client, each copy is an ordinary entity with the replicated
components and a `Replica` (its `id()`, `owner()`, `epoch()` and
`is_mine()`). The engine spawns and despawns copies; your systems read
them, draw them and add their own local components to them. Components the
server doesn't replicate never leave it.

## Host and join

A game is a server or a client by its message endpoint:

```rust
use ponko::net::{NetMessages, Replication};

# fn games() -> std::io::Result<()> {
// The server: up to 16 clients over UDP, and browsers over WebSocket.
let server = Game::new("Arena", 800, 600)
    .replication(Replication::new())
    .messages(NetMessages::host("0.0.0.0:7777", 16)?.accept_web("0.0.0.0:7778")?);
// A client.
let client = Game::new("Arena", 800, 600)
    .replication(Replication::new())
    .messages(NetMessages::join("127.0.0.1:7777")?);
# let _ = (server, client);
# Ok(())
# }
```

| Endpoint | Role (`ctx.net.replication.role()`) |
|---|---|
| none | `Server`: a single-player game runs the same code, offline |
| one that hosts | `Server`, and a player too when the host plays (a listen server) |
| one that joined | `Client` |

The role is known from the first tick on. Systems that are the server's
rules (spawning, scoring, deciding who hit what) return at once on a
client: its world holds only what the server sends it, so the rules have
nothing to work on there.

```rust
use ponko::net::ReplicationRole;

fn spawn_monsters(ctx: &mut Ctx) {
    if ctx.net.replication.role() == ReplicationRole::Client {
        return; // the server's copy reaches us
    }
    // ...
}
# let _ = spawn_monsters;
```

On a client, `is_joined()` turns true once the server accepted its
declarations, `me()` is its `PeerId`, and `server_tick()` is the newest
server tick it has seen. On the server, `clients()` lists who joined:
spawn a player for each newcomer and despawn the player of one that left
(Orb Arena's `join_and_leave` below).

## Showing it smoothly

A component declared with `component::<T>()` is set the moment its update
arrives. Movement arrives in steps (a packet a tick at best, fewer under a
tight budget), so for things that move, declare them `interpolated`:

```rust
use ponko::net::Replication;

let replication = Replication::new().interpolated::<ponko::d2::Transform>();
# let _ = replication;
```

Clients then show the component 0.1 s in the past, between the two values
around that time. Past the newest value it holds still, so something that
stopped doesn't drift on. `interpolated_with(SnapshotConfig { .. })` sets
the delay, the buffer and how far to extrapolate. Your own type implements
`Interpolate` (Orb Arena's `Orb` does). A client's own predicted entities
are never delayed.

## Your own player at once: prediction

Waiting a round trip to see your own player move feels like wading
through mud. With prediction, a client moves its own entity the moment
the key is pressed, and the server's state only confirms it:

```rust
use ponko::net::{Replicate, Replication};

#[derive(Component, Replicate, Clone, Copy, Default, PartialEq)]
struct Runner {
    #[replicate(min = -500.0, max = 500.0, step = 0.01)]
    at: Vec2,
}

/// What a player presses each tick.
#[derive(Replicate, Clone, Copy, Default)]
struct Stick {
    #[replicate(min = -1.0, max = 1.0, step = 0.01)]
    dir: Vec2,
}

/// One input's movement: the server runs it, and so does the owner.
fn run(r: &mut Runner, s: &Stick) {
    r.at = (r.at + s.dir * 3.0).clamp(Vec2::splat(-490.0), Vec2::splat(490.0));
}

fn controls(ctx: &mut Ctx) {
    let x = ctx.input.key_down(Key::Right) as i32 - ctx.input.key_down(Key::Left) as i32;
    ctx.net.replication.set_input(Stick { dir: vec2(x as f32, 0.0) });
}

let game = Game::new("Runner", 800, 600)
    .replication(Replication::new().input::<Stick>().predicted::<Runner, Stick>(run))
    .sim_system(controls);
# let _ = game;
```

1. `input::<I>()` declares the one input type, and `set_input` gives this
   tick's. A tick without one sends `I::default()` (nothing pressed).
2. `predicted::<S, I>(step)` replicates `S` and names the step that applies
   an input to it. The owner runs `step` at once; the server runs it when
   the input arrives, once per input and in order. Other clients see `S`
   as a plain component.
3. When the server's state for an input differs from the prediction, the
   client takes the server's and replays its newer inputs on top.

Both sides round the state and the input to their quantization after
every step, so a step that depends only on the state and the input
predicts **exactly**: no corrections, even under loss. Orb Arena's test
checks that (`corrections == 0`). Two rules keep it that way:

- Put the whole movement in `step`, walls included. Changes other server
  systems make (a knockback, a teleport) arrive as corrections, which is
  right for them but shows as a snap.
- Read nothing else in `step`: no time, no random numbers, no other
  entities.

On the server, `input_of::<I>(peer)` is the input it applied last for a
player (for aiming, say). On a listen server, `set_input` moves the
host's own entities (owned by `PeerId::HOST`). `client_stats()` counts
`confirmed`, `corrections` and `replayed` inputs.

## Events: things that happen once

State says where things *are*; events say what *happened*: a hit, a
pickup, a chat line, a vote. They are reliable and ordered, and arrive at
the start of a tick as `NetEvent<T>` in `ctx.events`, with the sender and
the server tick it was sent at.

```rust
use ponko::net::{NetEvent, Replication};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct Chat {
    text: String,
}

// Any side: send a line. The server passes every line on to everyone.
fn chat(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Enter) {
        ctx.net.replication.to_server(&Chat { text: "gg".into() });
    }
    let heard: Vec<Chat> = ctx.events.read::<NetEvent<Chat>>().iter().map(|e| e.event.clone()).collect();
    for line in heard {
        ctx.net.replication.to_clients(&line); // does nothing on a client
    }
}

let game = Game::new("Chat", 800, 600)
    .replication(Replication::new().event::<Chat>())
    .sim_system(chat);
# let _ = game;
```

| Call | Goes to |
|---|---|
| `to_server(&e)` | the server (on the server itself, its own next tick) |
| `to_client(peer, &e)` | one client; `PeerId::HOST` is the host's own player |
| `to_clients(&e)` | every client that joined (not the host's own player) |

Events are serde values, so they can carry anything; name entities in them
by `NetId` (it serializes as a number). They travel in binary
([MessagePack](https://msgpack.org), fields by position rather than by
name), after a header of two varints (the event type and the tick):

| Event | Bytes per event | As JSON before |
|---|---|---|
| `Hit { target: NetId, damage: f32, at: Vec2, critical: bool }` | 26 | 76 |
| `Chat { text: "gg" }` | 9 | 24 |
| `Pickup::Coin(25)` (an enum variant travels by name) | 12 | 22 |
| `Pickup::Key { door: 7 }` | 12 | 29 |

Because fields travel by position, an event type can't leave fields out
(`#[serde(skip_serializing_if = ...)]`): the other side can't tell which
field is which and drops the event. Send the `Option` as it is instead. A client's `NetEvent::tick` is the
newest server tick it had seen when it sent the event: what it was looking
at, for judging what it aimed at.

### Raw events: bytes you encode

An event type can also be declared by name, with plain bytes for events:
for C and C++ games, or a Rust game that encodes its own. `raw_event`
declares it, `to_server_raw`, `to_client_raw` and `to_clients_raw` send
the bytes, and they arrive unchanged as `NetEvent<RawEvent>`, every raw
type in one stream, told apart by `name()` (or `kind()`, the type's index
among the declared events, which is how C and C++ name them):

```rust
use ponko::net::{NetEvent, RawEvent, Replication};

fn ping(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::P) {
        // An error, not a panic: an undeclared name, or too many bytes.
        if let Err(e) = ctx.net.replication.to_server_raw("ping", &[1, 2, 3]) {
            eprintln!("{e}");
        }
    }
    let pings: Vec<Vec<u8>> = ctx
        .events
        .read::<NetEvent<RawEvent>>()
        .iter()
        .filter(|e| e.event.name() == "ping")
        .map(|e| e.event.bytes.clone())
        .collect();
    for bytes in pings {
        let _ = ctx.net.replication.to_clients_raw("ping", &bytes);
    }
}

let game = Game::new("Ping", 800, 600)
    .replication(Replication::new().raw_event("ping"))
    .sim_system(ping);
# let _ = game;
```

Raw events share the typed events' lane: reliable, ordered (one order
with typed events), delivered at the start of a tick. One carries at most
`MAX_RAW_EVENT_BYTES` (1 MiB less a 16-byte header). Both sides declare
it, in the same place among the events; a raw `"Chat"` never matches a
typed `Chat`, and the server refuses a client whose declarations differ
that way.

## Interest and the budget

A small arena can send everything to everyone. A big world can't, and
shouldn't: a client only needs what is near it. Both limits are
declarations:

```rust
use ponko::net::{InterestConfig, InterestTier, Replication};

let replication = Replication::new()
    .interpolated::<ponko::d2::Transform>()
    // 32 KB/s for each client, at most.
    .budget(32_000)
    // Near things every tick, farther ones every 4th, the rest not at all.
    .interest(InterestConfig {
        tiers: vec![
            InterestTier { radius: 200.0, every: 1 },
            InterestTier { radius: 600.0, every: 4 },
        ],
        ..InterestConfig::default()
    });
# let _ = replication;
```

**The budget** caps the bytes a client gets per second (64 KB/s by
default, `DEFAULT_BUDGET`). When the changes don't fit, the rest wait for
a later tick, and each waiting entity gains its priority every tick (scaled
by its interest ring), so the most important go first and nothing waits
forever. A client's own entities are always sent.

**Each change goes once.** Packets of state are unreliable, so the server
keeps what each client acknowledged as that client's baseline, and sends
changes against it. A change goes out once; while the acknowledgement is
on its way, the entity isn't sent again. It goes again only if the entity
changed again meanwhile, if the client acknowledged packets sent after its
one but not that one (it was lost: resent at once), or if the client hasn't
acknowledged it after a resend timeout. The timeout follows each client's
round trip, measured from its acknowledgements: the smoothed round trip
plus four of its deviations, at least two ticks and at most a second (0.2 s
until the first is measured), doubling with each try while nothing comes
back, up to 4 s. Spawns and despawns are resent the same way. On a link
with a round trip of 100 ms (six ticks at 60 Hz), sending a change in
every packet until it was acknowledged sent it six times. [Sent once,
resent when lost](#sent-once-resent-when-lost) has the details and the
numbers.

**Interest** sends each client only the entities within the outer ring,
nearer rings more often. An entity that leaves a client's rings is
despawned there and spawned again, in full, when it comes back. Where an
entity is comes from its `d2::Transform` or `d3::Transform`, or from your
`position(|world, entity| ...)` function. Where a client is comes from the
entities it owns, or `set_viewer(peer, Some(at))` for a spectator camera.
Entities without a position, and `global()` ones, go to everyone. The
engine's `Interest` (with hysteresis, so things at the edge don't flicker
in and out) does the deciding.

`peer_stats(peer)` on the server and `client_stats()` on a client count
packets, bytes, records, deltas, deferred entities and losses;
`resend_stats(peer)` on the server says what went again and why, and how
long it waits now ([Reading the resends](#reading-the-resends)).

## Test it

Multiplayer bugs hide in timing, so tests play the network on purpose:
`NetMessages::loopback(n)` connects `n` endpoints in memory,
`NetMessages::loopback_lossy(n, k)` loses every `k`th datagram and delivers
the rest newest first, and `NetMessages::loopback_link(n, link)` plays a
link that loses a share of the datagrams and delays each one, some more
than others, so they overtake each other:

```rust
use std::time::Duration;
use ponko::net::{LoopbackLink, NetMessages};

// 5% lost each way, a frame of delay plus up to 50 ms: packets overtake.
let link = LoopbackLink::new()
    .loss(0.05)
    .delay(Duration::from_millis(17))
    .jitter(Duration::from_millis(50))
    .seed(1);
let nets: Vec<NetMessages> = NetMessages::loopback_link(3, link)
    .into_iter()
    .map(NetMessages::game_clock)
    .collect();
# assert_eq!(nets.len(), 3);
```

The dice are a hash of the seed, the sender and the datagram's number, so
the same seed loses and delays the same datagrams on every run (on game
time, with the sides taking turns as below). `LoopbackLink` is Rust-only
for now; C and C++ tests use the plain loopback. To make a run the same
every time:

1. **Count game time.** `NetMessages::game_clock()` makes an endpoint's
   resends and timeouts count the game's ticks instead of the wall clock.
   A headless test plays a minute of game in a fraction of a second, and
   on the wall clock nothing would ever be resent.
2. **Take turns.** Each side is a whole headless game on its own thread,
   and they play one frame each, always in the same order:

```rust,no_run
{{#include ../../tests/net_arena.rs:take_turns}}
```

```rust,no_run
{{#include ../../tests/net_arena.rs:play}}
```

Choose the loss period so it doesn't divide the resend time (100 ms, 7
frames at 60 Hz). With every 7th datagram lost, the same resent message
would be lost every time.

The engine's own suite (`src/net/replication_conformance.rs`) runs on a
scripted network with loss, jitter, reordering and duplication on a
deterministic clock. Its table lists what replication guarantees, each
with its test: replicas converge once changes stop, spawns and despawns
arrive once, changes travel as deltas, the budget holds, interest
despawns what leaves, prediction is exact, events arrive once and in
order. It runs again over the lossy loopback, UDP, WebSocket and the Steam
relay (`--features steam`).

## Orb Arena: the reference game

`examples/net_arena` is a small game built only on replication, with no
packet code: players collect drifting orbs in a walled square. The server
owns everything; clients send controls and predict their own player;
events announce every collection and every wave.

```sh
cargo run --example net_arena                         # host and play (UDP 7777, browsers 7778)
cargo run --example net_arena -- server               # a dedicated server: no window, logs only
cargo run --example net_arena -- join 127.0.0.1:7777  # join one
cargo run --example net_arena -- join 127.0.0.1:7777 --bot   # let a bot play
```

Move with the arrow keys or WASD, dash with Shift, wave with Space.
`tests/net_arena.rs` plays it with no window: a dedicated server and two
bots over a lossy network, checking that both bots see the same players
and scores as the server, that every collection reaches every bot once and
in order, and that prediction was never corrected.

Everything that replicates, declared once in `game.rs`:

```rust,no_run
{{#include ../../examples/net_arena/game.rs:declare}}
```

The server's rules, which do nothing on a client:

```rust,no_run
{{#include ../../examples/net_arena/game.rs:server}}
```

A client's controls, every tick:

```rust,no_run
{{#include ../../examples/net_arena/main.rs:controls}}
```

The bot reads its own player (`Replica::is_mine` on a client, the host's
`Replicated` on a listen server) and steers like a player would:

```rust,no_run
{{#include ../../examples/net_arena/game.rs:bot}}
```

## Dedicated servers and browsers

A dedicated server is the same game with no window, paced to wall time
([Dedicated servers](servers.md) covers logs, shutdown and containers):

```rust,no_run
{{#include ../../examples/net_arena/main.rs:dedicated}}
```

A browser can't host, but it can join: the server also accepts WebSocket
connections (`accept_web`), and the web build joins with
`NetMessages::join_web("ws://host:7778")`. Browser players and native
players share one session.

## What it costs

`crates/ponko_bench` measures a dedicated server replicating 1,000
entities to 16 clients at 60 Hz: each entity has a position on a 2 km
square to the centimetre, a heading and a health. Every side is a headless
game over the in-memory loopback, taking turns on game time, so the bytes
are exact: the same on every run and every machine.

```sh
cargo run --release -p ponko_bench --bin replication -- 3
```

| Scenario | Payload, B/s per client | On the wire, B/s per client | Entities a client holds | Server CPU per tick: median, p90 |
|---|---|---|---|---|
| All move, no budget | 571,428 | 588,949 | 1,000 | 0.73 ms, 0.77 ms |
| All move, 64 KB/s budget (the default) | 63,893 | 67,578 | 1,000 | 0.38 ms, 0.42 ms |
| 1 in 10 move, 64 KB/s | 62,006 | 65,636 | 1,000 | 0.26 ms, 0.28 ms |
| 1 in 20 move, 64 KB/s | 31,266 | 34,891 | 1,000 | 0.14 ms, 0.39 ms (busy machine, see below) |
| All move, interest rings (250 m every tick, 500 m every 4th) | 62,246 (58,559 to 63,923 by client) | 65,905 | 235 | 0.36 ms, 0.38 ms |
| 1 in 10 move, 64 KB/s, 5% lost each way and reordered | 63,443 (63,055 to 63,831 by client) | 63,937 | 1,000 | 0.61 ms, 0.73 ms (busy machine, see below) |

*Payload* is replication's packets; *on the wire* adds the transport's
framing, acknowledgements and resends. The last scenario runs over
`loopback_link` (above): 5% of the datagrams lost each way, a frame of
delay plus up to 50 ms, so about one packet in three arrives after one
sent later. Its *payload* is what the server sent, and *on the wire* what
the clients received, so the lost datagrams count in the first and not
the second. The server resent 125 records per second to each client, all
of them early (acknowledgements skipped their packet) and none on the
timeout; the lost bandwidth comes out of the budget, so the payload rises
from 62,006 to the 64 KB/s cap. The link is seeded, so these bytes are
exact too. Ten seconds were measured after a
two-second warm-up (everyone joining and getting the spawn burst). Server
CPU is its whole tick: receiving, the game's systems, finding what
changed, encoding and sending to all 16 clients.

The CPU times come from three runs on a 14-core Apple M4 Max on an idle
machine (2026-11-20, nothing else running); the medians of the runs varied by
under 0.03 ms in every scenario (0.728 to 0.755 ms in the first). The byte
counts are exact and don't change. The `1 in 20` row's CPU time is not
from an idle machine: it was measured on 2026-11-21 while other builds
kept the same M4 Max busy (load average 100 to 180), so it is only good
for comparing with the row below it; an idle-machine run of it is still
to be done. The other rows' times were measured before changes were sent
once (next section) and didn't move beyond that busy machine's noise
since. The lossy row's time is from 2026-11-21 too, with the load average
at 97 to 114: three invocations of three runs each, alternating with the
lossless `1 in 10` row. Its medians were 0.608, 0.618 and 0.610 ms; the
lossless row's in the same invocations 0.521, 0.362 and 0.499 ms (it
measured 0.26 ms on the idle machine). On that machine the lossy link
costs the server somewhat more (more records, at the budget), but take
nothing finer than that from those numbers.

### Sent once, resent when lost

Sending a change in every packet until the client acknowledged it cost a
round trip's worth of packets per change. Sending it once, and again only
after the resend timeout, changes the bytes where entities change now and
then (the benchmark's loopback acknowledges within a few ticks; on real
links with longer round trips the saving grows with them):

| Scenario | Before: payload, on the wire (B/s per client) | After |
|---|---|---|
| 1 in 20 move, 64 KB/s | 63,893, 67,577 (at the budget) | 31,266, 34,891 |
| 1 in 10 move, 64 KB/s | 63,894, 67,578 (at the budget) | 62,006, 65,636 |
| All move, interest rings | 62,256, 65,915 | 62,246, 65,905 |
| All move (no budget, or 64 KB/s) | 571,428 / 63,893 | the same |

Before, the `1 in 20` scenario sat at the 64 KB/s budget, and entities
that moved waited behind ones resent; now it uses half of it. Where every
entity changes every tick there is nothing to resend, and nothing changed.

What sends a record again, once it went:

1. **The entity changed again.** The new state goes as soon as it is due.
2. **Acknowledgements skipped its packet.** The client acknowledges with
   the newest packet it has and a mask of the 64 before it. When it
   acknowledges a packet sent at a later tick but not the one that carried
   the record, that one is lost, and the record goes in the next packet:
   a round trip after the packet that followed it, instead of after the
   timeout. A record sent again since, or already acknowledged in a later
   packet, isn't. On a link that reorders packets, a packet called lost
   may still arrive; each time one does, the server waits a tick longer
   before calling the next one lost (packets sent that many ticks later
   must be acknowledged first), and after 128 acknowledged packets with no
   such case it waits a tick less.
3. **The resend timeout passed** without an acknowledgement: the smoothed
   round trip plus four deviations (above). Each time a timeout sends
   records again and nothing was acknowledged since, the next one waits
   twice as long, up to 4 s. The first acknowledgement brings it back to
   the plain timeout, and everything overdue goes at once.

The engine's suite measures both on its scripted network, a link of three
ticks each way (50 ms) at 60 Hz. The numbers are exact: the same on every
run and every machine.

A lost packet. Five entities change, and the packet carrying them is lost
(jitter of up to a tick each way, a round trip of 8.6 ticks):

| | Resent after |
|---|---|
| Before: on the timeout only | 12 ticks (the timeout) |
| After: when an acknowledgement skips it | 9 ticks |

On a link that keeps its timing steady the two meet: the timeout is the
round trip plus a tick, and so is the acknowledgement of the next packet.
The more the round trip varies, the longer the timeout and the more the
early resend saves. Over 20 seconds, with 100 counters each changing one
tick in 20 and losing 10% of the packets each way, ticks from a change on
the server to the client showing it:

| Link | Mean | Slowest 5% (the lost ones), mean | 99th percentile | Most | Server bytes/s |
|---|---|---|---|---|---|
| Jitter up to a tick: before | 6.02 | 20.0 | 24 | 44 | 1,630 |
| Jitter up to a tick: after | 5.80 | 18.2 | 23 | 37 | 1,632 |
| Jitter up to 3 ticks (packets overtake): before | 7.45 | 24.9 | 24 | 44 | 1,626 |
| Jitter up to 3 ticks: after | 7.17 | 22.7 | 25 | 40 | 1,635 |
| Jitter up to 3 ticks: after, without the reordering tolerance | 6.95 | 20.6 | 21 | 36 | 1,748 |

Without the tolerance, a link that reorders packets gets the late ones
resent too: 7.5% more bytes for the freshest replicas. The tolerance keeps
the bytes within 0.6% of before.

An outage. The link (round trip 8 ticks) is cut for 9 s, after five
entities changed and one was despawned:

| | Resent at (ticks into the outage) | Records, despawns sent | Server bytes/s | Client caught up after the link was back |
|---|---|---|---|---|
| Before | every 9 ticks | 300, 60 | 833 | 11 ticks |
| After | 9, 27, 63, 135, 279, 519 (the waits 9, 18, 36, 72, 144 and 240 ticks: 4 s) | 35, 7 | 565 | 18 ticks |

Most of the bytes left are the packet headers the server still sends
every tick (about 540 B/s). After the outage, the backed-off server waits
for the first acknowledgement before sending again: the client is up to
date a round trip plus the trip there (12 ticks) later, plus the 0.1 s
it shows interpolated values behind. Before, it was a matter of luck where
the 9-tick cycle stood: from 5 to 13 ticks for the data, plus the same
0.1 s. Changes made during an outage still go once, at once; only the
resends wait, so the budget is left for new changes.

The server's CPU per tick, in the `1 in 20` scenario, five runs of each,
the two builds alternating (2026-11-21, on the busy machine above): the
medians were 0.204 and 0.210 ms before (runs from 0.189 to 0.267 ms), and
0.138 and 0.160 ms after (0.136 to 0.290 ms): fewer records to encode,
though the busy machine's spread overlaps. In `all move, 64 KB/s` the
two builds didn't differ beyond the noise (0.92 to 0.95 ms both).

### Reading the resends

`resend_stats(peer)` on the server returns a `ResendStats` for that
client (`None` if it isn't one):

| Field | Counts |
|---|---|
| `early_resends` | records sent again at once because the client acknowledged later packets but not theirs |
| `timeout_resends` | records sent again because the resend timeout passed |
| `late_acks` | packets called lost (their records went again early) that were acknowledged after all: resends for nothing, each of which makes the server tolerate more reordering |
| `timeout_ticks` | the resend timeout now, in ticks, backed off |
| `backoff` | resend timeouts in a row with nothing acknowledged; each doubled the timeout |
| `reorder_ticks` | how late, in ticks, a packet may arrive behind later ones before it is called lost |

A resend is a record (a spawn, a state or a despawn) that goes again with
nothing changed since it last went; an entity that changed sends its new
state, which counts as new, not as a resend. A client's own entities go
in every packet and never count.

```rust
use ponko::prelude::*;

fn net_hud(ctx: &mut Ctx) {
    for peer in ctx.net.replication.clients() {
        let Some(r) = ctx.net.replication.resend_stats(peer) else { continue };
        if r.backoff > 2 {
            // Several timeouts in a row: this client hasn't answered for a while.
            let waits = f64::from(r.timeout_ticks) / 60.0;
            println!("{peer:?}: no answer, next resend in {waits:.1} s");
        }
    }
}
# let _ = net_hud;
```

Why a struct of its own and not more fields of `PeerStats`: `PeerStats`
is not `#[non_exhaustive]`, so a new field would break code that builds
one with a struct literal (a test's expected value, say), which the
[stability promises](../../STABILITY.md) rule out, and its C mirror
(`PonkoReplicationPeerStats`) would change layout. `ResendStats` is
`#[non_exhaustive]`, so it can grow; C and C++ read it with
`ponko_ctx_replication_resend_stats` and `ctx.replication_resend_stats(peer)`
(a `PonkoReplicationResendStats` of its own).

The engine's suite checks the counts on its scripted network: the five
records of a lost packet count as five early resends (five timeout
resends with the early rule off); on a link losing 10% and reordering up
to three ticks, some packets are late rather than lost and the server
learns a tolerance of a few ticks (with the tolerance off, more than twice
as many late acknowledgements and no tolerance); over a 9 s outage each
try sends the five records and the despawn again on the timeout,
`backoff` counts the tries, `timeout_ticks` stops at 4 s (240 ticks at
60 Hz), and the first acknowledgement brings both back.

### Only what changed

The server doesn't read and compare every replicated entity every tick.
Once a client has joined, it tracks the changes of the replicated
components ([change detection](ecs.md#change-detection), on the server only) and each tick
reads only:

- entities whose replicated components changed, or lost one;
- new entities, and ones handed to another owner;
- a rolling sweep of 1/64 of the rest, so that a write the world can't see
  (through a raw pointer, without `mark_changed`) still goes out, about a
  second later at 60 Hz.

For each client it then compares only those (and the ones whose baseline
the client just acknowledged) with what that client has. A world the game
replaces is read in full once.

So change only what changes: a mutable borrow counts as a change, written
or not. A query over `&mut Mover` marks every mover; one that reads with
`&Mover` and calls `get_mut` on the ones that move marks only those.
Values borrowed but left alone are still found equal and not sent, but
they cost the read.

The benchmark's `1 in 20 move, 64 KB/s` scenario moves 5% of the 1,000
entities each tick and borrows only those. Server CPU per tick, three runs
of each, the two builds alternating (2026-11-21, on the same M4 Max while
other builds kept it busy, load average about 90, so the absolute times
are higher than in the table above):

| Server tick, 5% change | Medians of the three runs |
|---|---|
| Before: every entity read and compared | 0.763, 0.736, 0.747 ms |
| After: only the changed ones | 0.578, 0.577, 0.585 ms |

The traffic is identical, byte for byte. With every entity changing every
tick (`all move, no budget`), the tracking's extra work didn't show beyond
the noise of the busy machine (2.23 to 2.48 ms before, 2.31 to 2.32 ms
after). The tracking costs 8 bytes per replicated component of each
entity on the server, and nothing on clients.

## Checks for a person

The tests above cover every transport in one process, and a browser
smoke test (`web/smoke/replication.mjs`, see its
[README](../../web/smoke/README.md#replication)) runs a dedicated server
and a headless browser tab of the web build: the tab must join, hold its
own player and see the replicated orbs move. Two things still need a
person:

- **A real browser, played.** Run `cargo run --example net_arena -- server`,
  build the example for the web ([Web builds](web.md)) and open it: it
  joins `ws://<the page's host>:7778` (or the server its page names with
  `?server=ws://host:port`). Join a native client too. Both should see each
  other move smoothly, scores should match, and a wave from either should
  show on both.
- **Two machines.** Run `net_arena server` on one, and
  `net_arena join <its address>:7777` on the other (and a second client
  anywhere). Move, collect and wave. Then pull the network cable or turn
  off Wi-Fi for a couple of seconds: the other players freeze, your own
  keeps moving, and everything catches up within a second of reconnecting.

## From C and C++

C and C++ games declare replicated components field by field (an offset,
a type and a quantization for each), mark entities, read the role, net ids
and replicas, and set viewers. That is `ponko_ctx_replication`,
`ponko_ctx_replicate` and the calls above, or `ctx.replicated<T>()`,
`ctx.replication()` and `ctx.replicate()` in C++. The budget and the
interest rings are arguments of `ponko_ctx_replication`.

Inputs, prediction, events and smooth motion work the same way, by name,
after `ponko_ctx_replication` in setup and the same on every side:

| C | C++ | Rust |
|---|---|---|
| `ponko_ctx_replication_input` | `ctx.replication_input<I>()` | `Replication::input` |
| `ponko_ctx_replication_predicted` | `ctx.replication_predicted<S, I>()` | `Replication::predicted` |
| `ponko_ctx_replication_interpolated` | `ctx.replication_interpolated()` | `Replication::interpolated_with` |
| `ponko_ctx_replication_event` | `ctx.replication_event()` | `Replication::raw_event` |
| `ponko_ctx_replication_position` | `ctx.replication_position<T>()` | `Replication::position` |
| `ponko_ctx_set_replication_input`, `ponko_ctx_replication_input_of` | `ctx.set_replication_input()`, `ctx.replication_input_of<I>()` | `set_input`, `input_of` |
| `ponko_ctx_replication_send` | `ctx.replication_send()` | `to_server_raw`, `to_client_raw`, `to_clients_raw` |
| `ponko_ctx_net_events` | `ctx.net_events()` | `ctx.events.read::<NetEvent<RawEvent>>()` |

```cpp
{{#include ../../examples/cpp/guide.cpp:replication_inputs}}
```

- The input is one plain struct of which the declared fields travel,
  every tick and redundantly, so a lost packet costs nothing.
- The prediction step gets the component's bytes and one input's bytes.
  It must give the same result for the same bytes on every machine, and
  must not call into the ctx: it runs on the server for each input and on
  the owning client to predict and to replay after a correction.
- An interpolated component's float fields move between the values
  received, `delay` in the past, angle fields (`PONKO_FIELD_ANGLE`) the
  short way round; at most 32 float fields. Its other fields are set when
  they arrive.
- Events are [raw events](#raw-events-bytes-you-encode): bytes of a
  kind, numbered from 0 in the order they were declared (Rust
  `RawEvent::kind`, `Replication::raw_event_kind`). `ponko_ctx_net_events` hands every event that arrived since the
  system last read them as one array, valid until the system returns.
  It works inside `ponko_query` callbacks (not `ponko_query_par`), so read
  it once per system: a second read gives only newer events.
- `ponko_ctx_replication_position` makes interest use 2 or 3 floats of a
  component instead of the entities' transforms; an entity without that
  component reaches every client.
