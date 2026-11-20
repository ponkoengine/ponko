# Game AI: behaviour trees and utility scoring

Most enemies need no AI library. A crawler that walks its route, chases the
player it sees and stops when stunned is an `enum` (or a `bool`) and a
`match` in a sim system, and that stays the best way to write it. For agents
with many moods, tuned by someone who isn't editing Rust, or thousands of
them, `ponko::ai` adds an optional kit: behaviour trees written as JSON,
utility scoring, a view of each agent's decisions in the inspector, and
think rates for crowds. Nothing runs it unless a game adds its system.

## Do you need it?

What the example games do with plain systems:

| Game | Agents | What they decide | State kept by hand |
|---|---|---|---|
| Shard Grove (crawlers) | a few | patrol, chase (with a give-up distance), stunned | route leg, stun timer, `chasing` flag |
| `battle` (20,000 units) | thousands | hit what is in reach, charge what is in sight, hit the base, follow the field; the general attacks or defends | `engaged` flag, cooldown, a hand-made "look every 4th tick unless engaged" |
| `ants` | thousands | carry food home or follow trails | `carrying` flag, energy |
| Ember Hop, Coin Dash, the tutorials | none | — | — |

None has more than three moods, and each fits in 20 to 40 lines next to the
code that moves the agents. A tree would not make them shorter. Plain
systems start to hurt when:

- moods multiply: each new one (flee when hurt, search where the player was
  last seen, call friends) is another flag and another place in a growing
  `if` chain whose order *is* the priority;
- steps follow steps: "go home, then rest until healed" needs a phase enum
  and code to resume it;
- someone tunes behaviour: every change of a threshold or an order is a
  rebuild;
- you ask "why is that guard doing this?": the inspector shows the flags,
  not the decision;
- crowds need think rates: every game writes its own `turns(4)` plus an
  "unless busy" flag.

The kit costs more per decision than a hand-written `match`. Measured with
`cargo run --release -p ponko_bench -- ai/10k` (10,000 agents, a 15-node
tree: flee when hurt, chase while alert, otherwise a utility choice between
eating, going home to rest and patrolling; Apple M4 Max, 14 cores, while
other builds kept the load average between 100 and 200, three runs):

| Benchmark | Median per tick | Fastest tick |
|---|---|---|
| `ai/10k-plain-1t`: `match` on an enum, one thread | 0.08 ms | 0.073–0.076 ms |
| `ai/10k-tree-1t`: the tree, one thread | 0.81 ms | 0.75–0.78 ms |
| `ai/10k-plain`: `match`, all cores | 0.07–0.10 ms | 0.03–0.04 ms |
| `ai/10k-tree`: `ai::think`, all cores | 0.86–1.33 ms | 0.20–0.23 ms |
| `ai/10k-tree-lod`: 10% every tick, 30% every 4th, 60% every 16th | 0.30–0.51 ms | 0.08–0.09 ms |

About 80 ns an agent against 8 ns for the `match`: ten times the cost, and
still under a millisecond of one core for 10,000 agents, with no heap
allocation per tick. Think rates bring a crowd to about a third. So: plain
systems for a handful of simple enemies; the kit when the behaviour is
bigger than the code that carries it out. The parallel medians are noisy on
a loaded machine; the one-thread numbers are the steady ones. A brain is
72 bytes plus 24 a node, a blackboard 24 bytes plus 40 an entry.

## Trees decide, systems act

A tree reads and writes only an agent's `Blackboard` and picks an
*action* by name. Your systems do the rest, in three steps each tick:

1. **Senses** write each blackboard: how far the player is, the health left.
2. **`ai::think`** runs the brains (on all cores).
3. **Actions** do what each `Brain` picked, and report when it's done.

The tree of the `ai_guards` example (`assets/ai/guard.json`): guards go
home to rest when hurt, chase an intruder they see, and otherwise choose
between their round and a rest by how tired they are.

```json
{{#include ../../assets/ai/guard.json}}
```

The keys it names, as constants (comparing two keys compares two numbers):

```rust
{{#include ../../examples/ai_guards/game.rs:keys}}
```

Senses, with a think rate by distance:

```rust
{{#include ../../examples/ai_guards/game.rs:senses}}
```

Actions: `brain.doing(KEY)` says what to do, `brain.param_num` reads the
`do` node's other fields, `brain.started()` is true on the tick an action
begins, and `brain.succeed()` or `brain.fail()` ends it.

```rust
{{#include ../../examples/ai_guards/game.rs:act}}
```

And the systems in order, the tree loaded, the guards spawned:

```rust
{{#include ../../examples/ai_guards/game.rs:game}}
```

`cargo run --example ai_guards` shows it; `tests/ai_guards.rs` plays it
headlessly.

## The tree format

`"blackboard"` gives starting values, set where an agent has none yet;
`"root"` is the top node. A node is an object with one *kind* field:

| Node | Does |
|---|---|
| `{"sequence": [..]}` | its children in order until one doesn't succeed |
| `{"selector": [..]}` | its children in order until one doesn't fail |
| `{"utility": [..], "stick": 0.1}` | its children best score first until one doesn't fail |
| `{"invert": node}` | success and failure swapped |
| `{"succeed": node}` | success when the node finishes either way |
| `{"check": "alert"}` | the entry is true, a number other than 0, a point or an entity |
| `{"check": "health", "below": 0.3}` | also `"above"`, `"is"`; a string compares with that entry |
| `{"set": "alerted", "to": true}` | writes the blackboard |
| `{"do": "chase", "speed": 4.5}` | an action, with parameters |
| `{"wait": 1.5}` | runs that many seconds, then succeeds |
| `{"cooldown": 3, "child": node}` | fails for 3 s after the child succeeded |

