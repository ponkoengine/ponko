//! Game AI: behaviour trees and utility scoring, as data.
//!
//! An optional kit: nothing in the engine runs it, and a game that doesn't
//! use it pays nothing. Plain systems with a `match` on an enum are the
//! simplest AI and stay the first choice for an enemy with two or three
//! moods (see the guide's "Game AI" chapter for when the kit pays off).
//!
//! # Design
//!
//! - **Trees decide, systems act.** A [`BehaviorTree`] reads and writes
//!   only an agent's [`Blackboard`] and picks one *action* by name. The
//!   game's own systems write the blackboard (senses: "the player is 4 m
//!   away"), and carry out the action ([`Brain::doing`]), reporting when it
//!   is done ([`Brain::succeed`], [`Brain::fail`]). The tree never touches
//!   the world, so thinking is a pure function of the blackboard: it runs
//!   on all cores, gives the same result on every machine, and is easy to
//!   test.
//! - **Data-driven.** Trees are JSON files loaded with
//!   [`Assets::load_behavior_tree`](crate::assets::Assets::load_behavior_tree);
//!   with [hot reload](crate::game::Game::hot_reload) on, saving the file
//!   swaps the tree in the running game (each agent starts it over), and a
//!   file that doesn't compile keeps the old tree and prints why.
//! - **Reflected.** [`Blackboard`] and [`Brain`] are reflected components,
//!   so the inspector shows and edits them, and world saves and the control
//!   endpoint see them as JSON. The inspector also shows the tree of the
//!   selected entity, with what each node did at the last think.
//! - **Deterministic.** Thinking happens in [`think`], a sim system the game
//!   adds, at fixed ticks; waits and cooldowns count ticks; a `utility` tie
//!   goes to the earlier child. The same blackboards give the same choices.
//! - **Level of detail.** A brain thinks every [`period`](Brain::period)
//!   ticks (staggered by entity, so a crowd spreads its thinking), and at
//!   once when its action ends or the game [wakes](Brain::wake) it. Far
//!   agents can think every 8th or 16th tick ([`lod_period`]).
//!
//! # The tree format
//!
//! ```json
//! {
//!   "blackboard": {"health": 1.0, "alert": false},
//!   "root": {"selector": [
//!     {"sequence": [{"check": "health", "below": 0.3}, {"do": "flee"}]},
//!     {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 4.5}]},
//!     {"utility": [
//!       {"score": [{"key": "hunger", "from": 20, "to": 100, "curve": "quadratic"}], "do": "eat"},
//!       {"score": 0.2, "sequence": [{"do": "patrol"}, {"wait": 2}]}
//!     ]}
//!   ]}
//! }
//! ```
//!
//! `"blackboard"` gives starting values (set where an agent has none);
//! `"root"` is the top node. A node is an object with one *kind* field:
//!
//! | Node | Does |
//! |---|---|
//! | `{"sequence": [..]}` | its children in order until one doesn't succeed; that one's result |
//! | `{"selector": [..]}` | its children in order until one doesn't fail (a fallback list) |
//! | `{"utility": [..], "stick": 0.1}` | its children best score first until one doesn't fail; each child has a `"score"`; `stick` is added to the child that was running, against dithering |
//! | `{"invert": node}` | success and failure swapped |
//! | `{"succeed": node}` | success when the node finishes either way |
//! | `{"check": "key"}` | succeeds if the entry is true, a number other than 0, a point or an entity |
//! | `{"check": "key", "below" / "above" / "is": v}` | compares the entry with `v`, or with the entry named by `v` if it is a string |
//! | `{"set": "key", "to": v}` | writes the blackboard; succeeds |
//! | `{"do": "action", ...}` | asks the game for an action; runs until the game reports; other fields are its parameters ([`Brain::param`]) |
//! | `{"wait": seconds}` | runs for that long, then succeeds |
//! | `{"cooldown": seconds, "child": node}` | fails for that long after the child succeeded |
//!
//! Any node may have a `"name"`, shown in the debug view. A score is a
//! number or a list of numbers and considerations,
//! `{"key", "from", "to", "curve", "invert"}` (see [`Consideration`] and
//! [`Curve`]), combined with [`combine`].
//!
//! Every think runs from the root, so a higher branch takes over as soon
//! as its check passes (a reactive tree). A `do` or `wait` that was running
//! carries on where it was when it is reached again at the next think;
//! one that wasn't reached is dropped, and starts over when reached later.
//! A running `sequence` resumes at its running step (the steps before it
//! don't run again), but asks its checks again first: a
//! `[check alert, do chase]` stops chasing once `alert` is false.
//!
//! # Use
//!
//! ```
//! use ponko::ai::{self, Blackboard, Brain, Key};
//! use ponko::assets::MemorySource;
//! use ponko::prelude::*;
//!
//! const ALERT: Key = Key::new("alert");
//! const CHASE: Key = Key::new("chase");
//! const PATROL: Key = Key::new("patrol");
//!
//! let mut world = Game::new("guards", 640, 480)
//!     .sim_system(|ctx: &mut Ctx| {
//!         // Senses: a sim system that writes the blackboards.
//!         let alert = ctx.time.sim_tick() >= 10;
//!         ctx.world.query::<&mut Blackboard>().for_each(|bb| bb.set(ALERT, alert));
//!     })
//!     .sim_system(ai::think)
//!     .sim_system(|ctx: &mut Ctx| {
//!         // Actions: a sim system that does what each brain chose.
//!         ctx.world.query::<&mut Brain>().for_each(|brain| {
//!             if brain.doing(CHASE) { /* run at the player */ }
//!             if brain.doing(PATROL) { /* walk the route */ }
//!         });
//!     })
//!     .run_headless(20, |ctx| {
//!         ctx.assets.add_source(MemorySource::new("built-in").with(
//!             "ai/guard.json",
//!             r#"{"root": {"selector": [
//!                 {"sequence": [{"check": "alert"}, {"do": "chase"}]},
//!                 {"do": "patrol"}
//!             ]}}"#,
//!         ));
//!         ctx.assets.load_behavior_tree(&mut ctx.world, "ai/guard.json");
//!         ctx.world.spawn((Brain::new("ai/guard.json"), Blackboard::new()));
//!     });
//! let brain = world.query::<&Brain>().single().unwrap();
//! assert!(brain.doing(CHASE));
//! ```
mod blackboard;
#[cfg(feature = "inspector")]
pub(crate) mod debug_view;
mod tree;
mod utility;

