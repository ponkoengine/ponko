//! Behaviour trees: the JSON format, its compiled form and one think.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value as Json};

use super::utility::{Consideration, Curve, combine_n};
use super::{Blackboard, Key, Status, Value};
use crate::error::{Error, ErrorKind};
use crate::random::{Random, Rng};

/// The most nodes a tree may have.
pub(crate) const MAX_NODES: usize = u16::MAX as usize;
/// The most children a `utility` node may have.
pub(crate) const MAX_UTILITY_CHILDREN: usize = 64;
/// The most children a `parallel` or `random` node may have (their memory
/// is a 64-bit set).
pub(crate) const MAX_SET_CHILDREN: usize = 64;

/// Node kinds before `parallel`, `random`, `chance`, `wait_for` and
/// `subtree`: a `do` node may use the newer names as parameters.
const OLD_KINDS: [&str; 10] = [
    "sequence", "selector", "utility", "invert", "succeed", "check", "set", "do", "wait", "cooldown",
];
const KINDS: [&str; 15] = [
    "sequence", "selector", "utility", "parallel", "random", "invert", "succeed", "check", "set", "do", "wait",
    "wait_for", "cooldown", "chance", "subtree",
];

/// A behaviour tree, compiled from JSON: what an agent does, decided from
/// its [`Blackboard`] only. Shared by every [`Brain`](super::Brain) that
/// runs it; see the [module docs](super) for the format.
///
/// ```
/// use ponko::ai::BehaviorTree;
/// let tree = BehaviorTree::from_json(r#"{
///     "root": {"selector": [
///         {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 4}]},
///         {"do": "patrol"}
///     ]}
/// }"#).unwrap();
/// assert_eq!(tree.len(), 5); // selector, sequence, check, two actions
/// let bad = BehaviorTree::from_json(r#"{"root": {"sequense": []}}"#).unwrap_err();
/// assert!(bad.to_string().contains("root: no node kind"), "{bad}");
/// ```
#[derive(Debug)]
pub struct BehaviorTree {
    /// Unique per compiled tree, so a brain notices a reloaded one.
    id: u64,
    pub(crate) nodes: Vec<Node>,
    kids: Vec<u16>,
    params: Vec<(Key, Value)>,
    terms: Vec<Term>,
    pub(crate) defaults: Vec<(Key, Value)>,
    /// The JSON it was compiled from, to link its subtrees again when one
    /// of them changes.
    source: Arc<Json>,
    /// The trees its `subtree` nodes name, each once: the ones it names
    /// itself, and once linked, the ones those name too.
    subtrees: Vec<&'static str>,
    /// The signals its `wait_for` nodes wait for, each once.
    listens: Vec<Key>,
    /// A hash of its shape, so a saved brain's memory is used only with the
    /// tree it was saved with.
    fingerprint: u64,
}

