//! Events: messages from one system to others.
//!
//! **Maturity: proven**, from the guide chapter `events.md`. Game: Coin Dash and Ember Hop,
//! packaged and played by the list of checks (recorded on macOS). Tests: `tests/events.rs`.
//! Platforms: Linux, macOS and Windows, headless (the workspace tests in the list of checks).
//! Graphics: not involved.
//!
//! A system sends with `ctx.events.send(Died { .. })` and others read with
//! `ctx.events.read::<Died>()`. Every system has its own read position, so a
//! system that runs every tick (or every frame) sees each event exactly
//! once: events sent earlier in the same tick right away, later ones on its
//! next run. Old events are dropped automatically.
#![deny(missing_docs)]

use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};

use serde::Serialize;
use serde_json::{Value, json};

use crate::reflect::{raw_json, short_name};

/// Messages between systems, by type: [`Ctx::events`](crate::ctx::Ctx::events).
///
/// ```
/// use ponko::prelude::*;
///
/// struct Died(Entity);
/// #[derive(Component)]
/// struct Health(i32);
///
/// fn damage(ctx: &mut Ctx) {
///     ctx.world.query::<(Entity, &Health)>().for_each(|(e, h)| {
///         if h.0 <= 0 {
///             ctx.events.send(Died(e));
///         }
///     });
/// }
///
/// fn funerals(ctx: &mut Ctx) {
///     for died in ctx.events.read::<Died>() {
///         ctx.world.despawn(died.0);
///     }
/// }
///
/// let world = Game::new("events", 320, 240)
///     .sim_system(damage)
///     .sim_system(funerals)
///     .run_headless(2, |ctx| {
///         ctx.world.spawn((Health(0),));
///         ctx.world.spawn((Health(5),));
///     });
/// assert_eq!(world.len(), 1);
/// ```
///
/// When events are visible (systems run one at a time):
///
/// - `send` is immediate: any later `read` sees the event, including one
///   by the sender itself. Events are not deferred like commands, so a
///   reader right after the sender sees its events and its applied
///   commands together.
/// - A reader earlier in the order (an earlier [`Phase`](crate::game::Phase), or
///   registered before the sender in the same phase) sees the event on its
///   next run: the next tick for simulation systems.
/// - Frame systems see the events of every tick in their frame. Events
///   sent by frame systems are stamped with the next tick and reach
///   simulation systems on the next tick that runs.
/// - At the start of each frame (each tick in headless runs), events are
///   dropped once they are at least one whole frame old **and** older than
///   the slowest system interval (`IntoSystem::every`) in ticks. Headless, with every
///   system running every tick, an event sent in tick `t` is kept through
///   tick `t + 1`. While the simulation is paused no ticks pass, so events
///   from frame systems are kept until simulation readers run again.
/// - Code outside systems (`setup`) shares one read position.
#[derive(Default)]
pub struct Events {
    channels: Vec<Box<dyn Channel>>,
    by_key: HashMap<Key, usize>,
    /// The channel used last: most systems send one type in a loop.
    last: Option<(Key, usize)>,
    /// The system running now (0 outside systems); each has its own cursor.
    pub(crate) reader: u32,
    frame: u64,
    tick: u64,
    /// Events live at least this many ticks: the slowest system interval.
    pub(crate) keep_ticks: u32,
    /// Extra ticks events are kept while a rollback session may load a
    /// state from that far back ([`EventsState`]); 0 otherwise.
    pub(crate) hold_ticks: u32,
    /// Types tools can see as JSON.
    reflected: Vec<ReflectedEvent>,
    /// What a tool subscribed to, collected between frames.
    pub(crate) feed: Option<Feed>,
    /// Debug builds: event types the running system used, while its
    /// declared access is checked (see [`SystemAccess`](crate::access::SystemAccess)).
    #[cfg(debug_assertions)]
    pub(crate) used: std::cell::RefCell<Option<crate::access::EventUse>>,
    /// Debug builds: the last check's logs, whose buffers the next check
    /// records into, so a checked system doesn't allocate every run.
    #[cfg(debug_assertions)]
    pub(crate) spare_logs: (ponko_ecs::AccessLog, crate::access::EventUse),
}

/// Reader id of the tools' feed; systems count up from 1.
const FEED_READER: u32 = u32::MAX;

struct ReflectedEvent {
    name: String,
    type_id: TypeId,
    /// Calls back with (tick, JSON) for each event `reader` hasn't seen.
    drain: fn(&mut Events, u32, &mut dyn FnMut(u64, Value)),
    kept: fn(&Events) -> usize,
}