use std::collections::HashMap;
use std::sync::Arc;

use ponko_ecs::{Component, Entity, With, Without, World};
use serde::{Deserialize, Serialize};

pub use blackboard::{Blackboard, Key, Value};
pub use tree::BehaviorTree;
pub use utility::{Consideration, Curve, best, combine, normalize};

use crate::ctx::Ctx;
use crate::reflect::WorldReflect;
use tree::{NodeState, Tick};

/// What a node, or an action, did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Not finished: carry on at the next think.
    #[default]
    Running,
    /// Done, and it worked.
    Success,
    /// Done, and it didn't.
    Failure,
}

/// The trees the game has loaded, by name: a resource, added by
/// [`add_tree`] and
/// [`Assets::load_behavior_tree`](crate::assets::Assets::load_behavior_tree).
#[derive(Default)]
pub struct BehaviorTrees {
    trees: HashMap<&'static str, Arc<BehaviorTree>>,
    /// Bumped on every change, so brains look their tree up again only then.
    generation: u64,
}

impl BehaviorTrees {
    /// Adds a tree under `name`, or replaces the one there: brains running
    /// it start the new one at their next think.
    pub fn insert(&mut self, name: &str, tree: BehaviorTree) {
        self.trees.insert(Key::named(name).name(), Arc::new(tree));
        self.generation += 1;
    }

    /// The tree called `name`.
    pub fn get(&self, name: &str) -> Option<&Arc<BehaviorTree>> {
        self.trees.get(name)
    }

    /// The names of every tree, sorted.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.trees.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// How many trees there are.
    pub fn len(&self) -> usize {
        self.trees.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }
}