#[derive(Debug)]
pub(crate) struct Node {
    pub(crate) kind: Kind,
    /// Only checks below it: it reads the blackboard, finishes at once and
    /// changes nothing, so a resumed sequence asks it again.
    pure: bool,
    /// What the debug view shows for it.
    pub(crate) label: String,
    pub(crate) depth: u16,
    /// One past its last descendant: its subtree is `index..end`.
    end: u16,
    /// A `do` node is in its subtree.
    acts: bool,
    /// Children: a range of `kids`.
    kids: (u32, u16),
    /// Score terms under a `utility` node: a range of `terms`.
    score: (u32, u16),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Kind {
    Sequence,
    Selector,
    Utility {
        stick: f32,
    },
    Invert,
    Succeed,
    Check {
        key: Key,
        test: Test,
    },
    Set {
        key: Key,
        value: Value,
    },
    Do {
        action: Key,
        params: (u32, u16),
    },
    Wait {
        seconds: f32,
    },
    Cooldown {
        seconds: f32,
    },
    /// Runs its child, the root of another tree; without a child the tree
    /// wasn't loaded, and it fails.
    Subtree,
    /// Every child at once; done when `success` children succeeded or
    /// `failure` children failed.
    Parallel {
        success: u8,
        failure: u8,
    },
    /// Its children in a random order until one doesn't fail.
    Random,
    /// Succeeds with probability `p`.
    Chance {
        p: f32,
    },
    /// Running until the brain hears `signal`; fails after `timeout`
    /// seconds if that is 0 or more.
    WaitFor {
        signal: Key,
        timeout: f32,
        store: Option<Key>,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Operand {
    Lit(Value),
    Key(Key),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Test {
    Truthy,
    Is(Operand),
    Below(Operand),
    Above(Operand),
}

#[derive(Debug, Clone, Copy)]
enum Term {
    Const(f32),
    Consider(Consideration),
}

/// One brain's memory of one node.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeState {
    /// A wait's start tick; a cooldown's ready tick; the children a
    /// `parallel` finished (a bit set); the child a `sequence` or `random`
    /// was running.
    pub(crate) at: u64,
    /// The think it was last ticked in.
    pub(crate) seen: u32,
    /// The think it was last scored in (children of `utility`).
    pub(crate) scored: u32,
    pub(crate) score: f32,
    pub(crate) status: Status,
}

impl NodeState {
    pub(crate) const NEW: NodeState = NodeState {
        at: 0,
        seen: u32::MAX,
        scored: u32::MAX,
        score: 0.0,
        status: Status::Failure,
    };
}

/// One think of one brain.
pub(crate) struct Tick<'a> {
    pub(crate) states: &'a mut [NodeState],
    pub(crate) bb: &'a mut Blackboard,
    /// This think's number.
    pub(crate) thinks: u32,
    pub(crate) now: u64,
    pub(crate) dt: f32,
    /// The action running before this think, and what the game reported.
    pub(crate) prev_action: Option<u16>,
    pub(crate) report: Status,
    /// The action running after it, and whether it started now.
    pub(crate) action: Option<u16>,
    pub(crate) started: bool,
    /// The signals the brain heard since its last think.
    pub(crate) signals: &'a [(Key, Option<Value>)],
    /// Where `random` and `chance` draw from: the game's seed (or the
    /// brain's) and the entity, with `now`.
    pub(crate) seed: u64,
    pub(crate) who: u64,
    pub(crate) rng: Option<Rng>,
}

impl Tick<'_> {
    /// This think's random numbers, made when first needed.
    fn rng(&mut self) -> &mut Rng {
        let (seed, now, who) = (self.seed, self.now, self.who);
        self.rng
            .get_or_insert_with(|| Random::new(seed, now, AI_STREAM).stream(who))
    }

    /// Ticked at the previous think and still running then.
    fn continuing(&self, i: usize) -> bool {
        let s = &self.states[i];
        s.seen == self.thinks.wrapping_sub(1) && s.status == Status::Running
    }

    fn ticks_for(&self, seconds: f32) -> u64 {
        if seconds <= 0.0 || self.dt <= 0.0 {
            return 0;
        }
        (seconds / self.dt - 1e-4).ceil().max(0.0) as u64
    }
}

/// How `subtree` nodes are linked while a tree compiles.
pub(crate) struct Linker<'a> {
    /// Finds a tree by name; `None` while compiling a tree on its own.
    resolve: Option<&'a dyn Fn(&str) -> Option<Arc<BehaviorTree>>>,
    /// The trees being compiled, outermost first: a name already here is a
    /// cycle.
    stack: Vec<&'static str>,
}

