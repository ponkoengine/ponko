//! Keys, values and the per-agent [`Blackboard`].
use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use glam::{Vec2, Vec3};
use ponko_ecs::{Component, Entity};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The name of a blackboard entry or an action, hashed once.
///
/// Make the keys a game uses constants, so that a lookup in a loop over
/// thousands of agents compares two numbers:
///
/// ```
/// use ponko::ai::Key;
/// const HEALTH: Key = Key::new("health");
/// assert_eq!(HEALTH, Key::named("health"));
/// assert_eq!(HEALTH.name(), "health");
/// ```
///
/// Two keys are equal when their names are. (The 64-bit FNV-1a hashes of
/// two different names could in theory collide; for the dozens of names
/// a game uses the chance is below one in 10^15.)
#[derive(Clone, Copy)]
pub struct Key {
    hash: u64,
    name: &'static str,
}

impl Key {
    /// The key for `name`, at compile time.
    pub const fn new(name: &'static str) -> Key {
        Key {
            hash: fnv1a(name.as_bytes()),
            name,
        }
    }

    /// The key for a name known only at run time (read from a file). Each
    /// distinct name is stored once for the life of the program, so call it
    /// when loading, not per agent per tick.
    pub fn named(name: &str) -> Key {
        Key {
            hash: fnv1a(name.as_bytes()),
            name: intern(name),
        }
    }

    /// The name the key was made from.
    pub fn name(self) -> &'static str {
        self.name
    }

    /// The key's number: the 64-bit FNV-1a hash of its name. Two keys are
    /// equal when their ids are. The C API passes keys by id.
    pub const fn id(self) -> u64 {
        self.hash
    }

    /// The key whose [`id`](Self::id) this is, if a key of that name was
    /// made with [`named`](Self::named) (as for every name in a loaded tree
    /// or blackboard).
    ///
    /// ```
    /// use ponko::ai::Key;
    /// let home = Key::named("home");
    /// assert_eq!(Key::from_id(home.id()), Some(home));
    /// assert_eq!(Key::from_id(12345), None);
    /// ```
    pub fn from_id(id: u64) -> Option<Key> {
        names().by_id.get(&id).map(|&name| Key { hash: id, name })
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        self.hash == other.hash
    }
}
impl Eq for Key {}
impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}
impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({:?})", self.name)
    }
}
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

/// Every run-time name, by text and by id.
#[derive(Default)]
struct Names {
    by_name: HashMap<String, &'static str>,
    by_id: HashMap<u64, &'static str>,
}

fn names() -> std::sync::MutexGuard<'static, Names> {
    static NAMES: OnceLock<Mutex<Names>> = OnceLock::new();
    NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Each distinct run-time name, stored once.
fn intern(name: &str) -> &'static str {
    let mut names = names();
    if let Some(&stored) = names.by_name.get(name) {
        return stored;
    }
    let stored: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.by_name.insert(name.to_owned(), stored);
    names.by_id.insert(fnv1a(name.as_bytes()), stored);
    stored
}

/// A blackboard value: a flag, a number, a 2D or 3D point, or an entity.
///
/// In JSON (tree files, saves, the inspector) a flag is `true`, a number
/// `0.5`, a point `[1, 2]` or `[1, 2, 3]`, an entity `{"entity": 42}`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// `true` or `false`.
    Bool(bool),
    /// A number (health, a distance, a count).
    Num(f32),
    /// A 2D point or direction.
    Vec2(Vec2),
    /// A 3D point or direction.
    Vec3(Vec3),
    /// Another entity (a target, a home).
    Entity(Entity),
}

impl Value {
    /// The number, if this is one.
    pub fn as_num(self) -> Option<f32> {
        match self {
            Value::Num(n) => Some(n),
            _ => None,
        }
    }
    /// The flag, if this is one.
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
    /// The 2D point, if this is one.
    pub fn as_vec2(self) -> Option<Vec2> {
        match self {
            Value::Vec2(v) => Some(v),
            _ => None,
        }
    }
    /// The 3D point, if this is one.
    pub fn as_vec3(self) -> Option<Vec3> {
        match self {
            Value::Vec3(v) => Some(v),
            _ => None,
        }
    }
    /// The entity, if this is one.
    pub fn as_entity(self) -> Option<Entity> {
        match self {
            Value::Entity(e) => Some(e),
            _ => None,
        }
    }
    /// What a `check` without a comparison tests: a flag that is `true`, a
    /// number that isn't 0, and any point or entity.
    pub fn truthy(self) -> bool {
        match self {
            Value::Bool(b) => b,
            Value::Num(n) => n != 0.0,
            Value::Vec2(_) | Value::Vec3(_) | Value::Entity(_) => true,
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}
impl From<f32> for Value {
    fn from(n: f32) -> Value {
        Value::Num(n)
    }
}
impl From<Vec2> for Value {
    fn from(v: Vec2) -> Value {
        Value::Vec2(v)
    }
}
impl From<Vec3> for Value {
    fn from(v: Vec3) -> Value {
        Value::Vec3(v)
    }
}
impl From<Entity> for Value {
    fn from(e: Entity) -> Value {
        Value::Entity(e)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Num(n) => write!(f, "{n}"),
            Value::Vec2(v) => write!(f, "[{}, {}]", v.x, v.y),
            Value::Vec3(v) => write!(f, "[{}, {}, {}]", v.x, v.y, v.z),
            Value::Entity(e) => write!(f, "{e:?}"),
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match *self {
            Value::Bool(b) => s.serialize_bool(b),
            Value::Num(n) => s.serialize_f32(n),
            Value::Vec2(v) => {
                let mut seq = s.serialize_seq(Some(2))?;
                seq.serialize_element(&v.x)?;
                seq.serialize_element(&v.y)?;
                seq.end()
            }
            Value::Vec3(v) => {
                let mut seq = s.serialize_seq(Some(3))?;
                seq.serialize_element(&v.x)?;
                seq.serialize_element(&v.y)?;
                seq.serialize_element(&v.z)?;
                seq.end()
            }
            Value::Entity(e) => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("entity", &e)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Value;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a flag, a number, [x, y], [x, y, z] or {\"entity\": id}")
            }
            fn visit_bool<E: de::Error>(self, b: bool) -> Result<Value, E> {
                Ok(Value::Bool(b))
            }
            fn visit_f64<E: de::Error>(self, n: f64) -> Result<Value, E> {
                Ok(Value::Num(n as f32))
            }
            fn visit_f32<E: de::Error>(self, n: f32) -> Result<Value, E> {
                Ok(Value::Num(n))
            }
            fn visit_i64<E: de::Error>(self, n: i64) -> Result<Value, E> {
                Ok(Value::Num(n as f32))
            }
            fn visit_u64<E: de::Error>(self, n: u64) -> Result<Value, E> {
                Ok(Value::Num(n as f32))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
                let mut xs = [0.0f32; 3];
                let mut len = 0;
                while let Some(x) = seq.next_element::<f32>()? {
                    if len == 3 {
                        return Err(de::Error::invalid_length(4, &"a point of 2 or 3 numbers"));
                    }
                    xs[len] = x;
                    len += 1;
                }
                match len {
                    2 => Ok(Value::Vec2(Vec2::new(xs[0], xs[1]))),
                    3 => Ok(Value::Vec3(Vec3::from(xs))),
                    n => Err(de::Error::invalid_length(n, &"a point of 2 or 3 numbers")),
                }
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
                let mut entity = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key != "entity" {
                        return Err(de::Error::unknown_field(&key, &["entity"]));
                    }
                    entity = Some(map.next_value::<Entity>()?);
                }
                entity
                    .map(Value::Entity)
                    .ok_or_else(|| de::Error::missing_field("entity"))
            }
        }
        d.deserialize_any(V)
    }
}