/// Events collected for a tool: the newest `capacity`, oldest first.
pub(crate) struct Feed {
    /// Event type names to collect; empty means every reflected type.
    pub(crate) types: Vec<String>,
    pub(crate) items: VecDeque<Value>,
    pub(crate) capacity: usize,
    /// Events that didn't fit since the last take.
    pub(crate) dropped: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Type(TypeId),
    Raw(u32),
}

impl Events {
    /// Sends an event to every system that reads `T`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// struct Hit(u32);
    /// let world = Game::new("t", 1, 1)
    ///     .sim_system(|ctx: &mut Ctx| ctx.events.send(Hit(7)))
    ///     .sim_system(|ctx: &mut Ctx| {
    ///         let total: u32 = ctx.events.read::<Hit>().iter().map(|h| h.0).sum();
    ///         ctx.world.insert_resource(total);
    ///     })
    ///     .run_headless(2, |_| {});
    /// assert_eq!(*world.resource::<u32>(), 7); // read once, then marked as seen
    /// ```
    pub fn send<T: Send + Sync + 'static>(&mut self, event: T) {
        self.note::<T>(true);
        let (frame, tick) = (self.frame, self.tick);
        let ch = self.typed::<T>();
        ch.log.push(frame, tick);
        ch.items.push(event);
    }

    /// The `T` events this system hasn't seen yet, oldest first. The next
    /// call from the same system returns only newer ones.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// struct Hit(u32);
    /// let world = Game::new("t", 1, 1)
    ///     .sim_system(|ctx: &mut Ctx| {
    ///         if ctx.time.sim_tick() == 0 {
    ///             ctx.events.send(Hit(3));
    ///         }
    ///     })
    ///     .sim_system(|ctx: &mut Ctx| {
    ///         // Each event reaches this system once, however many ticks pass.
    ///         let seen = ctx.events.read::<Hit>().len() as u32;
    ///         let total = ctx.world.get_resource::<u32>().copied().unwrap_or(0);
    ///         ctx.world.insert_resource(total + seen);
    ///     })
    ///     .run_headless(4, |_| {});
    /// assert_eq!(*world.resource::<u32>(), 1);
    /// ```
    pub fn read<T: 'static>(&mut self) -> &[T] {
        self.note::<T>(false);
        let reader = self.reader;
        let ch = self.typed::<T>();
        let range = ch.log.read(reader);
        &ch.items[range]
    }

    /// Every `T` event still kept, read or not, oldest first. Doesn't move
    /// this system's read position.
    #[must_use]
    pub fn peek<T: 'static>(&self) -> &[T] {
        self.note::<T>(false);
        match self.by_key.get(&Key::Type(TypeId::of::<T>())) {
            Some(&i) => {
                let any: &dyn Any = self.channels[i].as_any();
                &any.downcast_ref::<Typed<T>>().expect("channel type").items
            }
            None => &[],
        }
    }

    /// The kept `T` events as one value that may be shared between threads
    /// (a `Vec<T>`), or `None` if none were ever sent. For systems that run
    /// in a parallel batch: the batch only reads, and nothing is sent to
    /// this store until it ends.
    pub(crate) fn shared_items<T: Send + Sync + 'static>(&self) -> Option<&(dyn Any + Send + Sync)> {
        let &i = self.by_key.get(&Key::Type(TypeId::of::<T>()))?;
        let typed = self.channels[i]
            .as_any()
            .downcast_ref::<Typed<T>>()
            .expect("channel type");
        Some(&typed.items)
    }

    /// Indexes of the events of type `id` that `reader` hasn't seen, without
    /// marking them seen; empty if none were ever sent.
    pub(crate) fn unread(&self, id: TypeId, reader: u32) -> std::ops::Range<usize> {
        match self.by_key.get(&Key::Type(id)) {
            Some(&i) => self.channels[i].log().unread(reader),
            None => 0..0,
        }
    }

    /// Marks every event of type `id` kept now as seen by `reader`: what
    /// [`read`](Self::read) does, for a system that read them from
    /// [`shared_items`](Self::shared_items). Nothing to mark if none were
    /// ever sent: a reader without a position starts at the oldest kept event.
    pub(crate) fn mark_read(&mut self, id: TypeId, reader: u32) {
        if let Some(&i) = self.by_key.get(&Key::Type(id)) {
            let _ = self.channels[i].log_mut().read(reader);
        }
    }

    /// Debug builds, while checking a system's declared access: notes that
    /// it sent (or read) `T` events.
    #[inline]
    fn note<T: 'static>(&self, sent: bool) {
        #[cfg(debug_assertions)]
        if let Some(used) = self.used.borrow_mut().as_mut() {
            used.note(TypeId::of::<T>(), std::any::type_name::<T>(), sent);
        }
        #[cfg(not(debug_assertions))]
        let _ = sent;
    }

    /// Like `note`, for events without a Rust type.
    #[inline]
    fn note_raw(&self, id: u32, sent: bool) {
        #[cfg(debug_assertions)]
        if let Some(used) = self.used.borrow_mut().as_mut() {
            used.note_raw(id, sent);
        }
        #[cfg(not(debug_assertions))]
        let _ = (id, sent);
    }

    fn typed<T: 'static>(&mut self) -> &mut Typed<T> {
        let i = self.channel(Key::Type(TypeId::of::<T>()), || {
            Box::new(Typed::<T> {
                log: Log::default(),
                items: Vec::new(),
            })
        });
        self.channels[i]
            .as_any_mut()
            .downcast_mut::<Typed<T>>()
            .expect("channel type")
    }

    #[inline]
    fn channel(&mut self, key: Key, make: impl FnOnce() -> Box<dyn Channel>) -> usize {
        if let Some((k, i)) = self.last
            && k == key
        {
            return i;
        }
        let i = match self.by_key.get(&key) {
            Some(&i) => i,
            None => {
                self.channels.push(make());
                let i = self.channels.len() - 1;
                self.by_key.insert(key, i);
                i
            }
        };
        self.last = Some((key, i));
        i
    }

    /// Sends an event of foreign type `id` (C and C++ use component ids):
    /// `bytes` is one value, at most 16-byte aligned. The first send of an
    /// id fixes its size.
    #[doc(hidden)]
    pub fn send_raw(&mut self, id: u32, bytes: &[u8]) -> Result<(), &'static str> {
        self.note_raw(id, true);
        let (frame, tick) = (self.frame, self.tick);
        let ch = self.raw(id, bytes.len());
        if ch.size != bytes.len() {
            return Err("event size does not match earlier sends of this event type");
        }
        ch.push(bytes);
        ch.log.push(frame, tick);
        Ok(())
    }

    /// Unseen events of foreign type `id`: a pointer to `count` values of
    /// `size` bytes each, 16-byte aligned. Valid until the next send of this
    /// type or the end of the system.
    #[doc(hidden)]
    pub fn read_raw(&mut self, id: u32, size: usize) -> Result<(*const u8, usize), &'static str> {
        self.note_raw(id, false);
        let reader = self.reader;
        let ch = self.raw(id, size);
        if ch.size != size {
            return Err("event size does not match earlier sends of this event type");
        }
        let range = ch.log.read(reader);
        let bytes = ch.bytes();
        let start = range.start * size;
        // `start` is at most the length, so the pointer stays in (or one past) the buffer.
        Ok((bytes[start..].as_ptr(), range.len()))
    }

    fn raw(&mut self, id: u32, size: usize) -> &mut Raw {
        let i = self.channel(Key::Raw(id), || {
            Box::new(Raw {
                log: Log::default(),
                id,
                size,
                blocks: Vec::new(),
            })
        });
        self.channels[i]
            .as_any_mut()
            .downcast_mut::<Raw>()
            .expect("channel type")
    }

    /// Lets tools (the control endpoint, the MCP server) see `T` events as
    /// JSON: list them and subscribe to them. Named by the type's short name.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// #[derive(serde::Serialize)]
    /// struct Died { who: Entity }
    ///
    /// Game::new("events", 320, 240).run_headless(0, |ctx| {
    ///     ctx.events.reflect::<Died>();
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If another reflected event type has the same short name.
    #[track_caller]
    pub fn reflect<T: Serialize + 'static>(&mut self) {
        let type_name = std::any::type_name::<T>();
        let name = short_name(type_name);
        if let Some(r) = self.reflected.iter().find(|r| r.name == name) {
            assert!(
                r.type_id == TypeId::of::<T>(),
                "Events::reflect: two event types would both be named `{name}`"
            );
            return;
        }
        fn drain<T: Serialize + 'static>(ev: &mut Events, reader: u32, f: &mut dyn FnMut(u64, Value)) {
            let ch = ev.typed::<T>();
            let range = ch.log.read(reader);
            for i in range {
                let v = serde_json::to_value(&ch.items[i]).unwrap_or_else(|e| json!({ "error": e.to_string() }));
                f(ch.log.tick_at(i), v);
            }
        }
        fn kept<T: 'static>(ev: &Events) -> usize {
            ev.peek::<T>().len()
        }
        self.reflected.push(ReflectedEvent {
            name,
            type_id: TypeId::of::<T>(),
            drain: drain::<T>,
            kept: kept::<T>,
        });
    }

    /// Reflected event types and how many of each are kept now, including C
    /// events whose type is a component described with `reflect_fields`.
    pub(crate) fn reflected_json(&self, world: &ponko_ecs::World) -> Vec<Value> {
        let mut out: Vec<Value> = (self.reflected.iter())
            .map(|r| json!({ "name": r.name, "kept": (r.kept)(self) }))
            .collect();
        for ch in &self.channels {
            if let Some(raw) = ch.as_any().downcast_ref::<Raw>()
                && let Some(name) = raw_name(world, raw.id, raw)
            {
                out.push(json!({ "name": name, "kept": raw.log.len }));
            }
        }
        out
    }

    /// Engine side: moves new events of subscribed types into the feed.
    pub(crate) fn collect_feed(&mut self, world: &ponko_ecs::World) {
        let Some(mut feed) = self.feed.take() else {
            return;
        };
        let wanted = |feed: &Feed, name: &str| feed.types.is_empty() || feed.types.iter().any(|t| t == name);
        for i in 0..self.reflected.len() {
            if !wanted(&feed, &self.reflected[i].name) {
                continue;
            }
            let (drain, name) = (self.reflected[i].drain, self.reflected[i].name.clone());
            drain(self, FEED_READER, &mut |tick, data| {
                feed.push(json!({ "tick": tick, "type": name, "data": data }));
            });
        }
        // In the order the types were first sent, so the feed is reproducible.
        for i in 0..self.channels.len() {
            let Some(raw) = self.channels[i].as_any_mut().downcast_mut::<Raw>() else {
                continue;
            };
            let (id, size) = (raw.id, raw.size);
            let Some(name) = raw_name(world, id, raw).map(str::to_string) else {
                continue;
            };
            if !wanted(&feed, &name) {
                continue;
            }
            let range = raw.log.read(FEED_READER);
            let Some(component) = world.component_at(id as usize) else {
                continue;
            };
            for e in range {
                let p = raw.bytes()[e * size..].as_ptr();
                // SAFETY: `p` covers one `size`-byte event, and `size` is the
                // component's size (checked by `raw_name`).
                if let Some((_, data)) = unsafe { raw_json(world, component, p) } {
                    feed.push(json!({ "tick": raw.log.tick_at(e), "type": name, "data": data }));
                }
            }
        }
        self.feed = Some(feed);
    }

    /// Starts a feed of new events of `types` (all reflected types if empty).
    pub(crate) fn subscribe(&mut self, world: &ponko_ecs::World, types: Vec<String>, capacity: usize) {
        self.feed = Some(Feed {
            types,
            items: VecDeque::new(),
            capacity,
            dropped: 0,
        });
        // Only events from now on: skip what's already kept.
        self.collect_feed(world);
        if let Some(f) = &mut self.feed {
            f.items.clear();
        }
    }

    /// Engine side: a new frame starts at `frame`, after `tick` ticks. Drops
    /// events that every system has had its chance to read: at least one
    /// whole frame old and older than `keep_ticks` ticks.
    pub(crate) fn begin_frame(&mut self, frame: u64, tick: u64) {
        self.frame = frame;
        self.tick = tick;
        let keep = u64::from(self.keep_ticks.max(1)) + u64::from(self.hold_ticks);
        for ch in &mut self.channels {
            ch.drop_old(frame, tick, keep);
        }
    }

    /// Engine side: the tick counter moved on.
    pub(crate) fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// Rollback: notes what is kept now, to go back to with
    /// [`restore_state`](Self::restore_state). Reuses `into`'s memory.
    pub(crate) fn save_state(&self, into: &mut EventsState) {
        into.channels.resize_with(self.channels.len(), ChannelState::default);
        for (ch, saved) in self.channels.iter().zip(&mut into.channels) {
            let log = ch.log();
            saved.end = log.base + log.len as u64;
            saved.cursors.clone_from(&log.cursors);
        }
    }

    /// Rollback: forgets the events sent since `from` was saved, and moves
    /// the read positions of the readers `rolls_back` picks (simulation
    /// systems) back to where they were. Other readers (frame systems,
    /// tools) keep theirs, so they aren't shown events again at positions
    /// they already read. Events sent before the save are still kept:
    /// [`hold_ticks`](Self::hold_ticks) keeps them long enough.
    pub(crate) fn restore_state(&mut self, from: &EventsState, rolls_back: impl Fn(u32) -> bool) {
        for (i, ch) in self.channels.iter_mut().enumerate() {
            let saved = from.channels.get(i);
            let log = ch.log_mut();
            let end = saved.map_or(0, |s| s.end);
            debug_assert!(
                saved.is_none_or(|s| s.cursors.iter().all(|c| !rolls_back(c.0) || c.1 >= log.base)),
                "rollback: events a simulation system hadn't read were dropped before the load"
            );
            let keep = end.saturating_sub(log.base).min(log.len as u64) as usize;
            // Runs before the save are the same as then (the same history).
            log.runs.retain(|r| r.0 < end);
            log.cursors.retain(|c| !rolls_back(c.0));
            if let Some(s) = saved {
                log.cursors.extend(s.cursors.iter().filter(|c| rolls_back(c.0)));
            }
            ch.truncate(keep);
        }
    }
}

