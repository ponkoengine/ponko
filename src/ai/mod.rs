//! Game AI: behaviour trees and utility scoring, as data.
//!
//! **Maturity: tested**, from the guide chapter `ai.md`. Game: none: the reference games keep their
//! AI state by hand. Tests: `tests/ai.rs`, `tests/ai_guards.rs`. Platforms: Linux, macOS and
//! Windows, headless (the workspace tests in the list of checks). Graphics: not involved. Missing:
//! a reference game that uses it.
//!
//! An optional kit: nothing in the engine runs it, and a game that doesn't
//! use it pays nothing. Plain systems with a `match` on an enum are the
//! simplest AI and stay the first choice for an enemy with two or three
//! moods (see the guide's "Game AI" chapter for when the kit pays off).
//!
//! # Design
//!
//! - **Trees decide, systems act.** A [`BehaviorTree`] reads and writes
//!   only an agent's [`Blackboard`] and picks an *action* by name (or
//!   [several at once](#several-actions-at-once)). The
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
//!   goes to the earlier child; `random` and `chance` draw from the game's
//!   [seed](crate::game::Game::seed), the brain's tick and its entity. The
//!   same seed, blackboards and signals give the same choices.
//! - **Saved.** A brain's JSON holds its memory (the running nodes, how far
//!   its waits and cooldowns are), so a world read back from a save carries
//!   on at the same running node, counting ticks on from the save.
//! - **Events.** A `wait_for` node waits for a [`Signal`], an event the game
//!   sends (or [`forward`]s from the engine's own): the brain doesn't poll a
//!   blackboard entry, and the signal wakes it whatever its period.
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
//! | `{"parallel": [..], "success": "all", "failure": "any"}` | all its children at once; succeeds when that many succeeded (`"all"`, `"any"` or a count), fails when that many failed or all finished short; then stops the ones still running. Its children's actions run side by side; two children may not share an action name |
//! | `{"random": [..]}` | its children in a random order until one doesn't fail; a running child carries on |
//! | `{"invert": node}` | success and failure swapped |
//! | `{"succeed": node}` | success when the node finishes either way |
//! | `{"check": "key"}` | succeeds if the entry is true, a number other than 0, a point or an entity |
//! | `{"check": "key", "below" / "above" / "is": v}` | compares the entry with `v`, or with the entry named by `v` if it is a string |
//! | `{"set": "key", "to": v}` | writes the blackboard; succeeds |
//! | `{"do": "action", ...}` | asks the game for an action; runs until the game reports; other fields are its parameters ([`Brain::param`]) |
//! | `{"wait": seconds}` | runs for that long, then succeeds |
//! | `{"cooldown": seconds, "child": node}` | fails for that long after the child succeeded |
//! | `{"chance": p}` | succeeds with probability `p` (0 to 1), drawn each time it is reached |
//! | `{"wait_for": "signal", "timeout": s, "store": "key"}` | runs until the brain hears the [`Signal`]; then succeeds, writing the signal's value to `key`; fails after `timeout` seconds (if given; 0 asks only this think) |
//! | `{"subtree": "ai/patrol.json"}` | runs another tree (its root, with its blackboard defaults) by name; see [`BehaviorTrees`] |
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
//! `[check alert, do chase]` stops chasing once `alert` is false. A running
//! `parallel` likewise keeps its finished children's results but asks
//! its checks again: `{"parallel": [{"do": "aim"}, {"check": "alert"}]}`
//! aims while alert.
//!
//! # Several actions at once
//!
//! A `parallel` whose children hold `do` nodes runs their actions side by
//! side: `{"parallel": [{"do": "move", "to": [4, 2]}, {"do": "aim"}]}`
//! moves and aims. The design:
//!
//! - **An action is named by its name.** The game asks
//!   [`Brain::doing`]`(AIM)` and reports with
//!   [`Brain::succeed_action`]`(AIM)` / [`Brain::fail_action`], so a system
//!   that carries out one action needn't know what else runs. For that to
//!   be unambiguous, two children of a `parallel` that could run an action
//!   of the same name at once are refused when the tree compiles (rename
//!   one: `aim_left`, `aim_right`). Not refused: the same action in two
//!   branches that never run together (a selector's).
//! - **The single-action calls keep their meaning.** [`Brain::action`],
//!   [`started`](Brain::started), [`status`](Brain::status),
//!   [`param`](Brain::param), [`succeed`](Brain::succeed) and
//!   [`fail`](Brain::fail) are about the *first* action in tree order,
//!   which is the only one in a tree without such a `parallel`. So
//!   existing trees, systems and saves behave as before.
//!   [`Brain::actions`] lists them all, and [`started_action`](Brain::started_action)
//!   and [`action_param`](Brain::action_param) ask about one.
//! - **Each action ends on its own.** A reported action ends its `do`
//!   node; the others carry on. When the `parallel` itself ends (its
//!   policy decided, or a higher branch took over), every action below it
//!   is dropped, as one action was before.
//! - **No new cost for one action.** A brain keeps its actions in a small
//!   list held inline for up to two, reused at every think: no heap
//!   allocation per think, and none at all with two actions or fewer.
//!   Saves list the extra actions next to the first (`"also"` in the
//!   memory), so a save with one action reads and writes as before.
//!
//! Rejected: action *channels* (legs, hands, voice) declared by the tree,
//! where a new action on a channel replaces the old one. It needs a
//! schema every game would have to share, and the policy of a `parallel`
//! already says what runs together.
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
use tree::{Act, Acts, NodeState, Tick};

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
///
/// Trees here may run each other as subtrees (`{"subtree": "name"}`): each
/// is kept as given and linked, with the trees it names put in place of
/// those nodes. Adding or replacing a tree links again every tree that uses
/// it, so a changed subtree reaches every tree that runs it.
#[derive(Default)]
pub struct BehaviorTrees {
    /// Linked: what brains run.
    trees: HashMap<&'static str, Arc<BehaviorTree>>,
    /// As given, to link again.
    sources: HashMap<&'static str, Arc<BehaviorTree>>,
    /// Bumped on every change, so brains look their tree up again only then.
    generation: u64,
    /// Trees whose file `Assets` reloads.
    pub(crate) files: std::collections::HashSet<&'static str>,
}

