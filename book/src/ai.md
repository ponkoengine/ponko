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
eating, going home to rest and patrolling; Apple M4 Max, 14 cores; six runs (three for
`ai/10k-plain`) with the per-brain tick counter, the signal scan and several actions at
once in, while other builds kept the load average between 57 and 65):

| Benchmark | Median per tick | Fastest tick |
|---|---|---|
| `ai/10k-plain-1t`: `match` on an enum, one thread | 0.067–0.070 ms | 0.062–0.065 ms |
| `ai/10k-tree-1t`: the tree, one thread | 0.67–0.68 ms | 0.62–0.64 ms |
| `ai/10k-plain`: `match`, all cores | 0.07–0.10 ms | 0.026–0.033 ms |
| `ai/10k-tree`: `ai::think`, all cores | 0.40–1.49 ms | 0.18–0.20 ms |
| `ai/10k-tree-lod`: 10% every tick, 30% every 4th, 60% every 16th | 0.23–0.74 ms | 0.06–0.10 ms |

About 67 ns an agent against 7 ns for the `match`: ten times the cost, the
same ratio as before those additions, and well under a millisecond of one
core for 10,000 agents. The one-thread runs allocate nothing; the parallel
ones about one allocation in 50 ticks, fewer than the parallel `match`
(the thread pool, not the trees). Think rates bring a crowd to about a
third. So: plain systems for a handful of simple enemies; the kit when the
behaviour is bigger than the code that carries it out. The parallel medians
are noisy on a loaded machine; the one-thread numbers are the steady ones.
These were taken on a busy machine: a run on a quiet one is still owed.
A brain is 152 bytes plus 24 a node, a blackboard 24 bytes plus 40 an
entry.

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
| `{"parallel": [..], "success": "any"}` | its children at once, by a stated policy ([below](#parallel-nodes)) |
| `{"random": [..]}` | its children in a random order until one doesn't fail |
| `{"invert": node}` | success and failure swapped |
| `{"succeed": node}` | success when the node finishes either way |
| `{"check": "alert"}` | the entry is true, a number other than 0, a point or an entity |
| `{"check": "health", "below": 0.3}` | also `"above"`, `"is"`; a string compares with that entry |
| `{"set": "alerted", "to": true}` | writes the blackboard |
| `{"do": "chase", "speed": 4.5}` | an action, with parameters |
| `{"wait": 1.5}` | runs that many seconds, then succeeds |
| `{"cooldown": 3, "child": node}` | fails for 3 s after the child succeeded |
| `{"chance": 0.25}` | succeeds one time in four |
| `{"wait_for": "noise"}` | runs until the agent hears that signal ([below](#waiting-for-events)) |
| `{"subtree": "ai/patrol.json"}` | runs another tree ([below](#subtrees)) |

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

## Subtrees

A tree can run another by name: `{"subtree": "ai/patrol.json"}` stands
for that tree's root, with its `"blackboard"` defaults (the outer tree's
win). Patrols, fights and flights written once serve every kind of agent:

```json
{"root": {"selector": [
    {"sequence": [{"check": "health", "below": 0.3}, {"subtree": "ai/flee.json"}]},
    {"sequence": [{"check": "alert"}, {"subtree": "ai/fight.json"}]},
    {"subtree": "ai/patrol.json"}
]}}
```

`load_behavior_tree` loads the files a tree names (and the files those
name), and with hot reload on watches them all: saving `ai/patrol.json`
reaches every tree that runs it. Trees made in code link the same way
through `BehaviorTrees` (`ai::add_tree`, `BehaviorTrees::try_insert`), in
any order: a subtree that isn't there yet is a node that fails until it is.
A tree that runs itself, directly or through others, is refused with
every name on the cycle:

```rust
use ponko::ai::{BehaviorTree, BehaviorTrees};

let mut trees = BehaviorTrees::default();
let tree = |json: &str| BehaviorTree::from_json(json).unwrap();
trees.try_insert("ai/guard.json", tree(r#"{"root": {"subtree": "ai/fight.json"}}"#)).unwrap();
let error = trees
    .try_insert("ai/fight.json", tree(r#"{"root": {"subtree": "ai/guard.json"}}"#))
    .unwrap_err();
assert!(error.to_string().contains("subtree cycle: ai/fight.json -> ai/guard.json -> ai/fight.json"));
```

The debug view shows a subtree's nodes under a `subtree` line. A subtree is
copied into each tree that runs it, so a brain's memory and trace cover the
whole thing.

## Parallel nodes

A `parallel` runs all its children at each think, in order, with a stated
policy: `"success"` is how many must succeed and `"failure"` how many must
fail (`"all"`, `"any"` or a count). The defaults are all for success and
any for failure. Children that finished keep their result until the
parallel ends (checks are asked again each think, so a parallel can watch a
condition); if all finish and neither count is reached, it fails. When it
ends, children still running are stopped: an action they started is dropped.

```json
{"sequence": [
    {"parallel": [{"do": "walk_to", "target": "door"}, {"wait": 5}], "success": "any"},
    {"do": "knock"}
]}
```

walks to the door for at most five seconds, then knocks; and
`{"parallel": [{"do": "aim"}, {"check": "alert"}]}` aims while alert.

### Several actions at once

When several children of a parallel hold `do` nodes, their actions run
side by side: a soldier moves to cover *and* aims. Your systems ask for
each by name, as before, and report each on its own:

```rust
use ponko::ai::{BehaviorTree, Blackboard, Brain, Key};
use std::sync::Arc;

const MOVE: Key = Key::new("move");
const AIM: Key = Key::new("aim");
const SPREAD: Key = Key::new("spread");

let tree = Arc::new(BehaviorTree::from_json(r#"{"root": {"sequence": [
    {"parallel": [{"do": "move", "to": "cover"}, {"do": "aim", "spread": 0.5}]},
    {"do": "fire"}
]}}"#).unwrap());
let (mut brain, mut bb) = (Brain::new("soldier"), Blackboard::new());
brain.think(&tree, &mut bb, 0, 1.0 / 60.0);
assert!(brain.doing(MOVE) && brain.doing(AIM));            // both run
assert!(brain.started_action(AIM));                         // started this think
assert_eq!(brain.action_param(AIM, SPREAD).and_then(|v| v.as_num()), Some(0.5));
brain.succeed_action(AIM);                                  // aimed; still moving
brain.think(&tree, &mut bb, 1, 1.0 / 60.0);
assert_eq!(brain.actions().collect::<Vec<_>>(), [MOVE]);
brain.succeed_action(MOVE);                                 // in cover: both done
brain.think(&tree, &mut bb, 2, 1.0 / 60.0);
assert!(brain.doing(Key::new("fire")));
```

How it is designed, and why:

- **Actions are named by their names.** `brain.doing(AIM)` and
  `brain.succeed_action(AIM)` mean one action, so the system that aims
  needn't know what else runs. To keep that true, a tree where two
  children of one parallel could run an action of the same name at once is
  refused when it loads (`children [0] and [1] can both do "aim" at
  once`); the same action in two branches that never run together (a
  selector's) is fine.
- **The one-action calls keep their meaning.** `action()`, `started()`,
  `status()`, `param()`, `succeed()` and `fail()` are about the *first*
  action in tree order, the only one in a tree without such a parallel, so
  existing trees and systems work as before. `actions()` lists them all.
- **Each ends on its own.** A reported action ends its `do` node and the
  others carry on. When the parallel ends (its policy decided, or a higher
  branch took over), every action below it is dropped.
- **No new cost for one action.** A brain keeps up to two running actions
  inline, with no heap allocation per think. A save lists the extra
  actions beside the first, so a save of a one-action brain is unchanged.

Not chosen: *channels* (legs, hands, voice) declared by the tree, where a
new action on a channel replaces the old one. Every game would need the
same list of channels, and the parallel's policy already says what runs
together.

## Random choices

`{"random": [..]}` tries its children in a random order until one doesn't
fail; the one that runs keeps running at the next thinks instead of being
drawn again. `{"chance": 0.25}` succeeds one time in four, drawn each time
it is reached, so in front of an action in a sequence it weighs a choice:

```json
{"selector": [
    {"sequence": [{"chance": 0.2}, {"do": "taunt"}]},
    {"random": [{"do": "strafe_left"}, {"do": "strafe_right"}]}
]}
```

The numbers come from the game's seed (`Game::seed`), the brain's tick and
its entity, so a run is the same every time with the same seed and inputs,
on any number of cores. A brain you run yourself (`Brain::think`) draws
from `Brain::with_seed`: give each agent its own.

## Waiting for events

A `wait_for` node runs until the agent hears a *signal*, an event the
game sends with `ctx.events.send(ai::Signal::to(guard, NOISE))` (or
`Signal::broadcast(ALARM)` for every brain) from a system before
`ai::think`. The brain doesn't poll a flag each think: the signal wakes it
at once, whatever its period, so a crowd can think every 16th tick and
still jump at a gunshot. `"store": "key"` writes the signal's value
(`Signal::to(..).with(shooter)`) to the blackboard; `"timeout": 2` fails
the node after two seconds without one, and `"timeout": 0` only asks
whether the signal came at this think.

```json
{"sequence": [{"wait_for": "door_opened", "store": "door"}, {"do": "enter"}]}
```

`ai::forward` turns the engine's own events, or any of yours, into signals,
with no system of your own:

```rust
use ponko::ai::{self, Key, Signal};
use ponko::prelude::*;

struct Shot { by: Entity, at: Entity }
const SHOT_AT: Key = Key::new("shot_at");

Game::new("guards", 640, 480)
    .sim_system(ai::forward(|shot: &Shot| Some(Signal::to(shot.at, SHOT_AT).with(shot.by))))
    .sim_system(ai::think)
    .run_headless(1, |_| {});
```

A brain only hears signals its tree waits for (`BehaviorTree::signals`);
`Brain::signal` hands one to a brain directly.

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

Thinking reads only blackboards and signals, counts time in ticks
(`wait`, `cooldown`) and draws random numbers from the game's seed, and
each brain thinks on its own, so the same seed and inputs give the same
choices on any machine and any number of cores: safe for
[lockstep](networking.md) and replays.

## Saves

A brain's JSON holds its memory: the running nodes, how far its waits and
cooldowns are, the action and what the game reported for it. A world read
back from a save ([saves](storage.md)) carries on at the same running
node, as if the save never happened: the brain counts its ticks on from
the one it was saved at, whatever the game's clock says now, so its waits,
cooldowns, think periods and random numbers go on as they would have. A
brain whose tree changed since (another
`BehaviorTree::fingerprint`) starts the new tree over; its blackboard comes
back as it was either way.

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

Subtrees: `ponko_behavior_tree_new_linked` compiles a tree with the trees
its `subtree` nodes name, by name (`ponko::ai::behavior_tree(json,
{{"patrol", &patrol}})`). Signals: `ponko_brain_signal`. Random nodes:
`ponko_brain_set_seed` per agent. Saves: `ponko_brain_save_json` writes a
brain with its memory and blackboard, and `ponko_brain_load_json` reads it
back to carry on where it was. Several actions at once:
`ponko_brain_action_at` (`brain::action_at`), `ponko_brain_finish_action`
(`succeed_action`, `fail_action`), `ponko_brain_action_started`,
`ponko_brain_action_param_num`; an action key of 0 means the first.

### World brains and signals

A native game can also leave its brains to the engine, as a Rust game
does: `Brain` and `Blackboard` components on entities, run by the
`ai::think` system, woken by `ai::Signal` events.
`ponko_ctx_ai_load_tree` loads a tree from `assets/` (with hot reload) and
`ponko_ctx_ai_add_tree` adds a compiled one; `ponko_ctx_ai_add_brain`
gives an entity a brain; `ponko_ctx_ai_think`, called from a sim system,
is `ai::think`. Signals go to one brain or, with `PONKO_AI_EVERY_BRAIN`,
to all: `ponko_ctx_ai_signal` and `_num`, `_entity`, `_vec2`, `_vec3` for a
value that `"store"` writes. The actions and blackboards of world brains:
`ponko_ctx_ai_action`, `_started`, `_finish`, `_param_num`, and
`ponko_ctx_ai_set_num` (and `_flag`, `_vec2`, `_vec3`, `_entity`),
`ponko_ctx_ai_num`, `ponko_ctx_ai_flag`.

<div class="lang-pair">
<div>

```rust
use ponko::ai::{self, BehaviorTree, Blackboard, Brain, Key, Signal};
use ponko::prelude::*;

const ALARM: Key = Key::new("alarm");
const SHOUT: Key = Key::new("shout");
const RUN: Key = Key::new("run");

Game::new("sentries", 640, 480)
    .sim_system(|ctx: &mut Ctx| {                     // signals, before think
        if ctx.time.sim_tick() == 30 {
            ctx.events.send(Signal::broadcast(ALARM).with(vec2(320.0, 240.0)));
        }
    })
    .sim_system(ai::think)
    .sim_system(|ctx: &mut Ctx| {                     // act
        ctx.world.query::<&mut Brain>().for_each(|brain| {
            if brain.started_action(SHOUT) {          // two actions at once
                brain.succeed_action(SHOUT);          // shouted once
            }
            if brain.doing(RUN) { /* move toward the noise; finish when there */ }
        });
    })
    .run_headless(40, |ctx| {
        let tree = BehaviorTree::from_json(r#"{"root": {"sequence": [
            {"wait_for": "alarm", "store": "noise"},
            {"parallel": [{"do": "run", "speed": 90}, {"do": "shout"}]}
        ]}}"#).unwrap();
        ai::add_tree(&mut ctx.world, "sentry", tree);
        for _ in 0..3 {
            // Thinks every 16th tick; the alarm wakes it at once.
            ctx.world.spawn((Brain::new("sentry").with_period(16), Blackboard::new()));
        }
    });
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:ai_world}}
```

</div>
</div>

A system can't read a world brain while it runs a query over the world
(`ponko_query`): collect the entities first, as above.
