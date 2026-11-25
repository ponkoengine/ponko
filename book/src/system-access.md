# Declared system access

> **Maturity: [tested](maturity.md).** Game: none. Tests:
> `tests/system_access.rs`, `tests/system_views.rs`. Platforms: Linux, macOS
> and Windows, headless (the workspace tests in the list of checks). Graphics:
> not involved. Missing: a reference game that uses it.

Every system you have seen so far takes `&mut Ctx`: it may touch anything,
so the engine runs systems one at a time. A system can also **declare** what
it reads and writes. The declaration is checked, shows up in the access
report, and is what the optional scheduler uses to run systems side by
side: see [Parallel systems](parallel-systems.md).

Declaring access changes nothing by itself: declared systems run in phase
and registration order like any other, one at a time, until
`Game::parallel_systems(true)` is set. Plain `sim_system`, `system` and
`add` systems keep their API and stay *exclusive* (they conflict with
everything), so existing games behave exactly as before.

## View systems

A view system is a function of `&mut SystemView` instead of `&mut Ctx`. It
declares its access with a `SystemAccess`, attached where it is added
(`system.access(...)`), and its `SystemView` offers exactly what it
declared, and nothing else. It goes through the same `Game::add` as any
other system.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/system_views.rs:view_system}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:declared_access}}
```

</div>
</div>

(C and C++ systems keep their `ctx`; their declarations are checked in
debug builds, see [below](#c-and-c).)

What a declaration can name:

| Builder | Allows |
|---|---|
| `read::<T>()` / `write::<T>()` | component `T` (`&T` / `&mut T` in queries, `get` / `get_mut`) |
| `read_resource::<R>()` / `write_resource::<R>()` | resource `R` (writing includes inserting and removing it) |
| `read_events::<E>()` / `send_events::<E>()` | reading / sending `E` events (separate rights) |
| `commands()` | `sys.commands`: spawning, despawning, inserting and removing components |
| `read_service(s)` / `write_service(s)` | an engine service (below) |

`sys.world` is a `WorldView`: queries, `get`, `get_mut`, resources, and
entity checks (`is_alive`, `has`, `len`), all limited to the declaration. It
has no `spawn`, `despawn`, `insert` or `remove`: structural changes go
through commands, which apply after the system as usual. Its writes count
for [change detection](ecs.md#change-detection) like any others, and its
queries take the same filters as world queries: `filter::<F>()` with
`With`, `Without`, `Or`, `Added<T>` and `Changed<T>`.
A change filter on `T` reads the stamps `T`'s writers leave, so it needs `T`
declared (`read::<T>()` is enough); `sys.world.removed::<T>()` needs the
same. Like a `&mut Ctx` system, a view system is shown what changed since it
last ran. Tracking is explicit, as in a world query: call
`ctx.world.track_changes::<T>()` in setup; a change filter on an untracked
component panics and says so.

```rust
# use ponko::prelude::*;
# use ponko::access::SystemAccess;
# use ponko::system_view::SystemView;
#[derive(Component)]
struct Health(i32);
struct Hurt(Entity);

fn report_hurt(sys: &mut SystemView) {
    let mut hurt = Vec::new();
    sys.world
        .query::<Entity>()
        .filter::<Changed<Health>>()
        .for_each(|e| hurt.push(e));
    for e in hurt {
        sys.events.send(Hurt(e));
    }
}