/// What [`Events`] kept at a tick, for rollback: per channel, where its
/// events ended and where each reader was. The events themselves stay in
/// the store; a load forgets the ones sent after the save.
#[derive(Default)]
pub(crate) struct EventsState {
    channels: Vec<ChannelState>,
}

#[derive(Default)]
struct ChannelState {
    /// Sequence number after the last event kept at the save.
    end: u64,
    cursors: Vec<(u32, u64)>,
}

impl EventsState {
    /// Heap bytes held, for the numbers in the guide.
    pub(crate) fn heap_bytes(&self) -> usize {
        self.channels.capacity() * std::mem::size_of::<ChannelState>()
            + self.channels.iter().map(|c| c.cursors.capacity() * 16).sum::<usize>()
    }
}

impl Feed {
    fn push(&mut self, v: Value) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
            self.dropped += 1;
        }
        if self.capacity > 0 {
            self.items.push_back(v);
        }
    }
}

/// The name of a C event type, if its id is a component described with
/// `reflect_fields` and of the events' size.
fn raw_name<'w>(world: &'w ponko_ecs::World, id: u32, raw: &Raw) -> Option<&'w str> {
    let component = world.component_at(id as usize)?;
    if world.component_info(component).layout().size() != raw.size {
        return None;
    }
    crate::reflect::raw_name(world, component)
}