/// Adds `tree` to the world's [`BehaviorTrees`] under `name` (adding the
/// resource the first time), and reflects [`Brain`] and [`Blackboard`] so
/// the inspector and saves see them. For trees made in code or embedded in
/// the program; files go through
/// [`Assets::load_behavior_tree`](crate::assets::Assets::load_behavior_tree).
///
/// ```
/// use ponko::ai::{self, BehaviorTree, BehaviorTrees};
/// use ponko::prelude::*;
/// let mut world = World::new();
/// let tree = BehaviorTree::from_json(r#"{"root": {"do": "idle"}}"#).unwrap();
/// ai::add_tree(&mut world, "idle", tree);
/// assert!(world.resource::<BehaviorTrees>().get("idle").is_some());
/// ```
pub fn add_tree(world: &mut World, name: &str, tree: BehaviorTree) {
    world.reflect::<Brain>();
    world.reflect::<Blackboard>();
    if world.get_resource::<BehaviorTrees>().is_none() {
        world.insert_resource(BehaviorTrees::default());
    }
    world.resource_mut::<BehaviorTrees>().insert(name, tree);
}

impl crate::assets::Assets {
    /// Loads a [behaviour tree](BehaviorTree) from a JSON file in
    /// `assets/` and adds it to the world's [`BehaviorTrees`] under its
    /// path, the name a [`Brain`] runs it by. With [hot
    /// reload](crate::game::Game::hot_reload) on, saving the file replaces
    /// the tree while the game runs (brains running it start it over); a
    /// file that doesn't compile keeps the old tree and prints why.
    ///
    /// ```
    /// use ponko::ai::{BehaviorTrees, Brain, Blackboard};
    /// use ponko::assets::MemorySource;
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_source(MemorySource::new("built-in").with("ai/idle.json", r#"{"root": {"do": "idle"}}"#));
    ///     ctx.assets.load_behavior_tree(&mut ctx.world, "ai/idle.json");
    ///     ctx.world.spawn((Brain::new("ai/idle.json"), Blackboard::new()));
    ///     assert_eq!(ctx.world.resource::<BehaviorTrees>().names(), ["ai/idle.json"]);
    /// });
    /// ```
    ///
    /// # Panics
    /// If the file is missing or isn't a valid tree, with the engine's
    /// message naming the node and the problem.
    /// [`try_load_behavior_tree`](Self::try_load_behavior_tree) returns the
    /// error instead.
    pub fn load_behavior_tree(&mut self, world: &mut World, path: &str) {
        self.try_load_behavior_tree(world, path)
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// [`load_behavior_tree`](Self::load_behavior_tree), returning the
    /// error instead of panicking; hot reload is the same.
    ///
    /// # Errors
    /// [`NotFound`](crate::error::ErrorKind::NotFound) or
    /// [`Io`](crate::error::ErrorKind::Io) if the file can't be read, and
    /// [`BehaviorTree::from_json`]'s errors, with `path`. A failed load adds
    /// nothing and doesn't watch the file.
    pub fn try_load_behavior_tree(&mut self, world: &mut World, path: &str) -> Result<(), crate::error::Error> {
        let name = Key::named(path).name();
        self.load_data(
            world,
            path,
            Box::new(move |world: &mut World, text: &str| {
                add_tree(world, name, BehaviorTree::from_json(text)?);
                Ok(())
            }),
        )
    }
}

/// An agent's mind: which [tree](BehaviorTree) it runs, how often it
/// thinks, and the action it is doing. Put it on an entity with a
/// [`Blackboard`]; [`think`] runs it.
///
/// Its JSON (inspector, saves) is `{"tree": "ai/guard.json", "period": 1,
/// "action": "patrol", "status": "running"}`; `action` and `status` are for
/// reading only. A brain read back from JSON starts its tree over.
#[derive(Component, Clone, Debug)]
pub struct Brain {
    tree_name: &'static str,
    /// The tree the node states are for.
    bound: Option<Arc<BehaviorTree>>,
    /// The [`BehaviorTrees`] generation `bound` was looked up in.
    generation: u64,
    period: u32,
    thinks: u32,
    states: Vec<NodeState>,
    /// The running `do` node.
    action: Option<u16>,
    /// What the game said about it.
    report: Status,
    started: bool,
    wake: bool,
}

impl Brain {
    /// A brain that runs the tree called `tree` (for a file, its path under
    /// `assets/`), thinking every tick.
    pub fn new(tree: &str) -> Brain {
        Brain {
            tree_name: Key::named(tree).name(),
            bound: None,
            generation: u64::MAX,
            period: 1,
            thinks: 0,
            states: Vec::new(),
            action: None,
            report: Status::Running,
            started: false,
            wake: false,
        }
    }

    /// This brain, thinking every `period` ticks (see [`set_period`](Self::set_period)).
    pub fn with_period(mut self, period: u32) -> Brain {
        self.set_period(period);
        self
    }

    /// Thinks every `period` ticks (1 is every tick; 0 counts as 1). Brains
    /// with the same period think on different ticks, spread by entity. A
    /// brain still thinks at once when its action ends or it is
    /// [woken](Self::wake); its action carries on between thinks.
    pub fn set_period(&mut self, period: u32) {
        self.period = period.max(1);
    }

    /// How many ticks apart it thinks.
    pub fn period(&self) -> u32 {
        self.period
    }

    /// The name of the tree it runs.
    pub fn tree(&self) -> &'static str {
        self.tree_name
    }

    /// Whether it has found its tree (it does at its first think after the
    /// tree is loaded).
    pub fn has_tree(&self) -> bool {
        self.bound.is_some()
    }

    /// The action it is doing, if any.
    pub fn action(&self) -> Option<Key> {
        let tree = self.bound.as_ref()?;
        tree.action_of(self.action? as usize)
    }

    /// Whether it is doing `action`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::ai::{Brain, Key};
    /// const FLEE: Key = Key::new("flee");
    /// # fn act(ctx: &mut Ctx) {
    /// ctx.world.query::<&mut Brain>().for_each(|brain| {
    ///     if brain.doing(FLEE) {
    ///         // run away; when safe:
    ///         brain.succeed();
    ///     }
    /// });
    /// # }
    /// ```
    pub fn doing(&self, action: Key) -> bool {
        self.action() == Some(action)
    }

    /// Whether its action started at this tick's think: the moment to
    /// pick a destination or play a sound once.
    pub fn started(&self) -> bool {
        self.started && self.action.is_some()
    }

    /// What the game reported for the current action:
    /// [`Running`](Status::Running) until [`succeed`](Self::succeed) or
    /// [`fail`](Self::fail).
    pub fn status(&self) -> Status {
        self.report
    }

    /// The parameter `key` of the current action (its `do` node's other
    /// fields: `{"do": "chase", "speed": 4.5}`).
    pub fn param(&self, key: Key) -> Option<Value> {
        let tree = self.bound.as_ref()?;
        let node = self.action? as usize;
        tree.params(node).iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
    }

    /// The parameter `key` of the current action, if it is a number.
    pub fn param_num(&self, key: Key) -> Option<f32> {
        self.param(key).and_then(Value::as_num)
    }

    /// Reports the current action done and worked: the tree goes on at the
    /// next tick's think, whatever the period.
    pub fn succeed(&mut self) {
        if self.action.is_some() {
            self.report = Status::Success;
            self.wake = true;
        }
    }

    /// Reports the current action done and failed (the target is gone, no
    /// path): the tree goes on at the next tick's think.
    pub fn fail(&mut self) {
        if self.action.is_some() {
            self.report = Status::Failure;
            self.wake = true;
        }
    }

    /// Thinks at the next tick, whatever the period (something it should
    /// react to happened: a noise, a hit).
    pub fn wake(&mut self) {
        self.wake = true;
    }

    /// How many times it has thought.
    pub fn thinks(&self) -> u32 {
        self.thinks
    }

    /// Whether [`think`] would run it at sim tick `tick` for an entity of
    /// index `index`: when woken, before it has a tree, or when
    /// `(tick + index)` is a multiple of its period. For code that runs
    /// brains itself ([`Brain::think`]) and wants the same spread.
    pub fn due(&self, index: u32, tick: u64) -> bool {
        self.wake
            || self.bound.is_none()
            || self.period <= 1
            || (tick + u64::from(index)).is_multiple_of(u64::from(self.period))
    }

    /// Thinks once with `tree`: runs it from the root over `bb`, at sim tick
    /// `tick` of `dt` seconds. [`think`] does this for every due brain; call
    /// it yourself to run brains outside the world (a tool, a test, a
    /// native game's own storage). A tree other than the last one starts
    /// over, and sets its blackboard defaults.
    ///
    /// ```
    /// use ponko::ai::{BehaviorTree, Blackboard, Brain, Key};
    /// use std::sync::Arc;
    /// let tree = Arc::new(BehaviorTree::from_json(r#"{"root": {"sequence": [
    ///     {"do": "aim"}, {"do": "fire"}
    /// ]}}"#).unwrap());
    /// let (mut brain, mut bb) = (Brain::new("gun"), Blackboard::new());
    /// brain.think(&tree, &mut bb, 0, 1.0 / 60.0);
    /// assert!(brain.doing(Key::new("aim")) && brain.started());
    /// brain.succeed();
    /// brain.think(&tree, &mut bb, 1, 1.0 / 60.0);
    /// assert!(brain.doing(Key::new("fire")));
    /// ```
    pub fn think(&mut self, tree: &Arc<BehaviorTree>, bb: &mut Blackboard, tick: u64, dt: f32) {
        if self.bound.as_ref().is_none_or(|b| b.id() != tree.id()) {
            self.bind(tree.clone(), bb);
        }
        self.run(tree, bb, tick, dt);
    }

    /// One think with the tree it is bound to.
    fn run(&mut self, tree: &BehaviorTree, bb: &mut Blackboard, tick: u64, dt: f32) {
        self.thinks = self.thinks.wrapping_add(1);
        self.wake = false;
        let mut t = Tick {
            states: &mut self.states,
            bb,
            thinks: self.thinks,
            now: tick,
            dt,
            prev_action: self.action.take(),
            report: self.report,
            action: None,
            started: false,
        };
        tree.tick(0, &mut t);
        let (action, started) = (t.action, t.started);
        self.action = action;
        self.started = started;
        if started || action.is_none() {
            self.report = Status::Running;
        }
    }

    fn bind(&mut self, tree: Arc<BehaviorTree>, bb: &mut Blackboard) {
        for (key, value) in tree.defaults() {
            bb.set_default(key, value);
        }
        self.states.clear();
        self.states.resize(tree.len(), NodeState::NEW);
        self.action = None;
        self.report = Status::Running;
        self.started = false;
        // A new tree: think at once, not at the next period.
        self.wake = true;
        self.bound = Some(tree);
    }

    fn unbind(&mut self) {
        self.bound = None;
        self.states.clear();
        self.action = None;
        self.report = Status::Running;
        self.started = false;
    }

    /// The tree as the debug view shows it: every node in order, with its
    /// depth, its label, what it did at the last think (`None` if the
    /// think didn't reach it) and, for a child of a `utility` node, its
    /// score then. Empty before the brain has found its tree.
    ///
    /// ```
    /// use ponko::ai::{BehaviorTree, Blackboard, Brain, Status};
    /// use std::sync::Arc;
    /// let tree = Arc::new(BehaviorTree::from_json(r#"{"root": {"selector": [
    ///     {"check": "alert"}, {"do": "patrol"}
    /// ]}}"#).unwrap());
    /// let mut brain = Brain::new("guard");
    /// brain.think(&tree, &mut Blackboard::new(), 0, 0.1);
    /// let lines: Vec<_> = brain.trace().iter().map(|l| (l.depth, l.label, l.status)).collect();
    /// assert_eq!(lines, [
    ///     (0, "selector", Some(Status::Running)),
    ///     (1, "check alert", Some(Status::Failure)),
    ///     (1, "do patrol", Some(Status::Running)),
    /// ]);
    /// ```
    pub fn trace(&self) -> Vec<TraceLine<'_>> {
        let Some(tree) = &self.bound else {
            return Vec::new();
        };
        tree.nodes
            .iter()
            .zip(&self.states)
            .map(|(node, state)| TraceLine {
                depth: node.depth as usize,
                label: &node.label,
                status: (state.seen == self.thinks).then_some(state.status),
                score: (state.scored == self.thinks).then_some(state.score),
            })
            .collect()
    }
}

/// One node of [`Brain::trace`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceLine<'a> {
    /// How deep it is (the root is 0).
    pub depth: usize,
    /// What it is: `"do chase (speed: 4.5)"`, `"check health below 0.3"`.
    pub label: &'a str,
    /// What it did at the last think, if the think reached it.
    pub status: Option<Status>,
    /// Its score at the last think, for a child of a `utility` node.
    pub score: Option<f32>,
}