# fn main() {
let _game = Game::new("t", 1, 1).add(
    Phase::AfterPhysics,
    report_hurt.access(SystemAccess::new().read::<Health>().send_events::<Hurt>()),
);
# }
```

Every accessor
checks the declaration **in every build** and panics with the system's
name and what is missing:

```text
system `my_game::fall` doesn't declare that it may write component `my_game::Pos`;
add it to its declared access
```

The check runs once per call (a query, a `get`, a resource), never per
entity: a view system's loops cost the same as a `Ctx` system's.

### Services

Services follow the service fields of `Ctx`, and a view has them under the
same names: what is `ctx.input.action_down("jump")` in a `Ctx` system is
`sys.input.action_down("jump")` in a view system. A view offers the ones
whose state can be handed out on its own:

| `Service` | In a `SystemView` | Shortcuts, as on `Ctx` |
|---|---|---|
| `Input` | `sys.input`, read only: `action_down`, `mouse_down`, `gamepad_axis`, ... | `key_down`, `key_pressed`, `key_released`, `mouse_pos` |
| `Time` | `sys.time`, read only: `sim_tick`, `fixed_dt`, `elapsed`, `frame`, `turns`, ... | `dt` |
| `Random` | | `random()`, the system's stream for this tick (the same one `ctx.random()` gives) |
| `Audio` | `sys.audio`: `play_ex`, buses, the listener | `play_sound` |
| `Camera` | `sys.camera`, `sys.camera3d` | |
| `Spatial` | `sys.spatial`, read only | |
| `Physics`, `Physics3D` | `sys.physics`, `sys.physics3d`: ray casts and overlaps read, impulses and gravity write | |
| `Net` | `sys.net.lockstep`, lockstep networking (as `ctx.net.lockstep`) | |
| `Gfx`, `Ui`, `Assets`, `Perf` | not offered | |

Each of those fields checks the declaration when it is used: reading
needs `read_service` (or `write_service`), changing it (`sys.camera.zoom =
2.0`, anything that takes the service mutably) needs `write_service`.
`sys.commands` and `sys.events` work the same way, behind `commands()` and
`read_events` / `send_events`.

Drawing, UI, assets and profiling hold window, renderer and asset-table
state; systems that need them keep `&mut Ctx`. `Game::add` panics at
registration if a view system's declaration names a service the view can't
offer, or writing `Input`, `Time` or `Spatial`. A view system added without
a declaration can reach nothing.

## Declaring `Ctx` systems

The same `access(...)` attaches a declaration to an ordinary `&mut Ctx`
system: `game.add(phase, system.access(SystemAccess::new()...))`. There is
one declaration type for both kinds of system. A declared `Ctx` system
runs with the whole `Ctx`; in builds with debug assertions the
engine records what it touched (components, resources, events, commands
and structural changes) and panics after the run if it went beyond the
declaration. Services are not recorded for `Ctx` systems. Use this to
document and check existing systems; only view systems can ever run in
parallel, because only they can't reach anything undeclared.

The engine's own physics and navigation steps are declared this way:
`Game::physics` and `Game::physics3d` register `step_physics`,
`step_physics3d` and `interpolate_physics3d` with
`systems::step_physics_access()` and its siblings, and
`systems::step_navmesh3d_access()` is there for the navigation step you
add yourself ([what the steps touch](systems.md#what-the-steps-touch)).
The other built-in systems are undeclared, so exclusive.

## The access report

`Game::access_report()` lists every system with its declaration and, for
each pair of declared systems in the same phase, whether they could share
a parallel batch:

```rust
# use ponko::prelude::*;
# use ponko::access::SystemAccess;
# use ponko::system_view::SystemView;
# #[derive(Component)]
# struct Vel(f32);
# fn brake(_: &mut SystemView) {} fn steer(_: &mut SystemView) {}
let report = Game::new("report", 1, 1)
    .add(Phase::AfterPhysics, brake.access(SystemAccess::new().write::<Vel>()))
    .add(Phase::AfterPhysics, steer.access(SystemAccess::new().read::<Vel>()))
    .access_report();
