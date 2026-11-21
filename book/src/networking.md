# Lockstep, rollback and your own protocol

This chapter is for games that don't take the default path,
[replication](replication.md) (see [pick a model](multiplayer.md)):
deterministic **lockstep** and **rollback**, and **application messages**
with the building blocks for a protocol of your own.

## Lockstep

Lockstep is how RTS and simulation games do multiplayer: every player runs
the whole simulation, and only **inputs** cross the network. As long as
everyone applies the same inputs in the same order to the same start,
everyone has the same world, whether there are 10 units or 100,000. A
tick costs the same bandwidth either way: about 50 bytes per player.

Ponko's simulation is built for it already: a fixed clock, seeded
randomness per (seed, tick, system, entity), and deterministic queries and
physics. Lockstep adds the session: 2–8 players, plain UDP (WebSocket
for [browser players](#in-the-browser)), no server to run.

### Host and join

One player hosts, the others join by address (`join_web` and `--web` are
for [browser players](#in-the-browser)):

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/lockstep.rs:session}}

{{#include ../../examples/lockstep.rs:play}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:lockstep_setup}}
```

</div>
</div>

The host is player 0; the others get 1, 2, ... in the order they join. The
game starts when everyone is in. Until then, and whenever a player's input
is late, the simulation waits: frames (drawing, UI) go on, ticks don't.

### Inputs out, inputs in

A player's input is whatever you send with `ctx.net.set_input(&value)`: a
serde type in Rust (a plain struct in C++, up to 1024 bytes). Set it in a
frame system from the keys and mouse; the session sends it. Then
simulation systems read **every** player's input for the tick and apply
it:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/lockstep.rs:input}}

{{#include ../../examples/lockstep.rs:simulate}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:lockstep_input}}
```

</div>
</div>

The last input set before a tick goes into it and is then cleared, so set
it every frame for held keys. Without a session, `ctx.net` is offline: one
player whose input applies on the next tick. The same code runs alone and
online, so you can build the single-player game first.

### Input delay

Input set at tick `t` is used at tick `t + input_delay` on every machine
(default 3 ticks, 50 ms at 60 Hz). That's the time it has to reach the
others; as long as the round trip is shorter, nobody ever waits. A longer
delay hides a worse connection but makes controls feel laggier. Set it on
the host: `Net::host(addr, players)?.input_delay(5)`.

Packets get lost; nobody notices. Every packet repeats all inputs the other
side hasn't acknowledged yet, so the next one fills the gap.

### Staying in sync

Lockstep only works if the simulation really is deterministic. The rules:

- **Change the world only in simulation systems**, driven by inputs. Frame
  systems may read it and draw it, never change it.
- **Same start**: the same setup and the same `Game::seed` everywhere (a
  joining player with another seed is refused).
- **No outside state**: no wall clock, no `HashMap` iteration order, no
  thread-dependent results. Use `ctx.random()` for randomness;
  `par_for_each` and the spatial grid are deterministic.
- **Physics across platforms**: rapier gives the same result on the same
  platform. Players on different OSes or CPUs need the
  `physics-determinism` feature.

When something slips through, Ponko tells you. Every `checksum_every`
ticks (default 60) the peers compare a hash of every
[reflected](reflection.md) component and resource: the same
`ponko::reflect::world_hash` your own determinism tests can use, taken part
by part (`WorldHashParts`), read in place without making JSON. On a
mismatch, `ctx.net.desync()` reports the tick, the player and **which
components differ**, so you know where to look:

```text
ponko lockstep: desync with player 1 at tick 119: Unit differ
```

Only reflected components are compared, so reflect the ones your
simulation changes.

### Status

`ctx.net.state()` says where the session is: `Connecting`, `Running`, or
`Lost` (a player was silent for longer than the timeout, 10 s by default,
or the host refused us; `ctx.net.error()` says why). `ctx.net.player()` and
`players()` say who's who; `stats()` counts packets and bytes sent.

### Over Steam

With the `steam` feature, Steam carries the packets: players meet in a
friends-only Steam lobby, invites come from the Steam overlay, and Steam's
relays get through any router, with no port forwarding and no addresses
to type. The lockstep itself is the same, so nothing else in the game changes.

```toml
ponko = { version = "0.1", features = ["steam"] }
```

The host makes the lobby and invites; a friend either launches the game
from the invite (Steam passes `+connect_lobby`) or accepts it while the
game waits:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/lockstep.rs:steam}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:steam_setup}}
```

</div>
</div>

While connecting, `ctx.net.steam_lobby()` is the lobby, and
`ctx.net.steam_invite()` opens the overlay's invite dialog (the example
does that on the I key). `steamworks` is re-exported, so the same `Client`
also gives you achievements, stats and the rest of Steam.

Shipping: the game needs Steam running, its Steam app id (480, Valve's
test app "Spacewar", works for development) and Valve's `libsteam_api`
library next to the executable (`libsteam_api.dylib`, `libsteam_api.so`
or `steam_api64.dll`). Cargo puts it there for `cargo run`, examples and
tests; when you package the game, copy it from `target/<profile>/`. For C
and C++, build ponko_ffi with `--features steam`.

Try it with two Steam accounts on two machines:
`cargo run --example lockstep --features steam -- steam` on one, press I
to invite, and `cargo run --example lockstep --features steam -- steam-join`
on the other, then accept the invite.

### In the browser

A web build can't open UDP sockets, so browser players connect over
**WebSocket**, which every browser has. The host keeps its UDP socket for
desktop players and also listens for browsers with `accept_web`; a web
build joins with `Net::join_web` and the host's URL. Desktop and browser
players are in one session and one player count, and the lockstep itself
is the same, so nothing else in the game changes (port 0 picks a free port;
`web_addr()` says which):

```rust
# use ponko::net::Net;
// The host (a desktop game, or a headless server): UDP on 7777 for
// desktop players, WebSocket on TCP port 7778 for browser players.
let host = Net::host("0.0.0.0:7777", 4)?.accept_web("0.0.0.0:7778")?;
// The web build: the page's host, port 7778.
let browser = Net::join_web("ws://127.0.0.1:7778")?;
# let _ = (host, browser);
# Ok::<(), std::io::Error>(())
```

The lockstep example plays this way:
`cargo run --example lockstep -- host 7777 --players 3 --web 7778`, then
`web/build.sh lockstep`, serve `target/web/lockstep` and open it in two
browser windows (its page joins `?server=ws://...`, or port 7778 of the
page's own host). A desktop game can join over WebSocket too
(`join-web ws://127.0.0.1:7778`), for networks that only let web traffic
through.

Things to know:

- **Why WebSocket.** It works in every browser and needs nothing on the
  server but a TCP port. It is reliable and ordered, which costs nothing
  here: a lockstep tick waits for every player's input anyway, so an
  unreliable channel would make no tick run sooner. The packets are the
  same as over UDP, one WebSocket message each. An unreliable browser
  transport would help [messages](#application-messages-rust) more than
  lockstep: [WebRTC data channels](webrtc.md) (feature `webrtc`) carry
  message endpoints peer to peer, with real unreliable lanes, also between
  two browsers.
- **`https://` pages need `wss://`.** The host speaks plain `ws://`. A page
  served over HTTPS may only open `wss://` URLs, so put a TLS proxy (Caddy,
  nginx) in front of the WebSocket port and join `wss://your.host/...`.
  Connections are unauthenticated, like UDP sessions.
- **Rust only, for now.** C and C++ hosts take UDP players only, and a C
  or C++ game built for the web can't join over WebSocket.
- **Hidden tabs stop.** A browser gives a tab that isn't shown (behind
  another tab, or minimized) no animation frames, so its player stops
  sending inputs and the session waits for it. Players in one browser
  should use separate windows.

`web/smoke/multiplayer.mjs` in the engine's repository checks it in a
headless browser: two tabs of the web build, a native host and a native
client over UDP play the lockstep example together, and all four must end
up with the same world.

### Testing

`Net::loopback(players)` connects sessions in memory, so a test can run a
whole match headless, one thread per player, and check that every world
ends identical:

```rust
{{#include ../../tests/net.rs:loopback}}
```

### Cost

Measured with `cargo run --release -p ponko_bench -- net`: 10,000 ticks of
a small input-driven sim take 2 ms offline and 8.5 ms as two loopback
peers in two threads (under 1 µs extra per tick, mostly waiting for the
other thread). The guest sends about 48 bytes per tick. At 60 ticks per
second that's under 3 KB/s per player, whatever the size of the world.

Try it: `cargo run --example lockstep -- host 7777` in one terminal and
`cargo run --example lockstep -- join 127.0.0.1:7777` in another.


## Rollback

Lockstep waits: a tick runs once every player's input for it is in, and
input delay hides the round trip. That suits strategy games; a fighting or
racing game wants the press to show on the next frame whatever the ping.
**Rollback** (GGPO-style) runs every tick at once: inputs that haven't
arrived are **predicted** (each player keeps pressing what they pressed
last), and when a real input differs, the game **loads** the state saved
before that tick and **re-simulates** the ticks since with the real
inputs. It costs a save every tick and a few extra ticks when a guess was
wrong, so it fits games whose state is small and whose tick is cheap.

`ponko::net::Rollback` is the session: input delay, prediction, the saved
states, when to roll back, a stall when a peer falls too far behind, and
the packets that carry inputs between peers. The game implements
`RollbackGame`: save and load **its own** state, and advance one tick from
every player's input. What the state holds is up to the game; here, an ECS
world whose bodies are copied component by component:

```rust
use ponko::net::{Rollback, RollbackGame};
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy)]
struct Body { pos: Vec2, vel: Vec2 }

/// One player's input for one tick. `Default` is "nothing pressed".
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
struct Pad { x: i8, jump: bool }

struct Match { world: World, tick: u64 }

/// What a save holds: every body, and the tick.
#[derive(Default)]
struct Saved { tick: u64, bodies: Vec<(Entity, Body)> }

impl RollbackGame<Pad> for Match {
    type State = Saved;
    fn save(&mut self, into: &mut Saved) {
        into.tick = self.tick;
        into.bodies.clear(); // reuses the buffer: no allocation per tick
        into.bodies.extend(self.world.query::<(Entity, &Body)>().iter().map(|(e, b)| (e, *b)));
    }
    fn load(&mut self, from: &Saved) {
        self.tick = from.tick;
        for &(e, b) in &from.bodies {
            *self.world.get_mut::<Body>(e).unwrap() = b;
        }
    }
    fn advance(&mut self, inputs: &[Pad]) {
        // Everything a tick reads comes from the state and `inputs`; the
        // randomness is seeded by the tick, so a re-simulation repeats it.
        let mut rng = Rng::new(self.tick);
#         let _ = (inputs, rng.u64());
        self.tick += 1;
    }
}

# let mut world = World::new();
# let mut game = Match { world, tick: 0 };
# let pressed = Pad::default();
# fn send_to_peers(_: &[u8]) {}
# let received: Vec<Vec<u8>> = Vec::new();
// Player 0 of 2: two ticks of input delay, at most 8 ticks predicted.
let mut session = Rollback::new(2, 0).input_delay(2).max_prediction(8);
// Every fixed tick: what arrived, the tick, and a packet out.
for packet in &received {
    session.receive(packet);
}
session.advance(&mut game, pressed);
send_to_peers(session.packet());
```

- **Packets:** `packet()` is one datagram (at most 1,400 bytes) for every
  other peer; send it each tick over any unreliable transport, such as
  [messages](#application-messages-rust) on `Lane::Unreliable`. It repeats
  every input the peers haven't acknowledged, so a lost packet costs only
  bytes. `receive` takes theirs, in any order, duplicates included, and
  ignores packets from a session with other settings.
- **Input delay** (default 2 ticks, at most 16): input given now is used
  that many ticks later on every peer. Two ticks make a 33 ms round trip
  invisible at 60 Hz; 0 rolls back every time an input changes. Every
  peer must use the same delay.
- **Prediction window** (`max_prediction`, default 8, at most 32): a peer
  runs at most that many ticks past the last tick it has everyone's input
  for, then `advance` returns `Advance::Stalled` until packets come. It is
  also the deepest rollback, so the worst tick costs one load and
  `max_prediction + 1` saves and ticks.
- **Desyncs:** give `RollbackGame::checksum` a hash of a saved state and the
  peers compare it every `checksum_every` confirmed ticks (default 60);
  `desync()` names the first tick and player that differ.
- `stats()` counts ticks, predicted ticks, rollbacks, re-simulated ticks,
  the deepest rollback, stalls and bytes sent; `confirmed_frame()` is the
  last tick that will never be rolled back.
- **Free when unused:** nothing runs unless a game makes a `Rollback`: no
  thread, no socket, no work in the engine's loop. The game calls it from
  its own fixed tick.

`tests/net_rollback.rs` plays two and four peers over a scripted network
with 4–8 ticks of latency (so packets reorder), 10% loss and duplicates:
every peer ends in exactly the world an offline run with the same inputs
reaches, and the confirmed checksums agree on every tick.

### Rolling back a whole `Game`

`ponko::net::GameRollback` is the same session with the engine as the
game: start it with `ctx.net.start_rollback`, and from then on every
simulation tick runs through it. Before each tick the engine saves the
world (every component, the entity allocator, the resources you registered),
both physics worlds, the events and the tick; when an input arrives that
differs from the prediction, it loads the state saved before that tick and
runs the simulation systems again up to the present, within the frame.
Simulation systems read every player's input for the tick; frame systems
give this player's input and carry the packets:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
struct Pad { x: i8, jump: bool }

/// `Clone`: a rollback copies every component.
#[derive(Component, Clone, Copy)]
struct Fighter { player: usize, x: f32 }

/// A simulation system: every player's input for this tick, real or predicted.
fn walk(ctx: &mut Ctx) {
    let Some(pads) = ctx.net.rollback_inputs::<Pad>() else { return };
    ctx.world.query::<&mut Fighter>().for_each(|f| f.x += f32::from(pads[f.player].x));
}

/// A frame system: this player's input out, the peers' packets in.
fn network(ctx: &mut Ctx) {
#     fn read_pad(_: &Ctx) -> Pad { Pad::default() }
#     fn send_to_peers(_: &[u8]) {}
#     fn received() -> Vec<Vec<u8>> { Vec::new() }
    let pad = read_pad(ctx);
    let Some(session) = ctx.net.rollback::<Pad>() else { return };
    session.set_input(pad);
    send_to_peers(session.packet());
    for packet in received() {
        session.receive(&packet);
    }
}

Game::new("duel", 640, 360)
    .sim_system(walk)
    .system(network)
    .run_headless(10, |ctx| {
        ctx.world.spawn((Fighter { player: 0, x: 0.0 },));
        ctx.world.spawn((Fighter { player: 1, x: 100.0 },));
        // Player 0 of 2, started on every peer at the same tick of the same world.
        ctx.net.start_rollback(ponko::net::GameRollback::<Pad>::new(2, 0).input_delay(2));
    });
```

The session's settings, packets, stalls and stats are `Rollback`'s
(above). What the engine does for you, and what stays yours:

- **Components** must be `Clone`; `#[derive(Component)]` notices, and a
  hand-written `impl Component` (or a generic one) is registered with
  `World::register_clone`. A tick panics with the component's name
  otherwise. C and C++ components are copied as bytes unless they have a
  destructor. **Resources** are saved only if registered with
  `World::snapshot_resource`; others (a score shown in the UI, a sound
  handle) keep their values through a rollback.
- **Entity ids:** a load restores the allocator too, so the next spawns,
  direct or through commands, get the ids they got the first time. While a
  session runs, the ids commands set aside are tidied after every tick
  instead of once a frame, so they don't depend on how many ticks a frame
  ran. Keep the same `ctx.world` while a session runs.
- **Events** sent in a tick that runs again reach simulation systems as
  they did the first time, including those read a tick later. Frame
  systems keep their read positions, so they aren't shown events again;
  events a frame system sent to simulation systems after the loaded tick
  are forgotten (send input through the session instead).
- **Sounds** started while a tick runs again are dropped: they were heard
  the first time. `ctx.net.is_resimulating()` tells your own one-shot
  output (particle bursts, rumble, log lines) to skip too. A wrong
  prediction's sound has been heard; a tick that runs again doesn't add
  the corrected one.
- **The clock:** the sim tick goes back with the state, so `ctx.random()`,
  `sim_time()` and `every(n)` systems repeat exactly. Timers and cooldowns
  are components or resources, so they are restored with them.
- **Physics:** `ctx.physics` and `ctx.physics3d` are saved and loaded whole
  (`save_state` / `load_state`, also callable yourself): bodies, colliders,
  joints, contacts, islands, the broad phase and the links to entities. A
  loaded world steps bit for bit like the original; a load builds a fresh
  rapier pipeline, as the joint solver keeps indices there.
- **Not restored:** `Changed` filters see every restored component as
  changed on the next run (rebuild derived data from them, don't count
  them as game events); despawns undone by a load aren't reported to
  `removed`; component hooks don't run on a load. The UI, the camera and
  everything frame systems keep are not part of the state.
- **Desyncs:** checksums are `world_hash`es (every reflected component and
  resource) of the saved worlds, taken only on the ticks the session
  compares (`checksum_every`).
- A stalled tick still counts as one of `run_headless`'s ticks. A game uses
  rollback or lockstep, not both.

`tests/game_rollback.rs` runs two headless games on two threads over a
scripted link (latency, jitter, 10% loss, duplicates; and one slow enough
that the peers stall): the game spawns and despawns through commands,
sends events read a tick later, plays sounds and draws from
`ctx.random()`. Every per-tick checksum agrees, and each peer's world at
the end has the world hash of an offline run at that tick, with and
without 2D physics.

### What a rollback must capture

Everything a tick reads that an earlier tick wrote. In this engine:

| State | Must the game save it? | Can it? |
|---|---|---|
| The tick | Yes: it seeds the randomness | A field in the state |
| `ctx.random()` | No: its numbers come from (seed, tick, system, entity), so a re-simulated tick draws the same ones | — |
| An `Rng` the game keeps | Yes | `Rng` is `Clone` |
| Components and resources | Those that change | Copy them, as above; `World::save` / `World::load` work but cost far more (below) |
| Spawned and despawned entities | Yes, if the tick spawns | `World::save_snapshot` / `restore_snapshot` keep the allocator, so later spawns get the same ids (`GameRollback` uses them). Not through `World::load`: it resets the entity allocator, so the next spawn can get another id than on a peer that didn't roll back (`a_world_load_does_not_restore_which_ids_new_spawns_get`) |
| 2D and 3D physics (rapier) | Yes: bodies, colliders, joints, contacts, islands, the broad phase | `ctx.physics.save_state` / `load_state` (and `ctx.physics3d`'s): a loaded world steps bit for bit like the original. Your own rapier world: every part is `Clone`, but load it with a fresh `PhysicsPipeline`, as the joint solver keeps indices there |
| Events | Only those read in a later tick than they were sent | `GameRollback` restores the event store; with `Rollback` alone, keep such events in the state |
| Timers and cooldowns | Yes | Fields or components in the state |
| Sounds and effects | No: they are output | `GameRollback` drops sounds of ticks that run again; otherwise play them only for confirmed ticks, or remember (tick, what) and skip repeats |
| Derived data rebuilt every tick (the spatial grid) | No | — |

### What it costs

`cargo run --release -p ponko_bench -- rollback/` on an Apple M4 Max
(14 cores), one run on an idle machine (2026-11-20): medians. A "tick" is the
test's game (players pushing a crowd); physics is a pile of 10 px boxes in
rapier that has settled for a second.

| Per operation (ms) | 100 | 1,000 | 10,000 |
|---|---|---|---|
| Typed copy, save | < 0.001 | 0.002 | 0.054 |
| Typed copy, load | < 0.001 | 0.005 | 0.052 |
| `World::save` (JSON) | 0.44 | 3.7 | 34 |
| `World::load` (JSON) | 0.028 | 0.27 | 2.8 |
| One tick of the game | < 0.001 | 0.001 | 0.010 |
| Rapier state, save (clone) | 0.038 | 0.40 | 15 |
| Rapier state, load | 0.037 | 0.37 | 6.2 |
| One rapier step | 0.019 | 0.20 | 8.2 |

The worst tick is one load plus `max_prediction + 1` saves and ticks;
with the default window of 8:

- **Without physics, typed copies:** 10,000 entities, about 0.6 ms. A
  whole two-peer session of 300 ticks with 1,000 entities over a 4–8 tick,
  10%-loss link took 4.3 ms for both peers (about 2.1 ms each) against
  0.32 ms for one offline run: 475 ticks re-simulated across both peers,
  and a save every tick.
- **`World::save` and `World::load`:** 1,000 entities already take about 33 ms
  (9 saves of 3.7 ms), and allocate about 22,000 times per save; one save
  of 10,000 entities takes 34 ms. Too slow for every tick, and they don't restore entity ids
  (above).
- **With 2D physics:** 1,000 bodies, about 6 ms (a third of a 60 Hz
  frame); 100 bodies, under 1 ms. At 10,000 bodies one step takes 8 ms
  and one save 15 ms, together more than a 60 Hz frame, so there is no time
  to re-simulate.

The whole-game numbers come from a second run of the same command, on a
busy machine (load average 90 to 150 on the 14 cores, 2026-11-21): medians
of 5 runs, and of 3 with physics, where the spread was wide (the fastest
run is given too). `snapshot` is `World::save_snapshot` and
`restore_snapshot` of the test's world (two players and a crowd, `Pos` and
`Vel`) into a reused snapshot: no allocation after the first save. A
`game` run is a whole `Game` running 300 ticks headless, offline or
through a `GameRollback` whose remote player's input arrives 4 frames late
and changes every 4 ticks (133 ticks run again); the engine saves before
every tick and hashes the world every 60.

| Per operation (ms) | 100 | 1,000 | 10,000 |
|---|---|---|---|
| World snapshot, save | < 0.001 | 0.001 | 0.005 |
| World snapshot, restore | < 0.001 | < 0.001 | 0.005 |
| `Game`, 300 ticks offline | 0.61 | 0.89 | 4.0 |
| `Game`, 300 ticks through a session | 1.6 | 2.5 | 9.3 |
| `Game` with 2D physics, offline | 4.9 | 120 (fastest 26) | — |
| `Game` with 2D physics, session | 114 (fastest 15) | 351 (fastest 299) | — |

- **Without physics** a session costs about 0.02 ms a tick at 10,000
  entities over the offline game: the snapshot is a copy of the component
  columns, far below the 1 ms a save and load of 10,000 entities may take.
  It is not incremental: copying only the archetypes that changed would
  need a change stamp per archetype, and the whole copy is already a few
  microseconds.
- **With physics** the save before every tick dominates: rapier's sets are
  cloned (about 8,000 allocations for 1,000 bodies), about 1 ms a tick at
  1,000 bodies, so up to about a thousand bodies fit a 60 Hz frame with
  room to re-simulate, as with `Rollback` alone.

### The decision

**Shipped:** `Rollback` and `RollbackGame`, for games that own a small
state, and `GameRollback`, which rolls back a whole `Game` from the
engine's loop: a typed world snapshot (`World::save_snapshot`, which keeps
entity ids and the archetype layout, so queries visit entities in the same
order), the physics services' `save_state` and `load_state`, the event
store, and a loop that runs the simulation systems again from a saved tick.
Without physics it handles tens of thousands of entities; with physics,
about a thousand bodies (rapier's state is cloned every tick).

Lockstep stays the choice for big simulations (its cost doesn't grow with
the world), and client prediction for server-authoritative games.
`Rollback` and `GameRollback` are Rust-only: they are generic over the
game's own input type (and `Rollback` over its state), which C and C++
can't name; a C binding would take inputs as fixed-size bytes.
`WorldSnapshot` is Rust-only too (see "Still Rust-only" in the C and C++
chapter).

## Which transport

Everything networking is behind `ctx.net` (and in the `ponko::net` module):

| Pick | For | Start with |
|---|---|---|
| [Replication](replication.md) | Server-authoritative games: declare components, mark entities, the engine syncs them (deltas, interest, budget, prediction, events) | `Game::replication(Replication::new()...)` with `Game::messages(endpoint)`, then `ctx.net.replication` |
| Lockstep (above) | Deterministic simulations with 2–8 players: only inputs cross the network | `Game::lockstep(net)`, then `ctx.net.set_input` and `ctx.net.input` (`ctx.net.lockstep`) |
| [Messages](#application-messages-rust) | Everything else: your own packets on reliable and unreliable lanes | `Game::messages(endpoint)`, then `ctx.net.messages` |
| [Snapshots](#replicating-state-over-messages) | Server-authoritative worlds with your own packet format, on top of messages | `ctx.net.snapshots` and `ctx.net.interest` |
| [Rollback](#rollback) | Small deterministic games that must answer at once (fighting, racing): predict, roll back, re-simulate | `Rollback::new(players, me)`, a `RollbackGame`, packets over messages |

Lockstep and messages are independent and can run side by side. A
single-player game has an offline lockstep session and nothing else.

## Application messages (Rust)

For client/server games with their own packet format, give the game a
`NetMessages` endpoint with `Game::messages` (or set `ctx.net.messages`
later, from a menu). The engine polls it once per frame before any system
runs, also while the simulation is paused, so systems only read events and
send. This is independent of lockstep:
there is no simulation barrier and the engine does not synchronize worlds.
A host accepts up to `max_clients` remote clients. Joining clients address
`PeerId::HOST`; hosts use the identifiers from `MessageEvent::Connected`.
The host can relay messages explicitly, or each P2P participant can host an
endpoint and join the others. Peer identifiers belong to one endpoint.

`Lane::Reliable` delivers complete messages once, in order, while the
connection survives. `Lane::Unreliable` never retries and discards messages
older than the most recently delivered message on that peer's lane.
Ordering is independent in each direction. Renet handles fragmentation and
reassembly for both lanes. Each application payload is limited to 1 MiB;
an unreliable fragmented message is lost if any fragment is missing.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/net_messages.rs:messages}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:messages}}
```

</div>
</div>

`main` hosts in one game and joins from another:

```rust
{{#include ../../examples/net_messages.rs:play}}
```

Run the self-contained UDP example with
`cargo run --example net_messages --no-default-features`.
Browser clients join over WebSocket, as in [lockstep](#in-the-browser): the
host adds `.accept_web("0.0.0.0:7778")?` to `NetMessages::host`, and the web
build joins with `NetMessages::join_web("ws://host:7778")`. A WebSocket
delivers everything in order, so `Lane::Unreliable` messages arrive too,
only no sooner than reliable ones.
If the engine's poll fails (a socket error, or events left unread),
`ctx.net.message_error()` says why until a poll works again. An endpoint
outside a game (a lobby server, a test) is polled by hand with `poll()`
once per frame; the C and C++ endpoint is polled by hand too.
`send(peer, lane, bytes)` copies the payload into a bounded channel queue;
`send_serde` encodes JSON and `MessageEvent::decode::<T>()` decodes it.
`send_to_all` targets connected peers and can partially succeed if a later
peer's queue is full. The application remains responsible for its packet
schema, authority and version negotiation. The [replication
helpers](#replicating-state-over-messages) below cover the common pieces
without dictating a format.

Drain `events()` every frame, then apply any sends after dropping the
iterator (it borrows the endpoint); `next_event()` takes one at a time and
lets you send in between. Events include connections, complete
messages, and disconnections with diagnostic reasons. The default silence
and connection timeout is ten seconds; configure it with `.timeout(duration)`.
The clock starts when you first poll the endpoint, so startup work before
that call uses none of the timeout. The joining endpoint attempts its first
hello immediately. After that, each poll advances protocol time by at most
half the configured timeout (minimum step limit of one nanosecond). This
gives peers time to respond after a stalled frame. The endpoint checks
for incoming packets before testing for silence. A peer that stays silent
across polls still times out.
`disconnect(peer)` discards pending messages, sends three best-effort leave
notifications and reports a local event. If those notifications are lost,
the remote detects the timeout. Dropping an endpoint is detected by timeout.
A full host refuses new connections with a disconnection event.

Each Renet lane has a 4 MiB budget per direction per peer; a full send queue
returns `WouldBlock` so the application can retry or drop its update.
Incoming event storage is bounded to about 5 MiB and 4096 message events,
with a small allowance for connection events during a poll. Polling returns
`WouldBlock` if events have not been drained; continued receive pressure can
exhaust a reliable receive channel and disconnect that peer. Polling handles
at most 256 incoming datagrams per call. `stats()` counts datagrams and bytes,
including retransmissions and control traffic. Buffers are reused for framing,
but owned messages and Renet packet batches allocate; this API does not promise
allocation-free frames.

`loopback(n)` builds an in-memory host and clients, intentionally reversing
packet arrival to exercise reordering. `loopback_lossy(n, drop_one_in)` also
drops every Nth datagram. `poll_elapsed(duration)` advances an explicit clock
for deterministic tests without sleeping, with the same first-poll and
per-poll limits as `poll()`. An endpoint the game polls (`Game::messages`)
counts wall time; `NetMessages::game_clock()` makes it count the game's
own time instead, the time since the last frame, with the same limits, so
a headless test that plays a minute in a fraction of a second still sees
its resends and timeouts happen (see [testing replication](replication.md#test-it)).

UDP sessions use nonblocking `std::net::UdpSocket` on the calling thread;
there is no async runtime or worker thread. They are **unauthenticated and
unencrypted**. Session nonces reject delayed traffic from earlier connections,
but do not authenticate users. Use a trusted network or a secure tunnel;
public Internet authentication, encryption and admission policy belong in a
secure transport. This message API currently supports UDP and memory only.
With feature `steam`, `NetMessages` uses Steam Networking Messages on a
separate channel from lockstep. Steam sends unreliable packets without Nagle;
renet supplies delivery, ordering and fragmentation. Each transport packet is
at most 1,500 bytes, below Steam's message limit. `Lane::ReliableUnordered`
(`PONKO_LANE_RELIABLE_UNORDERED` in C++) retries without ordering messages.

Create one message endpoint per Steam client. The host creates a friends-only
lobby (1–249 clients plus host); join with its lobby id. Poll every frame and
drain the same events as UDP. Peer ids remain session-local; `steam_peer`
returns the authenticated Steam identity. Hosts do not automatically forward
client messages. The protocol metadata is checked before joining the message
handshake; applications should check their own game version before accepting
application messages. Host migration requires creating a new endpoint.

<div class="lang-pair">
<div class="lang-rust">

```rust,no_run
use ponko::prelude::*;
let client = ponko::net::steam::Client::init_app(480).unwrap();
let lobby_id = ponko::net::steam_lobby_from_args();
let mut net = match lobby_id {
    Some(id) => ponko::net::NetMessages::steam_join(&client, id)?,
    None => ponko::net::NetMessages::steam_host(&client, 3)?,
};
net.poll()?;
if lobby_id.is_none() {
    if let Some(lobby) = net.steam_lobby().filter(|l| l.id().is_some()) {
        lobby.set_metadata("version", "1")?;
        lobby.invite()?;
    }
}
# Ok::<(), std::io::Error>(())
```

</div>
<div class="lang-cpp">

```cpp
{{#include ../../examples/cpp/guide.cpp:steam_messages}}
```

</div>
</div>

For a lobby without a message endpoint, use `SteamLobby`. Visibility can be
friends-only, public or private; capacity includes the owner (1–250).
Creation and joining are asynchronous: `poll` runs callbacks and reports
errors, and `id` remains absent until ready. Metadata writes require ownership;
reads use Steam's callback-updated cache. Drop/free leaves the lobby, including
when a pending request completes after the handle was dropped. The existing
`steam_lobby_from_args` and `steam_wait_for_invite` helpers cover overlay invites
accepted before launch or while running. Steam is initialized only by the
application (or the C API's first call with an app id), never by fake relay tests.

<div class="lang-pair">
<div class="lang-rust">

```rust,no_run
use ponko::prelude::*;
let client = ponko::net::steam::Client::init_app(480).unwrap();
let mut lobby = ponko::net::SteamLobby::create(&client, ponko::net::SteamLobbyVisibility::Public, 4)?;
lobby.poll()?;
if lobby.id().is_some() {
    lobby.set_metadata("version", "1")?;
    let members = lobby.members();
    let version = lobby.metadata("version");
    lobby.invite()?;
    lobby.leave()?;
}
# Ok::<(), std::io::Error>(())
```

</div>
<div class="lang-cpp">

```cpp
{{#include ../../examples/cpp/guide.cpp:general_steam_lobby}}
```

</div>
</div>


The protocol layer uses [Renet](https://docs.rs/renet/2.0.0/renet/), whose
transport-independent channels provide retransmission, ordered delivery and
fragmentation without Bevy or an async runtime. `renet_steam` was evaluated,
but its Steamworks 0.12.2 dependency differs from this engine's 0.13.1, and
the engine instead adapts the existing renet packet protocol directly.

## Replicating state over messages

[Replication](replication.md) does everything in this section for you:
it builds on these same pieces and adds the wire format, deltas against
acknowledged baselines and a bandwidth budget. The helpers below are for
a game that keeps its own packet format.

Client/server and co-op games usually don't run lockstep: each player owns
their own motion (and the vehicle they drive), the host owns the rest, and
state travels as snapshots on an unreliable lane. The engine moves the bytes
(`NetMessages`) and gives you small helpers for the parts every such game
rewrites, **without imposing a wire format**: they take decoded values and
return decisions. You keep your own packet codec. None of them touches the
network, so they're plain data structures that work (and test) without a
session, in Rust and from C and C++.

In a Rust game they have a home on `ctx.net`: `ctx.net.snapshots` keeps one
`SnapshotStream<Pose>` per entity id with its duplicate filter (`accept`,
`stream(id).push`, `sample(id, now)`, `remove`), and `ctx.net.interest`
holds the `Interest` you update on your send tick. The engine drives
neither: only the game knows when a packet arrived and who is looking.

### Showing remote things smoothly: snapshot interpolation

Packets arrive unevenly, out of order, twice, or not at all; drawing each one
as it comes makes remote entities jitter. Instead, buffer the snapshots and
draw the entity a little in the past (**100 ms** by default), always between
two snapshots you already have.

A `SnapshotStream` is one remote entity's buffer plus an estimate of the
sender's clock. For each decoded packet, `push` the sender's timestamp, your
own clock and the state; every frame, `sample` your clock:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/net_replication.rs:snapshots}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:snapshots}}
```

</div>
</div>

- **Pose:** a position, a rotation and an optional velocity. Position is
  interpolated linearly, or along a cubic Hermite curve when both
  neighbouring snapshots carry a velocity (smoother through turns, and at low
  send rates). Rotation uses slerp. Rust's `SnapshotStream<T>` and
  `SnapshotBuffer<T>` work for any state you implement `Interpolate` for
  (`f32`, `Vec2`, `Vec3` and `Quat` already are); C and C++ have the pose.
- **Late packets:** when the buffer runs dry, the entity is **extrapolated**
  from the newest snapshot (by its velocity, or the speed between the last two
  snapshots) for at most **250 ms**, then held where the prediction ended,
  never snapped back. `sample` says which one you got (`Interpolated`,
  `Extrapolated`, `Held`), which is handy for showing a "connection problem".
  Rotation is held, not predicted.
- **Out of order and duplicates:** snapshots are stored by timestamp, so a
  late one still lands in the right place and a second copy is ignored
  (`push` tells you which).
- **The clock:** timestamps are on the sender's clock, which is none of
  yours. `ClockSync` learns the offset from the packets alone, no round
  trips: it follows the fastest of the recent packets (the 4th smallest of
  the last 32, so jitter and late packets don't move it, and neither do a few
  freakishly fast ones) and smooths that, so it also follows clock drift.
  A clock that jumps (the sender restarted) is noticed after 8 packets and
  the estimate starts over. Use any clock you like for yours (`ctx.time.elapsed()`),
  the same one for `push` and `sample`.
- **Teleports:** `clear()` the stream when something respawns or teleports,
  or it slides across the map. `reset()` on a new connection.
- **Tuning:** `SnapshotConfig` has `delay` (make it more than the gap between
  your packets plus their jitter), `max_extrapolation` and `capacity`
  (snapshots kept; at least twice `delay` × your send rate).

With the `3d` feature, the `NetPose` component holds a stream and the system
`systems::apply_net_poses` sets the `d3::Transform` position and rotation of every entity
that has one, from `ctx.time.elapsed()`; add it with `Game::system` so entities move
every frame:

```rust
# let mut game = Game::new("t", 800, 600);
game.system(systems::apply_net_poses);
// from the message handler:
//   world.get_mut::<NetPose>(entity).unwrap().stream.push(sender_time, ctx.time.elapsed(), pose);
```

### Who may update what: authority epochs and sequence numbers

With mixed authority, a handoff (the guest takes the wheel of a car the host
was driving) races with updates still in flight from the old owner. An
`Authority` is an owner peer and an **epoch**; a handoff bumps the epoch, and
updates carry the epoch they were sent under, so a late packet from the
previous owner is recognised and dropped (`AuthorityCheck::Stale`) instead of
yanking the car back:

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/net_replication.rs:authority}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:authority}}
```

</div>
</div>

Whoever decides (usually the host) calls `handoff` and announces
`(owner, epoch)` on a **reliable** lane; the others `adopt` it. An update that
overtakes its announcement is `Ahead`: drop it, or hold it until the
announcement arrives. If two peers hand the same entity off at once, `adopt`
breaks the tie in favour of the lower peer id, so everyone ends up with the
same owner.

For unreliable state updates, number them with a wrapping 16-bit sequence.
`seq_newer(a, b)` compares with wrap-around (65535 is followed by 0), and
`LatestWins` is the per-entity filter: it accepts an update only if its
`UpdateStamp { epoch, seq }` is newer than everything accepted before for the
same entity, which drops duplicates, reordered packets and updates from an old
epoch in one call (see `newest.accept(...)` in the snapshot example above).
Forget an entity with `forget` when it's gone, and after a long silence (half
the sequence range, 32,768 updates, is the limit for an unambiguous
comparison).

### Who needs what: interest by distance

Sending every entity to every peer doesn't scale. An `Interest` decides, per
peer, which entities to send and how often, from the position of that
peer's viewer(s) (the avatar, the camera) and the entities' positions. The
rings are `InterestConfig::tiers`; by default entities within 40 units are
sent every tick, within 120 units every 4th tick, and beyond that not at all.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/net_replication.rs:interest}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:interest}}
```

</div>
</div>

Call `update` once per send tick with all viewers and entities. Then
`entries` (Rust: `due` for just the ones to send now) lists what each peer is
interested in, with the ring (`tier`), the distance and a `send` flag, and
`changes` reports entities that **entered** (spawn them on that peer) or
**left** (despawn them, also when the entity is gone from the world). An
entity that has just entered always has `send` set.

- **Hysteresis** (5 units by default): an entity moves to a closer ring only
  once it is that far inside it, and to a farther ring (or out of interest)
  only once it is that far outside. One standing on a boundary doesn't flicker
  in and out, and doesn't cost a spawn and a despawn every other tick.
- **Staggering:** the slow rings send on different ticks for different
  entities (by id), so a ring of 1,000 entities every 4th tick is 250 sends
  per tick, not 1,000 every 4th. Turn it off with `stagger: false`.
- **Scale:** the entities go into a spatial grid first, so a peer only looks
  at those near its viewers, and the buffers are reused. One `update` in a
  release build on a laptop: 1,000 entities and 4 peers took 28 µs, 5,000
  entities and 8 peers 65 µs, 20,000 entities and 8 peers 272 µs (about 350
  entries in range in total); with everything in range of everyone (5,000
  entities, 8 peers, 29,000 entries) 1.4 ms, about 50 ns per entry. A peer
  may have several viewers
  (split screen); an entity counts by its nearest. Positions are 3D; for a
  flat world, use a constant height.
- Peers that have no viewer in an `update` are forgotten, with their entities
  reported as left; call `remove_peer` when one disconnects.

### Your own player at once: client prediction

With the server in charge of the world, a client that sends its input and
waits for the result feels a round trip late (150 ms is enough to feel
sluggish). Instead the client **predicts**: it runs its own input through
the same step the server runs, at once, and keeps the inputs the server
hasn't confirmed. The server answers with its state and the number of the
last input in it; the client compares that with what it predicted for that
input, and only if they differ, starts again from the server's state and
**replays** the newer inputs. A correction moves the player, so the drawn
position **slides** there instead of jumping. As with the other helpers,
the packets are yours.

The step both sides run, for one input:

```rust
{{#include ../../examples/net_prediction.rs:step}}
```

A `Prediction` on the client, an `InputQueue` per client on the server, and
a `Smoothing` for what the client draws:

```rust
{{#include ../../examples/net_prediction.rs:setup}}
```

Every fixed tick the client predicts and sends the inputs the server hasn't
confirmed. Sending all of them (or the newest 16) every tick means a lost
packet costs nothing: the next one repeats it.

```rust
{{#include ../../examples/net_prediction.rs:client_tick}}
```

The server pushes every input it receives (repeats and late ones are
ignored), takes them in order with `pop`, and sends its state with `ack`:

```rust
{{#include ../../examples/net_prediction.rs:server_tick}}
```

The client reconciles each state and draws through the smoothing:

```rust
{{#include ../../examples/net_prediction.rs:reconcile}}
```

`examples/net_prediction.rs` runs this over an in-memory link with 150 ms of
round trip and every 10th packet lost, with a wind on the server that the
client doesn't know about: the player moves on the tick of the input, no
input is lost, every correction is under 0.07 units and slides in no faster
than the wind itself (0.034 units a frame), and the client ends exactly
where the server is.

- **Inputs:** `predict(input, step)` numbers each input with a wrapping
  16-bit sequence and keeps up to 256 unconfirmed ones (`with_capacity`
  for more); `unacked()` lists them. `previous()` and `state()` are the
  last two ticks, to draw between them.
- **Reconciling:** `reconcile(ack, state, step)` returns `Confirmed` when
  the server agrees (compared with `==`: give your state a `PartialEq`
  with a tolerance if the two sides can differ by rounding), `Corrected`
  with the state before the correction and the number of inputs replayed,
  or `Ignored` for a state older than one already reconciled, a duplicate,
  or an input never predicted. `reset` sets the state outright (respawns).
- **The server:** `InputQueue` keeps a client's inputs in order and gives
  each out once. With **one step per input** (`while let Some(..) = pop()`)
  the server's state after input *n* is exactly the client's prediction
  for *n*; a late packet makes the player catch up in one tick (limit the
  inputs per tick if a client could send too many). When the step is the
  engine's (a body or a character stepped with everything else), take
  **one input per tick** and repeat `last()` while none arrives; those
  ticks belong to no input, so the client is corrected after them.
  Inputs that never arrived in any packet are skipped (`stats().skipped`).
- **Smoothing:** `correct(before, after)` takes the jump; `apply(value)`
  is where to draw; `update(dt)` lets it fade. The offset halves every
  `half_life` (0.1 s); jumps bigger than `snap` show at once. It works for
  `f32`, `Vec2`, `Vec3`, `Quat` and `Pose` (position and rotation), and any
  type you implement `Smooth` for. The simulation always uses the corrected
  state: only the picture lags.

**What can be predicted.** Whatever your step function computes from the
state and the input: movement you integrate yourself (2D or 3D), a
kinematic body you move by its transform, and the walking character, whose
step is `ctx.physics3d.move_character` (see
[3D physics](physics3d.md#a-walking-character)): the same move the physics
step makes, bit for bit, so the server can step a `d3::Character` normally
and the client predicts and replays it with that call. Limits:

- Replay sees the world **as it is now**: other bodies are not moved back
  to where they were at the replayed tick. Static levels replay exactly;
  near moving things (other players, doors, platforms), expect small
  corrections, which the smoothing hides.
- **Dynamic bodies** (2D or 3D) are not rewound: the physics world can't
  step one body back in time. Predict a body the player drives with a step
  of your own (or as a kinematic body), let the server own the bodies it
  pushes, and show those with snapshot interpolation. Pushes from a
  predicted character happen on the server's step.
- Exact agreement needs the **same step on both sides**: the same code,
  numbers and fixed tick. Different builds or platforms may round
  differently (feature `physics-determinism` makes the physics agree);
  then corrections are tiny and smoothed rather than absent.

**From C and C++.** The state and the input are plain bytes there: a
`ponko::prediction<State, Input>` takes any trivially copyable types, and a
step that is any callable `void(State&, const Input&)`. The server keeps a
`ponko::input_queue<Input>` per client.

```cpp
{{#include ../../examples/cpp/guide.cpp:native_prediction}}
```

```cpp
{{#include ../../examples/cpp/guide.cpp:native_prediction_loop}}
```

- **Copies:** the state and every input are copied in and out; nothing
  keeps a pointer to yours. States are compared **byte for byte**, so give
  the state no padding and make the step produce the same bytes on both
  sides (fixed point, or rounded floats).
- **Replay is deterministic:** a correction calls the same step with the
  stored inputs, oldest first, so the same server state and inputs give the
  same result every time.
- **A step that throws** stops the call: the remaining replay steps are
  skipped, the exception comes out of `predict` or `reconcile`, and the
  state is whatever the finished steps left. Call `reset` with a state you
  trust before predicting again. The step must not call its own prediction.
- **Capacity** is 256 unconfirmed inputs by default (`prediction(state,
  capacity)`, at most 32768); `unacked(seqs, inputs, n)` copies them without
  allocating, and a steady predict-and-reconcile loop allocates nothing.
- `Smoothing` stays Rust-only: fade the drawn offset from `*r.before`
  yourself.

C has the same thing as `ponko_prediction_new` (state size, input size,
capacity), `ponko_prediction_predict` and `ponko_prediction_reconcile` with
a `PonkoPredictStepFn` (it returns `false` to stop the call; buffers are
16-byte aligned), and `ponko_input_queue_*`. This covers the client and
server bookkeeping only; sending the packets and running a live game over a
real network are still yours, as in Rust.

### Testing it

Everything above is pure data, so tests need no sockets: feed `push` with
recorded or simulated packets and a made-up clock. The engine's own tests also
run two `NetMessages::loopback_lossy` endpoints with extra delay and jitter,
exchanging snapshots built with these helpers
(`tests/net_replication.rs`), and every helper has tests for jitter,
reordering, packet loss, the extrapolation cap, handoff races, sequence
wrap-around and interest hysteresis. `tests/net_prediction.rs` plays a
client and a server over a lossy loopback with 150 ms of round trip, and
replays the character controller against the physics step.