trait Channel: Any {
    fn drop_old(&mut self, frame: u64, tick: u64, keep_ticks: u64);
    /// Keeps the oldest `len` events (rollback).
    fn truncate(&mut self, len: usize);
    fn log(&self) -> &Log;
    fn log_mut(&mut self) -> &mut Log;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// When each kept event was sent, and where each reader is.
#[derive(Default)]
struct Log {
    /// Sequence number of the oldest kept event.
    base: u64,
    /// How many events are kept.
    len: usize,
    /// Runs of kept events sent in the same frame and tick: (first sequence
    /// number, frame, tick), oldest first.
    runs: Vec<(u64, u64, u64)>,
    /// Next sequence number each reader will read.
    cursors: Vec<(u32, u64)>,
}

impl Log {
    #[inline]
    fn push(&mut self, frame: u64, tick: u64) {
        match self.runs.last() {
            Some(&(_, f, t)) if f == frame && t == tick => {}
            _ => self.runs.push((self.base + self.len as u64, frame, tick)),
        }
        self.len += 1;
    }

    /// Indexes of events `reader` hasn't seen.
    fn unread(&self, reader: u32) -> std::ops::Range<usize> {
        let cursor = self.cursors.iter().find(|c| c.0 == reader).map_or(self.base, |c| c.1);
        // A frame reader may be past the end after a rollback forgot events.
        ((cursor.max(self.base) - self.base) as usize).min(self.len)..self.len
    }