// "... + ...: conflict on component `...Vel`"
print!("{report}");
assert_eq!(report.parallel_pairs().count(), 0);
```

`compatible_pairs()` lists pairs whose declarations don't conflict;
`parallel_pairs()` only those where both are view systems. Each
`ReportedSystem` has the registration's `id` (for `Game::remove` and
`Game::order`) and says whether it is `enforced` (a view system).

## When two systems conflict

Within a phase, two declared systems conflict, and so must keep their
order, when:

- either is exclusive (undeclared systems are);
- both touch the same component, resource or service and at least one
  writes it;
- events or commands make the order visible (below).

Systems that only read the same data never conflict. Phases are barriers:
systems in different phases are never compared.

### Commands and events: the batch barrier

`SystemAccess::conflicts_with` is the conservative, symmetric rule: any
system that sends an event type conflicts with any other that reads or
sends it, and a command recorder conflicts with every other system.

`SystemAccess::conflicts_before(later)` is the finer rule for a known
order (the report and the scheduler use it). It holds under the **batch
contract** the scheduler keeps:

1. Commands recorded in a parallel batch apply when the batch ends, in
   registration order.
2. Events sent in a batch become visible when it ends, in registration
   order, stamped as if sent sequentially.
3. A system outside every batch (exclusive, or any conflicting system) is
   a barrier of its own, exactly as today.

Under that contract, for an earlier system `a` and a later system `b`:

| `a` | `b` | Conflict? | Why |
|---|---|---|---|
| sends `E` | reads `E` | yes | sequentially `b` sees `a`'s events in the same tick |
| reads `E` | sends `E` | no | sequentially `a` only sees them on its next run anyway |
| sends `E` | sends `E` | no | events are published in registration order |
| records commands | anything | yes | sequentially `b` sees the applied commands |
| anything | records commands | no | `a` never saw `b`'s commands |

"Anything" includes a system that declares no data at all: every view can
see the world's structure (`len`, `is_alive`, `has`, `with` and `without`
filters) without declaring it, and commands change that structure.

Two systems can only share a batch if every system registered between
them in the phase could move out of the way: the report adds a
`Between` conflict naming the first one that can't (an undeclared system,
or one that conflicts with either), and `parallel_pairs()` also treats a
`Ctx` system between them as a barrier, since its declaration is only
checked in debug builds.

Running systems one after another (the default) keeps this contract
trivially, so declaring access never changes results.
`tests/system_views.rs` pins the sequential behaviour each relaxed rule
relies on, and `tests/system_schedule.rs` checks that batches give the same
results.

### If you rely on the per-system flush

By default every system is a barrier: a system sees the commands and
events of every system before it in the same tick. With
`parallel_systems(true)`, view systems batched together see each other's
commands and events only after the batch; the rules above keep systems
whose declarations show such a dependency out of one batch. To stay
correct either way:

- If `b` needs `a`'s commands or events in the same tick, declare both so
  the rules above make them conflict (`a` records commands or sends `E`;
  `b` reads `E`), put `b` in a later phase, or say it with
  `Game::order(a, b)` or `Game::barrier(phase)`.
- Don't count on registration order between systems that don't conflict.
- Keep "make a change and look at it" inside one system, through
  `world` (immediate) or a single `commands.add` closure.

## C and C++

C and C++ systems declare access by registered name (component, resource
and event names; C++ uses the type's name, as the world does):

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# struct Health(i32);
# struct Died(Entity);
# fn hurt(_: &mut Ctx) {}
// The same declaration, by name, as C and C++ build it.
let game = Game::new("named", 1, 1).add(
    Phase::AfterPhysics,
    hurt.access(
        ponko::access::SystemAccess::new()
            .write_named(std::any::type_name::<Health>())
            .send_events_named(std::any::type_name::<Died>())
            .commands(),
    ),
);
# let _ = game;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:named_access}}
```

</div>
</div>