/// The stream number brains draw random numbers from (with the seed and
/// the brain's tick), apart from every system's own.
const AI_STREAM: usize = 0x4149_5f42;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl BehaviorTree {
    /// Compiles a tree from its JSON text.
    ///
    /// # Errors
    /// [`ErrorKind::Json`] if the text isn't JSON or isn't a valid tree; the
    /// message names the node, as a path from the root
    /// (`root/selector[1]/do`), and what is wrong with it.
    /// [`ErrorKind::Limit`] for a tree of more than 65,535 nodes or a
    /// `utility` node with more than 64 children.
    pub fn from_json(text: &str) -> Result<BehaviorTree, Error> {
        let json: Json = serde_json::from_str(text)?;
        BehaviorTree::from_value(&json)
    }

    /// Compiles a tree from parsed JSON (see [`from_json`](Self::from_json)).
    ///
    /// # Errors
    /// As [`from_json`](Self::from_json).
    pub fn from_value(json: &Json) -> Result<BehaviorTree, Error> {
        BehaviorTree::compile(
            json,
            &mut Linker {
                resolve: None,
                stack: Vec::new(),
            },
        )
    }

    /// Compiles `src` again with its `subtree` nodes replaced by the trees
    /// `resolve` finds (a name it doesn't find stays a failing node).
    /// `name` is `src`'s own name, for cycles.
    ///
    /// # Errors
    /// [`ErrorKind::Invalid`] for a cycle (a tree that is, through its
    /// subtrees, part of itself), naming every tree on it;
    /// [`ErrorKind::Limit`] if the linked tree has too many nodes.
    pub(crate) fn link(
        name: &'static str,
        src: &BehaviorTree,
        resolve: &dyn Fn(&str) -> Option<Arc<BehaviorTree>>,
    ) -> Result<BehaviorTree, Error> {
        BehaviorTree::compile(
            &src.source,
            &mut Linker {
                resolve: Some(resolve),
                stack: vec![name],
            },
        )
    }

    fn compile(json: &Json, link: &mut Linker<'_>) -> Result<BehaviorTree, Error> {
        let top = json
            .as_object()
            .ok_or_else(|| bad("the tree", "must be an object with a \"root\" node"))?;
        for key in top.keys() {
            if key != "root" && key != "blackboard" {
                return Err(bad(
                    "the tree",
                    format!("unknown field \"{key}\" (allowed: root, blackboard)"),
                ));
            }
        }
        let mut tree = BehaviorTree {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            nodes: Vec::new(),
            kids: Vec::new(),
            params: Vec::new(),
            terms: Vec::new(),
            defaults: Vec::new(),
            source: Arc::new(json.clone()),
            subtrees: Vec::new(),
            listens: Vec::new(),
            fingerprint: 0,
        };
        if let Some(defaults) = top.get("blackboard") {
            let defaults = defaults
                .as_object()
                .ok_or_else(|| bad("blackboard", "must be an object from names to values"))?;
            for (name, value) in defaults {
                tree.defaults
                    .push((Key::named(name), value_of(value, &format!("blackboard/{name}"))?));
            }
        }
        let root = top.get("root").ok_or_else(|| bad("the tree", "has no \"root\" node"))?;
        tree.node(root, "root", 0, false, link)?;
        tree.fingerprint = tree.shape_hash();
        Ok(tree)
    }

    /// FNV-1a over every node's depth and label (which spell out its kind
    /// and fields).
    fn shape_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        for node in &self.nodes {
            eat(&node.depth.to_le_bytes());
            eat(node.label.as_bytes());
            eat(&[0xff]);
        }
        h
    }

    /// The names of the trees its `subtree` nodes run, each once. A tree
    /// from [`BehaviorTrees`](super::BehaviorTrees) lists the subtrees of
    /// its subtrees too.
    ///
    /// ```
    /// use ponko::ai::BehaviorTree;
    /// let tree = BehaviorTree::from_json(r#"{"root": {"selector": [
    ///     {"subtree": "ai/fight.json"}, {"subtree": "ai/patrol.json"}
    /// ]}}"#).unwrap();
    /// assert_eq!(tree.subtrees(), ["ai/fight.json", "ai/patrol.json"]);
    /// ```
    pub fn subtrees(&self) -> &[&'static str] {
        &self.subtrees
    }

    /// The signals its `wait_for` nodes wait for, each once: [`think`](super::think)
    /// delivers only these to its brains.
    pub fn signals(&self) -> &[Key] {
        &self.listens
    }

    /// A hash of its shape (its nodes, their kinds and fields): equal for
    /// two trees compiled from the same JSON. A saved brain keeps its
    /// memory only if its tree's fingerprint is still the same.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// How many nodes it has.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether it has no nodes (never true for a compiled tree).
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The blackboard values it starts agents with (its `"blackboard"`
    /// object), set on an agent's blackboard where it has no value yet.
    pub fn defaults(&self) -> impl Iterator<Item = (Key, Value)> + '_ {
        self.defaults.iter().copied()
    }

    /// The names of every action its `do` nodes ask for, each once.
    pub fn actions(&self) -> Vec<Key> {
        let mut out: Vec<Key> = Vec::new();
        for node in &self.nodes {
            if let Kind::Do { action, .. } = node.kind
                && !out.contains(&action)
            {
                out.push(action);
            }
        }
        out
    }

    /// Whether `other` was compiled from the same JSON.
    pub(crate) fn same_source(&self, other: &BehaviorTree) -> bool {
        self.source == other.source
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn children(&self, i: usize) -> &[u16] {
        let (start, len) = self.nodes[i].kids;
        &self.kids[start as usize..start as usize + len as usize]
    }

    pub(crate) fn params(&self, i: usize) -> &[(Key, Value)] {
        match self.nodes[i].kind {
            Kind::Do {
                params: (start, len), ..
            } => &self.params[start as usize..start as usize + len as usize],
            _ => &[],
        }
    }

    /// The action key of `do` node `i`.
    pub(crate) fn action_of(&self, i: usize) -> Option<Key> {
        match self.nodes[i].kind {
            Kind::Do { action, .. } => Some(action),
            _ => None,
        }
    }

    // ---- compiling ----------------------------------------------------------

    /// Compiles `json` as a node at `path`, returning its index.
    /// `scored`: it is a child of a `utility` node, so it has a `score`.
    fn node(&mut self, json: &Json, path: &str, depth: u16, scored: bool, link: &mut Linker<'_>) -> Result<u16, Error> {
        if self.nodes.len() >= MAX_NODES {
            return Err(Error::new(
                ErrorKind::Limit,
                format!("behaviour tree: more than {MAX_NODES} nodes"),
            ));
        }
        let obj = json
            .as_object()
            .ok_or_else(|| bad(path, "a node must be an object such as {\"do\": \"patrol\"}"))?;
        // A `do` node's other fields are parameters: the kinds added later
        // may be parameter names there, as they could before.
        let kinds_here: &[&str] = if obj.contains_key("do") { &OLD_KINDS } else { &KINDS };
        let mut kinds = kinds_here.iter().filter(|k| obj.contains_key(**k));
        let kind = *kinds.next().ok_or_else(|| {
            bad(
                path,
                format!(
                    "no node kind (one of {}; found {})",
                    KINDS.join(", "),
                    obj.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
                ),
            )
        })?;
        if let Some(other) = kinds.next() {
            return Err(bad(path, format!("has two node kinds, \"{kind}\" and \"{other}\"")));
        }
        let path = format!("{path}/{kind}");
        let index = self.nodes.len();
        self.nodes.push(Node {
            kind: Kind::Succeed,
            pure: false,
            label: String::new(),
            depth,
            end: 0,
            acts: false,
            kids: (0, 0),
            score: (0, 0),
        });

        // Fields other than the kind: `name`, `score`, and the kind's own.
        let own: &[&str] = match kind {
            "utility" => &["stick"],
            "parallel" => &["success", "failure"],
            "check" => &["is", "below", "above"],
            "set" => &["to"],
            "cooldown" => &["child"],
            "wait_for" => &["timeout", "store"],
            _ => &[],
        };
        for key in obj.keys() {
            let known = key == kind || key == "name" || key == "score" || own.contains(&key.as_str());
            if !known && kind != "do" {
                return Err(bad(&path, format!("unknown field \"{key}\"")));
            }
        }
        let name = match obj.get("name") {
            None => None,
            Some(Json::String(s)) => Some(s.clone()),
            Some(_) => return Err(bad(&path, "\"name\" must be a string")),
        };
        match (scored, obj.get("score")) {
            (true, Some(score)) => {
                let start = self.terms.len() as u32;
                self.score(score, &format!("{path}/score"))?;
                self.nodes[index].score = (start, (self.terms.len() as u32 - start) as u16);
            }
            (true, None) => return Err(bad(&path, "a child of \"utility\" needs a \"score\"")),
            (false, Some(_)) => return Err(bad(&path, "\"score\" only counts on a child of \"utility\"")),
            (false, None) => {}
        }

        let arg = &obj[kind];
        // The tree a `subtree` node runs, held while its root compiles.
        let mut held: Option<Arc<BehaviorTree>> = None;
        let mut entered: Option<&'static str> = None;
        let (node_kind, label, kids): (Kind, String, Vec<(&Json, String, bool)>) = match kind {
            "sequence" | "selector" | "utility" | "parallel" | "random" => {
                let list = arg
                    .as_array()
                    .ok_or_else(|| bad(&path, format!("\"{kind}\" takes a list of nodes")))?;
                if list.is_empty() {
                    return Err(bad(&path, format!("\"{kind}\" needs at least one node")));
                }
                let utility = kind == "utility";
                let most = match kind {
                    "utility" => MAX_UTILITY_CHILDREN,
                    "parallel" | "random" => MAX_SET_CHILDREN,
                    _ => usize::MAX,
                };
                if list.len() > most {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        format!("behaviour tree: {path}: more than {most} children"),
                    ));
                }
                let n = list.len();
                let (node_kind, label) = match kind {
                    "sequence" => (Kind::Sequence, kind.to_owned()),
                    "selector" => (Kind::Selector, kind.to_owned()),
                    "random" => (Kind::Random, kind.to_owned()),
                    "parallel" => {
                        let success = policy(obj.get("success"), n, n, &path, "success")?;
                        let failure = policy(obj.get("failure"), 1, n, &path, "failure")?;
                        let shown = |k: u8| match k as usize {
                            1 => "any".to_owned(),
                            k if k == n => "all".to_owned(),
                            k => k.to_string(),
                        };
                        (
                            Kind::Parallel { success, failure },
                            format!("parallel (success {}, failure {})", shown(success), shown(failure)),
                        )
                    }
                    _ => {
                        let stick = number(obj.get("stick"), &path, "stick")?.unwrap_or(0.0);
                        let label = if stick > 0.0 {
                            format!("utility (stick {stick})")
                        } else {
                            "utility".to_owned()
                        };
                        (Kind::Utility { stick }, label)
                    }
                };
                let kids = list
                    .iter()
                    .enumerate()
                    .map(|(i, k)| (k, format!("{path}[{i}]"), utility))
                    .collect();
                (node_kind, label, kids)
            }
            "invert" | "succeed" => {
                let kind_of = if kind == "invert" { Kind::Invert } else { Kind::Succeed };
                (kind_of, kind.to_owned(), vec![(arg, path.clone(), false)])
            }
            "cooldown" => {
                let seconds = seconds(arg, &path)?;
                let child = obj
                    .get("child")
                    .ok_or_else(|| bad(&path, "\"cooldown\" needs a \"child\" node"))?;
                (
                    Kind::Cooldown { seconds },
                    format!("cooldown {seconds}s"),
                    vec![(child, format!("{path}/child"), false)],
                )
            }
            "wait" => {
                let seconds = seconds(arg, &path)?;
                (Kind::Wait { seconds }, format!("wait {seconds}s"), Vec::new())
            }
            "wait_for" => {
                let signal = key_of(arg, &path)?;
                let timeout = match obj.get("timeout") {
                    None => -1.0,
                    Some(t) => seconds(t, &format!("{path}/timeout"))?,
                };
                let store = match obj.get("store") {
                    None => None,
                    Some(k) => Some(key_of(k, &format!("{path}/store"))?),
                };
                if !self.listens.contains(&signal) {
                    self.listens.push(signal);
                }
                let mut label = format!("wait for {signal}");
                if timeout >= 0.0 {
                    label.push_str(&format!(" (timeout {timeout}s)"));
                }
                if let Some(store) = store {
                    label.push_str(&format!(" into {store}"));
                }
                (Kind::WaitFor { signal, timeout, store }, label, Vec::new())
            }
            "chance" => {
                let p = match arg.as_f64() {
                    Some(p) if (0.0..=1.0).contains(&p) => p as f32,
                    _ => return Err(bad(&path, "takes a probability (a number from 0 to 1)")),
                };
                (Kind::Chance { p }, format!("chance {p}"), Vec::new())
            }
            "subtree" => {
                let sub = key_of(arg, &path)?.name();
                if !self.subtrees.contains(&sub) {
                    self.subtrees.push(sub);
                }
                match link.resolve.and_then(|resolve| resolve(sub)) {
                    Some(found) => {
                        if let Some(at) = link.stack.iter().position(|n| *n == sub) {
                            let mut chain: Vec<&str> = link.stack[at..].to_vec();
                            chain.push(sub);
                            return Err(Error::new(
                                ErrorKind::Invalid,
                                format!(
                                    "behaviour tree {}: subtree cycle: {} (a tree can't contain itself)",
                                    link.stack[0],
                                    chain.join(" -> ")
                                ),
                            ));
                        }
                        self.defaults.extend(found.defaults.iter().copied());
                        entered = Some(sub);
                        let found = held.insert(found);
                        (
                            Kind::Subtree,
                            format!("subtree {sub}"),
                            vec![(&found.source["root"], format!("{path}({sub})/root"), false)],
                        )
                    }
                    None => (Kind::Subtree, format!("subtree {sub} (not loaded)"), Vec::new()),
                }
            }
            "check" => {
                let key = key_of(arg, &path)?;
                let tests: Vec<&str> = ["is", "below", "above"]
                    .into_iter()
                    .filter(|t| obj.contains_key(*t))
                    .collect();
                if tests.len() > 1 {
                    return Err(bad(&path, "\"check\" takes one of \"is\", \"below\", \"above\""));
                }
                let (test, label) = match tests.first() {
                    None => (Test::Truthy, format!("check {key}")),
                    Some(&t) => {
                        let operand = operand(&obj[t], &format!("{path}/{t}"))?;
                        let shown = match operand {
                            Operand::Lit(v) => v.to_string(),
                            Operand::Key(k) => k.name().to_owned(),
                        };
                        let test = match t {
                            "is" => Test::Is(operand),
                            "below" => Test::Below(operand),
                            _ => Test::Above(operand),
                        };
                        (test, format!("check {key} {t} {shown}"))
                    }
                };
                (Kind::Check { key, test }, label, Vec::new())
            }
            "set" => {
                let key = key_of(arg, &path)?;
                let to = obj
                    .get("to")
                    .ok_or_else(|| bad(&path, "\"set\" needs a \"to\" value"))?;
                let value = value_of(to, &format!("{path}/to"))?;
                (Kind::Set { key, value }, format!("set {key} = {value}"), Vec::new())
            }
            _ => {
                // "do": every other field is a parameter.
                let action = key_of(arg, &path)?;
                let start = self.params.len() as u32;
                let mut shown = Vec::new();
                for (k, v) in obj {
                    if k == "do" || k == "name" || k == "score" {
                        continue;
                    }
                    let value = value_of(v, &format!("{path}/{k}"))?;
                    shown.push(format!("{k}: {value}"));
                    self.params.push((Key::named(k), value));
                }
                let params = (start, (self.params.len() as u32 - start) as u16);
                let label = if shown.is_empty() {
                    format!("do {action}")
                } else {
                    format!("do {action} ({})", shown.join(", "))
                };
                (Kind::Do { action, params }, label, Vec::new())
            }
        };

        if let Some(sub) = entered {
            link.stack.push(sub);
        }
        let mut indices = Vec::with_capacity(kids.len());
        for (json, path, scored) in kids {
            indices.push(self.node(json, &path, depth + 1, scored, link)?);
        }
        if entered.is_some() {
            link.stack.pop();
        }
        drop(held);
        let acting: Vec<usize> = (0..indices.len())
            .filter(|&n| self.nodes[indices[n] as usize].acts)
            .collect();
        if let (Kind::Parallel { .. }, [first, second, ..]) = (node_kind, acting.as_slice()) {
            return Err(bad(
                &path,
                format!(
                    "children [{first}] and [{second}] both have \"do\" nodes; a brain does one action at a time, \
                     so only one child of a \"parallel\" may (the others check, wait or wait for signals)"
                ),
            ));
        }
        let start = self.kids.len() as u32;
        self.kids.extend_from_slice(&indices);
        let pure = match node_kind {
            Kind::Check { .. } => true,
            Kind::Sequence | Kind::Selector | Kind::Invert | Kind::Succeed | Kind::Subtree => {
                !indices.is_empty() && indices.iter().all(|&k| self.nodes[k as usize].pure)
            }
            _ => false,
        };
        let end = self.nodes.len() as u16;
        let node = &mut self.nodes[index];
        node.kind = node_kind;
        node.pure = pure;
        node.acts = matches!(node_kind, Kind::Do { .. }) || !acting.is_empty();
        node.end = end;
        node.kids = (start, indices.len() as u16);
        node.label = match name {
            Some(name) => format!("{name} ({label})"),
            None => label,
        };
        Ok(index as u16)
    }

    /// Compiles a `score`: a number, or a list of numbers and considerations.
    fn score(&mut self, json: &Json, path: &str) -> Result<(), Error> {
        match json {
            Json::Number(n) => {
                self.terms.push(Term::Const(n.as_f64().unwrap_or(0.0) as f32));
                Ok(())
            }
            Json::Array(list) => {
                for (i, term) in list.iter().enumerate() {
                    let path = format!("{path}[{i}]");
                    match term {
                        Json::Number(n) => self.terms.push(Term::Const(n.as_f64().unwrap_or(0.0) as f32)),
                        Json::Object(obj) => self.terms.push(Term::Consider(consideration(obj, &path)?)),
                        _ => {
                            return Err(bad(
                                &path,
                                "a score term is a number or {\"key\": ..., \"from\": ..., \"to\": ...}",
                            ));
                        }
                    }
                }
                Ok(())
            }
            _ => Err(bad(path, "a score is a number or a list of terms")),
        }
    }

    // ---- thinking -----------------------------------------------------------

    /// The score of child `i` of a `utility` node.
    fn score_of(&self, i: usize, bb: &Blackboard) -> f32 {
        let (start, len) = self.nodes[i].score;
        let terms = &self.terms[start as usize..start as usize + len as usize];
        combine_n(
            terms.len(),
            terms.iter().map(|t| match t {
                Term::Const(c) => *c,
                Term::Consider(c) => c.score(bb),
            }),
        )
    }

    /// Ticks node `i`: what it does this think.
    pub(crate) fn tick(&self, i: usize, t: &mut Tick<'_>) -> Status {
        let status = match self.nodes[i].kind {
            Kind::Sequence => {
                // A running sequence resumes at the child that was running
                // (kept in `at`): finished steps don't run again, but its
                // checks are asked again, so it still stops when one fails.
                let resume = if t.continuing(i) { t.states[i].at as usize } else { 0 };
                let mut status = Status::Success;
                for (n, &k) in self.children(i).iter().enumerate() {
                    if n < resume && !self.nodes[k as usize].pure {
                        continue;
                    }
                    status = self.tick(k as usize, t);
                    if status != Status::Success {
                        if status == Status::Running {
                            t.states[i].at = n as u64;
                        }
                        break;
                    }
                }
                status
            }
            Kind::Selector => {
                let mut status = Status::Failure;
                for &k in self.children(i) {
                    status = self.tick(k as usize, t);
                    if status != Status::Failure {
                        break;
                    }
                }
                status
            }
            Kind::Utility { stick } => {
                let kids = self.children(i);
                for &k in kids {
                    let k = k as usize;
                    let mut score = self.score_of(k, t.bb);
                    if stick > 0.0 && score > 0.0 && t.continuing(k) {
                        score += stick;
                    }
                    let state = &mut t.states[k];
                    state.score = score;
                    state.scored = t.thinks;
                }
                // Best first; ties go to the earlier child. No allocation:
                // at most 64 children, tried ones marked in a bit set.
                let mut tried = 0u64;
                let mut status = Status::Failure;
                loop {
                    let mut pick: Option<(usize, f32)> = None;
                    for (n, &k) in kids.iter().enumerate() {
                        let score = t.states[k as usize].score;
                        if tried & (1 << n) == 0 && score > 0.0 && pick.is_none_or(|(_, s)| score > s) {
                            pick = Some((n, score));
                        }
                    }
                    let Some((n, _)) = pick else { break };
                    tried |= 1 << n;
                    status = self.tick(kids[n] as usize, t);
                    if status != Status::Failure {
                        break;
                    }
                }
                status
            }
            Kind::Invert => match self.tick(self.children(i)[0] as usize, t) {
                Status::Success => Status::Failure,
                Status::Failure => Status::Success,
                Status::Running => Status::Running,
            },
            Kind::Succeed => match self.tick(self.children(i)[0] as usize, t) {
                Status::Running => Status::Running,
                _ => Status::Success,
            },
            Kind::Cooldown { seconds } => {
                if t.now < t.states[i].at {
                    Status::Failure
                } else {
                    let status = self.tick(self.children(i)[0] as usize, t);
                    if status == Status::Success {
                        t.states[i].at = t.now + t.ticks_for(seconds);
                    }
                    status
                }
            }
            Kind::Wait { seconds } => {
                if !t.continuing(i) {
                    t.states[i].at = t.now;
                }
                if t.now.saturating_sub(t.states[i].at) >= t.ticks_for(seconds) {
                    Status::Success
                } else {
                    Status::Running
                }
            }
            Kind::Subtree => match self.children(i).first() {
                Some(&root) => self.tick(root as usize, t),
                None => Status::Failure,
            },
            Kind::Parallel { success, failure } => self.tick_parallel(i, success, failure, t),
            Kind::Random => {
                let kids = self.children(i);
                let mut tried = 0u64;
                let mut status = Status::Failure;
                // The child that was running carries on first; the others
                // follow in a new random order.
                let mut next = if t.continuing(i) {
                    Some(t.states[i].at as usize)
                } else {
                    None
                };
                loop {
                    let n = match next.take() {
                        Some(n) if n < kids.len() => n,
                        _ => {
                            let left = kids.len() as u32 - tried.count_ones();
                            if left == 0 {
                                break;
                            }
                            let mut pick = t.rng().below(left);
                            let mut n = 0;
                            loop {
                                if tried & (1 << n) == 0 {
                                    if pick == 0 {
                                        break;
                                    }
                                    pick -= 1;
                                }
                                n += 1;
                            }
                            n
                        }
                    };
                    tried |= 1 << n;
                    status = self.tick(kids[n] as usize, t);
                    if status != Status::Failure {
                        if status == Status::Running {
                            t.states[i].at = n as u64;
                        }
                        break;
                    }
                }
                status
            }
            Kind::Chance { p } => {
                if t.rng().chance(p) {
                    Status::Success
                } else {
                    Status::Failure
                }
            }
            Kind::WaitFor { signal, timeout, store } => {
                if !t.continuing(i) {
                    t.states[i].at = t.now;
                }
                if let Some(&(_, value)) = t.signals.iter().find(|(k, _)| *k == signal) {
                    if let (Some(key), Some(value)) = (store, value) {
                        t.bb.set(key, value);
                    }
                    Status::Success
                } else if timeout >= 0.0 && t.now.saturating_sub(t.states[i].at) >= t.ticks_for(timeout) {
                    Status::Failure
                } else {
                    Status::Running
                }
            }
            Kind::Check { key, test } => {
                let pass = t.bb.get(key).is_some_and(|v| match test {
                    Test::Truthy => v.truthy(),
                    Test::Is(op) => resolve(op, t.bb) == Some(v),
                    Test::Below(op) => compare(v, resolve(op, t.bb)).is_some_and(|(a, b)| a < b),
                    Test::Above(op) => compare(v, resolve(op, t.bb)).is_some_and(|(a, b)| a > b),
                });
                if pass { Status::Success } else { Status::Failure }
            }
            Kind::Set { key, value } => {
                t.bb.set(key, value);
                Status::Success
            }
            Kind::Do { .. } => {
                if t.prev_action == Some(i as u16) && t.continuing(i) {
                    if t.report == Status::Running {
                        t.action = Some(i as u16);
                    }
                    t.report
                } else {
                    t.action = Some(i as u16);
                    t.started = true;
                    Status::Running
                }
            }
        };
        let state = &mut t.states[i];
        state.seen = t.thinks;
        state.status = status;
        status
    }
}

