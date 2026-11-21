//! The registry of replicated component kinds: what the server reads out of
//! its world and what a client writes into its own, for Rust components
//! ([`Replicate`] types), interpolated ones, and plain-byte components
//! declared from C and C++ (field offsets and types).

use std::any::TypeId;
use std::marker::PhantomData;

use ponko_ecs::{Component, ComponentId, Entity, World};

use super::fields::{FieldSpec, Fields, Quantize, Replicate, read_words, schema_of, write_words};
use crate::net::{Interpolate, SnapshotBuffer, SnapshotConfig};

/// How a kind's values move between a world and its words.
pub(crate) trait KindAccess: Send + Sync {
    /// The entity's value as words; false if it doesn't have the component.
    fn read(&self, world: &World, e: Entity, out: &mut [u32]) -> bool;
    /// Sets (or adds) the entity's value from words.
    fn write(&self, world: &mut World, e: Entity, words: &[u32]);
    /// Removes the component.
    fn remove(&self, world: &mut World, e: Entity);
    /// Server: starts tracking the component's changes (it stays tracked)
    /// and returns its id, so only changed entities are read each tick.
    fn track(&self, world: &mut World) -> ComponentId;
    /// Client, interpolated kinds: keeps a received value (sent at server
    /// time `time`) to be shown later. False for kinds that aren't
    /// interpolated: write them at once.
    fn buffer(&self, _world: &mut World, _e: Entity, _time: f64, _words: &[u32]) -> bool {
        false
    }
    /// Client, interpolated kinds: sets every buffered entity's value to
    /// what it was its buffer's delay before server time `remote_now`.
    fn sample(&self, _world: &mut World, _remote_now: f64) {}
    /// Client: forgets the entity's buffered values (it became ours, or the
    /// component went away).
    fn unbuffer(&self, _world: &mut World, _e: Entity) {}
}

/// A replicated component kind.
pub(crate) struct Kind {
    pub(crate) name: String,
    pub(crate) specs: Vec<FieldSpec>,
    /// Where its words start in an entity's words.
    pub(crate) offset: usize,
    pub(crate) type_id: Option<TypeId>,
    pub(crate) access: Box<dyn KindAccess>,
    /// Kinds declared from C (`raw_component`): their layout, which
    /// prediction and interpolation of the kind need too.
    pub(crate) raw: Option<std::sync::Arc<Raw>>,
}

impl Kind {
    pub(crate) fn words(&self) -> usize {
        self.specs.len()
    }
}

/// A Rust component.
pub(crate) struct Typed<T>(PhantomData<fn() -> T>);

impl<T> Typed<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: Replicate + Component> KindAccess for Typed<T> {
    fn read(&self, world: &World, e: Entity, out: &mut [u32]) -> bool {
        match world.get::<T>(e) {
            Some(v) => {
                write_words(v, out);
                true
            }
            None => false,
        }
    }
    fn write(&self, world: &mut World, e: Entity, words: &[u32]) {
        if let Some(v) = world.get_mut::<T>(e) {
            read_words(v, words);
        } else if world.is_alive(e) {
            let mut v = T::default();
            read_words(&mut v, words);
            world.insert(e, v);
        }
    }
    fn remove(&self, world: &mut World, e: Entity) {
        world.remove::<T>(e);
    }
    fn track(&self, world: &mut World) -> ComponentId {
        world.track_changes::<T>()
    }
}

/// Component on a client's copy of a remote entity: the received values of
/// an interpolated kind, shown a little in the past.
#[derive(Component)]
pub(crate) struct NetBuffer<T: Interpolate + Send + Sync + 'static> {
    pub(crate) buffer: SnapshotBuffer<T>,
}

/// A Rust component shown between received values on clients.
pub(crate) struct Interpolated<T> {
    config: SnapshotConfig,
    marker: PhantomData<fn() -> T>,
}

impl<T> Interpolated<T> {
    pub(crate) fn new(config: SnapshotConfig) -> Self {
        Self {
            config,
            marker: PhantomData,
        }
    }
}