Any node may have a `"name"` for the debug view. Values are `true`, `0.5`,
`[x, y]`, `[x, y, z]` or `{"entity": id}`.

**How a think runs.** Every think starts at the root, so a higher branch
takes over as soon as its check passes. A `do` or `wait` that was running
carries on when it is reached again; one that isn't reached is dropped and
starts over later. A running `sequence` resumes at its running step (the
finished steps don't run again) but asks its checks again first, so
`[check alert, do chase]` stops chasing once `alert` turns false.

A file that doesn't compile is refused with the node and the problem:
`behaviour tree: root/selector[1]/sequence[0]/check: unknown field "beneath"`.

## Utility scoring

A `utility` node scores its children and tries the best first. A score is
a number or a list of *considerations*, each a blackboard number scaled
from `from` (0) to `to` (1) through a curve (`linear`, `quadratic`,
`cubic`, `sqrt`, `smooth`, `{"power": k}`, `{"logistic": k}`,
`{"step": t}`), maybe `"invert": true`. The terms multiply, made up for
how many there are so that an option with more reasons isn't punished.
Ties go to the earlier child. `"stick"` adds to the child that was running,
so two close options don't swap every tick.

The same helpers work in plain systems:

```rust
use ponko::ai::{best, combine, Blackboard, Consideration, Curve, Key};

const HUNGER: Key = Key::new("hunger");
const DANGER: Key = Key::new("danger");

let bb = Blackboard::new().with(HUNGER, 80.0).with(DANGER, 0.5);
let eat = combine(&[
    Consideration::new(HUNGER, 20.0, 100.0).curve(Curve::Quadratic).score(&bb),
    Consideration::new(DANGER, 0.0, 1.0).inverted().score(&bb),
]);
let flee = Curve::Logistic(12.0).eval(bb.num(DANGER));
assert_eq!(best([("eat", eat), ("flee", flee), ("idle", 0.1)]).map(|b| b.0), Some("eat"));
```

## Think rates for crowds

A `Brain` thinks every `period` ticks (`with_period`, `set_period`),
spread over the ticks by entity so a crowd doesn't think all at once.
Between thinks its action carries on. It thinks at once, whatever the
period, when its action ends (`succeed`, `fail`), when it gets a new tree,
or when you `wake()` it (it heard a shot). `ai::lod_period(distance,
bands, beyond)` picks a period from a distance, as in the senses above.

## Seeing what it thinks

`Brain` and `Blackboard` are reflected components: the
[inspector](inspector.md) shows and edits the blackboard, saves and the
[control endpoint](control.md) read both as JSON. Selecting an agent in
the inspector also shows its tree under **Behaviour tree**: each node
marked with what it did at the last think (▶ running, ✔ success,
✖ failure, · not reached) and each utility child's score.
`Brain::trace` gives the same lines to your own tools.

## Hot reload and determinism

`ctx.assets.load_behavior_tree(&mut ctx.world, "ai/guard.json")` loads a
tree under its path (`try_load_behavior_tree` returns the error instead of
panicking), the name `Brain::new` takes. With
[hot reload](data-files.md) on, saving the file swaps the tree in the
running game; brains running it start it over. A broken file keeps the old
tree and prints why. Trees made in code go in with `ai::add_tree`.

Thinking reads only blackboards and counts time in sim ticks (`wait`,
`cooldown`), and each brain thinks on its own, so the same blackboards give
the same choices on any machine and any number of cores: safe for
[lockstep](networking.md) and replays. A brain read back from a save
starts its tree over; its blackboard comes back as it was.

## From C and C++

A native game keeps a brain per agent in its own storage and runs it
itself (`ponko_behavior_tree_new`, `ponko_brain_new`, `ponko_brain_think`,
`ponko_brain_action`, `ponko_brain_finish`, the blackboard setters,
`ponko_brain_trace`; keys go by id from `ponko_ai_key`):

<div class="lang-pair">
<div>

```rust
use ponko::ai::{BehaviorTree, Blackboard, Brain, Key};
use std::sync::Arc;

const ALERT: Key = Key::new("alert");
const CHASE: Key = Key::new("chase");

let tree = Arc::new(BehaviorTree::from_json(r#"{"root": {"selector": [
    {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 120}]},
    {"do": "patrol"}
]}}"#).unwrap());
let (mut brain, mut bb) = (Brain::new("guard").with_period(4), Blackboard::new());
for tick in 0..10 {
    bb.set(ALERT, tick >= 5);                               // senses
    if brain.due(0, tick) {
        brain.think(&tree, &mut bb, tick, 1.0 / 60.0);      // think
    }
    if brain.doing(CHASE) { /* act */ }
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ai_native}}
```

</div>
</div>

Not in the C API: the world's `Brain` and `Blackboard` components, the
`ai::think` system and hot reload through `assets/`; a native game reloads
a tree by making a new one from the file's text.