impl BehaviorTree {
    /// A `parallel` node: ticks its unfinished children in order until its
    /// policy decides, then stops the ones still running.
    fn tick_parallel(&self, i: usize, success: u8, failure: u8, t: &mut Tick<'_>) -> Status {
        let kids = self.children(i);
        let mut done = if t.continuing(i) { t.states[i].at } else { 0 };
        let (action, started) = (t.action, t.started);
        let (mut ok, mut bad, mut finished) = (0u8, 0u8, 0usize);
        let mut status = Status::Running;
        for (n, &k) in kids.iter().enumerate() {
            let k = k as usize;
            // A finished child keeps its result, but checks are asked
            // again, so a `parallel` can watch a condition.
            let result = if done & (1 << n) != 0 && !self.nodes[k].pure {
                t.states[k].status
            } else {
                let result = self.tick(k, t);
                if result != Status::Running {
                    done |= 1 << n;
                }
                result
            };
            match result {
                Status::Success => ok += 1,
                Status::Failure => bad += 1,
                Status::Running => {}
            }
            if result != Status::Running {
                finished += 1;
            }
            if ok >= success {
                status = Status::Success;
                break;
            }
            if bad >= failure {
                status = Status::Failure;
                break;
            }
        }
        if status == Status::Running && finished == kids.len() {
            // Everyone finished and neither count was reached.
            status = Status::Failure;
        }
        t.states[i].at = done;
        if status != Status::Running {
            // Stop what still runs below it: an action it started is
            // dropped, and a running node starts over when next reached.
            for (n, &k) in kids.iter().enumerate() {
                if done & (1 << n) == 0 {
                    self.halt(k as usize, t);
                }
            }
            if t.action != action {
                t.action = action;
                t.started = started;
            }
        }
        status
    }