impl BehaviorTrees {
    /// Adds a tree under `name`, or replaces the one there: brains running
    /// it, or a tree that uses it as a subtree, start the new one at their
    /// next think. A subtree that isn't here yet is a node that fails until
    /// it is added.
    ///
    /// # Panics
    /// If the tree is, through its subtrees, part of itself (a cycle), with
    /// the names on the cycle; [`try_insert`](Self::try_insert) returns the
    /// error instead.
    pub fn insert(&mut self, name: &str, tree: BehaviorTree) {
        self.try_insert(name, tree).unwrap_or_else(|e| panic!("{e}"));
    }

    /// [`insert`](Self::insert), returning the error instead of panicking.
    /// On an error nothing changes.
    ///
    /// ```
    /// use ponko::ai::{BehaviorTree, BehaviorTrees};
    /// let mut trees = BehaviorTrees::default();
    /// let tree = |json: &str| BehaviorTree::from_json(json).unwrap();
    /// trees.try_insert("patrol", tree(r#"{"root": {"do": "walk"}}"#)).unwrap();
    /// trees.try_insert("guard", tree(r#"{"root": {"subtree": "patrol"}}"#)).unwrap();
    /// assert_eq!(trees.get("guard").unwrap().actions()[0].name(), "walk");
    ///
    /// // A tree that runs itself, through another:
    /// let error = trees
    ///     .try_insert("patrol", tree(r#"{"root": {"subtree": "guard"}}"#))
    ///     .unwrap_err();
    /// assert!(error.to_string().contains("patrol -> guard -> patrol"), "{error}");
    /// ```
    ///
    /// # Errors
    /// [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) for a
    /// cycle, naming every tree on it;
    /// [`Limit`](crate::error::ErrorKind::Limit) if a linked tree would have
    /// more than 65,535 nodes.
    /// A tree already shared (`Arc<BehaviorTree>`) goes in as it is.
    pub fn try_insert(&mut self, name: &str, tree: impl Into<Arc<BehaviorTree>>) -> Result<(), crate::error::Error> {
        let name = Key::named(name).name();
        let tree = tree.into();
        let mut sources = self.sources.clone();
        sources.insert(name, tree);
        // This tree, and every tree that uses it (its linked form names
        // every subtree below it, loaded or not).
        let mut relink: Vec<&'static str> = self
            .trees
            .iter()
            .filter(|(_, t)| t.subtrees().contains(&name))
            .map(|(&n, _)| n)
            .collect();
        relink.sort_unstable();
        // The new tree first, so a cycle is named from it.
        relink.retain(|n| *n != name);
        relink.insert(0, name);
        let resolve = |n: &str| sources.get(n).cloned();
        let mut linked = Vec::with_capacity(relink.len());
        for n in relink {
            let source = &sources[n];
            let tree = if source.subtrees().is_empty() {
                source.clone()
            } else {
                Arc::new(BehaviorTree::link(n, source, &resolve)?)
            };
            linked.push((n, tree));
        }
        self.trees.extend(linked);
        self.sources = sources;
        self.generation += 1;
        Ok(())
    }