    /// Indexes of events `reader` hasn't seen, and marks them seen.
    fn read(&mut self, reader: u32) -> std::ops::Range<usize> {
        let end = self.base + self.len as u64;
        let cursor = match self.cursors.iter_mut().find(|c| c.0 == reader) {
            // Past the end only after a rollback: stay there, so events sent
            // again at positions already read aren't shown again.
            Some(c) => {
                let old = c.1;
                c.1 = old.max(end);
                old
            }
            None => {
                self.cursors.push((reader, end));
                self.base
            }
        };
        let start = cursor.max(self.base) - self.base;
        (start as usize).min(self.len)..self.len
    }

    /// The tick kept event `index` was sent in.
    fn tick_at(&self, index: usize) -> u64 {
        let seq = self.base + index as u64;
        let run = self.runs.partition_point(|r| r.0 <= seq) - 1;
        self.runs[run].2
    }

    /// Forgets the oldest events that expired; returns how many.
    fn drop_expired(&mut self, frame: u64, tick: u64, keep_ticks: u64) -> usize {
        let gone = self
            .runs
            .iter()
            .take_while(|&&(_, f, t)| f + 1 < frame && t + keep_ticks < tick)
            .count();
        if gone == 0 {
            return 0;
        }
        self.runs.drain(..gone);
        let new_base = self.runs.first().map_or(self.base + self.len as u64, |r| r.0);
        let n = (new_base - self.base) as usize;
        self.base = new_base;
        self.len -= n;
        n
    }
}

struct Typed<T> {
    log: Log,
    items: Vec<T>,
}

impl<T: 'static> Channel for Typed<T> {
    fn drop_old(&mut self, frame: u64, tick: u64, keep_ticks: u64) {
        let n = self.log.drop_expired(frame, tick, keep_ticks);
        if n > 0 {
            self.items.drain(..n);
        }
    }
    fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
        self.log.len = len;
    }
    fn log(&self) -> &Log {
        &self.log
    }
    fn log_mut(&mut self) -> &mut Log {
        &mut self.log
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// 16 bytes, 16-byte aligned: storage for foreign events.
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C, align(16))]
struct Block([u8; 16]);