`SystemView` itself is Rust-only for now (see
[Still Rust-only](c-cpp.md#still-rust-only)): a C or C++ system always takes
the whole `ctx`.

They run with their `ctx` and are checked in debug builds of the library
like declared Rust `Ctx` systems; a violation stops the run with an error
naming the system. Raw C access can't tell reads from writes, so a
component or resource declared by name covers any use of it: declare a
write when the system changes it, so the report shows the conflict.
`ponko_access_conflicts` and `ponko_game_access_report` expose the rules
and the report. Build declarations at registration, not per frame: each
step copies the declaration and keeps its names for the rest of the
program. C and C++ systems always run one at a time: they receive the whole
`ctx`, so the [scheduler](parallel-systems.md) never puts them in a batch.
`ponko_game_parallel_systems` turns it on for the Rust view systems of a
mixed game, and `ponko_game_schedule_plan` shows the stages.

## Design: the soundness strategy (RFC)

*Status: implemented (#137, and the executor in #138).*

**Goal.** Let a scheduler run independent systems at the same time without
ever giving two threads `&mut Ctx` or `&mut World`, and without trusting
declarations it can't check.

**Declarations are metadata; views are enforcement.** A `SystemAccess` is
data: components, resources and events by type, commands, services. On its
own it proves nothing; a `&mut Ctx` system can use anything. So:

1. *View systems* get a `SystemView` built from separate borrows of `Ctx`
   fields. Its world part is the ECS's `WorldView`: a shared `&World` plus
   the declaration. There is no path from the view to anything else, and
   each accessor checks the declaration in every build. A wrong
   declaration is a panic naming the system, never a data race.
2. *Declared `Ctx` systems* (Rust `system.access(..)`, C and C++) are checked
   after each run in debug builds by the world's access recorder. They are
   never candidates for parallel execution.
3. *Undeclared systems* are exclusive, as before.

**How two views share one world.** Before the views exist, with
`&mut World`, each `ViewState` is prepared: the declared components are
registered and the declared *written* resources move out of the world into
the state. The views then share `&World`:

- Components are written through the archetype columns' own raw pointers,
  which carry write permission from their allocation. Two views whose
  declarations don't overlap (`ViewState::overlaps`: no component one
  writes and the other reads or writes) touch disjoint columns.
- A written resource lives in its view's state, so no other view can reach
  it; an overlapping declaration finds it missing (a panic, not a race).
- No view can change the world's structure, so archetypes and columns
  don't move while views exist; structural changes are commands, applied
  with `&mut World` at the barrier.
- Afterwards the resources go back, also when the system panics.

Preparing, viewing and finishing are private to the ECS crate; the step in
between is the unsafe one, and its contract is exactly "no overlapping view
or `&mut World` use while it lives". The two public ways in keep it:
`ViewState::run` for one view, and `ViewGroup`, which checks once that a set
of states is pairwise non-overlapping (and refuses the same state twice),
and whose `run` hands each state's view to its own task, only for the
states it checked. The scheduler goes through `ViewGroup`.
`crates/ponko_ecs/tests/views.rs` and `view_groups.rs` run under Miri.

**The executor (#138).** Each view system of a batch has its own command
buffer and event outbox, merged at the batch barrier in registration order
(the batch contract above), and reads events through a shared, read-only
slice with its own cursor. A batch runs through the same code on several
threads and on one, so the choice is invisible to the systems. Only
read-only services (input, clocks, the random stream, the spatial grid)
are shared in a batch; a view system that holds sound, a camera, a physics
world or networking runs alone. The executor itself has no `unsafe`: the
only shared-world entry is `ViewGroup::run` in the ECS.

**Hot patching** swaps a system's code, not its declaration. A patched
view system still can't reach anything undeclared; a patched `Ctx` system
is checked on its next debug run.

**Costs.** Release builds skip the `Ctx` recorder entirely. A view adds one
declaration lookup per accessor call, no per-entity cost, and no heap
allocation per run once its buffers have grown
(`crates/ponko_ecs/tests/views_alloc.rs`).

**Limits.** `QueryData` is sealed: what a query can fetch is `&T`,
`&mut T`, `Option` of those, `Entity` and tuples of them. A view's filters
never register a component: `With<T>` of a component no entity ever had
matches nothing, `Without<T>` of one matches everything. `Added<T>` /
`Changed<T>` of an untracked `T` panic, as in a world query.