impl<T: Replicate + Component + Interpolate> KindAccess for Interpolated<T> {
    fn read(&self, world: &World, e: Entity, out: &mut [u32]) -> bool {
        Typed::<T>::new().read(world, e, out)
    }
    fn write(&self, world: &mut World, e: Entity, words: &[u32]) {
        Typed::<T>::new().write(world, e, words);
    }
    fn remove(&self, world: &mut World, e: Entity) {
        world.remove::<T>(e);
        world.remove::<NetBuffer<T>>(e);
    }
    fn track(&self, world: &mut World) -> ComponentId {
        world.track_changes::<T>()
    }
    fn buffer(&self, world: &mut World, e: Entity, time: f64, words: &[u32]) -> bool {
        let mut value = world.get::<T>(e).cloned().unwrap_or_default();
        read_words(&mut value, words);
        if let Some(b) = world.get_mut::<NetBuffer<T>>(e) {
            b.buffer.push(time, value);
        } else if world.is_alive(e) {
            let mut buffer = SnapshotBuffer::with_config(self.config);
            buffer.push(time, value);
            world.insert(e, NetBuffer { buffer });
            if !world.has::<T>(e) {
                world.insert(e, value);
            }
        }
        true
    }
    fn sample(&self, world: &mut World, remote_now: f64) {
        world.query::<(&NetBuffer<T>, &mut T)>().for_each(|(b, value)| {
            if let Some(s) = b.buffer.sample(remote_now) {
                *value = s.value;
            }
        });
    }
    fn unbuffer(&self, world: &mut World, e: Entity) {
        world.remove::<NetBuffer<T>>(e);
    }
}

/// The type of one field of a component registered at run time (from C or
/// C++), for [`Replication::raw_component`](super::Replication::raw_component).
///
/// ```
/// assert_eq!(ponko::net::RawType::F32.size(), 4);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawType {
    /// `float`.
    F32,
    /// `int32_t`.
    I32,
    /// `uint32_t`.
    U32,
    /// `int16_t`.
    I16,
    /// `uint16_t`.
    U16,
    /// `int8_t`.
    I8,
    /// `uint8_t`.
    U8,
    /// `bool` (one byte, 0 or 1).
    Bool,
}

impl RawType {
    /// Its size in bytes.
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::F32 | Self::I32 | Self::U32 => 4,
            Self::I16 | Self::U16 => 2,
            Self::I8 | Self::U8 | Self::Bool => 1,
        }
    }
}

/// One replicated field of a component registered at run time: where it
/// is and how it travels.
///
/// ```
/// use ponko::net::{Quantize, RawField, RawType};
/// let x = RawField::new("x", 0, RawType::F32, Quantize::Range { min: -100.0, max: 100.0, step: 0.01 });
/// assert_eq!(x.offset, 0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct RawField {
    /// Its name (part of the declaration both sides must agree on).
    pub name: String,
    /// Its byte offset in the component.
    pub offset: usize,
    /// Its type.
    pub ty: RawType,
    /// How it is rounded for the wire.
    pub quantize: Quantize,
}

impl RawField {
    /// A field `name` of type `ty` at byte `offset`.
    #[must_use]
    pub fn new(name: &str, offset: usize, ty: RawType, quantize: Quantize) -> Self {
        Self {
            name: name.to_string(),
            offset,
            ty,
            quantize,
        }
    }
}

/// Visits `fields` of the plain bytes at `base` in `f`, in order.
///
/// # Safety
/// Every field lies inside the bytes `base` points to (`offset + size`),
/// which are readable, and writable too if `f` reads.
pub(crate) unsafe fn visit_raw(fields: &[RawField], base: *mut u8, f: &mut Fields<'_>) {
    for field in fields {
        // SAFETY: the caller guarantees `base` covers every field. Unaligned
        // reads and writes: C structs may pack fields.
        unsafe {
            let p = base.add(field.offset);
            let (name, q) = (field.name.as_str(), field.quantize);
            match field.ty {
                RawType::F32 => {
                    let mut v = p.cast::<f32>().read_unaligned();
                    f.f32(name, &mut v, q);
                    p.cast::<f32>().write_unaligned(v);
                }
                RawType::I32 => {
                    let mut v = p.cast::<i32>().read_unaligned();
                    f.i32(name, &mut v, q);
                    p.cast::<i32>().write_unaligned(v);
                }
                RawType::U32 => {
                    let mut v = p.cast::<u32>().read_unaligned();
                    f.u32(name, &mut v, q);
                    p.cast::<u32>().write_unaligned(v);
                }
                RawType::I16 => {
                    let mut v = p.cast::<i16>().read_unaligned();
                    f.i16(name, &mut v, q);
                    p.cast::<i16>().write_unaligned(v);
                }
                RawType::U16 => {
                    let mut v = p.cast::<u16>().read_unaligned();
                    f.u16(name, &mut v, q);
                    p.cast::<u16>().write_unaligned(v);
                }
                RawType::I8 => {
                    let mut v = p.cast::<i8>().read_unaligned();
                    f.i8(name, &mut v, q);
                    p.cast::<i8>().write_unaligned(v);
                }
                RawType::U8 => {
                    let mut v = p.read();
                    f.u8(name, &mut v, q);
                    p.write(v);
                }
                RawType::Bool => {
                    let mut v = p.read() != 0;
                    f.bool(name, &mut v);
                    p.write(u8::from(v));
                }
            }
        }
    }
}

