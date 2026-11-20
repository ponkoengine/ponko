//! Behaviour trees: the JSON format, its compiled form and one think.
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value as Json};

use super::utility::{Consideration, Curve, combine_n};
use super::{Blackboard, Key, Status, Value};
use crate::error::{Error, ErrorKind};

/// The most nodes a tree may have.
pub(crate) const MAX_NODES: usize = u16::MAX as usize;
/// The most children a `utility` node may have.
pub(crate) const MAX_UTILITY_CHILDREN: usize = 64;

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
    /// Children: a range of `kids`.
    kids: (u32, u16),
    /// Score terms under a `utility` node: a range of `terms`.
    score: (u32, u16),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Kind {
    Sequence,
    Selector,
    Utility { stick: f32 },
    Invert,
    Succeed,
    Check { key: Key, test: Test },
    Set { key: Key, value: Value },
    Do { action: Key, params: (u32, u16) },
    Wait { seconds: f32 },
    Cooldown { seconds: f32 },
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
    /// A wait's start tick; a cooldown's ready tick.
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
}

impl Tick<'_> {
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
        tree.node(root, "root", 0, false)?;
        Ok(tree)
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
    fn node(&mut self, json: &Json, path: &str, depth: u16, scored: bool) -> Result<u16, Error> {
        if self.nodes.len() >= MAX_NODES {
            return Err(Error::new(
                ErrorKind::Limit,
                format!("behaviour tree: more than {MAX_NODES} nodes"),
            ));
        }
        let obj = json
            .as_object()
            .ok_or_else(|| bad(path, "a node must be an object such as {\"do\": \"patrol\"}"))?;
        const KINDS: [&str; 10] = [
            "sequence", "selector", "utility", "invert", "succeed", "check", "set", "do", "wait", "cooldown",
        ];
        let mut kinds = KINDS.iter().filter(|k| obj.contains_key(**k));
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
            kids: (0, 0),
            score: (0, 0),
        });

        // Fields other than the kind: `name`, `score`, and the kind's own.
        let own: &[&str] = match kind {
            "utility" => &["stick"],
            "check" => &["is", "below", "above"],
            "set" => &["to"],
            "cooldown" => &["child"],
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
        let (node_kind, label, kids): (Kind, String, Vec<(&Json, String, bool)>) = match kind {
            "sequence" | "selector" | "utility" => {
                let list = arg
                    .as_array()
                    .ok_or_else(|| bad(&path, format!("\"{kind}\" takes a list of nodes")))?;
                if list.is_empty() {
                    return Err(bad(&path, format!("\"{kind}\" needs at least one node")));
                }
                let utility = kind == "utility";
                if utility && list.len() > MAX_UTILITY_CHILDREN {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        format!("behaviour tree: {path}: more than {MAX_UTILITY_CHILDREN} children"),
                    ));
                }
                let kind = match kind {
                    "sequence" => Kind::Sequence,
                    "selector" => Kind::Selector,
                    _ => Kind::Utility {
                        stick: number(obj.get("stick"), &path, "stick")?.unwrap_or(0.0),
                    },
                };
                let kids = list
                    .iter()
                    .enumerate()
                    .map(|(i, k)| (k, format!("{path}[{i}]"), utility))
                    .collect();
                let label = match kind {
                    Kind::Utility { stick } if stick > 0.0 => format!("utility (stick {stick})"),
                    _ => path.rsplit('/').next().unwrap_or_default().to_owned(),
                };
                (kind, label, kids)
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

        let mut indices = Vec::with_capacity(kids.len());
        for (json, path, scored) in kids {
            indices.push(self.node(json, &path, depth + 1, scored)?);
        }
        let start = self.kids.len() as u32;
        self.kids.extend_from_slice(&indices);
        let pure = match node_kind {
            Kind::Check { .. } => true,
            Kind::Sequence | Kind::Selector | Kind::Invert | Kind::Succeed => {
                indices.iter().all(|&k| self.nodes[k as usize].pure)
            }
            _ => false,
        };
        let node = &mut self.nodes[index];
        node.kind = node_kind;
        node.pure = pure;
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
