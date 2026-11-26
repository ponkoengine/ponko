//! [`world_hash`]: one number for a world's reflected state, for
//! determinism checks (lockstep peers, replays, tests).
//!
//! Every value goes through a word-at-a-time hasher (no JSON, no
//! allocation for plain components): Rust components through their
//! `Serialize`, components described with `reflect_fields` through their
//! fields, sprites field by field. Each (entity, component) pair and each
//! resource is hashed on its own and the results are added up, so the hash
//! doesn't depend on the order entities sit in memory or on the order
//! components were reflected in.

use std::sync::Arc;

use ponko_ecs::{ComponentId, Entity, World};
use serde::Serialize;
use serde::ser::{self, SerializeMap};

use super::{FieldLayout, FieldType, Reflected, Registry, registry};

/// Hashes every value of one component (or a resource) with the seed of
/// its name; returns the sum of the per-entity hashes.
pub(super) type HashFn = Arc<dyn Fn(&World, u64) -> u64 + Send + Sync>;

const K: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64's finalizer: every input bit reaches every output bit.
#[inline(always)]
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(K);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A 64-bit hash fed a word at a time. Each step is a bijection of the state
/// for a given word, so changing one word always changes the result. The
/// same on every platform: words are built from little-endian bytes and
/// float bits.
#[derive(Clone, Copy)]
pub(super) struct Words(u64);

impl Words {
    #[inline(always)]
    pub(super) fn new(seed: u64) -> Self {
        Words(mix(seed))
    }

    #[inline(always)]
    pub(super) fn word(&mut self, w: u64) {
        let h = (self.0 ^ w).wrapping_mul(K);
        self.0 = h ^ (h >> 32);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.word(bytes.len() as u64);
        let (chunks, rest) = bytes.as_chunks::<8>();
        for &c in chunks {
            self.word(u64::from_le_bytes(c));
        }
        if !rest.is_empty() {
            let mut last = [0u8; 8];
            last[..rest.len()].copy_from_slice(rest);
            self.word(u64::from_le_bytes(last));
        }
    }

    #[inline(always)]
    pub(super) fn finish(self) -> u64 {
        mix(self.0)
    }

    /// Feeds `value` through its `Serialize`. A value that refuses to be
    /// serialized leaves a marker word.
    pub(super) fn value<T: Serialize + ?Sized>(&mut self, value: &T) {
        if value.serialize(&mut *self).is_err() {
            self.word(u64::MAX);
        }
    }
}

/// The seed of a component's or resource's name.
pub(super) fn name_seed(name: &str) -> u64 {
    let mut h = Words::new(0x0123_4567_89AB_CDEF);
    h.bytes(name.as_bytes());
    h.finish()
}

/// The start of one entity's value of one component.
#[inline(always)]
fn entity_words(seed: u64, e: Entity) -> Words {
    Words::new(seed ^ e.to_bits().wrapping_mul(K))
}

/// [`HashFn`] of a Rust component, through its `Serialize`.
pub(super) fn of<T: ponko_ecs::Component + Serialize>() -> HashFn {
    Arc::new(|world: &World, seed: u64| {
        let mut sum = 0u64;
        world.query_ref::<(Entity, &T)>().for_each(|(e, value)| {
            let mut h = entity_words(seed, e);
            h.value(value);
            sum = sum.wrapping_add(h.finish());
        });
        sum
    })
}

/// [`HashFn`] of a Rust resource, through its `Serialize` (0 when absent).
pub(super) fn of_resource<R: Send + Sync + 'static + Serialize>() -> HashFn {
    Arc::new(|world: &World, seed: u64| {
        world.get_resource::<R>().map_or(0, |value| {
            let mut h = Words::new(seed);
            h.value(value);
            h.finish()
        })
    })
}