/// A component registered from C or C++: plain bytes with known fields.
#[derive(Clone)]
pub(crate) struct Raw {
    pub(crate) id: ComponentId,
    pub(crate) size: usize,
    pub(crate) fields: Vec<RawField>,
}

impl Raw {
    /// Visits the fields of the value at `base` (`size` bytes) in `f`.
    ///
    /// # Safety
    /// `base` points to `self.size` readable bytes, writable too if `f` reads.
    pub(crate) unsafe fn visit(&self, base: *mut u8, f: &mut Fields<'_>) {
        // SAFETY: forwarded; registration checked the fields lie inside `size`.
        unsafe { visit_raw(&self.fields, base, f) }
    }

    /// The words of this kind, from a zeroed value.
    pub(crate) fn schema(&self) -> Vec<FieldSpec> {
        let mut bytes = vec![0u8; self.size.max(1)];
        let mut specs = Vec::new();
        // SAFETY: `bytes` is `size` (or 1) zeroed bytes, readable and writable.
        unsafe { self.visit(bytes.as_mut_ptr(), &mut Fields::describe(&mut specs)) };
        specs
    }
}

impl KindAccess for Raw {
    fn read(&self, world: &World, e: Entity, out: &mut [u32]) -> bool {
        let Some(p) = world.raw_ref().get_dynamic(e, self.id) else {
            return false;
        };
        // Copy out first: writing mode only reads, but `visit` takes a
        // mutable pointer and the world gave a shared one. Small values
        // (all of them, in practice) stay on the stack.
        let mut stack = [0u8; 256];
        let mut heap = Vec::new();
        let copy: &mut [u8] = if self.size <= stack.len() {
            &mut stack[..self.size]
        } else {
            heap.resize(self.size, 0);
            &mut heap
        };
        // SAFETY: `p` points to this entity's live value of `size` bytes;
        // nothing changes the world while it is copied.
        unsafe { std::ptr::copy_nonoverlapping(p, copy.as_mut_ptr(), self.size) };
        // SAFETY: `copy` holds `size` bytes.
        unsafe { self.visit(copy.as_mut_ptr(), &mut Fields::writer(out)) };
        true
    }
    fn write(&self, world: &mut World, e: Entity, words: &[u32]) {
        if !world.is_alive(e) {
            return;
        }
        if world.raw_ref().get_dynamic(e, self.id).is_none() {
            let zeros = vec![0u8; self.size.max(1)];
            // SAFETY: a zeroed value of the registered size: registration
            // requires plain bytes without a destructor, for which zero is
            // a value C and C++ can hold.
            unsafe { world.raw().insert_dynamic(e, self.id, zeros.as_ptr()) };
        }
        let Some(p) = world.raw().get_dynamic_mut(e, self.id) else {
            return;
        };
        // SAFETY: `p` points to this entity's live value of `size` bytes,
        // and nothing touches the world while it is written.
        unsafe { self.visit(p, &mut Fields::reader(words)) };
        world.raw().mark_changed_dynamic(e, self.id);
    }
    fn remove(&self, world: &mut World, e: Entity) {
        world.raw().remove_dynamic(e, self.id);
    }
    fn track(&self, world: &mut World) -> ComponentId {
        world.raw().track_changes_dynamic(self.id);
        self.id
    }
}

/// A [`Kind`] for Rust component `T`.
pub(crate) fn typed_kind<T: Replicate + Component>() -> Kind {
    Kind {
        name: crate::reflect::short_name(std::any::type_name::<T>()).to_string(),
        specs: schema_of::<T>(),
        offset: 0,
        type_id: Some(TypeId::of::<T>()),
        access: Box::new(Typed::<T>::new()),
        raw: None,
    }
}

/// A [`Kind`] for Rust component `T`, interpolated on clients.
pub(crate) fn interpolated_kind<T: Replicate + Component + Interpolate>(config: SnapshotConfig) -> Kind {
    Kind {
        access: Box::new(Interpolated::<T>::new(config)),
        ..typed_kind::<T>()
    }
}