    /// The tree called `name`, with its subtrees linked in.
    pub fn get(&self, name: &str) -> Option<&Arc<BehaviorTree>> {
        self.trees.get(name)
    }

    /// Whether `name` holds a tree compiled from the same JSON as `tree`.
    fn holds(&self, name: &str, tree: &BehaviorTree) -> bool {
        self.sources.get(name).is_some_and(|t| t.same_source(tree))
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
        let sources = self.asset_sources.clone();
        self.load_data(
            world,
            path,
            Box::new(move |world: &mut World, text: &str| {
                let tree = BehaviorTree::from_json(text)?;
                let mut found = Vec::new();
                read_subtrees(world, &sources, &tree, &mut vec![name], &mut found)?;
                for (sub, tree) in found {
                    try_add_tree(world, sub, tree)?;
                }
                try_add_tree(world, name, tree)
            }),
        )?;
        // Watch the subtrees' files too, so editing one reloads it (and
        // every tree that runs it).
        let trees = world.resource_mut::<BehaviorTrees>();
        trees.files.insert(name);
        let subtrees: Vec<&'static str> = trees
            .get(name)
            .map(|t| t.subtrees().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !trees.files.contains(s) && trees.sources.contains_key(s))
            .collect();
        for sub in subtrees {
            if !world.resource::<BehaviorTrees>().files.contains(sub) {
                self.try_load_behavior_tree(world, sub)?;
            }
        }
        Ok(())
    }
}

