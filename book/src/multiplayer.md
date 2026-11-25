# Multiplayer: pick a model, then follow it

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/net.rs`,
> `tests/net_replication.rs`, `tests/net_messages.rs`,
> `tests/net_rollback.rs`, on loopback. Platforms: Linux, macOS and Windows,
> headless (the workspace tests in the list of checks). Graphics: not
> involved. Missing: a reference game and a run over a real network.

Every multiplayer game answers one question first: **what crosses the
network?** Ponko has an answer for each common one. Pick the model that
fits your game, then follow its chapters in order; you rarely need the
others.

| Your game | Model | What crosses the network | Follow |
|---|---|---|---|
| Action, co-op, shooters, sports, arenas, sandboxes: a server decides, players see the result | **Replication** (the default) | The server's state, as small deltas; players send only their controls | [Replication](replication.md), then [Dedicated servers](servers.md) |
| RTS, tactics, fighting games, huge simulations where every player can run everything | **Lockstep** | Only inputs, about 50 bytes a tick, however big the world | [Lockstep](networking.md) |
| A turn-based game, a chat, a lobby, or a protocol you design yourself | **Messages** | Your bytes, on a reliable or an unreliable lane | [Application messages](networking.md#application-messages-rust) and [the building blocks](networking.md#replicating-state-over-messages) |

How to choose, in three questions:

1. **Can every player run the whole simulation, the same way, bit for
   bit?** Then lockstep is the cheapest model there is: the bandwidth
   doesn't grow with the world. It needs a deterministic simulation (Ponko's
   is, see [the simulation clock](sim-clock.md)), every player's input
   before a tick can run, and the same build everywhere. It suits 2 to 8
   players.
2. **Should one machine own the truth?** Anti-cheat, players joining and
   leaving at any time, many players, a dedicated server in a data center:
   that is replication. The server runs the game; clients see it a moment
   late (smoothed by [interpolation](replication.md#showing-it-smoothly)),
   except their own player, which they [predict](replication.md#your-own-player-at-once-prediction).
3. **Neither?** A card game, a lobby, a chat or a protocol of your own
   design: send [messages](networking.md#application-messages-rust).

The models share the transports: plain UDP, WebSocket for players in a
browser, Steam relays, and the [lobby server's relay](lobby.md#game-sessions-through-the-relay)
for players who can't reach each other ([which transport](networking.md#which-transport)).
[Putting a game online](online.md) is one complete setup.
They also share the testing approach: a scripted network that loses,
delays, reorders and duplicates packets on a deterministic clock, so a
multiplayer test runs in milliseconds and the same way every time.

## The replication path, step by step

Most games take this path. Each step is a section of
[Replication](replication.md):

1. [Declare what replicates](replication.md#declare-what-replicates):
   which components, and how precisely each field travels.
2. [Mark entities](replication.md#mark-entities) on the server; clients
   get them, and lose them when they despawn or leave the client's interest.
3. [Host and join](replication.md#host-and-join): give the game a message
   endpoint. A host is the server, a joining game a client.
4. [Send controls and predict your own player](replication.md#your-own-player-at-once-prediction).
5. [Events](replication.md#events-things-that-happen-once) for things
   that happen once: a hit, a pickup, a chat line.
6. [Interest and the bandwidth budget](replication.md#interest-and-the-budget),
   once the world is big.
7. [Test it](replication.md#test-it) with a scripted network, then
   [run a dedicated server](servers.md).

[Orb Arena](replication.md#orb-arena-the-reference-game) is the
reference: a small game that does all of it, with a dedicated server and a
headless test that plays it with bots.