/// Foreign events: `size`-byte values packed back to back.
struct Raw {
    log: Log,
    id: u32,
    size: usize,
    blocks: Vec<Block>,
}

impl Raw {
    fn bytes(&self) -> &[u8] {
        let all: &[u8] = bytemuck::cast_slice(&self.blocks);
        &all[..self.log.len * self.size]
    }

    fn push(&mut self, value: &[u8]) {
        let start = self.log.len * self.size;
        let end = start + value.len();
        self.blocks.resize(end.div_ceil(16), Block([0; 16]));
        let all: &mut [u8] = bytemuck::cast_slice_mut(&mut self.blocks);
        all[start..end].copy_from_slice(value);
    }
}

impl Channel for Raw {
    fn drop_old(&mut self, frame: u64, tick: u64, keep_ticks: u64) {
        let old_len = self.log.len;
        let n = self.log.drop_expired(frame, tick, keep_ticks);
        if n > 0 {
            let all: &mut [u8] = bytemuck::cast_slice_mut(&mut self.blocks);
            all.copy_within(n * self.size..old_len * self.size, 0);
            self.blocks.truncate((self.log.len * self.size).div_ceil(16));
        }
    }
    fn truncate(&mut self, len: usize) {
        self.log.len = len;
        self.blocks.truncate((len * self.size).div_ceil(16));
    }
    fn log(&self) -> &Log {
        &self.log
    }
    fn log_mut(&mut self) -> &mut Log {
        &mut self.log
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The kept events of one type, as a system in a parallel batch sees them:
/// shared, read-only, with the range it hasn't read yet.
pub(crate) struct EventSlot<'a> {
    pub(crate) id: TypeId,
    /// A `Vec<T>` of the kept events.
    pub(crate) items: &'a (dyn Any + Send + Sync),
    pub(crate) unread: std::ops::Range<usize>,
}

/// Events a system sent while it ran in a parallel batch. They join
/// [`Events`] when the batch ends, system after system in registration
/// order, so the store ends up as if the systems had run one at a time.
#[derive(Default)]
pub(crate) struct Outbox {
    /// One per event type ever sent, in the order of the first sends.
    channels: Vec<Box<dyn Out>>,
}

trait Out: Send {
    fn id(&self) -> TypeId;
    fn flush(&mut self, into: &mut Events);
    fn clear(&mut self);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct OutOf<T>(Vec<T>);

impl<T: Send + Sync + 'static> Out for OutOf<T> {
    fn id(&self) -> TypeId {
        TypeId::of::<T>()
    }
    fn flush(&mut self, into: &mut Events) {
        for event in self.0.drain(..) {
            into.send(event);
        }
    }
    fn clear(&mut self) {
        self.0.clear();
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Outbox {
    pub(crate) fn send<T: Send + Sync + 'static>(&mut self, event: T) {
        let id = TypeId::of::<T>();
        let i = match self.channels.iter().position(|c| c.id() == id) {
            Some(i) => i,
            None => {
                self.channels.push(Box::new(OutOf::<T>(Vec::new())));
                self.channels.len() - 1
            }
        };
        let out = self.channels[i].as_any_mut().downcast_mut::<OutOf<T>>();
        out.expect("outbox type").0.push(event);
    }

    /// Drops everything, keeping the buffers.
    pub(crate) fn clear(&mut self) {
        for c in &mut self.channels {
            c.clear();
        }
    }

    /// Sends everything to `into`, keeping the buffers for the next batch.
    pub(crate) fn flush(&mut self, into: &mut Events) {
        for c in &mut self.channels {
            c.flush(into);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_and_mark_read_match_read() {
        let mut ev = Events::default();
        let id = TypeId::of::<u32>();
        assert_eq!(ev.unread(id, 1), 0..0);
        ev.mark_read(id, 1); // nothing sent yet: nothing to mark
        ev.send(1u32);
        ev.send(2u32);
        assert_eq!(ev.unread(id, 1), 0..2);
        assert_eq!(ev.unread(id, 1), 0..2, "looking doesn't mark");
        let shared = ev.shared_items::<u32>().unwrap();
        assert_eq!(shared.downcast_ref::<Vec<u32>>().unwrap(), &[1, 2]);
        ev.mark_read(id, 1);
        assert_eq!(ev.unread(id, 1), 2..2);
        ev.send(3u32);
        assert_eq!(ev.unread(id, 1), 2..3);
        ev.reader = 1;
        assert_eq!(ev.read::<u32>(), [3]);
        // Another reader still has everything.
        assert_eq!(ev.unread(id, 2), 0..3);
        assert!(ev.shared_items::<u64>().is_none());
    }

    #[test]
    fn an_outbox_delivers_in_send_order_and_can_be_reused() {
        let mut ev = Events::default();
        let mut out = Outbox::default();
        ev.send(0u32);
        out.send(1u32);
        out.send("a");
        out.send(2u32);
        assert_eq!(ev.peek::<u32>(), [0], "nothing arrives before the flush");
        out.flush(&mut ev);
        assert_eq!(ev.peek::<u32>(), [0, 1, 2]);
        assert_eq!(ev.peek::<&str>(), ["a"]);
        out.flush(&mut ev);
        out.send(3u32);
        out.flush(&mut ev);
        assert_eq!(ev.peek::<u32>(), [0, 1, 2, 3]);
    }

    #[test]
    fn each_reader_sees_each_event_once() {
        let mut ev = Events::default();
        ev.send(1u32);
        ev.reader = 1;
        assert_eq!(ev.read::<u32>(), &[1]);
        assert!(ev.read::<u32>().is_empty());
        ev.send(2u32);
        ev.reader = 2;
        assert_eq!(ev.read::<u32>(), &[1, 2]);
        ev.reader = 1;
        assert_eq!(ev.read::<u32>(), &[2]);
        assert_eq!(ev.peek::<u32>(), &[1, 2]);
    }

    #[test]
    fn events_last_one_frame_and_keep_ticks() {
        let mut ev = Events {
            keep_ticks: 2,
            ..Default::default()
        };
        ev.begin_frame(1, 0);
        ev.send("a");
        ev.begin_frame(2, 1);
        ev.begin_frame(3, 2);
        assert_eq!(ev.peek::<&str>(), &["a"], "kept for keep_ticks ticks");
        ev.begin_frame(4, 3);
        assert!(ev.peek::<&str>().is_empty());
        // A paused game (no ticks) keeps them for frame systems.
        ev.send("b");
        ev.begin_frame(5, 3);
        ev.begin_frame(6, 3);
        assert_eq!(ev.peek::<&str>(), &["b"]);
    }

    #[test]
    fn a_rollback_load_forgets_later_events_and_moves_only_sim_readers_back() {
        const SIM: u32 = 1;
        const FRAME: u32 = 2;
        let mut ev = Events::default();
        let read = |ev: &mut Events, reader| {
            ev.reader = reader;
            let seen = ev.read::<u32>().to_vec();
            ev.reader = 0;
            seen
        };
        ev.begin_frame(1, 0);
        ev.send(1u32);
        assert_eq!(read(&mut ev, FRAME), [1]);
        let mut saved = EventsState::default();
        ev.save_state(&mut saved);
        // After the save: one more event, and raw events of another type.
        ev.send(2u32);
        ev.send_raw(7, &[9; 4]).unwrap();
        assert_eq!(read(&mut ev, SIM), [1, 2]);
        assert_eq!(read(&mut ev, FRAME), [2]);
        ev.restore_state(&saved, |r| r == SIM);
        assert_eq!(ev.peek::<u32>(), &[1], "sent after the save: forgotten");
        assert_eq!(ev.read_raw(7, 4).unwrap().1, 0, "raw events too");
        // The tick runs again and sends something else at the same place.
        ev.send(3u32);
        assert_eq!(read(&mut ev, SIM), [1, 3], "the sim reader reads as at the save");
        assert!(
            read(&mut ev, FRAME).is_empty(),
            "the frame reader read that place already"
        );
        ev.send(4u32);
        assert_eq!(read(&mut ev, FRAME), [4]);
        assert!(EventsState::default().heap_bytes() == 0 && saved.heap_bytes() > 0);
    }

    #[test]
    fn hold_ticks_keep_events_longer() {
        let mut ev = Events {
            keep_ticks: 1,
            hold_ticks: 3,
            ..Default::default()
        };
        ev.begin_frame(1, 0);
        ev.send(1u8);
        ev.begin_frame(4, 4);
        assert_eq!(ev.peek::<u8>(), &[1]);
        ev.begin_frame(5, 5);
        assert!(ev.peek::<u8>().is_empty());
    }

    #[test]
    fn a_reader_that_missed_dropped_events_gets_what_is_left() {
        let mut ev = Events {
            reader: 1,
            ..Default::default()
        };
        assert!(ev.read::<u8>().is_empty());
        ev.begin_frame(1, 0);
        ev.send(1u8);
        ev.begin_frame(5, 5);
        ev.send(2u8);
        assert_eq!(ev.read::<u8>(), &[2]);
    }

    #[test]
    fn raw_events_are_packed_aligned_and_dropped() {
        let mut ev = Events::default();
        for i in 0..5u8 {
            ev.send_raw(9, &[i, i, i]).unwrap();
        }
        assert!(ev.send_raw(9, &[0; 4]).is_err());
        assert!(ev.read_raw(9, 4).is_err());
        ev.reader = 3;
        let (ptr, n) = ev.read_raw(9, 3).unwrap();
        assert_eq!(n, 5);
        assert_eq!(ptr as usize % 16, 0);
        assert_eq!(ev.raw(9, 3).bytes()[12..15], [4, 4, 4]);
        ev.begin_frame(1, 0);
        ev.send_raw(9, &[7, 7, 7]).unwrap();
        ev.begin_frame(2, 3);
        assert_eq!(ev.raw(9, 3).bytes(), &[7, 7, 7]);
        let (_, n) = ev.read_raw(9, 3).unwrap();
        assert_eq!(n, 1);
        let (_, n) = ev.read_raw(9, 3).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn feeds_collect_reflected_rust_and_c_events() {
        use crate::reflect::{Field, FieldType, WorldReflect};
        use ponko_ecs::World;
        #[derive(serde::Serialize)]
        struct Ping(u32);
        struct Hidden;

        let mut world = World::new();
        // A C struct { float damage; uint32_t target; }.
        // SAFETY: plain bytes, no destructor; any field values are valid.
        let hit = unsafe { world.raw().register_component("Hit", 8, 4, None) };
        unsafe {
            world.reflect_fields(
                hit,
                &[
                    Field::new("damage", FieldType::F32, 0),
                    Field::new("target", FieldType::U32, 4),
                ],
            );
        }
        let mut ev = Events::default();
        ev.reflect::<Ping>();
        ev.send(Ping(1)); // before subscribing: not in the feed
        ev.subscribe(&world, Vec::new(), 10);
        ev.send(Ping(2));
        ev.send(Hidden);
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&2.5f32.to_ne_bytes());
        bytes[4..].copy_from_slice(&7u32.to_ne_bytes());
        ev.send_raw(hit.index() as u32, &bytes).unwrap();
        ev.collect_feed(&world);
        let feed = ev.feed.as_ref().unwrap();
        let items: Vec<&Value> = feed.items.iter().collect();
        assert_eq!(
            items,
            [
                &json!({ "tick": 0, "type": "Ping", "data": 2 }),
                &json!({ "tick": 0, "type": "Hit", "data": { "damage": 2.5, "target": 7 } }),
            ]
        );
        let names: Vec<Value> = ev.reflected_json(&world).iter().map(|v| v["name"].clone()).collect();
        assert_eq!(names, [json!("Ping"), json!("Hit")]);

        // Only the asked-for types.
        ev.subscribe(&world, vec!["Hit".into()], 10);
        ev.send(Ping(3));
        ev.send_raw(hit.index() as u32, &bytes).unwrap();
        ev.collect_feed(&world);
        assert_eq!(ev.feed.as_ref().unwrap().items.len(), 1);
    }
}