/// Reads the files of `tree`'s subtrees that aren't loaded yet, and theirs,
/// into `found`, deepest first. `stack`: the files being read, for cycles.
fn read_subtrees(
    world: &World,
    sources: &crate::assets::SharedSources,
    tree: &BehaviorTree,
    stack: &mut Vec<&'static str>,
    found: &mut Vec<(&'static str, BehaviorTree)>,
) -> Result<(), crate::error::Error> {
    for &sub in tree.subtrees() {
        if let Some(at) = stack.iter().position(|n| *n == sub) {
            let mut chain = stack[at..].to_vec();
            chain.push(sub);
            return Err(crate::error::Error::new(
                crate::error::ErrorKind::Invalid,
                format!(
                    "behaviour tree {}: subtree cycle: {} (a tree can't contain itself)",
                    stack[0],
                    chain.join(" -> ")
                ),
            ));
        }
        let loaded = world
            .get_resource::<BehaviorTrees>()
            .is_some_and(|t| t.sources.contains_key(sub));
        if loaded || found.iter().any(|(n, _)| *n == sub) {
            continue;
        }
        let text = sources
            .find(sub)
            .and_then(|f| f.into_text(sub))
            .map_err(|e| e.context(format!("subtree of {}", stack.last().copied().unwrap_or(sub))))?;
        let subtree = BehaviorTree::from_json(&text).map_err(|e| e.context(sub).with_path(sub))?;
        stack.push(sub);
        read_subtrees(world, sources, &subtree, stack, found)?;
        stack.pop();
        found.push((sub, subtree));
    }
    Ok(())
}

/// [`add_tree`], returning the error; a tree compiled from the same JSON as
/// the one there changes nothing (brains running it carry on).
fn try_add_tree(world: &mut World, name: &'static str, tree: BehaviorTree) -> Result<(), crate::error::Error> {
    if world
        .get_resource::<BehaviorTrees>()
        .is_some_and(|t| t.holds(name, &tree))
    {
        return Ok(());
    }
    world.reflect::<Brain>();
    world.reflect::<Blackboard>();
    if world.get_resource::<BehaviorTrees>().is_none() {
        world.insert_resource(BehaviorTrees::default());
    }
    world.resource_mut::<BehaviorTrees>().try_insert(name, tree)
}

/// An agent's mind: which [tree](BehaviorTree) it runs, how often it
/// thinks, and the action it is doing. Put it on an entity with a
/// [`Blackboard`]; [`think`] runs it.
///
/// Its JSON (inspector, saves) is `{"tree": "ai/guard.json", "period": 1,
/// "action": "patrol", "status": "running", "memory": {..}}`; `action` is
/// for reading only. `memory` (once it has thought) holds the running
/// nodes, by index, and how far its waits and cooldowns are: a brain read
/// back with it carries on where it was, counting its ticks on from the
/// save, if its tree is the same ([`BehaviorTree::fingerprint`]); without
/// it, or with another tree, it starts its tree over.
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
    /// The running `do` nodes, in tree order, with what the game said
    /// about each.
    actions: Acts,
    wake: bool,
    /// Added to the sim tick to give the brain's own tick, which waits,
    /// cooldowns, think periods and random numbers count in: 0, unless the
    /// brain was read back from a save.
    offset: u64,
    /// The brain's tick at the last think pass, for saves.
    clock: u64,
    /// Read back from a save: its tree's memory, used when it finds that
    /// tree. Until then its tick stays the one after the save.
    restore: Option<Box<Memory>>,
    /// Signals heard since the last think.
    signals: Vec<(Key, Option<Value>)>,
    /// The seed for `random` and `chance` nodes when the game runs the
    /// brain itself ([`think`](Self::think)); [`ai::think`](crate::ai::think) uses the
    /// game's.
    seed: u64,
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
            actions: Acts::new(),
            wake: false,
            offset: 0,
            clock: 0,
            restore: None,
            signals: Vec::new(),
            seed: crate::random::DEFAULT_SEED,
        }
    }

    /// This brain, drawing the random numbers of `random` and `chance`
    /// nodes from `seed` when you run it yourself ([`think`](Self::think)):
    /// give each agent its own. The [`ai::think`](crate::ai::think) system ignores it and
    /// uses the game's [seed](crate::game::Game::seed) and the entity.
    pub fn with_seed(mut self, seed: u64) -> Brain {
        self.set_seed(seed);
        self
    }

    /// Sets the seed of [`with_seed`](Self::with_seed).
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    /// The seed of [`with_seed`](Self::with_seed).
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Hears `signal`: a `wait_for` node waiting for it succeeds at the
    /// next think, which comes at the next tick whatever the period. A
    /// signal its tree doesn't wait for wakes it all the same. Signals
    /// heard are forgotten after that think; the same signal twice before
    /// it counts once. [`ai::think`](crate::ai::think) delivers the [`Signal`] events
    /// sent to a brain this way.
    ///
    /// ```
    /// use ponko::ai::{BehaviorTree, Blackboard, Brain, Key};
    /// use std::sync::Arc;
    /// let tree = Arc::new(BehaviorTree::from_json(r#"{"root": {"sequence": [
    ///     {"wait_for": "door_opened"}, {"do": "enter"}
    /// ]}}"#).unwrap());
    /// let (mut brain, mut bb) = (Brain::new("guest"), Blackboard::new());
    /// brain.think(&tree, &mut bb, 0, 0.1);
    /// assert_eq!(brain.action(), None); // waiting
    /// brain.signal(Key::new("door_opened"));
    /// brain.think(&tree, &mut bb, 1, 0.1);
    /// assert!(brain.doing(Key::new("enter")));
    /// ```
    pub fn signal(&mut self, signal: Key) {
        hear(&mut self.signals, signal, None);
        self.wake = true;
    }

    /// [`signal`](Self::signal) with a value, which a `wait_for` node with
    /// `"store": "key"` writes to the blackboard (who shouted, where).
    pub fn signal_with(&mut self, signal: Key, value: impl Into<Value>) {
        hear(&mut self.signals, signal, Some(value.into()));
        self.wake = true;
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

    /// The action it is doing, if any; with [several at
    /// once](Self::actions), the first in tree order.
    pub fn action(&self) -> Option<Key> {
        self.key_of(self.actions.first()?)
    }

    /// Every action it is doing, in tree order: one, or several when a
    /// `parallel` node runs `do` nodes side by side (move and aim). Their
    /// names differ (a tree where two could share one is refused).
    ///
    /// ```
    /// use ponko::ai::{BehaviorTree, Blackboard, Brain, Key};
    /// use std::sync::Arc;
    /// const MOVE: Key = Key::new("move");
    /// const AIM: Key = Key::new("aim");
    /// let tree = Arc::new(BehaviorTree::from_json(r#"{"root": {"parallel": [
    ///     {"do": "move", "to": [4, 2]}, {"do": "aim"}
    /// ]}}"#).unwrap());
    /// let (mut brain, mut bb) = (Brain::new("soldier"), Blackboard::new());
    /// brain.think(&tree, &mut bb, 0, 0.1);
    /// assert_eq!(brain.actions().collect::<Vec<_>>(), [MOVE, AIM]);
    /// assert!(brain.doing(MOVE) && brain.doing(AIM));
    /// // Each is reported on its own; the other carries on.
    /// brain.fail_action(AIM);
    /// brain.think(&tree, &mut bb, 1, 0.1);
    /// assert_eq!(brain.actions().collect::<Vec<_>>(), []); // all must succeed
    /// ```
    pub fn actions(&self) -> impl Iterator<Item = Key> + '_ {
        self.actions.iter().filter_map(|a| self.key_of(a))
    }

    fn key_of(&self, act: &Act) -> Option<Key> {
        self.bound.as_ref()?.action_of(act.node as usize)
    }

    /// The running action called `action`.
    fn act(&self, action: Key) -> Option<&Act> {
        let tree = self.bound.as_ref()?;
        self.actions
            .iter()
            .find(|a| tree.action_of(a.node as usize) == Some(action))
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
        self.act(action).is_some()
    }

    /// Whether its action started at this tick's think: the moment to
    /// pick a destination or play a sound once. With several actions, the
    /// first; [`started_action`](Self::started_action) asks about one.
    pub fn started(&self) -> bool {
        self.actions.first().is_some_and(|a| a.started)
    }

    /// Whether it is doing `action` and that started at this tick's think.
    pub fn started_action(&self, action: Key) -> bool {
        self.act(action).is_some_and(|a| a.started)
    }

    /// What the game reported for the current action (the first, with
    /// several): [`Running`](Status::Running) until
    /// [`succeed`](Self::succeed) or [`fail`](Self::fail), and while it
    /// does none.
    pub fn status(&self) -> Status {
        self.actions.first().map_or(Status::Running, |a| a.report)
    }

    /// The parameter `key` of the current action (its `do` node's other
    /// fields: `{"do": "chase", "speed": 4.5}`); with several actions, of
    /// the first ([`action_param`](Self::action_param) names one).
    pub fn param(&self, key: Key) -> Option<Value> {
        let tree = self.bound.as_ref()?;
        let node = self.actions.first()?.node as usize;
        tree.params(node).iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
    }

    /// The parameter `key` of the running action `action`.
    pub fn action_param(&self, action: Key, key: Key) -> Option<Value> {
        let tree = self.bound.as_ref()?;
        let node = self.act(action)?.node as usize;
        tree.params(node).iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
    }

    /// The parameter `key` of the current action, if it is a number.
    pub fn param_num(&self, key: Key) -> Option<f32> {
        self.param(key).and_then(Value::as_num)
    }

    /// Reports the current action done and worked: the tree goes on at the
    /// next tick's think, whatever the period. With several actions, the
    /// first; [`succeed_action`](Self::succeed_action) names one.
    pub fn succeed(&mut self) {
        self.report(0, Status::Success);
    }

    /// Reports the current action done and failed (the target is gone, no
    /// path): the tree goes on at the next tick's think. With several
    /// actions, the first; [`fail_action`](Self::fail_action) names one.
    pub fn fail(&mut self) {
        self.report(0, Status::Failure);
    }

    /// Reports the running action `action` done and worked (nothing if it
    /// isn't doing it); its other actions carry on.
    pub fn succeed_action(&mut self, action: Key) {
        if let Some(n) = self.position(action) {
            self.report(n, Status::Success);
        }
    }

    /// Reports the running action `action` done and failed (nothing if it
    /// isn't doing it); its other actions carry on.
    pub fn fail_action(&mut self, action: Key) {
        if let Some(n) = self.position(action) {
            self.report(n, Status::Failure);
        }
    }

    fn position(&self, action: Key) -> Option<usize> {
        let tree = self.bound.as_ref()?;
        self.actions
            .iter()
            .position(|a| tree.action_of(a.node as usize) == Some(action))
    }

    /// Sets the report of the `n`th running action, and wakes the brain.
    fn report(&mut self, n: usize, status: Status) {
        if let Some(act) = self.actions.get_mut(n) {
            act.report = status;
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
            || (self.clock_at(tick).wrapping_add(u64::from(index))).is_multiple_of(u64::from(self.period))
    }

    /// The brain's own tick at sim tick `tick`: the same, unless it was
    /// read back from a save, where it goes on from the tick it was saved
    /// at.
    fn clock_at(&self, tick: u64) -> u64 {
        match &self.restore {
            Some(memory) => memory.clock.wrapping_add(1),
            None => tick.wrapping_add(self.offset),
        }
    }

    /// Starts a think pass at sim tick `tick`: a brain read back from a
    /// save goes on at the tick after the one it was saved at.
    fn pass(&mut self, tick: u64) -> u64 {
        self.offset = self.clock_at(tick).wrapping_sub(tick);
        self.clock = tick.wrapping_add(self.offset);
        self.clock
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
        let now = self.pass(tick);
        if self.bound.as_ref().is_none_or(|b| b.id() != tree.id()) {
            self.bind(tree.clone(), bb);
        }
        self.run(tree, bb, now, dt, self.seed, 0);
    }

    /// One think with the tree it is bound to, at the brain's tick `now`.
    /// `seed` and `who` pick the random stream (with `now`).
    fn run(&mut self, tree: &BehaviorTree, bb: &mut Blackboard, now: u64, dt: f32, seed: u64, who: u64) {
        self.thinks = self.thinks.wrapping_add(1);
        self.wake = false;
        // The actions running now stay in front while the tree runs, and
        // the ones running after it go behind them: no allocation.
        let prev = self.actions.len();
        let mut t = Tick {
            states: &mut self.states,
            bb,
            thinks: self.thinks,
            now,
            dt,
            actions: &mut self.actions,
            prev,
            signals: &self.signals,
            seed,
            who,
            rng: None,
        };
        tree.tick(0, &mut t);
        self.actions.drain(..prev);
        self.signals.clear();
    }

    fn bind(&mut self, tree: Arc<BehaviorTree>, bb: &mut Blackboard) {
        for (key, value) in tree.defaults() {
            bb.set_default(key, value);
        }
        self.states.clear();
        self.states.resize(tree.len(), NodeState::NEW);
        self.actions.clear();
        // A new tree: think at once, not at the next period.
        self.wake = true;
        if let Some(memory) = self.restore.take()
            && memory.fits(&tree)
        {
            // The tree it was saved with: carry on where it was.
            for &(node, seen, status, at) in &memory.nodes {
                self.states[node as usize] = NodeState {
                    at,
                    seen,
                    status,
                    ..NodeState::NEW
                };
            }
            self.thinks = memory.thinks;
            if let Some(node) = memory.running {
                self.actions.push(Act {
                    node,
                    report: memory.status,
                    started: false,
                });
            }
            self.actions.extend(memory.also.iter().map(|&(node, report)| Act {
                node,
                report,
                started: false,
            }));
            self.wake = memory.wake;
        }
        self.bound = Some(tree);
    }

    /// What it would need to carry on after a save: `None` before it has
    /// found its tree (unless it still holds a save's).
    fn memory(&self) -> Option<Memory> {
        let Some(tree) = &self.bound else {
            return self.restore.as_deref().cloned();
        };
        let nodes = self
            .states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.seen != NodeState::NEW.seen || s.at != 0)
            .map(|(i, s)| (i as u16, s.seen, s.status, s.at))
            .collect();
        Some(Memory {
            tree: format!("{:016x}", tree.fingerprint()),
            clock: self.clock,
            thinks: self.thinks,
            running: self.actions.first().map(|a| a.node),
            status: self.status(),
            also: self.actions.iter().skip(1).map(|a| (a.node, a.report)).collect(),
            wake: self.wake,
            nodes,
        })
    }

    fn unbind(&mut self) {
        self.bound = None;
        self.states.clear();
        self.actions.clear();
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
            actions: if self.actions.len() > 1 {
                self.actions().map(|k| k.name().to_owned()).collect()
            } else {
                Vec::new()
            },
            status: self.status(),
            memory: self.memory(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Brain {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Brain, D::Error> {
        let json = BrainJson::deserialize(d)?;
        let mut brain = Brain::new(&json.tree).with_period(json.period);
        brain.restore = json.memory.map(Box::new);
        Ok(brain)
    }
}

#[derive(Serialize, Deserialize)]
struct BrainJson {
    tree: String,
    #[serde(default = "one")]
    period: u32,
    #[serde(default)]
    action: Option<String>,
    /// Every action, when it does several at once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<String>,
    #[serde(default)]
    status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<Memory>,
}

/// A brain's memory in a save: enough to carry on at the same running
/// node, at the same point of its waits and cooldowns.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Memory {
    /// The tree's [fingerprint](BehaviorTree::fingerprint), in hex: the
    /// memory is used only with the same tree.
    tree: String,
    /// The brain's tick at the save.
    clock: u64,
    thinks: u32,
    /// The running `do` node (the first, with several).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    running: Option<u16>,
    /// What the game reported for it.
    #[serde(default)]
    status: Status,
    /// The other running `do` nodes, with their reports, when it does
    /// several actions at once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    also: Vec<(u16, Status)>,
    #[serde(default)]
    wake: bool,
    /// `[node, think last ticked in, status, tick or bits]` for every node
    /// with something to remember.
    nodes: Vec<(u16, u32, Status, u64)>,
}

impl Memory {
    fn fits(&self, tree: &BehaviorTree) -> bool {
        self.tree == format!("{:016x}", tree.fingerprint())
            && self
                .running
                .iter()
                .chain(self.also.iter().map(|(n, _)| n))
                .all(|&n| (n as usize) < tree.len() && tree.action_of(n as usize).is_some())
            && self.nodes.iter().all(|&(n, ..)| (n as usize) < tree.len())
    }
}

/// Adds `signal` to what a brain heard, once (the latest value wins).
fn hear(heard: &mut Vec<(Key, Option<Value>)>, signal: Key, value: Option<Value>) {
    match heard.iter_mut().find(|(k, _)| *k == signal) {
        Some(entry) => entry.1 = value.or(entry.1),
        None => heard.push((signal, value)),
    }
}

/// An event that wakes brains and ends their `wait_for` nodes: send it with
/// `ctx.events.send(Signal::to(guard, NOISE))` (or
/// [`broadcast`](Self::broadcast) for every brain) from a system before
/// [`think`], or turn the engine's own events into signals with
/// [`forward`]. Brains whose tree doesn't wait for it don't hear it.
///
/// ```
/// use ponko::ai::{self, Blackboard, Brain, Key, Signal};
/// use ponko::assets::MemorySource;
/// use ponko::prelude::*;
///
/// const ALARM: Key = Key::new("alarm");
/// let mut world = Game::new("t", 1, 1)
///     .sim_system(|ctx: &mut Ctx| {
///         if ctx.time.sim_tick() == 5 {
///             ctx.events.send(Signal::broadcast(ALARM));
///         }
///     })
///     .sim_system(ai::think)
///     .run_headless(6, |ctx| {
///         ctx.assets.add_source(MemorySource::new("m").with("ai/guard.json",
///             r#"{"root": {"selector": [
///                 {"sequence": [{"wait_for": "alarm"}, {"set": "alerted", "to": true}]},
///                 {"do": "sleep"}
///             ]}}"#));
///         ctx.assets.load_behavior_tree(&mut ctx.world, "ai/guard.json");
///         // Thinks every 100 ticks, but the alarm wakes it at once.
///         ctx.world.spawn((Brain::new("ai/guard.json").with_period(100), Blackboard::new()));
///     });
/// let bb = world.query::<&Blackboard>().single().unwrap();
/// assert!(bb.flag(Key::new("alerted")));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Signal {
    /// The brain's entity; `None` for every brain.
    pub target: Option<Entity>,
    /// What happened.
    pub name: Key,
    /// A value a `wait_for` node with `"store"` writes to the blackboard.
    pub value: Option<Value>,
}