impl Serialize for Brain {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        BrainJson {
            tree: self.tree_name.to_owned(),
            period: self.period,
            action: self.action().map(|k| k.name().to_owned()),
            status: self.report,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Brain {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Brain, D::Error> {
        let json = BrainJson::deserialize(d)?;
        Ok(Brain::new(&json.tree).with_period(json.period))
    }
}

#[derive(Serialize, Deserialize)]
struct BrainJson {
    tree: String,
    #[serde(default = "one")]
    period: u32,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    status: Status,
}

fn one() -> u32 {
    1
}

/// The sim system that runs every due [`Brain`] on its entity's
/// [`Blackboard`], on all cores: add it with `.sim_system(ai::think)`,
/// after the systems that write blackboards and before those that act.
///
/// A brain thinks when its [period](Brain::period) comes round, when its
/// action ended, when it was [woken](Brain::wake) or its tree changed. A
/// brain whose entity has no blackboard gets an empty one. A brain whose
/// tree isn't loaded does nothing. Without any [`BehaviorTrees`] it
/// returns at once.
pub fn think(ctx: &mut Ctx) {
    if ctx.world.get_resource::<BehaviorTrees>().is_none() {
        return;
    }
    let (tick, dt) = (ctx.time.sim_tick(), ctx.time.dt());
    let mut bare = Vec::new();
    ctx.world
        .query::<Entity>()
        .filter::<(With<Brain>, Without<Blackboard>)>()
        .for_each(|e| bare.push(e));
    for e in bare {
        ctx.world.insert(e, Blackboard::new());
    }
    ctx.world.resource_scope::<BehaviorTrees, _>(|world, trees| {
        world
            .query::<(Entity, &mut Brain, &mut Blackboard)>()
            .par_for_each(|(e, brain, bb)| {
                brain.started = false;
                if brain.generation != trees.generation {
                    brain.generation = trees.generation;
                    match trees.get(brain.tree_name) {
                        Some(tree) => {
                            if brain.bound.as_ref().is_none_or(|b| !Arc::ptr_eq(b, tree)) {
                                brain.bind(tree.clone(), bb);
                            }
                        }
                        None => brain.unbind(),
                    }
                }
                if !brain.due(e.index(), tick) {
                    return;
                }
                // Taken out and put back: no reference count traffic
                // between cores.
                if let Some(tree) = brain.bound.take() {
                    brain.run(&tree, bb, tick, dt);
                    brain.bound = Some(tree);
                }
            });
    });
}

/// The think period for an agent `distance` away from what matters (the
/// camera, the player): the period of the first band whose distance it is
/// within, or `beyond` past them all. Bands go from near to far.
///
/// ```
/// use ponko::ai::lod_period;
/// let bands = [(20.0, 1), (60.0, 4)];
/// assert_eq!(lod_period(5.0, &bands, 16), 1);
/// assert_eq!(lod_period(30.0, &bands, 16), 4);
/// assert_eq!(lod_period(500.0, &bands, 16), 16);
/// ```
pub fn lod_period(distance: f32, bands: &[(f32, u32)], beyond: u32) -> u32 {
    bands
        .iter()
        .find(|(d, _)| distance <= *d)
        .map_or(beyond, |&(_, p)| p)
        .max(1)
}