/// What one agent knows: named [`Value`]s that the game's systems write
/// ("senses": the distance to the player, the health left) and that its
/// [behaviour tree](super::BehaviorTree) reads (`check`, scores) and writes
/// (`set`).
///
/// A component, reflected when the game loads a tree, so the inspector,
/// world saves and the control endpoint read and edit it as JSON: an object
/// from names to values, `{"health": 0.5, "alert": true}`.
///
/// ```
/// use ponko::ai::{Blackboard, Key, Value};
/// const HEALTH: Key = Key::new("health");
/// let mut bb = Blackboard::new().with(HEALTH, 1.0);
/// bb.set(HEALTH, 0.25);
/// assert_eq!(bb.num(HEALTH), 0.25);
/// assert_eq!(bb.get(Key::new("missing")), None);
/// assert_eq!(serde_json::to_string(&bb).unwrap(), r#"{"health":0.25}"#);
/// ```
#[derive(Component, Clone, Default, Debug, PartialEq)]
pub struct Blackboard {
    entries: Vec<(Key, Value)>,
}

impl Blackboard {
    /// An empty blackboard.
    pub fn new() -> Blackboard {
        Blackboard::default()
    }

    /// This blackboard with `key` set to `value` (for spawning).
    pub fn with(mut self, key: Key, value: impl Into<Value>) -> Blackboard {
        self.set(key, value);
        self
    }

    /// The value of `key`, if it has one.
    pub fn get(&self, key: Key) -> Option<Value> {
        self.entries.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
    }

    /// Sets `key` (adding it the first time).
    pub fn set(&mut self, key: Key, value: impl Into<Value>) {
        let value = value.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// Sets `key` only if it has no value yet (the tree's defaults).
    pub(crate) fn set_default(&mut self, key: Key, value: Value) {
        if !self.contains(key) {
            self.entries.push((key, value));
        }
    }

    /// Removes `key`, returning its value.
    pub fn remove(&mut self, key: Key) -> Option<Value> {
        let i = self.entries.iter().position(|(k, _)| *k == key)?;
        Some(self.entries.remove(i).1)
    }

    /// Whether `key` has a value.
    pub fn contains(&self, key: Key) -> bool {
        self.entries.iter().any(|(k, _)| *k == key)
    }

    /// The number at `key`, or 0 if it has none or isn't a number.
    pub fn num(&self, key: Key) -> f32 {
        self.get(key).and_then(Value::as_num).unwrap_or(0.0)
    }

    /// The flag at `key`, or `false` if it has none or isn't a flag.
    pub fn flag(&self, key: Key) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    /// The 2D point at `key`, if it holds one.
    pub fn vec2(&self, key: Key) -> Option<Vec2> {
        self.get(key).and_then(Value::as_vec2)
    }

    /// The 3D point at `key`, if it holds one.
    pub fn vec3(&self, key: Key) -> Option<Vec3> {
        self.get(key).and_then(Value::as_vec3)
    }

    /// The entity at `key`, if it holds one.
    pub fn entity(&self, key: Key) -> Option<Entity> {
        self.get(key).and_then(Value::as_entity)
    }

    /// Every entry, in the order they were first set.
    pub fn iter(&self) -> impl Iterator<Item = (Key, Value)> + '_ {
        self.entries.iter().copied()
    }

    /// How many entries it has.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Serialize for Blackboard {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key.name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Blackboard {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Blackboard, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Blackboard;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object from names to values")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Blackboard, A::Error> {
                let mut bb = Blackboard::new();
                while let Some((name, value)) = map.next_entry::<String, Value>()? {
                    bb.set(Key::named(&name), value);
                }
                Ok(bb)
            }
        }
        d.deserialize_map(V)
    }
}