impl Signal {
    /// `name`, for the brain of entity `target`.
    pub fn to(target: Entity, name: Key) -> Signal {
        Signal {
            target: Some(target),
            name,
            value: None,
        }
    }

    /// `name`, for every brain that waits for it.
    pub fn broadcast(name: Key) -> Signal {
        Signal {
            target: None,
            name,
            value: None,
        }
    }

    /// This signal, carrying `value` (who made the noise, where).
    pub fn with(mut self, value: impl Into<Value>) -> Signal {
        self.value = Some(value.into());
        self
    }
}

/// A sim system that turns each `T` event into a [`Signal`] with `map`
/// (or none, for `None`): engine events (a hit, a trigger entered, a sound
/// heard) end `wait_for` nodes without a system of your own. Add it before
/// [`think`].
///
/// ```
/// use ponko::ai::{self, Key, Signal};
/// use ponko::prelude::*;
///
/// struct Shot { by: Entity, at: Entity }
/// const SHOT_AT: Key = Key::new("shot_at");
/// Game::new("t", 1, 1)
///     .sim_system(ai::forward(|shot: &Shot| Some(Signal::to(shot.at, SHOT_AT).with(shot.by))))
///     .sim_system(ai::think)
///     .run_headless(1, |_| {});
/// ```
pub fn forward<T: 'static>(map: impl Fn(&T) -> Option<Signal> + 'static) -> impl FnMut(&mut Ctx) + 'static {
    let mut out = Vec::new();
    move |ctx: &mut Ctx| {
        out.extend(ctx.events.read::<T>().iter().filter_map(&map));
        for signal in out.drain(..) {
            ctx.events.send(signal);
        }
    }
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
    let (tick, dt, seed) = (ctx.time.sim_tick(), ctx.time.dt(), ctx.seed);
    let signals: &[Signal] = ctx.events.read::<Signal>();
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
                for act in &mut brain.actions {
                    act.started = false;
                }
                let now = brain.pass(tick);
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
                if let Some(tree) = &brain.bound {
                    for s in signals {
                        if s.target.is_none_or(|t| t == e) && tree.signals().contains(&s.name) {
                            hear(&mut brain.signals, s.name, s.value);
                            brain.wake = true;
                        }
                    }
                }
                if !brain.due(e.index(), tick) {
                    return;
                }
                // Taken out and put back: no reference count traffic
                // between cores.
                if let Some(tree) = brain.bound.take() {
                    brain.run(&tree, bb, now, dt, seed, e.to_bits());
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