    /// Marks every node below and at `k` that is running in this think as
    /// stopped (failed), so it doesn't carry on at the next one.
    fn halt(&self, k: usize, t: &mut Tick<'_>) {
        for state in &mut t.states[k..self.nodes[k].end as usize] {
            if state.seen == t.thinks && state.status == Status::Running {
                state.status = Status::Failure;
            }
        }
    }
}

fn resolve(op: Operand, bb: &Blackboard) -> Option<Value> {
    match op {
        Operand::Lit(v) => Some(v),
        Operand::Key(k) => bb.get(k),
    }
}

fn compare(a: Value, b: Option<Value>) -> Option<(f32, f32)> {
    Some((a.as_num()?, b?.as_num()?))
}

fn bad(path: &str, message: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Json, format!("behaviour tree: {path}: {message}"))
}

fn key_of(json: &Json, path: &str) -> Result<Key, Error> {
    match json {
        Json::String(s) if !s.is_empty() => Ok(Key::named(s)),
        _ => Err(bad(path, "expects a name (a non-empty string)")),
    }
}

fn value_of(json: &Json, path: &str) -> Result<Value, Error> {
    serde_json::from_value::<Value>(json.clone()).map_err(|e| bad(path, e))
}

/// A literal, or a string: the name of another blackboard entry.
fn operand(json: &Json, path: &str) -> Result<Operand, Error> {
    match json {
        Json::String(s) if !s.is_empty() => Ok(Operand::Key(Key::named(s))),
        _ => value_of(json, path).map(Operand::Lit),
    }
}

