# Events: messages between systems

> **Maturity: [proven](maturity.md).** Game: Coin Dash and Ember Hop, packaged
> and played on Linux, macOS and Windows. Tests: `tests/events.rs`. Platforms:
> Linux, macOS and Windows, headless (the workspace tests in the list of
> checks). Graphics: not involved.

A system often needs to tell other systems that something happened: a unit
died, a building finished, the player scored. Send an event and let whoever
cares read it, instead of having systems call each other or poll flags.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/events.rs:events}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:events}}
```

</div>
</div>

Any type can be an event: a Rust type, or a plain C++ struct. In C, register
the event struct like a component and pass its id. Events can be at most
16-byte aligned.

## Who sees what

Every system has its own read position. `read` returns the events it hasn't
seen yet, oldest first, and the next `read` returns only newer ones. So:

- A system after the sender, in the same tick, sees the events right away.
- A system before the sender sees them on its next run, the next tick.
- A system that runs every tick (or every frame) sees every event **exactly
  once**. With an interval (`system.every(n)`), it sees everything sent since its
  last run.
- Events from `setup` reach the first tick.

Within simulation systems this is deterministic: the same sends in the same
order give the same reads, whatever the frame rate.

Events are dropped once every system has had its chance: when they're at
least one frame old and older than the slowest system interval. You
never clear them yourself. `peek` shows everything still kept without
moving the read position.

[Systems and the frame](systems.md#when-changes-become-visible) lists how
events relate to deferred commands, phases, frames and pauses.

## Sending from queries

`ctx.events` is a separate field from `ctx.world`, so you can send inside a
query and read while changing the world (as above). C++ `ctx.send` works
inside `world().each` but not `each_par`, which runs on many threads:
collect there, then send.

## Tools and agents see events

Reflect an event type and the [control endpoint](control.md) and the
[MCP server](agents.md) can collect it: an agent subscribes, runs ticks,
and gets every event with the tick it was sent in, as JSON. In C and C++,
events whose type is reflected (described with its fields) show up the same
way.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/events.rs:events_reflect}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:events_reflect}}
```

</div>
</div>

Collecting costs nothing until a tool subscribes.

## Cost

Sending is a push onto a vector, and reading returns a slice of it: about
4 ns per event sent, read and dropped (100 000 events per tick cost
0.43 ms, `cargo run --release -p ponko_bench -- events`). Steady state
allocates nothing.

Events aren't saved with the world (`world.save`); a save between ticks
drops the ones in flight.