/// [`HashFn`] of sprites: the texture by id, then the rest field by field.
pub(super) fn of_sprites() -> HashFn {
    Arc::new(|world: &World, seed: u64| {
        let mut sum = 0u64;
        world.query_ref::<(Entity, &crate::d2::Sprite)>().for_each(|(e, s)| {
            let mut h = entity_words(seed, e);
            h.word(u64::from(s.texture.id));
            h.value(&s.color);
            h.word(u64::from(s.z.to_bits()));
            h.value(&s.pivot);
            match s.region {
                None => h.word(0),
                Some(r) => {
                    h.word(1);
                    h.value(&r);
                }
            }
            // Only flipped sprites add a word, so unflipped worlds hash as before.
            if s.flip_x || s.flip_y {
                h.word(2 | u64::from(s.flip_x) << 2 | u64::from(s.flip_y) << 3);
            }
            sum = sum.wrapping_add(h.finish());
        });
        sum
    })
}

/// Sums the hashes of every value of component `id` described by `layout`.
fn of_layout(world: &World, id: ComponentId, layout: &FieldLayout, seed: u64) -> u64 {
    let mut sum = 0u64;
    for arch in world.raw_ref().archetypes() {
        let Some(base) = arch.column_ptr(id) else { continue };
        for (row, &e) in arch.entities().iter().enumerate() {
            let mut h = entity_words(seed, e);
            // SAFETY: `row` is a row of this archetype, so the value is a
            // live, `size`-byte value of the component.
            unsafe { layout.hash(base.add(row * layout.size), &mut h) };
            sum = sum.wrapping_add(h.finish());
        }
    }
    sum
}

/// Sums the hashes of every value of a reflected component without a
/// [`HashFn`]: through its JSON, entity by entity (slower).
fn of_json(world: &World, c: &Reflected, seed: u64) -> u64 {
    let Some(id) = world.raw_ref().component_by_name(&c.type_name) else {
        return 0;
    };
    let mut sum = 0u64;
    for arch in world.raw_ref().archetypes() {
        if !arch.has(id) {
            continue;
        }
        for &e in arch.entities() {
            let mut h = entity_words(seed, e);
            match (c.get)(world, e) {
                Some(Ok(json)) => h.value(&json),
                // Not saved (a sprite of a texture made in code) or refused.
                _ => h.word(u64::MAX - 1),
            }
            sum = sum.wrapping_add(h.finish());
        }
    }
    sum
}

/// `n` bytes at `p + offset` as a little-endian word.
///
/// # Safety
/// The bytes are readable.
unsafe fn word_at(p: *const u8, offset: usize, n: usize) -> u64 {
    let mut b = [0u8; 8];
    // SAFETY: forwarded.
    unsafe { std::ptr::copy_nonoverlapping(p.add(offset), b.as_mut_ptr(), n) };
    u64::from_le_bytes(b)
}

impl FieldLayout {
    /// Feeds the fields of the value at `p` to `h`.
    ///
    /// # Safety
    /// `p` points to a readable value of this layout's component.
    unsafe fn hash(&self, p: *const u8, h: &mut Words) {
        for f in &self.fields {
            // SAFETY (all reads): inside the value, checked when the layout
            // was made; any bits are fine to hash.
            match f.ty {
                FieldType::Bool => h.word(u64::from(unsafe { word_at(p, f.offset, 1) } != 0)),
                FieldType::Vec2 => {
                    h.word(unsafe { word_at(p, f.offset, 4) });
                    h.word(unsafe { word_at(p, f.offset + 4, 4) });
                }
                FieldType::Color => {
                    for channel in 0..4 {
                        h.word(unsafe { word_at(p, f.offset + channel * 4, 4) });
                    }
                }
                ty => h.word(unsafe { word_at(p, f.offset, ty.size()) }),
            }
        }
    }
}