fn number(json: Option<&Json>, path: &str, field: &str) -> Result<Option<f32>, Error> {
    match json {
        None => Ok(None),
        Some(Json::Number(n)) => Ok(Some(n.as_f64().unwrap_or(0.0) as f32)),
        Some(_) => Err(bad(path, format!("\"{field}\" must be a number"))),
    }
}

/// A `parallel` policy: `"all"`, `"any"` or a count from 1 to `n`.
fn policy(json: Option<&Json>, default: usize, n: usize, path: &str, field: &str) -> Result<u8, Error> {
    let k = match json {
        None => default,
        Some(Json::String(s)) if s == "all" => n,
        Some(Json::String(s)) if s == "any" => 1,
        Some(Json::Number(k)) => match k.as_u64() {
            Some(k) if (1..=n as u64).contains(&k) => k as usize,
            _ => {
                return Err(bad(
                    path,
                    format!("\"{field}\" counts children: from 1 to {n}, or \"any\" or \"all\""),
                ));
            }
        },
        Some(_) => {
            return Err(bad(
                path,
                format!("\"{field}\" is \"all\", \"any\" or a number of children"),
            ));
        }
    };
    Ok(k as u8)
}

fn seconds(json: &Json, path: &str) -> Result<f32, Error> {
    match json.as_f64() {
        Some(s) if s >= 0.0 && s.is_finite() => Ok(s as f32),
        _ => Err(bad(path, "takes seconds (a number, 0 or more)")),
    }
}

fn consideration(obj: &Map<String, Json>, path: &str) -> Result<Consideration, Error> {
    for key in obj.keys() {
        if !["key", "from", "to", "curve", "invert"].contains(&key.as_str()) {
            return Err(bad(
                path,
                format!("unknown field \"{key}\" (allowed: key, from, to, curve, invert)"),
            ));
        }
    }
    let key = key_of(obj.get("key").unwrap_or(&Json::Null), &format!("{path}/key"))?;
    let from = number(obj.get("from"), path, "from")?.unwrap_or(0.0);
    let to = number(obj.get("to"), path, "to")?.unwrap_or(1.0);
    let curve = match obj.get("curve") {
        None => Curve::Linear,
        Some(c) => serde_json::from_value::<Curve>(c.clone()).map_err(|e| bad(&format!("{path}/curve"), e))?,
    };
    let invert = match obj.get("invert") {
        None => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(bad(path, "\"invert\" must be true or false")),
    };
    let mut c = Consideration::new(key, from, to).curve(curve);
    c.invert = invert;
    Ok(c)
}