/// A hash of `world`'s reflected state: every reflected component of every
/// entity (by entity id), and every reflected resource. Use it to check
/// that two runs, two lockstep peers or a run and its replay are in the
/// same state:
///
/// ```
/// use ponko::prelude::*;
/// use ponko::reflect::world_hash;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Component, Serialize, Deserialize)]
/// struct Health(f32);
///
/// let make = || {
///     let mut world = World::new();
///     world.reflect::<Health>();
///     world.spawn((Health(10.0),));
///     world
/// };
/// let (a, mut b) = (make(), make());
/// assert_eq!(world_hash(&a), world_hash(&b));
/// b.query::<&mut Health>().for_each(|h| h.0 -= 1.0);
/// assert_ne!(world_hash(&a), world_hash(&b));
/// ```
///
/// Bit for bit: `-0.0` and `0.0` differ, as do two NaNs with other bits.
/// The order entities sit in memory and the order things were reflected in
/// don't matter; entity ids do (the same spawns in the same order give the
/// same ids, see the determinism promises in `STABILITY.md`). Components
/// and resources that aren't reflected are left out: reflect what the game
/// state is made of. Entities marked
/// [`NotSaved`](super::NotSaved) are hashed too.
///
/// The same on every platform and thread count, and fast: it reads the
/// components in place, without making JSON (only components with a
/// custom reflection, such as models and animations, go through their
/// JSON). The value can change between engine versions; compare hashes
/// made by the same build.
#[must_use]
pub fn world_hash(world: &World) -> u64 {
    let mut total = 0u64;
    each_part(world, |_, sum| total = total.wrapping_add(sum));
    mix(total)
}

/// [`world_hash`], part by part: its [`total`](Self::total) and the hash
/// of each reflected component and resource, so a check that finds two
/// worlds differ can say what differs ([`differing`](Self::differing)).
/// Lockstep sessions compare these to name the components in a
/// [`Desync`](crate::net::Desync).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::reflect::{WorldHashParts, world_hash};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Component, Serialize, Deserialize)]
/// struct Health(f32);
/// #[derive(Component, Serialize, Deserialize)]
/// struct Armor(u32);
///
/// let make = || {
///     let mut world = World::new();
///     world.reflect::<Health>();
///     world.reflect::<Armor>();
///     world.spawn((Health(10.0), Armor(2)));
///     world
/// };
/// let (a, mut b) = (make(), make());
/// b.query::<&mut Health>().for_each(|h| h.0 -= 1.0);
/// let (pa, pb) = (WorldHashParts::of(&a), WorldHashParts::of(&b));
/// assert_eq!(pa.total, world_hash(&a));
/// assert_eq!(pa.differing(&pb), ["Health"]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldHashParts {
    /// The same number as [`world_hash`].
    pub total: u64,
    /// Each reflected component (by its reflected name, such as
    /// `Transform2D`) and resource (`resource:Name`) that has a value, with
    /// the hash of its values, sorted by name. Components no entity has and
    /// absent resources are left out.
    pub parts: Vec<(String, u64)>,
}

impl WorldHashParts {
    /// Hashes `world` part by part (as fast as [`world_hash`]; the names
    /// are the only allocations).
    #[must_use]
    pub fn of(world: &World) -> Self {
        let mut total = 0u64;
        let mut parts = Vec::new();
        each_part(world, |name, sum| {
            total = total.wrapping_add(sum);
            if sum != 0 {
                parts.push((name.to_string(), sum));
            }
        });
        parts.sort_unstable();
        WorldHashParts {
            total: mix(total),
            parts,
        }
    }

    /// The names of the parts whose hashes differ between `self` and
    /// `other`, or that only one of them has; empty when the totals match.
    #[must_use]
    pub fn differing(&self, other: &WorldHashParts) -> Vec<String> {
        if self.total == other.total {
            return Vec::new();
        }
        let find = |parts: &[(String, u64)], name: &str| parts.iter().find(|(n, _)| n == name).map(|&(_, h)| h);
        let mut names: Vec<String> = self
            .parts
            .iter()
            .filter(|(name, hash)| find(&other.parts, name) != Some(*hash))
            .map(|(name, _)| name.clone())
            .collect();
        names.extend(
            other
                .parts
                .iter()
                .filter(|(name, _)| find(&self.parts, name).is_none())
                .map(|(name, _)| name.clone()),
        );
        names
    }
}

/// A part of [`WorldHashParts`], named without allocating.
#[derive(Clone, Copy)]
enum PartName<'a> {
    Component(&'a str),
    Resource(&'a str),
}

impl std::fmt::Display for PartName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Component(name) => f.write_str(name),
            Self::Resource(name) => write!(f, "resource:{name}"),
        }
    }
}

/// Calls `f` with each part's name (components by name, resources as
/// `resource:Name`) and the sum of its values' hashes.
fn each_part(world: &World, mut f: impl FnMut(PartName<'_>, u64)) {
    let Some(reg) = registry(world) else { return };
    for c in &reg.list {
        let seed = name_seed(&c.name);
        f(PartName::Component(&c.name), component_sum(world, reg, c, seed));
    }
    for r in &reg.resources {
        let seed = name_seed(&r.name) ^ 0x5245_534F_5552_4345;
        let sum = match reg.resource_hashers.get(&r.name) {
            Some(hash) => hash(world, seed),
            None => match (r.get)(world) {
                Some(Ok(json)) => {
                    let mut h = Words::new(seed);
                    h.value(&json);
                    h.finish()
                }
                _ => 0,
            },
        };
        f(PartName::Resource(&r.name), sum);
    }
}

fn component_sum(world: &World, reg: &Registry, c: &Reflected, seed: u64) -> u64 {
    if let Some(hash) = reg.hashers.get(&c.name) {
        return hash(world, seed);
    }
    let described = reg.layouts.iter().find(|(_, l)| l.component == c.name);
    match described {
        Some((&id, layout)) => of_layout(world, id, layout, seed),
        None => of_json(world, c, seed),
    }
}

// ---- the serializer that feeds `Words` -----------------------------------------------

/// A value that couldn't be serialized.
#[derive(Debug)]
pub(super) struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the value can't be serialized")
    }
}
impl std::error::Error for Refused {}
impl ser::Error for Refused {
    fn custom<M: std::fmt::Display>(_: M) -> Self {
        Refused
    }
}

impl<'a> ser::Serializer for &'a mut Words {
    type Ok = ();
    type Error = Refused;
    type SerializeSeq = Seq<'a>;
    type SerializeTuple = &'a mut Words;
    type SerializeTupleStruct = &'a mut Words;
    type SerializeTupleVariant = &'a mut Words;
    type SerializeMap = Map<'a>;
    type SerializeStruct = &'a mut Words;
    type SerializeStructVariant = &'a mut Words;

    fn serialize_bool(self, v: bool) -> Result<(), Refused> {
        self.word(u64::from(v));
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> Result<(), Refused> {
        self.serialize_i64(v.into())
    }
    fn serialize_i16(self, v: i16) -> Result<(), Refused> {
        self.serialize_i64(v.into())
    }
    fn serialize_i32(self, v: i32) -> Result<(), Refused> {
        self.serialize_i64(v.into())
    }
    fn serialize_i64(self, v: i64) -> Result<(), Refused> {
        self.word(v as u64);
        Ok(())
    }
    fn serialize_i128(self, v: i128) -> Result<(), Refused> {
        self.word(v as u64);
        self.word((v >> 64) as u64);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> Result<(), Refused> {
        self.serialize_u64(v.into())
    }
    fn serialize_u16(self, v: u16) -> Result<(), Refused> {
        self.serialize_u64(v.into())
    }
    fn serialize_u32(self, v: u32) -> Result<(), Refused> {
        self.serialize_u64(v.into())
    }
    fn serialize_u64(self, v: u64) -> Result<(), Refused> {
        self.word(v);
        Ok(())
    }
    fn serialize_u128(self, v: u128) -> Result<(), Refused> {
        self.word(v as u64);
        self.word((v >> 64) as u64);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> Result<(), Refused> {
        self.word(u64::from(v.to_bits()));
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<(), Refused> {
        self.word(v.to_bits());
        Ok(())
    }
    fn serialize_char(self, v: char) -> Result<(), Refused> {
        self.word(u64::from(v));
        Ok(())
    }
    fn serialize_str(self, v: &str) -> Result<(), Refused> {
        self.bytes(v.as_bytes());
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<(), Refused> {
        self.bytes(v);
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Refused> {
        self.word(0);
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Refused> {
        self.word(1);
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Refused> {
        self.word(2);
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Refused> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(self, _: &'static str, index: u32, _: &'static str) -> Result<(), Refused> {
        self.word(u64::from(index));
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<(), Refused> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        index: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), Refused> {
        self.word(u64::from(index));
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Seq<'a>, Refused> {
        // The length goes in at the end: it isn't always known up front.
        Ok(Seq { words: self, len: 0 })
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Refused> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Refused> {
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, index: u32, _: &'static str, _: usize) -> Result<Self, Refused> {
        self.word(u64::from(index));
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Map<'a>, Refused> {
        Ok(Map {
            words: self,
            sum: 0,
            len: 0,
            entry: Words(0),
        })
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Refused> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, index: u32, _: &'static str, _: usize) -> Result<Self, Refused> {
        self.word(u64::from(index));
        Ok(self)
    }
}

/// A sequence: its elements, then its length.
pub(super) struct Seq<'a> {
    words: &'a mut Words,
    len: u64,
}

impl ser::SerializeSeq for Seq<'_> {
    type Ok = ();
    type Error = Refused;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Refused> {
        self.len += 1;
        value.serialize(&mut *self.words)
    }
    fn end(self) -> Result<(), Refused> {
        self.words.word(self.len);
        Ok(())
    }
}

/// A map: each entry hashed on its own and the results added up, so the
/// order a `HashMap` lists its entries in doesn't matter.
pub(super) struct Map<'a> {
    words: &'a mut Words,
    sum: u64,
    len: u64,
    entry: Words,
}

impl SerializeMap for Map<'_> {
    type Ok = ();
    type Error = Refused;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Refused> {
        self.entry = Words::new(0x4D41_5045_4E54_5259);
        key.serialize(&mut self.entry)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Refused> {
        value.serialize(&mut self.entry)?;
        self.sum = self.sum.wrapping_add(self.entry.finish());
        self.len += 1;
        Ok(())
    }
    fn end(self) -> Result<(), Refused> {
        self.words.word(self.sum);
        self.words.word(self.len);
        Ok(())
    }
}

macro_rules! in_order {
    ($($tr:ident :: $method:ident),*) => {$(
        impl ser::$tr for &mut Words {
            type Ok = ();
            type Error = Refused;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Refused> {
                value.serialize(&mut **self)
            }
            fn end(self) -> Result<(), Refused> {
                Ok(())
            }
        }
    )*};
}
in_order!(
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field
);

impl ser::SerializeStruct for &mut Words {
    type Ok = ();
    type Error = Refused;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, value: &T) -> Result<(), Refused> {
        value.serialize(&mut **self)
    }
    fn skip_field(&mut self, _: &'static str) -> Result<(), Refused> {
        // A skipped field and a present one must not hash alike.
        self.word(3);
        Ok(())
    }
    fn end(self) -> Result<(), Refused> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Words {
    type Ok = ();
    type Error = Refused;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, value: &T) -> Result<(), Refused> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), Refused> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    fn hash<T: Serialize>(v: &T) -> u64 {
        let mut h = Words::new(1);
        h.value(v);
        h.finish()
    }

    #[test]
    fn maps_hash_alike_whatever_order_they_list_their_entries_in() {
        let ordered: BTreeMap<String, u32> = (0..50).map(|i| (format!("k{i}"), i)).collect();
        let hashed: HashMap<String, u32> = ordered.clone().into_iter().collect();
        assert_eq!(hash(&ordered), hash(&hashed));
        let mut other = ordered.clone();
        other.insert("k3".into(), 4);
        assert_ne!(hash(&ordered), hash(&other));
    }

    #[test]
    fn nearby_values_hash_apart() {
        assert_ne!(hash(&(1.0f32, 2.0f32)), hash(&(2.0f32, 1.0f32)));
        assert_ne!(hash(&0.0f32), hash(&-0.0f32));
        assert_ne!(hash(&Some(0u32)), hash(&None::<u32>));
        assert_ne!(hash(&vec![0u8; 3]), hash(&vec![0u8; 4]));
        assert_ne!(hash(&"ab"), hash(&"ab\0"));
        assert_eq!(hash(&[1u32, 2, 3]), hash(&[1u32, 2, 3]));
    }
}
